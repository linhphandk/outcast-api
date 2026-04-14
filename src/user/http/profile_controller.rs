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
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::user::crypto::jwt::verify_jwt;
use crate::user::repository::profile_repository::{
    ProfileRepository, ProfileRepositoryError, RateInput, SocialHandleInput,
};
use crate::user::usecase::profile_service::{ProfileService, ProfileServiceError};

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

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct SocialHandleRes {
    pub id: Uuid,
    pub platform: String,
    pub handle: String,
    pub url: String,
    pub follower_count: i32,
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct RateRes {
    pub id: Uuid,
    pub rate_type: String,
    pub amount: String,
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
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
        (status = 409, description = "Profile already exists"),
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
        Err(ProfileServiceError::RepositoryError(ProfileRepositoryError::DieselError(
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _),
        ))) => (StatusCode::CONFLICT, "Profile already exists").into_response(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use crate::user::crypto::jwt::create_jwt;
    use crate::user::repository::profile_repository::ProfileRepository;
    use crate::user::repository::user_repository::NewUser;
    use crate::user::usecase::profile_service::ProfileService;
    use diesel::prelude::*;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

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

    async fn create_test_user(pool: &deadpool_diesel::postgres::Pool) -> Uuid {
        let conn = pool.get().await.unwrap();
        let id = Uuid::new_v4();
        conn.interact(move |conn| {
            diesel::insert_into(crate::schema::users::table)
                .values(&NewUser {
                    id,
                    email: format!("user-{}@example.com", id),
                    password: "hashed".to_string(),
                })
                .execute(conn)
        })
        .await
        .unwrap()
        .unwrap();
        id
    }

    #[derive(Clone)]
    struct TestState {
        service: ProfileService<ProfileRepository>,
        jwt_secret: String,
    }

    impl axum::extract::FromRef<TestState> for ProfileService<ProfileRepository> {
        fn from_ref(state: &TestState) -> Self {
            state.service.clone()
        }
    }

    impl axum::extract::FromRef<TestState> for String {
        fn from_ref(state: &TestState) -> Self {
            state.jwt_secret.clone()
        }
    }

    fn build_app(pool: deadpool_diesel::postgres::Pool) -> Router {
        let repo = ProfileRepository::new(pool);
        let service = ProfileService::new(repo);
        let state = TestState {
            service,
            jwt_secret: TEST_JWT_SECRET.to_string(),
        };
        Router::new()
            .route("/profile", post(create_profile))
            .with_state(state)
    }

    fn profile_body(username: &str) -> String {
        serde_json::json!({
            "name": "Alice",
            "bio": "Tech creator",
            "niche": "technology",
            "avatar_url": "https://example.com/avatar.png",
            "username": username,
            "social_handles": [
                {
                    "platform": "instagram",
                    "handle": "@alice",
                    "url": "https://instagram.com/alice",
                    "follower_count": 10000
                }
            ],
            "rates": [
                {
                    "rate_type": "post",
                    "amount": "500.00"
                }
            ]
        })
        .to_string()
    }

    #[tokio::test]
    async fn test_create_profile_success() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let app = build_app(pool);

        let token = create_jwt(user_id, "test@example.com", TEST_JWT_SECRET).unwrap();

        let request = Request::builder()
            .method("POST")
            .uri("/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from(profile_body("alice_tech")))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: CreateProfileRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(res.user_id, user_id);
        assert_eq!(res.name, "Alice");
        assert_eq!(res.username, "alice_tech");
        assert_eq!(res.social_handles.len(), 1);
        assert_eq!(res.social_handles[0].platform, "instagram");
        assert_eq!(res.rates.len(), 1);
        assert_eq!(res.rates[0].rate_type, "post");
    }

    #[tokio::test]
    async fn test_create_profile_with_no_social_handles_and_rates() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let app = build_app(pool);

        let token = create_jwt(user_id, "test@example.com", TEST_JWT_SECRET).unwrap();

        let body = serde_json::json!({
            "name": "Bob",
            "bio": "Minimal profile",
            "niche": "gaming",
            "avatar_url": "https://example.com/bob.png",
            "username": "bob_gamer",
            "social_handles": [],
            "rates": []
        })
        .to_string();

        let request = Request::builder()
            .method("POST")
            .uri("/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: CreateProfileRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(res.user_id, user_id);
        assert!(res.social_handles.is_empty());
        assert!(res.rates.is_empty());
    }

    #[tokio::test]
    async fn test_create_profile_duplicate_username_returns_conflict() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let app = build_app(pool.clone());

        let token = create_jwt(user_id, "test@example.com", TEST_JWT_SECRET).unwrap();

        let request = Request::builder()
            .method("POST")
            .uri("/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", token.clone()))
            .body(Body::from(profile_body("duplicate_user")))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // Second user tries the same username
        let second_user_id = create_test_user(&pool).await;
        let second_token =
            create_jwt(second_user_id, "second@example.com", TEST_JWT_SECRET).unwrap();
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", second_token))
            .body(Body::from(profile_body("duplicate_user")))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_create_profile_missing_token_returns_bad_request() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/profile")
            .header("content-type", "application/json")
            .body(Body::from(profile_body("no_auth_user")))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_profile_invalid_token_returns_unauthorized() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/profile")
            .header("content-type", "application/json")
            .header("Authorization", "Bearer this.is.not.a.valid.token")
            .body(Body::from(profile_body("invalid_auth_user")))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_create_profile_invalid_rate_amount_returns_bad_request() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let app = build_app(pool);

        let token = create_jwt(user_id, "test@example.com", TEST_JWT_SECRET).unwrap();

        let body = serde_json::json!({
            "name": "Alice",
            "bio": "Tech creator",
            "niche": "technology",
            "avatar_url": "https://example.com/avatar.png",
            "username": "alice_invalid_rate",
            "social_handles": [],
            "rates": [
                {
                    "rate_type": "post",
                    "amount": "not_a_number"
                }
            ]
        })
        .to_string();

        let request = Request::builder()
            .method("POST")
            .uri("/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_profile_invalid_json_returns_bad_request() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let app = build_app(pool);

        let token = create_jwt(user_id, "test@example.com", TEST_JWT_SECRET).unwrap();

        let request = Request::builder()
            .method("POST")
            .uri("/profile")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from("{invalid json}"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
