# 需求文档：推送自动构建 Windows EXE

## 功能概述

为维护者提供无需创建 Git Tag 的 Windows 桌面端快照构建：代码推送到 `main` 后，GitHub Actions 自动检查并构建 Tauri NSIS `.exe`，维护者可直接从工作流的 Artifacts 下载验证。

## 历史经验与坑

- **可复用经验**: 复用现有 `release.yml` 中经过验证的 Node、Rust、Tauri 和 NSIS 构建步骤。
- **必须规避的坑**: 不修改正式 Release 工作流，也不在无 Tag 构建中创建 GitHub Release；Artifact 必须有唯一、可追踪的名称并在缺失安装包时失败。

## 术语定义

- **快照构建**: 由分支推送生成、仅存放在 Actions Artifacts 中的临时安装包。
- **正式 Release**: 由现有 `release.yml` 基于 `v*` Tag 或手动输入 Tag 创建的 GitHub Release。

## 范围边界

**In Scope**

- `main` 分支 push 后自动构建 Windows x64 NSIS `.exe`。
- 支持在 Actions 页面手动重新触发同一工作流。
- 执行前端检查、锁定依赖的 Rust 测试和 Tauri NSIS 构建。
- 上传带工作流运行编号和尝试编号的 Artifact，保留 14 天。
- 在 README 说明无 Tag 下载入口。

**Out of Scope**

- 创建或更新 Git Tag、GitHub Release。
- Windows 代码签名、自动发布、自动升级应用版本。
- macOS 或 Linux 安装包构建。
- 修改现有 `.github/workflows/release.yml` 和 `.github/workflows/ci.yml`。

## 需求列表

### FR-1: 推送自动生成 Windows 安装包

**优先级:** Must

**用户故事:** 作为项目维护者，我想在推送 `main` 后自动获得 Windows `.exe`，以便无需 Tag 即可下载和验证 GUI 安装包。

#### 验收标准

1. WHEN 提交被推送到 `main` THEN GitHub Actions SHALL 在 `windows-latest` 上启动 Windows EXE 构建。
2. WHEN 依赖安装完成 THEN 工作流 SHALL 依次通过前端检查、`cargo test --locked` 和 Tauri NSIS 构建。
3. WHEN 构建成功 THEN 工作流 SHALL 上传 `src-tauri/target/release/bundle/nsis/*.exe`。
4. IF 未生成 `.exe` THEN Artifact 上传步骤 SHALL 失败并使工作流可见地失败。

### FR-2: 可追踪且有界的快照产物

**优先级:** Must

**用户故事:** 作为项目维护者，我想区分每次推送产生的安装包，以便定位产物对应的工作流执行。

#### 验收标准

1. WHEN Artifact 被上传 THEN 名称 SHALL 包含运行编号和运行尝试编号。
2. WHEN Artifact 超过 14 天 THEN GitHub Actions SHALL 按保留策略清理它。
3. WHILE 同一分支的旧构建仍在运行 THEN 新推送 SHALL 取消旧构建，避免浪费构建资源。

### FR-3: 保留正式发布边界

**优先级:** Must

**用户故事:** 作为发布维护者，我想让快照构建与正式 Release 分离，以免无 Tag 推送意外发布版本。

#### 验收标准

1. WHEN 快照工作流运行 THEN 它 SHALL 只申请读取仓库内容的权限。
2. WHEN 快照工作流成功 THEN 它 SHALL 不创建 Tag 或 GitHub Release。
3. WHILE 正式发布流程继续使用 THEN 现有 `release.yml` SHALL 保持不变。

## 非功能需求

- **NFR-1（安全）**: 工作流权限限制为 `contents: read`，不授予发布写权限。
- **NFR-2（可维护性）**: 使用仓库现有 Node 20、stable Rust、npm 和 Tauri 命令，不引入新依赖。
- **NFR-3（兼容性）**: 产物为 Tauri 2 在 `windows-latest` 生成的 NSIS Windows x64 安装程序。

## 依赖关系

- GitHub Actions 托管的 `windows-latest` runner。
- npm 锁文件、Cargo 锁文件以及现有 Tauri NSIS 配置。
- GitHub Actions Artifact 存储。

## 检查清单

- [x] 需求覆盖核心场景与失败场景。
- [x] 每条需求都有唯一 ID 并可回链。
- [x] 范围明确排除 Tag、Release、签名和 macOS。
- [x] 权限、并发和保留期要求明确。
