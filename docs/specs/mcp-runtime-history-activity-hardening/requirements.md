# 需求文档：mcp-runtime-history-activity-hardening

## 功能概述

降低 listener 异常退出、历史状态错误投影和 MCP 调用不可观测造成的连接排查成本。

## 范围边界

- In Scope：MCP/Actions 内嵌 listener 生命周期、History v3、仅内存的脱敏活动追踪、桌面活动监控。
- Out of Scope：Linux Headless Gateway、远程 Web Admin、持久化完整请求或响应、替换现有隧道架构。

## 需求列表

| FR ID | 需求摘要 | 主子规格 |
|---|---|---|
| FR-1 | 启动前回收已结束 handle，并在 Tokio runtime 内完成 listener 转换 | runtime-lifecycle |
| FR-2 | 当前状态只投影当前 session 最新 checkpoint，返回持久化完整度 | history-v3 |
| FR-3 | 诊断 malformed block 和派生快照 stale/incomplete 状态 | history-v3 |
| FR-4 | 记录有界、脱敏的 MCP 调用状态、耗时和错误 | activity-observability |
| FR-5 | GUI 可筛选、刷新和检查近期 MCP 活动 | activity-observability |
| FR-6 | 保持现有 Transport v2、OAuth、Cloudflare 和公开工具契约兼容 | 全部 |

## 非功能需求

- 活动追踪不得保留 password、token、api_key、Authorization、cookie、raw_user_input 或 initial_user_input 的明文。
- 活动数据只驻留内存，单字段与总条数均有硬上限。
- 停止必须有界等待；启动失败必须记录可操作错误，不能伪装成 Running。
- History 数字 Markdown 仍是不可替代事实源，派生文件可安全重建。
- GUI 不得因轮询失败阻塞工作区主流程。

## 依赖关系

依赖关系以 `spec-manifest.json` 为准。
