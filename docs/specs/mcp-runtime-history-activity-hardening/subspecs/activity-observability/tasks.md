# 子任务：脱敏活动追踪与桌面监控

- [ ] 1.1 实现有界 ActivityStore、递归脱敏和 MCP trace 接入
  - 证据块: `src-tauri/src/mcp/listener.rs` 请求分类与 dispatch 路径
  - 涉及文件: `src-tauri/src/activity.rs`, `app_state.rs`, `mcp/listener.rs`, `commands/activity.rs`, `lib.rs`
  - _需求: FR-4, FR-6_
- [ ] 1.2 实现活动 API、导航入口和筛选页面
  - 证据块: `src/routes/+layout.svelte` 导航模式与 `src/lib/api` IPC 封装
  - 涉及文件: `src/lib/api/activity.ts`, `src/routes/activity/+page.svelte`, `src/routes/+layout.svelte`
  - _需求: FR-5, FR-6_
