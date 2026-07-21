//! Streaming SSE converters: upstream chunks -> client-format SSE bytes.
//! Port of Go internal/service/convert/stream.go.

use std::collections::HashMap;

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::billing::{merge_usage, Usage};
use crate::upstream::{encode_sse_data, encode_sse_event};
use crate::util::json_as_i64;

/// Converts upstream stream events into client-format SSE bytes.
pub trait StreamConverter: Send {
    /// Handles one upstream data payload. Returns bytes to write to the client (may be empty).
    fn on_data(&mut self, event: &str, data: &str) -> Vec<u8>;
    /// Returns any trailing bytes (e.g. `[DONE]`) and the final captured usage.
    fn finish(&mut self) -> (Vec<u8>, Usage);
}

// ---- OpenAIPassthrough ----

/// Same-format OpenAI stream passthrough (also captures usage).
#[derive(Default)]
pub struct OpenAIPassthrough {
    usage: Usage,
    done: bool,
}

impl OpenAIPassthrough {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StreamConverter for OpenAIPassthrough {
    fn on_data(&mut self, _event: &str, data: &str) -> Vec<u8> {
        if data == "[DONE]" {
            self.done = true;
            return encode_sse_data("[DONE]");
        }
        if let Ok(chunk) = serde_json::from_str::<Value>(data) {
            if let Some(u) = extract_usage_from_openai_chunk(&chunk) {
                self.usage = merge_usage(self.usage, u);
            }
        }
        encode_sse_data(data)
    }

    fn finish(&mut self) -> (Vec<u8>, Usage) {
        if !self.done {
            (encode_sse_data("[DONE]"), self.usage)
        } else {
            (Vec::new(), self.usage)
        }
    }
}

// ---- ClaudePassthrough ----

/// Same-format Claude SSE passthrough (also captures usage).
#[derive(Default)]
pub struct ClaudePassthrough {
    usage: Usage,
}

impl ClaudePassthrough {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StreamConverter for ClaudePassthrough {
    fn on_data(&mut self, event: &str, data: &str) -> Vec<u8> {
        if let Some(u) = try_claude_usage(data) {
            self.usage = merge_usage(self.usage, u);
        }
        encode_sse_event(event, data)
    }

    fn finish(&mut self) -> (Vec<u8>, Usage) {
        (Vec::new(), self.usage)
    }
}

// ---- OpenAIToClaudeStream ----

/// Upstream OpenAI chunks -> client Claude SSE events.
pub struct OpenAIToClaudeStream {
    model: String,
    msg_id: String,
    started: bool,
    usage: Usage,
    // index -> tool id. A BTreeMap gives deterministic close-block ordering;
    // Go's map iteration order is randomized, so this is a harmless, strictly
    // more-deterministic deviation.
    tool_index: std::collections::BTreeMap<i64, String>,
    content_started: bool,
}

impl OpenAIToClaudeStream {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            msg_id: format!("msg_{}", Uuid::new_v4()),
            started: false,
            usage: Usage::default(),
            tool_index: std::collections::BTreeMap::new(),
            content_started: false,
        }
    }
}

impl StreamConverter for OpenAIToClaudeStream {
    fn on_data(&mut self, _event: &str, data: &str) -> Vec<u8> {
        if data == "[DONE]" {
            // Already closed on finish_reason usually; nothing trailing here.
            return Vec::new();
        }

        // Always try map-based usage extraction first so trailing OpenCode-GO
        // cost chunks (empty choices + normalizedUsage) are not lost.
        if let Ok(raw) = serde_json::from_str::<Value>(data) {
            if let Some(u) = extract_usage_from_openai_chunk(&raw) {
                self.usage = merge_usage(self.usage, u);
            }
        }

        let chunk: crate::dto::OpenAIChatResponse = match serde_json::from_str(data) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            let start = json!({
                "type": "message_start",
                "message": {
                    "id": self.msg_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": self.model,
                    "stop_reason": Value::Null,
                    "stop_sequence": Value::Null,
                    "usage": {"input_tokens": 0, "output_tokens": 0},
                },
            });
            out.extend(encode_sse_event("message_start", &start.to_string()));
            out.extend(encode_sse_event("ping", r#"{"type":"ping"}"#));
        }

        if chunk.choices.is_empty() {
            return out;
        }
        let choice = &chunk.choices[0];
        let empty_delta = crate::dto::OpenAIMessage::default();
        let delta = choice.delta.as_ref().unwrap_or(&empty_delta);

        let text = crate::convert::content_to_string(delta.content.as_ref());
        if !text.is_empty() {
            if !self.content_started {
                self.content_started = true;
                let block_start = json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "text", "text": ""},
                });
                out.extend(encode_sse_event(
                    "content_block_start",
                    &block_start.to_string(),
                ));
            }
            let d = json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": text},
            });
            out.extend(encode_sse_event("content_block_delta", &d.to_string()));
        }

        // tool calls streaming
        for tc in &delta.tool_calls {
            let idx = tc.index.unwrap_or(0);
            // use index+1 for content block index if text started
            let block_idx = if self.content_started { idx + 1 } else { idx };
            if !tc.id.is_empty() {
                self.tool_index.insert(idx, tc.id.clone());
                let bs = json!({
                    "type": "content_block_start",
                    "index": block_idx,
                    "content_block": {
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.function.name,
                        "input": {},
                    },
                });
                out.extend(encode_sse_event("content_block_start", &bs.to_string()));
            }
            if !tc.function.arguments.is_empty() {
                let d = json!({
                    "type": "content_block_delta",
                    "index": block_idx,
                    "delta": {"type": "input_json_delta", "partial_json": tc.function.arguments},
                });
                out.extend(encode_sse_event("content_block_delta", &d.to_string()));
            }
        }

        if let Some(fr) = choice.finish_reason.as_deref() {
            if !fr.is_empty() {
                // close open blocks then message_delta
                if self.content_started {
                    let stop = json!({"type": "content_block_stop", "index": 0});
                    out.extend(encode_sse_event("content_block_stop", &stop.to_string()));
                }
                for idx in self.tool_index.keys() {
                    let block_idx = if self.content_started { idx + 1 } else { *idx };
                    let stop = json!({"type": "content_block_stop", "index": block_idx});
                    out.extend(encode_sse_event("content_block_stop", &stop.to_string()));
                }
                let sr = crate::convert::map_openai_finish_reason(fr);
                let md = json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": sr, "stop_sequence": Value::Null},
                    "usage": {"output_tokens": self.usage.completion_tokens},
                });
                out.extend(encode_sse_event("message_delta", &md.to_string()));
                let ms = json!({"type": "message_stop"});
                out.extend(encode_sse_event("message_stop", &ms.to_string()));
            }
        }

        out
    }

    fn finish(&mut self) -> (Vec<u8>, Usage) {
        (Vec::new(), self.usage)
    }
}

// ---- ClaudeToOpenAIStream ----

#[derive(Clone, Default)]
struct ToolMeta {
    id: String,
    /// Kept for parity with Go's toolMeta; only the id is re-emitted in deltas.
    #[allow(dead_code)]
    name: String,
}

/// Upstream Claude SSE -> client OpenAI chunks.
pub struct ClaudeToOpenAIStream {
    model: String,
    id: String,
    created: i64,
    usage: Usage,
    tool_meta: HashMap<i64, ToolMeta>,
    done: bool,
}

impl ClaudeToOpenAIStream {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            id: format!("chatcmpl-{}", Uuid::new_v4()),
            created: crate::util::unix_now(),
            usage: Usage::default(),
            tool_meta: HashMap::new(),
            done: false,
        }
    }

    fn base_chunk(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("id".to_string(), json!(self.id));
        m.insert("object".to_string(), json!("chat.completion.chunk"));
        m.insert("created".to_string(), json!(self.created));
        m.insert("model".to_string(), json!(self.model));
        m
    }
}

fn int_from(v: Option<&Value>) -> i64 {
    v.and_then(json_as_i64).unwrap_or(0)
}

impl StreamConverter for ClaudeToOpenAIStream {
    fn on_data(&mut self, event: &str, data: &str) -> Vec<u8> {
        if let Some(u) = try_claude_usage(data) {
            if u.prompt_tokens > 0 {
                self.usage.prompt_tokens = u.prompt_tokens;
            }
            if u.completion_tokens > 0 {
                self.usage.completion_tokens = u.completion_tokens;
            }
            if u.cache_read_tokens > 0 {
                self.usage.cache_read_tokens = u.cache_read_tokens;
            }
            if u.cache_write_tokens > 0 {
                self.usage.cache_write_tokens = u.cache_write_tokens;
            }
        }

        let payload: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let typ = payload
            .get("type")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| event.to_string());

        match typ.as_str() {
            "content_block_start" => {
                let cb = match payload.get("content_block").and_then(|v| v.as_object()) {
                    Some(cb) => cb,
                    None => return Vec::new(),
                };
                if cb.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
                    return Vec::new();
                }
                let idx = int_from(payload.get("index"));
                let id = cb.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = cb
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.tool_meta.insert(
                    idx,
                    ToolMeta {
                        id: id.clone(),
                        name: name.clone(),
                    },
                );
                let mut chunk = self.base_chunk();
                let tc = json!({
                    "index": idx,
                    "id": id,
                    "type": "function",
                    "function": {"name": name, "arguments": ""},
                });
                chunk.insert(
                    "choices".to_string(),
                    json!([{
                        "index": 0,
                        "delta": {"tool_calls": [tc]},
                        "finish_reason": Value::Null,
                    }]),
                );
                encode_sse_data(&Value::Object(chunk).to_string())
            }
            "content_block_delta" => {
                let delta = match payload.get("delta").and_then(|v| v.as_object()) {
                    Some(d) => d,
                    None => return Vec::new(),
                };
                let d_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let mut chunk = self.base_chunk();
                match d_type {
                    "text_delta" => {
                        let text = delta.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        chunk.insert(
                            "choices".to_string(),
                            json!([{
                                "index": 0,
                                "delta": {"content": text},
                                "finish_reason": Value::Null,
                            }]),
                        );
                    }
                    "input_json_delta" => {
                        let partial = delta
                            .get("partial_json")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let idx = int_from(payload.get("index"));
                        let meta = self.tool_meta.get(&idx).cloned().unwrap_or_default();
                        // map tool block index - if only tools, index is fine
                        let tool_idx = self.tool_meta.keys().filter(|&&k| k < idx).count() as i64;
                        let mut tc = json!({
                            "index": tool_idx,
                            "function": {"arguments": partial},
                        });
                        if !meta.id.is_empty() {
                            tc["id"] = json!(meta.id);
                            tc["type"] = json!("function");
                        }
                        chunk.insert(
                            "choices".to_string(),
                            json!([{
                                "index": 0,
                                "delta": {"tool_calls": [tc]},
                                "finish_reason": Value::Null,
                            }]),
                        );
                    }
                    _ => return Vec::new(),
                }
                encode_sse_data(&Value::Object(chunk).to_string())
            }
            "message_delta" => {
                let d = payload.get("delta").and_then(|v| v.as_object());
                let mut fr = "stop".to_string();
                if let Some(d) = d {
                    if let Some(sr) = d.get("stop_reason").and_then(|v| v.as_str()) {
                        fr = crate::convert::map_claude_stop_reason(sr);
                    }
                }
                if let Some(u) = payload.get("usage").and_then(|v| v.as_object()) {
                    if let Some(ot) = u.get("output_tokens").and_then(|v| v.as_f64()) {
                        self.usage.completion_tokens = ot as i64;
                    }
                }
                let mut chunk = self.base_chunk();
                chunk.insert(
                    "choices".to_string(),
                    json!([{
                        "index": 0,
                        "delta": {},
                        "finish_reason": fr,
                    }]),
                );
                encode_sse_data(&Value::Object(chunk).to_string())
            }
            "message_start" => {
                if let Some(msg) = payload.get("message").and_then(|v| v.as_object()) {
                    if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                        if !id.is_empty() {
                            self.id = id.to_string();
                        }
                    }
                    if let Some(u) = msg.get("usage").and_then(|v| v.as_object()) {
                        if let Some(it) = u.get("input_tokens").and_then(|v| v.as_f64()) {
                            self.usage.prompt_tokens = it as i64;
                        }
                    }
                }
                let mut chunk = self.base_chunk();
                chunk.insert(
                    "choices".to_string(),
                    json!([{
                        "index": 0,
                        "delta": {"role": "assistant", "content": ""},
                        "finish_reason": Value::Null,
                    }]),
                );
                encode_sse_data(&Value::Object(chunk).to_string())
            }
            "message_stop" => {
                let mut chunk = self.base_chunk();
                chunk.insert("choices".to_string(), json!([]));
                chunk.insert(
                    "usage".to_string(),
                    json!({
                        "prompt_tokens": self.usage.prompt_tokens,
                        "completion_tokens": self.usage.completion_tokens,
                        "total_tokens": self.usage.prompt_tokens + self.usage.completion_tokens,
                    }),
                );
                let mut out = encode_sse_data(&Value::Object(chunk).to_string());
                out.extend(encode_sse_data("[DONE]"));
                self.done = true;
                out
            }
            _ => Vec::new(),
        }
    }

    fn finish(&mut self) -> (Vec<u8>, Usage) {
        // Go unconditionally emitted a second [DONE] after message_stop had
        // already sent one; only emit here when the upstream never reached
        // message_stop (fixes the duplicate-[DONE] quirk of the Go version).
        if self.done {
            (Vec::new(), self.usage)
        } else {
            (encode_sse_data("[DONE]"), self.usage)
        }
    }
}

// ---- converter selection ----

/// Port of Go relayStream's converter switch.
pub fn make_converter(client_format: &str, channel_type: &str, model: &str) -> Box<dyn StreamConverter> {
    if client_format == "openai" && channel_type == crate::models::CHANNEL_TYPE_OPENAI {
        Box::new(OpenAIPassthrough::new())
    } else if client_format == "claude" && channel_type == crate::models::CHANNEL_TYPE_CLAUDE {
        Box::new(ClaudePassthrough::new())
    } else if client_format == "openai" && channel_type == crate::models::CHANNEL_TYPE_CLAUDE {
        Box::new(ClaudeToOpenAIStream::new(model))
    } else if client_format == "claude" && channel_type == crate::models::CHANNEL_TYPE_OPENAI {
        Box::new(OpenAIToClaudeStream::new(model))
    } else {
        Box::new(OpenAIPassthrough::new())
    }
}

// ---- usage extraction ----

/// extract_usage_from_openai_chunk pulls usage from a stream chunk.
/// Supports:
///  1. Standard OpenAI: "usage": { prompt_tokens, completion_tokens, prompt_tokens_details.cached_tokens }
///  2. DeepSeek-style top-level: prompt_cache_hit_tokens
///  3. OpenCode-GO trailing cost event: "normalizedUsage": { inputTokens, outputTokens, cacheReadTokens, ... }
///     often with empty choices and "x-opencode-type": "inference-cost"
pub fn extract_usage_from_openai_chunk(chunk: &Value) -> Option<Usage> {
    let mut u = Usage::default();
    let mut found = false;

    if let Some(usage) = chunk.get("usage").and_then(|v| v.as_object()) {
        found = true;
        u = parse_openai_usage_map(usage);
    }

    // OpenCode-GO / some gateways put normalized usage on the root of a late chunk
    if let Some(nu) = chunk.get("normalizedUsage").and_then(|v| v.as_object()) {
        found = true;
        if let Some(v) = nu.get("inputTokens").and_then(json_as_i64) {
            u.prompt_tokens = v;
        }
        if let Some(v) = nu.get("outputTokens").and_then(json_as_i64) {
            u.completion_tokens = v;
        }
        // outputTokens often excludes reasoning; OpenCode normalizedUsage's
        // outputTokens is already the total completion count, so
        // reasoningTokens is intentionally not added (mirrors Go comment).
        if let Some(v) = nu.get("cacheReadTokens").and_then(json_as_i64) {
            u.cache_read_tokens = v;
        }
        let mut cache_write = 0i64;
        if let Some(v) = nu.get("cacheWrite5mTokens").and_then(json_as_i64) {
            cache_write += v;
        }
        if let Some(v) = nu.get("cacheWrite1hTokens").and_then(json_as_i64) {
            cache_write += v;
        }
        if let Some(v) = nu.get("cacheWriteTokens").and_then(json_as_i64) {
            cache_write += v;
        }
        if cache_write > 0 {
            u.cache_write_tokens = cache_write;
        }
    }

    if found
        && (u.prompt_tokens > 0
            || u.completion_tokens > 0
            || u.cache_read_tokens > 0
            || u.cache_write_tokens > 0)
    {
        Some(u)
    } else {
        None
    }
}

fn parse_openai_usage_map(u: &Map<String, Value>) -> Usage {
    let mut usage = Usage::default();
    if let Some(v) = u.get("prompt_tokens").and_then(json_as_i64) {
        usage.prompt_tokens = v;
    }
    if let Some(v) = u.get("completion_tokens").and_then(json_as_i64) {
        usage.completion_tokens = v;
    }
    // DeepSeek / OpenCode: top-level cache hit
    if let Some(v) = u.get("prompt_cache_hit_tokens").and_then(json_as_i64) {
        usage.cache_read_tokens = v;
    }
    if let Some(d) = u.get("prompt_tokens_details").and_then(|v| v.as_object()) {
        if let Some(v) = d.get("cached_tokens").and_then(json_as_i64) {
            usage.cache_read_tokens = v;
        }
    }
    // some providers nest cache write under completion/prompt details
    if let Some(d) = u.get("prompt_tokens_details").and_then(|v| v.as_object()) {
        if let Some(v) = d.get("cache_write_tokens").and_then(json_as_i64) {
            usage.cache_write_tokens = v;
        }
    }
    usage
}

/// Reads "usage" and "message.usage" maps from a Claude SSE data payload.
/// Final prompt_tokens = input + cache_read + cache_write (mirrors the Go
/// comment: billing expects prompt_tokens to include the cached portions).
pub fn try_claude_usage(data: &str) -> Option<Usage> {
    let m: Value = serde_json::from_str(data).ok()?;
    let mut u = Usage::default();
    let mut found = false;

    if let Some(usage) = m.get("usage").and_then(|v| v.as_object()) {
        found = true;
        if let Some(v) = usage.get("input_tokens").and_then(json_as_i64) {
            u.prompt_tokens = v;
        }
        if let Some(v) = usage.get("output_tokens").and_then(json_as_i64) {
            u.completion_tokens = v;
        }
        if let Some(v) = usage.get("cache_read_input_tokens").and_then(json_as_i64) {
            u.cache_read_tokens = v;
        }
        if let Some(v) = usage.get("cache_creation_input_tokens").and_then(json_as_i64) {
            u.cache_write_tokens = v;
        }
    }
    if let Some(msg) = m.get("message").and_then(|v| v.as_object()) {
        if let Some(usage) = msg.get("usage").and_then(|v| v.as_object()) {
            found = true;
            if let Some(v) = usage.get("input_tokens").and_then(json_as_i64) {
                u.prompt_tokens = v;
            }
            if let Some(v) = usage.get("output_tokens").and_then(json_as_i64) {
                u.completion_tokens = v;
            }
        }
    }

    if !found {
        return None;
    }
    // prompt may need to add cache tokens into prompt for billing non_cached calc:
    // we store input as non-cache portion from Claude input_tokens field
    // (already non-cache), but our billing expects prompt_tokens as total
    // prompt including cache_read / cache_write.
    u.prompt_tokens = u.prompt_tokens + u.cache_read_tokens + u.cache_write_tokens;
    Some(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_openai_usage_from_chunk() {
        let chunk = json!({
            "id": "d007c232-1f41-4c15-979d-cffd84c7cfc4",
            "object": "chat.completion.chunk",
            "model": "deepseek-v4-flash",
            "choices": [
                {"index": 0, "finish_reason": "stop", "delta": {"content": ""}}
            ],
            "usage": {
                "prompt_tokens": 88,
                "completion_tokens": 545,
                "total_tokens": 633,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 88,
                "prompt_tokens_details": {"cached_tokens": 0},
                "completion_tokens_details": {"reasoning_tokens": 268},
            },
        });
        let u = extract_usage_from_openai_chunk(&chunk).expect("expected usage found");
        assert_eq!(u.prompt_tokens, 88);
        assert_eq!(u.completion_tokens, 545);
    }

    #[test]
    fn test_extract_opencode_normalized_usage() {
        // trailing OpenCode-GO inference-cost chunk with empty choices
        let chunk = json!({
            "choices": [],
            "x-opencode-type": "inference-cost",
            "cost": "0.00016492",
            "normalizedUsage": {
                "inputTokens": 88,
                "outputTokens": 545,
                "reasoningTokens": 268,
                "cacheReadTokens": 12,
                "cacheWrite5mTokens": 3,
                "cacheWrite1hTokens": 1,
            },
        });
        let u = extract_usage_from_openai_chunk(&chunk).expect("expected normalized usage found");
        assert_eq!(u.prompt_tokens, 88);
        assert_eq!(u.completion_tokens, 545);
        assert_eq!(u.cache_read_tokens, 12);
        assert_eq!(u.cache_write_tokens, 4); // 3+1
    }

    #[test]
    fn test_openai_passthrough_merges_late_usage() {
        let mut p = OpenAIPassthrough::new();
        // content chunk without usage
        let _ = p.on_data(
            "",
            r#"{"id":"1","object":"chat.completion.chunk","choices":[{"delta":{"content":"hi"}}]}"#,
        );
        // late usage chunk
        let _ = p.on_data(
            "",
            r#"{
                "id":"1","object":"chat.completion.chunk",
                "choices":[{"index":0,"finish_reason":"stop","delta":{"content":""}}],
                "usage":{"prompt_tokens":88,"completion_tokens":545,"prompt_tokens_details":{"cached_tokens":0}}
            }"#,
        );
        // OpenCode cost trailer
        let _ = p.on_data(
            "",
            r#"{
                "choices":[],
                "x-opencode-type":"inference-cost",
                "normalizedUsage":{"inputTokens":88,"outputTokens":545,"cacheReadTokens":0,"cacheWrite5mTokens":0,"cacheWrite1hTokens":0}
            }"#,
        );
        let (_, u) = p.finish();
        assert_eq!(u.prompt_tokens, 88);
        assert_eq!(u.completion_tokens, 545);
    }
}
