# 设计文档：桌面历史会话与托盘控制

## 概述

本设计覆盖 FR-1 至 FR-5。历史侧沿用现有工具内核，以 Markdown 为事实源、JSON 为有界派生状态；桌面侧在根 layout 挂载关闭确认，由 Rust 管理托盘、单实例信号和真退出状态。

## 技术方案

| 类别 | 选择 | 理由 | 关联需求 |
|---|---|---|---|
| 历史事实源 | 数字 Markdown | 可审阅、可备份、无数据库依赖 | FR-1, FR-2, FR-3 |
| 当前状态 | 有界 `state.json` | 快速 bootstrap，避免随历史线性增长 | FR-1 |
| 历史定位 | 确定性关键词 + 分页读取 | ChatGPT 连接器可显式调用，结果可测 | FR-2 |
| 关闭交互 | Svelte 应用内 modal | 支持一致的三按钮布局和无障碍语义 | FR-4 |
| 后台生命周期 | Tauri tray + hide/show commands | 保留同一进程内的 MCP/Actions/隧道 | FR-4, FR-5 |
| Windows 单实例 | named mutex + named event | 二次启动可唤起已隐藏的主实例 | FR-5 |

## 架构设计

```text
ChatGPT tools/call
  -> mcp/server.rs 注入宿主会话 key
  -> tools/dispatch.rs
  -> tools/history/{mod,storage,markdown,model}.rs
       -> docs/history-session/N.md
       -> docs/history-session/memory/{state,manifest}.json

主窗口 CloseRequested
  -> close-guard.ts / Rust 兜底事件
  -> CloseConfirmDialog.svelte
       -> hide_to_tray
       -> quit_app -> shutdown_managed_services -> app.exit
  -> TrayIcon 显示/退出
```

## 数据模型

| 实体 | 关键字段 | 约束 |
|---|---|---|
| HistoryState | state_revision、archive_revision、current_focus、recent_changes、open_items、references | 每个数组和文本均有上限 |
| HistoryManifest | number、path、title、timestamps、bytes、sha256、keywords | 不复制正文 |
| SearchResult | number、path、score、snippet、matched_terms | 页面数量有上限 |
| ReadPage | content、cursor、next_cursor、total_bytes、sha256 | UTF-8 边界；最大 64 KiB |

## API 设计

| 方法 | 入参 | 出参 | 需求 |
|---|---|---|---|
| history_session_bootstrap | session/title/initial_user_input | 有界 state、统计、捕获状态、检索指引 | FR-1, FR-3 |
| history_session_search | query/limit/cursor/filter | 命中页与 next_cursor | FR-2 |
| history_session_read | number/path/cursor/max_bytes/hash | 原始 Markdown 页与 next_cursor | FR-2 |
| history_session_checkpoint | stable target/turn/raw_user_input/结构化字段 | 幂等或修订结果、捕获状态 | FR-3 |
| hide_to_tray/show_main_window/quit_app | 无 | Tauri command result | FR-4 |

## 文件结构

主要实现修改沿用上游提交中的真实路径：

```text
src-tauri/src/tools/history/*.rs
src-tauri/src/tools/{registry,dispatch}.rs
src-tauri/src/mcp/server.rs
src-tauri/src/commands/{mod,ui_memory,window_chrome}.rs
src-tauri/src/lib.rs
src/lib/{api/window-chrome,close-guard}.ts
src/lib/components/{ChatGptSessionPrompt,CloseConfirmDialog}.svelte
src/routes/+layout.svelte
src/app.css
src-tauri/tests/history_session.rs
```

## 设计决策

### 决策 1：提交级移植后手工合并重叠文件

上游 HEAD 以当前本地 HEAD 为直接祖先，但 `lib.rs`、Cargo 文件和 README 已被固定域名任务修改。无重叠文件使用上游内容；重叠文件按 diff 合并，保留 `shutdown_managed_services` 和稳定隧道配置。

### 决策 2：search/read 是精确历史恢复主路径

bootstrap 只提供当前状态和引用。需要旧事实时先 search 定位，再 read 按页读取，避免无界上下文和递归摘要。

### 决策 3：应用内 modal 与 Rust 兜底并存

前端负责完整交互，Rust `CloseRequested` 防止前端监听尚未挂载时直接销毁。真退出和 UI 重建通过原子标志绕过拦截。

### 决策 4：托盘退出复用有界清理

`quit_app` 和托盘菜单均标记允许退出后调用应用退出；RunEvent 只执行一次有界 runtime/tunnel 清理，避免孤儿进程。

## 测试策略

- Rust 单元/集成：bootstrap 大小、派生状态重建、搜索排序、UTF-8 分页、路径边界、原文缺失告警、修订幂等。
- Rust 编译：验证 tray feature、Windows cfg 和固定域名改动可共存。
- Svelte 检查：验证 modal、事件监听和 API 类型。
- UI 截图：桌面与窄视口检查关闭确认不重叠、文案和三按钮可用。
- 人工边界：真实 Windows 托盘、二次启动唤起和系统关闭仍需安装包环境验收。

## 风险评估

| 风险 | 影响 | 缓解措施 |
|---|---|---|
| `lib.rs` 合并漏掉退出清理 | 高 | 对照两侧 diff，增加 RunEvent 路径审查和编译验证 |
| 历史响应再次无界 | 高 | 测试 64 KiB 上限，全文只由 read 返回 |
| Windows API feature 不完整 | 中 | 对照上游 Cargo feature 并交叉编译检查可行性 |
| 前后端双重 CloseRequested 重复弹框 | 中 | 对话框 open 操作幂等，允许退出标志绕过 |
| UI 重建抢焦点 | 中 | destroy 前记录可见性，隐藏态重建后立即 hide |
