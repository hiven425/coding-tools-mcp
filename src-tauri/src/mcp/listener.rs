use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Form, Query, State};
use axum::http::{
    header::{ACCEPT, CACHE_CONTROL},
    HeaderMap, StatusCode,
};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::oneshot;
use tower_http::cors::CorsLayer;

use crate::activity::ActivityStore;
use crate::auth::{
    authorization_server_metadata, authorize_get, authorize_post, external_base_url,
    protected_resource_metadata, protected_resource_metadata_url, token_exchange,
    verify_bearer_header, verify_oauth_bearer_header_with_metadata, AuthorizeForm, AuthorizeParams,
    OAuthRuntime, TokenForm,
};
use crate::mcp::server::{handle_request, new_state, SharedState};
use crate::secret::SecretStore;
use crate::tools::policy::PolicySettings;
use crate::tools::Workspace;
use crate::tunnel::{append_profile_log, new_trace_id, sanitize_log_line};
use crate::workspace::{AuthConfig, RuntimeConfig};

pub type ShutdownSender = oneshot::Sender<()>;
const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonRpcEnvelope {
    Request,
    Notification,
    Response,
}

#[derive(Clone)]
struct ListenerState {
    mcp: SharedState,
    auth: AuthConfig,
    workspace_id: String,
    workspace_name: String,
    workspace_path: String,
    bind_port: u16,
    configured_public_url: String,
    bearer_token: Option<String>,
    oauth: Option<Arc<OAuthRuntime>>,
    oauth_client_secret: Option<String>,
    transport_v2: bool,
    activity: Arc<ActivityStore>,
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_listener(
    port: u16,
    workspace_path: PathBuf,
    workspace_id: String,
    workspace_name: String,
    auth: AuthConfig,
    public_base_url: String,
    oauth_client_secret: Option<String>,
    oauth_password: Option<String>,
    oauth_token_secret: Option<String>,
    runtime: RuntimeConfig,
    transport_v2: bool,
    activity: Arc<ActivityStore>,
) -> Result<(ShutdownSender, tauri::async_runtime::JoinHandle<()>), String> {
    let workspace_display = workspace_path.display().to_string();
    let workspace = Workspace::new(workspace_path).map_err(|e| e.message())?;
    let policy = PolicySettings::from_runtime(&runtime);
    let mcp = new_state(
        workspace,
        auth.clone(),
        policy,
        runtime.tool_profile.clone(),
        runtime.permission_mode.clone(),
    );
    let bearer_token = if auth.bearer_enabled() {
        let key = "bearer_token";
        if auth.use_shared_secrets {
            SecretStore::get_shared(key).map_err(|e| e.to_string())?
        } else {
            SecretStore::get(&workspace_id, key).map_err(|e| e.to_string())?
        }
    } else {
        None
    };
    let configured_public_url = public_base_url.trim().to_string();
    let oauth = if auth.oauth_enabled() {
        let password = oauth_password.unwrap_or_default();
        let token_secret = oauth_token_secret.unwrap_or_default();
        let oauth_base = external_base_url(&HeaderMap::new(), port, &configured_public_url);
        Some(Arc::new(OAuthRuntime::new(
            oauth_base,
            auth.oauth_client_id.clone(),
            oauth_client_secret.clone(),
            password,
            token_secret,
        )))
    } else {
        None
    };
    let state = ListenerState {
        mcp,
        auth,
        workspace_id,
        workspace_name,
        workspace_path: workspace_display,
        bind_port: port,
        configured_public_url,
        bearer_token,
        oauth,
        oauth_client_secret,
        transport_v2,
        activity,
    };
    let trace_id = new_trace_id();
    let started_at = Instant::now();
    append_profile_log(
        &state.workspace_id,
        "stderr.log",
        &format!("[trace={trace_id}] stage=local-starting port={port}"),
    );
    // 在返回 Running 之前完成 bind，避免后台任务里的端口冲突被伪装成启动成功。
    let listener = match bind_listener(port) {
        Ok(listener) => listener,
        Err(error) => {
            append_profile_log(
                &state.workspace_id,
                "stderr.log",
                &format!(
                    "[trace={trace_id}] stage=local-error elapsed_ms={} error={error}",
                    started_at.elapsed().as_millis()
                ),
            );
            return Err(error);
        }
    };
    append_profile_log(
        &state.workspace_id,
        "stderr.log",
        &format!(
            "[trace={trace_id}] stage=local-ready elapsed_ms={}",
            started_at.elapsed().as_millis()
        ),
    );
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let profile_id = state.workspace_id.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let result = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => serve(listener, port, state, shutdown_rx).await,
            Err(error) => Err(format!("MCP 本地监听器初始化失败: {error}").into()),
        };
        if let Err(err) = &result {
            append_profile_log(
                &profile_id,
                "stderr.log",
                &format!(
                    "[trace={trace_id}] stage=local-error elapsed_ms={} error={err}",
                    started_at.elapsed().as_millis()
                ),
            );
            eprintln!(
                "mcp listener stopped: {}",
                sanitize_log_line(&err.to_string())
            );
        } else {
            append_profile_log(
                &profile_id,
                "stderr.log",
                &format!(
                    "[trace={trace_id}] stage=local-stopped elapsed_ms={}",
                    started_at.elapsed().as_millis()
                ),
            );
        }
    });
    Ok((shutdown_tx, handle))
}

async fn serve(
    listener: tokio::net::TcpListener,
    port: u16,
    state: ListenerState,
    shutdown: oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let profile_id = state.workspace_id.clone();
    let app = listener_router(state);

    append_profile_log(
        &profile_id,
        "stdout.log",
        &format!("[mcp] listening on http://127.0.0.1:{port}/mcp"),
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = shutdown.await;
        })
        .await?;
    Ok(())
}

fn listener_router(state: ListenerState) -> Router {
    let transport_v2 = state.transport_v2;
    let mcp_route = if transport_v2 {
        post(mcp_post)
    } else {
        get(mcp_discovery).post(mcp_post)
    };
    let router = Router::new()
        .route("/mcp", mcp_route)
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_authorization_server_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(oauth_protected_resource_metadata),
        )
        .route(
            "/oauth/authorize",
            get(oauth_authorize_get).post(oauth_authorize_post),
        )
        .route("/oauth/token", post(oauth_token_post));
    let router = if transport_v2 {
        router.route(
            "/.well-known/oauth-protected-resource/mcp",
            get(oauth_protected_resource_metadata),
        )
    } else {
        router
    };
    router.with_state(state).layer(CorsLayer::permissive())
}

fn bind_listener(port: u16) -> Result<std::net::TcpListener, String> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = std::net::TcpListener::bind(addr)
        .map_err(|err| format!("MCP 本地端口 {port} 绑定失败: {err}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|err| format!("MCP 本地端口 {port} 设置非阻塞失败: {err}"))?;
    Ok(listener)
}

fn resolve_oauth_base(state: &ListenerState, headers: &HeaderMap) -> String {
    let resolved = external_base_url(headers, state.bind_port, &state.configured_public_url);
    if !state.configured_public_url.is_empty()
        && (headers.contains_key("x-forwarded-host") || headers.contains_key("forwarded"))
        && external_base_url(headers, state.bind_port, "") != resolved
    {
        append_profile_log(
            &state.workspace_id,
            "stderr.log",
            "[oauth] ignored proxy origin because configured canonical origin takes precedence",
        );
    }
    resolved
}

async fn mcp_post(
    State(state): State<ListenerState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Some(response) = require_mcp_auth(&state, &headers) {
        return response;
    }
    let envelope = if state.transport_v2 {
        if !accepts_json(&headers) {
            return mcp_http_error(
                StatusCode::NOT_ACCEPTABLE,
                "not_acceptable",
                "Accept header must allow application/json",
            );
        }
        if !supported_protocol_version(&headers) {
            return mcp_http_error(
                StatusCode::BAD_REQUEST,
                "unsupported_protocol_version",
                "Unsupported MCP protocol version",
            );
        }
        match classify_json_rpc(&body) {
            Ok(envelope) => envelope,
            Err(message) => {
                return mcp_http_error(StatusCode::BAD_REQUEST, "invalid_json_rpc", message)
            }
        }
    } else {
        JsonRpcEnvelope::Request
    };
    if envelope == JsonRpcEnvelope::Response {
        return StatusCode::ACCEPTED.into_response();
    }

    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let request_id = body.get("id").cloned().unwrap_or(Value::Null);
    let tool_name = body
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    append_profile_log(
        &state.workspace_id,
        "mcp-requests.log",
        &format!(
            "[rpc] request id={} method={} tool={}",
            request_id, method, tool_name
        ),
    );

    let activity_trace_id =
        state
            .activity
            .begin_trace(&state.workspace_id, &state.workspace_name, "/mcp", &body);
    let mcp = state.mcp.clone();
    let profile_id = state.workspace_id.clone();
    let activity = state.activity.clone();
    let result = tokio::task::spawn_blocking(move || handle_request(&mcp, &body)).await;
    match result {
        Ok(response) => {
            activity.complete_trace(&activity_trace_id, &response);
            append_profile_log(
                &profile_id,
                "mcp-requests.log",
                &format!(
                    "[rpc] completed id={} method={} tool={}",
                    request_id, method, tool_name
                ),
            );
            if tool_name == "exec_command" || tool_name == "exec_health_check" {
                let structured = response
                    .get("result")
                    .and_then(|result| result.get("structuredContent"));
                let status = structured
                    .and_then(|value| value.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let termination_reason = structured
                    .and_then(|value| value.get("termination_reason"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let exit_code = structured
                    .and_then(|value| value.get("exit_code"))
                    .map(Value::to_string)
                    .unwrap_or_default();
                let is_error = response
                    .get("result")
                    .and_then(|result| result.get("isError"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                append_profile_log(
                    &profile_id,
                    "mcp-requests.log",
                    &format!(
                        "[exec] id={} tool={} is_error={} status={} termination_reason={} exit_code={}",
                        request_id, tool_name, is_error, status, termination_reason, exit_code
                    ),
                );
            }
            if envelope == JsonRpcEnvelope::Notification {
                StatusCode::ACCEPTED.into_response()
            } else {
                Json(response).into_response()
            }
        }
        Err(error) => {
            activity.fail_trace(&activity_trace_id, &error.to_string());
            append_profile_log(
                &profile_id,
                "mcp-requests.log",
                &format!(
                    "[rpc] worker_failed id={} method={} tool={} error={error}",
                    request_id, method, tool_name
                ),
            );
            if envelope == JsonRpcEnvelope::Notification {
                StatusCode::ACCEPTED.into_response()
            } else {
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {
                        "code": -32603,
                        "message": "Exec RPC worker failed",
                        "data": {
                            "stage": "rpc_worker",
                            "reason": "worker_failed",
                            "retryable": true,
                            "suggestion": "重试请求或重启 MCP 运行时"
                        }
                    }
                }))
                .into_response()
            }
        }
    }
}

async fn mcp_discovery() -> Response {
    ([(CACHE_CONTROL, "no-store")], Json(mcp_discovery_payload())).into_response()
}

fn mcp_discovery_payload() -> Value {
    json!({
        "name": "coding-tools-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": MCP_PROTOCOL_VERSION
    })
}

fn accepts_json(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(ACCEPT) else {
        return true;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    value.split(',').any(|item| {
        matches!(
            item.trim().split(';').next().unwrap_or(""),
            "application/json" | "application/*" | "*/*"
        )
    })
}

fn supported_protocol_version(headers: &HeaderMap) -> bool {
    headers
        .get(MCP_PROTOCOL_VERSION_HEADER)
        .map(|value| value.as_bytes() == MCP_PROTOCOL_VERSION.as_bytes())
        .unwrap_or(true)
}

fn classify_json_rpc(body: &Value) -> Result<JsonRpcEnvelope, &'static str> {
    let Some(object) = body.as_object() else {
        return Err("JSON-RPC body must be an object");
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("jsonrpc must be 2.0");
    }

    if let Some(method) = object.get("method") {
        if method.as_str().is_none_or(str::is_empty) {
            return Err("method must be a non-empty string");
        }
        return Ok(match object.get("id") {
            Some(id) if !id.is_null() => JsonRpcEnvelope::Request,
            _ => JsonRpcEnvelope::Notification,
        });
    }

    let has_id = object.get("id").is_some_and(|id| !id.is_null());
    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");
    if has_id && has_result != has_error {
        return Ok(JsonRpcEnvelope::Response);
    }
    Err("body is not a JSON-RPC request, notification, or response")
}

fn mcp_http_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({
            "jsonrpc": "2.0",
            "id": Value::Null,
            "error": { "code": -32600, "message": message, "data": { "reason": code } }
        })),
    )
        .into_response()
}

fn require_mcp_auth(state: &ListenerState, headers: &HeaderMap) -> Option<Response> {
    if state.auth.bearer_enabled() {
        let expected = state.bearer_token.as_deref().unwrap_or("");
        return verify_bearer_header(headers, expected);
    }
    if state.auth.oauth_enabled() {
        if let Some(oauth) = state.oauth.as_ref() {
            let server_url = resolve_oauth_base(state, headers);
            if state.transport_v2 {
                let metadata_url = protected_resource_metadata_url(&server_url);
                return verify_oauth_bearer_header_with_metadata(
                    headers,
                    oauth,
                    &server_url,
                    &metadata_url,
                );
            }
            return crate::auth::verify_oauth_bearer_header(headers, oauth, &server_url);
        }
    }
    None
}

async fn oauth_authorization_server_metadata(
    State(state): State<ListenerState>,
    headers: HeaderMap,
) -> Response {
    if !state.auth.oauth_enabled() {
        return oauth_not_configured();
    }
    let base = resolve_oauth_base(&state, &headers);
    Json(authorization_server_metadata(
        &base,
        state.oauth_client_secret.as_deref(),
    ))
    .into_response()
}

async fn oauth_protected_resource_metadata(
    State(state): State<ListenerState>,
    headers: HeaderMap,
) -> Response {
    if !state.auth.oauth_enabled() {
        return oauth_not_configured();
    }
    Json(protected_resource_metadata(&resolve_oauth_base(
        &state, &headers,
    )))
    .into_response()
}

async fn oauth_authorize_get(
    State(state): State<ListenerState>,
    Query(params): Query<AuthorizeParams>,
) -> Response {
    let Some(oauth) = state.oauth.as_ref() else {
        return oauth_not_configured();
    };
    authorize_get(oauth, params, Some(state.workspace_path.as_str()))
}

async fn oauth_authorize_post(
    State(state): State<ListenerState>,
    headers: HeaderMap,
    Form(form): Form<AuthorizeForm>,
) -> Response {
    let Some(oauth) = state.oauth.as_ref() else {
        return oauth_not_configured();
    };
    authorize_post(oauth, form, &resolve_oauth_base(&state, &headers))
}

async fn oauth_token_post(
    State(state): State<ListenerState>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> Response {
    let Some(oauth) = state.oauth.as_ref() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "unsupported_grant_type" })),
        )
            .into_response();
    };
    token_exchange(oauth, &headers, form, &resolve_oauth_base(&state, &headers))
}

fn oauth_not_configured() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "OAuth not configured" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{
        header::{CACHE_CONTROL, CONTENT_TYPE, WWW_AUTHENTICATE},
        Method, Request, StatusCode,
    };
    use serde_json::json;
    use tower::ServiceExt;

    use super::{bind_listener, listener_router, ListenerState, MCP_PROTOCOL_VERSION_HEADER};
    use crate::tools::ToolContext;
    use crate::workspace::AuthConfig;

    fn test_state() -> (ListenerState, tempfile::TempDir, tempfile::TempDir) {
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        let harness = tempfile::tempdir().expect("harness tempdir");
        let mcp = Arc::new(
            ToolContext::for_test(workspace.path().to_path_buf(), harness.path().to_path_buf())
                .expect("tool context"),
        );
        let mut auth = AuthConfig::default();
        auth.auth_type = "noauth".into();
        (
            ListenerState {
                mcp,
                auth,
                workspace_id: "workspace-test".into(),
                workspace_name: "Workspace Test".into(),
                workspace_path: workspace.path().to_string_lossy().into_owned(),
                bind_port: 28_766,
                configured_public_url: "https://fixed.example.invalid".into(),
                bearer_token: None,
                oauth: None,
                oauth_client_secret: None,
                transport_v2: true,
                activity: Arc::new(crate::activity::ActivityStore::new()),
            },
            workspace,
            harness,
        )
    }

    fn mcp_request(method: Method, body: Option<serde_json::Value>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri("/mcp");
        let body = if let Some(body) = body {
            builder = builder.header(CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        } else {
            Body::empty()
        };
        builder.body(body).expect("request")
    }

    fn oauth_test_state() -> (ListenerState, tempfile::TempDir, tempfile::TempDir) {
        let (mut state, workspace, harness) = test_state();
        state.auth.auth_type = "oauth".into();
        state.oauth = Some(Arc::new(crate::auth::OAuthRuntime::new(
            state.configured_public_url.clone(),
            "client-placeholder".into(),
            None,
            "password-placeholder".into(),
            "signing-secret-placeholder".into(),
        )));
        (state, workspace, harness)
    }

    #[test]
    fn bind_listener_reports_port_conflict_synchronously() {
        let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("占用测试端口");
        let port = occupied.local_addr().expect("读取测试端口").port();

        assert!(bind_listener(port).is_err());
    }

    #[tokio::test]
    async fn get_mcp_returns_method_not_allowed_without_sse() {
        let (state, _workspace, _harness) = test_state();
        let response = listener_router(state)
            .oneshot(mcp_request(Method::GET, None))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn feature_flag_off_restores_legacy_discovery_and_notification_response() {
        let (mut state, _workspace, _harness) = test_state();
        state.transport_v2 = false;
        let app = listener_router(state);

        let response = app
            .clone()
            .oneshot(mcp_request(Method::GET, None))
            .await
            .expect("legacy discovery response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");

        let response = app
            .oneshot(mcp_request(
                Method::POST,
                Some(json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized"
                })),
            ))
            .await
            .expect("legacy notification response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("legacy response body")
            .is_empty());
    }

    #[tokio::test]
    async fn request_returns_json_while_notification_and_response_return_empty_202() {
        let (state, _workspace, _harness) = test_state();
        let app = listener_router(state);
        let request = json!({"jsonrpc":"2.0","id":1,"method":"ping"});
        let response = app
            .clone()
            .oneshot(mcp_request(Method::POST, Some(request)))
            .await
            .expect("request response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "application/json");

        for body in [
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":1,"result":{}}),
        ] {
            let response = app
                .clone()
                .oneshot(mcp_request(Method::POST, Some(body)))
                .await
                .expect("accepted response");
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            assert!(to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body")
                .is_empty());
        }
    }

    #[tokio::test]
    async fn transport_rejects_unsupported_content_accept_and_protocol_version() {
        let (state, _workspace, _harness) = test_state();
        let app = listener_router(state);
        let request_body = json!({"jsonrpc":"2.0","id":1,"method":"ping"});

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mcp")
                    .body(Body::from(request_body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("content type response");
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let mut request = mcp_request(Method::POST, Some(request_body.clone()));
        request
            .headers_mut()
            .insert("accept", "text/plain".parse().unwrap());
        let response = app.clone().oneshot(request).await.expect("accept response");
        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);

        let mut request = mcp_request(Method::POST, Some(request_body));
        request
            .headers_mut()
            .insert(MCP_PROTOCOL_VERSION_HEADER, "2099-01-01".parse().unwrap());
        let response = app.oneshot(request).await.expect("protocol response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn transport_rejects_invalid_json_rpc_before_dispatch() {
        let (state, _workspace, _harness) = test_state();
        let response = listener_router(state)
            .oneshot(mcp_request(
                Method::POST,
                Some(json!({"jsonrpc":"1.0","id":1,"method":"tools/call"})),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn oauth_401_challenge_points_to_path_aware_metadata() {
        let (state, _workspace, _harness) = oauth_test_state();
        let response = listener_router(state)
            .oneshot(mcp_request(
                Method::POST,
                Some(json!({"jsonrpc":"2.0","id":1,"method":"ping"})),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()[WWW_AUTHENTICATE],
            "Bearer resource_metadata=\"https://fixed.example.invalid/.well-known/oauth-protected-resource/mcp\""
        );
    }

    #[tokio::test]
    async fn oauth_metadata_routes_keep_canonical_identity_despite_forwarded_headers() {
        let (state, _workspace, _harness) = oauth_test_state();
        let app = listener_router(state);

        for path in [
            "/.well-known/oauth-protected-resource",
            "/.well-known/oauth-protected-resource/mcp",
        ] {
            let request = Request::builder()
                .uri(path)
                .header("x-forwarded-proto", "http")
                .header("x-forwarded-host", "conflict.example.invalid")
                .body(Body::empty())
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("metadata body");
            let metadata: serde_json::Value = serde_json::from_slice(&body).expect("metadata json");
            assert_eq!(metadata["resource"], "https://fixed.example.invalid/mcp");
            assert_eq!(
                metadata["authorization_servers"],
                json!(["https://fixed.example.invalid"])
            );
        }

        let request = Request::builder()
            .uri("/.well-known/oauth-authorization-server")
            .header("x-forwarded-proto", "http")
            .header("x-forwarded-host", "conflict.example.invalid")
            .body(Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("response");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("metadata body");
        let metadata: serde_json::Value = serde_json::from_slice(&body).expect("metadata json");
        assert_eq!(metadata["issuer"], "https://fixed.example.invalid");
        assert_eq!(
            metadata["token_endpoint"],
            "https://fixed.example.invalid/oauth/token"
        );
    }
}
