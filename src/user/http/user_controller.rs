use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::user::repository::user_repository::UserRepository;
use crate::user::usecase::user_service::UserService;

#[derive(Deserialize)]
pub struct CreateUserReq {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize)]
pub struct CreateUserRes {
    pub id: Uuid,
    pub email: String,
}

pub async fn create_user(
    State(service): State<UserService<UserRepository>>,
    Json(payload): Json<CreateUserReq>,
) -> impl IntoResponse {
    let result = service.create(payload.email, payload.password).await;

    match result {
        Ok(user) => {
            let res = CreateUserRes {
                id: user.id,
                email: user.email,
            };
            (StatusCode::CREATED, Json(res)).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create user").into_response(),
    }
}

pub fn router<S>() -> Router<S>
where
    UserService<UserRepository>: axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/user", post(create_user))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

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

    fn build_app(pool: deadpool_diesel::postgres::Pool) -> Router {
        let repo = UserRepository::new(pool);
        let service = UserService::new(repo);
        Router::new()
            .route("/user", post(create_user))
            .with_state(service)
    }

    #[tokio::test]
    async fn test_create_user_success() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": "test@example.com",
                    "password": "password123"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let res: CreateUserRes = serde_json::from_slice(&body).unwrap();
        assert_eq!(res.email, "test@example.com");
        assert!(!res.id.is_nil());
    }

    #[tokio::test]
    async fn test_create_duplicate_user_fails() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool.clone());

        let body = serde_json::json!({
            "email": "duplicate@example.com",
            "password": "password123"
        })
        .to_string();

        // First request should succeed
        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // Second request with same email should fail
        let app = build_app(pool);
        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_create_user_invalid_json() {
        let (_container, pool) = setup_test_db().await;
        let app = build_app(pool);

        let request = Request::builder()
            .method("POST")
            .uri("/user")
            .header("content-type", "application/json")
            .body(Body::from("{invalid json}"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
