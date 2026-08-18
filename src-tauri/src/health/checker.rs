use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use serde::Serialize;
use serde_json::{json, Value};

use crate::settings::{AppSettings, ProxyConfig};
use crate::tunnel::{append_profile_log, new_trace_id};
use crate::workspace::WorkspaceProfile;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MCP_PROBE_BUDGET: Duration = Duration::from_secs(10);
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthItem {
    pub key: String,
    pub layer: String,
    pub status: String,
    pub trace_id: String,
    pub retryable: bool,
    pub label: String,
    pub ok: bool,
    pub detail: String,
    pub hint: String,
}

#[derive(Clone, Copy)]
enum ProbeAuth<'a> {
    None,
    Bearer(&'a str),
}

#[derive(Debug)]
struct ProbeResult {
    ok: bool,
    detail: String,
}

#[cfg(test)]
fn http_client() -> reqwest::Client {
    http_client_with_proxy(&ProxyConfig {
        mode: "none".into(),
        url: String::new(),
    })
    .expect("failed to build direct HTTP client")
}

fn http_client_with_proxy(proxy: &ProxyConfig) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().timeout(REQUEST_TIMEOUT);
    match proxy.mode.trim() {
        "" | "system" => {}
        "none" => builder = builder.no_proxy(),
        "manual" => {
            let url = proxy.url.trim();
            if url.is_empty() {
                return Err("手动代理模式缺少代理地址".into());
            }
            let configured = reqwest::Proxy::all(url)
                .map_err(|error| format!("代理地址无效: {error}"))?
                .no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1,::1"));
            builder = builder.proxy(configured);
        }
        mode => return Err(format!("不支持的代理模式: {mode}")),
    }
    builder
        .build()
        .map_err(|error| format!("创建健康检查 HTTP 客户端失败: {error}"))
}

async fn probe_mcp_endpoint(
    client: &reqwest::Client,
    url: &str,
    auth: ProbeAuth<'_>,
) -> ProbeResult {
    if url.is_empty() {
        return ProbeResult {
            ok: false,
            detail: "URL not configured".into(),
        };
    }

    let probe = async {
        let initialize = send_rpc(
            client,
            url,
            auth,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "coding-tools-health", "version": env!("CARGO_PKG_VERSION") }
                }
            }),
        )
        .await?;
        if initialize.status() != reqwest::StatusCode::OK {
            return Err(format!(
                "initialize returned HTTP {}",
                initialize.status().as_u16()
            ));
        }
        let initialize_body: Value = initialize
            .json()
            .await
            .map_err(|error| format!("initialize JSON invalid: {error}"))?;
        if initialize_body.get("id") != Some(&json!(1))
            || initialize_body["result"]["protocolVersion"] != MCP_PROTOCOL_VERSION
        {
            return Err("initialize response ID or protocolVersion mismatch".into());
        }

        let initialized = send_rpc(
            client,
            url,
            auth,
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )
        .await?;
        let initialized_status = initialized.status();
        let initialized_body = initialized
            .bytes()
            .await
            .map_err(|error| format!("initialized body failed: {error}"))?;
        if initialized_status != reqwest::StatusCode::ACCEPTED || !initialized_body.is_empty() {
            return Err(format!(
                "initialized expected HTTP 202 empty body, got HTTP {} with {} bytes",
                initialized_status.as_u16(),
                initialized_body.len()
            ));
        }

        let tools = send_rpc(
            client,
            url,
            auth,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
        )
        .await?;
        if tools.status() != reqwest::StatusCode::OK {
            return Err(format!("tools/list returned HTTP {}", tools.status().as_u16()));
        }
        let tools_body: Value = tools
            .json()
            .await
            .map_err(|error| format!("tools/list JSON invalid: {error}"))?;
        let tool_count = tools_body["result"]["tools"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        if tools_body.get("id") != Some(&json!(2)) || tool_count == 0 {
            return Err("tools/list response ID mismatch or tool catalog empty".into());
        }
        Ok(format!(
            "initialize 200; initialized 202; tools/list 200 ({tool_count} tools)"
        ))
    };

    match tokio::time::timeout(MCP_PROBE_BUDGET, probe).await {
        Ok(Ok(detail)) => ProbeResult { ok: true, detail },
        Ok(Err(detail)) => ProbeResult { ok: false, detail },
        Err(_) => ProbeResult {
            ok: false,
            detail: format!("MCP handshake exceeded {}s budget", MCP_PROBE_BUDGET.as_secs()),
        },
    }
}

async fn probe_legacy_endpoint(
    client: &reqwest::Client,
    url: &str,
    auth: ProbeAuth<'_>,
) -> ProbeResult {
    if url.is_empty() {
        return ProbeResult {
            ok: false,
            detail: "URL not configured".into(),
        };
    }
    let mut request = client.get(url);
    if let ProbeAuth::Bearer(token) = auth {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    match request.send().await {
        Ok(response) if response.status().is_success() => ProbeResult {
            ok: true,
            detail: format!("legacy GET returned HTTP {}", response.status().as_u16()),
        },
        Ok(response) => ProbeResult {
            ok: false,
            detail: format!("legacy GET returned HTTP {}", response.status().as_u16()),
        },
        Err(error) => ProbeResult {
            ok: false,
            detail: error.to_string(),
        },
    }
}

async fn send_rpc(
    client: &reqwest::Client,
    url: &str,
    auth: ProbeAuth<'_>,
    body: Value,
) -> Result<reqwest::Response, String> {
    let mut request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
        .json(&body);
    if let ProbeAuth::Bearer(token) = auth {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    request.send().await.map_err(|error| error.to_string())
}

async fn check_oauth_layer(
    client: &reqwest::Client,
    base_url: &str,
    mcp_url: &str,
    transport_v2: bool,
) -> ProbeResult {
    let metadata_path = if transport_v2 {
        ".well-known/oauth-protected-resource/mcp"
    } else {
        ".well-known/oauth-protected-resource"
    };
    let expected_metadata = format!("{}/{metadata_path}", base_url.trim_end_matches('/'));
    let challenge = match send_rpc(
        client,
        mcp_url,
        ProbeAuth::None,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            return ProbeResult {
                ok: false,
                detail: error,
            }
        }
    };
    let challenge_value = challenge
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if challenge.status() != reqwest::StatusCode::UNAUTHORIZED
        || !challenge_value.contains(&expected_metadata)
    {
        return ProbeResult {
            ok: false,
            detail: "OAuth 401 challenge missing canonical resource_metadata".into(),
        };
    }

    for path in [".well-known/oauth-authorization-server", metadata_path] {
        let url = format!("{}/{path}", base_url.trim_end_matches('/'));
        let response = match client.get(&url).send().await {
            Ok(response) => response,
            Err(error) => {
                return ProbeResult {
                    ok: false,
                    detail: error.to_string(),
                }
            }
        };
        if !response.status().is_success() {
            return ProbeResult {
                ok: false,
                detail: format!("{path} returned HTTP {}", response.status().as_u16()),
            };
        }
        let payload: Value = match response.json().await {
            Ok(payload) => payload,
            Err(error) => {
                return ProbeResult {
                    ok: false,
                    detail: format!("{path} JSON invalid: {error}"),
                }
            }
        };
        if path.ends_with("authorization-server") && payload["issuer"] != base_url {
            return ProbeResult {
                ok: false,
                detail: "OAuth issuer differs from canonical origin".into(),
            };
        }
        if path.contains("oauth-protected-resource") && payload["resource"] != mcp_url {
            return ProbeResult {
                ok: false,
                detail: "OAuth protected resource differs from canonical MCP URL".into(),
            };
        }
    }

    ProbeResult {
        ok: true,
        detail: "401 challenge and canonical OAuth metadata verified".into(),
    }
}

async fn check_url(client: &reqwest::Client, url: &str) -> (bool, String) {
    if url.is_empty() {
        return (false, "URL not configured".to_string());
    }
    match client.get(url).send().await {
        Ok(response) => {
            let code = response.status().as_u16();
            (response.status().is_success(), format!("HTTP {code}"))
        }
        Err(error) => (false, error.to_string()),
    }
}

pub async fn run_health_checks(
    profile: &WorkspaceProfile,
    bearer_token: Option<&str>,
) -> Vec<HealthItem> {
    let trace_id = new_trace_id();
    let started_at = Instant::now();
    append_profile_log(
        &profile.id,
        "health.log",
        &format!("[trace={trace_id}] stage=health-start"),
    );
    let settings = AppSettings::load_or_default();
    let client = match http_client_with_proxy(&settings.proxy) {
        Ok(client) => client,
        Err(error) => {
            let items = vec![health_item(
                "mcp.config",
                "config",
                "MCP 配置",
                "fail",
                error,
                "检查设置 → 通用 → 网络代理。",
                &trace_id,
            )];
            append_profile_log(
                &profile.id,
                "health.log",
                &format!(
                    "[trace={trace_id}] stage=health-complete status=public-degraded elapsed_ms={}",
                    started_at.elapsed().as_millis()
                ),
            );
            return items;
        }
    };
    let local_mcp = profile.local_endpoint();
    let public_base = profile.effective_public_url();
    let public_mcp = profile.public_endpoint();
    let tunnel_configured = !matches!(profile.tunnel.tunnel_type.as_str(), "" | "none");
    let transport_v2 = profile.tunnel.tunnel_type == "cloudflare"
        && profile.tunnel.cloudflare_mode == "named"
        && profile.tunnel.mcp_transport_v2;
    let config_ok = !tunnel_configured || !public_mcp.is_empty();

    let auth = if profile.auth.bearer_enabled() {
        bearer_token
            .filter(|token| !token.is_empty())
            .map(ProbeAuth::Bearer)
    } else if profile.auth.oauth_enabled() {
        None
    } else {
        Some(ProbeAuth::None)
    };

    let local_probe = if transport_v2 {
        if let Some(auth) = auth {
            Some(probe_mcp_endpoint(&client, &local_mcp, auth).await)
        } else {
            None
        }
    } else {
        Some(
            probe_legacy_endpoint(&client, &local_mcp, auth.unwrap_or(ProbeAuth::None)).await,
        )
    };
    let public_probe = if public_mcp.is_empty() {
        None
    } else if transport_v2 {
        if let Some(auth) = auth {
            Some(probe_mcp_endpoint(&client, &public_mcp, auth).await)
        } else {
            None
        }
    } else {
        Some(
            probe_legacy_endpoint(&client, &public_mcp, auth.unwrap_or(ProbeAuth::None)).await,
        )
    };
    let oauth_probe = if profile.auth.oauth_enabled() {
        let base = if public_base.is_empty() {
            format!("http://127.0.0.1:{}", profile.runtime.local_port)
        } else {
            public_base.clone()
        };
        let endpoint = if public_mcp.is_empty() {
            local_mcp.clone()
        } else {
            public_mcp.clone()
        };
        Some(check_oauth_layer(&client, &base, &endpoint, transport_v2).await)
    } else {
        None
    };

    let local_status = probe_status(local_probe.as_ref(), profile.auth.oauth_enabled());
    let public_status = if !tunnel_configured {
        "skip"
    } else {
        probe_status(public_probe.as_ref(), profile.auth.oauth_enabled())
    };
    let handshake_status = if !transport_v2 {
        if local_probe.as_ref().is_some_and(|result| result.ok)
            && (!tunnel_configured || public_probe.as_ref().is_some_and(|result| result.ok))
        {
            "warn"
        } else {
            "fail"
        }
    } else if profile.auth.oauth_enabled() {
        if oauth_probe.as_ref().is_some_and(|result| result.ok) {
            "warn"
        } else {
            "fail"
        }
    } else if local_probe.as_ref().is_some_and(|result| result.ok)
        && (!tunnel_configured || public_probe.as_ref().is_some_and(|result| result.ok))
    {
        "pass"
    } else {
        "fail"
    };

    let mut items = vec![
        health_item(
            "mcp.config",
            "config",
            "MCP 配置",
            if config_ok { "pass" } else { "fail" },
            if config_ok {
                if transport_v2 {
                    "端口、固定域名与 endpoint 配置有效".into()
                } else {
                    "兼容 transport 已启用；固定域名 v2 未启用".into()
                }
            } else {
                "已配置隧道但缺少公网 endpoint".into()
            },
            "检查固定域名、隧道模式和公网 URL。",
            &trace_id,
        ),
        health_item(
            "mcp.local_transport",
            "local",
            "本地 MCP transport",
            local_status,
            probe_detail(local_probe.as_ref(), profile.auth.oauth_enabled()),
            "确认 MCP listener 已启动且本地端口可访问。",
            &trace_id,
        ),
        health_item(
            "mcp.public_transport",
            "public",
            "公网 MCP transport",
            public_status,
            if !tunnel_configured {
                "未配置公网隧道".into()
            } else {
                probe_detail(public_probe.as_ref(), profile.auth.oauth_enabled())
            },
            "检查 tunnel provider 状态、DNS 和 canonical URL。",
            &trace_id,
        ),
        health_item(
            "mcp.oauth",
            "oauth",
            "MCP OAuth",
            match oauth_probe.as_ref() {
                Some(result) if result.ok => "pass",
                Some(_) => "fail",
                None => "skip",
            },
            oauth_probe
                .as_ref()
                .map(|result| result.detail.clone())
                .unwrap_or_else(|| "当前认证模式不是 OAuth".into()),
            "确认 401 challenge、issuer 和 protected resource metadata 使用同一 canonical origin。",
            &trace_id,
        ),
        health_item(
            "mcp.handshake",
            "handshake",
            "MCP 握手",
            handshake_status,
            if !transport_v2 {
                "兼容 transport 仅验证 GET 可达性；启用稳定固定域名链路后执行完整握手"
                    .into()
            } else if profile.auth.oauth_enabled() {
                "OAuth 元数据已验证；无人值守检查不签发用户 access token".into()
            } else {
                format!(
                    "local={}; public={}",
                    local_probe
                        .as_ref()
                        .map(|result| result.ok.to_string())
                        .unwrap_or_else(|| "skipped".into()),
                    public_probe
                        .as_ref()
                        .map(|result| result.ok.to_string())
                        .unwrap_or_else(|| "skipped".into())
                )
            },
            "根据失败阶段检查 initialize、notification 202 或 tools/list。",
            &trace_id,
        ),
    ];

    let actions_local = profile.actions_local_base_url();
    let actions_public = profile.actions_effective_public_url();
    for (key, layer, label, url) in [
        (
            "actions.local_health",
            "local",
            "本地 Actions /health",
            format!("{actions_local}/health"),
        ),
        (
            "actions.local_openapi",
            "local",
            "本地 Actions /openapi.json",
            format!("{actions_local}/openapi.json"),
        ),
        (
            "actions.public_openapi",
            "public",
            "公网 Actions /openapi.json",
            if actions_public.is_empty() {
                String::new()
            } else {
                format!("{}/openapi.json", actions_public.trim_end_matches('/'))
            },
        ),
    ] {
        let (ok, detail) = check_url(&client, &url).await;
        items.push(health_item(
            key,
            layer,
            label,
            if url.is_empty() { "skip" } else if ok { "pass" } else { "fail" },
            detail,
            "确认 Actions listener 与隧道配置。",
            &trace_id,
        ));
    }

    let aggregate = aggregate_mcp_health(&items);
    append_profile_log(
        &profile.id,
        "health.log",
        &format!(
            "[trace={trace_id}] stage=health-complete status={aggregate} elapsed_ms={}",
            started_at.elapsed().as_millis()
        ),
    );

    items
}

fn aggregate_mcp_health(items: &[HealthItem]) -> &'static str {
    let mcp_items = items.iter().filter(|item| item.key.starts_with("mcp."));
    if mcp_items.clone().any(|item| item.status == "fail") {
        "public-degraded"
    } else if mcp_items
        .clone()
        .any(|item| item.key == "mcp.public_transport" && item.status == "pass")
    {
        "public-ready"
    } else {
        "local-ready"
    }
}

fn probe_status(result: Option<&ProbeResult>, oauth: bool) -> &'static str {
    match result {
        Some(result) if result.ok => "pass",
        Some(_) => "fail",
        None if oauth => "skip",
        None => "fail",
    }
}

fn probe_detail(result: Option<&ProbeResult>, oauth: bool) -> String {
    result
        .map(|result| result.detail.clone())
        .unwrap_or_else(|| {
            if oauth {
                "OAuth 需要用户 access token，transport 由 challenge/metadata 检查覆盖".into()
            } else {
                "认证凭据不可用".into()
            }
        })
}

fn health_item(
    key: &str,
    layer: &str,
    label: &str,
    status: &str,
    detail: String,
    hint: &str,
    trace_id: &str,
) -> HealthItem {
    let ok = matches!(status, "pass" | "warn" | "skip");
    HealthItem {
        key: key.into(),
        layer: layer.into(),
        status: status.into(),
        trace_id: trace_id.into(),
        retryable: status == "fail",
        label: label.into(),
        ok,
        detail,
        hint: if ok { String::new() } else { hint.into() },
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::State;
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use axum::{Json, Router};

    use super::*;

    #[test]
    fn actions_failure_does_not_degrade_mcp_aggregate() {
        let items = vec![
            health_item(
                "mcp.local_transport",
                "local",
                "local",
                "pass",
                "ok".into(),
                "",
                "trace-test",
            ),
            health_item(
                "actions.local_health",
                "local",
                "actions",
                "fail",
                "offline".into(),
                "retry",
                "trace-test",
            ),
        ];

        assert_eq!(aggregate_mcp_health(&items), "local-ready");
    }

    #[derive(Clone, Copy)]
    enum FakeMode {
        Compliant,
        InitializeFailure,
        BadNotification,
        EmptyTools,
    }

    async fn fake_mcp(State(mode): State<FakeMode>, Json(body): Json<Value>) -> Response {
        match body.get("method").and_then(Value::as_str).unwrap_or("") {
            "initialize" if matches!(mode, FakeMode::InitializeFailure) => {
                reqwest::StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
            "initialize" => Json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "protocolVersion": MCP_PROTOCOL_VERSION }
            }))
            .into_response(),
            "notifications/initialized" if matches!(mode, FakeMode::BadNotification) => {
                Json(json!(null)).into_response()
            }
            "notifications/initialized" => reqwest::StatusCode::ACCEPTED.into_response(),
            "tools/list" => Json(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "tools": if matches!(mode, FakeMode::EmptyTools) {
                        json!([])
                    } else {
                        json!([{"name":"read_file"}])
                    }
                }
            }))
            .into_response(),
            _ => reqwest::StatusCode::BAD_REQUEST.into_response(),
        }
    }

    async fn fake_server(mode: FakeMode) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route(
                "/mcp",
                get(|| async { reqwest::StatusCode::OK }).post(fake_mcp),
            )
            .with_state(mode);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind fake MCP");
        let address = listener.local_addr().expect("fake MCP address");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{address}/mcp"), handle)
    }

    #[tokio::test]
    async fn handshake_probe_requires_the_full_streamable_http_sequence() {
        let client = http_client();
        for (mode, expected_ok, expected_detail) in [
            (FakeMode::Compliant, true, "tools/list 200"),
            (FakeMode::InitializeFailure, false, "initialize returned"),
            (FakeMode::BadNotification, false, "initialized expected HTTP 202"),
            (FakeMode::EmptyTools, false, "tool catalog empty"),
        ] {
            let (url, handle) = fake_server(mode).await;
            let result = probe_mcp_endpoint(&client, &url, ProbeAuth::None).await;
            handle.abort();

            assert_eq!(result.ok, expected_ok, "{}", result.detail);
            assert!(result.detail.contains(expected_detail), "{}", result.detail);
        }
    }

    #[tokio::test]
    async fn get_reachability_does_not_hide_initialize_failure() {
        let client = http_client();
        let (url, handle) = fake_server(FakeMode::InitializeFailure).await;
        let get_status = client.get(&url).send().await.expect("GET").status();
        let result = probe_mcp_endpoint(&client, &url, ProbeAuth::None).await;
        handle.abort();

        assert_eq!(get_status, reqwest::StatusCode::OK);
        assert!(!result.ok);
    }

    #[tokio::test]
    async fn legacy_probe_preserves_get_reachability_for_rollback() {
        let client = http_client();
        let (url, handle) = fake_server(FakeMode::InitializeFailure).await;
        let result = probe_legacy_endpoint(&client, &url, ProbeAuth::None).await;
        handle.abort();

        assert!(result.ok, "{}", result.detail);
        assert!(result.detail.contains("legacy GET returned HTTP 200"));
    }

    #[tokio::test]
    async fn health_client_uses_manual_proxy() {
        let proxy_app = Router::new().fallback(|| async { reqwest::StatusCode::OK });
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind fake proxy");
        let proxy_address = listener.local_addr().expect("fake proxy address");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, proxy_app).await;
        });
        let proxy = crate::settings::ProxyConfig {
            mode: "manual".into(),
            url: format!("http://{proxy_address}"),
        };

        let client = http_client_with_proxy(&proxy).expect("manual proxy client");
        let result =
            probe_legacy_endpoint(&client, "http://health-probe.invalid/mcp", ProbeAuth::None)
                .await;
        handle.abort();

        assert!(result.ok, "{}", result.detail);
    }

    #[tokio::test]
    async fn health_client_allows_response_beyond_legacy_three_second_limit() {
        let app = Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(3_200)).await;
                reqwest::StatusCode::OK
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind slow health server");
        let address = listener.local_addr().expect("slow health address");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let result = probe_legacy_endpoint(
            &http_client(),
            &format!("http://{address}/slow"),
            ProbeAuth::None,
        )
        .await;
        handle.abort();

        assert!(result.ok, "{}", result.detail);
    }

    #[test]
    fn health_items_keep_legacy_fields_and_share_trace_identity() {
        let item = health_item(
            "mcp.handshake",
            "handshake",
            "MCP 握手",
            "warn",
            "OAuth user token required".into(),
            "Authorize before retrying.",
            "trace-placeholder",
        );

        assert!(item.ok);
        assert_eq!(item.key, "mcp.handshake");
        assert_eq!(item.layer, "handshake");
        assert_eq!(item.status, "warn");
        assert_eq!(item.trace_id, "trace-placeholder");
    }
}
