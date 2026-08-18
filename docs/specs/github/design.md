# 设计文档：推送自动构建 Windows EXE

## 概述

新增独立的 `.github/workflows/windows-exe.yml`。它面向日常 `main` 分支快照构建，仅上传 Actions Artifact，不介入现有 Tag Release 流程。

**对应需求:** FR-1、FR-2、FR-3、NFR-1、NFR-2、NFR-3

## 技术方案

### 技术选型

| 类别 | 选择 | 理由 | 关联需求 |
| --- | --- | --- | --- |
| 运行环境 | `windows-latest` | 直接生成并验证 Windows NSIS 安装包 | FR-1, NFR-3 |
| 前端工具链 | Node 20 + `npm ci` | 与现有发布工作流一致并遵循锁文件 | FR-1, NFR-2 |
| Rust 工具链 | stable + Rust cache | 与现有 Tauri 构建一致并降低重复编译时间 | FR-1, NFR-2 |
| 交付方式 | `actions/upload-artifact@v4` | 不依赖 Tag，可从每次 Actions 运行直接下载 | FR-2, FR-3 |

### 构建流程

```text
push main / workflow_dispatch
        |
        v
checkout -> Node 20 -> stable Rust -> npm ci
        |
        v
npm run check -> cargo test --locked -> Tauri NSIS build
        |
        v
upload *.exe Artifact (14 days)
```

同一 Git ref 使用同一个 concurrency group；新运行取消仍在执行的旧运行。工作流只声明 `contents: read`，因此不能创建 Release 或修改仓库内容。

## 数据模型

不涉及持久化数据。Artifact 名称格式为 `coding-tools-mcp-windows-<run_number>-<run_attempt>`。

## API 设计

不涉及应用 API。GitHub Actions 入口为：

| 入口 | 条件 | 输出 | 关联需求 |
| --- | --- | --- | --- |
| `push` | branch 为 `main` | Windows NSIS Artifact | FR-1 |
| `workflow_dispatch` | 维护者手动触发 | Windows NSIS Artifact | FR-1 |

## 文件结构

```text
.github/workflows/windows-exe.yml
README.md
docs/specs/github/requirements.md
docs/specs/github/design.md
docs/specs/github/tasks.md
```

## 设计决策

### 决策 1: 独立快照工作流（关联需求: FR-3）

**问题**: 现有 `release.yml` 具有 `contents: write` 并负责创建正式 Release，直接扩展其 push 触发会混淆快照与正式发布。

**选项**:

1. 扩展 `release.yml` 并通过条件跳过发布步骤。
2. 新增只读、仅上传 Artifact 的 Windows 工作流。

**决策**: 选择独立 Windows 工作流。

**理由**: 权限和行为边界更清晰，也不会改变已有 Tag 发布路径。

### 决策 2: 不在工作流中自动改版本（关联需求: FR-2）

**问题**: 项目要求应用版本在多个清单中同步，CI 临时改写会让安装包版本与提交内容不一致。

**决策**: 工作流使用提交中已同步的应用版本，只用 Actions 运行编号区分快照 Artifact。

**理由**: 版本递增仍由功能提交按项目规则完成，CI 不制造未提交的版本状态。

## 测试策略

- 用 YAML 解析器检查工作流语法和关键字段。
- 静态核对触发条件、只读权限、并发策略、锁定测试命令、NSIS 产物路径和 Artifact 保留期。
- 用 `git diff --check` 检查格式问题。
- 首次推送后由 GitHub `windows-latest` 实际验证 Windows 构建；本地 Linux 环境不声称完成 Windows 打包验证。

## 风险评估

| 风险 | 影响 | 缓解措施 |
| --- | --- | --- |
| GitHub runner 或 Actions 服务短暂不可用 | 中 | 保留 `workflow_dispatch` 以便重试 |
| Tauri/NSIS 产物路径变化 | 中 | `if-no-files-found: error` 使问题立即可见 |
| 连续推送浪费 runner 时间 | 低 | 同 ref 新运行取消旧运行 |
| 未递增应用版本就推送功能代码 | 中 | CI 不掩盖版本；继续遵循项目既有版本同步规则 |
