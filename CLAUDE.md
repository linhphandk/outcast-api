# CLAUDE.md

This file provides context for Claude AI when working on this project.

## Project Overview

**outcast-api** is a Rust-based backend API built with a focus on modularity and clean separation of concerns. It provides user management functionality with authentication capabilities.

## Architecture

The project follows **Hexagonal Architecture** (Ports and Adapters) to ensure business logic remains decoupled from external dependencies like the database and the web framework.

### Directory Structure

```
.
├── .github/workflows/     # CI/CD workflows (Rust CI, Claude Action)
├── migrations/            # Diesel database migrations
│   ├── 00000000000000_diesel_initial_setup/
│   └── 2026-04-12-120305-0000_create_user/
├── src/
│   ├── main.rs            # Application entry point, config, state, and router setup
│   ├── schema.rs          # Diesel auto-generated schema (do NOT edit manually)
│   └── user/              # User domain module
│       ├── mod.rs          # Module declarations
│       ├── crypto/         # Password hashing & JWT utilities
│       ├── http/           # HTTP controllers (Axum handlers/routes)
│       ├── repository/     # Data persistence layer (PostgreSQL via Diesel)
│       └── usecase/        # Business logic / service layer
├── Cargo.toml             # Rust dependencies and project metadata
├── diesel.toml            # Diesel CLI configuration
├── docker-compose.yaml    # Local dev PostgreSQL + Adminer setup
└── README.md
```

### Layers

| Layer | Path | Responsibility |
|-------|------|----------------|
| **HTTP (Delivery)** | `src/user/http/` | Axum request handlers, route definitions, request/response DTOs |
| **Use Case (Service)** | `src/user/usecase/` | Business logic orchestration, coordinates between HTTP and repository |
| **Repository (Persistence)** | `src/user/repository/` | Database access via Diesel ORM, implements repository traits |
| **Crypto** | `src/user/crypto/` | Password hashing (bcrypt) and JWT token management |

### Data Flow

```
HTTP Request → Axum Router → Controller (http/) → Service (usecase/) → Repository (repository/) → PostgreSQL
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
| **Config** | config + dotenvy | 0.15.22 / 0.15.7 |
| **Serialization** | serde | 1.0.228 |
| **Error Handling** | thiserror | 2.0.18 |

## Dependencies (Cargo.toml)

### Runtime Dependencies
- **axum** (0.8.1) — Web framework
- **axum-extra** (0.10.0) — Cookie support
- **axum-macros** (0.5.0) — Debug handler macros
- **diesel** (2.3.7) — ORM with PostgreSQL and UUID support
- **deadpool-diesel** (0.6.1) — Diesel connection pooling
- **deadpool-postgres** (0.14.1) — PostgreSQL connection pooling
- **tokio** (1.51.1) — Async runtime (full features)
- **tokio-postgres** (0.7.17) — Async PostgreSQL driver
- **jsonwebtoken** (9.3.1) — JWT encoding/decoding
- **bcrypt** (0.17.0) — Password hashing
- **hmac** (0.12.1) + **sha2** (0.10.8) — HMAC for password peppering
- **serde** (1.0.228) — Serialization/deserialization
- **uuid** (1.23.0) — UUID generation (v4)
- **chrono** (0.4.39) — Date/time handling
- **config** (0.15.22) — Configuration management
- **dotenvy** (0.15.7) — .env file loading
- **thiserror** (2.0.18) — Error derive macros
- **async-trait** (0.1.86) — Async trait support
- **hex** (0.4.3) — Hex encoding

### Dev Dependencies
- **diesel_migrations** (2.3.1) — Run migrations in tests
- **testcontainers** (0.27.2) + **testcontainers-modules** (0.15.0) — Docker-based integration testing
- **mockall** (0.14.0) — Mocking framework
- **tower** (0.5) — HTTP testing utilities
- **http-body-util** (0.1) — HTTP body utilities for tests
- **serde_json** (1.0) — JSON handling in tests

## Database Schema

### `users` table
| Column | Type | Constraints |
|--------|------|-------------|
| `id` | UUID | Primary Key |
| `email` | VARCHAR(255) | — |
| `password` | VARCHAR(255) | — |

## Application Configuration

The app loads configuration from environment variables (with `__` as separator):

- `LISTEN` — Server bind address (e.g., `0.0.0.0:3000`)
- `PG__*` — PostgreSQL connection config (deadpool-postgres format)
- `DATABASE_URL` — Diesel database URL
- `PASSWORD_PEPPER` — Secret pepper for password hashing
- `JWT_SECRET` — Secret key for JWT token signing

## Application State (`AppState`)

The shared application state contains:
- `pool` — deadpool-postgres connection pool (for raw queries)
- `user_service` — User service with injected repository (for Diesel-based operations)
- `jwt_secret` — JWT signing secret

## Development Setup

### Prerequisites
- Rust (latest stable)
- Docker and Docker Compose
- Diesel CLI (`cargo install diesel_cli --no-default-features --features postgres`)

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
# Run all tests (uses testcontainers — Docker must be running)
cargo test
```

### API Endpoints
- `GET /v1.0/event.list` — List events
- User routes are defined in `src/user/http/user_controller.rs`

## Constraints & Rules

- **Do NOT manually edit `src/schema.rs`** — It is auto-generated by Diesel CLI
- Follow Hexagonal Architecture patterns when adding new domains
- New domain modules should mirror the `user/` structure: `http/`, `usecase/`, `repository/`, and optionally `crypto/`
- Use `deadpool-diesel` for ORM-based database access in repositories
- Use `thiserror` for error types
- Use `async-trait` for async trait definitions
- Keep controllers thin — business logic belongs in the `usecase/` layer
- Integration tests use `testcontainers` for PostgreSQL — no external DB needed for tests
