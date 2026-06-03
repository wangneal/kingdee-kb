//! 研究大纲节点管理（outline_nodes 表）
//!
//! 支持树形结构的大纲节点 CRUD、子树删除、节点移动（含环检测）、
//! 导出为 Markdown 列表/标题格式，以及统计信息查询。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

// ─── 数据类型 ───

/// 大纲节点完整记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineNode {
    pub id: i64,
    pub session_id: i64,
    pub parent_id: Option<i64>,
    pub content: String,
    /// 排序权重（分数索引）
    pub sort_order: f64,
    /// 关联的问答记录 ID
    pub question_id: Option<i64>,
    pub notes: String,
    /// JSON 数组格式的标签列表
    pub tags: String,
    /// 是否折叠（前端树节点展开/收起状态）
    pub collapsed: bool,
    /// 是否已完成
    pub completed: bool,
    /// 节点标记（如图标标识）
    pub marker: String,
    /// 优先级
    pub priority: String,
    /// 备注（区别于 notes，更轻量的单行备注）
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
}

// ─── 存储层 ───

/// 树节点（用于导出渲染，内部使用）
struct TreeNode {
    node: OutlineNode,
    children: Vec<TreeNode>,
}

/// outline_nodes 表的数据操作层
pub struct OutlineStore {
    conn: Mutex<Connection>,
}

impl OutlineStore {
    /// 使用指定的数据库文件路径创建存储
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let conn =
            Connection::open(db_path).map_err(|e| format!("打开大纲数据库失败: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("设置大纲数据库忙超时失败: {}", e))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_tables()?;
        Ok(store)
    }

    /// 创建内存数据库（降级兜底）
    pub fn new_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("创建内存大纲数据库失败: {}", e))?;
        // 内存数据库中禁用外键约束（用于降级和单元测试）
        conn.execute_batch("PRAGMA foreign_keys = OFF")
            .map_err(|e| format!("禁用外键约束失败: {}", e))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_tables()?;
        Ok(store)
    }

    /// 初始化 outline_nodes 表及索引（幂等）
    fn init_tables(&self) -> Result<(), String> {
        {
            let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS outline_nodes (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id  INTEGER NOT NULL REFERENCES research_sessions(id) ON DELETE CASCADE,
                    parent_id   INTEGER REFERENCES outline_nodes(id),
                    content     TEXT NOT NULL DEFAULT '',
                    sort_order  REAL NOT NULL,
                    question_id INTEGER REFERENCES session_qa_records(id) ON DELETE SET NULL,
                    notes       TEXT NOT NULL DEFAULT '',
                    tags        TEXT NOT NULL DEFAULT '[]',
                    collapsed   INTEGER NOT NULL DEFAULT 0,
                    completed   INTEGER NOT NULL DEFAULT 0,
                    marker      TEXT NOT NULL DEFAULT '',
                    priority    TEXT NOT NULL DEFAULT '',
                    note        TEXT NOT NULL DEFAULT '',
                    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_outline_session ON outline_nodes(session_id);
                CREATE INDEX IF NOT EXISTS idx_outline_parent ON outline_nodes(parent_id);
                ",
            )
            .map_err(|e| format!("创建 outline_nodes 表失败: {}", e))?;
        }
        // 迁移历史问答记录到大纲节点（需要独立获取锁）
        self.migrate_qa_to_outline()?;
        Ok(())
    }

    /// 将历史问答记录（session_qa_records）迁移到大纲节点（outline_nodes）。
    ///
    /// 幂等设计：
    /// - 跳过已有大纲节点的 session（不创建重复根节点）
    /// - 跳过已通过 question_id 关联到大纲节点的 QA 记录
    /// - 若 session_qa_records 表不存在则静默跳过（内存数据库场景）
    ///
    /// 返回成功迁移的节点数量。
    pub fn migrate_qa_to_outline(&self) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;

        // 检查 session_qa_records 表是否存在（内存数据库或未初始化时可能不存在）
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='session_qa_records'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| format!("检查 session_qa_records 表是否存在失败: {}", e))?
            > 0;

        if !table_exists {
            return Ok(0);
        }

        // 步骤 1：为有问答记录但无大纲节点的 session 创建根节点
        conn.execute(
            "INSERT INTO outline_nodes (session_id, parent_id, content, sort_order, notes, tags, collapsed, completed, marker, priority, note)
             SELECT
                 r.session_id,
                 NULL as parent_id,
                 '调研问答记录' as content,
                 1.0 as sort_order,
                 '' as notes,
                 '[]' as tags,
                 0 as collapsed,
                 0 as completed,
                 '' as marker,
                 '' as priority,
                 '共 ' || COUNT(*) || ' 条问答记录' as note
             FROM session_qa_records r
             WHERE NOT EXISTS (
                 SELECT 1 FROM outline_nodes WHERE session_id = r.session_id
             )
             GROUP BY r.session_id",
            [],
        )
        .map_err(|e| format!("创建迁移根节点失败: {}", e))?;

        // 步骤 2：将每个尚未关联的 QA 记录转换为根节点下的子节点
        let qa_rows: Vec<(i64, i64, String, String, i64)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT r.id, r.session_id, r.question_text, r.answer_text,
                            ROW_NUMBER() OVER (PARTITION BY r.session_id ORDER BY r.sort_order, r.id) as rn
                     FROM session_qa_records r
                     WHERE NOT EXISTS (
                         SELECT 1 FROM outline_nodes
                         WHERE session_id = r.session_id AND question_id = r.id
                     )",
                )
                .map_err(|e| format!("准备迁移查询失败: {}", e))?;

            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,    // qa_id
                        row.get::<_, i64>(1)?,    // session_id
                        row.get::<_, String>(2)?, // question_text
                        row.get::<_, String>(3)?, // answer_text
                        row.get::<_, i64>(4)?,    // rn
                    ))
                })
                .map_err(|e| format!("执行迁移查询失败: {}", e))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| format!("读取迁移行失败: {}", e))?);
            }
            results
        };

        let mut count: i64 = 0;
        for (qa_id, session_id, question, answer, rn) in qa_rows {
            // 查找该 session 的根节点（迁移刚创建的 "调研问答记录" 节点）
            let root_id: i64 = match conn.query_row(
                "SELECT id FROM outline_nodes
                 WHERE session_id = ?1 AND parent_id IS NULL
                 ORDER BY sort_order LIMIT 1",
                params![session_id],
                |row| row.get(0),
            ) {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!(
                        "迁移问答到大纲时未找到会话根节点，已跳过记录: session_id={}, qa_id={}, error={}",
                        session_id,
                        qa_id,
                        e
                    );
                    continue;
                }
            };

            let content = if answer.is_empty() {
                question
            } else {
                format!("{}\n\n{}", question, answer)
            };

            conn.execute(
                "INSERT INTO outline_nodes
                     (session_id, parent_id, content, sort_order, question_id, notes, tags, collapsed, completed, marker, priority, note)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, '', '', '')",
                params![session_id, root_id, content, rn as f64, qa_id, "", "[]"],
            )
            .map_err(|e| format!("插入迁移子节点失败: {}", e))?;

            count += 1;
        }

        Ok(count)
    }

    // ─── CRUD 操作 ───

    /// 创建大纲节点。
    ///
    /// - parent_id 为 None 时创建根节点，sort_order = 同 session 下根节点最大值 + 1.0
    /// - parent_id 为 Some 时创建子节点，sort_order = 同父节点下子节点最大值 + 1.0
    pub fn create_node(
        &self,
        session_id: i64,
        parent_id: Option<i64>,
        content: &str,
    ) -> Result<OutlineNode, String> {
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;

        // 计算 sort_order
        // 分数索引：首个节点 1.0，追加 MAX + 1.0
        // 先计数判断是否有子节点，避免 COALESCE(MAX, 0.0) 在 MAX=0.0 时的歧义
        let parent_condition = match parent_id {
            None => "parent_id IS NULL".to_string(),
            Some(pid) => format!("parent_id = {}", pid),
        };
        let count_sql = format!(
            "SELECT COUNT(*) FROM outline_nodes WHERE session_id = ?1 AND {}",
            parent_condition
        );
        let count: i64 = conn
            .query_row(&count_sql, params![session_id], |row| row.get(0))
            .map_err(|e| format!("查询子节点数量失败: {}", e))?;

        let new_order = if count == 0 {
            1.0
        } else {
            let max_sql = format!(
                "SELECT MAX(sort_order) FROM outline_nodes WHERE session_id = ?1 AND {}",
                parent_condition
            );
            let max_val: f64 = conn
                .query_row(&max_sql, params![session_id], |row| row.get(0))
                .map_err(|e| format!("查询最大排序值失败: {}", e))?;
            max_val + 1.0
        };

        conn.execute(
            "INSERT INTO outline_nodes (session_id, parent_id, content, sort_order)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_id, parent_id, content, new_order],
        )
        .map_err(|e| format!("插入大纲节点失败: {}", e))?;

        let id = conn.last_insert_rowid();
        drop(conn);
        self.get_node(id)?.ok_or_else(|| "创建节点后查询失败".to_string())
    }

    /// 更新大纲节点（部分更新，仅更新传入的 Some 字段）。
    pub fn update_node(
        &self,
        id: i64,
        content: Option<&str>,
        notes: Option<&str>,
        tags: Option<&str>,
        collapsed: Option<bool>,
        completed: Option<bool>,
        marker: Option<&str>,
        priority: Option<&str>,
        note: Option<&str>,
    ) -> Result<OutlineNode, String> {
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;

        // 先读取现有值
        let existing = self.get_node_locked(&conn, id)?;
        let new_content = content.unwrap_or(&existing.content);
        let new_notes = notes.unwrap_or(&existing.notes);
        let new_tags = tags.unwrap_or(&existing.tags);
        let new_collapsed = collapsed.unwrap_or(existing.collapsed);
        let new_completed = completed.unwrap_or(existing.completed);
        let new_marker = marker.unwrap_or(&existing.marker);
        let new_priority = priority.unwrap_or(&existing.priority);
        let new_note = note.unwrap_or(&existing.note);

        let affected = conn
            .execute(
                "UPDATE outline_nodes SET
                    content = ?1, notes = ?2, tags = ?3,
                    collapsed = ?4, completed = ?5, marker = ?6, priority = ?7, note = ?8,
                    updated_at = datetime('now')
                 WHERE id = ?9",
                params![
                    new_content, new_notes, new_tags,
                    new_collapsed as i32, new_completed as i32, new_marker, new_priority, new_note,
                    id,
                ],
            )
            .map_err(|e| format!("更新大纲节点失败: {}", e))?;
        if affected == 0 {
            return Err(format!("大纲节点未找到: id={}", id));
        }

        drop(conn);
        self.get_node(id)?.ok_or_else(|| "更新节点后查询失败".to_string())
    }

    /// 递归删除节点及其所有后代（子树删除）。
    ///
    /// 删除后清理孤立的自动生成且未收藏的问答记录。
    /// 整个操作在显式事务中执行，失败时自动回滚。
    pub fn delete_node_subtree(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;

        let do_delete = || -> Result<(), String> {
            conn.execute_batch("BEGIN").map_err(|e| format!("开始事务失败: {}", e))?;

            // 收集子树中所有节点 ID（含自身）
            let node_ids = self.collect_subtree_ids(&conn, id)?;

            // 收集子树中关联的 question_id（用于后续清理）
            let mut question_ids: Vec<i64> = Vec::new();
            for nid in &node_ids {
                let qid: Option<i64> = conn
                    .query_row(
                        "SELECT question_id FROM outline_nodes WHERE id = ?1",
                        params![nid],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("查询节点 question_id 失败: {}", e))?;
                if let Some(q) = qid {
                    question_ids.push(q);
                }
            }

            // 删除子树（从叶子到根）
            for nid in node_ids.iter().rev() {
                conn.execute("DELETE FROM outline_nodes WHERE id = ?1", params![nid])
                    .map_err(|e| format!("删除大纲节点失败: {}", e))?;
            }

            // 清理孤立的自动生成且未收藏的问答记录
            if !question_ids.is_empty() {
                for qid in &question_ids {
                    // 检查该 question_id 是否还被其他大纲节点引用
                    let ref_count: i64 = conn
                        .query_row(
                            "SELECT COUNT(*) FROM outline_nodes WHERE question_id = ?1",
                            params![qid],
                            |row| row.get(0),
                        )
                        .map_err(|e| format!("查询引用计数失败: {}", e))?;
                    if ref_count == 0 {
                        // 仅删除自动生成且未收藏的记录
                        conn.execute(
                            "DELETE FROM session_qa_records WHERE id = ?1
                             AND (source != 'manual' OR source IS NULL) AND (is_bookmarked = 0 OR is_bookmarked IS NULL)",
                            params![qid],
                        )
                        .map_err(|e| format!("清理孤立问答记录失败: {}", e))?;
                    }
                }
            }

            conn.execute_batch("COMMIT").map_err(|e| format!("提交事务失败: {}", e))?;
            Ok(())
        };

        match do_delete() {
            Ok(()) => Ok(()),
            Err(e) => {
                // 失败时回滚事务
                conn.execute_batch("ROLLBACK").ok();
                Err(e)
            }
        }
    }

    /// 移动节点到新的父节点下，并更新排序权重。
    ///
    /// 包含环检测：不允许将节点移动到其后代节点下。
    pub fn move_node(
        &self,
        id: i64,
        new_parent_id: Option<i64>,
        new_sort_order: f64,
    ) -> Result<OutlineNode, String> {
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;

        // 自引用检查：不允许将节点移动到自身下
        if Some(id) == new_parent_id {
            return Err("不能将节点移动到自身下".to_string());
        }

        // 环检测：验证 new_parent_id 不是 id 的后代
        if let Some(npid) = new_parent_id {
            if self.is_descendant(&conn, npid, id)? {
                return Err("不能将节点移动到其子节点下".to_string());
            }
        }

        let affected = conn
            .execute(
                "UPDATE outline_nodes SET parent_id = ?1, sort_order = ?2, updated_at = datetime('now')
                 WHERE id = ?3",
                params![new_parent_id, new_sort_order, id],
            )
            .map_err(|e| format!("移动大纲节点失败: {}", e))?;
        if affected == 0 {
            return Err(format!("大纲节点未找到: id={}", id));
        }

        drop(conn);
        self.get_node(id)?.ok_or_else(|| "移动节点后查询失败".to_string())
    }

    /// 获取指定 session 的完整大纲（平铺列表，按 parent_id 和 sort_order 排序）。
    pub fn get_tree(&self, session_id: i64) -> Result<Vec<OutlineNode>, String> {
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;
        self.query_list(
            &conn,
            "SELECT id, session_id, parent_id, content, sort_order, question_id, notes, tags,
                    collapsed, completed, marker, priority, note, created_at, updated_at
             FROM outline_nodes WHERE session_id = ?1
             ORDER BY parent_id IS NULL DESC, parent_id, sort_order",
            params![session_id],
        )
    }

    /// 按 ID 获取单个节点。
    pub fn get_node(&self, id: i64) -> Result<Option<OutlineNode>, String> {
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;
        self.query_one(
            &conn,
            "SELECT id, session_id, parent_id, content, sort_order, question_id, notes, tags,
                    collapsed, completed, marker, priority, note, created_at, updated_at
             FROM outline_nodes WHERE id = ?1",
            params![id],
        )
    }

    /// 获取指定节点的直接子节点。
    pub fn get_children(&self, parent_id: i64) -> Result<Vec<OutlineNode>, String> {
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;
        self.query_list(
            &conn,
            "SELECT id, session_id, parent_id, content, sort_order, question_id, notes, tags,
                    collapsed, completed, marker, priority, note, created_at, updated_at
             FROM outline_nodes WHERE parent_id = ?1 ORDER BY sort_order",
            params![parent_id],
        )
    }

    /// 获取指定 session 的根节点列表。
    pub fn get_root_nodes(&self, session_id: i64) -> Result<Vec<OutlineNode>, String> {
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;
        self.query_list(
            &conn,
            "SELECT id, session_id, parent_id, content, sort_order, question_id, notes, tags,
                    collapsed, completed, marker, priority, note, created_at, updated_at
             FROM outline_nodes WHERE session_id = ?1 AND parent_id IS NULL ORDER BY sort_order",
            params![session_id],
        )
    }

    /// 导出大纲为指定格式的字符串。
    ///
    /// - `"markdown_list"`：缩进的无序列表
    /// - `"markdown_headings"`：## / ### / #### 层级标题
    pub fn export_outline(&self, session_id: i64, format: &str) -> Result<String, String> {
        let nodes = self.get_tree(session_id)?;
        let tree = Self::build_tree_from_flat(&nodes);
        match format {
            "markdown_list" => Ok(Self::render_markdown_list(&tree, 0)),
            "markdown_headings" => Ok(Self::render_markdown_headings(&tree, 2)),
            _ => Err(format!("不支持的导出格式: {}", format)),
        }
    }

    /// 获取大纲统计信息。
    pub fn get_outline_stats(&self, session_id: i64) -> Result<serde_json::Value, String> {
        let nodes = self.get_tree(session_id)?;
        let total_nodes = nodes.len();
        let tree = Self::build_tree_from_flat(&nodes);
        let depth = Self::calc_depth(&tree, 0);
        let max_children = self.calc_max_children(&nodes);

        Ok(serde_json::json!({
            "total_nodes": total_nodes,
            "depth": depth,
            "max_children": max_children,
        }))
    }

    /// 从 Markdown 字符串导入/同步大纲。
    /// 解析标题行作为大纲节点，非标题行作为上一标题节点的 notes。
    /// 并在数据库中执行增量 Diff 更新。
    pub fn import_markdown_outline(&self, session_id: i64, markdown: &str) -> Result<(), String> {
        // 1. 解析 Markdown
        struct ParsedNode {
            content: String,
            level: usize,
            notes: String,
            matched_id: Option<i64>,
            matched_question_id: Option<i64>, // 保存已匹配到的旧 Q&A 记录 ID
        }

        let mut parsed_nodes: Vec<ParsedNode> = Vec::new();
        let mut current_heading_idx: Option<usize> = None;
        let lines: Vec<&str> = markdown.lines().collect();

        // 查找最小的 # 数量作为 base_depth
        let mut min_hashes = 999;
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
                if hash_count > 0 && trimmed[hash_count..].starts_with(' ') {
                    if hash_count < min_hashes {
                        min_hashes = hash_count;
                    }
                }
            }
        }
        if min_hashes == 999 {
            min_hashes = 2; // 默认以 ## (二级标题) 为第一层
        }

        for line in lines {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
                if hash_count > 0 && trimmed[hash_count..].starts_with(' ') {
                    let content = trimmed[hash_count..].trim().to_string();
                    let level = if hash_count >= min_hashes {
                        hash_count - min_hashes
                    } else {
                        0
                    };
                    parsed_nodes.push(ParsedNode {
                        content,
                        level,
                        notes: String::new(),
                        matched_id: None,
                        matched_question_id: None,
                    });
                    current_heading_idx = Some(parsed_nodes.len() - 1);
                    continue;
                }
            }

            if let Some(idx) = current_heading_idx {
                if !parsed_nodes[idx].notes.is_empty() {
                    parsed_nodes[idx].notes.push('\n');
                }
                parsed_nodes[idx].notes.push_str(line);
            }
        }

        // 去除 notes 的首尾空白
        for node in &mut parsed_nodes {
            node.notes = node.notes.trim().to_string();
        }

        // 获取旧节点
        let old_nodes = self.get_tree(session_id)?;
        let mut old_nodes_pool = old_nodes.clone();

        // 两轮匹配
        // 第一轮：精确匹配 content 和 notes 相同
        for parsed_node in &mut parsed_nodes {
            if let Some(pos) = old_nodes_pool.iter().position(|o| o.content == parsed_node.content && o.notes == parsed_node.notes) {
                let matched = old_nodes_pool.remove(pos);
                parsed_node.matched_id = Some(matched.id);
                parsed_node.matched_question_id = matched.question_id;
            }
        }
        // 第二轮：模糊匹配 content 相同
        for parsed_node in &mut parsed_nodes {
            if parsed_node.matched_id.is_none() {
                if let Some(pos) = old_nodes_pool.iter().position(|o| o.content == parsed_node.content) {
                    let matched = old_nodes_pool.remove(pos);
                    parsed_node.matched_id = Some(matched.id);
                    parsed_node.matched_question_id = matched.question_id;
                }
            }
        }

        // 计算每个 parsed_node 的 parent_idx
        // 改为支持跨级标题（例如从 level 0 直接到 level 2），挂载到离它最近且层级小于它的上一个节点
        let mut parent_indices: Vec<Option<usize>> = vec![None; parsed_nodes.len()];
        for i in 0..parsed_nodes.len() {
            let current_level = parsed_nodes[i].level;
            if current_level > 0 {
                for j in (0..i).rev() {
                    if parsed_nodes[j].level < current_level {
                        parent_indices[i] = Some(j);
                        break;
                    }
                }
            }
        }

        // 执行数据库更新
        let conn = self.conn.lock().map_err(|e| format!("加锁失败: {}", e))?;
        
        let do_update = || -> Result<(), String> {
            conn.execute_batch("BEGIN").map_err(|e| format!("开始事务失败: {}", e))?;

            // 检查问答记录表是否存在，以容忍内存测试数据库等未启用问答模块的情况
            let has_qa_table: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='session_qa_records')",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(false);

            // 存储每一项插入或更新后的真正 db_id
            let mut db_ids: Vec<Option<i64>> = vec![None; parsed_nodes.len()];

            for i in 0..parsed_nodes.len() {
                let parsed_node = &parsed_nodes[i];
                let parent_db_id = parent_indices[i]
                    .and_then(|p_idx| db_ids[p_idx]);

                let sort_order = (i + 1) as f64;
                let mut question_id = parsed_node.matched_question_id;

                // 标题即 Q，正文即 A。当且仅当问答记录表存在时，自动同步到问答中
                if has_qa_table {
                    if !parsed_node.notes.trim().is_empty() {
                        match question_id {
                            Some(qid) => {
                                // 更新已有问答
                                conn.execute(
                                    "UPDATE session_qa_records SET
                                        question_text = ?1,
                                        answer_text = ?2,
                                        updated_at = datetime('now')
                                     WHERE id = ?3",
                                    params![parsed_node.content, parsed_node.notes, qid],
                                ).map_err(|e| format!("更新问答记录失败: {}", e))?;
                            }
                            None => {
                                // 插入新问答（标记为 auto 自动提取）
                                conn.execute(
                                    "INSERT INTO session_qa_records (session_id, question_text, answer_text, sort_order, source)
                                     VALUES (?1, ?2, ?3, ?4, 'auto')",
                                    params![session_id, parsed_node.content, parsed_node.notes, sort_order],
                                ).map_err(|e| format!("创建问答记录失败: {}", e))?;
                                question_id = Some(conn.last_insert_rowid());
                            }
                        }
                    } else {
                        // 如果正文为空，清空关联的问答（如果没有其他节点引用此问答，执行清理）
                        if let Some(qid) = question_id {
                            let ref_count: i64 = conn
                                .query_row(
                                    "SELECT COUNT(*) FROM outline_nodes WHERE question_id = ?1 AND id != ?2",
                                    params![qid, parsed_node.matched_id.unwrap_or(0)],
                                    |row| row.get(0),
                                )
                                .map_err(|e| format!("查询 question 引用失败: {}", e))?;
                            if ref_count == 0 {
                                conn.execute(
                                    "DELETE FROM session_qa_records WHERE id = ?1
                                     AND (source != 'manual' OR source IS NULL) AND (is_bookmarked = 0 OR is_bookmarked IS NULL)",
                                    params![qid],
                                )
                                .map_err(|e| format!("清理空闲问答记录失败: {}", e))?;
                            }
                            question_id = None;
                        }
                    }
                }


                let db_id = match parsed_node.matched_id {
                    Some(id) => {
                        // 更新节点，添加对 question_id 的更新
                        conn.execute(
                            "UPDATE outline_nodes SET
                                parent_id = ?1,
                                content = ?2,
                                notes = ?3,
                                sort_order = ?4,
                                question_id = ?5,
                                updated_at = datetime('now')
                             WHERE id = ?6",
                            params![parent_db_id, parsed_node.content, parsed_node.notes, sort_order, question_id, id],
                        ).map_err(|e| format!("更新大纲节点失败: {}", e))?;
                        id
                    }
                    None => {
                        // 插入新节点，包含 question_id 字段
                        conn.execute(
                            "INSERT INTO outline_nodes (session_id, parent_id, content, notes, sort_order, question_id)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![session_id, parent_db_id, parsed_node.content, parsed_node.notes, sort_order, question_id],
                        ).map_err(|e| format!("插入大纲节点失败: {}", e))?;
                        conn.last_insert_rowid()
                    }
                };

                db_ids[i] = Some(db_id);
            }

            // 先将所有待删除节点的 parent_id 置为 NULL，以避免自引用外键约束失败（FOREIGN KEY constraint failed）
            for old_node in &old_nodes_pool {
                conn.execute(
                    "UPDATE outline_nodes SET parent_id = NULL WHERE id = ?1",
                    params![old_node.id],
                ).map_err(|e| format!("解除旧大纲节点父级关联失败: {}", e))?;
            }

            // 清理已删除的旧节点以及关联 of QA
            for old_node in old_nodes_pool {
                // 清理关联的 question
                if let Some(qid) = old_node.question_id {
                    let ref_count: i64 = conn
                        .query_row(
                            "SELECT COUNT(*) FROM outline_nodes WHERE question_id = ?1 AND id != ?2",
                            params![qid, old_node.id],
                            |row| row.get(0),
                        )
                        .map_err(|e| format!("查询 question 引用失败: {}", e))?;
                    if ref_count == 0 {
                        conn.execute(
                            "DELETE FROM session_qa_records WHERE id = ?1
                             AND (source != 'manual' OR source IS NULL) AND (is_bookmarked = 0 OR is_bookmarked IS NULL)",
                            params![qid],
                        )
                        .map_err(|e| format!("清理孤立问答记录失败: {}", e))?;
                    }
                }

                conn.execute("DELETE FROM outline_nodes WHERE id = ?1", params![old_node.id])
                    .map_err(|e| format!("删除被移除的大纲节点失败: {}", e))?;
            }

            conn.execute_batch("COMMIT").map_err(|e| format!("提交事务失败: {}", e))?;
            Ok(())
        };

        match do_update() {
            Ok(()) => Ok(()),
            Err(e) => {
                conn.execute_batch("ROLLBACK").ok();
                Err(e)
            }
        }
    }

    // ─── 私有辅助方法 ───

    /// 在已持锁的情况下按 ID 查询节点（避免重复加锁）。
    fn get_node_locked(&self, conn: &Connection, id: i64) -> Result<OutlineNode, String> {
        self.query_one(
            conn,
            "SELECT id, session_id, parent_id, content, sort_order, question_id, notes, tags,
                    collapsed, completed, marker, priority, note, created_at, updated_at
             FROM outline_nodes WHERE id = ?1",
            params![id],
        )?
        .ok_or_else(|| format!("大纲节点未找到: id={}", id))
    }

    /// 收集子树中所有节点 ID（DFS，含自身）。
    fn collect_subtree_ids(&self, conn: &Connection, root_id: i64) -> Result<Vec<i64>, String> {
        let mut ids = vec![root_id];
        let mut stack = vec![root_id];
        while let Some(current) = stack.pop() {
            let children: Vec<i64> = {
                let mut stmt = conn
                    .prepare("SELECT id FROM outline_nodes WHERE parent_id = ?1")
                    .map_err(|e| format!("查询子节点失败: {}", e))?;
                let rows = stmt
                    .query_map(params![current], |row| row.get(0))
                    .map_err(|e| format!("查询子节点失败: {}", e))?;
                rows.collect::<Result<Vec<i64>, _>>()
                    .map_err(|e| format!("读取子节点失败: {}", e))?
            };
            for child in children {
                ids.push(child);
                stack.push(child);
            }
        }
        Ok(ids)
    }

    /// 检查候选节点是否是祖先节点的后代。
    fn is_descendant(
        &self,
        conn: &Connection,
        candidate: i64,
        ancestor: i64,
    ) -> Result<bool, String> {
        let mut current = candidate;
        loop {
            let parent: Option<i64> = conn
                .query_row(
                    "SELECT parent_id FROM outline_nodes WHERE id = ?1",
                    params![current],
                    |row| row.get(0),
                )
                .map_err(|e| format!("查询父节点失败: {}", e))?;
            match parent {
                None => return Ok(false),
                Some(pid) if pid == ancestor => return Ok(true),
                Some(pid) => current = pid,
            }
        }
    }

    /// 查询单个节点。
    fn query_one(
        &self,
        conn: &Connection,
        sql: &str,
        p: impl rusqlite::Params,
    ) -> Result<Option<OutlineNode>, String> {
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;
        let mut rows = stmt
            .query_map(p, Self::row_to_node)
            .map_err(|e| format!("执行查询失败: {}", e))?;
        match rows.next() {
            Some(Ok(node)) => Ok(Some(node)),
            Some(Err(e)) => Err(format!("读取行失败: {}", e)),
            None => Ok(None),
        }
    }

    /// 查询节点列表。
    fn query_list(
        &self,
        conn: &Connection,
        sql: &str,
        p: impl rusqlite::Params,
    ) -> Result<Vec<OutlineNode>, String> {
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| format!("准备查询失败: {}", e))?;
        let rows = stmt
            .query_map(p, Self::row_to_node)
            .map_err(|e| format!("执行查询失败: {}", e))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("读取行失败: {}", e))?);
        }
        Ok(results)
    }

    /// 将数据库行映射为 OutlineNode。
    fn row_to_node(row: &rusqlite::Row) -> rusqlite::Result<OutlineNode> {
        let collapsed_int: i32 = row.get(8)?;
        let completed_int: i32 = row.get(9)?;
        Ok(OutlineNode {
            id: row.get(0)?,
            session_id: row.get(1)?,
            parent_id: row.get(2)?,
            content: row.get(3)?,
            sort_order: row.get(4)?,
            question_id: row.get(5)?,
            notes: row.get(6)?,
            tags: row.get(7)?,
            collapsed: collapsed_int != 0,
            completed: completed_int != 0,
            marker: row.get(10)?,
            priority: row.get(11)?,
            note: row.get(12)?,
            created_at: row.get(13)?,
            updated_at: row.get(14)?,
        })
    }

    // ─── 树结构构建与渲染 ───

    /// 从平铺列表构建树结构。
    fn build_tree_from_flat(nodes: &[OutlineNode]) -> Vec<TreeNode> {
        use std::collections::HashMap;

        // 构建 id → 节点映射
        let mut map: HashMap<i64, OutlineNode> = HashMap::new();
        for n in nodes {
            map.insert(n.id, n.clone());
        }

        // 构建 parent_id → children 映射
        let mut children_map: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
        for n in nodes {
            children_map
                .entry(n.parent_id)
                .or_default()
                .push(n.id);
        }

        // 递归构建树
        fn build_subtree(
            node_id: i64,
            map: &HashMap<i64, OutlineNode>,
            children_map: &HashMap<Option<i64>, Vec<i64>>,
        ) -> TreeNode {
            let node = map[&node_id].clone();
            let child_ids = children_map.get(&Some(node_id)).cloned().unwrap_or_default();
            let children = child_ids
                .into_iter()
                .map(|cid| build_subtree(cid, map, children_map))
                .collect();
            TreeNode { node, children }
        }

        // 从根节点开始
        let root_ids = children_map.get(&None).cloned().unwrap_or_default();
        root_ids
            .into_iter()
            .map(|rid| build_subtree(rid, &map, &children_map))
            .collect()
    }

    /// 渲染为缩进的 Markdown 无序列表。
    fn render_markdown_list(nodes: &[TreeNode], depth: usize) -> String {
        let indent = "  ".repeat(depth);
        let mut result = String::new();
        for node in nodes {
            result.push_str(&format!("{}- {}\n", indent, node.node.content));
            if !node.children.is_empty() {
                result.push_str(&Self::render_markdown_list(&node.children, depth + 1));
            }
        }
        result
    }

    /// 渲染为 Markdown 标题层级。
    fn render_markdown_headings(nodes: &[TreeNode], level: usize) -> String {
        let prefix = "#".repeat(level.min(6));
        let mut result = String::new();
        for node in nodes {
            result.push_str(&format!("{} {}\n\n", prefix, node.node.content));
            if !node.node.notes.is_empty() {
                result.push_str(&format!("{}\n\n", node.node.notes));
            }
            if !node.children.is_empty() {
                result.push_str(&Self::render_markdown_headings(&node.children, level + 1));
            }
        }
        result
    }

    /// 计算树的最大深度。
    fn calc_depth(nodes: &[TreeNode], current: usize) -> usize {
        if nodes.is_empty() {
            return current;
        }
        nodes
            .iter()
            .map(|n| Self::calc_depth(&n.children, current + 1))
            .max()
            .unwrap_or(current)
    }

    /// 计算所有节点中最大的直接子节点数。
    fn calc_max_children(&self, nodes: &[OutlineNode]) -> usize {
        use std::collections::HashMap;
        let mut child_count: HashMap<Option<i64>, usize> = HashMap::new();
        for n in nodes {
            *child_count.entry(n.parent_id).or_insert(0) += 1;
        }
        child_count.values().copied().max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_store() -> OutlineStore {
        OutlineStore::new_in_memory().expect("创建内存大纲存储失败")
    }

    #[test]
    fn test_create_root_node() {
        let store = new_store();
        // 注意：内存数据库没有 research_sessions 表，外键约束在测试中可能不生效
        // 这里只测试 outline_nodes 自身的逻辑
        let node = store.create_node(1, None, "根节点").unwrap();
        assert_eq!(node.content, "根节点");
        assert_eq!(node.parent_id, None);
        assert_eq!(node.session_id, 1);
        assert_eq!(node.sort_order, 1.0);
    }

    #[test]
    fn test_create_child_node() {
        let store = new_store();
        let root = store.create_node(1, None, "根").unwrap();
        let child = store.create_node(1, Some(root.id), "子节点").unwrap();
        assert_eq!(child.parent_id, Some(root.id));
        assert_eq!(child.sort_order, 1.0);
    }

    #[test]
    fn test_multiple_root_nodes_sort_order() {
        let store = new_store();
        let n1 = store.create_node(1, None, "A").unwrap();
        let n2 = store.create_node(1, None, "B").unwrap();
        let n3 = store.create_node(1, None, "C").unwrap();
        assert_eq!(n1.sort_order, 1.0);
        assert_eq!(n2.sort_order, 2.0);
        assert_eq!(n3.sort_order, 3.0);
    }

    #[test]
    fn test_update_node_partial() {
        let store = new_store();
        let node = store.create_node(1, None, "原始内容").unwrap();
        let updated = store
            .update_node(node.id, Some("新内容"), None, Some("[\"tag1\"]"), None, None, None, None, None)
            .unwrap();
        assert_eq!(updated.content, "新内容");
        assert_eq!(updated.notes, ""); // 未修改
        assert_eq!(updated.tags, "[\"tag1\"]");
    }

    #[test]
    fn test_delete_node_subtree() {
        let store = new_store();
        let root = store.create_node(1, None, "根").unwrap();
        let c1 = store.create_node(1, Some(root.id), "子1").unwrap();
        let _c2 = store.create_node(1, Some(root.id), "子2").unwrap();
        let _gc = store.create_node(1, Some(c1.id), "孙1").unwrap();

        store.delete_node_subtree(root.id).unwrap();
        let tree = store.get_tree(1).unwrap();
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_move_node_cycle_detection() {
        let store = new_store();
        let root = store.create_node(1, None, "根").unwrap();
        let child = store.create_node(1, Some(root.id), "子").unwrap();
        let grandchild = store.create_node(1, Some(child.id), "孙").unwrap();

        // 尝试将根节点移动到孙节点下 → 应报错
        let result = store.move_node(root.id, Some(grandchild.id), 1.0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("不能将节点移动到其子节点下"));
    }

    #[test]
    fn test_move_node_valid() {
        let store = new_store();
        let root1 = store.create_node(1, None, "根1").unwrap();
        let root2 = store.create_node(1, None, "根2").unwrap();
        let child = store.create_node(1, Some(root1.id), "子").unwrap();

        // 将子节点从根1移到根2下
        let moved = store.move_node(child.id, Some(root2.id), 1.0).unwrap();
        assert_eq!(moved.parent_id, Some(root2.id));
    }

    #[test]
    fn test_export_markdown_list() {
        let store = new_store();
        let root = store.create_node(1, None, "第一章").unwrap();
        store.create_node(1, Some(root.id), "1.1 节").unwrap();
        store.create_node(1, Some(root.id), "1.2 节").unwrap();

        let md = store.export_outline(1, "markdown_list").unwrap();
        assert!(md.contains("- 第一章"));
        assert!(md.contains("  - 1.1 节"));
        assert!(md.contains("  - 1.2 节"));
    }

    #[test]
    fn test_export_markdown_headings() {
        let store = new_store();
        let root = store.create_node(1, None, "概览").unwrap();
        store.create_node(1, Some(root.id), "背景").unwrap();

        let md = store.export_outline(1, "markdown_headings").unwrap();
        assert!(md.contains("## 概览"));
        assert!(md.contains("### 背景"));
    }

    #[test]
    fn test_export_unsupported_format() {
        let store = new_store();
        store.create_node(1, None, "节点").unwrap();
        let result = store.export_outline(1, "pdf");
        assert!(result.is_err());
    }

    #[test]
    fn test_outline_stats() {
        let store = new_store();
        let root = store.create_node(1, None, "根").unwrap();
        let c1 = store.create_node(1, Some(root.id), "子1").unwrap();
        store.create_node(1, Some(root.id), "子2").unwrap();
        store.create_node(1, Some(c1.id), "孙1").unwrap();

        let stats = store.get_outline_stats(1).unwrap();
        assert_eq!(stats["total_nodes"], 4);
        assert_eq!(stats["depth"], 3); // 根→子→孙
        assert_eq!(stats["max_children"], 2); // 根有2个子节点
    }

    #[test]
    fn test_import_markdown_outline() {
        let store = new_store();
        let markdown = "\
## 一级节点A
这是A的备注。
第一行。
第二行。

### 二级节点A1
这是A1的备注。

## 一级节点B
这是B的备注。
";
        store.import_markdown_outline(1, markdown).unwrap();
        let tree = store.get_tree(1).unwrap();
        assert_eq!(tree.len(), 3);

        // 验证节点 A
        let node_a = tree.iter().find(|n| n.content == "一级节点A").unwrap();
        assert_eq!(node_a.parent_id, None);
        assert_eq!(node_a.notes, "这是A的备注。\n第一行。\n第二行。");

        // 验证节点 A1
        let node_a1 = tree.iter().find(|n| n.content == "二级节点A1").unwrap();
        assert_eq!(node_a1.parent_id, Some(node_a.id));
        assert_eq!(node_a1.notes, "这是A1的备注。");

        // 验证节点 B
        let node_b = tree.iter().find(|n| n.content == "一级节点B").unwrap();
        assert_eq!(node_b.parent_id, None);
        assert_eq!(node_b.notes, "这是B的备注。");

        // 测试增量 Diff 同步：修改、添加、删除
        let new_markdown = "\
## 一级节点A
这是A的修改后备注。

## 一级节点C
";
        store.import_markdown_outline(1, new_markdown).unwrap();
        let new_tree = store.get_tree(1).unwrap();
        
        // 旧的 A1 和 B 应该被删除了，只剩下 A 和新加的 C
        assert_eq!(new_tree.len(), 2);
        let updated_a = new_tree.iter().find(|n| n.content == "一级节点A").unwrap();
        // A 的 ID 应该保持不变以验证重用
        assert_eq!(updated_a.id, node_a.id);
        assert_eq!(updated_a.notes, "这是A的修改后备注。");

        let node_c = new_tree.iter().find(|n| n.content == "一级节点C").unwrap();
        assert_eq!(node_c.parent_id, None);
    }
}
