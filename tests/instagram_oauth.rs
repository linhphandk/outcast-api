use std::sync::Arc;

use axum::{Router, body::Body, http::Request};
use diesel::prelude::*;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use http_body_util::BodyExt;
use outcast_api::{
    config::InstagramConfig,
    instagram::{
        client::IgClient,
        repository::OAuthTokenRepository,
        service::InstagramService,
    },
    schema::oauth_tokens,
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

    let manager = deadpool_diesel::postgres::Manager::new(conn_string, deadpool_diesel::Runtime::Tokio1);
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
    let session_repo: Arc<dyn SessionRepositoryTrait> = Arc::new(SessionRepository::new(pool.clone()));
    let session_user_repository: Arc<dyn UserRepositoryTrait> = Arc::new(UserRepository::new(pool.clone()));
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
        profile_repository,
        instagram_service: InstagramService::new(instagram_client, oauth_repository),
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

#[tokio::test]
async fn instagram_oauth_authorize_callback_and_disconnect_flow() {
    let (_container, pool) = setup_test_db().await;
    let mock_server = MockServer::start().await;
    let app = build_app(pool.clone(), &mock_server);
    let created = create_user(&app, "ig_oauth_flow@example.com").await;

    let profile_repo = ProfileRepository::new(pool.clone());
    let profile = profile_repo
        .create(
            created.id,
            "IG User".to_string(),
            "Creator".to_string(),
            "tech".to_string(),
            "https://example.com/avatar.png".to_string(),
            "ig_user".to_string(),
        )
        .await
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/v19.0/oauth/access_token"))
        .and(query_param("code", "oauth-code-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "short-lived-token",
            "token_type": "bearer",
            "expires_in": 3600
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/access_token"))
        .and(query_param("grant_type", "ig_exchange_token"))
        .and(query_param("access_token", "short-lived-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "long-lived-token",
            "token_type": "bearer",
            "expires_in": 5183944
        })))
        .mount(&mock_server)
        .await;

    let authorize_request = Request::builder()
        .method("GET")
        .uri("/oauth/instagram")
        .header("Authorization", format!("Bearer {}", created.token))
        .body(Body::empty())
        .unwrap();
    let authorize_response = app.clone().oneshot(authorize_request).await.unwrap();
    assert_eq!(
        authorize_response.status(),
        axum::http::StatusCode::SEE_OTHER
    );

    let location = authorize_response
        .headers()
        .get(axum::http::header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    let callback_state = url::Url::parse(location)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .unwrap();
    let oauth_cookie = authorize_response
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let callback_request = Request::builder()
        .method("GET")
        .uri(format!(
            "/oauth/instagram/callback?code=oauth-code-123&state={callback_state}"
        ))
        .header(axum::http::header::COOKIE, oauth_cookie)
        .body(Body::empty())
        .unwrap();
    let callback_response = app.clone().oneshot(callback_request).await.unwrap();
    assert_eq!(
        callback_response.status(),
        axum::http::StatusCode::SEE_OTHER
    );
    assert_eq!(
        callback_response
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap(),
        "/dashboard"
    );

    assert_eq!(oauth_token_count(&pool, profile.id).await, 1);

    let disconnect_request = Request::builder()
        .method("DELETE")
        .uri("/oauth/instagram")
        .header("Authorization", format!("Bearer {}", created.token))
        .body(Body::empty())
        .unwrap();
    let disconnect_response = app.clone().oneshot(disconnect_request).await.unwrap();
    assert_eq!(
        disconnect_response.status(),
        axum::http::StatusCode::NO_CONTENT
    );

    assert_eq!(oauth_token_count(&pool, profile.id).await, 0);
}
