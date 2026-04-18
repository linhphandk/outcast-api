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

#[derive(Clone)]
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

/// Envelope returned by `GET /me/accounts`.
#[derive(Debug, serde::Deserialize)]
struct PagesResponse {
    data: Vec<PageData>,
    paging: Option<Paging>,
}

#[derive(Debug, serde::Deserialize)]
struct PageData {
    id: String,
}

#[derive(Debug, serde::Deserialize)]
struct Paging {
    next: Option<String>,
}

/// Envelope returned by `GET /{page_id}?fields=instagram_business_account`.
#[derive(Debug, serde::Deserialize)]
struct IgBusinessAccountResponse {
    instagram_business_account: Option<IgBusinessAccount>,
}

#[derive(Debug, serde::Deserialize)]
struct IgBusinessAccount {
    id: String,
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

/// Profile statistics returned by the Instagram Graph API for a Business or
/// Creator account (`GET /{ig-user-id}?fields=…`).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct ProfileStats {
    pub id: String,
    pub username: Option<String>,
    pub name: Option<String>,
    pub biography: Option<String>,
    pub followers_count: Option<i64>,
    pub follows_count: Option<i64>,
    pub media_count: Option<i64>,
    pub profile_picture_url: Option<String>,
    pub website: Option<String>,
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

    /// Fetch the Instagram Business Account ID connected to one of the user's
    /// Facebook Pages.
    ///
    /// Walks `GET /me/accounts` (following pagination) and, for every page
    /// returned, queries `GET /{page_id}?fields=instagram_business_account`.
    /// Returns the first connected IG Business Account ID found, or
    /// `IgError::NoBusinessAccount` when none of the pages have one.
    pub async fn fetch_business_account(&self, token: &str) -> Result<String, IgError> {
        let mut next_url: Option<String> = Some(self.build_pages_url(token).to_string());

        while let Some(url) = next_url.take() {
            let res = self.http.get(&url).send().await?;
            if !res.status().is_success() {
                let status = res.status();
                let headers = res.headers().clone();
                let body = res.text().await?;
                return Err(IgError::from_response_parts(status, &headers, body));
            }

            let body = res.text().await?;
            let pages: PagesResponse = serde_json::from_str(&body)?;

            for page in &pages.data {
                if let Some(ig_id) = self.fetch_page_ig_account(token, &page.id).await? {
                    return Ok(ig_id);
                }
            }

            next_url = pages.paging.and_then(|p| p.next);
        }

        Err(IgError::NoBusinessAccount)
    }

    fn build_pages_url(&self, token: &str) -> url::Url {
        let mut url = url::Url::parse(&format!(
            "https://{}/{}/me/accounts",
            FACEBOOK_GRAPH_HOST, self.cfg.graph_api_version
        ))
        .expect("BUG: Failed to construct Facebook Graph pages URL");

        url.query_pairs_mut()
            .append_pair("access_token", token);
        url
    }

    fn build_page_ig_url(&self, token: &str, page_id: &str) -> url::Url {
        let mut url = url::Url::parse(&format!(
            "https://{}/{}/{}",
            FACEBOOK_GRAPH_HOST, self.cfg.graph_api_version, page_id
        ))
        .expect("BUG: Failed to construct Facebook Graph page IG URL");

        url.query_pairs_mut()
            .append_pair("fields", "instagram_business_account")
            .append_pair("access_token", token);
        url
    }

    /// Query a single Facebook Page for its connected IG Business Account.
    async fn fetch_page_ig_account(
        &self,
        token: &str,
        page_id: &str,
    ) -> Result<Option<String>, IgError> {
        let res = self
            .http
            .get(self.build_page_ig_url(token, page_id).to_string())
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await?;
            return Err(IgError::from_response_parts(status, &headers, body));
        }

        let body = res.text().await?;
        let parsed: IgBusinessAccountResponse = serde_json::from_str(&body)?;
        Ok(parsed.instagram_business_account.map(|acct| acct.id))
    }

    /// Fetch public-ish profile statistics for an Instagram Business /
    /// Creator account.
    ///
    /// Calls `GET /{ig_user_id}?fields=username,name,biography,
    /// followers_count,follows_count,media_count,profile_picture_url,website`
    /// on the Facebook Graph API.
    pub async fn fetch_profile_stats(
        &self,
        token: &str,
        ig_user_id: &str,
    ) -> Result<ProfileStats, IgError> {
        let res = self
            .http
            .get(self.build_profile_stats_url(token, ig_user_id).to_string())
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

    fn build_profile_stats_url(&self, token: &str, ig_user_id: &str) -> url::Url {
        let mut url = url::Url::parse(&format!(
            "https://{}/{}/{}",
            FACEBOOK_GRAPH_HOST, self.cfg.graph_api_version, ig_user_id
        ))
        .expect("BUG: Failed to construct Facebook Graph profile stats URL");

        url.query_pairs_mut()
            .append_pair(
                "fields",
                "username,name,biography,followers_count,follows_count,media_count,profile_picture_url,website",
            )
            .append_pair("access_token", token);
        url
    }

    /// Fetch the most recent media items for an Instagram Business / Creator
    /// account and compute engagement metrics.
    ///
    /// Calls `GET /{ig_user_id}/media?fields=id,like_count,comments_count,
    /// timestamp,media_type&limit={limit}` on the Facebook Graph API.
    ///
    /// The returned [`RecentMediaSummary`] contains the raw items together with
    /// pre-computed totals and an average engagement rate per post.
    pub async fn fetch_recent_media(
        &self,
        token: &str,
        ig_user_id: &str,
        limit: u32,
    ) -> Result<RecentMediaSummary, IgError> {
        let res = self
            .http
            .get(self.build_recent_media_url(token, ig_user_id, limit).to_string())
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await?;
            return Err(IgError::from_response_parts(status, &headers, body));
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

    /// Refresh a long-lived Instagram Graph access token before it expires.
    ///
    /// Calls `GET /refresh_access_token?grant_type=ig_refresh_token&access_token={token}`
    /// on the Instagram Graph API. Long-lived tokens expire after ~60 days and must
    /// be refreshed while they are still valid.
    ///
    /// Returns a new [`CodeExchange`] containing the refreshed token and its new
    /// `expires_in` (≈ 5 184 000 seconds / 60 days).
    ///
    /// On a `401` response the caller should treat the stored token as invalid and
    /// clear it from the database.
    pub async fn refresh_long_lived_token(&self, long_lived: &str) -> Result<CodeExchange, IgError> {
        let res = self
            .http
            .get(self.build_refresh_url(long_lived))
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

    fn build_refresh_url(&self, long_lived: &str) -> url::Url {
        let mut url =
            url::Url::parse(&format!("https://{}/refresh_access_token", INSTAGRAM_GRAPH_HOST))
                .expect("BUG: Failed to construct Instagram Graph refresh token URL - this should never happen with valid constants");

        url.query_pairs_mut()
            .append_pair("grant_type", "ig_refresh_token")
            .append_pair("access_token", long_lived);
        url
    }

    fn build_recent_media_url(
        &self,
        token: &str,
        ig_user_id: &str,
        limit: u32,
    ) -> url::Url {
        let mut url = url::Url::parse(&format!(
            "https://{}/{}/{}/media",
            FACEBOOK_GRAPH_HOST, self.cfg.graph_api_version, ig_user_id
        ))
        .expect("BUG: Failed to construct Facebook Graph recent media URL");

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        CodeExchange, IgBusinessAccountResponse, IgClient, MediaItem, MediaResponse,
        PagesResponse, ProfileStats, RecentMediaSummary, INSTAGRAM_GRAPH_HOST,
        SCOPE_BUSINESS_MANAGEMENT, SCOPE_INSTAGRAM_BASIC, SCOPE_INSTAGRAM_MANAGE_INSIGHTS,
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

    #[test]
    fn build_pages_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_pages_url("my-token/unsafe?x=1");
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("graph.facebook.com"));
        assert_eq!(parsed.path(), "/v19.0/me/accounts");
        assert_eq!(
            query.get("access_token"),
            Some(&"my-token/unsafe?x=1".to_string())
        );
    }

    #[test]
    fn build_page_ig_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_page_ig_url("my-token", "123456789");
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("graph.facebook.com"));
        assert_eq!(parsed.path(), "/v19.0/123456789");
        assert_eq!(
            query.get("fields"),
            Some(&"instagram_business_account".to_string())
        );
        assert_eq!(
            query.get("access_token"),
            Some(&"my-token".to_string())
        );
    }

    #[test]
    fn pages_response_parses_with_data_and_next() {
        let raw = r#"{
            "data": [
                { "id": "111" },
                { "id": "222" }
            ],
            "paging": {
                "next": "https://graph.facebook.com/v19.0/me/accounts?after=abc"
            }
        }"#;

        let parsed: PagesResponse = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].id, "111");
        assert_eq!(parsed.data[1].id, "222");
        assert_eq!(
            parsed.paging.unwrap().next.unwrap(),
            "https://graph.facebook.com/v19.0/me/accounts?after=abc"
        );
    }

    #[test]
    fn pages_response_parses_without_paging() {
        let raw = r#"{ "data": [{ "id": "333" }] }"#;
        let parsed: PagesResponse = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.data.len(), 1);
        assert!(parsed.paging.is_none());
    }

    #[test]
    fn ig_business_account_response_parses_with_account() {
        let raw = r#"{
            "instagram_business_account": { "id": "17841400000000001" },
            "id": "111"
        }"#;

        let parsed: IgBusinessAccountResponse = serde_json::from_str(raw).expect("should parse");
        assert_eq!(
            parsed.instagram_business_account.unwrap().id,
            "17841400000000001"
        );
    }

    #[test]
    fn ig_business_account_response_parses_without_account() {
        let raw = r#"{ "id": "111" }"#;
        let parsed: IgBusinessAccountResponse = serde_json::from_str(raw).expect("should parse");
        assert!(parsed.instagram_business_account.is_none());
    }

    #[test]
    fn build_profile_stats_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_profile_stats_url("my-token", "17841400000000001");
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("graph.facebook.com"));
        assert_eq!(parsed.path(), "/v19.0/17841400000000001");
        assert_eq!(
            query.get("fields"),
            Some(&"username,name,biography,followers_count,follows_count,media_count,profile_picture_url,website".to_string())
        );
        assert_eq!(
            query.get("access_token"),
            Some(&"my-token".to_string())
        );
    }

    #[test]
    fn profile_stats_parses_full_response() {
        let raw = r#"{
            "id": "17841400000000001",
            "username": "creator_jane",
            "name": "Jane Doe",
            "biography": "Content creator & photographer",
            "followers_count": 125000,
            "follows_count": 340,
            "media_count": 512,
            "profile_picture_url": "https://example.com/pic.jpg",
            "website": "https://janedoe.com"
        }"#;

        let parsed: ProfileStats = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.id, "17841400000000001");
        assert_eq!(parsed.username.as_deref(), Some("creator_jane"));
        assert_eq!(parsed.name.as_deref(), Some("Jane Doe"));
        assert_eq!(
            parsed.biography.as_deref(),
            Some("Content creator & photographer")
        );
        assert_eq!(parsed.followers_count, Some(125000));
        assert_eq!(parsed.follows_count, Some(340));
        assert_eq!(parsed.media_count, Some(512));
        assert_eq!(
            parsed.profile_picture_url.as_deref(),
            Some("https://example.com/pic.jpg")
        );
        assert_eq!(parsed.website.as_deref(), Some("https://janedoe.com"));
    }

    #[test]
    fn profile_stats_parses_minimal_response() {
        let raw = r#"{ "id": "17841400000000001" }"#;
        let parsed: ProfileStats = serde_json::from_str(raw).expect("should parse");
        assert_eq!(parsed.id, "17841400000000001");
        assert!(parsed.username.is_none());
        assert!(parsed.name.is_none());
        assert!(parsed.biography.is_none());
        assert!(parsed.followers_count.is_none());
        assert!(parsed.follows_count.is_none());
        assert!(parsed.media_count.is_none());
        assert!(parsed.profile_picture_url.is_none());
        assert!(parsed.website.is_none());
    }

    #[test]
    fn build_recent_media_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_recent_media_url("my-token", "17841400000000001", 20);
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some("graph.facebook.com"));
        assert_eq!(parsed.path(), "/v19.0/17841400000000001/media");
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
        assert_eq!(parsed.data[0].timestamp.as_deref(), Some("2026-04-01T12:00:00+0000"));
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
    fn build_refresh_url_has_expected_host_path_and_query_params() {
        let client = IgClient::new(test_config());
        let url = client.build_refresh_url("long-lived-token/unsafe?x=1");
        let parsed = url::Url::parse(url.as_ref()).expect("URL should parse");
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.host_str(), Some(INSTAGRAM_GRAPH_HOST));
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
    fn refresh_response_parses_with_expires_in() {
        let raw = r#"{
            "access_token": "refreshed-token-abc",
            "token_type": "bearer",
            "expires_in": 5183944
        }"#;

        let parsed: CodeExchange = serde_json::from_str(raw).expect("response should parse");
        assert_eq!(parsed.access_token, "refreshed-token-abc");
        assert_eq!(parsed.token_type, "bearer");
        assert_eq!(parsed.expires_in, Some(5183944));
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
}
