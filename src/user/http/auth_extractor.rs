use axum::{
    extract::{FromRef, FromRequestParts},
    http::{StatusCode, request::Parts},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, Cookie, authorization::Bearer},
};
use tracing::{instrument, warn};
use uuid::Uuid;

use crate::user::crypto::jwt::{Claims, verify_jwt};

/// Authenticated user extracted from a validated JWT.
/// The JWT is read from the `Authorization: Bearer` header or the `token` cookie.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub email: String,
    pub session_id: Uuid,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    String: axum::extract::FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    #[instrument(skip_all)]
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let jwt_secret = String::from_ref(state);

        // Try Authorization: Bearer header first
        let token = if let Ok(TypedHeader(Authorization(bearer))) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state).await
        {
            bearer.token().to_string()
        } else if let Ok(TypedHeader(cookie)) =
            TypedHeader::<Cookie>::from_request_parts(parts, state).await
        {
            // Fall back to the `token` HttpOnly cookie
            match cookie.get("token") {
                Some(t) => t.to_string(),
                None => {
                    warn!("No bearer token or token cookie found");
                    return Err((StatusCode::UNAUTHORIZED, "Missing authentication token"));
                }
            }
        } else {
            warn!("No authentication credentials found");
            return Err((StatusCode::UNAUTHORIZED, "Missing authentication token"));
        };

        let claims: Claims = verify_jwt(&token, &jwt_secret).map_err(|_| {
            warn!("JWT verification failed");
            (StatusCode::UNAUTHORIZED, "Invalid or expired token")
        })?;

        Ok(AuthUser {
            user_id: claims.sub,
            email: claims.email,
            session_id: claims.session_id,
        })
    }
}
