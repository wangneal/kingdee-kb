#!/bin/bash
# ============================================================
# scan-files.sh — 金蝶ERP项目文件变更扫描器 v2.0
#
# 检测指定时间范围内的文件变更，附加 git author 信息。
# 支持快速模式（仅文件名，不读内容）。
#
# 用法:
#   ./scan-files.sh <start_date> <end_date> [project_root] [flags]
#
# Flags:
#   --include-uncommitted    同时输出未提交的本地变更（Git 模式）
#   --quick                  快速模式：仅输出文件清单，供 AI 基于文件名推断
#   --include-authors        输出 git author 信息（Git 模式）
#
# 输出: JSON 到 stdout
# ============================================================

set -euo pipefail

START_DATE="${1:?Usage: $0 <start_date> <end_date> [project_root] [flags]}"
END_DATE="${2:?}"
PROJECT_ROOT="${3:-.}"

INCLUDE_UNCOMMITTED=false
QUICK_MODE=false
INCLUDE_AUTHORS=false

shift 3 2>/dev/null || true
for arg in "$@"; do
    case "$arg" in
        --include-uncommitted) INCLUDE_UNCOMMITTED=true ;;
        --quick) QUICK_MODE=true ;;
        --include-authors) INCLUDE_AUTHORS=true ;;
    esac
done

cd "$PROJECT_ROOT"

EXCLUDE_DIRS=('.git' '.claude' '被裁剪文件' '待整理' 'node_modules' '__pycache__')

is_git_repo() {
    [ -d ".git" ] && git rev-parse --git-dir >/dev/null 2>&1
}

should_exclude() {
    local path="$1"
    for d in "${EXCLUDE_DIRS[@]}"; do
        [[ "$path" == "$d"* || "$path" == *"/$d"* ]] && return 0
    done
    local bn; bn=$(basename "$path")
    [[ "$bn" == ~$* || "$bn" == ".DS_Store" || "$bn" == "Thumbs.db" ]] && return 0
    return 1
}

# ----- Git 模式：已提交变更（含 author）-----
scan_git_committed() {
    if $INCLUDE_AUTHORS; then
        git log \
            --since="$START_DATE 00:00:00" \
            --until="$END_DATE 23:59:59" \
            --name-status \
            --diff-filter=AMDR \
            --pretty=format:"__COMMIT__%h|%ad|%an|%s" \
            --date=short \
            -- . 2>/dev/null \
            | awk -F'|' '
            /^__COMMIT__/ { hash=$1; gsub(/^__COMMIT__/,"",hash); date=$2; author=$3; subj=$4; next }
            /^[AMDR]\t/ {
                status = substr($0,1,1)
                if (status == "R") { split($0,parts,"\t"); file=parts[3] }
                else { file=substr($0,3) }
                if (file ~ /(\.git|\.claude|被裁剪文件|待整理|node_modules|~$)/) next
                key = file "|" status
                if (!(key in seen)) {
                    seen[key] = 1
                    printf ",{\"path\":\"%s\",\"status\":\"%s\",\"source\":\"git\",\"author\":\"%s\",\"commit_date\":\"%s\",\"commit_msg\":\"%s\"}", file, status, author, date, subj
                }
            }'
    else
        git log \
            --since="$START_DATE 00:00:00" \
            --until="$END_DATE 23:59:59" \
            --name-status \
            --diff-filter=AMDR \
            --pretty=format:"" \
            -- . 2>/dev/null \
            | awk '
            /^[AMDR]\t/ {
                status = substr($0,1,1)
                if (status == "R") { split($0,parts,"\t"); file=parts[3] }
                else { file=substr($0,3) }
                if (file !~ /(\.git|\.claude|被裁剪文件|待整理|node_modules|~$)/) {
                    if (!(file in files)) files[file]=status
                }
            }
            END { for(f in files) printf ",{\"path\":\"%s\",\"status\":\"%s\",\"source\":\"git\"}", f, files[f] }'
    fi
}

# ----- Git 模式：未提交变更 -----
scan_git_uncommitted() {
    git status --porcelain -- . 2>/dev/null \
        | awk -v include_authors="$INCLUDE_AUTHORS" '
        {
            sc = substr($0,1,2); file = substr($0,4)
            if (sc ~ /^[AMDR]/) s = substr(sc,1,1)
            else if (sc ~ /^.M/) s = "M"
            else if (sc ~ /^.D/) s = "D"
            else if (sc ~ /^\?\?/) s = "?"
            else s = "M"
            if (file !~ /(\.git|\.claude|被裁剪文件|待整理|node_modules|~$)/) {
                author_info = ""
                if (include_authors == "true") {
                    # 尝试获取文件最后修改者
                    cmd = "git log -1 --format=\"%an\" -- " file " 2>/dev/null"
                    cmd | getline author; close(cmd)
                    if (author != "") author_info = ",\"author\":\"" author "\""
                }
                printf ",{\"path\":\"%s\",\"status\":\"%s\",\"source\":\"working_tree\"%s}", file, s, author_info
            }
        }'
}

# ----- 文件系统模式 -----
scan_find() {
    find . -type f \
        -newermt "$START_DATE" ! -newermt "$END_DATE 23:59:59" \
        -not -path '*/.git/*' -not -path '*/.claude/*' \
        -not -path '*/被裁剪文件/*' -not -path '*/待整理/*' \
        -not -name '~$*' -not -name '.DS_Store' -not -name 'Thumbs.db' \
        -printf '%TFT%TZ %p\n' 2>/dev/null \
        | while IFS= read -r line; do
            [ -z "$line" ] && continue
            mtime="${line%% *}"; path="${line#* }"; path="${path#./}"
            should_exclude "$path" && continue
            printf ",{\"path\":\"%s\",\"status\":\"M\",\"mtime\":\"%s\",\"source\":\"find\"}" "$path" "$mtime"
        done

    # 删除检测：对比 worklog.md 中记录的文件（如存在）
    if [ -f "worklog.md" ]; then
        grep -oP '^\| ([^|]+) \|' worklog.md 2>/dev/null | sed 's/^| //;s/ |$//' | while IFS= read -r oldfile; do
            [ -z "$oldfile" ] && continue
            [ -f "$oldfile" ] || printf ",{\"path\":\"%s\",\"status\":\"D\",\"source\":\"find_deleted\"}" "$oldfile"
        done || true
    fi
}

# ----- 入口 -----
MODE="find"
COMMITTED=""
UNCOMMITTED=""

if is_git_repo; then
    MODE="git"
    if $QUICK_MODE; then
        # 快速模式：仅用 git log --name-only，不获取 diff
        COMMITTED=$(scan_git_committed)
    else
        COMMITTED=$(scan_git_committed)
    fi
    if $INCLUDE_UNCOMMITTED; then
        UNCOMMITTED=$(scan_git_uncommitted)
    fi
else
    COMMITTED=$(scan_find)
fi

# 去掉开头的逗号
COMMITTED="${COMMITTED#,}"
UNCOMMITTED="${UNCOMMITTED#,}"

# 计数
total=0
if [ -n "$COMMITTED" ] && [ "$COMMITTED" != "" ]; then
    total=$(echo "$COMMITTED" | tr ',' '\n' | grep -c '"path"' || echo 0)
fi
if [ -n "$UNCOMMITTED" ] && [ "$UNCOMMITTED" != "" ]; then
    u=$(echo "$UNCOMMITTED" | tr ',' '\n' | grep -c '"path"' || echo 0)
    total=$((total + u))
fi

printf '{"committed":[%s],"uncommitted":[%s],"meta":{"mode":"%s","total":%d,"quick":%s,"authors":%s}}\n' \
    "$COMMITTED" "$UNCOMMITTED" "$MODE" "$total" "$QUICK_MODE" "$INCLUDE_AUTHORS"
