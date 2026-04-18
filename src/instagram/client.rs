use std::time::Duration;

use serde::Deserialize;

use crate::config::InstagramConfig;

// ── Error type ───────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum IgClientError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Instagram API error: {message}")]
    Api { message: String },
}

// ── Graph API response DTOs ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MediaResponse {
    pub data: Vec<MediaItem>,
}

#[derive(Debug, Deserialize)]
pub struct MediaItem {
    pub id: String,
    #[serde(default)]
    pub like_count: u64,
    #[serde(default)]
    pub comments_count: u64,
}

/// Aggregated engagement data returned by [`IgClient::fetch_recent_media`].
#[derive(Debug)]
pub struct MediaInsights {
    /// Number of media items actually returned (≤ requested `limit`).
    pub media_count: u32,
    /// Sum of `like_count` across all returned media.
    pub total_likes: u64,
    /// Sum of `comments_count` across all returned media.
    pub total_comments: u64,
    /// `(total_likes + total_comments) / media_count` — average interactions
    /// per post.  `None` when `media_count` is zero.
    pub engagement_rate: Option<f64>,
}

// ── Client ───────────────────────────────────────────────────────────

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

    /// Fetch the most recent media items for `ig_user_id` and compute
    /// engagement metrics.
    ///
    /// The Instagram Graph API endpoint used is:
    /// `GET /{graph_api_version}/{ig_user_id}/media?fields=id,like_count,comments_count&limit={limit}`
    pub async fn fetch_recent_media(
        &self,
        token: &str,
        ig_user_id: &str,
        limit: u32,
    ) -> Result<MediaInsights, IgClientError> {
        let url = format!(
            "https://graph.instagram.com/{version}/{ig_user_id}/media",
            version = self.cfg.graph_api_version,
        );

        let resp = self
            .http
            .get(&url)
            .query(&[
                ("fields", "id,like_count,comments_count"),
                ("limit", &limit.to_string()),
                ("access_token", token),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(IgClientError::Api { message: body });
        }

        let media_resp: MediaResponse = resp.json().await?;

        Ok(compute_insights(&media_resp.data))
    }
}

// ── Pure computation (easy to unit-test) ─────────────────────────────

fn compute_insights(items: &[MediaItem]) -> MediaInsights {
    let media_count = items.len() as u32;
    let total_likes: u64 = items.iter().map(|m| m.like_count).sum();
    let total_comments: u64 = items.iter().map(|m| m.comments_count).sum();

    let engagement_rate = if media_count > 0 {
        Some((total_likes + total_comments) as f64 / media_count as f64)
    } else {
        None
    };

    MediaInsights {
        media_count,
        total_likes,
        total_comments,
        engagement_rate,
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_insights_with_items() {
        let items = vec![
            MediaItem { id: "1".into(), like_count: 100, comments_count: 10 },
            MediaItem { id: "2".into(), like_count: 200, comments_count: 20 },
            MediaItem { id: "3".into(), like_count: 300, comments_count: 30 },
        ];

        let insights = compute_insights(&items);

        assert_eq!(insights.media_count, 3);
        assert_eq!(insights.total_likes, 600);
        assert_eq!(insights.total_comments, 60);
        // (600 + 60) / 3 = 220.0
        assert!((insights.engagement_rate.unwrap() - 220.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compute_insights_empty() {
        let insights = compute_insights(&[]);

        assert_eq!(insights.media_count, 0);
        assert_eq!(insights.total_likes, 0);
        assert_eq!(insights.total_comments, 0);
        assert!(insights.engagement_rate.is_none());
    }

    #[test]
    fn compute_insights_single_item_zero_engagement() {
        let items = vec![MediaItem { id: "1".into(), like_count: 0, comments_count: 0 }];

        let insights = compute_insights(&items);

        assert_eq!(insights.media_count, 1);
        assert_eq!(insights.total_likes, 0);
        assert_eq!(insights.total_comments, 0);
        assert!((insights.engagement_rate.unwrap() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn deserialize_media_response() {
        let json = r#"{
            "data": [
                { "id": "123", "like_count": 42, "comments_count": 5 },
                { "id": "456" }
            ]
        }"#;

        let resp: MediaResponse = serde_json::from_str(json).unwrap();

        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[0].like_count, 42);
        assert_eq!(resp.data[0].comments_count, 5);
        // Missing fields default to 0
        assert_eq!(resp.data[1].like_count, 0);
        assert_eq!(resp.data[1].comments_count, 0);
    }
}
