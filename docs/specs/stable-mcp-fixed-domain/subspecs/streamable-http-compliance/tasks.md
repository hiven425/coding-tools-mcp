# 子任务：Streamable HTTP 与 OAuth 合规

- [x] 2.1 **SHC-1** 完成 rmcp/维护中 Rust MCP SDK 兼容性 spike。
  - 证据块：当前 Cargo manifest 只有 Axum/Tokio/Reqwest，没有 MCP SDK；`mcp/server.rs` 直接包装现有工具结果。
  - 验证：无状态 HTTP、Axum、OAuth middleware、工具 schema、structuredContent、错误码和取消语义。
  - 输出：一页 ADR 或 design 决策更新；记录版本、最小示例、通过/失败证据。
  - _需求：FR-6_

- [x] 2.2 **SHC-2** 先增加 Streamable HTTP 表驱动 RED 测试。
  - 证据块：`mcp/listener.rs:130` 当前 GET 返回 JSON；`mcp/server.rs:18` notification 返回 `Value::Null`，handler 最终返回 200 JSON。
  - 覆盖：GET 405、notification 202 空 body、request 200 JSON、415、406、400 protocol version、未初始化/非法 JSON-RPC。
  - 涉及文件：新增 `tests/mcp_http_transport.rs`，必要时提取测试 router factory。
  - _需求：FR-5、FR-6_

- [x] 2.3 **SHC-3** 用 SDK 或薄 adapter 实现 transport 合规。
  - 证据块：2.1 的 spike 决定 SDK 或薄 adapter 路径，2.2 的响应矩阵 RED 测试定义最小改动边界。
  - 约束：保留 `list_tools_for_profile`、`call_tool`、`wrap_mcp_tool_result` 和公开工具响应 golden fixture。
  - 涉及文件：`mcp/listener.rs`、`mcp/server.rs`、`mcp/mod.rs`、Cargo manifest/lock。
  - _需求：FR-5、FR-6_

- [x] 2.4 **SHC-4** 先增加 OAuth discovery/challenge RED 测试。
  - 证据块：当前只注册根级 `/.well-known/oauth-protected-resource`，且 canonical URL 会从 header/配置共同推导。
  - 覆盖：401 `WWW-Authenticate`、根级/path-aware metadata、固定 issuer/resource/audience、转发 header 冲突、重启稳定性。
  - 涉及文件：新增 `tests/oauth_discovery.rs`，扩展 `auth` 模块测试。
  - _需求：FR-7_

- [x] 2.5 **SHC-5** 实现 canonical OAuth discovery 和兼容路由。
  - 证据块：2.4 的 discovery/challenge RED 测试和现有 auth 路由快照共同锁定兼容行为。
  - 约束：显式固定域名优先；未启用固定域名的现有动态 tunnel 行为保持兼容。
  - 涉及文件：`mcp/listener.rs`、`auth/oauth.rs`、`auth/oauth_flow.rs`。
  - _需求：FR-7_

- [x] 2.6 **SHC-6** 运行协议契约与工具 golden 回归。
  - 证据块：2.2、2.4 的协议矩阵与现有工具 golden fixture 是该回归的输入基线。
  - 检查：新 HTTP/OAuth 测试、现有 `call_tool_contract`、`call_tool_security`、历史会话契约。
  - _需求：FR-5、FR-6、FR-7_
