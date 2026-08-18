use tauri::State;

use std::sync::LazyLock;
use std::time::Duration;

use tokio::sync::Mutex as AsyncMutex;

use crate::app_state::AppState;
use crate::error::{AppError, AppResult};
use crate::platform::platform;
use crate::runtime::{
    await_listener_shutdown, port_busy_message, try_reclaim_previous_macos_app_port,
    wait_for_port_free, ServiceKind,
};
use crate::tunnel::{
    append_profile_log, maybe_start_for_runtime, stop_for_runtime, sync_managed_runtime_routes,
    TunnelServiceKind,
};
use crate::workspace::resources::{validate_service_start, WorkspaceService};
use crate::workspace::RuntimeStatusDto;

/// Serialize MCP/Actions restarts so secret-save and form-save cannot tear down
/// the same listener concurrently (that race could abort the process on Windows).
static RESTART_GATE: LazyLock<AsyncMutex<()>> = LazyLock::new(|| AsyncMutex::new(()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpTunnelStop {
    Stop,
    Preserve,
}

fn profile_by_id(state: &AppState, id: &str) -> AppResult<crate::workspace::WorkspaceProfile> {
    state.with_workspaces(|store| {
        store
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::Message(format!("workspace not found: {id}")))
    })
}

fn validate_start_resources(
    state: &AppState,
    id: &str,
    service: WorkspaceService,
) -> AppResult<()> {
    state.with_workspaces(|store| validate_service_start(store.list(), id, service))
}

fn persist_tunnel_url(
    state: &AppState,
    id: &str,
    kind: TunnelServiceKind,
    url: &str,
) -> AppResult<()> {
    if url.is_empty() {
        return Ok(());
    }

    state.with_workspaces(|store| {
        let Some(mut profile) = store.get(id).cloned() else {
            return Ok(());
        };

        match kind {
            TunnelServiceKind::Mcp => profile.tunnel.public_url = url.to_string(),
            TunnelServiceKind::Actions => profile.actions.public_url = url.to_string(),
        }

        store.update(profile)?;
        Ok(())
    })
}

async fn sync_tunnel_routes_from_runtime(state: &AppState) -> AppResult<()> {
    let active_keys = state.with_runtime(|runtime| Ok(runtime.active_tunnel_service_keys()))?;
    sync_managed_runtime_routes(active_keys).await
}

#[allow(clippy::collapsible_if)]
async fn ensure_port_available(port: u16, service_label: &str) -> AppResult<()> {
    let Some(pid) = platform().find_pid_listening_on_port(port)? else {
        return Ok(());
    };

    if crate::runtime::is_own_process(pid) {
        if wait_for_port_free(port, Duration::from_secs(3)).await {
            return Ok(());
        }
    }

    if try_reclaim_previous_macos_app_port(port) {
        return Ok(());
    }

    if let Some(pid) = platform().find_pid_listening_on_port(port)? {
        return Err(AppError::Message(port_busy_message(
            port,
            service_label,
            pid,
        )));
    }

    Ok(())
}

async fn stop_mcp_service(
    state: &AppState,
    id: &str,
    tunnel_stop: McpTunnelStop,
) -> AppResult<RuntimeStatusDto> {
    let profile = profile_by_id(state, id)?;
    let port = profile.runtime.local_port;
    let handle = state.with_runtime(|runtime| Ok(runtime.begin_stop(id, ServiceKind::Mcp)))?;
    await_listener_shutdown(handle, port).await;
    state.with_runtime(|runtime| {
        runtime.finish_stop(id, ServiceKind::Mcp);
        Ok(runtime.mcp_status(&profile))
    })?;
    if tunnel_stop == McpTunnelStop::Stop {
        stop_for_runtime(&profile, TunnelServiceKind::Mcp).await?;
        sync_tunnel_routes_from_runtime(state).await?;
    }
    state.with_runtime(|runtime| Ok(runtime.mcp_status(&profile)))
}

async fn runtime_status_with_tunnel(
    state: &AppState,
    profile: &crate::workspace::WorkspaceProfile,
    service: ServiceKind,
    tunnel_kind: TunnelServiceKind,
) -> AppResult<RuntimeStatusDto> {
    let mut status = state.with_runtime(|runtime| {
        match service {
            ServiceKind::Mcp => runtime.refresh_mcp(profile),
            ServiceKind::Actions => runtime.refresh_actions(profile),
        }
        Ok(match service {
            ServiceKind::Mcp => runtime.mcp_status(profile),
            ServiceKind::Actions => runtime.actions_status(profile),
        })
    })?;
    let settings = state.with_settings(|store| Ok(store.settings()))?;
    let tunnel_status =
        crate::tunnel::supervisor()
            .lock()
            .await
            .status(profile, tunnel_kind, &settings);

    if tunnel_status.provider_state != "public-stopped"
        || (status.public_error.is_none() && status.public_state != "not-configured")
    {
        status.public_state = tunnel_status.provider_state;
        status.public_error = tunnel_status.last_error;
        if let Some(error) = status.public_error.as_ref() {
            status.public_message = format!("公网隧道不可用：{error}");
        } else if !tunnel_status.public_url.is_empty() {
            status.public_message = tunnel_status.public_url;
        }
    }

    state.with_runtime(|runtime| {
        runtime.set_public_status(
            &profile.id,
            service,
            &status.public_state,
            status.public_error.clone(),
        );
        Ok(())
    })?;
    Ok(status)
}

async fn start_mcp_service(state: &AppState, id: &str) -> AppResult<RuntimeStatusDto> {
    validate_start_resources(state, id, WorkspaceService::Mcp)?;
    let profile = profile_by_id(state, id)?;
    ensure_port_available(profile.runtime.local_port, "本地 MCP").await?;
    let activity = state.activity.clone();
    state.with_runtime(|runtime| runtime.start_mcp(&profile, activity))?;
    sync_tunnel_routes_from_runtime(state).await?;

    match maybe_start_for_runtime(&profile, TunnelServiceKind::Mcp).await {
        Ok(Some(url)) => {
            persist_tunnel_url(state, id, TunnelServiceKind::Mcp, &url)?;
            state.with_runtime(|runtime| {
                runtime.set_public_status(id, ServiceKind::Mcp, "public-ready", None);
                Ok(())
            })?;
        }
        Ok(None) => {
            state.with_runtime(|runtime| {
                runtime.set_public_status(id, ServiceKind::Mcp, "not-configured", None);
                Ok(())
            })?;
        }
        Err(error) => {
            let message = error.to_string();
            append_profile_log(
                id,
                "cloudflared.log",
                &format!("[auto-start] MCP 公网隧道启动失败：{message}"),
            );
            state.with_runtime(|runtime| {
                runtime.set_public_status(id, ServiceKind::Mcp, "public-error", Some(message));
                Ok(())
            })?;
        }
    }

    let profile = profile_by_id(state, id)?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    runtime_status_with_tunnel(state, &profile, ServiceKind::Mcp, TunnelServiceKind::Mcp).await
}

async fn stop_actions_service(state: &AppState, id: &str) -> AppResult<RuntimeStatusDto> {
    let profile = profile_by_id(state, id)?;
    let port = profile.actions.local_port;
    let handle = state.with_runtime(|runtime| Ok(runtime.begin_stop(id, ServiceKind::Actions)))?;
    await_listener_shutdown(handle, port).await;
    state.with_runtime(|runtime| {
        runtime.finish_stop(id, ServiceKind::Actions);
        Ok(runtime.actions_status(&profile))
    })?;
    stop_for_runtime(&profile, TunnelServiceKind::Actions).await?;
    sync_tunnel_routes_from_runtime(state).await?;
    state.with_runtime(|runtime| Ok(runtime.actions_status(&profile)))
}

async fn start_actions_service(state: &AppState, id: &str) -> AppResult<RuntimeStatusDto> {
    validate_start_resources(state, id, WorkspaceService::Actions)?;
    let profile = profile_by_id(state, id)?;
    ensure_port_available(profile.actions.local_port, "本地 Actions").await?;
    state.with_runtime(|runtime| runtime.start_actions(&profile))?;
    sync_tunnel_routes_from_runtime(state).await?;

    match maybe_start_for_runtime(&profile, TunnelServiceKind::Actions).await {
        Ok(Some(url)) => {
            persist_tunnel_url(state, id, TunnelServiceKind::Actions, &url)?;
            state.with_runtime(|runtime| {
                runtime.set_public_status(id, ServiceKind::Actions, "public-ready", None);
                Ok(())
            })?;
        }
        Ok(None) => {
            state.with_runtime(|runtime| {
                runtime.set_public_status(id, ServiceKind::Actions, "not-configured", None);
                Ok(())
            })?;
        }
        Err(error) => {
            let message = error.to_string();
            append_profile_log(
                id,
                "actions-cloudflared.log",
                &format!("[auto-start] Actions 公网隧道启动失败：{message}"),
            );
            state.with_runtime(|runtime| {
                runtime.set_public_status(id, ServiceKind::Actions, "public-error", Some(message));
                Ok(())
            })?;
        }
    }

    let profile = profile_by_id(state, id)?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    runtime_status_with_tunnel(
        state,
        &profile,
        ServiceKind::Actions,
        TunnelServiceKind::Actions,
    )
    .await
}

/// Async stop→start for MCP. Used by the Tauri command and secret-change hooks.
pub(crate) async fn restart_mcp_by_id(state: &AppState, id: &str) -> AppResult<RuntimeStatusDto> {
    let _guard = RESTART_GATE.lock().await;
    let was_running = state.with_runtime(|runtime| Ok(runtime.is_running(id, ServiceKind::Mcp)))?;
    if was_running {
        let profile = profile_by_id(state, id)?;
        let tunnel_stop = if profile.tunnel.tunnel_type == "cloudflare"
            && profile.tunnel.cloudflare_mode == "named"
            && profile.tunnel.mcp_transport_v2
        {
            McpTunnelStop::Preserve
        } else {
            McpTunnelStop::Stop
        };
        let _ = stop_mcp_service(state, id, tunnel_stop).await?;
    }
    start_mcp_service(state, id).await
}

/// Async stop→start for Actions. Used by the Tauri command and secret-change hooks.
pub(crate) async fn restart_actions_by_id(
    state: &AppState,
    id: &str,
) -> AppResult<RuntimeStatusDto> {
    let _guard = RESTART_GATE.lock().await;
    let was_running =
        state.with_runtime(|runtime| Ok(runtime.is_running(id, ServiceKind::Actions)))?;
    if was_running {
        let _ = stop_actions_service(state, id).await?;
    }
    start_actions_service(state, id).await
}

#[tauri::command]
pub async fn start_runtime(state: State<'_, AppState>, id: String) -> AppResult<RuntimeStatusDto> {
    start_mcp_service(&state, &id).await
}

#[tauri::command]
pub async fn stop_runtime(state: State<'_, AppState>, id: String) -> AppResult<RuntimeStatusDto> {
    stop_mcp_service(&state, &id, McpTunnelStop::Stop).await
}

#[tauri::command]
pub async fn get_runtime_status(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<RuntimeStatusDto> {
    let profile = profile_by_id(&state, &id)?;
    runtime_status_with_tunnel(&state, &profile, ServiceKind::Mcp, TunnelServiceKind::Mcp).await
}

#[tauri::command]
pub async fn start_actions_runtime(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<RuntimeStatusDto> {
    start_actions_service(&state, &id).await
}

#[tauri::command]
pub async fn stop_actions_runtime(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<RuntimeStatusDto> {
    stop_actions_service(&state, &id).await
}

#[tauri::command]
pub async fn get_actions_runtime_status(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<RuntimeStatusDto> {
    let profile = profile_by_id(&state, &id)?;
    runtime_status_with_tunnel(
        &state,
        &profile,
        ServiceKind::Actions,
        TunnelServiceKind::Actions,
    )
    .await
}

#[tauri::command]
pub async fn restart_runtime(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<RuntimeStatusDto> {
    restart_mcp_by_id(&state, &id).await
}

#[tauri::command]
pub async fn restart_actions_runtime(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<RuntimeStatusDto> {
    restart_actions_by_id(&state, &id).await
}
