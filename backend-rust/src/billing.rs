//! Cost calculation, port of Go internal/service/billing.

use crate::models::{Channel, ModelPrice};

/// Token counts extracted from upstream responses.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// Computed cost in CNY.
#[derive(Debug, Clone, Default)]
pub struct BillingResult {
    pub cost_rmb: f64,
    pub price_missing: bool,
    pub price: ModelPrice,
    /// Kept for parity with Go billing.Result; log rows compute totals themselves.
    #[allow(dead_code)]
    pub total_tokens: i64,
}

/// cost_rmb = (non_cached_input * input + cache_read * cache_read
///             + cache_write * cache_write + completion * output) / 1_000_000
pub fn calculate(price: &ModelPrice, found: bool, u: &Usage) -> BillingResult {
    let total = u.prompt_tokens + u.completion_tokens;
    if !found {
        return BillingResult {
            cost_rmb: 0.0,
            price_missing: true,
            price: ModelPrice::default(),
            total_tokens: total,
        };
    }
    let non_cached_input = (u.prompt_tokens - u.cache_read_tokens).max(0);
    let cost = (non_cached_input as f64 * price.input
        + u.cache_read_tokens as f64 * price.cache_read
        + u.cache_write_tokens as f64 * price.cache_write
        + u.completion_tokens as f64 * price.output)
        / 1_000_000.0;
    BillingResult {
        cost_rmb: cost,
        price_missing: false,
        price: price.clone(),
        total_tokens: total,
    }
}

/// Look up model price on the channel (client name, then upstream-mapped name),
/// then calculate.
pub fn calculate_for_channel(ch: &Channel, model_name: &str, u: &Usage) -> BillingResult {
    let price = ch
        .get_model_price(model_name)
        .or_else(|| ch.get_model_price(&ch.map_model(model_name)));
    match price {
        Some(p) => calculate(&p, true, u),
        None => calculate(&ModelPrice::default(), false, u),
    }
}

/// Keep non-zero fields from later chunks without wiping earlier data with zeros.
pub fn merge_usage(mut base: Usage, next: Usage) -> Usage {
    if next.prompt_tokens > 0 {
        base.prompt_tokens = next.prompt_tokens;
    }
    if next.completion_tokens > 0 {
        base.completion_tokens = next.completion_tokens;
    }
    if next.cache_read_tokens > 0 {
        base.cache_read_tokens = next.cache_read_tokens;
    }
    if next.cache_write_tokens > 0 {
        base.cache_write_tokens = next.cache_write_tokens;
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_basic() {
        // 1M output tokens at 2 CNY/1M = 2 CNY
        let r = calculate(
            &ModelPrice {
                input: 0.5,
                output: 2.0,
                cache_read: 0.05,
                cache_write: 0.5,
            },
            true,
            &Usage {
                completion_tokens: 1_000_000,
                ..Default::default()
            },
        );
        assert_eq!(r.cost_rmb, 2.0);
        assert!(!r.price_missing);
    }

    #[test]
    fn test_calculate_with_cache() {
        // 500k non-cache input @1, 500k cache_read @0.1, 100k output @2 = 0.75
        let r = calculate(
            &ModelPrice {
                input: 1.0,
                output: 2.0,
                cache_read: 0.1,
                cache_write: 3.0,
            },
            true,
            &Usage {
                prompt_tokens: 1_000_000,
                cache_read_tokens: 500_000,
                completion_tokens: 100_000,
                cache_write_tokens: 0,
            },
        );
        assert!(r.cost_rmb > 0.749 && r.cost_rmb < 0.751, "got {}", r.cost_rmb);
    }

    #[test]
    fn test_price_missing() {
        let r = calculate(
            &ModelPrice::default(),
            false,
            &Usage {
                prompt_tokens: 100,
                ..Default::default()
            },
        );
        assert!(r.price_missing);
        assert_eq!(r.cost_rmb, 0.0);
    }

    #[test]
    fn test_merge_usage_does_not_wipe_with_zeros() {
        let base = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            cache_read_tokens: 2,
            cache_write_tokens: 0,
        };
        let next = Usage {
            prompt_tokens: 0,
            completion_tokens: 30,
            ..Default::default()
        };
        let got = merge_usage(base, next);
        assert_eq!(got.prompt_tokens, 10);
        assert_eq!(got.completion_tokens, 30);
        assert_eq!(got.cache_read_tokens, 2);
    }
}
