use reqwest::StatusCode;
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum TikTokError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("rate limited")]
    RateLimited,
    #[error("TikTok API error {code}: {message}")]
    Api {
        code: String,
        message: String,
        log_id: String,
    },
    #[error("HTTP error: {status} {body}")]
    Http { status: u16, body: String },
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    #[error(transparent)]
    Parse(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
pub(crate) struct TikTokErrorEnvelope {
    pub error: TikTokErrorBody,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TikTokErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub log_id: String,
}

impl TikTokError {
    pub(crate) fn from_response_parts(status: StatusCode, body: String) -> Self {
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Self::Unauthorized,
            StatusCode::TOO_MANY_REQUESTS => Self::RateLimited,
            _ => {
                if let Ok(envelope) = serde_json::from_str::<TikTokErrorEnvelope>(&body) {
                    return classify(envelope.error);
                }

                let truncated: String = body.chars().take(256).collect();
                Self::Http {
                    status: status.as_u16(),
                    body: truncated,
                }
            }
        }
    }
}

fn classify(err: TikTokErrorBody) -> TikTokError {
    match err.code.as_str() {
        "access_token_invalid" | "access_token_expired" => TikTokError::Unauthorized,
        "rate_limit_exceeded" => TikTokError::RateLimited,
        _ => TikTokError::Api {
            code: err.code,
            message: err.message,
            log_id: err.log_id,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::TikTokError;
    use reqwest::StatusCode;

    #[test]
    fn maps_401_to_unauthorized() {
        let error = TikTokError::from_response_parts(StatusCode::UNAUTHORIZED, String::new());
        assert!(matches!(error, TikTokError::Unauthorized));
    }

    #[test]
    fn maps_429_to_rate_limited() {
        let error = TikTokError::from_response_parts(StatusCode::TOO_MANY_REQUESTS, String::new());
        assert!(matches!(error, TikTokError::RateLimited));
    }

    #[test]
    fn maps_error_envelope_to_api_variant() {
        let body = r#"{
            "error": {
                "code": "internal_error",
                "message": "Something went wrong",
                "log_id": "20260419001"
            }
        }"#
        .to_string();

        let error = TikTokError::from_response_parts(StatusCode::BAD_REQUEST, body);
        assert!(matches!(
            error,
            TikTokError::Api {
                ref code,
                ref message,
                ref log_id
            } if code == "internal_error" && message == "Something went wrong" && log_id == "20260419001"
        ));
    }

    #[test]
    fn falls_back_to_http_variant_when_body_is_not_error_envelope() {
        let error =
            TikTokError::from_response_parts(StatusCode::BAD_GATEWAY, "oops".to_string());
        assert!(matches!(
            error,
            TikTokError::Http {
                status: 502,
                ref body
            } if body == "oops"
        ));
    }

    #[test]
    fn classifies_access_token_invalid_as_unauthorized() {
        let body = r#"{"error":{"code":"access_token_invalid","message":"bad","log_id":"x"}}"#
            .to_string();
        let error = TikTokError::from_response_parts(StatusCode::BAD_REQUEST, body);
        assert!(matches!(error, TikTokError::Unauthorized));
    }

    #[test]
    fn classifies_access_token_expired_as_unauthorized() {
        let body = r#"{"error":{"code":"access_token_expired","message":"exp","log_id":"x"}}"#
            .to_string();
        let error = TikTokError::from_response_parts(StatusCode::BAD_REQUEST, body);
        assert!(matches!(error, TikTokError::Unauthorized));
    }

    #[test]
    fn classifies_rate_limit_exceeded_as_rate_limited() {
        let body = r#"{"error":{"code":"rate_limit_exceeded","message":"slow","log_id":"x"}}"#
            .to_string();
        let error = TikTokError::from_response_parts(StatusCode::BAD_REQUEST, body);
        assert!(matches!(error, TikTokError::RateLimited));
    }

    #[test]
    fn truncates_non_envelope_body_to_256_chars() {
        let body = "x".repeat(1000);
        let error = TikTokError::from_response_parts(StatusCode::BAD_GATEWAY, body);
        match error {
            TikTokError::Http { status, body } => {
                assert_eq!(status, 502);
                assert_eq!(body.chars().count(), 256);
            }
            other => panic!("expected Http variant, got {other:?}"),
        }
    }
}
