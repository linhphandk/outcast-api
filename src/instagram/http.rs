use axum::{
    Json,
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, instrument, warn};

use crate::instagram::{
    error::IgError,
    service::InstagramService,
    service::InstagramSyncError,
    state::{OAUTH_STATE_COOKIE_NAME, issue_state_cookie, verify_state_cookie},
};
use crate::user::http::auth_extractor::AuthUser;
use crate::user::repository::profile_repository::{
    ProfileRepository, ProfileRepositoryTrait, SocialHandle,
};

const DASHBOARD_REDIRECT_PATH: &str = "/dashboard";
const EMPTY_PROVIDER_USER_ID: &str = "";
const EMPTY_SCOPES: &str = "";
const INSTAGRAM_REFRESH_COOLDOWN: Duration = Duration::minutes(5);

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct InstagramCallbackQuery {
    code: String,
    state: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct InstagramSocialHandleRes {
    pub platform: String,
    pub handle: String,
    pub url: String,
    pub follower_count: i32,
    pub engagement_rate: String,
    pub last_synced_at: Option<String>,
}

impl From<SocialHandle> for InstagramSocialHandleRes {
    fn from(value: SocialHandle) -> Self {
        Self {
            platform: value.platform,
            handle: value.handle,
            url: value.url,
            follower_count: value.follower_count,
            engagement_rate: value.engagement_rate.to_string(),
            last_synced_at: value.last_synced_at.map(|v| v.to_rfc3339()),
        }
    }
}

#[utoipa::path(
    get,
    path = "/oauth/instagram",
    responses(
        (status = 303, description = "Redirect to Instagram authorization page"),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_token" = [])),
    tag = "Instagram OAuth"
)]
#[instrument(skip_all)]
pub async fn instagram_authorize(
    auth_user: AuthUser,
    jar: CookieJar,
    State(jwt_secret): State<String>,
    State(instagram_service): State<InstagramService>,
) -> impl IntoResponse {
    let (state, state_cookie) = issue_state_cookie(auth_user.user_id, jwt_secret.as_bytes());
    let authorize_url = instagram_service.build_authorize_url(&state);
    let jar = jar.add(state_cookie);
    (jar, Redirect::to(&authorize_url)).into_response()
}

#[utoipa::path(
    get,
    path = "/oauth/instagram/callback",
    params(
        ("code" = String, Query, description = "Authorization code returned by Instagram OAuth"),
        ("state" = String, Query, description = "OAuth state returned by Instagram OAuth")
    ),
    responses(
        (status = 303, description = "Redirect back to dashboard after successful OAuth callback"),
        (status = 400, description = "Invalid or missing OAuth state"),
        (status = 404, description = "Profile not found"),
        (status = 500, description = "Failed to persist OAuth token"),
        (status = 502, description = "Instagram token exchange failed")
    ),
    tag = "Instagram OAuth"
)]
#[instrument(skip_all)]
pub async fn instagram_callback(
    jar: CookieJar,
    Query(query): Query<InstagramCallbackQuery>,
    State(jwt_secret): State<String>,
    State(profile_repo): State<ProfileRepository>,
    State(instagram_service): State<InstagramService>,
) -> impl IntoResponse {
    let state_cookie = match jar.get(OAUTH_STATE_COOKIE_NAME) {
        Some(cookie) => cookie.value(),
        None => {
            warn!("Instagram OAuth callback missing state cookie");
            return (StatusCode::BAD_REQUEST, "Missing OAuth state cookie").into_response();
        }
    };

    let user_id = match verify_state_cookie(&query.state, state_cookie, jwt_secret.as_bytes()) {
        Ok(user_id) => user_id,
        Err(err) => {
            warn!(error = %err, "Instagram OAuth callback state verification failed");
            return (StatusCode::BAD_REQUEST, "Invalid OAuth state").into_response();
        }
    };

    let profile_id = match profile_repo.find_by_user_id(user_id).await {
        Ok(Some(profile)) => profile.id,
        Ok(None) => {
            warn!(user_id = %user_id, "Instagram OAuth callback user profile not found");
            return (StatusCode::NOT_FOUND, "Profile not found").into_response();
        }
        Err(err) => {
            error!(error = %err, user_id = %user_id, "Failed to resolve profile for Instagram OAuth callback");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to resolve profile",
            )
                .into_response();
        }
    };

    let short = match instagram_service.exchange_code(&query.code).await {
        Ok(token) => token,
        Err(err) => {
            error!(error = %err, "Failed to exchange Instagram OAuth code");
            return (StatusCode::BAD_GATEWAY, "Failed to exchange OAuth code").into_response();
        }
    };

    let long = match instagram_service.exchange_for_long_lived(&short.access_token).await {
        Ok(token) => token,
        Err(err) => {
            error!(error = %err, "Failed to exchange for Instagram long-lived token");
            return (
                StatusCode::BAD_GATEWAY,
                "Failed to exchange for long-lived token",
            )
                .into_response();
        }
    };

    let expires_at = long.expires_in.and_then(|seconds| {
        i64::try_from(seconds)
            .ok()
            .map(|seconds| Utc::now() + Duration::seconds(seconds))
    });

    if let Err(err) = instagram_service
        .upsert_oauth_token(
            profile_id,
            "instagram",
            &long.access_token,
            None,
            expires_at,
            EMPTY_PROVIDER_USER_ID,
            EMPTY_SCOPES,
        )
        .await
    {
        error!(error = %err, profile_id = %profile_id, "Failed to upsert Instagram OAuth token");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to persist OAuth token",
        )
            .into_response();
    }

    info!(profile_id = %profile_id, "Instagram OAuth callback completed");
    Redirect::to(DASHBOARD_REDIRECT_PATH).into_response()
}

#[utoipa::path(
    delete,
    path = "/oauth/instagram",
    responses(
        (status = 204, description = "Instagram disconnected"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_token" = [])),
    tag = "Instagram OAuth"
)]
#[instrument(skip_all)]
pub async fn disconnect_instagram(
    auth_user: AuthUser,
    State(profile_repo): State<ProfileRepository>,
    State(instagram_service): State<InstagramService>,
) -> impl IntoResponse {
    let profile_id = match profile_repo.find_by_user_id(auth_user.user_id).await {
        Ok(Some(profile)) => profile.id,
        Ok(None) => {
            warn!(user_id = %auth_user.user_id, "Disconnect Instagram: profile not found");
            return StatusCode::NO_CONTENT;
        }
        Err(err) => {
            error!(error = %err, user_id = %auth_user.user_id, "Failed to resolve profile for Instagram disconnect");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    };

    if let Err(err) = instagram_service.delete_oauth_token(profile_id, "instagram").await {
        error!(error = %err, profile_id = %profile_id, "Failed to delete Instagram OAuth token");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    if let Err(err) = profile_repo
        .reset_social_handle_by_platform(profile_id, "instagram")
        .await
    {
        error!(error = %err, profile_id = %profile_id, "Failed to reset Instagram social handle");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    info!(profile_id = %profile_id, "Instagram disconnected successfully");
    StatusCode::NO_CONTENT
}

#[utoipa::path(
    post,
    path = "/oauth/instagram/refresh",
    responses(
        (status = 200, description = "Instagram social handle refreshed", body = InstagramSocialHandleRes),
        (status = 401, description = "Unauthorized"),
        (status = 429, description = "Rate limited by Instagram"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_token" = [])),
    tag = "Instagram OAuth"
)]
#[instrument(skip_all)]
pub async fn refresh_instagram(
    auth_user: AuthUser,
    State(profile_repo): State<ProfileRepository>,
    State(instagram_service): State<InstagramService>,
) -> impl IntoResponse {
    let profile_id = match profile_repo.find_by_user_id(auth_user.user_id).await {
        Ok(Some(profile)) => profile.id,
        Ok(None) => {
            warn!(user_id = %auth_user.user_id, "Instagram refresh: profile not found");
            return (StatusCode::NOT_FOUND, "Profile not found").into_response();
        }
        Err(err) => {
            error!(error = %err, user_id = %auth_user.user_id, "Failed to resolve profile for Instagram refresh");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to resolve profile").into_response();
        }
    };

    match profile_repo
        .find_social_handle_last_synced_at_by_platform(profile_id, "instagram")
        .await
    {
        Ok(Some(last_synced_at)) => {
            let elapsed = Utc::now().signed_duration_since(last_synced_at);
            if elapsed < INSTAGRAM_REFRESH_COOLDOWN {
                let retry_after_seconds = (INSTAGRAM_REFRESH_COOLDOWN - elapsed)
                    .num_seconds()
                    .clamp(1, INSTAGRAM_REFRESH_COOLDOWN.num_seconds());

                warn!(
                    profile_id = %profile_id,
                    retry_after_seconds,
                    "Instagram refresh blocked by profile cooldown"
                );

                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(
                        axum::http::header::RETRY_AFTER,
                        retry_after_seconds.to_string(),
                    )],
                    "Instagram refresh cooldown active",
                )
                    .into_response();
            }
        }
        Ok(None) => {}
        Err(err) => {
            error!(error = %err, profile_id = %profile_id, "Failed to read Instagram refresh cooldown state");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to resolve Instagram refresh cooldown",
            )
                .into_response();
        }
    }

    match instagram_service.sync_profile(profile_id).await {
        Ok(social_handle) => (
            StatusCode::OK,
            Json(InstagramSocialHandleRes::from(social_handle)),
        )
            .into_response(),
        Err(InstagramSyncError::Instagram(IgError::Unauthorized)) => {
            warn!(profile_id = %profile_id, "Instagram token unauthorized: deleting token and clearing sync timestamp");
            if let Err(err) = instagram_service.delete_oauth_token(profile_id, "instagram").await {
                error!(error = %err, profile_id = %profile_id, "Failed to delete Instagram OAuth token after 401");
            }
            if let Err(err) = profile_repo
                .clear_social_handle_last_synced_at_by_platform(profile_id, "instagram")
                .await
            {
                error!(error = %err, profile_id = %profile_id, "Failed to clear Instagram social handle last_synced_at after 401");
            }
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "instagram_reauth_required"})),
            )
                .into_response()
        }
        Err(InstagramSyncError::Instagram(IgError::RateLimited { .. })) => {
            (StatusCode::TOO_MANY_REQUESTS, "Instagram rate limited").into_response()
        }
        Err(InstagramSyncError::NotConnected) => {
            (StatusCode::NOT_FOUND, "Instagram account not connected").into_response()
        }
        Err(err) => {
            error!(error = %err, profile_id = %profile_id, "Instagram refresh failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Instagram refresh failed").into_response()
        }
    }
}

pub fn router<S>() -> Router<S>
where
    String: axum::extract::FromRef<S>,
    ProfileRepository: axum::extract::FromRef<S>,
    InstagramService: axum::extract::FromRef<S>,
    std::sync::Arc<dyn crate::session::repository::session_repository::SessionRepositoryTrait>:
        axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(instagram_authorize).delete(disconnect_instagram))
        .route("/callback", get(instagram_callback))
        .route("/refresh", post(refresh_instagram))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InstagramConfig;
    use crate::instagram::client::IgClient;
    use crate::instagram::repository::OAuthTokenRepository;
    use crate::instagram::repository::OAuthTokenRepositoryTrait;
    use crate::instagram::service::InstagramService;
    use crate::instagram::state::issue_state_cookie;
    use axum::http::Request;
    use tower::ServiceExt;

    #[derive(Clone)]
    struct TestState {
        jwt_secret: String,
        profile_repository: ProfileRepository,
        instagram_service: InstagramService,
    }

    impl axum::extract::FromRef<TestState> for String {
        fn from_ref(state: &TestState) -> Self {
            state.jwt_secret.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for ProfileRepository {
        fn from_ref(state: &TestState) -> Self {
            state.profile_repository.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for InstagramService {
        fn from_ref(state: &TestState) -> Self {
            state.instagram_service.clone()
        }
    }

    fn test_state() -> TestState {
        let manager = deadpool_diesel::postgres::Manager::new(
            "postgres://postgres:postgres@127.0.0.1:5432/postgres",
            deadpool_diesel::Runtime::Tokio1,
        );
        let pool = deadpool_diesel::postgres::Pool::builder(manager)
            .max_size(1)
            .build()
            .expect("pool should build");

        let ig_cfg = InstagramConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            redirect_uri: "http://localhost:3000/oauth/instagram/callback".to_string(),
            graph_api_version: "v19.0".to_string(),
        };

        TestState {
            jwt_secret: "test-jwt-secret".to_string(),
            profile_repository: ProfileRepository::new(pool.clone()),
            instagram_service: InstagramService::new(
                IgClient::new(ig_cfg),
                OAuthTokenRepository::new(pool),
            ),
        }
    }

    fn app() -> Router {
        Router::new()
            .route("/callback", get(instagram_callback))
            .with_state(test_state())
    }

    #[tokio::test]
    async fn callback_missing_state_cookie_returns_bad_request() {
        let request = Request::builder()
            .uri("/callback?code=abc&state=state-1")
            .body(axum::body::Body::empty())
            .expect("request should build");

        let response = app().oneshot(request).await.expect("response should be returned");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn callback_invalid_state_returns_bad_request() {
        let (_state, cookie) = issue_state_cookie(
            uuid::Uuid::new_v4(),
            test_state().jwt_secret.as_bytes(),
        );

        let request = Request::builder()
            .uri("/callback?code=abc&state=different-state")
            .header("Cookie", format!("{}={}", cookie.name(), cookie.value()))
            .body(axum::body::Body::empty())
            .expect("request should build");

        let response = app().oneshot(request).await.expect("response should be returned");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ── Integration tests for DELETE /oauth/instagram ────────────────────────

    mod disconnect_tests {
        use super::*;
        use crate::session::repository::session_repository::{
            SessionRepository, SessionRepositoryTrait,
        };
        use crate::session::usecase::session_service::SessionService;
        use crate::user::http::user_controller::CreateUserRes;
        use crate::user::repository::profile_repository::ProfileRepositoryTrait;
        use crate::user::repository::user_repository::{UserRepository, UserRepositoryTrait};
        use crate::user::usecase::user_service::UserService;
        use axum::body::Body;
        use axum::routing::{delete, post};
        use bigdecimal::BigDecimal;
        use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
        use http_body_util::BodyExt;
        use std::sync::Arc;

        pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
        const TEST_PEPPER: &str = "test_pepper";
        const TEST_JWT_SECRET: &str = "test_jwt_secret";

        #[derive(Clone)]
        struct IntegrationTestState {
            user_service: UserService<UserRepository>,
            profile_repository: ProfileRepository,
            instagram_service: InstagramService,
            session_service: SessionService,
            session_repo: Arc<dyn SessionRepositoryTrait>,
            jwt_secret: String,
        }

        impl axum::extract::FromRef<IntegrationTestState> for UserService<UserRepository> {
            fn from_ref(state: &IntegrationTestState) -> Self {
                state.user_service.clone()
            }
        }

        impl axum::extract::FromRef<IntegrationTestState> for ProfileRepository {
            fn from_ref(state: &IntegrationTestState) -> Self {
                state.profile_repository.clone()
            }
        }

        impl axum::extract::FromRef<IntegrationTestState> for InstagramService {
            fn from_ref(state: &IntegrationTestState) -> Self {
                state.instagram_service.clone()
            }
        }

        impl axum::extract::FromRef<IntegrationTestState> for SessionService {
            fn from_ref(state: &IntegrationTestState) -> Self {
                state.session_service.clone()
            }
        }

        impl axum::extract::FromRef<IntegrationTestState> for Arc<dyn SessionRepositoryTrait> {
            fn from_ref(state: &IntegrationTestState) -> Self {
                state.session_repo.clone()
            }
        }

        impl axum::extract::FromRef<IntegrationTestState> for String {
            fn from_ref(state: &IntegrationTestState) -> Self {
                state.jwt_secret.clone()
            }
        }

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

            let manager = deadpool_diesel::postgres::Manager::new(
                conn_string,
                deadpool_diesel::Runtime::Tokio1,
            );
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

        fn build_app(pool: deadpool_diesel::postgres::Pool) -> Router {
            let user_repository = UserRepository::new(pool.clone());
            let profile_repository = ProfileRepository::new(pool.clone());
            let oauth_repository = OAuthTokenRepository::new(pool.clone());
            let session_repo: Arc<dyn SessionRepositoryTrait> =
                Arc::new(SessionRepository::new(pool.clone()));
            let session_user_repository: Arc<dyn UserRepositoryTrait> =
                Arc::new(UserRepository::new(pool.clone()));
            let session_service =
                SessionService::new(session_repo.clone(), session_user_repository);
            let ig_cfg = InstagramConfig {
                client_id: "test-client-id".to_string(),
                client_secret: "test-client-secret".to_string(),
                redirect_uri: "http://localhost:3000/oauth/instagram/callback".to_string(),
                graph_api_version: "v19.0".to_string(),
            };

            let state = IntegrationTestState {
                user_service: UserService::new(user_repository, TEST_PEPPER.to_string()),
                profile_repository,
                instagram_service: InstagramService::new(IgClient::new(ig_cfg), oauth_repository),
                session_service,
                session_repo,
                jwt_secret: TEST_JWT_SECRET.to_string(),
            };

            Router::new()
                .route("/user", post(crate::user::http::user_controller::create_user))
                .route("/oauth/instagram", delete(disconnect_instagram))
                .with_state(state)
        }

        async fn create_user(app: &Router, email: &str) -> CreateUserRes {
            let request = Request::builder()
                .method("POST")
                .uri("/user")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": email,
                        "password": "password123"
                    })
                    .to_string(),
                ))
                .unwrap();

            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
            let body = response.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice::<CreateUserRes>(&body).unwrap()
        }

        #[tokio::test]
        async fn disconnect_with_connected_account_returns_204() {
            let (_container, pool) = setup_test_db().await;
            let app = build_app(pool.clone());
            let created = create_user(&app, "ig_disconnect_ok@example.com").await;

            let profile_repo = ProfileRepository::new(pool.clone());
            let profile = profile_repo
                .create(
                    created.id,
                    "Alice".to_string(),
                    "Tech creator".to_string(),
                    "technology".to_string(),
                    "https://example.com/alice.png".to_string(),
                    "alice_disconnect".to_string(),
                )
                .await
                .unwrap();

            // Insert a social handle for instagram
            profile_repo
                .add_social_handle(
                    profile.id,
                    "instagram".to_string(),
                    "@alice_tech".to_string(),
                    "https://instagram.com/alice_tech".to_string(),
                    50_000,
                )
                .await
                .unwrap();

            // Insert an oauth token
            let oauth_repo = OAuthTokenRepository::new(pool.clone());
            oauth_repo
                .upsert(
                    profile.id,
                    "instagram",
                    "access-token-1",
                    None,
                    None,
                    "ig-user-1",
                    "instagram_basic",
                )
                .await
                .unwrap();

            // DELETE /oauth/instagram
            let request = Request::builder()
                .method("DELETE")
                .uri("/oauth/instagram")
                .header("Authorization", format!("Bearer {}", created.token))
                .body(Body::empty())
                .unwrap();

            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);

            // Verify social handle is zeroed out
            let handles = profile_repo
                .find_social_handles_by_profile_id(profile.id)
                .await
                .unwrap();
            assert_eq!(handles.len(), 1);
            assert_eq!(handles[0].follower_count, 0);
            assert_eq!(handles[0].engagement_rate, BigDecimal::from(0));
            assert!(handles[0].last_synced_at.is_none());
            // handle and url are preserved
            assert_eq!(handles[0].handle, "@alice_tech");
            assert_eq!(handles[0].url, "https://instagram.com/alice_tech");
        }

        #[tokio::test]
        async fn disconnect_without_connected_account_returns_204() {
            let (_container, pool) = setup_test_db().await;
            let app = build_app(pool.clone());
            let created = create_user(&app, "ig_disconnect_noop@example.com").await;

            let profile_repo = ProfileRepository::new(pool.clone());
            profile_repo
                .create(
                    created.id,
                    "Bob".to_string(),
                    "No IG".to_string(),
                    "lifestyle".to_string(),
                    "https://example.com/bob.png".to_string(),
                    "bob_noop".to_string(),
                )
                .await
                .unwrap();

            // No oauth token or social handle for instagram

            let request = Request::builder()
                .method("DELETE")
                .uri("/oauth/instagram")
                .header("Authorization", format!("Bearer {}", created.token))
                .body(Body::empty())
                .unwrap();

            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }
    }
}
