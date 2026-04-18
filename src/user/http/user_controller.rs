use axum::{
    Json, Router,
    extract::{Multipart, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::session::http::cookies::set_auth_cookies;
use crate::session::usecase::session_service::SessionService;
use crate::user::http::auth_extractor::AuthUser;
use crate::user::repository::user_repository::{RepositoryError, UserRepository};
use crate::user::storage::StoragePort;
use crate::user::usecase::user_service::{ServiceError, UserService};
use diesel::result::{DatabaseErrorKind, Error as DieselError};

const MAX_AVATAR_BYTES: usize = 5 * 1024 * 1024;
const ALLOWED_AVATAR_CONTENT_TYPES: [&str; 3] = ["image/jpeg", "image/png", "image/webp"];

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
    jar: CookieJar,
    State(service): State<UserService<UserRepository>>,
    State(jwt_secret): State<String>,
    State(session_service): State<SessionService>,
    Json(payload): Json<CreateUserReq>,
) -> impl IntoResponse {
    info!("Create user request received");
    let result = service.create(payload.email, payload.password).await;

    match result {
        Ok(user) => {
            match session_service
                .create_session(user.id, &user.email, None, None, &jwt_secret)
                .await
            {
                Ok(tokens) => {
                    info!(user_id = %user.id, "User created successfully");
                    let res = CreateUserRes {
                        id: user.id,
                        email: user.email,
                        token: tokens.access_token.clone(),
                    };
                    let jar = set_auth_cookies(jar, tokens.access_token, tokens.refresh_token);
                    (StatusCode::CREATED, jar, Json(res)).into_response()
                }
                Err(_) => {
                    error!(user_id = %user.id, "Failed to create session for new user");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to generate token",
                    )
                        .into_response()
                }
            }
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
    jar: CookieJar,
    State(service): State<UserService<UserRepository>>,
    State(jwt_secret): State<String>,
    State(session_service): State<SessionService>,
    Json(payload): Json<LoginUserReq>,
) -> impl IntoResponse {
    info!("Login request received");
    let result = service.authenticate(payload.email, payload.password).await;

    match result {
        Ok(user) => {
            match session_service
                .create_session(user.id, &user.email, None, None, &jwt_secret)
                .await
            {
                Ok(tokens) => {
                    info!(user_id = %user.id, "User logged in successfully");
                    let res = CreateUserRes {
                        id: user.id,
                        email: user.email,
                        token: tokens.access_token.clone(),
                    };
                    let jar = set_auth_cookies(jar, tokens.access_token, tokens.refresh_token);
                    (StatusCode::OK, jar, Json(res)).into_response()
                }
                Err(_) => {
                    error!(user_id = %user.id, "Failed to create session during login");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to generate token",
                    )
                        .into_response()
                }
            }
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

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct UploadAvatarRes {
    pub avatar_url: String,
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
    auth_user: AuthUser,
    State(service): State<UserService<UserRepository>>,
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

#[utoipa::path(
    post,
    path = "/user/profile/image",
    responses(
        (status = 200, description = "Avatar uploaded successfully", body = UploadAvatarRes),
        (status = 400, description = "Invalid file type or size"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_token" = [])),
    tag = "Users"
)]
#[instrument(skip_all)]
pub async fn upload_profile_image(
    auth_user: AuthUser,
    State(service): State<UserService<UserRepository>>,
    State(storage): State<Arc<dyn StoragePort>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let Some(field) = multipart.next_field().await.unwrap_or(None) else {
        return (StatusCode::BAD_REQUEST, "Missing image field").into_response();
    };

    if field.name() != Some("image") {
        return (StatusCode::BAD_REQUEST, "Invalid multipart field").into_response();
    }

    let content_type = match field.content_type() {
        Some(content_type) => content_type.to_string(),
        None => return (StatusCode::BAD_REQUEST, "Missing content type").into_response(),
    };

    if !ALLOWED_AVATAR_CONTENT_TYPES.contains(&content_type.as_str()) {
        return (StatusCode::BAD_REQUEST, "Unsupported image type").into_response();
    }

    let data = match field.bytes().await {
        Ok(data) => data,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid multipart payload").into_response(),
    };

    if data.len() > MAX_AVATAR_BYTES {
        return (StatusCode::BAD_REQUEST, "Image size exceeds 5MB").into_response();
    }

    if multipart.next_field().await.unwrap_or(None).is_some() {
        return (StatusCode::BAD_REQUEST, "Only one image file is allowed").into_response();
    }

    match service
        .upload_avatar(
            auth_user.user_id,
            Bytes::from(data),
            &content_type,
            storage,
        )
        .await
    {
        Ok(avatar_url) => Json(UploadAvatarRes { avatar_url }).into_response(),
        Err(ServiceError::StorageError(err)) => {
            error!(user_id = %auth_user.user_id, error = %err, "Avatar upload storage failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to upload avatar").into_response()
        }
        Err(err) => {
            error!(user_id = %auth_user.user_id, error = %err, "Avatar upload failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to upload avatar").into_response()
        }
    }
}

pub fn router<S>() -> Router<S>
where
    UserService<UserRepository>: axum::extract::FromRef<S>,
    String: axum::extract::FromRef<S>,
    SessionService: axum::extract::FromRef<S>,
    Arc<dyn StoragePort>: axum::extract::FromRef<S>,
    std::sync::Arc<dyn crate::session::repository::session_repository::SessionRepositoryTrait>:
        axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/user", post(create_user))
        .route("/user/login", post(login_user))
        .route("/user/me", get(get_me))
        .route("/user/profile/image", post(upload_profile_image))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::ApiDoc;
    use crate::session::repository::session_repository::{
        MockSessionRepositoryTrait, SessionRepository, SessionRepositoryTrait,
    };
    use crate::session::usecase::session_service::SessionService;
    use crate::user::crypto::jwt::verify_jwt;
    use crate::user::repository::user_repository::{UserRepository, UserRepositoryTrait};
    use crate::user::storage::{MockStoragePort, StorageError, StoragePort};
    use axum::body::Body;
    use axum::http::{Request, header};
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use http_body_util::BodyExt;
    use mockall::predicate::{always, eq};
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
        session_service: SessionService,
        session_repo: Arc<dyn SessionRepositoryTrait>,
        storage: Arc<dyn StoragePort>,
        jwt_secret: String,
    }

    impl axum::extract::FromRef<TestState> for UserService<UserRepository> {
        fn from_ref(state: &TestState) -> Self {
            state.service.clone()
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

    impl axum::extract::FromRef<TestState> for Arc<dyn StoragePort> {
        fn from_ref(state: &TestState) -> Self {
            state.storage.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for String {
        fn from_ref(state: &TestState) -> Self {
            state.jwt_secret.clone()
        }
    }

    fn build_app(pool: deadpool_diesel::postgres::Pool) -> Router {
        let repo = UserRepository::new(pool.clone());
        let service = UserService::new(repo, "test_pepper".to_string());
        let session_repo: Arc<dyn SessionRepositoryTrait> =
            Arc::new(SessionRepository::new(pool.clone()));
        let session_user_repo: Arc<dyn UserRepositoryTrait> =
            Arc::new(UserRepository::new(pool));
        let session_service = SessionService::new(session_repo.clone(), session_user_repo);
        let storage: Arc<dyn StoragePort> = Arc::new(MockStoragePort::new());
        let state = TestState {
            service,
            session_service,
            session_repo,
            storage,
            jwt_secret: "test_jwt_secret".to_string(),
        };
        Router::new()
            .route("/user", post(create_user))
            .route("/user/login", post(login_user))
            .route("/user/me", get(get_me))
            .route("/user/profile/image", post(upload_profile_image))
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

    #[tokio::test]
    async fn test_login_then_get_me_succeeds() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());

        // Create a user
        let create_request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "integration@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);

        // Login
        let app = build_app(pool.clone());
        let login_request = Request::builder()
            .method("POST")
            .uri("/user/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "integration@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();
        let login_response = app.oneshot(login_request).await.unwrap();
        assert_eq!(login_response.status(), StatusCode::OK);

        // Verify both cookies are set
        let set_cookie_headers: Vec<_> = login_response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert!(
            set_cookie_headers.iter().any(|h| h.starts_with("token=")),
            "expected token cookie, got: {:?}",
            set_cookie_headers
        );
        assert!(
            set_cookie_headers.iter().any(|h| h.starts_with("refresh_token=")),
            "expected refresh_token cookie, got: {:?}",
            set_cookie_headers
        );

        let body = login_response.into_body().collect().await.unwrap().to_bytes();
        let login_res: CreateUserRes = serde_json::from_slice(&body).unwrap();
        assert!(!login_res.token.is_empty());

        // Use the token to call GET /user/me
        let app = build_app(pool);
        let me_request = Request::builder()
            .method("GET")
            .uri("/user/me")
            .header("Authorization", format!("Bearer {}", login_res.token))
            .body(Body::empty())
            .unwrap();
        let me_response = app.oneshot(me_request).await.unwrap();
        assert_eq!(me_response.status(), StatusCode::OK);

        let body = me_response.into_body().collect().await.unwrap().to_bytes();
        let me_res: MeRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(me_res.email, "integration@example.com");
    }

    /// Builds an app whose SessionRepository always fails on `create`, to test
    /// the "Failed to generate token" error branch in create_user / login_user.
    fn build_app_with_failing_session(pool: deadpool_diesel::postgres::Pool) -> Router {
        let repo = UserRepository::new(pool.clone());
        let service = UserService::new(repo, "test_pepper".to_string());

        let mut mock_session_repo = MockSessionRepositoryTrait::new();
        mock_session_repo.expect_create().returning(|_, _, _, _, _| {
            Err(
                crate::session::repository::session_repository::SessionRepositoryError::DieselError(
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::Unknown,
                        Box::new("forced failure".to_string()),
                    ),
                ),
            )
        });
        // find_by_id needed for AuthUser extractor if used, but not for these tests
        mock_session_repo.expect_find_by_id().returning(|_| Ok(None));

        let failing_session_repo: Arc<dyn SessionRepositoryTrait> = Arc::new(mock_session_repo);
        let session_user_repo: Arc<dyn UserRepositoryTrait> =
            Arc::new(UserRepository::new(pool));
        let session_service =
            SessionService::new(failing_session_repo.clone(), session_user_repo);
        let storage: Arc<dyn StoragePort> = Arc::new(MockStoragePort::new());

        let state = TestState {
            service,
            session_service,
            session_repo: failing_session_repo,
            storage,
            jwt_secret: "test_jwt_secret".to_string(),
        };

        Router::new()
            .route("/user", post(create_user))
            .route("/user/login", post(login_user))
            .route("/user/me", get(get_me))
            .route("/user/profile/image", post(upload_profile_image))
            .with_state(state)
    }

    fn build_multipart_body(
        boundary: &str,
        field_name: &str,
        filename: &str,
        content_type: &str,
        data: &[u8],
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{field_name}\"; filename=\"{filename}\"\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(data);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        body
    }

    fn build_app_with_storage(
        pool: deadpool_diesel::postgres::Pool,
        storage: Arc<dyn StoragePort>,
    ) -> Router {
        let repo = UserRepository::new(pool.clone());
        let service = UserService::new(repo, "test_pepper".to_string());
        let session_repo: Arc<dyn SessionRepositoryTrait> =
            Arc::new(SessionRepository::new(pool.clone()));
        let session_user_repo: Arc<dyn UserRepositoryTrait> =
            Arc::new(UserRepository::new(pool));
        let session_service = SessionService::new(session_repo.clone(), session_user_repo);
        let state = TestState {
            service,
            session_service,
            session_repo,
            storage,
            jwt_secret: "test_jwt_secret".to_string(),
        };

        Router::new()
            .route("/user", post(create_user))
            .route("/user/login", post(login_user))
            .route("/user/me", get(get_me))
            .route("/user/profile/image", post(upload_profile_image))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_upload_profile_image_success() {
        let (_container, pool) = setup_test_db().await;

        let mut mock_storage = MockStoragePort::new();
        mock_storage
            .expect_upload()
            .with(always(), always(), eq("image/png"))
            .times(1)
            .returning(|key, _, _| Ok(format!("s3://test-bucket/{key}")));
        let app = build_app_with_storage(pool.clone(), Arc::new(mock_storage));

        let create_request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "avatar_ok@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.clone().oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body = create_response.into_body().collect().await.unwrap().to_bytes();
        let created: CreateUserRes = serde_json::from_slice(&create_body).unwrap();

        let boundary = "boundary123";
        let body = build_multipart_body(boundary, "image", "avatar.png", "image/png", b"png-data");
        let request = Request::builder()
            .method("POST")
            .uri("/user/profile/image")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let res_body = response.into_body().collect().await.unwrap().to_bytes();
        let res: UploadAvatarRes = serde_json::from_slice(&res_body).unwrap();
        assert!(res.avatar_url.starts_with("s3://test-bucket/avatars/"));
    }

    #[tokio::test]
    async fn test_upload_profile_image_rejects_unsupported_type() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let create_request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "avatar_type@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.clone().oneshot(create_request).await.unwrap();
        let create_body = create_response.into_body().collect().await.unwrap().to_bytes();
        let created: CreateUserRes = serde_json::from_slice(&create_body).unwrap();

        let boundary = "boundary456";
        let body = build_multipart_body(
            boundary,
            "image",
            "avatar.gif",
            "image/gif",
            b"gif-data",
        );
        let request = Request::builder()
            .method("POST")
            .uri("/user/profile/image")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_upload_profile_image_rejects_oversized_file() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let create_request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "avatar_big@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.clone().oneshot(create_request).await.unwrap();
        let create_body = create_response.into_body().collect().await.unwrap().to_bytes();
        let created: CreateUserRes = serde_json::from_slice(&create_body).unwrap();

        let oversized = vec![b'a'; MAX_AVATAR_BYTES + 1];
        let boundary = "boundary789";
        let body = build_multipart_body(boundary, "image", "avatar.png", "image/png", &oversized);
        let request = Request::builder()
            .method("POST")
            .uri("/user/profile/image")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_upload_profile_image_requires_auth() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let boundary = "boundary000";
        let body = build_multipart_body(boundary, "image", "avatar.png", "image/png", b"png-data");
        let request = Request::builder()
            .method("POST")
            .uri("/user/profile/image")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_upload_profile_image_storage_error_returns_500() {
        let (_container, pool) = setup_test_db().await;

        let mut mock_storage = MockStoragePort::new();
        mock_storage
            .expect_upload()
            .times(1)
            .returning(|_, _, _| Err(StorageError::UploadFailed("boom".to_string())));
        let app = build_app_with_storage(pool.clone(), Arc::new(mock_storage));

        let create_request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "avatar_storage_err@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.clone().oneshot(create_request).await.unwrap();
        let create_body = create_response.into_body().collect().await.unwrap().to_bytes();
        let created: CreateUserRes = serde_json::from_slice(&create_body).unwrap();

        let boundary = "boundary500";
        let body = build_multipart_body(boundary, "image", "avatar.png", "image/png", b"png-data");
        let request = Request::builder()
            .method("POST")
            .uri("/user/profile/image")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_create_user_session_creation_fails_returns_500() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app_with_failing_session(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "session_fail@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("Failed to generate token"));
    }

    #[tokio::test]
    async fn test_login_user_session_creation_fails_returns_500() {
        let (_container, pool) = setup_test_db().await;

        // Create the user first using the normal app.
        let normal_app = build_app(pool.clone());
        let create_request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "login_session_fail@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = normal_app.oneshot(create_request).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Now login using the app with failing session service.
        let failing_app = build_app_with_failing_session(pool);
        let login_request = Request::builder()
            .method("POST")
            .uri("/user/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "login_session_fail@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let response = failing_app.oneshot(login_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("Failed to generate token"));
    }

    #[tokio::test]
    async fn test_login_user_hash_error_returns_500() {
        let (_container, pool) = setup_test_db().await;

        // Insert a user with a corrupt password hash directly in the DB.
        {
            use crate::schema::users;
            use diesel::prelude::*;
            let conn = pool.get().await.unwrap();
            conn.interact(move |conn| {
                diesel::insert_into(users::table)
                    .values((
                        users::id.eq(uuid::Uuid::new_v4()),
                        users::email.eq("corrupt_hash@example.com"),
                        users::password.eq("not_a_valid_bcrypt_hash"),
                    ))
                    .execute(conn)
            })
            .await
            .unwrap()
            .unwrap();
        }

        let app = build_app(pool);
        let login_request = Request::builder()
            .method("POST")
            .uri("/user/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "corrupt_hash@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(login_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("Login failed"));
    }
}
