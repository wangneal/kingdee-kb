"""Renderer — §9 v3.0-DESIGN.md
Accepts snapshot + analysis, injects into index.html.tmpl, outputs HTML string.
All static resources are inlined for single-file distribution.
"""
from __future__ import annotations
import base64
import json
import re
from pathlib import Path
from typing import Any

from . import health_score, anomaly_detector, skill_registry


def _read_file(path: Path) -> str:
    try:
        with open(path, "r", encoding="utf-8") as f:
            return f.read()
    except Exception:
        return ""


def _b64_image(path: Path) -> str:
    try:
        with open(path, "rb") as f:
            return base64.b64encode(f.read()).decode("ascii")
    except Exception:
        return ""


def _suite_version(base_dir: Path) -> str:
    """读取套件根 SKILL.md frontmatter 的 version 字段，失败兜底 v5.0.0。"""
    try:
        suite_skill = base_dir.parent.parent.parent / "SKILL.md"
        for line in _read_file(suite_skill).splitlines():
            m = re.match(r"\s*version:\s*v?([\d.]+)", line)
            if m:
                return "v" + m.group(1)
    except Exception:
        pass
    return "v5.0.0"


def _today_from_snapshot(snapshot: dict[str, Any]) -> str:
    today = snapshot.get("today", "")
    if not today:
        meta = snapshot.get("meta", {})
        today = meta.get("generated_at", "")[:10]
    if not today:
        from datetime import date as _date
        today = _date.today().isoformat()
    return today


def _compute_day_n(start_date: str, today: str) -> int:
    try:
        from datetime import date
        y1, m1, d1 = int(start_date[:4]), int(start_date[5:7]), int(start_date[8:10])
        y2, m2, d2 = int(today[:4]), int(today[5:7]), int(today[8:10])
        return max(1, (date(y2, m2, d2) - date(y1, m1, d1)).days)
    except Exception:
        return 1


def _next_milestone(milestones: list[dict[str, str]], today: str) -> dict[str, Any]:
    pending = [m for m in milestones if m.get("status") in ("pending", "未开始")]
    if not pending:
        return {"days": "—", "name": "无待办里程碑"}
    nearest = None
    nearest_days = 9999
    for m in pending:
        pd = m.get("plan_date") or m.get("planned_date")
        if pd and pd != "-":
            try:
                from datetime import date
                ty, tm, td = int(today[:4]), int(today[5:7]), int(today[8:10])
                py, pm, pd_d = int(pd[:4]), int(pd[5:7]), int(pd[8:10])
                days = (date(py, pm, pd_d) - date(ty, tm, td)).days
                if days < nearest_days:
                    nearest_days = days
                    nearest = m
            except Exception:
                continue
    if nearest is None:
        return {"days": "—", "name": pending[0].get("name", "待定")}
    return {
        "days": f"T{nearest_days}d" if nearest_days >= 0 else f"T+{abs(nearest_days)}d",
        "name": nearest.get("name", ""),
    }


def _build_baselines(health: dict[str, Any], snapshot: dict[str, Any]) -> list[dict[str, Any]]:
    """五基线卡：主数字为业务原值，RAG 色条由健康度子分驱动（§2.2 v5.0-DESIGN）。"""
    sub = health.get("subscores", {})
    m = health.get("metrics", {})
    estimated = set(health.get("estimated", []))

    def _rag(v: int) -> str:
        if v >= 80:
            return "green"
        if v >= 60:
            return "yellow"
        return "red"

    cost_pct = m.get("cost_pct")
    cost_main = f"{cost_pct}%" if cost_pct is not None else "—"
    contract_days = m.get("contract_days")
    contract_label = f"{contract_days}" if contract_days else "未配置"

    baselines = [
        {"id": "scope", "label": "范围", "value": f'{m.get("approved_change_days", 0)} 人天',
         "sub": "已批准变更影响", "rag": _rag(sub.get("scope", 70)), "estimated": "scope" in estimated},
        {"id": "schedule", "label": "进度", "value": f'{m.get("completed_phases", 0)}/{m.get("total_phases", 7)}',
         "sub": f'已完成阶段 · SPI {m.get("spi", "—")}', "rag": _rag(sub.get("schedule", 70)), "estimated": "schedule" in estimated},
        {"id": "quality", "label": "质量", "value": str(m.get("defect_active", 0)),
         "sub": f'活跃缺陷 · 严重 {m.get("defect_critical", 0)}', "rag": _rag(sub.get("quality", 70)), "estimated": "quality" in estimated},
        {"id": "risk", "label": "风险", "value": str(m.get("risk_active", 0)),
         "sub": f'活跃风险 · 高危 {m.get("risk_high", 0)}', "rag": _rag(sub.get("risk", 70)), "estimated": "risk" in estimated},
        {"id": "cost", "label": "成本", "value": cost_main,
         "sub": f'已耗/合同 {contract_label} 人天', "rag": _rag(sub.get("cost", 70)), "estimated": "cost" in estimated},
    ]
    return baselines


def _build_skills(snapshot: dict[str, Any]) -> dict[str, Any]:
    phases = snapshot.get("phases", [])
    current_phase = ""
    for p in phases:
        if p.get("status") == "进行中":
            current_phase = p.get("name", "")
            break
    phase_map = {
        "启动": "01_启动", "需求": "02_需求", "方案": "03_方案",
        "构建": "04_构建", "测试": "05_测试", "上线": "06_上线", "验收": "07_验收",
    }
    mapped = phase_map.get(current_phase, "")
    usage_log = snapshot.get("usage_log", [])
    # Fallback to activity_log if usage_log not present
    if not usage_log:
        usage_log = [{"skill": a.get("source", ""), "date": a.get("ts", "")} for a in snapshot.get("activity_log", [])]
    return skill_registry.get_recommended(mapped, usage_log)


def _build_deliverables(snapshot: dict[str, Any]) -> list[dict[str, Any]]:
    """真实交付物网格。无扫描数据时返回空列表，前端显示「暂无数据」，不编造。"""
    deliverables = snapshot.get("deliverables", {})
    if isinstance(deliverables, dict) and deliverables.get("cells"):
        cells = deliverables.get("cells", [])
        phases: dict[str, list[Any]] = {}
        for c in cells:
            phase = c.get("phase", "其他")
            phases.setdefault(phase, []).append(c)
        return [{"name": k, "cells": v} for k, v in phases.items()]
    return []


def _build_stakeholders(snapshot: dict[str, Any]) -> list[dict[str, Any]]:
    stakeholders = snapshot.get("stakeholders", [])
    today = _today_from_snapshot(snapshot)
    result = []
    for s in stakeholders:
        stale_days = 0
        last_contact = s.get("last_contact")
        if last_contact and today:
            try:
                from datetime import date
                lc = last_contact[:10]
                y1, m1, d1 = int(lc[:4]), int(lc[5:7]), int(lc[8:10])
                y2, m2, d2 = int(today[:4]), int(today[5:7]), int(today[8:10])
                stale_days = max(0, (date(y2, m2, d2) - date(y1, m1, d1)).days)
            except Exception:
                pass
        result.append({
            "name": s.get("name", ""),
            "role": s.get("role", ""),
            "stale_days": stale_days,
        })
    return result


def _build_meetings(snapshot: dict[str, Any]) -> list[dict[str, Any]]:
    meetings = snapshot.get("meetings_this_week", [])
    result = []
    for m in meetings:
        result.append({
            "id": m.get("meeting_id", m.get("id", "")),
            "date": m.get("ts", "")[:10],
            "summary": m.get("summary", "会议"),
            "participants": m.get("participants", []),
        })
    return result


def render(snapshot: dict[str, Any], analysis: dict[str, Any] | None = None) -> str:
    base_dir = Path(__file__).resolve().parent.parent
    tmpl_path = base_dir / "frontend" / "index.html.tmpl"
    css_tokens = base_dir / "frontend" / "styles" / "kingdee-tokens.css"
    css_components = base_dir / "frontend" / "styles" / "kingdee-components.css"
    css_layout = base_dir / "frontend" / "styles" / "dashboard-layout.css"
    alpine_path = base_dir / "frontend" / "vendor" / "alpine.min.js"
    logo_path = base_dir / "frontend" / "assets" / "kingdee-blue.png"

    tmpl = _read_file(tmpl_path)
    tokens_css = _read_file(css_tokens)
    components_css = _read_file(css_components)
    layout_css = _read_file(css_layout)
    alpine_js = _read_file(alpine_path)
    logo_b64 = _b64_image(logo_path)

    if analysis is None:
        health = health_score.calculate(snapshot)
        anomalies = anomaly_detector.detect(snapshot)
    else:
        health = analysis.get("health")
        if health is None:
            health = health_score.calculate(snapshot)
        anomalies = analysis.get("anomalies")
        if anomalies is None:
            anomalies = anomaly_detector.detect(snapshot)

    today = _today_from_snapshot(snapshot)
    start_date = snapshot.get("start_date", "")
    day_n = _compute_day_n(start_date, today) if start_date and today else snapshot.get("day_count", 1)
    milestones = snapshot.get("milestones", [])
    next_m = _next_milestone(milestones, today)

    top3 = anomaly_detector.top3(anomalies)
    top3_dicts = [
        {
            "id": a.id, "rule_id": a.rule_id, "severity": a.severity,
            "title": a.title, "detail": a.detail,
            "action_label": a.action_label, "action_cmd": a.action_cmd,
            "baseline": a.baseline,
        }
        for a in top3
    ]

    skills_data = _build_skills(snapshot)
    baselines = _build_baselines(health, snapshot)
    deliverables = _build_deliverables(snapshot)
    stakeholders = _build_stakeholders(snapshot)
    meetings = _build_meetings(snapshot)

    trend = (analysis or {}).get("trend") or {
        "count": 0, "dates": [], "health": [],
        "baselines": {k: [] for k in ("schedule", "cost", "quality", "risk", "scope", "comm")},
    }

    data = {
        "health": health,
        "anomalies_top3": top3_dicts,
        "skills": skills_data,
        "baselines": baselines,
        "deliverables": deliverables,
        "stakeholders": stakeholders,
        "meetings": meetings,
        "next_milestone": next_m,
        "trend": trend,
        "generated_at": today,
    }

    data_json = json.dumps(data, ensure_ascii=False, indent=2)

    # Replace placeholders
    html = tmpl
    html = html.replace("{{TOKENS_CSS}}", tokens_css)
    html = html.replace("{{COMPONENTS_CSS}}", components_css)
    html = html.replace("{{LAYOUT_CSS}}", layout_css)
    html = html.replace("{{ALPINE_JS}}", alpine_js)
    html = html.replace("{{LOGO_BASE64}}", logo_b64)
    html = html.replace("{{PROJECT_NAME}}", snapshot.get("project_name", "未命名项目"))
    html = html.replace("{{PRODUCT_TYPE}}", snapshot.get("product_type", "—"))
    html = html.replace("{{PM_NAME}}", snapshot.get("pm_name", "—"))
    html = html.replace("{{DAY_N}}", str(day_n))
    html = html.replace("{{CONFIDENTIAL}}", "内部公开")
    html = html.replace("{{SUITE_VERSION}}", _suite_version(base_dir))
    html = html.replace("{{DATA_JSON}}", data_json)

    return html
