package billing

import (
        "testing"

        "github.com/newnew/gateway/internal/model"
)

func TestCalculateBasic(t *testing.T) {
        // 1M output tokens at 2 CNY/1M = 2 CNY
        r := Calculate(
                model.ModelPrice{Input: 0.5, Output: 2.0, CacheRead: 0.05, CacheWrite: 0.5},
                true,
                Usage{PromptTokens: 0, CompletionTokens: 1_000_000},
        )
        if r.CostRMB != 2.0 {
                t.Fatalf("expected 2.0, got %v", r.CostRMB)
        }
        if r.PriceMissing {
                t.Fatal("should not be missing")
        }
}

func TestCalculateWithCache(t *testing.T) {
        // 500k non-cache input @1, 500k cache_read @0.1, 100k output @2
        // = 0.5*1 + 0.5*0.1 + 0.1*2 = 0.75
        r := Calculate(
                model.ModelPrice{Input: 1, Output: 2, CacheRead: 0.1, CacheWrite: 3},
                true,
                Usage{
                        PromptTokens:     1_000_000,
                        CacheReadTokens:  500_000,
                        CompletionTokens: 100_000,
                },
        )
        if r.CostRMB < 0.749 || r.CostRMB > 0.751 {
                t.Fatalf("expected ~0.75, got %v", r.CostRMB)
        }
}

func TestPriceMissing(t *testing.T) {
        r := Calculate(model.ModelPrice{}, false, Usage{PromptTokens: 100})
        if !r.PriceMissing || r.CostRMB != 0 {
                t.Fatalf("expected missing price with 0 cost, got %+v", r)
        }
}
