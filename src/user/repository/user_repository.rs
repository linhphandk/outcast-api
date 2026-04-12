use crate::schema::users;
use deadpool_diesel::InteractError;
use deadpool_diesel::postgres::Pool;
use diesel::prelude::*;
use uuid::Uuid;

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

impl UserRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, email: String, password: String) -> Result<User, RepositoryError> {
        let conn = self.pool.get().await?;
        let id = Uuid::new_v4();

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
            .await??;

        Ok(inserted_user)
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
}
