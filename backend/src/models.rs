use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::util::{opt_db_time_to_rfc3339, unix_now};

pub const CHANNEL_TYPE_OPENAI: &str = "openai";
pub const CHANNEL_TYPE_CLAUDE: &str = "claude";
pub const CHANNEL_STATUS_ENABLED: i64 = 1;

pub const TOKEN_STATUS_ENABLED: i64 = 1;

pub const SETTING_LOG_BODY_MAX_BYTES: &str = "log_body_max_bytes";
pub const SETTING_PRICE_MISSING_POLICY: &str = "price_missing_policy"; // allow | reject
pub const SETTING_REQUEST_TIMEOUT: &str = "request_timeout"; // seconds

pub const PRICE_POLICY_ALLOW: &str = "allow";
pub const PRICE_POLICY_REJECT: &str = "reject";

/// Model price, CNY per 1M tokens. Matches Go model.ModelPrice JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelPrice {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    #[allow(dead_code)]
    pub created_at: Option<String>,
    #[allow(dead_code)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Token {
    pub id: i64,
    pub name: Option<String>,
    pub key: Option<String>,
    pub status: Option<i64>,
    pub model_limits: Option<String>,
    pub expired_at: Option<i64>,
    pub created_at: Option<String>,
    pub accessed_at: Option<String>,
}

impl Token {
    pub fn is_expired(&self) -> bool {
        match self.expired_at {
            None | Some(0) => false,
            Some(t) => unix_now() > t,
        }
    }

    pub fn name_str(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }

    /// JSON shape identical to Go model.Token marshaling.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name.as_deref().unwrap_or(""),
            "key": self.key.as_deref().unwrap_or(""),
            "status": self.status.unwrap_or(0),
            "model_limits": self.model_limits.as_deref().unwrap_or(""),
            "expired_at": self.expired_at.unwrap_or(0),
            "created_at": opt_db_time_to_rfc3339(&self.created_at),
            "accessed_at": opt_db_time_to_rfc3339(&self.accessed_at),
        })
    }
}

#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct Channel {
    pub id: i64,
    pub icon: Option<String>,
    pub name: Option<String>,
    #[sqlx(rename = "type")]
    pub channel_type: Option<String>,
    pub base_url: Option<String>,
    pub full_url: Option<bool>,
    pub api_key: Option<String>,
    pub models: Option<String>,
    pub model_mapping: Option<String>,
    pub status: Option<i64>,
    pub weight: Option<i64>,
    pub priority: Option<i64>,
    pub pricing: Option<String>,
    pub remark: Option<String>,
    pub response_time: Option<i64>,
    pub test_time: Option<i64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl Channel {
    pub fn name_str(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }
    pub fn type_str(&self) -> &str {
        self.channel_type.as_deref().unwrap_or("")
    }
    pub fn base_url_str(&self) -> &str {
        self.base_url.as_deref().unwrap_or("")
    }
    pub fn api_key_str(&self) -> &str {
        self.api_key.as_deref().unwrap_or("")
    }
    pub fn is_full_url(&self) -> bool {
        self.full_url.unwrap_or(false)
    }

    /// Comma-separated model list -> trimmed non-empty names.
    pub fn get_models(&self) -> Vec<String> {
        match &self.models {
            None => vec![],
            Some(s) => s
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string())
                .collect(),
        }
    }

    pub fn supports_model(&self, model: &str) -> bool {
        self.get_models().iter().any(|m| m == model)
    }

    /// Newline-separated API keys -> trimmed non-empty keys.
    pub fn get_keys(&self) -> Vec<String> {
        match &self.api_key {
            None => vec![],
            Some(s) => s
                .split('\n')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string())
                .collect(),
        }
    }

    pub fn get_model_mapping(&self) -> HashMap<String, String> {
        self.model_mapping
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    /// Map a client model name to the upstream model name.
    pub fn map_model(&self, client_model: &str) -> String {
        match self.get_model_mapping().get(client_model) {
            Some(up) if !up.is_empty() => up.clone(),
            _ => client_model.to_string(),
        }
    }

    pub fn get_pricing(&self) -> HashMap<String, ModelPrice> {
        self.pricing
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    pub fn get_model_price(&self, model: &str) -> Option<ModelPrice> {
        self.get_pricing().get(model).cloned()
    }
}

#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct RequestLog {
    pub id: i64,
    pub created_at: Option<String>,
    pub request_id: Option<String>,
    pub token_id: Option<i64>,
    pub token_name: Option<String>,
    pub channel_id: Option<i64>,
    pub channel_name: Option<String>,
    pub model: Option<String>,
    pub upstream_model: Option<String>,
    pub is_stream: Option<bool>,
    pub first_token_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cost_rmb: Option<f64>,
    pub status: Option<String>,
    pub error_message: Option<String>,
    pub ip: Option<String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub detail: Option<String>,
}

impl RequestLog {
    /// JSON shape identical to Go model.RequestLog marshaling.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "created_at": opt_db_time_to_rfc3339(&self.created_at),
            "request_id": self.request_id.as_deref().unwrap_or(""),
            "token_id": self.token_id.unwrap_or(0),
            "token_name": self.token_name.as_deref().unwrap_or(""),
            "channel_id": self.channel_id.unwrap_or(0),
            "channel_name": self.channel_name.as_deref().unwrap_or(""),
            "model": self.model.as_deref().unwrap_or(""),
            "upstream_model": self.upstream_model.as_deref().unwrap_or(""),
            "is_stream": self.is_stream.unwrap_or(false),
            "first_token_ms": self.first_token_ms.unwrap_or(0),
            "duration_ms": self.duration_ms.unwrap_or(0),
            "prompt_tokens": self.prompt_tokens.unwrap_or(0),
            "completion_tokens": self.completion_tokens.unwrap_or(0),
            "cache_read_tokens": self.cache_read_tokens.unwrap_or(0),
            "cache_write_tokens": self.cache_write_tokens.unwrap_or(0),
            "total_tokens": self.total_tokens.unwrap_or(0),
            "cost_rmb": self.cost_rmb.unwrap_or(0.0),
            "status": self.status.as_deref().unwrap_or(""),
            "error_message": self.error_message.as_deref().unwrap_or(""),
            "ip": self.ip.as_deref().unwrap_or(""),
            "request_body": self.request_body.as_deref().unwrap_or(""),
            "response_body": self.response_body.as_deref().unwrap_or(""),
            "detail": self.detail.as_deref().unwrap_or(""),
        })
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Conversation {
    pub id: i64,
    pub title: Option<String>,
    pub model: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl Conversation {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title.as_deref().unwrap_or(""),
            "model": self.model.as_deref().unwrap_or(""),
            "created_at": opt_db_time_to_rfc3339(&self.created_at),
            "updated_at": opt_db_time_to_rfc3339(&self.updated_at),
        })
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConversationMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub role: Option<String>,
    pub content: Option<String>,
    pub created_at: Option<String>,
}

impl ConversationMessage {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "conversation_id": self.conversation_id,
            "role": self.role.as_deref().unwrap_or(""),
            "content": self.content.as_deref().unwrap_or(""),
            "created_at": opt_db_time_to_rfc3339(&self.created_at),
        })
    }
}
