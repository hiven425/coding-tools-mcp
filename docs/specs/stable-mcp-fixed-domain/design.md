# 设计文档：stable-mcp-fixed-domain

## 概述

保持当前每工作区工具内核和本地 listener，把“配置解析、listener、Named Tunnel、OAuth identity、远端握手”建模为一条组合连接状态。固定域名覆盖只在工作区选择 Cloudflare Named 模式时生效；多个工作区不能同时声明同一固定入口。

## 设计原则

1. canonical identity 与临时进程状态分离：域名不因 listener/tunnel 重启改变。
2. local-ready 与 public-ready 分离：公网失败不伪装成本地失败，也不把本地成功伪装成公网成功。
3. transport 与工具内核分离：协议层可以切换到 SDK，工具 schema 和 dispatch 保持不变。
4. secret 在边界解析、在边界脱敏，不进入普通 DTO、日志或规格。

## 对应需求

- 配置、固定域名和隧道生命周期：FR-1、FR-2、FR-3、FR-4。
- Streamable HTTP 与 OAuth：FR-5、FR-6、FR-7。
- 健康检查与可观测性：FR-8、FR-9。
- Windows、回归与迁移回滚：FR-10、FR-11、FR-12。

## 技术方案

### 总体架构

```text
process env / ignored .env / existing settings + SecretStore
                         |
              FixedDomainConfigProvider
                         |
                CanonicalEndpointSet
       origin / mcp / issuer / resource / metadata
                         |
          ConnectionOrchestrator(workspace, MCP)
             /                         \
 RuntimeSupervisor                 TunnelSupervisor
 local listener                    cloudflared named
             \                         /
               ConnectionStateMachine
                         |
      config -> local -> public -> oauth -> MCP handshake
                         |
          RuntimeStatusDto / TunnelStatus / Health UI
```

## 配置与 secret 边界

新增 `settings/fixed_domain.rs`，以结构化解析器读取 `.env`，不手写按行拆分。建议使用 `dotenvy` 的 iterator API，不调用会全局污染进程环境的批量加载函数。

配置优先级：

1. 当前进程的 `cloudflare_host_name` / `cloudflare_token`。
2. 显式项目根目录下被 Git 忽略的 `.env`。
3. 工作区 `tunnel.public_url` 与既有 `SecretStore::get(workspace_id, "cloudflare_token")`。

仅当工作区 `tunnel_type=cloudflare` 且 `cloudflare_mode=named` 时使用环境覆盖。Token 不自动写回；canonical origin 可以进入现有连接 UI，但 `.env` 原始行和配置来源路径不进入远端 DTO。

`cloudflare_host_name` 接受裸主机名或 HTTPS origin。使用 `reqwest::Url` 规范化，最终值必须满足：HTTPS、host 非空、path 为 `/`、无 userinfo/query/fragment。`/mcp`、OAuth 和 metadata 地址统一由 `CanonicalEndpointSet` 派生。

## 连接状态机

```text
stopped
  -> local-starting
  -> local-ready
  -> public-starting
  -> public-ready

public-starting/public-ready
  -> public-degraded
  -> public-recovering
  -> public-ready | public-error

任意运行态
  -> stopping
  -> stopped
```

状态约束：

| 状态 | 本地 listener | cloudflared | 远端握手 |
|---|---|---|---|
| `local-ready` | 通过 | 未要求或失败 | 未通过 |
| `public-starting` | 通过 | 启动中 | 未执行 |
| `public-ready` | 通过 | edge 已注册 | 通过 |
| `public-degraded` | 通过 | 不确定/失败 | 失败 |
| `public-error` | 可用或失败 | 恢复耗尽 | 失败 |

恢复策略使用带抖动的有界指数退避，例如 1s、2s、4s、8s、16s，单轮最多 5 次，之后进入 90s 冷却。测试注入 clock/jitter，不依赖真实时间。最终数值在实现时作为常量集中定义并写入状态 DTO。

## 生命周期编排

### 启动

1. 解析并校验固定域名配置，不创建子进程。
2. bind listener，完成本地 initialize smoke。
3. 启动 cloudflared named，并强制 `--protocol http2`。
4. 并发读取 stdout/stderr；只有 `registered tunnel connection` 可发出 ready。
5. 对公网入口验证 TLS/OAuth challenge/MCP 握手后进入 public-ready。

### listener 重启

1. 快照当前 tunnel handle、PID、canonical URL 和状态。
2. 保留健康 cloudflared，停止旧 listener，等待端口释放。
3. 启动新 listener并完成本地握手。
4. 成功后复用 tunnel；失败时尝试恢复旧 listener，否则明确进入 error。

### 显式停止

显式停止、工作区删除、固定域名或 Token 变化会停止 cloudflared。普通 listener 配置刷新不得自动销毁健康 tunnel。进程停止仍使用 PID/镜像路径归属检查，避免影响外部 cloudflared。

## Streamable HTTP 设计

先做 `rmcp` 兼容性 spike。通过条件：

- 支持无状态 Streamable HTTP 和 Axum 集成。
- 可以挂接现有 bearer/OAuth middleware。
- 可以复用现有 `list_tools_for_profile`、`call_tool` 和 `wrap_mcp_tool_result`。
- 不改变公开工具 schema、结构化结果和错误契约。

满足条件时由 SDK 负责 transport/lifecycle；不满足时保留现有 JSON-RPC dispatch，仅新增 `HttpMcpAdapter` 处理 header、状态码和空响应。

协议响应矩阵：

| 输入 | 预期响应 |
|---|---|
| GET `/mcp`，未实现 SSE | `405`，无 discovery JSON |
| 合法 JSON-RPC request | `200 application/json` 或协商后的 SSE |
| 合法 notification/response | `202`，空 body |
| 非 JSON Content-Type | `415` |
| 不可接受的 Accept | `406` |
| 无效/不支持的 `MCP-Protocol-Version` | `400` |
| 未认证受保护请求 | `401` + `WWW-Authenticate` |

服务端不分配 `Mcp-Session-Id`，保持无状态 transport；ChatGPT 的 `_meta.openai/session` 继续只作为业务历史会话输入。

## OAuth discovery

固定 canonical origin 优先于转发 header，避免 tunnel 重启改变 issuer/audience。提供：

- `/.well-known/oauth-authorization-server`
- `/.well-known/oauth-protected-resource`
- `/.well-known/oauth-protected-resource/mcp`
- `/oauth/authorize`
- `/oauth/token`

401 challenge 的 `resource_metadata` 必须指向实际提供的 metadata。根级和 path-aware payload 的 `resource` 都必须与既定兼容策略一致，并由契约测试锁定。

## 健康检查

`HealthItem` 扩展 `key`、`layer`、`status`、`trace_id`、`retryable`，保留 `ok/detail/hint` 兼容字段。检查顺序：

1. `config`：固定域名、Token 存在性、域名冲突。
2. `local`：端口、initialize、initialized 202、tools/list。
3. `public`：DNS/TLS、未认证 401/405 语义、provider 状态。
4. `oauth`：authorization/protected-resource metadata 和 canonical URL。
5. `handshake`：使用测试凭据或内部受限探针完成远端 MCP 初始化；无法安全认证时明确 `skip`，不得当作通过。

聚合状态由必需项计算，不由单一 GET 状态码推断。

## 日志与脱敏

新增集中 `RedactedFields`/`sanitize_log_value`，在格式化前处理：

- `Authorization`、Bearer、OAuth code/token、Client Secret。
- `cloudflare_token` 和完整 `.env` 行。
- cloudflared 命令行中的 `--token` 后继参数。

canonical URL 可以在连接 UI 中展示；日志默认只记录 host 的稳定哈希或 `<configured-host>`。每次连接操作生成 trace ID，日志至少包含 workspace、service、stage、state transition、elapsed 和 retry count。

## 文件结构

| 路径 | 变更 |
|---|---|
| `src-tauri/Cargo.toml`、`Cargo.lock` | `dotenvy`；通过 spike 后可加入 `rmcp` |
| `src-tauri/src/settings/fixed_domain.rs` | 新增配置解析和 canonical endpoint |
| `src-tauri/src/settings/mod.rs` | 导出配置 provider |
| `src-tauri/src/workspace/model.rs` | 兼容状态/配置扩展，新增字段必须有默认值 |
| `src-tauri/src/tunnel/cloudflare.rs` | HTTP/2、真就绪、退出/超时清理 |
| `src-tauri/src/tunnel/supervisor.rs` | 连接状态和有界恢复 |
| `src-tauri/src/tunnel/access.rs` | 自动启动错误向组合状态传播 |
| `src-tauri/src/commands/runtime.rs` | listener 重启保留 tunnel |
| `src-tauri/src/mcp/listener.rs` | transport/SDK adapter、metadata 路由 |
| `src-tauri/src/mcp/server.rs` | 保留工具 dispatch，移除 transport 状态码职责 |
| `src-tauri/src/auth/*` | canonical identity 与 challenge |
| `src-tauri/src/health/checker.rs` | 分层握手探针 |
| `src-tauri/src/commands/health.rs` | 新状态 DTO |
| `src/lib/api/health.ts`、`HealthPanel.svelte` | 分层状态展示 |
| `src-tauri/tests/` | HTTP/OAuth/health 集成测试 |
| `docs/` | 固定域名部署、Windows 常驻和回滚说明 |

## 兼容与回滚

- 新逻辑只对 named + fixed-domain 配置生效；Quick、FRP 和手工 URL 保持原路径。
- transport 切换使用 workspace `TunnelConfig.mcp_transport_v2` 临时 feature flag，serde 默认 `false`，稳定后再移除旧实现。
- 新增持久化字段使用 serde 默认值；Token 不迁移。
- 回滚只切换实现开关并重启 listener，不删除工作区、secret 或历史会话。

## 风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| SDK 与现有手写工具结果不兼容 | 高 | 先做 spike 和 golden contract，失败时仅实现薄 adapter |
| 保留 tunnel 时 listener 短暂不可达 | 中 | 本地握手后再提交状态，公网探针确认恢复 |
| `.env` 在安装包环境不可定位 | 中 | `.env` 仅作显式项目根覆盖；生产继续支持环境变量和 SecretStore |
| 一个固定域名被多个工作区使用 | 高 | 启动前资源冲突校验，只允许单一 owner |
| Windows GUI 进程退出导致服务消失 | 中 | 文档明确登录会话边界；后续 headless/Service 单独立项 |
