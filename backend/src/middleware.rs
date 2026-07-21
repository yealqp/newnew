//! CORS / request-id / admin JWT auth / relay token auth.

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::models::{Token, TOKEN_STATUS_ENABLED};
use crate::state::AppState;
use crate::util::{now_db_string, openai_error, unix_now};

/// Request-scoped values inserted by middleware.
#[derive(Clone)]
pub struct RequestId(pub String);

#[derive(Clone)]
pub struct ClientIp(pub String);

#[derive(Clone)]
pub struct AuthUser {
    pub user_id: i64,
    pub username: String,
}

// ---- CORS ----

pub async fn cors_mw(req: Request, next: Next) -> Response {
    let is_options = req.method() == Method::OPTIONS;
    let mut res = if is_options {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(req).await
    };
    let h = res.headers_mut();
    h.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET,POST,PUT,PATCH,DELETE,OPTIONS"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(
            "Origin,Content-Type,Accept,Authorization,x-api-key,anthropic-version,anthropic-beta",
        ),
    );
    h.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("X-Request-Id"),
    );
    res
}

// ---- Request ID + client IP ----

pub async fn request_id_mw(mut req: Request, next: Next) -> Response {
    let rid = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_default();

    req.extensions_mut().insert(RequestId(rid.clone()));
    req.extensions_mut().insert(ClientIp(ip));

    let mut res = next.run(req).await;
    if let Ok(v) = HeaderValue::from_str(&rid) {
        res.headers_mut().insert("x-request-id", v);
    }
    res
}

// ---- JWT ----

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub user_id: i64,
    pub username: String,
    pub exp: i64,
    pub iat: i64,
}

pub fn generate_jwt(secret: &str, user_id: i64, username: &str) -> Result<String, String> {
    let now = unix_now();
    let claims = JwtClaims {
        user_id,
        username: username.to_string(),
        exp: now + 72 * 3600,
        iat: now,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| e.to_string())
}

fn unauthorized(msg: &str, err: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"success": false, "message": msg, "error": err})),
    )
        .into_response()
}

/// Admin JWT guard for /api/admin (login is mounted outside this layer).
pub async fn admin_auth_mw(
    State(st): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Some(token_str) = auth.strip_prefix("Bearer ") else {
        return unauthorized("未登录或 token 缺失", "unauthorized");
    };
    let mut validation = Validation::default();
    validation.validate_exp = true;
    let data = decode::<JwtClaims>(
        token_str,
        &DecodingKey::from_secret(st.cfg.jwt_secret.as_bytes()),
        &validation,
    );
    match data {
        Ok(d) => {
            req.extensions_mut().insert(AuthUser {
                user_id: d.claims.user_id,
                username: d.claims.username,
            });
            next.run(req).await
        }
        Err(_) => unauthorized("token 无效或已过期，请重新登录", "invalid token"),
    }
}

// ---- Relay token auth ----

fn extract_api_key(req: &Request) -> String {
    if let Some(k) = req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
        if !k.is_empty() {
            return k.to_string();
        }
    }
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if let Some(k) = auth.strip_prefix("Bearer ") {
        return k.to_string();
    }
    if !auth.is_empty() {
        return auth.to_string();
    }
    if let Some(q) = req.uri().query() {
        for pair in q.split('&') {
            if let Some(v) = pair.strip_prefix("key=") {
                if !v.is_empty() {
                    return urlencoding_decode(v);
                }
            }
        }
    }
    String::new()
}

fn urlencoding_decode(s: &str) -> String {
    // minimal percent-decoding for the query fallback
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 3 <= bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn token_error(msg: &str, typ: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(openai_error(msg, typ))).into_response()
}

/// Validates client API keys (sk-xxx) for /v1. No quota checks.
///
/// A valid admin JWT is also accepted: the playground frontend calls the
/// relay's own /v1/chat/completions with the admin session token, so chat
/// goes through the exact same channel-selection / format-conversion path
/// as external clients. Those requests are logged as token "游乐场" (id 0).
pub async fn token_auth_mw(
    State(st): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let key = extract_api_key(&req);
    if key.is_empty() {
        return token_error("missing api key", "invalid_request_error");
    }
    let tok = sqlx::query_as::<_, Token>("SELECT * FROM tokens WHERE `key` = ?")
        .bind(&key)
        .fetch_optional(&st.pool)
        .await
        .ok()
        .flatten();
    let Some(tok) = tok else {
        let jwt_ok = decode::<JwtClaims>(
            &key,
            &DecodingKey::from_secret(st.cfg.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .is_ok();
        if jwt_ok {
            req.extensions_mut().insert(Token {
                id: 0,
                name: Some("游乐场".to_string()),
                key: None,
                status: Some(TOKEN_STATUS_ENABLED),
                model_limits: None,
                expired_at: Some(0),
                created_at: None,
                accessed_at: None,
            });
            return next.run(req).await;
        }
        return token_error("invalid api key", "invalid_api_key");
    };
    if tok.status.unwrap_or(0) != TOKEN_STATUS_ENABLED {
        return token_error("api key disabled", "invalid_api_key");
    }
    if tok.is_expired() {
        return token_error("api key expired", "invalid_api_key");
    }
    let _ = sqlx::query("UPDATE tokens SET accessed_at = ? WHERE id = ?")
        .bind(now_db_string())
        .bind(tok.id)
        .execute(&st.pool)
        .await;
    req.extensions_mut().insert(tok);
    next.run(req).await
}
