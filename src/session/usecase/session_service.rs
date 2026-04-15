use crate::session::repository::session_repository::{
    Session, SessionRepositoryError, SessionRepositoryTrait,
};
use crate::user::crypto::jwt::create_jwt;
use chrono::Duration;
use rand::RngCore;
use tracing::{info, instrument, warn};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum SessionServiceError {
    #[error("Repository error: {0}")]
    RepositoryError(#[from] SessionRepositoryError),
    #[error("Session not found")]
    SessionNotFound,
    #[error("Session revoked")]
    SessionRevoked,
    #[error("Session expired")]
    SessionExpired,
    #[error("Unauthorized")]
    Unauthorized,
    #[error("JWT error: {0}")]
    JwtError(#[from] jsonwebtoken::errors::Error),
}

pub struct SessionService<R: SessionRepositoryTrait> {
    repository: R,
    jwt_secret: String,
}

impl<R: SessionRepositoryTrait + Clone> Clone for SessionService<R> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            jwt_secret: self.jwt_secret.clone(),
        }
    }
}

/// Generates a cryptographically secure 64-byte random opaque refresh token (hex-encoded).
fn generate_refresh_token() -> String {
    let mut bytes = [0u8; 64];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

impl<R: SessionRepositoryTrait> SessionService<R> {
    pub fn new(repository: R, jwt_secret: String) -> Self {
        Self {
            repository,
            jwt_secret,
        }
    }

    /// Creates a new session for a user.
    /// Returns `(access_token, refresh_token, session)`.
    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn create_session(
        &self,
        user_id: Uuid,
        email: &str,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) -> Result<(String, String, Session), SessionServiceError> {
        info!("Creating session for user");
        let refresh_token = generate_refresh_token();
        let expires_at = (chrono::Utc::now() + Duration::days(30)).naive_utc();

        let session = self
            .repository
            .create(
                user_id,
                refresh_token.clone(),
                user_agent,
                ip_address,
                expires_at,
            )
            .await?;

        let access_token = create_jwt(user_id, email, session.id, &self.jwt_secret)?;

        info!(session_id = %session.id, "Session created");
        Ok((access_token, refresh_token, session))
    }

    /// Refreshes a session: validates the refresh token, rotates it, and issues a new access token.
    /// Returns `(new_access_token, new_refresh_token)`.
    #[instrument(skip(self, refresh_token))]
    pub async fn refresh(
        &self,
        refresh_token: String,
        email: &str,
    ) -> Result<(String, String), SessionServiceError> {
        info!("Refreshing session");
        let session = self
            .repository
            .find_by_refresh_token(refresh_token)
            .await?
            .ok_or_else(|| {
                warn!("Refresh token not found");
                SessionServiceError::SessionNotFound
            })?;

        if session.revoked_at.is_some() {
            warn!(session_id = %session.id, "Attempted use of revoked refresh token");
            return Err(SessionServiceError::SessionRevoked);
        }

        if session.expires_at < chrono::Utc::now().naive_utc() {
            warn!(session_id = %session.id, "Attempted use of expired session");
            return Err(SessionServiceError::SessionExpired);
        }

        let new_refresh_token = generate_refresh_token();
        self.repository
            .update_refresh_token(session.id, new_refresh_token.clone())
            .await?;

        let access_token = create_jwt(session.user_id, email, session.id, &self.jwt_secret)?;

        info!(session_id = %session.id, "Session refreshed");
        Ok((access_token, new_refresh_token))
    }

    /// Revokes a specific session. Validates that the session belongs to the given user.
    #[instrument(skip(self), fields(session_id = %session_id, user_id = %user_id))]
    pub async fn revoke_session(
        &self,
        session_id: Uuid,
        user_id: Uuid,
    ) -> Result<(), SessionServiceError> {
        info!("Revoking session");
        let sessions = self.repository.find_active_by_user_id(user_id).await?;
        let session = sessions.iter().find(|s| s.id == session_id);

        if session.is_none() {
            warn!(session_id = %session_id, "Session not found or does not belong to user");
            return Err(SessionServiceError::Unauthorized);
        }

        self.repository.revoke(session_id).await?;
        info!(session_id = %session_id, "Session revoked");
        Ok(())
    }

    /// Revokes all sessions for a user (logout everywhere).
    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn revoke_all_sessions(&self, user_id: Uuid) -> Result<(), SessionServiceError> {
        info!("Revoking all sessions for user");
        self.repository.revoke_all_for_user(user_id).await?;
        info!(user_id = %user_id, "All sessions revoked");
        Ok(())
    }

    /// Lists all active sessions for a user.
    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn list_sessions(&self, user_id: Uuid) -> Result<Vec<Session>, SessionServiceError> {
        info!("Listing active sessions for user");
        let sessions = self.repository.find_active_by_user_id(user_id).await?;
        info!(count = sessions.len(), "Sessions listed");
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::repository::session_repository::MockSessionRepositoryTrait;
    use chrono::Duration;
    use mockall::predicate::*;
    use uuid::Uuid;

    fn make_session(
        id: Uuid,
        user_id: Uuid,
        revoked: bool,
        expired: bool,
    ) -> Session {
        use chrono::NaiveDateTime;
        let expires_at = if expired {
            (chrono::Utc::now() - Duration::hours(1)).naive_utc()
        } else {
            (chrono::Utc::now() + Duration::days(30)).naive_utc()
        };
        let revoked_at: Option<NaiveDateTime> = if revoked {
            Some(chrono::Utc::now().naive_utc())
        } else {
            None
        };
        Session {
            id,
            user_id,
            refresh_token: "test_token".to_string(),
            user_agent: None,
            ip_address: None,
            expires_at,
            revoked_at,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[tokio::test]
    async fn test_create_session_success() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let mut mock = MockSessionRepositoryTrait::new();

        mock.expect_create()
            .times(1)
            .returning(move |uid, rt, ua, ip, exp| {
                Ok(Session {
                    id: session_id,
                    user_id: uid,
                    refresh_token: rt,
                    user_agent: ua,
                    ip_address: ip,
                    expires_at: exp,
                    revoked_at: None,
                    created_at: chrono::Utc::now().naive_utc(),
                    updated_at: chrono::Utc::now().naive_utc(),
                })
            });

        let service = SessionService::new(mock, "test_secret".to_string());
        let result = service
            .create_session(user_id, "user@example.com", None, None)
            .await;

        assert!(result.is_ok());
        let (access_token, refresh_token, session) = result.unwrap();
        assert!(!access_token.is_empty());
        assert!(!refresh_token.is_empty());
        assert_eq!(session.user_id, user_id);
    }

    #[tokio::test]
    async fn test_refresh_success() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let mut mock = MockSessionRepositoryTrait::new();

        mock.expect_find_by_refresh_token()
            .with(eq("old_token".to_string()))
            .times(1)
            .returning(move |rt| Ok(Some(make_session_with_token(session_id, user_id, rt))));

        mock.expect_update_refresh_token()
            .with(eq(session_id), always())
            .times(1)
            .returning(|_, _| Ok(()));

        let service = SessionService::new(mock, "test_secret".to_string());
        let result = service
            .refresh("old_token".to_string(), "user@example.com")
            .await;

        assert!(result.is_ok());
        let (access_token, new_refresh) = result.unwrap();
        assert!(!access_token.is_empty());
        assert!(!new_refresh.is_empty());
        assert_ne!(new_refresh, "old_token");
    }

    fn make_session_with_token(id: Uuid, user_id: Uuid, token: String) -> Session {
        Session {
            id,
            user_id,
            refresh_token: token,
            user_agent: None,
            ip_address: None,
            expires_at: (chrono::Utc::now() + Duration::days(30)).naive_utc(),
            revoked_at: None,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[tokio::test]
    async fn test_refresh_revoked_token() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let mut mock = MockSessionRepositoryTrait::new();

        mock.expect_find_by_refresh_token()
            .times(1)
            .returning(move |_| Ok(Some(make_session(session_id, user_id, true, false))));

        let service = SessionService::new(mock, "test_secret".to_string());
        let result = service
            .refresh("revoked_token".to_string(), "user@example.com")
            .await;

        assert!(matches!(result, Err(SessionServiceError::SessionRevoked)));
    }

    #[tokio::test]
    async fn test_refresh_expired_session() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let mut mock = MockSessionRepositoryTrait::new();

        mock.expect_find_by_refresh_token()
            .times(1)
            .returning(move |_| Ok(Some(make_session(session_id, user_id, false, true))));

        let service = SessionService::new(mock, "test_secret".to_string());
        let result = service
            .refresh("expired_token".to_string(), "user@example.com")
            .await;

        assert!(matches!(result, Err(SessionServiceError::SessionExpired)));
    }

    #[tokio::test]
    async fn test_refresh_not_found() {
        let mut mock = MockSessionRepositoryTrait::new();

        mock.expect_find_by_refresh_token()
            .times(1)
            .returning(|_| Ok(None));

        let service = SessionService::new(mock, "test_secret".to_string());
        let result = service
            .refresh("unknown_token".to_string(), "user@example.com")
            .await;

        assert!(matches!(result, Err(SessionServiceError::SessionNotFound)));
    }

    #[tokio::test]
    async fn test_revoke_session_success() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let mut mock = MockSessionRepositoryTrait::new();

        mock.expect_find_active_by_user_id()
            .with(eq(user_id))
            .times(1)
            .returning(move |uid| Ok(vec![make_session(session_id, uid, false, false)]));

        mock.expect_revoke()
            .with(eq(session_id))
            .times(1)
            .returning(|_| Ok(()));

        let service = SessionService::new(mock, "test_secret".to_string());
        let result = service.revoke_session(session_id, user_id).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_revoke_session_unauthorized() {
        let user_id = Uuid::new_v4();
        let other_session_id = Uuid::new_v4();
        let mut mock = MockSessionRepositoryTrait::new();

        mock.expect_find_active_by_user_id()
            .with(eq(user_id))
            .times(1)
            .returning(|_| Ok(vec![]));

        let service = SessionService::new(mock, "test_secret".to_string());
        let result = service.revoke_session(other_session_id, user_id).await;

        assert!(matches!(result, Err(SessionServiceError::Unauthorized)));
    }

    #[tokio::test]
    async fn test_revoke_all_sessions() {
        let user_id = Uuid::new_v4();
        let mut mock = MockSessionRepositoryTrait::new();

        mock.expect_revoke_all_for_user()
            .with(eq(user_id))
            .times(1)
            .returning(|_| Ok(()));

        let service = SessionService::new(mock, "test_secret".to_string());
        let result = service.revoke_all_sessions(user_id).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let user_id = Uuid::new_v4();
        let mut mock = MockSessionRepositoryTrait::new();

        mock.expect_find_active_by_user_id()
            .with(eq(user_id))
            .times(1)
            .returning(move |uid| {
                Ok(vec![
                    make_session(Uuid::new_v4(), uid, false, false),
                    make_session(Uuid::new_v4(), uid, false, false),
                ])
            });

        let service = SessionService::new(mock, "test_secret".to_string());
        let result = service.list_sessions(user_id).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }
}
