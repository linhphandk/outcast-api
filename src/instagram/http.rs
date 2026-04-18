use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::get,
};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use serde::Deserialize;
use tracing::{error, info, instrument, warn};

use crate::instagram::{
    client::IgClient,
    repository::{OAuthTokenRepository, OAuthTokenRepositoryTrait},
    state::{OAUTH_STATE_COOKIE_NAME, verify_state_cookie},
};
use crate::user::repository::profile_repository::{ProfileRepository, ProfileRepositoryTrait};

const DASHBOARD_REDIRECT_PATH: &str = "/dashboard";

#[derive(Debug, Deserialize)]
pub struct InstagramCallbackQuery {
    code: String,
    state: String,
}

#[instrument(skip_all)]
pub async fn instagram_callback(
    jar: CookieJar,
    Query(query): Query<InstagramCallbackQuery>,
    State(jwt_secret): State<String>,
    State(profile_repo): State<ProfileRepository>,
    State(client): State<IgClient>,
    State(oauth_repo): State<OAuthTokenRepository>,
) -> impl IntoResponse {
    let state_cookie = match jar.get(OAUTH_STATE_COOKIE_NAME) {
        Some(cookie) => cookie.value(),
        None => {
            warn!("Instagram OAuth callback missing state cookie");
            return (StatusCode::BAD_REQUEST, "Missing OAuth state cookie").into_response();
        }
    };

    let user_id = match verify_state_cookie(&query.state, state_cookie, jwt_secret.as_bytes()) {
        Ok(user_id) => user_id,
        Err(err) => {
            warn!(error = %err, "Instagram OAuth callback state verification failed");
            return (StatusCode::BAD_REQUEST, "Invalid OAuth state").into_response();
        }
    };

    let profile_id = match profile_repo.find_by_user_id(user_id).await {
        Ok(Some(profile)) => profile.id,
        Ok(None) => {
            warn!(user_id = %user_id, "Instagram OAuth callback user profile not found");
            return (StatusCode::NOT_FOUND, "Profile not found").into_response();
        }
        Err(err) => {
            error!(error = %err, user_id = %user_id, "Failed to resolve profile for Instagram OAuth callback");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to resolve profile",
            )
                .into_response();
        }
    };

    let short = match client.exchange_code(&query.code).await {
        Ok(token) => token,
        Err(err) => {
            error!(error = %err, "Failed to exchange Instagram OAuth code");
            return (StatusCode::BAD_GATEWAY, "Failed to exchange OAuth code").into_response();
        }
    };

    let long = match client.exchange_for_long_lived(&short.access_token).await {
        Ok(token) => token,
        Err(err) => {
            error!(error = %err, "Failed to exchange for Instagram long-lived token");
            return (
                StatusCode::BAD_GATEWAY,
                "Failed to exchange for long-lived token",
            )
                .into_response();
        }
    };

    let expires_at = long.expires_in.and_then(|seconds| {
        i64::try_from(seconds)
            .ok()
            .map(|seconds| Utc::now() + Duration::seconds(seconds))
    });

    if let Err(err) = oauth_repo
        .upsert(
            profile_id,
            "instagram",
            &long.access_token,
            None,
            expires_at,
            "",
            "",
        )
        .await
    {
        error!(error = %err, profile_id = %profile_id, "Failed to upsert Instagram OAuth token");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to persist OAuth token",
        )
            .into_response();
    }

    info!(profile_id = %profile_id, "Instagram OAuth callback completed");
    Redirect::to(DASHBOARD_REDIRECT_PATH).into_response()
}

pub fn router<S>() -> Router<S>
where
    String: axum::extract::FromRef<S>,
    ProfileRepository: axum::extract::FromRef<S>,
    OAuthTokenRepository: axum::extract::FromRef<S>,
    IgClient: axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/oauth/instagram/callback", get(instagram_callback))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InstagramConfig;
    use crate::instagram::state::issue_state_cookie;
    use axum::http::Request;
    use tower::ServiceExt;

    #[derive(Clone)]
    struct TestState {
        jwt_secret: String,
        profile_repository: ProfileRepository,
        oauth_repository: OAuthTokenRepository,
        instagram_client: IgClient,
    }

    impl axum::extract::FromRef<TestState> for String {
        fn from_ref(state: &TestState) -> Self {
            state.jwt_secret.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for ProfileRepository {
        fn from_ref(state: &TestState) -> Self {
            state.profile_repository.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for OAuthTokenRepository {
        fn from_ref(state: &TestState) -> Self {
            state.oauth_repository.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for IgClient {
        fn from_ref(state: &TestState) -> Self {
            state.instagram_client.clone()
        }
    }

    fn test_state() -> TestState {
        let manager = deadpool_diesel::postgres::Manager::new(
            "postgres://postgres:postgres@127.0.0.1:1/postgres",
            deadpool_diesel::Runtime::Tokio1,
        );
        let pool = deadpool_diesel::postgres::Pool::builder(manager)
            .max_size(1)
            .build()
            .expect("pool should build");

        let ig_cfg = InstagramConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            redirect_uri: "http://localhost:3000/oauth/instagram/callback".to_string(),
            graph_api_version: "v19.0".to_string(),
        };

        TestState {
            jwt_secret: "test-jwt-secret".to_string(),
            profile_repository: ProfileRepository::new(pool.clone()),
            oauth_repository: OAuthTokenRepository::new(pool),
            instagram_client: IgClient::new(ig_cfg),
        }
    }

    fn app() -> Router {
        Router::new()
            .route("/oauth/instagram/callback", get(instagram_callback))
            .with_state(test_state())
    }

    #[tokio::test]
    async fn callback_missing_state_cookie_returns_bad_request() {
        let request = Request::builder()
            .uri("/oauth/instagram/callback?code=abc&state=state-1")
            .body(axum::body::Body::empty())
            .expect("request should build");

        let response = app().oneshot(request).await.expect("response should be returned");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn callback_invalid_state_returns_bad_request() {
        let (_state, cookie) = issue_state_cookie(
            uuid::Uuid::new_v4(),
            test_state().jwt_secret.as_bytes(),
        );

        let request = Request::builder()
            .uri("/oauth/instagram/callback?code=abc&state=different-state")
            .header("Cookie", format!("{}={}", cookie.name(), cookie.value()))
            .body(axum::body::Body::empty())
            .expect("request should build");

        let response = app().oneshot(request).await.expect("response should be returned");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
