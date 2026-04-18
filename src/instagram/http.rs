use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::delete,
};
use tracing::{error, info, instrument, warn};

use crate::{
    instagram::repository::OAuthTokenRepository,
    user::{
        http::auth_extractor::AuthUser,
        repository::profile_repository::{ProfileRepository, ProfileRepositoryTrait},
    },
};

#[instrument(skip_all)]
pub async fn disconnect_instagram(
    auth_user: AuthUser,
    State(profile_repo): State<ProfileRepository>,
    State(oauth_repo): State<OAuthTokenRepository>,
) -> impl IntoResponse {
    let profile = match profile_repo.find_by_user_id(auth_user.user_id).await {
        Ok(Some(profile)) => profile,
        Ok(None) => {
            warn!(user_id = %auth_user.user_id, "Profile not found for instagram disconnect");
            return (StatusCode::NOT_FOUND, "Profile not found").into_response();
        }
        Err(err) => {
            error!(error = %err, user_id = %auth_user.user_id, "Failed to resolve profile");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to disconnect instagram")
                .into_response();
        }
    };

    if let Err(err) = oauth_repo.delete(profile.id, "instagram").await {
        error!(error = %err, profile_id = %profile.id, "Failed to delete oauth token");
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to disconnect instagram")
            .into_response();
    }

    if let Err(err) = profile_repo.reset_instagram_social_metrics(profile.id).await {
        error!(error = %err, profile_id = %profile.id, "Failed to reset social metrics");
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to disconnect instagram")
            .into_response();
    }

    info!(profile_id = %profile.id, "Instagram disconnected");
    StatusCode::NO_CONTENT.into_response()
}

pub fn router<S>() -> Router<S>
where
    ProfileRepository: axum::extract::FromRef<S>,
    OAuthTokenRepository: axum::extract::FromRef<S>,
    String: axum::extract::FromRef<S>,
    std::sync::Arc<dyn crate::session::repository::session_repository::SessionRepositoryTrait>:
        axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/oauth/instagram", delete(disconnect_instagram))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        instagram::repository::OAuthTokenRepositoryTrait,
        session::{
            repository::session_repository::{SessionRepository, SessionRepositoryTrait},
            usecase::session_service::SessionService,
        },
        user::{
            http::{profile_controller::CreatorProfileWithDetailsRes, user_controller::CreateUserRes},
            repository::{
                profile_repository::ProfileRepository,
                user_repository::{UserRepository, UserRepositoryTrait},
            },
            usecase::{profile_service::ProfileService, user_service::UserService},
        },
    };
    use axum::{Router, body::Body, http::Request, routing::post};
    use chrono::Utc;
    use diesel::sql_types::Uuid as DieselUuid;
    use diesel::RunQueryDsl;
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
        profile_repo: ProfileRepository,
        oauth_repo: OAuthTokenRepository,
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

    impl axum::extract::FromRef<TestState> for ProfileRepository {
        fn from_ref(state: &TestState) -> Self {
            state.profile_repo.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for OAuthTokenRepository {
        fn from_ref(state: &TestState) -> Self {
            state.oauth_repo.clone()
        }
    }

    fn build_app(pool: deadpool_diesel::postgres::Pool) -> Router {
        let user_repository = UserRepository::new(pool.clone());
        let profile_repository = ProfileRepository::new(pool.clone());
        let oauth_repository = OAuthTokenRepository::new(pool.clone());
        let session_repo: Arc<dyn SessionRepositoryTrait> =
            Arc::new(SessionRepository::new(pool.clone()));
        let session_user_repository: Arc<dyn UserRepositoryTrait> =
            Arc::new(UserRepository::new(pool.clone()));
        let session_service = SessionService::new(session_repo.clone(), session_user_repository);

        let state = TestState {
            user_service: UserService::new(user_repository, TEST_PEPPER.to_string()),
            profile_service: ProfileService::new(profile_repository.clone()),
            session_service,
            session_repo,
            jwt_secret: TEST_JWT_SECRET.to_string(),
            profile_repo: profile_repository,
            oauth_repo: oauth_repository,
        };

        Router::new()
            .route("/user", post(crate::user::http::user_controller::create_user))
            .merge(crate::user::http::profile_controller::router())
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
    async fn disconnect_instagram_connected_returns_204_and_zeroes_metrics() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "disconnect_connected@example.com").await;
        let profile_repo = ProfileRepository::new(pool.clone());
        let oauth_repo = OAuthTokenRepository::new(pool.clone());

        let profile = profile_repo
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
        profile_repo
            .add_social_handle(
                profile.id,
                "instagram".to_string(),
                "@alice".to_string(),
                "https://instagram.com/alice".to_string(),
                1234,
            )
            .await
            .unwrap();

        let profile_id = profile.id;
        let conn = pool.get().await.unwrap();
        conn.interact(move |conn| {
            diesel::sql_query(
                "UPDATE social_handles
                 SET engagement_rate = 0.1234, last_synced_at = NOW()
                 WHERE profile_id = $1 AND platform = 'instagram'",
            )
            .bind::<DieselUuid, _>(profile_id)
            .execute(conn)
        })
        .await
        .unwrap()
        .unwrap();

        oauth_repo
            .upsert(
                profile.id,
                "instagram",
                "access-token",
                Some("refresh-token"),
                Some(Utc::now() + chrono::Duration::hours(1)),
                "ig-user",
                "instagram_basic",
            )
            .await
            .unwrap();

        let request = Request::builder()
            .method("DELETE")
            .uri("/oauth/instagram")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let deleted_again = oauth_repo.delete(profile.id, "instagram").await.unwrap();
        assert_eq!(deleted_again, 0);

        let request = Request::builder()
            .method("GET")
            .uri("/user/profile")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let profile: CreatorProfileWithDetailsRes = serde_json::from_slice(&body).unwrap();
        let instagram = profile
            .social_handles
            .iter()
            .find(|h| h.platform == "instagram")
            .unwrap();
        assert_eq!(instagram.follower_count, 0);
        assert_eq!(instagram.engagement_rate, "0");
        assert!(instagram.last_synced_at.is_none());
    }

    #[tokio::test]
    async fn disconnect_instagram_not_connected_is_idempotent() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());
        let created = create_user(&app, "disconnect_not_connected@example.com").await;
        let profile_repo = ProfileRepository::new(pool.clone());

        let profile = profile_repo
            .create(
                created.id,
                "Bob".to_string(),
                "Creator".to_string(),
                "gaming".to_string(),
                "https://example.com/bob.png".to_string(),
                "bob".to_string(),
            )
            .await
            .unwrap();
        profile_repo
            .add_social_handle(
                profile.id,
                "instagram".to_string(),
                "@bob".to_string(),
                "https://instagram.com/bob".to_string(),
                987,
            )
            .await
            .unwrap();

        let profile_id = profile.id;
        let conn = pool.get().await.unwrap();
        conn.interact(move |conn| {
            diesel::sql_query(
                "UPDATE social_handles
                 SET engagement_rate = 0.9876, last_synced_at = NOW()
                 WHERE profile_id = $1 AND platform = 'instagram'",
            )
            .bind::<DieselUuid, _>(profile_id)
            .execute(conn)
        })
        .await
        .unwrap()
        .unwrap();

        let request = Request::builder()
            .method("DELETE")
            .uri("/oauth/instagram")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let request = Request::builder()
            .method("GET")
            .uri("/user/profile")
            .header("Authorization", format!("Bearer {}", created.token))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let profile: CreatorProfileWithDetailsRes = serde_json::from_slice(&body).unwrap();
        let instagram = profile
            .social_handles
            .iter()
            .find(|h| h.platform == "instagram")
            .unwrap();
        assert_eq!(instagram.follower_count, 0);
        assert_eq!(instagram.engagement_rate, "0");
        assert!(instagram.last_synced_at.is_none());
    }
}
