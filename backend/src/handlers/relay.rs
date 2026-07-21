//! `/v1/*` relay handlers: OpenAI-compatible `/chat/completions`, Claude-compatible
//! `/messages`, and `/models`. Port of Go internal/handler/relay/relay.go.

use std::time::Instant;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use futures_util::StreamExt;
use serde_json::{json, Map, Value};

use crate::billing::{self, BillingResult, Usage};
use crate::channel_select;
use crate::convert;
use crate::dto::{self, ModelItem, ModelsListResponse};
use crate::logsvc::{self, WriteInput};
use crate::middleware::{ClientIp, RequestId};
use crate::models::{
    Channel, ModelPrice, Token, CHANNEL_TYPE_CLAUDE, CHANNEL_TYPE_OPENAI, PRICE_POLICY_REJECT,
    SETTING_PRICE_MISSING_POLICY,
};
use crate::state::AppState;
use crate::stream;
use crate::upstream;
use crate::util::err_json;

pub const FORMAT_OPENAI: &str = "openai";
pub const FORMAT_CLAUDE: &str = "claude";

// ---- routes ----

pub async fn list_models(State(st): State<AppState>) -> Response {
    let models = channel_select::list_enabled_models(&st.pool).await;
    let now = crate::util::unix_now();
    let data: Vec<ModelItem> = models
        .into_iter()
        .map(|m| ModelItem {
            id: m,
            object: "model".to_string(),
            created: now,
            owned_by: "gateway".to_string(),
        })
        .collect();
    Json(ModelsListResponse {
        object: "list".to_string(),
        data,
    })
    .into_response()
}

pub async fn chat_completions(
    State(st): State<AppState>,
    Extension(tok): Extension<Token>,
    Extension(rid): Extension<RequestId>,
    Extension(ip): Extension<ClientIp>,
    body: Bytes,
) -> Response {
    relay_common(st, tok, rid, ip, body, FORMAT_OPENAI).await
}

pub async fn messages(
    State(st): State<AppState>,
    Extension(tok): Extension<Token>,
    Extension(rid): Extension<RequestId>,
    Extension(ip): Extension<ClientIp>,
    body: Bytes,
) -> Response {
    relay_common(st, tok, rid, ip, body, FORMAT_CLAUDE).await
}

// ---- shared relay logic ----

fn json_resp(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

async fn relay_common(
    st: AppState,
    tok: Token,
    rid: RequestId,
    ip: ClientIp,
    body: Bytes,
    client_format: &str,
) -> Response {
    let start = Instant::now();
    let request_id = rid.0;
    let client_ip = ip.0;
    let raw_body = body.to_vec();
    let body_str = String::from_utf8_lossy(&raw_body).into_owned();

    let (model_name, stream) = match peek_model_stream(&raw_body) {
        Ok(v) => v,
        Err(msg) => {
            return json_resp(StatusCode::BAD_REQUEST, err_json(client_format, &msg));
        }
    };

    if let Some(limits_str) = tok.model_limits.as_deref() {
        if !limits_str.is_empty() && limits_str != "[]" {
            if let Ok(limits) = serde_json::from_str::<Vec<String>>(limits_str) {
                if !limits.is_empty() && !limits.iter().any(|m| m == &model_name) {
                    return json_resp(
                        StatusCode::FORBIDDEN,
                        err_json(client_format, "model not allowed for this token"),
                    );
                }
            }
        }
    }

    let ch = match channel_select::select(&st.pool, &model_name).await {
        Ok(c) => c,
        Err(e) => {
            return json_resp(StatusCode::SERVICE_UNAVAILABLE, err_json(client_format, &e));
        }
    };

    let upstream_model = ch.map_model(&model_name);

    let (price, price_found) = match ch.get_model_price(&model_name) {
        Some(p) => (p, true),
        None => match ch.get_model_price(&upstream_model) {
            Some(p) => (p, true),
            None => (ModelPrice::default(), false),
        },
    };
    if !price_found {
        let policy = crate::db::get_setting(&st.pool, SETTING_PRICE_MISSING_POLICY).await;
        if policy == PRICE_POLICY_REJECT {
            return json_resp(
                StatusCode::BAD_REQUEST,
                err_json(client_format, "model price not configured on channel"),
            );
        }
    }

    let upstream_body =
        match prepare_upstream_body(&raw_body, client_format, ch.type_str(), &upstream_model) {
            Ok(b) => b,
            Err(e) => {
                return json_resp(
                    StatusCode::BAD_REQUEST,
                    err_json(client_format, &format!("convert request: {e}")),
                );
            }
        };

    let api_key = channel_select::pick_key(&ch);

    if stream {
        relay_stream(
            st,
            client_format,
            ch,
            tok,
            model_name,
            upstream_model,
            upstream_body,
            api_key,
            body_str,
            start,
            request_id,
            price,
            price_found,
            client_ip,
        )
        .await
    } else {
        relay_non_stream(
            st,
            client_format,
            ch,
            tok,
            model_name,
            upstream_model,
            upstream_body,
            api_key,
            body_str,
            start,
            request_id,
            price,
            price_found,
            client_ip,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn relay_non_stream(
    st: AppState,
    client_format: &str,
    ch: Channel,
    tok: Token,
    model_name: String,
    upstream_model: String,
    upstream_body: Vec<u8>,
    api_key: String,
    body_str: String,
    start: Instant,
    request_id: String,
    price: ModelPrice,
    price_found: bool,
    client_ip: String,
) -> Response {
    let timeout_secs = st.request_timeout_secs().await;

    let send_result = if ch.type_str() == CHANNEL_TYPE_OPENAI {
        upstream::post_chat_completions(
            &st.http,
            ch.base_url_str(),
            &api_key,
            upstream_body,
            false,
            ch.is_full_url(),
            timeout_secs,
        )
        .await
    } else if ch.type_str() == CHANNEL_TYPE_CLAUDE {
        upstream::post_messages(
            &st.http,
            ch.base_url_str(),
            &api_key,
            upstream_body,
            false,
            ch.is_full_url(),
            timeout_secs,
        )
        .await
    } else {
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            err_json(client_format, "unknown channel type"),
        );
    };

    let resp = match send_result {
        Ok(r) => r,
        Err(e) => {
            let err_msg = e.to_string();
            logsvc::write(
                &st.pool,
                WriteInput {
                    request_id,
                    token: Some(&tok),
                    channel: Some(&ch),
                    model: model_name,
                    upstream_model,
                    is_stream: false,
                    duration_ms: start.elapsed().as_millis() as i64,
                    first_token_ms: 0,
                    usage: Usage::default(),
                    cost: BillingResult {
                        price_missing: !price_found,
                        price,
                        ..Default::default()
                    },
                    status: "error".to_string(),
                    error_message: err_msg.clone(),
                    ip: client_ip,
                    request_body: body_str,
                    response_body: String::new(),
                    detail: None,
                },
            )
            .await;
            return json_resp(StatusCode::BAD_GATEWAY, err_json(client_format, &err_msg));
        }
    };

    let status_code = resp.status();
    // Mirrors Go's `raw, _ := io.ReadAll(resp.Body)`: a body-read error is
    // treated as an empty body rather than a hard failure.
    let raw = resp.bytes().await.unwrap_or_default();
    let resp_body_str = String::from_utf8_lossy(&raw).into_owned();

    if status_code.as_u16() >= 400 {
        let err_msg = upstream::pretty_upstream_error(&raw);
        logsvc::write(
            &st.pool,
            WriteInput {
                request_id,
                token: Some(&tok),
                channel: Some(&ch),
                model: model_name,
                upstream_model,
                is_stream: false,
                duration_ms: start.elapsed().as_millis() as i64,
                first_token_ms: 0,
                usage: Usage::default(),
                cost: BillingResult {
                    price_missing: !price_found,
                    price,
                    ..Default::default()
                },
                status: "error".to_string(),
                error_message: err_msg,
                ip: client_ip,
                request_body: body_str,
                response_body: resp_body_str,
                detail: None,
            },
        )
        .await;
        return Response::builder()
            .status(status_code.as_u16())
            .header("Content-Type", "application/json")
            .body(Body::from(raw))
            .unwrap();
    }

    let usage = if ch.type_str() == CHANNEL_TYPE_OPENAI {
        extract_openai_usage(&raw)
    } else if ch.type_str() == CHANNEL_TYPE_CLAUDE {
        extract_claude_usage(&raw)
    } else {
        Usage::default()
    };

    let client_resp = match convert_non_stream_response(&raw, ch.type_str(), client_format, &model_name)
    {
        Ok(b) => b,
        Err(e) => {
            let cost = billing::calculate(&price, price_found, &usage);
            logsvc::write(
                &st.pool,
                WriteInput {
                    request_id,
                    token: Some(&tok),
                    channel: Some(&ch),
                    model: model_name,
                    upstream_model,
                    is_stream: false,
                    duration_ms: start.elapsed().as_millis() as i64,
                    first_token_ms: 0,
                    usage,
                    cost,
                    status: "error".to_string(),
                    error_message: e.clone(),
                    ip: client_ip,
                    request_body: body_str,
                    response_body: resp_body_str,
                    detail: None,
                },
            )
            .await;
            return json_resp(StatusCode::INTERNAL_SERVER_ERROR, err_json(client_format, &e));
        }
    };

    let cost = billing::calculate(&price, price_found, &usage);
    let client_resp_str = String::from_utf8_lossy(&client_resp).into_owned();
    logsvc::write(
        &st.pool,
        WriteInput {
            request_id,
            token: Some(&tok),
            channel: Some(&ch),
            model: model_name,
            upstream_model,
            is_stream: false,
            duration_ms: start.elapsed().as_millis() as i64,
            first_token_ms: 0,
            usage,
            cost,
            status: "success".to_string(),
            error_message: String::new(),
            ip: client_ip,
            request_body: body_str,
            response_body: client_resp_str,
            detail: None,
        },
    )
    .await;

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(client_resp))
        .unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn relay_stream(
    st: AppState,
    client_format: &str,
    ch: Channel,
    tok: Token,
    model_name: String,
    upstream_model: String,
    upstream_body: Vec<u8>,
    api_key: String,
    body_str: String,
    start: Instant,
    request_id: String,
    price: ModelPrice,
    price_found: bool,
    client_ip: String,
) -> Response {
    let timeout_secs = st.request_timeout_secs().await;

    let send_result = if ch.type_str() == CHANNEL_TYPE_OPENAI {
        upstream::post_chat_completions(
            &st.http,
            ch.base_url_str(),
            &api_key,
            upstream_body,
            true,
            ch.is_full_url(),
            timeout_secs,
        )
        .await
    } else if ch.type_str() == CHANNEL_TYPE_CLAUDE {
        upstream::post_messages(
            &st.http,
            ch.base_url_str(),
            &api_key,
            upstream_body,
            true,
            ch.is_full_url(),
            timeout_secs,
        )
        .await
    } else {
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            err_json(client_format, "unknown channel type"),
        );
    };

    // NOTE: unlike Go (which opens a 200 SSE body stream via fasthttp's
    // SetBodyStreamWriter before it knows whether upstream will fail, and
    // therefore writes any connection/upstream error *into* that already-200
    // stream), we send the upstream request first and inspect the outcome
    // before committing to a response. axum has no equivalent of "start a
    // 200 response, then change your mind", so a failed connection or a
    // >=400 upstream status is surfaced here as a real 502/upstream status
    // JSON response instead of a 200 SSE body containing an error payload.
    let resp = match send_result {
        Ok(r) => r,
        Err(e) => {
            let err_msg = e.to_string();
            logsvc::write(
                &st.pool,
                WriteInput {
                    request_id,
                    token: Some(&tok),
                    channel: Some(&ch),
                    model: model_name,
                    upstream_model,
                    is_stream: true,
                    duration_ms: start.elapsed().as_millis() as i64,
                    first_token_ms: 0,
                    usage: Usage::default(),
                    cost: BillingResult {
                        price_missing: !price_found,
                        price,
                        ..Default::default()
                    },
                    status: "error".to_string(),
                    error_message: err_msg.clone(),
                    ip: client_ip,
                    request_body: body_str,
                    response_body: String::new(),
                    detail: None,
                },
            )
            .await;
            return json_resp(StatusCode::BAD_GATEWAY, err_json(client_format, &err_msg));
        }
    };

    let status_code = resp.status();
    if status_code.as_u16() >= 400 {
        let raw = resp.bytes().await.unwrap_or_default();
        let err_msg = upstream::pretty_upstream_error(&raw);
        let resp_body_str = String::from_utf8_lossy(&raw).into_owned();
        logsvc::write(
            &st.pool,
            WriteInput {
                request_id,
                token: Some(&tok),
                channel: Some(&ch),
                model: model_name,
                upstream_model,
                is_stream: true,
                duration_ms: start.elapsed().as_millis() as i64,
                first_token_ms: 0,
                usage: Usage::default(),
                cost: BillingResult {
                    price_missing: !price_found,
                    price,
                    ..Default::default()
                },
                status: "error".to_string(),
                error_message: err_msg,
                ip: client_ip,
                request_body: body_str,
                response_body: resp_body_str,
                detail: None,
            },
        )
        .await;
        return Response::builder()
            .status(status_code.as_u16())
            .header("Content-Type", "application/json")
            .body(Body::from(raw))
            .unwrap();
    }

    let mut converter = stream::make_converter(client_format, ch.type_str(), &model_name);

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::convert::Infallible>>(64);

    tokio::spawn(async move {
        let mut parser = upstream::SseParser::new();
        let mut byte_stream = resp.bytes_stream();
        let mut response_body_bytes: Vec<u8> = Vec::new();
        let mut usage = Usage::default();
        let mut first_token_ms: i64 = 0;
        let mut first_sent = false;

        'outer: while let Some(chunk_result) = byte_stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(_) => break 'outer,
            };
            let events = parser.feed(&chunk);
            for (event, data) in events {
                let out = converter.on_data(&event, &data);
                if out.is_empty() {
                    continue;
                }
                if !first_sent {
                    first_token_ms = start.elapsed().as_millis() as i64;
                    first_sent = true;
                }
                response_body_bytes.extend_from_slice(&out);
                if tx.send(Ok(bytes::Bytes::from(out))).await.is_err() {
                    // Client disconnected; stop reading upstream but still
                    // finalize below so the log write happens exactly once.
                    break 'outer;
                }
            }
        }

        let (trailing, final_usage) = converter.finish();
        if !trailing.is_empty() {
            response_body_bytes.extend_from_slice(&trailing);
            let _ = tx.send(Ok(bytes::Bytes::from(trailing))).await;
        }
        if final_usage.prompt_tokens > 0 || final_usage.completion_tokens > 0 {
            usage = final_usage;
        }

        let cost = billing::calculate(&price, price_found, &usage);
        let response_body = String::from_utf8_lossy(&response_body_bytes).into_owned();
        logsvc::write(
            &st.pool,
            WriteInput {
                request_id,
                token: Some(&tok),
                channel: Some(&ch),
                model: model_name,
                upstream_model,
                is_stream: true,
                duration_ms: start.elapsed().as_millis() as i64,
                first_token_ms,
                usage,
                cost,
                status: "success".to_string(),
                error_message: String::new(),
                ip: client_ip,
                request_body: body_str,
                response_body,
                detail: None,
            },
        )
        .await;
    });

    Response::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx)))
        .unwrap()
}

// ---- helpers ----

/// Port of Go peekModelStream: the whole body must parse as a JSON object;
/// a missing/empty "model" field is a 400 "model is required", any other
/// parse failure surfaces its message verbatim (matches Go's err.Error()).
fn peek_model_stream(body: &[u8]) -> Result<(String, bool), String> {
    let m: Map<String, Value> = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    let model_name = m
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if model_name.is_empty() {
        return Err("model is required".to_string());
    }
    let stream = m.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok((model_name, stream))
}

/// Port of Go prepareUpstreamBody.
fn prepare_upstream_body(
    raw: &[u8],
    client_format: &str,
    channel_type: &str,
    upstream_model: &str,
) -> Result<Vec<u8>, String> {
    let same = (client_format == FORMAT_OPENAI && channel_type == CHANNEL_TYPE_OPENAI)
        || (client_format == FORMAT_CLAUDE && channel_type == CHANNEL_TYPE_CLAUDE);
    if same {
        let mut m: Map<String, Value> = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
        m.insert("model".to_string(), Value::String(upstream_model.to_string()));
        if channel_type == CHANNEL_TYPE_OPENAI {
            let is_stream = m.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
            if is_stream {
                m.insert(
                    "stream_options".to_string(),
                    json!({"include_usage": true}),
                );
            }
        }
        return serde_json::to_vec(&Value::Object(m)).map_err(|e| e.to_string());
    }

    if client_format == FORMAT_OPENAI && channel_type == CHANNEL_TYPE_CLAUDE {
        let mut req: dto::OpenAIChatRequest =
            serde_json::from_slice(raw).map_err(|e| e.to_string())?;
        req.model = upstream_model.to_string();
        let claude_req = convert::openai_chat_to_claude(&req);
        return serde_json::to_vec(&claude_req).map_err(|e| e.to_string());
    }

    if client_format == FORMAT_CLAUDE && channel_type == CHANNEL_TYPE_OPENAI {
        let mut req: dto::ClaudeRequest = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
        req.model = upstream_model.to_string();
        let oai = convert::claude_to_openai_chat(&req);
        return serde_json::to_vec(&oai).map_err(|e| e.to_string());
    }

    Ok(raw.to_vec())
}

/// Port of Go convertNonStreamResponse.
fn convert_non_stream_response(
    raw: &[u8],
    channel_type: &str,
    client_format: &str,
    request_model: &str,
) -> Result<Vec<u8>, String> {
    let same = (client_format == FORMAT_OPENAI && channel_type == CHANNEL_TYPE_OPENAI)
        || (client_format == FORMAT_CLAUDE && channel_type == CHANNEL_TYPE_CLAUDE);
    if same {
        if let Ok(mut m) = serde_json::from_slice::<Map<String, Value>>(raw) {
            m.insert("model".to_string(), Value::String(request_model.to_string()));
            return serde_json::to_vec(&Value::Object(m)).map_err(|e| e.to_string());
        }
        return Ok(raw.to_vec());
    }

    if client_format == FORMAT_OPENAI && channel_type == CHANNEL_TYPE_CLAUDE {
        let resp: dto::ClaudeResponse = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
        let oai = convert::claude_response_to_openai(&resp, request_model);
        return serde_json::to_vec(&oai).map_err(|e| e.to_string());
    }

    if client_format == FORMAT_CLAUDE && channel_type == CHANNEL_TYPE_OPENAI {
        let resp: dto::OpenAIChatResponse =
            serde_json::from_slice(raw).map_err(|e| e.to_string())?;
        return match convert::openai_response_to_claude(&resp) {
            Some(cl) => serde_json::to_vec(&cl).map_err(|e| e.to_string()),
            None => Err("convert response: empty choices".to_string()),
        };
    }

    Ok(raw.to_vec())
}

/// Port of Go extractOpenAIUsage.
fn extract_openai_usage(raw: &[u8]) -> Usage {
    let resp: Result<dto::OpenAIChatResponse, _> = serde_json::from_slice(raw);
    match resp {
        Ok(r) => match r.usage {
            Some(u) => Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                cache_read_tokens: u
                    .prompt_tokens_details
                    .map(|d| d.cached_tokens)
                    .unwrap_or(0),
                cache_write_tokens: 0,
            },
            None => Usage::default(),
        },
        Err(_) => Usage::default(),
    }
}

/// Port of Go extractClaudeUsage.
fn extract_claude_usage(raw: &[u8]) -> Usage {
    match serde_json::from_slice::<dto::ClaudeResponse>(raw) {
        Ok(r) => Usage {
            prompt_tokens: r.usage.input_tokens
                + r.usage.cache_read_input_tokens
                + r.usage.cache_creation_input_tokens,
            completion_tokens: r.usage.output_tokens,
            cache_read_tokens: r.usage.cache_read_input_tokens,
            cache_write_tokens: r.usage.cache_creation_input_tokens,
        },
        Err(_) => Usage::default(),
    }
}
