pub mod schema;
mod session;
mod user;
use tracing::info;
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
use session::repository::session_repository::SessionRepository;
use session::usecase::session_service::SessionService;
use user::repository::user_repository::UserRepository;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};
use uuid::Uuid;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::user::http::user_controller::create_user,
        crate::user::http::user_controller::login_user,
        crate::user::http::user_controller::get_me,
    ),
    components(
        schemas(
            crate::user::http::user_controller::CreateUserReq,
            crate::user::http::user_controller::CreateUserRes,
            crate::user::http::user_controller::LoginUserReq,
            crate::user::http::user_controller::MeRes,
        )
    ),
    tags(
        (name = "Users", description = "User management endpoints")
    ),
    info(
        title = "Outcast API",
        version = "1.0.0",
        description = "Outcast API documentation",
        license(name = "MIT", url = "https://opensource.org/licenses/MIT")
    )
)]
pub struct ApiDoc;

#[derive(Debug, Deserialize)]
struct Config {
    listen: String,
    pg: deadpool_postgres::Config,
    database_url: String,
    password_pepper: String,
    jwt_secret: String,
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
    pub session_service: SessionService<SessionRepository>,
    pub jwt_secret: String,
}

impl axum::extract::FromRef<AppState> for deadpool_postgres::Pool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl axum::extract::FromRef<AppState>
    for crate::user::usecase::user_service::UserService<UserRepository>
{
    fn from_ref(state: &AppState) -> Self {
        state.user_service.clone()
    }
}

impl axum::extract::FromRef<AppState> for SessionService<SessionRepository> {
    fn from_ref(state: &AppState) -> Self {
        state.session_service.clone()
    }
}

impl axum::extract::FromRef<AppState> for String {
    fn from_ref(state: &AppState) -> Self {
        state.jwt_secret.clone()
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
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

    let user_repository = UserRepository::new(diesel_pool.clone());
    let user_service = crate::user::usecase::user_service::UserService::new(
        user_repository,
        config.password_pepper,
    );

    let session_repository = SessionRepository::new(diesel_pool);
    let session_service = SessionService::new(session_repository, config.jwt_secret.clone());

    let state = AppState {
        pool,
        user_service,
        session_service,
        jwt_secret: config.jwt_secret,
    };

    let app = Router::new()
        .route("/v1.0/event.list", get(event_list))
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .merge(crate::user::http::user_controller::router())
        .merge(crate::session::http::session_controller::router())
        .merge(Scalar::with_url("/scalar", ApiDoc::openapi()))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&config.listen).await.unwrap();
    info!("Server running at http://{}/", &config.listen);
    info!(
        "Try the following URLs: http://{}/v1.0/event.list",
        &config.listen,
    );
    axum::serve(listener, app).await.unwrap();
}
