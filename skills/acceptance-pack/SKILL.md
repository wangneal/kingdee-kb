---

name: acceptance-pack
version: 8.0.0
category: stage
phase: 验收
description: |
  验收阶段文档包。AI扫描全目录盘点交付物，聚合全阶段状态生成验收报告。
    This skill should be used when the user wants to "验收阶段" "验收报告" "项目文档清单" "项目总结" "交付物盘点",
    or mentions similar keywords related to acceptance-pack.
paths: "07_验收, acceptance"
---

## 概述

金蝶 ERP 验收阶段文档生成，包括验收报告、项目文档清单、项目总结。

## 触发条件

当用户说出以下任意短语时激活：
- "验收阶段" "验收报告" "项目文档清单" "项目总结" "交付物盘点" "acceptance"

## 工作流

### Step 1: 交付物盘点

扫描项目全目录：
1. 列出所有交付物
2. 检查完成状态
3. 识别缺失文件

### Step 2: 项目数据聚合

汇总各阶段数据：
- 需求覆盖率
- 测试通过率
- 缺陷关闭率
- 变更完成率

### Step 3: 生成验收报告

按模板生成验收报告：
- 项目概况
- 实施过程回顾
- 交付物清单
- 遗留问题与后续计划

### Step 4: 项目总结

生成项目总结文档：
- 项目成果
- 经验教训
- 改进建议

## 参考文件

- `_shared/deliverable-scan.md` - 交付物扫描规范

## 反模式

- 交付物盘点不完整 → 必须扫描全目录
- 项目总结流于形式 → 需包含具体经验教训

## 上下文更新

- 读取：各阶段目录
- 写入：07_验收阶段/ 下各文件
- 写入：CLAUDE.md → ## 活动日志

