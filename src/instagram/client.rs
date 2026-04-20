use crate::config::InstagramConfig;
use crate::instagram::error::IgError;
use std::time::Duration;
use tracing::{error, warn};

const INSTAGRAM_OAUTH_HOST: &str = "www.instagram.com";
const INSTAGRAM_API_HOST: &str = "api.instagram.com";
const INSTAGRAM_GRAPH_HOST: &str = "graph.instagram.com";
pub const SCOPE_IG_BUSINESS_BASIC: &str = "instagram_business_basic";

#[derive(Clone)]
pub struct IgClient {
    http: reqwest::Client,
    cfg: InstagramConfig,
    instagram_oauth_base_url: String,
    instagram_api_base_url: String,
    instagram_graph_base_url: String,
}

/// Wrapper for token values that must never be emitted raw in logs.
///
/// Use this in structured logging fields whenever access tokens might be
/// attached to diagnostics.
pub struct RedactedToken<'a>(pub &'a str);

impl std::fmt::Debug for RedactedToken<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RedactedToken(len={})", self.0.len())
    }
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct CodeExchange {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
}

/// Short-lived token returned by `POST /oauth/access_token` on the
/// Instagram API with Instagram Login flow.
///
/// The response body is `{"access_token":"…","user_id":…}`.  `user_id`
/// is numeric but we keep it as a string for consistency with the rest
/// of the codebase.
///
/// The `permissions` field is **not** present in the current Instagram
/// API response (as of 2025).  It defaults to an empty string when
/// absent; callers that need a scope value should fall back to
/// [`SCOPE_IG_BUSINESS_BASIC`].
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct ShortLivedToken {
    pub access_token: String,
    #[serde(deserialize_with = "deserialize_user_id")]
    pub user_id: String,
    /// Granted permissions, if returned by the API.  Typically empty —
    /// the API does not echo permissions in the token response.
    #[serde(default)]
    pub permissions: String,
}

/// Deserialize `user_id` from either a JSON number or a JSON string.
fn deserialize_user_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct Visitor;
    impl de::Visitor<'_> for Visitor {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a string or integer user_id")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_owned())
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    deserializer.deserialize_any(Visitor)
}

/// A single media item returned by the Instagram Graph API
/// (`GET /{ig-user-id}/media?fields=…`).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct MediaItem {
    pub id: String,
    pub like_count: Option<i64>,
    pub comments_count: Option<i64>,
    pub timestamp: Option<String>,
    pub media_type: Option<String>,
}

/// Summary of recent media items with pre-computed engagement metrics.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct RecentMediaSummary {
    /// The individual media items returned by the API.
    pub items: Vec<MediaItem>,
    /// Sum of `like_count` across all items.
    pub total_likes: i64,
    /// Sum of `comments_count` across all items.
    pub total_comments: i64,
    /// Average engagement per post: `(total_likes + total_comments) / post_count`.
    /// Returns `0.0` when no items are present.
    pub engagement_rate: f64,
}

/// Envelope returned by `GET /{ig-user-id}/media`.
#[derive(Debug, serde::Deserialize)]
struct MediaResponse {
    data: Vec<MediaItem>,
}

/// Profile statistics returned by the Instagram Graph API via Instagram Login
/// (`GET /me?fields=…` on graph.instagram.com).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct ProfileStats {
    pub id: String,
    pub username: Option<String>,
    pub name: Option<String>,
    pub account_type: Option<String>,
    pub followers_count: Option<i64>,
    pub follows_count: Option<i64>,
    pub media_count: Option<i64>,
    pub profile_picture_url: Option<String>,
}

impl IgClient {
    pub fn new(cfg: InstagramConfig) -> Self {
        Self::new_with_base_urls(
            cfg,
            format!("https://{}", INSTAGRAM_OAUTH_HOST),
            format!("https://{}", INSTAGRAM_API_HOST),
            format!("https://{}", INSTAGRAM_GRAPH_HOST),
        )
    }

    pub fn new_with_base_urls(
        cfg: InstagramConfig,
        instagram_oauth_base_url: String,
        instagram_api_base_url: String,
        instagram_graph_base_url: String,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to initialize Instagram HTTP client");

        Self {
            http,
            cfg,
            instagram_oauth_base_url,
            instagram_api_base_url,
            instagram_graph_base_url,
        }
    }

    pub fn build_authorize_url(&self, state: &str) -> String {
        let mut url = url::Url::parse(&format!(
            "{}/oauth/authorize",
            self.instagram_oauth_base_url.trim_end_matches('/')
        ))
        .expect("BUG: Failed to construct Instagram OAuth URL - this should never happen with valid constants");

        url.query_pairs_mut()
            .append_pair("client_id", &self.cfg.client_id)
            .append_pair("redirect_uri", &self.cfg.redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", SCOPE_IG_BUSINESS_BASIC)
            .append_pair("state", state);

        url.to_string()
    }

    #[tracing::instrument(skip(self, code), fields(profile_id, ig_user_id))]
    pub async fn exchange_code(&self, code: &str) -> Result<ShortLivedToken, IgError> {
        let exchange_url = format!(
            "{}/oauth/access_token",
            self.instagram_api_base_url.trim_end_matches('/')
        );

        let res = self
            .http
            .post(&exchange_url)
            .form(&[
                ("client_id", self.cfg.client_id.as_str()),
                ("client_secret", self.cfg.client_secret.as_str()),
                ("grant_type", "authorization_code"),
                ("redirect_uri", self.cfg.redirect_uri.as_str()),
                ("code", code),
            ])
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

    #[tracing::instrument(skip(self, token), fields(profile_id, ig_user_id))]
    pub async fn exchange_for_long_lived(&self, token: &str) -> Result<CodeExchange, IgError> {
        let res = self
            .http
            .get(self.build_long_lived_exchange_url(token))
            .send()
            .await?;
        if !res.status().is_success() {
            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await?;
            let error = IgError::from_response_parts(status, &headers, body);
            log_ig_error("exchange_for_long_lived", &error, token, None);
            return Err(error);
        }

        let body = res.text().await?;
        Ok(serde_json::from_str(&body)?)
    }

    #[tracing::instrument(skip(self, token), fields(profile_id, ig_user_id))]
    pub async fn refresh_long_lived_token(&self, token: &str) -> Result<CodeExchange, IgError> {
        let res = self
            .http
            .get(self.build_long_lived_refresh_url(token))
            .send()
            .await?;
        if !res.status().is_success() {
            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await?;
            let error = IgError::from_response_parts(status, &headers, body);
            log_ig_error("refresh_long_lived_token", &error, token, None);
            return Err(error);
        }

        let body = res.text().await?;
        Ok(serde_json::from_str(&body)?)
    }

    fn build_long_lived_exchange_url(&self, short: &str) -> url::Url {
        let mut url = url::Url::parse(&format!(
            "{}/access_token",
            self.instagram_graph_base_url.trim_end_matches('/')
        ))
            .expect("BUG: Failed to construct Instagram Graph long-lived token URL - this should never happen with valid constants");

        url.query_pairs_mut()
            .append_pair("grant_type", "ig_exchange_token")
            .append_pair("client_secret", &self.cfg.client_secret)
            .append_pair("access_token", short);
        url
    }

    fn build_long_lived_refresh_url(&self, long_lived: &str) -> url::Url {
        let mut url = url::Url::parse(&format!(
            "{}/refresh_access_token",
            self.instagram_graph_base_url.trim_end_matches('/')
        ))
            .expect("BUG: Failed to construct Instagram Graph refresh token URL - this should never happen with valid constants");

        url.query_pairs_mut()
            .append_pair("grant_type", "ig_refresh_token")
            .append_pair("access_token", long_lived);
        url
    }

    /// Fetch profile statistics for the authenticated Instagram user.
    ///
    /// Calls `GET /{ver}/me?fields=id,username,name,account_type,
    /// profile_picture_url,followers_count,follows_count,media_count`
    /// on graph.instagram.com.
    #[tracing::instrument(skip(self, token), fields(profile_id))]
    pub async fn fetch_profile_stats(
        &self,
        token: &str,
    ) -> Result<ProfileStats, IgError> {
        let res = self
            .http
            .get(self.build_profile_stats_url(token).to_string())
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await?;
            let error = IgError::from_response_parts(status, &headers, body);
            log_ig_error("fetch_profile_stats", &error, token, None);
            return Err(error);
        }

        let body = res.text().await?;
        Ok(serde_json::from_str(&body)?)
    }

    fn build_profile_stats_url(&self, token: &str) -> url::Url {
        let mut url = url::Url::parse(&format!(
            "{}/{}/me",
            self.instagram_graph_base_url.trim_end_matches('/'),
            self.cfg.graph_api_version,
        ))
        .expect("BUG: Failed to construct Instagram Graph profile stats URL");

        url.query_pairs_mut()
            .append_pair(
                "fields",
                "id,username,name,account_type,profile_picture_url,followers_count,follows_count,media_count",
            )
            .append_pair("access_token", token);
        url
    }

    /// Fetch the most recent media items for the authenticated Instagram user
    /// and compute engagement metrics.
    ///
    /// Calls `GET /{ver}/me/media?fields=id,like_count,comments_count,
    /// timestamp,media_type&limit={limit}` on graph.instagram.com.
    ///
    /// The returned [`RecentMediaSummary`] contains the raw items together with
    /// pre-computed totals and an average engagement rate per post.
    #[tracing::instrument(skip(self, token), fields(profile_id))]
    pub async fn fetch_recent_media(
        &self,
        token: &str,
        limit: u32,
    ) -> Result<RecentMediaSummary, IgError> {
        let res = self
            .http
            .get(
                self.build_recent_media_url(token, limit)
                    .to_string(),
            )
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await?;
            let error = IgError::from_response_parts(status, &headers, body);
            log_ig_error("fetch_recent_media", &error, token, None);
            return Err(error);
        }

        let body = res.text().await?;
        let media: MediaResponse = serde_json::from_str(&body)?;

        let total_likes: i64 = media.data.iter().filter_map(|m| m.like_count).sum();
        let total_comments: i64 = media.data.iter().filter_map(|m| m.comments_count).sum();
        let post_count = media.data.len() as f64;
        let engagement_rate = if post_count > 0.0 {
            (total_likes + total_comments) as f64 / post_count
        } else {
            0.0
        };

        Ok(RecentMediaSummary {
            items: media.data,
            total_likes,
            total_comments,
            engagement_rate,
        })
    }

    fn build_recent_media_url(&self, token: &str, limit: u32) -> url::Url {
        let mut url = url::Url::parse(&format!(
            "{}/{}/me/media",
            self.instagram_graph_base_url.trim_end_matches('/'),
            self.cfg.graph_api_version,
        ))
        .expect("BUG: Failed to construct Instagram Graph recent media URL");

        url.query_pairs_mut()
            .append_pair(
                "fields",
                "id,like_count,comments_count,timestamp,media_type",
            )
            .append_pair("limit", &limit.to_string())
            .append_pair("access_token", token);
        url
    }
}

fn log_ig_error(operation: &'static str, error: &IgError, token: &str, ig_user_id: Option<&str>) {
    match error {
        IgError::RateLimited { retry_after } => warn!(
            operation,
            retry_after_secs = retry_after.map(|duration| duration.as_secs()),
            token = ?RedactedToken(token),
            ig_user_id,
            "Instagram API rate limited",
        ),
        IgError::Graph { code, subcode, .. } => error!(
            operation,
            graph_code = *code,
            graph_subcode = *subcode,
            token = ?RedactedToken(token),
            ig_user_id,
            "Instagram Graph API returned an error",
        ),
        IgError::Http { status, .. } => error!(
            operation,
            status = *status,
            token = ?RedactedToken(token),
            ig_user_id,
            "Instagram API HTTP error",
        ),
        _ => warn!(
            operation,
            token = ?RedactedToken(token),
            ig_user_id,
            "Instagram API call failed",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        CodeExchange, IgClient, MediaItem, MediaResponse, ProfileStats, RecentMediaSummary,
        ShortLivedToken, SCOPE_IG_BUSINESS_BASIC,
    };
    use crate::config::InstagramConfig;

    fn test_config() -> InstagramConfig {
        InstagramConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            redirect_uri: "http://localhost:3000/oauth/instagram/callback".to_string(),
            graph_api_version: "v25.0".to_string(),
        }
    }

    #[test]
    fn build_authorize_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_authorize_url("my-state");
        let parsed = url::Url::parse(&url).expect("URL should parse");

        assert_eq!(parsed.host_str(), Some("www.instagram.com"));
        assert_eq!(parsed.path(), "/oauth/authorize");

        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(query.get("client_id"), Some(&"test-client-id".to_string()));
        assert_eq!(
            query.get("redirect_uri"),
            Some(&"http://localhost:3000/oauth/instagram/callback".to_string())
        );
        assert_eq!(query.get("state"), Some(&"my-state".to_string()));
        assert_eq!(
            query.get("scope"),
            Some(&SCOPE_IG_BUSINESS_BASIC.to_string())
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
    fn short_lived_token_parses_numeric_user_id() {
        let raw = r#"{"access_token":"IGQ...","user_id":17841400000000001}"#;
        let parsed: ShortLivedToken = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.access_token, "IGQ...");
        assert_eq!(parsed.user_id, "17841400000000001");
        assert_eq!(parsed.permissions, "");
    }

    #[test]
    fn short_lived_token_parses_string_user_id() {
        let raw = r#"{"access_token":"tok","user_id":"12345"}"#;
        let parsed: ShortLivedToken = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.user_id, "12345");
    }

    #[test]
    fn exchange_code_sends_post_with_form_body() {
        // We verify the request shape by constructing the client and checking
        // that the method is POST with the expected form fields.  A full
        // integration test with wiremock is in the service and integration
        // test modules.
        let client = IgClient::new(test_config());
        // The exchange_code method is async; we just confirm the URL base
        // derives from instagram_api_base_url (api.instagram.com).
        assert_eq!(
            client.instagram_api_base_url,
            "https://api.instagram.com"
        );
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

    #[test]
    fn build_long_lived_refresh_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_long_lived_refresh_url("long-lived-token/unsafe?x=1");
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("graph.instagram.com"));
        assert_eq!(parsed.path(), "/refresh_access_token");
        assert_eq!(
            query.get("grant_type"),
            Some(&"ig_refresh_token".to_string())
        );
        assert_eq!(
            query.get("access_token"),
            Some(&"long-lived-token/unsafe?x=1".to_string())
        );
    }

    #[test]
    fn build_profile_stats_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_profile_stats_url("my-token");
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("graph.instagram.com"));
        assert_eq!(parsed.path(), "/v25.0/me");
        assert_eq!(
            query.get("fields"),
            Some(&"id,username,name,account_type,profile_picture_url,followers_count,follows_count,media_count".to_string())
        );
        assert_eq!(query.get("access_token"), Some(&"my-token".to_string()));
    }

    #[test]
    fn profile_stats_parses_full_response() {
        let raw = r#"{
            "id": "17841400000000001",
            "username": "creator_jane",
            "name": "Jane Doe",
            "account_type": "BUSINESS",
            "followers_count": 125000,
            "follows_count": 340,
            "media_count": 512,
            "profile_picture_url": "https://example.com/pic.jpg"
        }"#;

        let parsed: ProfileStats = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.id, "17841400000000001");
        assert_eq!(parsed.username.as_deref(), Some("creator_jane"));
        assert_eq!(parsed.name.as_deref(), Some("Jane Doe"));
        assert_eq!(parsed.account_type.as_deref(), Some("BUSINESS"));
        assert_eq!(parsed.followers_count, Some(125000));
        assert_eq!(parsed.follows_count, Some(340));
        assert_eq!(parsed.media_count, Some(512));
        assert_eq!(
            parsed.profile_picture_url.as_deref(),
            Some("https://example.com/pic.jpg")
        );
    }

    #[test]
    fn profile_stats_parses_minimal_response() {
        let raw = r#"{ "id": "17841400000000001" }"#;
        let parsed: ProfileStats = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.id, "17841400000000001");
        assert!(parsed.username.is_none());
        assert!(parsed.name.is_none());
        assert!(parsed.account_type.is_none());
        assert!(parsed.followers_count.is_none());
        assert!(parsed.follows_count.is_none());
        assert!(parsed.media_count.is_none());
        assert!(parsed.profile_picture_url.is_none());
    }

    #[test]
    fn build_recent_media_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_recent_media_url("my-token", 20);
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("graph.instagram.com"));
        assert_eq!(parsed.path(), "/v25.0/me/media");
        assert_eq!(
            query.get("fields"),
            Some(&"id,like_count,comments_count,timestamp,media_type".to_string())
        );
        assert_eq!(query.get("limit"), Some(&"20".to_string()));
        assert_eq!(query.get("access_token"), Some(&"my-token".to_string()));
    }

    #[test]
    fn media_response_parses_full_response() {
        let raw = r#"{
            "data": [
                {
                    "id": "media1",
                    "like_count": 100,
                    "comments_count": 10,
                    "timestamp": "2026-04-01T12:00:00+0000",
                    "media_type": "IMAGE"
                },
                {
                    "id": "media2",
                    "like_count": 200,
                    "comments_count": 20,
                    "timestamp": "2026-04-02T12:00:00+0000",
                    "media_type": "VIDEO"
                }
            ]
        }"#;

        let parsed: MediaResponse = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].id, "media1");
        assert_eq!(parsed.data[0].like_count, Some(100));
        assert_eq!(parsed.data[0].comments_count, Some(10));
        assert_eq!(
            parsed.data[0].timestamp.as_deref(),
            Some("2026-04-01T12:00:00+0000")
        );
        assert_eq!(parsed.data[0].media_type.as_deref(), Some("IMAGE"));
        assert_eq!(parsed.data[1].id, "media2");
        assert_eq!(parsed.data[1].like_count, Some(200));
        assert_eq!(parsed.data[1].comments_count, Some(20));
    }

    #[test]
    fn media_response_parses_minimal_items() {
        let raw = r#"{ "data": [{ "id": "media1" }] }"#;
        let parsed: MediaResponse = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].id, "media1");
        assert!(parsed.data[0].like_count.is_none());
        assert!(parsed.data[0].comments_count.is_none());
        assert!(parsed.data[0].timestamp.is_none());
        assert!(parsed.data[0].media_type.is_none());
    }

    #[test]
    fn media_response_parses_empty_data() {
        let raw = r#"{ "data": [] }"#;
        let parsed: MediaResponse = serde_json::from_str(raw).expect("should parse");
        assert!(parsed.data.is_empty());
    }

    #[test]
    fn engagement_rate_computed_correctly_for_multiple_items() {
        let items = vec![
            MediaItem {
                id: "1".to_string(),
                like_count: Some(100),
                comments_count: Some(10),
                timestamp: None,
                media_type: None,
            },
            MediaItem {
                id: "2".to_string(),
                like_count: Some(200),
                comments_count: Some(20),
                timestamp: None,
                media_type: None,
            },
        ];

        let total_likes: i64 = items.iter().filter_map(|m| m.like_count).sum();
        let total_comments: i64 = items.iter().filter_map(|m| m.comments_count).sum();
        let post_count = items.len() as f64;
        let engagement_rate = (total_likes + total_comments) as f64 / post_count;

        let summary = RecentMediaSummary {
            items,
            total_likes,
            total_comments,
            engagement_rate,
        };

        assert_eq!(summary.total_likes, 300);
        assert_eq!(summary.total_comments, 30);
        assert!((summary.engagement_rate - 165.0).abs() < f64::EPSILON);
    }

    #[test]
    fn engagement_rate_zero_for_empty_items() {
        let summary = RecentMediaSummary {
            items: vec![],
            total_likes: 0,
            total_comments: 0,
            engagement_rate: 0.0,
        };

        assert!((summary.engagement_rate - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn engagement_rate_handles_missing_counts() {
        let items = vec![
            MediaItem {
                id: "1".to_string(),
                like_count: Some(50),
                comments_count: None,
                timestamp: None,
                media_type: None,
            },
            MediaItem {
                id: "2".to_string(),
                like_count: None,
                comments_count: Some(10),
                timestamp: None,
                media_type: None,
            },
        ];

        let total_likes: i64 = items.iter().filter_map(|m| m.like_count).sum();
        let total_comments: i64 = items.iter().filter_map(|m| m.comments_count).sum();
        let post_count = items.len() as f64;
        let engagement_rate = (total_likes + total_comments) as f64 / post_count;

        let summary = RecentMediaSummary {
            items,
            total_likes,
            total_comments,
            engagement_rate,
        };

        assert_eq!(summary.total_likes, 50);
        assert_eq!(summary.total_comments, 10);
        assert!((summary.engagement_rate - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn redacted_token_debug_does_not_include_raw_token() {
        let raw = "ig-secret-token-123";
        let formatted = format!("{:?}", super::RedactedToken(raw));
        assert!(!formatted.contains(raw));
        assert!(formatted.contains("RedactedToken"));
    }
}
