use std::sync::Arc;

use chrono::NaiveDateTime;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::session::repository::session_repository::{
    Session, SessionRepositoryError, SessionRepositoryTrait,
};

#[derive(Debug, thiserror::Error)]
pub enum SessionServiceError {
    #[error("Repository error: {0}")]
    RepositoryError(#[from] SessionRepositoryError),
    #[error("Session not found")]
    SessionNotFound,
    #[error("Session has been revoked")]
    SessionRevoked,
    #[error("Session has expired")]
    SessionExpired,
}

fn generate_refresh_token() -> String {
    use rand::RngCore;
    let mut rng = rand::rng();
    let mut bytes = [0u8; 64];
    rng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Access-token cookie max-age (15 minutes, matching JWT expiry).
pub const TOKEN_COOKIE_MAX_AGE_SECS: u32 = 900;

/// Refresh-token cookie max-age (7 days).
pub const REFRESH_COOKIE_MAX_AGE_SECS: u32 = 7 * 24 * 3600;

fn session_expires_at() -> NaiveDateTime {
    chrono::Utc::now().naive_utc()
        + chrono::Duration::seconds(REFRESH_COOKIE_MAX_AGE_SECS as i64)
}

#[derive(Clone)]
pub struct SessionService {
    repo: Arc<dyn SessionRepositoryTrait>,
}

impl SessionService {
    pub fn new(repo: Arc<dyn SessionRepositoryTrait>) -> Self {
        Self { repo }
    }

    #[instrument(skip(self, user_agent, ip_address), fields(user_id = %user_id))]
    pub async fn create_session(
        &self,
        user_id: Uuid,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) -> Result<Session, SessionServiceError> {
        info!("Creating session");
        let refresh_token = generate_refresh_token();
        let expires_at = session_expires_at();
        let session = self
            .repo
            .create(user_id, &refresh_token, user_agent, ip_address, expires_at)
            .await?;
        info!(session_id = %session.id, "Session created");
        Ok(session)
    }

    #[instrument(skip(self, refresh_token))]
    pub async fn find_valid_session_by_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<Session, SessionServiceError> {
        let session = self
            .repo
            .find_by_refresh_token(refresh_token)
            .await?
            .ok_or_else(|| {
                warn!("Session not found for refresh token");
                SessionServiceError::SessionNotFound
            })?;

        if session.revoked_at.is_some() {
            warn!(session_id = %session.id, "Session is revoked");
            return Err(SessionServiceError::SessionRevoked);
        }

        let now = chrono::Utc::now().naive_utc();
        if session.expires_at < now {
            warn!(session_id = %session.id, "Session has expired");
            return Err(SessionServiceError::SessionExpired);
        }

        Ok(session)
    }

    /// Deletes the old session and creates a new one with a fresh refresh token.
    #[instrument(skip(self, user_agent, ip_address), fields(old_session_id = %old_session_id))]
    pub async fn rotate_session(
        &self,
        old_session_id: Uuid,
        user_id: Uuid,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) -> Result<Session, SessionServiceError> {
        info!("Rotating session");
        self.repo.delete(old_session_id).await?;
        let refresh_token = generate_refresh_token();
        let expires_at = session_expires_at();
        let new_session = self
            .repo
            .create(user_id, &refresh_token, user_agent, ip_address, expires_at)
            .await?;
        info!(session_id = %new_session.id, "Session rotated");
        Ok(new_session)
    }

    #[instrument(skip(self), fields(session_id = %session_id))]
    pub async fn revoke_session(&self, session_id: Uuid) -> Result<(), SessionServiceError> {
        info!("Revoking session");
        self.repo.revoke(session_id).await?;
        Ok(())
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn revoke_all_sessions(&self, user_id: Uuid) -> Result<(), SessionServiceError> {
        info!("Revoking all sessions for user");
        self.repo.delete_all_by_user_id(user_id).await?;
        Ok(())
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn list_sessions(&self, user_id: Uuid) -> Result<Vec<Session>, SessionServiceError> {
        let sessions = self.repo.find_all_by_user_id(user_id).await?;
        let now = chrono::Utc::now().naive_utc();
        let active = sessions
            .into_iter()
            .filter(|s| s.revoked_at.is_none() && s.expires_at > now)
            .collect();
        Ok(active)
    }

    /// Deletes a session if it belongs to the requesting user; returns NotFound otherwise.
    #[instrument(skip(self), fields(user_id = %user_id, session_id = %session_id))]
    pub async fn delete_session(
        &self,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Result<(), SessionServiceError> {
        let session = self.repo.find_by_id(session_id).await?.ok_or_else(|| {
            warn!(session_id = %session_id, "Session not found");
            SessionServiceError::SessionNotFound
        })?;

        if session.user_id != user_id {
            warn!(
                session_id = %session_id,
                owner = %session.user_id,
                requester = %user_id,
                "Session does not belong to requesting user"
            );
            // Return NotFound so we do not leak the existence of other users' sessions.
            return Err(SessionServiceError::SessionNotFound);
        }

        self.repo.delete(session_id).await?;
        info!(session_id = %session_id, "Session deleted");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::repository::session_repository::{
        MockSessionRepositoryTrait, Session as SessionModel,
    };
    use chrono::Utc;
    use mockall::predicate::*;
    use uuid::Uuid;

    fn make_session(id: Uuid, user_id: Uuid, revoked: bool) -> SessionModel {
        let now = Utc::now().naive_utc();
        SessionModel {
            id,
            user_id,
            refresh_token: "rt_value".to_owned(),
            user_agent: None,
            ip_address: None,
            expires_at: now + chrono::Duration::days(7),
            revoked_at: if revoked { Some(now) } else { None },
            created_at: now,
            updated_at: now,
        }
    }

    fn make_expired_session(id: Uuid, user_id: Uuid) -> SessionModel {
        let now = Utc::now().naive_utc();
        SessionModel {
            id,
            user_id,
            refresh_token: "rt_expired".to_owned(),
            user_agent: None,
            ip_address: None,
            expires_at: now - chrono::Duration::days(1),
            revoked_at: None,
            created_at: now - chrono::Duration::days(8),
            updated_at: now - chrono::Duration::days(8),
        }
    }

    #[tokio::test]
    async fn test_create_session() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_create()
            .times(1)
            .returning(move |uid, _, _, _, _| Ok(make_session(session_id, uid, false)));

        let svc = SessionService::new(Arc::new(mock));
        let session = svc.create_session(user_id, None, None).await.unwrap();
        assert_eq!(session.user_id, user_id);
    }

    #[tokio::test]
    async fn test_find_valid_session_active() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let session = make_session(session_id, user_id, false);

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_refresh_token()
            .with(eq("good_token"))
            .return_once(move |_| Ok(Some(session)));

        let svc = SessionService::new(Arc::new(mock));
        let result = svc.find_valid_session_by_refresh_token("good_token").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_find_valid_session_not_found() {
        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_refresh_token()
            .return_once(|_| Ok(None));

        let svc = SessionService::new(Arc::new(mock));
        let err = svc
            .find_valid_session_by_refresh_token("missing")
            .await
            .unwrap_err();
        assert!(matches!(err, SessionServiceError::SessionNotFound));
    }

    #[tokio::test]
    async fn test_find_valid_session_revoked() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let session = make_session(session_id, user_id, true);

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_refresh_token()
            .return_once(move |_| Ok(Some(session)));

        let svc = SessionService::new(Arc::new(mock));
        let err = svc
            .find_valid_session_by_refresh_token("revoked")
            .await
            .unwrap_err();
        assert!(matches!(err, SessionServiceError::SessionRevoked));
    }

    #[tokio::test]
    async fn test_find_valid_session_expired() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let session = make_expired_session(session_id, user_id);

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_refresh_token()
            .return_once(move |_| Ok(Some(session)));

        let svc = SessionService::new(Arc::new(mock));
        let err = svc
            .find_valid_session_by_refresh_token("expired")
            .await
            .unwrap_err();
        assert!(matches!(err, SessionServiceError::SessionExpired));
    }

    #[tokio::test]
    async fn test_revoke_session() {
        let session_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let session = make_session(session_id, user_id, false);

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_revoke()
            .with(eq(session_id))
            .return_once(move |_| Ok(session));

        let svc = SessionService::new(Arc::new(mock));
        assert!(svc.revoke_session(session_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_revoke_all_sessions() {
        let user_id = Uuid::new_v4();
        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_delete_all_by_user_id()
            .with(eq(user_id))
            .return_once(|_| Ok(()));

        let svc = SessionService::new(Arc::new(mock));
        assert!(svc.revoke_all_sessions(user_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_list_sessions_filters_revoked_and_expired() {
        let user_id = Uuid::new_v4();
        let active_id = Uuid::new_v4();
        let revoked_id = Uuid::new_v4();
        let expired_id = Uuid::new_v4();

        let sessions = vec![
            make_session(active_id, user_id, false),
            make_session(revoked_id, user_id, true),
            make_expired_session(expired_id, user_id),
        ];

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_all_by_user_id()
            .with(eq(user_id))
            .return_once(move |_| Ok(sessions));

        let svc = SessionService::new(Arc::new(mock));
        let result = svc.list_sessions(user_id).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, active_id);
    }

    #[tokio::test]
    async fn test_delete_session_owned() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let session = make_session(session_id, user_id, false);

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_id()
            .with(eq(session_id))
            .return_once(move |_| Ok(Some(session)));
        mock.expect_delete()
            .with(eq(session_id))
            .return_once(|_| Ok(()));

        let svc = SessionService::new(Arc::new(mock));
        assert!(svc.delete_session(user_id, session_id).await.is_ok());
    }

    #[tokio::test]
    async fn test_delete_session_not_owned() {
        let owner_id = Uuid::new_v4();
        let requester_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let session = make_session(session_id, owner_id, false);

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_id()
            .with(eq(session_id))
            .return_once(move |_| Ok(Some(session)));

        let svc = SessionService::new(Arc::new(mock));
        let err = svc
            .delete_session(requester_id, session_id)
            .await
            .unwrap_err();
        assert!(matches!(err, SessionServiceError::SessionNotFound));
    }

    #[tokio::test]
    async fn test_delete_session_not_found() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let mut mock = MockSessionRepositoryTrait::new();
        mock.expect_find_by_id()
            .with(eq(session_id))
            .return_once(|_| Ok(None));

        let svc = SessionService::new(Arc::new(mock));
        let err = svc
            .delete_session(user_id, session_id)
            .await
            .unwrap_err();
        assert!(matches!(err, SessionServiceError::SessionNotFound));
    }
}
