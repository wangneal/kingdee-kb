"""Project health score calculator — §4 v3.0-DESIGN.md"""
from __future__ import annotations
import math
from typing import Any


def _clamp(v: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, v))


def _parse_date(date_str: str | None) -> str | None:
    if not date_str or date_str.strip() in ("-", "", "未配置"):
        return None
    return date_str.strip()


def _today_from_snapshot(snapshot: dict[str, Any]) -> str:
    today = snapshot.get("today", "")
    if not today:
        meta = snapshot.get("meta", {})
        today = meta.get("generated_at", "")[:10]
    if not today:
        from datetime import date as _date
        today = _date.today().isoformat()
    return today


def calculate(snapshot: dict[str, Any]) -> dict[str, Any]:
    """
    输入 snapshot dict，输出:
    {
      score: int (0-100),
      rag: 'green' | 'yellow' | 'red',
      subscores: {schedule:int, cost:int, quality:int,
                  risk:int, scope:int, comm:int},
      trend: int | None,
      notes: dict[str, str]
    }
    """
    phases = snapshot.get("phases", [])
    milestones = snapshot.get("milestones", [])
    risks = snapshot.get("risks", [])
    changes = snapshot.get("changes", [])
    defects = snapshot.get("defects", [])
    weekly_reports = snapshot.get("weekly_reports", {})
    signals = snapshot.get("signals", {})
    events = signals.get("events", []) if isinstance(signals, dict) else []

    contract_days = snapshot.get("contract_days")
    start_date = snapshot.get("start_date", "")
    today = _today_from_snapshot(snapshot)

    total_phases = 7
    completed = sum(1 for p in phases if p.get("status") == "已完成")

    # 业务原值指标（供五基线卡展示，§2.2 v5.0-DESIGN）
    m_approved_days = 0
    m_defect_active = 0
    m_defect_critical = 0
    m_risk_active = 0
    m_risk_high = 0
    m_consumed_days = None
    m_cost_pct = None

    # ---- Schedule ----
    planned_completed = 0
    has_plan_dates = any(p.get("plan_end") or p.get("planned_end") for p in phases)
    if has_plan_dates:
        if today:
            for p in phases:
                plan_end = _parse_date(p.get("plan_end") or p.get("planned_end"))
                if plan_end and plan_end <= today:
                    planned_completed += 1
        else:
            planned_completed = completed
    else:
        current_idx = 0
        for i, p in enumerate(phases):
            if p.get("status") == "进行中":
                current_idx = i + 1
                break
        planned_completed = max(1, current_idx)

    spi = completed / max(1, planned_completed)
    schedule = int(_clamp(100 - abs(1 - spi) * 100, 0, 100))
    schedule_note = ""

    # ---- Cost ----
    if contract_days is None or contract_days == 0:
        cost = 70
        cost_note = "估算（缺合同人天）"
    else:
        timesheet_hours = sum(
            e.get("payload", {}).get("hours", 0)
            for e in events
            if e.get("baseline") == "cost" and e.get("type") == "timesheet.logged"
        )
        consumed_days = timesheet_hours / 8 if timesheet_hours else (completed / total_phases) * contract_days
        progress_pct = completed / total_phases
        cost = int(_clamp(100 - max(0, consumed_days / contract_days - progress_pct) * 200, 0, 100))
        cost_note = ""
        m_consumed_days = round(consumed_days, 1)
        m_cost_pct = int(consumed_days / contract_days * 100) if contract_days else None

    # ---- Quality ----
    # 优先用 defects 字段，否则从 signals 推断
    if not defects:
        # 从 signals 统计缺陷
        defect_events = [e for e in events if e.get("baseline") == "quality" and "defect" in e.get("type", "")]
        if not defect_events:
            quality = 80
            quality_note = "估算（无缺陷数据）"
        else:
            created = [e for e in defect_events if "created" in e.get("type", "")]
            closed_ids = {e.get("id") for e in defect_events if "closed" in e.get("type", "")}
            active = [e for e in created if e.get("id") not in closed_ids]
            p0 = sum(1 for e in active if e.get("payload", {}).get("level") == "P0")
            p1 = sum(1 for e in active if e.get("payload", {}).get("level") == "P1")
            p2 = sum(1 for e in active if e.get("payload", {}).get("level") == "P2")
            p3 = sum(1 for e in active if e.get("payload", {}).get("level") == "P3")
            quality = int(100 - min(100, p0 * 20 + p1 * 10 + p2 * 3 + p3 * 1))
            quality_note = ""
            m_defect_active = len(active)
            m_defect_critical = p0 + p1
    else:
        active_d = [d for d in defects if d.get("status") != "已关闭"]
        p0 = sum(1 for d in active_d if d.get("level") == "P0")
        p1 = sum(1 for d in active_d if d.get("level") == "P1")
        p2 = sum(1 for d in active_d if d.get("level") == "P2")
        p3 = sum(1 for d in active_d if d.get("level") == "P3")
        quality = int(100 - min(100, p0 * 20 + p1 * 10 + p2 * 3 + p3 * 1))
        quality_note = ""
        m_defect_active = len(active_d)
        m_defect_critical = p0 + p1

    # ---- Risk ----
    if not risks:
        # 从 signals 推断风险
        risk_events = [e for e in events if e.get("baseline") == "risk" and e.get("type") == "risk.created"]
        if not risk_events:
            risk_score = 80
            risk_note = "估算（无风险数据）"
        else:
            high = sum(1 for e in risk_events if e.get("payload", {}).get("severity") == "高")
            medium = sum(1 for e in risk_events if e.get("payload", {}).get("severity") == "中")
            stall_total = 0
            for e in risk_events:
                ts = e.get("ts", "")[:10]
                if ts and today:
                    try:
                        from datetime import date
                        y1, m1, d1 = int(ts[:4]), int(ts[5:7]), int(ts[8:10])
                        y2, m2, d2 = int(today[:4]), int(today[5:7]), int(today[8:10])
                        stall_total += max(0, (date(y2, m2, d2) - date(y1, m1, d1)).days)
                    except Exception:
                        pass
            risk_score = int(100 - min(100, high * 15 + medium * 5 + stall_total))
            risk_note = ""
            m_risk_active = len(risk_events)
            m_risk_high = high
    else:
        high = sum(1 for r in risks if r.get("severity") == "高")
        medium = sum(1 for r in risks if r.get("severity") == "中")
        stall_total = 0
        for r in risks:
            stall = r.get("stall_days", 0)
            if isinstance(stall, (int, float)):
                stall_total += stall
        risk_score = int(100 - min(100, high * 15 + medium * 5 + stall_total))
        risk_note = ""
        m_risk_active = len(risks)
        m_risk_high = high

    # ---- Scope ----
    if not changes:
        # 从 signals 推断变更
        change_events = [e for e in events if e.get("baseline") == "scope" and "change" in e.get("type", "")]
        if not change_events:
            scope = 90
            scope_note = "估算（无变更数据）"
        else:
            approved_days = sum(
                e.get("payload", {}).get("impact_days", 0)
                for e in change_events
                if "approved" in e.get("type", "")
            )
            m_approved_days = approved_days
            if not contract_days:
                scope = 90
                scope_note = "估算（缺合同人天）"
            else:
                scope = int(_clamp(100 - approved_days / contract_days * 300, 0, 100))
                scope_note = ""
    else:
        approved_days = sum(
            c.get("impact_days", 0) for c in changes if c.get("status") == "已批准"
        )
        m_approved_days = approved_days
        if not contract_days:
            scope = 90
            scope_note = "估算（缺合同人天）"
        else:
            scope = int(_clamp(100 - approved_days / contract_days * 300, 0, 100))
            scope_note = ""

    # ---- Comm ----
    if isinstance(weekly_reports, dict):
        wr_count = weekly_reports.get("count", 0)
        wr_on_time = weekly_reports.get("on_time_rate")
    else:
        wr_count = len(weekly_reports)
        wr_on_time = None

    if wr_count == 0:
        comm = 70
        comm_note = "估算（无周报）"
    else:
        if wr_on_time is not None:
            comm = int(wr_on_time * 100)
            comm_note = ""
        else:
            if start_date and today and len(start_date) >= 10 and len(today) >= 10:
                try:
                    from datetime import date
                    sy, sm, sd = int(start_date[:4]), int(start_date[5:7]), int(start_date[8:10])
                    ty, tm, td = int(today[:4]), int(today[5:7]), int(today[8:10])
                    weeks = max(1, (date(ty, tm, td) - date(sy, sm, sd)).days // 7)
                    on_time_rate = min(1.0, wr_count / weeks)
                    comm = int(on_time_rate * 100)
                    comm_note = ""
                except Exception:
                    comm = 70
                    comm_note = "估算（日期解析失败）"
            else:
                comm = 70
                comm_note = "估算（缺开始日期）"

    # ---- Aggregate ----
    health = int(
        0.25 * schedule
        + 0.20 * cost
        + 0.20 * quality
        + 0.20 * risk_score
        + 0.10 * scope
        + 0.05 * comm
    )

    if health >= 80:
        rag = "green"
    elif health >= 60:
        rag = "yellow"
    else:
        rag = "red"

    notes = {}
    if schedule_note:
        notes["schedule"] = schedule_note
    if cost_note:
        notes["cost"] = cost_note
    if quality_note:
        notes["quality"] = quality_note
    if risk_note:
        notes["risk"] = risk_note
    if scope_note:
        notes["scope"] = scope_note
    if comm_note:
        notes["comm"] = comm_note

    return {
        "score": health,
        "rag": rag,
        "subscores": {
            "schedule": schedule,
            "cost": cost,
            "quality": quality,
            "risk": risk_score,
            "scope": scope,
            "comm": comm,
        },
        "metrics": {
            "completed_phases": completed,
            "total_phases": total_phases,
            "spi": round(spi, 2),
            "approved_change_days": m_approved_days,
            "defect_active": m_defect_active,
            "defect_critical": m_defect_critical,
            "risk_active": m_risk_active,
            "risk_high": m_risk_high,
            "contract_days": contract_days,
            "consumed_days": m_consumed_days,
            "cost_pct": m_cost_pct,
        },
        "trend": None,
        "notes": notes,
        "estimated": sorted(notes.keys()),
    }
