"""CLI entry — §9.2 v3.0-DESIGN.md"""
from __future__ import annotations
import argparse
import sys
from datetime import date, datetime
from pathlib import Path

from . import aggregator, health_score, anomaly_detector, renderer, history

# 看板输出固定文件名：每次生成覆盖，不带日期。
# 生成时间在看板 footer 内已标注，无需进文件名；固定名避免「带日期 = 像过期残留」。
OUTPUT_FILENAME = "项目看板.html"


def _append_dashboard_log(claude_md: Path, filename: str) -> None:
    """在 CLAUDE.md 的 ## 活动日志 末尾追加记录，失败静默。"""
    try:
        text = claude_md.read_text(encoding="utf-8")
        lines = text.splitlines(keepends=True)

        section_idx = None
        for i, ln in enumerate(lines):
            if ln.rstrip("\n\r") == "## 活动日志":
                section_idx = i
                break

        if section_idx is None:
            return

        # 找 section 结束位置（下一个 ## 开头或文件末尾）
        insert_idx = len(lines)
        for i in range(section_idx + 1, len(lines)):
            if lines[i].startswith("## "):
                insert_idx = i
                break

        # 跳过末尾空行
        while insert_idx > section_idx + 1 and lines[insert_idx - 1].strip() == "":
            insert_idx -= 1

        now = datetime.now().strftime("%Y-%m-%d %H:%M")
        log_line = f"- {now} ｜ /project-dashboard ｜ 生成项目看板 {filename}\n"

        new_lines = lines[:insert_idx]
        if insert_idx > 0 and not new_lines[-1].endswith("\n"):
            new_lines[-1] += "\n"
        new_lines.append(log_line)
        new_lines.extend(lines[insert_idx:])

        claude_md.write_text("".join(new_lines), encoding="utf-8")
    except Exception:
        pass


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="生成项目看板 HTML")
    parser.add_argument("--project", required=True, help="项目根目录路径")
    parser.add_argument("--output", required=True, help="HTML 输出目录")
    args = parser.parse_args(argv)

    project_root = Path(args.project).resolve()
    output_dir = Path(args.output).resolve()

    # 前置检查：CLAUDE.md 必须存在
    claude_md = project_root / "CLAUDE.md"
    if not claude_md.exists():
        print("❌ 当前目录不是已初始化的项目。看板需要先运行 /project-init")
        return 1

    # 聚合数据
    try:
        snapshot = aggregator.collect(project_root)
    except Exception as e:
        print(f"⚠ 数据聚合出错: {e}")
        snapshot = {
            "project_name": "未命名项目",
            "meta": {"generated_at": date.today().isoformat()},
            "phases": [], "milestones": [], "risks": [], "changes": [],
            "weekly_reports": {"count": 0}, "signals": {"events": []},
            "activity_log": [], "stakeholders": [], "meetings_this_week": [],
        }

    # 分析
    health = health_score.calculate(snapshot)
    anomalies = anomaly_detector.detect(snapshot)

    # 健康度历史：先记录当次快照，再读最近趋势（F1 趋势线）
    today = snapshot.get("meta", {}).get("generated_at", date.today().isoformat())[:10]
    history.record(project_root, today, health)
    trend = history.read_trend(project_root, limit=8)

    analysis = {"health": health, "anomalies": anomalies, "trend": trend}

    # 渲染
    html = renderer.render(snapshot, analysis)

    # 输出路径（固定文件名，覆盖式）
    filename = OUTPUT_FILENAME
    output_dir.mkdir(parents=True, exist_ok=True)
    output_path = output_dir / filename

    try:
        with open(output_path, "w", encoding="utf-8") as f:
            f.write(html)
    except Exception as e:
        print(f"❌ 写入失败: {e}")
        return 2

    # 追加活动日志
    _append_dashboard_log(claude_md, filename)

    # 输出结果
    rag_emoji = {"green": "🟢", "yellow": "🟡", "red": "🔴"}.get(health["rag"], "⚪")
    rag_word = {"green": "健康", "yellow": "关注", "red": "告警"}.get(health["rag"], "未知")
    focus_count = len(anomaly_detector.top3(anomalies))

    print("✅ 项目看板已生成")
    print(f"📄 {output_path}")
    print(f"{rag_emoji} 健康度 {health['score']} / 100（{rag_word}）")
    print(f"⚠ 今日聚焦：{focus_count} 项")

    return 0


if __name__ == "__main__":
    sys.exit(main())
