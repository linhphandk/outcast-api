use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::{delete, get, post}};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use tracing::{error, info, instrument, warn};

use crate::session::http::cookies::{REFRESH_TOKEN_COOKIE_NAME, set_auth_cookies};
use crate::session::usecase::session_service::{SessionService, SessionServiceError};

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


pub async fn logout() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn logout_all() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}


pub async fn list_sessions() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn delete_session() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub fn router<S>() -> Router<S>
where
    SessionService: axum::extract::FromRef<S>,
    String: axum::extract::FromRef<S>,
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

    use axum::{Router, body::Body, http::Request, routing::post};
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
        jwt_secret: String,
    }

    impl axum::extract::FromRef<TestState> for SessionService {
        fn from_ref(state: &TestState) -> Self {
            state.session_service.clone()
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
        let session_service = SessionService::new(session_repo, user_repo);
        let state = TestState {
            session_service,
            jwt_secret: JWT_SECRET.to_string(),
        };
        Router::new()
            .route("/auth/refresh", post(refresh_session))
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
}
