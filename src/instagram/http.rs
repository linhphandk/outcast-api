use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{delete, get},
};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use serde::Deserialize;
use tracing::{error, info, instrument, warn};

use crate::instagram::{
    client::IgClient,
    repository::{OAuthTokenRepository, OAuthTokenRepositoryTrait},
    state::{OAUTH_STATE_COOKIE_NAME, verify_state_cookie},
};
use crate::user::http::auth_extractor::AuthUser;
use crate::user::repository::profile_repository::{ProfileRepository, ProfileRepositoryTrait};

const DASHBOARD_REDIRECT_PATH: &str = "/dashboard";
const EMPTY_PROVIDER_USER_ID: &str = "";
const EMPTY_SCOPES: &str = "";

#[derive(Debug, Deserialize)]
pub struct InstagramCallbackQuery {
    code: String,
    state: String,
}

#[instrument(skip_all)]
pub async fn instagram_callback(
    jar: CookieJar,
    Query(query): Query<InstagramCallbackQuery>,
    State(jwt_secret): State<String>,
    State(profile_repo): State<ProfileRepository>,
    State(client): State<IgClient>,
    State(oauth_repo): State<OAuthTokenRepository>,
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

    let short = match client.exchange_code(&query.code).await {
        Ok(token) => token,
        Err(err) => {
            error!(error = %err, "Failed to exchange Instagram OAuth code");
            return (StatusCode::BAD_GATEWAY, "Failed to exchange OAuth code").into_response();
        }
    };

    let long = match client.exchange_for_long_lived(&short.access_token).await {
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

    if let Err(err) = oauth_repo
        .upsert(
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

#[instrument(skip_all)]
pub async fn disconnect_instagram(
    auth_user: AuthUser,
    State(profile_repo): State<ProfileRepository>,
    State(oauth_repo): State<OAuthTokenRepository>,
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

    if let Err(err) = oauth_repo.delete(profile_id, "instagram").await {
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

pub fn router<S>() -> Router<S>
where
    String: axum::extract::FromRef<S>,
    ProfileRepository: axum::extract::FromRef<S>,
    OAuthTokenRepository: axum::extract::FromRef<S>,
    IgClient: axum::extract::FromRef<S>,
    std::sync::Arc<dyn crate::session::repository::session_repository::SessionRepositoryTrait>:
        axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/oauth/instagram/callback", get(instagram_callback))
        .route("/oauth/instagram", delete(disconnect_instagram))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InstagramConfig;
    use crate::instagram::state::issue_state_cookie;
    use axum::http::Request;
    use tower::ServiceExt;

    #[derive(Clone)]
    struct TestState {
        jwt_secret: String,
        profile_repository: ProfileRepository,
        oauth_repository: OAuthTokenRepository,
        instagram_client: IgClient,
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

    impl axum::extract::FromRef<TestState> for OAuthTokenRepository {
        fn from_ref(state: &TestState) -> Self {
            state.oauth_repository.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for IgClient {
        fn from_ref(state: &TestState) -> Self {
            state.instagram_client.clone()
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
            oauth_repository: OAuthTokenRepository::new(pool),
            instagram_client: IgClient::new(ig_cfg),
        }
    }

    fn app() -> Router {
        Router::new()
            .route("/oauth/instagram/callback", get(instagram_callback))
            .with_state(test_state())
    }

    #[tokio::test]
    async fn callback_missing_state_cookie_returns_bad_request() {
        let request = Request::builder()
            .uri("/oauth/instagram/callback?code=abc&state=state-1")
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
            .uri("/oauth/instagram/callback?code=abc&state=different-state")
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
        use axum::routing::post;
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
            oauth_repository: OAuthTokenRepository,
            instagram_client: IgClient,
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

        impl axum::extract::FromRef<IntegrationTestState> for OAuthTokenRepository {
            fn from_ref(state: &IntegrationTestState) -> Self {
                state.oauth_repository.clone()
            }
        }

        impl axum::extract::FromRef<IntegrationTestState> for IgClient {
            fn from_ref(state: &IntegrationTestState) -> Self {
                state.instagram_client.clone()
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
                oauth_repository,
                instagram_client: IgClient::new(ig_cfg),
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
