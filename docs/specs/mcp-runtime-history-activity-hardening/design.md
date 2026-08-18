# 设计文档：mcp-runtime-history-activity-hardening

## 概述

改动沿用现有 `RuntimeSupervisor -> listener -> tools` 调用链，以局部增强替代架构重写。

## 对应需求

- FR-1：Runtime 生命周期和 reactor 边界。
- FR-2、FR-3：History v3 状态与完整性。
- FR-4、FR-5：安全活动追踪与 GUI。
- FR-6：跨模块兼容性。

## 技术方案

- Runtime：`start` 在复用状态前检查 `JoinHandle::is_finished`，移出并异步回收旧 handle；MCP/Actions 均先同步 bind 标准 listener，再在 Tauri async runtime 中转换并 serve。
- History：MemoryState 升级到 v3；`open_items` 来自当前 session 最新有效 checkpoint；新增 `memory/snapshot.json` 作为 index/manifest/state 写入完成标记。
- Activity：在 AppState 中维护有界环形队列；MCP listener 在 dispatch 前后记录 trace；递归脱敏先于截断和存储。
- GUI：通过只读 Tauri command 获取快照，以低频轮询刷新；页面只展示摘要、状态、耗时与已脱敏预览。

## 文件结构

- `src-tauri/src/runtime/`：生命周期状态机与端口释放。
- `src-tauri/src/mcp/listener.rs`、`actions/listener.rs`：listener runtime 边界。
- `src-tauri/src/tools/history/`、`src-tauri/tests/history_session.rs`：History v3。
- `src-tauri/src/activity.rs`、`commands/activity.rs`：安全活动存储与 IPC。
- `src/lib/api/activity.ts`、`src/routes/activity/+page.svelte`：桌面活动监控。

## 设计决策

1. 活动数据不落盘，避免生成新的敏感数据资产。
2. 脱敏在进入队列前完成，截断不能作为脱敏替代。
3. History snapshot 只是完成标记，不宣称提供跨文件事务。
4. 已结束 handle 的回收不持有 RuntimeSupervisor 锁等待。
