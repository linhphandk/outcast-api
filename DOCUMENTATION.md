# Outcast API — Complete Documentation

> **Who is this for?** This document is written for someone who has **never seen Rust code**, has **never used any of the libraries** in this project, and is **not familiar with concepts like sessions, tokens, password hashing, or APIs**. Every concept is explained from scratch.

---

## Table of Contents

1. [What Is This Project?](#1-what-is-this-project)
2. [Key Concepts Explained From Scratch](#2-key-concepts-explained-from-scratch)
   - [What Is an API?](#21-what-is-an-api)
   - [What Is a Server?](#22-what-is-a-server)
   - [What Is a Database?](#23-what-is-a-database)
   - [What Is Rust?](#24-what-is-rust)
   - [What Is Authentication?](#25-what-is-authentication)
   - [What Are Passwords, Hashing, and Salting?](#26-what-are-passwords-hashing-and-salting)
   - [What Is a Session?](#27-what-is-a-session)
   - [What Is a Token (JWT)?](#28-what-is-a-token-jwt)
   - [What Are Cookies?](#29-what-are-cookies)
   - [What Is a Refresh Token?](#210-what-is-a-refresh-token)
   - [What Is a UUID?](#211-what-is-a-uuid)
3. [High-Level Architecture](#3-high-level-architecture)
4. [How the Project is Organized (Directory Structure)](#4-how-the-project-is-organized-directory-structure)
5. [The Database — What Data We Store](#5-the-database--what-data-we-store)
6. [Every API Endpoint Explained](#6-every-api-endpoint-explained)
   - [POST /user — Create a New Account](#61-post-user--create-a-new-account)
   - [POST /user/login — Log In](#62-post-userlogin--log-in)
   - [GET /user/me — Get Your Own Profile](#63-get-userme--get-your-own-profile)
   - [POST /auth/refresh — Refresh Your Token](#64-post-authrefresh--refresh-your-token)
   - [POST /auth/logout — Log Out](#65-post-authlogout--log-out)
   - [POST /auth/logout-all — Log Out Everywhere](#66-post-authlogout-all--log-out-everywhere)
   - [GET /auth/sessions — List Your Active Sessions](#67-get-authsessions--list-your-active-sessions)
   - [DELETE /auth/sessions/{id} — Delete a Specific Session](#68-delete-authsessionsid--delete-a-specific-session)
7. [The Complete Sign-Up Flow (Step by Step)](#7-the-complete-sign-up-flow-step-by-step)
8. [The Complete Login Flow (Step by Step)](#8-the-complete-login-flow-step-by-step)
9. [The Complete Token Refresh Flow (Step by Step)](#9-the-complete-token-refresh-flow-step-by-step)
10. [How Authentication Works for Protected Endpoints](#10-how-authentication-works-for-protected-endpoints)
11. [Password Security — How Passwords Are Protected](#11-password-security--how-passwords-are-protected)
12. [The Architecture Pattern (Hexagonal Architecture)](#12-the-architecture-pattern-hexagonal-architecture)
13. [Every Technology Used and Why](#13-every-technology-used-and-why)
14. [How to Set Up and Run the Project](#14-how-to-set-up-and-run-the-project)
15. [How to Run Tests](#15-how-to-run-tests)
16. [Configuration — Environment Variables](#16-configuration--environment-variables)
17. [Glossary](#17-glossary)

---

## 1. What Is This Project?

**Outcast API** is a **backend web application** written in the [Rust programming language](https://www.rust-lang.org/). It provides a service where:

- People can **create an account** (sign up) with their email and password.
- People can **log in** to their account.
- People can **stay logged in** safely using tokens and sessions.
- People can **create a profile** with personal details, social media handles, and pricing rates (like a creator/influencer marketplace).
- People can **manage their login sessions** (see where they're logged in, log out of specific devices, log out everywhere).

Think of it like the behind-the-scenes engine that a mobile app or website would talk to. It does not have any visual screens — it only processes requests and sends back data.

---

## 2. Key Concepts Explained From Scratch

### 2.1 What Is an API?

**API** stands for **Application Programming Interface**. It is a way for two computer programs to talk to each other.

Imagine you are at a restaurant. You (the customer) don't go into the kitchen yourself. Instead, you tell the waiter what you want, the waiter goes to the kitchen, and the waiter brings back your food. In this analogy:

- **You** = a mobile app or website (called the "client")
- **The waiter** = the API
- **The kitchen** = the server and database where the real work happens

When the mobile app wants to create a new user, it sends a **request** to the API. The API does the work (saves the user in the database) and sends back a **response** (the newly created user's information).

### 2.2 What Is a Server?

A **server** is just a computer program that is always running, waiting for requests. When someone sends it a request (like "create a new user"), it processes that request and sends back a response.

This project creates a server that listens on a network address (like `0.0.0.0:3000`), meaning it waits for requests on port 3000 of the machine it runs on.

### 2.3 What Is a Database?

A **database** is like a giant organized spreadsheet where information is stored permanently. Even if the server restarts, the data in the database survives.

This project uses **PostgreSQL** (often called "Postgres"), which is one of the most popular databases in the world. It stores data in **tables** (like sheets in a spreadsheet), where each table has **columns** (like headers) and **rows** (like individual entries).

For example, the `users` table looks like this:

| id (UUID) | email | password (hashed) |
|---|---|---|
| `a1b2c3d4-...` | `alice@example.com` | `$2b$12$...` (scrambled) |
| `e5f6g7h8-...` | `bob@example.com` | `$2b$12$...` (scrambled) |

### 2.4 What Is Rust?

**Rust** is a programming language known for being:
- **Fast** — it runs almost as fast as C/C++, which are the fastest languages.
- **Safe** — it prevents many common bugs that crash programs.
- **Concurrent** — it can handle thousands of requests at the same time without breaking.

Rust code is written in files ending in `.rs`. The project is managed by a tool called **Cargo**, which:
- Downloads libraries (called "crates" in Rust) that the project depends on.
- Compiles (translates) the Rust code into a program the computer can run.
- Runs tests to make sure the code works correctly.

The file `Cargo.toml` lists all the libraries this project depends on (like a recipe listing ingredients).

### 2.5 What Is Authentication?

**Authentication** is the process of proving "you are who you say you are." When you log into any website, you type your email and password — that's authentication. The server checks if the email exists and if the password matches.

This project has a full authentication system that:
1. Lets users **sign up** with an email and password.
2. Lets users **log in** by checking their email and password.
3. Gives logged-in users a **token** (a digital pass) so they don't have to send their password with every single request.

### 2.6 What Are Passwords, Hashing, and Salting?

When you create an account, you type a password like `MySecret123`. But this project **never stores your actual password**. Instead, it scrambles it into something unreadable using a process called **hashing**.

**Hashing** is a one-way mathematical operation. It turns `MySecret123` into something like `$2b$12$xQ7z...` (a long random-looking string). The key property is:
- You can go **forward** (password → hash) easily.
- You **cannot** go **backward** (hash → password). It's mathematically impossible.

This means even if a hacker steals the database, they cannot figure out the actual passwords.

This project adds two extra layers of security:

1. **Pepper** — A secret key stored on the server (not in the database). Before hashing the password, it is combined with this secret key using HMAC (a special mixing algorithm). This means even if someone steals the database, they still can't crack the passwords without also stealing the server's secret.

2. **Salt** — The bcrypt algorithm (used for the final hashing) automatically adds a random value called a "salt" to each password before hashing. This means two users with the same password will have completely different hashes.

```
Step 1: User types "MySecret123"
Step 2: Combine with pepper using HMAC → "a4f8c2e1..." (hex string)
Step 3: Hash with bcrypt (which adds its own salt) → "$2b$12$xQ7z..." (final stored hash)
```

### 2.7 What Is a Session?

A **session** represents "one login" of a user. Think of it like a visitor badge at an office building:

- When you arrive (log in), you get a visitor badge (a session is created).
- While you're in the building, you show your badge to access rooms (you send your token with each request).
- When you leave (log out), your badge is deactivated (the session is revoked).

If you log in from your phone AND your laptop, you have **two separate sessions** — two badges. You can see all your active sessions and deactivate (revoke) any of them.

Each session is stored as a row in the `sessions` database table, and it contains:
- A unique ID
- Which user it belongs to
- A refresh token (explained next)
- When it was created
- When it expires
- Whether it has been revoked (deactivated)

### 2.8 What Is a Token (JWT)?

A **JWT** (JSON Web Token, pronounced "jot") is like a digital ID card that the server gives you after you log in. Instead of sending your email and password with every request, you just show your JWT.

A JWT contains three parts, separated by dots:
```
eyJhbGciOi...   .   eyJzdWIiOi...   .   SflKxwRJSM...
[Header]            [Payload]            [Signature]
```

- **Header** — Says what algorithm was used to sign it (HS256 in this project).
- **Payload** — Contains your data (user ID, email, session ID, expiration time).
- **Signature** — A cryptographic seal that proves the token wasn't tampered with. Only the server can create valid signatures because only the server knows the secret key.

**Important properties of JWTs in this project:**
- They expire after **15 minutes**. This is short on purpose — if a token is stolen, it's only useful for a brief time.
- They contain the `session_id`, which links the token back to a session in the database.
- The server can reject a JWT even if it hasn't expired, by checking if the linked session is still active.

### 2.9 What Are Cookies?

A **cookie** is a small piece of data that the server asks the browser to store. Every time the browser makes a request to the same server, it automatically sends the cookie back. It's like a name tag the server sticks on you.

This project uses two cookies:

| Cookie Name | What It Stores | Lifetime | Path |
|---|---|---|---|
| `token` | The JWT access token | 15 minutes | `/` (sent with every request) |
| `refresh_token` | The refresh token | 7 days | `/auth/refresh` (only sent to the refresh endpoint) |

Both cookies have these security settings:
- **HttpOnly** — JavaScript code running in the browser **cannot** read these cookies. This prevents a type of attack called XSS (Cross-Site Scripting).
- **SameSite=Lax** — The browser only sends these cookies to the same website that set them. This prevents a type of attack called CSRF (Cross-Site Request Forgery).

### 2.10 What Is a Refresh Token?

The access token (JWT) expires after 15 minutes for security. But it would be annoying if users had to log in again every 15 minutes. That's where the **refresh token** comes in.

A refresh token is a long random string (128 characters of hexadecimal, generated from 64 random bytes) that is stored both:
- On the client side (in a cookie).
- On the server side (in the `sessions` database table).

When the access token expires, the client can send the refresh token to the server and get a **brand new** access token and a **brand new** refresh token. The old refresh token is then deactivated (the session is "rotated").

This gives us the best of both worlds:
- Short-lived access tokens (secure — if stolen, limited damage).
- Long-lived refresh tokens (convenient — users don't need to re-enter their password often).
- Refresh token rotation (extra secure — each refresh token can only be used once).

```
Timeline:
├─ Login ──────────────────────────────────────────────────────┤
│  Access Token (15 min)    │  EXPIRED                         │
│  Refresh Token (7 days)   │  Still valid                     │
│                           │                                  │
│                           ├─ Refresh ────────────────────────┤
│                           │  NEW Access Token (15 min)       │
│                           │  NEW Refresh Token (7 days)      │
│                           │  OLD Refresh Token → REVOKED     │
```

### 2.11 What Is a UUID?

A **UUID** (Universally Unique Identifier) is a 128-bit number used as a unique ID. It looks like this: `550e8400-e29b-41d4-a716-446655440000`.

Every user, session, profile, social handle, and rate in this project gets its own UUID. The special thing about UUIDs is that they are **practically guaranteed to be unique** across the entire world — you can generate billions of them and never get a duplicate. This project uses **UUID v4**, which is generated randomly.

---

## 3. High-Level Architecture

This diagram shows every major component in the system and how they connect to each other:

```mermaid
graph TB
    subgraph "Client (Browser or Mobile App)"
        CLIENT["🖥️ Browser / Mobile App<br/>Sends HTTP requests<br/>Stores cookies"]
    end

    subgraph "Outcast API Server (Rust)"
        subgraph "Web Framework Layer (Axum)"
            ROUTER["🔀 Router<br/>Receives HTTP requests<br/>Routes them to the right handler"]
            CORS["🛡️ CORS Middleware<br/>Controls which websites<br/>can access the API"]
            TRACE["📝 Trace Middleware<br/>Logs every request"]
        end

        subgraph "HTTP Controllers"
            UC["👤 User Controller<br/>POST /user<br/>POST /user/login<br/>GET /user/me"]
            SC["🔐 Session Controller<br/>POST /auth/refresh<br/>POST /auth/logout<br/>POST /auth/logout-all<br/>GET /auth/sessions<br/>DELETE /auth/sessions/:id"]
            AUTH["🔑 Auth Extractor<br/>Reads JWT from header or cookie<br/>Validates it<br/>Checks session is active"]
        end

        subgraph "Business Logic (Use Cases / Services)"
            US["👤 User Service<br/>Create user<br/>Authenticate user<br/>Get user info"]
            SS["🔐 Session Service<br/>Create session<br/>Refresh tokens<br/>Logout / Logout all<br/>List sessions<br/>Delete session"]
            PS["📋 Profile Service<br/>Create profile<br/>with social handles & rates"]
        end

        subgraph "Security (Crypto)"
            HASH["🔒 Password Hasher<br/>HMAC + bcrypt<br/>Hash & verify passwords"]
            JWT["🎫 JWT Manager<br/>Create & verify<br/>JSON Web Tokens"]
        end

        subgraph "Data Access (Repositories)"
            UR["💾 User Repository<br/>Create user in DB<br/>Find user by email/ID"]
            SR["💾 Session Repository<br/>Create/find/revoke/delete<br/>sessions in DB"]
            PR["💾 Profile Repository<br/>Create profiles with<br/>social handles & rates"]
        end
    end

    subgraph "Database (PostgreSQL)"
        DB[("🗄️ PostgreSQL Database<br/>users table<br/>sessions table<br/>profiles table<br/>social_handles table<br/>rates table")]
    end

    CLIENT -->|"HTTP Request"| ROUTER
    ROUTER --> CORS
    CORS --> TRACE
    TRACE --> UC
    TRACE --> SC
    SC --> AUTH
    UC --> AUTH
    UC --> US
    SC --> SS
    US --> HASH
    US --> UR
    SS --> JWT
    SS --> SR
    SS --> UR
    PS --> PR
    AUTH --> JWT
    AUTH --> SR
    UR --> DB
    SR --> DB
    PR --> DB
    ROUTER -->|"HTTP Response"| CLIENT
```

---

## 4. How the Project is Organized (Directory Structure)

Every file in this project has a specific purpose. Here is the complete file tree with an explanation of each file:

```
outcast-api/
│
├── Cargo.toml                     # 📦 Lists all external libraries ("crates") the project needs.
│                                  #    Think of it as a recipe's ingredient list.
│
├── Cargo.lock                     # 🔒 Records the exact versions of every library downloaded.
│                                  #    Ensures everyone gets the same versions.
│
├── diesel.toml                    # ⚙️  Configuration for the Diesel ORM tool.
│
├── docker-compose.yaml            # 🐳 Defines how to start PostgreSQL + Adminer using Docker.
│                                  #    (Docker = a tool that runs apps in isolated containers)
│
├── migrations/                    # 📊 Database migration files — instructions for creating tables.
│   ├── 00000000000000_diesel_initial_setup/
│   │   └── up.sql / down.sql      #    Initial Diesel setup (creates internal tracking table).
│   ├── 2026-04-12-120305-0000_create_user/
│   │   └── up.sql / down.sql      #    Creates the `users` table.
│   ├── 2026-04-14-134310-0001_create_profiles_social_handles_rates/
│   │   └── up.sql / down.sql      #    Creates `profiles`, `social_handles`, and `rates` tables.
│   └── 2026-04-15-080000-0002_create_sessions/
│       └── up.sql / down.sql      #    Creates the `sessions` table.
│
└── src/                           # 📁 All Rust source code lives here.
    │
    ├── main.rs                    # 🚀 The entry point — where the program starts.
    │                              #    Sets up the database connection, creates services,
    │                              #    defines routes, and starts the server.
    │
    ├── schema.rs                  # 🤖 AUTO-GENERATED by Diesel. Do NOT edit manually.
    │                              #    Describes the database tables in Rust code so the
    │                              #    compiler can check your database queries.
    │
    ├── user/                      # 👤 Everything related to users.
    │   ├── mod.rs                 #    Declares the sub-modules (crypto, http, repository, usecase).
    │   │
    │   ├── crypto/                # 🔐 Cryptography (security) utilities.
    │   │   ├── mod.rs             #    Declares sub-modules.
    │   │   ├── hash_password.rs   #    Functions to hash passwords (HMAC + bcrypt) and verify them.
    │   │   └── jwt.rs             #    Functions to create and verify JWT tokens.
    │   │
    │   ├── http/                  # 🌐 HTTP layer — handles incoming web requests.
    │   │   ├── mod.rs             #    Declares sub-modules.
    │   │   ├── user_controller.rs #    Defines the API endpoints: POST /user, POST /user/login,
    │   │   │                      #    GET /user/me. Processes requests and sends responses.
    │   │   └── auth_extractor.rs  #    The "bouncer" — extracts and validates the JWT from
    │   │                          #    incoming requests to verify the user is logged in.
    │   │
    │   ├── repository/            # 💾 Data access layer — talks to the database.
    │   │   ├── mod.rs             #    Declares sub-modules.
    │   │   ├── user_repository.rs #    Functions to create/find users in the database.
    │   │   └── profile_repository.rs # Functions to create profiles with social handles and rates.
    │   │
    │   └── usecase/               # 🧠 Business logic layer — the "brain" of the application.
    │       ├── mod.rs             #    Declares sub-modules.
    │       ├── user_service.rs    #    Orchestrates user creation (hashes password, calls repo)
    │       │                      #    and user authentication (checks password).
    │       └── profile_service.rs #    Orchestrates profile creation.
    │
    └── session/                   # 🔐 Everything related to login sessions.
        ├── mod.rs                 #    Declares sub-modules.
        │
        ├── http/                  # 🌐 HTTP layer for session endpoints.
        │   ├── mod.rs             #    Declares sub-modules.
        │   ├── session_controller.rs # Defines the API endpoints: POST /auth/refresh,
        │   │                      #    POST /auth/logout, POST /auth/logout-all,
        │   │                      #    GET /auth/sessions, DELETE /auth/sessions/:id.
        │   └── cookies.rs         #    Helper functions to set and clear authentication cookies.
        │
        ├── repository/            # 💾 Data access layer for sessions.
        │   ├── mod.rs             #    Declares sub-modules.
        │   └── session_repository.rs # Functions to create/find/revoke/delete sessions in the DB.
        │
        └── usecase/               # 🧠 Business logic for sessions.
            ├── mod.rs             #    Declares sub-modules.
            └── session_service.rs #    Orchestrates session creation, token refresh, logout,
                                   #    and session management.
```

---

## 5. The Database — What Data We Store

The database has **5 tables**. Here is a complete diagram showing all tables, their columns, data types, and how they relate to each other:

```mermaid
erDiagram
    USERS {
        UUID id PK "Unique identifier (auto-generated)"
        VARCHAR_255 email UK "User's email address (must be unique)"
        VARCHAR_255 password "Hashed password (never plain text)"
    }

    SESSIONS {
        UUID id PK "Unique identifier (auto-generated)"
        UUID user_id FK "Which user this session belongs to"
        VARCHAR_512 refresh_token UK "Random 128-char hex string (unique)"
        TEXT user_agent "Browser info (e.g., 'Mozilla/5.0...')"
        VARCHAR_45 ip_address "IP address of the device"
        TIMESTAMP expires_at "When this session expires"
        TIMESTAMP revoked_at "When this session was deactivated (null if still active)"
        TIMESTAMP created_at "When this session was created"
        TIMESTAMP updated_at "When this session was last modified"
    }

    PROFILES {
        UUID id PK "Unique identifier (auto-generated)"
        UUID user_id FK "Which user this profile belongs to"
        TEXT name "Display name"
        TEXT bio "Biography / description"
        TEXT niche "Content niche (e.g., 'technology')"
        TEXT avatar_url "URL to profile picture"
        CITEXT username UK "Public username (case-insensitive, unique)"
        TIMESTAMPTZ updated_at "Last update time"
        TIMESTAMPTZ created_at "Creation time"
    }

    SOCIAL_HANDLES {
        UUID id PK "Unique identifier (auto-generated)"
        UUID profile_id FK "Which profile this belongs to"
        TEXT platform "Platform name: instagram, tiktok, or youtube"
        TEXT handle "Social media handle (e.g., '@alice_tech')"
        TEXT url "Full URL to the social media page"
        INT follower_count "Number of followers (must be >= 0)"
        TIMESTAMPTZ updated_at "Last update time"
    }

    RATES {
        UUID id PK "Unique identifier (auto-generated)"
        UUID profile_id FK "Which profile this belongs to"
        TEXT type "Rate type (free-form text)"
        NUMERIC_10_2 amount "Price in dollars (must be >= 0)"
    }

    USERS ||--o{ SESSIONS : "has many"
    USERS ||--o{ PROFILES : "has many"
    PROFILES ||--o{ SOCIAL_HANDLES : "has many"
    PROFILES ||--o{ RATES : "has many"
```

### Table Details

#### `users` — Stores registered user accounts
- **`id`** — A UUID that uniquely identifies this user. Generated automatically.
- **`email`** — The user's email. Must be unique (no two users can have the same email).
- **`password`** — The user's password, but **scrambled** (hashed). The actual password is never stored.

#### `sessions` — Stores login sessions (one row per login)
- **`id`** — Unique ID for this session.
- **`user_id`** — Links to the `users` table — says which user this session belongs to. If the user is deleted, all their sessions are automatically deleted too (CASCADE).
- **`refresh_token`** — A long random string (128 hex characters) used to get a new access token. Must be unique.
- **`user_agent`** — The browser or app that created this session (e.g., "Mozilla/5.0 (iPhone; ...)"). Optional.
- **`ip_address`** — The IP address of the device that created this session. Optional.
- **`expires_at`** — When this session expires (7 days after creation).
- **`revoked_at`** — If this session has been deactivated (logged out or refreshed), this records when. If it's `NULL`, the session is still active.
- **`created_at`** / **`updated_at`** — Timestamps for auditing.

#### `profiles` — Stores creator/influencer profiles
- **`username`** — A unique, case-insensitive username. "Alice" and "alice" are considered the same.
- **`niche`** — The content category (e.g., "technology", "fashion", "food").

#### `social_handles` — Stores social media accounts linked to a profile
- **`platform`** — Must be one of: `instagram`, `tiktok`, or `youtube`.
- Each profile can have at most **one handle per platform** (enforced by a unique constraint).

#### `rates` — Stores pricing for different content types
- **`type`** — Free-form text (for example: `post`, `story`, `reel`, or custom values).
- **`amount`** — Price with up to 2 decimal places (e.g., `500.00`).
- Each profile can have at most **one rate per type** (enforced by a unique constraint).

---

## 6. Every API Endpoint Explained

An **endpoint** is a specific URL that the server responds to. Each endpoint does one specific thing.

### 6.1 POST /user — Create a New Account

**What it does:** Creates a brand new user account.

**What you send:**
```json
{
  "email": "alice@example.com",
  "password": "MySecretPassword123"
}
```

**What you get back (success — 201 Created):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "alice@example.com",
  "token": "eyJhbGciOiJIUzI1NiIs..."
}
```

The server also sets two cookies (`token` and `refresh_token`) in the response so the browser automatically stores them.

**What can go wrong:**
| Status Code | Meaning |
|---|---|
| `409 Conflict` | A user with this email already exists. |
| `500 Internal Server Error` | Something unexpected went wrong on the server. |

---

### 6.2 POST /user/login — Log In

**What it does:** Authenticates an existing user by checking their email and password.

**What you send:**
```json
{
  "email": "alice@example.com",
  "password": "MySecretPassword123"
}
```

**What you get back (success — 200 OK):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "alice@example.com",
  "token": "eyJhbGciOiJIUzI1NiIs..."
}
```

Again, the `token` and `refresh_token` cookies are set in the response.

**What can go wrong:**
| Status Code | Meaning |
|---|---|
| `401 Unauthorized` | The email doesn't exist, or the password is wrong. (The error message is intentionally vague — it says "Invalid email or password" for both cases, so attackers can't tell which one is wrong.) |
| `500 Internal Server Error` | Something unexpected went wrong on the server. |

---

### 6.3 GET /user/me — Get Your Own Profile

**What it does:** Returns information about the currently logged-in user.

**Requires authentication:** Yes. You must include a valid JWT in either:
- An `Authorization: Bearer <token>` header, OR
- The `token` cookie (sent automatically by browsers).

**What you send:** Nothing — just the request with your token.

**What you get back (success — 200 OK):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "email": "alice@example.com"
}
```

**What can go wrong:**
| Status Code | Meaning |
|---|---|
| `401 Unauthorized` | Missing, invalid, or expired token. Or the session has been revoked. |
| `404 Not Found` | The user in the token doesn't exist anymore. |
| `500 Internal Server Error` | Something unexpected went wrong on the server. |

---

### 6.4 POST /auth/refresh — Refresh Your Token

**What it does:** Exchanges an old refresh token for a brand new access token and a brand new refresh token. This is called **token rotation**.

**What you send:** Nothing in the body — the `refresh_token` cookie is sent automatically by the browser (because the cookie's path is `/auth/refresh`).

**What you get back (success — 200 OK):**
```json
{
  "access_token": "eyJhbGciOiJIUzI1NiIs..."
}
```

New `token` and `refresh_token` cookies are set in the response.

**What happens behind the scenes:**
1. The server looks up the old refresh token in the database.
2. If found and not revoked/expired, the old session is **revoked** (deactivated).
3. A brand new session is created with a new refresh token.
4. A new JWT access token is created tied to the new session.
5. The old refresh token can **never** be used again. If someone tries to use it, they get rejected. This protects against stolen tokens.

**What can go wrong:**
| Status Code | Meaning |
|---|---|
| `401 Unauthorized` | No refresh token cookie, or the token is unknown, already used (revoked), or expired. |
| `500 Internal Server Error` | Something unexpected went wrong on the server. |

---

### 6.5 POST /auth/logout — Log Out

**What it does:** Logs out the current session. The session is marked as "revoked" in the database, and the cookies are cleared.

**Requires authentication:** Yes.

**What you get back (success — 204 No Content):** Empty response. The cookies are cleared.

---

### 6.6 POST /auth/logout-all — Log Out Everywhere

**What it does:** Deletes **all** sessions for the current user. If you're logged in on your phone, laptop, and tablet, all three are logged out.

**Requires authentication:** Yes.

**What you get back (success — 204 No Content):** Empty response. The cookies are cleared.

---

### 6.7 GET /auth/sessions — List Your Active Sessions

**What it does:** Returns a list of all active (non-revoked, non-expired) sessions for the current user.

**Requires authentication:** Yes.

**What you get back (success — 200 OK):**
```json
[
  {
    "id": "a1b2c3d4-...",
    "user_id": "550e8400-...",
    "user_agent": "Mozilla/5.0 (iPhone; CPU iPhone OS 15_0 like Mac OS X)",
    "ip_address": "192.168.1.100",
    "created_at": "2026-04-15 10:30:00",
    "expires_at": "2026-04-22 10:30:00"
  },
  {
    "id": "e5f6g7h8-...",
    "user_id": "550e8400-...",
    "user_agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
    "ip_address": "10.0.0.5",
    "created_at": "2026-04-14 08:00:00",
    "expires_at": "2026-04-21 08:00:00"
  }
]
```

---

### 6.8 DELETE /auth/sessions/{id} — Delete a Specific Session

**What it does:** Deletes a specific session by its ID. Useful for remotely logging out a specific device. You can only delete sessions that belong to you.

**Requires authentication:** Yes.

**Example:** `DELETE /auth/sessions/a1b2c3d4-e5f6-7890-abcd-ef1234567890`

**What you get back (success — 204 No Content):** Empty response.

**What can go wrong:**
| Status Code | Meaning |
|---|---|
| `404 Not Found` | Session doesn't exist or doesn't belong to you. |
| `500 Internal Server Error` | Something unexpected went wrong on the server. |

---

## 7. The Complete Sign-Up Flow (Step by Step)

This diagram shows exactly what happens when a new user creates an account, from start to finish:

```mermaid
sequenceDiagram
    participant Client as 🖥️ Client (Browser/App)
    participant Router as 🔀 Axum Router
    participant UserCtrl as 👤 User Controller
    participant UserSvc as 🧠 User Service
    participant HashPwd as 🔒 Password Hasher
    participant UserRepo as 💾 User Repository
    participant SessionSvc as 🔐 Session Service
    participant SessionRepo as 💾 Session Repository
    participant JWTMgr as 🎫 JWT Manager
    participant DB as 🗄️ PostgreSQL
    participant Cookies as 🍪 Cookie Helper

    Client->>Router: POST /user {"email": "alice@example.com", "password": "MySecret123"}
    Router->>UserCtrl: Route to create_user handler

    Note over UserCtrl: Step 1: Create the user account
    UserCtrl->>UserSvc: create("alice@example.com", "MySecret123")

    Note over UserSvc,HashPwd: Step 2: Secure the password
    UserSvc->>HashPwd: hash_password("MySecret123", pepper)
    Note over HashPwd: 2a. HMAC("MySecret123", pepper) → hex string
    Note over HashPwd: 2b. bcrypt(hex string) → "$2b$12$xQ7z..."
    HashPwd-->>UserSvc: "$2b$12$xQ7z..." (hashed password)

    Note over UserSvc,DB: Step 3: Save user to database
    UserSvc->>UserRepo: create("alice@example.com", "$2b$12$xQ7z...")
    UserRepo->>DB: INSERT INTO users (id, email, password) VALUES (uuid, ...)
    DB-->>UserRepo: ✅ User row created
    UserRepo-->>UserSvc: User { id, email, password }
    UserSvc-->>UserCtrl: User { id, email, password }

    Note over UserCtrl,SessionSvc: Step 4: Create a login session
    UserCtrl->>SessionSvc: create_session(user_id, email, None, None, jwt_secret)

    Note over SessionSvc: 4a. Generate 128-char random refresh token
    SessionSvc->>SessionRepo: create(user_id, refresh_token, ..., expires_at)
    SessionRepo->>DB: INSERT INTO sessions (...) VALUES (...)
    DB-->>SessionRepo: ✅ Session row created
    SessionRepo-->>SessionSvc: Session { id, ... }

    Note over SessionSvc,JWTMgr: 4b. Create a JWT access token
    SessionSvc->>JWTMgr: create_jwt(user_id, email, session_id, secret)
    Note over JWTMgr: Sign with HS256 algorithm<br/>Expires in 15 minutes<br/>Contains: user_id, email, session_id
    JWTMgr-->>SessionSvc: "eyJhbGciOi..." (JWT string)
    SessionSvc-->>UserCtrl: { access_token, refresh_token }

    Note over UserCtrl,Client: Step 5: Send response with cookies
    UserCtrl->>Cookies: set_auth_cookies(jar, access_token, refresh_token)
    Note over Cookies: Set 'token' cookie (15 min, HttpOnly, Path=/)<br/>Set 'refresh_token' cookie (7 days, HttpOnly, Path=/auth/refresh)
    UserCtrl-->>Client: 201 Created + JSON body + Set-Cookie headers
    Note over Client: Client stores cookies automatically<br/>Response body: { id, email, token }
```

---

## 8. The Complete Login Flow (Step by Step)

```mermaid
sequenceDiagram
    participant Client as 🖥️ Client
    participant UserCtrl as 👤 User Controller
    participant UserSvc as 🧠 User Service
    participant UserRepo as 💾 User Repository
    participant HashPwd as 🔒 Password Hasher
    participant SessionSvc as 🔐 Session Service
    participant SessionRepo as 💾 Session Repository
    participant JWTMgr as 🎫 JWT Manager
    participant DB as 🗄️ PostgreSQL

    Client->>UserCtrl: POST /user/login {"email": "alice@example.com", "password": "MySecret123"}

    Note over UserCtrl,UserSvc: Step 1: Authenticate the user
    UserCtrl->>UserSvc: authenticate("alice@example.com", "MySecret123")

    Note over UserSvc,DB: Step 2: Find user by email
    UserSvc->>UserRepo: find_by_email("alice@example.com")
    UserRepo->>DB: SELECT * FROM users WHERE email = 'alice@example.com'
    DB-->>UserRepo: User row (or nothing)
    UserRepo-->>UserSvc: Some(User) or None

    alt User NOT found
        UserSvc-->>UserCtrl: ❌ Error: UserNotFound
        UserCtrl-->>Client: 401 Unauthorized "Invalid email or password"
    end

    Note over UserSvc,HashPwd: Step 3: Verify the password
    UserSvc->>HashPwd: verify_password("MySecret123", stored_hash, pepper)
    Note over HashPwd: 3a. HMAC("MySecret123", pepper) → hex string
    Note over HashPwd: 3b. bcrypt_verify(hex_string, stored_hash) → true/false
    HashPwd-->>UserSvc: true ✅ (or false ❌)

    alt Password does NOT match
        UserSvc-->>UserCtrl: ❌ Error: InvalidCredentials
        UserCtrl-->>Client: 401 Unauthorized "Invalid email or password"
    end

    UserSvc-->>UserCtrl: ✅ User { id, email, password }

    Note over UserCtrl,SessionSvc: Step 4: Create a new session (same as sign-up)
    UserCtrl->>SessionSvc: create_session(user_id, email, None, None, jwt_secret)
    SessionSvc->>SessionRepo: create(user_id, refresh_token, ..., expires_at)
    SessionRepo->>DB: INSERT INTO sessions
    SessionSvc->>JWTMgr: create_jwt(user_id, email, session_id, secret)
    JWTMgr-->>SessionSvc: JWT access token
    SessionSvc-->>UserCtrl: { access_token, refresh_token }

    UserCtrl-->>Client: 200 OK + JSON body + Set-Cookie headers
```

---

## 9. The Complete Token Refresh Flow (Step by Step)

```mermaid
sequenceDiagram
    participant Client as 🖥️ Client
    participant SessionCtrl as 🔐 Session Controller
    participant SessionSvc as 🧠 Session Service
    participant SessionRepo as 💾 Session Repository
    participant UserRepo as 💾 User Repository
    participant JWTMgr as 🎫 JWT Manager
    participant DB as 🗄️ PostgreSQL

    Note over Client: Access token expired (after 15 min).<br/>Browser automatically sends refresh_token<br/>cookie to /auth/refresh.

    Client->>SessionCtrl: POST /auth/refresh (refresh_token cookie is sent automatically)

    Note over SessionCtrl: Step 1: Read the refresh_token cookie
    SessionCtrl->>SessionSvc: refresh(old_refresh_token, jwt_secret)

    Note over SessionSvc,DB: Step 2: Look up the session by refresh token
    SessionSvc->>SessionRepo: find_by_refresh_token(old_refresh_token)
    SessionRepo->>DB: SELECT * FROM sessions WHERE refresh_token = '...'
    DB-->>SessionRepo: Session row (or nothing)

    alt Token NOT found
        SessionSvc-->>SessionCtrl: ❌ NotFound
        SessionCtrl-->>Client: 401 Unauthorized
    end

    alt Session is REVOKED (already used)
        Note over SessionSvc: ⚠️ REUSE DETECTION!<br/>Someone may have stolen the old token.
        SessionSvc-->>SessionCtrl: ❌ Revoked
        SessionCtrl-->>Client: 401 Unauthorized
    end

    alt Session is EXPIRED
        SessionSvc-->>SessionCtrl: ❌ Expired
        SessionCtrl-->>Client: 401 Unauthorized
    end

    Note over SessionSvc,DB: Step 3: Revoke the old session
    SessionSvc->>SessionRepo: revoke(old_session_id)
    SessionRepo->>DB: UPDATE sessions SET revoked_at = NOW() WHERE id = ...
    Note over DB: Old session can never be used again

    Note over SessionSvc,DB: Step 4: Look up the user (for the new JWT)
    SessionSvc->>UserRepo: find_by_id(user_id)
    UserRepo->>DB: SELECT * FROM users WHERE id = ...
    DB-->>UserRepo: User row

    Note over SessionSvc: Step 5: Generate a new refresh token (128 hex chars)

    Note over SessionSvc,DB: Step 6: Create a new session in the database
    SessionSvc->>SessionRepo: create(user_id, new_refresh_token, ..., new_expires_at)
    SessionRepo->>DB: INSERT INTO sessions
    DB-->>SessionRepo: New session row

    Note over SessionSvc,JWTMgr: Step 7: Create a new JWT for the new session
    SessionSvc->>JWTMgr: create_jwt(user_id, email, new_session_id, secret)
    JWTMgr-->>SessionSvc: New JWT access token

    SessionSvc-->>SessionCtrl: { new_access_token, new_refresh_token }
    SessionCtrl-->>Client: 200 OK + new cookies + { access_token }
    Note over Client: Old tokens are now useless.<br/>New tokens are stored in cookies.
```

---

## 10. How Authentication Works for Protected Endpoints

Some endpoints (like `GET /user/me`) require the user to be logged in. This is enforced by the **Auth Extractor** — a piece of code that runs before the main handler and checks if the request has a valid token.

```mermaid
flowchart TD
    A["📨 Incoming Request to a protected endpoint<br/>(e.g., GET /user/me)"] --> B{"Does the request have an<br/>Authorization: Bearer header?"}

    B -->|"Yes"| C["Extract the token from the header"]
    B -->|"No"| D{"Does the request have<br/>a 'token' cookie?"}

    D -->|"Yes"| E["Extract the token from the cookie"]
    D -->|"No"| F["❌ 401 Unauthorized<br/>'Missing authentication token'"]

    C --> G["Verify the JWT signature and expiration"]
    E --> G

    G -->|"Invalid or expired"| H["❌ 401 Unauthorized<br/>'Invalid or expired token'"]
    G -->|"Valid"| I["Extract claims: user_id, email, session_id"]

    I --> J["Look up session by session_id in the database"]

    J -->|"Session not found"| K["❌ 401 Unauthorized<br/>'Session not found or revoked'"]
    J -->|"Session found"| L{"Is the session revoked?<br/>(revoked_at is NOT null)"}

    L -->|"Yes, it's revoked"| M["❌ 401 Unauthorized<br/>'Session has been revoked'"]
    L -->|"No, it's still active"| N["✅ Authentication successful!<br/>Create AuthUser with user_id, email, session_id<br/>and pass it to the handler"]

    style F fill:#ff6b6b,color:#fff
    style H fill:#ff6b6b,color:#fff
    style K fill:#ff6b6b,color:#fff
    style M fill:#ff6b6b,color:#fff
    style N fill:#51cf66,color:#fff
```

---

## 11. Password Security — How Passwords Are Protected

This project uses a multi-layer approach to protect passwords:

```mermaid
flowchart TD
    A["User enters password:<br/><b>MySecret123</b>"] --> B["Step 1: HMAC with Pepper<br/>Combine password with a server secret<br/>using HMAC-SHA256"]

    B --> C["Result: <b>a4f8c2e1b7d9...</b><br/>(64-character hex string)"]

    C --> D["Step 2: bcrypt Hash<br/>bcrypt adds its own random salt<br/>and hashes the result"]

    D --> E["Result: <b>$2b$12$xQ7z...</b><br/>(60-character bcrypt hash)"]

    E --> F["Step 3: Store in Database<br/>Only the final hash is stored<br/>The original password is GONE"]

    G["⚠️ Attacker steals database"] --> H{"Can they recover passwords?"}
    H -->|"No"| I["They only have bcrypt hashes.<br/>bcrypt is intentionally SLOW<br/>to make brute-force impractical."]

    J["⚠️ Attacker also steals database<br/>AND knows bcrypt"] --> K{"Can they recover passwords?"}
    K -->|"Still No"| L["They'd also need the PEPPER<br/>(stored on the server, not in the DB).<br/>Without it, they can't even start."]

    style F fill:#51cf66,color:#fff
    style I fill:#51cf66,color:#fff
    style L fill:#51cf66,color:#fff
```

### Why This Design?

| Layer | What It Does | What It Protects Against |
|---|---|---|
| **HMAC with Pepper** | Mixes the password with a secret key that's only on the server | Database theft — even with the hashes, attackers need the pepper |
| **bcrypt (with salt)** | Hashes the result with a slow algorithm + random salt per password | Rainbow tables (precomputed hashes) and brute-force attacks |
| **Cost factor 12** | bcrypt takes ~250ms per hash (intentionally slow) | Makes trying millions of passwords impractical |

---

## 12. The Architecture Pattern (Hexagonal Architecture)

This project follows a design pattern called **Hexagonal Architecture** (also known as "Ports and Adapters"). The core idea is to organize code into layers, where each layer has a specific responsibility and only talks to its neighbors.

```mermaid
graph LR
    subgraph "Outside World"
        HTTP["🌐 HTTP Requests<br/>(from browsers, apps)"]
        PG["🗄️ PostgreSQL<br/>Database"]
    end

    subgraph "HTTP Layer (Adapters - Inbound)"
        CTRL["Controllers<br/>user_controller.rs<br/>session_controller.rs"]
        EXTRACT["Auth Extractor<br/>auth_extractor.rs"]
        COOKIE["Cookie Helpers<br/>cookies.rs"]
    end

    subgraph "Business Logic (Core)"
        SVC["Services<br/>user_service.rs<br/>session_service.rs<br/>profile_service.rs"]
        CRYPTO["Crypto<br/>hash_password.rs<br/>jwt.rs"]
    end

    subgraph "Data Layer (Adapters - Outbound)"
        REPO["Repositories<br/>user_repository.rs<br/>session_repository.rs<br/>profile_repository.rs"]
    end

    HTTP --> CTRL
    CTRL --> SVC
    SVC --> CRYPTO
    SVC --> REPO
    REPO --> PG
    EXTRACT --> CRYPTO
    CTRL --> COOKIE

    style SVC fill:#e3f2fd,stroke:#1976d2,stroke-width:2px
    style CRYPTO fill:#e3f2fd,stroke:#1976d2,stroke-width:2px
```

### Why This Matters

Each layer can be **changed independently**:

- **Want to switch from PostgreSQL to MySQL?** Only change the Repository layer. The Services don't need to change.
- **Want to switch from Axum to another web framework?** Only change the HTTP layer. The Services don't need to change.
- **Want to test the business logic without a real database?** Use **mock** repositories (fake ones that pretend to be a database). The tests in this project do exactly this.

### The Three Layers Explained

| Layer | Where | What It Does | Example |
|---|---|---|---|
| **HTTP Layer** | `src/*/http/` | Receives web requests, parses them, calls the service, and formats the response. | "Parse the JSON body, call `user_service.create()`, return a 201 response." |
| **Service Layer** | `src/*/usecase/` | Contains the actual business rules and logic. Coordinates between different parts. | "Hash the password, then tell the repository to save the user." |
| **Repository Layer** | `src/*/repository/` | Talks directly to the database. Translates between Rust objects and database rows. | "Run `INSERT INTO users ...` and return the created user." |

---

## 13. Every Technology Used and Why

| Technology | What It Is | Why It's Used Here | Where You See It |
|---|---|---|---|
| **Rust** | A systems programming language | Speed, safety, and reliability for a web server | Everywhere — all `.rs` files |
| **Axum** | A web framework for Rust | Handles HTTP requests and routing (like Express.js for Node.js) | `src/main.rs`, all controller files |
| **Tokio** | An async runtime for Rust | Lets the server handle thousands of requests simultaneously | `#[tokio::main]` in `main.rs` |
| **PostgreSQL** | A relational database | Stores all persistent data (users, sessions, profiles) | Connected via `database_url` |
| **Diesel** | An ORM (Object-Relational Mapper) | Lets you write database queries using Rust code instead of raw SQL | All repository files, `schema.rs` |
| **deadpool-diesel** | A connection pool for Diesel | Reuses database connections instead of opening a new one per request (much faster) | `main.rs`, all repositories |
| **bcrypt** | A password hashing algorithm | Securely hashes passwords so they can't be reversed | `hash_password.rs` |
| **HMAC-SHA256** | A cryptographic signing algorithm | Adds a "pepper" to passwords before hashing for extra security | `hash_password.rs` |
| **jsonwebtoken** | A JWT library | Creates and verifies JWT access tokens | `jwt.rs` |
| **Serde** | A serialization framework | Converts Rust objects to/from JSON (and other formats) | All request/response types |
| **UUID** | Universally Unique Identifier | Generates unique IDs for users, sessions, etc. | All models |
| **Chrono** | A date/time library | Handles timestamps, expiration times, etc. | Sessions, JWT claims |
| **thiserror** | An error-handling library | Makes it easy to define custom error types | All `Error` enums |
| **async-trait** | A Rust macro | Allows async functions in traits (interfaces) | All `*Trait` definitions |
| **config** | A configuration library | Loads settings from environment variables | `main.rs` (Config struct) |
| **dotenvy** | A `.env` file loader | Loads environment variables from a `.env` file during development | `main.rs` |
| **Docker** | A containerization tool | Runs PostgreSQL locally without installing it directly | `docker-compose.yaml` |
| **testcontainers** | A testing library | Spins up temporary PostgreSQL databases in Docker for integration tests | All `#[cfg(test)]` modules |
| **mockall** | A mocking library | Creates fake implementations of traits for unit testing | Service tests |
| **tower-http** | HTTP middleware | Provides CORS (Cross-Origin Resource Sharing) and request tracing | `main.rs` |
| **utoipa** | OpenAPI documentation generator | Auto-generates API documentation from code annotations | `main.rs`, controllers |
| **utoipa-scalar** | API documentation UI | Serves a web page where you can browse and test the API | `main.rs` (at `/scalar`) |
| **rand** | Random number generator | Generates cryptographically random refresh tokens | `session_service.rs` |
| **hex** | Hex encoding library | Converts binary data to hexadecimal strings | `hash_password.rs`, `session_service.rs` |
| **tracing** | A logging/diagnostics framework | Records what the application is doing for debugging | Throughout all files |

---

## 14. How to Set Up and Run the Project

### Prerequisites (Things You Need Installed First)

1. **Rust** — Install from [rustup.rs](https://rustup.rs/). This gives you the `cargo` command.
2. **Docker** and **Docker Compose** — Install from [docker.com](https://www.docker.com/). This runs PostgreSQL.
3. **Diesel CLI** — Install by running:
   ```bash
   cargo install diesel_cli --no-default-features --features postgres
   ```

### Step 1: Start the Database

Open a terminal and run:
```bash
docker-compose up -d
```

This starts two things:
- **PostgreSQL** — The database, available at `localhost:5432`. Username: `postgres`, Password: `example`.
- **Adminer** — A web-based database viewer, available at `http://localhost:8080`.

### Step 2: Create a `.env` File

Create a file called `.env` in the project root (this file is ignored by Git, so it won't be committed):

```env
LISTEN=0.0.0.0:3000
PG__HOST=localhost
PG__PORT=5432
PG__USER=postgres
PG__PASSWORD=example
PG__DBNAME=postgres
DATABASE_URL=postgres://postgres:example@localhost:5432/postgres
PASSWORD_PEPPER=your-secret-pepper-value-here
JWT_SECRET=your-secret-jwt-key-here
INSTAGRAM__CLIENT_ID=your-instagram-client-id
INSTAGRAM__CLIENT_SECRET=your-instagram-client-secret
INSTAGRAM__REDIRECT_URI=http://localhost:3000/oauth/instagram/callback
INSTAGRAM__GRAPH_API_VERSION=v25.0
```

> ⚠️ **Important:** The `PASSWORD_PEPPER` and `JWT_SECRET` should be long, random strings in production. For local development, any string works.

### Step 3: Run Database Migrations

```bash
diesel migration run
```

This creates all the tables in the database (`users`, `sessions`, `profiles`, `social_handles`, `rates`).

### Step 4: Start the Server

```bash
cargo run
```

The first time, this will download all dependencies and compile the project (which can take a few minutes). After that, you'll see:

```
Server running at http://0.0.0.0:3000/
```

### Step 5: Try It Out

You can now use a tool like `curl` or a browser to test the API:

```bash
# Create a user
curl -X POST http://localhost:3000/user \
  -H "Content-Type: application/json" \
  -d '{"email": "test@example.com", "password": "password123"}'

# Log in
curl -X POST http://localhost:3000/user/login \
  -H "Content-Type: application/json" \
  -d '{"email": "test@example.com", "password": "password123"}'

# Get your profile (replace TOKEN with the token from the login response)
curl http://localhost:3000/user/me \
  -H "Authorization: Bearer TOKEN"
```

You can also visit `http://localhost:3000/scalar` in your browser to see the interactive API documentation.

---

## 15. How to Run Tests

```bash
cargo test
```

This runs all tests in the project. **Docker must be running** because the integration tests use `testcontainers` to spin up temporary PostgreSQL databases.

### Types of Tests in This Project

| Type | What It Tests | Example |
|---|---|---|
| **Unit tests** | Individual functions in isolation, using mocks | `user_service.rs` tests use `MockUserRepositoryTrait` instead of a real database |
| **Integration tests** | Real database operations using temporary Docker containers | `user_repository.rs` tests create a real PostgreSQL container and run real queries |

### What "Mocking" Means

When testing the `UserService`, we don't want to need a real database. Instead, we create a **mock** — a fake version of the `UserRepository` that pretends to be a real database. We tell the mock: "When someone calls `find_by_email`, return this specific user." This lets us test the service logic without any external dependencies.

---

## 16. Configuration — Environment Variables

The application reads its configuration from environment variables. The separator for nested values is `__` (double underscore).

### Instagram OAuth Setup (Instagram API with Instagram Login)

1. Go to [Meta for Developers](https://developers.facebook.com/) and create an app.
2. Add the **Instagram** product and configure **Instagram Login**.
3. Configure the OAuth callback URL to match your backend callback endpoint.
4. Copy these values to your environment:
   - `INSTAGRAM__CLIENT_ID` (Instagram App ID — **not** the Facebook App ID)
   - `INSTAGRAM__CLIENT_SECRET` (Instagram App Secret)
   - `INSTAGRAM__REDIRECT_URI` (OAuth callback URL)
   - `INSTAGRAM__GRAPH_API_VERSION` (optional, defaults to `v25.0`)
5. The OAuth scope used is `instagram_business_basic`.

| Variable | Description | Example Value |
|---|---|---|
| `LISTEN` | The address and port the server listens on | `0.0.0.0:3000` |
| `PG__HOST` | PostgreSQL server hostname | `localhost` |
| `PG__PORT` | PostgreSQL server port | `5432` |
| `PG__USER` | PostgreSQL username | `postgres` |
| `PG__PASSWORD` | PostgreSQL password | `example` |
| `PG__DBNAME` | PostgreSQL database name | `postgres` |
| `DATABASE_URL` | Full PostgreSQL connection string (used by Diesel) | `postgres://postgres:example@localhost:5432/postgres` |
| `PASSWORD_PEPPER` | Secret key used to pepper passwords before hashing | (A long random string) |
| `JWT_SECRET` | Secret key used to sign JWT tokens | (A long random string) |
| `INSTAGRAM__CLIENT_ID` | Instagram App ID (uses Instagram Login, not Facebook App ID) | `1234567890` |
| `INSTAGRAM__CLIENT_SECRET` | Instagram App Secret | (Instagram app secret) |
| `INSTAGRAM__REDIRECT_URI` | Instagram OAuth callback URL | `http://localhost:3000/oauth/instagram/callback` |
| `INSTAGRAM__GRAPH_API_VERSION` | Instagram Graph API version | `v25.0` |

> **Security note:** `PASSWORD_PEPPER` and `JWT_SECRET` are extremely sensitive. If someone obtains the `JWT_SECRET`, they can create fake tokens and impersonate any user. If someone obtains the `PASSWORD_PEPPER`, they can attempt to crack password hashes from the database. Never commit these values to version control.

---

## 17. Glossary

| Term | Definition |
|---|---|
| **API** | Application Programming Interface — a way for programs to talk to each other over the network. |
| **Async** | Short for "asynchronous" — a way to handle multiple tasks at the same time without waiting for each to finish. |
| **Authentication** | The process of verifying a user's identity (usually with email + password). |
| **Authorization** | The process of determining what an authenticated user is allowed to do. |
| **bcrypt** | A password hashing algorithm that is intentionally slow to make brute-force attacks impractical. |
| **CORS** | Cross-Origin Resource Sharing — a security feature that controls which websites can access the API. |
| **Cookie** | A small piece of data the server asks the browser to store and send back with future requests. |
| **Crate** | A Rust library or package (like an npm package in JavaScript or a pip package in Python). |
| **CRUD** | Create, Read, Update, Delete — the four basic database operations. |
| **DTO** | Data Transfer Object — a struct used to represent data sent between the client and server. |
| **Endpoint** | A specific URL that the server responds to (e.g., `POST /user`). |
| **Handler** | A function that processes an incoming HTTP request and generates a response. |
| **Hash** | A one-way mathematical transformation that turns input into a fixed-size, seemingly random output. |
| **HMAC** | Hash-based Message Authentication Code — a way to sign data using a secret key. |
| **HTTP** | HyperText Transfer Protocol — the protocol used to send data between browsers and servers. |
| **HttpOnly** | A cookie flag that prevents JavaScript from reading the cookie (security feature). |
| **JWT** | JSON Web Token — a compact, self-contained token used to represent claims between two parties. |
| **Middleware** | Code that runs before or after a request handler, adding cross-cutting functionality. |
| **Migration** | A versioned change to the database schema (like creating or altering tables). |
| **Mock** | A fake implementation of an interface used in testing. |
| **ORM** | Object-Relational Mapper — a tool that lets you interact with a database using programming language objects instead of raw SQL. |
| **Pepper** | A secret value stored on the server that is mixed with passwords before hashing (unlike salts, peppers are not stored in the database). |
| **Pool** | A collection of reusable database connections (creating connections is slow, so we reuse them). |
| **Repository** | A pattern that abstracts database access behind a clean interface. |
| **Revoke** | To deactivate a session or token so it can no longer be used. |
| **Router** | The part of the web framework that maps URLs to handler functions. |
| **Salt** | A random value added to each password before hashing, ensuring identical passwords produce different hashes. |
| **SameSite** | A cookie attribute that controls when the cookie is sent (prevents CSRF attacks). |
| **Schema** | The structure of the database (tables, columns, types, constraints). |
| **Session** | A record of a user's login, stored in the database. Contains a refresh token. |
| **Token** | A string that represents a user's identity and permissions. |
| **Token Rotation** | The practice of replacing a refresh token with a new one each time it's used, so the old one can never be reused. |
| **Trait** | Rust's version of an interface — defines a set of methods that a type must implement. |
| **UUID** | Universally Unique Identifier — a 128-bit number guaranteed to be unique. |
