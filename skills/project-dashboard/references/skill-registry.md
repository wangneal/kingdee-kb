# Skill 注册表（27 个完整元数据）

> 实现位置：`server/skill_registry.py`
> 用途：驱动侧边栏"Skill 调用台"的三组分类展示

---

## 元数据字段

| 字段 | 说明 |
|------|------|
| `name` | Skill 目录名（唯一标识） |
| `label` | 展示名称 |
| `trigger` | 一键复制的触发命令 |
| `phase` | 适用阶段列表；`["all"]` 表示全阶段通用 |
| `category` | 分组：`core` / `stage` / `mgmt` / `tool` |
| `icon` | 展示图标（Unicode emoji） |

---

## 核心 Skill（3 个）

| # | name | label | trigger | phase | icon |
|---|------|-------|---------|-------|------|
| 1 | project-init | 项目初始化 | `/project-init` | `["all"]` | 🚀 |
| 2 | project-sync | 项目同步 | `/project-sync` | `["all"]` | 🔄 |
| 3 | skill-updater | 更新套件 | `/skill-updater` | `["all"]` | ⬆️ |

## 阶段 Skill（9 个）

| # | name | label | trigger | phase | icon |
|---|------|-------|---------|-------|------|
| 4 | kickoff-pack | 启动会材料 | `/kickoff-pack` | `["01_启动"]` | 🎬 |
| 5 | survey-assistant | 需求调研 | `/survey-assistant` | `["02_需求"]` | 🔍 |
| 6 | blueprint-tools | 蓝图设计 | `/blueprint-tools` | `["03_方案"]` | 🏗️ |
| 7 | build-tracker | 构建跟踪 | `/build-tracker` | `["04_构建"]` | 🔧 |
| 8 | test-manager | 测试管理 | `/test-manager` | `["05_测试"]` | 🧪 |
| 9 | golive-pack | 上线准备 | `/golive-pack` | `["06_上线"]` | 🚢 |
| 10 | acceptance-pack | 验收打包 | `/acceptance-pack` | `["07_验收"]` | ✅ |
| 11 | weekly-report | 生成周报 | `/weekly-report` | `["all"]` | 📊 |
| 12 | stakeholder-comms | 会议纪要 | `/stakeholder-comms` | `["all"]` | 🤝 |

## 管理 Skill（3 个）

| # | name | label | trigger | phase | icon |
|---|------|-------|---------|-------|------|
| 13 | change-manager | 提变更 | `/change-manager 新增变更` | `["all"]` | 📝 |
| 14 | risk-manager | 新增风险 | `/risk-manager 新增风险` | `["all"]` | ⚠️ |
| 15 | qa-root-cause-analysis | 质量根因 | `/qa-root-cause-analysis` | `["all"]` | 🔬 |

## 工具 Skill（12 个）

| # | name | label | trigger | phase | icon |
|---|------|-------|---------|-------|------|
| 16 | openai-whisper | 语音转文字 | `/openai-whisper` | `["all"]` | 🎙️ |
| 17 | ux-flow-designer | 流程图 | `/ux-flow-designer` | `["all"]` | 🔄 |
| 18 | claude-req-analysis | 需求分析 | `/claude-req-analysis` | `["all"]` | 📋 |
| 19 | drafter-diagram | 工程图 | `/drafter-diagram` | `["all"]` | 📐 |
| 20 | kdclub-ai-product-qa | 产品问答 | `/kdclub-ai-product-qa` | `["all"]` | 💬 |
| 21 | humanizer | 去AI味 | `/humanizer` | `["all"]` | ✍️ |
| 22 | doc-sanitizer | 文档脱敏 | `/doc-sanitizer` | `["all"]` | 🔒 |
| 23 | data-cleaner | 数据清洗 | `/data-cleaner` | `["all"]` | 🧹 |
| 24 | data-auditor | 数据审计 | `/data-auditor` | `["all"]` | 📈 |
| 25 | kingdee-ppt | 金蝶PPT | `/kingdee-ppt` | `["all"]` | 🎨 |
| 26 | doc-tools | 文档工具 | `/doc-tools` | `["all"]` | 📄 |
| 27 | project-dashboard | 项目看板 | `/project-dashboard` | `["all"]` | 📈 |

---

## 侧边栏分组逻辑

### 1. 常用（Top 5）
从 `worklog.md` / CLAUDE.md `## 活动日志` 统计最近 30 天调用频次最高的 5 个 Skill。

### 2. 当前阶段推荐
读取 CLAUDE.md `## 项目状态` 中标记为"进行中"的阶段，匹配 `phase` 字段包含该阶段的 Skill。

### 3. 全量索引
按 `category` 分组折叠展示：
- **核心** — 项目基础设施
- **阶段** — 按七阶段组织
- **管理** — 跨阶段管理能力
- **工具** — 辅助工具链

---

## 维护说明

- v3.0 采用硬编码，不依赖 Kingdee Skill Hub 动态查询
- 新增 Skill 时，同时修改本文件和 `server/skill_registry.py`
- `project-dashboard` 自身也列入注册表，方便用户复制命令重新生成看板
