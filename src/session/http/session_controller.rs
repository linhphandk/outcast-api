use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{AppendHeaders, IntoResponse},
    routing::{delete, get, post},
};
use axum_extra::extract::CookieJar;
use serde::Serialize;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::session::repository::session_repository::{Session, SessionRepositoryTrait};
use crate::session::usecase::session_service::{
    SessionService, SessionServiceError, REFRESH_COOKIE_MAX_AGE_SECS, TOKEN_COOKIE_MAX_AGE_SECS,
};
use crate::user::crypto::jwt::create_jwt;
use crate::user::http::auth_extractor::AuthUser;
use crate::user::repository::user_repository::UserRepository;
use crate::user::usecase::user_service::UserService;

fn make_token_cookie(token: &str) -> String {
    format!(
        "token={}; HttpOnly; Path=/; SameSite=Strict; Max-Age={}",
        token, TOKEN_COOKIE_MAX_AGE_SECS
    )
}

fn make_refresh_cookie(refresh_token: &str) -> String {
    format!(
        "refresh_token={}; HttpOnly; Path=/auth/refresh; SameSite=Strict; Max-Age={}",
        refresh_token, REFRESH_COOKIE_MAX_AGE_SECS
    )
}

#[derive(Serialize)]
pub struct SessionRes {
    pub id: Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: String,
    pub expires_at: String,
}

impl From<Session> for SessionRes {
    fn from(s: Session) -> Self {
        Self {
            id: s.id,
            user_agent: s.user_agent,
            ip_address: s.ip_address,
            created_at: s.created_at.to_string(),
            expires_at: s.expires_at.to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct RefreshRes {
    pub token: String,
}

#[instrument(skip_all)]
pub async fn refresh(
    State(session_service): State<SessionService>,
    State(user_service): State<UserService<UserRepository>>,
    State(jwt_secret): State<String>,
    jar: CookieJar,
    headers: HeaderMap,
) -> impl IntoResponse {
    let refresh_token = match jar.get("refresh_token").map(|c| c.value().to_owned()) {
        Some(t) => t,
        None => {
            warn!("Refresh request missing refresh_token cookie");
            return (StatusCode::UNAUTHORIZED, "Missing refresh token").into_response();
        }
    };

    let session = match session_service
        .find_valid_session_by_refresh_token(&refresh_token)
        .await
    {
        Ok(s) => s,
        Err(SessionServiceError::SessionNotFound) => {
            warn!("Refresh token not found");
            return (StatusCode::UNAUTHORIZED, "Invalid refresh token").into_response();
        }
        Err(SessionServiceError::SessionRevoked) => {
            warn!("Refresh token is revoked");
            return (StatusCode::UNAUTHORIZED, "Session has been revoked").into_response();
        }
        Err(SessionServiceError::SessionExpired) => {
            warn!("Refresh token has expired");
            return (StatusCode::UNAUTHORIZED, "Session has expired").into_response();
        }
        Err(e) => {
            error!(error = %e, "Failed to look up session by refresh token");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    let user = match user_service.get_me(session.user_id).await {
        Ok(u) => u,
        Err(_) => {
            error!(user_id = %session.user_id, "User not found during token refresh");
            return (StatusCode::UNAUTHORIZED, "User not found").into_response();
        }
    };

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    let new_session = match session_service
        .rotate_session(session.id, session.user_id, user_agent, None)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "Failed to rotate session");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to refresh session")
                .into_response();
        }
    };

    let token = match create_jwt(user.id, &user.email, new_session.id, &jwt_secret) {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "Failed to create JWT during refresh");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate token").into_response();
        }
    };

    info!(user_id = %user.id, session_id = %new_session.id, "Session refreshed successfully");

    (
        StatusCode::OK,
        AppendHeaders([
            (header::SET_COOKIE, make_token_cookie(&token)),
            (header::SET_COOKIE, make_refresh_cookie(&new_session.refresh_token)),
        ]),
        Json(RefreshRes { token }),
    )
        .into_response()
}

#[instrument(skip_all)]
pub async fn logout(
    State(session_service): State<SessionService>,
    auth_user: AuthUser,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, session_id = %auth_user.session_id, "Logout request");

    match session_service.revoke_session(auth_user.session_id).await {
        Ok(()) => {
            info!(session_id = %auth_user.session_id, "Logged out successfully");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!(error = %e, "Failed to revoke session during logout");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to logout").into_response()
        }
    }
}

#[instrument(skip_all)]
pub async fn logout_all(
    State(session_service): State<SessionService>,
    auth_user: AuthUser,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "Logout all request");

    match session_service.revoke_all_sessions(auth_user.user_id).await {
        Ok(()) => {
            info!(user_id = %auth_user.user_id, "All sessions revoked");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!(error = %e, "Failed to revoke all sessions");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to logout all").into_response()
        }
    }
}

#[instrument(skip_all)]
pub async fn list_sessions(
    State(session_service): State<SessionService>,
    auth_user: AuthUser,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "List sessions request");

    match session_service.list_sessions(auth_user.user_id).await {
        Ok(sessions) => {
            info!(user_id = %auth_user.user_id, count = sessions.len(), "Sessions listed");
            let res: Vec<SessionRes> = sessions.into_iter().map(SessionRes::from).collect();
            Json(res).into_response()
        }
        Err(e) => {
            error!(error = %e, "Failed to list sessions");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list sessions").into_response()
        }
    }
}

#[instrument(skip(session_service, auth_user), fields(session_id = %session_id))]
pub async fn delete_session(
    State(session_service): State<SessionService>,
    auth_user: AuthUser,
    Path(session_id): Path<Uuid>,
) -> impl IntoResponse {
    info!(
        user_id = %auth_user.user_id,
        session_id = %session_id,
        "Delete session request"
    );

    match session_service
        .delete_session(auth_user.user_id, session_id)
        .await
    {
        Ok(()) => {
            info!(session_id = %session_id, "Session deleted");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(SessionServiceError::SessionNotFound) => {
            warn!(session_id = %session_id, "Session not found or not owned by user");
            (StatusCode::NOT_FOUND, "Session not found").into_response()
        }
        Err(e) => {
            error!(error = %e, "Failed to delete session");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete session").into_response()
        }
    }
}

pub fn router<S>() -> Router<S>
where
    SessionService: axum::extract::FromRef<S>,
    UserService<UserRepository>: axum::extract::FromRef<S>,
    Arc<dyn SessionRepositoryTrait>: axum::extract::FromRef<S>,
    String: axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-all", post(logout_all))
        .route("/auth/sessions", get(list_sessions))
        .route("/auth/sessions/{id}", delete(delete_session))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use axum::{Router, body::Body, routing::{get, post}};
    use axum::extract::FromRef;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::session::repository::session_repository::SessionRepository;
    use crate::user::repository::user_repository::UserRepository;
    use crate::user::usecase::user_service::UserService;
    use crate::user::http::user_controller::{CreateUserRes, create_user, login_user, get_me};

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
        user_service: UserService<UserRepository>,
        jwt_secret: String,
        session_service: SessionService,
        session_repo: Arc<dyn SessionRepositoryTrait>,
    }

    impl FromRef<TestState> for UserService<UserRepository> {
        fn from_ref(s: &TestState) -> Self {
            s.user_service.clone()
        }
    }

    impl FromRef<TestState> for String {
        fn from_ref(s: &TestState) -> Self {
            s.jwt_secret.clone()
        }
    }

    impl FromRef<TestState> for SessionService {
        fn from_ref(s: &TestState) -> Self {
            s.session_service.clone()
        }
    }

    impl FromRef<TestState> for Arc<dyn SessionRepositoryTrait> {
        fn from_ref(s: &TestState) -> Self {
            s.session_repo.clone()
        }
    }

    fn build_test_app(pool: deadpool_diesel::postgres::Pool) -> Router {
        let user_repo = UserRepository::new(pool.clone());
        let user_service = UserService::new(user_repo, "test_pepper".to_string());
        let session_repo: Arc<dyn SessionRepositoryTrait> =
            Arc::new(SessionRepository::new(pool));
        let session_service = SessionService::new(session_repo.clone());
        let state = TestState {
            user_service,
            jwt_secret: "test_jwt_secret".to_string(),
            session_service,
            session_repo,
        };
        Router::new()
            .route("/user", post(create_user))
            .route("/user/login", post(login_user))
            .route("/user/me", get(get_me))
            .merge(router())
            .with_state(state)
    }

    fn extract_cookie_value<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> Option<String> {
        headers
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| {
                let s = v.to_str().ok()?;
                let parts: Vec<&str> = s.splitn(2, '=').collect();
                if parts.len() == 2 && parts[0] == name {
                    Some(
                        parts[1]
                            .split(';')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_owned(),
                    )
                } else {
                    None
                }
            })
    }

    // -------------------------------------------------------------------------
    // E2E: signup → access protected endpoint → refresh → access → logout → 401
    // -------------------------------------------------------------------------
    #[tokio::test]
    async fn test_e2e_signup_refresh_logout() {
        let (_container, pool) = setup_test_db().await;
        let app = build_test_app(pool);

        // 1. Signup
        let signup_req = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"email": "e2e@example.com", "password": "pass123"})
                    .to_string(),
            ))
            .unwrap();
        let res = app.clone().oneshot(signup_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let token_cookie_val = extract_cookie_value(res.headers(), "token").unwrap();
        let refresh_token_val = extract_cookie_value(res.headers(), "refresh_token").unwrap();

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let created: CreateUserRes = serde_json::from_slice(&body).unwrap();
        let access_token = created.token.clone();
        assert!(!access_token.is_empty());
        assert_eq!(access_token, token_cookie_val);

        // 2. Access protected endpoint with access token → 200
        let me_req = Request::builder()
            .method("GET")
            .uri("/user/me")
            .header("Authorization", format!("Bearer {}", access_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(me_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // 3. Refresh → new access token + new refresh token cookies
        let refresh_req = Request::builder()
            .method("POST")
            .uri("/auth/refresh")
            .header("Cookie", format!("refresh_token={}", refresh_token_val))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(refresh_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let new_token_cookie_val = extract_cookie_value(res.headers(), "token").unwrap();
        let new_refresh_token_val =
            extract_cookie_value(res.headers(), "refresh_token").unwrap();

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let refresh_res: RefreshRes = serde_json::from_slice(&body).unwrap();
        let new_access_token = refresh_res.token.clone();
        assert!(!new_access_token.is_empty());
        assert_eq!(new_access_token, new_token_cookie_val);
        // Old and new refresh tokens must differ (rotation)
        assert_ne!(new_refresh_token_val, refresh_token_val);

        // 4. Access protected endpoint with new token → 200
        let me_req2 = Request::builder()
            .method("GET")
            .uri("/user/me")
            .header("Authorization", format!("Bearer {}", new_access_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(me_req2).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // 5. Logout with new token
        let logout_req = Request::builder()
            .method("POST")
            .uri("/auth/logout")
            .header("Authorization", format!("Bearer {}", new_access_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(logout_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // 6. Access protected endpoint after logout → 401
        let me_req3 = Request::builder()
            .method("GET")
            .uri("/user/me")
            .header("Authorization", format!("Bearer {}", new_access_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(me_req3).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_refresh_with_invalid_token_returns_401() {
        let (_container, pool) = setup_test_db().await;
        let app = build_test_app(pool);

        let req = Request::builder()
            .method("POST")
            .uri("/auth/refresh")
            .header("Cookie", "refresh_token=not_a_real_token")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_refresh_without_cookie_returns_401() {
        let (_container, pool) = setup_test_db().await;
        let app = build_test_app(pool);

        let req = Request::builder()
            .method("POST")
            .uri("/auth/refresh")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_list_sessions_returns_active_sessions() {
        let (_container, pool) = setup_test_db().await;
        let app = build_test_app(pool);

        // Signup
        let signup_req = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"email": "list@example.com", "password": "pass123"})
                    .to_string(),
            ))
            .unwrap();
        let res = app.clone().oneshot(signup_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let created: CreateUserRes = serde_json::from_slice(&body).unwrap();

        // GET /auth/sessions
        let req = Request::builder()
            .method("GET")
            .uri("/auth/sessions")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let sessions: Vec<SessionRes> = serde_json::from_slice(&body).unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_session_other_user_returns_404() {
        let (_container, pool) = setup_test_db().await;
        let app = build_test_app(pool);

        // Signup user A
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/user")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "a@example.com", "password": "pass123"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let user_a: CreateUserRes = serde_json::from_slice(&body).unwrap();

        // Get user A's session id
        let sessions_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/auth/sessions")
                    .header("Authorization", format!("Bearer {}", user_a.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = sessions_res.into_body().collect().await.unwrap().to_bytes();
        let sessions_a: Vec<SessionRes> = serde_json::from_slice(&body).unwrap();
        let session_a_id = sessions_a[0].id;

        // Signup user B
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/user")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "b@example.com", "password": "pass456"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let user_b: CreateUserRes = serde_json::from_slice(&body).unwrap();

        // User B tries to delete user A's session → 404
        let del_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/auth/sessions/{}", session_a_id))
                    .header("Authorization", format!("Bearer {}", user_b.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_logout_all_revokes_all_sessions() {
        let (_container, pool) = setup_test_db().await;
        let app = build_test_app(pool);

        // Signup (creates first session)
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/user")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "all@example.com", "password": "pass123"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let refresh_token1 =
            extract_cookie_value(res.headers(), "refresh_token").unwrap();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let created: CreateUserRes = serde_json::from_slice(&body).unwrap();

        // Login again (creates second session)
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/user/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"email": "all@example.com", "password": "pass123"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let login: CreateUserRes = serde_json::from_slice(&body).unwrap();

        // logout-all using the login token
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/logout-all")
                    .header("Authorization", format!("Bearer {}", login.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // Refresh using first session's refresh token should fail
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/refresh")
                    .header("Cookie", format!("refresh_token={}", refresh_token1))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Session was deleted by logout-all, so refresh token is now invalid
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
