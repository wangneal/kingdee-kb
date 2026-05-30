---

name: blueprint-tools
version: 8.0.0
category: stage
phase: 方案
description: |
  蓝图设计工具集。业务流程分析、需求规格输出、流程图绘制。
    This skill should be used when the user wants to "蓝图设计" "方案设计" "流程分析" "需求规格",
    or mentions similar keywords related to blueprint-tools.
paths: "03_蓝图, blueprint"
---

## 概述

金蝶 ERP 蓝图设计阶段工具集，包含业务流程图绘制、需求规格撰写、蓝图方案输出。

## 触发条件

当用户说出以下任意短语时激活：
- "蓝图设计" "方案设计" "流程分析" "需求规格"

## 工作流

### Step 0: 模板检查

检查蓝图阶段模板是否就绪。

### Step 1: 业务流程梳理

基于调研结果：
1. 识别核心业务流程
2. 绘制 As-Is 流程图（当前状态）
3. 设计 To-Be 流程图（目标状态）

### Step 2: 差距分析

对比 As-Is 与 To-Be：
- 标准功能可满足
- 需要配置调整
- 需要二次开发
- 需要业务流程重组

### Step 3: 需求规格撰写

按模块撰写需求规格说明书：
- 功能描述
- 业务规则
- 界面要求
- 数据要求

### Step 4: 蓝图方案评审

生成蓝图评审材料，支持客户评审。

## 参考文件

- `ux-flow-designer` - Mermaid 流程图
- `claude-req-analysis` - 需求分析

## 反模式

- 只画图不分析差距 → 流程图的价值在于差距识别
- 需求规格过于笼统 → 需具体到字段级别

## 上下文更新

- 读取：02_调研阶段/ 需求矩阵
- 写入：03_蓝图阶段/ 下各文件
- 写入：CLAUDE.md → ## 活动日志

