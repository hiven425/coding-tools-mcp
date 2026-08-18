# 任务清单：stable-mcp-fixed-domain

## 交付策略

子规格 `tasks.md` 是任务明细的唯一来源；本文件只维护实施阶段、依赖和 FR 覆盖。实施必须遵循 RED -> GREEN -> focused regression，并在编辑每个函数/类/方法前执行 GitNexus upstream impact。

## 交付物清单

- 固定域名安全配置解析、canonical endpoint 与唯一 owner 校验。
- Cloudflare Named Tunnel 真就绪、组合连接状态机与有界恢复。
- MCP `2025-06-18` Streamable HTTP、OAuth discovery 和协议契约测试。
- 分层 MCP 健康探针、统一脱敏与状态展示。
- Windows 登录会话内常驻、兼容迁移、回滚与离线回归矩阵。
- 固定域名部署、诊断和平台边界文档。

## 任务列表

1. [固定域名与隧道生命周期](subspecs/fixed-domain-tunnel/tasks.md)：`fixed-domain-tunnel/1.1`、`fixed-domain-tunnel/1.2`、`fixed-domain-tunnel/1.3`、`fixed-domain-tunnel/1.4`、`fixed-domain-tunnel/1.5`、`fixed-domain-tunnel/1.6`、`fixed-domain-tunnel/1.7`。
2. [Streamable HTTP 与 OAuth 合规](subspecs/streamable-http-compliance/tasks.md)：`streamable-http-compliance/2.1`、`streamable-http-compliance/2.2`、`streamable-http-compliance/2.3`、`streamable-http-compliance/2.4`、`streamable-http-compliance/2.5`、`streamable-http-compliance/2.6`。
3. [健康检查与可观测性](subspecs/health-observability/tasks.md)：`health-observability/3.1`、`health-observability/3.2`、`health-observability/3.3`、`health-observability/3.4`、`health-observability/3.5`。
4. [Windows 常驻、迁移与回归验证](subspecs/windows-migration-validation/tasks.md)：`windows-migration-validation/4.1`、`windows-migration-validation/4.2`、`windows-migration-validation/4.3`、`windows-migration-validation/4.4`、`windows-migration-validation/4.5`、`windows-migration-validation/4.6`、`windows-migration-validation/4.7`。

## 文件变更清单

- Rust 后端：`src-tauri/src/settings/`、`tunnel/`、`runtime/`、`mcp/`、`auth/`、`health/`、`commands/` 与 workspace model。
- 前端状态：`src/lib/api/health.ts`、`HealthPanel.svelte`。
- 依赖与测试：`src-tauri/Cargo.toml`、`Cargo.lock`、`src-tauri/tests/`、`.github/workflows/ci.yml`。
- 文档：`docs/fixed-domain-mcp.md`、项目中英文 README；实际实现时以影响分析和最小改动为准。

## 需求覆盖矩阵

| FR ID | 子规格 | 任务引用 | 状态 |
|---|---|---|---|
| FR-1 | fixed-domain-tunnel | fixed-domain-tunnel/FDT-1、FDT-2 | 已完成 |
| FR-2 | fixed-domain-tunnel | fixed-domain-tunnel/FDT-1、FDT-2 | 已完成 |
| FR-3 | fixed-domain-tunnel | fixed-domain-tunnel/FDT-3、FDT-4 | 已完成 |
| FR-4 | fixed-domain-tunnel | fixed-domain-tunnel/FDT-5、FDT-6 | 已完成 |
| FR-5 | streamable-http-compliance | streamable-http-compliance/SHC-2、SHC-3 | 已完成 |
| FR-6 | streamable-http-compliance | streamable-http-compliance/SHC-1、SHC-3 | 已完成 |
| FR-7 | streamable-http-compliance | streamable-http-compliance/SHC-4、SHC-5 | 已完成 |
| FR-8 | health-observability | health-observability/HOB-1、HOB-2 | 已完成 |
| FR-9 | health-observability | health-observability/HOB-3、HOB-4 | 已完成 |
| FR-10 | windows-migration-validation | windows-migration-validation/WMV-1、WMV-2 | 已完成 |
| FR-11 | windows-migration-validation | windows-migration-validation/WMV-3、WMV-4 | 已完成 |
| FR-12 | windows-migration-validation | windows-migration-validation/WMV-5、WMV-6 | 已完成 |

## 全局闸门

- [x] 规格通过 `check_spec` 后才允许修改源码。
- [x] `.env` 真实值未出现在 `git diff`、日志 fixture 或快照。
- [x] 每个待修改符号均有 GitNexus impact 记录；HIGH/CRITICAL 先向用户告警。
- [x] SDK 选型必须以 spike 证据为准，不把依赖替换与业务重构混为一步。
- [x] 最终运行 GitNexus detect-changes，确认只影响预期执行流（bridge disabled 时按 `code_insight` 降级并人工审查文件/调用范围）。

## 估算

- **故事点**：聚合 29 SP；fixed-domain-tunnel 8、streamable-http-compliance 8、health-observability 5、windows-migration-validation 8。
- **乐观/正常/悲观**：72h（9 人日）/ 104h（13 人日）/ 160h（20 人日），按 8h/人日、1 名资深工程师计算。
- **PERT 期望**：约 108h（13.5 人日）；置信度中等。
- **正常工期拆分**：固定域名与隧道 28h、HTTP/OAuth 28h、健康与可观测性 20h、Windows/迁移/离线验证 20h、集成审查与文档 8h。
- **主要不确定性**：rmcp 与现有认证/工具结果兼容度、Windows 子进程行为、ChatGPT OAuth path-aware discovery 差异。
- **估算假设**：不引入多工作区 Gateway；cloudflared 与 Windows 验证环境可用；SDK spike 失败时采用薄 adapter；不包含无用户登录 Windows Service。
