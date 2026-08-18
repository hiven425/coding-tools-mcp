# 子任务：Windows 常驻、迁移与回归验证

- [x] 4.1 **WMV-1** 增加 Windows listener/tunnel 生命周期 RED 测试或平台抽象契约测试。
  - 证据块：`runtime/port.rs:175` 已有 listener shutdown 超时/abort；新逻辑必须证明 tunnel 保留与显式退出边界。
  - 覆盖：端口释放、进程树停止、页面重建不停止后端、应用退出停止、crash 后陈旧 PID。
  - 涉及文件：`runtime/port.rs`、Windows platform 模块及测试。
  - _需求：FR-10、FR-11_

- [x] 4.2 **WMV-2** 明确并实现 Windows 登录会话内常驻行为。
  - 证据块：现有交付形态是 Tauri 桌面进程，尚无独立 headless executable 或 Windows Service 宿主。
  - 约束：优先复用托盘/应用生命周期；文档可提供 Task Scheduler 登录触发示例，但不得声称是 Windows Service。
  - 涉及文件：`lib.rs`、必要的窗口生命周期代码、`docs/fixed-domain-mcp.md`。
  - _需求：FR-10_

- [x] 4.3 **WMV-3** 建立无外网的端到端测试 harness。
  - 证据块：固定域名、OAuth 和 cloudflared 真实外部依赖不可作为确定性 CI 前提，必须由本地可控替身提供输入。
  - 组件：假 cloudflared 输出、假 TLS/反向代理边界、假 OAuth metadata、MCP router、可控 clock/process probe。
  - 涉及文件：`src-tauri/tests/common/`、新增 transport/health/lifecycle 测试。
  - _需求：FR-11_

- [x] 4.4 **WMV-4** 扩展 Windows/Linux CI 与专项回归矩阵。
  - 证据块：4.1 的平台契约测试和 4.3 的无外网 harness 提供可在 Windows/Linux runner 重复执行的验证集。
  - 检查：Rust tests、`cargo check`、前端 check；Windows job 验证编译、协议和平台纯逻辑，真实域名仅手工签发。
  - 涉及文件：`.github/workflows/ci.yml`。
  - _需求：FR-11_

- [x] 4.5 **WMV-5** 实现兼容 flag、默认值和灰度迁移。
  - 证据块：现有 workspace JSON 必须继续反序列化；新增字段均有 serde default，Token 不迁移。
  - 覆盖：旧配置、Quick、FRP、手工 public URL、named fixed-domain、关闭 flag 回退。
  - 涉及文件：workspace/settings 数据模型、迁移测试、连接编排入口。
  - _需求：FR-12_

- [x] 4.6 **WMV-6** 编写部署、诊断和回滚文档。
  - 证据块：FR-10 的登录会话边界和 FR-12 的兼容开关决定文档必须明确的部署与回滚路径。
  - 内容：环境变量名、Named Tunnel 前置条件、canonical URL、OAuth、健康层含义、Windows/WSL 边界、登录后常驻、回滚命令/界面步骤。
  - 安全：示例只用 `.invalid` 域名和占位 Token，不复制真实 `.env`。
  - 涉及文件：`docs/fixed-domain-mcp.md`、`README.md`、`README.en.md`。
  - _需求：FR-10、FR-12_

- [x] 4.7 **WMV-7** 完成发布前收敛。
  - 证据块：4.1 至 4.6 的平台测试、离线契约、迁移矩阵和文档共同构成发布闸门输入。
  - 检查：任务级测试通过；GitNexus detect-changes 仅命中预期流程；secret 扫描无新增；人工验证固定域名后清除临时日志。
  - _需求：FR-10、FR-11、FR-12_
