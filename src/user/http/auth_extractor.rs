use std::sync::Arc;

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{StatusCode, request::Parts},
};
use axum_extra::{
    TypedHeader,
    extract::CookieJar,
    headers::{Authorization, authorization::Bearer},
};
use uuid::Uuid;

use crate::{
    session::repository::session_repository::SessionRepositoryTrait,
    user::crypto::jwt::verify_jwt,
};

/// Authenticated user extracted from a valid JWT in the `Authorization: Bearer` header
/// or the `token` HttpOnly cookie. The associated session must be active (not revoked).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub email: String,
    pub session_id: Uuid,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Arc<dyn SessionRepositoryTrait>: FromRef<S>,
    String: FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let jwt_secret = String::from_ref(state);
        let session_repo = Arc::<dyn SessionRepositoryTrait>::from_ref(state);

        // 1. Try Authorization: Bearer header first, fall back to `token` cookie.
        let token =
            if let Ok(TypedHeader(Authorization(bearer))) =
                TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state).await
            {
                bearer.token().to_owned()
            } else {
                let jar = CookieJar::from_request_parts(parts, state)
                    .await
                    .unwrap_or_default();
                jar.get("token")
                    .map(|c| c.value().to_owned())
                    .ok_or((StatusCode::UNAUTHORIZED, "Missing authentication token"))?
            };

        // 2. Validate JWT signature and expiry.
        let claims = verify_jwt(&token, &jwt_secret)
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        // 3. Verify session is still active in the DB.
        let session = session_repo
            .find_by_id(claims.session_id)
            .await
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Session lookup failed"))?
            .ok_or((StatusCode::UNAUTHORIZED, "Session not found or revoked"))?;

        if session.revoked_at.is_some() {
            return Err((StatusCode::UNAUTHORIZED, "Session has been revoked"));
        }

        Ok(AuthUser {
            user_id: claims.sub,
            email: claims.email,
            session_id: claims.session_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use chrono::Utc;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use uuid::Uuid;

    use crate::session::repository::session_repository::{
        MockSessionRepositoryTrait, Session,
    };
    use crate::user::crypto::jwt::{Claims, create_jwt};

    // ---------------------------------------------------------------------------
    // Minimal test state — avoids a real AppState / DB connection in unit tests.
    // ---------------------------------------------------------------------------

    struct TestState {
        jwt_secret: String,
        session_repo: Arc<dyn SessionRepositoryTrait>,
    }

    impl FromRef<TestState> for String {
        fn from_ref(s: &TestState) -> Self {
            s.jwt_secret.clone()
        }
    }

    impl FromRef<TestState> for Arc<dyn SessionRepositoryTrait> {
        fn from_ref(s: &TestState) -> Self {
            s.session_repo.clone()
        }
    }

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    const SECRET: &str = "test-secret";

    fn make_active_session(session_id: Uuid, user_id: Uuid) -> Session {
        let now = Utc::now().naive_utc();
        Session {
            id: session_id,
            user_id,
            refresh_token: "rt".to_owned(),
            user_agent: None,
            ip_address: None,
            expires_at: now + chrono::Duration::days(7),
            revoked_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_revoked_session(session_id: Uuid, user_id: Uuid) -> Session {
        let now = Utc::now().naive_utc();
        Session {
            revoked_at: Some(now),
            ..make_active_session(session_id, user_id)
        }
    }

    fn make_expired_token(user_id: Uuid, session_id: Uuid) -> String {
        let now = Utc::now();
        let claims = Claims {
            sub: user_id,
            email: "a@b.com".to_owned(),
            session_id,
            // 120 s in the past — exceeds jsonwebtoken's default 60 s clock-skew leeway
            exp: (now.timestamp() - 120) as usize,
            iat: (now.timestamp() - 16 * 60) as usize,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(SECRET.as_ref()),
        )
        .unwrap()
    }

    // ---------------------------------------------------------------------------
    // Tests
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_happy_path_bearer_header() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let token = create_jwt(user_id, "user@example.com", session_id, SECRET).unwrap();
        let session = make_active_session(session_id, user_id);

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_id()
            .with(mockall::predicate::eq(session_id))
            .return_once(move |_| Ok(Some(session)));

        let state = TestState {
            jwt_secret: SECRET.to_owned(),
            session_repo: Arc::new(mock),
        };

        let req = Request::builder()
            .header("Authorization", format!("Bearer {}", token))
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = AuthUser::from_request_parts(&mut parts, &state).await;
        assert!(result.is_ok());
        let auth_user = result.unwrap();
        assert_eq!(auth_user.user_id, user_id);
        assert_eq!(auth_user.session_id, session_id);
        assert_eq!(auth_user.email, "user@example.com");
    }

    #[tokio::test]
    async fn test_happy_path_cookie() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let token = create_jwt(user_id, "user@example.com", session_id, SECRET).unwrap();
        let session = make_active_session(session_id, user_id);

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_id()
            .return_once(move |_| Ok(Some(session)));

        let state = TestState {
            jwt_secret: SECRET.to_owned(),
            session_repo: Arc::new(mock),
        };

        let req = Request::builder()
            .header("Cookie", format!("token={}", token))
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = AuthUser::from_request_parts(&mut parts, &state).await;
        assert!(result.is_ok());
        let auth_user = result.unwrap();
        assert_eq!(auth_user.user_id, user_id);
    }

    #[tokio::test]
    async fn test_missing_token_returns_401() {
        let mock = MockSessionRepositoryTrait::new();
        let state = TestState {
            jwt_secret: SECRET.to_owned(),
            session_repo: Arc::new(mock),
        };

        let req = Request::builder().body(()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = AuthUser::from_request_parts(&mut parts, &state).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_invalid_signature_returns_401() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        // Signed with a different secret
        let token = create_jwt(user_id, "user@example.com", session_id, "wrong-secret").unwrap();

        let mock = MockSessionRepositoryTrait::new();
        let state = TestState {
            jwt_secret: SECRET.to_owned(),
            session_repo: Arc::new(mock),
        };

        let req = Request::builder()
            .header("Authorization", format!("Bearer {}", token))
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = AuthUser::from_request_parts(&mut parts, &state).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_expired_token_returns_401() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let token = make_expired_token(user_id, session_id);

        let mock = MockSessionRepositoryTrait::new();
        let state = TestState {
            jwt_secret: SECRET.to_owned(),
            session_repo: Arc::new(mock),
        };

        let req = Request::builder()
            .header("Authorization", format!("Bearer {}", token))
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = AuthUser::from_request_parts(&mut parts, &state).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_revoked_session_returns_401() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let token = create_jwt(user_id, "user@example.com", session_id, SECRET).unwrap();
        let session = make_revoked_session(session_id, user_id);

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_id()
            .return_once(move |_| Ok(Some(session)));

        let state = TestState {
            jwt_secret: SECRET.to_owned(),
            session_repo: Arc::new(mock),
        };

        let req = Request::builder()
            .header("Authorization", format!("Bearer {}", token))
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = AuthUser::from_request_parts(&mut parts, &state).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_session_db_error_returns_401() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let token = create_jwt(user_id, "user@example.com", session_id, SECRET).unwrap();

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_id()
            .return_once(|_| {
                Err(crate::session::repository::session_repository::SessionRepositoryError::DieselError(
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::Unknown,
                        Box::new("connection error".to_string()),
                    ),
                ))
            });

        let state = TestState {
            jwt_secret: SECRET.to_owned(),
            session_repo: Arc::new(mock),
        };

        let req = Request::builder()
            .header("Authorization", format!("Bearer {}", token))
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = AuthUser::from_request_parts(&mut parts, &state).await;
        assert!(result.is_err());
        let (status, msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(msg, "Session lookup failed");
    }
}
