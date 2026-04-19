# CLAUDE.md

This file provides context for Claude AI when working on this project.

## Project Overview

**outcast-api** is a Rust-based backend API for a creator marketplace platform. It provides user management, creator profile management, session-based authentication with refresh token rotation, S3-backed avatar storage, and Instagram OAuth integration.

## Architecture

The project follows **Hexagonal Architecture** (Ports and Adapters) to ensure business logic remains decoupled from external dependencies like the database and web framework.

### Directory Structure

```
.
├── .github/workflows/     # CI/CD workflows (Rust CI, Claude Action)
├── migrations/            # Diesel database migrations
├── src/
│   ├── main.rs            # Application entry point, AppState, router setup
│   ├── lib.rs             # OpenAPI (utoipa) doc definition (ApiDoc)
│   ├── config.rs          # Config structs loaded from env vars
│   ├── schema.rs          # Diesel auto-generated schema (do NOT edit manually)
│   ├── user/              # User & profile domain
│   │   ├── mod.rs
│   │   ├── crypto/        # Password hashing (bcrypt+pepper) & JWT utilities
│   │   ├── http/          # Axum handlers: user_controller, profile_controller, auth_extractor
│   │   ├── repository/    # Diesel repositories: user_repository, profile_repository
│   │   ├── storage/       # StoragePort trait + S3Adapter implementation
│   │   └── usecase/       # Services: user_service, profile_service
│   ├── session/           # Session / refresh-token domain
│   │   ├── mod.rs
│   │   ├── http/          # session_controller, cookies helpers
│   │   ├── repository/    # session_repository (Diesel)
│   │   └── usecase/       # session_service (create, refresh, logout, list, delete)
│   └── instagram/         # Instagram OAuth integration
│       ├── mod.rs
│       ├── client.rs      # IgClient — HTTP calls to Instagram API (Instagram Login flow)
│       ├── error.rs       # IgError (unauthorized, rate-limited, graph, transport, parse)
│       ├── http.rs        # OAuth HTTP handlers (authorize, callback, disconnect, refresh)
│       ├── repository.rs  # OAuthTokenRepository (Diesel, upsert/delete)
│       ├── service.rs     # InstagramService — coordinates client + repository
│       └── state.rs       # OAuth CSRF state cookie (issue + verify via JWT)
├── tests/
│   ├── avatar_upload.rs   # Integration tests: S3 avatar upload via HTTP
│   └── instagram_oauth.rs # Integration tests: Instagram OAuth flow via HTTP
├── Cargo.toml
├── diesel.toml
├── docker-compose.yaml    # Local dev PostgreSQL + Adminer
└── shell.nix              # Nix shell with postgresql
```

### Layers

| Layer | Path | Responsibility |
|-------|------|----------------|
| **HTTP (Delivery)** | `src/*/http/` | Axum request handlers, route definitions, request/response DTOs |
| **Use Case (Service)** | `src/*/usecase/` | Business logic, coordinates between HTTP and repository |
| **Repository (Persistence)** | `src/*/repository/` | Database access via Diesel ORM, implements repository traits |
| **Crypto** | `src/user/crypto/` | Password hashing (bcrypt+HMAC pepper) and JWT management |
| **Storage** | `src/user/storage/` | `StoragePort` trait + `S3Adapter` for avatar uploads |

### Data Flow

```
HTTP Request → Axum Router → Controller (http/) → Service (usecase/) → Repository / Storage / Client → PostgreSQL / S3 / Instagram API
```

## Tech Stack

| Component | Technology | Version |
|-----------|-----------|---------|
| **Language** | Rust | Edition 2024 |
| **Web Framework** | Axum | 0.8.1 |
| **Database** | PostgreSQL | (via Docker) |
| **ORM** | Diesel | 2.3.7 |
| **Connection Pooling** | deadpool-diesel / deadpool-postgres | 0.6.1 / 0.14.1 |
| **Async Runtime** | Tokio | 1.51.1 |
| **Authentication** | jsonwebtoken + bcrypt | 9.3.1 / 0.17.0 |
| **Password Security** | bcrypt + HMAC-SHA2 (pepper) | — |
| **Session Management** | Refresh token rotation (DB-backed) | — |
| **Object Storage** | AWS SDK S3 | 1.129.0 |
| **HTTP Client** | reqwest | 0.12 |
| **OpenAPI** | utoipa + utoipa-scalar | 5 / 0.3 |
| **Config** | config + dotenvy | 0.15.22 / 0.15.7 |
| **Serialization** | serde | 1.0.228 |
| **Error Handling** | thiserror | 2.0.18 |
| **Logging** | tracing + tracing-subscriber | 0.1 / 0.3 |
| **Decimal Numbers** | bigdecimal | 0.4.8 |

## Dependencies (Cargo.toml)

### Runtime Dependencies
- **axum** (0.8.1) — Web framework
- **axum-extra** (0.10.0) — Cookie support (`CookieJar`, typed headers)
- **axum-macros** (0.5.0) — `#[debug_handler]` macro
- **diesel** (2.3.7) — ORM with `postgres`, `uuid`, `chrono`, `numeric` features
- **deadpool-diesel** (0.6.1) — Diesel connection pooling
- **deadpool-postgres** (0.14.1) — Raw PostgreSQL connection pooling
- **tokio** (1.51.1) — Async runtime (full features)
- **tokio-postgres** (0.7.17) — Async PostgreSQL driver
- **jsonwebtoken** (9.3.1) — JWT encoding/decoding (HS256)
- **bcrypt** (0.17.0) — Password hashing (cost 12)
- **hmac** (0.12.1) + **sha2** (0.10.8) — HMAC-SHA256 password pepper
- **serde** (1.0.228) — Serialization/deserialization
- **serde_json** (1.0) — JSON support
- **uuid** (1.23.0) — UUID v4 generation with serde
- **chrono** (0.4.39) — Date/time handling
- **config** (0.15.22) — Configuration management
- **dotenvy** (0.15.7) — `.env` file loading
- **thiserror** (2.0.18) — Error derive macros
- **async-trait** (0.1.86) — Async trait definitions
- **hex** (0.4.3) — Hex encoding (refresh tokens)
- **rand** (latest) — Cryptographically secure random bytes for refresh tokens
- **bytes** (1) — Byte buffer utilities
- **reqwest** (0.12) — HTTP client with `json` + `rustls-tls` features
- **url** (2.5.8) — URL parsing and construction
- **tower-http** (0.6) — `CorsLayer` and `TraceLayer` middleware
- **tracing** (0.1) — Structured logging macros
- **tracing-subscriber** (0.3) — Logging with `env-filter`
- **aws-sdk-s3** (1.129.0) — AWS S3 client
- **aws-config** (1.8.15) — AWS SDK config loading
- **aws-credential-types** (1.2.14) — AWS credential primitives
- **utoipa** (5) — OpenAPI code generation via proc macros
- **utoipa-scalar** (0.3) — Scalar UI for OpenAPI docs
- **bigdecimal** (0.4.8) — Arbitrary-precision decimals (rates/amounts)
- **cookie** (0.18) — Cookie parsing with percent-encoding

### Dev Dependencies
- **diesel_migrations** (2.3.1) — Embed and run migrations in tests
- **testcontainers** (0.27.2) + **testcontainers-modules** (0.15.0) — Ephemeral PostgreSQL via Docker
- **mockall** (0.14.0) — Mock generation for trait-based repositories/services
- **wiremock** (0.6.5) — HTTP mocking for Instagram client tests
- **tower** (0.5) — `ServiceExt::oneshot` for HTTP handler tests
- **http-body-util** (0.1) — `BodyExt::collect` for reading response bodies in tests
- **serde_json** (1.0) — JSON construction/assertion in tests

## Database Schema

### `users`
| Column | Type | Constraints |
|--------|------|-------------|
| `id` | UUID | Primary Key |
| `email` | VARCHAR(255) | — |
| `password` | VARCHAR(255) | bcrypt+pepper hash |
| `avatar_url` | VARCHAR(512) | Nullable, S3 URI |

### `profiles`
| Column | Type | Constraints |
|--------|------|-------------|
| `id` | UUID | Primary Key |
| `user_id` | UUID | FK → users |
| `name` | Text | — |
| `bio` | Text | — |
| `niche` | Text | — |
| `avatar_url` | Text | — |
| `username` | Citext | Unique |
| `created_at` | Timestamptz | Nullable |
| `updated_at` | Timestamptz | Nullable |

### `social_handles`
| Column | Type | Constraints |
|--------|------|-------------|
| `id` | UUID | Primary Key |
| `profile_id` | UUID | FK → profiles |
| `platform` | Text | — |
| `handle` | Text | — |
| `url` | Text | — |
| `follower_count` | Int4 | — |
| `engagement_rate` | Numeric | — |
| `updated_at` | Timestamptz | Nullable |
| `last_synced_at` | Timestamptz | Nullable |
| — | — | Unique (profile_id, platform) |

### `rates`
| Column | Type | Constraints |
|--------|------|-------------|
| `id` | UUID | Primary Key |
| `profile_id` | UUID | FK → profiles |
| `type_` | Text | aliased from `"type"` |
| `amount` | Numeric | — |
| — | — | Unique (profile_id, type) |

### `sessions`
| Column | Type | Constraints |
|--------|------|-------------|
| `id` | UUID | Primary Key |
| `user_id` | UUID | FK → users |
| `refresh_token` | VARCHAR(512) | — |
| `user_agent` | Text | Nullable |
| `ip_address` | Text | Nullable |
| `expires_at` | Timestamp | — |
| `revoked_at` | Timestamp | Nullable |
| `created_at` | Timestamp | — |
| `updated_at` | Timestamp | — |

### `oauth_tokens`
| Column | Type | Constraints |
|--------|------|-------------|
| `id` | UUID | Primary Key |
| `profile_id` | UUID | FK → profiles |
| `provider` | Text | e.g. `"instagram"` |
| `access_token` | Text | — |
| `refresh_token` | Text | Nullable |
| `expires_at` | Timestamptz | Nullable |
| `provider_user_id` | Text | — |
| `scopes` | Text | — |
| `created_at` | Timestamptz | — |
| `updated_at` | Timestamptz | — |
| — | — | Unique (profile_id, provider) |

## Application Configuration

Environment variables (using `__` as separator):

| Variable | Description |
|----------|-------------|
| `LISTEN` | Server bind address (e.g. `0.0.0.0:3000`) |
| `PG__*` | PostgreSQL connection config (deadpool-postgres format) |
| `DATABASE_URL` | Diesel database URL |
| `PASSWORD_PEPPER` | Secret pepper for password hashing |
| `JWT_SECRET` | Secret key for JWT signing (HS256) |
| `INSTAGRAM__CLIENT_ID` | Instagram App ID (not Facebook App ID — uses Instagram Login) |
| `INSTAGRAM__CLIENT_SECRET` | Instagram App Secret |
| `INSTAGRAM__REDIRECT_URI` | OAuth redirect URI |
| `INSTAGRAM__GRAPH_API_VERSION` | Graph API version (default: `v25.0`) |
| `TIKTOK__CLIENT_KEY` | TikTok app client key |
| `TIKTOK__CLIENT_SECRET` | TikTok app client secret |
| `TIKTOK__REDIRECT_URI` | TikTok OAuth redirect URI |
| `TIKTOK__SCOPES` | Optional TikTok scopes (default: `user.info.basic,user.info.profile,user.info.stats`) |
| `TIKTOK__API_BASE_URL` | Optional TikTok API base URL (default: `https://open.tiktokapis.com`) |
| `TIKTOK__AUTH_BASE_URL` | Optional TikTok auth base URL (default: `https://www.tiktok.com`) |
| `S3__BUCKET` | S3 bucket name |
| `S3__REGION` | AWS region |
| `S3__ENDPOINT_URL` | Optional custom endpoint (e.g. Moto for tests) |

## Application State (`AppState`)

| Field | Type | Purpose |
|-------|------|---------|
| `pool` | `deadpool_postgres::Pool` | Raw SQL queries |
| `user_service` | `UserService<UserRepository>` | User CRUD + avatar upload |
| `profile_service` | `ProfileService<ProfileRepository>` | Profile CRUD + social handles + rates |
| `profile_repository` | `ProfileRepository` | Direct repo access (Instagram callback) |
| `instagram_service` | `InstagramService` | Instagram OAuth orchestration |
| `instagram_client` | `IgClient` | Direct Graph API client |
| `instagram_oauth_repository` | `OAuthTokenRepository` | OAuth token persistence |
| `session_repository` | `Arc<dyn SessionRepositoryTrait>` | Session lookup (used by AuthUser extractor) |
| `session_service` | `SessionService` | Session create / refresh / logout / list |
| `jwt_secret` | `String` | JWT signing key |
| `storage` | `Arc<dyn StoragePort>` | S3 avatar storage |

## API Endpoints

### Users
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/user` | — | Register new user |
| `POST` | `/user/login` | — | Login |
| `GET` | `/user/me` | Bearer | Get current user |
| `POST` | `/user/profile/image` | Bearer | Upload avatar (multipart, max 5 MB, jpeg/png/webp) |

### Creator Profiles
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/user/profile` | Bearer | Create profile with social handles + rates |
| `GET` | `/user/profile` | Bearer | Get profile with social handles + rates |
| `PUT` | `/user/profile` | Bearer | Update profile |
| `GET` | `/platforms` | — | List supported social platforms |

### Session Management
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/auth/refresh` | Refresh cookie | Rotate refresh token, mint new access token |
| `POST` | `/auth/logout` | Bearer | Revoke current session, clear cookies |
| `POST` | `/auth/logout-all` | Bearer | Delete all sessions |
| `GET` | `/auth/sessions` | Bearer | List active sessions |
| `DELETE` | `/auth/sessions/:id` | Bearer | Delete a specific session |

### Instagram OAuth (Instagram API with Instagram Login)
| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/oauth/instagram` | Bearer | Start Instagram OAuth flow (redirects to Instagram) |
| `GET` | `/oauth/instagram/callback` | State cookie | OAuth callback — exchanges code, persists token |
| `DELETE` | `/oauth/instagram` | Bearer | Disconnect Instagram (deletes token, resets social handle stats) |

### Other
| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1.0/event.list` | List events (legacy raw SQL) |
| `GET` | `/openapi.json` | OpenAPI spec |
| `GET` | `/scalar` | Scalar API explorer UI |

## Authentication Flow

1. **Register / Login** → server creates a session row in DB, returns:
   - Short-lived JWT (15 min) in response body and `token` HttpOnly cookie
   - Long-lived refresh token (7 days) in `refresh_token` HttpOnly cookie
2. **Authenticated requests** → `AuthUser` extractor validates JWT signature + expiry, then checks session is active (not revoked) in DB
3. **Token refresh** (`POST /auth/refresh`) → rotates refresh token (old revoked, new session created), mints new JWT
4. **Logout** → revokes session in DB, clears cookies

## Logging

All modules use `tracing` with `tracing-subscriber` (env-filter, default level: `info`):
- `#[instrument]` on every public function
- `info!` for significant operations
- `debug!` for detailed internals
- `warn!` for recoverable errors (auth failures, missing resources)
- `error!` for failures (DB errors, S3 errors, Instagram API errors)
- Structured fields (e.g. `user_id = %id`, `error = %e`)

Set `RUST_LOG=debug` for verbose output.

## Development Setup

### Prerequisites
- Rust (latest stable)
- Docker and Docker Compose (required for tests — uses testcontainers)
- Diesel CLI: `cargo install diesel_cli --no-default-features --features postgres`
- Nix (optional): `nix-shell` in the project root activates a shell with `postgresql`

### Local Development
```bash
# Start PostgreSQL + Adminer
docker-compose up -d

# Run database migrations
diesel migration run

# Start the server
cargo run
```

### Testing
```bash
# Run all tests (Docker must be running — testcontainers spins up ephemeral PostgreSQL)
cargo test
```

## Constraints & Rules

- **Do NOT manually edit `src/schema.rs`** — auto-generated by Diesel CLI (`diesel print-schema`)
- Follow Hexagonal Architecture when adding domains: `http/`, `usecase/`, `repository/`, optional `crypto/` or `storage/`
- Use `deadpool-diesel` for all ORM-based repository access
- Use `thiserror` for custom error types
- Use `async-trait` for async trait definitions
- Use `mockall` (`#[automock]`) on repository/service traits to enable unit testing
- Keep controllers thin — business logic belongs in `usecase/`
- Integration tests use `testcontainers` — no external DB or S3 required
- New Instagram API calls belong in `IgClient` (`src/instagram/client.rs`); `InstagramService` only orchestrates
- Instagram OAuth uses the "Instagram API with Instagram Login" flow; scope is `instagram_business_basic`
- Session validation on every authenticated request hits the DB — keep session lookups fast
