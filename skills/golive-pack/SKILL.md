---

name: golive-pack
version: 8.0.0
category: stage
phase: 上线
description: |
  上线阶段文档包。切换方案、上线检查、初始化清单、上线动员会PPT。
    This skill should be used when the user wants to "上线阶段" "切换方案" "上线检查" "初始化清单" "go-live",
    or mentions similar keywords related to golive-pack.
paths: "06_上线, golive, cutover"
---

## 概述

金蝶 ERP 上线阶段文档生成，包括切换方案、上线检查表、初始化清单。

## 触发条件

当用户说出以下任意短语时激活：
- "上线阶段" "切换方案" "上线检查" "初始化清单" "go-live" "cutover"

## 工作流

### Step 0: 模板检查

检查上线阶段模板是否就绪。

### Step 1: 切换方案制定

- 切换时间窗口
- 数据迁移步骤
- 回滚方案
- 应急预案

### Step 2: 上线检查表

按模块生成检查清单：
- 系统配置检查
- 数据完整性检查
- 权限配置检查
- 接口联调检查

### Step 3: 初始化清单

系统初始化操作清单：
- 基础数据初始化
- 期初数据录入
- 系统参数配置

### Step 4: 上线动员会 PPT

调用 kingdee-ppt Skill，生成上线动员会材料。

## 反模式

- 切换方案不包含回滚方案 → 必须有应急回退路径
- 检查表不按模块 → 需按业务模块细分

## 上下文更新

- 读取：04_构建阶段/ 配置清单
- 写入：06_上线阶段/ 下各文件
- 写入：CLAUDE.md → ## 活动日志

