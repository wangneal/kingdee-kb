---

name: weekly-report
version: 8.0.0
category: stage
phase: 项目管理
description: |
  双周滚动周报生成器。扫描项目文件，引导式访谈，输出客户版+内部版双版本。
    This skill should be used when the user wants to "生成周报" "周报" "双周周报" "工作汇报",
    or mentions similar keywords related to weekly-report.
paths: "周报, weekly"
---

## 概述

生成金蝶实施项目双周滚动周报，包含 RAG 差距识别和升级逻辑。

## 触发条件

当用户说出以下任意短语时激活：
- "生成周报" "周报" "双周周报" "工作汇报"

## 工作流

### Step 1: 扫描项目状态

1. 读取 `CLAUDE.md` → ## 项目状态
2. 扫描各阶段目录的交付物完成情况
3. 识别关键里程碑进度

### Step 2: RAG 评估

对每个维度进行 RAG（红/黄/绿）评估：
- 进度：计划 vs 实际
- 资源：人员到位情况
- 风险：已识别风险等级
- 质量：交付物完整度

### Step 3: 差距识别与升级

- 绿色：正常推进
- 黄色：存在偏差，需关注
- 红色：严重偏差，需升级处理

### Step 4: 生成周报

按模板生成双周滚动周报，包含：
- 本期工作完成情况
- 下期工作计划
- 风险与问题
- 需协调事项

## 参考文件

- `references/phase-activities.md` - 各阶段典型活动
- `references/report-templates.md` - 周报模板
- `references/version-rules.md` - 版本管理规则

## 反模式

- 只罗列工作不分析差距 → 周报核心是差距识别，不是工作清单
- 风险不升级 → 红色风险必须有升级路径

## 上下文更新

- 读取：CLAUDE.md → ## 项目状态
- 写入：CLAUDE.md → ## 活动日志

