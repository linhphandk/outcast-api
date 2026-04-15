use axum::{Router, http::StatusCode, response::IntoResponse, routing::{delete, get, post}};

use crate::session::usecase::session_service::SessionService;

pub async fn refresh_session() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
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
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/refresh", post(refresh_session))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-all", post(logout_all))
        .route("/auth/sessions", get(list_sessions))
        .route("/auth/sessions/{id}", delete(delete_session))
}
