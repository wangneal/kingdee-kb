---
name: data-cleaner
description: "数据清洗 / 数据整理 / 格式转换 / 数据导入 / 去重。数据清洗、格式转换、去重，为构建阶段提供干净数据"
version: "8.0.0"
category: tool
---

# 金蝶项目交付 Skill 套件

基于金蝶实施方法�?V10.0，为交付顾问提供从项目启动到验收的全流程 AI 辅助工具集�?
## 架构

```
3 核心 + 9 阶段 + 3 管理 + 12 工具 = 27 Skills
```

### 核心 Skill（预装）

| Skill | 用�?|
|-------|------|
| project-init | 引导式项目启�?· 裁剪方案智能判定 · 材料上传解析 |
| project-sync | 团队文件 Git 同步 · 中文交互 |
| skill-updater | 套件整包版本更新 · 首次安装引导 |

### 阶段 Skill

| Skill | 用�?|
|-------|------|
| weekly-report | 双周滚动周报 · RAG 差距识别 |
| kickoff-pack | 启动�?PPT · 任命�?· 干系人沟�?· 交互式前置确�?|
| survey-assistant | 调研计划 · 访谈纪要 · 需求矩�?|
| blueprint-tools | 蓝图设计 · 流程分析 · 需求规�?|
| build-tracker | 构建跟踪 · 数据清洗 · 系统配置 |
| test-manager | 测试用例生成 · 流程测试 |
| golive-pack | 切换方案 · 上线检�?· 初始化清�?|
| acceptance-pack | 验收报告 · 项目总结 · 交付物盘�?|
| stakeholder-comms | 会议纪要 · 干系人沟�?|

### 管理 Skill（跨阶段�?
| Skill | 用�?|
|-------|------|
| change-manager | 变更申请 · OOXML 模板自动填充 |
| risk-manager | 风险识别 · 7keys 评估 · xlsx 级联跟踪 |
| qa-root-cause-analysis | 5-Why · 鱼骨�?· 8D · Pareto 质量复盘 |

### 工具 Skill（辅助工具链�?
| Skill | 用�?|
|-------|------|
| humanizer | AI 文案去味 · 24 种模式检�?|
| kdclub-ai-product-qa | 金蝶产品智能问答 · 社区 API |
| kingdee-ppt | PPTX / HTML 幻灯片生�?|
| doc-tools | OOXML 编辑 · docx/PDF 生成 |
| doc-sanitizer | 文档脱敏 · 敏感信息清理 |
| openai-whisper | 语音转文�?· 会议录音转录 |
| ux-flow-designer | 流程�?· 状态图 · Mermaid |
| claude-req-analysis | 客户需求分�?· 资料梳理 |
| drafter-diagram | 工程�?· 架构�?· 拓扑�?|
| data-cleaner | 数据清洗 · 格式转换 · 去重 |
| data-auditor | 数据质量审计 · 完整性检�?|
| project-dashboard | HTML 看板 · 进度可视�?· 项目全景 |

## 全局原则

1. **默认输出 Markdown** �?后续按需转为 docx/pptx/xlsx
2. **裁剪经用户确�?* �?文件操作须用户明确确�?3. **保留不删�?* �?非必选交付物移入 `被裁剪文�?`
4. **禁止强制推�?* �?不执行破坏�?Git 操作
5. **错误不中�?* �?非致命错误记录后继续
6. **模板懒加�?* �?27 �?Skill 全预装；Office 模板按需�?Gitee 私有仓下�?7. **输出人性化** �?客户文档�?Humanizer �?AI �?
## 模板系统

91 �?Office 交付物模板明文存放于 **Gitee 私有仓库**，按需懒加载，安装包仅�?4MB。阶�?Skill �?Step 0 自动检查→下载→解压→就绪，用户完全无感�?
**两层模型**：套件仅发布到公�?Kingdee Skill Hub（访问控制层，员工下载即获得访问令牌）；模板存于 Gitee 私有仓库（防泄露层，明文但不被公网访问）。`.claude/template-token` �?Gitee access token，随套件�?Hub 分发；下载脚本为�?Python，经 HTTPS + access_token 拉取，跨平台�?
## 依赖

- Claude Code CLI
- Python 3（模板下载、项目看板必需�?- Git（project-sync 团队同步用）
- openai-whisper（可选，语音转文字）

## 版本


当前版本 **v6.0.0**。完整版本历史见 [`CHANGELOG.md`](CHANGELOG.md)�?

## �?Skill 来源声明

本套件遵循「先借用开源再改造」原则。以下是各子 Skill 的灵感来源与原始作者�?
### 核心 Skill

| Skill | 来源 | 说明 |
|-------|------|------|
| project-init | [serejaris/ris-claude-code](https://github.com/serejaris/ris-claude-code) �?`project-init` | 采用引导式访谈模式，扩展金蝶裁剪方案判定、三级目录初始化、已有项目整�?|
| project-sync | 原创 | 金蝶ERP团队文件 Git 同步，中文交互封�?|
| skill-updater | 原创 | 版本更新与增量升级机�?|

### 阶段 Skill

| Skill | 来源 | 说明 |
|-------|------|------|
| weekly-report | [legalopsconsulting/lpm-skills](https://github.com/legalopsconsulting/lpm-skills) �?`status-report-drafter` | 改�?RAG 评估、差距识别、升级逻辑，适配金蝶双周滚动周报 |
| kickoff-pack | 原创（基于金�?V10.0 模板�?| 启动阶段文档生成 |
| survey-assistant | 原创（基于金�?V10.0 模板�?| 需求调研辅�?|
| stakeholder-comms | [legalopsconsulting/lpm-skills](https://github.com/legalopsconsulting/lpm-skills) + 原创 | 会议录音→Whisper→AI 结构化→会议纪要模板 |
| blueprint-tools | 原创（基于金�?V10.0 模板�? [ThomasPraun/ux-flow-designer](https://github.com/ThomasPraun/ux-flow-designer) + [chrzamz/claude-req-analysis](https://github.com/chrzamz/claude-req-analysis) | Mermaid 流程图、需求分析、文档脱�?|
| build-tracker | 原创（基于金�?V10.0 模板�? [huangji6693-max/office-skills-pro](https://github.com/huangji6693-max/office-skills-pro) + [tathadn/data-quality-skills-pipeline](https://github.com/tathadn/data-quality-skills-pipeline) | 数据质量审计→清洗→导入，系统配置清�?|
| test-manager | 原创（基于金�?V10.0 模板�?| �?To-Be 流程图提取测试步骤，写入方法�?xlsx 模板 |
| golive-pack | 原创（基于金�?V10.0 模板�? [WayneZhon/KingDee-PPT-Skill](https://github.com/WayneZhon/KingDee-PPT-Skill) | 切换方案+用户权限+初始化清�?检查表+上线动员会PPT |
| acceptance-pack | 原创（基于金�?V10.0 模板�?| AI 扫描全目录盘点交付物 + 聚合全阶段状态生成验收报�?|

### 管理 Skill

| Skill | 来源 | 说明 |
|-------|------|------|
| change-manager | 原创（基�?V10.0 变更申请单模板） | 变更申请 · OOXML 模板自动填充 |
| risk-manager | 原创（基�?V10.0 风险跟踪记录表模板） | 风险识别 · 7Keys 评估 · xlsx 级联跟踪 |
| qa-root-cause-analysis | Kingdee Skill Hub（上传者：金蝶软件/产品研发中心/星瀚产品管理部/央企研发中心 耿虎山） | 5-Why/鱼骨�?8D/Pareto 四种方法 |

### 工具 Skill

| Skill | 来源 | 说明 |
|-------|------|------|
| humanizer | Kingdee Skill Hub（原始出�?ClawdHub�?| 基于 Wikipedia Signs of AI writing�?4 种模式检�?|
| kdclub-ai-product-qa | Kingdee Skill Hub（上传者：产品研发中心/产品管理�?社区平台与运营部 田定强） | 调用金蝶云社区官方知识库 API |
| kingdee-ppt | Kingdee Skill Hub（上传者：金蝶软件/产品研发中心/苍穹平台�?AI平台生态产品部 钟伟纯） | 金蝶官方风格 PPTX/HTML 幻灯片生成，双风�?|
| doc-tools | 原创（OOXML + Playwright�?| Word 模板编辑 + HTML→PDF |
| doc-sanitizer | 原创 | 客户文档敏感信息脱敏 |
| openai-whisper | [openai/whisper](https://github.com/openai/whisper) | 本地语音转文�?|
| ux-flow-designer | [ThomasPraun/ux-flow-designer](https://github.com/ThomasPraun/ux-flow-designer) | 改造为 ERP 蓝图阶段流程图示工具（Mermaid�?|
| claude-req-analysis | [chrzamz/claude-req-analysis](https://github.com/chrzamz/claude-req-analysis) | 客户资料系统性分析，结构化需求文�?|
| drafter-diagram | qoder work 技能市�?| 工程蓝图风格 HTML 图表（架构图、拓扑图、流程图�?|
| data-cleaner | [huangji6693-max/office-skills-pro](https://github.com/huangji6693-max/office-skills-pro) | 数据清洗 · 格式转换 · 去重 |
| data-auditor | [tathadn/data-quality-skills-pipeline](https://github.com/tathadn/data-quality-skills-pipeline) | 数据质量审计 · 完整性检�?|
| project-dashboard | 原创 | HTML 看板 · 进度可视�?· 项目全景 |

### 开发方法论来源

| 来源 | 用�?|
|------|------|
| [anthropics/claude-code](https://github.com/anthropics/claude-code) �?`plugin-dev/skills/skill-development` | Skill 官方开发指�?|
| [obra/superpowers](https://github.com/obra/superpowers) �?`writing-skills` | CSO 原则（Description = 触发条件�?|
| [daymade/claude-code-skills](https://github.com/daymade/claude-code-skills) �?`skill-creator` | Skill 创建工具 |
| [trailofbits/skills](https://github.com/trailofbits/skills) | Skill 质量标准 |
| [davila7/claude-code-templates](https://github.com/davila7/claude-code-templates) | Anthropic 最佳实�?|

## 致谢

感谢以下开源项目和社区的贡献：**serejaris**（项目启动模式）�?*legalopsconsulting**（RAID管理/状态报�?干系人沟通）�?*WayneZhon**（PPT生成）�?*ThomasPraun**（流程图）�?*chrzamz**（需求分析）�?*Anthropic**（Claude Code 平台�?Skill 开发指南）�?*obra**（CSO 概念）�?
## 参�?
- 套件详情：`CLAUDE.md` �?完整 Skill 注册表与开发规�?- 模板仓库：`gitee.com/mjlkevin/kingdee-impleme-ntation-templates`
- 开源致谢：`README.md` �?�?Skill 来源声明
