use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{AppendHeaders, IntoResponse},
    routing::{delete, get, post},
};
use chrono::NaiveDateTime;
use serde::Serialize;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::session::repository::session_repository::SessionRepository;
use crate::session::usecase::session_service::{SessionService, SessionServiceError};
use crate::user::http::auth_extractor::AuthUser;

#[derive(Serialize)]
pub struct SessionRes {
    pub id: Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: NaiveDateTime,
    pub expires_at: NaiveDateTime,
}

#[derive(Serialize)]
pub struct RefreshRes {
    pub access_token: String,
}

/// POST /auth/refresh — issues a new access token using the refresh_token cookie.
/// Accepts an expired access token in Authorization header to extract email.
#[instrument(skip_all)]
pub async fn refresh(
    State(session_service): State<SessionService<SessionRepository>>,
    State(jwt_secret): State<String>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Read refresh token from cookie
    let refresh_token = match extract_cookie(&headers, "refresh_token") {
        Some(t) => t,
        None => {
            warn!("Refresh token cookie missing");
            return (StatusCode::UNAUTHORIZED, "Missing refresh token").into_response();
        }
    };

    // Extract email from Authorization header (token may be expired — we skip expiry check)
    let email = match extract_email_from_header(&headers, &jwt_secret) {
        Some(e) => e,
        None => {
            warn!("Could not extract email from token for refresh");
            return (StatusCode::UNAUTHORIZED, "Invalid or missing access token").into_response();
        }
    };

    match session_service.refresh(refresh_token, &email).await {
        Ok((access_token, new_refresh_token)) => {
            info!("Session refreshed successfully");
            let access_cookie = format!(
                "token={}; HttpOnly; Path=/; Max-Age=900; SameSite=Lax",
                access_token
            );
            let refresh_cookie = format!(
                "refresh_token={}; HttpOnly; Path=/auth/refresh; Max-Age=2592000; SameSite=Strict",
                new_refresh_token
            );
            (
                StatusCode::OK,
                AppendHeaders([
                    (header::SET_COOKIE, access_cookie),
                    (header::SET_COOKIE, refresh_cookie),
                ]),
                Json(RefreshRes { access_token }),
            )
                .into_response()
        }
        Err(SessionServiceError::SessionNotFound) => {
            warn!("Refresh failed: token not found");
            (StatusCode::UNAUTHORIZED, "Invalid refresh token").into_response()
        }
        Err(SessionServiceError::SessionRevoked) => {
            warn!("Refresh failed: token revoked");
            (StatusCode::UNAUTHORIZED, "Refresh token has been revoked").into_response()
        }
        Err(SessionServiceError::SessionExpired) => {
            warn!("Refresh failed: session expired");
            (StatusCode::UNAUTHORIZED, "Session has expired").into_response()
        }
        Err(e) => {
            error!(error = %e, "Refresh failed due to internal error");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to refresh token").into_response()
        }
    }
}

/// POST /auth/logout — revokes the current session.
#[instrument(skip_all)]
pub async fn logout(
    State(session_service): State<SessionService<SessionRepository>>,
    auth_user: AuthUser,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, session_id = %auth_user.session_id, "Logout request");
    match session_service
        .revoke_session(auth_user.session_id, auth_user.user_id)
        .await
    {
        Ok(()) => {
            let clear_access = "token=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax";
            let clear_refresh =
                "refresh_token=; HttpOnly; Path=/auth/refresh; Max-Age=0; SameSite=Strict";
            (
                StatusCode::OK,
                AppendHeaders([
                    (header::SET_COOKIE, clear_access.to_string()),
                    (header::SET_COOKIE, clear_refresh.to_string()),
                ]),
                "Logged out",
            )
                .into_response()
        }
        Err(SessionServiceError::Unauthorized) => {
            warn!("Logout: session not found for user");
            (StatusCode::NOT_FOUND, "Session not found").into_response()
        }
        Err(e) => {
            error!(error = %e, "Logout failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Logout failed").into_response()
        }
    }
}

/// POST /auth/logout-all — revokes all sessions for the user.
#[instrument(skip_all)]
pub async fn logout_all(
    State(session_service): State<SessionService<SessionRepository>>,
    auth_user: AuthUser,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "Logout-all request");
    match session_service.revoke_all_sessions(auth_user.user_id).await {
        Ok(()) => {
            let clear_access = "token=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax";
            let clear_refresh =
                "refresh_token=; HttpOnly; Path=/auth/refresh; Max-Age=0; SameSite=Strict";
            (
                StatusCode::OK,
                AppendHeaders([
                    (header::SET_COOKIE, clear_access.to_string()),
                    (header::SET_COOKIE, clear_refresh.to_string()),
                ]),
                "Logged out from all devices",
            )
                .into_response()
        }
        Err(e) => {
            error!(error = %e, "Logout-all failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Logout failed").into_response()
        }
    }
}

/// GET /auth/sessions — lists active sessions for the user.
#[instrument(skip_all)]
pub async fn list_sessions(
    State(session_service): State<SessionService<SessionRepository>>,
    auth_user: AuthUser,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "List sessions request");
    match session_service.list_sessions(auth_user.user_id).await {
        Ok(sessions) => {
            let res: Vec<SessionRes> = sessions
                .into_iter()
                .map(|s| SessionRes {
                    id: s.id,
                    user_agent: s.user_agent,
                    ip_address: s.ip_address,
                    created_at: s.created_at,
                    expires_at: s.expires_at,
                })
                .collect();
            Json(res).into_response()
        }
        Err(e) => {
            error!(error = %e, "List sessions failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list sessions").into_response()
        }
    }
}

/// DELETE /auth/sessions/:id — revokes a specific session.
#[instrument(skip_all, fields(session_id = %session_id))]
pub async fn revoke_session(
    State(session_service): State<SessionService<SessionRepository>>,
    auth_user: AuthUser,
    Path(session_id): Path<Uuid>,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, session_id = %session_id, "Revoke session request");
    match session_service
        .revoke_session(session_id, auth_user.user_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(SessionServiceError::Unauthorized) => {
            warn!("Revoke session: not authorized");
            (StatusCode::NOT_FOUND, "Session not found").into_response()
        }
        Err(e) => {
            error!(error = %e, "Revoke session failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to revoke session").into_response()
        }
    }
}

fn extract_cookie(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookie_str| {
            cookie_str
                .split(';')
                .map(|s| s.trim())
                .find(|s| s.starts_with(&format!("{}=", name)))
                .map(|s| s[name.len() + 1..].to_string())
        })
}

/// Extracts email from an Authorization: Bearer token, ignoring expiry.
fn extract_email_from_header(
    headers: &axum::http::HeaderMap,
    jwt_secret: &str,
) -> Option<String> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())?;

    use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
    use crate::user::crypto::jwt::Claims;

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = false;

    decode::<Claims>(
        &token,
        &DecodingKey::from_secret(jwt_secret.as_ref()),
        &validation,
    )
    .ok()
    .map(|data| data.claims.email)
}

pub fn router<S>() -> Router<S>
where
    SessionService<SessionRepository>: axum::extract::FromRef<S>,
    String: axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-all", post(logout_all))
        .route("/auth/sessions", get(list_sessions))
        .route("/auth/sessions/:id", delete(revoke_session))
}
