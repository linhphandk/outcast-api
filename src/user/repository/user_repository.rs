use async_trait::async_trait;
use deadpool_postgres::{Client, Pool};
use uuid::Uuid;

pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Database pool error: {0}")]
    PoolError(#[from] deadpool_postgres::PoolError),
    #[error("PostgreSQL error: {0}")]
    PgError(#[from] tokio_postgres::Error),
}

#[async_trait]
pub trait UserRepository {
    async fn create(&self, username: &str, email: &str) -> Result<User, RepositoryError>;
}

pub struct PgUserRepository {
    pool: Pool,
}

impl PgUserRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn create(&self, username: &str, email: &str) -> Result<User, RepositoryError> {
        let client: Client = self.pool.get().await?;
        let id = Uuid::new_v4();

        let stmt = client
            .prepare_cached("INSERT INTO users (id, username, email) VALUES ($1, $2, $3)")
            .await?;

        client.execute(&stmt, &[&id, &username, &email]).await?;

        Ok(User {
            id,
            username: username.to_string(),
            email: email.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deadpool_postgres::Runtime;
    use std::env;

    /// Helper to initialize a connection pool for testing.
    /// It defaults to a standard local configuration if DATABASE_URL is unset.
    async fn get_test_pool() -> Pool {
        dotenvy::dotenv().ok();
        let mut cfg = deadpool_postgres::Config::new();
        cfg.url = Some(env::var("DATABASE_URL").unwrap_or_else(|_| {
            "host=localhost user=postgres password=postgres dbname=postgres".to_string()
        }));
        cfg.create_pool(Some(Runtime::Tokio1), tokio_postgres::NoTls)
            .expect("Failed to create test pool")
    }

    #[tokio::test]
    async fn test_pg_user_repository_create_success() {
        let pool = get_test_pool().await;
        let repo = PgUserRepository::new(pool);

        let test_username = format!("test_user_{}", Uuid::new_v4());
        let test_email = format!("{}@example.com", test_username);

        let result = repo.create(&test_username, &test_email).await;

        assert!(result.is_ok(), "Repository should successfully create a user");
        
        let user = result.unwrap();
        assert_eq!(user.username, test_username);
        assert_eq!(user.email, test_email);
        assert!(!user.id.is_nil(), "User should have a generated UUID");
    }
}