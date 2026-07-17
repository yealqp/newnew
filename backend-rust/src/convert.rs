//! OpenAI <-> Claude request/response conversion.
//! Port of Go internal/service/convert/convert.go.

use serde_json::{json, Value};

use crate::dto::{
    ClaudeContentBlock, ClaudeMessage, ClaudeRequest, ClaudeResponse, ClaudeTool, ClaudeUsage,
    OpenAIChatRequest, OpenAIChatResponse, OpenAIChoice, OpenAIFunction, OpenAIMessage,
    OpenAITool, OpenAIToolCall, OpenAIToolCallFunction, OpenAIUsage, PromptTokensDetails,
    StreamOptions,
};

// ---- public API (cross-module contract) ----

/// Converts an OpenAI chat request to a Claude messages request.
pub fn openai_chat_to_claude(req: &OpenAIChatRequest) -> ClaudeRequest {
    let mut out = ClaudeRequest {
        model: req.model.clone(),
        stream: req.stream,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: 4096,
        ..Default::default()
    };
    if let Some(mt) = req.max_tokens {
        if mt > 0 {
            out.max_tokens = mt;
        }
    }

    // stop
    match req.stop.as_ref() {
        Some(Value::String(s)) if !s.is_empty() => out.stop_sequences = vec![s.clone()],
        Some(Value::Array(arr)) => {
            out.stop_sequences = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        _ => {}
    }

    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<ClaudeMessage> = Vec::with_capacity(req.messages.len());
    for m in &req.messages {
        let mut role: &str = m.role.as_str();
        match role {
            "system" => {
                system_parts.push(content_to_string(m.content.as_ref()));
                continue;
            }
            "assistant" | "user" => {}
            "tool" => {
                // tool result as user message with tool_result block
                let block = ClaudeContentBlock {
                    block_type: "tool_result".to_string(),
                    tool_use_id: m.tool_call_id.clone(),
                    content: Some(Value::String(content_to_string(m.content.as_ref()))),
                    ..Default::default()
                };
                messages.push(ClaudeMessage {
                    role: "user".to_string(),
                    content: Some(
                        serde_json::to_value(vec![block]).unwrap_or(Value::Array(vec![])),
                    ),
                });
                continue;
            }
            _ => {
                role = "user";
            }
        }

        // assistant tool_calls
        if role == "assistant" && !m.tool_calls.is_empty() {
            let mut blocks: Vec<ClaudeContentBlock> = Vec::new();
            let text = content_to_string(m.content.as_ref());
            if !text.is_empty() {
                blocks.push(ClaudeContentBlock {
                    block_type: "text".to_string(),
                    text,
                    ..Default::default()
                });
            }
            for tc in &m.tool_calls {
                let parsed = if tc.function.arguments.is_empty() {
                    None
                } else {
                    serde_json::from_str::<Value>(&tc.function.arguments).ok()
                };
                let input = match parsed {
                    Some(Value::Null) | None => Value::Object(serde_json::Map::new()),
                    Some(v) => v,
                };
                blocks.push(ClaudeContentBlock {
                    block_type: "tool_use".to_string(),
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input: Some(input),
                    ..Default::default()
                });
            }
            messages.push(ClaudeMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::to_value(blocks).unwrap_or(Value::Array(vec![]))),
            });
            continue;
        }

        messages.push(ClaudeMessage {
            role: role.to_string(),
            content: Some(openai_content_to_claude(m.content.as_ref())),
        });
    }
    out.messages = messages;
    if !system_parts.is_empty() {
        out.system = Some(Value::String(system_parts.join("\n")));
    }

    // tools
    for t in &req.tools {
        if t.tool_type != "function" {
            continue;
        }
        out.tools.push(ClaudeTool {
            name: t.function.name.clone(),
            description: t.function.description.clone(),
            input_schema: t.function.parameters.clone(),
        });
    }
    if let Some(tc) = req.tool_choice.as_ref() {
        out.tool_choice = Some(map_openai_tool_choice(tc));
    }
    out
}

/// Converts a Claude messages request to an OpenAI chat request.
pub fn claude_to_openai_chat(req: &ClaudeRequest) -> OpenAIChatRequest {
    let mut out = OpenAIChatRequest {
        model: req.model.clone(),
        stream: req.stream,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: Some(req.max_tokens),
        ..Default::default()
    };
    if req.stop_sequences.len() == 1 {
        out.stop = Some(Value::String(req.stop_sequences[0].clone()));
    } else if req.stop_sequences.len() > 1 {
        out.stop = Some(Value::Array(
            req.stop_sequences.iter().cloned().map(Value::String).collect(),
        ));
    }

    let mut messages: Vec<OpenAIMessage> = Vec::with_capacity(req.messages.len() + 1);
    let sys = claude_system_to_string(req.system.as_ref());
    if !sys.is_empty() {
        messages.push(OpenAIMessage {
            role: "system".to_string(),
            content: Some(Value::String(sys)),
            ..Default::default()
        });
    }
    for m in &req.messages {
        messages.extend(claude_message_to_openai(m));
    }
    out.messages = messages;

    for t in &req.tools {
        out.tools.push(OpenAITool {
            tool_type: "function".to_string(),
            function: OpenAIFunction {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
            },
        });
    }
    if let Some(tc) = req.tool_choice.as_ref() {
        out.tool_choice = Some(map_claude_tool_choice(tc));
    }
    if req.stream {
        out.stream_options = Some(StreamOptions { include_usage: true });
    }
    out
}

/// Converts a non-stream Claude response to an OpenAI chat response.
pub fn claude_response_to_openai(resp: &ClaudeResponse, request_model: &str) -> OpenAIChatResponse {
    let (content, tool_calls) = claude_blocks_to_openai(&resp.content);
    let mut msg = OpenAIMessage {
        role: "assistant".to_string(),
        content: content.clone(),
        ..Default::default()
    };
    if !tool_calls.is_empty() {
        msg.tool_calls = tool_calls;
        // claude_blocks_to_openai only ever returns Some(non-empty string) or
        // None, so this mirrors Go's (always-false in practice) empty-string
        // guard rather than being a real branch.
        if matches!(content.as_ref(), Some(Value::String(s)) if s.is_empty()) {
            msg.content = None;
        }
    }
    let fr = map_claude_stop_reason(&resp.stop_reason);
    let model_name = if resp.model.is_empty() {
        request_model.to_string()
    } else {
        resp.model.clone()
    };

    let prompt_tokens =
        resp.usage.input_tokens + resp.usage.cache_read_input_tokens + resp.usage.cache_creation_input_tokens;
    OpenAIChatResponse {
        id: resp.id.clone(),
        object: "chat.completion".to_string(),
        created: crate::util::unix_now(),
        model: model_name,
        choices: vec![OpenAIChoice {
            index: 0,
            message: Some(msg),
            delta: None,
            finish_reason: Some(fr),
        }],
        usage: Some(OpenAIUsage {
            prompt_tokens,
            completion_tokens: resp.usage.output_tokens,
            total_tokens: prompt_tokens + resp.usage.output_tokens,
            prompt_tokens_details: Some(PromptTokensDetails {
                cached_tokens: resp.usage.cache_read_input_tokens,
            }),
        }),
    }
}

/// Converts a non-stream OpenAI chat response to a Claude response.
/// Returns None when there are no choices (mirrors Go's nil result).
pub fn openai_response_to_claude(resp: &OpenAIChatResponse) -> Option<ClaudeResponse> {
    if resp.choices.is_empty() {
        return None;
    }
    let msg = resp.choices[0].message.clone().unwrap_or_default();
    let blocks = openai_message_to_claude_blocks(&msg);
    let stop_reason = match resp.choices[0].finish_reason.as_deref() {
        Some(fr) => map_openai_finish_reason(fr),
        None => "end_turn".to_string(),
    };
    let mut usage = ClaudeUsage::default();
    if let Some(u) = resp.usage.as_ref() {
        let cache_read = u
            .prompt_tokens_details
            .as_ref()
            .map(|d| d.cached_tokens)
            .unwrap_or(0);
        let mut input_tokens = u.prompt_tokens - cache_read;
        if input_tokens < 0 {
            input_tokens = u.prompt_tokens;
        }
        usage.input_tokens = input_tokens;
        usage.cache_read_input_tokens = cache_read;
        usage.output_tokens = u.completion_tokens;
    }
    let id = if resp.id.is_empty() {
        format!("msg_{}", uuid::Uuid::new_v4())
    } else {
        resp.id.clone()
    };
    Some(ClaudeResponse {
        id,
        resp_type: "message".to_string(),
        role: "assistant".to_string(),
        content: blocks,
        model: resp.model.clone(),
        stop_reason,
        stop_sequence: None,
        usage,
    })
}

/// Stringify an OpenAI-style `content` field: string passthrough, joins the
/// `text` parts of a content-part array, "" for null/absent, JSON-encodes
/// anything else.
pub fn content_to_string(content: Option<&Value>) -> String {
    match content {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => {
            let mut b = String::new();
            for part in arr {
                if let Value::Object(m) = part {
                    if m.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(t) = m.get("text").and_then(|v| v.as_str()) {
                            b.push_str(t);
                        }
                    }
                }
            }
            b
        }
        Some(v) => serde_json::to_string(v).unwrap_or_default(),
    }
}

/// Stringify a Claude-style `system` field: string passthrough, joins the
/// `text` parts of a block array with "\n", "" for null/absent.
pub fn claude_system_to_string(sys: Option<&Value>) -> String {
    match sys {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => {
            let mut parts: Vec<String> = Vec::new();
            for p in arr {
                if let Value::Object(m) = p {
                    if m.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(t) = m.get("text").and_then(|v| v.as_str()) {
                            parts.push(t.to_string());
                        }
                    }
                }
            }
            parts.join("\n")
        }
        Some(v) => content_to_string(Some(v)),
    }
}

pub fn map_claude_stop_reason(r: &str) -> String {
    match r {
        "end_turn" | "stop_sequence" => "stop".to_string(),
        "max_tokens" => "length".to_string(),
        "tool_use" => "tool_calls".to_string(),
        "" => "stop".to_string(),
        other => other.to_string(),
    }
}

pub fn map_openai_finish_reason(r: &str) -> String {
    match r {
        "stop" => "end_turn".to_string(),
        "length" => "max_tokens".to_string(),
        "tool_calls" => "tool_use".to_string(),
        _ => "end_turn".to_string(),
    }
}

// ---- private helpers ----

/// Converts an OpenAI-style `content` value (string / content-part array)
/// into the Claude equivalent (string or an array of content blocks).
fn openai_content_to_claude(content: Option<&Value>) -> Value {
    match content {
        None | Some(Value::Null) => Value::String(String::new()),
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(Value::Array(arr)) => {
            let mut blocks: Vec<ClaudeContentBlock> = Vec::with_capacity(arr.len());
            for part in arr {
                let Value::Object(m) = part else { continue };
                let typ = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match typ {
                    "text" => {
                        let t = m.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        blocks.push(ClaudeContentBlock {
                            block_type: "text".to_string(),
                            text: t,
                            ..Default::default()
                        });
                    }
                    "image_url" => {
                        // basic skip or pass as text note; vision full support later
                        blocks.push(ClaudeContentBlock {
                            block_type: "text".to_string(),
                            text: "[image]".to_string(),
                            ..Default::default()
                        });
                    }
                    _ => {}
                }
            }
            if blocks.len() == 1 && blocks[0].block_type == "text" {
                Value::String(blocks[0].text.clone())
            } else {
                serde_json::to_value(&blocks).unwrap_or(Value::Array(vec![]))
            }
        }
        Some(v) => Value::String(content_to_string(Some(v))),
    }
}

/// Converts one Claude message into zero or more OpenAI messages
/// (a `tool_result`-bearing user message can expand into several).
fn claude_message_to_openai(m: &ClaudeMessage) -> Vec<OpenAIMessage> {
    match m.content.as_ref() {
        Some(Value::String(s)) => vec![OpenAIMessage {
            role: m.role.clone(),
            content: Some(Value::String(s.clone())),
            ..Default::default()
        }],
        Some(Value::Array(arr)) => claude_any_blocks_to_openai(&m.role, arr),
        _ => vec![OpenAIMessage {
            role: m.role.clone(),
            content: Some(Value::String(content_to_string(m.content.as_ref()))),
            ..Default::default()
        }],
    }
}

/// Converts a Claude content-block array (as raw JSON values) into OpenAI
/// messages: text + tool_use collapse into one assistant/user message,
/// tool_result blocks on a user message become standalone `tool` messages.
fn claude_any_blocks_to_openai(role: &str, blocks: &[Value]) -> Vec<OpenAIMessage> {
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<OpenAIToolCall> = Vec::new();
    let mut tool_results: Vec<OpenAIMessage> = Vec::new();

    for part in blocks {
        let Value::Object(m) = part else { continue };
        let typ = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match typ {
            "text" => {
                if let Some(t) = m.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(t.to_string());
                }
            }
            "tool_use" => {
                let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let input_val = m.get("input").cloned().unwrap_or(Value::Null);
                let args = serde_json::to_string(&input_val).unwrap_or_else(|_| "null".to_string());
                tool_calls.push(OpenAIToolCall {
                    id,
                    call_type: "function".to_string(),
                    function: OpenAIToolCallFunction { name, arguments: args },
                    index: None,
                });
            }
            "tool_result" => {
                let tid = m
                    .get("tool_use_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let content = content_to_string(m.get("content"));
                tool_results.push(OpenAIMessage {
                    role: "tool".to_string(),
                    tool_call_id: tid,
                    content: Some(Value::String(content)),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }

    let mut out: Vec<OpenAIMessage> = Vec::new();
    if !tool_results.is_empty() && role == "user" {
        out.extend(tool_results);
        return out;
    }
    let mut msg = OpenAIMessage {
        role: role.to_string(),
        ..Default::default()
    };
    if !text_parts.is_empty() {
        msg.content = Some(Value::String(text_parts.join("")));
    }
    if !tool_calls.is_empty() {
        msg.tool_calls = tool_calls;
        if matches!(msg.content.as_ref(), Some(Value::String(s)) if s.is_empty()) {
            msg.content = None;
        }
    }
    if msg.content.is_some() || !msg.tool_calls.is_empty() {
        out.push(msg);
    }
    out
}

/// Flattens typed Claude content blocks into an OpenAI (content, tool_calls)
/// pair. `content` is `Some(text)` only when there is non-empty text.
fn claude_blocks_to_openai(blocks: &[ClaudeContentBlock]) -> (Option<Value>, Vec<OpenAIToolCall>) {
    let mut text = String::new();
    let mut tool_calls: Vec<OpenAIToolCall> = Vec::new();
    for b in blocks {
        match b.block_type.as_str() {
            "text" => text.push_str(&b.text),
            "tool_use" => {
                let input_val = b.input.clone().unwrap_or(Value::Null);
                let args = serde_json::to_string(&input_val).unwrap_or_else(|_| "null".to_string());
                tool_calls.push(OpenAIToolCall {
                    id: b.id.clone(),
                    call_type: "function".to_string(),
                    function: OpenAIToolCallFunction {
                        name: b.name.clone(),
                        arguments: args,
                    },
                    index: None,
                });
            }
            _ => {}
        }
    }
    let content = if !text.is_empty() { Some(Value::String(text)) } else { None };
    (content, tool_calls)
}

/// Converts an OpenAI message (content + tool_calls) into Claude content
/// blocks; always returns at least one block (an empty text block if there
/// is nothing else), matching Go's fallback.
fn openai_message_to_claude_blocks(msg: &OpenAIMessage) -> Vec<ClaudeContentBlock> {
    let mut blocks: Vec<ClaudeContentBlock> = Vec::new();
    let s = content_to_string(msg.content.as_ref());
    if !s.is_empty() {
        blocks.push(ClaudeContentBlock {
            block_type: "text".to_string(),
            text: s,
            ..Default::default()
        });
    }
    for tc in &msg.tool_calls {
        let parsed = serde_json::from_str::<Value>(&tc.function.arguments).ok();
        let input = match parsed {
            Some(Value::Null) | None => Value::Object(serde_json::Map::new()),
            Some(v) => v,
        };
        blocks.push(ClaudeContentBlock {
            block_type: "tool_use".to_string(),
            id: tc.id.clone(),
            name: tc.function.name.clone(),
            input: Some(input),
            ..Default::default()
        });
    }
    if blocks.is_empty() {
        blocks.push(ClaudeContentBlock {
            block_type: "text".to_string(),
            text: String::new(),
            ..Default::default()
        });
    }
    blocks
}

fn map_openai_tool_choice(tc: &Value) -> Value {
    match tc {
        Value::String(v) => match v.as_str() {
            "auto" => json!({"type": "auto"}),
            "none" => json!({"type": "none"}),
            "required" => json!({"type": "any"}),
            _ => tc.clone(),
        },
        Value::Object(m) => {
            if m.get("type").and_then(|v| v.as_str()) == Some("function") {
                if let Some(Value::Object(fn_obj)) = m.get("function") {
                    let name = fn_obj.get("name").cloned().unwrap_or(Value::Null);
                    return json!({"type": "tool", "name": name});
                }
            }
            tc.clone()
        }
        _ => tc.clone(),
    }
}

fn map_claude_tool_choice(tc: &Value) -> Value {
    let Value::Object(m) = tc else { return tc.clone() };
    match m.get("type").and_then(|v| v.as_str()) {
        Some("auto") => Value::String("auto".to_string()),
        Some("none") => Value::String("none".to_string()),
        Some("any") => Value::String("required".to_string()),
        Some("tool") => {
            let name = m.get("name").cloned().unwrap_or(Value::Null);
            json!({"type": "function", "function": {"name": name}})
        }
        _ => tc.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_chat_to_claude_system() {
        let req = OpenAIChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "system".to_string(),
                    content: Some(Value::String("you are helpful".to_string())),
                    ..Default::default()
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: Some(Value::String("hi".to_string())),
                    ..Default::default()
                },
            ],
            max_tokens: Some(100),
            ..Default::default()
        };
        let out = openai_chat_to_claude(&req);
        assert_eq!(out.system, Some(Value::String("you are helpful".to_string())));
        assert_eq!(out.messages.len(), 1, "messages: {:?}", out.messages);
        assert_eq!(out.messages[0].role, "user");
        assert_eq!(out.max_tokens, 100);
    }

    #[test]
    fn test_openai_chat_to_claude_default_max_tokens() {
        let req = OpenAIChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(Value::String("hi".to_string())),
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = openai_chat_to_claude(&req);
        assert_eq!(out.max_tokens, 4096);
    }

    #[test]
    fn test_claude_to_openai_chat() {
        let req = ClaudeRequest {
            model: "claude-3".to_string(),
            max_tokens: 256,
            system: Some(Value::String("sys".to_string())),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: Some(Value::String("hello".to_string())),
            }],
            ..Default::default()
        };
        let out = claude_to_openai_chat(&req);
        assert!(out.messages.len() >= 2, "expected system+user, got {:?}", out.messages);
        assert_eq!(out.messages[0].role, "system");
        assert_eq!(out.messages[0].content, Some(Value::String("sys".to_string())));
    }

    #[test]
    fn test_claude_response_to_openai() {
        let resp = ClaudeResponse {
            id: "msg_1".to_string(),
            role: "assistant".to_string(),
            content: vec![ClaudeContentBlock {
                block_type: "text".to_string(),
                text: "world".to_string(),
                ..Default::default()
            }],
            model: "claude-3".to_string(),
            stop_reason: "end_turn".to_string(),
            usage: ClaudeUsage {
                input_tokens: 10,
                output_tokens: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let oai = claude_response_to_openai(&resp, "claude-3");
        assert_eq!(oai.choices.len(), 1);
        assert_eq!(
            oai.choices[0].message.as_ref().unwrap().content,
            Some(Value::String("world".to_string()))
        );
        let usage = oai.usage.expect("usage");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
    }

    #[test]
    fn test_openai_response_to_claude_no_choices() {
        let resp = OpenAIChatResponse::default();
        assert!(openai_response_to_claude(&resp).is_none());
    }

    #[test]
    fn test_openai_response_to_claude_tool_calls() {
        let resp = OpenAIChatResponse {
            id: "chatcmpl-1".to_string(),
            model: "gpt-4o".to_string(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: Some(OpenAIMessage {
                    role: "assistant".to_string(),
                    tool_calls: vec![OpenAIToolCall {
                        id: "call_1".to_string(),
                        call_type: "function".to_string(),
                        function: OpenAIToolCallFunction {
                            name: "get_weather".to_string(),
                            arguments: "{\"city\":\"sf\"}".to_string(),
                        },
                        index: None,
                    }],
                    ..Default::default()
                }),
                delta: None,
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 100,
                completion_tokens: 20,
                total_tokens: 120,
                prompt_tokens_details: Some(PromptTokensDetails { cached_tokens: 40 }),
            }),
            ..Default::default()
        };
        let claude = openai_response_to_claude(&resp).expect("some");
        assert_eq!(claude.stop_reason, "tool_use");
        assert_eq!(claude.content.len(), 1);
        assert_eq!(claude.content[0].block_type, "tool_use");
        assert_eq!(claude.content[0].name, "get_weather");
        assert_eq!(claude.content[0].input, Some(json!({"city": "sf"})));
        assert_eq!(claude.usage.input_tokens, 60);
        assert_eq!(claude.usage.cache_read_input_tokens, 40);
    }

    #[test]
    fn test_tool_result_round_trip() {
        // OpenAI "tool" message -> Claude user message with tool_result block.
        let req = OpenAIChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![OpenAIMessage {
                role: "tool".to_string(),
                tool_call_id: "call_1".to_string(),
                content: Some(Value::String("sunny".to_string())),
                ..Default::default()
            }],
            ..Default::default()
        };
        let claude = openai_chat_to_claude(&req);
        assert_eq!(claude.messages.len(), 1);
        assert_eq!(claude.messages[0].role, "user");
        let blocks = claude.messages[0].content.as_ref().unwrap().as_array().unwrap();
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[0]["tool_use_id"], "call_1");
        assert_eq!(blocks[0]["content"], "sunny");

        // And back: Claude user message with tool_result -> OpenAI tool message.
        let claude_req = ClaudeRequest {
            model: "claude-3".to_string(),
            max_tokens: 100,
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: Some(json!([{"type": "tool_result", "tool_use_id": "call_1", "content": "sunny"}])),
            }],
            ..Default::default()
        };
        let oai = claude_to_openai_chat(&claude_req);
        assert_eq!(oai.messages.len(), 1);
        assert_eq!(oai.messages[0].role, "tool");
        assert_eq!(oai.messages[0].tool_call_id, "call_1");
        assert_eq!(oai.messages[0].content, Some(Value::String("sunny".to_string())));
    }

    #[test]
    fn test_map_stop_reason_and_finish_reason() {
        assert_eq!(map_claude_stop_reason("end_turn"), "stop");
        assert_eq!(map_claude_stop_reason("stop_sequence"), "stop");
        assert_eq!(map_claude_stop_reason("max_tokens"), "length");
        assert_eq!(map_claude_stop_reason("tool_use"), "tool_calls");
        assert_eq!(map_claude_stop_reason(""), "stop");
        assert_eq!(map_claude_stop_reason("weird"), "weird");

        assert_eq!(map_openai_finish_reason("stop"), "end_turn");
        assert_eq!(map_openai_finish_reason("length"), "max_tokens");
        assert_eq!(map_openai_finish_reason("tool_calls"), "tool_use");
        assert_eq!(map_openai_finish_reason("weird"), "end_turn");
    }

    #[test]
    fn test_tool_choice_mapping() {
        assert_eq!(map_openai_tool_choice(&json!("auto")), json!({"type": "auto"}));
        assert_eq!(map_openai_tool_choice(&json!("none")), json!({"type": "none"}));
        assert_eq!(map_openai_tool_choice(&json!("required")), json!({"type": "any"}));
        assert_eq!(
            map_openai_tool_choice(&json!({"type": "function", "function": {"name": "foo"}})),
            json!({"type": "tool", "name": "foo"})
        );

        assert_eq!(map_claude_tool_choice(&json!({"type": "auto"})), json!("auto"));
        assert_eq!(map_claude_tool_choice(&json!({"type": "none"})), json!("none"));
        assert_eq!(map_claude_tool_choice(&json!({"type": "any"})), json!("required"));
        assert_eq!(
            map_claude_tool_choice(&json!({"type": "tool", "name": "foo"})),
            json!({"type": "function", "function": {"name": "foo"}})
        );
    }

    #[test]
    fn test_content_to_string_variants() {
        assert_eq!(content_to_string(None), "");
        assert_eq!(content_to_string(Some(&Value::Null)), "");
        assert_eq!(content_to_string(Some(&json!("hi"))), "hi");
        assert_eq!(
            content_to_string(Some(&json!([{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]))),
            "ab"
        );
    }

    #[test]
    fn test_claude_system_to_string_variants() {
        assert_eq!(claude_system_to_string(None), "");
        assert_eq!(claude_system_to_string(Some(&json!("sys"))), "sys");
        assert_eq!(
            claude_system_to_string(Some(&json!([{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]))),
            "a\nb"
        );
    }
}
