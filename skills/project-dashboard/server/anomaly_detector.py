"""Anomaly detector — §5 v3.0-DESIGN.md"""
from __future__ import annotations
from dataclasses import dataclass
from typing import Any


@dataclass
class Anomaly:
    id: str
    rule_id: str
    severity: str  # high / medium / low
    title: str
    detail: str
    action_label: str
    action_cmd: str
    baseline: str


def _days_between(d1: str, d2: str) -> int:
    """两个 YYYY-MM-DD 字符串之间的天数差（d2 - d1）"""
    try:
        from datetime import date
        y1, m1, day1 = int(d1[:4]), int(d1[5:7]), int(d1[8:10])
        y2, m2, day2 = int(d2[:4]), int(d2[5:7]), int(d2[8:10])
        return (date(y2, m2, day2) - date(y1, m1, day1)).days
    except Exception:
        return 0


def _today_from_snapshot(snapshot: dict[str, Any]) -> str:
    today = snapshot.get("today", "")
    if not today:
        meta = snapshot.get("meta", {})
        today = meta.get("generated_at", "")[:10]
    if not today:
        from datetime import date as _date
        today = _date.today().isoformat()
    return today


def detect(snapshot: dict[str, Any]) -> list[Anomaly]:
    """
    输入 snapshot，输出按 severity 降序排列的 Anomaly 列表。
    """
    anomalies: list[Anomaly] = []
    today = _today_from_snapshot(snapshot)

    risks = snapshot.get("risks", [])
    changes = snapshot.get("changes", [])
    milestones = snapshot.get("milestones", [])
    weekly_reports = snapshot.get("weekly_reports", {})
    signals = snapshot.get("signals", {})
    events = signals.get("events", []) if isinstance(signals, dict) else []

    start_date = snapshot.get("start_date", "")

    # ---- R-STALL: 风险 updated_at 距今 > 3 天 ----
    # 优先用 risks 列表的 updated_at；若不存在，从 signals risk.created 推断
    for idx, r in enumerate(risks):
        updated = r.get("updated_at") or r.get("last_update")
        if updated and updated != "-":
            stall = _days_between(updated, today)
            if stall > 3:
                anomalies.append(Anomaly(
                    id=f"R-STALL-{idx}",
                    rule_id="R-STALL",
                    severity="high",
                    title=f"风险 {r.get('risk_id', r.get('id', 'R-???'))} 已停滞 {stall} 天",
                    detail=f"{r.get('title', '无标题')}，最后更新 {updated}",
                    action_label="立即处理",
                    action_cmd=f"/risk-manager 更新 {r.get('risk_id', r.get('id', ''))}",
                    baseline="risk",
                ))
    # 从 signals 补充（若 risks 列表为空）
    if not risks:
        for idx, e in enumerate(events):
            if e.get("baseline") == "risk" and e.get("type") == "risk.created":
                ts = e.get("ts", "")[:10]
                if ts:
                    stall = _days_between(ts, today)
                    if stall > 3:
                        payload = e.get("payload", {})
                        rid = e.get("id", f"R-{idx}")
                        anomalies.append(Anomaly(
                            id=f"R-STALL-S{idx}",
                            rule_id="R-STALL",
                            severity="high",
                            title=f"风险 {rid} 已停滞 {stall} 天",
                            detail=f"{payload.get('title', '无标题')}，创建于 {ts}",
                            action_label="立即处理",
                            action_cmd=f"/risk-manager 更新 {rid}",
                            baseline="risk",
                        ))

    # ---- C-PENDING: 变更 submitted > 2 天未审批 ----
    for idx, c in enumerate(changes):
        if c.get("status") == "submitted":
            submitted = c.get("submitted_at")
            if submitted and submitted != "-":
                pending = _days_between(submitted, today)
                if pending > 2:
                    anomalies.append(Anomaly(
                        id=f"C-PENDING-{idx}",
                        rule_id="C-PENDING",
                        severity="high",
                        title=f"变更 {c.get('pcr_id', 'PCR-???')} 待审批 {pending} 天",
                        detail=f"{c.get('title', '无标题')}，提交于 {submitted}",
                        action_label="打开审批",
                        action_cmd=f"/change-manager 审批 {c.get('pcr_id', '')}",
                        baseline="scope",
                    ))
    # 从 signals 补充
    for idx, e in enumerate(events):
        if e.get("baseline") == "scope" and e.get("type") == "change.submitted":
            ts = e.get("ts", "")[:10]
            if ts:
                pending = _days_between(ts, today)
                if pending > 2:
                    payload = e.get("payload", {})
                    pcr = e.get("id", f"PCR-{idx}")
                    anomalies.append(Anomaly(
                        id=f"C-PENDING-S{idx}",
                        rule_id="C-PENDING",
                        severity="high",
                        title=f"变更 {pcr} 待审批 {pending} 天",
                        detail=f"提交于 {ts}",
                        action_label="打开审批",
                        action_cmd=f"/change-manager 审批 {pcr}",
                        baseline="scope",
                    ))

    # ---- D-DIVERGE: 近 3 天缺陷新增 > 关闭 ----
    created_recent = 0
    closed_recent = 0
    defects = snapshot.get("defects", [])
    for d in defects:
        created = d.get("created_at", "")
        closed = d.get("closed_at", "")
        if created and _days_between(created, today) <= 3:
            created_recent += 1
        if closed and _days_between(closed, today) <= 3:
            closed_recent += 1
    # 从 signals 读取
    for e in events:
        if e.get("baseline") == "quality":
            ts = e.get("ts", "")
            if ts and _days_between(ts[:10], today) <= 3:
                if e.get("type") == "defect.created":
                    created_recent += 1
                elif e.get("type") == "defect.closed":
                    closed_recent += 1
    if created_recent > closed_recent:
        anomalies.append(Anomaly(
            id="D-DIVERGE-0",
            rule_id="D-DIVERGE",
            severity="high",
            title=f"近 3 天缺陷新增 {created_recent} 个 > 关闭 {closed_recent} 个",
            detail="缺陷未收敛，建议重点关注测试进度",
            action_label="查看缺陷收敛",
            action_cmd="/test-manager 查看缺陷收敛",
            baseline="quality",
        ))

    # ---- M-NEAR: 里程碑距今 ≤ 7 天且 pending ----
    for idx, m in enumerate(milestones):
        status = m.get("status", "")
        if status in ("pending", "未开始"):
            plan_date = m.get("plan_date") or m.get("planned_date")
            if plan_date and plan_date != "-":
                days_to = _days_between(today, plan_date)
                if 0 <= days_to <= 7:
                    anomalies.append(Anomaly(
                        id=f"M-NEAR-{idx}",
                        rule_id="M-NEAR",
                        severity="medium",
                        title=f"里程碑「{m.get('name', '未知')}」距今 {days_to} 天",
                        detail=f"计划日期 {plan_date}，尚未完成",
                        action_label="起草周报",
                        action_cmd="/weekly-report 起草本周周报",
                        baseline="schedule",
                    ))

    # ---- W-LATE: 周报漏期 ----
    if start_date and start_date != "-":
        try:
            from datetime import date
            sy, sm, sd = int(start_date[:4]), int(start_date[5:7]), int(start_date[8:10])
            ty, tm, td = int(today[:4]), int(today[5:7]), int(today[8:10])
            weeks_expected = max(1, (date(ty, tm, td) - date(sy, sm, sd)).days // 7)
            if isinstance(weekly_reports, dict):
                weeks_actual = weekly_reports.get("count", 0)
            else:
                weeks_actual = len(weekly_reports)
            if weeks_actual < weeks_expected:
                anomalies.append(Anomaly(
                    id="W-LATE-0",
                    rule_id="W-LATE",
                    severity="medium",
                    title=f"周报漏期：应有 {weeks_expected} 期，实际 {weeks_actual} 期",
                    detail="建议补录上周周报",
                    action_label="生成上周周报",
                    action_cmd="/weekly-report 生成上周周报",
                    baseline="comm",
                ))
        except Exception:
            pass

    # 排序：high > medium > low
    severity_rank = {"high": 3, "medium": 2, "low": 1}
    anomalies.sort(key=lambda a: severity_rank.get(a.severity, 0), reverse=True)
    return anomalies


def top3(anomalies: list[Anomaly]) -> list[Anomaly]:
    """取前 3 条；不足 3 条时返回全部（调用方负责展示兜底文案）"""
    return anomalies[:3]
