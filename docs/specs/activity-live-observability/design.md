# 设计文档：activity-live-observability

## 概述

本设计在现有 ActivityStore 和 Activity 页面上增加后台命令关联、Tauri 实时事件和主动健康验证视图。活动数据仍是内存态、脱敏且有界；服务真实性由 Runtime 状态和用户触发的完整协议握手独立表达。

**对应需求:** FR-1、FR-2、FR-3、FR-4、FR-5、NFR-1、NFR-2、NFR-3、NFR-4

---

## 技术方案

### 技术选型

| 类别 | 选择 | 理由 | 关联需求 |
|------|------|------|----------|
| 活动状态 | `Mutex<ActivityInner>` + `VecDeque` + 有界 `HashMap` | 保持现有同步调用路径，统一保护 trace 与进程关联 | FR-2, NFR-1 |
| 实时通知 | `tokio::sync::broadcast` + Tauri `Emitter` | 单个后台桥接任务即可向窗口广播，前端可取消监听 | FR-3 |
| 服务验证 | 复用 `get_runtime_status` 与 `run_health_checks` | 不重复实现端口和 MCP 握手逻辑 | FR-1, FR-4 |
| GUI | Svelte 5 + 现有 API/store/token | 保持现有桌面运维体验和类型契约 | FR-4, FR-5 |

### 架构设计

```text
MCP /mcp listener
  -> ActivityStore.begin_trace
  -> MCP tool dispatch
  -> ActivityStore.complete_trace / fail_trace
       -> update bounded traces
       -> correlate bounded active processes
       -> broadcast sanitized ActivityEvent
            -> one Tauri AppHandle bridge
                 -> activity://event
                      -> visible Activity page refresh

Activity page
  -> list_activity snapshot (authoritative recovery)
  -> get_runtime_status per workspace (listener state)
  -> explicit run_health_checks (protocol proof at timestamp)
```

实时事件只承担低延迟通知，快照仍是权威数据源。页面收到事件后采用短防抖刷新快照，避免高并发调用造成逐事件 IPC 风暴；窗口隐藏时不刷新，恢复可见后立即同步。

---

## 数据模型

| 实体/字段 | 类型 | 约束 | 说明 |
|-----------|------|------|------|
| `ActivityTrace.process_session_id` | `String` | 256 字节 | 请求参数或响应中的后台会话标识 |
| `ActivityTrace.operation_id` | `String` | 256 字节 | 工具执行操作标识 |
| `ActivityTrace.parent_trace_id` | `String` | 256 字节 | 后续进程调用关联的初始 trace |
| `ActivityProcess` | struct | 最多 100 条 | 活跃后台命令的脱敏摘要和更新时间 |
| `ActivitySnapshot.active_processes` | array | 最多 100 条 | 当前等待终态的后台命令 |
| `ActivityEvent` | tagged payload | broadcast 容量 256 | trace/process/clear 状态变化 |
| `WorkspaceHealthView` | 前端状态 | 不持久化 | listener、local/public/handshake 结果与验证时间 |

清空操作同时清空 `traces`、`processes` 和 `process_trace_by_session`。事件和快照复用同一已脱敏数据，不额外保存原始载荷。

---

## API 设计

| 方法/函数 | 路径/签名 | 入参 | 出参 | 关联需求 |
|-----------|-----------|------|------|----------|
| `ActivityStore::subscribe` | Rust method | 无 | `broadcast::Receiver<ActivityEvent>` | FR-3 |
| `ActivityStore::snapshot` | Rust method | `ActivityQuery` | 含 trace 和 active process 的快照 | FR-2, FR-5 |
| `activity://event` | Tauri event | 后端广播 | `ActivityEvent` | FR-3 |
| `listActivity` | Tauri invoke | 现有筛选 | 扩展后的 `ActivitySnapshot` | FR-2, FR-5 |
| `getRuntimeStatus` | Tauri invoke | workspace id | listener 状态 | FR-4 |
| `runHealthChecks` | Tauri invoke | workspace id | 分层健康项 | FR-4 |

MCP HTTP、工具 schema 和现有 Tauri command 名称保持兼容；`ActivitySnapshot` 只新增字段。

---

## 文件结构

```text
docs/specs/activity-live-observability/
├── requirements.md
├── design.md
└── tasks.md
src-tauri/src/
├── activity.rs
├── lib.rs
└── mcp/listener.rs
src/lib/
├── api/activity.ts
├── components/ActivityProcessPanel.svelte
├── components/ActivityServiceHealth.svelte
└── types.ts
src/routes/activity/+page.svelte
```

`mcp/listener.rs` 仅在必要时调整 Activity 调用参数；如果 ActivityStore 可从已有 trace 完成关联，则保持该文件不变。

---

## 设计决策

### 决策 1: 使用 Tauri 原生事件而非新增 SSE 服务（关联需求: FR-3）

**问题**: 桌面页面需要实时更新，但新增 HTTP SSE 会扩大认证和端口暴露面。

**选项**:
1. Admin SSE：适合远程 Web，但需要额外 listener、认证和生命周期。
2. Tauri event：复用桌面 IPC，前端可在销毁时取消监听。

**决策**: 选择 Tauri event，并以有界 Rust broadcast 作为内部解耦层。

**理由**: 本轮只面向现有桌面 GUI，不扩大网络攻击面；未来 Headless Web Admin 可复用同一 broadcast。

### 决策 2: 事件触发快照刷新而非前端直接合并完整状态（关联需求: FR-3, FR-5）

**问题**: 直接合并事件容易在丢包、清空和筛选变化时产生不一致。

**决策**: 事件只触发防抖快照刷新，15 秒低频同步作为恢复路径。

**理由**: 逻辑简单、可恢复，并限制高频 IPC。

### 决策 3: 主动健康检查必须由用户触发（关联需求: FR-4）

**问题**: 完整握手会产生真实 MCP 调用和公网流量，周期执行会污染 Activity。

**决策**: 页面加载只读取 runtime 状态；用户点击后才执行完整本地/公网握手并保留时间戳。

**理由**: 避免自触发噪音，同时保持“已验证”结论可追溯。

### 决策 4: 移植进程关联但不移植原文保存（关联需求: FR-2, NFR-2）

**问题**: 参考实现可关联后台进程，但将密码、token、session 和命令原文放入 Activity。

**决策**: 所有新增字段沿用 `redact_text`、`sensitive_key` 和 `bounded_text`；事件只发送已处理对象。

**理由**: 可观测性不能降低既有安全边界。

---

## 测试策略

- Rust 单测验证后台命令建立、后续调用关联、终态移除、父 trace、容量淘汰和清空。
- Rust 单测验证请求、响应、进程命令和事件均不包含敏感原文。
- Rust 单测验证 broadcast 收到 started/completed/process-updated/cleared 事件。
- `cargo check` 验证 Tauri event 桥接和 Rust 类型。
- `npm run check` 验证 Svelte/TypeScript 类型、生命周期和可访问性。
- `npm run build` 验证生产构建；浏览器可用时再做 Activity 页面桌面/窄窗口截图检查。
- 人工语义检查：无请求显示“无进行中请求”；未主动验证显示“尚未验证”；Actions 不出现在 MCP 活动列表。

---

## 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 高频事件导致 IPC 刷新过多 | 中 | 前端防抖、隐藏暂停、15 秒兜底，不逐事件重绘完整表格 |
| 后台会话没有后续读取而长期显示活跃 | 中 | 文案使用“等待终态”，容量限制 100，并展示最后更新时间而非声称进程存活 |
| 新字段泄露命令或会话敏感内容 | 高 | 存储和广播前统一脱敏、截断，并增加负向测试 |
| 健康检查自身产生 Activity | 低 | 仅用户触发，并在界面说明验证会产生协议调用 |
| GitNexus 不可用导致影响分析降级 | 中 | 逐符号 `code_insight` 降级报告、`rg` 调用者清单和聚焦回归测试 |

---

## 检查清单

- [x] 技术方案与现有架构一致
- [x] 全部 FR 均被设计覆盖
- [x] 文件结构对照真实代码库
- [x] 数据模型和接口契约清晰
- [x] 关键设计决策已记录并关联需求
- [x] 测试策略可验证验收标准
