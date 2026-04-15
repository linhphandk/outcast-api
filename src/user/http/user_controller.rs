use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{AppendHeaders, IntoResponse},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::session::repository::session_repository::SessionRepositoryTrait;
use crate::session::usecase::session_service::{
    SessionService, REFRESH_COOKIE_MAX_AGE_SECS, TOKEN_COOKIE_MAX_AGE_SECS,
};
use crate::user::crypto::jwt::create_jwt;
use crate::user::http::auth_extractor::AuthUser;
use crate::user::repository::user_repository::{RepositoryError, UserRepository};
use crate::user::usecase::user_service::{ServiceError, UserService};
use diesel::result::{DatabaseErrorKind, Error as DieselError};

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateUserReq {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateUserRes {
    pub id: Uuid,
    pub email: String,
    pub token: String,
}

#[utoipa::path(
    post,
    path = "/user",
    request_body = CreateUserReq,
    responses(
        (status = 201, description = "User created successfully", body = CreateUserRes),
        (status = 409, description = "User already exists"),
        (status = 500, description = "Internal server error")
    ),
    tag = "Users"
)]
#[instrument(skip_all)]
pub async fn create_user(
    State(service): State<UserService<UserRepository>>,
    State(jwt_secret): State<String>,
    State(session_service): State<SessionService>,
    headers: HeaderMap,
    Json(payload): Json<CreateUserReq>,
) -> impl IntoResponse {
    info!("Create user request received");
    let result = service.create(payload.email, payload.password).await;

    match result {
        Ok(user) => {
            let user_agent = headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned());

            let session = match session_service
                .create_session(user.id, user_agent, None)
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    error!(user_id = %user.id, error = %e, "Failed to create session for new user");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create session")
                        .into_response();
                }
            };

            let token = match create_jwt(user.id, &user.email, session.id, &jwt_secret) {
                Ok(t) => t,
                Err(e) => {
                    error!(user_id = %user.id, error = %e, "Failed to generate JWT for new user");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate token")
                        .into_response();
                }
            };

            info!(user_id = %user.id, "User created successfully");
            let res = CreateUserRes {
                id: user.id,
                email: user.email,
                token: token.clone(),
            };

            let token_cookie = format!(
                "token={}; HttpOnly; Path=/; SameSite=Strict; Max-Age={}",
                token, TOKEN_COOKIE_MAX_AGE_SECS
            );
            let refresh_cookie = format!(
                "refresh_token={}; HttpOnly; Path=/auth/refresh; SameSite=Strict; Max-Age={}",
                session.refresh_token, REFRESH_COOKIE_MAX_AGE_SECS
            );

            (
                StatusCode::CREATED,
                AppendHeaders([
                    (header::SET_COOKIE, token_cookie),
                    (header::SET_COOKIE, refresh_cookie),
                ]),
                Json(res),
            )
                .into_response()
        }
        Err(err) => match err {
            RepositoryError::DieselError(DieselError::DatabaseError(
                DatabaseErrorKind::UniqueViolation,
                _,
            )) => {
                warn!("User creation failed: duplicate email");
                (StatusCode::CONFLICT, "User already exists").into_response()
            }
            _ => {
                error!(error = %err, "User creation failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create user").into_response()
            }
        },
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct LoginUserReq {
    pub email: String,
    pub password: String,
}

#[utoipa::path(
    post,
    path = "/user/login",
    request_body = LoginUserReq,
    responses(
        (status = 200, description = "Login successful", body = CreateUserRes),
        (status = 401, description = "Invalid email or password"),
        (status = 500, description = "Login failed")
    ),
    tag = "Users"
)]
#[instrument(skip_all)]
pub async fn login_user(
    State(service): State<UserService<UserRepository>>,
    State(jwt_secret): State<String>,
    State(session_service): State<SessionService>,
    headers: HeaderMap,
    Json(payload): Json<LoginUserReq>,
) -> impl IntoResponse {
    info!("Login request received");
    let result = service.authenticate(payload.email, payload.password).await;

    match result {
        Ok(user) => {
            let user_agent = headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned());

            let session = match session_service
                .create_session(user.id, user_agent, None)
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    error!(user_id = %user.id, error = %e, "Failed to create session during login");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create session")
                        .into_response();
                }
            };

            let token = match create_jwt(user.id, &user.email, session.id, &jwt_secret) {
                Ok(t) => t,
                Err(e) => {
                    error!(user_id = %user.id, error = %e, "Failed to generate JWT during login");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate token")
                        .into_response();
                }
            };

            info!(user_id = %user.id, "User logged in successfully");
            let res = CreateUserRes {
                id: user.id,
                email: user.email,
                token: token.clone(),
            };

            let token_cookie = format!(
                "token={}; HttpOnly; Path=/; SameSite=Strict; Max-Age={}",
                token, TOKEN_COOKIE_MAX_AGE_SECS
            );
            let refresh_cookie = format!(
                "refresh_token={}; HttpOnly; Path=/auth/refresh; SameSite=Strict; Max-Age={}",
                session.refresh_token, REFRESH_COOKIE_MAX_AGE_SECS
            );

            (
                StatusCode::OK,
                AppendHeaders([
                    (header::SET_COOKIE, token_cookie),
                    (header::SET_COOKIE, refresh_cookie),
                ]),
                Json(res),
            )
                .into_response()
        }
        Err(err) => match err {
            ServiceError::UserNotFound | ServiceError::InvalidCredentials => {
                warn!("Login failed: invalid credentials");
                (StatusCode::UNAUTHORIZED, "Invalid email or password").into_response()
            }
            ServiceError::RepositoryError(_) | ServiceError::HashError(_) => {
                error!(error = %err, "Login failed due to internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Login failed").into_response()
            }
        },
    }
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct MeRes {
    pub id: Uuid,
    pub email: String,
}

#[utoipa::path(
    get,
    path = "/user/me",
    responses(
        (status = 200, description = "Current user info", body = MeRes),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_token" = [])),
    tag = "Users"
)]
#[instrument(skip_all)]
pub async fn get_me(
    State(service): State<UserService<UserRepository>>,
    auth_user: AuthUser,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "Get me request");
    match service.get_me(auth_user.user_id).await {
        Ok(user) => {
            info!(user_id = %user.id, "Get me successful");
            Json(MeRes {
                id: user.id,
                email: user.email,
            })
            .into_response()
        }
        Err(ServiceError::UserNotFound) => {
            warn!(user_id = %auth_user.user_id, "Get me: user not found");
            (StatusCode::NOT_FOUND, "User not found").into_response()
        }
        Err(err) => {
            error!(user_id = %auth_user.user_id, error = %err, "Get me failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get user").into_response()
        }
    }
}

pub fn router<S>() -> Router<S>
where
    UserService<UserRepository>: axum::extract::FromRef<S>,
    String: axum::extract::FromRef<S>,
    SessionService: axum::extract::FromRef<S>,
    Arc<dyn SessionRepositoryTrait>: axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/user", post(create_user))
        .route("/user/login", post(login_user))
        .route("/user/me", get(get_me))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiDoc;
    use crate::session::repository::session_repository::SessionRepository;
    use crate::user::crypto::jwt::verify_jwt;
    use axum::body::Body;
    use axum::http::Request;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use utoipa::OpenApi;
    use utoipa_scalar::{Scalar, Servable};

    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

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

    #[derive(Clone)]
    struct TestState {
        service: UserService<UserRepository>,
        jwt_secret: String,
        session_service: SessionService,
        session_repo: Arc<dyn SessionRepositoryTrait>,
    }

    impl axum::extract::FromRef<TestState> for UserService<UserRepository> {
        fn from_ref(state: &TestState) -> Self {
            state.service.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for String {
        fn from_ref(state: &TestState) -> Self {
            state.jwt_secret.clone()
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

    fn build_app(pool: deadpool_diesel::postgres::Pool) -> Router {
        let repo = UserRepository::new(pool.clone());
        let service = UserService::new(repo, "test_pepper".to_string());
        let session_repo: Arc<dyn SessionRepositoryTrait> =
            Arc::new(SessionRepository::new(pool));
        let session_service = SessionService::new(session_repo.clone());
        let state = TestState {
            service,
            jwt_secret: "test_jwt_secret".to_string(),
            session_service,
            session_repo,
        };
        Router::new()
            .route("/user", post(create_user))
            .route("/user/login", post(login_user))
            .route("/user/me", get(get_me))
            .merge(Scalar::with_url("/scalar", ApiDoc::openapi()))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_create_user_success() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "test@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(response.headers().contains_key(header::SET_COOKIE));

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: CreateUserRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(res.email, "test@example.com");
        assert!(!res.id.is_nil());
        assert!(!res.token.is_empty());

        // Verify the token manually
        let claims = verify_jwt(&res.token, "test_jwt_secret").expect("Failed to verify token");
        assert_eq!(claims.email, "test@example.com");
    }

    #[tokio::test]
    async fn test_create_duplicate_user_fails() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());

        let body = serde_json::json!({
            "email": "duplicate@example.com",
            "password": "password123"
        })
        .to_string();

        // First request should succeed
        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // Second request with same email should fail
        let app = build_app(pool);
        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_create_user_invalid_json() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from("{invalid json}"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_login_user_success() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());

        // First create the user
        let create_request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "login@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let create_response = app.oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);

        // Now login
        let app = build_app(pool);
        let login_request = Request::builder()
            .method("POST")
            .uri("/user/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "login@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(login_request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key(header::SET_COOKIE));

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: CreateUserRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(res.email, "login@example.com");
        assert!(!res.id.is_nil());
        assert!(!res.token.is_empty());

        let claims = verify_jwt(&res.token, "test_jwt_secret").expect("Failed to verify token");
        assert_eq!(claims.email, "login@example.com");
    }

    #[tokio::test]
    async fn test_login_user_not_found() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/user/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "nonexistent@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_login_user_wrong_password() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());

        // First create the user
        let create_request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "wrongpw@example.com",
                    "password": "correct_password"
                })
                .to_string(),
            ))
            .unwrap();

        let create_response = app.oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);

        // Now login with wrong password
        let app = build_app(pool);
        let login_request = Request::builder()
            .method("POST")
            .uri("/user/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "wrongpw@example.com",
                    "password": "wrong_password"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(login_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_login_user_invalid_json() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/user/login")
            .header("content-type", "application/json")
            .body(Body::from("{invalid json}"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_scalar_ui_accessible() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("GET")
            .uri("/scalar")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_me_success() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());

        // Create a user first
        let create_request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "me@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let create_response = app.oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let body = create_response.into_body().collect().await.unwrap().to_bytes();
        let created: CreateUserRes = serde_json::from_slice(&body).unwrap();

        // Call /user/me with the token
        let app = build_app(pool);
        let me_request = Request::builder()
            .method("GET")
            .uri("/user/me")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(me_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let me_res: MeRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(me_res.id, created.id);
        assert_eq!(me_res.email, "me@example.com");
    }

    #[tokio::test]
    async fn test_get_me_missing_token() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("GET")
            .uri("/user/me")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_get_me_invalid_token() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("GET")
            .uri("/user/me")
            .header("Authorization", "Bearer this.is.not.a.valid.token")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
