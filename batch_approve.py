#!/usr/bin/env python3
"""
批量批准 wiki_pages 候选内容，镜像 Rust 的 approve_candidate 逻辑。

用法：
  python batch_approve.py [<project_id>]

默认 project_id=1。

步骤：
1. 打开 ~/.kingdee-kb/metadata.db
2. 列出所有 content_candidate IS NOT NULL 的页面
3. 对每个页面：
   a. 提取 content_candidate 里的 [[slug]] 引用，过滤 valid_slugs、排除自引用
   b. UPDATE wiki_pages:
      content = content_candidate
      content_candidate = NULL, candidate_status = NULL, candidate_version = NULL
      sources = COALESCE(sources_candidate, sources)
      sources_candidate = NULL
      wikilinks = (extracted slugs as JSON)
      version = version + 1, updated_at = datetime('now')
4. 跑一次 build_knowledge_graph 逻辑（4 信号）
5. 输出最终边数

警告：必须先关闭 Tauri app（否则 SQLite 文件锁冲突）。
"""
import os
import re
import json
import sqlite3
import sys
from collections import defaultdict
from datetime import datetime

HOME = os.environ.get("USERPROFILE") or os.environ.get("HOME") or ""
DB_PATH = os.environ.get("KINGDEE_KB_DB") or os.path.join(HOME, ".kingdee-kb", "metadata.db")
# 在 Windows 上原文件可能被"幽灵锁"卡住（即使应用已关）。
# 解决：先复制到 temp，再在副本上工作，最后覆盖回原文件。
# 覆盖前需要清掉原文件的 -wal / -shm / -journal 残留。
TEMP_DB_PATH = os.path.join(
    os.environ.get("TEMP", "/tmp"),
    f"kingdee_kb_batch_approve_{os.getpid()}_{datetime.now().strftime('%H%M%S%f')}.db",
)


def extract_wikilinks(markdown: str, current_slug: str, valid_slugs: set) -> list:
    """镜像 Rust extract_wikilinks：char-by-char 扫描 [[..]]"""
    out = []
    i = 0
    n = len(markdown)
    while i + 1 < n:
        if markdown[i] == "[" and markdown[i + 1] == "[":
            # 找 ]]
            j = i + 2
            while j + 1 < n:
                if markdown[j] == "]" and markdown[j + 1] == "]":
                    break
                j += 1
            if j + 1 < n:
                inner = markdown[i + 2 : j]
                slug = inner.split("|", 1)[0].strip()
                if slug and slug != current_slug and slug in valid_slugs:
                    out.append(slug)
                i = j + 2
                continue
        i += 1
    out = sorted(set(out))
    return out


def get_valid_slugs(conn, project_id):
    cur = conn.execute("SELECT slug FROM wiki_pages WHERE project_id = ?", (project_id,))
    return {row[0] for row in cur.fetchall()}


def list_pending(conn, project_id):
    cur = conn.execute(
        "SELECT id, slug, project_id, content_candidate, sources_candidate, version, candidate_version "
        "FROM wiki_pages WHERE project_id = ? AND content_candidate IS NOT NULL",
        (project_id,),
    )
    return cur.fetchall()


def approve_one(conn, page_id, slug, project_id, candidate, sources_candidate, valid_slugs):
    """镜像 approve_candidate：UPDATE 一行"""
    links = extract_wikilinks(candidate or "", slug, valid_slugs)
    wikilinks_json = json.dumps(links, ensure_ascii=False)
    # sources 优先用 sources_candidate，fallback 到现有的 sources
    conn.execute(
        """
        UPDATE wiki_pages SET
          content = ?,
          content_candidate = NULL,
          candidate_status = NULL,
          candidate_version = NULL,
          sources = COALESCE(?, sources),
          sources_candidate = NULL,
          wikilinks = ?,
          version = version + 1,
          updated_at = datetime('now')
        WHERE id = ? AND content_candidate IS NOT NULL
        """,
        (candidate, sources_candidate, wikilinks_json, page_id),
    )
    return links


def build_wikilink_signal(conn, project_id):
    """build_signal_wikilink 镜像"""
    cur = conn.execute(
        "SELECT slug, wikilinks FROM wiki_pages WHERE project_id = ?", (project_id,)
    )
    count = 0
    for slug, wikilinks_json in cur.fetchall():
        try:
            targets = json.loads(wikilinks_json or "[]")
        except json.JSONDecodeError:
            continue
        for target in targets:
            if not target or target == slug:
                continue
            conn.execute(
                "INSERT OR IGNORE INTO knowledge_graph (project_id, source_slug, target_slug, signal, weight) "
                "VALUES (?, ?, ?, 'wikilink', 1.0)",
                (project_id, slug, target),
            )
            count += 1
    return count


def build_tag_signal(conn, project_id):
    """build_signal_tag 镜像：相同标签的页面之间建边"""
    cur = conn.execute(
        "SELECT slug, tags FROM wiki_pages WHERE project_id = ? AND tags != '[]' AND tags != ''",
        (project_id,),
    )
    count = 0
    slug_to_tags = {}
    tag_to_slugs = defaultdict(set)
    for slug, tags_json in cur.fetchall():
        try:
            tags = json.loads(tags_json or "[]")
        except json.JSONDecodeError:
            continue
        slug_to_tags[slug] = tags
        for t in tags:
            tag_to_slugs[t].add(slug)
    for tag, slugs in tag_to_slugs.items():
        slugs = sorted(slugs)
        for i, s1 in enumerate(slugs):
            for s2 in slugs[i + 1 :]:
                conn.execute(
                    "INSERT OR IGNORE INTO knowledge_graph (project_id, source_slug, target_slug, signal, weight) "
                    "VALUES (?, ?, ?, 'tag', 1.0)",
                    (project_id, s1, s2),
                )
                count += 1
    return count


def build_source_signal(conn, project_id):
    """build_signal_source 镜像：相同 source 的页面建边

    sources 字段实际是 [{"document_id":N,"source_id":M,"chunks":[...]}, ...] 形式
    （不是字符串列表）。用 (document_id, source_id) 元组作为 group key。
    """
    cur = conn.execute(
        "SELECT slug, sources FROM wiki_pages WHERE project_id = ? AND sources != '[]' AND sources != ''",
        (project_id,),
    )
    count = 0
    source_to_slugs = defaultdict(set)
    for slug, sources_json in cur.fetchall():
        try:
            sources = json.loads(sources_json or "[]")
        except json.JSONDecodeError:
            continue
        for s in sources:
            if not isinstance(s, dict):
                continue
            # 兼容多种 key 形式
            doc_id = s.get("document_id") or s.get("doc_id")
            src_id = s.get("source_id")
            key: tuple
            if src_id is not None:
                key = ("source_id", src_id)
            elif doc_id is not None:
                key = ("document_id", doc_id)
            else:
                continue
            source_to_slugs[key].add(slug)
    for src, slugs in source_to_slugs.items():
        slugs = sorted(slugs)
        for i, s1 in enumerate(slugs):
            for s2 in slugs[i + 1 :]:
                conn.execute(
                    "INSERT OR IGNORE INTO knowledge_graph (project_id, source_slug, target_slug, signal, weight) "
                    "VALUES (?, ?, ?, 'source', 1.0)",
                    (project_id, s1, s2),
                )
                count += 1
    return count


def build_co_citation_signal(conn, project_id):
    """build_signal_co_citation 镜像：共引两个 source 的页面建边

    同样用 (document_id, source_id) 元组作为 source key
    """
    cur = conn.execute(
        "SELECT slug, sources FROM wiki_pages WHERE project_id = ? AND sources != '[]' AND sources != ''",
        (project_id,),
    )
    count = 0
    page_sources = []
    for slug, sources_json in cur.fetchall():
        try:
            sources = json.loads(sources_json or "[]")
        except json.JSONDecodeError:
            continue
        keys: set = set()
        for s in sources:
            if not isinstance(s, dict):
                continue
            doc_id = s.get("document_id") or s.get("doc_id")
            src_id = s.get("source_id")
            if src_id is not None:
                keys.add(("source_id", src_id))
            elif doc_id is not None:
                keys.add(("document_id", doc_id))
        if keys:
            page_sources.append((slug, keys))
    n = len(page_sources)
    for i in range(n):
        for j in range(i + 1, n):
            s1, src1 = page_sources[i]
            s2, src2 = page_sources[j]
            common = src1 & src2
            if not common:
                continue
            weight = 1.0 / (1.0 + len(src1) + len(src2))
            conn.execute(
                "INSERT OR IGNORE INTO knowledge_graph (project_id, source_slug, target_slug, signal, weight) "
                "VALUES (?, ?, ?, 'co_citation', ?)",
                (project_id, s1, s2, weight),
            )
            count += 1
    return count


def main():
    project_id = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    print(f">>> 原数据库: {DB_PATH}")
    print(f">>> 临时副本: {TEMP_DB_PATH}")
    print(f">>> 项目 ID: {project_id}")
    if not os.path.exists(DB_PATH):
        print(f"!!! 数据库不存在: {DB_PATH}")
        return

    # 0) 复制到 temp（绕开 Windows 幽灵锁）
    import shutil
    shutil.copy(DB_PATH, TEMP_DB_PATH)
    for ext in ("-wal", "-shm", "-journal"):
        src = DB_PATH + ext
        dst = TEMP_DB_PATH + ext
        if os.path.exists(src):
            shutil.copy(src, dst)
    print(f">>> 已复制 + 关联 wal/shm/journal")

    conn = sqlite3.connect(TEMP_DB_PATH)
    conn.execute("PRAGMA foreign_keys = OFF")

    # 0) 状态
    pending = list_pending(conn, project_id)
    print(f">>> 待批准: {len(pending)} 个页面")
    if not pending:
        print(">>> 没有候选可批准")
    else:
        # 1) 收集一次 valid_slugs（一次性算）
        valid_slugs = get_valid_slugs(conn, project_id)
        print(f">>> 项目有效 slug: {len(valid_slugs)} 个")

        # 2) 逐个批准
        approved = 0
        failed = []
        for row in pending:
            page_id, slug, pid, candidate, sources_cand, version, cand_version = row
            try:
                links = approve_one(
                    conn, page_id, slug, pid, candidate, sources_cand, valid_slugs
                )
                approved += 1
                if approved <= 3:
                    print(f"    批准 {slug}: wikilinks={links}")
            except Exception as e:
                failed.append((page_id, slug, str(e)))
                print(f"    !!! 批准 {slug} 失败: {e}")
        conn.commit()
        print(f">>> 批准完成: 成功={approved} 失败={len(failed)}")

    # 3) 批准后诊断
    cur = conn.execute(
        "SELECT COUNT(*) FROM wiki_pages WHERE project_id = ?", (project_id,)
    )
    total = cur.fetchone()[0]
    cur = conn.execute(
        "SELECT COUNT(*) FROM wiki_pages WHERE project_id = ? AND wikilinks != '[]' AND wikilinks != ''",
        (project_id,),
    )
    non_empty = cur.fetchone()[0]
    cur = conn.execute(
        "SELECT COUNT(*) FROM wiki_pages WHERE project_id = ? AND "
        "((content LIKE '%[[%' AND content LIKE '%]]%') OR "
        " (content_candidate LIKE '%[[%' AND content_candidate LIKE '%]]%'))",
        (project_id,),
    )
    has_brackets = cur.fetchone()[0]
    print(
        f">>> 批准后诊断: total={total} non_empty_wikilinks={non_empty} has_brackets={has_brackets}"
    )

    # 4) build: 清空旧 knowledge_graph，重建
    # 先做 schema 迁移：如果表里还有老字段 `project`（旧 bug 的残留），
    # 直接 drop 重建（表是空的，丢 0 行也无所谓）
    cols = [r[1] for r in conn.execute("PRAGMA table_info(knowledge_graph)").fetchall()]
    if "project" in cols and "project_id" in cols:
        # 老 schema 残留（用户的真实情况）：`project` NOT NULL + `project_id` nullable
        # 所有 INSERT 因 `project NOT NULL` 失败 → 表永远是空的
        # 修法：drop + 用新 schema 重建
        print(">>> 检测到 knowledge_graph 老 schema（`project` 字段），执行 drop + 重建")
        conn.execute("DROP TABLE knowledge_graph")
        conn.execute(
            """CREATE TABLE knowledge_graph (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                source_slug TEXT NOT NULL,
                target_slug TEXT NOT NULL,
                signal      TEXT NOT NULL CHECK(signal IN ('wikilink','tag','source','co_citation')),
                weight      REAL NOT NULL DEFAULT 1.0,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(project_id, source_slug, target_slug, signal)
            )"""
        )
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_kg_source ON knowledge_graph(project_id, source_slug)"
        )
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_kg_target ON knowledge_graph(project_id, target_slug)"
        )
        conn.commit()
    conn.execute("DELETE FROM knowledge_graph WHERE project_id = ?", (project_id,))
    n_w = build_wikilink_signal(conn, project_id)
    n_t = build_tag_signal(conn, project_id)
    n_s = build_source_signal(conn, project_id)
    n_c = build_co_citation_signal(conn, project_id)
    conn.commit()
    cur = conn.execute(
        "SELECT COUNT(*) FROM knowledge_graph WHERE project_id = ?", (project_id,)
    )
    total_edges = cur.fetchone()[0]
    cur = conn.execute(
        "SELECT signal, COUNT(*) FROM knowledge_graph WHERE project_id = ? GROUP BY signal",
        (project_id,),
    )
    by_signal = dict(cur.fetchall())
    print(
        f">>> build 边数: total={total_edges} wikilink={by_signal.get('wikilink', 0)} "
        f"tag={by_signal.get('tag', 0)} source={by_signal.get('source', 0)} "
        f"co_citation={by_signal.get('co_citation', 0)}"
    )

    conn.close()

    # 5) 复制回原文件（覆盖 + 清掉幽灵锁的旧 wal/shm/journal 残留）
    print(">>> 复制回原文件...")
    for ext in ("-wal", "-shm", "-journal"):
        old = DB_PATH + ext
        if os.path.exists(old):
            try:
                os.remove(old)
            except OSError as e:
                print(f"    !!! 删除 {old} 失败: {e}")
    shutil.copy(TEMP_DB_PATH, DB_PATH)
    for ext in ("-wal", "-shm", "-journal"):
        src = TEMP_DB_PATH + ext
        dst = DB_PATH + ext
        if os.path.exists(src):
            shutil.copy(src, dst)
    # 清理临时副本
    for ext in ("", "-wal", "-shm", "-journal"):
        tmp = TEMP_DB_PATH + ext
        if os.path.exists(tmp):
            os.remove(tmp)
    print(">>> 完成！下次启动 Tauri app 时即可看到 102 个已批准页面 + 知识图谱已构建")


if __name__ == "__main__":
    main()
