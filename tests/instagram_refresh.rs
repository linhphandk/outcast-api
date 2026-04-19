use std::sync::Arc;

use axum::{Router, body::Body, http::Request};
use bigdecimal::BigDecimal;
use chrono::Utc;
use diesel::prelude::*;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use http_body_util::BodyExt;
use outcast_api::{
    config::InstagramConfig,
    instagram::{
        client::IgClient,
        repository::{OAuthTokenRepository, OAuthTokenRepositoryTrait},
        service::InstagramService,
    },
    schema::{oauth_tokens, social_handles},
    session::{
        repository::session_repository::{SessionRepository, SessionRepositoryTrait},
        usecase::session_service::SessionService,
    },
    user::{
        http::user_controller::{CreateUserRes, router as user_router},
        repository::{
            profile_repository::{ProfileRepository, ProfileRepositoryTrait},
            user_repository::{UserRepository, UserRepositoryTrait},
        },
        usecase::user_service::UserService,
    },
};
use tower::ServiceExt;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
const TEST_PEPPER: &str = "test_pepper";
const TEST_JWT_SECRET: &str = "test_jwt_secret";

#[derive(Clone)]
struct TestState {
    user_service: UserService<UserRepository>,
    profile_repository: ProfileRepository,
    instagram_service: InstagramService,
    session_service: SessionService,
    session_repo: Arc<dyn SessionRepositoryTrait>,
    jwt_secret: String,
}

impl axum::extract::FromRef<TestState> for UserService<UserRepository> {
    fn from_ref(state: &TestState) -> Self {
        state.user_service.clone()
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

impl axum::extract::FromRef<TestState> for SessionService {
    fn from_ref(state: &TestState) -> Self {
        state.session_service.clone()
    }
}

impl axum::extract::FromRef<TestState> for Arc<dyn SessionRepositoryTrait> {
    fn from_ref(state: &TestState) -> Self {
        state.session_repo.clone()
    }
}

impl axum::extract::FromRef<TestState> for String {
    fn from_ref(state: &TestState) -> Self {
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

    let manager =
        deadpool_diesel::postgres::Manager::new(conn_string, deadpool_diesel::Runtime::Tokio1);
    let pool = deadpool_diesel::postgres::Pool::builder(manager).build().unwrap();

    let conn = pool.get().await.unwrap();
    conn.interact(|conn| conn.run_pending_migrations(MIGRATIONS).map(|_| ()))
        .await
        .unwrap()
        .unwrap();

    (container, pool)
}

fn build_app(pool: deadpool_diesel::postgres::Pool, mock_server: &MockServer) -> Router {
    let user_repository = UserRepository::new(pool.clone());
    let profile_repository = ProfileRepository::new(pool.clone());
    let oauth_repository = OAuthTokenRepository::new(pool.clone());
    let session_repo: Arc<dyn SessionRepositoryTrait> =
        Arc::new(SessionRepository::new(pool.clone()));
    let session_user_repository: Arc<dyn UserRepositoryTrait> =
        Arc::new(UserRepository::new(pool.clone()));
    let session_service = SessionService::new(session_repo.clone(), session_user_repository);
    let ig_cfg = InstagramConfig {
        client_id: "test-client-id".to_string(),
        client_secret: "test-client-secret".to_string(),
        redirect_uri: "http://localhost:3000/oauth/instagram/callback".to_string(),
        graph_api_version: "v19.0".to_string(),
    };
    let base_url = mock_server.uri();
    let instagram_client = IgClient::new_with_base_urls(
        ig_cfg,
        base_url.clone(),
        base_url.clone(),
        base_url,
    );

    let state = TestState {
        user_service: UserService::new(user_repository, TEST_PEPPER.to_string()),
        profile_repository: profile_repository.clone(),
        instagram_service: InstagramService::new_with_profile_repository(
            instagram_client,
            oauth_repository,
            profile_repository,
        ),
        session_service,
        session_repo,
        jwt_secret: TEST_JWT_SECRET.to_string(),
    };

    Router::new()
        .merge(user_router())
        .nest("/oauth/instagram", outcast_api::instagram::http::router())
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
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<CreateUserRes>(&body).unwrap()
}

async fn oauth_token_count(pool: &deadpool_diesel::postgres::Pool, profile_id: uuid::Uuid) -> i64 {
    let conn = pool.get().await.unwrap();
    conn.interact(move |conn| {
        oauth_tokens::table
            .filter(oauth_tokens::profile_id.eq(profile_id))
            .filter(oauth_tokens::provider.eq("instagram"))
            .count()
            .get_result::<i64>(conn)
    })
    .await
    .unwrap()
    .unwrap()
}

async fn social_handle_last_synced_at(
    pool: &deadpool_diesel::postgres::Pool,
    profile_id: uuid::Uuid,
) -> Option<chrono::DateTime<Utc>> {
    let conn = pool.get().await.unwrap();
    conn.interact(move |conn| {
        social_handles::table
            .filter(social_handles::profile_id.eq(profile_id))
            .filter(social_handles::platform.eq("instagram"))
            .select(social_handles::last_synced_at)
            .first::<Option<chrono::DateTime<Utc>>>(conn)
            .optional()
    })
    .await
    .unwrap()
    .unwrap()
    .flatten()
}

fn mount_refresh_success_mocks<'a>(
    mock_server: &'a MockServer,
    access_token: &'a str,
    refreshed_token: &'a str,
) -> impl std::future::Future<Output = ()> + 'a {
    async move {
        Mock::given(method("GET"))
            .and(path("/refresh_access_token"))
            .and(query_param("grant_type", "ig_refresh_token"))
            .and(query_param("access_token", access_token))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": refreshed_token,
                "token_type": "bearer",
                "expires_in": 5183944
            })))
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v19.0/me/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "page-1"}]
            })))
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v19.0/page-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "instagram_business_account": {"id": "ig-user-1"},
                "id": "page-1"
            })))
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v19.0/ig-user-1/media"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"id": "media-1", "like_count": 5, "comments_count": 3},
                    {"id": "media-2", "like_count": 2, "comments_count": 1}
                ]
            })))
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v19.0/ig-user-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ig-user-1",
                "username": "test_creator",
                "followers_count": 1000
            })))
            .mount(mock_server)
            .await;
    }
}

/// End-to-end test: connect Instagram (mock happy OAuth), first refresh succeeds,
/// second refresh returns 401 from Graph API.
///
/// Assertions:
/// - Endpoint returns 401 with `{"error": "instagram_reauth_required"}`
/// - `oauth_tokens` row is deleted
/// - `social_handles.last_synced_at` is cleared (NULL)
#[tokio::test]
async fn instagram_refresh_on_graph_401_cleans_up_token_and_returns_reauth_required() {
    let (_container, pool) = setup_test_db().await;
    let mock_server = MockServer::start().await;
    let app = build_app(pool.clone(), &mock_server);

    // --- Connect: create user, profile, and insert an OAuth token directly ---
    let created = create_user(&app, "ig_refresh_401@example.com").await;

    let profile_repo = ProfileRepository::new(pool.clone());
    let profile = profile_repo
        .create(
            created.id,
            "IG Refresh User".to_string(),
            "Creator".to_string(),
            "tech".to_string(),
            "https://example.com/avatar.png".to_string(),
            "ig_refresh_401_user".to_string(),
        )
        .await
        .unwrap();

    let oauth_repo = OAuthTokenRepository::new(pool.clone());
    oauth_repo
        .upsert(
            profile.id,
            "instagram",
            "long-lived-token",
            None,
            None,
            "ig-user-1",
            "instagram_basic",
        )
        .await
        .unwrap();

    // Seed the social handle with a last_synced_at in the past (> cooldown)
    // so the first refresh won't be blocked.
    let old_sync_time = Utc::now() - chrono::Duration::minutes(10);
    profile_repo
        .upsert_social_handle_sync_by_platform(
            profile.id,
            "instagram",
            "test_creator".to_string(),
            "https://instagram.com/test_creator".to_string(),
            500,
            BigDecimal::from(5),
            old_sync_time,
        )
        .await
        .unwrap();

    // --- First refresh: mock all Graph API endpoints to succeed ---
    mount_refresh_success_mocks(&mock_server, "long-lived-token", "refreshed-token").await;

    let first_refresh = Request::builder()
        .method("POST")
        .uri("/oauth/instagram/refresh")
        .header("Authorization", format!("Bearer {}", created.token))
        .body(Body::empty())
        .unwrap();

    let first_response = app.clone().oneshot(first_refresh).await.unwrap();
    let first_status = first_response.status();
    let first_body = first_response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        first_status,
        axum::http::StatusCode::OK,
        "First refresh should succeed; body: {:?}",
        String::from_utf8_lossy(&first_body)
    );

    // oauth_tokens row should still exist with the refreshed token
    assert_eq!(oauth_token_count(&pool, profile.id).await, 1);

    // Reset last_synced_at to a past timestamp to bypass cooldown for the second call
    let conn = pool.get().await.unwrap();
    let pid = profile.id;
    conn.interact(move |conn| {
        use outcast_api::schema::social_handles;
        diesel::update(
            social_handles::table
                .filter(social_handles::profile_id.eq(pid))
                .filter(social_handles::platform.eq("instagram")),
        )
        .set(
            social_handles::last_synced_at
                .eq(Some(Utc::now() - chrono::Duration::minutes(10))),
        )
        .execute(conn)
    })
    .await
    .unwrap()
    .unwrap();

    // --- Second refresh: Graph API returns 401 ---
    Mock::given(method("GET"))
        .and(path("/refresh_access_token"))
        .and(query_param("grant_type", "ig_refresh_token"))
        .and(query_param("access_token", "refreshed-token"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&mock_server)
        .await;

    let second_refresh = Request::builder()
        .method("POST")
        .uri("/oauth/instagram/refresh")
        .header("Authorization", format!("Bearer {}", created.token))
        .body(Body::empty())
        .unwrap();

    let second_response = app.clone().oneshot(second_refresh).await.unwrap();
    assert_eq!(
        second_response.status(),
        axum::http::StatusCode::UNAUTHORIZED,
        "Second refresh should return 401"
    );

    // Response body should contain {"error": "instagram_reauth_required"}
    let body_bytes = second_response.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        body["error"],
        "instagram_reauth_required",
        "Response body should indicate reauth required"
    );

    // oauth_tokens row should be deleted
    assert_eq!(
        oauth_token_count(&pool, profile.id).await,
        0,
        "oauth_tokens row should be deleted on upstream 401"
    );

    // social_handles.last_synced_at should be cleared (NULL)
    let last_synced = social_handle_last_synced_at(&pool, profile.id).await;
    assert!(
        last_synced.is_none(),
        "last_synced_at should be cleared (NULL) after upstream 401"
    );
}
