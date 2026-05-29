#!/bin/bash
# skill-updater pack-suite — 将套件打包为 Kingdee Skill Hub 上传用 ZIP
# Usage: bash pack-suite.sh [output_dir]

set -e

# 定位项目根目录（skill-updater 的 4 层上级）
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
OUTPUT_DIR="${1:-$PROJECT_ROOT}"
VERSION="${2:-v0.1.0}"

ZIP_NAME="kingdee-implementation-suite-${VERSION}.zip"
ZIP_PATH="$OUTPUT_DIR/$ZIP_NAME"
TMPDIR=$(mktemp -d)
PACKDIR="$TMPDIR/kingdee-erp-suite"

echo "📦 打包套件 v${VERSION}..."
echo "   项目根目录: $PROJECT_ROOT"

# 创建临时打包目录
mkdir -p "$PACKDIR"

# 1. 复制 SKILL.md（从 CLAUDE.md 生成，加 frontmatter）
cat > "$PACKDIR/SKILL.md" << 'FRONTMATTER'
---
name: kingdee-implementation-suite
description: 以金蝶实施方法论V10.0为基础，为交付顾问构建的AI技能套件。采用1个主Skill+22个子Skill架构，核心预装、其余懒加载——初始化轻量，首次触发时自动从Hub下载安装。覆盖项目启动→需求调研→方案设计→构建→测试→上线→验收全流程，打造持续迭代的高效、高质、趁手的AI技能包。融合改造自GitHub开源社区：serejaris/ris-claude-code（项目启动）、legalopsconsulting/lpm-skills（周报&干系人沟通）、WayneZhon/KingDee-PPT-Skill（PPT生成）等项目。触发词："启动项目" "生成周报" "蓝图设计" "做PPT" "会议纪要" "项目看板"
---

FRONTMATTER
# Append original CLAUDE.md content (skip first H1 title to avoid duplicate)
tail -n +2 "$PROJECT_ROOT/CLAUDE.md" >> "$PACKDIR/SKILL.md"

# 2. 复制 README.md
cp "$PROJECT_ROOT/README.md" "$PACKDIR/"

# 3. 复制 .claude/skills/ 子 skill（排除非 ERP 的通用写作/开发 skill）
EXCLUDE_SKILLS="brainstorming subagent-driven-development test-driven-development verification-before-completion writing-plans writing-skills"
mkdir -p "$PACKDIR/.claude/skills"
for skill_dir in "$PROJECT_ROOT/.claude/skills/"*/; do
    skill_name=$(basename "$skill_dir")
    if echo "$EXCLUDE_SKILLS" | grep -qw "$skill_name"; then
        echo "   ✕ $skill_name（非ERP，已排除）"
        continue
    fi
    echo "   → $skill_name"
    cp -r "$skill_dir" "$PACKDIR/.claude/skills/$skill_name"
done

# 4. 排除不需要的文件
echo "   🧹 清理干扰文件..."
find "$PACKDIR" -name ".DS_Store" -delete
find "$PACKDIR" -name "~$*" -delete 2>/dev/null || true
# 排除 settings.local.json（含敏感权限配置）
rm -f "$PACKDIR/.claude/settings.local.json" 2>/dev/null || true

# 5. 统计子技能数量（必须在打包前计数）
SKILL_COUNT=$(find "$PACKDIR/.claude/skills" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')

# 6. 打包（先删除旧 ZIP，zip 默认追加而非覆盖）
rm -f "$ZIP_PATH"
cd "$TMPDIR"
zip -rq "$ZIP_PATH" "kingdee-erp-suite" -x "*.git*"

# 7. 清理
rm -rf "$TMPDIR"

# 8. 报告
FILE_SIZE=$(ls -lh "$ZIP_PATH" | awk '{print $5}')
ZIP_SIZE_BYTES=$(stat -f%z "$ZIP_PATH" 2>/dev/null || stat -c%s "$ZIP_PATH" 2>/dev/null)

echo ""
echo "✅ 打包完成！"
echo "   📄 $ZIP_NAME"
echo "   📏 $FILE_SIZE"
echo "   📊 包含 $SKILL_COUNT 个子技能"
echo ""
echo "💡 上传到 Kingdee Skill Hub："
echo "   1. 打开 https://skills.kingdee.com/upload"
echo "   2. 拖入 $ZIP_PATH"
echo "   3. 填写分类/标签"
