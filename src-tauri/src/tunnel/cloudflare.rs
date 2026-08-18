use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::time;

use crate::error::{AppError, AppResult};
use crate::platform::platform;
use crate::settings::ProxyConfig;

use super::logs::sanitize_log_line;

const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Handle to a supervised `cloudflared` child process.
pub struct CloudflareTunnelHandle {
    pub child: Child,
    pub public_url: String,
    pub pid: Option<u32>,
}

pub fn resolve_cloudflared() -> AppResult<PathBuf> {
    platform()
        .cloudflared_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| cached_cloudflared_path().filter(|path| path.is_file()))
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
const CLOUDFLARED_VERSION: &str = "2025.6.1";

/// Download cloudflared into the app cache `bin/` directory, honoring the
/// configured mirror + proxy. Windows/Linux assets are raw binaries; macOS
/// assets are `.tgz` archives that need extraction.
pub(crate) async fn download_cloudflared_to_cache() -> AppResult<PathBuf> {
    let settings = crate::settings::AppSettings::load_or_default();
    let asset = cloudflared_release_asset()?;
    let url = format!(
        "https://github.com/cloudflare/cloudflared/releases/download/{CLOUDFLARED_VERSION}/{asset}"
    );
    let dest = cached_cloudflared_path()
        .ok_or_else(|| AppError::Message("无法解析缓存目录。".into()))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let bytes = crate::tunnel::download::download_release_asset(&settings, &url, "cloudflared").await?;

    if asset.ends_with(".tgz") {
        extract_cloudflared_from_tar_gz(&bytes, &dest)?;
    } else {
        std::fs::write(&dest, &bytes)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&dest) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&dest, perms);
        }
    }

    if dest.is_file() {
        Ok(dest)
    } else {
        Err(AppError::Message("cloudflared 自动安装失败。".into()))
    }
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
    let cloudflared = resolve_cloudflared()?;
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
    if use_proxy {
        apply_proxy_env(&mut cmd, &settings.proxy);
    }

    cmd.args(cloudflared_args(port, quick, cloudflare_token));

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
            let expected = if quick {
                "trycloudflare.com 公网地址"
            } else {
                "Cloudflare Edge 注册确认"
            };
            Err(AppError::Message(format!(
                "cloudflared 已启动，但在 {} 秒内没有返回{expected}。请确认本机端口 {port} 可用、网络代理配置正确，并查看日志：{}",
                READY_TIMEOUT.as_secs(),
                log_path_for_error.display()
            )))
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
        let sanitized = sanitize_log_line(&line);
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
    if let Some(pid) = pid {
        let _ = platform().terminate_process_tree(pid);
    }

    let _ = child.kill().await;
    let _ = time::timeout(Duration::from_secs(3), child.wait()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tokio::io::AsyncWriteExt;
    use tokio::sync::oneshot;

    use super::{
        cloudflared_args, extract_trycloudflare_url, named_tunnel_ready_line,
        stream_cloudflare_output, QuickTunnelReady,
    };
    use crate::error::AppResult;

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
    fn named_tunnel_requires_registered_connection_for_readiness() {
        assert!(!named_tunnel_ready_line(
            "INF Starting metrics server on 127.0.0.1:20241/metrics"
        ));
        assert!(named_tunnel_ready_line(
            "INF Registered tunnel connection connIndex=0 protocol=http2"
        ));
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
        let ready = named_readiness(
            "INF Registered tunnel connection connIndex=0 protocol=http2\n",
            &temp.path().join("cloudflared.log"),
        )
        .await
        .expect("named readiness");

        assert!(ready.named_ready);
        assert_eq!(
            ready.public_url.as_deref(),
            Some("https://fixed.example.invalid")
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
}
