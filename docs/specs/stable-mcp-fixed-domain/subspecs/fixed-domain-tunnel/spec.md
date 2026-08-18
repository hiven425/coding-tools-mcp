# 子规格：固定域名与隧道生命周期

## 范围

负责 `.env`/环境覆盖、canonical HTTPS origin、Cloudflare Named Tunnel 真就绪、listener/tunnel 解耦和有界恢复。仍以当前工作区为连接所有者，不引入统一多工作区 Gateway。

## 需求回链

- FR-1
- FR-2
- FR-3
- FR-4

## 验收标准（EARS）

1. WHEN named 工作区启动 THEN 系统 SHALL 从安全配置 provider 解析域名/Token，并在子进程创建前完成校验。
2. IF 配置缺失、域名非法或同域名已有 owner THEN 系统 SHALL 失败且不泄露 secret。
3. WHEN cloudflared 输出 `registered tunnel connection` 且进程存活 THEN 系统 SHALL 把公网状态提交为 ready。
4. WHILE 仅有 metrics/init 日志 THEN 系统 SHALL 保持 starting。
5. WHEN 仅重启 listener THEN 系统 SHALL 保留健康 tunnel，并在本地握手成功后恢复 public-ready。
6. IF 自动恢复耗尽 THEN 系统 SHALL 停止重启循环并进入带下一步建议的 public-error。

## 涉及文件

- `src-tauri/src/settings/fixed_domain.rs`（新增）
- `src-tauri/src/settings/mod.rs`
- `src-tauri/src/workspace/model.rs`
- `src-tauri/src/workspace/resources.rs`
- `src-tauri/src/tunnel/cloudflare.rs`
- `src-tauri/src/tunnel/supervisor.rs`
- `src-tauri/src/tunnel/access.rs`
- `src-tauri/src/commands/runtime.rs`
- `src-tauri/src/commands/tunnel.rs`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`

## 不做项

- 不创建或修改 Cloudflare 账户、DNS、Tunnel Token。
- 不让多个工作区共享一个根 `/mcp`。
- 不把 Quick Tunnel 自动迁移为 Named Tunnel。

## 设计要点

- `.env` 使用结构化解析器，环境覆盖不写回磁盘。
- canonical URL 使用 URL parser，不用字符串拼接校验。
- `TunnelSession` 扩展 provider state、last_error、attempt、next_retry_at；secret 不进入结构体。
- 重启采用“快照旧状态、启动/验证新 listener、成功提交、失败恢复”的事务式流程。
- clock、jitter、cloudflared output 和 process probe 必须可注入测试。
