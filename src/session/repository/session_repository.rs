use crate::schema::sessions;
use async_trait::async_trait;
use chrono::NaiveDateTime;
use deadpool_diesel::InteractError;
use deadpool_diesel::postgres::Pool;
use diesel::prelude::*;
#[cfg(test)]
use mockall::{automock, predicate::*};
use tracing::{debug, error, info, instrument};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Queryable, Selectable)]
#[diesel(table_name = sessions)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub refresh_token: String,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub expires_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = sessions)]
pub struct NewSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub refresh_token: String,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub expires_at: NaiveDateTime,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionRepositoryError {
    #[error("Database pool error: {0}")]
    PoolError(#[from] deadpool_diesel::PoolError),
    #[error("Diesel interaction error: {0}")]
    InteractError(#[from] InteractError),
    #[error("Diesel error: {0}")]
    DieselError(#[from] diesel::result::Error),
}

#[derive(Clone)]
pub struct SessionRepository {
    pool: Pool,
}

#[cfg_attr(test, automock)]
#[async_trait]
pub trait SessionRepositoryTrait {
    async fn create(
        &self,
        user_id: Uuid,
        refresh_token: String,
        user_agent: Option<String>,
        ip_address: Option<String>,
        expires_at: NaiveDateTime,
    ) -> Result<Session, SessionRepositoryError>;

    async fn find_by_refresh_token(
        &self,
        token: String,
    ) -> Result<Option<Session>, SessionRepositoryError>;

    async fn find_active_by_user_id(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<Session>, SessionRepositoryError>;

    async fn revoke(&self, session_id: Uuid) -> Result<(), SessionRepositoryError>;

    async fn revoke_all_for_user(&self, user_id: Uuid) -> Result<(), SessionRepositoryError>;

    async fn update_refresh_token(
        &self,
        session_id: Uuid,
        new_refresh_token: String,
    ) -> Result<(), SessionRepositoryError>;

    async fn delete_expired(&self) -> Result<usize, SessionRepositoryError>;
}

impl SessionRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SessionRepositoryTrait for SessionRepository {
    #[instrument(skip(self, refresh_token), fields(user_id = %user_id))]
    async fn create(
        &self,
        user_id: Uuid,
        refresh_token: String,
        user_agent: Option<String>,
        ip_address: Option<String>,
        expires_at: NaiveDateTime,
    ) -> Result<Session, SessionRepositoryError> {
        info!("Creating new session");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let id = Uuid::new_v4();
        let new_session = NewSession {
            id,
            user_id,
            refresh_token,
            user_agent,
            ip_address,
            expires_at,
        };

        let session = conn
            .interact(move |conn| {
                diesel::insert_into(sessions::table)
                    .values(&new_session)
                    .get_result::<Session>(conn)
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during session creation");
                SessionRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error during session creation");
                SessionRepositoryError::DieselError(e)
            })?;

        info!(session_id = %session.id, "Session created successfully");
        Ok(session)
    }

    #[instrument(skip(self, token))]
    async fn find_by_refresh_token(
        &self,
        token: String,
    ) -> Result<Option<Session>, SessionRepositoryError> {
        debug!("Looking up session by refresh token");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let session = conn
            .interact(move |conn| {
                sessions::table
                    .filter(sessions::refresh_token.eq(&token))
                    .first::<Session>(conn)
                    .optional()
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during find_by_refresh_token");
                SessionRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error during find_by_refresh_token");
                SessionRepositoryError::DieselError(e)
            })?;

        debug!(found = session.is_some(), "find_by_refresh_token result");
        Ok(session)
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    async fn find_active_by_user_id(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<Session>, SessionRepositoryError> {
        debug!("Looking up active sessions for user");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let now = chrono::Utc::now().naive_utc();
        let result = conn
            .interact(move |conn| {
                sessions::table
                    .filter(sessions::user_id.eq(user_id))
                    .filter(sessions::revoked_at.is_null())
                    .filter(sessions::expires_at.gt(now))
                    .load::<Session>(conn)
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during find_active_by_user_id");
                SessionRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error during find_active_by_user_id");
                SessionRepositoryError::DieselError(e)
            })?;

        debug!(count = result.len(), "find_active_by_user_id result");
        Ok(result)
    }

    #[instrument(skip(self), fields(session_id = %session_id))]
    async fn revoke(&self, session_id: Uuid) -> Result<(), SessionRepositoryError> {
        info!("Revoking session");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let now = chrono::Utc::now().naive_utc();
        conn.interact(move |conn| {
            diesel::update(sessions::table.filter(sessions::id.eq(session_id)))
                .set((
                    sessions::revoked_at.eq(Some(now)),
                    sessions::updated_at.eq(now),
                ))
                .execute(conn)
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during revoke");
            SessionRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during revoke");
            SessionRepositoryError::DieselError(e)
        })?;

        info!(session_id = %session_id, "Session revoked");
        Ok(())
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    async fn revoke_all_for_user(&self, user_id: Uuid) -> Result<(), SessionRepositoryError> {
        info!("Revoking all sessions for user");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let now = chrono::Utc::now().naive_utc();
        conn.interact(move |conn| {
            diesel::update(
                sessions::table
                    .filter(sessions::user_id.eq(user_id))
                    .filter(sessions::revoked_at.is_null()),
            )
            .set((
                sessions::revoked_at.eq(Some(now)),
                sessions::updated_at.eq(now),
            ))
            .execute(conn)
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during revoke_all_for_user");
            SessionRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during revoke_all_for_user");
            SessionRepositoryError::DieselError(e)
        })?;

        info!(user_id = %user_id, "All sessions revoked for user");
        Ok(())
    }

    #[instrument(skip(self, new_refresh_token), fields(session_id = %session_id))]
    async fn update_refresh_token(
        &self,
        session_id: Uuid,
        new_refresh_token: String,
    ) -> Result<(), SessionRepositoryError> {
        debug!("Rotating refresh token");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let now = chrono::Utc::now().naive_utc();
        conn.interact(move |conn| {
            diesel::update(sessions::table.filter(sessions::id.eq(session_id)))
                .set((
                    sessions::refresh_token.eq(new_refresh_token),
                    sessions::updated_at.eq(now),
                ))
                .execute(conn)
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during update_refresh_token");
            SessionRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during update_refresh_token");
            SessionRepositoryError::DieselError(e)
        })?;

        debug!(session_id = %session_id, "Refresh token rotated");
        Ok(())
    }

    #[instrument(skip(self))]
    async fn delete_expired(&self) -> Result<usize, SessionRepositoryError> {
        debug!("Deleting expired sessions");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let now = chrono::Utc::now().naive_utc();
        let count = conn
            .interact(move |conn| {
                diesel::delete(sessions::table.filter(sessions::expires_at.lt(now))).execute(conn)
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during delete_expired");
                SessionRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error during delete_expired");
                SessionRepositoryError::DieselError(e)
            })?;

        info!(count = count, "Expired sessions deleted");
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

    async fn setup_test_db() -> (
        testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
        Pool,
    ) {
        use testcontainers::runners::AsyncRunner;
        use testcontainers_modules::postgres::Postgres;

        let container = Postgres::default().start().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let host = container.get_host().await.unwrap();
        let conn_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");

        let manager =
            deadpool_diesel::postgres::Manager::new(conn_string, deadpool_diesel::Runtime::Tokio1);
        let pool = deadpool_diesel::postgres::Pool::builder(manager)
            .build()
            .unwrap();

        let conn = pool.get().await.unwrap();
        conn.interact(|conn| conn.run_pending_migrations(MIGRATIONS).map(|_| ()))
            .await
            .unwrap()
            .unwrap();

        (container, pool)
    }

    async fn create_test_user(pool: &Pool) -> Uuid {
        use crate::schema::users;
        use crate::user::repository::user_repository::NewUser;

        let conn = pool.get().await.unwrap();
        let user_id = Uuid::new_v4();
        conn.interact(move |conn| {
            diesel::insert_into(users::table)
                .values(&NewUser {
                    id: user_id,
                    email: format!("test-{}@example.com", user_id),
                    password: "hashed".to_string(),
                })
                .execute(conn)
        })
        .await
        .unwrap()
        .unwrap();
        user_id
    }

    #[tokio::test]
    async fn test_create_session() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let expires_at = (chrono::Utc::now() + Duration::days(30)).naive_utc();
        let session = repo
            .create(
                user_id,
                "test_refresh_token".to_string(),
                Some("Mozilla/5.0".to_string()),
                Some("127.0.0.1".to_string()),
                expires_at,
            )
            .await
            .unwrap();

        assert_eq!(session.user_id, user_id);
        assert_eq!(session.refresh_token, "test_refresh_token");
        assert!(session.revoked_at.is_none());
    }

    #[tokio::test]
    async fn test_find_by_refresh_token() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let expires_at = (chrono::Utc::now() + Duration::days(30)).naive_utc();
        let created = repo
            .create(
                user_id,
                "find_by_token_test".to_string(),
                None,
                None,
                expires_at,
            )
            .await
            .unwrap();

        let found = repo
            .find_by_refresh_token("find_by_token_test".to_string())
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, created.id);

        let not_found = repo
            .find_by_refresh_token("nonexistent_token".to_string())
            .await
            .unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_find_active_by_user_id() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let expires_at = (chrono::Utc::now() + Duration::days(30)).naive_utc();
        let session1 = repo
            .create(user_id, "token1".to_string(), None, None, expires_at)
            .await
            .unwrap();
        let _session2 = repo
            .create(user_id, "token2".to_string(), None, None, expires_at)
            .await
            .unwrap();

        // Revoke session1
        repo.revoke(session1.id).await.unwrap();

        let active = repo.find_active_by_user_id(user_id).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].refresh_token, "token2");
    }

    #[tokio::test]
    async fn test_revoke_all_for_user() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let expires_at = (chrono::Utc::now() + Duration::days(30)).naive_utc();
        repo.create(user_id, "token_a".to_string(), None, None, expires_at)
            .await
            .unwrap();
        repo.create(user_id, "token_b".to_string(), None, None, expires_at)
            .await
            .unwrap();

        repo.revoke_all_for_user(user_id).await.unwrap();

        let active = repo.find_active_by_user_id(user_id).await.unwrap();
        assert_eq!(active.len(), 0);
    }

    #[tokio::test]
    async fn test_update_refresh_token() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let expires_at = (chrono::Utc::now() + Duration::days(30)).naive_utc();
        let session = repo
            .create(
                user_id,
                "old_refresh_token".to_string(),
                None,
                None,
                expires_at,
            )
            .await
            .unwrap();

        repo.update_refresh_token(session.id, "new_refresh_token".to_string())
            .await
            .unwrap();

        let found = repo
            .find_by_refresh_token("new_refresh_token".to_string())
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, session.id);
    }

    #[tokio::test]
    async fn test_delete_expired() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let past = (chrono::Utc::now() - Duration::hours(1)).naive_utc();
        let future = (chrono::Utc::now() + Duration::days(30)).naive_utc();

        repo.create(user_id, "expired_token".to_string(), None, None, past)
            .await
            .unwrap();
        repo.create(user_id, "valid_token".to_string(), None, None, future)
            .await
            .unwrap();

        let deleted = repo.delete_expired().await.unwrap();
        assert_eq!(deleted, 1);

        let active = repo.find_active_by_user_id(user_id).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].refresh_token, "valid_token");
    }
}
