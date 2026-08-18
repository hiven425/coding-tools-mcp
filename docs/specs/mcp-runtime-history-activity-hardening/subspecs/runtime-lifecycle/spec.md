# 子规格：Runtime 优雅启停与异常回收

## 范围

加固 MCP 和 Actions listener 的启动、停止、异常退出检测与状态回收。

## 需求回链

- FR-1
- FR-6

## 验收标准（EARS）

1. WHEN 启动服务且已有 handle 已结束 THEN 系统 SHALL 清理旧状态并创建新 listener，而不是返回旧 Running。
2. WHEN 标准 listener 已同步绑定 THEN 系统 SHALL 在 Tauri Tokio runtime 内转换为 Tokio listener。
3. WHEN 用户停止服务或退出应用 THEN 系统 SHALL 发出 graceful shutdown 并在有界时间内等待 listener 和端口释放。
4. WHEN listener 转换或 serve 失败 THEN 系统 SHALL 将错误写入对应工作区日志并使状态可恢复。

## 涉及文件

- `src-tauri/src/runtime/supervisor.rs`
- `src-tauri/src/mcp/listener.rs`
- `src-tauri/src/actions/listener.rs`
- `src-tauri/src/runtime/port.rs`

## 不做项

- 不改变 Cloudflare/FRP provider 状态机。
- 不终止无法确认归属的外部进程。

## 设计要点

回收旧 handle 时先从 supervisor 状态移出，避免在持锁期间等待异步任务。
