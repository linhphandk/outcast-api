use diesel::RunQueryDsl;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use tokio::sync::{Mutex, MutexGuard, OnceCell};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

struct SharedTestDb {
    _container: testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
    pool: deadpool_diesel::postgres::Pool,
}

pub struct TestDbSession {
    pub pool: deadpool_diesel::postgres::Pool,
    _guard: MutexGuard<'static, ()>,
}

static SHARED_TEST_DB: OnceCell<SharedTestDb> = OnceCell::const_new();
static DB_TEST_LOCK: Mutex<()> = Mutex::const_new(());

async fn init_test_db() -> SharedTestDb {
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default().start().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let host = container.get_host().await.unwrap();
    let conn_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    let manager =
        deadpool_diesel::postgres::Manager::new(conn_string, deadpool_diesel::Runtime::Tokio1);
    let pool = deadpool_diesel::postgres::Pool::builder(manager).build().unwrap();

    let conn = pool.get().await.unwrap();
    conn.interact(|conn| conn.run_pending_migrations(MIGRATIONS).map(|_| ()))
        .await
        .unwrap()
        .unwrap();

    SharedTestDb {
        _container: container,
        pool,
    }
}

async fn reset_test_db(pool: &deadpool_diesel::postgres::Pool) {
    let conn = pool.get().await.unwrap();
    conn.interact(|conn| {
        diesel::sql_query(
            "TRUNCATE TABLE oauth_tokens, social_handles, rates, profiles, sessions, users RESTART IDENTITY CASCADE",
        )
        .execute(conn)
    })
    .await
    .unwrap()
    .unwrap();
}

pub async fn acquire_test_db() -> TestDbSession {
    let guard = DB_TEST_LOCK.lock().await;
    let db = SHARED_TEST_DB.get_or_init(init_test_db).await;
    reset_test_db(&db.pool).await;

    TestDbSession {
        pool: db.pool.clone(),
        _guard: guard,
    }
}
