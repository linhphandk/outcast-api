use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
};
use axum_extra::TypedHeader;
use axum_extra::headers::Authorization;
use axum_extra::headers::authorization::Bearer;
use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::user::crypto::jwt::verify_jwt;
use crate::user::repository::profile_repository::{
    ProfileRepository, RateInput, SocialHandleInput,
};
use crate::user::usecase::profile_service::ProfileService;

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SocialHandleReq {
    pub platform: String,
    pub handle: String,
    pub url: String,
    pub follower_count: i32,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RateReq {
    pub rate_type: String,
    pub amount: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateProfileReq {
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
    pub social_handles: Vec<SocialHandleReq>,
    pub rates: Vec<RateReq>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SocialHandleRes {
    pub id: Uuid,
    pub platform: String,
    pub handle: String,
    pub url: String,
    pub follower_count: i32,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RateRes {
    pub id: Uuid,
    pub rate_type: String,
    pub amount: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CreateProfileRes {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
    pub social_handles: Vec<SocialHandleRes>,
    pub rates: Vec<RateRes>,
}

#[utoipa::path(
    post,
    path = "/profile",
    request_body = CreateProfileReq,
    responses(
        (status = 201, description = "Profile created successfully", body = CreateProfileRes),
        (status = 400, description = "Invalid amount format"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_token" = [])),
    tag = "Profiles"
)]
pub async fn create_profile(
    State(profile_service): State<ProfileService<ProfileRepository>>,
    State(jwt_secret): State<String>,
    TypedHeader(bearer): TypedHeader<Authorization<Bearer>>,
    Json(payload): Json<CreateProfileReq>,
) -> impl IntoResponse {
    let claims = match verify_jwt(bearer.token(), &jwt_secret) {
        Ok(c) => c,
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, "Invalid or expired token").into_response();
        }
    };

    let social_handles: Vec<SocialHandleInput> = payload
        .social_handles
        .into_iter()
        .map(|h| SocialHandleInput {
            platform: h.platform,
            handle: h.handle,
            url: h.url,
            follower_count: h.follower_count,
        })
        .collect();

    let rates: Result<Vec<RateInput>, _> = payload
        .rates
        .into_iter()
        .map(|r| {
            BigDecimal::from_str(&r.amount).map(|amount| RateInput {
                rate_type: r.rate_type,
                amount,
            })
        })
        .collect();

    let rates = match rates {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid amount format").into_response();
        }
    };

    match profile_service
        .add_profile(
            claims.sub,
            payload.name,
            payload.bio,
            payload.niche,
            payload.avatar_url,
            payload.username,
            social_handles,
            rates,
        )
        .await
    {
        Ok(details) => {
            let res = CreateProfileRes {
                id: details.profile.id,
                user_id: details.profile.user_id,
                name: details.profile.name,
                bio: details.profile.bio,
                niche: details.profile.niche,
                avatar_url: details.profile.avatar_url,
                username: details.profile.username,
                social_handles: details
                    .social_handles
                    .into_iter()
                    .map(|h| SocialHandleRes {
                        id: h.id,
                        platform: h.platform,
                        handle: h.handle,
                        url: h.url,
                        follower_count: h.follower_count,
                    })
                    .collect(),
                rates: details
                    .rates
                    .into_iter()
                    .map(|r| RateRes {
                        id: r.id,
                        rate_type: r.rate_type,
                        amount: r.amount.to_string(),
                    })
                    .collect(),
            };
            (StatusCode::CREATED, Json(res)).into_response()
        }
        Err(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create profile").into_response()
        }
    }
}

pub fn router<S>() -> Router<S>
where
    ProfileService<ProfileRepository>: axum::extract::FromRef<S>,
    String: axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/profile", post(create_profile))
}
