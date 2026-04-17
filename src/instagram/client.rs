use std::fmt;
use std::time::Duration;

#[derive(Clone)]
pub struct InstagramConfig {
    pub base_url: String,
    pub access_token: String,
}

impl fmt::Debug for InstagramConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstagramConfig")
            .field("base_url", &self.base_url)
            .field("access_token", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct IgClient {
    http: reqwest::Client,
    cfg: InstagramConfig,
}

impl IgClient {
    pub fn new(cfg: InstagramConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client");
        Self { http, cfg }
    }
}
