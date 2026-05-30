---
name: ux-flow-designer
version: 8.0.0
category: tool
phase: all
description: |
  流程图设计工具。Mermaid语法生成flowchart/state/sequence图表。
    This skill should be used when the user wants to "流程图" "状态图" "时序图" "Mermaid",
    or mentions similar keywords related to ux-flow-designer.
---

## 概述

使用 Mermaid 语法绘制业务流程图、状态图、时序图、架构图。

## 触发条件

当用户说出以下任意短语时激活：
- "流程图" "状态图" "时序图" "架构图" "流程梳理" "Mermaid"

## 工作流

### Step 1: 需求分析

确定图表类型和内容：
- 流程图：业务流程
- 状态图：状态转换
- 时序图：交互流程
- 架构图：系统架构

### Step 2: 数据收集

从项目文档提取关键信息：
- 业务流程节点
- 决策点
- 参与角色
- 数据流向

### Step 3: Mermaid 生成

生成 Mermaid 代码：
```mermaid
graph TD
    A[开始] --> B{判断}
    B -->|是| C[处理]
    B -->|否| D[结束]
```

### Step 4: 渲染与导出

支持导出：
- SVG 矢量图
- PNG 图片
- 嵌入 Markdown

## 参考文件

- `references/mermaid-patterns.md` - Mermaid 模式库

## 反模式

- 图表过于复杂 → 单图节点不超过 20 个
- 不标注角色 → 流程图需标注每步的责任角色

## 上下文更新

- 写入：03_蓝图阶段/ 流程图/

