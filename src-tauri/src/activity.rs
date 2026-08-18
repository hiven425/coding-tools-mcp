use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const MAX_TRACES: usize = 500;
const MAX_VALUE_BYTES: usize = 16 * 1024;
const MAX_PREVIEW_BYTES: usize = 4 * 1024;
const REDACTED: &str = "[REDACTED]";

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityTrace {
    pub trace_id: String,
    pub rpc_id: String,
    pub method: String,
    pub tool: String,
    pub route: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub status: String,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub request: Value,
    pub response: Value,
    pub error: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityQuery {
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub status: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshot {
    pub traces: Vec<ActivityTrace>,
    pub total_matching: usize,
    pub retained: usize,
    pub max_entries: usize,
}

pub struct ActivityStore {
    traces: Mutex<VecDeque<ActivityTrace>>,
}

impl ActivityStore {
    pub fn new() -> Self {
        Self {
            traces: Mutex::new(VecDeque::with_capacity(MAX_TRACES)),
        }
    }

    pub fn begin_trace(
        &self,
        workspace_id: &str,
        workspace_name: &str,
        route: &str,
        body: &Value,
    ) -> String {
        let trace_id = format!("trace_{}", uuid::Uuid::new_v4().simple());
        let trace = ActivityTrace {
            trace_id: trace_id.clone(),
            rpc_id: bounded_text(
                &body.get("id").map(Value::to_string).unwrap_or_default(),
                256,
            ),
            method: bounded_text(
                body.get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                256,
            ),
            tool: bounded_text(tool_name(body), 256),
            route: bounded_text(route, 256),
            workspace_id: bounded_text(workspace_id, 256),
            workspace_name: bounded_text(workspace_name, 256),
            status: "running".into(),
            started_at_ms: now_ms(),
            finished_at_ms: None,
            duration_ms: None,
            request: sanitize_and_limit(body),
            response: Value::Null,
            error: Value::Null,
        };
        let mut traces = self
            .traces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        traces.push_back(trace);
        while traces.len() > MAX_TRACES {
            traces.pop_front();
        }
        trace_id
    }

    pub fn complete_trace(&self, trace_id: &str, response: &Value) {
        let finished_at_ms = now_ms();
        let failed = response_failed(response);
        self.update_trace(trace_id, |trace| {
            trace.status = if failed { "failed" } else { "completed" }.into();
            trace.finished_at_ms = Some(finished_at_ms);
            trace.duration_ms = Some(finished_at_ms.saturating_sub(trace.started_at_ms));
            trace.response = sanitize_and_limit(response);
            trace.error = response
                .get("error")
                .map(sanitize_and_limit)
                .unwrap_or(Value::Null);
        });
    }

    pub fn fail_trace(&self, trace_id: &str, message: &str) {
        let finished_at_ms = now_ms();
        self.update_trace(trace_id, |trace| {
            trace.status = "failed".into();
            trace.finished_at_ms = Some(finished_at_ms);
            trace.duration_ms = Some(finished_at_ms.saturating_sub(trace.started_at_ms));
            trace.error = json!({
                "message": bounded_text(&redact_text(message), 1024),
            });
        });
    }

    pub fn snapshot(&self, query: &ActivityQuery) -> ActivitySnapshot {
        let traces = self
            .traces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let total_matching = traces
            .iter()
            .filter(|trace| matches_query(trace, query))
            .count();
        let items = traces
            .iter()
            .rev()
            .filter(|trace| matches_query(trace, query))
            .take(query.limit.clamp(1, MAX_TRACES))
            .cloned()
            .collect();
        ActivitySnapshot {
            traces: items,
            total_matching,
            retained: traces.len(),
            max_entries: MAX_TRACES,
        }
    }

    pub fn get(&self, trace_id: &str) -> Option<ActivityTrace> {
        self.traces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|trace| trace.trace_id == trace_id)
            .cloned()
    }

    pub fn clear(&self) -> usize {
        let mut traces = self
            .traces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let removed = traces.len();
        traces.clear();
        removed
    }

    fn update_trace(&self, trace_id: &str, update: impl FnOnce(&mut ActivityTrace)) {
        let mut traces = self
            .traces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(trace) = traces.iter_mut().find(|trace| trace.trace_id == trace_id) {
            update(trace);
        }
    }
}

impl Default for ActivityStore {
    fn default() -> Self {
        Self::new()
    }
}

fn default_limit() -> usize {
    200
}

fn matches_query(trace: &ActivityTrace, query: &ActivityQuery) -> bool {
    (contains_fold(&trace.workspace_name, &query.workspace)
        || contains_fold(&trace.workspace_id, &query.workspace))
        && contains_fold(&trace.tool, &query.tool)
        && contains_fold(&trace.status, &query.status)
}

fn contains_fold(value: &str, needle: &str) -> bool {
    needle.trim().is_empty()
        || value
            .to_ascii_lowercase()
            .contains(&needle.trim().to_ascii_lowercase())
}

fn tool_name(body: &Value) -> &str {
    body.get("params")
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn response_failed(response: &Value) -> bool {
    response.get("error").is_some()
        || response
            .get("result")
            .and_then(|result| result.get("isError"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || response
            .get("result")
            .and_then(|result| result.get("structuredContent"))
            .is_some_and(|structured| {
                structured.get("ok").and_then(Value::as_bool) == Some(false)
                    || structured.get("command_ok").and_then(Value::as_bool) == Some(false)
            })
}

fn sanitize_and_limit(value: &Value) -> Value {
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

fn redact_text(value: &str) -> String {
    let value = BEARER_RE.replace_all(value, "Bearer [REDACTED]");
    let value = ENV_SECRET_RE.replace_all(&value, "$1=[REDACTED]");
    FLAG_SECRET_RE
        .replace_all(&value, "$1[REDACTED]")
        .into_owned()
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... [truncated]", &value[..end])
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ActivityQuery, ActivityStore, MAX_TRACES, MAX_VALUE_BYTES, REDACTED};

    #[test]
    fn trace_payloads_are_redacted_before_storage() {
        let store = ActivityStore::new();
        let request = json!({
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "exec_command",
                "arguments": {
                    "password": "plain-password",
                    "nested": {"api_key": "plain-key", "raw_user_input": "private prompt"},
                    "cmd": "runner --token plain-token --api-key=plain-api PASSWORD=plain-env Bearer plain-bearer"
                }
            }
        });
        let trace_id = store.begin_trace("workspace-1", "Demo", "/mcp", &request);
        let response = json!({
            "result": {"structuredContent": {
                "authorization": "Bearer response-secret",
                "command": "tool COOKIE=session-secret --secret hidden"
            }}
        });
        store.complete_trace(&trace_id, &response);

        let trace = store.get(&trace_id).expect("stored trace");
        let encoded = serde_json::to_string(&trace).expect("serialize trace");
        for secret in [
            "plain-password",
            "plain-key",
            "private prompt",
            "plain-token",
            "plain-api",
            "plain-env",
            "plain-bearer",
            "response-secret",
            "session-secret",
            "hidden",
        ] {
            assert!(!encoded.contains(secret), "secret leaked: {secret}");
        }
        assert_eq!(trace.request["params"]["arguments"]["password"], REDACTED);
    }

    #[test]
    fn activity_storage_and_values_are_bounded() {
        let store = ActivityStore::new();
        for id in 0..MAX_TRACES + 20 {
            let body = json!({
                "id": id,
                "method": "tools/call",
                "params": {"name": "read_file", "arguments": {"content": "x".repeat(40_000)}}
            });
            store.begin_trace("workspace-1", "Demo", "/mcp", &body);
        }
        let snapshot = store.snapshot(&ActivityQuery {
            limit: MAX_TRACES + 20,
            ..ActivityQuery::default()
        });
        assert_eq!(snapshot.retained, MAX_TRACES);
        assert_eq!(snapshot.traces.len(), MAX_TRACES);
        assert!(
            serde_json::to_vec(&snapshot.traces[0].request)
                .expect("serialize request")
                .len()
                <= MAX_VALUE_BYTES
        );
    }

    #[test]
    fn snapshot_filters_and_clear_are_deterministic() {
        let store = ActivityStore::new();
        let first = json!({"id": 1, "method": "tools/call", "params": {"name": "read_file"}});
        let second = json!({"id": 2, "method": "tools/call", "params": {"name": "exec_command"}});
        store.begin_trace("one", "Alpha", "/mcp", &first);
        let second_id = store.begin_trace("two", "Beta", "/mcp", &second);
        store.fail_trace(&second_id, "worker failed");

        let snapshot = store.snapshot(&ActivityQuery {
            workspace: "beta".into(),
            tool: "exec".into(),
            status: "failed".into(),
            limit: 20,
        });
        assert_eq!(snapshot.total_matching, 1);
        assert_eq!(snapshot.traces[0].trace_id, second_id);
        assert_eq!(store.clear(), 2);
        assert_eq!(store.clear(), 0);
    }
}
