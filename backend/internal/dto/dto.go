package dto

// OpenAI Chat Completions request/response (subset)

type OpenAIChatRequest struct {
        Model            string            `json:"model"`
        Messages         []OpenAIMessage   `json:"messages"`
        MaxTokens        *int              `json:"max_tokens,omitempty"`
        Temperature      *float64          `json:"temperature,omitempty"`
        TopP             *float64          `json:"top_p,omitempty"`
        Stream           bool              `json:"stream,omitempty"`
        Stop             any               `json:"stop,omitempty"`
        Tools            []OpenAITool      `json:"tools,omitempty"`
        ToolChoice       any               `json:"tool_choice,omitempty"`
        User             string            `json:"user,omitempty"`
        FrequencyPenalty *float64          `json:"frequency_penalty,omitempty"`
        PresencePenalty  *float64          `json:"presence_penalty,omitempty"`
        N                *int              `json:"n,omitempty"`
        ResponseFormat   any               `json:"response_format,omitempty"`
        StreamOptions    *StreamOptions    `json:"stream_options,omitempty"`
}

type StreamOptions struct {
        IncludeUsage bool `json:"include_usage,omitempty"`
}

type OpenAIMessage struct {
        Role       string `json:"role"`
        Content    any    `json:"content"` // string or []content parts
        Name       string `json:"name,omitempty"`
        ToolCalls  []OpenAIToolCall `json:"tool_calls,omitempty"`
        ToolCallID string `json:"tool_call_id,omitempty"`
}

type OpenAITool struct {
        Type     string         `json:"type"`
        Function OpenAIFunction `json:"function"`
}

type OpenAIFunction struct {
        Name        string `json:"name"`
        Description string `json:"description,omitempty"`
        Parameters  any    `json:"parameters,omitempty"`
}

type OpenAIToolCall struct {
        ID       string `json:"id"`
        Type     string `json:"type"`
        Function struct {
                Name      string `json:"name"`
                Arguments string `json:"arguments"`
        } `json:"function"`
        Index *int `json:"index,omitempty"`
}

type OpenAIChatResponse struct {
        ID      string         `json:"id"`
        Object  string         `json:"object"`
        Created int64          `json:"created"`
        Model   string         `json:"model"`
        Choices []OpenAIChoice `json:"choices"`
        Usage   *OpenAIUsage   `json:"usage,omitempty"`
}

type OpenAIChoice struct {
        Index        int           `json:"index"`
        Message      OpenAIMessage `json:"message,omitempty"`
        Delta        OpenAIMessage `json:"delta,omitempty"`
        FinishReason *string       `json:"finish_reason"`
}

type OpenAIUsage struct {
        PromptTokens        int                  `json:"prompt_tokens"`
        CompletionTokens    int                  `json:"completion_tokens"`
        TotalTokens         int                  `json:"total_tokens"`
        PromptTokensDetails *PromptTokensDetails `json:"prompt_tokens_details,omitempty"`
}

type PromptTokensDetails struct {
        CachedTokens int `json:"cached_tokens,omitempty"`
}

// Claude Messages API (subset)

type ClaudeRequest struct {
        Model         string          `json:"model"`
        Messages      []ClaudeMessage `json:"messages"`
        MaxTokens     int             `json:"max_tokens"`
        System        any             `json:"system,omitempty"` // string or []blocks
        Temperature   *float64        `json:"temperature,omitempty"`
        TopP          *float64        `json:"top_p,omitempty"`
        Stream        bool            `json:"stream,omitempty"`
        StopSequences []string        `json:"stop_sequences,omitempty"`
        Tools         []ClaudeTool    `json:"tools,omitempty"`
        ToolChoice    any             `json:"tool_choice,omitempty"`
}

type ClaudeMessage struct {
        Role    string `json:"role"`
        Content any    `json:"content"` // string or []ClaudeContentBlock
}

type ClaudeContentBlock struct {
        Type      string          `json:"type"`
        Text      string          `json:"text,omitempty"`
        ID        string          `json:"id,omitempty"`
        Name      string          `json:"name,omitempty"`
        Input     any             `json:"input,omitempty"`
        ToolUseID string          `json:"tool_use_id,omitempty"`
        Content   any             `json:"content,omitempty"`
        Source    *ClaudeImageSrc `json:"source,omitempty"`
}

type ClaudeImageSrc struct {
        Type      string `json:"type"`
        MediaType string `json:"media_type,omitempty"`
        Data      string `json:"data,omitempty"`
}

type ClaudeTool struct {
        Name        string `json:"name"`
        Description string `json:"description,omitempty"`
        InputSchema any    `json:"input_schema,omitempty"`
}

type ClaudeResponse struct {
        ID           string               `json:"id"`
        Type         string               `json:"type"`
        Role         string               `json:"role"`
        Content      []ClaudeContentBlock `json:"content"`
        Model        string               `json:"model"`
        StopReason   string               `json:"stop_reason"`
        StopSequence *string              `json:"stop_sequence"`
        Usage        ClaudeUsage          `json:"usage"`
}

type ClaudeUsage struct {
        InputTokens              int `json:"input_tokens"`
        OutputTokens             int `json:"output_tokens"`
        CacheReadInputTokens     int `json:"cache_read_input_tokens,omitempty"`
        CacheCreationInputTokens int `json:"cache_creation_input_tokens,omitempty"`
}

// Models list (OpenAI format)
type ModelsListResponse struct {
        Object string       `json:"object"`
        Data   []ModelItem  `json:"data"`
}

type ModelItem struct {
        ID      string `json:"id"`
        Object  string `json:"object"`
        Created int64  `json:"created"`
        OwnedBy string `json:"owned_by"`
}
