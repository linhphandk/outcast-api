use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
};
use http_body_util::BodyExt;
use outcast_api::{
    session::{
        repository::session_repository::{SessionRepository, SessionRepositoryTrait},
        usecase::session_service::SessionService,
    },
    user::{
        http::user_controller::{
            CreateUserRes, UploadAvatarRes, create_user, get_me, login_user,
            upload_profile_image,
        },
        repository::user_repository::{UserRepository, UserRepositoryTrait},
        storage::{StoragePort, s3_adapter::S3Adapter},
        usecase::user_service::UserService,
    },
};
use tower::ServiceExt;

mod common;


const TEST_PEPPER: &str = "test_pepper";
const TEST_JWT_SECRET: &str = "test_jwt_secret";
const MOTO_BUCKET: &str = "test-avatars";

// ---------------------------------------------------------------------------
// Test state (mirrors what main.rs builds but only with fields the router needs)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct TestState {
    user_service: UserService<UserRepository>,
    session_service: SessionService,
    session_repo: Arc<dyn SessionRepositoryTrait>,
    jwt_secret: String,
}

impl axum::extract::FromRef<TestState> for UserService<UserRepository> {
    fn from_ref(state: &TestState) -> Self {
        state.user_service.clone()
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

// ---------------------------------------------------------------------------
// Moto S3 fixture
// ---------------------------------------------------------------------------

struct MotoServer {
    child: std::process::Child,
    endpoint: String,
}

impl MotoServer {
    /// Spawn `moto_server` on an available port and poll until it is ready.
    ///
    /// Returns an error if `moto_server` cannot be started or if it
    /// fails to respond within 10 seconds.
    async fn start() -> Result<Self, std::io::Error> {
        // Pick a random port by binding to 0 then dropping it
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let child = std::process::Command::new("moto_server")
            .args(["-p", &port.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let endpoint = format!("http://127.0.0.1:{port}");

        // Wait for the server to become ready (up to 10 s)
        let client = reqwest::Client::new();
        for _ in 0..100 {
            if client.get(&endpoint).send().await.is_ok() {
                return Ok(Self { child, endpoint });
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "moto_server did not become ready in time",
        ))
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

async fn start_moto_or_skip() -> Option<MotoServer> {
    match MotoServer::start().await {
        Ok(server) => Some(server),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("Skipping test: `moto_server` not found (`pip install moto[s3,server]`).");
            None
        }
        Err(err) => panic!("Failed to start moto_server: {err}"),
    }
}

impl Drop for MotoServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// S3 client + bucket creation helper
// ---------------------------------------------------------------------------

/// Build an [`S3Adapter`] backed by a Moto server. Creates the test bucket
/// and uses fake AWS credentials so no real AWS account is needed.
async fn build_s3_adapter(moto_endpoint: &str) -> S3Adapter {
    let s3_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .endpoint_url(moto_endpoint)
        .credentials_provider(aws_credential_types::Credentials::new(
            "testing",
            "testing",
            None,
            None,
            "moto",
        ))
        .load()
        .await;

    let s3_client = aws_sdk_s3::Client::from_conf(
        aws_sdk_s3::config::Builder::from(&s3_config)
            .force_path_style(true)
            .build(),
    );

    // Create the bucket in Moto
    s3_client
        .create_bucket()
        .bucket(MOTO_BUCKET)
        .send()
        .await
        .expect("Failed to create test bucket in Moto");

    S3Adapter::new(s3_client, MOTO_BUCKET.to_string())
}

// ---------------------------------------------------------------------------
// App builder
// ---------------------------------------------------------------------------

/// Build an Axum router wired with the user/session routes and the given
/// storage backend.
fn build_app(pool: deadpool_diesel::postgres::Pool, storage: Arc<dyn StoragePort>) -> Router {
    let repo = UserRepository::new(pool.clone());
    let service = UserService::new_with_storage(repo, TEST_PEPPER.to_string(), storage);
    let session_repo: Arc<dyn SessionRepositoryTrait> =
        Arc::new(SessionRepository::new(pool.clone()));
    let session_user_repo: Arc<dyn UserRepositoryTrait> =
        Arc::new(UserRepository::new(pool));
    let session_service = SessionService::new(session_repo.clone(), session_user_repo);

    let state = TestState {
        user_service: service,
        session_service,
        session_repo,
        jwt_secret: TEST_JWT_SECRET.to_string(),
    };

    Router::new()
        .route("/user", post(create_user))
        .route("/user/login", post(login_user))
        .route("/user/me", get(get_me))
        .route("/user/profile/image", post(upload_profile_image))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Helper: create a user and return the auth token + user id
// ---------------------------------------------------------------------------

/// Create a test user account via the HTTP endpoint and return the JWT token
/// and user ID.
async fn create_test_user(
    pool: deadpool_diesel::postgres::Pool,
    storage: Arc<dyn StoragePort>,
) -> (String, uuid::Uuid) {
    let app = build_app(pool, storage);

    let request = Request::builder()
        .method("POST")
        .uri("/user")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "email": "upload-test@example.com",
                "password": "password123"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let res: CreateUserRes = serde_json::from_slice(&body).unwrap();
    (res.token, res.id)
}

// ---------------------------------------------------------------------------
// Helper: build a multipart body for the image upload
// ---------------------------------------------------------------------------

/// Build a `multipart/form-data` body containing one `image` field.
///
/// Returns `(content_type_header, body_bytes)` for use in a request.
fn build_multipart_body(
    content_type: &str,
    data: &[u8],
) -> (String, Vec<u8>) {
    let boundary = "----TestBoundary1234567890";
    let mut body = Vec::new();

    // field part
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"image\"; filename=\"avatar.png\"\r\n\
             Content-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(b"\r\n");

    // closing boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let content_type_header = format!("multipart/form-data; boundary={boundary}");
    (content_type_header, body)
}

// ===========================================================================
// Tests
// ===========================================================================

#[tokio::test]
async fn upload_avatar_happy_path() {
    let Some(moto) = start_moto_or_skip().await else {
        return;
    };
    let s3_adapter = build_s3_adapter(moto.endpoint()).await;
    let storage: Arc<dyn StoragePort> = Arc::new(s3_adapter);
    let test_db = common::acquire_test_db().await;
    let pool = test_db.pool.clone();

    // 1. Create a user and get a token
    let (token, user_id) = create_test_user(pool.clone(), storage.clone()).await;

    // 2. Upload a valid PNG image
    let fake_png = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes (small payload)
    let (ct_header, body) = build_multipart_body("image/png", &fake_png);

    let app = build_app(pool.clone(), storage.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/user/profile/image")
        .header("Authorization", format!("Bearer {token}"))
        .header("content-type", ct_header)
        .body(Body::from(body))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let resp_body = response.into_body().collect().await.unwrap().to_bytes();
    let upload_res: UploadAvatarRes = serde_json::from_slice(&resp_body).unwrap();

    // The avatar_url should contain the expected S3 URI
    let expected_key = format!("avatars/{user_id}");
    assert!(
        upload_res.avatar_url.contains(&expected_key),
        "Expected avatar_url to contain '{expected_key}', got: {}",
        upload_res.avatar_url
    );

    // 3. Verify the object actually exists in Moto S3
    let downloaded = storage.download(&expected_key).await.unwrap();
    assert_eq!(downloaded.as_ref(), &fake_png);

    // 4. Verify avatar_url is persisted in the DB
    let repo = UserRepository::new(pool);
    let db_user = repo.find_by_id(user_id).await.unwrap().unwrap();
    assert!(
        db_user.avatar_url.is_some(),
        "Expected avatar_url to be persisted in the database"
    );
    assert!(
        db_user.avatar_url.as_ref().unwrap().contains(&expected_key),
        "DB avatar_url should contain the S3 key"
    );
}

#[tokio::test]
async fn upload_avatar_invalid_mime_type_returns_400() {
    let Some(moto) = start_moto_or_skip().await else {
        return;
    };
    let s3_adapter = build_s3_adapter(moto.endpoint()).await;
    let storage: Arc<dyn StoragePort> = Arc::new(s3_adapter);
    let test_db = common::acquire_test_db().await;
    let pool = test_db.pool.clone();

    let (token, _user_id) = create_test_user(pool.clone(), storage.clone()).await;

    // Upload with an unsupported MIME type (application/pdf)
    let fake_pdf = b"%PDF-1.4 fake data";
    let (ct_header, body) = build_multipart_body("application/pdf", fake_pdf);

    let app = build_app(pool, storage);
    let request = Request::builder()
        .method("POST")
        .uri("/user/profile/image")
        .header("Authorization", format!("Bearer {token}"))
        .header("content-type", ct_header)
        .body(Body::from(body))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let resp_body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&resp_body);
    assert!(
        text.contains("Unsupported image type"),
        "Expected 'Unsupported image type' error, got: {text}"
    );
}

#[tokio::test]
async fn upload_avatar_oversized_file_returns_400() {
    let Some(moto) = start_moto_or_skip().await else {
        return;
    };
    let s3_adapter = build_s3_adapter(moto.endpoint()).await;
    let storage: Arc<dyn StoragePort> = Arc::new(s3_adapter);
    let test_db = common::acquire_test_db().await;
    let pool = test_db.pool.clone();

    let (token, _user_id) = create_test_user(pool.clone(), storage.clone()).await;

    // Upload a file that exceeds 5 MB
    let oversized = vec![0u8; 5 * 1024 * 1024 + 1]; // 5 MB + 1 byte
    let (ct_header, body) = build_multipart_body("image/png", &oversized);

    let app = build_app(pool, storage);
    let request = Request::builder()
        .method("POST")
        .uri("/user/profile/image")
        .header("Authorization", format!("Bearer {token}"))
        .header("content-type", ct_header)
        .body(Body::from(body))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let resp_body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&resp_body);
    // Axum's multipart parser may reject the oversized payload before the
    // manual size check, producing "Invalid multipart payload" instead of
    // "Image size exceeds 5MB".  Both are valid 400 responses.
    assert!(
        text.contains("Image size exceeds 5MB") || text.contains("Invalid multipart payload"),
        "Expected a size-related 400 error, got: {text}"
    );
}
