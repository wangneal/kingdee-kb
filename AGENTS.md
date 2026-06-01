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
