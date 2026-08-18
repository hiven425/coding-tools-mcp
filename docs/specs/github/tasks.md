# 任务清单：推送自动构建 Windows EXE

## 概述

实现 `main` 推送即生成 Windows NSIS Artifact 的独立工作流，并记录无 Tag 下载方式。

## 交付物清单

- **预计新建文件数**: 4 个
- **预计修改文件数**: 1 个
- **预计新增或修改函数数**: 0 个
- **交付物逐项列举**:
  1. `.github/workflows/windows-exe.yml`
  2. `README.md` 中的快照下载说明
  3. `docs/specs/github/requirements.md`
  4. `docs/specs/github/design.md`
  5. `docs/specs/github/tasks.md`

## 任务列表

### 阶段 1: 准备与规格

- [x] 1.1 核对现有 Release 与 CI 触发方式，锁定快照工作流边界
  - **证据块**: `.github/workflows/release.yml:1-6` 当前仅接受 `v*` Tag 和 `workflow_dispatch`；`.github/workflows/ci.yml:1-4` 当前仅手动触发。
  - **涉及文件**: `docs/specs/github/` 3 个文件，约 220 行。
  - _需求: FR-1, FR-3_ ｜ _设计: 技术方案、设计决策 1_

### 阶段 2: 核心实现

- [x] 2.1 新增 main 推送 Windows NSIS 构建，上传唯一且有界的 Artifact
  - **证据块**: `.github/workflows/release.yml:12-45` 已验证 Node 20、stable Rust、`npm ci`、前端检查、Rust 测试及 `--bundles nsis` 的构建路径，可复用同一工具链。
  - **涉及文件**: `.github/workflows/windows-exe.yml`，约 60 行。
  - _需求: FR-1, FR-2, FR-3_ ｜ _设计: 技术方案、构建流程_

- [x] 2.2 补充 Actions Artifact 下载入口，区分快照与正式 Release
  - **证据块**: `README.md:47-56` 目前只说明从 Releases 下载正式安装包，没有无 Tag 快照入口。
  - **涉及文件**: `README.md`，约 4 行。
  - _需求: FR-1, FR-3_ ｜ _设计: 设计决策 1_

### 阶段 3: 验证

- [x] 3.1 校验工作流语法和关键契约，并检查本次差异范围
  - **证据块**: `src-tauri/tauri.conf.json:24-35` 已启用 bundle 和 Windows `.ico`；NSIS 输出路径与现有 Release 工作流一致。
  - **涉及文件**: 不新增测试文件；验证 `.github/workflows/windows-exe.yml` 和相关差异。
  - _需求: FR-1, FR-2, FR-3_ ｜ _设计: 测试策略_

## 检查点

- [x] 阶段 1 完成后：规格覆盖 push、手动触发、权限、并发、失败和正式发布边界。
- [x] 阶段 2 完成后：工作流和 README 与规格一致。
- [x] 阶段 3 完成后：YAML 解析、静态契约检查和 `git diff --check` 通过。

## 需求覆盖矩阵

| 需求 ID | 设计章节 | 任务编号 | 状态 |
| --- | --- | --- | --- |
| FR-1 | 技术方案、构建流程 | 2.1, 2.2, 3.1 | 完成 |
| FR-2 | 构建流程、数据模型 | 2.1, 3.1 | 完成 |
| FR-3 | 设计决策 1 | 1.1, 2.1, 2.2, 3.1 | 完成 |

## 文件变更清单

| 文件 | 操作 | 行数预算 | 说明 |
| --- | --- | --- | --- |
| `.github/workflows/windows-exe.yml` | 新建 | 60 | Windows 快照构建与 Artifact 上传 |
| `README.md` | 修改 | 4 | 无 Tag 快照下载说明 |
| `docs/specs/github/requirements.md` | 新建 | 110 | 需求与边界 |
| `docs/specs/github/design.md` | 新建 | 110 | 构建流程和决策 |
| `docs/specs/github/tasks.md` | 新建 | 90 | 实施与验收任务 |

## 检查清单

- [x] 交付物数量已锁定。
- [x] 每条任务含现状证据、文件预算和需求回链。
- [x] 每条 FR 至少映射到一个实现与验证任务。
- [x] 任务不修改正式 Release 或既有 CI。
