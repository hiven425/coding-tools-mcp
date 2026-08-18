# 需求文档：activity-live-observability

## 功能概述

为使用 Coding Tools MCP Desktop 管理本地与公网 MCP 服务的开发者提供可信的实时可观测界面。界面必须区分服务存活、主动协议验证、单次 MCP 调用和后台命令会话，避免以“没有请求”推断服务已就绪，并在不泄露用户原文、密钥或命令参数的前提下实时呈现调用进展。

## 历史经验与坑

- **可复用经验**: 复用现有 `run_health_checks` 完整 Streamable HTTP 握手、`RuntimeSupervisor` 状态刷新，以及 ActivityStore 的先脱敏后截断和 500 条轨迹上限。
- **必须规避的坑**: 不复制参考仓库原样保存请求、会话和命令的实现；不把 `running` trace、空活动列表或端口占用等同于协议可用；不让实时事件订阅形成无界队列或后台轮询。

## 术语定义

- **MCP 调用轨迹**: 进入本应用 `/mcp` listener 的一次 JSON-RPC 请求及其结果，不代表服务持续存活。
- **主动协议验证**: 对目标端点执行 `initialize`、`notifications/initialized` 和 `tools/list` 完整序列，并记录本次结果和完成时间。
- **后台命令会话**: `exec_command` 返回 `status=running` 后，由 `session_id` 标识并通过后续输出、输入或终止调用推进状态的进程。
- **活动事件**: ActivityStore 产生的 started、completed、failed、process-updated 或 cleared 有界通知。

---

## 范围边界

**In Scope（本次要做）**

- 修正 Activity 页面状态文案和指标含义，明确仅展示 MCP 活动。
- 安全关联后台命令会话、父子调用和终态，并限制活跃进程容量。
- 通过 Tauri 原生事件向可见页面推送 Activity 变化，保留低频兜底同步。
- 展示各工作区 listener 状态，并允许用户触发现有完整 MCP 健康检查，分别显示本地和公网结果及验证时间。
- 增加 Rust 回归测试，并通过前端类型检查和生产构建验证。

**Out of Scope（本次不做）**

- 单域名多工作区 Gateway、会话路由或连接地址变更。
- Actions 请求追踪、跨进程持久化 Activity、外部监控平台或系统通知。
- 自动周期性公网健康探测，避免产生持续流量和自触发 Activity 噪音。

---

## 需求列表

### FR-1: 可信区分活动与服务状态

**优先级:** Must
**用户故事:** 作为 MCP 运维者，我想区分请求活动和服务可用性，以便不会被“已就绪”或“运行中”等模糊文案误导。

#### 验收标准（EARS）

1. WHEN Activity 页面没有进行中请求 THEN 系统 SHALL 显示“无进行中请求”而不是“已就绪”。
2. WHILE 页面展示 trace 统计 THE 系统 SHALL 将 `running` 明确标记为“进行中请求”。
3. IF 用户尚未执行主动协议验证 THEN 系统 SHALL 显示“尚未验证”，不得根据 Activity 数据生成可用结论。

### FR-2: 安全关联后台命令会话

**优先级:** Must
**用户故事:** 作为开发者，我想查看仍待终态的后台命令及相关调用，以便快速定位长任务和失败点。

#### 验收标准（EARS）

1. WHEN `exec_command` 返回 `status=running` 和 `session_id` THEN 系统 SHALL 建立有界后台进程记录并关联原始 trace。
2. WHEN 后续调用携带相同 `session_id` THEN 系统 SHALL 关联父 trace、更新进程状态，并在终态后移出活跃列表。
3. IF 请求、响应、命令或错误包含敏感字段和常见凭据形式 THEN 系统 SHALL 在存储与事件发送前脱敏并应用大小限制。
4. IF 活跃后台进程超过 100 个 THEN 系统 SHALL 淘汰最久未更新记录并清理其关联映射。

### FR-3: 有界实时活动事件

**优先级:** Must
**用户故事:** 作为桌面端用户，我想在调用发生时立即看到变化，以便无需等待固定轮询周期。

#### 验收标准（EARS）

1. WHEN trace 或后台进程状态变化 THEN 系统 SHALL 通过容量为 256 的广播通道发送已脱敏事件。
2. WHILE Activity 页面可见 THE 系统 SHALL 响应 Tauri 事件并合并刷新列表。
3. WHILE Activity 页面隐藏 THE 系统 SHALL 忽略事件触发的刷新并暂停兜底同步。
4. IF 事件丢失、订阅滞后或窗口重建 THEN 系统 SHALL 通过最多每 15 秒一次的快照同步恢复一致状态。

### FR-4: 主动验证 MCP 服务真实性

**优先级:** Must
**用户故事:** 作为 MCP 运维者，我想主动验证本地和公网协议端点，以便知道某一时刻完整 MCP 握手是否成功。

#### 验收标准（EARS）

1. WHEN 页面加载或用户刷新 listener 状态 THEN 系统 SHALL 查询每个工作区的当前 MCP runtime 状态。
2. WHEN 用户点击“验证 MCP” THEN 系统 SHALL 调用现有健康检查并展示本地传输、公网传输和完整握手结果。
3. WHEN 验证完成 THEN 系统 SHALL 显示完成时间，并注明结果只代表该次检查时刻。
4. IF 公网未配置或检查失败 THEN 系统 SHALL 显示明确的未配置、降级或失败状态，不影响本地状态展示。

### FR-5: 保持现有兼容性与操作能力

**优先级:** Should
**用户故事:** 作为现有用户，我想继续使用筛选、详情和清空功能，以便升级不改变既有工作流。

#### 验收标准（EARS）

1. WHILE 新监控能力启用 THE 系统 SHALL 保留工作区、工具和状态筛选以及 trace 详情。
2. WHEN 用户清空 Activity THEN 系统 SHALL 同时清理 trace、活跃进程和关联映射，并实时更新页面。
3. IF Actions 服务有流量 THEN 系统 SHALL 不将其混入本页，并在界面中明确范围为 MCP。

---

## 非功能需求

- **NFR-1（性能）**: trace 上限保持 500，单个 JSON 值上限保持 16 KiB，活跃进程上限 100，事件通道容量 256，兜底同步周期不低于 15 秒。
- **NFR-2（安全）**: `password`、token、secret、API key、authorization、cookie、`raw_user_input`、`initial_user_input` 及常见命令行/环境变量凭据在任何 Activity 快照和事件中均不得出现原文。
- **NFR-3（兼容性）**: 不改变 MCP、Actions、隧道、OAuth、Bearer 或工作区公共契约；Windows 11 桌面端保持可构建。
- **NFR-4（可访问性）**: 状态不能仅依赖颜色，按钮使用 Lucide 图标并提供可读标签或提示，紧凑布局在窄窗口不重叠。

---

## 依赖关系

- 依赖 `src-tauri/src/health/checker.rs` 的完整 MCP 握手结果。
- 依赖 `RuntimeSupervisor` 和现有工作区 IPC 获取 listener 状态。
- 依赖 Tauri 2 `Emitter`/前端 `listen` 事件能力和 Svelte 5 生命周期。
- 依赖现有 ActivityStore、Activity API、工作区 store 与 UI token。

---

## 检查清单

- [x] 已消化参考仓库的实时事件与进程关联模式，并规避原文泄漏风险
- [x] 需求覆盖核心场景与边界场景
- [x] 每条需求有唯一 ID，并由设计和任务回链
- [x] 验收标准使用 EARS 格式且可测
- [x] 已标注优先级
- [x] 范围边界明确
- [x] 非功能需求可量化
- [x] 依赖关系完整
