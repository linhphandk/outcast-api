pub mod schema;
mod user;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use axum_macros::debug_handler;
use config::ConfigError;
use deadpool_postgres::{Client, Pool, PoolError, Runtime};
use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use user::repository::user_repository::UserRepository;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct Config {
    listen: String,
    pg: deadpool_postgres::Config,
    database_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        config::Config::builder()
            .add_source(config::Environment::default().separator("__"))
            .build()
            .unwrap()
            .try_deserialize()
    }
}

#[derive(Deserialize, Serialize)]
struct Event {
    id: Uuid,
    title: String,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: deadpool_postgres::Pool,
    pub user_service: crate::user::usecase::user_service::UserService<UserRepository>,
}

impl axum::extract::FromRef<AppState> for deadpool_postgres::Pool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl axum::extract::FromRef<AppState> for crate::user::usecase::user_service::UserService<UserRepository> {
    fn from_ref(state: &AppState) -> Self {
        state.user_service.clone()
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("Pool error: {0}")]
    PoolError(#[from] PoolError),
    #[error("PostgreSQL error: {0}")]
    PgError(#[from] tokio_postgres::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "An internal error occurred. Please try again later.",
        )
            .into_response()
    }
}

#[debug_handler]
async fn event_list(pool: State<Pool>) -> Result<Json<Vec<Event>>, Error> {
    let client: Client = pool.get().await?;
    let stmt = client.prepare_cached("SELECT id, title FROM event").await?;
    let rows = client.query(&stmt, &[]).await?;
    let events = rows
        .into_iter()
        .map(|row| Event {
            id: row.get(0),
            title: row.get(1),
        })
        .collect::<Vec<_>>();
    Ok(Json(events))
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let config = Config::from_env().unwrap();
    let pool = config
        .pg
        .create_pool(Some(Runtime::Tokio1), tokio_postgres::NoTls)
        .unwrap();

    let diesel_manager = deadpool_diesel::postgres::Manager::new(
        &config.database_url,
        deadpool_diesel::Runtime::Tokio1,
    );
    let diesel_pool = deadpool_diesel::postgres::Pool::builder(diesel_manager)
        .build()
        .unwrap();

    let user_repository = UserRepository::new(diesel_pool);
    let user_service = crate::user::usecase::user_service::UserService::new(user_repository);

    let state = AppState { pool, user_service };

    let app = Router::new()
        .route("/v1.0/event.list", get(event_list))
        .merge(crate::user::http::user_controller::router())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&config.listen).await.unwrap();
    println!("Server running at http://{}/", &config.listen);
    println!(
        "Try the following URLs: http://{}/v1.0/event.list",
        &config.listen,
    );
    axum::serve(listener, app).await.unwrap();
}
