---
name: qa-root-cause-analysis
version: 8.0.0
category: mgmt
phase: all
description: |
  质量根因分析工具。5-Why、鱼骨图、8D报告、Pareto分析。
    This skill should be used when the user wants to "根因分析" "5-Why" "鱼骨图" "8D报告" "质量复盘",
    or mentions similar keywords related to qa-root-cause-analysis.
---

## 概述

质量问题根因分析工具，支持 5-Why、鱼骨图、8D 报告、Pareto 分析四种方法。

## 触发条件

当用户说出以下任意短语时激活：
- "根因分析" "5-Why" "鱼骨图" "8D报告" "Pareto" "质量复盘" "问题分析"

## 工作流

### Step 1: 问题描述

收集问题信息：
- 问题现象
- 发生时间
- 影响范围
- 已采取的临时措施

### Step 2: 选择分析方法

| 方法 | 适用场景 |
|------|---------|
| 5-Why | 单一原因链式分析 |
| 鱼骨图 | 多因素系统分析 |
| 8D 报告 | 客户投诉处理 |
| Pareto | 优先级排序 |

### Step 3: 执行分析

**5-Why 分析**：
- 连续追问 5 个"为什么"
- 找到根本原因

**鱼骨图分析**：
- 人、机、料、法、环、测 六因素
- 逐项排查

**8D 报告**：
- D1: 建立团队
- D2: 问题描述
- D3: 临时措施
- D4: 根因分析
- D5: 永久措施
- D6: 验证措施
- D7: 预防措施
- D8: 团队祝贺

**Pareto 分析**：
- 统计各类问题频率
- 按频率排序
- 识别关键少数

### Step 4: 输出报告

生成根因分析报告。

## 反模式

- 停在表面原因 → 必须追问到根本原因
- 只分析不制定措施 → 必须有改进措施和责任人

## 上下文更新

- 写入：00_项目管理/质量管理/
- 写入：CLAUDE.md → ## 活动日志

