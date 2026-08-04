---
name: mcp-probe-kit
description: >-
  在已配置 mcp-probe-kit 的项目中，于新功能、Bug、UI、重构或提交前读取；统一选择首个 MCP、汇总当前对话构造完整参数，并让复杂功能通过 start_feature 自动采用 flat 或 parent-child Spec。仅负责工具路由与参数纪律，不承载完整研发流程。Routes coding intent, builds complete MCP arguments, and selects flat or parent-child specs for complex features.
mcp-probe-kit-version: "4.0.0-rc.8"
---

# MCP 调用时机 — mcp-probe-kit

> 本 Skill 负责：**什么情况调哪个 MCP，以及调用前如何构造完整参数**。不是开发流程剧本。
> 由 mcp-probe-kit 自动安装；支持 MCP 的 Agent 客户端可从 `.agents/skills/` 加载。

## 总规则

1. **先查下表**，有对应 MCP 就先调，再写代码 / 改文件
2. **拿不准** → `workflow`：`{ "intent": "<结合当前对话整理的完整任务摘要>" }`
3. `start_*` 会列出后续该调的 MCP；按返回逐步调用即可

## 执行通道与自动降级

### 首选：原生 MCP

如果当前 Agent 可以直接调用 `workflow`、`start_feature`、`start_bugfix`、`start_ui`、`plan_heartbeat`、`resume_plan`、`converge` 等工具，优先使用原生 MCP。

### 降级：项目 CLI

如果 MCP 面板已连接，但当前 Agent 会话看不到上述 MCP 工具，不要跳过 mcp-probe-kit 工作流，也不要要求用户安装。直接通过终端调用项目内版本锁定启动器：

Windows（PowerShell / CMD，优先使用不受脚本执行策略影响的 CMD 启动器）：

```powershell
& ./.mcp-probe-kit/bin/probe.cmd exec <tool> --stdin
```

macOS / Linux：

```bash
./.mcp-probe-kit/bin/probe exec <tool> --stdin
```

CLI 返回 JSON；读取 `structuredContent`、`content` 和 `isError`，继续执行与原生 MCP 相同的工具链。

### 启动器缺失时自修复

如果 `.mcp-probe-kit/bin/probe.*` 不存在，由 Agent 自行执行：

Windows：

```powershell
npx.cmd --yes mcp-probe-kit@4.0.0-rc.8 install-agent --project-root .
```

macOS / Linux：

```bash
npx --yes mcp-probe-kit@4.0.0-rc.8 install-agent --project-root .
```

安装后立即改用项目内启动器。不得使用 `@latest` 或 `@next`，不得全局安装，也不得修改用户项目的 `package.json`。Skill、CLI 启动器和 MCP 必须保持同一精确版本 `4.0.0-rc.8`。

### GitNexus 托管运行时

GitNexus 不打包进 mcp-probe-kit 主包，也不做全局安装。优先使用用户显式配置或系统已有的兼容 CLI；否则使用 mcp-probe-kit 管理的精确版本 Sidecar。

如果工具结果包含 `managed_install_required`，Agent 不要让用户手动安装，自动执行一次：

Windows：

```powershell
& ./.mcp-probe-kit/bin/probe.cmd doctor gitnexus --install
```

macOS / Linux：

```bash
./.mcp-probe-kit/bin/probe doctor gitnexus --install
```

安装完成后重试原工具。安装失败或超时则保留降级结果继续，不得阻塞主工作流。可通过 `MCP_GITNEXUS_MODE=system|managed|off` 控制策略。

---

## 参数构造纪律

- 用户只说“继续 / 开始 / 往下做”时，先结合当前对话、已有 Spec 和用户已确认决定，重建完整任务摘要；禁止把短确认语原样传给 `workflow.intent` 或 `start_*.description`。
- 新功能默认调用 `start_feature`，并传 `description=<完整范围摘要>`、`spec_layout=auto` 和明确的 `project_root`；让编排器决定 flat 或 parent-child。
- 跨模块、多阶段、大版本或架构升级不得直接调用 `add_feature`；只有布局和 `subspecs` 已明确时，才按 `start_feature` 返回的 plan 调用它。
- 工具参数必须表达当前任务事实，不要只复制用户最后一条消息；当前项目代码和已落盘 Spec 优先于历史记忆。
- 拿到 Delegated Plan 后首次调用 `plan_heartbeat` 时附完整 plan；每完成、跳过或阻塞步骤后更新检查点。

---

## 意图速查（第一个该调的 MCP）

| 用户说什么 / 什么情况 | 第一个 MCP |
|----------------------|------------|
| 新功能、加模块、做需求 | `start_feature` |
| Bug、报错、异常、排查、不生效 | `start_bugfix` |
| 页面、组件、样式、UI、交互 | `start_ui` |
| 不熟代码、架构、调用链、影响面 | `code_insight` |
| 新项目上手、熟悉仓库 | `start_onboard` |
| 产品方案、PRD、原型 | `start_product` |
| 长周期自主迭代（Ralph） | `start_ralph` |
| 缺 AGENTS.md / 项目上下文 | `init_project_context` |
| 全新空仓库脚手架 | `init_project` |
| 写 commit message | `gencommit` |
| 代码评审、安全检查 | `code_review` |
| 重构、整理代码 | `refactor（大改前先 code_insight）` |
| 估算工时、排期 | `estimate` |
| 校验规格是否写全 | `check_spec` |
| 查历史踩坑、可复用经验 | `search_memory` |
| 需求不清楚、要澄清 | `ask_user 或 interview` |
| 工作报告、周报、git 汇总 | `git_work_report` |
| 不确定用哪个 MCP | `workflow` |

---

## 全工具：何时调用

### 编排入口 `start_*`（复杂任务的第一步）

| MCP | 何时调用 |
|-----|----------|
| `start_feature` | 任何**新功能 / 增强 / 大版本升级**的首选入口；先把当前对话已确认的完整范围汇总到 description，默认 `spec_layout=auto`，复杂多模块需求先拆 parent-child 子规格，再指引 `add_feature` → `check_spec` → 实现 |
| `start_bugfix` | 任何 **Bug / 报错**；指引 `fix_bug`（真因）→ `gentest` → 测试 |
| `start_ui` | 任何 **UI / 页面 / 组件**；指引设计系统、模板检索、实现约束 |
| `start_onboard` | **新成员 / 新仓库**快速建立心智模型 |
| `start_product` | 从 0 做**产品方案**（PRD、原型思路） |
| `start_ralph` | 需要**多轮自主迭代**、长任务循环时 |

### 路由

| MCP | 何时调用 |
|-----|----------|
| `workflow` | **不确定**该用哪个 MCP；或担心 Agent 跳过 MCP 直接写代码时。intent 必须是完整任务摘要，不是“继续/开始”等最后一句 |

### 项目与规格

| MCP | 何时调用 |
|-----|----------|
| `init_project_context` | 没有 **AGENTS.md**、`docs/project-context/`、图谱索引；大改前缺上下文 |
| `init_project` | **空目录**需要初始化项目结构 |
| `add_feature` | 仅在规格布局已确定时生成 `docs/specs/<feature>/`；复杂需求不得把它当首个入口，通常由 `start_feature` 的 plan 触发 |
| `check_spec` | 规格写完后、**写实现代码前**；或 Bug 修完要过规格闸门 |
| `estimate` | 需要**故事点 / 工时 / 风险**评估（通常在 `add_feature` 之后） |

### 代码分析（可直接调，不必等 start_*）

| MCP | 何时调用 |
|-----|----------|
| `code_insight` | 读不懂代码、找入口、看**调用链 / 影响面**；大重构前；`mode=impact` 评估改动范围 |
| `fix_bug` | 需要 **TBP 真因分析**指南（通常由 `start_bugfix` 触发） |
| `gentest` | 需要**补测试 / 回归用例**（Bug 修复后、功能完成后） |
| `code_review` | 用户要**审查**指定文件或 diff（含安全项） |
| `refactor` | 需要**分步重构计划**；范围大时先 `code_insight` |

### Git

| MCP | 何时调用 |
|-----|----------|
| `gencommit` | 变更完成，需要**规范 commit message** |
| `git_work_report` | 需要基于 git 历史的**工作报告 / 周报** |

### UI 子工具（通常由 `start_ui` 串联）

| MCP | 何时调用 |
|-----|----------|
| `ui_design_system` | 需要**设计 token / 组件规范** |
| `ui_search` | 需要搜 **UI/UX 模板、模式** |
| `sync_ui_data` | UI 内嵌数据过期，需要**同步缓存** |

### 记忆（需 MEMORY 已配置）

| MCP | 何时调用 |
|-----|----------|
| `search_memory` | 主动查**历史经验**；`start_*` 未覆盖时补查 |
| `read_memory_asset` | `search_memory` 命中后需要**读全文** |
| `memorize_asset` | 已有已验证 MemoryCandidate，且 **converge passed=true** 后正式沉淀成功或负面经验 |
| `update_memory_asset` | 修正已有记忆条目 |
| `delete_memory_asset` | 删除错误记忆（需 `confirm: true`） |
| `scan_and_extract_patterns` | 从代码库**批量提取**可复用模式并建议沉淀 |

### 计划状态、恢复与收敛

| MCP | 何时调用 |
|-----|----------|
| `plan_heartbeat` | 执行 Delegated Plan 后记录完成步骤、证据、未决事项和 revision；首次调用附完整 plan |
| `resume_plan` | 会话中断、重启或切换 Agent 后，按 plan_id 恢复下一可执行步骤 |
| `converge` | 实现与验证完成后，检查需求/规格/实现/测试/审查证据；通过后才正式沉淀记忆 |

### 交互

| MCP | 何时调用 |
|-----|----------|
| `ask_user` | 目标模糊、缺关键信息，需要**向用户提问** |
| `interview` | 需要结构化**需求访谈** |

---

## 常见链路（只是调用顺序参考）

**新功能**：`start_feature → plan_heartbeat → add_feature → check_spec（通过）→ 写代码 → gentest → code_review → converge（通过）→ memorize_asset（可选）→ gencommit`

**修 Bug**：`start_bugfix → plan_heartbeat → fix_bug → 改代码 → gentest → 跑测试 → code_review → converge（通过）→ memorize_asset（成功或负面记忆）`

**不熟代码**：`code_insight → 再 start_feature / start_bugfix`

**大重构**：`code_insight（impact）→ refactor → plan_heartbeat → gentest → code_review → converge`

**会话中断后继续**：`resume_plan → 执行 nextStepId → plan_heartbeat → 最终 converge`

---

## 不要

- 有对应 MCP 却**直接大段写实现**
- 把用户的“继续 / 开始 / 往下做”原样当作 `workflow.intent` 或 `start_feature.description`
- 大型跨模块需求绕过 `start_feature` 直接手写单体 Spec
- `check_spec` **未通过**就写功能代码
- 长流程执行步骤后**不** `plan_heartbeat`，导致中断后无法恢复
- `converge` 未通过就把候选经验正式写入 `memorize_asset`
- `delete_memory_asset` 不带 `confirm: true`

---

*mcp-probe-kit 按版本自动同步（当前 `4.0.0-rc.8`）。路径：`.agents/skills/mcp-probe-kit/SKILL.md`*

