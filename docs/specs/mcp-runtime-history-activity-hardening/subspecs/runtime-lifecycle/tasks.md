# 子任务：Runtime 优雅启停与异常回收

- [ ] 1.1 在启动前识别并回收已结束 handle，补状态机单元测试
  - 证据块: `src-tauri/src/runtime/supervisor.rs` 的 `start`、`refresh`、`begin_stop`
  - 涉及文件: `src-tauri/src/runtime/supervisor.rs`
  - _需求: FR-1, FR-6_
- [ ] 1.2 将 MCP/Actions listener 转换移入 async runtime，保持同步端口冲突报告
  - 证据块: `src-tauri/src/mcp/listener.rs` 与 `src-tauri/src/actions/listener.rs` 的 `spawn_listener`
  - 涉及文件: `src-tauri/src/mcp/listener.rs`, `src-tauri/src/actions/listener.rs`
  - _需求: FR-1, FR-6_
