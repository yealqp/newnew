//! Request-log persistence, port of Go internal/service/log.

use serde_json::{json, Map, Value};
use sqlx::SqlitePool;

use crate::billing::{BillingResult, Usage};
use crate::db::get_setting;
use crate::models::{Channel, Token, SETTING_LOG_BODY_MAX_BYTES};
use crate::util::{now_db_string, truncate_body};

#[derive(Default)]
pub struct WriteInput<'a> {
    pub request_id: String,
    pub token: Option<&'a Token>,
    pub channel: Option<&'a Channel>,
    pub model: String,
    pub upstream_model: String,
    pub is_stream: bool,
    pub duration_ms: i64,
    pub first_token_ms: i64,
    pub usage: Usage,
    pub cost: BillingResult,
    pub status: String,
    pub error_message: String,
    pub ip: String,
    pub request_body: String,
    pub response_body: String,
    pub detail: Option<Map<String, Value>>,
}

pub async fn write(pool: &SqlitePool, input: WriteInput<'_>) {
    let mut max_bytes: usize = 65536;
    let v = get_setting(pool, SETTING_LOG_BODY_MAX_BYTES).await;
    if !v.is_empty() {
        if let Ok(n) = v.parse::<usize>() {
            if n > 0 {
                max_bytes = n;
            }
        }
    }

    let mut detail = input.detail.unwrap_or_default();
    if input.cost.price_missing {
        detail.insert("price_missing".to_string(), Value::Bool(true));
    } else {
        detail.insert(
            "price".to_string(),
            serde_json::to_value(&input.cost.price).unwrap_or(Value::Null),
        );
    }
    let detail_json = Value::Object(detail).to_string();

    let (token_id, token_name) = match input.token {
        Some(t) => (t.id, t.name_str().to_string()),
        None => (0, String::new()),
    };
    let (channel_id, channel_name) = match input.channel {
        Some(c) => (c.id, c.name_str().to_string()),
        None => (0, String::new()),
    };

    let _ = sqlx::query(
        "INSERT INTO logs (created_at, request_id, token_id, token_name, channel_id, channel_name, \
         model, upstream_model, is_stream, duration_ms, first_token_ms, prompt_tokens, completion_tokens, \
         cache_read_tokens, cache_write_tokens, total_tokens, cost_rmb, status, error_message, ip, \
         request_body, response_body, detail) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(now_db_string())
    .bind(&input.request_id)
    .bind(token_id)
    .bind(&token_name)
    .bind(channel_id)
    .bind(&channel_name)
    .bind(&input.model)
    .bind(&input.upstream_model)
    .bind(input.is_stream)
    .bind(input.duration_ms)
    .bind(input.first_token_ms)
    .bind(input.usage.prompt_tokens)
    .bind(input.usage.completion_tokens)
    .bind(input.usage.cache_read_tokens)
    .bind(input.usage.cache_write_tokens)
    .bind(input.usage.prompt_tokens + input.usage.completion_tokens)
    .bind(input.cost.cost_rmb)
    .bind(&input.status)
    .bind(&input.error_message)
    .bind(&input.ip)
    .bind(truncate_body(&input.request_body, max_bytes))
    .bind(truncate_body(&input.response_body, max_bytes))
    .bind(detail_json)
    .execute(pool)
    .await;
}

/// Direct log insert used by channel-test / playground (no truncation settings
/// lookup differences: they wrote raw bodies in Go, so keep raw here too).
#[allow(clippy::too_many_arguments)]
pub async fn write_raw(pool: &SqlitePool, log: RawLog) {
    let _ = sqlx::query(
        "INSERT INTO logs (created_at, request_id, token_id, token_name, channel_id, channel_name, \
         model, upstream_model, is_stream, duration_ms, first_token_ms, prompt_tokens, completion_tokens, \
         cache_read_tokens, cache_write_tokens, total_tokens, cost_rmb, status, error_message, ip, \
         request_body, response_body, detail) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(now_db_string())
    .bind(&log.request_id)
    .bind(log.token_id)
    .bind(&log.token_name)
    .bind(log.channel_id)
    .bind(&log.channel_name)
    .bind(&log.model)
    .bind(&log.upstream_model)
    .bind(log.is_stream)
    .bind(log.duration_ms)
    .bind(log.first_token_ms)
    .bind(log.usage.prompt_tokens)
    .bind(log.usage.completion_tokens)
    .bind(log.usage.cache_read_tokens)
    .bind(log.usage.cache_write_tokens)
    .bind(log.usage.prompt_tokens + log.usage.completion_tokens)
    .bind(log.cost_rmb)
    .bind(&log.status)
    .bind(&log.error_message)
    .bind(&log.ip)
    .bind(&log.request_body)
    .bind(&log.response_body)
    .bind(json!({}).to_string())
    .execute(pool)
    .await;
}

#[derive(Default)]
pub struct RawLog {
    pub request_id: String,
    pub token_id: i64,
    pub token_name: String,
    pub channel_id: i64,
    pub channel_name: String,
    pub model: String,
    pub upstream_model: String,
    pub is_stream: bool,
    pub duration_ms: i64,
    pub first_token_ms: i64,
    pub usage: Usage,
    pub cost_rmb: f64,
    pub status: String,
    pub error_message: String,
    pub ip: String,
    pub request_body: String,
    pub response_body: String,
}
