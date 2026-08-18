# 子任务：固定域名与隧道生命周期

- [x] 1.1 **FDT-1** 对配置和资源归属执行 RED 测试。
  - 证据块：`workspace/model.rs:276` 当前从工作区字段计算 public URL；代码尚未读取 `cloudflare_host_name`。
  - 覆盖：优先级、裸 host/HTTPS、非法 URL、缺 Token、同域名双 owner、secret 不出现在 Debug/error。
  - 涉及文件：新增 `settings/fixed_domain.rs` 测试，扩展 workspace resources 测试。
  - _需求：FR-1、FR-2_

- [x] 1.2 **FDT-2** 实现 `FixedDomainConfigProvider`、canonical endpoint 和域名资源校验。
  - 证据块：`SecretStore::get(workspace_id, "cloudflare_token")` 已是现有 Token 入口，环境覆盖不得破坏该 fallback。
  - 涉及文件：`settings/fixed_domain.rs`、`settings/mod.rs`、`workspace/model.rs`、`workspace/resources.rs`、Cargo manifest/lock。
  - _需求：FR-1、FR-2_

- [x] 1.3 **FDT-3** 先增加 Named Tunnel 真就绪 RED 测试。
  - 证据块：`tunnel/cloudflare.rs:410` 当前把 `starting metrics server` 视为 ready。
  - 覆盖：HTTP/2 参数、metrics 不就绪、registered 就绪、提前退出、30s 超时清理、日志打不开。
  - 涉及文件：`tunnel/cloudflare.rs` 测试模块和本地假 output reader。
  - _需求：FR-3_

- [x] 1.4 **FDT-4** 实现 HTTP/2、进程存活确认和真就绪提交。
  - 证据块：`spawn_cloudflare_tunnel` 已集中创建 child 和合并 stdout/stderr，可在该边界最小改动。
  - 涉及文件：`tunnel/cloudflare.rs`。
  - _需求：FR-3_

- [x] 1.5 **FDT-5** 先增加 listener 重启保隧道和恢复耗尽 RED 测试。
  - 证据块：`commands/runtime.rs:99-109` 当前停止 MCP 时无条件停止 tunnel；`runtime.rs:120-127` 吞掉 auto-start 错误。
  - 覆盖：健康 tunnel 复用、bind 失败回滚、tunnel crash、退避、冷却、显式 stop。
  - 涉及文件：`runtime/supervisor.rs`、`tunnel/supervisor.rs`、`commands/runtime.rs` 测试。
  - _需求：FR-4_

- [x] 1.6 **FDT-6** 实现组合连接状态和解耦生命周期。
  - 证据块：现有 `RuntimeStatusDto` 和 `TunnelStatus` 分开报告，需要扩展而非破坏既有字段。
  - 涉及文件：`workspace/model.rs`、`tunnel/supervisor.rs`、`tunnel/access.rs`、`commands/runtime.rs`、`commands/tunnel.rs`。
  - _需求：FR-4_

- [x] 1.7 **FDT-7** 运行固定域名/隧道专项回归。
  - 证据块：以 1.1 至 1.6 的 RED/GREEN 结果、进程状态迁移记录和脱敏扫描作为完成证据。
  - 检查：Rust 目标模块测试；确认 fixture 只含 `.invalid` 域名和假 Token；检查 diff 无 `.env` 值。
  - _需求：FR-1、FR-2、FR-3、FR-4_
