use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use std::str::FromStr;
use std::time::Instant;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use crate::instagram::client::{CodeExchange, IgClient, ShortLivedToken};
use crate::instagram::error::IgError;
use crate::instagram::repository::{
    OAuthToken, OAuthTokenRepository, OAuthTokenRepositoryError, OAuthTokenRepositoryTrait,
};
use crate::user::repository::profile_repository::{
    ProfileRepository, ProfileRepositoryError, ProfileRepositoryTrait, SocialHandle,
};

#[derive(Clone)]
pub struct InstagramService {
    client: IgClient,
    oauth_repository: OAuthTokenRepository,
    profile_repository: Option<ProfileRepository>,
}

#[derive(Debug, thiserror::Error)]
pub enum InstagramSyncError {
    #[error(transparent)]
    OAuthRepository(#[from] OAuthTokenRepositoryError),
    #[error(transparent)]
    ProfileRepository(#[from] ProfileRepositoryError),
    #[error(transparent)]
    Instagram(#[from] IgError),
    #[error("Instagram account is not connected")]
    NotConnected,
    #[error("Instagram service is missing profile repository dependency")]
    ServiceMisconfigured,
}

impl InstagramService {
    pub fn new(client: IgClient, oauth_repository: OAuthTokenRepository) -> Self {
        Self {
            client,
            oauth_repository,
            profile_repository: None,
        }
    }

    pub fn new_with_profile_repository(
        client: IgClient,
        oauth_repository: OAuthTokenRepository,
        profile_repository: ProfileRepository,
    ) -> Self {
        Self {
            client,
            oauth_repository,
            profile_repository: Some(profile_repository),
        }
    }

    #[instrument(skip(self), fields(state = %state))]
    pub fn build_authorize_url(&self, state: &str) -> String {
        let url = self.client.build_authorize_url(state);
        debug!("Instagram OAuth authorize URL built");
        url
    }

    #[instrument(skip_all)]
    pub async fn exchange_code(&self, code: &str) -> Result<ShortLivedToken, IgError> {
        info!("Exchanging Instagram OAuth code for short-lived token");
        let result = self.client.exchange_code(code).await;
        match &result {
            Ok(_) => debug!("Instagram code exchange successful"),
            Err(error) => log_sync_ig_error(error),
        }
        result
    }

    #[instrument(skip_all)]
    pub async fn exchange_for_long_lived(&self, short: &str) -> Result<CodeExchange, IgError> {
        info!("Exchanging Instagram short-lived token for long-lived token");
        let result = self.client.exchange_for_long_lived(short).await;
        match &result {
            Ok(token) => {
                debug!(expires_in = ?token.expires_in, "Long-lived token exchange successful")
            }
            Err(error) => log_sync_ig_error(error),
        }
        result
    }

    #[instrument(skip(self, access_token, refresh_token, scopes), fields(profile_id = %profile_id, provider = %provider))]
    pub async fn upsert_oauth_token(
        &self,
        profile_id: Uuid,
        provider: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
        provider_user_id: &str,
        scopes: &str,
    ) -> Result<OAuthToken, OAuthTokenRepositoryError> {
        info!("Upserting OAuth token");
        let result = self
            .oauth_repository
            .upsert(
                profile_id,
                provider,
                access_token,
                refresh_token,
                expires_at,
                provider_user_id,
                scopes,
            )
            .await;
        match &result {
            Ok(token) => info!(oauth_token_id = %token.id, "OAuth token upserted"),
            Err(e) => error!(error = %e, "Failed to upsert OAuth token"),
        }
        result
    }

    #[instrument(skip(self), fields(profile_id = %profile_id, provider = %provider))]
    pub async fn delete_oauth_token(
        &self,
        profile_id: Uuid,
        provider: &str,
    ) -> Result<bool, OAuthTokenRepositoryError> {
        info!("Deleting OAuth token");
        let result = self.oauth_repository.delete(profile_id, provider).await;
        match &result {
            Ok(true) => info!("OAuth token deleted"),
            Ok(false) => warn!("OAuth token delete: no token found to delete"),
            Err(e) => error!(error = %e, "Failed to delete OAuth token"),
        }
        result
    }

    #[tracing::instrument(skip(self), fields(profile_id = %profile_id))]
    pub async fn sync_profile(&self, profile_id: Uuid) -> Result<SocialHandle, InstagramSyncError> {
        let started_at = Instant::now();
        let oauth_token = self
            .oauth_repository
            .find_by_profile_and_provider(profile_id, "instagram")
            .await?
            .ok_or(InstagramSyncError::NotConnected)?;

        let existing_followers = self
            .profile_repository
            .as_ref()
            .ok_or(InstagramSyncError::ServiceMisconfigured)?
            .find_social_handles_by_profile_id(profile_id)
            .await?
            .into_iter()
            .find(|handle| handle.platform == "instagram")
            .map(|handle| handle.follower_count)
            .unwrap_or(0);

        let refreshed = self
            .client
            .refresh_long_lived_token(&oauth_token.access_token)
            .await
            .map_err(|error| {
                log_sync_ig_error(&error);
                InstagramSyncError::Instagram(error)
            })?;

        let expires_at = refreshed.expires_in.and_then(|seconds| {
            i64::try_from(seconds)
                .ok()
                .map(|seconds| Utc::now() + chrono::Duration::seconds(seconds))
        });

        let persisted_token = self
            .upsert_oauth_token(
                profile_id,
                "instagram",
                &refreshed.access_token,
                oauth_token.refresh_token.as_deref(),
                expires_at,
                &oauth_token.provider_user_id,
                &oauth_token.scopes,
            )
            .await?;

        let profile_stats = self
            .client
            .fetch_profile_stats(&persisted_token.access_token)
            .await
            .map_err(|error| {
                log_sync_ig_error(&error);
                InstagramSyncError::Instagram(error)
            })?;
        let recent_media = self
            .client
            .fetch_recent_media(&persisted_token.access_token, 25)
            .await
            .map_err(|error| {
                log_sync_ig_error(&error);
                InstagramSyncError::Instagram(error)
            })?;

        let username = profile_stats.username.unwrap_or_default();
        let url = if username.is_empty() {
            String::new()
        } else {
            format!("https://instagram.com/{username}")
        };
        let follower_count = profile_stats
            .followers_count
            .unwrap_or(0)
            .clamp(0, i64::from(i32::MAX)) as i32;
        let engagement_rate =
            BigDecimal::from_str(&recent_media.engagement_rate.to_string()).unwrap_or_default();

        let profile_repository = self
            .profile_repository
            .as_ref()
            .ok_or(InstagramSyncError::ServiceMisconfigured)?;

        let followers_count_delta = i64::from(follower_count) - i64::from(existing_followers);
        let duration_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        let social_handle = profile_repository
            .upsert_social_handle_sync_by_platform(
                profile_id,
                "instagram",
                username,
                url,
                follower_count,
                engagement_rate,
                Utc::now(),
            )
            .await
            .map_err(InstagramSyncError::from)?;

        info!(
            duration_ms,
            followers_count_delta,
            engagement_rate = recent_media.engagement_rate,
            "Instagram profile sync completed",
        );

        Ok(social_handle)
    }
}

/// Service-level Instagram sync error logging without token context.
///
/// `IgClient` emits token-aware diagnostics with redacted token fields; this
/// helper is only used where token context is unavailable in the service layer.
fn log_sync_ig_error(error: &IgError) {
    match error {
        IgError::RateLimited { retry_after } => warn!(
            retry_after_secs = retry_after.map(|duration| duration.as_secs()),
            "Instagram sync rate-limited",
        ),
        IgError::Graph { code, subcode, .. } => error!(
            graph_code = *code,
            graph_subcode = *subcode,
            "Instagram sync Graph API failure",
        ),
        IgError::Http { status, .. } => error!(status = *status, "Instagram sync HTTP failure",),
        _ => warn!("Instagram sync failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InstagramConfig;
    use crate::instagram::client::IgClient;
    use crate::instagram::repository::OAuthTokenRepository;
    use crate::schema::{profiles, users};
    use diesel::prelude::*;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use wiremock::matchers::{method, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

    fn test_config(base_url: &str) -> InstagramConfig {
        InstagramConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            redirect_uri: "http://localhost:3000/oauth/instagram/callback".to_string(),
            graph_api_version: "v25.0".to_string(),
        }
    }

    fn make_service(mock_server: &MockServer) -> InstagramService {
        let base = mock_server.uri();
        let cfg = test_config(&base);
        let client = IgClient::new_with_base_urls(cfg, base.clone(), base.clone(), base.clone());
        // Use a dummy pool for tests that don't hit the DB.
        let manager = deadpool_diesel::postgres::Manager::new(
            "postgres://postgres:postgres@127.0.0.1:5432/postgres",
            deadpool_diesel::Runtime::Tokio1,
        );
        let pool = deadpool_diesel::postgres::Pool::builder(manager)
            .max_size(1)
            .build()
            .expect("pool should build");
        InstagramService::new(client, OAuthTokenRepository::new(pool))
    }

    // ── build_authorize_url ───────────────────────────────────────────────────

    #[tokio::test]
    async fn build_authorize_url_passes_state_to_client() {
        let mock_server = MockServer::start().await;
        let service = make_service(&mock_server);
        let url = service.build_authorize_url("my-state");
        assert!(url.contains("state=my-state"), "url = {url}");
        assert!(url.contains("client_id=test-client-id"), "url = {url}");
        assert!(url.contains("response_type=code"), "url = {url}");
    }

    #[tokio::test]
    async fn build_authorize_url_different_states_produce_different_urls() {
        let mock_server = MockServer::start().await;
        let service = make_service(&mock_server);
        let url_a = service.build_authorize_url("state-aaa");
        let url_b = service.build_authorize_url("state-bbb");
        assert_ne!(url_a, url_b);
    }

    // ── exchange_code ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn exchange_code_success_returns_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex("/oauth/access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"access_token":"short-token","user_id":17841400000000001}"#,
                "application/json",
            ))
            .mount(&mock_server)
            .await;

        let service = make_service(&mock_server);
        let result = service.exchange_code("auth-code-123").await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let token = result.unwrap();
        assert_eq!(token.access_token, "short-token");
        assert_eq!(token.user_id, "17841400000000001");
    }

    #[tokio::test]
    async fn exchange_code_api_error_returns_err() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path_regex("/oauth/access_token"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        let service = make_service(&mock_server);
        let result = service.exchange_code("bad-code").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IgError::Unauthorized));
    }

    // ── exchange_for_long_lived ───────────────────────────────────────────────

    #[tokio::test]
    async fn exchange_for_long_lived_success_returns_token_with_expiry() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/access_token"))
            .and(query_param("grant_type", "ig_exchange_token"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"access_token":"long-token","token_type":"bearer","expires_in":5183944}"#,
                "application/json",
            ))
            .mount(&mock_server)
            .await;

        let service = make_service(&mock_server);
        let result = service.exchange_for_long_lived("short-token").await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let token = result.unwrap();
        assert_eq!(token.access_token, "long-token");
        assert_eq!(token.expires_in, Some(5183944));
    }

    #[tokio::test]
    async fn exchange_for_long_lived_api_error_returns_err() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/access_token"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&mock_server)
            .await;

        let service = make_service(&mock_server);
        let result = service.exchange_for_long_lived("short-token").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IgError::RateLimited { .. }));
    }

    // ── upsert_oauth_token / delete_oauth_token ───────────────────────────────

    async fn setup_test_db() -> (
        testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
        deadpool_diesel::postgres::Pool,
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

    async fn create_test_profile(pool: &deadpool_diesel::postgres::Pool) -> Uuid {
        let user_id = Uuid::new_v4();
        let profile_id = Uuid::new_v4();
        let email = format!("svc-test-{user_id}@example.com");
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

    fn make_db_service(pool: deadpool_diesel::postgres::Pool) -> InstagramService {
        let cfg = InstagramConfig {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "http://localhost/callback".to_string(),
            graph_api_version: "v25.0".to_string(),
        };
        InstagramService::new(IgClient::new(cfg), OAuthTokenRepository::new(pool))
    }

    #[tokio::test]
    async fn upsert_oauth_token_inserts_and_returns_token() {
        let (_container, pool) = setup_test_db().await;
        let profile_id = create_test_profile(&pool).await;
        let service = make_db_service(pool);

        let result = service
            .upsert_oauth_token(
                profile_id,
                "instagram",
                "access-token-1",
                None,
                None,
                "ig-user-1",
                "instagram_basic",
            )
            .await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let token = result.unwrap();
        assert_eq!(token.profile_id, profile_id);
        assert_eq!(token.provider, "instagram");
        assert_eq!(token.access_token, "access-token-1");
    }

    #[tokio::test]
    async fn upsert_oauth_token_updates_existing_token() {
        let (_container, pool) = setup_test_db().await;
        let profile_id = create_test_profile(&pool).await;
        let service = make_db_service(pool);

        service
            .upsert_oauth_token(profile_id, "instagram", "token-v1", None, None, "u1", "s1")
            .await
            .unwrap();

        let result = service
            .upsert_oauth_token(profile_id, "instagram", "token-v2", None, None, "u1", "s2")
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().access_token, "token-v2");
    }

    #[tokio::test]
    async fn delete_oauth_token_returns_true_when_token_exists() {
        let (_container, pool) = setup_test_db().await;
        let profile_id = create_test_profile(&pool).await;
        let service = make_db_service(pool);

        service
            .upsert_oauth_token(profile_id, "instagram", "tok", None, None, "u", "s")
            .await
            .unwrap();

        let deleted = service.delete_oauth_token(profile_id, "instagram").await;
        assert!(deleted.is_ok());
        assert!(deleted.unwrap(), "should return true when token existed");
    }

    #[tokio::test]
    async fn delete_oauth_token_returns_false_when_token_absent() {
        let (_container, pool) = setup_test_db().await;
        let profile_id = create_test_profile(&pool).await;
        let service = make_db_service(pool);

        // No token inserted — delete should return false.
        let deleted = service.delete_oauth_token(profile_id, "instagram").await;
        assert!(deleted.is_ok());
        assert!(
            !deleted.unwrap(),
            "should return false when no token present"
        );
    }
}
