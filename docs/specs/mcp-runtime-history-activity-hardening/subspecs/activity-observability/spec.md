# 子规格：脱敏活动追踪与桌面监控

## 范围

为本地 MCP 调用提供仅内存、脱敏、有界的活动记录，并在桌面 GUI 展示。

## 需求回链

- FR-4
- FR-5
- FR-6

## 验收标准（EARS）

1. WHEN MCP 请求进入 dispatch THEN 系统 SHALL 记录唯一 trace、工具名、开始时间和 Running 状态。
2. WHEN请求完成或失败 THEN 系统 SHALL 更新耗时、最终状态和脱敏错误摘要。
3. WHEN任意键或命令文本包含敏感凭据 THEN 系统 SHALL 在进入活动队列前替换为 `[REDACTED]`。
4. WHEN活动条目或字段超过上限 THEN 系统 SHALL 丢弃最旧条目或截断预览，并保持 UI 响应。
5. WHEN用户打开活动页 THEN GUI SHALL 展示近期活动，并支持工作区、工具和状态筛选及手动刷新。

## 涉及文件

- `src-tauri/src/activity.rs`
- `src-tauri/src/app_state.rs`
- `src-tauri/src/mcp/listener.rs`
- `src-tauri/src/commands/activity.rs`
- `src-tauri/src/lib.rs`
- `src/lib/api/activity.ts`
- `src/routes/activity/+page.svelte`
- `src/routes/+layout.svelte`

## 不做项

- 不保存原始 Authorization、cookie、用户原文或命令凭据。
- 不持久化活动记录，不提供远程 Web Admin。

## 设计要点

先递归脱敏，再进行大小限制并存储；GUI 只消费后端已脱敏 DTO。
