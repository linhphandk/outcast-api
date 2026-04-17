use ::config::ConfigError;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub listen: String,
    pub pg: deadpool_postgres::Config,
    pub database_url: String,
    pub password_pepper: String,
    pub jwt_secret: String,
    pub instagram: InstagramConfig,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        ::config::Config::builder()
            .add_source(::config::Environment::default().separator("__"))
            .add_source(
                ::config::Environment::with_prefix("INSTAGRAM")
                    .separator("__")
                    .keep_prefix(true),
            )
            .build()?
            .try_deserialize()
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct InstagramConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    #[serde(default = "default_graph_api_version")]
    pub graph_api_version: String,
}

impl InstagramConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_environment(::config::Environment::with_prefix("INSTAGRAM").separator("__"))
    }

    fn from_environment(environment: ::config::Environment) -> Result<Self, ConfigError> {
        ::config::Config::builder()
            .add_source(environment)
            .build()?
            .try_deserialize()
    }
}

fn default_graph_api_version() -> String {
    "v19.0".to_string()
}

#[cfg(test)]
mod tests {
    use super::InstagramConfig;
    use std::collections::HashMap;

    #[test]
    fn instagram_config_from_env_happy_path() {
        let mut env = HashMap::new();
        env.insert("INSTAGRAM__CLIENT_ID".to_string(), "ig-client-id".to_string());
        env.insert(
            "INSTAGRAM__CLIENT_SECRET".to_string(),
            "ig-client-secret".to_string(),
        );
        env.insert(
            "INSTAGRAM__REDIRECT_URI".to_string(),
            "http://localhost:3000/oauth/instagram/callback".to_string(),
        );

        let cfg = InstagramConfig::from_environment(
            ::config::Environment::with_prefix("INSTAGRAM")
                .separator("__")
                .source(Some(env)),
        )
        .expect("expected instagram config to deserialize");

        assert_eq!(cfg.client_id, "ig-client-id");
        assert_eq!(cfg.client_secret, "ig-client-secret");
        assert_eq!(
            cfg.redirect_uri,
            "http://localhost:3000/oauth/instagram/callback"
        );
        assert_eq!(cfg.graph_api_version, "v19.0");
    }

    #[test]
    fn instagram_config_from_env_missing_required_var_returns_error() {
        let mut env = HashMap::new();
        env.insert("INSTAGRAM__CLIENT_ID".to_string(), "ig-client-id".to_string());
        env.insert(
            "INSTAGRAM__REDIRECT_URI".to_string(),
            "http://localhost:3000/oauth/instagram/callback".to_string(),
        );

        let err = InstagramConfig::from_environment(
            ::config::Environment::with_prefix("INSTAGRAM")
                .separator("__")
                .source(Some(env)),
        )
        .expect_err("expected deserialization error when required variable is missing");

        let msg = err.to_string();
        assert!(
            msg.contains("client_secret"),
            "error should mention missing client_secret, got: {msg}"
        );
    }
}
