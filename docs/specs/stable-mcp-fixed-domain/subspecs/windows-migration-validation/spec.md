# 子规格：Windows 常驻、迁移与回归验证

## 范围

负责 Windows 11 进程/端口边界、登录会话内常驻说明、确定性测试、兼容迁移和回滚。该子规格验证前三项交付，不把桌面应用伪装成无登录 Windows Service。

## 需求回链

- FR-10
- FR-11
- FR-12

## 验收标准（EARS）

1. WHEN Tauri 页面关闭或重建但应用进程仍存活 THEN 后端 listener/tunnel SHALL 继续运行。
2. WHEN 应用退出或显式停止 THEN SHALL 有界结束受管进程树并释放端口。
3. WHEN Windows 用户配置登录后启动 THEN 文档 SHALL 使用受支持的托盘/任务计划方式，并明确未登录不保证运行。
4. WHEN 运行 CI/本地回归 THEN 网络交互 SHALL 使用本地假服务，不读取真实 `.env`。
5. WHEN 未启用新模式或执行回滚 THEN 旧工作区、Quick/FRP、工具契约和配置 SHALL 保持可用。

## 涉及文件

- `src-tauri/src/runtime/port.rs`
- `src-tauri/src/platform/windows/process.rs`
- `src-tauri/src/platform/windows/net.rs`
- `src-tauri/src/lib.rs`
- `.github/workflows/ci.yml`
- `src-tauri/tests/` 中新增/扩展的协议与生命周期测试
- `docs/fixed-domain-mcp.md`（新增）
- `README.md`、`README.en.md`

## 不做项

- 不内置 Windows Service 安装器。
- 不承诺注销用户后 Tauri/WebView 继续运行。
- 不自动把 Windows 路径转换成 WSL 路径或反向转换。
- 不删除旧 transport 或旧配置，直至新路径经过独立发布验证。

## 设计要点

- Windows 验证使用显式 PID/端口和进程树，不按镜像名批量终止。
- CI 至少增加 Windows Rust compile/test 目标；无法在 CI 验证的 GUI/托盘场景列为签发清单。
- 固定域名发布先默认关闭，以 workspace/feature flag 灰度；健康检查通过后再设为推荐。
- 回滚步骤不涉及数据迁移，只切换 flag/工作区 tunnel 模式并重启服务。
