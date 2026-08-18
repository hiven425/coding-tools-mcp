# 需求文档：stable-mcp-fixed-domain

## 功能概述

为当前 Rust/Tauri Coding Tools MCP 增加固定域名连接模式。系统从安全配置源解析 Cloudflare Named Tunnel 的固定主机名和 Token，稳定管理本地 MCP listener、公网隧道、OAuth identity、协议握手与健康状态，避免“本地已启动但公网不可用”“隧道尚未连边却显示运行”和“GET 可达但 MCP POST 不兼容”等假成功。

## 当前问题

- `src-tauri/src/tunnel/cloudflare.rs` 把 metrics server 启动日志也视为 Named Tunnel 就绪，早于真实 edge connection。
- `src-tauri/src/commands/runtime.rs` 重启 MCP 时先停止对应隧道，固定域名仍会产生不必要断连。
- 隧道自动启动错误只写 stderr，本地 runtime 仍可能显示为 running。
- `src-tauri/src/health/checker.rs` 只做 GET/状态码检查，没有执行 MCP 初始化握手。
- `src-tauri/src/mcp/listener.rs` 和 `server.rs` 是手写传输层，当前 GET、notification 和协议版本处理不完全符合 MCP `2025-06-18`。

## 术语

- **canonical origin**：固定公网 HTTPS 源，例如 `https://mcp.example.invalid`，不包含 `/mcp`、查询或片段。
- **effective endpoint**：由 canonical origin 派生的实际 MCP 地址 `<origin>/mcp`。
- **真就绪**：本地 listener 正常、cloudflared 子进程存活，并观察到 `registered tunnel connection`。
- **public-degraded**：本地 MCP 可用，但公网隧道、TLS、OAuth 或远端 MCP 握手至少一层失败。

## 范围边界

**In Scope**

- 当前每工作区 MCP 模型中的 Cloudflare Named Tunnel 固定域名模式。
- `.env`/进程环境覆盖、canonical URL、secret 边界和同域名资源冲突。
- listener 与 tunnel 生命周期解耦、有界恢复和明确状态。
- MCP `2025-06-18` Streamable HTTP 与 OAuth metadata 兼容。
- 分层健康检查、脱敏日志、Windows 11 运行边界和回归测试。

**Out of Scope**

- 不把当前桌面端重构为单一多工作区 Gateway。
- 不新增旧式 SSE transport；不支持服务端主动消息时，`GET /mcp` 返回 `405`。
- 不自动创建 Cloudflare DNS、Named Tunnel 或远端 Access 策略。
- 不把 `.env`、Token 或 OAuth secret 纳入 Git。
- 不承诺 Windows 未登录场景的原生 Service；无用户会话常驻属于后续 headless 交付。

## 需求列表

### FR-1：安全解析固定域名配置

**优先级：** Must

#### 验收标准（EARS）

1. WHEN 启动 Cloudflare Named Tunnel THEN 系统 SHALL 按“进程环境、项目根 `.env`、现有工作区/SecretStore 配置”的顺序解析覆盖值。
2. IF `cloudflare_host_name` 或 Named Tunnel 所需的 `cloudflare_token` 缺失或为空 THEN 系统 SHALL 在创建子进程前返回缺失变量名，不返回任何已存在的值。
3. WHEN 记录日志、错误、trace 或测试快照 THEN 系统 SHALL 永不输出 `cloudflare_token`、Authorization、Client Secret 或原始 `.env` 行。
4. WHEN 环境覆盖生效 THEN 系统 SHALL 不把 Token 自动复制到工作区 JSON 或其他明文持久化文件。

### FR-2：唯一 canonical HTTPS identity

**优先级：** Must

#### 验收标准（EARS）

1. WHEN `cloudflare_host_name` 是合法裸主机名或 HTTPS origin THEN 系统 SHALL 规范化为无尾斜杠的 canonical origin。
2. IF 输入包含非 HTTPS scheme、用户凭据、额外路径、查询或片段 THEN 系统 SHALL 拒绝启动并返回不包含原始敏感输入的校验错误。
3. WHEN 生成 MCP、issuer、resource 和 metadata URL THEN 系统 SHALL 只从同一 canonical origin 派生。
4. IF 两个工作区同时声明同一固定域名 THEN 系统 SHALL 阻止第二条线路启动并指出冲突工作区，不并行争用同一入口。

### FR-3：Cloudflare Named Tunnel 真就绪

**优先级：** Must

#### 验收标准（EARS）

1. WHEN 启动 Named Tunnel THEN 系统 SHALL 向 cloudflared 传递 `--protocol http2`。
2. WHILE 仅观察到 metrics server 或其他初始化日志 THEN 系统 SHALL 保持 `public-starting`，不得报告 ready。
3. WHEN 首次观察到 `registered tunnel connection` 且子进程仍存活 THEN 系统 SHALL 进入 `public-ready`。
4. IF 就绪超时或子进程提前退出 THEN 系统 SHALL 返回失败阶段、退出状态和日志路径，并清理新建子进程。

### FR-4：解耦生命周期与有界恢复

**优先级：** Must

#### 验收标准（EARS）

1. WHEN 仅重启 MCP listener 且现有 Named Tunnel 健康 THEN 系统 SHALL 保留 cloudflared 子进程和 canonical identity。
2. IF 新 listener 无法绑定或完成本地握手 THEN 系统 SHALL 恢复旧 listener/状态，或明确进入 error；不得静默丢失公网线路。
3. IF 隧道进程退出或公网探针连续失败 THEN 系统 SHALL 进入 `public-degraded`，使用有界指数退避和冷却尝试恢复。
4. WHEN 用户显式停止 MCP、删除工作区或修改固定域名/Token THEN 系统 SHALL 停止旧隧道并释放其进程归属记录。
5. IF 自动启动隧道失败 THEN 系统 SHALL 保留 `local-ready` 事实，同时把公网状态标记为失败，不得仅写 stderr 后返回公网可用。

### FR-5：Streamable HTTP 响应合规

**优先级：** Must

#### 验收标准（EARS）

1. WHEN 服务端接受 JSON-RPC notification 或 response THEN 系统 SHALL 返回 `202 Accepted` 且 body 为空。
2. WHEN 客户端 GET `/mcp` 且服务端未实现 SSE THEN 系统 SHALL 返回 `405 Method Not Allowed`。
3. WHEN POST `/mcp` 收到受支持 JSON-RPC request THEN 系统 SHALL 根据协商返回 `application/json` 或规范允许的 SSE 响应。
4. IF `Accept`、`Content-Type` 或消息结构不满足支持范围 THEN 系统 SHALL 返回确定的 `4xx`，不得进入工具分发。

### FR-6：协议版本与 SDK 适配

**优先级：** Must

#### 验收标准（EARS）

1. WHEN initialize 成功后收到后续 HTTP 请求 THEN 系统 SHALL 校验 `MCP-Protocol-Version`。
2. IF 协议版本无效或不受支持 THEN 系统 SHALL 返回 `400 Bad Request`。
3. WHEN 评估维护中的 Rust MCP SDK/rmcp THEN 实现 SHALL 用兼容性 spike 证明工具目录、认证中间件、无状态 HTTP 和现有返回结构能否接入。
4. IF SDK 不能满足现有契约 THEN 实现 SHALL 记录可复现证据，并只为缺口保留最小传输适配层，不重写工具内核。

### FR-7：OAuth discovery 与固定 identity

**优先级：** Must

#### 验收标准（EARS）

1. WHEN 未认证客户端访问受保护 `/mcp` THEN 系统 SHALL 返回 `401` 和指向 protected resource metadata 的 `WWW-Authenticate`。
2. WHEN 客户端读取 metadata THEN 系统 SHALL 同时兼容根级 `/.well-known/oauth-protected-resource` 和 `/mcp` 对应的 path-aware discovery 路径。
3. WHEN listener 或 tunnel 重启 THEN issuer、resource、audience 和 OAuth endpoint SHALL 保持 canonical origin 不变。
4. IF 反向代理 Host/X-Forwarded-* 与显式 canonical origin 冲突 THEN 系统 SHALL 以已校验的显式 canonical origin 为准并记录非敏感警告。

### FR-8：真实 MCP 分层健康检查

**优先级：** Must

#### 验收标准（EARS）

1. WHEN 用户运行健康检查 THEN 系统 SHALL 分别报告 config、local transport、public transport、OAuth 和 MCP handshake 层。
2. WHEN 检查本地或可认证的公网 MCP THEN 系统 SHALL 执行 `initialize`、`notifications/initialized` 和 `tools/list`，验证响应 ID、协议版本和非空工具目录。
3. IF GET 可达但 MCP 握手失败 THEN 系统 SHALL 报告 `public-degraded`，不得报告 public-ready。
4. WHEN 所有必需层通过 THEN 系统 SHALL 报告 `public-ready`；仅本地通过时 SHALL 报告 `local-ready`。

### FR-9：可观测性与脱敏

**优先级：** Must

#### 验收标准（EARS）

1. WHEN 一次启动、重启、恢复或健康检查开始 THEN 系统 SHALL 生成 trace ID，并记录状态迁移、失败阶段和耗时。
2. WHEN 日志字段包含认证 header、Token、Secret 或 `.env` 来源值 THEN 系统 SHALL 在写文件前统一脱敏。
3. WHEN 用户主动查看连接配置 THEN UI SHALL 可以显示 canonical MCP URL，但不得显示 Token 或原始 `.env` 内容。
4. IF 错误可重试 THEN 结构化状态 SHALL 标记 retryable、下次重试时间和最终停止原因。

### FR-10：Windows 11 运行与环境边界

**优先级：** Should

#### 验收标准（EARS）

1. WHEN Windows 用户保持桌面应用进程运行 THEN listener、cloudflared 和恢复循环 SHALL 不依赖前端页面是否打开。
2. WHEN 应用优雅退出 THEN 系统 SHALL 有界等待 listener 和受管子进程退出，并确认端口释放。
3. WHEN 文档提供常驻方式 THEN 系统 SHALL 明确“登录后启动/托盘常驻”与“无用户会话 Windows Service”边界，不把前者描述为后者。
4. WHEN workspace 位于 Windows 或 WSL THEN 系统 SHALL 保持命令、路径和子进程在同一环境执行，不隐式跨边界转换。

### FR-11：确定性回归覆盖

**优先级：** Must

#### 验收标准（EARS）

1. WHEN 运行自动化测试 THEN 所有网络场景 SHALL 使用本地假 MCP、OAuth 和 cloudflared 输出源，不访问公网。
2. WHEN 协议测试运行 THEN SHALL 覆盖 GET、notification、版本 header、Accept/Content-Type、401 challenge 和 metadata 路径矩阵。
3. WHEN 生命周期测试运行 THEN SHALL 覆盖真就绪、提前退出、超时、listener 重启保隧道、显式停止和恢复耗尽。
4. WHEN Windows CI/验证运行 THEN SHALL 覆盖进程树停止、端口释放和路径边界。

### FR-12：兼容迁移与回滚

**优先级：** Must

#### 验收标准（EARS）

1. WHEN 旧工作区未启用固定域名覆盖 THEN 系统 SHALL 保持当前 FRP、Cloudflare Quick 和手工 public URL 行为。
2. WHEN 固定域名配置首次生效 THEN 系统 SHALL 不改变工具 API、工作区 ID、端口和权限策略。
3. IF 新 transport 或生命周期实现失败 THEN 发布 SHALL 可以通过单一配置开关回退旧 transport/旧 tunnel orchestration，且不迁移或删除用户数据。
4. WHEN 回滚完成 THEN 旧版本 SHALL 能读取原有工作区配置；新增字段必须具有 serde 默认值或保存在独立设置中。

## 非功能需求

- **NFR-1 可靠性**：自动恢复必须有最大尝试次数、最大总时长和冷却时间，禁止无限重启循环。
- **NFR-2 安全**：所有 secret 只驻留于进程内存、环境或既有 SecretStore，不出现在业务 DTO。
- **NFR-3 兼容性**：保持 MCP 工具名称、schema、结果包装和当前工作区调用语义。
- **NFR-4 可测试性**：传输、状态机、配置解析和日志脱敏均可通过依赖注入或纯函数测试。
- **NFR-5 性能**：不执行工具调用时，健康循环不得持续高频轮询；后台空闲 CPU 应接近当前基线。

## 依赖关系

- `RuntimeSupervisor` 管理本地 MCP listener。
- `TunnelSupervisor`、`cloudflare.rs` 和 `tunnel/access.rs` 管理公网子进程。
- `SecretStore` 管理现有 workspace/shared secrets。
- `mcp/listener.rs`、`mcp/server.rs` 和 `auth/` 提供当前 MCP/OAuth 行为。
- `health/checker.rs` 和前端 `HealthPanel.svelte` 提供现有健康检查展示。

## 检查清单

- [x] FR-1 至 FR-12 均有可测试验收标准
- [x] 固定域名与 Token 安全边界明确
- [x] 协议、OAuth、生命周期和 Windows 范围明确
- [x] 兼容迁移、回滚和不做项明确
