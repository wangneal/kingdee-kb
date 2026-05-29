"""Skill registry — §6.1 v3.0-DESIGN.md
硬编码 27 个 Skill 元数据，从套件根目录 CLAUDE.md 注册表抄录。
"""
from __future__ import annotations
from typing import Any

SKILLS: list[dict[str, Any]] = [
    # ===== 核心（3）=====
    {
        "name": "project-init",
        "label": "项目启动",
        "trigger": "/project-init",
        "phase": ["all"],
        "category": "core",
        "icon": "🚀",
    },
    {
        "name": "project-sync",
        "label": "项目同步",
        "trigger": "/project-sync",
        "phase": ["all"],
        "category": "core",
        "icon": "🔄",
    },
    {
        "name": "skill-updater",
        "label": "检查更新",
        "trigger": "/skill-updater 检查更新",
        "phase": ["all"],
        "category": "core",
        "icon": "⬆️",
    },
    # ===== 阶段（9）=====
    {
        "name": "kickoff-pack",
        "label": "启动会材料",
        "trigger": "/kickoff-pack",
        "phase": ["01_启动"],
        "category": "stage",
        "icon": "🎬",
    },
    {
        "name": "survey-assistant",
        "label": "需求调研",
        "trigger": "/survey-assistant",
        "phase": ["02_需求"],
        "category": "stage",
        "icon": "📝",
    },
    {
        "name": "blueprint-tools",
        "label": "蓝图设计",
        "trigger": "/blueprint-tools",
        "phase": ["03_方案"],
        "category": "stage",
        "icon": "📐",
    },
    {
        "name": "build-tracker",
        "label": "构建追踪",
        "trigger": "/build-tracker",
        "phase": ["04_构建"],
        "category": "stage",
        "icon": "🔧",
    },
    {
        "name": "test-manager",
        "label": "测试管理",
        "trigger": "/test-manager",
        "phase": ["05_测试"],
        "category": "stage",
        "icon": "🧪",
    },
    {
        "name": "golive-pack",
        "label": "上线准备",
        "trigger": "/golive-pack",
        "phase": ["06_上线"],
        "category": "stage",
        "icon": "🚢",
    },
    {
        "name": "acceptance-pack",
        "label": "验收材料",
        "trigger": "/acceptance-pack",
        "phase": ["07_验收"],
        "category": "stage",
        "icon": "✅",
    },
    {
        "name": "weekly-report",
        "label": "生成周报",
        "trigger": "/weekly-report",
        "phase": ["all"],
        "category": "stage",
        "icon": "📊",
    },
    {
        "name": "stakeholder-comms",
        "label": "会议纪要",
        "trigger": "/stakeholder-comms",
        "phase": ["all"],
        "category": "stage",
        "icon": "🤝",
    },
    # ===== 管理（3）=====
    {
        "name": "change-manager",
        "label": "提变更",
        "trigger": "/change-manager 新增变更",
        "phase": ["all"],
        "category": "mgmt",
        "icon": "⚠️",
    },
    {
        "name": "risk-manager",
        "label": "新增风险",
        "trigger": "/risk-manager 新增风险",
        "phase": ["all"],
        "category": "mgmt",
        "icon": "🚨",
    },
    {
        "name": "qa-root-cause-analysis",
        "label": "根因分析",
        "trigger": "/qa-root-cause-analysis",
        "phase": ["all"],
        "category": "mgmt",
        "icon": "🔍",
    },
    # ===== 工具（12）=====
    {
        "name": "openai-whisper",
        "label": "语音转文字",
        "trigger": "/openai-whisper",
        "phase": ["all"],
        "category": "tool",
        "icon": "🎙️",
    },
    {
        "name": "ux-flow-designer",
        "label": "流程图",
        "trigger": "/ux-flow-designer",
        "phase": ["all"],
        "category": "tool",
        "icon": "🗺️",
    },
    {
        "name": "claude-req-analysis",
        "label": "需求分析",
        "trigger": "/claude-req-analysis",
        "phase": ["all"],
        "category": "tool",
        "icon": "🧠",
    },
    {
        "name": "drafter-diagram",
        "label": "工程图",
        "trigger": "/drafter-diagram",
        "phase": ["all"],
        "category": "tool",
        "icon": "📏",
    },
    {
        "name": "kdclub-ai-product-qa",
        "label": "产品问答",
        "trigger": "/kdclub-ai-product-qa",
        "phase": ["all"],
        "category": "tool",
        "icon": "💬",
    },
    {
        "name": "humanizer",
        "label": "去AI味",
        "trigger": "/humanizer",
        "phase": ["all"],
        "category": "tool",
        "icon": "✍️",
    },
    {
        "name": "doc-sanitizer",
        "label": "文档脱敏",
        "trigger": "/doc-sanitizer",
        "phase": ["all"],
        "category": "tool",
        "icon": "🔒",
    },
    {
        "name": "data-cleaner",
        "label": "数据清洗",
        "trigger": "/data-cleaner",
        "phase": ["all"],
        "category": "tool",
        "icon": "🧹",
    },
    {
        "name": "data-auditor",
        "label": "数据审计",
        "trigger": "/data-auditor",
        "phase": ["all"],
        "category": "tool",
        "icon": "📋",
    },
    {
        "name": "kingdee-ppt",
        "label": "生成PPT",
        "trigger": "/kingdee-ppt",
        "phase": ["all"],
        "category": "tool",
        "icon": "🎯",
    },
    {
        "name": "doc-tools",
        "label": "文档工具",
        "trigger": "/doc-tools",
        "phase": ["all"],
        "category": "tool",
        "icon": "📄",
    },
    {
        "name": "project-dashboard",
        "label": "项目看板",
        "trigger": "/project-dashboard",
        "phase": ["all"],
        "category": "tool",
        "icon": "📈",
    },
]


def get_recommended(current_phase: str, usage_log: list[dict[str, Any]]) -> dict[str, Any]:
    """
    返回三组 Skill：
    - usual: 最近 30 天调用频次 Top 5
    - current_phase: 匹配当前阶段的 Skill
    - all: 按 category 分组的全量
    """
    # 常用 Top 5（简单计数）
    counts: dict[str, int] = {}
    for log in usage_log:
        name = log.get("skill", "")
        if name:
            counts[name] = counts.get(name, 0) + 1
    usual_names = sorted(counts, key=counts.get, reverse=True)[:5]
    usual = [s for s in SKILLS if s["name"] in usual_names]

    # 当前阶段推荐
    current = [s for s in SKILLS if current_phase in s.get("phase", []) or "all" in s.get("phase", [])]
    # 去重并优先阶段专属
    current = [s for s in current if current_phase in s.get("phase", [])][:6]
    if len(current) < 6:
        extras = [s for s in SKILLS if "all" in s.get("phase", []) and s not in current]
        current += extras[:6 - len(current)]

    # 全量按 category 分组
    all_by_cat: dict[str, list[dict[str, Any]]] = {}
    for s in SKILLS:
        cat = s.get("category", "other")
        all_by_cat.setdefault(cat, []).append(s)

    return {
        "usual": usual,
        "current_phase": current,
        "all": all_by_cat,
    }


def get_by_name(name: str) -> dict[str, Any] | None:
    for s in SKILLS:
        if s["name"] == name:
            return s
    return None
