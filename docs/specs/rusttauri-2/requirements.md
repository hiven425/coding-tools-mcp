# 需求文档：桌面历史会话与托盘控制

## 功能概述

为使用固定域名 MCP 的桌面用户补齐有界历史恢复、原文传递提示和窗口后台运行能力。实现以 `mybolide/coding-tools-mcp` 的 0.2.0 提交为参考，并与本地固定域名、隧道恢复和有界退出清理合并。

## 历史经验与坑

- 可复用经验：上游提交 `4384593` 与本地基线 `72d7a5f` 同源，可按文件级提交差异移植。
- 必须规避的坑：不能让 bootstrap 回传全量历史；服务端不能假装读取未作为工具参数传入的聊天原文；托盘退出不能绕过现有 runtime 和 tunnel 有界清理。

## 术语定义

- 有界当前状态：大小和条目数受限、可由 Markdown 档案重建的 `memory/state.json` 投影。
- 原始档案：`docs/history-session/N.md` 中可审阅、可分页读取的事实源。
- 后台运行：隐藏主窗口但保留桌面进程、MCP、Actions 和隧道服务。

## 范围边界

### In Scope

- 将 `history_session_bootstrap` 改为返回有界当前状态与检索指引。
- 新增 `history_session_search` 和 `history_session_read`，支持确定性关键词定位与 UTF-8 安全分页。
- 在 bootstrap/checkpoint Schema、MCP 初始化说明和桌面会话提示中要求 `initial_user_input` / `raw_user_input`。
- 提供关闭三选确认、托盘隐藏/恢复/退出、UI 重建隐藏态保持和 Windows 二次启动唤起。
- 保留固定域名、OAuth、健康检查、隧道恢复和退出清理的现有改动。

### Out of Scope

- 新增数据库、向量检索或外部记忆服务。
- 将历史全文直接展示成新的桌面档案编辑器。
- 持久化“关闭时总是后台运行”偏好。
- 发布安装包、推送远端或真实 Cloudflare 域名验收。

## 需求列表

### FR-1：有界会话启动

**优先级：Must**

1. WHEN 新会话调用 bootstrap THEN 系统 SHALL 返回有界状态、版本、统计和 search/read 指引，且不返回全量历史摘要或档案全文。
2. WHEN 调用方提供 `initial_user_input` THEN 系统 SHALL 脱敏后逐字归档并报告捕获状态。
3. IF 调用方未提供首次原文 THEN 系统 SHALL 返回明确 warning，不得宣称已读取远程聊天内容。

### FR-2：历史定位与分页精读

**优先级：Must**

1. WHEN 调用 search 并提供 query THEN 系统 SHALL 返回有界、稳定排序的档案命中与短片段。
2. WHEN 调用 read 并提供编号或安全路径 THEN 系统 SHALL 在 UTF-8 边界返回最多 64 KiB 的原始 Markdown 页和 `next_cursor`。
3. IF 路径越界、cursor 非法或内容哈希变化 THEN 系统 SHALL 返回可观察错误。

### FR-3：逐轮原文和提示同步

**优先级：Must**

1. WHEN checkpoint 收到 `raw_user_input` THEN 系统 SHALL 逐字归档并保留同一 turn 的修订证据。
2. IF `raw_user_input` 缺失 THEN 系统 SHALL 报告未捕获且说明服务端无法读取未传入内容。
3. WHEN 客户端初始化或用户复制桌面会话提示 THEN 文案 SHALL 明确 bootstrap/search/read/checkpoint 的调用顺序和原文参数。

### FR-4：关闭确认与后台运行

**优先级：Must**

1. WHEN 用户关闭主窗口 THEN 系统 SHALL 展示“取消 / 后台运行 / 直接关闭”三选确认。
2. WHEN 用户选择后台运行 THEN 系统 SHALL 隐藏窗口并保持 MCP、Actions 和隧道运行。
3. WHEN 用户点击托盘或选择显示窗口 THEN 系统 SHALL 恢复、取消最小化并聚焦主窗口。
4. WHEN 用户直接关闭或从托盘退出 THEN 系统 SHALL 执行现有有界服务清理后退出。

### FR-5：生命周期边界

**优先级：Must**

1. IF UI 静默重建前窗口处于隐藏态 THEN 重建后系统 SHALL 保持隐藏且不抢焦点。
2. WHEN Windows 已有实例在托盘且用户二次启动 THEN 新进程 SHALL 尽量通知现有实例显示窗口，然后自行结束。
3. IF 唤起信号失败 THEN 系统 SHALL 记录可诊断信息且不得启动第二套服务。

## 非功能需求

- NFR-1：bootstrap 正常响应序列化后不超过 64 KiB；read 单页最大 64 KiB。
- NFR-2：Markdown 是永久事实源，派生 state/manifest 损坏时可重建，修复过程不改写旧档案。
- NFR-3：Windows、macOS 编译兼容；非 Windows 单实例行为保持现状。
- NFR-4：新增 UI 复用现有设计 Token、Lucide 图标和根布局，不引入额外前端依赖。

## 依赖关系

- Tauri 2 `tray-icon` feature、现有 dialog 插件和 Windows API crate。
- 现有 `tools/history`、`mcp/server`、`commands/ui_memory`、根 Svelte layout。
- 已完成的固定域名 MCP 稳定链路及应用退出有界清理。

## 检查清单

- [x] 核心场景和异常边界已覆盖
- [x] FR 编号可由设计和任务逐项回链
- [x] 范围明确且保持公共行为兼容
- [x] 性能、安全和跨平台约束已列出
