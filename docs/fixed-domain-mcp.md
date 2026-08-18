# 固定域名 MCP 部署与回滚

固定域名链路按工作区启用，不需要也不会创建统一 Gateway。每个工作区继续拥有自己的 MCP listener、OAuth 配置和 Cloudflare Named Tunnel。

## 配置

在工作区根目录的 `.env` 中配置以下变量。进程环境变量优先于工作区 `.env`；旧工作区保存的公网 URL 和 SecretStore 仅作为兼容回退。

```dotenv
cloudflare_host_name=mcp.example.invalid
cloudflare_token=token-placeholder
```

- `cloudflare_host_name` 必须是裸主机名或无路径的 HTTPS origin。系统固定派生 `/mcp` 和 OAuth metadata URL。
- `cloudflare_token` 是 Cloudflare Named Tunnel Token。它只驻留于进程环境、工作区 `.env` 或既有 SecretStore，不会进入状态 DTO 或日志。
- 同一固定域名只能由一个工作区占用。冲突会在保存或启动前直接报错。

在工作区中选择 Cloudflare 和 Named Tunnel，并勾选“稳定固定域名链路”后启动 MCP。该工作区开关默认关闭；开启后才启用固定域名解析、新 Streamable HTTP transport 和 Named Tunnel 恢复逻辑。Named Tunnel 强制使用 HTTP/2；只有 cloudflared 报告 `registered tunnel connection` 且进程仍存活，公网状态才会进入 `public-ready`。

## 桌面 GUI 日常使用

1. 打开桌面应用并进入目标工作区。
2. 在隧道配置中选择 `Cloudflare -> Named Tunnel`，勾选“稳定固定域名链路”，保存后启动 MCP。
3. 等健康面板的本地握手和公网状态通过，再把 `https://<固定域名>/mcp` 配置到 ChatGPT 连接器。
4. 在工作区页面找到“ChatGPT 新会话启动提示词”，点击“复制完整提示词”，粘贴到每个新的 ChatGPT 会话。
5. 提示词会要求 ChatGPT 首轮调用 `history_session_bootstrap` 并把用户首轮原文放入 `initial_user_input`。需要早期细节时，ChatGPT 会先调用 `history_session_search`，再用 `history_session_read` 分页精读原始 Markdown。
6. 每轮任务完成前，ChatGPT 应调用 `history_session_checkpoint`，传回 bootstrap 返回的稳定会话目标和本轮逐字 `raw_user_input`。

服务端无法读取没有作为工具参数传入的聊天内容。只有 checkpoint 返回 `ok=true` 且会话目标一致时，才能认为本轮历史已保存。

关闭主窗口时可选择“取消”“后台运行”或“直接关闭”。“后台运行”只隐藏窗口，MCP、Actions 和隧道继续运行；通过系统托盘可重新打开或真正退出。UI 静默重建时会保留托盘隐藏状态。Windows 二次启动会尽量唤起已有实例。

## Canonical URL 与 OAuth

以 `https://mcp.example.invalid` 为例：

- MCP endpoint：`https://mcp.example.invalid/mcp`
- OAuth issuer：`https://mcp.example.invalid`
- Protected Resource Metadata：`https://mcp.example.invalid/.well-known/oauth-protected-resource/mcp`

显式固定域名优先于代理转发头，避免反向代理错误地改变 issuer 或 audience。`GET /mcp` 返回 `405`；JSON-RPC request 返回 JSON，notification 和 client response 返回空的 `202`。

## 健康与诊断

健康检查分别报告 config、local transport、public transport、OAuth 和 MCP handshake。握手会顺序验证 `initialize`、`notifications/initialized` 的空 `202` 和非空 `tools/list`。OAuth 模式下，无人值守检查只验证 401 challenge 与 metadata，不会伪造用户 access token。

公网状态含义：

- `public-ready`：固定域名隧道已注册并存活。
- `public-degraded`：连接失败，正在按指数退避恢复。
- `public-error`：连续 5 次恢复失败，需要检查网络和 cloudflared 日志后手动重试。
- `not-configured`：该工作区未配置公网隧道。

日志会统一清理 Authorization、Bearer、Token、OAuth code 和 URL userinfo。诊断时仍不要把 `.env` 或原始 cloudflared 命令行贴入 issue。

## Windows 11 与 WSL 边界

当前交付是 Tauri 桌面进程，不是 Windows Service。listener、cloudflared 和恢复循环不依赖具体前端页面，WebView 重建不会停止后端；但 Windows 用户注销或桌面应用进程退出后，服务不再可用。真正退出时应用会有界停止 listener 和受管 tunnel，并等待端口释放。

需要登录后自动运行时，可使用 Windows 的“启动应用”或任务计划程序，并选择“仅当用户登录时运行”。这仍是登录会话内常驻，不能保证未登录状态下运行。无用户会话的 Windows Service/headless 宿主需要单独交付。

Windows 工作区必须由 Windows 桌面应用和 Windows 版 cloudflared 处理；WSL 工作区必须在 WSL 环境内使用对应进程。系统不会自动把 `C:\...` 转成 `/mnt/c/...`，也不会跨边界终止进程。

## 回滚

在工作区的 Cloudflare Named Tunnel 配置中取消勾选“稳定固定域名链路”，然后重启 MCP。系统会恢复 legacy `/mcp` discovery/response 和旧 tunnel 重启编排，继续使用原有 FRP、Cloudflare Quick、手工公网 URL 或旧 Named Tunnel 配置。回滚不会删除或迁移工作区、SecretStore、端口、权限策略和历史会话；重新勾选并重启即可恢复固定域名链路。
