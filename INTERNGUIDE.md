# Outcast API — Intern's Complete Guide

> **Who is this guide for?** You — someone who just joined the team, has probably written Python or JavaScript before, and has never touched Rust. This document walks you through the entire codebase from zero: what the app does, how every layer fits together, what all those weird Rust keywords mean, and exactly what happens when you call each API endpoint.

---

## Table of Contents

1. [What Does This App Do?](#1-what-does-this-app-do)
2. [Rust Crash Course](#2-rust-crash-course)
3. [Architecture — The Big Picture](#3-architecture)
4. [Project Folder Structure](#4-project-folder-structure)
5. [Database Schema](#5-database-schema)
6. [API Endpoints Reference](#6-api-endpoints-reference)
7. [Sequence Diagrams — Request Flows](#7-sequence-diagrams)
8. [Deep Dive: Each Layer Explained](#8-deep-dive-each-layer-explained)
9. [Security Design Explained](#9-security-design-explained)
10. [Testing Strategy](#10-testing-strategy)
11. [Local Development Setup](#11-local-development-setup)
12. [Glossary](#12-glossary)

---

## 1. What Does This App Do?

**Outcast API** is a backend REST API (the server-side of a web application). Think of it like the kitchen in a restaurant — the frontend (browser/mobile app) is the dining room, and this API is where all the actual work happens behind the scenes.

It handles:

| Feature | Description |
|---------|-------------|
| **User registration** | A new user provides an email + password; the server stores them safely |
| **Login** | A user proves who they are; the server gives them tokens for future requests |
| **"Who am I?"** | A logged-in user can ask the server for their own account details |
| **Session management** | Track active logins per device, refresh tokens, log out from one or all devices |
| **Creator profiles** | Rich profiles with social media handles and pricing rates |
| **Interactive docs** | A Scalar UI at `/scalar` showing all API endpoints |

---

## 2. Rust Crash Course

Before diving into the code, here are the Rust concepts you will encounter. Don't skip this section — it will save you hours of confusion.

### 2.1 Ownership (The Thing That Makes Rust Unique)

In most languages, two variables can point to the same data at the same time. In Rust, **each piece of data has exactly one owner**. When the owner goes out of scope, the data is automatically freed — no garbage collector needed.

```rust
let name = String::from("Alice");  // `name` owns the String
let other = name;                  // ownership MOVES to `other`
// println!("{}", name);           // ERROR! `name` no longer owns anything
println!("{}", other);             // OK
```

**Borrowing** lets you temporarily lend data without giving up ownership:

```rust
fn greet(name: &String) {   // `&` means "borrowing, not taking"
    println!("Hello, {}", name);
}
let name = String::from("Alice");
greet(&name);               // lend it
println!("{}", name);       // OK — we still own it
```

### 2.2 `Result<T, E>` — Handling Errors Without Exceptions

Rust has no `try/catch`. Instead, functions that can fail return a `Result`:

```rust
fn divide(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 {
        Err("Cannot divide by zero".to_string())
    } else {
        Ok(a / b)
    }
}

match divide(10.0, 2.0) {
    Ok(result) => println!("Answer: {}", result),
    Err(msg)   => println!("Oops: {}", msg),
}
```

You will see `?` all over the codebase — it is shorthand for "if this is an `Err`, return that error immediately":

```rust
let conn = self.pool.get().await?;  // if pool.get() fails, return its error
```

### 2.3 `Option<T>` — Representing "Maybe Nothing"

When something might not exist (like a user who might not be in the database), Rust uses `Option<T>`:

```rust
fn find_user(id: u32) -> Option<String> {
    if id == 1 { Some("Alice".to_string()) } else { None }
}

match find_user(42) {
    Some(name) => println!("Found: {}", name),
    None       => println!("User not found"),
}
```

You will often see `.ok_or(SomeError)` which converts `None` into an `Err`:

```rust
let user = find_user(42).ok_or(ServiceError::UserNotFound)?;
```

### 2.4 Traits — Rust's Version of Interfaces

A **trait** defines a contract: any type that implements this trait MUST have these methods.

```rust
trait UserRepositoryTrait {
    async fn create(&self, email: String, password: String) -> Result<User, RepositoryError>;
    async fn find_by_email(&self, email: String) -> Result<Option<User>, RepositoryError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepositoryError>;
}

// Real implementation (production)
impl UserRepositoryTrait for UserRepository {
    async fn create(&self, email: String, password: String) -> Result<User, RepositoryError> {
        // ... real SQL ...
    }
}

// Fake implementation (tests only)
impl UserRepositoryTrait for MockUserRepository {
    async fn create(&self, email: String, password: String) -> Result<User, RepositoryError> {
        Ok(User { id: Uuid::new_v4(), email, password }) // return fake data
    }
}
```

This is how business logic can be tested without a real database.

### 2.5 Generics — Code That Works for Multiple Types

```rust
// UserService works with ANY type R, as long as R implements UserRepositoryTrait
pub struct UserService<R: UserRepositoryTrait> {
    repository: R,
    pepper: String,
}
```

In production, `R` = `UserRepository`. In tests, `R` = `MockUserRepositoryTrait`.

### 2.6 Async/Await — Non-Blocking Concurrency

An HTTP server must handle many simultaneous requests. `async`/`await` achieves this without spawning a new OS thread per request:

```rust
async fn get_user(id: Uuid) -> Option<User> {
    // `.await` pauses here until the DB responds, allowing other requests to run
    let conn = pool.get().await?;
    // ...
}
```

The runtime that schedules async tasks is **Tokio** (the `tokio` crate).

### 2.7 Enums with Data — Rust's Superpower

Rust enums can hold data. This is used for rich error types:

```rust
#[derive(thiserror::Error, Debug)]
pub enum ServiceError {
    #[error("Repository error: {0}")]
    RepositoryError(#[from] RepositoryError),
    #[error("User not found")]
    UserNotFound,
    #[error("Invalid credentials")]
    InvalidCredentials,
}
```

`#[from]` means: automatically convert a `RepositoryError` into `ServiceError::RepositoryError`. This is what makes the `?` operator work across different error types.

### 2.8 `Arc<T>` — Shared Ownership Across Threads

`Arc` (Atomically Reference Counted) lets multiple parts of the code share ownership of the same data:

```rust
let session_repo: Arc<dyn SessionRepositoryTrait> = Arc::new(SessionRepository::new(pool));
// Now multiple services can hold a reference to the same repo
```

`dyn SessionRepositoryTrait` means "some type that implements the trait, decided at runtime" (dynamic dispatch).

### 2.9 Derive Macros — Auto-Generated Code

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Queryable)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password: String,
}
```

| Derive | What it generates |
|--------|------------------|
| `Debug` | A `{:?}` formatter for logging |
| `Clone` | A `.clone()` method for deep copies |
| `Serialize` | Convert to JSON |
| `Deserialize` | Create from JSON |
| `Queryable` | Let Diesel map DB rows to this struct |
| `Insertable` | Let Diesel use this struct for INSERT queries |

### 2.10 `#[instrument]` — Automatic Tracing

```rust
#[instrument(skip_all)]  // don't log arguments (they may contain secrets)
pub async fn create_user(...) {
    info!("Create user request received");  // logged with context
}
```

This is from the `tracing` crate and records when functions start/end and captures structured log fields.

---

## 3. Architecture

This project uses **Hexagonal Architecture** (Ports and Adapters). The core idea: keep business logic completely separated from outside infrastructure.

```
+------------------------------------------------------------------+
|                      HTTP REQUEST COMES IN                       |
|                             |                                    |
|                      Axum Router (main.rs)                       |
|                             |                                    |
|              +--------------+---------------+                    |
|              |                              |                    |
|   user_controller.rs           session_controller.rs            |
|   (HTTP layer)                 (HTTP layer)                      |
|              |                              |                    |
|              v calls                        v calls              |
|   +---------------------+    +---------------------------+      |
|   |   UserService       |    |   SessionService          |      |
|   |   (usecase layer)   |    |   (usecase layer)         |      |
|   |  - hash password    |    |  - rotate refresh tokens  |      |
|   |  - verify password  |    |  - check expiry           |      |
|   +----------+----------+    +-------------+-------------+      |
|              | (via trait)                 | (via trait)         |
|              v                             v                     |
|   +---------------------+    +---------------------------+      |
|   |  UserRepository     |    |  SessionRepository        |      |
|   |  (repository layer) |    |  (repository layer)       |      |
|   |  Diesel ORM + SQL   |    |  Diesel ORM + SQL         |      |
|   +----------+----------+    +-------------+-------------+      |
|              +--------------------+---------+                   |
|                                   v                             |
|                          PostgreSQL Database                    |
+------------------------------------------------------------------+
```

**Rules to follow when adding features:**
- Put business rules in the **use-case layer** (services), not in controllers or repositories
- Controllers: parse request, call service, format response — nothing more
- Repositories: only SQL/data access — no business logic

---

## 4. Project Folder Structure

```
outcast-api/
|
+-- Cargo.toml              <- Rust's package.json
+-- Cargo.lock              <- Locked dependency versions (always commit this)
+-- diesel.toml             <- Diesel CLI configuration
+-- docker-compose.yaml     <- Starts PostgreSQL + Adminer locally
|
+-- migrations/             <- Ordered SQL files that create/modify the database
|   +-- 00000000000000_diesel_initial_setup/
|   +-- 2026-04-12-..._create_user/        <- creates `users` table
|   +-- 2026-04-14-..._create_profiles_.../<- creates profiles, social_handles, rates
|   +-- 2026-04-15-..._create_sessions/    <- creates `sessions` table
|
+-- src/
    +-- main.rs             <- Entry point: config, pools, services, router, server
    +-- schema.rs           <- AUTO-GENERATED by Diesel CLI — NEVER edit manually
    |
    +-- user/
    |   +-- http/
    |   |   +-- user_controller.rs   <- POST /user, POST /user/login, GET /user/me
    |   |   +-- auth_extractor.rs    <- Validates JWT + session for protected routes
    |   +-- usecase/
    |   |   +-- user_service.rs      <- create(), authenticate(), get_me()
    |   |   +-- profile_service.rs   <- add_profile()
    |   +-- repository/
    |   |   +-- user_repository.rs   <- SQL for users table
    |   |   +-- profile_repository.rs<- SQL for profiles/social_handles/rates tables
    |   +-- crypto/
    |       +-- hash_password.rs     <- HMAC-SHA256 pepper + bcrypt
    |       +-- jwt.rs               <- Create and verify HS256 JWTs
    |
    +-- session/
        +-- http/
        |   +-- session_controller.rs<- /auth/refresh, /auth/logout, /auth/sessions, etc.
        |   +-- cookies.rs           <- set_auth_cookies() and clear_auth_cookies()
        +-- usecase/
        |   +-- session_service.rs   <- create_session(), refresh(), logout(), list, delete
        +-- repository/
            +-- session_repository.rs<- SQL for sessions table
```

---

## 5. Database Schema

```
+----------------------------------------------+
|                   users                      |
+----------------------------------------------+
| id          UUID        PRIMARY KEY          |
| email       VARCHAR(255)                     |
| password    VARCHAR(255) <- bcrypt hash      |
+-------------------+--------------------------+
                    | 1
          +---------+---------+
          | many               | many
          v                    v
+------------------+  +-------------------------------------+
|    sessions      |  |              profiles               |
+------------------+  +-------------------------------------+
| id          UUID |  | id           UUID                   |
| user_id  -> UUID |  | user_id   -> UUID (-> users.id)     |
| refresh_token    |  | name, bio, niche, avatar_url        |
| user_agent  TEXT |  | username     CITEXT (case-insensitive unique)
| ip_address  TEXT |  | updated_at, created_at TIMESTAMPTZ  |
| expires_at  TIME |  +----------------+--------------------+
| revoked_at  TIME |                   | 1
| created_at  TIME |          +--------+---------+
| updated_at  TIME |          | many             | many
+------------------+          v                  v
                  +------------------+  +---------------------+
                  |  social_handles  |  |        rates        |
                  +------------------+  +---------------------+
                  | id          UUID |  | id          UUID    |
                  | profile_id  UUID |  | profile_id  UUID    |
                  | platform    TEXT |  | rate_type   TEXT    |
                  | handle      TEXT |  | amount      NUMERIC |
                  | url         TEXT |  +---------------------+
                  | follower_count   |
                  +------------------+
```

**Key concepts:**
- `UUID` — globally unique random ID like `550e8400-e29b-41d4-a716-446655440000`
- `CITEXT` — case-insensitive text; "Alice" and "alice" are treated as equal
- `revoked_at` on sessions — `NULL` means active; a timestamp means logged out

---

## 6. API Endpoints Reference

### User Endpoints

| Method | Path | Auth | Description |
|--------|------|:----:|-------------|
| `POST` | `/user` | No | Register a new user |
| `POST` | `/user/login` | No | Login with email + password |
| `GET` | `/user/me` | Yes | Get currently authenticated user |

### Session Endpoints

| Method | Path | Auth | Description |
|--------|------|:----:|-------------|
| `POST` | `/auth/refresh` | No (cookie) | Get new access token via refresh token |
| `POST` | `/auth/logout` | Yes | Log out current session |
| `POST` | `/auth/logout-all` | Yes | Log out ALL sessions |
| `GET` | `/auth/sessions` | Yes | List all active sessions |
| `DELETE` | `/auth/sessions/{id}` | Yes | Delete a specific session |

### Utility

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1.0/event.list` | List events |
| `GET` | `/openapi.json` | OpenAPI spec |
| `GET` | `/scalar` | Interactive API docs UI |

### Authentication

Protected routes accept either:
- `Authorization: Bearer <access_token>` header, **OR**
- `token` HttpOnly cookie (set automatically on login/register)

**Access tokens expire after 15 minutes.** Use `/auth/refresh` to get a new one.

### Request/Response Examples

**`POST /user` — Register (201 Created):**
```json
// Request body
{ "email": "alice@example.com", "password": "secret123" }

// Response body
{ "id": "uuid...", "email": "alice@example.com", "token": "eyJ..." }
// Also sets: token cookie (15 min) and refresh_token cookie (7 days)
```

**`POST /user/login` — Login (200 OK):** Same shape as register.

**`GET /user/me` — Current user (200 OK):**
```json
{ "id": "uuid...", "email": "alice@example.com" }
```

**`POST /auth/refresh` — Refresh (200 OK):**
```json
// No body needed; reads refresh_token cookie automatically
// Response:
{ "access_token": "eyJ..." }
// Also sets fresh token and refresh_token cookies
```

---

## 7. Sequence Diagrams

These diagrams show every step inside the server for each request. Time flows downward.

### 7.1 Register a New User

```
Client        user_controller   UserService    UserRepository  SessionService    DB
  |                 |                |               |               |            |
  |--POST /user---->|                |               |               |            |
  |  {email,pass}   |                |               |               |            |
  |                 |--service       |               |               |            |
  |                 |  .create()---->|               |               |            |
  |                 |                |--hash_password|               |            |
  |                 |                |  HMAC+bcrypt  |               |            |
  |                 |                |               |               |            |
  |                 |                |--repo.create->|               |            |
  |                 |                |               |--INSERT users->            |
  |                 |                |               |<-----------User{id,...}    |
  |                 |<---------------|  Ok(User)     |               |            |
  |                 |                |               |               |            |
  |                 |--session_service.create_session()------------>|            |
  |                 |                |               | generate refresh_token     |
  |                 |                |               | (64 random bytes -> hex)   |
  |                 |                |               |--INSERT sessions---------->|
  |                 |                |               |<--------------------------|
  |                 |                |               | create_jwt(uid, email,     |
  |                 |                |               |   session_id, secret)      |
  |                 |<-----------------------------------------Ok(RefreshTokens) |
  |<----------------|  201 Created   |               |               |            |
  |  {id,email,     |  Set-Cookie: token=<jwt>        |               |            |
  |   token:<jwt>}  |  Set-Cookie: refresh_token=<hex>|               |            |
```

**What to notice:**
- Password is **never stored in plain text** — HMAC + bcrypt before the DB.
- Two tokens issued: short-lived **access token** (JWT, 15 min) and long-lived **refresh token** (hex, 7 days).
- Both arrive as HttpOnly cookies AND the access token is in the JSON body.

---

### 7.2 Login

```
Client        user_controller   UserService    UserRepository  SessionService    DB
  |                 |                |               |               |            |
  |--POST /user/    |                |               |               |            |
  |  login-------->|                |               |               |            |
  |                 |--service       |               |               |            |
  |                 |  .authenticate>|               |               |            |
  |                 |                |--find_by_email|               |            |
  |                 |                |               |--SELECT WHERE email=?---->|
  |                 |                |               |<-----------Some(User)/None |
  |                 |                | if None: Err(UserNotFound)                 |
  |                 |                |--verify_password (HMAC + bcrypt compare)   |
  |                 |                | if wrong: Err(InvalidCredentials)          |
  |                 |<---------------|  Ok(User)     |               |            |
  |                 |--session_service.create_session() (same as register)        |
  |                 |<-----------------------------------------Ok(tokens)        |
  |<----------------|  200 OK        |               |               |            |
  |  {id,email,tok} |  Set-Cookie: token + refresh_token             |            |
```

**Security note:** Whether the email doesn't exist OR the password is wrong, the server always returns `401 Unauthorized` — this prevents attackers from discovering which emails are registered.

---

### 7.3 Get Current User (Protected Route)

The `AuthUser` extractor runs **before** the handler and validates the token.

```
Client       Axum       AuthUser extractor   SessionRepo  Controller  UserService  DB
  |           |                 |                 |            |            |        |
  |--GET      |                 |                 |            |            |        |
  |  /user/me>|                 |                 |            |            |        |
  |  Bearer   |                 |                 |            |            |        |
  |  <jwt>    |--extract        |                 |            |            |        |
  |           |  AuthUser------>|                 |            |            |        |
  |           |                 |--verify_jwt()   |            |            |        |
  |           |                 |  (signature +   |            |            |        |
  |           |                 |   expiry check) |            |            |        |
  |           |                 | if invalid: 401 |            |            |        |
  |           |                 |                 |            |            |        |
  |           |                 |--find_by_id(    |            |            |        |
  |           |                 |   session_id)-->|            |            |        |
  |           |                 |                 |--SELECT--->|            |        |
  |           |                 |<----------------|Some(Session)            |        |
  |           |                 | if None: 401    |            |            |        |
  |           |                 | if revoked: 401 |            |            |        |
  |           |<----------------|AuthUser{uid,...}|            |            |        |
  |           |                 |                 |            |            |        |
  |           |--call handler------------------------------->  |            |        |
  |           |                 |                 |            |--get_me()->|        |
  |           |                 |                 |            |            |--SEL->|
  |           |                 |                 |            |            |<------|
  |           |                 |                 |            |<-----------Ok(User)|
  |<----------|  200 {id,email} |                 |            |            |        |
```

**Key insight:** Even with a valid JWT, the server checks the database to confirm the session is still active. Logout truly invalidates tokens.

---

### 7.4 Refresh Token

When the access token expires (after 15 min), use the refresh token to get a new one.

```
Client       session_controller  SessionService  SessionRepo  UserRepo      DB
  |                 |                 |               |            |          |
  |--POST /auth/    |                 |               |            |          |
  |  refresh------->|                 |               |            |          |
  |  Cookie:        |                 |               |            |          |
  |  refresh_token=X|                 |               |            |          |
  |                 |--service        |               |            |          |
  |                 |  .refresh(X)--->|               |            |          |
  |                 |                 |--find_by_      |            |          |
  |                 |                 |  refresh_token>|            |          |
  |                 |                 |               |--SELECT--->|          |
  |                 |                 |               |<-----------Some(sess) |
  |                 |                 | if None: 401 (not found)              |
  |                 |                 | if revoked: 401 (reuse detected!)     |
  |                 |                 | if expired: 401                       |
  |                 |                 |               |            |          |
  |                 |                 |--revoke(old)->|            |          |
  |                 |                 |               |--UPDATE revoked_at--->|
  |                 |                 |               |<----------------------|
  |                 |                 |               |            |          |
  |                 |                 |--find_by_id(user_id)------>|          |
  |                 |                 |               |            |--SELECT->|
  |                 |                 |               |            |<---------|
  |                 |                 |<--------------------------User        |
  |                 |                 |                            |          |
  |                 |                 | generate new refresh token            |
  |                 |                 |--create(new session)------>|          |
  |                 |                 |               |--INSERT--->|          |
  |                 |                 |               |<-----------|          |
  |                 |                 | create_jwt(uid, email, new_session_id)|
  |                 |<----------------|Ok(RefreshTokens)           |          |
  |<----------------|  200 OK         |               |            |          |
  |  {access_token} |  Set-Cookie: token=<new_jwt>    |            |          |
  |                 |  Set-Cookie: refresh_token=<new> |            |          |
```

**Token rotation:** The old refresh token is immediately revoked, a brand-new one is issued. Each refresh token can only be used once. If an attacker steals it and uses it, your legitimate next refresh will detect the reuse (token already revoked).

---

### 7.5 Logout

```
Client       session_controller  SessionService  SessionRepo            DB
  |                 |                 |               |                  |
  |--POST /auth/    |                 |               |                  |
  |  logout-------->|                 |               |                  |
  |  Bearer <jwt>   |                 |               |                  |
  |                 | [AuthUser validates JWT + session]                  |
  |                 |--service.logout(session_id)---->|                  |
  |                 |                 |--revoke(id)--->|                  |
  |                 |                 |               |--UPDATE revoked_at=now-->|
  |                 |                 |               |<-----------------|
  |                 |<----------------|  Ok(())        |                  |
  |<----------------|  204 No Content |               |                  |
  |                 |  Set-Cookie: token=""; Max-Age=0 |                  |
  |                 |  Set-Cookie: refresh_token=""; Max-Age=0            |
```

`Max-Age=0` tells the browser to delete the cookies immediately.

---

### 7.6 Logout All Devices

```
Client       session_controller  SessionService  SessionRepo            DB
  |                 |                 |               |                  |
  |--POST /auth/    |                 |               |                  |
  |  logout-all---->|                 |               |                  |
  |                 | [AuthUser validates]             |                  |
  |                 |--service.logout_all(user_id)---->|                 |
  |                 |                 |--delete_all_   |                  |
  |                 |                 |  by_user_id--->|                  |
  |                 |                 |               |--DELETE sessions->|
  |                 |                 |               |  WHERE user_id=?  |
  |                 |                 |               |<-----------------|
  |                 |<----------------|  Ok(())        |                  |
  |<----------------|  204 No Content |               |                  |
  |                 |  (cookies cleared)               |                  |
```

---

### 7.7 List Sessions

```
Client       session_controller  SessionService  SessionRepo            DB
  |                 |                 |               |                  |
  |--GET /auth/     |                 |               |                  |
  |  sessions------>|                 |               |                  |
  |                 | [AuthUser validates]             |                  |
  |                 |--service.list_sessions(user_id)->|                 |
  |                 |                 |--find_all_by_  |                  |
  |                 |                 |  user_id------>|                  |
  |                 |                 |               |--SELECT sessions->|
  |                 |                 |               |<-----------------|
  |                 |                 |<--------------Vec<Session>        |
  |                 |                 | filter: keep only where           |
  |                 |                 |   revoked_at IS NULL              |
  |                 |                 |   AND expires_at > now            |
  |                 |<----------------|Vec<Session> (active only)         |
  |<----------------|  200 [{id,      |               |                  |
  |   user_agent,   |   ip_address,   |               |                  |
  |   created_at,   |   expires_at}]  |               |                  |
```

---

### 7.8 Delete a Specific Session

```
Client        session_controller  SessionService  SessionRepo            DB
  |                  |                 |               |                  |
  |--DELETE /auth/   |                 |               |                  |
  |  sessions/{id}-->|                 |               |                  |
  |                  | [AuthUser validates]             |                  |
  |                  |--service.delete_session(         |                  |
  |                  |   session_id, user_id)---------->|                 |
  |                  |                 |--find_by_id(sid)>               |
  |                  |                 |               |--SELECT--------->|
  |                  |                 |               |<-----------------|
  |                  |                 |<--------------Some(Session)      |
  |                  |                 | if session.user_id != requesting |
  |                  |                 | user_id: Err(NotFound) <- SECURITY
  |                  |                 |               |                  |
  |                  |                 |--delete(id)--->|                 |
  |                  |                 |               |--DELETE--------->|
  |                  |                 |               |<-----------------|
  |                  |<----------------|  Ok(())        |                  |
  |<-----------------|  204 No Content |               |                  |
```

**Security check:** The service verifies the session belongs to the requesting user before deleting it.

---

## 8. Deep Dive: Each Layer Explained

### 8.1 Entry Point — `main.rs`

`main.rs` is the boot sequence of the entire application:

1. Load `.env` file (`dotenvy`)
2. Initialize structured logging (`tracing_subscriber`)
3. Parse configuration into a typed `Config` struct from environment variables
4. Create two connection pools:
   - `deadpool-postgres` — for the raw SQL event endpoint
   - `deadpool-diesel` — for Diesel ORM queries in all repositories
5. Create repositories (pass Diesel pool)
6. Create services (inject repositories + pepper/secrets)
7. Bundle everything into `AppState`
8. Build the router (merge sub-routers)
9. Start the TCP listener and hand it to Axum

**`AppState` and `FromRef`:**

```rust
#[derive(Clone)]
pub struct AppState {
    pub pool: deadpool_postgres::Pool,
    pub user_service: UserService<UserRepository>,
    pub jwt_secret: String,
    pub session_repository: Arc<dyn SessionRepositoryTrait>,
    pub session_service: SessionService,
}

// Tell Axum how to extract SessionService from AppState
impl axum::extract::FromRef<AppState> for SessionService {
    fn from_ref(state: &AppState) -> Self {
        state.session_service.clone()
    }
}
```

Handlers extract just the piece they need using `State(service): State<SessionService>`.

---

### 8.2 HTTP Layer — Controllers

Every handler follows the same pattern:

```rust
pub async fn create_user(
    jar: CookieJar,                                // extract cookies
    State(service): State<UserService<...>>,       // from AppState
    State(jwt_secret): State<String>,              // from AppState
    State(session_service): State<SessionService>, // from AppState
    Json(payload): Json<CreateUserReq>,            // parse JSON body
) -> impl IntoResponse {
    // call service, return response
}
```

Controllers return typed HTTP status codes:

```rust
match result {
    Ok(user) => (StatusCode::CREATED, Json(response)).into_response(),    // 201
    Err(UniqueViolation) => (StatusCode::CONFLICT, "Already exists").into_response(), // 409
    Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Failed").into_response(), // 500
}
```

---

### 8.3 Authentication Guard — `auth_extractor.rs`

`AuthUser` is a custom Axum extractor. Adding `auth_user: AuthUser` to a handler's parameters causes Axum to automatically run this logic before calling the handler:

**Step 1 — Find the token (header preferred, cookie fallback):**
```rust
let token = if let Ok(TypedHeader(Authorization(bearer))) = try_bearer_header {
    bearer.token().to_owned()
} else {
    jar.get("token").map(|c| c.value().to_owned())
       .ok_or((StatusCode::UNAUTHORIZED, "Missing authentication token"))?
};
```

**Step 2 — Validate JWT signature and expiry:**
```rust
let claims = verify_jwt(&token, &jwt_secret)
    .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;
```

**Step 3 — Verify session is still active in DB:**
```rust
let session = session_repo.find_by_id(claims.session_id).await?
    .ok_or((StatusCode::UNAUTHORIZED, "Session not found"))?;
if session.revoked_at.is_some() {
    return Err((StatusCode::UNAUTHORIZED, "Session has been revoked"));
}
```

Step 3 is what makes logout truly work: a revoked session immediately blocks the token from working.

---

### 8.4 Use-Case Layer — Services

Services contain **business rules** and orchestrate calls to repositories.

**`UserService<R>`:**
- `create(email, password)` — hash the password, call `repository.create()`
- `authenticate(email, password)` — look up by email, verify password, return user or error
- `get_me(user_id)` — find user by ID

**`SessionService`:**
- `create_session(...)` — generate refresh token, INSERT session, create JWT
- `refresh(old_token, ...)` — validate, revoke old, create new session + JWT
- `logout(session_id)` — revoke the session
- `logout_all(user_id)` — delete all sessions
- `list_sessions(user_id)` — fetch and filter to active only
- `delete_session(session_id, user_id)` — verify ownership, then delete

---

### 8.5 Repository Layer — Database Access

Repositories use **Diesel ORM** for type-safe SQL queries.

```rust
// Type-safe query: SELECT * FROM users WHERE email = $1 LIMIT 1
users::table
    .filter(users::email.eq(&email))
    .first::<User>(conn)
    .optional()  // return Option<User>
```

**Why `.interact()`?** Diesel is synchronous (blocking), but the server is async. `deadpool-diesel`'s `.interact()` runs Diesel operations on a background thread pool:

```rust
let result = conn.interact(move |conn| {
    diesel::insert_into(users::table)
        .values(&new_user)
        .execute(conn)
}).await??;  // double ? unwraps: InteractError, then DieselError
```

---

### 8.6 Crypto Layer

#### Password Hashing (`hash_password.rs`)

```
plain_password + server_pepper
        |
        v
   HMAC-SHA256  (keyed hash; pepper is the key)
        |
        v  32 bytes
   hex encoding  ->  64-char hex string
        |
        v
   bcrypt(DEFAULT_COST)  (slow adaptive hash)
        |
        v
   "$2b$12$..."  (stored in users.password column)
```

**Why two steps?** The pepper prevents database-only attacks. bcrypt's slow speed prevents brute force.

#### JWT (`jwt.rs`)

A JWT has three dot-separated parts:
```
<base64 header>.<base64 payload>.<signature>
```

The payload (claims):

| Field | Meaning |
|-------|---------|
| `sub` | User ID |
| `email` | User's email |
| `session_id` | DB session this token belongs to |
| `exp` | Expiry timestamp (now + 15 min) |
| `iat` | Issued-at timestamp |

The signature uses HMAC-SHA256 with `JWT_SECRET`. Changing any claim invalidates the signature.

---

### 8.7 Cookie Helpers

Every auth cookie uses these security attributes:

| Setting | Value | Reason |
|---------|-------|--------|
| `HttpOnly` | `true` | JavaScript cannot read it — blocks XSS token theft |
| `SameSite=Strict` | Strict | Not sent cross-site — blocks CSRF |
| `Secure` | `true` in production | HTTPS only — blocks interception |
| `Path` access token | `/` | Sent with all requests |
| `Path` refresh token | `/auth/refresh` | Sent only to refresh endpoint |

Lifetimes: access token = 15 min, refresh token = 7 days.

To delete a cookie, the server sends the same cookie with `Max-Age=0` and an empty value — the browser removes it.

---

## 9. Security Design Explained

### Two Tokens, Why?

| | Access Token (JWT) | Refresh Token (hex) |
|-|-------------------|---------------------|
| **Lifetime** | 15 minutes | 7 days |
| **Validated in DB?** | Yes (revocation check) | Yes (lookup) |
| **Purpose** | Authenticate API calls | Obtain new access tokens |

Short access token lifetime limits the blast radius of a stolen token. Refresh tokens are long-lived but single-use.

### Why HMAC + bcrypt for Passwords?

- **Pepper** = server-side secret. Database dump alone is worthless without it.
- **bcrypt** = deliberately slow. Brute-forcing thousands of guesses takes too long.
- **HMAC first** = handles bcrypt's 72-byte limit gracefully.

### Why Cross-Check Sessions in the DB?

JWTs are stateless — they are valid until expiry, regardless of logout. The DB session check adds statefulness: revoking a session immediately blocks the associated JWT.

### Refresh Token Rotation (Single-Use)

Old token is revoked before issuing a new one. If an attacker steals and uses the old token first, the legitimate user's next refresh will return `401 Revoked` — a signal that the token was compromised.

---

## 10. Testing Strategy

### Level 1 — Unit Tests (no database)

Located in the same file as the code under test, inside `#[cfg(test)] mod tests { ... }`.

Use **mockall** to create fake repository implementations:

```rust
let mut mock = MockUserRepositoryTrait::new();
mock.expect_create()
    .with(eq("test@example.com".to_string()), always())
    .times(1)  // must be called exactly once
    .returning(|email, password| Ok(User { id: Uuid::nil(), email, password }));

let service = UserService::new(mock, "pepper".to_string());
let result = service.create("test@example.com".to_string(), "pass".to_string()).await;
assert!(result.is_ok());
```

Runs in milliseconds, no Docker needed.

### Level 2 — Integration Tests (real database)

Located at the bottom of repository files.

```rust
#[tokio::test]
async fn test_containers_create_user() {
    // Start a PostgreSQL container (Docker must be running)
    let (_container, pool) = setup_test_db().await;
    // Migrations are run inside setup_test_db

    let repo = UserRepository::new(pool);
    let user = repo.create("test@example.com".to_string(), "pass".to_string())
        .await.unwrap();

    assert_eq!(user.email, "test@example.com");
    // _container drops here => Docker container automatically removed
}
```

Each test gets its own fresh database — no shared state between tests.

### Level 3 — End-to-End HTTP Tests

Located in controller test modules. Start a full Axum router + DB, make real HTTP requests:

```rust
let response = app.oneshot(
    Request::builder()
        .method("POST").uri("/user")
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"email":"e@e.com","password":"pass"}"#))
        .unwrap(),
).await.unwrap();
assert_eq!(response.status(), StatusCode::CREATED);
```

### Running Tests

```bash
# All tests (Docker must be running)
cargo test

# Specific test by name
cargo test test_authenticate_success

# Only tests in a specific module
cargo test user::usecase::user_service::tests
```

---

## 11. Local Development Setup

### Prerequisites

| Tool | Install |
|------|---------|
| Rust | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Docker Desktop | https://www.docker.com/products/docker-desktop |
| Diesel CLI | `cargo install diesel_cli --no-default-features --features postgres` |

### Steps

```bash
# 1. Start the database
docker-compose up -d
# PostgreSQL: localhost:5432
# Adminer UI: http://localhost:8080 (login: server=db, user=postgres, password=postgres)

# 2. Create .env
cat > .env << 'ENVEOF'
LISTEN=0.0.0.0:3000
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres
PG__HOST=localhost
PG__PORT=5432
PG__DBNAME=postgres
PG__USER=postgres
PG__PASSWORD=postgres
PASSWORD_PEPPER=my_very_secret_pepper_string
JWT_SECRET=my_very_secret_jwt_key
RUST_LOG=info
ENVEOF

# 3. Run migrations
diesel migration run

# 4. Start the server
cargo run

# 5. Test it
curl -X POST http://localhost:3000/user \
  -H "Content-Type: application/json" \
  -d '{"email":"alice@example.com","password":"secret123"}'

# 6. Open interactive docs
# Browse to http://localhost:3000/scalar
```

### Common Commands

```bash
cargo build                       # compile
cargo build --release             # compile optimised
cargo run                         # run server
cargo test                        # run all tests
cargo clippy                      # lint
cargo fmt                         # auto-format code

diesel migration generate <name>  # create up.sql + down.sql pair
diesel migration run              # apply pending migrations
diesel migration revert           # undo last migration

RUST_LOG=debug cargo run          # verbose logging
RUST_LOG=trace cargo run          # extremely verbose logging
```

---

## 12. Glossary

| Term | Definition |
|------|-----------|
| **API** | Application Programming Interface — a server that responds to structured HTTP requests |
| **Arc** | Atomically Reference Counted — Rust's way of sharing data ownership across threads safely |
| **async/await** | Concurrency model where tasks pause and resume, enabling many concurrent requests without many threads |
| **bcrypt** | A deliberately slow password hashing algorithm designed to resist brute-force attacks |
| **CITEXT** | PostgreSQL column type comparing text case-insensitively (`Alice` = `alice`) |
| **Cookie** | A small piece of data sent by the server, returned by the browser on every subsequent request |
| **Diesel** | A Rust ORM: translates between Rust structs and SQL — type-checked at compile time |
| **Extractor** | In Axum, a type that pulls data from an HTTP request (body, headers, cookies, state) |
| **Generics** | Code that works with different types, specified by the caller |
| **HMAC** | Hash-based Message Authentication Code — a keyed hash using a secret (the pepper) |
| **HTTP** | HyperText Transfer Protocol — the communication protocol of the web |
| **HttpOnly** | Cookie flag that prevents JavaScript from reading it — protects against XSS |
| **Hexagonal Architecture** | Design pattern isolating business logic from infrastructure (DB, HTTP framework) |
| **JWT** | JSON Web Token — cryptographically signed token encoding user identity |
| **Migration** | A versioned SQL file that changes the DB schema; applied in order and tracked by Diesel |
| **Mock** | A fake trait implementation used in tests — returns predetermined data instead of hitting a DB |
| **Option** | Rust's type for "either Some(value) or None" — a safe alternative to null |
| **ORM** | Object-Relational Mapper — a library to query databases using your language instead of raw SQL |
| **Pepper** | A server-side secret mixed into password hashes — database dump alone is not enough to crack passwords |
| **Pool** | A pre-opened set of reusable DB connections — avoids the overhead of opening a new connection per request |
| **Refresh Token** | Long-lived credential used only to get new access tokens; single-use (rotated on each use) |
| **Repository** | A layer abstracting all database access — services say "find user by email", repositories write SQL |
| **Result** | Rust's type for "either Ok(value) or Err(error)" — the language-level replacement for exceptions |
| **SameSite=Strict** | Cookie attribute preventing it from being sent on cross-site requests — protects against CSRF |
| **Serde** | A Rust library for converting between Rust types and formats like JSON |
| **Session** | A DB record tracking a single login; one user can have many (multiple devices/browsers) |
| **Struct** | A Rust custom type grouping named fields — similar to a TypeScript interface or Python dataclass |
| **Tokio** | The async runtime for Rust — schedules async tasks, like Node.js's event loop |
| **Trait** | Rust's interface — defines methods a type must implement |
| **UUID** | Universally Unique Identifier — a 128-bit random ID like `550e8400-e29b-41d4-a716-446655440000` |
| **XSS** | Cross-Site Scripting — injected JavaScript stealing cookies or tokens from a page |
