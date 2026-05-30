---
name: drafter-diagram
version: 8.0.0
category: tool
phase: all
description: |
  工程蓝图风格图表生成器。架构图、拓扑图、流程图的HTML可视化。
    This skill should be used when the user wants to "画图" "架构图" "拓扑图" "工程图",
    or mentions similar keywords related to drafter-diagram.
---

## 概述

工程蓝图风格 HTML 图表生成，支持架构图、拓扑图、流程图。

## 触发条件

当用户说出以下任意短语时激活：
- "画图" "工程图" "技术蓝图" "架构图" "拓扑图" "flat diagram"

## 工作流

### Step 1: 图表类型选择

确定图表类型：
- 架构图：系统组件关系
- 拓扑图：网络拓扑
- 流程图：技术流程
- 部署图：部署架构

### Step 2: 元素定义

定义图表元素：
- 节点（服务器、服务、组件）
- 连接（数据流、依赖关系）
- 分组（层级、区域）

### Step 3: HTML 生成

生成交互式 HTML 图表。

### Step 4: 导出

支持导出为 SVG/PNG。

## 反模式

- 图表信息不完整 → 每个节点需有明确标签
- 连接关系混乱 → 需标注连接类型

## 上下文更新

- 写入：03_蓝图阶段/ 技术图/

