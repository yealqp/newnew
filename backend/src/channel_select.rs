//! Channel selection, port of Go internal/service/channel.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

use rand::Rng;
use sqlx::SqlitePool;

use crate::models::{Channel, CHANNEL_STATUS_ENABLED};

static KEY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Pick an enabled channel that supports the model, by priority DESC then
/// weighted random among the top-priority group.
pub async fn select(pool: &SqlitePool, model_name: &str) -> Result<Channel, String> {
    let channels = sqlx::query_as::<_, Channel>("SELECT * FROM channels WHERE status = ?")
        .bind(CHANNEL_STATUS_ENABLED)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut candidates: Vec<Channel> = channels
        .into_iter()
        .filter(|ch| ch.supports_model(model_name))
        .collect();
    if candidates.is_empty() {
        return Err(format!("no available channel for model {model_name}"));
    }

    candidates.sort_by_key(|ch| std::cmp::Reverse(ch.priority.unwrap_or(0)));
    let top_priority = candidates[0].priority.unwrap_or(0);
    let top: Vec<Channel> = candidates
        .into_iter()
        .filter(|ch| ch.priority.unwrap_or(0) == top_priority)
        .collect();
    if top.len() == 1 {
        return Ok(top.into_iter().next().unwrap());
    }

    let weight_of = |ch: &Channel| -> u64 {
        let w = ch.weight.unwrap_or(1);
        if w <= 0 {
            1
        } else {
            w as u64
        }
    };
    let total: u64 = top.iter().map(weight_of).sum();
    let r = rand::thread_rng().gen_range(0..total);
    let mut acc = 0u64;
    for ch in &top {
        acc += weight_of(ch);
        if r < acc {
            return Ok(ch.clone());
        }
    }
    Ok(top.into_iter().next().unwrap())
}

/// Round-robin key pick for multi-key channels.
pub fn pick_key(ch: &Channel) -> String {
    let keys = ch.get_keys();
    match keys.len() {
        0 => ch.api_key_str().to_string(),
        1 => keys.into_iter().next().unwrap(),
        n => {
            let i = KEY_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
            keys[(i % n as u64) as usize].clone()
        }
    }
}

/// Unique sorted model names from enabled channels.
pub async fn list_enabled_models(pool: &SqlitePool) -> Vec<String> {
    let channels = sqlx::query_as::<_, Channel>("SELECT * FROM channels WHERE status = ?")
        .bind(CHANNEL_STATUS_ENABLED)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let set: BTreeSet<String> = channels.iter().flat_map(|ch| ch.get_models()).collect();
    set.into_iter().collect()
}
