use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const OAUTH_STATE_COOKIE_NAME: &str = "ig_oauth_state";
const OAUTH_STATE_TTL_SECS: i64 = 10 * 60;

#[derive(Debug, Serialize, Deserialize)]
struct StateClaims {
    sub: Uuid,
    state: String,
    exp: usize,
    iat: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum StateCookieError {
    #[error("invalid oauth state")]
    InvalidState,
    #[error("invalid oauth state cookie")]
    InvalidCookie(#[from] jsonwebtoken::errors::Error),
}

pub fn issue_state_cookie(user_id: Uuid, jwt_secret: &[u8]) -> (String, Cookie<'static>) {
    let state = Uuid::new_v4().simple().to_string();
    let now = Utc::now();
    let exp = now
        .checked_add_signed(Duration::seconds(OAUTH_STATE_TTL_SECS))
        .expect("valid timestamp")
        .timestamp() as usize;
    let iat = now.timestamp() as usize;

    let claims = StateClaims {
        sub: user_id,
        state: state.clone(),
        exp,
        iat,
    };

    let token = jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret),
    )
    .expect("state token encoding should not fail");

    let cookie = Cookie::build((OAUTH_STATE_COOKIE_NAME, token))
        .path("/oauth/instagram")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(!cfg!(debug_assertions))
        .max_age(cookie::time::Duration::seconds(OAUTH_STATE_TTL_SECS))
        .build();

    (state, cookie)
}

pub fn verify_state_cookie(
    state: &str,
    cookie_value: &str,
    jwt_secret: &[u8],
) -> Result<Uuid, StateCookieError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;

    let token = jsonwebtoken::decode::<StateClaims>(
        cookie_value,
        &DecodingKey::from_secret(jwt_secret),
        &validation,
    )?;

    if token.claims.state != state {
        return Err(StateCookieError::InvalidState);
    }

    Ok(token.claims.sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret";

    #[test]
    fn issue_and_verify_round_trip_returns_user_id() {
        let user_id = Uuid::new_v4();
        let (state, cookie) = issue_state_cookie(user_id, SECRET);

        assert_eq!(cookie.name(), OAUTH_STATE_COOKIE_NAME);
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        assert_eq!(cookie.path(), Some("/oauth/instagram"));

        let verified_user_id =
            verify_state_cookie(&state, cookie.value(), SECRET).expect("state should verify");
        assert_eq!(verified_user_id, user_id);
    }

    #[test]
    fn verify_fails_when_state_does_not_match() {
        let user_id = Uuid::new_v4();
        let (_state, cookie) = issue_state_cookie(user_id, SECRET);

        let result = verify_state_cookie("different-state", cookie.value(), SECRET);
        assert!(matches!(result, Err(StateCookieError::InvalidState)));
    }

    #[test]
    fn verify_fails_for_malformed_cookie() {
        let result = verify_state_cookie("state", "not-a-jwt", SECRET);
        assert!(matches!(result, Err(StateCookieError::InvalidCookie(_))));
    }

    #[test]
    fn verify_fails_for_tampered_cookie() {
        let user_id = Uuid::new_v4();
        let (state, cookie) = issue_state_cookie(user_id, SECRET);
        let mut tampered = cookie.value().to_owned();
        let last = tampered.pop().expect("token should not be empty");
        tampered.push(if last == 'a' { 'b' } else { 'a' });

        let result = verify_state_cookie(&state, &tampered, SECRET);
        assert!(matches!(result, Err(StateCookieError::InvalidCookie(_))));
    }
}
