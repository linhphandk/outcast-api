use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum IgError {
    #[error("unauthorized — token expired or revoked")]
    Unauthorized,
    #[error("rate limited by Graph API")]
    RateLimited { retry_after: Option<Duration> },
    #[error("HTTP error: {status} {body}")]
    Http { status: u16, body: String },
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    #[error(transparent)]
    Parse(#[from] serde_json::Error),
    #[error("Graph API error {code}: {message}")]
    Graph {
        code: i64,
        message: String,
        subcode: Option<i64>,
    },
}

#[derive(Debug, Deserialize)]
struct GraphErrorEnvelope {
    error: GraphErrorBody,
}

#[derive(Debug, Deserialize)]
struct GraphErrorBody {
    code: i64,
    message: String,
    #[serde(rename = "error_subcode")]
    error_subcode: Option<i64>,
}

impl IgError {
    pub fn from_response_parts(status: StatusCode, headers: &HeaderMap, body: String) -> Self {
        match status {
            StatusCode::UNAUTHORIZED => Self::Unauthorized,
            StatusCode::TOO_MANY_REQUESTS => {
                let retry_after = headers
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(Duration::from_secs);

                Self::RateLimited { retry_after }
            }
            _ => {
                if let Ok(envelope) = serde_json::from_str::<GraphErrorEnvelope>(&body) {
                    return Self::Graph {
                        code: envelope.error.code,
                        message: envelope.error.message,
                        subcode: envelope.error.error_subcode,
                    };
                }

                Self::Http {
                    status: status.as_u16(),
                    body,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IgError;
    use reqwest::StatusCode;
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
    use std::time::Duration;

    #[test]
    fn maps_401_to_unauthorized() {
        let headers = HeaderMap::new();
        let error = IgError::from_response_parts(StatusCode::UNAUTHORIZED, &headers, String::new());

        assert!(matches!(error, IgError::Unauthorized));
    }

    #[test]
    fn maps_429_with_retry_after_header_to_rate_limited() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("60"));
        let error =
            IgError::from_response_parts(StatusCode::TOO_MANY_REQUESTS, &headers, String::new());

        assert!(matches!(
            error,
            IgError::RateLimited {
                retry_after: Some(duration)
            } if duration == Duration::from_secs(60)
        ));
    }

    #[test]
    fn maps_graph_error_envelope_to_graph_variant() {
        let headers = HeaderMap::new();
        let body = r#"{
            "error": {
                "code": 190,
                "message": "Invalid OAuth 2.0 Access Token",
                "error_subcode": 463
            }
        }"#
        .to_string();

        let error = IgError::from_response_parts(StatusCode::BAD_REQUEST, &headers, body);

        assert!(matches!(
            error,
            IgError::Graph {
                code: 190,
                ref message,
                subcode: Some(463)
            } if message == "Invalid OAuth 2.0 Access Token"
        ));
    }
}
