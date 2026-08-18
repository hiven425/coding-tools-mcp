# ADR: Streamable HTTP transport boundary

## Decision

Keep the existing MCP tool dispatcher and add a thin Streamable HTTP adapter at the Axum
listener boundary. Do not adopt `rmcp` in this change.

## Evidence

- Evaluated `rmcp 3.1.3` from crates.io on 2026-08-18. It supports server-side
  Streamable HTTP and requires Rust 1.88.
- The current server builds its tool catalog dynamically through `list_tools_for_profile`,
  dispatches through `call_tool`, preserves `_meta.openai/session`, and wraps results through
  `wrap_mcp_tool_result`.
- Moving those contracts into `rmcp::ServiceHandler` would combine a transport correction with
  a tool-runtime rewrite and would make golden response compatibility harder to prove.

## Adapter contract

- Axum owns authentication, Content-Type, Accept, protocol-version validation, and HTTP status.
- `handle_request` continues to process JSON-RPC requests and notifications.
- Notifications and client responses return `202` with an empty body.
- Requests return `200 application/json`.
- GET `/mcp` returns `405`; this server does not advertise SSE or resumability.
- The adapter remains stateless and does not issue `Mcp-Session-Id`.

## Revisit

Reconsider the SDK after a separate compatibility branch proves the existing tool schema,
structured results, OAuth middleware, cancellation, and history-session golden fixtures without
translation loss.
