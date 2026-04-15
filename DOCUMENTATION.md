# Outcast API — Full Documentation

> A Rust-based backend API for influencer/creator profile management with user authentication, session management, and profile CRUD capabilities. Built with Hexagonal Architecture for clean separation of concerns.

---

## Table of Contents

1. [Project Overview](#project-overview)
2. [Architecture](#architecture)
3. [Directory Structure](#directory-structure)
4. [Tech Stack](#tech-stack)
5. [Database Schema](#database-schema)
6. [Application Startup](#application-startup)
7. [API Endpoints](#api-endpoints)
8. [Sequence Diagrams](#sequence-diagrams)
   - [User Registration](#1-user-registration-post-user)
   - [User Login](#2-user-login-post-userlogin)
   - [Get Current User](#3-get-current-user-get-userme)
   - [Token Refresh](#4-token-refresh-post-authrefresh)
   - [Auth Extraction (Middleware)](#5-authentication-extraction-middleware)
   - [Profile Creation](#6-profile-creation-internal-flow)
9. [Authentication & Security](#authentication--security)
   - [Password Security](#password-security)
   - [JWT Access Tokens](#jwt-access-tokens)
   - [Refresh Tokens](#refresh-tokens)
   - [Cookie Management](#cookie-management)
   - [Token Rotation & Reuse Detection](#token-rotation--reuse-detection)
10. [Session Management](#session-management)
11. [Error Handling](#error-handling)
12. [Configuration](#configuration)
13. [Testing Strategy](#testing-strategy)
14. [Development Setup](#development-setup)

---

## Project Overview

**Outcast API** is a backend service designed for managing influencer/creator profiles. It provides:

- **User Management** — Registration, login, and user profile retrieval
- **Session Management** — Secure, server-side session tracking with refresh token rotation
- **Profile Management** — Creator profiles with social media handles and rate cards
- **Authentication** — JWT-based access tokens with HttpOnly cookie transport and refresh token rotation

The API is built in Rust using the Axum web framework and PostgreSQL for persistence, following Hexagonal Architecture (Ports and Adapters) to keep business logic decoupled from infrastructure concerns.

---

## Architecture

The project follows **Hexagonal Architecture** (also known as Ports and Adapters), which separates the application into distinct layers:

```
┌──────────────────────────────────────────────────────────────┐
│                     HTTP Layer (Delivery)                     │
│         Controllers, Route Definitions, Request/Response DTOs │
│         Axum handlers, extractors, cookie management          │
├──────────────────────────────────────────────────────────────┤
│                     Use Case Layer (Service)                  │
│         Business logic orchestration                          │
│         Coordinates between HTTP and Repository layers        │
├──────────────────────────────────────────────────────────────┤
│                     Repository Layer (Persistence)            │
│         Trait definitions (ports) and implementations         │
│         Database access via Diesel ORM                        │
├──────────────────────────────────────────────────────────────┤
│                     Crypto Layer (Cross-Cutting)              │
│         Password hashing (bcrypt + HMAC pepper)               │
│         JWT token creation and verification                   │
├──────────────────────────────────────────────────────────────┤
│                     Infrastructure                            │
│         PostgreSQL, Connection Pools, Configuration            │
└──────────────────────────────────────────────────────────────┘
```

### Key Principles

- **Dependency Inversion** — Service layers depend on trait interfaces (ports), not concrete implementations. Repository traits are defined in the repository module; concrete implementations can be swapped (e.g., for mocks in tests).
- **Thin Controllers** — HTTP handlers only handle request parsing, response formatting, and cookie management. Business logic lives in the use-case (service) layer.
- **Testability** — Every layer boundary uses traits (`UserRepositoryTrait`, `SessionRepositoryTrait`, `ProfileRepositoryTrait`) enabling mock-based unit tests via `mockall` and Docker-based integration tests via `testcontainers`.

---

## Directory Structure

```
.
├── .github/workflows/          # CI/CD workflows (Rust CI, Claude Action)
├── migrations/                 # Diesel database migrations
│   ├── 00000000000000_diesel_initial_setup/
│   ├── 2026-04-12-120305-0000_create_user/
│   │   ├── up.sql              # Creates `users` table
│   │   └── down.sql
│   ├── 2026-04-14-134310-0001_create_profiles_social_handles_rates/
│   │   ├── up.sql              # Creates `profiles`, `social_handles`, `rates` tables
│   │   └── down.sql
│   └── 2026-04-15-080000-0002_create_sessions/
│       ├── up.sql              # Creates `sessions` table with indexes
│       └── down.sql
├── src/
│   ├── main.rs                 # Application entry point, config, state, and router setup
│   ├── schema.rs               # Diesel auto-generated schema (DO NOT edit manually)
│   ├── user/                   # User domain module
│   │   ├── mod.rs              # Module declarations
│   │   ├── crypto/
│   │   │   ├── mod.rs
│   │   │   ├── hash_password.rs # HMAC-SHA256 pepper + bcrypt password hashing
│   │   │   └── jwt.rs           # JWT creation (create_jwt) and verification (verify_jwt)
│   │   ├── http/
│   │   │   ├── mod.rs
│   │   │   ├── auth_extractor.rs # AuthUser extractor: validates JWT + session from Bearer/Cookie
│   │   │   └── user_controller.rs # POST /user, POST /user/login, GET /user/me
│   │   ├── repository/
│   │   │   ├── mod.rs
│   │   │   ├── user_repository.rs      # UserRepositoryTrait + Diesel implementation
│   │   │   └── profile_repository.rs   # ProfileRepositoryTrait + Diesel implementation
│   │   └── usecase/
│   │       ├── mod.rs
│   │       ├── user_service.rs    # UserService: create, authenticate, get_me
│   │       └── profile_service.rs # ProfileService: add_profile (with social handles + rates)
│   └── session/                # Session domain module
│       ├── mod.rs              # Module declarations
│       ├── http/
│       │   ├── mod.rs
│       │   ├── cookies.rs      # set_auth_cookies, clear_auth_cookies helpers
│       │   └── session_controller.rs # POST /auth/refresh, POST /auth/logout, etc.
│       ├── repository/
│       │   ├── mod.rs
│       │   └── session_repository.rs # SessionRepositoryTrait + Diesel implementation
│       └── usecase/
│           ├── mod.rs
│           └── session_service.rs    # SessionService: create_session, refresh
├── Cargo.toml                  # Rust dependencies and project metadata
├── diesel.toml                 # Diesel CLI configuration
├── docker-compose.yaml         # Local dev PostgreSQL + Adminer setup
├── CLAUDE.md                   # AI assistant context file
└── README.md                   # Brief project readme
```

---

## Tech Stack

| Component              | Technology                     | Version   |
|------------------------|--------------------------------|-----------|
| Language               | Rust                           | Edition 2024 |
| Web Framework          | Axum                           | 0.8.1     |
| Database               | PostgreSQL                     | (via Docker) |
| ORM                    | Diesel                         | 2.3.7     |
| Connection Pooling     | deadpool-diesel / deadpool-postgres | 0.6.1 / 0.14.1 |
| Async Runtime          | Tokio                          | 1.51.1    |
| JWT                    | jsonwebtoken                   | 9.3.1     |
| Password Hashing       | bcrypt                         | 0.17.0    |
| Password Pepper        | HMAC-SHA256 (hmac + sha2)      | 0.12.1 / 0.10.8 |
| Configuration          | config + dotenvy               | 0.15.22 / 0.15.7 |
| Serialization          | serde + serde_json             | 1.0.228   |
| Error Handling         | thiserror                      | 2.0.18    |
| API Documentation      | utoipa + utoipa-scalar          | 5.x / 0.3 |
| HTTP Middleware         | tower-http (CORS, tracing)     | 0.6       |
| Logging / Tracing      | tracing + tracing-subscriber   | 0.1 / 0.3 |
| Testing (mocking)      | mockall                        | 0.14.0    |
| Testing (integration)  | testcontainers                 | 0.27.2    |

---

## Database Schema

### Entity Relationship Diagram

```
┌──────────────────┐         ┌───────────────────────┐
│      users       │         │       sessions        │
├──────────────────┤         ├───────────────────────┤
│ id (UUID, PK)    │◄────────│ user_id (UUID, FK)    │
│ email (VARCHAR)  │    1:N  │ id (UUID, PK)         │
│ password (VARCHAR│         │ refresh_token (VARCHAR)│
└──────────────────┘         │ user_agent (TEXT)      │
        │                    │ ip_address (VARCHAR)   │
        │ 1:N                │ expires_at (TIMESTAMP) │
        ▼                    │ revoked_at (TIMESTAMP) │
┌──────────────────┐         │ created_at (TIMESTAMP) │
│     profiles     │         │ updated_at (TIMESTAMP) │
├──────────────────┤         └───────────────────────┘
│ id (UUID, PK)    │
│ user_id (UUID,FK)│
│ name (TEXT)      │
│ bio (TEXT)       │
│ niche (TEXT)     │
│ avatar_url (TEXT)│
│ username (CITEXT)│  UNIQUE
│ updated_at       │
│ created_at       │
└──────────────────┘
        │
        │ 1:N                     1:N
        ▼                         ▼
┌──────────────┐         ┌──────────────┐
│social_handles│         │    rates     │
├──────────────┤         ├──────────────┤
│ id (UUID, PK)│         │ id (UUID, PK)│
│ profile_id   │         │ profile_id   │
│ (UUID, FK)   │         │ (UUID, FK)   │
│ platform     │         │ type (TEXT)  │
│ handle       │         │ amount       │
│ url          │         │ (NUMERIC)    │
│ follower_cnt │         └──────────────┘
│ updated_at   │
└──────────────┘
```

### Table Details

#### `users`
| Column     | Type          | Constraints                        |
|------------|---------------|------------------------------------|
| `id`       | UUID          | PRIMARY KEY, DEFAULT uuid_generate_v4() |
| `email`    | VARCHAR(255)  | UNIQUE, NOT NULL                   |
| `password` | VARCHAR(255)  | NOT NULL (bcrypt hash)             |

#### `sessions`
| Column          | Type          | Constraints                          |
|-----------------|---------------|--------------------------------------|
| `id`            | UUID          | PRIMARY KEY, DEFAULT uuid_generate_v4() |
| `user_id`       | UUID          | NOT NULL, FK → users(id) ON DELETE CASCADE |
| `refresh_token` | VARCHAR(512)  | UNIQUE, NOT NULL                     |
| `user_agent`    | TEXT          | Nullable                             |
| `ip_address`    | VARCHAR(45)   | Nullable                             |
| `expires_at`    | TIMESTAMP     | NOT NULL                             |
| `revoked_at`    | TIMESTAMP     | Nullable (set when revoked/rotated)  |
| `created_at`    | TIMESTAMP     | NOT NULL, DEFAULT NOW()              |
| `updated_at`    | TIMESTAMP     | NOT NULL, DEFAULT NOW()              |

**Indexes:** `idx_sessions_user_id`, `idx_sessions_refresh_token`

#### `profiles`
| Column       | Type        | Constraints                          |
|--------------|-------------|--------------------------------------|
| `id`         | UUID        | PRIMARY KEY, DEFAULT gen_random_uuid() |
| `user_id`    | UUID        | NOT NULL, FK → users(id) ON DELETE CASCADE |
| `name`       | TEXT        | NOT NULL                             |
| `bio`        | TEXT        | NOT NULL                             |
| `niche`      | TEXT        | NOT NULL                             |
| `avatar_url` | TEXT        | NOT NULL                             |
| `username`   | CITEXT      | NOT NULL, UNIQUE (case-insensitive)  |
| `updated_at` | TIMESTAMPTZ | DEFAULT now()                        |
| `created_at` | TIMESTAMPTZ | DEFAULT now()                        |

#### `social_handles`
| Column           | Type    | Constraints                                |
|------------------|---------|--------------------------------------------|
| `id`             | UUID    | PRIMARY KEY, DEFAULT gen_random_uuid()     |
| `profile_id`     | UUID    | NOT NULL, FK → profiles(id) ON DELETE CASCADE |
| `platform`       | TEXT    | NOT NULL, CHECK IN ('instagram', 'tiktok', 'youtube') |
| `handle`         | TEXT    | NOT NULL                                   |
| `url`            | TEXT    | NOT NULL                                   |
| `follower_count` | INT     | NOT NULL, CHECK >= 0                       |
| `updated_at`     | TIMESTAMPTZ | DEFAULT now()                          |

**Constraints:** UNIQUE (profile_id, platform) — one handle per platform per profile

#### `rates`
| Column       | Type           | Constraints                              |
|--------------|----------------|------------------------------------------|
| `id`         | UUID           | PRIMARY KEY, DEFAULT gen_random_uuid()   |
| `profile_id` | UUID           | NOT NULL, FK → profiles(id) ON DELETE CASCADE |
| `type`       | TEXT           | NOT NULL, CHECK IN ('post', 'story', 'reel') |
| `amount`     | NUMERIC(10,2)  | NOT NULL, CHECK >= 0                     |

**Constraints:** UNIQUE (profile_id, type) — one rate per type per profile

---

## Application Startup

When the application starts (`main.rs`), the following initialization sequence occurs:

```
┌─────────────────────────────────────────────────────┐
│                   Application Startup                │
├─────────────────────────────────────────────────────┤
│ 1. Load .env file (dotenvy)                         │
│ 2. Initialize tracing subscriber (log levels)       │
│ 3. Load Config from environment variables           │
│ 4. Create deadpool-postgres pool (for raw queries)  │
│ 5. Create deadpool-diesel pool (for ORM queries)    │
│ 6. Instantiate UserRepository (with diesel pool)    │
│ 7. Instantiate SessionRepository (with diesel pool) │
│ 8. Instantiate SessionService                       │
│    (with session_repo + user_repo)                  │
│ 9. Instantiate UserService                          │
│    (with user_repo + password_pepper)               │
│ 10. Build AppState (pool, user_service,             │
│     jwt_secret, session_repository,                 │
│     session_service)                                │
│ 11. Build Axum Router with all routes               │
│ 12. Apply middleware (CORS, Tracing)                │
│ 13. Bind TCP listener and start serving             │
└─────────────────────────────────────────────────────┘
```

### AppState

The shared application state (`AppState`) is an Axum extractor-compatible struct:

```rust
pub struct AppState {
    pub pool: deadpool_postgres::Pool,           // Raw PostgreSQL connection pool
    pub user_service: UserService<UserRepository>, // User business logic
    pub jwt_secret: String,                        // JWT signing key
    pub session_repository: Arc<dyn SessionRepositoryTrait>, // Session persistence
    pub session_service: SessionService,           // Session business logic
}
```

Each field implements `FromRef<AppState>`, allowing Axum handlers to extract individual components via `State<T>`.

---

## API Endpoints

### Route Table

| Method | Path                   | Handler            | Auth Required | Description                          |
|--------|------------------------|---------------------|---------------|--------------------------------------|
| POST   | `/user`                | `create_user`       | No            | Register a new user                  |
| POST   | `/user/login`          | `login_user`        | No            | Authenticate and get tokens          |
| GET    | `/user/me`             | `get_me`            | Yes (JWT)     | Get current authenticated user info  |
| POST   | `/auth/refresh`        | `refresh_session`   | Cookie        | Rotate refresh token, get new tokens |
| POST   | `/auth/logout`         | `logout`            | TBD           | Logout (not yet implemented)         |
| POST   | `/auth/logout-all`     | `logout_all`        | TBD           | Logout all sessions (not yet implemented) |
| GET    | `/auth/sessions`       | `list_sessions`     | TBD           | List active sessions (not yet implemented) |
| DELETE | `/auth/sessions/{id}`  | `delete_session`    | TBD           | Delete a session (not yet implemented) |
| GET    | `/v1.0/event.list`     | `event_list`        | No            | List events (raw SQL query)          |
| GET    | `/openapi.json`        | (inline)            | No            | OpenAPI spec (JSON)                  |
| GET    | `/scalar`              | Scalar UI           | No            | Interactive API documentation UI     |

### Request/Response DTOs

#### `POST /user` — Create User
**Request:**
```json
{
  "email": "user@example.com",
  "password": "securepassword123"
}
```
**Response (201 Created):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "user@example.com",
  "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
}
```
**Cookies Set:** `token` (access), `refresh_token` (refresh)

#### `POST /user/login` — Login
**Request:**
```json
{
  "email": "user@example.com",
  "password": "securepassword123"
}
```
**Response (200 OK):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "user@example.com",
  "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
}
```
**Cookies Set:** `token` (access), `refresh_token` (refresh)

#### `GET /user/me` — Get Current User
**Headers:** `Authorization: Bearer <JWT>` or `token` cookie
**Response (200 OK):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "user@example.com"
}
```

#### `POST /auth/refresh` — Refresh Tokens
**Cookie Required:** `refresh_token`
**Response (200 OK):**
```json
{
  "access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
}
```
**Cookies Set:** New `token` (access) and `refresh_token` (refresh)

---

## Sequence Diagrams

### 1. User Registration (`POST /user`)

```
Client                  Controller            UserService       UserRepository    SessionService     SessionRepository       DB
  │                         │                       │                  │                  │                    │              │
  │  POST /user             │                       │                  │                  │                    │              │
  │  {email, password}      │                       │                  │                  │                    │              │
  │────────────────────────►│                       │                  │                  │                    │              │
  │                         │                       │                  │                  │                    │              │
  │                         │  create(email, pass)  │                  │                  │                    │              │
  │                         │──────────────────────►│                  │                  │                    │              │
  │                         │                       │                  │                  │                    │              │
  │                         │                       │  HMAC-SHA256     │                  │                    │              │
  │                         │                       │  (password +     │                  │                    │              │
  │                         │                       │   pepper)        │                  │                    │              │
  │                         │                       │  then bcrypt     │                  │                    │              │
  │                         │                       │  hash            │                  │                    │              │
  │                         │                       │                  │                  │                    │              │
  │                         │                       │  create(email,   │                  │                    │              │
  │                         │                       │   hashed_pass)   │                  │                    │              │
  │                         │                       │─────────────────►│                  │                    │              │
  │                         │                       │                  │                  │                    │              │
  │                         │                       │                  │  INSERT INTO     │                    │              │
  │                         │                       │                  │  users            │                    │              │
  │                         │                       │                  │─────────────────────────────────────────────────────►│
  │                         │                       │                  │                  │                    │              │
  │                         │                       │                  │  User{id, email} │                    │              │
  │                         │                       │                  │◄─────────────────────────────────────────────────────│
  │                         │                       │                  │                  │                    │              │
  │                         │                       │  Ok(User)        │                  │                    │              │
  │                         │                       │◄─────────────────│                  │                    │              │
  │                         │                       │                  │                  │                    │              │
  │                         │  Ok(User)             │                  │                  │                    │              │
  │                         │◄──────────────────────│                  │                  │                    │              │
  │                         │                       │                  │                  │                    │              │
  │                         │  create_session(user_id, email, ..., jwt_secret)            │                    │              │
  │                         │──────────────────────────────────────────────────────────►  │                    │              │
  │                         │                       │                  │                  │                    │              │
  │                         │                       │                  │                  │  Generate 64-byte  │              │
  │                         │                       │                  │                  │  random refresh    │              │
  │                         │                       │                  │                  │  token (128 hex)   │              │
  │                         │                       │                  │                  │                    │              │
  │                         │                       │                  │                  │  create(user_id,   │              │
  │                         │                       │                  │                  │   refresh_token,   │              │
  │                         │                       │                  │                  │   expires_at)      │              │
  │                         │                       │                  │                  │───────────────────►│              │
  │                         │                       │                  │                  │                    │              │
  │                         │                       │                  │                  │                    │ INSERT INTO  │
  │                         │                       │                  │                  │                    │ sessions     │
  │                         │                       │                  │                  │                    │─────────────►│
  │                         │                       │                  │                  │                    │              │
  │                         │                       │                  │                  │                    │ Session{id}  │
  │                         │                       │                  │                  │                    │◄─────────────│
  │                         │                       │                  │                  │                    │              │
  │                         │                       │                  │                  │  Ok(Session)       │              │
  │                         │                       │                  │                  │◄───────────────────│              │
  │                         │                       │                  │                  │                    │              │
  │                         │                       │                  │                  │  create_jwt(       │              │
  │                         │                       │                  │                  │   user_id, email,  │              │
  │                         │                       │                  │                  │   session_id,      │              │
  │                         │                       │                  │                  │   jwt_secret)      │              │
  │                         │                       │                  │                  │                    │              │
  │                         │  Ok(RefreshTokens{access_token, refresh_token})             │                    │              │
  │                         │◄─────────────────────────────────────────────────────────── │                    │              │
  │                         │                       │                  │                  │                    │              │
  │                         │  set_auth_cookies(    │                  │                  │                    │              │
  │                         │   jar, access,        │                  │                  │                    │              │
  │                         │   refresh)            │                  │                  │                    │              │
  │                         │                       │                  │                  │                    │              │
  │  201 Created            │                       │                  │                  │                    │              │
  │  Set-Cookie: token=...  │                       │                  │                  │                    │              │
  │  Set-Cookie:            │                       │                  │                  │                    │              │
  │   refresh_token=...     │                       │                  │                  │                    │              │
  │  {id, email, token}     │                       │                  │                  │                    │              │
  │◄────────────────────────│                       │                  │                  │                    │              │
```

### 2. User Login (`POST /user/login`)

```
Client                  Controller            UserService       UserRepository          SessionService      DB
  │                         │                       │                  │                       │              │
  │  POST /user/login       │                       │                  │                       │              │
  │  {email, password}      │                       │                  │                       │              │
  │────────────────────────►│                       │                  │                       │              │
  │                         │                       │                  │                       │              │
  │                         │  authenticate(email,  │                  │                       │              │
  │                         │   password)           │                  │                       │              │
  │                         │──────────────────────►│                  │                       │              │
  │                         │                       │                  │                       │              │
  │                         │                       │  find_by_email   │                       │              │
  │                         │                       │  (email)         │                       │              │
  │                         │                       │─────────────────►│                       │              │
  │                         │                       │                  │                       │              │
  │                         │                       │                  │  SELECT * FROM users  │              │
  │                         │                       │                  │  WHERE email = ?      │              │
  │                         │                       │                  │──────────────────────────────────────►│
  │                         │                       │                  │                       │              │
  │                         │                       │                  │  Ok(Some(User))       │              │
  │                         │                       │                  │◄──────────────────────────────────────│
  │                         │                       │                  │                       │              │
  │                         │                       │  Ok(Some(User))  │                       │              │
  │                         │                       │◄─────────────────│                       │              │
  │                         │                       │                  │                       │              │
  │                         │                       │  verify_password │                       │              │
  │                         │                       │  (input_password,│                       │              │
  │                         │                       │   stored_hash,   │                       │              │
  │                         │                       │   pepper)        │                       │              │
  │                         │                       │                  │                       │              │
  │                         │                       │  1. HMAC-SHA256  │                       │              │
  │                         │                       │     (password +  │                       │              │
  │                         │                       │      pepper)     │                       │              │
  │                         │                       │  2. Hex-encode   │                       │              │
  │                         │                       │  3. bcrypt verify│                       │              │
  │                         │                       │     against      │                       │              │
  │                         │                       │     stored hash  │                       │              │
  │                         │                       │                  │                       │              │
  │                         │                       │  ✓ Match!        │                       │              │
  │                         │                       │                  │                       │              │
  │                         │  Ok(User)             │                  │                       │              │
  │                         │◄──────────────────────│                  │                       │              │
  │                         │                       │                  │                       │              │
  │                         │  create_session(user_id, email, jwt_secret)                     │              │
  │                         │────────────────────────────────────────────────────────────────►│              │
  │                         │                       │                  │                       │              │
  │                         │  (Same session creation flow as registration above)             │              │
  │                         │                       │                  │                       │              │
  │                         │  Ok(RefreshTokens)    │                  │                       │              │
  │                         │◄────────────────────────────────────────────────────────────────│              │
  │                         │                       │                  │                       │              │
  │  200 OK                 │                       │                  │                       │              │
  │  Set-Cookie: token=...  │                       │                  │                       │              │
  │  Set-Cookie:            │                       │                  │                       │              │
  │   refresh_token=...     │                       │                  │                       │              │
  │  {id, email, token}     │                       │                  │                       │              │
  │◄────────────────────────│                       │                  │                       │              │
```

**Error Cases:**
- If user not found → `401 Unauthorized` ("Invalid email or password")
- If password mismatch → `401 Unauthorized` ("Invalid email or password")
- If repository/hash error → `500 Internal Server Error`

### 3. Get Current User (`GET /user/me`)

```
Client                 AuthExtractor        SessionRepository       Controller          UserService       UserRepository       DB
  │                         │                       │                    │                    │                  │              │
  │  GET /user/me           │                       │                    │                    │                  │              │
  │  Authorization: Bearer  │                       │                    │                    │                  │              │
  │  <JWT>                  │                       │                    │                    │                  │              │
  │  (or Cookie: token=...) │                       │                    │                    │                  │              │
  │────────────────────────►│                       │                    │                    │                  │              │
  │                         │                       │                    │                    │                  │              │
  │                         │  1. Extract JWT from  │                    │                    │                  │              │
  │                         │     Bearer header     │                    │                    │                  │              │
  │                         │     OR token cookie   │                    │                    │                  │              │
  │                         │                       │                    │                    │                  │              │
  │                         │  2. verify_jwt(       │                    │                    │                  │              │
  │                         │     token, secret)    │                    │                    │                  │              │
  │                         │  → Claims{sub,email,  │                    │                    │                  │              │
  │                         │    session_id,exp,iat} │                    │                    │                  │              │
  │                         │                       │                    │                    │                  │              │
  │                         │  3. find_by_id        │                    │                    │                  │              │
  │                         │     (session_id)      │                    │                    │                  │              │
  │                         │──────────────────────►│                    │                    │                  │              │
  │                         │                       │                    │                    │                  │              │
  │                         │                       │  SELECT * FROM     │                    │                  │              │
  │                         │                       │  sessions WHERE    │                    │                  │              │
  │                         │                       │  id = ?            │                    │                  │              │
  │                         │                       │───────────────────────────────────────────────────────────────────────►  │
  │                         │                       │                    │                    │                  │              │
  │                         │  Ok(Some(Session))    │                    │                    │                  │              │
  │                         │◄──────────────────────│                    │                    │                  │              │
  │                         │                       │                    │                    │                  │              │
  │                         │  4. Check             │                    │                    │                  │              │
  │                         │     session.revoked_at│                    │                    │                  │              │
  │                         │     is None           │                    │                    │                  │              │
  │                         │     ✓ Not revoked     │                    │                    │                  │              │
  │                         │                       │                    │                    │                  │              │
  │                         │  → AuthUser{user_id,  │                    │                    │                  │              │
  │                         │    email, session_id}  │                    │                    │                  │              │
  │                         │─────────────────────────────────────────►  │                    │                  │              │
  │                         │                       │                    │                    │                  │              │
  │                         │                       │                    │  get_me(user_id)   │                  │              │
  │                         │                       │                    │───────────────────►│                  │              │
  │                         │                       │                    │                    │                  │              │
  │                         │                       │                    │                    │  find_by_id      │              │
  │                         │                       │                    │                    │  (user_id)       │              │
  │                         │                       │                    │                    │─────────────────►│              │
  │                         │                       │                    │                    │                  │              │
  │                         │                       │                    │                    │                  │ SELECT * FROM│
  │                         │                       │                    │                    │                  │ users WHERE  │
  │                         │                       │                    │                    │                  │ id = ?       │
  │                         │                       │                    │                    │                  │─────────────►│
  │                         │                       │                    │                    │                  │              │
  │                         │                       │                    │                    │  Ok(Some(User))  │              │
  │                         │                       │                    │                    │◄─────────────────│              │
  │                         │                       │                    │                    │                  │              │
  │                         │                       │                    │  Ok(User)          │                  │              │
  │                         │                       │                    │◄───────────────────│                  │              │
  │                         │                       │                    │                    │                  │              │
  │  200 OK                 │                       │                    │                    │                  │              │
  │  {id, email}            │                       │                    │                    │                  │              │
  │◄──────────────────────────────────────────────────────────────────── │                    │                  │              │
```

### 4. Token Refresh (`POST /auth/refresh`)

```
Client               SessionController      SessionService      SessionRepository     UserRepository        DB
  │                         │                       │                    │                    │                  │
  │  POST /auth/refresh     │                       │                    │                    │                  │
  │  Cookie:                │                       │                    │                    │                  │
  │   refresh_token=abc123  │                       │                    │                    │                  │
  │────────────────────────►│                       │                    │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │  1. Extract           │                    │                    │                  │
  │                         │  refresh_token cookie │                    │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │  refresh(old_token,   │                    │                    │                  │
  │                         │   jwt_secret)         │                    │                    │                  │
  │                         │──────────────────────►│                    │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  Step 1: Look up  │                    │                  │
  │                         │                       │  session by token  │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  find_by_          │                    │                  │
  │                         │                       │  refresh_token     │                    │                  │
  │                         │                       │  (old_token)       │                    │                  │
  │                         │                       │───────────────────►│                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │                    │  SELECT * FROM     │                  │
  │                         │                       │                    │  sessions WHERE    │                  │
  │                         │                       │                    │  refresh_token = ? │                  │
  │                         │                       │                    │──────────────────────────────────────►│
  │                         │                       │                    │                    │                  │
  │                         │                       │  Ok(Some(Session)) │                    │                  │
  │                         │                       │◄───────────────────│                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  Step 2: Reuse    │                    │                  │
  │                         │                       │  detection check   │                    │                  │
  │                         │                       │  (revoked_at must  │                    │                  │
  │                         │                       │   be None)         │                    │                  │
  │                         │                       │  ✓ Not revoked     │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  Step 3: Expiry   │                    │                  │
  │                         │                       │  check             │                    │                  │
  │                         │                       │  (expires_at >     │                    │                  │
  │                         │                       │   now)             │                    │                  │
  │                         │                       │  ✓ Not expired     │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  Step 4: Revoke   │                    │                  │
  │                         │                       │  old session       │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  revoke(           │                    │                  │
  │                         │                       │   session.id)      │                    │                  │
  │                         │                       │───────────────────►│                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │                    │  UPDATE sessions   │                  │
  │                         │                       │                    │  SET revoked_at =  │                  │
  │                         │                       │                    │  NOW() WHERE id = ?│                  │
  │                         │                       │                    │──────────────────────────────────────►│
  │                         │                       │                    │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  Step 5: Fetch    │                    │                  │
  │                         │                       │  user (for email   │                    │                  │
  │                         │                       │  in new JWT)       │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  find_by_id        │                    │                  │
  │                         │                       │  (session.user_id) │                    │                  │
  │                         │                       │───────────────────────────────────────►│                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │                    │                    │ SELECT * FROM    │
  │                         │                       │                    │                    │ users WHERE      │
  │                         │                       │                    │                    │ id = ?           │
  │                         │                       │                    │                    │─────────────────►│
  │                         │                       │                    │                    │                  │
  │                         │                       │  Ok(User)          │                    │                  │
  │                         │                       │◄──────────────────────────────────────── │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  Step 6: Generate │                    │                  │
  │                         │                       │  new refresh token │                    │                  │
  │                         │                       │  (64 random bytes  │                    │                  │
  │                         │                       │  → 128 hex chars)  │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  Step 7: Create   │                    │                  │
  │                         │                       │  new session row   │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  create(user_id,   │                    │                  │
  │                         │                       │   new_token,       │                    │                  │
  │                         │                       │   user_agent,      │                    │                  │
  │                         │                       │   ip, expires_at)  │                    │                  │
  │                         │                       │───────────────────►│                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │                    │  INSERT INTO       │                  │
  │                         │                       │                    │  sessions          │                  │
  │                         │                       │                    │──────────────────────────────────────►│
  │                         │                       │                    │                    │                  │
  │                         │                       │  Ok(new Session)   │                    │                  │
  │                         │                       │◄───────────────────│                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │                       │  Step 8: Mint     │                    │                  │
  │                         │                       │  new JWT access    │                    │                  │
  │                         │                       │  token with        │                    │                  │
  │                         │                       │  new session_id    │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │  Ok(RefreshTokens{    │                    │                    │                  │
  │                         │   access_token,       │                    │                    │                  │
  │                         │   refresh_token})     │                    │                    │                  │
  │                         │◄──────────────────────│                    │                    │                  │
  │                         │                       │                    │                    │                  │
  │                         │  set_auth_cookies(    │                    │                    │                  │
  │                         │   jar, access,        │                    │                    │                  │
  │                         │   refresh)            │                    │                    │                  │
  │                         │                       │                    │                    │                  │
  │  200 OK                 │                       │                    │                    │                  │
  │  Set-Cookie: token=...  │                       │                    │                    │                  │
  │  Set-Cookie:            │                       │                    │                    │                  │
  │   refresh_token=...     │                       │                    │                    │                  │
  │  {access_token}         │                       │                    │                    │                  │
  │◄────────────────────────│                       │                    │                    │                  │
```

**Error Cases:**
- Missing `refresh_token` cookie → `401 Unauthorized`
- Token not found in DB → `401 Unauthorized`
- Session already revoked (reuse detection) → `401 Unauthorized`
- Session expired → `401 Unauthorized`

### 5. Authentication Extraction (Middleware)

The `AuthUser` extractor is an Axum `FromRequestParts` implementation that runs before any handler requiring authentication. It performs a multi-step validation:

```
Incoming Request
       │
       ▼
┌──────────────────────────────┐
│ 1. Try Authorization: Bearer │
│    header                    │
│                              │
│    Found? ──────► Use token  │
│    Not found? ──► Try cookie │
├──────────────────────────────┤
│ 2. Try `token` cookie        │
│                              │
│    Found? ──────► Use token  │
│    Not found? ──► 401 Error  │
├──────────────────────────────┤
│ 3. verify_jwt(token, secret) │
│                              │
│    Valid? ──────► Extract     │
│                  Claims{sub, │
│                  email,      │
│                  session_id, │
│                  exp, iat}   │
│    Invalid? ───► 401 Error   │
├──────────────────────────────┤
│ 4. Look up session in DB     │
│    find_by_id(session_id)    │
│                              │
│    Found? ──────► Continue   │
│    Not found? ──► 401 Error  │
├──────────────────────────────┤
│ 5. Check session.revoked_at  │
│                              │
│    None? ──────► ✓ Active    │
│    Some(date) ─► 401 Error   │
├──────────────────────────────┤
│ 6. Return AuthUser           │
│    {user_id, email,          │
│     session_id}              │
└──────────────────────────────┘
```

### 6. Profile Creation (Internal Flow)

Profile creation is handled by `ProfileService` and `ProfileRepository`. It creates a profile with associated social handles and rates in a single operation:

```
Caller                ProfileService        ProfileRepository                             DB
  │                         │                       │                                       │
  │  add_profile(           │                       │                                       │
  │   user_id, name, bio,   │                       │                                       │
  │   niche, avatar_url,    │                       │                                       │
  │   username,             │                       │                                       │
  │   social_handles[],     │                       │                                       │
  │   rates[])              │                       │                                       │
  │────────────────────────►│                       │                                       │
  │                         │                       │                                       │
  │                         │  create_with_details( │                                       │
  │                         │   user_id, name, bio, │                                       │
  │                         │   niche, avatar_url,  │                                       │
  │                         │   username,           │                                       │
  │                         │   social_handles[],   │                                       │
  │                         │   rates[])            │                                       │
  │                         │──────────────────────►│                                       │
  │                         │                       │                                       │
  │                         │                       │  1. INSERT INTO profiles               │
  │                         │                       │     (user_id, name, bio, niche,        │
  │                         │                       │      avatar_url, username)             │
  │                         │                       │──────────────────────────────────────►│
  │                         │                       │                                       │
  │                         │                       │  Profile{id, ...}                     │
  │                         │                       │◄──────────────────────────────────────│
  │                         │                       │                                       │
  │                         │                       │  2. For each social handle:            │
  │                         │                       │     INSERT INTO social_handles         │
  │                         │                       │     (profile_id, platform, handle,     │
  │                         │                       │      url, follower_count)              │
  │                         │                       │──────────────────────────────────────►│
  │                         │                       │                                       │
  │                         │                       │  3. For each rate:                     │
  │                         │                       │     INSERT INTO rates                  │
  │                         │                       │     (profile_id, type, amount)         │
  │                         │                       │──────────────────────────────────────►│
  │                         │                       │                                       │
  │                         │  Ok(ProfileWithDetails│                                       │
  │                         │   {profile,           │                                       │
  │                         │    social_handles[],  │                                       │
  │                         │    rates[]})          │                                       │
  │                         │◄──────────────────────│                                       │
  │                         │                       │                                       │
  │  Ok(ProfileWithDetails) │                       │                                       │
  │◄────────────────────────│                       │                                       │
```

---

## Authentication & Security

### Password Security

Password storage uses a **two-layer** approach:

```
┌─────────────────────────────────────────────────────┐
│                  Password Hashing                    │
│                                                     │
│  Input: plaintext password + server pepper           │
│                                                     │
│  Step 1: HMAC-SHA256                                │
│  ┌───────────────────────────────────────────┐      │
│  │ peppered = HMAC-SHA256(                   │      │
│  │   key = pepper,                            │      │
│  │   message = password                       │      │
│  │ )                                          │      │
│  └───────────────────────────────────────────┘      │
│                                                     │
│  Step 2: Hex-encode the HMAC output                 │
│  ┌───────────────────────────────────────────┐      │
│  │ hex_input = hex::encode(peppered)          │      │
│  └───────────────────────────────────────────┘      │
│                                                     │
│  Step 3: bcrypt hash (DEFAULT_COST = 12)            │
│  ┌───────────────────────────────────────────┐      │
│  │ hashed = bcrypt::hash(hex_input, 12)       │      │
│  └───────────────────────────────────────────┘      │
│                                                     │
│  Output: bcrypt hash string (stored in DB)           │
└─────────────────────────────────────────────────────┘
```

**Why this approach?**
- **HMAC pepper** acts as a server-side secret that prevents offline brute-force attacks even if the database is compromised. The pepper is stored in environment configuration, never in the database.
- **bcrypt** provides adaptive hashing with a configurable work factor (cost=12 by default), making each hash attempt computationally expensive.
- **Hex encoding** normalizes the HMAC output into a string suitable for bcrypt input.

### JWT Access Tokens

JWT tokens are short-lived access credentials:

| Property     | Value                                    |
|--------------|------------------------------------------|
| Algorithm    | HS256 (HMAC-SHA256)                      |
| Expiry       | 15 minutes from creation                 |
| Signing Key  | `JWT_SECRET` environment variable        |

**Claims structure:**
```json
{
  "sub": "550e8400-e29b-41d4-a716-446655440000",  // user_id (UUID)
  "email": "user@example.com",                     // user email
  "session_id": "660e8400-e29b-41d4-a716-446655440001", // session UUID
  "exp": 1713200100,                               // expiration timestamp
  "iat": 1713199200                                // issued-at timestamp
}
```

The `session_id` claim ties each access token to a specific server-side session, enabling:
- Session revocation (logout) that invalidates all tokens for that session
- Fine-grained session management (view/delete individual sessions)

### Refresh Tokens

Refresh tokens are long-lived, opaque tokens used to obtain new access tokens:

| Property     | Value                                    |
|--------------|------------------------------------------|
| Format       | 128 hex characters (64 random bytes)     |
| Storage      | `sessions` table in PostgreSQL           |
| Expiry       | 7 days (604,800 seconds)                 |
| Generation   | `rand::rng().fill_bytes()` (CSPRNG)      |

### Cookie Management

Two HttpOnly cookies are set upon login/registration/refresh:

#### `token` Cookie (Access Token)
| Attribute  | Value                                     |
|------------|-------------------------------------------|
| Name       | `token`                                   |
| HttpOnly   | `true`                                    |
| SameSite   | `Strict`                                  |
| Path       | `/`                                       |
| Secure     | `true` in release builds, `false` in debug |
| Max-Age    | 900 seconds (15 minutes)                  |

#### `refresh_token` Cookie (Refresh Token)
| Attribute  | Value                                     |
|------------|-------------------------------------------|
| Name       | `refresh_token`                           |
| HttpOnly   | `true`                                    |
| SameSite   | `Strict`                                  |
| Path       | `/auth/refresh`                           |
| Secure     | `true` in release builds, `false` in debug |
| Max-Age    | 604,800 seconds (7 days)                  |

**Security rationale:**
- `HttpOnly` prevents JavaScript access (XSS protection)
- `SameSite=Strict` prevents CSRF attacks
- `Path=/auth/refresh` for refresh token limits cookie transmission to only the refresh endpoint
- `Secure` ensures cookies are only sent over HTTPS in production

### Token Rotation & Reuse Detection

The refresh flow implements **token rotation** with **reuse detection**:

```
┌──────────────────────────────────────────────────────────────┐
│                    Token Rotation Flow                        │
│                                                              │
│  1. Client sends refresh_token cookie to /auth/refresh       │
│  2. Server looks up session by refresh_token                 │
│  3. If session.revoked_at is set → REJECT (reuse detected!) │
│     This means the token was already used and rotated.       │
│     Possible token theft scenario.                           │
│  4. If session.expires_at < now → REJECT (expired)           │
│  5. Server REVOKES the old session (sets revoked_at = now)   │
│  6. Server creates a NEW session with:                       │
│     - New refresh token (64 random bytes)                    │
│     - New expiry (7 days from now)                           │
│     - Carries forward user_agent and ip_address              │
│  7. Server mints a NEW JWT access token bound to new session │
│  8. Server returns both new tokens as cookies + JSON body    │
└──────────────────────────────────────────────────────────────┘
```

**Reuse detection:** If a previously-rotated refresh token is presented, the `revoked_at` field will be set, causing the request to be rejected. This detects scenarios where an attacker may have stolen a refresh token and is trying to use it after the legitimate user already refreshed.

---

## Session Management

Sessions are server-side records stored in the `sessions` table. Each session represents an authenticated device/browser session.

### Session Lifecycle

```
Created ────► Active ────► Revoked (on rotation or logout)
                │
                └──────────► Expired (after 7 days)
```

### SessionRepositoryTrait Operations

| Method                 | Description                                         |
|------------------------|-----------------------------------------------------|
| `create`               | Insert a new session row                            |
| `find_by_refresh_token`| Look up a session by its refresh token              |
| `find_by_id`           | Look up a session by its UUID                       |
| `find_all_by_user_id`  | List all sessions for a user                        |
| `revoke`               | Set `revoked_at = NOW()` for a session              |
| `delete`               | Hard-delete a session row                           |
| `delete_all_by_user_id`| Hard-delete all sessions for a user                 |

### SessionService Methods

| Method           | Description                                                |
|------------------|------------------------------------------------------------|
| `create_session` | Generate refresh token, persist session, mint JWT          |
| `refresh`        | Validate old token → revoke → create new session → mint JWT |

---

## Error Handling

Errors follow a layered pattern, with each layer defining its own error type using `thiserror`:

### Error Type Hierarchy

```
RepositoryError (user/repository)
├── PoolError          ← deadpool connection pool exhaustion
├── InteractError      ← Diesel thread pool interaction failure
└── DieselError        ← SQL/ORM errors (constraint violations, etc.)

SessionRepositoryError (session/repository)
├── PoolError
├── InteractError
└── DieselError

ProfileRepositoryError (user/repository)
├── PoolError
├── InteractError
└── DieselError

ServiceError (user/usecase)
├── RepositoryError    ← wraps RepositoryError
├── UserNotFound       ← user lookup returned None
├── InvalidCredentials ← password mismatch
└── HashError          ← bcrypt failure

SessionServiceError (session/usecase)
├── NotFound           ← refresh token not in DB
├── Revoked            ← session already revoked (reuse detection)
├── Expired            ← session past expiry
├── UserNotFound       ← user not found during refresh
├── SessionRepository  ← wraps SessionRepositoryError
├── UserRepository     ← wraps RepositoryError
└── Jwt                ← JWT encoding failure
```

### HTTP Status Code Mapping

| Error Scenario                    | HTTP Status              |
|-----------------------------------|--------------------------|
| User already exists (duplicate)   | `409 Conflict`           |
| Invalid email or password         | `401 Unauthorized`       |
| Missing/invalid JWT               | `401 Unauthorized`       |
| Session revoked or not found      | `401 Unauthorized`       |
| Expired token                     | `401 Unauthorized`       |
| User not found (get_me)           | `404 Not Found`          |
| Internal/repository/hash error    | `500 Internal Server Error` |

---

## Configuration

Configuration is loaded from environment variables using the `config` crate with `__` as the separator for nested keys.

### Required Environment Variables

| Variable           | Description                              | Example                              |
|--------------------|------------------------------------------|--------------------------------------|
| `LISTEN`           | Server bind address                      | `0.0.0.0:3000`                       |
| `PG__HOST`         | PostgreSQL host                          | `localhost`                          |
| `PG__PORT`         | PostgreSQL port                          | `5432`                               |
| `PG__USER`         | PostgreSQL user                          | `postgres`                           |
| `PG__PASSWORD`     | PostgreSQL password                      | `example`                            |
| `PG__DBNAME`       | PostgreSQL database name                 | `postgres`                           |
| `DATABASE_URL`     | Full PostgreSQL URL (for Diesel ORM)     | `postgres://postgres:example@localhost/postgres` |
| `PASSWORD_PEPPER`  | Secret pepper for password hashing       | (random string, keep secret)         |
| `JWT_SECRET`       | Secret key for JWT signing               | (random string, keep secret)         |

### Config Struct

```rust
struct Config {
    listen: String,           // Server bind address
    pg: deadpool_postgres::Config, // PostgreSQL config (via PG__* vars)
    database_url: String,     // Diesel database URL
    password_pepper: String,  // HMAC pepper for passwords
    jwt_secret: String,       // JWT signing secret
}
```

---

## Testing Strategy

The project employs a **multi-layer testing strategy**:

### 1. Unit Tests (with Mocks)

Each service layer has unit tests using `mockall` to mock repository traits:

- **`user_service.rs` tests** — Mock `UserRepositoryTrait` to test `create`, `authenticate`, and `get_me` logic in isolation
- **`profile_service.rs` tests** — Mock `ProfileRepositoryTrait` to test `add_profile` logic
- **`session_service.rs` tests** — Mock both `SessionRepositoryTrait` and `UserRepositoryTrait` to test `refresh` logic (rotation, reuse detection, expiry)
- **`auth_extractor.rs` tests** — Mock `SessionRepositoryTrait` to test JWT validation, cookie extraction, and session verification
- **`cookies.rs` tests** — Test cookie attribute setting and clearing
- **`jwt.rs` tests** — Test JWT creation, verification, expiry, and claim roundtrips
- **`hash_password.rs` tests** — Test password hashing and verification with pepper

### 2. Integration Tests (with Testcontainers)

Repository layers have integration tests that spin up a real PostgreSQL container via `testcontainers`:

- **`user_repository.rs` tests** — Test CRUD operations against a real database
- **`session_repository.rs` tests** — Test all session operations (create, find, revoke, delete)
- **`profile_repository.rs` tests** — Test profile creation with social handles and rates
- **`user_controller.rs` tests** — Full HTTP integration tests with real database, testing the complete request/response cycle
- **`session_controller.rs` tests** — Full HTTP integration tests for token refresh

### Test Setup Pattern

```
┌──────────────────────────────────────────────┐
│  1. Start PostgreSQL container               │
│     (testcontainers::Postgres)               │
│                                              │
│  2. Create deadpool-diesel connection pool   │
│                                              │
│  3. Run all migrations                       │
│     (embed_migrations! macro)                │
│                                              │
│  4. Create repository instances              │
│                                              │
│  5. Run test assertions                      │
│                                              │
│  6. Container is automatically cleaned up    │
│     when the test completes                  │
└──────────────────────────────────────────────┘
```

### Running Tests

```bash
# Run all tests (Docker must be running for integration tests)
cargo test

# Run only unit tests (no Docker required)
cargo test --lib

# Run a specific test
cargo test test_create_user_success
```

---

## Development Setup

### Prerequisites

- **Rust** (Edition 2024 or later)
- **Docker** and **Docker Compose** (for PostgreSQL + Adminer)
- **Diesel CLI** — `cargo install diesel_cli --no-default-features --features postgres`

### Step-by-Step Setup

```bash
# 1. Clone the repository
git clone <repo-url>
cd outcast-api

# 2. Start PostgreSQL and Adminer
docker-compose up -d
#    PostgreSQL: localhost:5432 (password: example)
#    Adminer UI: http://localhost:8080

# 3. Create a .env file with required configuration
cat > .env <<EOF
LISTEN=0.0.0.0:3000
PG__HOST=localhost
PG__PORT=5432
PG__USER=postgres
PG__PASSWORD=example
PG__DBNAME=postgres
DATABASE_URL=postgres://postgres:example@localhost/postgres
PASSWORD_PEPPER=your-secret-pepper-value
JWT_SECRET=your-secret-jwt-key
EOF

# 4. Run database migrations
diesel migration run

# 5. Build and run the server
cargo run

# 6. Access the API
# API:      http://localhost:3000
# Scalar:   http://localhost:3000/scalar
# OpenAPI:  http://localhost:3000/openapi.json
```

### Docker Compose Services

| Service  | Port | Description                   |
|----------|------|-------------------------------|
| `db`     | 5432 | PostgreSQL database           |
| `adminer`| 8080 | Database management web UI    |

### API Documentation UI

The project includes **Scalar** (via `utoipa-scalar`) as an interactive API documentation interface, accessible at `/scalar`. The OpenAPI specification is auto-generated from the handler annotations and served at `/openapi.json`.

---

## Observability

The application uses structured logging via the `tracing` crate:

- **Structured spans** — Each handler and repository method is instrumented with `#[instrument]`
- **Log levels** — Configurable via the `RUST_LOG` environment variable
  - `info` — Request handling, session lifecycle events
  - `debug` — Connection pool operations, query details
  - `warn` — Authentication failures, reuse detection
  - `error` — Database errors, hash failures
- **HTTP tracing** — `tower-http::TraceLayer` logs all incoming HTTP requests
- **CORS** — `tower-http::CorsLayer::permissive()` allows all cross-origin requests (suitable for development)

Example log output:
```
INFO  outcast_api::user::http::user_controller: Create user request received
INFO  outcast_api::user::usecase::user_service: Password hashed, creating user in repository
INFO  outcast_api::user::repository::user_repository: Creating new user user_id=<uuid>
INFO  outcast_api::session::repository::session_repository: Creating new session user_id=<uuid>
INFO  outcast_api::session::usecase::session_service: Session created session_id=<uuid> user_id=<uuid>
INFO  outcast_api::user::http::user_controller: User created successfully user_id=<uuid>
```
