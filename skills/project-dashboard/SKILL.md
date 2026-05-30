---
name: project-dashboard
version: 8.0.0
category: tool
phase: all
description: |
  项目看板。HTML可视化看板，健康度评分，异常检测，交付物网格。
    This skill should be used when the user wants to "项目看板" "dashboard" "项目概览" "可视化",
    or mentions similar keywords related to project-dashboard.
---

## 概述

项目可视化看板，聚合项目数据生成 HTML 交互式看板。

## 触发条件

当用户说出以下任意短语时激活：
- "项目看板" "dashboard" "项目概览" "可视化" "进度看板"

## 工作流

### Step 1: 数据聚合

扫描项目目录，收集：
- 各阶段完成度
- 交付物状态
- 风险统计
- 变更统计

### Step 2: 健康评分

计算项目健康评分：
- 进度维度
- 质量维度
- 风险维度
- 成本维度

### Step 3: 看板生成

生成 HTML 看板：
- 项目概览卡片
- 阶段进度条
- 风险热力图
- 变更趋势图

### Step 4: 交互功能

支持：
- 点击查看详情
- 时间范围筛选
- 导出 PDF

## 参考文件

- `references/health-score.md` - 健康评分算法
- `references/signals-schema.md` - 信号数据格式
- `references/default-deliverables.md` - 默认交付物清单

## 反模式

- 数据不实时 → 每次打开需重新扫描
- 看板信息过载 → 突出关键指标

## 上下文更新

- 读取：各阶段目录
- 读取：CLAUDE.md → ## 项目状态

