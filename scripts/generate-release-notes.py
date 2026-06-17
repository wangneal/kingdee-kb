import sys
import subprocess
import re
import os

def get_commits(current_tag):
    try:
        # Get all tags sorted by version descending
        tags_raw = subprocess.check_output(["git", "tag", "--sort=-v:refname"], encoding="utf-8")
        tags = [t.strip() for t in tags_raw.strip().split("\n") if t.strip()]
    except Exception as e:
        print(f"Error listing tags: {e}")
        tags = []

    print(f"Available tags: {tags}")
    
    prev_tag = None
    if current_tag in tags:
        idx = tags.index(current_tag)
        if idx + 1 < len(tags):
            prev_tag = tags[idx + 1]
    
    # If current_tag is not yet in local tags, try to find the tag before HEAD
    if not prev_tag and tags:
        for t in tags:
            if t != current_tag:
                prev_tag = t
                break

    git_range = f"{prev_tag}..{current_tag}" if prev_tag else current_tag
    print(f"Comparing range: {git_range}")
    try:
        log_raw = subprocess.check_output([
            "git", "log", git_range, "--pretty=format:%h|%s"
        ], encoding="utf-8")
        commits = [line.strip().split("|", 1) for line in log_raw.strip().split("\n") if line.strip()]
    except Exception as e:
        print(f"Error getting git log: {e}")
        commits = []

    return commits

def parse_commits(commits, repo_url):
    categories = {
        "feat": ("🚀 Features", []),
        "fix": ("🐛 Bug Fixes", []),
        "perf": ("⚡ Performance", []),
        "refactor": ("♻️ Code Refactoring", []),
        "docs": ("📝 Documentation", []),
        "ci": ("🔧 CI/CD & Build", []),
        "build": ("🔧 CI/CD & Build", []),
        "chore": ("🔧 CI/CD & Build", []),
        "style": ("🎨 Style & Formatting", []),
        "test": ("🧪 Tests", []),
        "other": ("📦 Other Changes", [])
    }

    pattern = re.compile(r"^(\w+)(?:\(([^)]+)\))?\s*:\s*(.*)$")

    for item in commits:
        if len(item) != 2:
            continue
        commit_hash, subject = item
        match = pattern.match(subject)
        commit_link = f"[{commit_hash}]({repo_url}/commit/{commit_hash})" if repo_url else f"`{commit_hash}`"
        
        if match:
            type_name = match.group(1).lower()
            scope = match.group(2)
            desc = match.group(3)
            
            if type_name not in categories:
                cat_key = "other"
            else:
                cat_key = type_name
                
            scope_str = f"**{scope}**: " if scope else ""
            entry = f"- {scope_str}{desc} ({commit_link})"
            categories[cat_key][1].append(entry)
        else:
            entry = f"- {subject} ({commit_link})"
            categories["other"][1].append(entry)

    # Merge build, chore, ci under "🔧 CI/CD & Build"
    ci_build_chore = []
    for k in ["ci", "build", "chore"]:
        ci_build_chore.extend(categories[k][1])
    
    merged_categories = {
        "feat": categories["feat"],
        "fix": categories["fix"],
        "perf": categories["perf"],
        "refactor": categories["refactor"],
        "docs": categories["docs"],
        "ci_build_chore": ("🔧 CI/CD & Build", ci_build_chore),
        "style": categories["style"],
        "test": categories["test"],
        "other": categories["other"]
    }

    return merged_categories

def generate_notes(version, repo_url):
    commits = get_commits(version)
    parsed = parse_commits(commits, repo_url)

    changelog_md = ""
    has_changes = False
    
    # Define display order
    order = ["feat", "fix", "perf", "refactor", "docs", "style", "test", "ci_build_chore", "other"]
    
    for key in order:
        title, items = parsed[key]
        if items:
            has_changes = True
            changelog_md += f"### {title}\n\n"
            for item in items:
                changelog_md += f"{item}\n"
            changelog_md += "\n"

    if not has_changes:
        changelog_md = "*No changes recorded.*\n\n"

    notes = f"""金蝶 ERP 实施知识库桌面应用 — AI 驱动的调研助手、风险把控、文档生成。

---

## 📋 更新日志 (Changelog)

{changelog_md}
---

## 📦 下载说明

**Windows**
- `.msi` / `.exe`：标准安装包（推荐，含卸载程序、开始菜单快捷方式）
- `-portable.zip`：绿色版（无需安装，解压即用，适合便携场景）

**macOS**
- `.dmg`（ARM 用于 Apple Silicon，x64 用于 Intel）

**Linux**
- `.AppImage`：免安装，`chmod +x` 后直接运行
- `.deb`：Debian / Ubuntu 安装包

### 首次使用
1. 启动应用
2. 进入「设置」配置 LLM API Key
3. 在「知识库」中导入文档
4. 在「AI 助手」中开始提问
"""
    return notes

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python3 generate-release-notes.py <version> <repo_url>")
        sys.exit(1)
        
    version = sys.argv[1]
    repo_url = sys.argv[2]
    
    notes = generate_notes(version, repo_url)
    
    with open("release-notes.md", "w", encoding="utf-8") as f:
        f.write(notes)
    
    print("Release notes written to release-notes.md")
