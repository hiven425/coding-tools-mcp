use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

pub(super) const MAX_VALUE_BYTES: usize = 16 * 1024;
const MAX_PREVIEW_BYTES: usize = 4 * 1024;
pub(super) const REDACTED: &str = "[REDACTED]";

static BEARER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bbearer\s+[^\s\"']+"#).expect("valid bearer redaction regex")
});
static ENV_SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(password|passwd|pwd|token|secret|api[_-]?key|authorization|cookie)=([^\s]+)",
    )
    .expect("valid environment secret redaction regex")
});
static FLAG_SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(--(?:password|passwd|pwd|token|secret|api[_-]?key|authorization|cookie)(?:=|\s+))(\"[^\"]*\"|'[^']*'|[^\s]+)"#,
    )
    .expect("valid command flag redaction regex")
});

pub(super) fn sanitize_and_limit(value: &Value) -> Value {
    let sanitized = sanitize_value(value);
    let encoded = serde_json::to_string(&sanitized).unwrap_or_default();
    if encoded.len() <= MAX_VALUE_BYTES {
        sanitized
    } else {
        json!({
            "truncated": true,
            "originalBytes": encoded.len(),
            "preview": bounded_text(&encoded, MAX_PREVIEW_BYTES),
        })
    }
}

fn sanitize_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let value = if sensitive_key(key) {
                        Value::String(REDACTED.into())
                    } else {
                        sanitize_value(value)
                    };
                    (key.clone(), value)
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(sanitize_value).collect()),
        Value::String(value) => Value::String(redact_text(value)),
        _ => value.clone(),
    }
}

fn sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "password"
            | "passwd"
            | "pwd"
            | "token"
            | "secret"
            | "apikey"
            | "authorization"
            | "cookie"
            | "rawuserinput"
            | "initialuserinput"
    ) || normalized.ends_with("password")
        || normalized.ends_with("token")
        || normalized.ends_with("secret")
        || normalized.contains("authorization")
        || normalized.ends_with("cookie")
}

pub(super) fn redact_text(value: &str) -> String {
    let value = BEARER_RE.replace_all(value, "Bearer [REDACTED]");
    let value = ENV_SECRET_RE.replace_all(&value, "$1=[REDACTED]");
    FLAG_SECRET_RE
        .replace_all(&value, "$1[REDACTED]")
        .into_owned()
}

pub(super) fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... [truncated]", &value[..end])
}
