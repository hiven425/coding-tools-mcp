use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::time;

use crate::error::{AppError, AppResult};
use crate::platform::platform;
use crate::settings::ProxyConfig;

use super::logs::{
    format_cloudflared_log_line, sanitize_log_line, timestamped_log_line,
};

const READY_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
const DIAGNOSTIC_LOG_TAIL_BYTES: u64 = 16 * 1024;

/// Handle to a supervised `cloudflared` child process.
pub struct CloudflareTunnelHandle {
    pub child: Child,
    pub public_url: String,
    pub pid: Option<u32>,
}

pub fn resolve_cloudflared() -> AppResult<PathBuf> {
    cached_cloudflared_path()
        .filter(|path| path.is_file())
        .or_else(|| {
            platform()
                .cloudflared_candidates()
                .into_iter()
                .find(|path| path.is_file())
        })
        .ok_or_else(|| {
            AppError::Message(
                "未找到 cloudflared。请到「软件管理」安装，或自行安装 Cloudflare Tunnel CLI。\n\
                 Windows 可执行：winget install Cloudflare.cloudflared"
                    .into(),
            )
        })
}

/// Path where the app caches a self-managed cloudflared binary.
pub(crate) fn cached_cloudflared_path() -> Option<PathBuf> {
    platform()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("bin").join(cloudflared_binary_name()))
}

pub(crate) fn cloudflared_binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "cloudflared.exe"
    }
    #[cfg(not(windows))]
    {
        "cloudflared"
    }
}

/// GitHub release asset name for the current platform.
fn cloudflared_release_asset() -> AppResult<&'static str> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        Ok("cloudflared-windows-amd64.exe")
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        Ok("cloudflared-windows-arm64.exe")
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Ok("cloudflared-linux-amd64")
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        Ok("cloudflared-linux-arm64")
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        Ok("cloudflared-darwin-amd64.tgz")
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Ok("cloudflared-darwin-arm64.tgz")
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
    )))]
    {
        Err(AppError::Message(
            "当前平台暂不支持自动下载 cloudflared。".into(),
        ))
    }
}

/// Latest cloudflared release. Pinned for reproducibility; bump as needed.
const CLOUDFLARED_VERSION: &str = "2026.8.2";

fn cloudflared_release_sha256(asset: &str) -> AppResult<&'static str> {
    match asset {
        "cloudflared-windows-amd64.exe" => {
            Ok("c29eee2b121f5436a642eed69fd9767da7e7b8c510fa50aaa130337f931357b5")
        }
        "cloudflared-linux-amd64" => {
            Ok("fcfb02b575a52ca1af2e3267af4e1517bcdeb30ac48c834c69abaed3c0576ad2")
        }
        "cloudflared-linux-arm64" => {
            Ok("7747d94570fb390cf47dcb4f9555c193c6355cda9793f0d878d9049e5d6a7790")
        }
        "cloudflared-darwin-amd64.tgz" => {
            Ok("f1727723c586500e2092368ae21871b3df7ddfd2cb097f22d81bee4a9c458bb4")
        }
        "cloudflared-darwin-arm64.tgz" => {
            Ok("9042c2c5d8b2de78e60f313d5fb31b6c5c1cebde787a3caf1f2c9588084ac442")
        }
        other => Err(AppError::Message(format!(
            "cloudflared {CLOUDFLARED_VERSION} 没有受信任的 {other} 校验信息，已拒绝自动安装。"
        ))),
    }
}

fn verify_cloudflared_checksum(bytes: &[u8], expected_sha256: &str) -> AppResult<()> {
    let actual_sha256 = format!("{:x}", Sha256::digest(bytes));
    if actual_sha256 == expected_sha256 {
        return Ok(());
    }
    Err(AppError::Message(format!(
        "cloudflared 下载文件 SHA-256 校验失败，已保留现有版本：expected={expected_sha256} actual={actual_sha256}"
    )))
}

fn cloudflared_version_is_current(output: &str) -> bool {
    output.lines().any(|line| {
        line.trim()
            .strip_prefix("cloudflared version ")
            .and_then(|rest| rest.split_whitespace().next())
            == Some(CLOUDFLARED_VERSION)
    })
}

fn cached_cloudflared_is_current(path: &Path) -> bool {
    std::process::Command::new(path)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| cloudflared_version_is_current(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or(false)
}

async fn ensure_managed_cloudflared_current() -> AppResult<()> {
    let Some(path) = cached_cloudflared_path().filter(|path| path.is_file()) else {
        return Ok(());
    };
    if cached_cloudflared_is_current(&path) {
        return Ok(());
    }

    download_cloudflared_to_cache().await?;
    Ok(())
}

fn unique_sibling_path(dest: &Path, suffix: &str) -> PathBuf {
    let file_name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cloudflared");
    dest.with_file_name(format!(
        ".{file_name}.{}.{suffix}",
        uuid::Uuid::new_v4().simple()
    ))
}

fn replace_cloudflared_file(staged: &Path, dest: &Path) -> AppResult<()> {
    if !dest.is_file() {
        std::fs::rename(staged, dest)?;
        return Ok(());
    }

    let backup = unique_sibling_path(dest, "backup");
    std::fs::rename(dest, &backup).map_err(|err| {
        AppError::Message(format!("备份旧版 cloudflared 失败，未执行升级: {err}"))
    })?;
    if let Err(install_err) = std::fs::rename(staged, dest) {
        return match std::fs::rename(&backup, dest) {
            Ok(()) => Err(AppError::Message(format!(
                "替换 cloudflared 失败，已恢复旧版本: {install_err}"
            ))),
            Err(restore_err) => Err(AppError::Message(format!(
                "替换 cloudflared 失败，且恢复旧版本失败: {install_err}; restore={restore_err}; backup={}",
                backup.display()
            ))),
        };
    }

    let _ = std::fs::remove_file(backup);
    Ok(())
}

/// Download cloudflared into the app cache `bin/` directory, honoring the
/// configured mirror + proxy. Windows/Linux assets are raw binaries; macOS
/// assets are `.tgz` archives that need extraction.
pub(crate) async fn download_cloudflared_to_cache() -> AppResult<PathBuf> {
    let settings = crate::settings::AppSettings::load_or_default();
    let asset = cloudflared_release_asset()?;
    let expected_sha256 = cloudflared_release_sha256(asset)?;
    let url = format!(
        "https://github.com/cloudflare/cloudflared/releases/download/{CLOUDFLARED_VERSION}/{asset}"
    );
    let dest = cached_cloudflared_path()
        .ok_or_else(|| AppError::Message("无法解析缓存目录。".into()))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let bytes = crate::tunnel::download::download_release_asset(&settings, &url, "cloudflared").await?;
    verify_cloudflared_checksum(&bytes, expected_sha256)?;
    let staged = unique_sibling_path(&dest, "download");

    let install_result = if asset.ends_with(".tgz") {
        extract_cloudflared_from_tar_gz(&bytes, &staged)
    } else {
        std::fs::write(&staged, &bytes).map_err(AppError::from)
    };
    if let Err(err) = install_result {
        let _ = std::fs::remove_file(&staged);
        return Err(err);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&staged) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&staged, perms);
        }
    }

    if !cached_cloudflared_is_current(&staged) {
        let _ = std::fs::remove_file(&staged);
        return Err(AppError::Message(format!(
            "下载的 cloudflared 版本不是预期的 {CLOUDFLARED_VERSION}，已保留现有版本。"
        )));
    }
    // Concurrent tunnel starts may have completed the same upgrade while this
    // download was in flight. Avoid replacing a now-current, possibly running binary.
    if cached_cloudflared_is_current(&dest) {
        let _ = std::fs::remove_file(&staged);
        return Ok(dest);
    }

    let replace_result = replace_cloudflared_file(&staged, &dest);
    if replace_result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    replace_result?;
    Ok(dest)
}

#[cfg(target_os = "macos")]
fn extract_cloudflared_from_tar_gz(bytes: &[u8], dest: &Path) -> AppResult<()> {
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .map_err(|err| AppError::Message(format!("解压 cloudflared 安装包失败: {err}")))?
    {
        let mut entry =
            entry.map_err(|err| AppError::Message(format!("读取 cloudflared 安装包失败: {err}")))?;
        let path = entry
            .path()
            .map_err(|err| AppError::Message(err.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        if path.ends_with("cloudflared") {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(AppError::Message(
        "cloudflared 安装包中未找到可执行文件。".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn extract_cloudflared_from_tar_gz(_bytes: &[u8], _dest: &Path) -> AppResult<()> {
    Err(AppError::Message(
        "当前平台的 cloudflared 无需解压。".into(),
    ))
}

pub fn extract_trycloudflare_url(line: &str) -> Option<String> {
    const PREFIX: &str = "https://";
    const SUFFIX: &str = ".trycloudflare.com";
    let lower = line.to_ascii_lowercase();
    let mut search_from = 0;

    while let Some(rel) = lower[search_from..].find(PREFIX) {
        let start = search_from + rel;
        let Some(suffix_rel) = lower[start..].find(SUFFIX) else {
            break;
        };
        let end = start + suffix_rel + SUFFIX.len();
        let host = &line[start + PREFIX.len()..end - SUFFIX.len()];
        if host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !host.is_empty()
        {
            return Some(line[start..end].trim_end_matches('/').to_string());
        }
        search_from = start + PREFIX.len();
    }
    None
}

/// Apply the global proxy to a tunnel child process environment.
pub(crate) fn apply_proxy_env(cmd: &mut Command, proxy: &ProxyConfig) {
    let url = match proxy.mode.as_str() {
        "manual" if !proxy.url.trim().is_empty() => Some(proxy.url.trim().to_string()),
        "system" => std::env::var("HTTPS_PROXY")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("HTTP_PROXY").ok().filter(|s| !s.is_empty()))
            .or_else(|| std::env::var("ALL_PROXY").ok().filter(|s| !s.is_empty())),
        _ => None,
    };
    if let Some(url) = url {
        for key in [
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "https_proxy",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            cmd.env(key, &url);
        }
        // Some cloudflared builds consult this dedicated variable.
        cmd.env("TUNNEL_HTTP_PROXY", &url);
    }
}

fn cloudflared_args(port: u16, quick: bool, token: &str) -> Vec<String> {
    if quick {
        return vec![
            "tunnel".into(),
            "--url".into(),
            format!("http://127.0.0.1:{port}"),
        ];
    }

    vec![
        "tunnel".into(),
        "--protocol".into(),
        "http2".into(),
        "run".into(),
        "--token".into(),
        token.trim().into(),
    ]
}

fn named_tunnel_ready_line(line: &str) -> bool {
    line.to_ascii_lowercase()
        .contains("registered tunnel connection")
}

fn safe_proxy_endpoint(value: &str) -> String {
    let Ok(url) = reqwest::Url::parse(value) else {
        return "<configured-unparseable>".into();
    };
    let Some(host) = url.host_str() else {
        return "<configured-unparseable>".into();
    };
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    }
}

fn configured_proxy_description(use_proxy: bool, proxy: &ProxyConfig) -> String {
    if !use_proxy {
        return "disabled".into();
    }
    let description = match proxy.mode.as_str() {
        "manual" if !proxy.url.trim().is_empty() => {
            format!("manual({})", safe_proxy_endpoint(proxy.url.trim()))
        }
        "system" => std::env::var("HTTPS_PROXY")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| std::env::var("HTTP_PROXY").ok().filter(|value| !value.is_empty()))
            .or_else(|| std::env::var("ALL_PROXY").ok().filter(|value| !value.is_empty()))
            .map(|value| format!("system({})", safe_proxy_endpoint(&value)))
            .unwrap_or_else(|| "system(unresolved)".into()),
        mode => format!("{mode}(unresolved)"),
    };
    sanitize_log_line(&description)
}

fn append_cloudflared_diagnostic(log_path: &Path, line: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(file, "{}", timestamped_log_line(line));
    }
}

fn recent_cloudflared_error(log_path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(log_path).ok()?;
    let size = file.seek(SeekFrom::End(0)).ok()?;
    file.seek(SeekFrom::Start(size.saturating_sub(DIAGNOSTIC_LOG_TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);
    tail.lines()
        .rev()
        .find(|line| line.contains(" ERR ") || line.starts_with("ERR "))
        .map(sanitize_log_line)
}

fn readiness_timeout_message(
    quick: bool,
    port: u16,
    proxy: &str,
    recent_error: Option<&str>,
    log_path: &Path,
) -> String {
    let expected = if quick {
        "trycloudflare.com 公网地址"
    } else {
        "Cloudflare Edge 注册确认"
    };
    let transport = if quick {
        "auto(QUIC/UDP 7844，失败后回退 HTTP/2/TCP 7844)"
    } else {
        "http2/TCP 7844"
    };
    let recent_error = recent_error.unwrap_or("未捕获到 cloudflared ERR 行");
    format!(
        "cloudflared 已启动，但在 {} 秒内没有返回{expected}。诊断：failure_boundary=cloudflared->Cloudflare Edge；edge_transport={transport}；configured_proxy={proxy}；proxy_scope=HTTP(S) Origin only；local_origin=http://127.0.0.1:{port}；last_edge_error={recent_error}。Cloudflare Edge 隧道链路不会由当前 HTTP 代理变量转发；若最近错误包含 dial tcp <edge-ip>:7844 i/o timeout，请在本机防火墙、路由器或代理软件中放行 Cloudflare Tunnel 的 TCP 7844 出站连接。该错误不表示本地端口 {port} 不可用。日志：{}",
        READY_TIMEOUT.as_secs(),
        log_path.display()
    )
}

/// Spawn `cloudflared tunnel --url http://127.0.0.1:{port}` (quick) or named `tunnel run --token`.
pub async fn spawn_cloudflare_tunnel(
    port: u16,
    cwd: &Path,
    log_path: &Path,
    cloudflare_mode: &str,
    cloudflare_token: &str,
    named_public_url: &str,
    use_proxy: bool,
) -> AppResult<CloudflareTunnelHandle> {
    let quick = cloudflare_mode != "named";

    if !quick {
        if cloudflare_token.trim().is_empty() {
            return Err(AppError::Message(
                "Cloudflare 命名隧道模式需要填写 Tunnel Token。".into(),
            ));
        }
        if named_public_url.trim().is_empty() {
            return Err(AppError::Message(
                "Cloudflare 命名隧道模式需要填写固定公网地址。".into(),
            ));
        }
    }

    ensure_managed_cloudflared_current().await?;
    let cloudflared = resolve_cloudflared()?;

    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut cmd = Command::new(&cloudflared);
    cmd.current_dir(cwd);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }

    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    let settings = crate::settings::AppSettings::load_or_default();
    let proxy_description = configured_proxy_description(use_proxy, &settings.proxy);
    if use_proxy {
        apply_proxy_env(&mut cmd, &settings.proxy);
    }

    cmd.args(cloudflared_args(port, quick, cloudflare_token));
    append_cloudflared_diagnostic(
        log_path,
        &format!(
            "event=cloudflared-start mode={} edge_transport={} configured_proxy={} proxy_scope=HTTP(S)-origin-only local_origin=http://127.0.0.1:{port} readiness={}",
            if quick { "quick" } else { "named" },
            if quick { "auto/7844" } else { "http2/tcp/7844" },
            proxy_description,
            if quick {
                "trycloudflare-url"
            } else {
                "registered-tunnel-connection"
            }
        ),
    );

    let mut child = cmd
        .spawn()
        .map_err(|err| AppError::Message(format!("启动 cloudflared 失败: {err}")))?;
    let pid = child.id();

    let (ready_tx, ready_rx) = oneshot::channel();
    let log_path = log_path.to_path_buf();
    let named_url = named_public_url.trim_end_matches('/').to_string();
    let log_path_for_error = log_path.clone();

    let Some(stdout) = child.stdout.take() else {
        stop_child(child, pid).await?;
        return Err(AppError::Message(
            "cloudflared 未提供可读取的输出流，已停止新建进程。".into(),
        ));
    };
    let stderr = child.stderr.take();
    tokio::spawn(async move {
        stream_cloudflare_output(stdout, stderr, &log_path, quick, named_url, ready_tx).await;
    });

    let readiness = match time::timeout(READY_TIMEOUT, ready_rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(AppError::Message(
            "cloudflared 输出流在隧道就绪前意外结束。".into(),
        )),
        Err(_) => {
            let recent_error = recent_cloudflared_error(&log_path_for_error);
            let message = readiness_timeout_message(
                quick,
                port,
                &proxy_description,
                recent_error.as_deref(),
                &log_path_for_error,
            );
            append_cloudflared_diagnostic(
                &log_path_for_error,
                &format!("event=cloudflared-readiness-timeout {message}"),
            );
            Err(AppError::Message(message))
        }
    };
    let ready = match readiness {
        Ok(ready) => ready,
        Err(error) => {
            let exit_status = child
                .try_wait()
                .ok()
                .flatten()
                .map(|status| format!("，退出状态：{status}"))
                .unwrap_or_default();
            let message = format!("{error}{exit_status}");
            stop_child(child, pid).await?;
            return Err(AppError::Message(message));
        }
    };

    match child.try_wait() {
        Ok(None) => {}
        Ok(Some(status)) => {
            return Err(AppError::Message(format!(
                "cloudflared 报告隧道就绪后立即退出，退出状态：{status}。请查看日志：{}",
                log_path_for_error.display()
            )));
        }
        Err(error) => {
            stop_child(child, pid).await?;
            return Err(AppError::Message(format!(
                "无法确认 cloudflared 就绪后的进程状态：{error}"
            )));
        }
    }

    let public_url = if quick {
        ready.public_url.ok_or_else(|| {
            AppError::Message(format!(
                "cloudflared 已启动，但没有解析到 trycloudflare.com 地址。请查看日志：{}",
                log_path_for_error.display()
            ))
        })?
    } else {
        named_public_url.trim_end_matches('/').to_string()
    };

    Ok(CloudflareTunnelHandle {
        child,
        public_url,
        pid,
    })
}

struct QuickTunnelReady {
    public_url: Option<String>,
    #[allow(dead_code)]
    named_ready: bool,
}

async fn stream_cloudflare_output<R, E>(
    stdout: R,
    stderr: Option<E>,
    log_path: &Path,
    quick: bool,
    named_url: String,
    ready_tx: oneshot::Sender<AppResult<QuickTunnelReady>>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    E: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut ready_tx = Some(ready_tx);
    let mut public_url: Option<String> = None;

    let mut log = match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await
    {
        Ok(file) => file,
        Err(error) => {
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(Err(AppError::Message(format!(
                    "无法打开 cloudflared 日志文件 {}：{error}",
                    log_path.display()
                ))));
            }
            return;
        }
    };

    let send_ready = |tx: &mut Option<oneshot::Sender<AppResult<QuickTunnelReady>>>,
                      url: Option<String>,
                      named_ready: bool| {
        if let Some(sender) = tx.take() {
            let _ = sender.send(Ok(QuickTunnelReady {
                public_url: url,
                named_ready,
            }));
        }
    };

    let handle_line = |line: &str,
                           public_url: &mut Option<String>,
                           ready_tx: &mut Option<oneshot::Sender<AppResult<QuickTunnelReady>>>| {
        if quick {
            if public_url.is_none() {
                if let Some(url) = extract_trycloudflare_url(line) {
                    *public_url = Some(url.clone());
                    send_ready(ready_tx, Some(url), false);
                }
            }
        } else {
            if named_tunnel_ready_line(line) {
                send_ready(ready_tx, Some(named_url.clone()), true);
            }
        }
    };

    // cloudflared logs primarily to stderr; read stdout and stderr concurrently.
    let (line_tx, mut line_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let stderr_line_tx = line_tx.clone();

    tokio::spawn(async move {
        let mut stdout = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = stdout.next_line().await {
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });

    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let mut stderr = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = stderr.next_line().await {
                if stderr_line_tx.send(line).is_err() {
                    break;
                }
            }
        });
    } else {
        drop(stderr_line_tx);
    }

    while let Some(line) = line_rx.recv().await {
        let sanitized = format_cloudflared_log_line(&line);
        let write_result = async {
            log.write_all(sanitized.as_bytes()).await?;
            log.write_all(b"\n").await?;
            log.flush().await
        }
        .await;
        if let Err(error) = write_result {
            if let Some(sender) = ready_tx.take() {
                let _ = sender.send(Err(AppError::Message(format!(
                    "写入 cloudflared 日志失败：{error}"
                ))));
            }
            return;
        }
        handle_line(&line, &mut public_url, &mut ready_tx);
    }

    if let Some(sender) = ready_tx.take() {
        let expected = if quick {
            "trycloudflare.com 公网地址"
        } else {
            "registered tunnel connection"
        };
        let _ = sender.send(Err(AppError::Message(format!(
            "cloudflared 输出在出现 {expected} 前结束。"
        ))));
    }
}

pub async fn stop_child(mut child: Child, pid: Option<u32>) -> AppResult<()> {
    if child
        .try_wait()
        .map_err(|error| AppError::Message(format!("检查隧道进程状态失败: {error}")))?
        .is_some()
    {
        return Ok(());
    }

    let tracked_pid = pid.or_else(|| child.id());
    let terminate_error = tracked_pid.and_then(|pid| {
        platform()
            .terminate_process_tree(pid)
            .err()
            .map(|error| error.to_string())
    });

    match time::timeout(STOP_TIMEOUT, child.wait()).await {
        Ok(Ok(_)) => return Ok(()),
        Ok(Err(error)) => return Err(AppError::Message(format!("等待隧道进程退出失败: {error}"))),
        Err(_) => {}
    }

    let force_error = child.start_kill().err().map(|error| error.to_string());
    match time::timeout(STOP_TIMEOUT, child.wait()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(AppError::Message(format!(
            "强制停止隧道进程后等待失败: {error}"
        ))),
        Err(_) => {
            let still_alive = tracked_pid
                .map(|pid| platform().is_process_alive(pid))
                .unwrap_or(true);
            let mut details = Vec::new();
            if let Some(error) = terminate_error {
                details.push(format!("终止进程树失败: {error}"));
            }
            if let Some(error) = force_error {
                details.push(format!("强制停止失败: {error}"));
            }
            if still_alive {
                details.push("进程仍存活".into());
            }
            Err(AppError::Message(format!(
                "隧道进程未能在 {} 秒内退出{}",
                STOP_TIMEOUT.as_secs() * 2,
                if details.is_empty() {
                    String::new()
                } else {
                    format!("：{}", details.join("；"))
                }
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;
    use tokio::sync::oneshot;

    use super::{
        cloudflared_args, cloudflared_release_asset, cloudflared_release_sha256,
        cloudflared_version_is_current, configured_proxy_description, extract_trycloudflare_url,
        named_tunnel_ready_line, readiness_timeout_message, recent_cloudflared_error,
        replace_cloudflared_file, stop_child, stream_cloudflare_output,
        verify_cloudflared_checksum, QuickTunnelReady,
    };
    use crate::error::AppResult;
    use crate::settings::ProxyConfig;

    async fn named_readiness(lines: &str, log_path: &Path) -> AppResult<QuickTunnelReady> {
        let (mut writer, reader) = tokio::io::duplex(2_048);
        writer.write_all(lines.as_bytes()).await.expect("fake output");
        writer.shutdown().await.expect("close fake output");
        let (ready_tx, ready_rx) = oneshot::channel();

        stream_cloudflare_output(
            reader,
            None::<tokio::io::Empty>,
            log_path,
            false,
            "https://fixed.example.invalid".into(),
            ready_tx,
        )
        .await;

        ready_rx.await.expect("readiness result")
    }

    #[test]
    fn extracts_trycloudflare_url_from_log_line() {
        let line = "INF | https://abc-def.trycloudflare.com is your tunnel URL";
        assert_eq!(
            extract_trycloudflare_url(line).as_deref(),
            Some("https://abc-def.trycloudflare.com")
        );
    }

    #[test]
    fn ignores_invalid_hosts() {
        let line = "https://bad_host.trycloudflare.com";
        assert!(extract_trycloudflare_url(line).is_none());
    }

    #[test]
    fn named_tunnel_command_forces_http2() {
        let args = cloudflared_args(28_766, false, "token-placeholder");

        assert_eq!(
            args,
            [
                "tunnel",
                "--protocol",
                "http2",
                "run",
                "--token",
                "token-placeholder",
            ]
        );
    }

    #[test]
    fn managed_cloudflared_version_includes_connectivity_prechecks() {
        assert_eq!(super::CLOUDFLARED_VERSION, "2026.8.2");
    }

    #[test]
    fn selected_cloudflared_release_has_a_pinned_sha256() {
        let asset = cloudflared_release_asset().expect("supported release asset");
        let sha256 = cloudflared_release_sha256(asset).expect("pinned release checksum");

        assert_eq!(sha256.len(), 64);
        assert!(sha256.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn cloudflared_download_checksum_rejects_modified_bytes() {
        const ABC_SHA256: &str =
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        verify_cloudflared_checksum(b"abc", ABC_SHA256).expect("matching checksum");
        let error = verify_cloudflared_checksum(b"modified", ABC_SHA256)
            .expect_err("modified bytes must be rejected");

        assert!(error.to_string().contains("SHA-256 校验失败"), "{error}");
    }

    #[test]
    fn managed_cloudflared_version_requires_exact_match() {
        assert!(cloudflared_version_is_current(
            "cloudflared version 2026.8.2 (built 2026-08-14T04:22 UTC)"
        ));
        assert!(!cloudflared_version_is_current(
            "cloudflared version 2025.6.1 (built 2025-06-17T16:37 UTC)"
        ));
        assert!(!cloudflared_version_is_current(
            "cloudflared development build"
        ));
    }

    #[test]
    fn staged_cloudflared_replaces_existing_binary() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dest = temp.path().join("cloudflared");
        let staged = temp.path().join("cloudflared.download");
        std::fs::write(&dest, b"old-version").expect("write old binary");
        std::fs::write(&staged, b"new-version").expect("write staged binary");

        replace_cloudflared_file(&staged, &dest).expect("replace cloudflared");

        assert_eq!(
            std::fs::read(&dest).expect("read installed binary"),
            b"new-version"
        );
        assert!(!staged.exists());
        assert_eq!(
            std::fs::read_dir(temp.path())
                .expect("read tempdir")
                .count(),
            1,
            "successful replacement must remove the backup"
        );
    }

    #[test]
    fn failed_cloudflared_replacement_restores_existing_binary() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dest = temp.path().join("cloudflared");
        let missing_staged = temp.path().join("missing.download");
        std::fs::write(&dest, b"old-version").expect("write old binary");

        let error = replace_cloudflared_file(&missing_staged, &dest)
            .expect_err("missing staged binary must fail");

        assert!(error.to_string().contains("已恢复旧版本"), "{error}");
        assert_eq!(
            std::fs::read(&dest).expect("read restored binary"),
            b"old-version"
        );
        assert_eq!(
            std::fs::read_dir(temp.path())
                .expect("read tempdir")
                .count(),
            1,
            "rollback must not leave a backup behind"
        );
    }

    #[test]
    fn named_tunnel_requires_registered_connection_for_readiness() {
        assert!(!named_tunnel_ready_line(
            "INF Starting metrics server on 127.0.0.1:20241/metrics"
        ));
        assert!(named_tunnel_ready_line(
            "INF Registered tunnel connection connIndex=0 protocol=http2"
        ));
    }

    #[test]
    fn timeout_diagnostic_identifies_edge_egress_and_proxy_scope() {
        let proxy = ProxyConfig {
            mode: "manual".into(),
            url: "http://user:password-placeholder@127.0.0.1:7890".into(),
        };
        let proxy = configured_proxy_description(true, &proxy);
        let message = readiness_timeout_message(
            false,
            28_766,
            &proxy,
            Some("ERR DialContext error: dial tcp 198.41.192.57:7844: i/o timeout"),
            Path::new("cloudflared.log"),
        );

        assert!(message.contains("failure_boundary=cloudflared->Cloudflare Edge"));
        assert!(message.contains("edge_transport=http2/TCP 7844"));
        assert!(message.contains("configured_proxy=manual(http://127.0.0.1:7890)"));
        assert!(message.contains("proxy_scope=HTTP(S) Origin only"));
        assert!(message.contains("198.41.192.57:7844: i/o timeout"));
        assert!(message.contains("该错误不表示本地端口 28766 不可用"));
        assert!(!message.contains("password-placeholder"), "{message}");
    }

    #[test]
    fn recent_error_survives_log_tail_starting_inside_utf8() {
        let temp = tempfile::tempdir().expect("tempdir");
        let log_path = temp.path().join("cloudflared.log");
        let content = format!(
            "{}\n2026-08-19T08:43:16+08:00 ERR DialContext error: dial tcp 198.41.192.57:7844: i/o timeout\n",
            "中".repeat(6_000)
        );
        std::fs::write(&log_path, content).expect("write cloudflared log");

        let error = recent_cloudflared_error(&log_path).expect("recent edge error");
        assert!(error.contains("198.41.192.57:7844: i/o timeout"));
    }

    #[tokio::test]
    async fn metrics_only_output_ends_without_marking_named_tunnel_ready() {
        let temp = tempfile::tempdir().expect("tempdir");
        let result = named_readiness(
            "INF Starting metrics server on 127.0.0.1:20241/metrics\n",
            &temp.path().join("cloudflared.log"),
        )
        .await;

        let error = match result {
            Ok(_) => panic!("metrics output must not mark tunnel ready"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("registered tunnel connection"));
    }

    #[tokio::test]
    async fn registered_connection_marks_named_tunnel_ready() {
        let temp = tempfile::tempdir().expect("tempdir");
        let log_path = temp.path().join("cloudflared.log");
        let ready = named_readiness(
            "2026-08-31T18:43:16Z INF Registered tunnel connection connIndex=0 protocol=http2\n",
            &log_path,
        )
        .await
        .expect("named readiness");

        assert!(ready.named_ready);
        assert_eq!(
            ready.public_url.as_deref(),
            Some("https://fixed.example.invalid")
        );
        let log = std::fs::read_to_string(log_path).expect("read cloudflared log");
        assert!(
            log.contains("2026-09-01T02:43:16+08:00 INF Registered tunnel connection"),
            "{log}"
        );
    }

    #[tokio::test]
    async fn log_open_failure_is_reported_before_readiness() {
        let temp = tempfile::tempdir().expect("tempdir");
        let result = named_readiness("", temp.path()).await;

        let error = match result {
            Ok(_) => panic!("directory cannot be used as a log file"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("无法打开 cloudflared 日志文件"));
    }

    #[tokio::test]
    async fn stop_child_reaps_the_managed_process() {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd.exe");
            command.args(["/D", "/S", "/C", "ping -n 30 127.0.0.1 > NUL"]);
            command
        };
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };

        let child = command.spawn().expect("spawn managed child");
        let pid = child.id().expect("managed child pid");
        assert!(crate::platform::platform().is_process_alive(pid));

        stop_child(child, Some(pid)).await.expect("stop child");

        assert!(!crate::platform::platform().is_process_alive(pid));
    }
}
