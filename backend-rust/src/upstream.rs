//! Upstream HTTP helpers: URL resolution, request builders, SSE parsing,
//! error prettifying. Port of Go internal/relay/{openai,claude}.

use std::time::Duration;

use reqwest::Response;
use serde_json::Value;

/// If full_url, base_url is the complete endpoint; otherwise append default_path.
pub fn resolve_url(base_url: &str, full_url: bool, default_path: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if full_url {
        base.to_string()
    } else {
        format!("{base}{default_path}")
    }
}

/// POST to an OpenAI-compatible chat endpoint.
pub async fn post_chat_completions(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: Vec<u8>,
    stream: bool,
    full_url: bool,
    timeout_secs: u64,
) -> Result<Response, reqwest::Error> {
    let url = resolve_url(base_url, full_url, "/v1/chat/completions");
    let mut req = http
        .post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .timeout(Duration::from_secs(if timeout_secs == 0 { 300 } else { timeout_secs }))
        .body(body);
    if stream {
        req = req.header("Accept", "text/event-stream");
    }
    req.send().await
}

/// POST to a Claude-compatible messages endpoint.
pub async fn post_messages(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: Vec<u8>,
    stream: bool,
    full_url: bool,
    timeout_secs: u64,
) -> Result<Response, reqwest::Error> {
    let url = resolve_url(base_url, full_url, "/v1/messages");
    let mut req = http
        .post(url)
        .header("Content-Type", "application/json")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(Duration::from_secs(if timeout_secs == 0 { 300 } else { timeout_secs }))
        .body(body);
    if stream {
        req = req.header("Accept", "text/event-stream");
    }
    req.send().await
}

/// Extract a human-readable message from an upstream error body.
/// Handles {"error":{"message":...}} and {"error":"..."}; falls back to
/// the raw body truncated to 500 bytes.
pub fn pretty_upstream_error(body: &[u8]) -> String {
    if let Ok(v) = serde_json::from_slice::<Value>(body) {
        if let Some(e) = v.get("error") {
            if let Some(msg) = e.get("message").and_then(|m| m.as_str()) {
                return msg.to_string();
            }
            if let Some(s) = e.as_str() {
                return s.to_string();
            }
        }
    }
    let s = String::from_utf8_lossy(body);
    crate::util::truncate_str_plain(&s, 500)
}

/// Incremental SSE parser. Feed raw bytes, get complete (event, data) pairs.
/// Blank lines reset the current event name (Claude-style); OpenAI-style
/// consumers simply ignore the event component.
///
/// A single line is capped at 10MB (Go used the same bufio.Scanner limit).
/// Unlike Go — which aborted the whole stream — an oversized line is dropped
/// and parsing resumes at the next newline.
#[derive(Default)]
pub struct SseParser {
    pending: Vec<u8>,
    cur_event: String,
    overflowed: bool,
}

const MAX_SSE_LINE: usize = 10 * 1024 * 1024;

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<(String, String)> {
        let mut chunk = chunk;
        if self.overflowed {
            // Discard until the oversized line ends.
            match chunk.iter().position(|&b| b == b'\n') {
                Some(pos) => {
                    chunk = &chunk[pos + 1..];
                    self.overflowed = false;
                }
                None => return Vec::new(),
            }
        }
        self.pending.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = self.pending.drain(..=pos).collect();
            let mut line = String::from_utf8_lossy(&line_bytes).into_owned();
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            if line.is_empty() {
                self.cur_event.clear();
                continue;
            }
            if let Some(ev) = line.strip_prefix("event:") {
                self.cur_event = ev.trim().to_string();
                continue;
            }
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if !data.is_empty() {
                    out.push((self.cur_event.clone(), data.to_string()));
                }
            }
        }
        if self.pending.len() > MAX_SSE_LINE {
            self.pending.clear();
            self.overflowed = true;
        }
        out
    }
}

/// Encode an SSE frame in OpenAI style (data only).
pub fn encode_sse_data(data: &str) -> Vec<u8> {
    format!("data: {data}\n\n").into_bytes()
}

/// Encode an SSE frame in Claude style (event + data).
pub fn encode_sse_event(event: &str, data: &str) -> Vec<u8> {
    if event.is_empty() {
        encode_sse_data(data)
    } else {
        format!("event: {event}\ndata: {data}\n\n").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_url() {
        assert_eq!(
            resolve_url("https://api.openai.com", false, "/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            resolve_url("https://x.com/custom/endpoint/", true, "/v1/chat/completions"),
            "https://x.com/custom/endpoint"
        );
    }

    #[test]
    fn test_sse_parser_openai_style() {
        let mut p = SseParser::new();
        let events = p.feed(b"data: {\"a\":1}\n\ndata: [DONE]\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], (String::new(), "{\"a\":1}".to_string()));
        assert_eq!(events[1].1, "[DONE]");
    }

    #[test]
    fn test_sse_parser_claude_style_and_split_chunks() {
        let mut p = SseParser::new();
        let mut events = p.feed(b"event: message_start\ndata: {\"type\":\"messa");
        assert!(events.is_empty());
        events = p.feed(b"ge_start\"}\n\nevent: ping\ndata: {}\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "message_start");
        assert_eq!(events[0].1, "{\"type\":\"message_start\"}");
        assert_eq!(events[1].0, "ping");
    }

    #[test]
    fn test_sse_parser_oversized_line_dropped_stream_survives() {
        let mut p = SseParser::new();
        // one giant newline-less blob exceeding the cap
        let events = p.feed(&vec![b'x'; MAX_SSE_LINE + 1]);
        assert!(events.is_empty());
        // remainder of the oversized line, then a normal frame
        let events = p.feed(b"yyy\ndata: {\"ok\":1}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, "{\"ok\":1}");
    }

    #[test]
    fn test_pretty_upstream_error() {
        assert_eq!(
            pretty_upstream_error(br#"{"error":{"message":"bad key"}}"#),
            "bad key"
        );
        assert_eq!(pretty_upstream_error(br#"{"error":"oops"}"#), "oops");
        assert_eq!(pretty_upstream_error(b"plain text"), "plain text");
    }
}
