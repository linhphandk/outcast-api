use axum::{Json, Router, extract::{Path, State}, http::StatusCode, response::IntoResponse, routing::{delete, get, post}};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::session::http::cookies::{REFRESH_TOKEN_COOKIE_NAME, clear_auth_cookies, set_auth_cookies};
use crate::session::repository::session_repository::Session;
use crate::session::usecase::session_service::{SessionService, SessionServiceError};
use crate::user::http::auth_extractor::AuthUser;

#[derive(Serialize, Deserialize)]
pub struct RefreshResponse {
    pub access_token: String,
}

#[instrument(skip_all)]
pub async fn refresh_session(
    jar: CookieJar,
    State(session_service): State<SessionService>,
    State(jwt_secret): State<String>,
) -> impl IntoResponse {
    let old_token = match jar.get(REFRESH_TOKEN_COOKIE_NAME) {
        Some(c) => c.value().to_owned(),
        None => {
            warn!("Refresh attempted without refresh_token cookie");
            return (StatusCode::UNAUTHORIZED, "Invalid or expired refresh token").into_response();
        }
    };

    info!("Token refresh requested");
    match session_service.refresh(&old_token, &jwt_secret).await {
        Ok(tokens) => {
            let jar = set_auth_cookies(jar, tokens.access_token.clone(), tokens.refresh_token);
            (jar, Json(RefreshResponse { access_token: tokens.access_token })).into_response()
        }
        Err(
            SessionServiceError::NotFound
            | SessionServiceError::Revoked
            | SessionServiceError::Expired,
        ) => {
            warn!("Refresh rejected: invalid, reused, or expired token");
            (StatusCode::UNAUTHORIZED, "Invalid or expired refresh token").into_response()
        }
        Err(e) => {
            error!(error = %e, "Token refresh failed due to internal error");
            (StatusCode::INTERNAL_SERVER_ERROR, "Token refresh failed").into_response()
        }
    }
}


#[derive(Serialize)]
pub struct SessionResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: String,
    pub expires_at: String,
}

impl From<Session> for SessionResponse {
    fn from(s: Session) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            user_agent: s.user_agent,
            ip_address: s.ip_address,
            created_at: s.created_at.to_string(),
            expires_at: s.expires_at.to_string(),
        }
    }
}

#[instrument(skip_all)]
pub async fn logout(
    auth_user: AuthUser,
    jar: CookieJar,
    State(session_service): State<SessionService>,
) -> impl IntoResponse {
    info!(session_id = %auth_user.session_id, user_id = %auth_user.user_id, "Logout requested");
    match session_service.logout(auth_user.session_id).await {
        Ok(()) => {
            let jar = clear_auth_cookies(jar);
            (jar, StatusCode::NO_CONTENT).into_response()
        }
        Err(e) => {
            error!(error = %e, "Logout failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[instrument(skip_all)]
pub async fn logout_all(
    auth_user: AuthUser,
    jar: CookieJar,
    State(session_service): State<SessionService>,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "Logout-all requested");
    match session_service.logout_all(auth_user.user_id).await {
        Ok(()) => {
            let jar = clear_auth_cookies(jar);
            (jar, StatusCode::NO_CONTENT).into_response()
        }
        Err(e) => {
            error!(error = %e, "Logout-all failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[instrument(skip_all)]
pub async fn list_sessions(
    auth_user: AuthUser,
    State(session_service): State<SessionService>,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "List sessions requested");
    match session_service.list_sessions(auth_user.user_id).await {
        Ok(sessions) => {
            let res: Vec<SessionResponse> = sessions.into_iter().map(SessionResponse::from).collect();
            Json(res).into_response()
        }
        Err(e) => {
            error!(error = %e, "List sessions failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[instrument(skip_all)]
pub async fn delete_session(
    auth_user: AuthUser,
    Path(id): Path<Uuid>,
    State(session_service): State<SessionService>,
) -> impl IntoResponse {
    info!(session_id = %id, user_id = %auth_user.user_id, "Delete session requested");
    match session_service.delete_session(id, auth_user.user_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(SessionServiceError::NotFound) => {
            warn!(session_id = %id, user_id = %auth_user.user_id, "Session not found or not owned by user");
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => {
            error!(error = %e, "Delete session failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub fn router<S>() -> Router<S>
where
    SessionService: axum::extract::FromRef<S>,
    String: axum::extract::FromRef<S>,
    std::sync::Arc<dyn crate::session::repository::session_repository::SessionRepositoryTrait>:
        axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/refresh", post(refresh_session))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-all", post(logout_all))
        .route("/auth/sessions", get(list_sessions))
        .route("/auth/sessions/{id}", delete(delete_session))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::{Router, body::Body, http::Request, routing::{delete, get, post}};
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::session::repository::session_repository::{SessionRepository, SessionRepositoryTrait};
    use crate::user::repository::user_repository::{UserRepository, UserRepositoryTrait};
    use crate::session::usecase::session_service::SessionService;

    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

    const JWT_SECRET: &str = "test-jwt-secret";

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
        session_service: SessionService,
        session_repo: Arc<dyn SessionRepositoryTrait>,
        jwt_secret: String,
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

    fn build_app(pool: deadpool_diesel::postgres::Pool) -> Router {
        let session_repo: Arc<dyn SessionRepositoryTrait> =
            Arc::new(SessionRepository::new(pool.clone()));
        let user_repo: Arc<dyn UserRepositoryTrait> = Arc::new(UserRepository::new(pool));
        let session_service = SessionService::new(session_repo.clone(), user_repo);
        let state = TestState {
            session_service,
            session_repo,
            jwt_secret: JWT_SECRET.to_string(),
        };
        Router::new()
            .route("/auth/refresh", post(refresh_session))
            .route("/auth/logout", post(logout))
            .route("/auth/logout-all", post(logout_all))
            .route("/auth/sessions", get(list_sessions))
            .route("/auth/sessions/{id}", delete(delete_session))
            .with_state(state)
    }

    /// Creates a user and a session in the DB, returning the raw refresh token.
    async fn setup_user_and_session(pool: &deadpool_diesel::postgres::Pool) -> String {
        use crate::schema::{sessions, users};
        use diesel::prelude::*;
        use uuid::Uuid;

        let user_id = Uuid::new_v4();
        let email = format!("refresh_test_{}@example.com", user_id);
        let conn = pool.get().await.unwrap();

        conn.interact(move |conn| {
            diesel::insert_into(users::table)
                .values((
                    users::id.eq(user_id),
                    users::email.eq(email),
                    users::password.eq("hashed"),
                ))
                .execute(conn)
                .unwrap();

            let refresh_token = "test_refresh_token_abcdef1234567890".to_string();
            let expires_at = chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
            diesel::insert_into(sessions::table)
                .values((
                    sessions::id.eq(Uuid::new_v4()),
                    sessions::user_id.eq(user_id),
                    sessions::refresh_token.eq(&refresh_token),
                    sessions::expires_at.eq(expires_at),
                ))
                .execute(conn)
                .unwrap();

            refresh_token
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_refresh_missing_cookie_returns_401() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/auth/refresh")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_refresh_invalid_token_returns_401() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/auth/refresh")
            .header("Cookie", "refresh_token=invalid_or_unknown_token")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_refresh_success_returns_new_tokens() {
        let (_container, pool) = setup_test_db().await;
        let refresh_token = setup_user_and_session(&pool).await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/auth/refresh")
            .header("Cookie", format!("refresh_token={}", refresh_token))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Both Set-Cookie headers must be present (token + refresh_token).
        let set_cookie_headers: Vec<_> = response
            .headers()
            .get_all("Set-Cookie")
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

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: RefreshResponse = serde_json::from_slice(&body).unwrap();
        assert!(!res.access_token.is_empty());

        let claims = crate::user::crypto::jwt::verify_jwt(&res.access_token, JWT_SECRET).unwrap();
        assert!(!claims.sub.is_nil());
    }

    #[tokio::test]
    async fn test_refresh_reuse_returns_401() {
        let (_container, pool) = setup_test_db().await;
        let refresh_token = setup_user_and_session(&pool).await;

        // First refresh succeeds — token is rotated.
        let app = build_app(pool.clone());
        let request = Request::builder()
            .method("POST")
            .uri("/auth/refresh")
            .header("Cookie", format!("refresh_token={}", refresh_token))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Second refresh with the same (now-revoked) token must be rejected.
        let app = build_app(pool);
        let request = Request::builder()
            .method("POST")
            .uri("/auth/refresh")
            .header("Cookie", format!("refresh_token={}", refresh_token))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // -------------------------------------------------------------------------
    // Helpers shared by endpoint tests
    // -------------------------------------------------------------------------

    /// Creates a user + session directly in the DB and returns
    /// `(user_id, session_id, access_token, refresh_token)`.
    async fn login(
        pool: &deadpool_diesel::postgres::Pool,
    ) -> (Uuid, Uuid, String, String) {
        use crate::schema::{sessions, users};
        use diesel::prelude::*;

        let user_id = Uuid::new_v4();
        let email = format!("e2e_{}@example.com", user_id);
        let conn = pool.get().await.unwrap();
        let email_clone = email.clone();

        let session_id = conn
            .interact(move |conn| {
                diesel::insert_into(users::table)
                    .values((
                        users::id.eq(user_id),
                        users::email.eq(&email_clone),
                        users::password.eq("hashed"),
                    ))
                    .execute(conn)
                    .unwrap();

                let session_id = Uuid::new_v4();
                let refresh_token = hex::encode(vec![0u8; 64]);
                let expires_at =
                    chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
                diesel::insert_into(sessions::table)
                    .values((
                        sessions::id.eq(session_id),
                        sessions::user_id.eq(user_id),
                        sessions::refresh_token.eq(&refresh_token),
                        sessions::expires_at.eq(expires_at),
                    ))
                    .execute(conn)
                    .unwrap();

                (session_id, refresh_token)
            })
            .await
            .unwrap();

        let (session_id, refresh_token) = session_id;
        let access_token =
            crate::user::crypto::jwt::create_jwt(user_id, &email, session_id, JWT_SECRET)
                .unwrap();
        (user_id, session_id, access_token, refresh_token)
    }

    // -------------------------------------------------------------------------
    // GET /auth/sessions
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_list_sessions_returns_caller_sessions() {
        let (_container, pool) = setup_test_db().await;
        let (_user_id, _session_id, access_token, _refresh_token) = login(&pool).await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("GET")
            .uri("/auth/sessions")
            .header("Authorization", format!("Bearer {}", access_token))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let sessions: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(!sessions.is_empty(), "expected at least one session");
    }

    #[tokio::test]
    async fn test_list_sessions_unauthenticated_returns_401() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("GET")
            .uri("/auth/sessions")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // -------------------------------------------------------------------------
    // DELETE /auth/sessions/:id
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_delete_session_own_session_returns_204() {
        let (_container, pool) = setup_test_db().await;
        let (_user_id, session_id, access_token, _refresh_token) = login(&pool).await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/auth/sessions/{}", session_id))
            .header("Authorization", format!("Bearer {}", access_token))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_delete_session_other_user_returns_404() {
        let (_container, pool) = setup_test_db().await;
        // Create two independent users/sessions.
        let (_uid1, _sid1, token1, _) = login(&pool).await;
        let (_uid2, sid2, _token2, _) = login(&pool).await;

        let app = build_app(pool);

        // user1 tries to delete user2's session → 404
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/auth/sessions/{}", sid2))
            .header("Authorization", format!("Bearer {}", token1))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // -------------------------------------------------------------------------
    // POST /auth/logout
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_logout_revokes_current_session_and_clears_cookies() {
        let (_container, pool) = setup_test_db().await;
        let (_user_id, _session_id, access_token, _refresh_token) = login(&pool).await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/auth/logout")
            .header("Authorization", format!("Bearer {}", access_token))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Both cookies must be cleared (Set-Cookie with empty value / Max-Age=0).
        let set_cookie_headers: Vec<_> = response
            .headers()
            .get_all("Set-Cookie")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert!(
            set_cookie_headers.iter().any(|h| h.starts_with("token=")),
            "expected token clear cookie, got: {:?}",
            set_cookie_headers
        );
        assert!(
            set_cookie_headers.iter().any(|h| h.starts_with("refresh_token=")),
            "expected refresh_token clear cookie, got: {:?}",
            set_cookie_headers
        );
    }

    // -------------------------------------------------------------------------
    // POST /auth/logout-all
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_logout_all_removes_all_sessions_and_clears_cookies() {
        let (_container, pool) = setup_test_db().await;
        let (user_id, _sid, access_token, _) = login(&pool).await;

        // Create a second session for the same user directly.
        {
            use crate::schema::sessions;
            use diesel::prelude::*;
            let conn = pool.get().await.unwrap();
            conn.interact(move |conn| {
                let expires_at =
                    chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
                diesel::insert_into(sessions::table)
                    .values((
                        sessions::id.eq(Uuid::new_v4()),
                        sessions::user_id.eq(user_id),
                        sessions::refresh_token.eq("second_session_token"),
                        sessions::expires_at.eq(expires_at),
                    ))
                    .execute(conn)
                    .unwrap();
            })
            .await
            .unwrap();
        }

        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/auth/logout-all")
            .header("Authorization", format!("Bearer {}", access_token))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let set_cookie_headers: Vec<_> = response
            .headers()
            .get_all("Set-Cookie")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert!(
            set_cookie_headers.iter().any(|h| h.starts_with("token=")),
            "expected token clear cookie"
        );
        assert!(
            set_cookie_headers.iter().any(|h| h.starts_with("refresh_token=")),
            "expected refresh_token clear cookie"
        );
    }

    // -------------------------------------------------------------------------
    // End-to-end: login → protected → refresh → protected → logout → 401
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_e2e_login_refresh_logout() {
        use crate::user::http::user_controller::{LoginUserReq, MeRes};
        use crate::user::usecase::user_service::UserService;
        use crate::user::repository::user_repository::UserRepository as UserRepo;

        let (_container, pool) = setup_test_db().await;

        // Build a full app that includes the user routes as well.
        let session_repo: Arc<dyn SessionRepositoryTrait> =
            Arc::new(SessionRepository::new(pool.clone()));
        let user_repo_arc: Arc<dyn UserRepositoryTrait> =
            Arc::new(UserRepository::new(pool.clone()));
        let session_service = SessionService::new(session_repo.clone(), user_repo_arc);
        let user_repo = UserRepo::new(pool.clone());
        let user_service = UserService::new(user_repo, "pepper".to_string());

        #[derive(Clone)]
        struct FullState {
            session_service: SessionService,
            session_repo: Arc<dyn SessionRepositoryTrait>,
            jwt_secret: String,
            user_service: UserService<UserRepo>,
        }

        impl axum::extract::FromRef<FullState> for SessionService {
            fn from_ref(s: &FullState) -> Self {
                s.session_service.clone()
            }
        }
        impl axum::extract::FromRef<FullState> for Arc<dyn SessionRepositoryTrait> {
            fn from_ref(s: &FullState) -> Self {
                s.session_repo.clone()
            }
        }
        impl axum::extract::FromRef<FullState> for String {
            fn from_ref(s: &FullState) -> Self {
                s.jwt_secret.clone()
            }
        }
        impl axum::extract::FromRef<FullState>
            for UserService<UserRepo>
        {
            fn from_ref(s: &FullState) -> Self {
                s.user_service.clone()
            }
        }

        let state = FullState {
            session_service,
            session_repo,
            jwt_secret: JWT_SECRET.to_string(),
            user_service,
        };

        let app = Router::new()
            .route("/auth/refresh", post(refresh_session))
            .route("/auth/logout", post(logout))
            .route("/auth/logout-all", post(logout_all))
            .route("/auth/sessions", get(list_sessions))
            .route("/auth/sessions/{id}", delete(delete_session))
            .merge(crate::user::http::user_controller::router())
            .with_state(state);

        // ── Step 1: Sign up (creates user + session) ──────────────────────────
        let signup_body = serde_json::to_vec(&serde_json::json!({
            "email": "e2e@example.com",
            "password": "password123"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/user")
            .header("Content-Type", "application/json")
            .body(Body::from(signup_body))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "signup failed");

        // Extract access token and refresh token cookie from signup response.
        let cookies: Vec<String> = resp
            .headers()
            .get_all("Set-Cookie")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        let access_token = {
            let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
            let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            body["token"].as_str().unwrap().to_owned()
        };
        let refresh_cookie = cookies
            .iter()
            .find(|c| c.starts_with("refresh_token="))
            .cloned()
            .expect("refresh_token cookie missing after signup");
        let refresh_token = refresh_cookie
            .split(';')
            .next()
            .unwrap()
            .trim_start_matches("refresh_token=")
            .to_owned();

        // ── Step 2: Access protected endpoint with the access token ───────────
        let req = Request::builder()
            .method("GET")
            .uri("/user/me")
            .header("Authorization", format!("Bearer {}", access_token))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "GET /user/me should succeed");
        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let me: MeRes = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(me.email, "e2e@example.com");

        // ── Step 3: Refresh tokens ────────────────────────────────────────────
        let req = Request::builder()
            .method("POST")
            .uri("/auth/refresh")
            .header("Cookie", format!("refresh_token={}", refresh_token))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "refresh failed");

        let new_access_token = {
            let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
            let body: RefreshResponse = serde_json::from_slice(&body_bytes).unwrap();
            body.access_token
        };

        // ── Step 4: Access protected endpoint with the new access token ───────
        let req = Request::builder()
            .method("GET")
            .uri("/user/me")
            .header("Authorization", format!("Bearer {}", new_access_token))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "GET /user/me with new token should succeed"
        );

        // ── Step 5: Logout ────────────────────────────────────────────────────
        let req = Request::builder()
            .method("POST")
            .uri("/auth/logout")
            .header("Authorization", format!("Bearer {}", new_access_token))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT, "logout failed");

        // ── Step 6: Protected endpoint now returns 401 ────────────────────────
        let req = Request::builder()
            .method("GET")
            .uri("/user/me")
            .header("Authorization", format!("Bearer {}", new_access_token))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "revoked session should return 401"
        );
    }
}
