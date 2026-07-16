package billing

import (
        "github.com/newnew/gateway/internal/model"
)

// Usage holds token counts extracted from upstream responses.
type Usage struct {
        PromptTokens     int
        CompletionTokens int
        CacheReadTokens  int
        CacheWriteTokens int
}

// Result is the computed cost in CNY.
type Result struct {
        CostRMB      float64
        PriceMissing bool
        Price        model.ModelPrice
        TotalTokens  int
}

// Calculate computes cost_rmb using channel pricing in CNY per 1M tokens.
//
// cost_rmb = (non_cached_input * input + cache_read * cache_read
//             + cache_write * cache_write + completion * output) / 1_000_000
func Calculate(price model.ModelPrice, found bool, u Usage) Result {
        total := u.PromptTokens + u.CompletionTokens
        if !found {
                return Result{
                        CostRMB:      0,
                        PriceMissing: true,
                        TotalTokens:  total,
                }
        }
        nonCachedInput := u.PromptTokens - u.CacheReadTokens
        if nonCachedInput < 0 {
                nonCachedInput = 0
        }
        cost := (float64(nonCachedInput)*price.Input +
                float64(u.CacheReadTokens)*price.CacheRead +
                float64(u.CacheWriteTokens)*price.CacheWrite +
                float64(u.CompletionTokens)*price.Output) / 1_000_000.0
        return Result{
                CostRMB:      cost,
                PriceMissing: false,
                Price:        price,
                TotalTokens:  total,
        }
}

// CalculateForChannel looks up model price on the channel then calculates.
func CalculateForChannel(ch *model.Channel, modelName string, u Usage) Result {
        price, ok := ch.GetModelPrice(modelName)
        if !ok {
                // also try upstream-mapped name already applied by caller
                price, ok = ch.GetModelPrice(ch.MapModel(modelName))
        }
        return Calculate(price, ok, u)
}
