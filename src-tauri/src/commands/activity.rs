use tauri::State;

use crate::activity::{ActivityQuery, ActivitySnapshot, ActivityTrace};
use crate::app_state::AppState;

#[tauri::command]
pub fn list_activity(
    state: State<'_, AppState>,
    workspace: Option<String>,
    tool: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
) -> ActivitySnapshot {
    state.activity.snapshot(&ActivityQuery {
        workspace: workspace.unwrap_or_default(),
        tool: tool.unwrap_or_default(),
        status: status.unwrap_or_default(),
        limit: limit.unwrap_or(200),
    })
}

#[tauri::command]
pub fn get_activity(state: State<'_, AppState>, trace_id: String) -> Option<ActivityTrace> {
    state.activity.get(&trace_id)
}

#[tauri::command]
pub fn clear_activity(state: State<'_, AppState>) -> usize {
    state.activity.clear()
}
