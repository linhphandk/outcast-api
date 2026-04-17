use std::time::Duration;
use crate::config::InstagramConfig;

pub struct IgClient {
    http: reqwest::Client,
    cfg: InstagramConfig,
}

impl IgClient {
    pub fn new(cfg: InstagramConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to initialize Instagram HTTP client");

        Self { http, cfg }
    }
}
