use ::config::ConfigError;
use deadpool_postgres::Config as PgConfig;
use serde::Deserialize;

fn default_instagram_graph_api_version() -> String {
    "v19.0".to_string()
}

fn default_tiktok_scopes() -> String {
    "user.info.basic,user.info.profile,user.info.stats".into()
}

fn default_tiktok_api_base_url() -> String {
    "https://open.tiktokapis.com".into()
}

fn default_tiktok_auth_base_url() -> String {
    "https://www.tiktok.com".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstagramConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    #[serde(default = "default_instagram_graph_api_version")]
    pub graph_api_version: String,
}

impl InstagramConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        ::config::Config::builder()
            .add_source(
                ::config::Environment::with_prefix("INSTAGRAM").prefix_separator("__"),
            )
            .build()?
            .try_deserialize()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TikTokConfig {
    pub client_key: String,
    pub client_secret: String,
    pub redirect_uri: String,
    #[serde(default = "default_tiktok_scopes")]
    pub scopes: String,
    #[serde(default = "default_tiktok_api_base_url")]
    pub api_base_url: String,
    #[serde(default = "default_tiktok_auth_base_url")]
    pub auth_base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub listen: String,
    pub pg: PgConfig,
    pub database_url: String,
    pub password_pepper: String,
    pub jwt_secret: String,
    pub instagram: InstagramConfig,
    pub tiktok: TikTokConfig,
    pub s3: S3Config,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        ::config::Config::builder()
            .add_source(::config::Environment::default().separator("__"))
            .build()?
            .try_deserialize()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, InstagramConfig};
    use std::sync::{Mutex, OnceLock};

    const INSTAGRAM_CLIENT_ID: &str = "INSTAGRAM__CLIENT_ID";
    const INSTAGRAM_CLIENT_SECRET: &str = "INSTAGRAM__CLIENT_SECRET";
    const INSTAGRAM_REDIRECT_URI: &str = "INSTAGRAM__REDIRECT_URI";
    const INSTAGRAM_GRAPH_API_VERSION: &str = "INSTAGRAM__GRAPH_API_VERSION";
    const TIKTOK_CLIENT_KEY: &str = "TIKTOK__CLIENT_KEY";
    const TIKTOK_CLIENT_SECRET: &str = "TIKTOK__CLIENT_SECRET";
    const TIKTOK_REDIRECT_URI: &str = "TIKTOK__REDIRECT_URI";
    const TIKTOK_SCOPES: &str = "TIKTOK__SCOPES";
    const TIKTOK_API_BASE_URL: &str = "TIKTOK__API_BASE_URL";
    const TIKTOK_AUTH_BASE_URL: &str = "TIKTOK__AUTH_BASE_URL";
    const LISTEN: &str = "LISTEN";
    const DATABASE_URL: &str = "DATABASE_URL";
    const PASSWORD_PEPPER: &str = "PASSWORD_PEPPER";
    const JWT_SECRET: &str = "JWT_SECRET";
    const PG_HOST: &str = "PG__HOST";
    const PG_PORT: &str = "PG__PORT";
    const PG_USER: &str = "PG__USER";
    const PG_PASSWORD: &str = "PG__PASSWORD";
    const PG_DBNAME: &str = "PG__DBNAME";
    const S3_BUCKET: &str = "S3__BUCKET";
    const S3_REGION: &str = "S3__REGION";

    struct EnvGuard {
        previous_values: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn snapshot(keys: &[&str]) -> Self {
            let previous_values = keys
                .iter()
                .map(|key| (key.to_string(), std::env::var(key).ok()))
                .collect();
            Self { previous_values }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.previous_values {
                match value {
                    Some(value) => unsafe { std::env::set_var(key, value) },
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn instagram_config_from_env_happy_path() {
        let _lock = env_lock().lock().expect("env mutex poisoned");
        let _guard = EnvGuard::snapshot(&[
            INSTAGRAM_CLIENT_ID,
            INSTAGRAM_CLIENT_SECRET,
            INSTAGRAM_REDIRECT_URI,
            INSTAGRAM_GRAPH_API_VERSION,
        ]);

        unsafe {
            std::env::set_var(INSTAGRAM_CLIENT_ID, "my-client-id");
            std::env::set_var(INSTAGRAM_CLIENT_SECRET, "my-client-secret");
            std::env::set_var(
                INSTAGRAM_REDIRECT_URI,
                "http://localhost:3000/oauth/instagram/callback",
            );
            std::env::remove_var(INSTAGRAM_GRAPH_API_VERSION);
        }

        let config = InstagramConfig::from_env().expect("instagram config should load");

        assert_eq!(config.client_id, "my-client-id");
        assert_eq!(config.client_secret, "my-client-secret");
        assert_eq!(
            config.redirect_uri,
            "http://localhost:3000/oauth/instagram/callback"
        );
        assert_eq!(config.graph_api_version, "v19.0");
    }

    #[test]
    fn instagram_config_from_env_missing_required_var_returns_error() {
        let _lock = env_lock().lock().expect("env mutex poisoned");
        let _guard = EnvGuard::snapshot(&[
            INSTAGRAM_CLIENT_ID,
            INSTAGRAM_CLIENT_SECRET,
            INSTAGRAM_REDIRECT_URI,
            INSTAGRAM_GRAPH_API_VERSION,
        ]);

        unsafe {
            std::env::set_var(INSTAGRAM_CLIENT_ID, "my-client-id");
            std::env::remove_var(INSTAGRAM_CLIENT_SECRET);
            std::env::set_var(
                INSTAGRAM_REDIRECT_URI,
                "http://localhost:3000/oauth/instagram/callback",
            );
            std::env::set_var(INSTAGRAM_GRAPH_API_VERSION, "v19.0");
        }

        let result = InstagramConfig::from_env();
        assert!(result.is_err());
    }

    #[test]
    fn loads_tiktok_section() {
        let _lock = env_lock().lock().expect("env mutex poisoned");
        let _guard = EnvGuard::snapshot(&[
            LISTEN,
            DATABASE_URL,
            PASSWORD_PEPPER,
            JWT_SECRET,
            PG_HOST,
            PG_PORT,
            PG_USER,
            PG_PASSWORD,
            PG_DBNAME,
            INSTAGRAM_CLIENT_ID,
            INSTAGRAM_CLIENT_SECRET,
            INSTAGRAM_REDIRECT_URI,
            INSTAGRAM_GRAPH_API_VERSION,
            TIKTOK_CLIENT_KEY,
            TIKTOK_CLIENT_SECRET,
            TIKTOK_REDIRECT_URI,
            TIKTOK_SCOPES,
            TIKTOK_API_BASE_URL,
            TIKTOK_AUTH_BASE_URL,
            S3_BUCKET,
            S3_REGION,
        ]);

        unsafe {
            std::env::set_var(LISTEN, "0.0.0.0:3000");
            std::env::set_var(DATABASE_URL, "postgres://postgres:example@localhost:5432/postgres");
            std::env::set_var(PASSWORD_PEPPER, "pepper");
            std::env::set_var(JWT_SECRET, "jwt-secret");
            std::env::set_var(PG_HOST, "localhost");
            std::env::set_var(PG_PORT, "5432");
            std::env::set_var(PG_USER, "postgres");
            std::env::set_var(PG_PASSWORD, "example");
            std::env::set_var(PG_DBNAME, "postgres");
            std::env::set_var(INSTAGRAM_CLIENT_ID, "ig-client-id");
            std::env::set_var(INSTAGRAM_CLIENT_SECRET, "ig-client-secret");
            std::env::set_var(
                INSTAGRAM_REDIRECT_URI,
                "http://localhost:3000/oauth/instagram/callback",
            );
            std::env::remove_var(INSTAGRAM_GRAPH_API_VERSION);
            std::env::set_var(TIKTOK_CLIENT_KEY, "tt-client-key");
            std::env::set_var(TIKTOK_CLIENT_SECRET, "tt-client-secret");
            std::env::set_var(TIKTOK_REDIRECT_URI, "http://localhost:3000/oauth/tiktok/callback");
            std::env::set_var(
                TIKTOK_SCOPES,
                "user.info.basic,user.info.profile,user.info.stats",
            );
            std::env::set_var(TIKTOK_API_BASE_URL, "https://open.tiktokapis.com");
            std::env::set_var(TIKTOK_AUTH_BASE_URL, "https://www.tiktok.com");
            std::env::set_var(S3_BUCKET, "outcast-uploads");
            std::env::set_var(S3_REGION, "eu-north-1");
        }

        let config = AppConfig::from_env().expect("app config should load");

        assert_eq!(config.tiktok.client_key, "tt-client-key");
        assert_eq!(config.tiktok.client_secret, "tt-client-secret");
        assert_eq!(
            config.tiktok.redirect_uri,
            "http://localhost:3000/oauth/tiktok/callback"
        );
        assert_eq!(
            config.tiktok.scopes,
            "user.info.basic,user.info.profile,user.info.stats"
        );
        assert_eq!(config.tiktok.api_base_url, "https://open.tiktokapis.com");
        assert_eq!(config.tiktok.auth_base_url, "https://www.tiktok.com");
    }

    #[test]
    fn loads_tiktok_section_with_optional_defaults() {
        let _lock = env_lock().lock().expect("env mutex poisoned");
        let _guard = EnvGuard::snapshot(&[
            LISTEN,
            DATABASE_URL,
            PASSWORD_PEPPER,
            JWT_SECRET,
            PG_HOST,
            PG_PORT,
            PG_USER,
            PG_PASSWORD,
            PG_DBNAME,
            INSTAGRAM_CLIENT_ID,
            INSTAGRAM_CLIENT_SECRET,
            INSTAGRAM_REDIRECT_URI,
            INSTAGRAM_GRAPH_API_VERSION,
            TIKTOK_CLIENT_KEY,
            TIKTOK_CLIENT_SECRET,
            TIKTOK_REDIRECT_URI,
            TIKTOK_SCOPES,
            TIKTOK_API_BASE_URL,
            TIKTOK_AUTH_BASE_URL,
            S3_BUCKET,
            S3_REGION,
        ]);

        unsafe {
            std::env::set_var(LISTEN, "0.0.0.0:3000");
            std::env::set_var(DATABASE_URL, "postgres://postgres:example@localhost:5432/postgres");
            std::env::set_var(PASSWORD_PEPPER, "pepper");
            std::env::set_var(JWT_SECRET, "jwt-secret");
            std::env::set_var(PG_HOST, "localhost");
            std::env::set_var(PG_PORT, "5432");
            std::env::set_var(PG_USER, "postgres");
            std::env::set_var(PG_PASSWORD, "example");
            std::env::set_var(PG_DBNAME, "postgres");
            std::env::set_var(INSTAGRAM_CLIENT_ID, "ig-client-id");
            std::env::set_var(INSTAGRAM_CLIENT_SECRET, "ig-client-secret");
            std::env::set_var(
                INSTAGRAM_REDIRECT_URI,
                "http://localhost:3000/oauth/instagram/callback",
            );
            std::env::remove_var(INSTAGRAM_GRAPH_API_VERSION);
            std::env::set_var(TIKTOK_CLIENT_KEY, "tt-client-key");
            std::env::set_var(TIKTOK_CLIENT_SECRET, "tt-client-secret");
            std::env::set_var(TIKTOK_REDIRECT_URI, "http://localhost:3000/oauth/tiktok/callback");
            std::env::remove_var(TIKTOK_SCOPES);
            std::env::remove_var(TIKTOK_API_BASE_URL);
            std::env::remove_var(TIKTOK_AUTH_BASE_URL);
            std::env::set_var(S3_BUCKET, "outcast-uploads");
            std::env::set_var(S3_REGION, "eu-north-1");
        }

        let config = AppConfig::from_env().expect("app config should load");

        assert_eq!(
            config.tiktok.scopes,
            "user.info.basic,user.info.profile,user.info.stats"
        );
        assert_eq!(config.tiktok.api_base_url, "https://open.tiktokapis.com");
        assert_eq!(config.tiktok.auth_base_url, "https://www.tiktok.com");
    }
}
