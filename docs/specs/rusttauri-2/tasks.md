# 任务清单：桌面历史会话与托盘控制

## 概述

按同源上游提交逐项移植历史会话和托盘能力，并保留本地固定域名稳定链路。

## 交付物清单

- 预计新建文件数：10 个左右（规格、托盘命令/API/组件和上游 close-to-tray 规格）。
- 预计修改文件数：20 个左右（历史工具、注册分发、MCP 提示、入口、样式、测试和说明）。
- 预计新增/修改函数数：约 35 个。
- 交付物：有界历史工具、原文参数契约、关闭确认 GUI、托盘/单实例生命周期、测试与文档。

## 任务列表

### 阶段 1：差异和规格

- [x] 1.1 核对上游提交与本地基线，锁定可移植差异和重叠文件
  - 证据块：`mybolide` 提交 `4384593` 的 merge-base 为本地 HEAD `72d7a5f`；本地 `tools/history/mod.rs:108-178` 当前仍组装全量摘要与最新全文。
  - 涉及文件：只读检查和本规格目录。
  - 需求：FR-1 至 FR-5；设计：决策 1。

- [x] 1.2 校验完整规格并评估工作量
  - 证据块：项目 `docs/project-context.md` 指定新功能先通过 check_spec。
  - 涉及文件：`docs/specs/rusttauri-2/*.md`。
  - 需求：FR-1 至 FR-5；设计：全部章节。

### 阶段 2：核心实现

- [x] 2.1 移植有界历史状态与 search/read，保持 Markdown 事实源和 64 KiB 页上限
  - 证据块：`src-tauri/src/tools/history/mod.rs:108-178` 目前返回 history_numbers、session_summaries、all_history_summary 和 latest_handoff；`registry.rs:499-530` 尚无原文字段及 search/read Schema。
  - 涉及文件：`src-tauri/src/tools/history/*.rs`、`registry.rs`、`dispatch.rs`、`mcp/server.rs`、历史测试，单个历史模块超过 500 行但保持现有领域拆分。
  - 需求：FR-1、FR-2、FR-3；设计：历史架构、API、决策 2。

- [x] 2.2 合并托盘、关闭确认和 Windows 唤起，复用现有退出清理
  - 证据块：`src-tauri/src/lib.rs:37-59` 现有 mutex 只拒绝二次启动；`lib.rs:66-89` 已有 runtime/tunnel 有界清理；`lib.rs:158-161` 仅在 UI 重建时阻止退出。
  - 涉及文件：`src-tauri/src/lib.rs`、`commands/{mod,ui_memory,window_chrome}.rs`、Cargo/tauri 配置、根 layout、close guard、modal、API 与样式。
  - 需求：FR-4、FR-5；设计：桌面架构、决策 3、决策 4。

- [x] 2.3 同步桌面会话提示与 README 使用步骤
  - 证据块：`ChatGptSessionPrompt.svelte` 当前仍要求 bootstrap 读取所有累计摘要；README 尚未列出 search/read。
  - 涉及文件：会话提示组件、`README.md`、`README.en.md`。
  - 需求：FR-2、FR-3；设计：API 设计。

### 阶段 3：验证

- [x] 3.1 运行历史会话和 MCP 契约回归
  - 证据块：上游新增 `src-tauri/tests/history_session.rs` 覆盖有界状态、分页和输入捕获。
  - 涉及文件：Rust 测试目标。
  - 需求：FR-1、FR-2、FR-3；设计：测试策略。

- [x] 3.2 运行 Svelte 检查并对关闭确认做桌面/窄视口截图评审
  - 证据块：根 layout 是全局窗口交互唯一挂载点，modal 使用项目现有 Token。
  - 涉及文件：前端源码与本地截图产物。
  - 需求：FR-4、FR-5；设计：测试策略。

- [x] 3.3 检查最终差异、敏感信息和预期影响面
  - 证据块：GitNexus sidecar 不可用，必须以 `code_insight` 降级结果、`git diff` 和调用链人工审查补足。
  - 涉及文件：全部本轮 owned paths。
  - 需求：FR-1 至 FR-5；设计：风险评估。

## 检查点

- [x] 阶段 1：check_spec 通过并完成 estimate。
- [x] 阶段 2：所有工具注册/分发一致，托盘退出保留有界清理。
- [x] 阶段 3：定向测试、编译、Svelte 检查和视觉验收均有证据。

## 需求覆盖矩阵

| 需求 ID | 设计章节 | 任务编号 | 状态 |
|---|---|---|---|
| FR-1 | 历史架构、API | 1.1、2.1、3.1 | 完成 |
| FR-2 | 历史定位、API | 2.1、2.3、3.1 | 完成 |
| FR-3 | 原文参数、API | 2.1、2.3、3.1 | 完成 |
| FR-4 | 桌面架构、决策 3/4 | 2.2、3.2 | 完成 |
| FR-5 | 生命周期边界 | 2.2、3.2 | 完成 |

## 文件变更清单

| 文件组 | 操作 | 说明 |
|---|---|---|
| `src-tauri/src/tools/history/*.rs` | 修改 | 有界 state、manifest、search/read、修订归档 |
| `src-tauri/src/tools/{registry,dispatch}.rs` | 修改 | 工具注册、Schema、分发 |
| `src-tauri/src/mcp/server.rs` | 修改 | 初始化提示和元数据注入测试 |
| `src-tauri/src/lib.rs` 与 `commands/*` | 新建/修改 | 托盘、单实例唤起、UI 隐藏态和退出 |
| `src/lib/*` 与 `src/routes/+layout.svelte` | 新建/修改 | 关闭确认 GUI 和会话提示 |
| `README*.md` 与功能规格 | 新建/修改 | 使用说明和行为契约 |
| Rust 测试 | 修改 | 回归覆盖 |

## 检查清单

- [x] 交付物和边界已锁定
- [x] 每个任务均有现状证据、真实文件和需求回链
- [x] 覆盖矩阵无遗漏
- [x] 验证任务覆盖协议、UI 和生命周期
