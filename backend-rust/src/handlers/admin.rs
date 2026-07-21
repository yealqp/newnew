//! Admin API handlers (auth, channels, tokens, logs, dashboard, settings).
//! Port of Go internal/handler/admin/admin.go, excluding the playground
//! handlers (those live in handlers/playground.rs).

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use chrono::{Duration, Local, SecondsFormat, TimeZone};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};

use crate::billing;
use crate::channel_select;
use crate::db;
use crate::logsvc;
use crate::middleware::{self, AuthUser, ClientIp, RequestId};
use crate::models::{
    self, Channel, RequestLog, Token, User, CHANNEL_STATUS_ENABLED, CHANNEL_TYPE_CLAUDE,
    CHANNEL_TYPE_OPENAI, TOKEN_STATUS_ENABLED,
};
use crate::state::AppState;
use crate::upstream;
use crate::util;

// ---- Auth ----

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct LoginReq {
    username: String,
    password: String,
}

pub async fn login(State(st): State<AppState>, body: Bytes) -> Response {
    let req: LoginReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    let user = match sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
        .bind(&req.username)
        .fetch_optional(&st.pool)
        .await
    {
        Ok(Some(u)) => u,
        Ok(None) => return util::fail_resp(401, "invalid username or password"),
        Err(_) => return util::fail_resp(401, "invalid username or password"),
    };
    let valid = bcrypt::verify(&req.password, &user.password_hash).unwrap_or(false);
    if !valid {
        return util::fail_resp(401, "invalid username or password");
    }
    let token = match middleware::generate_jwt(&st.cfg.jwt_secret, user.id, &user.username) {
        Ok(t) => t,
        Err(_) => return util::fail_resp(500, "token error"),
    };
    util::ok_resp(json!({"token": token, "username": user.username}))
}

pub async fn me(Extension(user): Extension<AuthUser>) -> Response {
    util::ok_resp(json!({"username": user.username, "id": user.user_id}))
}

// ---- Setup (first-run initialization) ----

pub async fn setup_status(State(st): State<AppState>) -> Response {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&st.pool)
        .await
        .unwrap_or(0);
    util::ok_resp(json!({"initialized": count > 0}))
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct SetupReq {
    username: String,
    password: String,
}

pub async fn setup(State(st): State<AppState>, body: Bytes) -> Response {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&st.pool)
        .await
        .unwrap_or(0);
    if count > 0 {
        return util::fail_resp(400, "already initialized");
    }

    let req: SetupReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    let username = req.username.trim().to_string();
    if username.len() < 3 {
        return util::fail_resp(400, "username too short (min 3 characters)");
    }
    if req.password.len() < 6 {
        return util::fail_resp(400, "password too short (min 6 characters)");
    }

    let hash = match bcrypt::hash(&req.password, bcrypt::DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => return util::fail_resp(500, "hash error"),
    };
    let now = util::now_db_string();
    let res = sqlx::query(
        "INSERT INTO users (username, password_hash, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&username)
    .bind(&hash)
    .bind(&now)
    .bind(&now)
    .execute(&st.pool)
    .await;
    let user_id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };

    let token = match middleware::generate_jwt(&st.cfg.jwt_secret, user_id, &username) {
        Ok(t) => t,
        Err(_) => return util::fail_resp(500, "token error"),
    };
    util::ok_resp(json!({"token": token, "username": username}))
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ChangePasswordReq {
    old_password: String,
    new_username: String,
    new_password: String,
}

pub async fn change_password(
    State(st): State<AppState>,
    Extension(user): Extension<AuthUser>,
    body: Bytes,
) -> Response {
    let req: ChangePasswordReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    let mut u = match sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(user.user_id)
        .fetch_optional(&st.pool)
        .await
    {
        Ok(Some(u)) => u,
        Ok(None) => return util::fail_resp(404, "user not found"),
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };
    if !bcrypt::verify(&req.old_password, &u.password_hash).unwrap_or(false) {
        return util::fail_resp(400, "old password incorrect");
    }

    let mut changed = false;
    let new_name = req.new_username.trim().to_string();
    if !new_name.is_empty() && new_name != u.username {
        // Go compares len() in bytes, not runes; mirror that exactly.
        if new_name.len() < 3 {
            return util::fail_resp(400, "username too short");
        }
        let cnt: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = ? AND id <> ?")
            .bind(&new_name)
            .bind(u.id)
            .fetch_one(&st.pool)
            .await
            .unwrap_or(0);
        if cnt > 0 {
            return util::fail_resp(400, "username already exists");
        }
        u.username = new_name;
        changed = true;
    }
    if !req.new_password.is_empty() {
        if req.new_password.len() < 6 {
            return util::fail_resp(400, "password too short");
        }
        let hash = match bcrypt::hash(&req.new_password, bcrypt::DEFAULT_COST) {
            Ok(h) => h,
            Err(_) => return util::fail_resp(500, "hash error"),
        };
        u.password_hash = hash;
        changed = true;
    }
    if !changed {
        return util::fail_resp(400, "nothing to update");
    }

    let now = util::now_db_string();
    if let Err(e) = sqlx::query("UPDATE users SET username = ?, password_hash = ?, updated_at = ? WHERE id = ?")
        .bind(&u.username)
        .bind(&u.password_hash)
        .bind(&now)
        .bind(u.id)
        .execute(&st.pool)
        .await
    {
        return util::fail_resp(500, &e.to_string());
    }

    let token = match middleware::generate_jwt(&st.cfg.jwt_secret, u.id, &u.username) {
        Ok(t) => t,
        Err(_) => return util::fail_resp(500, "token error"),
    };
    util::ok_resp(json!({"username": u.username, "token": token}))
}

// ---- Channels ----

#[derive(Debug, Clone, Default)]
struct ChannelUsage {
    requests: i64,
    total_tokens: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cost_rmb: f64,
}

fn channel_view(ch: &Channel, full_key: bool, usage: &ChannelUsage) -> Value {
    let key = if full_key {
        ch.api_key_str().to_string()
    } else {
        util::mask_key(ch.api_key_str())
    };
    json!({
        "id": ch.id,
        "name": ch.name.as_deref().unwrap_or(""),
        "type": ch.channel_type.as_deref().unwrap_or(""),
        "base_url": ch.base_url.as_deref().unwrap_or(""),
        "full_url": ch.is_full_url(),
        "api_key": key,
        "models": ch.models.as_deref().unwrap_or(""),
        "model_mapping": ch.model_mapping.as_deref().unwrap_or(""),
        "status": ch.status.unwrap_or(0),
        "weight": ch.weight.unwrap_or(0),
        "priority": ch.priority.unwrap_or(0),
        "pricing": ch.pricing.as_deref().unwrap_or(""),
        "remark": ch.remark.as_deref().unwrap_or(""),
        "icon": ch.icon.as_deref().unwrap_or(""),
        "response_time": ch.response_time.unwrap_or(0),
        "test_time": ch.test_time.unwrap_or(0),
        "created_at": util::opt_db_time_to_rfc3339(&ch.created_at),
        "updated_at": util::opt_db_time_to_rfc3339(&ch.updated_at),
        "total_tokens": usage.total_tokens,
        "prompt_tokens": usage.prompt_tokens,
        "completion_tokens": usage.completion_tokens,
        "requests": usage.requests,
        "cost_rmb": usage.cost_rmb,
    })
}

pub async fn list_channels(State(st): State<AppState>) -> Response {
    let list = match sqlx::query_as::<_, Channel>("SELECT * FROM channels ORDER BY id DESC")
        .fetch_all(&st.pool)
        .await
    {
        Ok(v) => v,
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };

    let rows = sqlx::query(
        "SELECT channel_id, count(*) as requests, coalesce(sum(total_tokens),0) as total, \
         coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, \
         coalesce(sum(cost_rmb),0) as cost_rmb FROM logs WHERE status = 'success' GROUP BY channel_id",
    )
    .fetch_all(&st.pool)
    .await
    .unwrap_or_default();

    let mut usage_map: HashMap<i64, ChannelUsage> = HashMap::new();
    for row in &rows {
        let channel_id: i64 = row.try_get("channel_id").unwrap_or(0);
        usage_map.insert(
            channel_id,
            ChannelUsage {
                requests: row.try_get("requests").unwrap_or(0),
                total_tokens: row.try_get("total").unwrap_or(0),
                prompt_tokens: row.try_get("prompt_tokens").unwrap_or(0),
                completion_tokens: row.try_get("completion_tokens").unwrap_or(0),
                cost_rmb: row.try_get("cost_rmb").unwrap_or(0.0),
            },
        );
    }

    let out: Vec<Value> = list
        .iter()
        .map(|ch| {
            let usage = usage_map.get(&ch.id).cloned().unwrap_or_default();
            channel_view(ch, false, &usage)
        })
        .collect();
    util::ok_resp(Value::Array(out))
}

pub async fn get_channel(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    match sqlx::query_as::<_, Channel>("SELECT * FROM channels WHERE id = ?")
        .bind(id)
        .fetch_optional(&st.pool)
        .await
    {
        Ok(Some(ch)) => util::ok_resp(channel_view(&ch, true, &ChannelUsage::default())),
        Ok(None) => util::fail_resp(404, "not found"),
        Err(e) => util::fail_resp(500, &e.to_string()),
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ChannelReq {
    icon: String,
    name: String,
    #[serde(rename = "type")]
    channel_type: String,
    base_url: String,
    full_url: bool,
    api_key: String,
    models: String,
    model_mapping: String,
    status: i64,
    /// u64 so a negative weight fails deserialization -> 400 "invalid body",
    /// matching Go's uint field semantics.
    weight: u64,
    priority: i64,
    pricing: String,
    remark: String,
}

pub async fn create_channel(State(st): State<AppState>, body: Bytes) -> Response {
    let req: ChannelReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    if req.name.is_empty() || req.channel_type.is_empty() || req.base_url.is_empty() || req.api_key.is_empty() {
        return util::fail_resp(400, "name, type, base_url, api_key required");
    }
    if req.channel_type != CHANNEL_TYPE_OPENAI && req.channel_type != CHANNEL_TYPE_CLAUDE {
        return util::fail_resp(400, "type must be openai or claude");
    }
    let weight = if req.weight == 0 { 1 } else { req.weight as i64 };
    let status = if req.status == 0 { CHANNEL_STATUS_ENABLED } else { req.status };
    let now = util::now_db_string();

    let result = sqlx::query(
        "INSERT INTO channels (icon, name, type, base_url, full_url, api_key, models, model_mapping, \
         status, weight, priority, pricing, remark, created_at, updated_at) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&req.icon)
    .bind(&req.name)
    .bind(&req.channel_type)
    .bind(&req.base_url)
    .bind(req.full_url)
    .bind(&req.api_key)
    .bind(&req.models)
    .bind(&req.model_mapping)
    .bind(status)
    .bind(weight)
    .bind(req.priority)
    .bind(&req.pricing)
    .bind(&req.remark)
    .bind(&now)
    .bind(&now)
    .execute(&st.pool)
    .await;
    let result = match result {
        Ok(r) => r,
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };

    let ch = Channel {
        id: result.last_insert_rowid(),
        icon: Some(req.icon),
        name: Some(req.name),
        channel_type: Some(req.channel_type),
        base_url: Some(req.base_url),
        full_url: Some(req.full_url),
        api_key: Some(req.api_key),
        models: Some(req.models),
        model_mapping: Some(req.model_mapping),
        status: Some(status),
        weight: Some(weight),
        priority: Some(req.priority),
        pricing: Some(req.pricing),
        remark: Some(req.remark),
        response_time: Some(0),
        test_time: Some(0),
        created_at: Some(now.clone()),
        updated_at: Some(now),
    };
    util::ok_resp(channel_view(&ch, false, &ChannelUsage::default()))
}

pub async fn update_channel(State(st): State<AppState>, Path(id): Path<i64>, body: Bytes) -> Response {
    let mut ch = match sqlx::query_as::<_, Channel>("SELECT * FROM channels WHERE id = ?")
        .bind(id)
        .fetch_optional(&st.pool)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => return util::fail_resp(404, "not found"),
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };
    let req: ChannelReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };

    ch.name = Some(req.name);
    ch.channel_type = Some(req.channel_type);
    ch.base_url = Some(req.base_url);
    ch.full_url = Some(req.full_url);
    if !req.api_key.is_empty() && !req.api_key.contains("***") {
        ch.api_key = Some(req.api_key);
    }
    ch.models = Some(req.models);
    ch.model_mapping = Some(req.model_mapping);
    ch.status = Some(req.status);
    ch.weight = Some(req.weight as i64);
    ch.priority = Some(req.priority);
    ch.pricing = Some(req.pricing);
    ch.remark = Some(req.remark);
    ch.icon = Some(req.icon);
    let now = util::now_db_string();
    ch.updated_at = Some(now.clone());

    let result = sqlx::query(
        "UPDATE channels SET icon=?, name=?, type=?, base_url=?, full_url=?, api_key=?, models=?, \
         model_mapping=?, status=?, weight=?, priority=?, pricing=?, remark=?, updated_at=? WHERE id=?",
    )
    .bind(ch.icon.as_deref().unwrap_or(""))
    .bind(ch.name.as_deref().unwrap_or(""))
    .bind(ch.channel_type.as_deref().unwrap_or(""))
    .bind(ch.base_url.as_deref().unwrap_or(""))
    .bind(ch.full_url.unwrap_or(false))
    .bind(ch.api_key.as_deref().unwrap_or(""))
    .bind(ch.models.as_deref().unwrap_or(""))
    .bind(ch.model_mapping.as_deref().unwrap_or(""))
    .bind(ch.status.unwrap_or(0))
    .bind(ch.weight.unwrap_or(0))
    .bind(ch.priority.unwrap_or(0))
    .bind(ch.pricing.as_deref().unwrap_or(""))
    .bind(ch.remark.as_deref().unwrap_or(""))
    .bind(&now)
    .bind(id)
    .execute(&st.pool)
    .await;
    if let Err(e) = result {
        return util::fail_resp(500, &e.to_string());
    }
    util::ok_resp(channel_view(&ch, false, &ChannelUsage::default()))
}

pub async fn delete_channel(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    match sqlx::query("DELETE FROM channels WHERE id = ?")
        .bind(id)
        .execute(&st.pool)
        .await
    {
        Ok(_) => util::ok_resp(Value::Null),
        Err(e) => util::fail_resp(500, &e.to_string()),
    }
}

fn channel_test_request_body(channel_type: &str, upstream_model: &str, max_tokens: i64) -> String {
    json!({
        "model": upstream_model,
        "max_tokens": max_tokens,
        "messages": [{"role": "user", "content": "hi"}],
        "stream": false,
        "type": channel_type,
    })
    .to_string()
}

fn extract_channel_test_usage(channel_type: &str, raw: &[u8]) -> billing::Usage {
    let v: Value = match serde_json::from_slice(raw) {
        Ok(v) => v,
        Err(_) => return billing::Usage::default(),
    };
    if channel_type == CHANNEL_TYPE_OPENAI {
        let usage_v = match v.get("usage").filter(|u| u.is_object()) {
            Some(u) => u,
            None => return billing::Usage::default(),
        };
        let prompt = usage_v.get("prompt_tokens").and_then(Value::as_i64).unwrap_or(0);
        let completion = usage_v.get("completion_tokens").and_then(Value::as_i64).unwrap_or(0);
        let cache_read = usage_v
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        return billing::Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            cache_read_tokens: cache_read,
            cache_write_tokens: 0,
        };
    }

    let usage_v = v.get("usage");
    let input = usage_v.and_then(|u| u.get("input_tokens")).and_then(Value::as_i64).unwrap_or(0);
    let output = usage_v.and_then(|u| u.get("output_tokens")).and_then(Value::as_i64).unwrap_or(0);
    let cache_read = usage_v
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_write = usage_v
        .and_then(|u| u.get("cache_creation_input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    billing::Usage {
        prompt_tokens: input + cache_read + cache_write,
        completion_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
    }
}

/// GET /api/admin/channels/{id}/test
///
/// Deviation from Go: the id path segment is bound as `Path<i64>` per the
/// fixed handler signature, so a non-numeric id is rejected by axum's own
/// extractor (generic 400) before this handler runs, instead of Go's
/// custom `fail("invalid channel id")` body. All other behavior matches.
pub async fn test_channel(
    State(st): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Extension(client_ip): Extension<ClientIp>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let ch = match sqlx::query_as::<_, Channel>("SELECT * FROM channels WHERE id = ?")
        .bind(id)
        .fetch_optional(&st.pool)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => return util::fail_resp(404, "channel not found"),
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };

    let mut test_model = params
        .get("model")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if test_model.is_empty() {
        if let Some(m) = ch.get_models().into_iter().next() {
            test_model = m;
        }
    }
    if test_model.is_empty() {
        return util::fail_resp(400, "channel has no models to test");
    }

    let upstream_model = ch.map_model(&test_model);
    let api_key = channel_select::pick_key(&ch);
    if api_key.trim().is_empty() {
        return util::fail_resp(400, "channel has no api key");
    }

    let mut timeout_sec: u64 = 30;
    let v = db::get_setting(&st.pool, models::SETTING_REQUEST_TIMEOUT).await;
    if let Ok(n) = v.parse::<u64>() {
        if n > 0 && n < timeout_sec {
            timeout_sec = n;
        }
    }

    let max_tokens: i64 = 16;
    let body_bytes = serde_json::to_vec(&json!({
        "model": upstream_model,
        "max_tokens": max_tokens,
        "messages": [{"role": "user", "content": "hi"}],
        "stream": false,
    }))
    .unwrap_or_default();

    let start = Instant::now();

    let send_result = if ch.type_str() == CHANNEL_TYPE_OPENAI {
        upstream::post_chat_completions(
            &st.http,
            ch.base_url_str(),
            &api_key,
            body_bytes,
            false,
            ch.is_full_url(),
            timeout_sec,
        )
        .await
    } else if ch.type_str() == CHANNEL_TYPE_CLAUDE {
        upstream::post_messages(
            &st.http,
            ch.base_url_str(),
            &api_key,
            body_bytes,
            false,
            ch.is_full_url(),
            timeout_sec,
        )
        .await
    } else {
        return util::fail_resp(400, "unsupported channel type");
    };

    let mut status_code: u16 = 0;
    let mut resp_body: Vec<u8> = Vec::new();
    let mut test_err: Option<String> = None;

    match send_result {
        Ok(resp) => {
            status_code = resp.status().as_u16();
            resp_body = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
            if status_code >= 400 {
                test_err = Some(format!(
                    "upstream {}: {}",
                    status_code,
                    upstream::pretty_upstream_error(&resp_body)
                ));
            }
        }
        Err(e) => {
            test_err = Some(e.to_string());
        }
    }

    let ms = start.elapsed().as_millis() as i64;
    let now_unix = util::unix_now();
    let _ = sqlx::query("UPDATE channels SET response_time = ?, test_time = ? WHERE id = ?")
        .bind(ms)
        .bind(now_unix)
        .bind(id)
        .execute(&st.pool)
        .await;

    let usage = extract_channel_test_usage(ch.type_str(), &resp_body);
    let cost = billing::calculate_for_channel(&ch, &test_model, &usage);
    let (test_status, test_err_message) = match &test_err {
        Some(e) => ("error", e.clone()),
        None => ("success", String::new()),
    };

    logsvc::write_raw(
        &st.pool,
        logsvc::RawLog {
            request_id: request_id.0.clone(),
            token_id: 0,
            token_name: "测试".to_string(),
            channel_id: ch.id,
            channel_name: ch.name_str().to_string(),
            model: test_model.clone(),
            upstream_model: upstream_model.clone(),
            is_stream: false,
            duration_ms: ms,
            first_token_ms: 0,
            usage,
            cost_rmb: cost.cost_rmb,
            status: test_status.to_string(),
            error_message: test_err_message,
            ip: client_ip.0.clone(),
            request_body: channel_test_request_body(ch.type_str(), &upstream_model, max_tokens),
            response_body: String::from_utf8_lossy(&resp_body).into_owned(),
        },
    )
    .await;

    if let Some(err_msg) = test_err {
        return util::resp(
            StatusCode::OK,
            Json(json!({
                "success": false,
                "message": err_msg,
                "data": {
                    "channel_id": ch.id,
                    "model": test_model,
                    "upstream_model": upstream_model,
                    "response_time": ms,
                    "time": ms as f64 / 1000.0,
                    "status_code": status_code,
                }
            })),
        );
    }

    let preview = util::truncate_str(&String::from_utf8_lossy(&resp_body), 500);
    util::ok_resp(json!({
        "channel_id": ch.id,
        "model": test_model,
        "upstream_model": upstream_model,
        "response_time": ms,
        "time": ms as f64 / 1000.0,
        "status_code": status_code,
        "preview": preview,
    }))
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct FetchModelsReq {
    base_url: String,
    api_key: String,
    #[serde(rename = "type")]
    channel_type: String,
    full_url: bool,
    channel_id: i64,
}

fn build_models_url(base_url: &str, full_url: bool) -> String {
    let base = base_url.trim().trim_end_matches('/').to_string();
    if full_url {
        let lower = base.to_lowercase();
        for suffix in ["/chat/completions", "/v1/chat/completions", "/messages", "/v1/messages"] {
            if lower.ends_with(suffix) {
                return format!("{}/models", &base[..base.len() - suffix.len()]);
            }
        }
        if lower.ends_with("/models") {
            return base;
        }
        if let Some(i) = base.rfind('/') {
            if i > 0 {
                return format!("{}/models", &base[..i]);
            }
        }
        return format!("{base}/models");
    }
    let lower = base.to_lowercase();
    if lower.ends_with("/v1") {
        return format!("{base}/models");
    }
    if lower.ends_with("/v1/models") {
        return base;
    }
    format!("{base}/v1/models")
}

/// Port of Go parseModelIDs, including its "bug-compatible" behavior: if the
/// top-level JSON is not an object (e.g. a bare array), the equivalent Go
/// `map[string]any` unmarshal fails and the function returns no models —
/// the Go "plain string array" fallback is dead code for real top-level
/// arrays, and we intentionally do not resurrect it here.
fn parse_model_ids(body: &[u8]) -> Vec<String> {
    #[derive(Deserialize)]
    struct OaiItem {
        #[serde(default)]
        id: String,
    }
    #[derive(Deserialize, Default)]
    struct OaiResp {
        #[serde(default)]
        data: Vec<OaiItem>,
    }

    if let Ok(oai) = serde_json::from_slice::<OaiResp>(body) {
        if !oai.data.is_empty() {
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for item in &oai.data {
                let id = item.id.trim().to_string();
                if id.is_empty() || seen.contains(&id) {
                    continue;
                }
                seen.insert(id.clone());
                out.push(id);
            }
            return out;
        }
    }

    let generic: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let obj = match generic.as_object() {
        Some(o) => o,
        None => return Vec::new(),
    };
    if let Some(Value::Array(data)) = obj.get("data") {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for item in data {
            if let Value::Object(m) = item {
                let mut id = m.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if id.is_empty() {
                    id = m.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
                }
                let id = id.trim().to_string();
                if id.is_empty() || seen.contains(&id) {
                    continue;
                }
                seen.insert(id.clone());
                out.push(id);
            }
        }
        return out;
    }
    Vec::new()
}

async fn fetch_upstream_model_ids(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    channel_type: &str,
    full_url: bool,
) -> Result<Vec<String>, String> {
    let url = build_models_url(base_url, full_url);
    let mut req = http
        .get(&url)
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(30));
    if channel_type == CHANNEL_TYPE_CLAUDE {
        req = req.header("x-api-key", api_key).header("anthropic-version", "2023-06-01");
    } else {
        req = req.header("Authorization", format!("Bearer {api_key}"));
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return Err(format!("请求上游失败: {e}")),
    };
    let status = resp.status();
    let body = resp.bytes().await.unwrap_or_default();
    if status.as_u16() >= 400 {
        let msg = String::from_utf8_lossy(&body);
        let msg = util::truncate_str_plain(&msg, 300);
        return Err(format!("上游返回 {}: {}", status.as_u16(), msg));
    }

    let ids = parse_model_ids(&body);
    if ids.is_empty() {
        let raw = String::from_utf8_lossy(&body);
        return Err(format!(
            "上游未返回可用模型，原始响应: {}",
            util::truncate_str(&raw, 200)
        ));
    }
    Ok(ids)
}

pub async fn fetch_upstream_models(State(st): State<AppState>, body: Bytes) -> Response {
    let mut req: FetchModelsReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    req.base_url = req.base_url.trim().to_string();
    req.channel_type = req.channel_type.trim().to_string();
    if req.channel_type.is_empty() {
        req.channel_type = CHANNEL_TYPE_OPENAI.to_string();
    }
    if req.channel_type != CHANNEL_TYPE_OPENAI && req.channel_type != CHANNEL_TYPE_CLAUDE {
        return util::fail_resp(400, "type must be openai or claude");
    }
    if req.base_url.is_empty() {
        return util::fail_resp(400, "base_url required");
    }

    let mut api_key = req.api_key.trim().to_string();
    if api_key.is_empty() || api_key.contains("***") {
        if req.channel_id == 0 {
            return util::fail_resp(400, "请填写 API Key，或先保存渠道后再获取");
        }
        let ch = match sqlx::query_as::<_, Channel>("SELECT * FROM channels WHERE id = ?")
            .bind(req.channel_id)
            .fetch_optional(&st.pool)
            .await
        {
            Ok(Some(c)) => c,
            Ok(None) => return util::fail_resp(404, "channel not found"),
            Err(e) => return util::fail_resp(500, &e.to_string()),
        };
        let keys = ch.get_keys();
        if keys.is_empty() {
            return util::fail_resp(400, "渠道未配置 API Key");
        }
        api_key = keys[0].clone();
        if req.base_url.is_empty() {
            req.base_url = ch.base_url_str().to_string();
        }
        if !req.full_url {
            req.full_url = ch.is_full_url();
        }
    } else {
        // Take the first line, mirroring Go's unconditional
        // `strings.Split(apiKey, "\n")[0]` (a no-op when there's no newline).
        api_key = api_key.split('\n').next().unwrap_or("").trim().to_string();
    }
    if api_key.is_empty() {
        return util::fail_resp(400, "api_key required");
    }

    match fetch_upstream_model_ids(&st.http, &req.base_url, &api_key, &req.channel_type, req.full_url).await {
        Ok(mut ids) => {
            ids.sort();
            util::ok_resp(json!({"models": ids, "count": ids.len()}))
        }
        Err(msg) => util::fail_resp(502, &msg),
    }
}

// ---- Tokens ----

pub async fn list_tokens(State(st): State<AppState>) -> Response {
    match sqlx::query_as::<_, Token>("SELECT * FROM tokens ORDER BY id DESC")
        .fetch_all(&st.pool)
        .await
    {
        Ok(list) => {
            let arr: Vec<Value> = list.iter().map(|t| t.to_json()).collect();
            util::ok_resp(Value::Array(arr))
        }
        Err(e) => util::fail_resp(500, &e.to_string()),
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TokenReq {
    name: String,
    status: i64,
    model_limits: String,
    expired_at: i64,
}

pub async fn create_token(State(st): State<AppState>, body: Bytes) -> Response {
    let req: TokenReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    if req.name.is_empty() {
        return util::fail_resp(400, "name required");
    }
    let key = format!("sk-{}", util::random_hex(24));
    let now = util::now_db_string();
    let result = sqlx::query(
        "INSERT INTO tokens (name, key, status, model_limits, expired_at, created_at) VALUES (?,?,?,?,?,?)",
    )
    .bind(&req.name)
    .bind(&key)
    .bind(TOKEN_STATUS_ENABLED)
    .bind(&req.model_limits)
    .bind(req.expired_at)
    .bind(&now)
    .execute(&st.pool)
    .await;
    let result = match result {
        Ok(r) => r,
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };
    let tok = Token {
        id: result.last_insert_rowid(),
        name: Some(req.name),
        key: Some(key),
        status: Some(TOKEN_STATUS_ENABLED),
        model_limits: Some(req.model_limits),
        expired_at: Some(req.expired_at),
        created_at: Some(now),
        accessed_at: None,
    };
    util::ok_resp(tok.to_json())
}

pub async fn update_token(State(st): State<AppState>, Path(id): Path<i64>, body: Bytes) -> Response {
    let mut tok = match sqlx::query_as::<_, Token>("SELECT * FROM tokens WHERE id = ?")
        .bind(id)
        .fetch_optional(&st.pool)
        .await
    {
        Ok(Some(t)) => t,
        Ok(None) => return util::fail_resp(404, "not found"),
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };
    let req: TokenReq = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    tok.name = Some(req.name);
    tok.status = Some(req.status);
    tok.model_limits = Some(req.model_limits);
    tok.expired_at = Some(req.expired_at);

    let result = sqlx::query("UPDATE tokens SET name=?, status=?, model_limits=?, expired_at=? WHERE id=?")
        .bind(tok.name.as_deref().unwrap_or(""))
        .bind(tok.status.unwrap_or(0))
        .bind(tok.model_limits.as_deref().unwrap_or(""))
        .bind(tok.expired_at.unwrap_or(0))
        .bind(id)
        .execute(&st.pool)
        .await;
    if let Err(e) = result {
        return util::fail_resp(500, &e.to_string());
    }
    util::ok_resp(tok.to_json())
}

pub async fn reset_token_key(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    let mut tok = match sqlx::query_as::<_, Token>("SELECT * FROM tokens WHERE id = ?")
        .bind(id)
        .fetch_optional(&st.pool)
        .await
    {
        Ok(Some(t)) => t,
        Ok(None) => return util::fail_resp(404, "not found"),
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };
    let key = format!("sk-{}", util::random_hex(24));
    tok.key = Some(key.clone());
    if let Err(e) = sqlx::query("UPDATE tokens SET key = ? WHERE id = ?")
        .bind(&key)
        .bind(id)
        .execute(&st.pool)
        .await
    {
        return util::fail_resp(500, &e.to_string());
    }
    util::ok_resp(tok.to_json())
}

pub async fn delete_token(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    match sqlx::query("DELETE FROM tokens WHERE id = ?")
        .bind(id)
        .execute(&st.pool)
        .await
    {
        Ok(_) => util::ok_resp(Value::Null),
        Err(e) => util::fail_resp(500, &e.to_string()),
    }
}

// ---- Logs ----

pub async fn list_logs(State(st): State<AppState>, Query(params): Query<HashMap<String, String>>) -> Response {
    let mut page: i64 = params.get("page").and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut page_size: i64 = params.get("page_size").and_then(|v| v.parse().ok()).unwrap_or(20);
    if page < 1 {
        page = 1;
    }
    if !(1..=100).contains(&page_size) {
        page_size = 20;
    }

    let mut conditions: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    for col in ["token_id", "channel_id", "model", "status"] {
        if let Some(v) = params.get(col) {
            if !v.is_empty() {
                conditions.push(format!("{col} = ?"));
                binds.push(v.clone());
            }
        }
    }
    if let Some(v) = params.get("start") {
        if !v.is_empty() {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(v) {
                let local = dt.with_timezone(&Local);
                conditions.push("created_at >= ?".to_string());
                binds.push(util::to_db_string(&local));
            }
        }
    }
    if let Some(v) = params.get("end") {
        if !v.is_empty() {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(v) {
                let local = dt.with_timezone(&Local);
                conditions.push("created_at <= ?".to_string());
                binds.push(util::to_db_string(&local));
            }
        }
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM logs{where_clause}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    let total: i64 = cq.fetch_one(&st.pool).await.unwrap_or(0);

    let list_sql = format!("SELECT * FROM logs{where_clause} ORDER BY id DESC LIMIT ? OFFSET ?");
    let mut lq = sqlx::query_as::<_, RequestLog>(&list_sql);
    for b in &binds {
        lq = lq.bind(b);
    }
    lq = lq.bind(page_size).bind((page - 1) * page_size);
    let list = match lq.fetch_all(&st.pool).await {
        Ok(v) => v,
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };

    util::ok_resp(json!({
        "list": list.iter().map(|l| l.to_json()).collect::<Vec<_>>(),
        "total": total,
        "page": page,
        "page_size": page_size,
    }))
}

pub async fn get_log(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    match sqlx::query_as::<_, RequestLog>("SELECT * FROM logs WHERE id = ?")
        .bind(id)
        .fetch_optional(&st.pool)
        .await
    {
        Ok(Some(l)) => util::ok_resp(l.to_json()),
        Ok(None) => util::fail_resp(404, "not found"),
        Err(e) => util::fail_resp(500, &e.to_string()),
    }
}

// ---- Dashboard ----

#[derive(Debug, Default)]
struct Agg {
    requests: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cost_rmb: f64,
}

async fn fetch_agg(pool: &SqlitePool, where_clause: &str, binds: &[String]) -> Agg {
    let sql = if where_clause.is_empty() {
        "SELECT count(*) as requests, coalesce(sum(prompt_tokens),0) as prompt_tokens, \
         coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb FROM logs"
            .to_string()
    } else {
        format!(
            "SELECT count(*) as requests, coalesce(sum(prompt_tokens),0) as prompt_tokens, \
             coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb \
             FROM logs WHERE {where_clause}"
        )
    };
    let mut q = sqlx::query(&sql);
    for b in binds {
        q = q.bind(b);
    }
    match q.fetch_one(pool).await {
        Ok(row) => Agg {
            requests: row.try_get("requests").unwrap_or(0),
            prompt_tokens: row.try_get("prompt_tokens").unwrap_or(0),
            completion_tokens: row.try_get("completion_tokens").unwrap_or(0),
            cost_rmb: row.try_get("cost_rmb").unwrap_or(0.0),
        },
        Err(_) => Agg::default(),
    }
}

pub async fn dashboard(State(st): State<AppState>, Query(params): Query<HashMap<String, String>>) -> Response {
    let start_str = params.get("start").map(|s| s.trim().to_string()).unwrap_or_default();
    let end_str = params.get("end").map(|s| s.trim().to_string()).unwrap_or_default();
    if !start_str.is_empty() && !end_str.is_empty() {
        let granularity = params.get("granularity").map(|s| s.trim().to_string()).unwrap_or_default();
        return dashboard_range(&st.pool, &start_str, &end_str, &granularity).await;
    }

    let now = Local::now();
    let naive_midnight = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap_or_else(|| now.naive_local());
    let today_start = Local
        .from_local_datetime(&naive_midnight)
        .earliest()
        .unwrap_or(now);
    let week_start = today_start - Duration::days(6);

    let today = fetch_agg(&st.pool, "created_at >= ?", &[util::to_db_string(&today_start)]).await;
    let total = fetch_agg(&st.pool, "", &[]).await;

    let series_sql = "SELECT strftime('%Y-%m-%d', created_at) as day, count(*) as requests, \
         coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, \
         coalesce(sum(cost_rmb),0) as cost_rmb FROM logs WHERE created_at >= ? \
         GROUP BY strftime('%Y-%m-%d', created_at) ORDER BY day ASC";
    let rows = sqlx::query(series_sql)
        .bind(util::to_db_string(&week_start))
        .fetch_all(&st.pool)
        .await
        .unwrap_or_default();
    let series: Vec<Value> = rows
        .iter()
        .map(|r| {
            let day: String = r.try_get("day").unwrap_or_default();
            let requests: i64 = r.try_get("requests").unwrap_or(0);
            let prompt_tokens: i64 = r.try_get("prompt_tokens").unwrap_or(0);
            let completion_tokens: i64 = r.try_get("completion_tokens").unwrap_or(0);
            let cost_rmb: f64 = r.try_get("cost_rmb").unwrap_or(0.0);
            json!({
                "day": day, "requests": requests, "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens, "cost_rmb": cost_rmb,
            })
        })
        .collect();

    let channel_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels")
        .fetch_one(&st.pool)
        .await
        .unwrap_or(0);
    let token_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tokens")
        .fetch_one(&st.pool)
        .await
        .unwrap_or(0);

    util::ok_resp(json!({
        "today": {
            "requests": today.requests,
            "prompt_tokens": today.prompt_tokens,
            "completion_tokens": today.completion_tokens,
            "total_tokens": today.prompt_tokens + today.completion_tokens,
            "cost_rmb": today.cost_rmb,
        },
        "total": {
            "requests": total.requests,
            "prompt_tokens": total.prompt_tokens,
            "completion_tokens": total.completion_tokens,
            "total_tokens": total.prompt_tokens + total.completion_tokens,
            "cost_rmb": total.cost_rmb,
        },
        "series": series,
        "channel_count": channel_count,
        "token_count": token_count,
    }))
}

async fn dashboard_range(pool: &SqlitePool, start_str: &str, end_str: &str, granularity: &str) -> Response {
    let (mut start, mut end) = match (util::parse_flexible_time(start_str), util::parse_flexible_time(end_str)) {
        (Some(s), Some(e)) => (s, e),
        _ => return util::fail_resp(400, "invalid start/end time"),
    };
    if end < start {
        std::mem::swap(&mut start, &mut end);
    }

    let start_db = util::to_db_string(&start);
    let end_db = util::to_db_string(&end);

    let summary = fetch_agg(
        pool,
        "created_at >= ? AND created_at <= ?",
        &[start_db.clone(), end_db.clone()],
    )
    .await;

    let duration_min = {
        let secs = (end - start).num_milliseconds() as f64 / 1000.0;
        let m = secs / 60.0;
        if m < 1.0 {
            1.0
        } else {
            m
        }
    };
    let total_tokens_f = (summary.prompt_tokens + summary.completion_tokens) as f64;
    let rpm = summary.requests as f64 / duration_min;
    let tpm = total_tokens_f / duration_min;

    const HOUR_EXPR: &str = "strftime('%Y-%m-%d %H:00', created_at)";
    const DAY_EXPR: &str = "strftime('%Y-%m-%d', created_at)";
    const WEEK_EXPR: &str = "date(created_at, '-' || ((strftime('%w', created_at) + 6) % 7) || ' days')";
    const MONTH_EXPR: &str = "strftime('%Y-%m', created_at)";

    let bucket_expr: &str = match granularity {
        "hour" => HOUR_EXPR,
        "day" => DAY_EXPR,
        "week" => WEEK_EXPR,
        "month" => MONTH_EXPR,
        _ => {
            if (end - start) <= Duration::hours(48) {
                HOUR_EXPR
            } else {
                DAY_EXPR
            }
        }
    };

    let series_sql = format!(
        "SELECT {bucket_expr} as time, count(*) as requests, coalesce(sum(prompt_tokens),0) as prompt_tokens, \
         coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb \
         FROM logs WHERE created_at >= ? AND created_at <= ? GROUP BY {bucket_expr} ORDER BY time ASC"
    );
    let rows = sqlx::query(&series_sql)
        .bind(&start_db)
        .bind(&end_db)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let series: Vec<Value> = rows
        .iter()
        .map(|r| {
            let time: String = r.try_get("time").unwrap_or_default();
            let requests: i64 = r.try_get("requests").unwrap_or(0);
            let prompt_tokens: i64 = r.try_get("prompt_tokens").unwrap_or(0);
            let completion_tokens: i64 = r.try_get("completion_tokens").unwrap_or(0);
            let cost_rmb: f64 = r.try_get("cost_rmb").unwrap_or(0.0);
            json!({
                "time": time, "requests": requests, "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens, "total_tokens": prompt_tokens + completion_tokens,
                "cost_rmb": cost_rmb,
            })
        })
        .collect();

    let dist_sql = "SELECT coalesce(nullif(channel_name,''), 'unknown') as channel_name, \
         coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, \
         coalesce(sum(cost_rmb),0) as cost_rmb FROM logs WHERE created_at >= ? AND created_at <= ? AND status = 'success' \
         GROUP BY coalesce(nullif(channel_name,''), 'unknown') ORDER BY prompt_tokens + completion_tokens DESC";
    let rows = sqlx::query(dist_sql)
        .bind(&start_db)
        .bind(&end_db)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let distribution: Vec<Value> = rows
        .iter()
        .map(|r| {
            let channel_name: String = r.try_get("channel_name").unwrap_or_default();
            let prompt_tokens: i64 = r.try_get("prompt_tokens").unwrap_or(0);
            let completion_tokens: i64 = r.try_get("completion_tokens").unwrap_or(0);
            let cost_rmb: f64 = r.try_get("cost_rmb").unwrap_or(0.0);
            json!({
                "channel_name": channel_name, "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens, "total_tokens": prompt_tokens + completion_tokens,
                "cost_rmb": cost_rmb,
            })
        })
        .collect();

    let model_stats_sql = "SELECT coalesce(nullif(model,''), 'unknown') as model, count(*) as count, \
         coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, \
         coalesce(sum(cost_rmb),0) as cost_rmb FROM logs WHERE created_at >= ? AND created_at <= ? \
         GROUP BY coalesce(nullif(model,''), 'unknown') ORDER BY count DESC";
    let rows = sqlx::query(model_stats_sql)
        .bind(&start_db)
        .bind(&end_db)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let model_stats: Vec<Value> = rows
        .iter()
        .map(|r| {
            let model: String = r.try_get("model").unwrap_or_default();
            let count: i64 = r.try_get("count").unwrap_or(0);
            let prompt_tokens: i64 = r.try_get("prompt_tokens").unwrap_or(0);
            let completion_tokens: i64 = r.try_get("completion_tokens").unwrap_or(0);
            let cost_rmb: f64 = r.try_get("cost_rmb").unwrap_or(0.0);
            json!({
                "model": model, "count": count, "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens, "total_tokens": prompt_tokens + completion_tokens,
                "cost_rmb": cost_rmb,
            })
        })
        .collect();

    let model_series_sql = format!(
        "SELECT {bucket_expr} as time, coalesce(nullif(model,''), 'unknown') as model, count(*) as count, \
         coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, \
         coalesce(sum(cost_rmb),0) as cost_rmb FROM logs WHERE created_at >= ? AND created_at <= ? \
         GROUP BY {bucket_expr}, coalesce(nullif(model,''), 'unknown') ORDER BY time ASC"
    );
    let rows = sqlx::query(&model_series_sql)
        .bind(&start_db)
        .bind(&end_db)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let model_series: Vec<Value> = rows
        .iter()
        .map(|r| {
            let time: String = r.try_get("time").unwrap_or_default();
            let model: String = r.try_get("model").unwrap_or_default();
            let count: i64 = r.try_get("count").unwrap_or(0);
            let prompt_tokens: i64 = r.try_get("prompt_tokens").unwrap_or(0);
            let completion_tokens: i64 = r.try_get("completion_tokens").unwrap_or(0);
            let cost_rmb: f64 = r.try_get("cost_rmb").unwrap_or(0.0);
            json!({
                "time": time, "model": model, "count": count, "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens, "total_tokens": prompt_tokens + completion_tokens,
                "cost_rmb": cost_rmb,
            })
        })
        .collect();

    let channel_series_sql = format!(
        "SELECT {bucket_expr} as time, coalesce(nullif(channel_name,''), 'unknown') as channel_name, \
         coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, \
         coalesce(sum(cost_rmb),0) as cost_rmb FROM logs WHERE created_at >= ? AND created_at <= ? AND status = 'success' \
         GROUP BY {bucket_expr}, coalesce(nullif(channel_name,''), 'unknown') ORDER BY time ASC"
    );
    let rows = sqlx::query(&channel_series_sql)
        .bind(&start_db)
        .bind(&end_db)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let channel_series: Vec<Value> = rows
        .iter()
        .map(|r| {
            let time: String = r.try_get("time").unwrap_or_default();
            let channel_name: String = r.try_get("channel_name").unwrap_or_default();
            let prompt_tokens: i64 = r.try_get("prompt_tokens").unwrap_or(0);
            let completion_tokens: i64 = r.try_get("completion_tokens").unwrap_or(0);
            let cost_rmb: f64 = r.try_get("cost_rmb").unwrap_or(0.0);
            json!({
                "time": time, "channel_name": channel_name, "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens, "total_tokens": prompt_tokens + completion_tokens,
                "cost_rmb": cost_rmb,
            })
        })
        .collect();

    util::ok_resp(json!({
        "requests": summary.requests,
        "prompt_tokens": summary.prompt_tokens,
        "completion_tokens": summary.completion_tokens,
        "total_tokens": summary.prompt_tokens + summary.completion_tokens,
        "cost_rmb": summary.cost_rmb,
        "rpm": rpm,
        "tpm": tpm,
        "series": series,
        "distribution": distribution,
        "model_stats": model_stats,
        "model_series": model_series,
        "channel_series": channel_series,
        "start": start.to_rfc3339_opts(SecondsFormat::Secs, true),
        "end": end.to_rfc3339_opts(SecondsFormat::Secs, true),
    }))
}

// ---- Settings ----

pub async fn get_settings(State(st): State<AppState>) -> Response {
    let rows = match sqlx::query("SELECT `key`, `value` FROM settings").fetch_all(&st.pool).await {
        Ok(r) => r,
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };
    let mut map = serde_json::Map::new();
    for row in &rows {
        let k: String = row.try_get("key").unwrap_or_default();
        let v: String = row.try_get("value").unwrap_or_default();
        map.insert(k, Value::String(v));
    }
    util::ok_resp(Value::Object(map))
}

pub async fn update_settings(State(st): State<AppState>, body: Bytes) -> Response {
    let req: HashMap<String, String> = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    let allowed = [
        models::SETTING_LOG_BODY_MAX_BYTES,
        models::SETTING_PRICE_MISSING_POLICY,
        models::SETTING_REQUEST_TIMEOUT,
    ];
    for (k, v) in req.iter() {
        if !allowed.contains(&k.as_str()) {
            continue;
        }
        if let Err(e) = db::set_setting(&st.pool, k, v).await {
            return util::fail_resp(500, &e.to_string());
        }
    }
    util::ok_resp(Value::Null)
}
