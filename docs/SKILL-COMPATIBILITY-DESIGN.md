# KingdeeKB 技能系统兼容性设计文档

> 版本: v1.0  
> 日期: 2026-05-30  
> 作者: KingdeeKB 架构组  
> 状态: 草案

---

## 目录

- [1. 背景与目标](#1-背景与目标)
- [2. 现状分析](#2-现状分析)
- [3. 原版套件结构](#3-原版套件结构)
- [4. Claude Code 技能加载机制](#4-claude-code-技能加载机制)
- [5. 兼容架构设计](#5-兼容架构设计)
- [6. 模块设计](#6-模块设计)
- [7. 接口定义](#7-接口定义)
- [8. 实现计划](#8-实现计划)
- [9. 风险评估](#9-风险评估)

---

## 1. 背景与目标

### 1.1 项目背景

KingdeeKB 是一款基于 Tauri v2 + React + TypeScript 构建的桌面应用，面向金蝶 ERP 实施顾问群体。核心功能是通过 RAG（检索增强生成）技术实现企业知识管理，帮助顾问快速检索、匹配和应用实施知识。

技能系统（Skill System）是 KingdeeKB 的关键子系统之一。它承载了一套完整的金蝶实施方法论，包含 27 个专业技能，覆盖从项目初始化到交付物产出的全生命周期。这套技能最初以 Claude Code 技能格式（SKILL.md）开发，运行在 `kingdee-implementation-suite-v8.0.0` 套件中。

### 1.2 要解决的问题

当前 KingdeeKB 的技能系统存在三个层面的问题：

**数据层问题**：27 个 SKILL.md 文件全部损坏，每个文件缺失约 2000 字节，中文字符被截断。原版套件中的完整文件（含脚本、引用文档、资产文件等）未被迁移。

**功能层问题**：KingdeeKB 仅实现了基础的技能扫描、搜索和展示功能，缺少 Claude Code 技能系统的核心机制，包括：技能触发（when_to_use）、系统提示注入、参数替换、脚本执行、条件激活等。

**架构层问题**：当前设计将技能视为静态文档，而非可执行的知识单元。缺少事件驱动架构、文件变更检测、技能执行上下文等支撑复杂交互的基础设施。

### 1.3 设计目标

本设计文档的目标是建立一套与 Claude Code 技能系统兼容的架构，分三个阶段实现：

| 阶段 | 目标 | 核心交付物 |
|------|------|-----------|
| Phase 1 | 修复数据，加载支撑文件 | 完整的技能文件迁移、支撑文件加载 |
| Phase 2 | 技能触发与提示注入 | when_to_use 匹配、系统提示组装 |
| Phase 3 | 高级特性 | 脚本执行、模板系统、事件驱动 |

### 1.4 设计原则

**渐进兼容**：不要求一次性实现 Claude Code 的全部特性，按优先级分阶段推进。

**向后兼容**：新架构必须兼容现有的 Tauri 命令接口，前端 Skills 页面不需要大规模重写。

**安全隔离**：脚本执行等高风险操作必须在沙箱环境中进行，遵循最小权限原则。

**性能优先**：技能加载和匹配必须在 100ms 内完成，不影响主应用的响应速度。

---

## 2. 现状分析

### 2.1 KingdeeKB 技能系统架构

当前架构由四层组成：

```
┌─────────────────────────────────────────────────┐
│                  前端展示层                        │
│  src/pages/Skills.tsx (卡片网格 + 搜索 + 详情)     │
└───────────────────────┬─────────────────────────┘
                        │ Tauri invoke
┌───────────────────────┴─────────────────────────┐
│                  命令桥接层                        │
│  src/lib/skill-commands.ts (TS invoke 封装)       │
│  src/lib/skill-types.ts (TS 类型定义)             │
└───────────────────────┬─────────────────────────┘
                        │ IPC
┌───────────────────────┴─────────────────────────┐
│                  Tauri 命令层                      │
│  src-tauri/src/commands/skill.rs (7 个命令)       │
└───────────────────────┬─────────────────────────┘
                        │
┌───────────────────────┴─────────────────────────┐
│                  核心服务层                        │
│  src-tauri/src/services/skill_manager.rs (809行)  │
│  src-tauri/src/services/skill_types.rs            │
└─────────────────────────────────────────────────┘
```

### 2.2 Rust 端核心实现

#### 2.2.1 数据类型 (skill_types.rs)

```rust
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: SkillCategory,
    pub phase: Option<SkillPhase>,
    pub tags: Vec<String>,
    pub path: PathBuf,
    pub metadata: SkillMetadata,
    pub content: String,        // SKILL.md 原始内容
    pub last_modified: u64,
}

pub struct SkillMetadata {
    pub version: Option<String>,
    pub author: Option<String>,
    pub dependencies: Vec<String>,
    pub arguments: Vec<SkillArgument>,
}

pub enum SkillCategory {
    ProjectManagement,
    Technical,
    Documentation,
    QualityAssurance,
    Communication,
    Custom(String),
}

pub enum SkillPhase {
    Init,
    Planning,
    Development,
    Testing,
    Deployment,
    Maintenance,
    Custom(String),
}
```

#### 2.2.2 SkillManager (skill_manager.rs)

核心功能方法：

| 方法 | 行数 | 功能 |
|------|------|------|
| `scan_skills_dir()` | ~80 | 扫描 skills/ 目录，解析 SKILL.md |
| `parse_skill_file()` | ~120 | 解析 YAML frontmatter + Markdown body |
| `search_skills()` | ~60 | 关键词搜索，支持中文 |
| `match_best()` | ~90 | 基于别名和描述的最佳匹配 |
| `import_skill()` | ~50 | 导入外部技能文件 |
| `rescan_skills()` | ~30 | 触发重新扫描 |

#### 2.2.3 Tauri 命令 (commands/skill.rs)

```rust
#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<Skill>, String>

#[tauri::command]
pub async fn get_skill(state: State<'_, AppState>, id: String) -> Result<Skill, String>

#[tauri::command]
pub async fn search_skills(state: State<'_, AppState>, query: String) -> Result<Vec<Skill>, String>

#[tauri::command]
pub async fn match_skill(state: State<'_, AppState>, query: String) -> Result<Option<Skill>, String>

#[tauri::command]
pub async fn import_skill(state: State<'_, AppState>, path: String) -> Result<Skill, String>

#[tauri::command]
pub async fn rescan_skills(state: State<'_, AppState>) -> Result<Vec<Skill>, String>

#[tauri::command]
pub async fn get_skill_stats(state: State<'_, AppState>) -> Result<SkillStats, String>
```

### 2.3 前端实现

`src/pages/Skills.tsx` 提供：

- 卡片网格视图，按分类和阶段筛选
- 全文搜索，支持中文
- 技能详情面板，渲染 Markdown 内容
- 技能导入对话框
- 统计面板（技能数量、分类分布）

### 2.4 差距分析

将 KingdeeKB 当前能力与 Claude Code 技能系统逐项对比：

| 特性 | Claude Code | KingdeeKB | 差距等级 |
|------|-------------|-----------|---------|
| SKILL.md 解析 | 完整 frontmatter + body | 基础 frontmatter | 中 |
| 支撑文件加载 | scripts/, references/, assets/ | 不支持 | 高 |
| _shared 资源 | 跨技能共享 | 不支持 | 高 |
| when_to_use 触发 | 自动匹配注入 | 不支持 | 高 |
| 系统提示注入 | 按预算注入技能列表 | 不支持 | 高 |
| 参数替换 | $ARGUMENTS, $0/$1 等 | 不支持 | 中 |
| 脚本执行 | !`cmd` 内联执行 | 不支持 | 高 |
| 条件激活 | paths 字段匹配 | 不支持 | 中 |
| 文件变更检测 | chokidar 监听 | 不支持 | 低 |
| 使用追踪 | 7 天半衰期指数衰减 | 不支持 | 低 |
| 权限模型 | deny → allow → safe | 不支持 | 中 |
| 技能去重 | 多源优先级去重 | 单源无去重 | 低 |

---

## 3. 原版套件结构

### 3.1 套件根目录

```
kingdee-implementation-suite-v8.0.0/
├── .claude/
│   ├── settings.json          # 空 JSON 对象 {}
│   ├── template-token         # Gitee access_token（敏感信息）
│   ├── templates-manifest.json # 91 个模板，8 个阶段
│   ├── scripts/               # 4 个 Python 脚本
│   │   ├── download_templates.py
│   │   ├── build_release.py
│   │   ├── check-deps.py
│   │   └── extract-content.py
│   └── skills/                # 27 个技能 + _shared/
├── SKILL.md                   # 套件级入口文件
├── CLAUDE.md                  # 项目配置 + 技能注册表
└── package.json
```

### 3.2 技能分类与复杂度

#### 3.2.1 简单技能（15 个）

仅包含 SKILL.md 文件，无额外资源。

```
skills/
├── kd-analyze/SKILL.md
├── kd-check/SKILL.md
├── kd-cleanup/SKILL.md
├── kd-code-review-fix/SKILL.md
├── kd-debug/SKILL.md
├── kd-distilled/SKILL.md
├── kd-list/SKILL.md
├── kd-next/SKILL.md
├── kd-progress/SKILL.md
├── kd-recommend/SKILL.md
├── kd-stats/SKILL.md
├── kd-template/SKILL.md
├── kd-thread/SKILL.md
├── kd-undo/SKILL.md
└── kd-update/SKILL.md
```

#### 3.2.2 中等技能（6 个）

SKILL.md + references/ 或 scripts/。

```
skills/
├── kd-discuss/
│   ├── SKILL.md
│   └── references/
│       ├── discussion-template.md
│       └── context-guide.md
├── kd-execute-phase/
│   ├── SKILL.md
│   └── references/
│       └── execution-patterns.md
├── kd-plan-phase/
│   ├── SKILL.md
│   └── references/
│       └── planning-guide.md
├── kd-spec/
│   ├── SKILL.md
│   └── references/
│       └── spec-template.md
├── kd-uat/
│   ├── SKILL.md
│   └── scripts/
│       └── run-tests.py
└── kd-verify/
    ├── SKILL.md
    └── references/
        └── verification-checklist.md
```

#### 3.2.3 复杂技能（4 个）

包含额外的 assets 或多个脚本。

```
skills/
├── kd-init/
│   ├── SKILL.md
│   ├── references/
│   │   ├── project-structure.md
│   │   ├── config-template.md
│   │   ├── dependency-list.md
│   │   └── setup-guide.md
│   └── scripts/
│       └── scaffold.py
├── kd-gen/
│   ├── SKILL.md
│   ├── references/
│   │   ├── plugin-templates.md
│   │   └── code-patterns.md
│   └── scripts/
│       └── codegen.py
├── kd-ship/
│   ├── SKILL.md
│   ├── references/
│   │   └── release-checklist.md
│   └── scripts/
│       └── prepare-release.py
└── kd-workspaces/
    ├── SKILL.md
    ├── references/
    │   └── workspace-guide.md
    └── scripts/
        └── ws-manager.py
```

#### 3.2.4 重量级技能（2 个）

包含完整的子应用或大量资产。

**kingdee-ppt（28 个文件）**：

```
kingdee-ppt/
├── SKILL.md
├── references/
│   ├── slide-master.md
│   ├── color-palette.md
│   ├── typography.md
│   ├── layout-grid.md
│   ├── chart-styles.md
│   └── animation-guide.md
├── scripts/
│   ├── generate-ppt.py
│   ├── extract-data.py
│   ├── render-charts.py
│   └── validate-output.py
└── assets/
    ├── templates/
    │   ├── cover-01.pptx
    │   ├── cover-02.pptx
    │   ├── content-blank.pptx
    │   ├── content-two-col.pptx
    │   ├── content-image.pptx
    │   ├── content-chart.pptx
    │   ├── ending-01.pptx
    │   └── ending-02.pptx
    ├── logos/
    │   └── kingdee-logo.png
    └── fonts/
        └── SourceHanSansCN-Regular.otf
```

**project-dashboard（完整前后端）**：

```
project-dashboard/
├── SKILL.md
├── references/
│   ├── dashboard-spec.md
│   └── api-design.md
├── frontend/
│   ├── index.html
│   ├── app.js
│   ├── styles.css
│   └── components/
│       ├── chart-widget.js
│       ├── progress-bar.js
│       └── status-card.js
└── server/
    ├── app.py
    ├── routes/
    │   ├── __init__.py
    │   ├── project.py
    │   └── metrics.py
    ├── models/
    │   ├── __init__.py
    │   └── project.py
    └── requirements.txt
```

### 3.3 共享资源 (_shared/)

```
_shared/
├── signals-writer.md          # 事件流格式规范 (JSONL)
├── template-check.md          # 模板检查流程
├── deliverable-scan.md        # 交付物扫描规范
└── scripts/
    ├── platform.py            # 平台检测
    ├── platform_adapter.py    # 平台适配器
    └── path_resolver.py       # 路径解析器
```

`_shared/` 目录存放跨技能共享的规范文档和工具脚本。技能通过相对路径引用这些资源，例如：

```markdown
<!-- 在 SKILL.md 中引用共享脚本 -->
!`python ${CLAUDE_SKILL_DIR}/../_shared/scripts/platform.py`

<!-- 引用共享规范 -->
参考 [_shared/signals-writer.md](../_shared/signals-writer.md) 中的事件流格式。
```

### 3.4 CLAUDE.md 技能注册表

原版套件的 CLAUDE.md 包含一个技能注册表，用于路由和匹配：

```markdown
## 技能注册表

| 技能 ID | 触发关键词 | 阶段 | 优先级 |
|---------|-----------|------|--------|
| kd-init | 初始化, 新建项目, init | Init | 1 |
| kd-plan-phase | 计划, 规划, plan | Planning | 2 |
| kd-discuss | 讨论, 需求, discuss | Planning | 2 |
| kd-execute-phase | 执行, 开发, execute | Development | 3 |
| kd-check | 检查, 规范, check | Testing | 4 |
| ... | ... | ... | ... |
```

这个注册表是当前所有 27 个 SKILL.md 文件内容相同的根本原因。每个 SKILL.md 被错误地写入了套件级描述，而非各技能的独立描述。

---

## 4. Claude Code 技能加载机制

### 4.1 核心文件清单

Claude Code 的技能系统由以下文件实现：

| 文件 | 行数 | 职责 |
|------|------|------|
| `loadSkillsDir.ts` | 1086 | 技能发现、加载、去重、条件激活 |
| `SkillTool.ts` | 1108 | 技能调用入口 |
| `frontmatterParser.ts` | 370 | YAML frontmatter 解析 |
| `prompt.ts` | 241 | 技能列表生成，预算控制 |

### 4.2 SKILL.md 格式规范

#### 4.2.1 完整格式

```markdown
---
name: skill-name
description: 简短描述，用于技能列表展示
when_to_use: |
  详细的使用场景描述。
  当用户的需求匹配这些场景时，模型会自动触发此技能。
allowed-tools:
  - Read
  - Write
  - Bash
  - Glob
arguments:
  - name: arg1
    description: 参数描述
    required: true
  - name: arg2
    description: 参数描述
    required: false
    default: "default-value"
model: claude-3-5-sonnet-20241022
context: inline | fork
effort: low | medium | high
hooks:
  pre: "echo 'before skill'"
  post: "echo 'after skill'"
paths:
  - "src/**/*.ts"
  - "docs/**/*.md"
shell: bash | powershell
version: "1.0.0"
category: development | documentation | testing
---

# 技能标题

技能的详细说明和使用指南。

## 使用步骤

1. 第一步说明
2. 第二步说明

## 参数说明

- `arg1`: 必需参数，用于...
- `arg2`: 可选参数，默认值为...

## 示例

具体的使用示例。

## 脚本调用

!`python ${CLAUDE_SKILL_DIR}/scripts/helper.py $ARGUMENTS`

## 引用文档

参考 [references/guide.md](references/guide.md) 获取详细信息。
```

#### 4.2.2 Frontmatter 字段详解

**name** (string, 必需)

技能的唯一标识符。用于：
- 目录名匹配
- 斜杠命令 `/skill-name`
- 日志和追踪

格式要求：小写字母、数字、连字符，不含空格。

**description** (string, 必需)

技能的简短描述（建议 50-100 字符）。用于：
- 技能列表展示
- 模型自动匹配的初步筛选
- 帮助文档生成

**when_to_use** (string, 推荐)

详细的使用场景描述（建议 100-300 字符）。这段文本会被注入到系统提示中，指导模型在什么情况下自动触发此技能。

示例：
```yaml
when_to_use: |
  Use when generating Kingdee plugin code (FormPlugin/WorkflowPlugin/
  OperationPlugin/BillPlugin/ReportPlugin) or when user asks to create
  Java code for Kingdee Cosmos. Triggers on '生成', 'gen', '代码', 'code',
  'plugin', '插件'
```

**allowed-tools** (string[], 可选)

技能执行时允许使用的工具列表。如果省略，继承默认权限。

**arguments** (object[], 可选)

技能接受的参数定义。每个参数包含：
- `name`: 参数名
- `description`: 参数描述
- `required`: 是否必需
- `default`: 默认值

**model** (string, 可选)

指定技能使用的模型。省略时使用当前会话的默认模型。

**context** (inline | fork, 可选)

技能的执行上下文：
- `inline`: 在当前对话中执行（默认）
- `fork`: 在新的分支对话中执行

**effort** (low | medium | high, 可选)

技能的计算复杂度指示。影响：
- 超时时间设置
- 资源分配优先级

**hooks** (object, 可选)

技能执行前后的钩子：
- `pre`: 技能加载前执行的 shell 命令
- `post`: 技能执行后执行的 shell 命令

**paths** (string[], 可选)

条件激活路径。使用 gitignore 风格的 glob 模式。当用户访问匹配的文件时，技能自动激活。

示例：
```yaml
paths:
  - "src/**/*.java"
  - "src/**/*.xml"
```

**shell** (string, 可选)

脚本执行使用的 shell。默认根据平台自动选择。

**version** (string, 可选)

语义化版本号。

**category** (string, 可选)

技能分类标签。

### 4.3 发现机制

#### 4.3.1 目录扫描

技能系统扫描以下目录（按优先级排序）：

```
1. Managed skills:  ~/.claude/managed-skills/     (最高优先级)
2. User skills:     ~/.claude/skills/
3. Project skills:  .claude/skills/               (最低优先级)
```

扫描规则：
- 只识别目录格式 `skill-name/SKILL.md`
- 忽略 SKILL.md 不存在的目录
- 忽略以 `.` 开头的目录
- 忽略 `_` 开头的目录（如 `_shared`）

#### 4.3.2 去重策略

当多个目录存在同名技能时，按优先级保留：

```
Managed > User > Project
```

同级目录内不允许同名技能。

### 4.4 加载链

完整的加载流程：

```
scanDirs()
  └─ for each dir:
       └─ readSkillFile(dir)
            └─ parseFrontmatter(content)
                 └─ validateFields(frontmatter)
                      └─ createSkillCommand(skill)
                           └─ addToCache(skill)
```

#### 4.4.1 createSkillCommand

将解析后的技能元数据转换为可执行的命令对象：

```typescript
interface SkillCommand {
  name: string;
  description: string;
  whenToUse?: string;
  allowedTools?: string[];
  arguments?: SkillArgument[];
  model?: string;
  context?: 'inline' | 'fork';
  effort?: 'low' | 'medium' | 'high';
  hooks?: { pre?: string; post?: string };
  paths?: string[];
  shell?: string;
  body: string;              // Markdown body（不含 frontmatter）
  filePath: string;          // SKILL.md 的绝对路径
  dirPath: string;           // 技能目录的绝对路径
  loadTime: number;          // 加载耗时（ms）
}
```

#### 4.4.2 Shell 命令执行

在加载阶段，SKILL.md 中的 shell 命令会被执行：

**内联命令**：
```markdown
当前平台: !`python ${CLAUDE_SKILL_DIR}/scripts/platform.py`
```

**块命令**：
```markdown
```! python scripts/setup.py --config ${CLAUDE_SKILL_DIR}/config.json ```
```

替换变量：
- `${CLAUDE_SKILL_DIR}`: 技能目录的绝对路径
- `${CLAUDE_SESSION_ID}`: 当前会话 ID
- `$ARGUMENTS`: 用户传入的所有参数
- `$0`, `$1`, ...: 按位置的参数

### 4.5 调用机制

#### 4.5.1 用户主动调用

用户通过斜杠命令调用技能：

```
/kd-plan-phase 阶段3 --research
```

处理流程：
1. 解析命令名 `kd-plan-phase` 和参数 `["阶段3", "--research"]`
2. 从缓存中查找对应的 SkillCommand
3. 参数替换：`$ARGUMENTS` → `"阶段3 --research"`，`$0` → `"阶段3"`
4. 执行 pre hook（如有）
5. 将技能 body 注入对话上下文
6. 模型根据技能指引执行任务
7. 执行 post hook（如有）

#### 4.5.2 模型自动触发

模型通过 `when_to_use` 字段自动匹配：

1. 系统提示中包含所有技能的 `name` + `description` + `when_to_use`
2. 用户发送消息时，模型评估是否匹配某个技能的 `when_to_use`
3. 如果匹配，模型调用 SkillTool 加载该技能
4. 技能内容注入对话上下文

#### 4.5.3 预算控制

技能列表注入系统提示时有严格的预算限制：

```
预算 = 上下文窗口大小 × 1%
```

对于 200K token 的上下文窗口，技能列表最多占用 2000 token。

每个技能的 token 占用估算：
- name: ~5 token
- description: ~20 token
- when_to_use: ~50 token

27 个技能总计约 2025 token，在预算范围内。

如果超出预算，按以下策略裁剪：
1. 移除 `when_to_use`，只保留 name + description
2. 按使用频率排序，移除低频技能
3. 压缩 description 长度

### 4.6 条件激活

#### 4.6.1 paths 字段

当用户在对话中访问了匹配 `paths` 模式的文件时，对应技能自动激活。

匹配逻辑：
```typescript
function shouldActivate(skill: SkillCommand, accessedFiles: string[]): boolean {
  if (!skill.paths || skill.paths.length === 0) return false;
  return accessedFiles.some(file => 
    skill.paths!.some(pattern => minimatch(file, pattern))
  );
}
```

#### 4.6.2 使用追踪

技能使用记录存储在本地，采用 7 天半衰期指数衰减：

```
score = Σ(usage_i × e^(-λ × days_ago_i))
```

其中 λ = ln(2) / 7 ≈ 0.099

高分技能在列表中优先展示。

### 4.7 权限模型

技能执行时的权限检查流程：

```
1. 检查 deny 列表 → 命中则拒绝
2. 检查 allow 列表 → 命中则允许
3. 检查 safe-attributes → 自动安全操作
4. 以上都不匹配 → 请求用户确认
```

---

## 5. 兼容架构设计

### 5.1 总体架构

新的兼容架构在现有四层基础上扩展：

```
┌─────────────────────────────────────────────────────────┐
│                      前端展示层                           │
│  src/pages/Skills.tsx                                    │
│  src/components/SkillRunner.tsx (新增)                    │
└───────────────────────┬─────────────────────────────────┘
                        │
┌───────────────────────┴─────────────────────────────────┐
│                      命令桥接层                           │
│  src/lib/skill-commands.ts (扩展)                        │
│  src/lib/skill-types.ts (扩展)                           │
│  src/lib/prompt-builder.ts (新增)                        │
└───────────────────────┬─────────────────────────────────┘
                        │
┌───────────────────────┴─────────────────────────────────┐
│                      Tauri 命令层                         │
│  src-tauri/src/commands/skill.rs (扩展)                  │
└───────────────────────┬─────────────────────────────────┘
                        │
┌───────────────────────┴─────────────────────────────────┐
│                      核心服务层                           │
│  skill_manager.rs (重构)                                 │
│  skill_types.rs (扩展)                                   │
│  skill_loader.rs (新增)                                  │
│  skill_trigger.rs (新增)                                 │
│  skill_executor.rs (新增)                                │
│  prompt_assembler.rs (新增)                              │
│  template_manager.rs (新增)                              │
│  signal_writer.rs (新增)                                 │
└─────────────────────────────────────────────────────────┘
```

### 5.2 Phase 1: 修复数据与加载支撑文件

#### 5.2.1 目标

- 修复 27 个损坏的 SKILL.md 文件
- 将原版套件的支撑文件迁移到 KingdeeKB
- 实现支撑文件的加载和展示

#### 5.2.2 数据迁移策略

**迁移源**：`E:\projects\kingdee\skills\kingdee-implementation-suite-v8.0.0\.claude\skills\`

**迁移目标**：`E:\projects\kingdee\KingdeeKB\skills\`

**迁移规则**：

1. SKILL.md 文件：从原版套件复制完整文件，替换损坏文件
2. references/ 目录：完整复制
3. scripts/ 目录：完整复制
4. assets/ 目录：完整复制
5. _shared/ 目录：完整复制到 skills/_shared/
6. 隐藏文件和临时文件：跳过

**迁移脚本**：

```python
# scripts/migrate-skills.py
import shutil
from pathlib import Path

SOURCE = Path(r"E:\projects\kingdee\skills\kingdee-implementation-suite-v8.0.0\.claude\skills")
TARGET = Path(r"E:\projects\kingdee\KingdeeKB\skills")

def migrate():
    for skill_dir in SOURCE.iterdir():
        if skill_dir.name.startswith(('_', '.')):
            continue
        if not skill_dir.is_dir():
            continue
        
        target_dir = TARGET / skill_dir.name
        target_dir.mkdir(parents=True, exist_ok=True)
        
        for item in skill_dir.rglob('*'):
            if item.is_file():
                rel = item.relative_to(skill_dir)
                dest = target_dir / rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(item, dest)
                print(f"  Copied: {rel}")
    
    # 复制 _shared
    shared_src = SOURCE / '_shared'
    if shared_src.exists():
        shared_dst = TARGET / '_shared'
        if shared_dst.exists():
            shutil.rmtree(shared_dst)
        shutil.copytree(shared_src, shared_dst)
        print("  Copied: _shared/")

if __name__ == '__main__':
    migrate()
```

#### 5.2.3 SKILL.md 修复方案

原版套件中 27 个 SKILL.md 文件内容相同（套件级描述）。修复方案：

1. 从原版套件的 CLAUDE.md 中提取技能注册表
2. 为每个技能生成独立的 frontmatter（name, description, when_to_use）
3. 将原版 SKILL.md 的 body 保留为技能内容

需要人工审核的字段：
- `description`: 每个技能的简短描述
- `when_to_use`: 触发条件
- `category`: 技能分类
- `phase`: 所属阶段

#### 5.2.4 支撑文件加载

修改 `skill_manager.rs` 的 `parse_skill_file()` 方法，使其在解析 SKILL.md 后继续扫描技能目录中的其他文件：

```rust
pub struct SkillFile {
    pub path: PathBuf,
    pub name: String,
    pub file_type: SkillFileType,
    pub size: u64,
    pub last_modified: u64,
}

pub enum SkillFileType {
    Reference,    // references/ 下的 .md 文件
    Script,       // scripts/ 下的 .py, .sh, .ps1 文件
    Asset,        // assets/ 下的其他文件
    Shared,       // _shared/ 下的文件
    Config,       // 配置文件 (json, yaml, toml)
}
```

新增方法：

```rust
impl SkillManager {
    /// 扫描技能目录中的支撑文件
    pub fn scan_supporting_files(&self, skill_dir: &Path) -> Result<Vec<SkillFile>, SkillError> {
        let mut files = Vec::new();
        
        for entry in WalkDir::new(skill_dir)
            .min_depth(1)
            .into_iter()
            .filter_entry(|e| !is_hidden(e))
        {
            let entry = entry?;
            let path = entry.path();
            
            // 跳过 SKILL.md 本身
            if path.file_name().map(|n| n == "SKILL.md").unwrap_or(false) {
                continue;
            }
            
            if path.is_file() {
                let file_type = classify_file(path, skill_dir);
                files.push(SkillFile {
                    path: path.to_path_buf(),
                    name: path.file_name().unwrap().to_string_lossy().to_string(),
                    file_type,
                    size: entry.metadata()?.len(),
                    last_modified: entry.metadata()?
                        .modified()?
                        .duration_since(UNIX_EPOCH)?
                        .as_secs(),
                });
            }
        }
        
        Ok(files)
    }
    
    /// 加载引用文件内容
    pub fn load_reference(&self, skill_id: &str, ref_path: &str) -> Result<String, SkillError> {
        let skill_dir = self.skills_dir.join(skill_id);
        let full_path = skill_dir.join(ref_path);
        
        // 安全检查：防止路径遍历
        if !full_path.starts_with(&skill_dir) {
            return Err(SkillError::PathTraversal(ref_path.to_string()));
        }
        
        fs::read_to_string(&full_path)
            .map_err(|e| SkillError::IoError(e))
    }
}
```

#### 5.2.5 _shared 资源处理

_shared 目录的特殊处理：

```rust
impl SkillManager {
    /// 解析 SKILL.md 中的 _shared 引用
    pub fn resolve_shared_references(&self, content: &str, skill_dir: &Path) -> String {
        let shared_dir = self.skills_dir.join("_shared");
        
        // 替换相对路径引用
        let re = Regex::new(r"\.\./_shared/(.+?)\.md").unwrap();
        re.replace_all(content, |caps: &Captures| {
            let ref_path = shared_dir.join(format!("{}.md", &caps[1]));
            fs::read_to_string(&ref_path).unwrap_or_else(|_| caps[0].to_string())
        }).to_string()
    }
}
```

### 5.3 Phase 2: 技能触发与系统提示注入

#### 5.3.1 目标

- 实现 when_to_use 自动匹配
- 实现技能列表注入系统提示
- 实现参数替换机制

#### 5.3.2 when_to_use 匹配引擎

新增 `skill_trigger.rs`：

```rust
pub struct SkillTrigger {
    /// 技能 ID → when_to_use 文本
    trigger_map: HashMap<String, String>,
    /// 中文关键词索引
    keyword_index: HashMap<String, Vec<String>>,
}

impl SkillTrigger {
    /// 从技能列表构建触发器
    pub fn from_skills(skills: &[Skill]) -> Self {
        let mut trigger_map = HashMap::new();
        let mut keyword_index: HashMap<String, Vec<String>> = HashMap::new();
        
        for skill in skills {
            if let Some(ref when_to_use) = skill.metadata.when_to_use {
                trigger_map.insert(skill.id.clone(), when_to_use.clone());
                
                // 提取关键词建立索引
                let keywords = extract_keywords(when_to_use);
                for keyword in keywords {
                    keyword_index
                        .entry(keyword)
                        .or_insert_with(Vec::new)
                        .push(skill.id.clone());
                }
            }
        }
        
        Self { trigger_map, keyword_index }
    }
    
    /// 匹配用户输入，返回候选技能列表
    pub fn match_candidates(&self, user_input: &str) -> Vec<SkillMatch> {
        let input_lower = user_input.to_lowercase();
        let input_keywords = extract_keywords(&input_lower);
        
        let mut scores: HashMap<String, f64> = HashMap::new();
        
        // 关键词匹配
        for keyword in &input_keywords {
            if let Some(skill_ids) = self.keyword_index.get(keyword) {
                for skill_id in skill_ids {
                    *scores.entry(skill_id.clone()).or_insert(0.0) += 1.0;
                }
            }
        }
        
        // 语义相似度（基于 when_to_use 文本）
        for (skill_id, trigger_text) in &self.trigger_map {
            let similarity = compute_similarity(&input_lower, &trigger_text.to_lowercase());
            *scores.entry(skill_id.clone()).or_insert(0.0) += similarity * 2.0;
        }
        
        // 排序并返回
        let mut matches: Vec<SkillMatch> = scores.into_iter()
            .map(|(id, score)| SkillMatch { skill_id: id, score })
            .collect();
        matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        matches
    }
}

/// 提取中英文关键词
fn extract_keywords(text: &str) -> Vec<String> {
    let mut keywords = Vec::new();
    
    // 英文单词
    let en_re = Regex::new(r"[a-zA-Z]+").unwrap();
    for mat in en_re.find_iter(text) {
        keywords.push(mat.as_str().to_lowercase());
    }
    
    // 中文字符（单字 + 常见词组）
    let chars: Vec<char> = text.chars().filter(|c| c.is_ascii() == false).collect();
    for ch in &chars {
        keywords.push(ch.to_string());
    }
    
    // 中文 2-gram
    for window in chars.windows(2) {
        keywords.push(window.iter().collect());
    }
    
    keywords
}
```

#### 5.3.3 系统提示组装

新增 `prompt_assembler.rs`：

```rust
pub struct PromptAssembler {
    /// 技能列表的 token 预算（上下文窗口的 1%）
    token_budget: usize,
}

impl PromptAssembler {
    pub fn new(context_window_size: usize) -> Self {
        Self {
            token_budget: context_window_size / 100,
        }
    }
    
    /// 生成技能列表注入系统提示的文本
    pub fn build_skill_list_prompt(&self, skills: &[Skill]) -> String {
        let mut prompt = String::from("# Available Skills\n\n");
        let mut used_tokens = 0;
        
        // 按使用频率排序
        let mut sorted_skills: Vec<&Skill> = skills.iter().collect();
        sorted_skills.sort_by(|a, b| {
            b.metadata.usage_score.partial_cmp(&a.metadata.usage_score).unwrap()
        });
        
        for skill in sorted_skills {
            let skill_text = self.format_skill_entry(skill);
            let tokens = estimate_tokens(&skill_text);
            
            if used_tokens + tokens > self.token_budget {
                // 尝试压缩
                let compressed = self.format_skill_entry_compressed(skill);
                let compressed_tokens = estimate_tokens(&compressed);
                
                if used_tokens + compressed_tokens > self.token_budget {
                    break; // 超出预算，跳过
                }
                
                prompt.push_str(&compressed);
                used_tokens += compressed_tokens;
            } else {
                prompt.push_str(&skill_text);
                used_tokens += tokens;
            }
        }
        
        prompt
    }
    
    fn format_skill_entry(&self, skill: &Skill) -> String {
        let mut entry = format!("## {}\n", skill.name);
        entry.push_str(&format!("{}\n", skill.description));
        
        if let Some(ref when_to_use) = skill.metadata.when_to_use {
            entry.push_str(&format!("When to use: {}\n", when_to_use));
        }
        
        entry.push_str("\n");
        entry
    }
    
    fn format_skill_entry_compressed(&self, skill: &Skill) -> String {
        format!("- **{}**: {}\n", skill.name, skill.description)
    }
}

/// 粗略估算 token 数量
fn estimate_tokens(text: &str) -> usize {
    // 中文约 1.5 字符/token，英文约 4 字符/token
    let chinese_chars = text.chars().filter(|c| !c.is_ascii()).count();
    let ascii_chars = text.len() - chinese_chars;
    (chinese_chars as f64 / 1.5 + ascii_chars as f64 / 4.0) as usize
}
```

#### 5.3.4 参数替换引擎

扩展 `skill_manager.rs`：

```rust
/// 参数替换上下文
pub struct SubstitutionContext {
    pub arguments: Vec<String>,
    pub skill_dir: PathBuf,
    pub session_id: String,
    pub custom_vars: HashMap<String, String>,
}

impl SubstitutionContext {
    /// 替换 SKILL.md 中的变量
    pub fn substitute(&self, content: &str) -> String {
        let mut result = content.to_string();
        
        // $ARGUMENTS → 所有参数
        result = result.replace("$ARGUMENTS", &self.arguments.join(" "));
        
        // $0, $1, ... → 按位置的参数
        for (i, arg) in self.arguments.iter().enumerate() {
            result = result.replace(&format!("${}", i), arg);
        }
        
        // ${CLAUDE_SKILL_DIR} → 技能目录绝对路径
        result = result.replace(
            "${CLAUDE_SKILL_DIR}",
            &self.skill_dir.to_string_lossy(),
        );
        
        // ${CLAUDE_SESSION_ID} → 会话 ID
        result = result.replace("${CLAUDE_SESSION_ID}", &self.session_id);
        
        // 自定义变量
        for (key, value) in &self.custom_vars {
            result = result.replace(&format!("${{{}}}", key), value);
        }
        
        result
    }
}
```

### 5.4 Phase 3: 高级特性

#### 5.4.1 脚本执行引擎

新增 `skill_executor.rs`：

```rust
pub struct SkillExecutor {
    /// 允许执行脚本的技能白名单
    allowed_skills: HashSet<String>,
    /// 脚本执行超时时间（秒）
    timeout: u64,
    /// 工作目录
    working_dir: PathBuf,
}

impl SkillExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self {
            allowed_skills: config.allowed_skills,
            timeout: config.timeout.unwrap_or(30),
            working_dir: config.working_dir,
        }
    }
    
    /// 执行内联 shell 命令
    pub async fn execute_inline_command(
        &self,
        command: &str,
        context: &SubstitutionContext,
    ) -> Result<String, ExecutorError> {
        // 安全检查
        self.validate_command(command)?;
        
        // 参数替换
        let resolved = context.substitute(command);
        
        // 执行命令
        let output = Command::new("sh")
            .arg("-c")
            .arg(&resolved)
            .current_dir(&self.working_dir)
            .timeout(Duration::from_secs(self.timeout))
            .output()
            .await
            .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;
        
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(ExecutorError::ExecutionFailed(
                String::from_utf8_lossy(&output.stderr).to_string()
            ))
        }
    }
    
    /// 执行块命令
    pub async fn execute_block_command(
        &self,
        lang: &str,
        script: &str,
        context: &SubstitutionContext,
    ) -> Result<String, ExecutorError> {
        let resolved = context.substitute(script);
        
        match lang {
            "python" => {
                let script_path = self.write_temp_script(&resolved, "py")?;
                self.execute_script("python", &script_path).await
            }
            "bash" | "sh" => {
                let script_path = self.write_temp_script(&resolved, "sh")?;
                self.execute_script("bash", &script_path).await
            }
            _ => Err(ExecutorError::UnsupportedLanguage(lang.to_string())),
        }
    }
    
    /// 命令安全验证
    fn validate_command(&self, command: &str) -> Result<(), ExecutorError> {
        // 禁止的命令模式
        let forbidden = [
            "rm -rf /",
            "rm -rf /*",
            ":(){:|:&};:",  // fork bomb
            "dd if=/dev/zero",
            "mkfs",
        ];
        
        for pattern in &forbidden {
            if command.contains(pattern) {
                return Err(ExecutorError::ForbiddenCommand(pattern.to_string()));
            }
        }
        
        Ok(())
    }
}
```

#### 5.4.2 模板下载系统

新增 `template_manager.rs`：

```rust
pub struct TemplateManager {
    /// Gitee API 基础 URL
    base_url: String,
    /// 访问令牌
    access_token: String,
    /// 本地缓存目录
    cache_dir: PathBuf,
    /// 模板清单
    manifest: Option<TemplateManifest>,
}

#[derive(Deserialize)]
pub struct TemplateManifest {
    pub version: String,
    pub phases: Vec<PhaseTemplates>,
}

#[derive(Deserialize)]
pub struct PhaseTemplates {
    pub phase: String,
    pub templates: Vec<Template>,
}

#[derive(Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
    pub url: String,
    pub size: u64,
    pub checksum: String,
}

impl TemplateManager {
    /// 从 Gitee 下载模板
    pub async fn download_template(&self, template_id: &str) -> Result<PathBuf, TemplateError> {
        let template = self.find_template(template_id)?;
        let cache_path = self.cache_dir.join(&template.name);
        
        // 检查缓存
        if cache_path.exists() {
            let checksum = self.compute_checksum(&cache_path)?;
            if checksum == template.checksum {
                return Ok(cache_path);
            }
        }
        
        // 下载
        let client = reqwest::Client::new();
        let response = client
            .get(&template.url)
            .header("Authorization", format!("token {}", self.access_token))
            .send()
            .await
            .map_err(|e| TemplateError::DownloadFailed(e.to_string()))?;
        
        let bytes = response.bytes().await
            .map_err(|e| TemplateError::DownloadFailed(e.to_string()))?;
        
        // 校验
        let checksum = compute_sha256(&bytes);
        if checksum != template.checksum {
            return Err(TemplateError::ChecksumMismatch);
        }
        
        // 写入缓存
        fs::write(&cache_path, &bytes)?;
        
        Ok(cache_path)
    }
    
    /// 批量下载阶段模板
    pub async fn download_phase_templates(&self, phase: &str) -> Result<Vec<PathBuf>, TemplateError> {
        let phase_templates = self.manifest.as_ref()
            .and_then(|m| m.phases.iter().find(|p| p.phase == phase))
            .ok_or(TemplateError::PhaseNotFound(phase.to_string()))?;
        
        let mut paths = Vec::new();
        for template in &phase_templates.templates {
            let path = self.download_template(&template.id).await?;
            paths.push(path);
        }
        
        Ok(paths)
    }
}
```

#### 5.4.3 事件驱动架构

新增 `signal_writer.rs`：

```rust
/// 事件类型
#[derive(Serialize)]
pub enum SignalEvent {
    SkillLoaded {
        skill_id: String,
        load_time_ms: u64,
        timestamp: u64,
    },
    SkillTriggered {
        skill_id: String,
        trigger_type: TriggerType,
        user_input: String,
        timestamp: u64,
    },
    SkillExecuted {
        skill_id: String,
        success: bool,
        duration_ms: u64,
        timestamp: u64,
    },
    TemplateDownloaded {
        template_id: String,
        size_bytes: u64,
        timestamp: u64,
    },
    ErrorOccurred {
        error_type: String,
        message: String,
        skill_id: Option<String>,
        timestamp: u64,
    },
}

#[derive(Serialize)]
pub enum TriggerType {
    UserCommand,      // 用户斜杠命令
    AutoMatch,        // 模型自动匹配
    ConditionalPath,  // 条件路径激活
}

/// 信号写入器，输出 JSONL 格式
pub struct SignalWriter {
    file: File,
    buffer: Vec<SignalEvent>,
    flush_threshold: usize,
}

impl SignalWriter {
    pub fn new(signals_path: PathBuf) -> Result<Self, SignalError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(signals_path)?;
        
        Ok(Self {
            file,
            buffer: Vec::new(),
            flush_threshold: 10,
        })
    }
    
    /// 写入事件
    pub fn write(&mut self, event: SignalEvent) -> Result<(), SignalError> {
        self.buffer.push(event);
        
        if self.buffer.len() >= self.flush_threshold {
            self.flush()?;
        }
        
        Ok(())
    }
    
    /// 刷新缓冲区到文件
    fn flush(&mut self) -> Result<(), SignalError> {
        for event in &self.buffer {
            let json = serde_json::to_string(event)?;
            writeln!(self.file, "{}", json)?;
        }
        self.buffer.clear();
        self.file.flush()?;
        Ok(())
    }
}
```

#### 5.4.4 文件变更检测

扩展 `skill_manager.rs`：

```rust
use notify::{Watcher, RecursiveMode, watcher};
use std::sync::mpsc::channel;

pub struct SkillFileWatcher {
    watcher: notify::RecommendedWatcher,
    rx: Receiver<notify::Event>,
}

impl SkillFileWatcher {
    pub fn new(skills_dir: &Path) -> Result<Self, SkillError> {
        let (tx, rx) = channel();
        
        let mut watcher = notify::recommended_watcher(move |res| {
            if let Ok(event) = res {
                tx.send(event).ok();
            }
        })?;
        
        watcher.watch(skills_dir, RecursiveMode::Recursive)?;
        
        Ok(Self { watcher, rx })
    }
    
    /// 检查是否有文件变更
    pub fn poll_changes(&self) -> Vec<FileChange> {
        let mut changes = Vec::new();
        
        while let Ok(event) = self.rx.try_recv() {
            for path in event.paths {
                if path.ends_with("SKILL.md") || 
                   path.extension().map(|e| e == "md").unwrap_or(false) {
                    changes.push(FileChange {
                        path,
                        kind: match event.kind {
                            notify::EventKind::Create(_) => ChangeKind::Created,
                            notify::EventKind::Modify(_) => ChangeKind::Modified,
                            notify::EventKind::Remove(_) => ChangeKind::Deleted,
                            _ => continue,
                        },
                    });
                }
            }
        }
        
        changes
    }
}

pub struct FileChange {
    pub path: PathBuf,
    pub kind: ChangeKind,
}

pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}
```

---

## 6. 模块设计

### 6.1 模块依赖关系

```
                    ┌─────────────────┐
                    │  skill_manager  │
                    │   (核心协调)     │
                    └──────┬──────────┘
                           │
          ┌────────────────┼────────────────┐
          │                │                │
    ┌─────┴─────┐   ┌─────┴─────┐   ┌─────┴─────┐
    │skill_loader│   │skill_trigger│  │prompt_asm │
    │ (文件加载) │   │ (触发匹配) │   │ (提示组装)│
    └─────┬─────┘   └─────┬─────┘   └───────────┘
          │                │
    ┌─────┴─────┐   ┌─────┴─────┐
    │skill_types│   │skill_exec │
    │ (数据类型) │   │ (脚本执行)│
    └───────────┘   └─────┬─────┘
                          │
                    ┌─────┴─────┐
                    │signal_writer│
                    │ (事件记录) │
                    └───────────┘
```

### 6.2 skill_types.rs 扩展

新增类型定义：

```rust
/// 技能文件信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFile {
    pub path: String,
    pub name: String,
    pub file_type: SkillFileType,
    pub size: u64,
    pub last_modified: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkillFileType {
    Reference,
    Script,
    Asset,
    Shared,
    Config,
}

/// 技能完整信息（含支撑文件）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFull {
    pub skill: Skill,
    pub supporting_files: Vec<SkillFile>,
    pub shared_references: Vec<SharedResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedResource {
    pub name: String,
    pub path: String,
    pub content: String,
}

/// 技能匹配结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMatch {
    pub skill_id: String,
    pub score: f64,
    pub match_type: MatchType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchType {
    KeywordMatch,
    SemanticMatch,
    PathMatch,
    UserCommand,
}

/// 技能触发上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerContext {
    pub user_input: String,
    pub accessed_files: Vec<String>,
    pub current_phase: Option<String>,
    pub session_id: String,
}

/// 执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub output: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// 技能统计信息（扩展）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStats {
    pub total_skills: usize,
    pub by_category: HashMap<String, usize>,
    pub by_phase: HashMap<String, usize>,
    pub with_scripts: usize,
    pub with_references: usize,
    pub with_assets: usize,
    pub shared_resources: usize,
    pub total_supporting_files: usize,
}
```

### 6.3 skill_loader.rs 设计

新模块，负责文件加载和内容处理：

```rust
pub struct SkillLoader {
    skills_dir: PathBuf,
    shared_dir: PathBuf,
    cache: RwLock<HashMap<String, CachedSkill>>,
}

struct CachedSkill {
    skill_full: SkillFull,
    loaded_at: Instant,
    file_hash: u64,
}

impl SkillLoader {
    pub fn new(skills_dir: PathBuf) -> Self {
        Self {
            shared_dir: skills_dir.join("_shared"),
            skills_dir,
            cache: RwLock::new(HashMap::new()),
        }
    }
    
    /// 加载单个技能（含支撑文件）
    pub fn load_skill(&self, skill_dir_name: &str) -> Result<SkillFull, SkillError> {
        let skill_dir = self.skills_dir.join(skill_dir_name);
        let skill_md = skill_dir.join("SKILL.md");
        
        // 读取并解析 SKILL.md
        let content = fs::read_to_string(&skill_md)?;
        let (frontmatter, body) = parse_frontmatter(&content)?;
        
        // 构建 Skill 对象
        let skill = build_skill_from_frontmatter(
            skill_dir_name,
            &frontmatter,
            &body,
            &skill_md,
        )?;
        
        // 扫描支撑文件
        let supporting_files = self.scan_supporting_files(&skill_dir)?;
        
        // 加载共享资源
        let shared_references = self.load_shared_references(&body)?;
        
        Ok(SkillFull {
            skill,
            supporting_files,
            shared_references,
        })
    }
    
    /// 批量加载所有技能
    pub fn load_all_skills(&self) -> Result<Vec<SkillFull>, SkillError> {
        let mut skills = Vec::new();
        
        for entry in fs::read_dir(&self.skills_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if !path.is_dir() || path.file_name().unwrap().to_string_lossy().starts_with('_') {
                continue;
            }
            
            let skill_md = path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }
            
            match self.load_skill(&path.file_name().unwrap().to_string_lossy()) {
                Ok(skill) => skills.push(skill),
                Err(e) => {
                    eprintln!("Failed to load skill {:?}: {}", path, e);
                    continue;
                }
            }
        }
        
        Ok(skills)
    }
    
    /// 带缓存的加载
    pub fn load_skill_cached(&self, skill_id: &str) -> Result<SkillFull, SkillError> {
        // 检查缓存
        {
            let cache = self.cache.read().unwrap();
            if let Some(cached) = cache.get(skill_id) {
                if cached.loaded_at.elapsed() < Duration::from_secs(60) {
                    return Ok(cached.skill_full.clone());
                }
            }
        }
        
        // 重新加载
        let skill = self.load_skill(skill_id)?;
        
        // 更新缓存
        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(skill_id.to_string(), CachedSkill {
                skill_full: skill.clone(),
                loaded_at: Instant::now(),
                file_hash: compute_hash(&skill),
            });
        }
        
        Ok(skill)
    }
}
```

### 6.4 skill_trigger.rs 设计

```rust
pub struct SkillTriggerEngine {
    trigger_index: TriggerIndex,
    matcher: SkillMatcher,
}

struct TriggerIndex {
    /// when_to_use 文本 → 技能 ID
    when_to_use_map: HashMap<String, String>,
    /// 关键词 → 技能 ID 列表
    keyword_map: HashMap<String, Vec<String>>,
    /// 路径模式 → 技能 ID 列表
    path_map: Vec<(GlobPattern, String)>,
}

struct SkillMatcher {
    /// 中文分词器
    tokenizer: ChineseTokenizer,
    /// 相似度计算器
    similarity: CosineSimilarity,
}

impl SkillTriggerEngine {
    pub fn new(skills: &[SkillFull]) -> Self {
        let trigger_index = TriggerIndex::build(skills);
        let matcher = SkillMatcher::new();
        
        Self { trigger_index, matcher }
    }
    
    /// 根据用户输入匹配技能
    pub fn match_by_input(&self, input: &str) -> Vec<SkillMatch> {
        let mut results: HashMap<String, f64> = HashMap::new();
        
        // 1. 关键词匹配
        let keywords = self.matcher.tokenizer.tokenize(input);
        for keyword in &keywords {
            if let Some(skill_ids) = self.trigger_index.keyword_map.get(keyword) {
                for skill_id in skill_ids {
                    *results.entry(skill_id.clone()).or_insert(0.0) += 1.0;
                }
            }
        }
        
        // 2. when_to_use 语义匹配
        for (trigger_text, skill_id) in &self.trigger_index.when_to_use_map {
            let similarity = self.matcher.similarity.compute(input, trigger_text);
            if similarity > 0.3 {
                *results.entry(skill_id.clone()).or_insert(0.0) += similarity * 3.0;
            }
        }
        
        // 排序并返回
        let mut matches: Vec<SkillMatch> = results.into_iter()
            .map(|(id, score)| SkillMatch {
                skill_id: id,
                score,
                match_type: if score > 2.0 { MatchType::SemanticMatch } else { MatchType::KeywordMatch },
            })
            .collect();
        matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        matches
    }
    
    /// 根据文件路径匹配技能（条件激活）
    pub fn match_by_paths(&self, accessed_files: &[String]) -> Vec<SkillMatch> {
        let mut matches = Vec::new();
        
        for file_path in accessed_files {
            for (pattern, skill_id) in &self.trigger_index.path_map {
                if pattern.matches(file_path) {
                    matches.push(SkillMatch {
                        skill_id: skill_id.clone(),
                        score: 1.0,
                        match_type: MatchType::PathMatch,
                    });
                }
            }
        }
        
        matches
    }
}
```

### 6.5 skill_manager.rs 重构

现有的 `SkillManager` 需要重构为协调器角色：

```rust
pub struct SkillManager {
    loader: SkillLoader,
    trigger: RwLock<Option<SkillTriggerEngine>>,
    prompt_assembler: PromptAssembler,
    signal_writer: SignalWriter,
    config: SkillConfig,
}

impl SkillManager {
    pub fn new(config: SkillConfig) -> Result<Self, SkillError> {
        let loader = SkillLoader::new(config.skills_dir.clone());
        let prompt_assembler = PromptAssembler::new(config.context_window_size);
        let signal_writer = SkillWriter::new(config.signals_path.clone())?;
        
        Ok(Self {
            loader,
            trigger: RwLock::new(None),
            prompt_assembler,
            signal_writer,
            config,
        })
    }
    
    /// 初始化：加载所有技能并构建索引
    pub fn initialize(&mut self) -> Result<(), SkillError> {
        let skills = self.loader.load_all_skills()?;
        
        let trigger = SkillTriggerEngine::new(&skills);
        *self.trigger.write().unwrap() = Some(trigger);
        
        self.signal_writer.write(SignalEvent::SkillLoaded {
            skill_id: "system".to_string(),
            load_time_ms: 0,
            timestamp: now(),
        })?;
        
        Ok(())
    }
    
    /// 获取技能列表
    pub fn list_skills(&self) -> Result<Vec<Skill>, SkillError> {
        let skills = self.loader.load_all_skills()?;
        Ok(skills.into_iter().map(|s| s.skill).collect())
    }
    
    /// 获取单个技能（含支撑文件）
    pub fn get_skill_full(&self, skill_id: &str) -> Result<SkillFull, SkillError> {
        self.loader.load_skill_cached(skill_id)
    }
    
    /// 搜索技能
    pub fn search_skills(&self, query: &str) -> Result<Vec<Skill>, SkillError> {
        let skills = self.loader.load_all_skills()?;
        let query_lower = query.to_lowercase();
        
        let mut results: Vec<(Skill, f64)> = skills.into_iter()
            .filter_map(|s| {
                let score = self.compute_search_score(&s.skill, &query_lower);
                if score > 0.0 { Some((s.skill, score)) } else { None }
            })
            .collect();
        
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(results.into_iter().map(|(s, _)| s).collect())
    }
    
    /// 匹配最佳技能
    pub fn match_best_skill(&self, context: &TriggerContext) -> Result<Option<SkillMatch>, SkillError> {
        let trigger = self.trigger.read().unwrap();
        let trigger = trigger.as_ref().ok_or(SkillError::NotInitialized)?;
        
        let mut all_matches = trigger.match_by_input(&context.user_input);
        
        // 合并路径匹配
        let path_matches = trigger.match_by_paths(&context.accessed_files);
        all_matches.extend(path_matches);
        
        // 按分数排序
        all_matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        
        Ok(all_matches.into_iter().next())
    }
    
    /// 生成系统提示中的技能列表
    pub fn build_skill_list_prompt(&self) -> Result<String, SkillError> {
        let skills = self.loader.load_all_skills()?;
        let skill_refs: Vec<&Skill> = skills.iter().map(|s| &s.skill).collect();
        Ok(self.prompt_assembler.build_skill_list_prompt(&skill_refs))
    }
}
```

### 6.6 新增依赖

`Cargo.toml` 新增依赖：

```toml
[dependencies]
# 已有依赖保持不变
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
tauri = { version = "2", features = ["shell-open"] }

# 新增依赖
notify = "6"                    # 文件系统监控
walkdir = "2"                   # 递归目录遍历
glob = "0.3"                    # glob 模式匹配
regex = "1"                     # 正则表达式
sha2 = "0.10"                   # SHA256 校验
reqwest = { version = "0.11", features = ["json"] }  # HTTP 客户端
unicode-segmentation = "1.10"   # Unicode 文本处理（中文分词）
```

---

## 7. 接口定义

### 7.1 Tauri 命令扩展

新增和修改的 Tauri 命令：

```rust
// ==================== 新增命令 ====================

/// 获取技能完整信息（含支撑文件）
#[tauri::command]
pub async fn get_skill_full(
    state: State<'_, AppState>,
    id: String,
) -> Result<SkillFull, String>

/// 获取技能的引用文件列表
#[tauri::command]
pub async fn list_skill_references(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<SkillFile>, String>

/// 读取技能的引用文件内容
#[tauri::command]
pub async fn read_skill_reference(
    state: State<'_, AppState>,
    skill_id: String,
    file_path: String,
) -> Result<String, String>

/// 获取共享资源列表
#[tauri::command]
pub async fn list_shared_resources(
    state: State<'_, AppState>,
) -> Result<Vec<SharedResource>, String>

/// 读取共享资源内容
#[tauri::command]
pub async fn read_shared_resource(
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String>

/// 触发技能匹配
#[tauri::command]
pub async fn trigger_skill_match(
    state: State<'_, AppState>,
    context: TriggerContext,
) -> Result<Vec<SkillMatch>, String>

/// 生成技能列表提示文本
#[tauri::command]
pub async fn get_skill_list_prompt(
    state: State<'_, AppState>,
) -> Result<String, String>

/// 执行技能脚本
#[tauri::command]
pub async fn execute_skill_script(
    state: State<'_, AppState>,
    skill_id: String,
    script_path: String,
    arguments: Vec<String>,
) -> Result<ExecutionResult, String>

/// 获取技能统计信息（扩展版）
#[tauri::command]
pub async fn get_skill_stats_extended(
    state: State<'_, AppState>,
) -> Result<SkillStats, String>

/// 下载模板
#[tauri::command]
pub async fn download_template(
    state: State<'_, AppState>,
    template_id: String,
) -> Result<String, String>

/// 获取模板清单
#[tauri::command]
pub async fn get_template_manifest(
    state: State<'_, AppState>,
) -> Result<TemplateManifest, String>

// ==================== 修改的命令 ====================

/// 列出所有技能（返回简要信息，用于列表展示）
#[tauri::command]
pub async fn list_skills(
    state: State<'_, AppState>,
) -> Result<Vec<SkillSummary>, String>  // 改为返回 SkillSummary
```

### 7.2 TypeScript 类型扩展

`src/lib/skill-types.ts` 新增类型：

```typescript
// ==================== 支撑文件相关 ====================

export interface SkillFile {
  path: string;
  name: string;
  fileType: SkillFileType;
  size: number;
  lastModified: number;
}

export type SkillFileType = 'reference' | 'script' | 'asset' | 'shared' | 'config';

export interface SkillFull {
  skill: Skill;
  supportingFiles: SkillFile[];
  sharedReferences: SharedResource[];
}

export interface SharedResource {
  name: string;
  path: string;
  content: string;
}

// ==================== 触发匹配相关 ====================

export interface TriggerContext {
  userInput: string;
  accessedFiles: string[];
  currentPhase?: string;
  sessionId: string;
}

export interface SkillMatch {
  skillId: string;
  score: number;
  matchType: MatchType;
}

export type MatchType = 'keyword' | 'semantic' | 'path' | 'userCommand';

// ==================== 执行相关 ====================

export interface ExecutionResult {
  success: boolean;
  output: string;
  durationMs: number;
  error?: string;
}

// ==================== 模板相关 ====================

export interface TemplateManifest {
  version: string;
  phases: PhaseTemplates[];
}

export interface PhaseTemplates {
  phase: string;
  templates: Template[];
}

export interface Template {
  id: string;
  name: string;
  description: string;
  url: string;
  size: number;
  checksum: string;
}

// ==================== 扩展统计 ====================

export interface SkillStats {
  totalSkills: number;
  byCategory: Record<string, number>;
  byPhase: Record<string, number>;
  withScripts: number;
  withReferences: number;
  withAssets: number;
  sharedResources: number;
  totalSupportingFiles: number;
}

// ==================== 技能摘要（用于列表） ====================

export interface SkillSummary {
  id: string;
  name: string;
  description: string;
  category: string;
  phase?: string;
  tags: string[];
  hasScripts: boolean;
  hasReferences: boolean;
  hasAssets: boolean;
  supportingFileCount: number;
  lastModified: number;
}
```

### 7.3 TypeScript 命令扩展

`src/lib/skill-commands.ts` 新增封装：

```typescript
import { invoke } from '@tauri-apps/api/core';

// ==================== 新增命令 ====================

export async function getSkillFull(id: string): Promise<SkillFull> {
  return invoke('get_skill_full', { id });
}

export async function listSkillReferences(id: string): Promise<SkillFile[]> {
  return invoke('list_skill_references', { id });
}

export async function readSkillReference(
  skillId: string,
  filePath: string
): Promise<string> {
  return invoke('read_skill_reference', { skillId, filePath });
}

export async function listSharedResources(): Promise<SharedResource[]> {
  return invoke('list_shared_resources');
}

export async function readSharedResource(path: string): Promise<string> {
  return invoke('read_shared_resource', { path });
}

export async function triggerSkillMatch(
  context: TriggerContext
): Promise<SkillMatch[]> {
  return invoke('trigger_skill_match', { context });
}

export async function getSkillListPrompt(): Promise<string> {
  return invoke('get_skill_list_prompt');
}

export async function executeSkillScript(
  skillId: string,
  scriptPath: string,
  arguments_: string[]
): Promise<ExecutionResult> {
  return invoke('execute_skill_script', {
    skillId,
    scriptPath,
    arguments: arguments_,
  });
}

export async function getSkillStatsExtended(): Promise<SkillStats> {
  return invoke('get_skill_stats_extended');
}

export async function downloadTemplate(templateId: string): Promise<string> {
  return invoke('download_template', { templateId });
}

export async function getTemplateManifest(): Promise<TemplateManifest> {
  return invoke('get_template_manifest');
}
```

### 7.4 数据流图

#### 7.4.1 技能加载流程

```
用户打开 Skills 页面
        │
        ▼
  invoke('list_skills')
        │
        ▼
  SkillManager.list_skills()
        │
        ▼
  SkillLoader.load_all_skills()
        │
        ├── 遍历 skills/ 目录
        │   ├── 读取 SKILL.md
        │   ├── 解析 YAML frontmatter
        │   ├── 提取 Markdown body
        │   └── 构建 Skill 对象
        │
        ├── 扫描支撑文件
        │   ├── references/*.md
        │   ├── scripts/*.py
        │   └── assets/*.*
        │
        └── 加载 _shared 资源
            ├── signals-writer.md
            ├── template-check.md
            └── scripts/*.py
        
        │
        ▼
  返回 Vec<SkillSummary> 到前端
```

#### 7.4.2 技能触发流程

```
用户输入: "帮我制定阶段3的计划"
        │
        ▼
  invoke('trigger_skill_match', context)
        │
        ▼
  SkillTriggerEngine.match_by_input()
        │
        ├── 关键词匹配
        │   ├── 分词: ["帮", "我", "制定", "阶段", "3", "计划"]
        │   ├── 查找: "计划" → ["kd-plan-phase"]
        │   └── 查找: "阶段" → ["kd-plan-phase", "kd-execute-phase"]
        │
        ├── when_to_use 语义匹配
        │   ├── 计算相似度(user_input, kd-plan-phase.when_to_use)
        │   └── 相似度 > 0.3 → 加入候选
        │
        └── 排序输出
            └── SkillMatch { skill_id: "kd-plan-phase", score: 4.2 }
        
        │
        ▼
  返回匹配结果到前端
```

#### 7.4.3 系统提示注入流程

```
LLM 对话开始
        │
        ▼
  getSkillListPrompt()
        │
        ▼
  PromptAssembler.build_skill_list_prompt()
        │
        ├── 获取所有技能
        ├── 按使用频率排序
        ├── 估算 token 预算 (上下文窗口 × 1%)
        └── 逐个格式化技能条目
            ├── 完整格式: name + description + when_to_use
            └── 压缩格式: name + description (超出预算时)
        
        │
        ▼
  注入系统提示:
  "# Available Skills
   ## kd-plan-phase
   制定阶段执行计划，分解任务到可执行粒度。
   When to use: 使用当需要为开发阶段创建详细执行计划时..."
```

#### 7.4.4 脚本执行流程

```
用户: "/kd-uat 阶段3"
        │
        ▼
  invoke('execute_skill_script', skill_id, script_path, args)
        │
        ▼
  SkillExecutor.execute_block_command()
        │
        ├── 安全检查 (validate_command)
        ├── 参数替换 (context.substitute)
        │   ├── $ARGUMENTS → "阶段3"
        │   └── ${CLAUDE_SKILL_DIR} → "/path/to/skills/kd-uat"
        │
        ├── 写入临时脚本文件
        ├── 执行: python /tmp/script.py 阶段3
        └── 收集输出
        
        │
        ▼
  ExecutionResult { success: true, output: "..." }
        │
        ▼
  SignalWriter.write(SkillExecuted { ... })
```

---

## 8. 实现计划

### 8.1 Phase 1: 修复数据与加载支撑文件

**优先级**: P0（阻塞后续所有工作）

**预估工时**: 3-5 天

| 任务 | 工时 | 依赖 | 产出 |
|------|------|------|------|
| 1.1 编写迁移脚本 | 0.5 天 | 无 | scripts/migrate-skills.py |
| 1.2 执行数据迁移 | 0.5 天 | 1.1 | skills/ 目录完整数据 |
| 1.3 修复 SKILL.md frontmatter | 1 天 | 1.2 | 27 个独立的 SKILL.md |
| 1.4 扩展 skill_types.rs | 0.5 天 | 无 | SkillFile, SkillFull 等类型 |
| 1.5 实现 skill_loader.rs | 1 天 | 1.4 | 文件加载、缓存、_shared 支持 |
| 1.6 修改 skill_manager.rs | 0.5 天 | 1.5 | 集成新 loader |
| 1.7 新增 Tauri 命令 | 0.5 天 | 1.6 | get_skill_full 等命令 |
| 1.8 前端支撑文件展示 | 0.5 天 | 1.7 | Skills 页面扩展 |
| 1.9 测试与验证 | 0.5 天 | 1.8 | 功能验证 |

**验收标准**:
- 27 个技能全部可加载，无损坏
- 支撑文件（references, scripts, assets）正确展示
- _shared 资源可被技能引用
- 现有 Tauri 命令接口保持兼容

### 8.2 Phase 2: 技能触发与系统提示注入

**优先级**: P1

**预估工时**: 5-7 天

| 任务 | 工时 | 依赖 | 产出 |
|------|------|------|------|
| 2.1 实现中文分词器 | 1 天 | 无 | ChineseTokenizer |
| 2.2 实现相似度计算 | 0.5 天 | 无 | CosineSimilarity |
| 2.3 实现 skill_trigger.rs | 1.5 天 | 2.1, 2.2 | 触发引擎 |
| 2.4 实现 prompt_assembler.rs | 1 天 | 无 | 提示组装器 |
| 2.5 实现参数替换 | 0.5 天 | 无 | SubstitutionContext |
| 2.6 集成到 SkillManager | 0.5 天 | 2.3, 2.4, 2.5 | 协调器更新 |
| 2.7 新增 Tauri 命令 | 0.5 天 | 2.6 | trigger_skill_match 等 |
| 2.8 前端触发 UI | 0.5 天 | 2.7 | 技能推荐面板 |
| 2.9 测试与验证 | 0.5 天 | 2.8 | 功能验证 |

**验收标准**:
- 用户输入可匹配到正确技能
- 系统提示中包含技能列表
- 参数替换正确工作
- 匹配延迟 < 100ms

### 8.3 Phase 3: 高级特性

**优先级**: P2

**预估工时**: 8-12 天

| 任务 | 工时 | 依赖 | 产出 |
|------|------|------|------|
| 3.1 实现脚本执行引擎 | 2 天 | Phase 2 | skill_executor.rs |
| 3.2 实现安全沙箱 | 1 天 | 3.1 | 命令验证、超时控制 |
| 3.3 实现模板管理器 | 2 天 | 无 | template_manager.rs |
| 3.4 实现 Gitee API 集成 | 1 天 | 3.3 | 模板下载 |
| 3.5 实现事件写入器 | 1 天 | 无 | signal_writer.rs |
| 3.6 实现文件变更检测 | 1 天 | 无 | SkillFileWatcher |
| 3.7 条件激活逻辑 | 0.5 天 | 2.3 | paths 匹配 |
| 3.8 使用追踪与衰减 | 0.5 天 | 3.5 | 使用统计 |
| 3.9 集成测试 | 1 天 | 3.1-3.8 | 端到端测试 |
| 3.10 文档更新 | 0.5 天 | 3.9 | 用户文档 |

**验收标准**:
- Python 脚本可在沙箱中执行
- 模板可从 Gitee 下载并缓存
- 事件流正确记录到 signals.jsonl
- 文件变更可触发技能重新加载
- 条件激活正确工作

### 8.4 里程碑时间线

```
Week 1:  Phase 1 完成
         ├── Day 1-2: 数据迁移 + SKILL.md 修复
         ├── Day 3: Rust 类型扩展 + Loader 实现
         ├── Day 4: Tauri 命令 + 前端扩展
         └── Day 5: 测试验证

Week 2:  Phase 2 完成
         ├── Day 1-2: 分词器 + 相似度 + 触发引擎
         ├── Day 3: 提示组装 + 参数替换
         ├── Day 4: 集成 + Tauri 命令
         └── Day 5: 前端 UI + 测试

Week 3-4: Phase 3 完成
         ├── Day 1-3: 脚本执行 + 沙箱
         ├── Day 4-5: 模板系统
         ├── Day 6-7: 事件系统 + 文件监控
         └── Day 8-10: 集成测试 + 文档
```

### 8.5 技术债务管理

以下事项记录为技术债务，不在本设计范围内解决：

| 编号 | 描述 | 优先级 | 计划解决时间 |
|------|------|--------|------------|
| TD-01 | 技能使用追踪的持久化存储 | 低 | Phase 3 后 |
| TD-02 | 技能版本管理和更新检查 | 低 | v2.0 |
| TD-03 | 技能市场的在线安装 | 中 | v2.0 |
| TD-04 | 技能间的依赖关系解析 | 中 | v2.0 |
| TD-05 | 技能执行结果的缓存 | 低 | Phase 3 后 |

---

## 9. 风险评估

### 9.1 风险矩阵

| 风险 | 影响 | 概率 | 等级 | 缓解措施 |
|------|------|------|------|---------|
| SKILL.md 修复后内容不准确 | 高 | 中 | 高 | 人工逐个审核，对照原版套件验证 |
| 中文分词精度不足 | 中 | 中 | 中 | 使用成熟分词库（jieba-rs），支持自定义词典 |
| 脚本执行安全漏洞 | 高 | 低 | 高 | 严格白名单 + 沙箱 + 命令验证 |
| 性能瓶颈（大量技能加载） | 中 | 低 | 低 | 缓存 + 懒加载 + 异步 |
| Tauri 命令接口破坏性变更 | 高 | 低 | 中 | 新增命令，不修改现有命令签名 |
| _shared 资源路径解析错误 | 中 | 中 | 中 | 路径遍历防护 + 单元测试 |
| 模板下载网络超时 | 低 | 中 | 低 | 重试机制 + 本地缓存 |
| 文件监控资源泄漏 | 中 | 低 | 低 | 正确实现 Drop trait |

### 9.2 关键风险详解

#### 9.2.1 SKILL.md 内容修复风险

**风险描述**：原版套件中 27 个 SKILL.md 文件内容相同（套件级描述），无法直接使用。需要为每个技能生成独立的描述和触发条件。

**影响**：如果描述不准确，技能触发匹配的准确率会大幅下降。

**缓解措施**：
1. 从 CLAUDE.md 的技能注册表中提取基础信息
2. 参考原版套件中各技能目录的其他文件（如有）获取上下文
3. 人工逐个审核和修正
4. 建立测试用例，验证每个技能的触发条件

**回退方案**：如果无法在短期内完成全部修复，先为高频使用的 5-8 个核心技能生成准确描述，其余技能使用通用描述。

#### 9.2.2 脚本执行安全风险

**风险描述**：技能中的 Python 脚本可能包含任意代码，直接执行存在安全风险。

**影响**：可能导致数据泄露、系统损坏等严重后果。

**缓解措施**：
1. 默认禁止所有脚本执行，需要用户显式授权
2. 实现命令白名单，只允许安全的 Python 模块
3. 设置执行超时（默认 30 秒）
4. 限制文件系统访问范围（只允许技能目录和临时目录）
5. 记录所有执行日志到信号文件

**回退方案**：Phase 3 初期只支持引用文件（references）展示，脚本执行功能放在最后实现，并标记为实验性功能。

#### 9.2.3 性能风险

**风险描述**：27 个技能的加载、索引构建、匹配计算可能影响应用启动速度。

**影响**：用户体验下降。

**缓解措施**：
1. 使用缓存，技能加载后缓存 60 秒
2. 索引构建异步化，不阻塞主界面
3. 匹配计算使用倒排索引，避免全量扫描
4. 懒加载支撑文件内容，只在需要时读取

**性能指标**：
- 技能列表加载: < 200ms
- 技能匹配: < 100ms
- 系统提示生成: < 50ms

### 9.3 依赖风险

| 依赖 | 版本 | 风险 | 备选方案 |
|------|------|------|---------|
| notify | 6.x | API 变更频繁 | 使用 polling 作为降级方案 |
| walkdir | 2.x | 稳定 | 无 |
| glob | 0.3.x | 稳定 | 无 |
| reqwest | 0.11.x | 与 tokio 版本耦合 | 使用 ureq（同步） |
| unicode-segmentation | 1.10.x | 稳定 | 无 |
| jieba-rs | 0.6.x | 中文分词质量 | 自定义简单分词 |

---

## 附录

### A. 术语表

| 术语 | 说明 |
|------|------|
| SKILL.md | Claude Code 技能系统的核心文件，包含 YAML frontmatter 和 Markdown body |
| Frontmatter | YAML 格式的元数据，位于 SKILL.md 的 `---` 分隔符之间 |
| when_to_use | 技能的触发条件描述，用于模型自动匹配 |
| _shared | 跨技能共享资源目录 |
| signals.jsonl | 事件流文件，JSONL 格式 |
| 条件激活 | 基于文件路径匹配的技能自动激活机制 |
| 系统提示注入 | 将技能列表注入 LLM 系统提示的机制 |

### B. 参考资料

- Claude Code 技能系统源码: `packages/core/src/skills/`
- 原版套件: `E:\projects\kingdee\skills\kingdee-implementation-suite-v8.0.0\`
- KingdeeKB 项目: `E:\projects\kingdee\KingdeeKB\`
- Tauri v2 文档: https://v2.tauri.app/
- notify crate 文档: https://docs.rs/notify/

### C. 变更记录

| 版本 | 日期 | 作者 | 变更内容 |
|------|------|------|---------|
| v1.0 | 2026-05-30 | 架构组 | 初始版本 |

---

> 本文档由 KingdeeKB 架构组编写，供开发团队参考。如有疑问或建议，请联系项目负责人。
