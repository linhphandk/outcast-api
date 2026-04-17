pub mod schema;
mod config;
mod instagram;
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
use deadpool_postgres::{Client, Pool, PoolError, Runtime};
use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use user::repository::profile_repository::ProfileRepository;
use user::repository::user_repository::{UserRepository, UserRepositoryTrait};
use session::repository::session_repository::{SessionRepository, SessionRepositoryTrait};
use session::usecase::session_service::SessionService;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use axum::http::{HeaderValue, Method, header};
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
        crate::user::http::profile_controller::get_my_profile,
        crate::user::http::profile_controller::update_my_profile,
    ),
    components(
        schemas(
            crate::user::http::user_controller::CreateUserReq,
            crate::user::http::user_controller::CreateUserRes,
            crate::user::http::user_controller::LoginUserReq,
            crate::user::http::user_controller::MeRes,
            crate::user::http::profile_controller::CreatorProfileRes,
            crate::user::http::profile_controller::UpdateCreatorProfileReq,
        )
    ),
    tags(
        (name = "Users", description = "User management endpoints"),
        (name = "Profiles", description = "Creator profile endpoints")
    ),
    info(
        title = "Outcast API",
        version = "1.0.0",
        description = "Outcast API documentation",
        license(name = "MIT", url = "https://opensource.org/licenses/MIT")
    )
)]
pub struct ApiDoc;

#[derive(Deserialize, Serialize)]
struct Event {
    id: Uuid,
    title: String,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: deadpool_postgres::Pool,
    pub user_service: crate::user::usecase::user_service::UserService<UserRepository>,
    pub profile_service: crate::user::usecase::profile_service::ProfileService<ProfileRepository>,
    pub jwt_secret: String,
    pub session_repository: Arc<dyn SessionRepositoryTrait>,
    pub session_service: SessionService,
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

impl axum::extract::FromRef<AppState>
    for crate::user::usecase::profile_service::ProfileService<ProfileRepository>
{
    fn from_ref(state: &AppState) -> Self {
        state.profile_service.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn SessionRepositoryTrait> {
    fn from_ref(state: &AppState) -> Self {
        state.session_repository.clone()
    }
}

impl axum::extract::FromRef<AppState> for SessionService {
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
    let config = crate::config::AppConfig::from_env().unwrap();
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
    let profile_repository = ProfileRepository::new(diesel_pool.clone());
    let session_repository: Arc<dyn SessionRepositoryTrait> =
        Arc::new(SessionRepository::new(diesel_pool.clone()));
    let session_user_repository: Arc<dyn UserRepositoryTrait> =
        Arc::new(UserRepository::new(diesel_pool.clone()));
    let session_service = SessionService::new(session_repository.clone(), session_user_repository);
    let user_service = crate::user::usecase::user_service::UserService::new(
        user_repository,
        config.password_pepper,
    );
    let profile_service = crate::user::usecase::profile_service::ProfileService::new(profile_repository);

    let state = AppState {
        pool,
        user_service,
        profile_service,
        jwt_secret: config.jwt_secret,
        session_repository,
        session_service,
    };

    let app = Router::new()
        .route("/v1.0/event.list", get(event_list))
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .merge(crate::user::http::user_controller::router())
        .merge(crate::user::http::profile_controller::router())
        .merge(crate::session::http::session_controller::router())
        .merge(Scalar::with_url("/scalar", ApiDoc::openapi()))
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::exact(HeaderValue::from_static("http://localhost:3000")))
                .allow_methods(AllowMethods::list([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS]))
                .allow_headers(AllowHeaders::list([header::CONTENT_TYPE, header::AUTHORIZATION]))
                .allow_credentials(true),
        )
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
