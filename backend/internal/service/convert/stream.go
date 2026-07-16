package convert

import (
        "encoding/json"
        "fmt"
        "strings"
        "time"

        "github.com/google/uuid"
        "github.com/newnew/gateway/internal/dto"
        "github.com/newnew/gateway/internal/service/billing"
)

// StreamConverter converts upstream stream events into client-format SSE bytes.
type StreamConverter interface {
        // OnData handles one upstream data payload. Returns bytes to write to client (may be empty).
        OnData(event string, data string) ([]byte, error)
        // Done returns any trailing bytes (e.g. [DONE]) and usage.
        Finish() ([]byte, billing.Usage)
}

// OpenAIToOpenAIPass is same-format OpenAI stream passthrough (adds include usage capture).
type OpenAIPassthrough struct {
        usage   billing.Usage
        done    bool
}

func NewOpenAIPassthrough() *OpenAIPassthrough {
        return &OpenAIPassthrough{}
}

func (p *OpenAIPassthrough) OnData(_ string, data string) ([]byte, error) {
        if data == "[DONE]" {
                p.done = true
                return []byte("data: [DONE]\n\n"), nil
        }
        var chunk map[string]any
        if err := json.Unmarshal([]byte(data), &chunk); err == nil {
                if u, ok := extractUsageFromOpenAIChunk(chunk); ok {
                        p.usage = mergeUsage(p.usage, u)
                }
        }
        return []byte("data: " + data + "\n\n"), nil
}

func (p *OpenAIPassthrough) Finish() ([]byte, billing.Usage) {
        if !p.done {
                return []byte("data: [DONE]\n\n"), p.usage
        }
        return nil, p.usage
}

// ClaudePassthrough same-format Claude SSE.
type ClaudePassthrough struct {
        usage billing.Usage
}

func NewClaudePassthrough() *ClaudePassthrough {
        return &ClaudePassthrough{}
}

func (p *ClaudePassthrough) OnData(event, data string) ([]byte, error) {
        if u := tryClaudeUsage(data); u != nil {
                p.usage = mergeUsage(p.usage, *u)
        }
        if event != "" {
                return []byte("event: " + event + "\ndata: " + data + "\n\n"), nil
        }
        return []byte("data: " + data + "\n\n"), nil
}

func (p *ClaudePassthrough) Finish() ([]byte, billing.Usage) {
        return nil, p.usage
}

// OpenAIToClaudeStream: upstream OpenAI chunks → client Claude SSE events.
type OpenAIToClaudeStream struct {
        model     string
        msgID     string
        started   bool
        usage     billing.Usage
        toolIndex map[int]string // index -> tool id
        contentStarted bool
}

func NewOpenAIToClaudeStream(model string) *OpenAIToClaudeStream {
        return &OpenAIToClaudeStream{
                model:     model,
                msgID:     "msg_" + uuid.NewString(),
                toolIndex: map[int]string{},
        }
}

func (s *OpenAIToClaudeStream) OnData(_ string, data string) ([]byte, error) {
        if data == "[DONE]" {
                return s.closeEvents(), nil
        }
        // Always try map-based usage extraction first so trailing OpenCode-GO
        // cost chunks (empty choices + normalizedUsage) are not lost.
        var raw map[string]any
        if err := json.Unmarshal([]byte(data), &raw); err == nil {
                if u, ok := extractUsageFromOpenAIChunk(raw); ok {
                        s.usage = mergeUsage(s.usage, u)
                }
        }
        var chunk dto.OpenAIChatResponse
        if err := json.Unmarshal([]byte(data), &chunk); err != nil {
                return nil, nil
        }
        var out strings.Builder
        if !s.started {
                s.started = true
                start := map[string]any{
                        "type": "message_start",
                        "message": map[string]any{
                                "id":            s.msgID,
                                "type":          "message",
                                "role":          "assistant",
                                "content":       []any{},
                                "model":         s.model,
                                "stop_reason":   nil,
                                "stop_sequence": nil,
                                "usage":         map[string]any{"input_tokens": 0, "output_tokens": 0},
                        },
                }
                b, _ := json.Marshal(start)
                out.WriteString(sseEvent("message_start", string(b)))
                out.WriteString(sseEvent("ping", `{"type":"ping"}`))
        }
        if len(chunk.Choices) == 0 {
                return []byte(out.String()), nil
        }
        delta := chunk.Choices[0].Delta
        if text := contentToString(delta.Content); text != "" {
                if !s.contentStarted {
                        s.contentStarted = true
                        blockStart, _ := json.Marshal(map[string]any{
                                "type":  "content_block_start",
                                "index": 0,
                                "content_block": map[string]any{"type": "text", "text": ""},
                        })
                        out.WriteString(sseEvent("content_block_start", string(blockStart)))
                }
                d, _ := json.Marshal(map[string]any{
                        "type":  "content_block_delta",
                        "index": 0,
                        "delta": map[string]any{"type": "text_delta", "text": text},
                })
                out.WriteString(sseEvent("content_block_delta", string(d)))
        }
        // tool calls streaming
        for _, tc := range delta.ToolCalls {
                idx := 0
                if tc.Index != nil {
                        idx = *tc.Index
                }
                // use index+1 for content block index if text started
                blockIdx := idx
                if s.contentStarted {
                        blockIdx = idx + 1
                }
                if tc.ID != "" {
                        s.toolIndex[idx] = tc.ID
                        bs, _ := json.Marshal(map[string]any{
                                "type":  "content_block_start",
                                "index": blockIdx,
                                "content_block": map[string]any{
                                        "type":  "tool_use",
                                        "id":    tc.ID,
                                        "name":  tc.Function.Name,
                                        "input": map[string]any{},
                                },
                        })
                        out.WriteString(sseEvent("content_block_start", string(bs)))
                }
                if tc.Function.Arguments != "" {
                        d, _ := json.Marshal(map[string]any{
                                "type":  "content_block_delta",
                                "index": blockIdx,
                                "delta": map[string]any{"type": "input_json_delta", "partial_json": tc.Function.Arguments},
                        })
                        out.WriteString(sseEvent("content_block_delta", string(d)))
                }
        }
        if chunk.Choices[0].FinishReason != nil && *chunk.Choices[0].FinishReason != "" {
                // close open blocks then message_delta
                if s.contentStarted {
                        stop, _ := json.Marshal(map[string]any{"type": "content_block_stop", "index": 0})
                        out.WriteString(sseEvent("content_block_stop", string(stop)))
                }
                for idx := range s.toolIndex {
                        blockIdx := idx
                        if s.contentStarted {
                                blockIdx = idx + 1
                        }
                        stop, _ := json.Marshal(map[string]any{"type": "content_block_stop", "index": blockIdx})
                        out.WriteString(sseEvent("content_block_stop", string(stop)))
                }
                sr := mapOpenAIFinishReason(*chunk.Choices[0].FinishReason)
                md, _ := json.Marshal(map[string]any{
                        "type":  "message_delta",
                        "delta": map[string]any{"stop_reason": sr, "stop_sequence": nil},
                        "usage": map[string]any{"output_tokens": s.usage.CompletionTokens},
                })
                out.WriteString(sseEvent("message_delta", string(md)))
                ms, _ := json.Marshal(map[string]any{"type": "message_stop"})
                out.WriteString(sseEvent("message_stop", string(ms)))
        }
        return []byte(out.String()), nil
}

func (s *OpenAIToClaudeStream) closeEvents() []byte {
        // already closed on finish_reason usually
        return nil
}

func (s *OpenAIToClaudeStream) Finish() ([]byte, billing.Usage) {
        return nil, s.usage
}

// ClaudeToOpenAIStream: upstream Claude SSE → client OpenAI chunks.
type ClaudeToOpenAIStream struct {
        model   string
        id      string
        created int64
        usage   billing.Usage
        toolArg map[int]string
        toolMeta map[int]struct{ ID, Name string }
}

func NewClaudeToOpenAIStream(model string) *ClaudeToOpenAIStream {
        return &ClaudeToOpenAIStream{
                model:    model,
                id:       "chatcmpl-" + uuid.NewString(),
                created:  time.Now().Unix(),
                toolArg:  map[int]string{},
                toolMeta: map[int]struct{ ID, Name string }{},
        }
}

func (s *ClaudeToOpenAIStream) OnData(event, data string) ([]byte, error) {
        if u := tryClaudeUsage(data); u != nil {
                // merge
                if u.PromptTokens > 0 {
                        s.usage.PromptTokens = u.PromptTokens
                }
                if u.CompletionTokens > 0 {
                        s.usage.CompletionTokens = u.CompletionTokens
                }
                if u.CacheReadTokens > 0 {
                        s.usage.CacheReadTokens = u.CacheReadTokens
                }
                if u.CacheWriteTokens > 0 {
                        s.usage.CacheWriteTokens = u.CacheWriteTokens
                }
        }
        var payload map[string]any
        if err := json.Unmarshal([]byte(data), &payload); err != nil {
                return nil, nil
        }
        typ, _ := payload["type"].(string)
        if typ == "" {
                typ = event
        }
        switch typ {
        case "content_block_start":
                cb, _ := payload["content_block"].(map[string]any)
                if cb == nil {
                        return nil, nil
                }
                if cb["type"] == "tool_use" {
                        idx := intFrom(payload["index"])
                        id, _ := cb["id"].(string)
                        name, _ := cb["name"].(string)
                        s.toolMeta[idx] = struct{ ID, Name string }{id, name}
                        chunk := s.baseChunk()
                        i := idx
                        tc := map[string]any{
                                "index": i,
                                "id":    id,
                                "type":  "function",
                                "function": map[string]any{
                                        "name":      name,
                                        "arguments": "",
                                },
                        }
                        chunk["choices"] = []any{map[string]any{
                                "index": 0,
                                "delta": map[string]any{"tool_calls": []any{tc}},
                                "finish_reason": nil,
                        }}
                        b, _ := json.Marshal(chunk)
                        return []byte("data: " + string(b) + "\n\n"), nil
                }
        case "content_block_delta":
                delta, _ := payload["delta"].(map[string]any)
                if delta == nil {
                        return nil, nil
                }
                dType, _ := delta["type"].(string)
                chunk := s.baseChunk()
                switch dType {
                case "text_delta":
                        text, _ := delta["text"].(string)
                        chunk["choices"] = []any{map[string]any{
                                "index": 0,
                                "delta": map[string]any{"content": text},
                                "finish_reason": nil,
                        }}
                case "input_json_delta":
                        partial, _ := delta["partial_json"].(string)
                        idx := intFrom(payload["index"])
                        meta := s.toolMeta[idx]
                        // map tool block index - if only tools, index is fine
                        toolIdx := 0
                        for k := range s.toolMeta {
                                if k < idx {
                                        toolIdx++
                                }
                        }
                        tc := map[string]any{
                                "index": toolIdx,
                                "function": map[string]any{
                                        "arguments": partial,
                                },
                        }
                        if meta.ID != "" {
                                tc["id"] = meta.ID
                                tc["type"] = "function"
                        }
                        chunk["choices"] = []any{map[string]any{
                                "index": 0,
                                "delta": map[string]any{"tool_calls": []any{tc}},
                                "finish_reason": nil,
                        }}
                default:
                        return nil, nil
                }
                b, _ := json.Marshal(chunk)
                return []byte("data: " + string(b) + "\n\n"), nil
        case "message_delta":
                d, _ := payload["delta"].(map[string]any)
                fr := "stop"
                if d != nil {
                        if sr, ok := d["stop_reason"].(string); ok {
                                fr = mapClaudeStopReason(sr)
                        }
                }
                if u, ok := payload["usage"].(map[string]any); ok {
                        if ot, ok := u["output_tokens"].(float64); ok {
                                s.usage.CompletionTokens = int(ot)
                        }
                }
                chunk := s.baseChunk()
                chunk["choices"] = []any{map[string]any{
                        "index": 0,
                        "delta": map[string]any{},
                        "finish_reason": fr,
                }}
                b, _ := json.Marshal(chunk)
                return []byte("data: " + string(b) + "\n\n"), nil
        case "message_start":
                if msg, ok := payload["message"].(map[string]any); ok {
                        if id, ok := msg["id"].(string); ok && id != "" {
                                s.id = id
                        }
                        if u, ok := msg["usage"].(map[string]any); ok {
                                if it, ok := u["input_tokens"].(float64); ok {
                                        s.usage.PromptTokens = int(it)
                                }
                        }
                }
                // role chunk
                chunk := s.baseChunk()
                chunk["choices"] = []any{map[string]any{
                        "index": 0,
                        "delta": map[string]any{"role": "assistant", "content": ""},
                        "finish_reason": nil,
                }}
                b, _ := json.Marshal(chunk)
                return []byte("data: " + string(b) + "\n\n"), nil
        case "message_stop":
                // usage chunk then DONE
                chunk := s.baseChunk()
                chunk["choices"] = []any{}
                chunk["usage"] = map[string]any{
                        "prompt_tokens":     s.usage.PromptTokens,
                        "completion_tokens": s.usage.CompletionTokens,
                        "total_tokens":      s.usage.PromptTokens + s.usage.CompletionTokens,
                }
                b, _ := json.Marshal(chunk)
                return []byte("data: " + string(b) + "\n\ndata: [DONE]\n\n"), nil
        }
        return nil, nil
}

func (s *ClaudeToOpenAIStream) baseChunk() map[string]any {
        return map[string]any{
                "id":      s.id,
                "object":  "chat.completion.chunk",
                "created": s.created,
                "model":   s.model,
        }
}

func (s *ClaudeToOpenAIStream) Finish() ([]byte, billing.Usage) {
        return []byte("data: [DONE]\n\n"), s.usage
}

func sseEvent(event, data string) string {
        return fmt.Sprintf("event: %s\ndata: %s\n\n", event, data)
}

func tryClaudeUsage(data string) *billing.Usage {
        var m map[string]any
        if err := json.Unmarshal([]byte(data), &m); err != nil {
                return nil
        }
        u := billing.Usage{}
        found := false
        if usage, ok := m["usage"].(map[string]any); ok {
                found = true
                if v, ok := asInt(usage["input_tokens"]); ok {
                        u.PromptTokens = v
                }
                if v, ok := asInt(usage["output_tokens"]); ok {
                        u.CompletionTokens = v
                }
                if v, ok := asInt(usage["cache_read_input_tokens"]); ok {
                        u.CacheReadTokens = v
                }
                if v, ok := asInt(usage["cache_creation_input_tokens"]); ok {
                        u.CacheWriteTokens = v
                }
        }
        if msg, ok := m["message"].(map[string]any); ok {
                if usage, ok := msg["usage"].(map[string]any); ok {
                        found = true
                        if v, ok := asInt(usage["input_tokens"]); ok {
                                u.PromptTokens = v
                        }
                        if v, ok := asInt(usage["output_tokens"]); ok {
                                u.CompletionTokens = v
                        }
                }
        }
        // Claude message_start input + message_delta output often separate
        if !found {
                return nil
        }
        // prompt may need to add cache tokens into prompt for billing non_cached calc
        // we store input as non-cache portion from Claude input_tokens field (already non-cache)
        // but our billing expects PromptTokens as total prompt including cache_read
        u.PromptTokens = u.PromptTokens + u.CacheReadTokens + u.CacheWriteTokens
        return &u
}

// extractUsageFromOpenAIChunk pulls usage from a stream chunk.
// Supports:
//  1. Standard OpenAI: "usage": { prompt_tokens, completion_tokens, prompt_tokens_details.cached_tokens }
//  2. DeepSeek-style top-level: prompt_cache_hit_tokens
//  3. OpenCode-GO trailing cost event: "normalizedUsage": { inputTokens, outputTokens, cacheReadTokens, ... }
//     often with empty choices and "x-opencode-type": "inference-cost"
func extractUsageFromOpenAIChunk(chunk map[string]any) (billing.Usage, bool) {
        var u billing.Usage
        found := false

        if usage, ok := chunk["usage"].(map[string]any); ok {
                found = true
                u = parseOpenAIUsageMap(usage)
        }

        // OpenCode-GO / some gateways put normalized usage on the root of a late chunk
        if nu, ok := chunk["normalizedUsage"].(map[string]any); ok {
                found = true
                if v, ok := asInt(nu["inputTokens"]); ok {
                        u.PromptTokens = v
                }
                if v, ok := asInt(nu["outputTokens"]); ok {
                        u.CompletionTokens = v
                }
                // outputTokens often excludes reasoning; add if present for billing completeness
                if v, ok := asInt(nu["reasoningTokens"]); ok && u.CompletionTokens > 0 {
                        // OpenCode normalizedUsage: outputTokens already includes or is separate?
                        // Sample: outputTokens=545, reasoningTokens=268 with completion_tokens=545
                        // so outputTokens is total completion; don't double-count reasoning.
                        _ = v
                }
                if v, ok := asInt(nu["cacheReadTokens"]); ok {
                        u.CacheReadTokens = v
                }
                cacheWrite := 0
                if v, ok := asInt(nu["cacheWrite5mTokens"]); ok {
                        cacheWrite += v
                }
                if v, ok := asInt(nu["cacheWrite1hTokens"]); ok {
                        cacheWrite += v
                }
                if v, ok := asInt(nu["cacheWriteTokens"]); ok {
                        cacheWrite += v
                }
                if cacheWrite > 0 {
                        u.CacheWriteTokens = cacheWrite
                }
        }

        return u, found && (u.PromptTokens > 0 || u.CompletionTokens > 0 || u.CacheReadTokens > 0 || u.CacheWriteTokens > 0)
}

func parseOpenAIUsageMap(u map[string]any) billing.Usage {
        usage := billing.Usage{}
        if v, ok := asInt(u["prompt_tokens"]); ok {
                usage.PromptTokens = v
        }
        if v, ok := asInt(u["completion_tokens"]); ok {
                usage.CompletionTokens = v
        }
        // DeepSeek / OpenCode: top-level cache hit
        if v, ok := asInt(u["prompt_cache_hit_tokens"]); ok {
                usage.CacheReadTokens = v
        }
        if d, ok := u["prompt_tokens_details"].(map[string]any); ok {
                if v, ok := asInt(d["cached_tokens"]); ok {
                        usage.CacheReadTokens = v
                }
        }
        // some providers nest cache write under completion/prompt details
        if d, ok := u["prompt_tokens_details"].(map[string]any); ok {
                if v, ok := asInt(d["cache_write_tokens"]); ok {
                        usage.CacheWriteTokens = v
                }
        }
        return usage
}

// mergeUsage keeps non-zero fields from later chunks without wiping earlier data with zeros.
func mergeUsage(base, next billing.Usage) billing.Usage {
        if next.PromptTokens > 0 {
                base.PromptTokens = next.PromptTokens
        }
        if next.CompletionTokens > 0 {
                base.CompletionTokens = next.CompletionTokens
        }
        if next.CacheReadTokens > 0 {
                base.CacheReadTokens = next.CacheReadTokens
        }
        if next.CacheWriteTokens > 0 {
                base.CacheWriteTokens = next.CacheWriteTokens
        }
        return base
}

func asInt(v any) (int, bool) {
        switch n := v.(type) {
        case float64:
                return int(n), true
        case float32:
                return int(n), true
        case int:
                return n, true
        case int64:
                return int(n), true
        case json.Number:
                i, err := n.Int64()
                if err != nil {
                        f, err2 := n.Float64()
                        if err2 != nil {
                                return 0, false
                        }
                        return int(f), true
                }
                return int(i), true
        case string:
                // rare: numeric string
                var f float64
                if err := json.Unmarshal([]byte(n), &f); err == nil {
                        return int(f), true
                }
                return 0, false
        default:
                return 0, false
        }
}

func intFrom(v any) int {
        n, _ := asInt(v)
        return n
}
