# mcp-runtime-history-activity-hardening

## 原则

在现有 Windows 11 Tauri/Svelte 桌面端中强化 MCP/Actions 生命周期、升级 History v3，并增加安全的活动监控。保持 Transport v2、Cloudflare、OAuth 和现有工具契约兼容，不引入 Linux Headless Gateway。

## 子规格索引

| ID | 标题 | FR | 依赖 |
|---|---|---|---|
| runtime-lifecycle | Runtime 优雅启停与异常回收 | FR-1, FR-6 | 无 |
| history-v3 | History v3 状态投影与完整性 | FR-2, FR-3, FR-6 | 无 |
| activity-observability | 脱敏活动追踪与桌面监控 | FR-4, FR-5, FR-6 | runtime-lifecycle |

## 依赖关系

`activity-observability` 依赖 `runtime-lifecycle` 提供稳定的 listener 生命周期；History v3 与 Runtime 可独立实施。机器可读关系以 `spec-manifest.json` 为准。

## 里程碑

1. 完成 Runtime 生命周期加固。
2. 完成 History v3 数据契约与迁移兼容。
3. 接入脱敏活动追踪和桌面监控页面。
4. 完成 Rust、前端和安全回归验证。
