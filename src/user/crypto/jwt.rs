use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, encode};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, instrument};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,     // Subject (user_id)
    pub email: String, // User email
    pub exp: usize,    // Expiration time (as timestamp)
    pub iat: usize,    // Issued at
}

#[instrument(skip(secret), fields(user_id = %user_id))]
pub fn create_jwt(
    user_id: Uuid,
    email: &str,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    debug!("Creating JWT");
    let expiration = Utc::now()
        .checked_add_signed(Duration::hours(24))
        .expect("valid timestamp")
        .timestamp();

    let claims = Claims {
        sub: user_id,
        email: email.to_owned(),
        exp: expiration as usize,
        iat: Utc::now().timestamp() as usize,
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
