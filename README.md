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
2. Copy `.env.example` to `.env` and fill in values
3. Run migrations: `diesel migration run`
4. Start the server: `cargo run`

### Instagram OAuth Configuration
To configure Instagram OAuth, create a Meta app and copy the credentials into your `.env` file.

1. Go to [Meta for Developers](https://developers.facebook.com/) and create an app.
2. Add **Instagram Graph API** to the app.
3. Configure OAuth redirect URL to match:
   - `INSTAGRAM__REDIRECT_URI` (default: `http://localhost:3000/oauth/instagram/callback`)
4. Copy values from the Meta app dashboard into:
   - `INSTAGRAM__CLIENT_ID`
   - `INSTAGRAM__CLIENT_SECRET`
   - `INSTAGRAM__REDIRECT_URI`
   - `INSTAGRAM__GRAPH_API_VERSION` (optional override, default `v19.0`)

### Testing
To run the integration tests:
```bash
cargo test
```
