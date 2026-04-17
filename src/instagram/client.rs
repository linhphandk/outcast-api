use std::time::Duration;

#[derive(Clone, Debug)]
pub struct InstagramConfig {
    pub base_url: String,
    pub access_token: String,
}

#[derive(Clone, Debug)]
pub struct IgClient {
    pub http: reqwest::Client,
    pub cfg: InstagramConfig,
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
