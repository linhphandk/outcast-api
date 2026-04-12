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