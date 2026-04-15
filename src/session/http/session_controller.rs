use axum::{Router, http::StatusCode, response::IntoResponse, routing::{delete, get, post}};

use crate::session::usecase::session_service::SessionService;

/// POST /auth/refresh — exchange a refresh token for new access/refresh tokens (→ PR 6b).
pub async fn refresh() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

/// POST /auth/logout — revoke the current session (→ PR 6d).
pub async fn logout() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

/// DELETE /auth/sessions — revoke all sessions for the authenticated user (→ PR 6d).
pub async fn logout_all() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

/// GET /auth/sessions — list all active sessions for the authenticated user (→ PR 6d).
pub async fn list_sessions() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

/// DELETE /auth/sessions/:id — delete a specific session by ID (→ PR 6d).
pub async fn delete_session() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

pub fn session_router<S>() -> Router<S>
where
    SessionService: axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/auth/sessions", get(list_sessions).delete(logout_all))
        .route("/auth/sessions/:id", delete(delete_session))
}
