use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: String,
    pub db_path: String,
    pub jwt_secret: String,
    pub admin_user: String,
    pub admin_password: String,
    pub request_timeout: u64, // seconds
}

impl Config {
    pub fn load() -> Self {
        let _ = dotenvy::dotenv();
        Self {
            port: get_env("PORT", "3000"),
            db_path: get_env("DB_PATH", "data/gateway.db"),
            jwt_secret: get_env("JWT_SECRET", "change-me-in-production-please"),
            admin_user: get_env("ADMIN_USER", "admin"),
            admin_password: get_env("ADMIN_PASSWORD", "admin123"),
            request_timeout: get_env("REQUEST_TIMEOUT", "300").parse().unwrap_or(300),
        }
    }
}

fn get_env(key: &str, def: &str) -> String {
    match env::var(key) {
        Ok(v) if !v.is_empty() => v,
        _ => def.to_string(),
    }
}
