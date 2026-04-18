use ::config::ConfigError;
use deadpool_postgres::Config as PgConfig;
use serde::Deserialize;

fn default_instagram_graph_api_version() -> String {
    "v19.0".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint_url: Option<String>,
}

impl S3Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        ::config::Config::builder()
            .add_source(::config::Environment::with_prefix("S3").prefix_separator("__"))
            .build()?
            .try_deserialize()
    }
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
pub struct AppConfig {
    pub listen: String,
    pub pg: PgConfig,
    pub database_url: String,
    pub password_pepper: String,
    pub jwt_secret: String,
    pub instagram: InstagramConfig,
    pub s3: S3Config,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        #[derive(Debug, Deserialize)]
        struct BaseAppConfig {
            listen: String,
            pg: PgConfig,
            database_url: String,
            password_pepper: String,
            jwt_secret: String,
        }

        let base_config: BaseAppConfig = ::config::Config::builder()
            .add_source(::config::Environment::default().separator("__"))
            .build()?
            .try_deserialize()?;

        let instagram = InstagramConfig::from_env()?;
        let s3 = S3Config::from_env()?;

        Ok(Self {
            listen: base_config.listen,
            pg: base_config.pg,
            database_url: base_config.database_url,
            password_pepper: base_config.password_pepper,
            jwt_secret: base_config.jwt_secret,
            instagram,
            s3,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::InstagramConfig;
    use std::sync::{Mutex, OnceLock};

    const INSTAGRAM_CLIENT_ID: &str = "INSTAGRAM__CLIENT_ID";
    const INSTAGRAM_CLIENT_SECRET: &str = "INSTAGRAM__CLIENT_SECRET";
    const INSTAGRAM_REDIRECT_URI: &str = "INSTAGRAM__REDIRECT_URI";
    const INSTAGRAM_GRAPH_API_VERSION: &str = "INSTAGRAM__GRAPH_API_VERSION";

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
}
