# outcast-api

A Rust-based backend API built with focus on modularity and clean separation of concerns.

## Architecture

This project follows **Hexagonal Architecture** (Ports and Adapters) to ensure business logic remains decoupled from external dependencies like the database and the web framework.

- **`/src/domain`**: The application core containing business logic and entities.
  - **`/usecase`**: Orchestrates the flow of data to and from the domain entities.
  - **`/repository`**: Defines ports (traits) for data persistence and provides adapters (e.g., PostgreSQL).
  - **`/http`**: The delivery layer using the Axum framework.

## Tech Stack

- **Framework**: Axum
- **Database**: PostgreSQL
- **Connection Pooling**: deadpool-postgres
- **Migrations**: Diesel
- **Async Runtime**: Tokio

## Development

### Prerequisites
- Docker and Docker Compose
- Diesel CLI

### Setup
1. Start the database: `docker-compose up -d`
2. Run migrations: `diesel migration run`
3. Start the server: `cargo run`

### Testing
To run the integration tests:
```bash
cargo test
```

### Instagram OAuth Configuration (Instagram API with Instagram Login)
1. Create a Meta app in the [Meta for Developers](https://developers.facebook.com/) dashboard.
2. Add the **Instagram** product and configure **Instagram Login**.
3. Copy the app credentials into your `.env`:
   - `INSTAGRAM__CLIENT_ID` ← Instagram App ID (**not** the Facebook App ID)
   - `INSTAGRAM__CLIENT_SECRET` ← Instagram App Secret
   - `INSTAGRAM__REDIRECT_URI` ← OAuth callback URL configured in the app settings (default: `http://localhost:3000/oauth/instagram/callback`)
4. Optionally set `INSTAGRAM__GRAPH_API_VERSION` (defaults to `v25.0`).
5. The OAuth scope used is `instagram_business_basic`.
