use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
};
use bigdecimal::BigDecimal;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::user::{
    http::auth_extractor::AuthUser,
    repository::profile_repository::{
        Profile, ProfileRepository, ProfileWithDetails, Rate, RateInput, SocialHandle,
        SocialHandleInput,
    },
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
pub struct SocialHandleRes {
    pub id: Uuid,
    pub platform: String,
    pub handle: String,
    pub url: String,
    pub follower_count: i32,
    pub engagement_rate: String,
    pub updated_at: Option<String>,
    pub last_synced_at: Option<String>,
}

impl From<SocialHandle> for SocialHandleRes {
    fn from(value: SocialHandle) -> Self {
        Self {
            id: value.id,
            platform: value.platform,
            handle: value.handle,
            url: value.url,
            follower_count: value.follower_count,
            engagement_rate: value.engagement_rate.to_string(),
            updated_at: value.updated_at.map(|dt| dt.to_rfc3339()),
            last_synced_at: value.last_synced_at.map(|dt| dt.to_rfc3339()),
        }
    }
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct RateRes {
    pub id: Uuid,
    pub rate_type: String,
    /// Serialized as string to preserve BigDecimal precision.
    pub amount: String,
}

impl From<Rate> for RateRes {
    fn from(value: Rate) -> Self {
        Self {
            id: value.id,
            rate_type: value.rate_type,
            amount: value.amount.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreatorProfileWithDetailsRes {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub social_handles: Vec<SocialHandleRes>,
    pub rates: Vec<RateRes>,
}

impl From<ProfileWithDetails> for CreatorProfileWithDetailsRes {
    fn from(value: ProfileWithDetails) -> Self {
        Self {
            id: value.profile.id,
            user_id: value.profile.user_id,
            name: value.profile.name,
            bio: value.profile.bio,
            niche: value.profile.niche,
            avatar_url: value.profile.avatar_url,
            username: value.profile.username,
            created_at: value.profile.created_at.map(|dt| dt.to_rfc3339()),
            updated_at: value.profile.updated_at.map(|dt| dt.to_rfc3339()),
            social_handles: value.social_handles.into_iter().map(SocialHandleRes::from).collect(),
            rates: value.rates.into_iter().map(RateRes::from).collect(),
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

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SocialHandleInputReq {
    pub platform: String,
    pub handle: String,
    pub url: String,
    pub follower_count: i32,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RateInputReq {
    pub rate_type: String,
    /// Serialized as string to preserve Numeric/BigDecimal precision.
    pub amount: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateCreatorProfileReq {
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
    pub social_handles: Vec<SocialHandleInputReq>,
    pub rates: Vec<RateInputReq>,
}

#[utoipa::path(
    post,
    path = "/user/profile",
    request_body = CreateCreatorProfileReq,
    responses(
        (status = 201, description = "Created profile with details", body = CreatorProfileWithDetailsRes),
        (status = 400, description = "Invalid payload"),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "Conflict"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_token" = [])),
    tag = "Profiles"
)]
#[instrument(skip_all)]
pub async fn create_my_profile(
    auth_user: AuthUser,
    State(service): State<ProfileService<ProfileRepository>>,
    Json(payload): Json<CreateCreatorProfileReq>,
) -> impl IntoResponse {
    info!(user_id = %auth_user.user_id, "Create my profile request");

    let social_handles = payload
        .social_handles
        .into_iter()
        .map(|handle| SocialHandleInput {
            platform: handle.platform,
            handle: handle.handle,
            url: handle.url,
            follower_count: handle.follower_count,
        })
        .collect();

    let mut rates = Vec::new();
    for rate in payload.rates {
        let amount = match BigDecimal::from_str(&rate.amount) {
            Ok(amount) => amount,
            Err(_) => {
                warn!(user_id = %auth_user.user_id, amount = %rate.amount, "Invalid amount format");
                return (StatusCode::BAD_REQUEST, "Invalid amount format").into_response();
            }
        };
        rates.push(RateInput {
            rate_type: rate.rate_type,
            amount,
        });
    }

    match service
        .add_profile(
            auth_user.user_id,
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
        Ok(details) => (StatusCode::CREATED, Json(CreatorProfileWithDetailsRes::from(details)))
            .into_response(),
        Err(ProfileServiceError::RepositoryError(
            crate::user::repository::profile_repository::ProfileRepositoryError::DieselError(
                DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _),
            ),
        )) => {
            warn!(user_id = %auth_user.user_id, "Create profile conflict");
            (StatusCode::CONFLICT, "Profile data conflicts with existing data").into_response()
        }
        Err(ProfileServiceError::RepositoryError(
            crate::user::repository::profile_repository::ProfileRepositoryError::DieselError(
                DieselError::DatabaseError(DatabaseErrorKind::CheckViolation, _),
            ),
        )) => {
            warn!(user_id = %auth_user.user_id, "Create profile validation failed");
            (StatusCode::BAD_REQUEST, "Profile data failed validation").into_response()
        }
        Err(err) => {
            error!(error = %err, user_id = %auth_user.user_id, "Create profile failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create profile").into_response()
        }
    }
}

#[utoipa::path(
    get,
    path = "/user/profile",
    responses(
        (status = 200, description = "Current user profile with details", body = CreatorProfileWithDetailsRes),
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
    match service.get_profile_with_details_by_user_id(auth_user.user_id).await {
        Ok(details) => Json(CreatorProfileWithDetailsRes::from(details)).into_response(),
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
        .route("/user/profile", post(create_my_profile))
        .route("/user/profile", put(update_my_profile))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::repository::session_repository::{SessionRepository, SessionRepositoryTrait};
    use crate::session::usecase::session_service::SessionService;
    use crate::user::http::user_controller::CreateUserRes;
    use crate::user::repository::profile_repository::{
        ProfileRepository, ProfileRepositoryTrait, RateInput, SocialHandleInput,
    };
    use crate::user::repository::user_repository::{UserRepository, UserRepositoryTrait};
    use crate::user::usecase::user_service::UserService;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use bigdecimal::BigDecimal;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use http_body_util::BodyExt;
    use std::str::FromStr;
    use std::sync::Arc;
    use tower::ServiceExt;

    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
    const TEST_PEPPER: &str = "test_pepper";
    const TEST_JWT_SECRET: &str = "test_jwt_secret";

    async fn setup_test_db() -> (
        testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
        deadpool_diesel::postgres::Pool,
    ) {
        use testcontainers::runners::AsyncRunner;
        use testcontainers_modules::postgres::Postgres;

        let container = Postgres::default().start().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let host = container.get_host().await.unwrap();
        let conn_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");

        let manager =
            deadpool_diesel::postgres::Manager::new(conn_string, deadpool_diesel::Runtime::Tokio1);
        let pool = deadpool_diesel::postgres::Pool::builder(manager)
            .build()
            .unwrap();

        let conn = pool.get().await.unwrap();
        conn.interact(|conn| conn.run_pending_migrations(MIGRATIONS).map(|_| ()))
            .await
            .unwrap()
            .unwrap();

        (container, pool)
    }

    #[derive(Clone)]
    struct TestState {
        user_service: UserService<UserRepository>,
        profile_service: ProfileService<ProfileRepository>,
        session_service: SessionService,
        session_repo: Arc<dyn SessionRepositoryTrait>,
        jwt_secret: String,
    }

    impl axum::extract::FromRef<TestState> for UserService<UserRepository> {
        fn from_ref(state: &TestState) -> Self {
            state.user_service.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for ProfileService<ProfileRepository> {
        fn from_ref(state: &TestState) -> Self {
            state.profile_service.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for SessionService {
        fn from_ref(state: &TestState) -> Self {
            state.session_service.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for Arc<dyn SessionRepositoryTrait> {
        fn from_ref(state: &TestState) -> Self {
            state.session_repo.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for String {
        fn from_ref(state: &TestState) -> Self {
            state.jwt_secret.clone()
        }
    }

    fn build_app(pool: deadpool_diesel::postgres::Pool) -> Router {
        let user_repository = UserRepository::new(pool.clone());
        let profile_repository = ProfileRepository::new(pool.clone());
        let session_repo: Arc<dyn SessionRepositoryTrait> =
            Arc::new(SessionRepository::new(pool.clone()));
        let session_user_repository: Arc<dyn UserRepositoryTrait> =
            Arc::new(UserRepository::new(pool.clone()));
        let session_service = SessionService::new(session_repo.clone(), session_user_repository);

        let state = TestState {
            user_service: UserService::new(user_repository, TEST_PEPPER.to_string()),
            profile_service: ProfileService::new(profile_repository),
            session_service,
            session_repo,
            jwt_secret: TEST_JWT_SECRET.to_string(),
        };

        Router::new()
            .route("/user", post(crate::user::http::user_controller::create_user))
            .merge(router())
            .with_state(state)
    }

    async fn create_user(app: &Router, email: &str) -> CreateUserRes {
        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": email,
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice::<CreateUserRes>(&body).unwrap()
    }

    #[tokio::test]
    async fn test_get_my_profile_success() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "profile_get_ok@example.com").await;

        let profile_repo = ProfileRepository::new(pool.clone());
        profile_repo
            .create(
                created.id,
                "Alice".to_string(),
                "Tech creator".to_string(),
                "technology".to_string(),
                "https://example.com/alice.png".to_string(),
                "alice".to_string(),
            )
            .await
            .unwrap();

        let request = Request::builder()
            .method("GET")
            .uri("/user/profile")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: CreatorProfileWithDetailsRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(res.user_id, created.id);
        assert_eq!(res.name, "Alice");
        assert_eq!(res.username, "alice");
        assert!(res.social_handles.is_empty());
        assert!(res.rates.is_empty());
    }

    #[tokio::test]
    async fn test_get_my_profile_missing_token() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());

        let request = Request::builder()
            .method("GET")
            .uri("/user/profile")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_get_my_profile_not_found() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "profile_get_404@example.com").await;
        let request = Request::builder()
            .method("GET")
            .uri("/user/profile")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_my_profile_success() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "profile_update_ok@example.com").await;

        let profile_repo = ProfileRepository::new(pool.clone());
        profile_repo
            .create(
                created.id,
                "Old Name".to_string(),
                "Old bio".to_string(),
                "old niche".to_string(),
                "https://example.com/old.png".to_string(),
                "old_name".to_string(),
            )
            .await
            .unwrap();

        let request = Request::builder()
            .method("PUT")
            .uri("/user/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::from(
                serde_json::json!({
                    "name": "New Name",
                    "bio": "New bio",
                    "niche": "new niche",
                    "avatar_url": "https://example.com/new.png",
                    "username": "new_name"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: CreatorProfileRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(res.name, "New Name");
        assert_eq!(res.bio, "New bio");
        assert_eq!(res.username, "new_name");
    }

    #[tokio::test]
    async fn test_update_my_profile_not_found() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "profile_update_404@example.com").await;
        let request = Request::builder()
            .method("PUT")
            .uri("/user/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::from(
                serde_json::json!({
                    "name": "Any",
                    "bio": "Any",
                    "niche": "Any",
                    "avatar_url": "https://example.com/any.png",
                    "username": "any"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_my_profile_missing_token() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());

        let request = Request::builder()
            .method("PUT")
            .uri("/user/profile")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "name": "Any",
                    "bio": "Any",
                    "niche": "Any",
                    "avatar_url": "https://example.com/any.png",
                    "username": "any"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_create_my_profile_success() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "profile_create_ok@example.com").await;

        let request = Request::builder()
            .method("POST")
            .uri("/user/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::from(
                serde_json::json!({
                    "name": "Creator One",
                    "bio": "Bio One",
                    "niche": "technology",
                    "avatar_url": "https://example.com/creator.png",
                    "username": "creator_one",
                    "social_handles": [{
                        "platform": "instagram",
                        "handle": "@creatorone",
                        "url": "https://instagram.com/creatorone",
                        "follower_count": 12345
                    }],
                    "rates": [{
                        "rate_type": "post",
                        "amount": "500.00"
                    }]
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: CreatorProfileWithDetailsRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(res.user_id, created.id);
        assert_eq!(res.name, "Creator One");
        assert_eq!(res.bio, "Bio One");
        assert_eq!(res.niche, "technology");
        assert_eq!(res.avatar_url, "https://example.com/creator.png");
        assert_eq!(res.username, "creator_one");
        assert_eq!(res.social_handles.len(), 1);
        assert_eq!(res.social_handles[0].platform, "instagram");
        assert_eq!(res.social_handles[0].handle, "@creatorone");
        assert_eq!(res.social_handles[0].url, "https://instagram.com/creatorone");
        assert_eq!(res.social_handles[0].follower_count, 12345);
        assert_eq!(res.rates.len(), 1);
        assert_eq!(res.rates[0].rate_type, "post");
        assert_eq!(res.rates[0].amount, "500.00");

        let profile_repo = ProfileRepository::new(pool.clone());
        let profile = profile_repo.find_by_user_id(created.id).await.unwrap().unwrap();
        assert_eq!(profile.username, "creator_one");

        let handles = profile_repo
            .find_social_handles_by_profile_id(profile.id)
            .await
            .unwrap();
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].platform, "instagram");

        let rates = profile_repo
            .find_rates_by_profile_id(profile.id)
            .await
            .unwrap();
        assert_eq!(rates.len(), 1);
        assert_eq!(rates[0].rate_type, "post");
        assert_eq!(rates[0].amount.to_string(), "500.00");
    }

    #[tokio::test]
    async fn test_create_my_profile_duplicate_username_returns_conflict() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created_1 = create_user(&app, "profile_create_dup1@example.com").await;
        let created_2 = create_user(&app, "profile_create_dup2@example.com").await;

        let first_request = Request::builder()
            .method("POST")
            .uri("/user/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", created_1.token))
            .body(Body::from(
                serde_json::json!({
                    "name": "Creator A",
                    "bio": "Bio A",
                    "niche": "lifestyle",
                    "avatar_url": "https://example.com/a.png",
                    "username": "same_username",
                    "social_handles": [],
                    "rates": []
                })
                .to_string(),
            ))
            .unwrap();
        let first_response = app.clone().oneshot(first_request).await.unwrap();
        assert_eq!(first_response.status(), StatusCode::CREATED);

        let second_request = Request::builder()
            .method("POST")
            .uri("/user/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", created_2.token))
            .body(Body::from(
                serde_json::json!({
                    "name": "Creator B",
                    "bio": "Bio B",
                    "niche": "fashion",
                    "avatar_url": "https://example.com/b.png",
                    "username": "same_username",
                    "social_handles": [],
                    "rates": []
                })
                .to_string(),
            ))
            .unwrap();
        let second_response = app.clone().oneshot(second_request).await.unwrap();
        assert_eq!(second_response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_create_my_profile_duplicate_platform_rolls_back_profile() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "profile_create_dup_platform@example.com").await;

        let request = Request::builder()
            .method("POST")
            .uri("/user/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::from(
                serde_json::json!({
                    "name": "Creator Rollback",
                    "bio": "Rollback bio",
                    "niche": "travel",
                    "avatar_url": "https://example.com/rollback.png",
                    "username": "rollback_user",
                    "social_handles": [
                        {
                            "platform": "instagram",
                            "handle": "@rollback1",
                            "url": "https://instagram.com/rollback1",
                            "follower_count": 100
                        },
                        {
                            "platform": "instagram",
                            "handle": "@rollback2",
                            "url": "https://instagram.com/rollback2",
                            "follower_count": 200
                        }
                    ],
                    "rates": []
                })
                .to_string(),
            ))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let profile_repo = ProfileRepository::new(pool.clone());
        let profile = profile_repo.find_by_user_id(created.id).await.unwrap();
        assert!(profile.is_none(), "Profile row should be rolled back");
    }

    #[tokio::test]
    async fn test_create_my_profile_invalid_rate_type_returns_bad_request_and_rolls_back() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "profile_create_invalid_rate@example.com").await;

        let request = Request::builder()
            .method("POST")
            .uri("/user/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::from(
                serde_json::json!({
                    "name": "Creator Invalid Rate",
                    "bio": "Bad rate",
                    "niche": "gaming",
                    "avatar_url": "https://example.com/badrate.png",
                    "username": "invalid_rate_user",
                    "social_handles": [],
                    "rates": [{
                        "rate_type": "invalid",
                        "amount": "500.00"
                    }]
                })
                .to_string(),
            ))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let profile_repo = ProfileRepository::new(pool.clone());
        let profile = profile_repo.find_by_user_id(created.id).await.unwrap();
        assert!(profile.is_none(), "Profile row should be rolled back");
    }

    #[tokio::test]
    async fn test_create_my_profile_missing_token() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());

        let request = Request::builder()
            .method("POST")
            .uri("/user/profile")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "name": "Creator Missing Auth",
                    "bio": "No auth",
                    "niche": "music",
                    "avatar_url": "https://example.com/noauth.png",
                    "username": "missing_auth_user",
                    "social_handles": [],
                    "rates": []
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_get_my_profile_with_details() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "profile_details@example.com").await;

        let profile_repo = ProfileRepository::new(pool.clone());
        let details = profile_repo
            .create_with_details(
                created.id,
                "Bob".to_string(),
                "Lifestyle creator".to_string(),
                "lifestyle".to_string(),
                "https://example.com/bob.png".to_string(),
                "bob_creator".to_string(),
                vec![SocialHandleInput {
                    platform: "instagram".to_string(),
                    handle: "@bob".to_string(),
                    url: "https://instagram.com/bob".to_string(),
                    follower_count: 10000,
                }],
                vec![RateInput {
                    rate_type: "post".to_string(),
                    amount: BigDecimal::from_str("250.00").unwrap(),
                }],
            )
            .await
            .unwrap();

        let request = Request::builder()
            .method("GET")
            .uri("/user/profile")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: CreatorProfileWithDetailsRes = serde_json::from_slice(&body).unwrap();

        assert_eq!(res.user_id, created.id);
        assert_eq!(res.name, "Bob");
        assert_eq!(res.username, "bob_creator");

        assert_eq!(res.social_handles.len(), 1);
        let sh = &res.social_handles[0];
        assert_eq!(sh.platform, "instagram");
        assert_eq!(sh.handle, "@bob");
        assert_eq!(sh.url, "https://instagram.com/bob");
        assert_eq!(sh.follower_count, 10000);
        assert_eq!(sh.id, details.social_handles[0].id);

        assert_eq!(res.rates.len(), 1);
        let rate = &res.rates[0];
        assert_eq!(rate.rate_type, "post");
        assert_eq!(rate.amount, "250.00");
        assert_eq!(rate.id, details.rates[0].id);
    }
}
