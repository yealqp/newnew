//! OpenAI / Claude wire formats (subset), mirroring Go internal/dto.
//! Unknown fields are ignored on input; output field presence matches the
//! Go structs' omitempty behavior.

use serde::{Deserialize, Serialize};
use serde_json::Value;

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(n: &i64) -> bool {
    *n == 0
}
#[allow(clippy::ptr_arg)] // serde skip_serializing_if passes &String
fn is_empty_str(s: &String) -> bool {
    s.is_empty()
}

// ---- OpenAI ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAIChatRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "is_false")]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<OpenAITool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub user: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamOptions {
    #[serde(skip_serializing_if = "is_false")]
    pub include_usage: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAIMessage {
    pub role: String,
    /// string or []content parts; always serialized (null when absent), like Go.
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<OpenAIToolCall>,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub tool_call_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunction,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAIFunction {
    pub name: String,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: OpenAIToolCallFunction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAIToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAIChatResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<OpenAIChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAIUsage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAIChoice {
    pub index: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<OpenAIMessage>,
    /// Always serialized (null when absent), like Go.
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAIUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptTokensDetails {
    #[serde(skip_serializing_if = "is_zero")]
    pub cached_tokens: i64,
}

// ---- Claude ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    pub max_tokens: i64,
    /// string or []blocks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "is_false")]
    pub stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ClaudeTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeMessage {
    pub role: String,
    /// string or []ClaudeContentBlock
    pub content: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub text: String,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub id: String,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub tool_use_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ClaudeImageSrc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeImageSrc {
    #[serde(rename = "type")]
    pub src_type: String,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub media_type: String,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub data: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeTool {
    pub name: String,
    #[serde(skip_serializing_if = "is_empty_str")]
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub resp_type: String,
    pub role: String,
    pub content: Vec<ClaudeContentBlock>,
    pub model: String,
    /// Go emits "" when empty; upstream may send null.
    #[serde(deserialize_with = "de_null_string")]
    pub stop_reason: String,
    /// Always serialized (null when absent), like Go.
    pub stop_sequence: Option<String>,
    pub usage: ClaudeUsage,
}

fn de_null_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub cache_read_input_tokens: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub cache_creation_input_tokens: i64,
}

// ---- Models list (OpenAI format) ----

#[derive(Debug, Clone, Serialize)]
pub struct ModelsListResponse {
    pub object: String,
    pub data: Vec<ModelItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelItem {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}
