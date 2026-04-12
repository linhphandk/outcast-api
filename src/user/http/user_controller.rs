use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::user::usecase::user_service::UserService;

#[derive(Deserialize)]
pub struct CreateUserReq {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct CreateUserRes {
    pub id: Uuid,
    pub email: String,
}

pub async fn create_user(
    State(service): State<UserService>,
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
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create user",
        )
            .into_response(),
    }
}

pub fn router<S>() -> Router<S>
where
    UserService: axum::extract::FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/user", post(create_user))
}
