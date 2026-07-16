package convert

import (
        "encoding/json"
        "fmt"
        "strings"
        "time"

        "github.com/google/uuid"
        "github.com/newnew/gateway/internal/dto"
)

// OpenAIChatToClaude converts OpenAI chat request to Claude messages request.
func OpenAIChatToClaude(req *dto.OpenAIChatRequest) (*dto.ClaudeRequest, error) {
        if req == nil {
                return nil, fmt.Errorf("nil request")
        }
        out := &dto.ClaudeRequest{
                Model:       req.Model,
                Stream:      req.Stream,
                Temperature: req.Temperature,
                TopP:        req.TopP,
                MaxTokens:   4096,
        }
        if req.MaxTokens != nil && *req.MaxTokens > 0 {
                out.MaxTokens = *req.MaxTokens
        }

        // stop
        switch v := req.Stop.(type) {
        case string:
                if v != "" {
                        out.StopSequences = []string{v}
                }
        case []any:
                for _, s := range v {
                        if str, ok := s.(string); ok {
                                out.StopSequences = append(out.StopSequences, str)
                        }
                }
        case []string:
                out.StopSequences = v
        }

        var systemParts []string
        messages := make([]dto.ClaudeMessage, 0, len(req.Messages))
        for _, m := range req.Messages {
                role := m.Role
                switch role {
                case "system":
                        systemParts = append(systemParts, contentToString(m.Content))
                        continue
                case "assistant", "user":
                case "tool":
                        // tool result as user message with tool_result block
                        block := dto.ClaudeContentBlock{
                                Type:      "tool_result",
                                ToolUseID: m.ToolCallID,
                                Content:   contentToString(m.Content),
                        }
                        messages = append(messages, dto.ClaudeMessage{Role: "user", Content: []dto.ClaudeContentBlock{block}})
                        continue
                default:
                        role = "user"
                }

                // assistant tool_calls
                if role == "assistant" && len(m.ToolCalls) > 0 {
                        blocks := make([]dto.ClaudeContentBlock, 0)
                        if text := contentToString(m.Content); text != "" {
                                blocks = append(blocks, dto.ClaudeContentBlock{Type: "text", Text: text})
                        }
                        for _, tc := range m.ToolCalls {
                                var input any
                                if tc.Function.Arguments != "" {
                                        _ = json.Unmarshal([]byte(tc.Function.Arguments), &input)
                                }
                                if input == nil {
                                        input = map[string]any{}
                                }
                                blocks = append(blocks, dto.ClaudeContentBlock{
                                        Type:  "tool_use",
                                        ID:    tc.ID,
                                        Name:  tc.Function.Name,
                                        Input: input,
                                })
                        }
                        messages = append(messages, dto.ClaudeMessage{Role: "assistant", Content: blocks})
                        continue
                }

                messages = append(messages, dto.ClaudeMessage{
                        Role:    role,
                        Content: openAIContentToClaude(m.Content),
                })
        }
        out.Messages = messages
        if len(systemParts) > 0 {
                out.System = strings.Join(systemParts, "\n")
        }

        // tools
        for _, t := range req.Tools {
                if t.Type != "function" {
                        continue
                }
                out.Tools = append(out.Tools, dto.ClaudeTool{
                        Name:        t.Function.Name,
                        Description: t.Function.Description,
                        InputSchema: t.Function.Parameters,
                })
        }
        if req.ToolChoice != nil {
                out.ToolChoice = mapOpenAIToolChoice(req.ToolChoice)
        }
        return out, nil
}

// ClaudeToOpenAIChat converts Claude messages request to OpenAI chat request.
func ClaudeToOpenAIChat(req *dto.ClaudeRequest) (*dto.OpenAIChatRequest, error) {
        if req == nil {
                return nil, fmt.Errorf("nil request")
        }
        maxTokens := req.MaxTokens
        out := &dto.OpenAIChatRequest{
                Model:       req.Model,
                Stream:      req.Stream,
                Temperature: req.Temperature,
                TopP:        req.TopP,
                MaxTokens:   &maxTokens,
        }
        if len(req.StopSequences) == 1 {
                out.Stop = req.StopSequences[0]
        } else if len(req.StopSequences) > 1 {
                out.Stop = req.StopSequences
        }

        messages := make([]dto.OpenAIMessage, 0, len(req.Messages)+1)
        if sys := claudeSystemToString(req.System); sys != "" {
                messages = append(messages, dto.OpenAIMessage{Role: "system", Content: sys})
        }
        for _, m := range req.Messages {
                msgs := claudeMessageToOpenAI(m)
                messages = append(messages, msgs...)
        }
        out.Messages = messages

        for _, t := range req.Tools {
                out.Tools = append(out.Tools, dto.OpenAITool{
                        Type: "function",
                        Function: dto.OpenAIFunction{
                                Name:        t.Name,
                                Description: t.Description,
                                Parameters:  t.InputSchema,
                        },
                })
        }
        if req.ToolChoice != nil {
                out.ToolChoice = mapClaudeToolChoice(req.ToolChoice)
        }
        if req.Stream {
                out.StreamOptions = &dto.StreamOptions{IncludeUsage: true}
        }
        return out, nil
}

// ClaudeResponseToOpenAI converts non-stream Claude response to OpenAI chat response.
func ClaudeResponseToOpenAI(resp *dto.ClaudeResponse, requestModel string) *dto.OpenAIChatResponse {
        if resp == nil {
                return nil
        }
        text, toolCalls := claudeBlocksToOpenAI(resp.Content)
        msg := dto.OpenAIMessage{Role: "assistant", Content: text}
        if len(toolCalls) > 0 {
                msg.ToolCalls = toolCalls
                if text == "" {
                        msg.Content = nil
                }
        }
        fr := mapClaudeStopReason(resp.StopReason)
        modelName := resp.Model
        if modelName == "" {
                modelName = requestModel
        }
        return &dto.OpenAIChatResponse{
                ID:      resp.ID,
                Object:  "chat.completion",
                Created: time.Now().Unix(),
                Model:   modelName,
                Choices: []dto.OpenAIChoice{{
                        Index:        0,
                        Message:      msg,
                        FinishReason: &fr,
                }},
                Usage: &dto.OpenAIUsage{
                        PromptTokens:     resp.Usage.InputTokens + resp.Usage.CacheReadInputTokens + resp.Usage.CacheCreationInputTokens,
                        CompletionTokens: resp.Usage.OutputTokens,
                        TotalTokens:      resp.Usage.InputTokens + resp.Usage.CacheReadInputTokens + resp.Usage.CacheCreationInputTokens + resp.Usage.OutputTokens,
                        PromptTokensDetails: &dto.PromptTokensDetails{
                                CachedTokens: resp.Usage.CacheReadInputTokens,
                        },
                },
        }
}

// OpenAIResponseToClaude converts non-stream OpenAI chat response to Claude response.
func OpenAIResponseToClaude(resp *dto.OpenAIChatResponse) *dto.ClaudeResponse {
        if resp == nil || len(resp.Choices) == 0 {
                return nil
        }
        msg := resp.Choices[0].Message
        blocks := openAIMessageToClaudeBlocks(msg)
        stopReason := "end_turn"
        if resp.Choices[0].FinishReason != nil {
                stopReason = mapOpenAIFinishReason(*resp.Choices[0].FinishReason)
        }
        usage := dto.ClaudeUsage{}
        if resp.Usage != nil {
                cacheRead := 0
                if resp.Usage.PromptTokensDetails != nil {
                        cacheRead = resp.Usage.PromptTokensDetails.CachedTokens
                }
                usage.InputTokens = resp.Usage.PromptTokens - cacheRead
                if usage.InputTokens < 0 {
                        usage.InputTokens = resp.Usage.PromptTokens
                }
                usage.CacheReadInputTokens = cacheRead
                usage.OutputTokens = resp.Usage.CompletionTokens
        }
        id := resp.ID
        if id == "" {
                id = "msg_" + uuid.NewString()
        }
        return &dto.ClaudeResponse{
                ID:         id,
                Type:       "message",
                Role:       "assistant",
                Content:    blocks,
                Model:      resp.Model,
                StopReason: stopReason,
                Usage:      usage,
        }
}

// ---- helpers ----

func contentToString(content any) string {
        switch v := content.(type) {
        case nil:
                return ""
        case string:
                return v
        case []any:
                var b strings.Builder
                for _, part := range v {
                        m, ok := part.(map[string]any)
                        if !ok {
                                continue
                        }
                        if m["type"] == "text" {
                                if t, ok := m["text"].(string); ok {
                                        b.WriteString(t)
                                }
                        }
                }
                return b.String()
        default:
                raw, _ := json.Marshal(v)
                return string(raw)
        }
}

func openAIContentToClaude(content any) any {
        switch v := content.(type) {
        case nil:
                return ""
        case string:
                return v
        case []any:
                blocks := make([]dto.ClaudeContentBlock, 0, len(v))
                for _, part := range v {
                        m, ok := part.(map[string]any)
                        if !ok {
                                continue
                        }
                        typ, _ := m["type"].(string)
                        switch typ {
                        case "text":
                                t, _ := m["text"].(string)
                                blocks = append(blocks, dto.ClaudeContentBlock{Type: "text", Text: t})
                        case "image_url":
                                // basic skip or pass as text note; vision full support later
                                blocks = append(blocks, dto.ClaudeContentBlock{Type: "text", Text: "[image]"})
                        }
                }
                if len(blocks) == 1 && blocks[0].Type == "text" {
                        return blocks[0].Text
                }
                return blocks
        default:
                return contentToString(content)
        }
}

func claudeSystemToString(sys any) string {
        switch v := sys.(type) {
        case nil:
                return ""
        case string:
                return v
        case []any:
                var parts []string
                for _, p := range v {
                        if m, ok := p.(map[string]any); ok {
                                if m["type"] == "text" {
                                        if t, ok := m["text"].(string); ok {
                                                parts = append(parts, t)
                                        }
                                }
                        }
                }
                return strings.Join(parts, "\n")
        default:
                return contentToString(sys)
        }
}

func claudeMessageToOpenAI(m dto.ClaudeMessage) []dto.OpenAIMessage {
        switch blocks := m.Content.(type) {
        case string:
                return []dto.OpenAIMessage{{Role: m.Role, Content: blocks}}
        case []any:
                return claudeAnyBlocksToOpenAI(m.Role, blocks)
        case []dto.ClaudeContentBlock:
                anyBlocks := make([]any, len(blocks))
                for i, b := range blocks {
                        raw, _ := json.Marshal(b)
                        var mm map[string]any
                        _ = json.Unmarshal(raw, &mm)
                        anyBlocks[i] = mm
                }
                return claudeAnyBlocksToOpenAI(m.Role, anyBlocks)
        default:
                return []dto.OpenAIMessage{{Role: m.Role, Content: contentToString(m.Content)}}
        }
}

func claudeAnyBlocksToOpenAI(role string, blocks []any) []dto.OpenAIMessage {
        var textParts []string
        var toolCalls []dto.OpenAIToolCall
        var toolResults []dto.OpenAIMessage

        for _, part := range blocks {
                m, ok := part.(map[string]any)
                if !ok {
                        continue
                }
                typ, _ := m["type"].(string)
                switch typ {
                case "text":
                        if t, ok := m["text"].(string); ok {
                                textParts = append(textParts, t)
                        }
                case "tool_use":
                        id, _ := m["id"].(string)
                        name, _ := m["name"].(string)
                        args, _ := json.Marshal(m["input"])
                        tc := dto.OpenAIToolCall{ID: id, Type: "function"}
                        tc.Function.Name = name
                        tc.Function.Arguments = string(args)
                        toolCalls = append(toolCalls, tc)
                case "tool_result":
                        tid, _ := m["tool_use_id"].(string)
                        content := contentToString(m["content"])
                        toolResults = append(toolResults, dto.OpenAIMessage{
                                Role:       "tool",
                                ToolCallID: tid,
                                Content:    content,
                        })
                }
        }

        var out []dto.OpenAIMessage
        if len(toolResults) > 0 && role == "user" {
                out = append(out, toolResults...)
                return out
        }
        msg := dto.OpenAIMessage{Role: role}
        if len(textParts) > 0 {
                msg.Content = strings.Join(textParts, "")
        }
        if len(toolCalls) > 0 {
                msg.ToolCalls = toolCalls
                if msg.Content == "" {
                        msg.Content = nil
                }
        }
        if msg.Content != nil || len(msg.ToolCalls) > 0 {
                out = append(out, msg)
        }
        return out
}

func claudeBlocksToOpenAI(blocks []dto.ClaudeContentBlock) (any, []dto.OpenAIToolCall) {
        var text strings.Builder
        var toolCalls []dto.OpenAIToolCall
        for _, b := range blocks {
                switch b.Type {
                case "text":
                        text.WriteString(b.Text)
                case "tool_use":
                        args, _ := json.Marshal(b.Input)
                        tc := dto.OpenAIToolCall{ID: b.ID, Type: "function"}
                        tc.Function.Name = b.Name
                        tc.Function.Arguments = string(args)
                        toolCalls = append(toolCalls, tc)
                }
        }
        var content any
        if text.Len() > 0 {
                content = text.String()
        }
        return content, toolCalls
}

func openAIMessageToClaudeBlocks(msg dto.OpenAIMessage) []dto.ClaudeContentBlock {
        var blocks []dto.ClaudeContentBlock
        if s := contentToString(msg.Content); s != "" {
                blocks = append(blocks, dto.ClaudeContentBlock{Type: "text", Text: s})
        }
        for _, tc := range msg.ToolCalls {
                var input any
                _ = json.Unmarshal([]byte(tc.Function.Arguments), &input)
                if input == nil {
                        input = map[string]any{}
                }
                blocks = append(blocks, dto.ClaudeContentBlock{
                        Type:  "tool_use",
                        ID:    tc.ID,
                        Name:  tc.Function.Name,
                        Input: input,
                })
        }
        if len(blocks) == 0 {
                blocks = append(blocks, dto.ClaudeContentBlock{Type: "text", Text: ""})
        }
        return blocks
}

func mapClaudeStopReason(r string) string {
        switch r {
        case "end_turn", "stop_sequence":
                return "stop"
        case "max_tokens":
                return "length"
        case "tool_use":
                return "tool_calls"
        default:
                if r == "" {
                        return "stop"
                }
                return r
        }
}

func mapOpenAIFinishReason(r string) string {
        switch r {
        case "stop":
                return "end_turn"
        case "length":
                return "max_tokens"
        case "tool_calls":
                return "tool_use"
        default:
                return "end_turn"
        }
}

func mapOpenAIToolChoice(tc any) any {
        switch v := tc.(type) {
        case string:
                switch v {
                case "auto":
                        return map[string]any{"type": "auto"}
                case "none":
                        return map[string]any{"type": "none"}
                case "required":
                        return map[string]any{"type": "any"}
                }
        case map[string]any:
                if v["type"] == "function" {
                        if fn, ok := v["function"].(map[string]any); ok {
                                return map[string]any{"type": "tool", "name": fn["name"]}
                        }
                }
        }
        return tc
}

func mapClaudeToolChoice(tc any) any {
        m, ok := tc.(map[string]any)
        if !ok {
                return tc
        }
        switch m["type"] {
        case "auto":
                return "auto"
        case "none":
                return "none"
        case "any":
                return "required"
        case "tool":
                return map[string]any{
                        "type":     "function",
                        "function": map[string]any{"name": m["name"]},
                }
        }
        return tc
}
