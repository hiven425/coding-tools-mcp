use serde_json::{json, Value};
use tokio::sync::broadcast::error::TryRecvError;

use super::sanitize::{MAX_VALUE_BYTES, REDACTED};
use super::{ActivityQuery, ActivityStore, MAX_PROCESSES, MAX_TRACES};

fn call(tool: &str, arguments: Value) -> Value {
    json!({
        "id": tool,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}
    })
}

fn result(data: Value) -> Value {
    json!({"result": {"structuredContent": data}})
}

fn start_process(store: &ActivityStore, session_id: &str, command: &str) -> String {
    let trace_id = store.begin_trace(
        "workspace-1",
        "Demo",
        "/mcp",
        &call("exec_command", json!({"cmd": command})),
    );
    store.complete_trace(
        &trace_id,
        &result(json!({
            "ok": true,
            "status": "running",
            "termination_reason": "running",
            "session_id": session_id,
            "operation_id": format!("operation-{session_id}")
        })),
    );
    trace_id
}

#[test]
fn trace_payloads_are_redacted_before_storage_and_broadcast() {
    let store = ActivityStore::new();
    let mut events = store.subscribe();
    let request = call(
        "exec_command",
        json!({
            "password": "plain-password",
            "nested": {"api_key": "plain-key", "raw_user_input": "private prompt"},
            "cmd": "runner --token plain-token --api-key=plain-api PASSWORD=plain-env Bearer plain-bearer"
        }),
    );
    let trace_id = store.begin_trace("workspace-1", "Demo", "/mcp", &request);
    let response = result(json!({
        "authorization": "Bearer response-secret",
        "command": "tool COOKIE=session-secret --secret hidden",
        "status": "running",
        "session_id": "session-opaque-value"
    }));
    store.complete_trace(&trace_id, &response);

    let trace = store.get(&trace_id).expect("stored trace");
    let snapshot = store.snapshot(&ActivityQuery::default());
    let mut encoded = serde_json::to_string(&trace).expect("serialize trace");
    encoded.push_str(&serde_json::to_string(&snapshot.active_processes).expect("processes"));
    while let Ok(event) = events.try_recv() {
        encoded.push_str(&serde_json::to_string(&event).expect("event"));
    }
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
        store.begin_trace(
            "workspace-1",
            "Demo",
            "/mcp",
            &call(
                "read_file",
                json!({"content": "x".repeat(40_000), "id": id}),
            ),
        );
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
    store.begin_trace("one", "Alpha", "/mcp", &call("read_file", json!({})));
    let second_id = store.begin_trace("two", "Beta", "/mcp", &call("exec_command", json!({})));
    store.fail_trace(&second_id, "worker failed");

    let snapshot = store.snapshot(&ActivityQuery {
        workspace: "beta".into(),
        tool: "exec".into(),
        status: "failed".into(),
        limit: 20,
    });
    assert_eq!(snapshot.total_matching, 1);
    assert_eq!(snapshot.active_requests, 1);
    assert_eq!(snapshot.traces[0].trace_id, second_id);
    assert_eq!(store.clear(), 2);
    assert_eq!(store.clear(), 0);
}

#[test]
fn background_process_links_follow_up_traces_and_is_removed_on_exit() {
    let store = ActivityStore::new();
    let parent_trace_id = start_process(&store, "session-1", "npm run dev");
    let active = store.snapshot(&ActivityQuery::default()).active_processes;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].trace_id, parent_trace_id);
    assert_eq!(active[0].operation_id, "operation-session-1");

    let read_trace_id = store.begin_trace(
        "workspace-1",
        "Demo",
        "/mcp",
        &call(
            "read_output",
            json!({"output_ref": "session:session-1:stdout"}),
        ),
    );
    let read_trace = store.get(&read_trace_id).expect("read trace");
    assert_eq!(read_trace.process_session_id, "session-1");
    assert_eq!(read_trace.parent_trace_id, parent_trace_id);
    store.complete_trace(&read_trace_id, &result(json!({"ok": true})));

    let kill_trace_id = store.begin_trace(
        "workspace-1",
        "Demo",
        "/mcp",
        &call("kill_session", json!({"session_id": "session-1"})),
    );
    store.complete_trace(
        &kill_trace_id,
        &result(json!({
            "ok": true,
            "session_id": "session-1",
            "status": "terminating",
            "termination_reason": "killed"
        })),
    );
    assert_eq!(
        store.snapshot(&ActivityQuery::default()).active_processes[0].status,
        "terminating"
    );

    let write_trace_id = store.begin_trace(
        "workspace-1",
        "Demo",
        "/mcp",
        &call(
            "write_stdin",
            json!({"session_id": "session-1", "chars": ""}),
        ),
    );
    store.complete_trace(
        &write_trace_id,
        &result(json!({
            "ok": true,
            "session_id": "session-1",
            "status": "exited",
            "termination_reason": "exited",
            "exit_code": 0
        })),
    );
    assert!(store
        .snapshot(&ActivityQuery::default())
        .active_processes
        .is_empty());
}

#[test]
fn active_processes_are_bounded_and_clear_removes_them() {
    let store = ActivityStore::new();
    for index in 0..MAX_PROCESSES + 1 {
        start_process(&store, &format!("session-{index}"), "sleep 30");
    }
    assert_eq!(
        store
            .snapshot(&ActivityQuery::default())
            .active_processes
            .len(),
        MAX_PROCESSES
    );
    assert_eq!(store.clear(), MAX_PROCESSES + 1);
    assert!(store
        .snapshot(&ActivityQuery::default())
        .active_processes
        .is_empty());
}

#[test]
fn activity_events_follow_state_transitions() {
    let store = ActivityStore::new();
    let mut events = store.subscribe();
    let trace_id = start_process(&store, "session-1", "npm run dev");
    store.clear();

    let kinds = std::iter::from_fn(|| match events.try_recv() {
        Ok(event) => Some(event.kind),
        Err(TryRecvError::Empty | TryRecvError::Closed) => None,
        Err(TryRecvError::Lagged(_)) => panic!("test receiver lagged"),
    })
    .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            "activity.started",
            "activity.completed",
            "activity.process-updated",
            "activity.cleared"
        ]
    );
    assert!(store.get(&trace_id).is_none());
}
