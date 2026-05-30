#!/usr/bin/env python3
"""平台抽象适配器 — Claude Code实现版本

调用约束：
- 作为包内模块导入时：from platform_adapter import get_adapter
- 直接执行时：python platform_adapter.py（会触发绝对路径导入fallback）
- 非包上下文导入时：将脚本所在目录加入sys.path后导入
"""
import json
import shutil
import subprocess
import sys
from datetime import datetime
from pathlib import Path

# 相对导入可能在非包上下文崩溃，使用try/except + 绝对路径fallback
try:
    from .platform import detect_platform, is_qoderwork, is_claude_code
except ImportError:
    _this_dir = Path(__file__).resolve().parent
    if str(_this_dir) not in sys.path:
        sys.path.insert(0, str(_this_dir))
    from platform import detect_platform, is_qoderwork, is_claude_code


class PlatformAdapter:
    """统一抽象接口，底层根据平台路由到具体实现"""

    def __init__(self):
        self.platform = detect_platform()

    # ========== 用户交互 ==========
    def user_choice(self, question: str, options: list, multi_select=False):
        """
        展示选项并获取用户选择。
        Claude Code: 打印文本块，读取stdin输入
        QoderWork:  调用AskUserQuestion（后续接入）
        """
        if self.platform == "claude-code":
            return self._cc_user_choice(question, options, multi_select)
        else:
            return self._cc_user_choice(question, options, multi_select)

    def _cc_user_choice(self, question, options, multi_select):
        print(f"\n📋 {question}")
        labels = "ABCDEFGHIJ"[:len(options)]
        for label, opt in zip(labels, options):
            print(f"  {label}. {opt}")
        print(f"\n输入选项（如{'多选以逗号分隔' if multi_select else 'A/B/C'}）：")
        return None  # 由调用方处理交互逻辑

    # ========== 文件展示 ==========
    def show_files(self, file_paths: list, title="交付文件"):
        """
        向用户展示生成的文件。
        Claude Code: 打印Markdown链接
        QoderWork:  调用present_files（后续接入）
        """
        print(f"\n📁 {title}")
        for fp in file_paths:
            print(f"  - {fp}")
        return True

    # ========== 看板渲染 ==========
    def render_dashboard(self, html_content: str, output_path: str, title="项目看板"):
        """
        渲染项目看板。
        Claude Code: 生成HTML文件，尝试用open/start/xdg-open打开
        QoderWork:  调用qoder_show_widget（后续接入）
        """
        with open(output_path, "w", encoding="utf-8") as f:
            f.write(html_content)
        print(f"✅ 看板已保存: {output_path}")

        if self.platform == "claude-code":
            try:
                if sys.platform == "darwin":
                    subprocess.run(["open", output_path], check=False)
                elif sys.platform == "win32":
                    subprocess.run(["start", output_path], shell=True, check=False)
                else:
                    subprocess.run(["xdg-open", output_path], check=False)
            except Exception:
                pass
        return True

    # ========== 定时任务 ==========
    def schedule_cron(self, name: str, schedule_type: str, schedule_value: str, message: str):
        """
        创建定时任务。
        Claude Code: 写入 scheduled_tasks.json
        QoderWork:  调用qoder_cron（后续接入）
        """
        if self.platform == "claude-code":
            return self._cc_schedule_cron(name, schedule_type, schedule_value, message)
        else:
            print(f"⏰ 定时任务配置（请在QoderWork中手动创建）:")
            print(f"   名称: {name}")
            print(f"   类型: {schedule_type}")
            print(f"   值: {schedule_value}")
            print(f"   内容: {message}")
            return False

    def _cc_schedule_cron(self, name, schedule_type, schedule_value, message):
        """Claude Code: 写入 .claude/scheduled_tasks.json"""
        tasks_file = Path.home() / ".claude" / "scheduled_tasks.json"
        tasks = []
        if tasks_file.exists():
            try:
                tasks = json.loads(tasks_file.read_text())
            except Exception:
                tasks = []
        tasks.append({
            "name": name,
            "type": schedule_type,
            "value": schedule_value,
            "message": message,
            "enabled": True
        })
        tasks_file.write_text(json.dumps(tasks, indent=2, ensure_ascii=False))
        print(f"✅ 定时任务已写入: {tasks_file}")
        return True

    # ========== 文件安全操作 ==========
    def safe_delete(self, file_path: str):
        """安全删除（移入系统Trash而非永久删除）

        macOS: 使用 osascript 调用 Finder，支持跨卷移动、保留元数据（"放回原处"可用）
        Windows: PowerShell 调用 FileSystem 删除到回收站
        Linux: 优先 gio trash，次选 trash-cli，最终 fallback 到临时目录
        """
        fp = Path(file_path)
        if not fp.exists():
            return True

        moved = False

        try:
            if sys.platform == "darwin":
                subprocess.run([
                    "osascript", "-e",
                    f'tell application "Finder" to delete POSIX file "{fp.resolve()}"'
                ], check=True, capture_output=True, timeout=10)
                moved = True
            elif sys.platform == "win32":
                subprocess.run([
                    "powershell.exe", "-Command",
                    f"Add-Type -AssemblyName Microsoft.VisualBasic; "
                    f"[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile('{fp.resolve()}', 'OnlyErrorDialogs', 'SendToRecycleBin')"
                ], check=True, timeout=10)
                moved = True
            else:
                result = subprocess.run(["gio", "trash", str(fp)], capture_output=True, timeout=10)
                if result.returncode == 0:
                    moved = True
                else:
                    result = subprocess.run(["trash", str(fp)], capture_output=True, timeout=10)
                    if result.returncode == 0:
                        moved = True
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired, FileNotFoundError):
            pass  # 系统工具不可用，进入最终fallback

        if not moved:
            trash_dir = Path.home() / ".qoderwork" / ".trash"
            trash_dir.mkdir(parents=True, exist_ok=True)
            dest = trash_dir / fp.name
            counter = 1
            while dest.exists():
                dest = trash_dir / f"{fp.stem}_{counter}{fp.suffix}"
                counter += 1
            shutil.move(str(fp), str(dest))
            print(f"⚠️ 系统Trash不可用，已移动到临时回收站: {dest}")

        return True

    def backup_file(self, file_path: str) -> str:
        """创建文件备份，返回备份路径"""
        fp = Path(file_path)
        if not fp.exists():
            return None
        backup_name = f"{fp.name}.{datetime.now().strftime('%Y%m%d_%H%M%S')}.bak"
        backup_path = fp.parent / backup_name
        shutil.copy2(str(fp), str(backup_path))
        return str(backup_path)

    # ========== IM/通知（纯占位，QoderWork侧接入）==========
    def notify_im(self, text: str, chat_name: str = None):
        """发送IM通知。Claude Code: 无操作。QoderWork: 后续接入。"""
        if self.platform == "claude-code":
            print(f"💬 IM通知（Claude Code不支持自动发送）: {text[:80]}...")
        return False

    def create_apple_note(self, title: str, body: str):
        """创建Apple Note。Claude Code: 无操作。"""
        return False

    def create_apple_reminder(self, title: str, due_date: str = None):
        """创建Apple Reminder。Claude Code: 无操作。"""
        return False

    def create_apple_calendar_event(self, title: str, start: str, end: str):
        """创建Apple Calendar事件。Claude Code: 无操作。"""
        return False

    def create_apple_mail_draft(self, to: str, subject: str, body: str, attachments: list = None):
        """创建Apple Mail草稿。Claude Code: 无操作。"""
        return False


# 全局单例
_adapter = None


def get_adapter() -> PlatformAdapter:
    global _adapter
    if _adapter is None:
        _adapter = PlatformAdapter()
    return _adapter
