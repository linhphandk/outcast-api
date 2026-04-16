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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::repository::session_repository::{SessionRepository, SessionRepositoryTrait};
    use crate::session::usecase::session_service::SessionService;
    use crate::user::http::user_controller::CreateUserRes;
    use crate::user::repository::profile_repository::{ProfileRepositoryTrait, ProfileRepository};
    use crate::user::repository::user_repository::{UserRepository, UserRepositoryTrait};
    use crate::user::usecase::user_service::UserService;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use http_body_util::BodyExt;
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
        let res: CreatorProfileRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(res.user_id, created.id);
        assert_eq!(res.name, "Alice");
        assert_eq!(res.username, "alice");
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
}
