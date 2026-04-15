use std::sync::Arc;

use rand::RngCore;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::session::repository::session_repository::{SessionRepositoryError, SessionRepositoryTrait};
use crate::user::repository::user_repository::{RepositoryError, UserRepositoryTrait};

pub const TOKEN_COOKIE_MAX_AGE_SECS: i64 = 900;
pub const REFRESH_COOKIE_MAX_AGE_SECS: i64 = 604_800;

#[derive(Debug, thiserror::Error)]
pub enum SessionServiceError {
    #[error("Refresh token not found")]
    NotFound,
    #[error("Session has been revoked")]
    Revoked,
    #[error("Session has expired")]
    Expired,
    #[error("User not found")]
    UserNotFound,
    #[error("Session repository error: {0}")]
    SessionRepository(#[from] SessionRepositoryError),
    #[error("User repository error: {0}")]
    UserRepository(#[from] RepositoryError),
    #[error("JWT error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
}

#[derive(Debug)]
pub struct RefreshTokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Clone)]
pub struct SessionService {
    pub repository: Arc<dyn SessionRepositoryTrait>,
    pub user_repository: Arc<dyn UserRepositoryTrait>,
}

impl SessionService {
    pub fn new(
        repository: Arc<dyn SessionRepositoryTrait>,
        user_repository: Arc<dyn UserRepositoryTrait>,
    ) -> Self {
        Self {
            repository,
            user_repository,
        }
    }

    #[instrument(skip_all)]
    pub async fn create_session(
        &self,
        user_id: Uuid,
        email: &str,
        user_agent: Option<String>,
        ip_address: Option<String>,
        jwt_secret: &str,
    ) -> Result<RefreshTokens, SessionServiceError> {
        let refresh_token = generate_refresh_token();
        let now = chrono::Utc::now().naive_utc();
        let expires_at = now + chrono::Duration::seconds(REFRESH_COOKIE_MAX_AGE_SECS);

        let session = self
            .repository
            .create(user_id, &refresh_token, user_agent, ip_address, expires_at)
            .await?;

        let access_token =
            crate::user::crypto::jwt::create_jwt(user_id, email, session.id, jwt_secret)?;

        info!(session_id = %session.id, user_id = %user_id, "Session created");

        Ok(RefreshTokens {
            access_token,
            refresh_token,
        })
    }

    #[instrument(skip_all)]
    pub async fn refresh(
        &self,
        old_refresh_token: &str,
        jwt_secret: &str,
    ) -> Result<RefreshTokens, SessionServiceError> {
        // 1. Look up session by old refresh token.
        let session = self
            .repository
            .find_by_refresh_token(old_refresh_token)
            .await?
            .ok_or_else(|| {
                warn!("Refresh attempted with unknown token");
                SessionServiceError::NotFound
            })?;

        // 2. Reuse detection: reject if the session was already rotated or logged out.
        if session.revoked_at.is_some() {
            warn!(session_id = %session.id, "Refresh token reuse detected — session already revoked");
            return Err(SessionServiceError::Revoked);
        }

        // 3. Reject expired sessions.
        let now = chrono::Utc::now().naive_utc();
        if session.expires_at < now {
            warn!(session_id = %session.id, "Refresh attempted with expired session");
            return Err(SessionServiceError::Expired);
        }

        // 4. Revoke the old session (rotate).
        self.repository.revoke(session.id).await?;

        // 5. Look up user to embed email in the new JWT.
        let user = self
            .user_repository
            .find_by_id(session.user_id)
            .await?
            .ok_or_else(|| {
                error!(user_id = %session.user_id, "User not found during token refresh");
                SessionServiceError::UserNotFound
            })?;

        // 6. Generate a fresh 128-char hex refresh token (64 random bytes).
        let new_refresh_token = generate_refresh_token();

        // 7. Persist the new session, carrying over user-agent and IP.
        let new_expires_at = now + chrono::Duration::seconds(REFRESH_COOKIE_MAX_AGE_SECS);
        let new_session = self
            .repository
            .create(
                session.user_id,
                &new_refresh_token,
                session.user_agent.clone(),
                session.ip_address.clone(),
                new_expires_at,
            )
            .await?;

        // 8. Mint a new short-lived JWT bound to the new session.
        let access_token =
            crate::user::crypto::jwt::create_jwt(user.id, &user.email, new_session.id, jwt_secret)?;

        info!(
            old_session_id = %session.id,
            new_session_id = %new_session.id,
            user_id = %user.id,
            "Token refresh successful"
        );

        Ok(RefreshTokens {
            access_token,
            refresh_token: new_refresh_token,
        })
    }
}

fn generate_refresh_token() -> String {
    let mut bytes = [0u8; 64];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;
    use mockall::predicate::eq;
    use uuid::Uuid;

    use crate::session::repository::session_repository::{MockSessionRepositoryTrait, Session};
    use crate::user::repository::user_repository::{MockUserRepositoryTrait, User};

    use super::*;

    const SECRET: &str = "test-secret";

    fn make_session(id: Uuid, user_id: Uuid, refresh_token: &str, revoked: bool) -> Session {
        let now = Utc::now().naive_utc();
        Session {
            id,
            user_id,
            refresh_token: refresh_token.to_owned(),
            user_agent: Some("Mozilla/5.0".to_owned()),
            ip_address: Some("127.0.0.1".to_owned()),
            expires_at: now + chrono::Duration::days(7),
            revoked_at: if revoked { Some(now) } else { None },
            created_at: now,
            updated_at: now,
        }
    }

    fn make_user(id: Uuid) -> User {
        User {
            id,
            email: "user@example.com".to_owned(),
            password: "hashed".to_owned(),
        }
    }

    fn make_expired_session(id: Uuid, user_id: Uuid, refresh_token: &str) -> Session {
        let now = Utc::now().naive_utc();
        Session {
            expires_at: now - chrono::Duration::seconds(1),
            ..make_session(id, user_id, refresh_token, false)
        }
    }

    #[tokio::test]
    async fn refresh_success_rotates_and_mints_jwt() {
        let user_id = Uuid::new_v4();
        let old_session_id = Uuid::new_v4();
        let new_session_id = Uuid::new_v4();
        let old_token = "old_refresh_token";

        let session = make_session(old_session_id, user_id, old_token, false);
        let user = make_user(user_id);

        let mut session_repo = MockSessionRepositoryTrait::new();
        session_repo
            .expect_find_by_refresh_token()
            .with(eq(old_token))
            .return_once(move |_| Ok(Some(session)));

        let revoked_session = {
            let now = Utc::now().naive_utc();
            Session {
                id: old_session_id,
                user_id,
                refresh_token: old_token.to_owned(),
                user_agent: None,
                ip_address: None,
                expires_at: now + chrono::Duration::days(7),
                revoked_at: Some(now),
                created_at: now,
                updated_at: now,
            }
        };
        session_repo
            .expect_revoke()
            .with(eq(old_session_id))
            .return_once(move |_| Ok(revoked_session));

        session_repo
            .expect_create()
            .return_once(move |u_id, rt, ua, ip, exp| {
                let now = Utc::now().naive_utc();
                Ok(Session {
                    id: new_session_id,
                    user_id: u_id,
                    refresh_token: rt.to_owned(),
                    user_agent: ua,
                    ip_address: ip,
                    expires_at: exp,
                    revoked_at: None,
                    created_at: now,
                    updated_at: now,
                })
            });

        let mut user_repo = MockUserRepositoryTrait::new();
        user_repo
            .expect_find_by_id()
            .with(eq(user_id))
            .return_once(move |_| Ok(Some(user)));

        let svc = SessionService::new(Arc::new(session_repo), Arc::new(user_repo));
        let result = svc.refresh(old_token, SECRET).await.unwrap();

        assert!(!result.access_token.is_empty());
        assert_ne!(result.refresh_token, old_token);
        assert_eq!(result.refresh_token.len(), 128); // 64 bytes = 128 hex chars

        let claims = crate::user::crypto::jwt::verify_jwt(&result.access_token, SECRET).unwrap();
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.session_id, new_session_id);
    }

    #[tokio::test]
    async fn refresh_unknown_token_returns_not_found() {
        let mut session_repo = MockSessionRepositoryTrait::new();
        session_repo
            .expect_find_by_refresh_token()
            .return_once(|_| Ok(None));

        let user_repo = MockUserRepositoryTrait::new();
        let svc = SessionService::new(Arc::new(session_repo), Arc::new(user_repo));
        let err = svc.refresh("nonexistent", SECRET).await.unwrap_err();
        assert!(matches!(err, SessionServiceError::NotFound));
    }

    #[tokio::test]
    async fn refresh_revoked_token_returns_revoked() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let session = make_session(session_id, user_id, "tok", true);

        let mut session_repo = MockSessionRepositoryTrait::new();
        session_repo
            .expect_find_by_refresh_token()
            .return_once(move |_| Ok(Some(session)));

        let user_repo = MockUserRepositoryTrait::new();
        let svc = SessionService::new(Arc::new(session_repo), Arc::new(user_repo));
        let err = svc.refresh("tok", SECRET).await.unwrap_err();
        assert!(matches!(err, SessionServiceError::Revoked));
    }

    #[tokio::test]
    async fn refresh_expired_session_returns_expired() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let session = make_expired_session(session_id, user_id, "tok");

        let mut session_repo = MockSessionRepositoryTrait::new();
        session_repo
            .expect_find_by_refresh_token()
            .return_once(move |_| Ok(Some(session)));

        let user_repo = MockUserRepositoryTrait::new();
        let svc = SessionService::new(Arc::new(session_repo), Arc::new(user_repo));
        let err = svc.refresh("tok", SECRET).await.unwrap_err();
        assert!(matches!(err, SessionServiceError::Expired));
    }
}
