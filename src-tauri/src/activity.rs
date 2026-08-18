use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

#[path = "activity_sanitize.rs"]
mod sanitize;

use sanitize::{bounded_text, redact_text, sanitize_and_limit};

const MAX_TRACES: usize = 500;
const MAX_PROCESSES: usize = 100;
const EVENT_CAPACITY: usize = 256;

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
    pub operation_id: String,
    pub process_session_id: String,
    pub parent_trace_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityProcess {
    pub session_id: String,
    pub operation_id: String,
    pub trace_id: String,
    pub workspace_name: String,
    pub command: String,
    pub status: String,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub exit_code: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub kind: String,
    pub trace: Option<ActivityTrace>,
    pub process: Option<ActivityProcess>,
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
    pub active_processes: Vec<ActivityProcess>,
    pub active_requests: usize,
    pub total_matching: usize,
    pub retained: usize,
    pub max_entries: usize,
}

struct ActivityInner {
    traces: VecDeque<ActivityTrace>,
    processes: HashMap<String, ActivityProcess>,
    process_trace_by_session: HashMap<String, String>,
}

pub struct ActivityStore {
    inner: Mutex<ActivityInner>,
    events: broadcast::Sender<ActivityEvent>,
}

impl ActivityStore {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            inner: Mutex::new(ActivityInner {
                traces: VecDeque::with_capacity(MAX_TRACES),
                processes: HashMap::new(),
                process_trace_by_session: HashMap::new(),
            }),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ActivityEvent> {
        self.events.subscribe()
    }

    pub fn begin_trace(
        &self,
        workspace_id: &str,
        workspace_name: &str,
        route: &str,
        body: &Value,
    ) -> String {
        let trace_id = format!("trace_{}", uuid::Uuid::new_v4().simple());
        let process_session_id = bounded_text(argument_session_id(body).unwrap_or_default(), 256);
        let trace = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let parent_trace_id = inner
                .process_trace_by_session
                .get(&process_session_id)
                .cloned()
                .unwrap_or_default();
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
                operation_id: String::new(),
                process_session_id,
                parent_trace_id,
            };
            inner.traces.push_back(trace.clone());
            while inner.traces.len() > MAX_TRACES {
                inner.traces.pop_front();
            }
            trace
        };
        self.emit("activity.started", Some(trace), None);
        trace_id
    }

    pub fn complete_trace(&self, trace_id: &str, response: &Value) {
        let finished_at_ms = now_ms();
        let failed = response_failed(response);
        let structured = structured_content(response);
        let mut process_event = None;
        let trace = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(index) = inner
                .traces
                .iter()
                .position(|trace| trace.trace_id == trace_id)
            else {
                return;
            };

            let tool = inner.traces[index].tool.clone();
            let related_session = inner.traces[index].process_session_id.clone();
            if tool == "exec_command" {
                if let Some(data) = structured {
                    let status = process_status(data).unwrap_or_default();
                    if status == "running" {
                        if let Some(raw_session_id) = data
                            .get("session_id")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            let session_id = bounded_text(raw_session_id, 256);
                            let process = ActivityProcess {
                                session_id: session_id.clone(),
                                operation_id: bounded_text(
                                    data.get("operation_id")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default(),
                                    256,
                                ),
                                trace_id: trace_id.to_string(),
                                workspace_name: inner.traces[index].workspace_name.clone(),
                                command: trace_command(&inner.traces[index]),
                                status: "running".into(),
                                started_at_ms: inner.traces[index].started_at_ms,
                                updated_at_ms: finished_at_ms,
                                exit_code: None,
                            };
                            inner
                                .process_trace_by_session
                                .insert(session_id.clone(), trace_id.to_string());
                            inner.processes.insert(session_id, process.clone());
                            enforce_process_limit(&mut inner);
                            process_event = Some(process);
                        }
                    }
                }
            } else if !related_session.is_empty() {
                let mut terminal = false;
                if let Some(process) = inner.processes.get_mut(&related_session) {
                    if let Some(data) = structured {
                        if let Some(status) = process_status(data) {
                            process.status = bounded_text(status, 64);
                        }
                        process.exit_code = data.get("exit_code").and_then(Value::as_i64);
                        if process.operation_id.is_empty() {
                            process.operation_id = bounded_text(
                                data.get("operation_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default(),
                                256,
                            );
                        }
                    }
                    process.updated_at_ms = finished_at_ms;
                    terminal = is_terminal_process_status(&process.status);
                    process_event = Some(process.clone());
                }
                if terminal {
                    inner.processes.remove(&related_session);
                    inner.process_trace_by_session.remove(&related_session);
                }
            }

            let trace = &mut inner.traces[index];
            trace.status = if failed { "failed" } else { "completed" }.into();
            trace.finished_at_ms = Some(finished_at_ms);
            trace.duration_ms = Some(finished_at_ms.saturating_sub(trace.started_at_ms));
            trace.response = sanitize_and_limit(response);
            trace.error = response
                .get("error")
                .map(sanitize_and_limit)
                .unwrap_or(Value::Null);
            if let Some(data) = structured {
                trace.operation_id = bounded_text(
                    data.get("operation_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    256,
                );
                if trace.process_session_id.is_empty() {
                    trace.process_session_id = bounded_text(
                        data.get("session_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                        256,
                    );
                }
            }
            trace.clone()
        };

        self.emit(
            if failed {
                "activity.failed"
            } else {
                "activity.completed"
            },
            Some(trace),
            None,
        );
        if let Some(process) = process_event {
            self.emit("activity.process-updated", None, Some(process));
        }
    }

    pub fn fail_trace(&self, trace_id: &str, message: &str) {
        let finished_at_ms = now_ms();
        let trace = self.update_trace(trace_id, |trace| {
            trace.status = "failed".into();
            trace.finished_at_ms = Some(finished_at_ms);
            trace.duration_ms = Some(finished_at_ms.saturating_sub(trace.started_at_ms));
            trace.error = json!({
                "message": bounded_text(&redact_text(message), 1024),
            });
        });
        if let Some(trace) = trace {
            self.emit("activity.failed", Some(trace), None);
        }
    }

    pub fn snapshot(&self, query: &ActivityQuery) -> ActivitySnapshot {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let total_matching = inner
            .traces
            .iter()
            .filter(|trace| matches_query(trace, query))
            .count();
        let active_requests = inner
            .traces
            .iter()
            .filter(|trace| trace.status == "running")
            .count();
        let traces = inner
            .traces
            .iter()
            .rev()
            .filter(|trace| matches_query(trace, query))
            .take(query.limit.clamp(1, MAX_TRACES))
            .cloned()
            .collect();
        let mut active_processes: Vec<_> = inner.processes.values().cloned().collect();
        active_processes.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
        ActivitySnapshot {
            traces,
            active_processes,
            active_requests,
            total_matching,
            retained: inner.traces.len(),
            max_entries: MAX_TRACES,
        }
    }

    pub fn get(&self, trace_id: &str) -> Option<ActivityTrace> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .traces
            .iter()
            .find(|trace| trace.trace_id == trace_id)
            .cloned()
    }

    pub fn clear(&self) -> usize {
        let removed = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let removed = inner.traces.len();
            inner.traces.clear();
            inner.processes.clear();
            inner.process_trace_by_session.clear();
            removed
        };
        self.emit("activity.cleared", None, None);
        removed
    }

    fn update_trace(
        &self,
        trace_id: &str,
        update: impl FnOnce(&mut ActivityTrace),
    ) -> Option<ActivityTrace> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let trace = inner
            .traces
            .iter_mut()
            .find(|trace| trace.trace_id == trace_id)?;
        update(trace);
        Some(trace.clone())
    }

    fn emit(&self, kind: &str, trace: Option<ActivityTrace>, process: Option<ActivityProcess>) {
        let _ = self.events.send(ActivityEvent {
            kind: kind.to_string(),
            trace,
            process,
        });
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

fn argument_session_id(body: &Value) -> Option<&str> {
    let arguments = body
        .get("params")
        .and_then(|params| params.get("arguments"))
        .and_then(Value::as_object)?;
    arguments
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            arguments
                .get("output_ref")
                .and_then(Value::as_str)
                .and_then(session_id_from_output_ref)
        })
}

fn session_id_from_output_ref(output_ref: &str) -> Option<&str> {
    let mut parts = output_ref.split(':');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("session"), Some(session_id), Some(_), None) if !session_id.is_empty() => {
            Some(session_id)
        }
        _ => None,
    }
}

fn structured_content(response: &Value) -> Option<&Value> {
    response
        .get("result")
        .and_then(|result| result.get("structuredContent"))
}

fn response_failed(response: &Value) -> bool {
    response.get("error").is_some()
        || response
            .get("result")
            .and_then(|result| result.get("isError"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || structured_content(response).is_some_and(|structured| {
            structured.get("ok").and_then(Value::as_bool) == Some(false)
                || structured.get("command_ok").and_then(Value::as_bool) == Some(false)
        })
}

fn process_status(data: &Value) -> Option<&str> {
    data.get("status")
        .or_else(|| data.get("termination_reason"))
        .and_then(Value::as_str)
}

fn is_terminal_process_status(status: &str) -> bool {
    matches!(
        status,
        "exited" | "failed" | "error" | "killed" | "timeout" | "spawn_failed"
    )
}

fn trace_command(trace: &ActivityTrace) -> String {
    bounded_text(
        trace
            .request
            .get("params")
            .and_then(|params| params.get("arguments"))
            .and_then(|arguments| arguments.get("cmd"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        1024,
    )
}

fn enforce_process_limit(inner: &mut ActivityInner) {
    while inner.processes.len() > MAX_PROCESSES {
        let Some(oldest_session) = inner
            .processes
            .iter()
            .min_by_key(|(_, process)| process.updated_at_ms)
            .map(|(session_id, _)| session_id.clone())
        else {
            break;
        };
        inner.processes.remove(&oldest_session);
        inner.process_trace_by_session.remove(&oldest_session);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
#[path = "activity_tests.rs"]
mod tests;
