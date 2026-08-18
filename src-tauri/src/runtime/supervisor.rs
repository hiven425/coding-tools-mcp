use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use tauri::async_runtime::JoinHandle;

use crate::actions;
use crate::error::AppResult;
use crate::mcp;
use crate::platform::platform;
use crate::runtime::port::{
    is_own_process, port_busy_message, try_reclaim_previous_macos_app_port,
    wait_for_port_free_blocking,
};
use crate::secret::SecretStore;
use crate::tools::policy::PolicySettings;
use crate::tunnel::{append_profile_log, cleanup_orphan_for_runtime, TunnelServiceKind};
use crate::workspace::{RuntimeStatusDto, WorkspaceProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceKind {
    Mcp,
    Actions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimePhase {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

struct RuntimeEntry {
    phase: RuntimePhase,
    shutdown: Option<mcp::ShutdownSender>,
    handle: Option<JoinHandle<()>>,
    error_message: Option<String>,
    public_state: String,
    public_error: Option<String>,
    started_at: Option<std::time::Instant>,
    missing_port_checks: u8,
}

#[derive(Default)]
pub struct RuntimeSupervisor {
    entries: HashMap<(String, ServiceKind), RuntimeEntry>,
}

impl RuntimeSupervisor {
    pub fn mcp_status(&self, profile: &WorkspaceProfile) -> RuntimeStatusDto {
        self.status(profile, ServiceKind::Mcp)
    }

    pub fn actions_status(&self, profile: &WorkspaceProfile) -> RuntimeStatusDto {
        self.status(profile, ServiceKind::Actions)
    }

    pub fn start_mcp(&mut self, profile: &WorkspaceProfile) -> AppResult<RuntimeStatusDto> {
        self.start(profile, ServiceKind::Mcp)
    }

    pub fn start_actions(&mut self, profile: &WorkspaceProfile) -> AppResult<RuntimeStatusDto> {
        self.start(profile, ServiceKind::Actions)
    }

    #[allow(dead_code)] // Kept for sync callers (tests / teardown helpers).
    pub fn restart_mcp(&mut self, profile: &WorkspaceProfile) -> AppResult<RuntimeStatusDto> {
        self.restart(profile, ServiceKind::Mcp)
    }

    #[allow(dead_code)] // Kept for sync callers (tests / teardown helpers).
    pub fn restart_actions(&mut self, profile: &WorkspaceProfile) -> AppResult<RuntimeStatusDto> {
        self.restart(profile, ServiceKind::Actions)
    }

    /// True when the service for this workspace is currently running.
    pub fn is_running(&self, workspace_id: &str, kind: ServiceKind) -> bool {
        matches!(
            self.entries
                .get(&(workspace_id.to_string(), kind))
                .map(|entry| &entry.phase),
            Some(RuntimePhase::Running)
        )
    }

    pub fn refresh_mcp(&mut self, profile: &WorkspaceProfile) {
        self.refresh(profile, ServiceKind::Mcp);
    }

    pub fn refresh_actions(&mut self, profile: &WorkspaceProfile) {
        self.refresh(profile, ServiceKind::Actions);
    }

    pub fn set_public_status(
        &mut self,
        workspace_id: &str,
        kind: ServiceKind,
        state: &str,
        error: Option<String>,
    ) {
        if let Some(entry) = self.entries.get_mut(&(workspace_id.to_string(), kind)) {
            entry.public_state = state.to_string();
            entry.public_error = error;
        }
    }

    pub fn drop_workspace(&mut self, profile: &WorkspaceProfile) {
        self.sync_stop_and_wait(profile, ServiceKind::Mcp);
        self.sync_stop_and_wait(profile, ServiceKind::Actions);
    }

    pub fn shutdown_all(&mut self, profiles: &[WorkspaceProfile]) {
        for profile in profiles {
            for kind in [ServiceKind::Mcp, ServiceKind::Actions] {
                if self.entries.contains_key(&(profile.id.clone(), kind)) {
                    self.sync_stop_and_wait(profile, kind);
                }
            }
        }
    }

    pub fn active_tunnel_service_keys(&self) -> HashSet<(String, TunnelServiceKind)> {
        self.entries
            .iter()
            .filter_map(|((workspace_id, kind), entry)| match entry.phase {
                RuntimePhase::Running | RuntimePhase::Starting => Some((
                    workspace_id.clone(),
                    match kind {
                        ServiceKind::Mcp => TunnelServiceKind::Mcp,
                        ServiceKind::Actions => TunnelServiceKind::Actions,
                    },
                )),
                _ => None,
            })
            .collect()
    }

    pub fn begin_stop(&mut self, workspace_id: &str, kind: ServiceKind) -> Option<JoinHandle<()>> {
        let key = (workspace_id.to_string(), kind);
        let entry = self.entries.get_mut(&key)?;

        entry.phase = RuntimePhase::Stopping;
        let shutdown = entry.shutdown.take();
        let handle = entry.handle.take();
        if let Some(shutdown) = shutdown {
            let _ = shutdown.send(());
        }
        handle
    }

    pub fn finish_stop(&mut self, workspace_id: &str, kind: ServiceKind) {
        self.entries.remove(&(workspace_id.to_string(), kind));
    }

    fn status(&self, profile: &WorkspaceProfile, kind: ServiceKind) -> RuntimeStatusDto {
        let key = (profile.id.clone(), kind);
        let phase = self
            .entries
            .get(&key)
            .map(|entry| entry.phase.clone())
            .unwrap_or(RuntimePhase::Stopped);
        let (public_state, public_error) = self
            .entries
            .get(&key)
            .map(|entry| (entry.public_state.clone(), entry.public_error.clone()))
            .unwrap_or_else(|| (initial_public_state(profile, kind).into(), None));

        let (local_endpoint, public_endpoint) = endpoints(profile, kind);
        let port = port_for(profile, kind);
        let service_label = service_label(kind);

        match phase {
            RuntimePhase::Running => {
                let public_message = public_error
                    .as_ref()
                    .map(|error| format!("公网隧道不可用：{error}"))
                    .unwrap_or_else(|| public_message_for(profile, kind));
                RuntimeStatusDto {
                    state: "running".into(),
                    pid: None,
                    local_message: format!("{service_label}正在监听 127.0.0.1:{port}"),
                    public_message,
                    local_endpoint,
                    public_endpoint,
                    public_state,
                    public_error,
                }
            }
            RuntimePhase::Starting => RuntimeStatusDto {
                state: "starting".into(),
                pid: None,
                local_message: format!("正在启动{service_label}端口 {port}"),
                public_message: "等待服务就绪".into(),
                local_endpoint,
                public_endpoint,
                public_state,
                public_error,
            },
            RuntimePhase::Stopping => RuntimeStatusDto {
                state: "stopping".into(),
                pid: None,
                local_message: "正在停止".into(),
                public_message: "正在停止".into(),
                local_endpoint,
                public_endpoint,
                public_state,
                public_error,
            },
            RuntimePhase::Error => {
                let message = self
                    .entries
                    .get(&key)
                    .and_then(|entry| entry.error_message.clone())
                    .unwrap_or_else(|| "运行失败".into());
                RuntimeStatusDto {
                    state: "error".into(),
                    pid: None,
                    local_message: message.clone(),
                    public_message: message,
                    local_endpoint,
                    public_endpoint,
                    public_state,
                    public_error,
                }
            }
            RuntimePhase::Stopped => RuntimeStatusDto {
                state: "stopped".into(),
                pid: None,
                local_message: "未启动".into(),
                public_message: "未知".into(),
                local_endpoint,
                public_endpoint,
                public_state,
                public_error,
            },
        }
    }

    fn start(
        &mut self,
        profile: &WorkspaceProfile,
        kind: ServiceKind,
    ) -> AppResult<RuntimeStatusDto> {
        let key = (profile.id.clone(), kind);
        if matches!(
            self.entries.get(&key).map(|e| &e.phase),
            Some(RuntimePhase::Running) | Some(RuntimePhase::Starting)
        ) {
            return Ok(self.status(profile, kind));
        }
        if matches!(
            self.entries.get(&key).map(|e| &e.phase),
            Some(RuntimePhase::Stopping)
        ) {
            return Err(crate::error::AppError::Message(format!(
                "{}正在停止，请稍后再试",
                service_label(kind).trim()
            )));
        }

        self.entries.insert(
            key.clone(),
            RuntimeEntry {
                phase: RuntimePhase::Starting,
                shutdown: None,
                handle: None,
                error_message: None,
                public_state: initial_public_state(profile, kind).into(),
                public_error: None,
                started_at: Some(std::time::Instant::now()),
                missing_port_checks: 0,
            },
        );

        let port = port_for(profile, kind);
        if let Some(pid) = platform().find_pid_listening_on_port(port)? {
            if is_own_process(pid) {
                wait_for_port_free_blocking(port, Duration::from_secs(3));
            }
            if try_reclaim_previous_macos_app_port(port) {
                // A previous source-built or installed instance of this macOS
                // app released the port; continue with the current listener.
            }
            if let Some(pid) = platform().find_pid_listening_on_port(port)? {
                self.entries.remove(&key);
                let message = port_busy_message(port, service_label(kind).trim(), pid);
                append_profile_log(
                    &profile.id,
                    stderr_log_name(kind),
                    &format!("[start] {message}"),
                );
                return Err(crate::error::AppError::Message(message));
            }
        }

        let spawn_result = match kind {
            ServiceKind::Mcp => {
                let use_shared = profile.auth.use_shared_secrets;
                let mut auth = profile.auth.clone();
                if use_shared {
                    if let Some(client_id) = SecretStore::get_shared("oauth_client_id")? {
                        auth.oauth_client_id = client_id;
                    }
                }
                // MCP OAuth matches legacy Python: client_secret is optional.
                // ChatGPT connectors use PKCE only and do not send client_secret.
                let oauth_client_secret = None;
                let oauth_password = if profile.auth.oauth_enabled() {
                    resolve_secret(&profile.id, "oauth_password", use_shared)?
                } else {
                    None
                };
                let oauth_token_secret = if profile.auth.oauth_enabled() {
                    resolve_secret(&profile.id, "oauth_token_secret", use_shared)?
                } else {
                    None
                };
                let transport_v2 = profile.tunnel.tunnel_type == "cloudflare"
                    && profile.tunnel.cloudflare_mode == "named"
                    && profile.tunnel.mcp_transport_v2;
                mcp::spawn_listener(
                    port,
                    PathBuf::from(&profile.path),
                    profile.id.clone(),
                    auth,
                    profile.effective_public_url(),
                    oauth_client_secret,
                    oauth_password,
                    oauth_token_secret,
                    profile.runtime.clone(),
                    transport_v2,
                )
            }
            ServiceKind::Actions => {
                let auth_type = profile.actions.auth_type.clone();
                let use_shared = profile.actions.use_shared_secrets;
                let api_key = if auth_type == "api_key" {
                    resolve_secret(&profile.id, "actions_api_key", use_shared)?
                } else {
                    None
                };
                let oauth_client_secret = if auth_type == "oauth" {
                    if use_shared {
                        resolve_secret(&profile.id, "actions_oauth_client_secret", true)?
                    } else {
                        Some(actions_oauth_secret(
                            &profile.id,
                            "actions_oauth_client_secret",
                        )?)
                    }
                } else {
                    None
                };
                let oauth_password = if auth_type == "oauth" {
                    if use_shared {
                        resolve_secret(&profile.id, "actions_oauth_password", true)?
                    } else {
                        Some(actions_oauth_secret(&profile.id, "actions_oauth_password")?)
                    }
                } else {
                    None
                };
                let oauth_token_secret = if auth_type == "oauth" {
                    if use_shared {
                        resolve_secret(&profile.id, "actions_oauth_token_secret", true)?
                    } else {
                        Some(actions_oauth_secret(
                            &profile.id,
                            "actions_oauth_token_secret",
                        )?)
                    }
                } else {
                    None
                };
                let public_base_url = profile.actions_public_base_url();
                let policy = PolicySettings::from_actions_config(&profile.actions);
                actions::spawn_listener(
                    &profile.id,
                    port,
                    PathBuf::from(&profile.path),
                    public_base_url,
                    auth_type,
                    api_key,
                    profile.actions.oauth_client_id.clone(),
                    oauth_client_secret,
                    oauth_password,
                    oauth_token_secret,
                    policy,
                )
            }
        };

        match spawn_result {
            Ok((shutdown, handle)) => {
                let (started_at, public_state, public_error) = self
                    .entries
                    .get(&key)
                    .map(|entry| {
                        (
                            entry.started_at,
                            entry.public_state.clone(),
                            entry.public_error.clone(),
                        )
                    })
                    .unwrap_or_else(|| {
                        (
                            Some(std::time::Instant::now()),
                            initial_public_state(profile, kind).into(),
                            None,
                        )
                    });
                self.entries.insert(
                    key,
                    RuntimeEntry {
                        phase: RuntimePhase::Running,
                        shutdown: Some(shutdown),
                        handle: Some(handle),
                        error_message: None,
                        public_state,
                        public_error,
                        started_at,
                        missing_port_checks: 0,
                    },
                );
            }
            Err(err) => {
                // spawn_listener can fail synchronously before the server task is
                // ever created (e.g. missing API key / OAuth secret). In that case
                // serve() never runs, so nothing writes to the stderr log and the
                // failure was previously invisible in the log viewer. Record it here.
                append_profile_log(
                    &profile.id,
                    stderr_log_name(kind),
                    &format!("[start] {}启动失败：{err}", service_label(kind).trim()),
                );
                self.entries.insert(
                    key,
                    RuntimeEntry {
                        phase: RuntimePhase::Error,
                        shutdown: None,
                        handle: None,
                        error_message: Some(err.to_string()),
                        public_state: "public-stopped".into(),
                        public_error: None,
                        started_at: None,
                        missing_port_checks: 0,
                    },
                );
            }
        }

        Ok(self.status(profile, kind))
    }

    /// Stop the current service (if running), then immediately start a new one.
    /// This is the canonical "restart" — used when the user regenerates a key or
    /// toggles the shared-secret switch, so the listener picks up the new value.
    ///
    /// stop_internal sends the graceful-shutdown signal but the OS port may not
    /// be freed instantly (the old listener's socket is closed on the tokio
    /// event loop). We retry `start` with a short back-off to smooth over this
    /// window.
    #[allow(dead_code)]
    fn restart(
        &mut self,
        profile: &WorkspaceProfile,
        kind: ServiceKind,
    ) -> AppResult<RuntimeStatusDto> {
        self.sync_stop_and_wait(profile, kind);
        self.start(profile, kind)
    }

    fn sync_stop_and_wait(&mut self, profile: &WorkspaceProfile, kind: ServiceKind) {
        let port = port_for(profile, kind);
        let handle = self.begin_stop(&profile.id, kind);
        if handle.is_some() {
            crate::runtime::port::await_listener_shutdown_blocking(handle, port);
        } else if platform()
            .find_pid_listening_on_port(port)
            .ok()
            .flatten()
            .is_some()
        {
            wait_for_port_free_blocking(port, Duration::from_secs(3));
        }
        self.finish_stop(&profile.id, kind);
    }

    fn refresh(&mut self, profile: &WorkspaceProfile, kind: ServiceKind) {
        let key = (profile.id.clone(), kind);
        let port = port_for(profile, kind);
        let mut should_cleanup_tunnel = false;
        if let Some(entry) = self.entries.get_mut(&key) {
            if entry.phase == RuntimePhase::Running {
                let listening = match platform().find_pid_listening_on_port(port) {
                    Ok(pid) => pid.is_some(),
                    Err(error) => {
                        append_profile_log(
                            &profile.id,
                            stderr_log_name(kind),
                            &format!("[refresh] 检查端口 {port} 失败，保留当前线路：{error}"),
                        );
                        return;
                    }
                };
                if should_mark_runtime_error(entry, listening) {
                    if let Some(handle) = entry.handle.take() {
                        handle.abort();
                        tauri::async_runtime::spawn(async move {
                            let _ = handle.await;
                        });
                    }
                    entry.shutdown.take();
                    let occupied_by_self = platform()
                        .find_pid_listening_on_port(port)
                        .ok()
                        .flatten()
                        .map(is_own_process)
                        .unwrap_or(false);
                    let message = if occupied_by_self {
                        format!(
                            "{}端口 {} 未能成功启动，可能仍被本应用上一次服务占用，请先停止后再试",
                            service_label(kind).trim(),
                            port
                        )
                    } else {
                        format!(
                            "{}端口 {} 未能成功启动，可能已被其他程序占用",
                            service_label(kind).trim(),
                            port
                        )
                    };
                    entry.phase = RuntimePhase::Error;
                    entry.error_message = Some(message.clone());
                    entry.public_state = "public-degraded".into();
                    entry.public_error = Some(message);
                    entry.started_at = None;
                    should_cleanup_tunnel = true;
                }
            }
        }

        // 状态查询本身不能改变其他工作区的隧道集合。只有本次刷新确认了
        // 一个原本 Running 的 runtime 已经进入 Error，才清理它对应的孤儿线路。
        // 之前无条件调用 cleanup_orphan 会把启动时的瞬时端口检测失败误认为
        // 孤儿 runtime，删除 route 后重启唯一的 frpc，导致其他工作区公网线路消失。
        if !should_cleanup_tunnel {
            return;
        }

        let tunnel_kind = match kind {
            ServiceKind::Mcp => TunnelServiceKind::Mcp,
            ServiceKind::Actions => TunnelServiceKind::Actions,
        };

        let profile = profile.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = cleanup_orphan_for_runtime(&profile, tunnel_kind, false).await {
                append_profile_log(
                    &profile.id,
                    stderr_log_name(kind),
                    &format!("[refresh] 清理失效隧道失败：{error}"),
                );
            }
        });
    }
}

fn should_mark_runtime_error(entry: &mut RuntimeEntry, listening: bool) -> bool {
    if entry.phase != RuntimePhase::Running {
        return false;
    }
    if listening {
        entry.missing_port_checks = 0;
        return false;
    }

    entry.missing_port_checks = entry.missing_port_checks.saturating_add(1);
    entry.missing_port_checks >= 3
        && entry
            .started_at
            .map(|started| started.elapsed() > Duration::from_millis(200))
            .unwrap_or(true)
}

fn port_for(profile: &WorkspaceProfile, kind: ServiceKind) -> u16 {
    match kind {
        ServiceKind::Mcp => profile.runtime.local_port,
        ServiceKind::Actions => profile.actions.local_port,
    }
}

fn endpoints(profile: &WorkspaceProfile, kind: ServiceKind) -> (String, String) {
    match kind {
        ServiceKind::Mcp => (profile.local_endpoint(), profile.public_endpoint()),
        ServiceKind::Actions => (
            profile.actions_local_base_url(),
            profile.actions_openapi_url(),
        ),
    }
}

fn public_message_for(profile: &WorkspaceProfile, kind: ServiceKind) -> String {
    match kind {
        ServiceKind::Mcp => profile.effective_public_url(),
        ServiceKind::Actions => profile.actions_effective_public_url(),
    }
}

fn initial_public_state(profile: &WorkspaceProfile, kind: ServiceKind) -> &'static str {
    let tunnel_type = match kind {
        ServiceKind::Mcp => profile.tunnel.tunnel_type.as_str(),
        ServiceKind::Actions => profile.actions.tunnel_type.as_str(),
    };
    if tunnel_type.is_empty() || tunnel_type == "none" {
        "not-configured"
    } else {
        "public-starting"
    }
}

fn service_label(kind: ServiceKind) -> &'static str {
    match kind {
        ServiceKind::Mcp => "本地 MCP ",
        ServiceKind::Actions => "本地 Actions ",
    }
}

fn stderr_log_name(kind: ServiceKind) -> &'static str {
    match kind {
        ServiceKind::Mcp => "stderr.log",
        ServiceKind::Actions => "actions-stderr.log",
    }
}

/// Resolve a secret from the shared pool or per-workspace keyring.
fn resolve_secret(profile_id: &str, key: &str, use_shared: bool) -> AppResult<Option<String>> {
    if use_shared {
        SecretStore::get_shared(key)
    } else {
        SecretStore::get(profile_id, key)
    }
}

fn actions_oauth_secret(profile_id: &str, key: &str) -> AppResult<String> {
    match SecretStore::get(profile_id, key)? {
        Some(value) if !value.is_empty() => Ok(value),
        _ => SecretStore::regenerate(profile_id, key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(phase: RuntimePhase, started_at: Option<std::time::Instant>) -> RuntimeEntry {
        RuntimeEntry {
            phase,
            shutdown: None,
            handle: None,
            error_message: None,
            public_state: "public-starting".into(),
            public_error: None,
            started_at,
            missing_port_checks: 0,
        }
    }

    #[test]
    fn refresh_does_not_cleanup_a_running_runtime_that_is_listening() {
        let mut runtime = entry(RuntimePhase::Running, Some(std::time::Instant::now()));
        assert!(!should_mark_runtime_error(&mut runtime, true));
    }

    #[test]
    fn refresh_does_not_cleanup_a_starting_runtime() {
        let mut runtime = entry(RuntimePhase::Starting, None);
        assert!(!should_mark_runtime_error(&mut runtime, false));
    }

    #[test]
    fn refresh_cleans_up_only_after_running_runtime_is_confirmed_missing() {
        let mut runtime = entry(
            RuntimePhase::Running,
            Some(std::time::Instant::now() - Duration::from_secs(1)),
        );
        assert!(!should_mark_runtime_error(&mut runtime, false));
        assert!(!should_mark_runtime_error(&mut runtime, false));
        assert!(should_mark_runtime_error(&mut runtime, false));
    }

    #[test]
    fn a_recovered_port_clears_missing_port_checks() {
        let mut runtime = entry(
            RuntimePhase::Running,
            Some(std::time::Instant::now() - Duration::from_secs(1)),
        );
        assert!(!should_mark_runtime_error(&mut runtime, false));
        assert!(!should_mark_runtime_error(&mut runtime, true));
        assert!(!should_mark_runtime_error(&mut runtime, false));
    }

    #[test]
    fn public_failure_does_not_mark_a_running_listener_as_failed() {
        let profile = WorkspaceProfile::new("C:/workspace/demo".into(), Some("demo".into()));
        let key = (profile.id.clone(), ServiceKind::Mcp);
        let mut supervisor = RuntimeSupervisor::default();
        supervisor.entries.insert(
            key,
            entry(RuntimePhase::Running, Some(std::time::Instant::now())),
        );

        supervisor.set_public_status(
            &profile.id,
            ServiceKind::Mcp,
            "public-error",
            Some("edge registration failed".into()),
        );

        let status = supervisor.mcp_status(&profile);
        assert_eq!(status.state, "running");
        assert_eq!(status.public_state, "public-error");
        assert_eq!(status.public_error.as_deref(), Some("edge registration failed"));
    }

    #[test]
    fn stopped_runtime_without_tunnel_reports_not_configured() {
        let mut profile = WorkspaceProfile::new("C:/workspace/demo".into(), Some("demo".into()));
        profile.tunnel.tunnel_type = "none".into();
        let supervisor = RuntimeSupervisor::default();

        let status = supervisor.mcp_status(&profile);

        assert_eq!(status.state, "stopped");
        assert_eq!(status.public_state, "not-configured");
        assert!(status.public_error.is_none());
    }
}
