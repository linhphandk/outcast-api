use std::time::Duration;
use crate::config::InstagramConfig;
use crate::instagram::error::IgError;

const FACEBOOK_OAUTH_HOST: &str = "www.facebook.com";
const FACEBOOK_GRAPH_HOST: &str = "graph.facebook.com";
const INSTAGRAM_GRAPH_HOST: &str = "graph.instagram.com";
pub const SCOPE_INSTAGRAM_BASIC: &str = "instagram_basic";
pub const SCOPE_INSTAGRAM_MANAGE_INSIGHTS: &str = "instagram_manage_insights";
pub const SCOPE_PAGES_SHOW_LIST: &str = "pages_show_list";
pub const SCOPE_BUSINESS_MANAGEMENT: &str = "business_management";
const AUTHORIZE_SCOPES: [&str; 4] = [
    SCOPE_INSTAGRAM_BASIC,
    SCOPE_INSTAGRAM_MANAGE_INSIGHTS,
    SCOPE_PAGES_SHOW_LIST,
    SCOPE_BUSINESS_MANAGEMENT,
];

pub struct IgClient {
    http: reqwest::Client,
    cfg: InstagramConfig,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct CodeExchange {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
}

impl IgClient {
    pub fn new(cfg: InstagramConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to initialize Instagram HTTP client");

        Self { http, cfg }
    }

    pub fn build_authorize_url(&self, state: &str) -> String {
        let mut url = url::Url::parse(&format!(
            "https://{}/{}/dialog/oauth",
            FACEBOOK_OAUTH_HOST, self.cfg.graph_api_version
        ))
        .expect("BUG: Failed to construct Facebook OAuth URL - this should never happen with valid constants");

        let scopes = AUTHORIZE_SCOPES.join(",");
        url.query_pairs_mut()
            .append_pair("client_id", &self.cfg.client_id)
            .append_pair("redirect_uri", &self.cfg.redirect_uri)
            .append_pair("state", state)
            .append_pair("scope", &scopes)
            .append_pair("response_type", "code");

        url.to_string()
    }

    pub async fn exchange_code(&self, code: &str) -> Result<CodeExchange, IgError> {
        let res = self.http.get(self.build_exchange_url(code)).send().await?;
        if !res.status().is_success() {
            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await?;
            return Err(IgError::from_response_parts(status, &headers, body));
        }

        let body = res.text().await?;
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn exchange_for_long_lived(&self, short: &str) -> Result<CodeExchange, IgError> {
        let res = self
            .http
            .get(self.build_long_lived_exchange_url(short))
            .send()
            .await?;
        if !res.status().is_success() {
            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await?;
            return Err(IgError::from_response_parts(status, &headers, body));
        }

        let body = res.text().await?;
        Ok(serde_json::from_str(&body)?)
    }

    fn build_exchange_url(&self, code: &str) -> url::Url {
        let mut url = url::Url::parse(&format!(
            "https://{}/{}/oauth/access_token",
            FACEBOOK_GRAPH_HOST, self.cfg.graph_api_version
        ))
        .expect("BUG: Failed to construct Facebook Graph token exchange URL - this should never happen with valid configuration");

        url.query_pairs_mut()
            .append_pair("client_id", &self.cfg.client_id)
            .append_pair("client_secret", &self.cfg.client_secret)
            .append_pair("redirect_uri", &self.cfg.redirect_uri)
            .append_pair("code", code);

        url
    }

    fn build_long_lived_exchange_url(&self, short: &str) -> url::Url {
        let mut url = url::Url::parse(&format!("https://{}/access_token", INSTAGRAM_GRAPH_HOST))
            .expect("BUG: Failed to construct Instagram Graph long-lived token URL - this should never happen with valid constants");

        url.query_pairs_mut()
            .append_pair("grant_type", "ig_exchange_token")
            .append_pair("client_secret", &self.cfg.client_secret)
            .append_pair("access_token", short);

        url
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        CodeExchange, IgClient, SCOPE_BUSINESS_MANAGEMENT, SCOPE_INSTAGRAM_BASIC,
        SCOPE_INSTAGRAM_MANAGE_INSIGHTS, SCOPE_PAGES_SHOW_LIST,
    };
    use crate::config::InstagramConfig;

    fn test_config() -> InstagramConfig {
        InstagramConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            redirect_uri: "http://localhost:3000/oauth/instagram/callback".to_string(),
            graph_api_version: "v19.0".to_string(),
        }
    }

    #[test]
    fn build_authorize_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_authorize_url("my-state");
        let parsed = url::Url::parse(&url).expect("URL should parse");

        assert_eq!(parsed.host_str(), Some("www.facebook.com"));
        assert_eq!(parsed.path(), "/v19.0/dialog/oauth");

        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(query.get("client_id"), Some(&"test-client-id".to_string()));
        assert_eq!(
            query.get("redirect_uri"),
            Some(&"http://localhost:3000/oauth/instagram/callback".to_string())
        );
        assert_eq!(query.get("state"), Some(&"my-state".to_string()));
        assert_eq!(
            query.get("scope"),
            Some(
                &[
                    SCOPE_INSTAGRAM_BASIC,
                    SCOPE_INSTAGRAM_MANAGE_INSIGHTS,
                    SCOPE_PAGES_SHOW_LIST,
                    SCOPE_BUSINESS_MANAGEMENT
                ]
                .join(",")
            )
        );
        assert_eq!(query.get("response_type"), Some(&"code".to_string()));
    }

    #[test]
    fn build_authorize_url_url_encodes_state() {
        let client = IgClient::new(test_config());
        let state = "state/with?unsafe&chars=1";
        let url = client.build_authorize_url(state);

        assert!(url.contains("state=state%2Fwith%3Funsafe%26chars%3D1"));
    }

    #[test]
    fn code_exchange_response_parses_expires_in() {
        let raw = r#"{
            "access_token": "abc123",
            "token_type": "bearer",
            "expires_in": 5183944
        }"#;

        let parsed: CodeExchange = serde_json::from_str(raw).expect("response should parse");
        assert_eq!(parsed.access_token, "abc123");
        assert_eq!(parsed.token_type, "bearer");
        assert_eq!(parsed.expires_in, Some(5183944));
    }

    #[test]
    fn build_exchange_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_exchange_url("auth-code/unsafe?x=1");
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("graph.facebook.com"));
        assert_eq!(parsed.path(), "/v19.0/oauth/access_token");
        assert_eq!(query.get("client_id"), Some(&"test-client-id".to_string()));
        assert_eq!(
            query.get("client_secret"),
            Some(&"test-client-secret".to_string())
        );
        assert_eq!(
            query.get("redirect_uri"),
            Some(&"http://localhost:3000/oauth/instagram/callback".to_string())
        );
        assert_eq!(query.get("code"), Some(&"auth-code/unsafe?x=1".to_string()));
    }

    #[test]
    fn build_long_lived_exchange_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_long_lived_exchange_url("short-lived-token/unsafe?x=1");
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("graph.instagram.com"));
        assert_eq!(parsed.path(), "/access_token");
        assert_eq!(
            query.get("grant_type"),
            Some(&"ig_exchange_token".to_string())
        );
        assert_eq!(
            query.get("client_secret"),
            Some(&"test-client-secret".to_string())
        );
        assert_eq!(
            query.get("access_token"),
            Some(&"short-lived-token/unsafe?x=1".to_string())
        );
    }
}
