# stable-mcp-fixed-domain

固定公网域名下的 MCP 稳定连接优化规格。该规格以当前每工作区 Rust/Tauri 运行时为基础，不直接引入多工作区 Gateway，也不在本阶段实现代码。

## 目标

- 使用项目根目录中被 Git 忽略的 `.env` 作为本地部署覆盖源，仅引用 `cloudflare_host_name` 和 `cloudflare_token` 变量名。
- 将 Cloudflare Named Tunnel、MCP listener、OAuth canonical identity 和健康检查收敛为一条可观测、可恢复、可回滚的连接链路。
- 修正 MCP `2025-06-18` Streamable HTTP 兼容问题，避免客户端升级后出现偶发连接失败。
- 保持既有工作区配置、工具名称、工具 schema、工具返回结构和非固定域名连接模式兼容。

## 原则

- 固定公网 identity 与 listener、cloudflared 等短生命周期进程解耦。
- 本地可用和公网可用分别判定，只有真实 MCP 握手通过才报告 `public-ready`。
- secret 只在配置边界读取并统一脱敏，不进入普通 DTO、日志、fixture 或版本库。
- 优先做小范围兼容适配；Rust MCP SDK 必须通过 spike 门禁后才能接管 transport。

## 子规格索引

| 子规格 | 负责需求 | 依赖 |
|---|---|---|
| [fixed-domain-tunnel](subspecs/fixed-domain-tunnel/spec.md) | FR-1 至 FR-4 | 无 |
| [streamable-http-compliance](subspecs/streamable-http-compliance/spec.md) | FR-5 至 FR-7 | 无 |
| [health-observability](subspecs/health-observability/spec.md) | FR-8 至 FR-9 | 前两项 |
| [windows-migration-validation](subspecs/windows-migration-validation/spec.md) | FR-10 至 FR-12 | 前三项 |

## 依赖关系

`fixed-domain-tunnel` 和 `streamable-http-compliance` 可以独立推进；`health-observability` 依赖二者输出的状态与协议契约；`windows-migration-validation` 负责在前三项稳定后完成平台、迁移和发布验证。外部运行依赖为 Rust/Tauri、Cloudflare Named Tunnel、现有 SecretStore 与 OAuth 模块。

## 里程碑

1. 先完成配置边界、固定域名规范化、Named Tunnel 真就绪和生命周期状态机。
2. 独立完成 SDK 适配验证和 Streamable HTTP/OAuth 合规层。
3. 基于前两项实现真实 MCP 握手健康检查和脱敏日志。
4. 最后完成 Windows 常驻运行边界、迁移、回滚和全链路回归。

## 安全边界

- 规格、Git diff、日志、错误、测试 fixture 和快照中不得出现 `.env` 的真实值。
- `cloudflare_token` 始终按 secret 处理；`cloudflare_host_name` 只允许在用户主动查看连接地址的 UI/API 中显示规范化结果。
- 自动化测试只使用本地假服务和虚构域名，不访问真实 Cloudflare 或公网。

## 文档入口

- [需求](requirements.md)
- [设计](design.md)
- [任务](tasks.md)
- [子规格清单](spec-manifest.json)
