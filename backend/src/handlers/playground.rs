//! Admin "playground" endpoints: conversation + message persistence.
//!
//! Chat itself is NOT handled here: the playground frontend calls the relay's
//! own OpenAI-compatible `/v1/chat/completions` (authenticated with the admin
//! JWT, see middleware::token_auth_mw), so channel selection and any
//! OpenAI/Claude format conversion happen in one place and every upstream
//! type works. This module only persists conversations and messages.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::models::{Conversation, ConversationMessage};
use crate::state::AppState;
use crate::util;

// ---- wire types ----

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ConvUpsertReq {
    title: String,
    model: String,
}

// ---- Conversation CRUD ----

pub async fn list_conversations(State(st): State<AppState>) -> Response {
    let rows = sqlx::query(
        "SELECT id, title, model, created_at, updated_at, \
         (SELECT COUNT(*) FROM conversation_messages WHERE conversation_id = conversations.id) AS message_count \
         FROM conversations ORDER BY updated_at desc",
    )
    .fetch_all(&st.pool)
    .await;

    let rows = match rows {
        Ok(r) => r,
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };

    let mut list: Vec<Value> = Vec::with_capacity(rows.len());
    for row in &rows {
        let conv = Conversation {
            id: row.try_get::<i64, _>("id").unwrap_or(0),
            title: row.try_get::<Option<String>, _>("title").unwrap_or(None),
            model: row.try_get::<Option<String>, _>("model").unwrap_or(None),
            created_at: row
                .try_get::<Option<String>, _>("created_at")
                .unwrap_or(None),
            updated_at: row
                .try_get::<Option<String>, _>("updated_at")
                .unwrap_or(None),
        };
        let message_count: i64 = row.try_get("message_count").unwrap_or(0);
        let mut obj = conv.to_json();
        if let Value::Object(ref mut map) = obj {
            map.insert("message_count".to_string(), json!(message_count));
        }
        list.push(obj);
    }
    util::ok_resp(Value::Array(list))
}

pub async fn create_conversation(State(st): State<AppState>, body: Bytes) -> Response {
    let req: ConvUpsertReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    let title = if req.title.is_empty() {
        "新对话".to_string()
    } else {
        req.title
    };
    let now = util::now_db_string();

    let res = sqlx::query(
        "INSERT INTO conversations (title, model, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&title)
    .bind(&req.model)
    .bind(&now)
    .bind(&now)
    .execute(&st.pool)
    .await;
    let res = match res {
        Ok(r) => r,
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };

    let conv = Conversation {
        id: res.last_insert_rowid(),
        title: Some(title),
        model: Some(req.model),
        created_at: Some(now.clone()),
        updated_at: Some(now),
    };
    util::ok_resp(conv.to_json())
}

pub async fn update_conversation(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    body: Bytes,
) -> Response {
    let req: ConvUpsertReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    let now = util::now_db_string();
    let res = sqlx::query(
        "UPDATE conversations SET title = ?, model = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&req.title)
    .bind(&req.model)
    .bind(&now)
    .bind(id)
    .execute(&st.pool)
    .await;
    if let Err(e) = res {
        return util::fail_resp(500, &e.to_string());
    }
    util::ok_resp(Value::Null)
}

pub async fn delete_conversation(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    let mut tx = match st.pool.begin().await {
        Ok(t) => t,
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };
    let _ = sqlx::query("DELETE FROM conversation_messages WHERE conversation_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await;
    let _ = sqlx::query("DELETE FROM conversations WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await;
    if let Err(e) = tx.commit().await {
        return util::fail_resp(500, &e.to_string());
    }
    util::ok_resp(Value::Null)
}

// ---- Messages ----

pub async fn list_messages(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    let rows = sqlx::query_as::<_, ConversationMessage>(
        "SELECT * FROM conversation_messages WHERE conversation_id = ? ORDER BY id asc",
    )
    .bind(id)
    .fetch_all(&st.pool)
    .await;
    match rows {
        Ok(list) => {
            let arr: Vec<Value> = list.iter().map(|m| m.to_json()).collect();
            util::ok_resp(Value::Array(arr))
        }
        Err(e) => util::fail_resp(500, &e.to_string()),
    }
}

pub async fn clear_messages(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    let res = sqlx::query("DELETE FROM conversation_messages WHERE conversation_id = ?")
        .bind(id)
        .execute(&st.pool)
        .await;
    if let Err(e) = res {
        return util::fail_resp(500, &e.to_string());
    }
    util::ok_resp(Value::Null)
}

// ---- Message create (playground persists its own chat turns) ----

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CreateMessageReq {
    role: String,
    content: String,
}

pub async fn create_message(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    body: Bytes,
) -> Response {
    let req: CreateMessageReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => return util::fail_resp(400, "invalid body"),
    };
    if req.role != "user" && req.role != "assistant" && req.role != "system" {
        return util::fail_resp(400, "role must be user/assistant/system");
    }

    let conv = sqlx::query_as::<_, Conversation>("SELECT * FROM conversations WHERE id = ?")
        .bind(id)
        .fetch_optional(&st.pool)
        .await;
    let conv = match conv {
        Ok(Some(c)) => c,
        Ok(None) => return util::fail_resp(404, "conversation not found"),
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };

    let now = util::now_db_string();
    let res = sqlx::query(
        "INSERT INTO conversation_messages (conversation_id, role, content, created_at) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&req.role)
    .bind(&req.content)
    .bind(&now)
    .execute(&st.pool)
    .await;
    let res = match res {
        Ok(r) => r,
        Err(e) => return util::fail_resp(500, &e.to_string()),
    };

    // Bump updated_at; auto-title the conversation from its first user message.
    if req.role == "user" && conv.title.as_deref() == Some("新对话") {
        let chars: Vec<char> = req.content.chars().collect();
        let title = if chars.len() > 40 {
            let mut t: String = chars[..40].iter().collect();
            t.push('…');
            t
        } else {
            req.content.clone()
        };
        let _ = sqlx::query("UPDATE conversations SET title = ?, updated_at = ? WHERE id = ?")
            .bind(&title)
            .bind(&now)
            .bind(id)
            .execute(&st.pool)
            .await;
    } else {
        let _ = sqlx::query("UPDATE conversations SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&st.pool)
            .await;
    }

    let msg = ConversationMessage {
        id: res.last_insert_rowid(),
        conversation_id: id,
        role: Some(req.role),
        content: Some(req.content),
        created_at: Some(now),
    };
    util::ok_resp(msg.to_json())
}
