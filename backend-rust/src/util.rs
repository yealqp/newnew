use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use rand::RngCore;
use serde_json::{json, Value};

// ---- time ----
//
// The Go backend (GORM + mattn/go-sqlite3) stores datetimes as TEXT like
// "2026-07-16 06:55:48.9276178-05:00" (space separator, local offset).
// We keep writing the same format so string comparison / ORDER BY stays
// consistent with existing rows, and convert to RFC3339 for JSON output.

const DB_TIME_FMT: &str = "%Y-%m-%d %H:%M:%S%.9f%:z";

/// Current local time in DB text format.
pub fn now_db_string() -> String {
    Local::now().format(DB_TIME_FMT).to_string()
}

/// Format an arbitrary time in DB text format (for WHERE bindings).
pub fn to_db_string(dt: &DateTime<Local>) -> String {
    dt.format(DB_TIME_FMT).to_string()
}

/// Convert DB text datetime to RFC3339 for JSON output (Go marshals
/// time.Time as RFC3339). "2026-07-16 06:55:48.92-05:00" -> "2026-07-16T06:55:48.92-05:00".
pub fn db_time_to_rfc3339(s: &str) -> String {
    if s.len() > 10 && s.as_bytes()[10] == b' ' {
        let mut out = s.to_string();
        out.replace_range(10..11, "T");
        out
    } else {
        s.to_string()
    }
}

/// Optional variant used by model serializers.
pub fn opt_db_time_to_rfc3339(s: &Option<String>) -> Value {
    match s {
        Some(v) if !v.is_empty() => Value::String(db_time_to_rfc3339(v)),
        _ => Value::Null,
    }
}

/// Port of Go parseFlexibleTime: RFC3339, "2006-01-02 15:04:05",
/// "2006-01-02T15:04:05", "2006-01-02" (naive forms interpreted as local time).
pub fn parse_flexible_time(s: &str) -> Option<DateTime<Local>> {
    let s = s.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&Local));
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            if let Some(t) = Local.from_local_datetime(&naive).earliest() {
                return Some(t);
            }
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if let Some(naive) = d.and_hms_opt(0, 0, 0) {
            if let Some(t) = Local.from_local_datetime(&naive).earliest() {
                return Some(t);
            }
        }
    }
    None
}

pub fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---- strings ----

/// Truncate to at most `n` bytes on a char boundary, appending "..." when cut.
pub fn truncate_str(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

/// Truncate to at most `n` bytes on a char boundary without a suffix
/// (Go upstream error prettifiers use plain slicing).
pub fn truncate_str_plain(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// Log-body truncation, matches Go logsvc.truncate (appends "...(truncated)").
pub fn truncate_body(s: &str, max: usize) -> String {
    if max == 0 || s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...(truncated)", &s[..end])
}

/// Mask an API key (multi-line aware), matches Go maskKey.
pub fn mask_key(key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    key.split('\n')
        .map(|p| {
            let p = p.trim();
            if p.chars().count() <= 8 {
                "****".to_string()
            } else {
                let chars: Vec<char> = p.chars().collect();
                let head: String = chars[..4].iter().collect();
                let tail: String = chars[chars.len() - 4..].iter().collect();
                format!("{head}****{tail}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- json ----

/// Lenient int extraction from a JSON value (number or numeric string),
/// matches Go convert.asInt.
pub fn json_as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else {
                n.as_f64().map(|f| f as i64)
            }
        }
        Value::String(s) => s.parse::<f64>().ok().map(|f| f as i64),
        _ => None,
    }
}

// ---- responses ----

pub fn ok_data(data: Value) -> Json<Value> {
    Json(json!({"success": true, "data": data}))
}

pub fn fail_msg(msg: &str) -> Json<Value> {
    Json(json!({"success": false, "message": msg}))
}

pub fn resp(status: StatusCode, body: Json<Value>) -> Response {
    (status, body).into_response()
}

pub fn ok_resp(data: Value) -> Response {
    ok_data(data).into_response()
}

pub fn fail_resp(status: u16, msg: &str) -> Response {
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        fail_msg(msg),
    )
        .into_response()
}

/// OpenAI-style error body used by token auth / relay.
pub fn openai_error(msg: &str, typ: &str) -> Value {
    json!({"error": {"message": msg, "type": typ}})
}

/// Relay error body in the client's format, matches Go errJSON.
pub fn err_json(client_format: &str, msg: &str) -> Value {
    if client_format == "claude" {
        json!({"type": "error", "error": {"type": "api_error", "message": msg}})
    } else {
        json!({"error": {"message": msg, "type": "api_error"}})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_time_to_rfc3339() {
        assert_eq!(
            db_time_to_rfc3339("2026-07-16 06:55:48.9276178-05:00"),
            "2026-07-16T06:55:48.9276178-05:00"
        );
        assert_eq!(
            db_time_to_rfc3339("2026-07-16T06:55:48Z"),
            "2026-07-16T06:55:48Z"
        );
    }

    #[test]
    fn test_mask_key() {
        assert_eq!(mask_key("short"), "****");
        assert_eq!(mask_key("sk-abcdefghijklmnop"), "sk-a****mnop");
    }

    #[test]
    fn test_truncate_body_char_boundary() {
        let s = "中文中文中文";
        let t = truncate_body(s, 4);
        assert!(t.ends_with("...(truncated)"));
        assert!(t.starts_with('中'));
    }

    #[test]
    fn test_now_db_string_format() {
        let s = now_db_string();
        // "2026-07-17 10:00:00.123456789+08:00" — space at index 10, parseable back
        assert_eq!(s.as_bytes()[10], b' ');
        assert!(DateTime::parse_from_rfc3339(&db_time_to_rfc3339(&s)).is_ok(), "{s}");
    }

    #[test]
    fn test_parse_flexible_time() {
        assert!(parse_flexible_time("2026-07-16T00:00:00Z").is_some());
        assert!(parse_flexible_time("2026-07-16 10:00:00").is_some());
        assert!(parse_flexible_time("2026-07-16").is_some());
        assert!(parse_flexible_time("garbage").is_none());
    }
}
