use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client as HttpClient, Response};
use serde::de::DeserializeOwned;
use url::Url;

use crate::config::TikTokConfig;
use crate::tiktok::error::TikTokError;

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
            .expect("Failed to build HTTP client for TikTok API");

        Arc::new(Self {
            http,
            auth_base_url: Url::parse(&cfg.auth_base_url)
                .expect("Invalid auth_base_url in TikTok config"),
            api_base_url: Url::parse(&cfg.api_base_url)
                .expect("Invalid api_base_url in TikTok config"),
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

pub(crate) async fn parse_response<T: DeserializeOwned>(
    response: Response,
) -> Result<T, TikTokError> {
    let status = response.status();
    let body = response.text().await?;

    if status.is_success() {
        return Ok(serde_json::from_str::<T>(&body)?);
    }

    Err(TikTokError::from_response_parts(status, body))
}

#[cfg(test)]
mod tests {
    use super::{TikTokClient, parse_response};
    use crate::config::TikTokConfig;
    use crate::tiktok::error::TikTokError;
    use serde::Deserialize;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
    fn new_normalizes_api_base_url_with_trailing_slash() {
        let mut cfg = test_config();
        cfg.api_base_url = "http://127.0.0.1:9999".to_string();

        let client = TikTokClient::new(&cfg);

        assert_eq!(client.api_base_url().as_str(), "http://127.0.0.1:9999/");
    }

    #[test]
    #[should_panic]
    fn new_panics_on_invalid_api_base_url() {
        let mut cfg = test_config();
        cfg.api_base_url = "not a url".to_string();

        let _ = TikTokClient::new(&cfg);
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Probe {
        hello: String,
    }

    async fn get(server: &MockServer) -> reqwest::Response {
        reqwest::get(format!("{}/probe", server.uri())).await.unwrap()
    }

    async fn mount(server: &MockServer, response: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(response)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn parse_200_ok_deserializes_body() {
        let server = MockServer::start().await;
        mount(
            &server,
            ResponseTemplate::new(200).set_body_string(r#"{"hello":"world"}"#),
        )
        .await;

        let probe: Probe = parse_response(get(&server).await).await.unwrap();
        assert_eq!(probe, Probe { hello: "world".into() });
    }

    #[tokio::test]
    async fn parse_401_maps_to_unauthorized() {
        let server = MockServer::start().await;
        mount(&server, ResponseTemplate::new(401)).await;

        let err = parse_response::<Probe>(get(&server).await).await.unwrap_err();
        assert!(matches!(err, TikTokError::Unauthorized));
    }

    #[tokio::test]
    async fn parse_429_maps_to_rate_limited() {
        let server = MockServer::start().await;
        mount(&server, ResponseTemplate::new(429)).await;

        let err = parse_response::<Probe>(get(&server).await).await.unwrap_err();
        assert!(matches!(err, TikTokError::RateLimited));
    }

    #[tokio::test]
    async fn parse_500_with_envelope_maps_to_api() {
        let server = MockServer::start().await;
        let body = r#"{"error":{"code":"internal_error","message":"boom","log_id":"abc"}}"#;
        mount(&server, ResponseTemplate::new(500).set_body_string(body)).await;

        let err = parse_response::<Probe>(get(&server).await).await.unwrap_err();
        match err {
            TikTokError::Api { code, message, log_id } => {
                assert_eq!(code, "internal_error");
                assert_eq!(message, "boom");
                assert_eq!(log_id, "abc");
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn parse_500_with_garbage_body_fallback() {
        let server = MockServer::start().await;
        let body = "x".repeat(1000);
        mount(&server, ResponseTemplate::new(500).set_body_string(body)).await;

        let err = parse_response::<Probe>(get(&server).await).await.unwrap_err();
        match err {
            TikTokError::Http { status, body } => {
                assert_eq!(status, 500);
                assert_eq!(body.chars().count(), 256);
            }
            other => panic!("expected Http fallback, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn parse_4xx_with_access_token_invalid_maps_to_unauthorized() {
        let server = MockServer::start().await;
        let body = r#"{"error":{"code":"access_token_invalid","message":"bad","log_id":"x"}}"#;
        mount(&server, ResponseTemplate::new(400).set_body_string(body)).await;

        let err = parse_response::<Probe>(get(&server).await).await.unwrap_err();
        assert!(matches!(err, TikTokError::Unauthorized));
    }

    #[tokio::test]
    async fn parse_200_invalid_json_maps_to_parse() {
        let server = MockServer::start().await;
        mount(&server, ResponseTemplate::new(200).set_body_string("not json")).await;

        let err = parse_response::<Probe>(get(&server).await).await.unwrap_err();
        assert!(matches!(err, TikTokError::Parse(_)));
    }
}
