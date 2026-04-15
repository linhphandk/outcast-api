use std::sync::Arc;

use crate::session::repository::session_repository::SessionRepositoryTrait;

/// Max-age for the short-lived access-token cookie (15 minutes).
pub const TOKEN_COOKIE_MAX_AGE_SECS: i64 = 900;

/// Max-age for the long-lived refresh-token cookie (7 days).
pub const REFRESH_COOKIE_MAX_AGE_SECS: i64 = 604_800;

#[derive(Clone)]
pub struct SessionService {
    pub repository: Arc<dyn SessionRepositoryTrait>,
}

impl SessionService {
    pub fn new(repository: Arc<dyn SessionRepositoryTrait>) -> Self {
        Self { repository }
    }
}
