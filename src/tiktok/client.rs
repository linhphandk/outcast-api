use std::sync::Arc;
use std::time::Duration;

use reqwest::Client as HttpClient;
use url::Url;

use crate::config::TikTokConfig;

#[derive(Clone)]
pub struct TikTokClient {
    http: HttpClient,
    auth_base_url: Url,
    api_base_url: Url,
    client_key: String,
    client_secret: String,
}

impl TikTokClient {
    pub fn new(cfg: &TikTokConfig) -> Arc<Self> {
        let http = HttpClient::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("outcast-api/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client builds");

        Arc::new(Self {
            http,
            auth_base_url: Url::parse(&cfg.auth_base_url).expect("auth_base_url parses"),
            api_base_url: Url::parse(&cfg.api_base_url).expect("api_base_url parses"),
            client_key: cfg.client_key.clone(),
            client_secret: cfg.client_secret.clone(),
        })
    }

    pub(crate) fn client_key(&self) -> &str {
        &self.client_key
    }

    pub(crate) fn client_secret(&self) -> &str {
        &self.client_secret
    }

    pub(crate) fn auth_base_url(&self) -> &Url {
        &self.auth_base_url
    }

    pub(crate) fn api_base_url(&self) -> &Url {
        &self.api_base_url
    }

    pub(crate) fn http(&self) -> &HttpClient {
        &self.http
    }
}

#[cfg(test)]
mod tests {
    use super::TikTokClient;
    use crate::config::TikTokConfig;

    fn test_config() -> TikTokConfig {
        TikTokConfig {
            client_key: "test-client-key".to_string(),
            client_secret: "test-client-secret".to_string(),
            redirect_uri: "http://localhost:3000/oauth/tiktok/callback".to_string(),
            scopes: "user.info.basic,user.info.profile,user.info.stats".to_string(),
            api_base_url: "https://open.tiktokapis.com".to_string(),
            auth_base_url: "https://www.tiktok.com".to_string(),
        }
    }

    #[test]
    fn constructor_parses_urls() {
        let mut cfg = test_config();
        cfg.api_base_url = "http://127.0.0.1:9999".to_string();

        let client = TikTokClient::new(&cfg);

        assert_eq!(client.api_base_url().as_str(), "http://127.0.0.1:9999/");
    }

    #[test]
    #[should_panic]
    fn constructor_rejects_invalid_url() {
        let mut cfg = test_config();
        cfg.api_base_url = "not a url".to_string();

        let _ = TikTokClient::new(&cfg);
    }
}
