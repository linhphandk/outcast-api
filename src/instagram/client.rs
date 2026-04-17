use std::time::Duration;

#[derive(Clone, Debug)]
pub struct InstagramConfig {
    // Placeholder for future configuration
}

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
