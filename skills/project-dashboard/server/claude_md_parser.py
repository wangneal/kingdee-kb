"""
纯正则解析 CLAUDE.md §3.3 描述的 sections。
不引入 markdown / pyyaml 等第三方库。
"""

import re
from pathlib import Path
from typing import Optional


def _find_section_lines(lines: list[str], header: str) -> list[str]:
    """
    找到某 section 下的所有行，直到下一个同级或更高级标题。
    header 如 '## 项目配置' 或 '### 当前阶段'。
    """
    level = len(header) - len(header.lstrip("#"))
    in_section = False
    result = []
    for line in lines:
        stripped = line.strip()
        if stripped.startswith(header):
            in_section = True
            continue
        if in_section:
            if stripped.startswith("#"):
                new_level = len(stripped) - len(stripped.lstrip("#"))
                if new_level <= level:
                    break
            result.append(line)
    return result


def _parse_markdown_table(lines: list[str]) -> list[dict]:
    """
    解析 markdown 表格，返回 list[dict]（首行为表头）。
    容错：忽略分隔行、忽略空单元格扩展、处理列数不足。
    """
    raw_rows: list[list[str]] = []
    for line in lines:
        line = line.strip()
        if not line.startswith("|"):
            continue
        # 跳过分隔行 |---|---|
        if re.match(r'^\|[-\s|:]+\|$', line):
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if cells and all(not c for c in cells):
            continue
        raw_rows.append(cells)

    if len(raw_rows) < 2:
        return []

    headers = raw_rows[0]
    result = []
    for row in raw_rows[1:]:
        row_dict = {}
        for i, header in enumerate(headers):
            row_dict[header] = row[i] if i < len(row) else ""
        result.append(row_dict)
    return result


def _parse_activity_log(lines: list[str]) -> list[dict]:
    """
    解析活动日志列表项。
    格式：- 2026-05-17 14:30 ｜ /risk-manager ｜ 新增 R-007 ...
    """
    logs = []
    for line in lines:
        line = line.strip()
        if not line.startswith("- ") and not line.startswith("* "):
            continue
        content = line[2:].strip()
        if not content:
            continue
        # 用全角/半角竖线分割
        parts = [p.strip() for p in re.split(r'[｜|]', content)]
        if len(parts) >= 3:
            logs.append({
                "ts": parts[0],
                "source": parts[1].lstrip("/").strip(),
                "summary": "｜".join(parts[2:]).strip(),
            })
        else:
            logs.append({
                "ts": parts[0] if parts else "",
                "source": "",
                "summary": content,
            })
    return logs


def parse_claude_md(path: Path) -> dict:
    """
    解析 CLAUDE.md，返回结构化 dict。
    缺失 section 时对应字段为空/None，永不抛异常。
    """
    result = {
        "project_name": None,
        "pm_name": None,
        "client_pm": None,
        "product_type": None,
        "contract_amount": None,
        "contract_days": None,
        "start_date": None,
        "plan_go_live": None,
        "phases": [],
        "milestones": [],
        "activity_log": [],
    }

    try:
        text = path.read_text(encoding="utf-8")
    except Exception:
        return result

    lines = text.splitlines()

    # ── 项目配置 ──
    config_lines = _find_section_lines(lines, "## 项目配置")
    config_rows = _parse_markdown_table(config_lines)
    for row in config_rows:
        key = row.get("字段", "")
        val = row.get("值", "")
        if not key or not val:
            continue
        if key == "项目名称":
            result["project_name"] = val
        elif key == "金蝶PM":
            result["pm_name"] = val
        elif key == "客户PM":
            result["client_pm"] = val
        elif key == "产品类型":
            result["product_type"] = val
        elif key == "合同金额":
            result["contract_amount"] = val
        elif key == "合同人天":
            # 尝试解析为整数
            try:
                result["contract_days"] = int(re.sub(r"[^\d]", "", val))
            except Exception:
                result["contract_days"] = None
        elif key == "开始日期":
            result["start_date"] = val
        elif key == "计划上线":
            result["plan_go_live"] = val

    # ── 项目状态 ──
    status_lines = _find_section_lines(lines, "## 项目状态")

    # 当前阶段
    phase_lines = _find_section_lines(status_lines, "### 当前阶段")
    phase_rows = _parse_markdown_table(phase_lines)
    for row in phase_rows:
        phase = {
            "name": row.get("阶段", "").strip(),
            "status": row.get("状态", "").strip(),
            "planned_start": row.get("计划开始", "").strip() or None,
            "planned_end": row.get("计划结束", "").strip() or None,
            "actual_start": row.get("实际开始", "").strip() or None,
            "actual_end": row.get("实际结束", "").strip() or None,
        }
        if phase["name"]:
            result["phases"].append(phase)

    # 关键里程碑
    milestone_lines = _find_section_lines(status_lines, "### 关键里程碑")
    milestone_rows = _parse_markdown_table(milestone_lines)
    for row in milestone_rows:
        milestone = {
            "name": row.get("里程碑", "").strip(),
            "planned_date": row.get("计划日期", "").strip() or None,
            "actual_date": row.get("实际日期", "").strip() or None,
            "status": row.get("状态", "").strip() or "pending",
        }
        if milestone["name"]:
            result["milestones"].append(milestone)

    # ── 活动日志 ──
    log_lines = _find_section_lines(lines, "## 活动日志")
    result["activity_log"] = _parse_activity_log(log_lines)

    return result
