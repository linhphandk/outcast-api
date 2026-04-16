use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::user::{
    http::auth_extractor::AuthUser,
    repository::profile_repository::{ProfileRepository, Profile},
    usecase::profile_service::{ProfileService, ProfileServiceError},
};

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreatorProfileRes {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl From<Profile> for CreatorProfileRes {
    fn from(value: Profile) -> Self {
        Self {
            id: value.id,
            user_id: value.user_id,
            name: value.name,
            bio: value.bio,
            niche: value.niche,
            avatar_url: value.avatar_url,
            username: value.username,
            created_at: value.created_at.map(|dt| dt.to_rfc3339()),
            updated_at: value.updated_at.map(|dt| dt.to_rfc3339()),
        }
    }
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateCreatorProfileReq {
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
}

#[utoipa::path(
    get,
    path = "/user/profile",
    responses(
        (status = 200, description = "Current user profile", body = CreatorProfileRes),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Profile not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_token" = [])),
    tag = "Profiles"
)]
#[instrument(skip_all)]
pub async fn get_my_profile(
    auth_user: AuthUser,
    State(service): State<ProfileService<ProfileRepository>>,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "Get my profile request");
    match service.get_profile_by_user_id(auth_user.user_id).await {
        Ok(profile) => Json(CreatorProfileRes::from(profile)).into_response(),
        Err(ProfileServiceError::ProfileNotFound) => {
            warn!(user_id = %auth_user.user_id, "Profile not found");
            (StatusCode::NOT_FOUND, "Profile not found").into_response()
        }
        Err(err) => {
            error!(error = %err, user_id = %auth_user.user_id, "Get profile failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get profile").into_response()
        }
    }
}

#[utoipa::path(
    put,
    path = "/user/profile",
    request_body = UpdateCreatorProfileReq,
    responses(
        (status = 200, description = "Updated profile", body = CreatorProfileRes),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Profile not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_token" = [])),
    tag = "Profiles"
)]
#[instrument(skip_all)]
pub async fn update_my_profile(
    auth_user: AuthUser,
    State(service): State<ProfileService<ProfileRepository>>,
    Json(payload): Json<UpdateCreatorProfileReq>,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "Update my profile request");
    match service
        .update_profile_by_user_id(
            auth_user.user_id,
            payload.name,
            payload.bio,
            payload.niche,
            payload.avatar_url,
            payload.username,
        )
        .await
    {
        Ok(profile) => Json(CreatorProfileRes::from(profile)).into_response(),
        Err(ProfileServiceError::ProfileNotFound) => {
            warn!(user_id = %auth_user.user_id, "Profile not found for update");
            (StatusCode::NOT_FOUND, "Profile not found").into_response()
        }
        Err(err) => {
            error!(error = %err, user_id = %auth_user.user_id, "Update profile failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to update profile").into_response()
        }
    }
}

pub fn router<S>() -> Router<S>
where
    ProfileService<ProfileRepository>: axum::extract::FromRef<S>,
    String: axum::extract::FromRef<S>,
    std::sync::Arc<dyn crate::session::repository::session_repository::SessionRepositoryTrait>:
        axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/user/profile", get(get_my_profile))
        .route("/user/profile", put(update_my_profile))
}
