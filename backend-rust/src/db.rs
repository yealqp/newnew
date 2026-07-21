//! Database init: schema (identical to the GORM-generated one so an existing
//! gateway.db keeps working), admin/settings seeding, settings helpers.

use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tracing::info;

use crate::config::Config;
use crate::models::{
    PRICE_POLICY_ALLOW, SETTING_LOG_BODY_MAX_BYTES, SETTING_PRICE_MISSING_POLICY,
    SETTING_REQUEST_TIMEOUT,
};

const SCHEMA: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS `users` (`id` integer PRIMARY KEY AUTOINCREMENT,`username` text NOT NULL,`password_hash` text NOT NULL,`created_at` datetime,`updated_at` datetime)",
    "CREATE UNIQUE INDEX IF NOT EXISTS `idx_users_username` ON `users`(`username`)",
    "CREATE TABLE IF NOT EXISTS `tokens` (`id` integer PRIMARY KEY AUTOINCREMENT,`name` text NOT NULL,`key` text NOT NULL,`status` integer DEFAULT 1,`model_limits` text,`expired_at` integer DEFAULT 0,`created_at` datetime,`accessed_at` datetime)",
    "CREATE UNIQUE INDEX IF NOT EXISTS `idx_tokens_key` ON `tokens`(`key`)",
    "CREATE TABLE IF NOT EXISTS `channels` (`id` integer PRIMARY KEY AUTOINCREMENT,`name` text NOT NULL,`type` text NOT NULL,`base_url` text NOT NULL,`full_url` numeric DEFAULT false,`api_key` text NOT NULL,`models` text,`model_mapping` text,`status` integer DEFAULT 1,`weight` integer DEFAULT 1,`priority` integer DEFAULT 0,`pricing` text,`remark` text,`response_time` integer DEFAULT 0,`test_time` integer DEFAULT 0,`created_at` datetime,`updated_at` datetime,`icon` text DEFAULT '')",
    "CREATE TABLE IF NOT EXISTS `logs` (`id` integer PRIMARY KEY AUTOINCREMENT,`created_at` datetime,`request_id` text,`token_id` integer,`token_name` text,`channel_id` integer,`channel_name` text,`model` text,`upstream_model` text,`is_stream` numeric,`duration_ms` integer,`prompt_tokens` integer,`completion_tokens` integer,`cache_read_tokens` integer,`cache_write_tokens` integer,`total_tokens` integer,`cost_rmb` real,`status` text,`error_message` text,`ip` text,`request_body` text,`response_body` text,`detail` text,`first_token_ms` integer DEFAULT 0)",
    "CREATE INDEX IF NOT EXISTS `idx_logs_channel_id` ON `logs`(`channel_id`)",
    "CREATE INDEX IF NOT EXISTS `idx_logs_token_id` ON `logs`(`token_id`)",
    "CREATE INDEX IF NOT EXISTS `idx_logs_request_id` ON `logs`(`request_id`)",
    "CREATE INDEX IF NOT EXISTS `idx_logs_created_at` ON `logs`(`created_at`)",
    "CREATE INDEX IF NOT EXISTS `idx_logs_status` ON `logs`(`status`)",
    "CREATE INDEX IF NOT EXISTS `idx_logs_model` ON `logs`(`model`)",
    "CREATE TABLE IF NOT EXISTS `settings` (`key` text,`value` text,PRIMARY KEY (`key`))",
    "CREATE TABLE IF NOT EXISTS `conversations` (`id` integer PRIMARY KEY AUTOINCREMENT,`title` text,`model` text,`created_at` datetime,`updated_at` datetime)",
    "CREATE TABLE IF NOT EXISTS `conversation_messages` (`id` integer PRIMARY KEY AUTOINCREMENT,`conversation_id` integer NOT NULL,`role` text NOT NULL,`content` text,`created_at` datetime)",
    "CREATE INDEX IF NOT EXISTS `idx_conversation_messages_conversation_id` ON `conversation_messages`(`conversation_id`)",
];

pub async fn init(cfg: &Config) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    if let Some(dir) = Path::new(&cfg.db_path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }

    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", cfg.db_path))?
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;

    for stmt in SCHEMA {
        sqlx::query(stmt).execute(&pool).await?;
    }

    admin_password_recovery(&pool, cfg).await?;
    seed_settings(&pool).await?;
    Ok(pool)
}

/// First-run admin creation now happens via the /api/admin/setup flow; this
/// only keeps the ADMIN_RESET_PASSWORD escape hatch for locked-out admins.
async fn admin_password_recovery(pool: &SqlitePool, cfg: &Config) -> Result<(), sqlx::Error> {
    let reset = std::env::var("ADMIN_RESET_PASSWORD").unwrap_or_default();
    if reset == "1" || reset == "true" {
        let hash = bcrypt::hash(&cfg.admin_password, bcrypt::DEFAULT_COST)
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        sqlx::query("UPDATE users SET password_hash = ? WHERE username = ?")
            .bind(&hash)
            .bind(&cfg.admin_user)
            .execute(pool)
            .await?;
        info!(
            "[seed] admin password reset via ADMIN_RESET_PASSWORD: username={} password={}",
            cfg.admin_user, cfg.admin_password
        );
    }
    Ok(())
}

async fn seed_settings(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let defaults: [(&str, &str); 3] = [
        (SETTING_LOG_BODY_MAX_BYTES, "65536"),
        (SETTING_PRICE_MISSING_POLICY, PRICE_POLICY_ALLOW),
        (SETTING_REQUEST_TIMEOUT, "300"),
    ];
    for (k, v) in defaults {
        sqlx::query("INSERT OR IGNORE INTO settings (`key`, `value`) VALUES (?, ?)")
            .bind(k)
            .bind(v)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn get_setting(pool: &SqlitePool, key: &str) -> String {
    sqlx::query_scalar::<_, Option<String>>("SELECT `value` FROM settings WHERE `key` = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .flatten()
        .unwrap_or_default()
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO settings (`key`, `value`) VALUES (?, ?) \
         ON CONFLICT(`key`) DO UPDATE SET `value` = excluded.`value`",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}
