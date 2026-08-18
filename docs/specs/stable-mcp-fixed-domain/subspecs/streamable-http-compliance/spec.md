# 子规格：Streamable HTTP 与 OAuth 合规

## 范围

负责 MCP `2025-06-18` HTTP 状态码、header 协商、协议版本、SDK/adapter 边界和 OAuth discovery。工具实现、权限策略和历史会话语义保持不变。

## 需求回链

- FR-5
- FR-6
- FR-7

## 验收标准（EARS）

1. WHEN notification/response 被接受 THEN 服务端 SHALL 返回 202 空 body。
2. WHEN GET `/mcp` 且无 SSE 能力 THEN 服务端 SHALL 返回 405。
3. IF Content-Type、Accept 或协议版本不受支持 THEN 服务端 SHALL 返回对应 4xx 且不分发工具。
4. WHEN SDK spike 通过兼容门禁 THEN transport SHALL 使用 SDK；否则 SHALL 使用记录过缺口的薄 adapter。
5. WHEN OAuth 启用 THEN 401 challenge、根级/path-aware metadata、issuer/resource/audience SHALL 使用稳定 canonical origin。

## 涉及文件

- `src-tauri/src/mcp/listener.rs`
- `src-tauri/src/mcp/server.rs`
- `src-tauri/src/mcp/mod.rs`
- `src-tauri/src/auth/bearer.rs`
- `src-tauri/src/auth/oauth.rs`
- `src-tauri/src/auth/oauth_flow.rs`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/tests/mcp_http_transport.rs`（新增）
- `src-tauri/tests/oauth_discovery.rs`（新增）

## 不做项

- 不实现 server-to-client SSE 和 resumability。
- 不生成 `Mcp-Session-Id`。
- 不重写 tools、exec、git、patch 或 history 内核。
- 不改变 OAuth 授权页面的产品交互，除非 SDK 接入必须适配请求结构。

## 设计要点

- transport handler 先分类 JSON-RPC request/notification/response，再决定 HTTP response。
- `handle_request` 只处理有 ID 的 JSON-RPC request；notification lifecycle 由 adapter 处理。
- 版本校验在认证成功后、业务分发前完成，错误不包含 header 中的敏感原文。
- OAuth path-aware 路径用同一 payload builder，避免两套 issuer/resource 计算。
- 对照官方 transport 规范建立表驱动测试，禁止继续用 GET discovery JSON 作为产品契约。
