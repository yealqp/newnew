use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: Arc<Config>,
    /// Shared HTTP client; per-request timeouts are set on each request.
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(pool: SqlitePool, cfg: Config) -> Self {
        Self {
            pool,
            cfg: Arc::new(cfg),
            http: reqwest::Client::new(),
        }
    }

    /// Effective relay timeout in seconds: settings.request_timeout,
    /// falling back to the env config value.
    pub async fn request_timeout_secs(&self) -> u64 {
        let v = crate::db::get_setting(&self.pool, crate::models::SETTING_REQUEST_TIMEOUT).await;
        match v.parse::<u64>() {
            Ok(n) if n > 0 => n,
            _ => self.cfg.request_timeout,
        }
    }
}
