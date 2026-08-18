# 子规格：健康检查与可观测性

## 范围

负责以真实 MCP 握手替换 GET 假健康，输出分层连接状态和可操作失败证据，并保证所有日志在写入前脱敏。

## 需求回链

- FR-8
- FR-9

## 验收标准（EARS）

1. WHEN 运行健康检查 THEN 系统 SHALL 按 config/local/public/oauth/handshake 顺序返回稳定 key 和 layer。
2. WHEN 本地或公网端点可认证 THEN 系统 SHALL 完成 initialize、initialized 202 和 tools/list。
3. IF GET/TLS 可达但握手失败 THEN 聚合状态 SHALL 为 public-degraded。
4. WHEN 连接操作或健康检查执行 THEN 日志 SHALL 包含 trace ID、阶段、耗时和状态迁移。
5. IF 日志输入含 secret THEN 写入后的日志 SHALL 只包含统一占位或不可逆摘要。

## 涉及文件

- `src-tauri/src/health/checker.rs`
- `src-tauri/src/health/mod.rs`
- `src-tauri/src/commands/health.rs`
- `src-tauri/src/tunnel/logs.rs`
- `src-tauri/src/tunnel/supervisor.rs`
- `src-tauri/src/mcp/listener.rs`
- `src/lib/api/health.ts`
- `src/lib/components/HealthPanel.svelte`
- `src-tauri/tests/mcp_health.rs`（新增）

## 不做项

- 不把真实公网和 Cloudflare 纳入 CI。
- 不在健康检查中执行有副作用的 MCP 工具。
- 不把 Token 临时显示在 UI、剪贴板或错误详情。

## 设计要点

- 健康检查使用独立只读 probe client 和固定超时预算。
- initialized notification 的 202 是握手必测项；tools/list 只验证工具目录，不调用工具。
- OAuth 无法在无人值守探针中安全完成时返回 skip/warn，并通过 challenge/metadata 验证认证层。
- `HealthItem` 保留旧字段以兼容前端，新增枚举状态避免 bool 抹平 degraded/skip。
- 所有日志统一经过 sanitization API，禁止调用方自行拼接 Token 命令行。
