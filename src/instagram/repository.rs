use crate::schema::oauth_tokens;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_diesel::InteractError;
use deadpool_diesel::postgres::Pool;
use diesel::prelude::*;
use tracing::{debug, error, info, instrument};
use uuid::Uuid;

#[derive(Debug, PartialEq, Clone, Queryable)]
pub struct OAuthToken {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub provider: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub provider_user_id: String,
    pub scopes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = oauth_tokens)]
pub struct NewOAuthToken {
    pub profile_id: Uuid,
    pub provider: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub provider_user_id: String,
    pub scopes: String,
}

#[derive(Debug, thiserror::Error)]
pub enum OAuthTokenRepositoryError {
    #[error("Database pool error: {0}")]
    PoolError(#[from] deadpool_diesel::PoolError),
    #[error("Diesel interaction error: {0}")]
    InteractError(#[from] InteractError),
    #[error("Diesel error: {0}")]
    DieselError(#[from] diesel::result::Error),
}

#[derive(Clone)]
pub struct OAuthTokenRepository {
    pool: Pool,
}

#[async_trait]
pub trait OAuthTokenRepositoryTrait: Send + Sync {
    async fn upsert(
        &self,
        profile_id: Uuid,
        provider: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
        provider_user_id: &str,
        scopes: &str,
    ) -> Result<OAuthToken, OAuthTokenRepositoryError>;
}

impl OAuthTokenRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OAuthTokenRepositoryTrait for OAuthTokenRepository {
    #[instrument(
        skip(self, access_token, refresh_token, provider_user_id, scopes),
        fields(profile_id = %profile_id, provider = %provider)
    )]
    async fn upsert(
        &self,
        profile_id: Uuid,
        provider: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
        provider_user_id: &str,
        scopes: &str,
    ) -> Result<OAuthToken, OAuthTokenRepositoryError> {
        info!("Upserting oauth token");

        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            OAuthTokenRepositoryError::PoolError(e)
        })?;

        let provider = provider.to_string();
        let access_token = access_token.to_string();
        let refresh_token = refresh_token.map(ToString::to_string);
        let provider_user_id = provider_user_id.to_string();
        let scopes = scopes.to_string();
        let now = Utc::now();

        let token = conn
            .interact(move |conn| {
                let new_token = NewOAuthToken {
                    profile_id,
                    provider: provider.clone(),
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    expires_at,
                    provider_user_id: provider_user_id.clone(),
                    scopes: scopes.clone(),
                };

                diesel::insert_into(oauth_tokens::table)
                    .values(&new_token)
                    .on_conflict((oauth_tokens::profile_id, oauth_tokens::provider))
                    .do_update()
                    .set((
                        oauth_tokens::access_token.eq(access_token),
                        oauth_tokens::refresh_token.eq(refresh_token),
                        oauth_tokens::expires_at.eq(expires_at),
                        oauth_tokens::provider_user_id.eq(provider_user_id),
                        oauth_tokens::scopes.eq(scopes),
                        oauth_tokens::updated_at.eq(now),
                    ))
                    .get_result::<OAuthToken>(conn)
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during oauth token upsert");
                OAuthTokenRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error during oauth token upsert");
                OAuthTokenRepositoryError::DieselError(e)
            })?;

        debug!(oauth_token_id = %token.id, "OAuth token upserted successfully");
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{profiles, users};
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

    async fn create_test_profile(pool: &Pool) -> Uuid {
        let user_id = Uuid::new_v4();
        let profile_id = Uuid::new_v4();
        let email = format!("test-{user_id}@example.com");
        let username = format!("creator_{profile_id}");
        let conn = pool.get().await.unwrap();

        conn.interact(move |conn| {
            diesel::insert_into(users::table)
                .values((
                    users::id.eq(user_id),
                    users::email.eq(email),
                    users::password.eq("hash"),
                ))
                .execute(conn)?;

            diesel::insert_into(profiles::table)
                .values((
                    profiles::id.eq(profile_id),
                    profiles::user_id.eq(user_id),
                    profiles::name.eq("Test Creator"),
                    profiles::bio.eq("Bio"),
                    profiles::niche.eq("Tech"),
                    profiles::avatar_url.eq("https://example.com/avatar.png"),
                    profiles::username.eq(username),
                ))
                .execute(conn)?;

            Ok::<_, diesel::result::Error>(profile_id)
        })
        .await
        .unwrap()
        .unwrap()
    }

    #[tokio::test]
    async fn upsert_inserts_new_oauth_token() {
        let (_container, pool) = setup_test_db().await;
        let profile_id = create_test_profile(&pool).await;
        let repo = OAuthTokenRepository::new(pool);
        let expires_at = Some(Utc::now() + chrono::Duration::hours(1));

        let token = repo
            .upsert(
                profile_id,
                "instagram",
                "access-1",
                Some("refresh-1"),
                expires_at,
                "ig-user-1",
                "instagram_basic,instagram_manage_insights",
            )
            .await
            .unwrap();

        assert_eq!(token.profile_id, profile_id);
        assert_eq!(token.provider, "instagram");
        assert_eq!(token.access_token, "access-1");
        assert_eq!(token.refresh_token.as_deref(), Some("refresh-1"));
        assert_eq!(
            token.expires_at.map(|v| v.timestamp_micros()),
            expires_at.map(|v| v.timestamp_micros())
        );
        assert_eq!(token.provider_user_id, "ig-user-1");
        assert_eq!(token.scopes, "instagram_basic,instagram_manage_insights");
    }

    #[tokio::test]
    async fn upsert_updates_existing_token_for_same_profile_and_provider() {
        let (_container, pool) = setup_test_db().await;
        let profile_id = create_test_profile(&pool).await;
        let repo = OAuthTokenRepository::new(pool);

        let first = repo
            .upsert(
                profile_id,
                "instagram",
                "access-1",
                Some("refresh-1"),
                Some(Utc::now() + chrono::Duration::hours(1)),
                "ig-user-1",
                "instagram_basic",
            )
            .await
            .unwrap();

        let second = repo
            .upsert(
                profile_id,
                "instagram",
                "access-2",
                None,
                None,
                "ig-user-2",
                "instagram_basic,instagram_manage_insights",
            )
            .await
            .unwrap();

        assert_eq!(second.id, first.id);
        assert_eq!(second.profile_id, profile_id);
        assert_eq!(second.access_token, "access-2");
        assert!(second.refresh_token.is_none());
        assert!(second.expires_at.is_none());
        assert_eq!(second.provider_user_id, "ig-user-2");
        assert_eq!(second.scopes, "instagram_basic,instagram_manage_insights");
        assert!(second.updated_at >= first.updated_at);
    }
}
