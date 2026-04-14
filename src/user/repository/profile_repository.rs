use crate::schema::profiles;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_diesel::InteractError;
use deadpool_diesel::postgres::Pool;
use diesel::prelude::*;
#[cfg(test)]
use mockall::{automock, predicate::*};
use uuid::Uuid;

#[derive(Debug, PartialEq, Queryable)]
pub struct Profile {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Insertable)]
#[diesel(table_name = profiles)]
pub struct NewProfile {
    pub user_id: Uuid,
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileRepositoryError {
    #[error("Database pool error: {0}")]
    PoolError(#[from] deadpool_diesel::PoolError),
    #[error("Diesel interaction error: {0}")]
    InteractError(#[from] InteractError),
    #[error("Diesel error: {0}")]
    DieselError(#[from] diesel::result::Error),
}

#[derive(Clone)]
pub struct ProfileRepository {
    pool: Pool,
}

#[cfg_attr(test, automock)]
#[async_trait]
pub trait ProfileRepositoryTrait {
    async fn create(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
    ) -> Result<Profile, ProfileRepositoryError>;
}

impl ProfileRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProfileRepositoryTrait for ProfileRepository {
    async fn create(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
    ) -> Result<Profile, ProfileRepositoryError> {
        let conn = self.pool.get().await?;

        let inserted_profile = conn
            .interact(move |conn| {
                let new_profile = NewProfile {
                    user_id,
                    name,
                    bio,
                    niche,
                    avatar_url,
                    username,
                };

                diesel::insert_into(profiles::table)
                    .values(&new_profile)
                    .get_result::<Profile>(conn)
            })
            .await??;

        Ok(inserted_profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user::repository::user_repository::NewUser;
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
        let conn = pool.get().await.unwrap();
        let id = Uuid::new_v4();
        conn.interact(move |conn| {
            diesel::insert_into(crate::schema::users::table)
                .values(&NewUser {
                    id,
                    email: format!("user-{}@example.com", id),
                    password: "hashed".to_string(),
                })
                .execute(conn)
        })
        .await
        .unwrap()
        .unwrap();
        id
    }

    #[tokio::test]
    async fn test_create_profile() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);

        let profile = repo
            .create(
                user_id,
                "Alice".to_string(),
                "Tech creator".to_string(),
                "technology".to_string(),
                "https://example.com/avatar.png".to_string(),
                "alice_tech".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(profile.user_id, user_id);
        assert_eq!(profile.name, "Alice");
        assert_eq!(profile.bio, "Tech creator");
        assert_eq!(profile.niche, "technology");
        assert_eq!(profile.avatar_url, "https://example.com/avatar.png");
        assert_eq!(profile.username, "alice_tech");
        assert!(profile.created_at.is_some());
        assert!(profile.updated_at.is_some());
    }

    #[tokio::test]
    async fn test_create_profile_duplicate_username_fails() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);

        repo.create(
            user_id,
            "Alice".to_string(),
            "Bio".to_string(),
            "niche".to_string(),
            "https://example.com/avatar.png".to_string(),
            "duplicate_user".to_string(),
        )
        .await
        .unwrap();

        let second_user_id = {
            let pool2 = repo.pool.clone();
            create_test_user(&pool2).await
        };

        let result = repo
            .create(
                second_user_id,
                "Bob".to_string(),
                "Bio".to_string(),
                "niche".to_string(),
                "https://example.com/avatar2.png".to_string(),
                "duplicate_user".to_string(),
            )
            .await;

        assert!(result.is_err(), "Expected duplicate username to fail");
    }

    #[tokio::test]
    async fn test_create_profile_returns_correct_id() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);

        let profile1 = repo
            .create(
                user_id,
                "User One".to_string(),
                "Bio one".to_string(),
                "niche".to_string(),
                "https://example.com/a.png".to_string(),
                "userone".to_string(),
            )
            .await
            .unwrap();

        let second_user_id = create_test_user(&repo.pool).await;
        let profile2 = repo
            .create(
                second_user_id,
                "User Two".to_string(),
                "Bio two".to_string(),
                "niche".to_string(),
                "https://example.com/b.png".to_string(),
                "usertwo".to_string(),
            )
            .await
            .unwrap();

        assert_ne!(profile1.id, profile2.id);
    }
}
