# 任务清单：activity-live-observability

## 概述

在不改变 MCP 连接契约的前提下，实现可信服务状态、实时 MCP 活动和有界后台命令关联。每项任务都回链到需求与设计，优先保持安全边界和 Windows 桌面兼容性。

> **二元禁令（零容忍）**：本文件及后续实现的交付物中不得使用占位符代替真实实现。

---

## 交付物清单（Scope-lock）

- **实际新建文件数**: 7 个
- **实际修改文件数**: 5 个；`mcp/listener.rs` 和 `commands/activity.rs` 无需修改
- **实际新增/修改函数数**: 约 18 个
- **交付物逐项列举**:
  1. `docs/specs/activity-live-observability/requirements.md`
  2. `docs/specs/activity-live-observability/design.md`
  3. `docs/specs/activity-live-observability/tasks.md`
  4. `src/lib/components/ActivityProcessPanel.svelte`
  5. `src/lib/components/ActivityServiceHealth.svelte`
  6. `src-tauri/src/activity.rs`
  7. `src-tauri/src/lib.rs`
  8. `src-tauri/src/mcp/listener.rs`（仅在 Activity 调用契约需要时修改）
  9. `src/lib/api/activity.ts`
  10. `src/lib/types.ts`
  11. `src/routes/activity/+page.svelte`
  12. `src-tauri/src/commands/activity.rs`（仅在事件桥接需要 IPC command 时修改）
  13. `src-tauri/src/activity_sanitize.rs`
  14. `src-tauri/src/activity_tests.rs`

---

## 任务列表

### 阶段 1: 影响分析与契约确认

- [x] 1.1 分析 ActivityStore、MCP listener、Tauri setup 和 Activity 页面调用链，锁定降级影响范围
  - **证据块**: `src-tauri/src/mcp/listener.rs:323` 在请求派发前调用 `begin_trace`，完成或 worker 失败后更新同一 trace；`src-tauri/src/lib.rs` 负责 AppState 与 setup；`src/routes/activity/+page.svelte:134` 当前每两秒轮询快照。
  - **涉及文件**: 只读分析上述文件及 `commands/activity.rs`、`api/activity.ts`、`types.ts`；无写入预算
  - _需求: FR-1, FR-2, FR-3, FR-4, FR-5_ ｜ _设计: 技术方案、API 设计_

---

### 阶段 2: 核心实现

- [x] 2.1 扩展 ActivityStore，实现最多 100 条后台会话关联和全链路脱敏事件
  - **证据块**: `src-tauri/src/activity.rs:71` 当前只持有 `VecDeque<ActivityTrace>`；`:106` 将请求标记为 running；`:253` 已实现先脱敏后 16 KiB 截断。
  - **涉及文件**: `src-tauri/src/activity.rs`，预计净增 170 行；如超过 520 行，将测试拆至 `src-tauri/tests/activity_observability.rs`
  - _需求: FR-2, FR-3, FR-5_ ｜ _设计: 数据模型、决策 4_

- [x] 2.2 建立单实例 Tauri 活动事件桥接，窗口可取消监听且不新增网络入口
  - **证据块**: `src-tauri/src/lib.rs` 已在 setup 阶段管理 AppState 和托盘生命周期；ActivityStore 为 `Arc`，适合创建单个 broadcast receiver。
  - **涉及文件**: `src-tauri/src/lib.rs` 预计新增 20 行；`src-tauri/src/commands/activity.rs` 和 `src-tauri/src/mcp/listener.rs` 仅在契约需要时各不超过 20 行
  - _需求: FR-3, NFR-3_ ｜ _设计: 决策 1、决策 2_

- [x] 2.3 新增后台进程与服务真实性组件，明确 listener、主动握手和活动的独立语义
  - **证据块**: `src/routes/activity/+page.svelte:151` 当前零运行请求显示“已就绪”；`src-tauri/src/health/checker.rs:72` 已完成 initialize、initialized 和 tools/list 协议验证；`src-tauri/src/commands/runtime.rs:126` 已刷新 listener task 和端口状态。
  - **涉及文件**: 新增 `ActivityProcessPanel.svelte` 不超过 220 行、`ActivityServiceHealth.svelte` 不超过 320 行；修改 `+page.svelte` 不超过 500 行
  - _需求: FR-1, FR-4, FR-5, NFR-4_ ｜ _设计: 架构设计、决策 3_

- [x] 2.4 扩展前端 Activity 类型与事件订阅 API，保留现有调用兼容性
  - **证据块**: `src/lib/types.ts:83` 定义现有 trace/snapshot；`src/lib/api/activity.ts:4` 封装 list/get/clear IPC。
  - **涉及文件**: `src/lib/types.ts` 预计新增 45 行；`src/lib/api/activity.ts` 预计新增 20 行
  - _需求: FR-2, FR-3, FR-5_ ｜ _设计: API 设计_

---

### 阶段 3: 集成测试

- [x] 3.1 对照验收标准验证进程关联、事件、脱敏、容量和清空行为
  - **证据块**: `src-tauri/src/activity.rs:339` 已有脱敏、容量和筛选单测，可扩展同一测试模块或按文件大小拆分集成测试。
  - **涉及文件**: `src-tauri/src/activity.rs` 测试或新增 `src-tauri/tests/activity_observability.rs`
  - _需求: FR-2, FR-3, FR-5, NFR-1, NFR-2_ ｜ _设计: 测试策略_

- [x] 3.2 验证 Rust 编译、前端类型、生产构建和 Activity 窄窗口布局
  - **证据块**: 项目验证命令为聚焦 Rust test、`cargo check`、`npm run check` 和 `npm run build`；现有页面使用响应式 grid 和设计 token。
  - **涉及文件**: 不新增实现文件；必要时只调整本任务拥有的 Svelte/CSS 文件
  - _需求: FR-1, FR-4, FR-5, NFR-3, NFR-4_ ｜ _设计: 测试策略_

---

## 检查点

- [x] 阶段 1 完成后：每个待修改符号均有 impact 降级报告和源码调用者证据，未发现未声明公共契约变更
- [x] 阶段 2 完成后：后台进程、事件桥接、服务验证和 UI 语义完整连通，所有数据先脱敏后发送
- [x] 阶段 3 完成后：6 个聚焦单测、Rust check、Svelte check/build、局部 rustfmt、diff 检查和双视口截图检查通过

---

## 需求覆盖矩阵

| 需求 ID | 设计章节 | 任务编号 | 状态 |
|---------|----------|----------|------|
| FR-1 | 架构设计、决策 3 | 1.1, 2.3, 3.2 | 已完成 |
| FR-2 | 数据模型、决策 4 | 1.1, 2.1, 2.4, 3.1 | 已完成 |
| FR-3 | 决策 1、决策 2 | 1.1, 2.1, 2.2, 2.4, 3.1 | 已完成 |
| FR-4 | API 设计、决策 3 | 1.1, 2.3, 3.2 | 已完成 |
| FR-5 | API 设计 | 2.1, 2.3, 2.4, 3.1, 3.2 | 已完成 |

---

## 文件变更清单

| 文件 | 操作 | 行数预算 | 说明 |
|------|------|----------|------|
| `docs/specs/activity-live-observability/*.md` | 新建 | 约 430 行 | 需求、设计和任务规格 |
| `src-tauri/src/activity.rs` | 修改 | 净增约 170 行 | 进程关联、有界事件和测试 |
| `src-tauri/src/activity_sanitize.rs` | 新建 | 约 100 行 | 脱敏与有界值辅助函数 |
| `src-tauri/src/activity_tests.rs` | 新建 | 约 230 行 | Activity 聚焦回归测试 |
| `src-tauri/src/lib.rs` | 修改 | 约 20 行 | Tauri 事件桥接 |
| `src-tauri/src/mcp/listener.rs` | 条件修改 | 不超过 20 行 | Activity 调用契约适配 |
| `src-tauri/src/commands/activity.rs` | 条件修改 | 不超过 20 行 | 仅在需要 command 订阅时修改 |
| `src/lib/types.ts` | 修改 | 约 45 行 | process/event/snapshot 类型 |
| `src/lib/api/activity.ts` | 修改 | 约 20 行 | 事件监听 API |
| `src/lib/components/ActivityProcessPanel.svelte` | 新建 | 不超过 220 行 | 后台命令列表 |
| `src/lib/components/ActivityServiceHealth.svelte` | 新建 | 不超过 320 行 | listener 与主动验证 |
| `src/routes/activity/+page.svelte` | 修改 | 最终不超过 500 行 | 页面编排、事件刷新和语义修正 |

---

## 检查清单

- [x] 交付物清单已填写并锁定边界
- [x] 每条任务标题具体且可验收
- [x] 每条任务包含先读后写证据
- [x] 每条任务标注文件和行数预算，页面超限已有组件拆分方案
- [x] 任务分阶段且可在单次提交内完成
- [x] 每条任务回链到 FR 和设计章节
- [x] 需求覆盖矩阵无遗漏
- [x] 阶段 3 包含逐条验收和真实验证
- [x] 全文无占位内容
