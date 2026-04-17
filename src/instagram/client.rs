use std::time::Duration;
use crate::config::InstagramConfig;

const FACEBOOK_OAUTH_HOST: &str = "www.facebook.com";
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
        .expect("Failed to parse Facebook OAuth authorize URL");

        let scopes = AUTHORIZE_SCOPES.join(",");
        url.query_pairs_mut()
            .append_pair("client_id", &self.cfg.client_id)
            .append_pair("redirect_uri", &self.cfg.redirect_uri)
            .append_pair("state", state)
            .append_pair("scope", &scopes)
            .append_pair("response_type", "code");

        url.into()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        IgClient, SCOPE_BUSINESS_MANAGEMENT, SCOPE_INSTAGRAM_BASIC, SCOPE_INSTAGRAM_MANAGE_INSIGHTS,
        SCOPE_PAGES_SHOW_LIST,
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
}
