use crate::schema::users;
use async_trait::async_trait;
use deadpool_diesel::InteractError;
use deadpool_diesel::postgres::Pool;
use diesel::prelude::*;
#[cfg(test)]
use mockall::{automock, predicate::*};
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

#[derive(Debug, PartialEq, Queryable)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password: String,
}

#[derive(Insertable)]
#[diesel(table_name = users)]
pub struct NewUser {
    pub id: Uuid,
    pub email: String,
    pub password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Database pool error: {0}")]
    PoolError(#[from] deadpool_diesel::PoolError),
    #[error("Diesel interaction error: {0}")]
    InteractError(#[from] InteractError),
    #[error("Diesel error: {0}")]
    DieselError(#[from] diesel::result::Error),
}

#[derive(Clone)]
pub struct UserRepository {
    pool: Pool,
}

#[cfg_attr(test, automock)]
#[async_trait]
pub trait UserRepositoryTrait {
    async fn create(&self, email: String, password: String) -> Result<User, RepositoryError>;
    async fn find_by_email(&self, email: String) -> Result<Option<User>, RepositoryError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepositoryError>;
}

impl UserRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepositoryTrait for UserRepository {
    #[instrument(skip(self, password), fields(email = %email))]
    async fn create(&self, email: String, password: String) -> Result<User, RepositoryError> {
        info!("Creating new user");

        debug!("Acquiring database connection from pool");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection from pool");
            RepositoryError::PoolError(e)
        })?;
        debug!("Database connection acquired");

        let id = Uuid::new_v4();
        info!(user_id = %id, "Generated new user ID");

        debug!(user_id = %id, "Inserting user into database");
        let inserted_user = conn
            .interact(move |conn| {
                let new_user = NewUser {
                    id,
                    email: email.clone(),
                    password: password.clone(),
                };

                diesel::insert_into(users::table)
                    .values(&new_user)
                    .execute(conn)?;

                Ok::<_, diesel::result::Error>(User {
                    id,
                    email,
                    password,
                })
            })
            .await
            .map_err(|e| {
                error!(user_id = %id, error = %e, "Interact error during user creation");
                RepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                match &e {
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::UniqueViolation,
                        info,
                    ) => {
                        warn!(
                            user_id = %id,
                            constraint = ?info.constraint_name(),
                            "Unique constraint violation while creating user"
                        );
                    }
                    _ => {
                        error!(user_id = %id, error = %e, "Diesel error during user creation");
                    }
                }
                RepositoryError::DieselError(e)
            })?;

        info!(user_id = %inserted_user.id, "User created successfully");
        debug!(user_id = %inserted_user.id, email = %inserted_user.email, "User creation details");
        Ok(inserted_user)
    }

    async fn find_by_email(&self, email: String) -> Result<Option<User>, RepositoryError> {
        let conn = self.pool.get().await?;

        let user = conn
            .interact(move |conn| {
                users::table
                    .filter(users::email.eq(&email))
                    .first::<User>(conn)
                    .optional()
            })
            .await??;

        Ok(user)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepositoryError> {
        let conn = self.pool.get().await?;

        let user = conn
            .interact(move |conn| {
                users::table
                    .filter(users::id.eq(&id))
                    .first::<User>(conn)
                    .optional()
            })
            .await??;

        Ok(user)
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

    #[tokio::test]
    async fn test_containers_create_user() {
        let (_container, pool) = setup_test_db().await;
        let repo = UserRepository::new(pool);

        let created_user = repo
            .create("test@example.com".to_string(), "supersecret".to_string())
            .await
            .unwrap();

        assert_eq!(created_user.email, "test@example.com");
        assert_eq!(created_user.password, "supersecret");
    }

    #[tokio::test]
    async fn test_containers_create_duplicate_user() {
        let (_container, pool) = setup_test_db().await;
        let repo = UserRepository::new(pool);

        let _ = repo
            .create(
                "duplicate@example.com".to_string(),
                "supersecret".to_string(),
            )
            .await
            .unwrap();

        let duplicate_result = repo
            .create(
                "duplicate@example.com".to_string(),
                "differentpassword".to_string(),
            )
            .await;

        assert!(
            duplicate_result.is_err(),
            "Expected duplicate email creation to fail!"
        );
    }

    #[tokio::test]
    async fn test_find_by_email_returns_user() {
        let (_container, pool) = setup_test_db().await;
        let repo = UserRepository::new(pool);

        let created_user = repo
            .create("findme@example.com".to_string(), "password123".to_string())
            .await
            .unwrap();

        let found_user = repo
            .find_by_email("findme@example.com".to_string())
            .await
            .unwrap();

        assert!(found_user.is_some());
        let found_user = found_user.unwrap();
        assert_eq!(found_user.id, created_user.id);
        assert_eq!(found_user.email, "findme@example.com");
        assert_eq!(found_user.password, "password123");
    }

    #[tokio::test]
    async fn test_find_by_email_returns_none_when_not_found() {
        let (_container, pool) = setup_test_db().await;
        let repo = UserRepository::new(pool);

        let found_user = repo
            .find_by_email("nonexistent@example.com".to_string())
            .await
            .unwrap();

        assert!(found_user.is_none());
    }

    #[tokio::test]
    async fn test_find_by_id_returns_user() {
        let (_container, pool) = setup_test_db().await;
        let repo = UserRepository::new(pool);

        let created_user = repo
            .create("byid@example.com".to_string(), "password123".to_string())
            .await
            .unwrap();

        let found_user = repo.find_by_id(created_user.id).await.unwrap();

        assert!(found_user.is_some());
        let found_user = found_user.unwrap();
        assert_eq!(found_user.id, created_user.id);
        assert_eq!(found_user.email, "byid@example.com");
    }

    #[tokio::test]
    async fn test_find_by_id_returns_none_when_not_found() {
        let (_container, pool) = setup_test_db().await;
        let repo = UserRepository::new(pool);

        let found_user = repo.find_by_id(Uuid::new_v4()).await.unwrap();

        assert!(found_user.is_none());
    }
}