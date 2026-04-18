use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::instagram::client::{CodeExchange, IgClient};
use crate::instagram::error::IgError;
use crate::instagram::repository::{
    OAuthToken, OAuthTokenRepository, OAuthTokenRepositoryError, OAuthTokenRepositoryTrait,
};

#[derive(Clone)]
pub struct InstagramService {
    client: IgClient,
    oauth_repository: OAuthTokenRepository,
}

impl InstagramService {
    pub fn new(client: IgClient, oauth_repository: OAuthTokenRepository) -> Self {
        Self {
            client,
            oauth_repository,
        }
    }

    pub fn build_authorize_url(&self, state: &str) -> String {
        self.client.build_authorize_url(state)
    }

    pub async fn exchange_code(&self, code: &str) -> Result<CodeExchange, IgError> {
        self.client.exchange_code(code).await
    }

    pub async fn exchange_for_long_lived(&self, short: &str) -> Result<CodeExchange, IgError> {
        self.client.exchange_for_long_lived(short).await
    }

    pub async fn upsert_oauth_token(
        &self,
        profile_id: Uuid,
        provider: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
        provider_user_id: &str,
        scopes: &str,
    ) -> Result<OAuthToken, OAuthTokenRepositoryError> {
        self.oauth_repository
            .upsert(
                profile_id,
                provider,
                access_token,
                refresh_token,
                expires_at,
                provider_user_id,
                scopes,
            )
            .await
    }

    pub async fn delete_oauth_token(
        &self,
        profile_id: Uuid,
        provider: &str,
    ) -> Result<bool, OAuthTokenRepositoryError> {
        self.oauth_repository.delete(profile_id, provider).await
    }
}
