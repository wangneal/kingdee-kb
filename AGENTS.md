# 项目规则

## 技能安装位置

安装新技能（skills）时，必须安装到**项目根目录的 `skills/` 文件夹**，不要放到 `.opencode/skills/`。

- ✅ 正确：`skills/<skill-name>/SKILL.md`
- ❌ 错误：`.opencode/skills/<skill-name>/SKILL.md`

`.opencode/skills/` 仅用于 superpowers 等框架级技能，用户安装的业务技能一律放到项目的 `skills/` 目录。

## 代码注释规范

所有注释必须使用中文。禁止在代码中使用英文注释。

## 技术规格书前置阅读

新增或修改功能前，必须先阅读 `docs/superpowers/plans/2026-06-01-kingdeekb-technical-spec.md`，确认变更范围不与其他子系统冲突。规格书中已有设计决策的，不得绕过自行实现。

## 路由验证原则

修改或新增页面功能前，必须按以下顺序确认目标页面：

1. 查路由表（`Layout.tsx` 或路由配置文件），确认菜单项对应的 path 和组件
2. 从路由表找到目标组件文件后，再读该组件完整代码
3. 禁止仅凭文件名、目录名或文件内中文文案推断页面归属

**错误示例**（实际发生过）：
- 看到 `outliner/` 目录和"知识库为空"文案，就认定是知识库组件
- 未验证路由表，导致修改加到调研助手页面而非真正的知识库页面（Browse.tsx）
