# 子任务：健康检查与可观测性

- [x] 3.1 **HOB-1** 先增加分层握手健康检查 RED 测试。
  - 证据块：`health/checker.rs:45-82` 当前只判断 GET 状态码；GET 200 不能证明 MCP POST 可用。
  - 覆盖：local-ready、public-ready、GET 可达/initialize 失败、notification 非 202、tools/list 失败、OAuth skip/fail、总超时。
  - 涉及文件：新增 `tests/mcp_health.rs`，扩展 `health/checker.rs` 测试。
  - _需求：FR-8_

- [x] 3.2 **HOB-2** 实现 probe client、分层 DTO 和聚合状态。
  - 证据块：3.1 的失败矩阵证明单一 GET 状态码不足，并定义各层成功、失败和 skip 的聚合输入。
  - 约束：单次检查共享 trace ID；各层独立超时；旧 `ok/detail/hint` 字段继续可反序列化。
  - 涉及文件：`health/checker.rs`、`health/mod.rs`、`commands/health.rs`、`api/health.ts`、`HealthPanel.svelte`。
  - _需求：FR-8_

- [x] 3.3 **HOB-3** 先增加日志脱敏与 trace RED 测试。
  - 证据块：cloudflared 当前通过 `--token <value>` 启动；任何命令行 Debug 输出都可能泄密。
  - 覆盖：Authorization、Bearer、cloudflare token、OAuth code/client secret、`.env` 行、URL userinfo、普通错误不被过度清洗。
  - 涉及文件：`tunnel/logs.rs`、相关调用点测试。
  - _需求：FR-9_

- [x] 3.4 **HOB-4** 实现集中脱敏、trace ID 和状态迁移日志。
  - 证据块：3.3 的敏感字段 RED 测试与 cloudflared `--token` 调用路径定义必须覆盖的格式化边界。
  - 约束：在字符串格式化/写文件之前脱敏；连接 UI 可显示 canonical URL，但诊断 bundle 默认替换 host。
  - 涉及文件：`tunnel/logs.rs`、`tunnel/supervisor.rs`、`mcp/listener.rs`、`health/checker.rs`。
  - _需求：FR-9_

- [x] 3.5 **HOB-5** 运行前后端专项验证。
  - 证据块：3.1 至 3.4 产出的分层状态、trace 和脱敏断言构成专项验证基线。
  - 检查：Rust health/log tests、`npm run check`；仅在 HealthPanel 行为变化需要时增加组件级测试。
  - _需求：FR-8、FR-9_
