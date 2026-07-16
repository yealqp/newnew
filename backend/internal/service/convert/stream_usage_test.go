package convert

import (
        "testing"

        "github.com/newnew/gateway/internal/service/billing"
)

func TestExtractOpenAIUsageFromChunk(t *testing.T) {
        chunk := map[string]any{
                "id":     "d007c232-1f41-4c15-979d-cffd84c7cfc4",
                "object": "chat.completion.chunk",
                "model":  "deepseek-v4-flash",
                "choices": []any{
                        map[string]any{
                                "index":         float64(0),
                                "finish_reason": "stop",
                                "delta":         map[string]any{"content": ""},
                        },
                },
                "usage": map[string]any{
                        "prompt_tokens":             float64(88),
                        "completion_tokens":         float64(545),
                        "total_tokens":              float64(633),
                        "prompt_cache_hit_tokens":   float64(0),
                        "prompt_cache_miss_tokens":  float64(88),
                        "prompt_tokens_details": map[string]any{
                                "cached_tokens": float64(0),
                        },
                        "completion_tokens_details": map[string]any{
                                "reasoning_tokens": float64(268),
                        },
                },
        }
        u, ok := extractUsageFromOpenAIChunk(chunk)
        if !ok {
                t.Fatal("expected usage found")
        }
        if u.PromptTokens != 88 || u.CompletionTokens != 545 {
                t.Fatalf("got %+v", u)
        }
}

func TestExtractOpenCodeNormalizedUsage(t *testing.T) {
        // trailing OpenCode-GO inference-cost chunk with empty choices
        chunk := map[string]any{
                "choices":          []any{},
                "x-opencode-type":  "inference-cost",
                "cost":             "0.00016492",
                "normalizedUsage": map[string]any{
                        "inputTokens":         float64(88),
                        "outputTokens":        float64(545),
                        "reasoningTokens":     float64(268),
                        "cacheReadTokens":     float64(12),
                        "cacheWrite5mTokens":  float64(3),
                        "cacheWrite1hTokens":  float64(1),
                },
        }
        u, ok := extractUsageFromOpenAIChunk(chunk)
        if !ok {
                t.Fatal("expected normalized usage found")
        }
        if u.PromptTokens != 88 {
                t.Fatalf("prompt=%d", u.PromptTokens)
        }
        if u.CompletionTokens != 545 {
                t.Fatalf("completion=%d", u.CompletionTokens)
        }
        if u.CacheReadTokens != 12 {
                t.Fatalf("cache_read=%d", u.CacheReadTokens)
        }
        if u.CacheWriteTokens != 4 { // 3+1
                t.Fatalf("cache_write=%d", u.CacheWriteTokens)
        }
}

func TestOpenAIPassthroughMergesLateUsage(t *testing.T) {
        p := NewOpenAIPassthrough()
        // content chunk without usage
        _, _ = p.OnData("", `{"id":"1","object":"chat.completion.chunk","choices":[{"delta":{"content":"hi"}}]}`)
        // late usage chunk
        _, _ = p.OnData("", `{
                "id":"1","object":"chat.completion.chunk",
                "choices":[{"index":0,"finish_reason":"stop","delta":{"content":""}}],
                "usage":{"prompt_tokens":88,"completion_tokens":545,"prompt_tokens_details":{"cached_tokens":0}}
        }`)
        // OpenCode cost trailer
        _, _ = p.OnData("", `{
                "choices":[],
                "x-opencode-type":"inference-cost",
                "normalizedUsage":{"inputTokens":88,"outputTokens":545,"cacheReadTokens":0,"cacheWrite5mTokens":0,"cacheWrite1hTokens":0}
        }`)
        _, u := p.Finish()
        if u.PromptTokens != 88 || u.CompletionTokens != 545 {
                t.Fatalf("usage not captured: %+v", u)
        }
}

func TestMergeUsageDoesNotWipeWithZeros(t *testing.T) {
        base := billing.Usage{PromptTokens: 10, CompletionTokens: 20, CacheReadTokens: 2}
        next := billing.Usage{PromptTokens: 0, CompletionTokens: 30}
        got := mergeUsage(base, next)
        if got.PromptTokens != 10 {
                t.Fatalf("prompt wiped: %d", got.PromptTokens)
        }
        if got.CompletionTokens != 30 {
                t.Fatalf("completion: %d", got.CompletionTokens)
        }
        if got.CacheReadTokens != 2 {
                t.Fatalf("cache wiped: %d", got.CacheReadTokens)
        }
}
