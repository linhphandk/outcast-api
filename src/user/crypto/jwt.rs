use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, encode};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, instrument};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,        // Subject (user_id)
    pub email: String,    // User email
    pub session_id: Uuid, // Session that issued this token
    pub exp: usize,       // Expiration time (as timestamp)
    pub iat: usize,       // Issued at
}

#[instrument(skip(secret), fields(user_id = %user_id))]
pub fn create_jwt(
    user_id: Uuid,
    email: &str,
    session_id: Uuid,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    debug!("Creating JWT");
    let now = Utc::now();
    let expiration = now
        .checked_add_signed(Duration::minutes(120))
        .expect("valid timestamp")
        .timestamp();

    let claims = Claims {
        sub: user_id,
        email: email.to_owned(),
        session_id,
        exp: expiration as usize,
        iat: now.timestamp() as usize,
    };

    let result = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    );

    if let Err(ref e) = result {
        error!(error = %e, "Failed to create JWT");
    } else {
        debug!("JWT created successfully");
    }

    result
}

#[instrument(skip_all)]
pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    debug!("Verifying JWT");
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|e| {
        error!(error = %e, "JWT verification failed");
        e
    })?;

    debug!(user_id = %token_data.claims.sub, "JWT verified successfully");
    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test_secret";

    #[test]
    fn test_create_and_verify_jwt() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let email = "user@example.com";

        let token = create_jwt(user_id, email, session_id, SECRET).expect("JWT creation failed");
        let claims = verify_jwt(&token, SECRET).expect("JWT verification failed");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.email, email);
        assert_eq!(claims.session_id, session_id);
    }

    #[test]
    fn test_round_trip_preserves_session_id() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let token =
            create_jwt(user_id, "a@b.com", session_id, SECRET).expect("JWT creation failed");
        let claims = verify_jwt(&token, SECRET).expect("JWT verification failed");

        assert_eq!(claims.session_id, session_id);
    }

    #[test]
    fn test_expired_token_fails_verification() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        // Manually craft a token that expired 1 second ago
        let now = Utc::now();
        let claims = Claims {
            sub: user_id,
            email: "a@b.com".to_owned(),
            session_id,
            // Use 120s in the past to exceed jsonwebtoken's default 60s leeway
            exp: (now.timestamp() - 120) as usize,
            iat: (now.timestamp() - 16 * 60) as usize,
        };
        let token = jsonwebtoken::encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(SECRET.as_ref()),
        )
        .expect("encode failed");

        let result = verify_jwt(&token, SECRET);
        assert!(result.is_err(), "Expected expired token to fail verification");
    }

    #[test]
    fn test_expiry_is_15_minutes() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let before = Utc::now().timestamp() as usize;
        let token =
            create_jwt(user_id, "a@b.com", session_id, SECRET).expect("JWT creation failed");
        let after = Utc::now().timestamp() as usize;

        let claims = verify_jwt(&token, SECRET).expect("JWT verification failed");
        let expected_min = before + 15 * 60;
        let expected_max = after + 15 * 60;

        assert!(
            claims.exp >= expected_min && claims.exp <= expected_max,
            "exp={} not in [{}, {}]",
            claims.exp,
            expected_min,
            expected_max
        );
    }
}
