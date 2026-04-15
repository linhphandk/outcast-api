use crate::schema::sessions;
use async_trait::async_trait;
use chrono::NaiveDateTime;
use deadpool_diesel::InteractError;
use deadpool_diesel::postgres::Pool;
use diesel::prelude::*;
#[cfg(test)]
use mockall::{automock, predicate::*};
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

#[derive(Debug, PartialEq, Clone, Queryable)]
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

#[derive(AsChangeset)]
#[diesel(table_name = sessions)]
pub struct UpdateSession {
    pub revoked_at: Option<NaiveDateTime>,
    pub updated_at: NaiveDateTime,
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
pub trait SessionRepositoryTrait: Send + Sync {
    async fn create(
        &self,
        user_id: Uuid,
        refresh_token: &str,
        user_agent: Option<&str>,
        ip_address: Option<&str>,
        expires_at: NaiveDateTime,
    ) -> Result<Session, SessionRepositoryError>;

    async fn find_by_refresh_token(
        &self,
        token: &str,
    ) -> Result<Option<Session>, SessionRepositoryError>;

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Session>, SessionRepositoryError>;

    async fn find_all_by_user_id(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<Session>, SessionRepositoryError>;

    async fn revoke(&self, id: Uuid) -> Result<Session, SessionRepositoryError>;

    async fn delete(&self, id: Uuid) -> Result<(), SessionRepositoryError>;

    async fn delete_all_by_user_id(&self, user_id: Uuid) -> Result<(), SessionRepositoryError>;
}

impl SessionRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SessionRepositoryTrait for SessionRepository {
    #[instrument(skip(self, refresh_token, user_agent, ip_address), fields(user_id = %user_id))]
    async fn create(
        &self,
        user_id: Uuid,
        refresh_token: &str,
        user_agent: Option<&str>,
        ip_address: Option<&str>,
        expires_at: NaiveDateTime,
    ) -> Result<Session, SessionRepositoryError> {
        info!("Creating new session");

        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection from pool");
            SessionRepositoryError::PoolError(e)
        })?;

        let id = Uuid::new_v4();
        let refresh_token = refresh_token.to_string();
        let user_agent = user_agent.map(str::to_string);
        let ip_address = ip_address.map(str::to_string);

        let session = conn
            .interact(move |conn| {
                let new_session = NewSession {
                    id,
                    user_id,
                    refresh_token,
                    user_agent,
                    ip_address,
                    expires_at,
                };

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
                match &e {
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::UniqueViolation,
                        info,
                    ) => {
                        warn!(
                            constraint = ?info.constraint_name(),
                            "Unique constraint violation while creating session"
                        );
                    }
                    _ => {
                        error!(error = %e, "Diesel error during session creation");
                    }
                }
                SessionRepositoryError::DieselError(e)
            })?;

        info!(session_id = %session.id, "Session created successfully");
        Ok(session)
    }

    #[instrument(skip(self, token))]
    async fn find_by_refresh_token(
        &self,
        token: &str,
    ) -> Result<Option<Session>, SessionRepositoryError> {
        debug!("Looking up session by refresh token");

        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let token = token.to_string();
        let session = conn
            .interact(move |conn| {
                sessions::table
                    .filter(sessions::refresh_token.eq(token))
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

    #[instrument(skip(self), fields(session_id = %id))]
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Session>, SessionRepositoryError> {
        debug!("Looking up session by ID");

        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let session = conn
            .interact(move |conn| {
                sessions::table
                    .filter(sessions::id.eq(id))
                    .first::<Session>(conn)
                    .optional()
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during find_by_id");
                SessionRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error during find_by_id");
                SessionRepositoryError::DieselError(e)
            })?;

        debug!(found = session.is_some(), "find_by_id result");
        Ok(session)
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    async fn find_all_by_user_id(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<Session>, SessionRepositoryError> {
        debug!("Looking up sessions by user ID");

        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let sessions_list = conn
            .interact(move |conn| {
                sessions::table
                    .filter(sessions::user_id.eq(user_id))
                    .load::<Session>(conn)
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during find_all_by_user_id");
                SessionRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error during find_all_by_user_id");
                SessionRepositoryError::DieselError(e)
            })?;

        debug!(count = sessions_list.len(), "find_all_by_user_id result");
        Ok(sessions_list)
    }

    #[instrument(skip(self), fields(session_id = %id))]
    async fn revoke(&self, id: Uuid) -> Result<Session, SessionRepositoryError> {
        info!("Revoking session");

        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let now = chrono::Utc::now().naive_utc();
        let session = conn
            .interact(move |conn| {
                diesel::update(sessions::table.filter(sessions::id.eq(id)))
                    .set(&UpdateSession {
                        revoked_at: Some(now),
                        updated_at: now,
                    })
                    .get_result::<Session>(conn)
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

        info!(session_id = %session.id, "Session revoked successfully");
        Ok(session)
    }

    #[instrument(skip(self), fields(session_id = %id))]
    async fn delete(&self, id: Uuid) -> Result<(), SessionRepositoryError> {
        info!("Deleting session");

        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        conn.interact(move |conn| {
            diesel::delete(sessions::table.filter(sessions::id.eq(id))).execute(conn)
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during delete");
            SessionRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during delete");
            SessionRepositoryError::DieselError(e)
        })?;

        info!("Session deleted successfully");
        Ok(())
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    async fn delete_all_by_user_id(&self, user_id: Uuid) -> Result<(), SessionRepositoryError> {
        info!("Deleting all sessions for user");

        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            SessionRepositoryError::PoolError(e)
        })?;

        let count = conn
            .interact(move |conn| {
                diesel::delete(sessions::table.filter(sessions::user_id.eq(user_id)))
                    .execute(conn)
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during delete_all_by_user_id");
                SessionRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error during delete_all_by_user_id");
                SessionRepositoryError::DieselError(e)
            })?;

        info!(deleted = count, "All sessions deleted for user");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn make_expires_at() -> NaiveDateTime {
        chrono::Utc::now().naive_utc() + chrono::Duration::days(7)
    }

    async fn create_test_user(pool: &Pool) -> Uuid {
        use crate::schema::users;

        let user_id = Uuid::new_v4();
        let email = format!("test{}@example.com", user_id);
        let conn = pool.get().await.unwrap();
        conn.interact(move |conn| {
            diesel::insert_into(users::table)
                .values((
                    users::id.eq(user_id),
                    users::email.eq(email),
                    users::password.eq("hash"),
                ))
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

        let session = repo
            .create(
                user_id,
                "token_abc",
                Some("Mozilla/5.0"),
                Some("127.0.0.1"),
                make_expires_at(),
            )
            .await
            .unwrap();

        assert_eq!(session.user_id, user_id);
        assert_eq!(session.refresh_token, "token_abc");
        assert_eq!(session.user_agent.as_deref(), Some("Mozilla/5.0"));
        assert_eq!(session.ip_address.as_deref(), Some("127.0.0.1"));
        assert!(session.revoked_at.is_none());
    }

    #[tokio::test]
    async fn test_create_session_minimal() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let session = repo
            .create(user_id, "token_min", None, None, make_expires_at())
            .await
            .unwrap();

        assert_eq!(session.user_id, user_id);
        assert!(session.user_agent.is_none());
        assert!(session.ip_address.is_none());
    }

    #[tokio::test]
    async fn test_create_duplicate_refresh_token() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        repo.create(user_id, "dup_token", None, None, make_expires_at())
            .await
            .unwrap();

        let result = repo
            .create(user_id, "dup_token", None, None, make_expires_at())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_find_by_refresh_token() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let created = repo
            .create(user_id, "find_token", None, None, make_expires_at())
            .await
            .unwrap();

        let found = repo.find_by_refresh_token("find_token").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, created.id);
    }

    #[tokio::test]
    async fn test_find_by_refresh_token_not_found() {
        let (_container, pool) = setup_test_db().await;
        let repo = SessionRepository::new(pool);

        let found = repo.find_by_refresh_token("nonexistent").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_by_id() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let created = repo
            .create(user_id, "id_token", None, None, make_expires_at())
            .await
            .unwrap();

        let found = repo.find_by_id(created.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, created.id);
    }

    #[tokio::test]
    async fn test_find_by_id_not_found() {
        let (_container, pool) = setup_test_db().await;
        let repo = SessionRepository::new(pool);

        let found = repo.find_by_id(Uuid::new_v4()).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_all_by_user_id() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        repo.create(user_id, "t1", None, None, make_expires_at())
            .await
            .unwrap();
        repo.create(user_id, "t2", None, None, make_expires_at())
            .await
            .unwrap();

        let sessions = repo.find_all_by_user_id(user_id).await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_find_all_by_user_id_empty() {
        let (_container, pool) = setup_test_db().await;
        let repo = SessionRepository::new(pool);

        let sessions = repo.find_all_by_user_id(Uuid::new_v4()).await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_revoke_session() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let created = repo
            .create(user_id, "revoke_token", None, None, make_expires_at())
            .await
            .unwrap();

        assert!(created.revoked_at.is_none());

        let revoked = repo.revoke(created.id).await.unwrap();
        assert!(revoked.revoked_at.is_some());
    }

    #[tokio::test]
    async fn test_delete_session() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        let created = repo
            .create(user_id, "delete_token", None, None, make_expires_at())
            .await
            .unwrap();

        repo.delete(created.id).await.unwrap();

        let found = repo.find_by_id(created.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_delete_all_by_user_id() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = SessionRepository::new(pool);

        repo.create(user_id, "da1", None, None, make_expires_at())
            .await
            .unwrap();
        repo.create(user_id, "da2", None, None, make_expires_at())
            .await
            .unwrap();

        repo.delete_all_by_user_id(user_id).await.unwrap();

        let sessions = repo.find_all_by_user_id(user_id).await.unwrap();
        assert!(sessions.is_empty());
    }
}
