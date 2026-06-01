# 知识库重构与调研脑图编辑器 — 设计规格书 (SPEC) 独立评审报告

为了提供最严谨的架构设计保障，我们在此排除实现计划（PLAN）的干扰，**仅对设计规格书 [2026-06-01-kb-refactor-research-outliner-spec.md](file:///E:/projects/kingdee/KingdeeKB/docs/superpowers/plans/2026-06-01-kb-refactor-research-outliner-spec.md) 的设计規格、接口契约、数据库Schema和业务自洽性**进行独立的闭环审查。

审查中发现了 10 处直接影响**系统自洽性、数据完整性、渲染正确性、并发安全与防止崩溃**的规格级设计缺陷：

---

## 1. 调研脑图编辑器设计 (工作项 B) 规格缺陷

### 1.1 `left_id` 与 `sort_order` 的双源设计矛盾 (P0 级)
* **规格事实**：
  - 3.2 节大纲表中同时定义了 `left_id` (链表指针) 与 `sort_order` (绝对序号)。
  - 3.3 节的 `move_outline_node` API 规格仅接收 `new_left_id`：
    ```rust
    fn move_outline_node(id: i64, new_parent_id: Option<i64>, new_left_id: Option<i64>)
    ```
  - 3.3 节的 `get_outline_tree` 规格依然使用 `ORDER BY sort_order`。
* **逻辑硬伤**：
  移动节点时仅更新了 `left_id`，但节点的 `sort_order` 在数据库中保持不变。由于查询依然使用 `ORDER BY sort_order` 排序，**导致拖拽排序的结果在页面刷新或重新拉取树后失效，恢复为旧位置**。这种双数据源设计在逻辑上不闭环。
* **修正规格**：
  **废除 `left_id` 字段**。在规格中，大纲顺序完全由浮点数 (`REAL`) 类型的 `sort_order` 实现。API 规格变更为：
  ```rust
  #[tauri::command]
  fn move_outline_node(id: i64, new_parent_id: Option<i64>, new_sort_order: f64) -> Result<(), String>;
  ```
  利用 **Fractional Indexing** 算法（新位置序号取目标前后兄弟序号的平均值），只需 `UPDATE` 移动节点的单行数据即可实现 $O(1)$ 的排序更新。

### 1.2 关联问答删除引发 SQLite 外键级联阻塞故障 (P1 级)
* **规格事实**：
  大纲表中 `question_id REFERENCES session_qa_records(id)` 指向问答表，且 3.6 节规定“大纲删除节点仅解除与 QA 记录的关联，不删除 QA”。
* **逻辑硬伤**：
  当开启 SQLite 的 `PRAGMA foreign_keys = ON;` 强约束后，由于大纲外键定义**未指定任何删除动作**（默认为 Restrict），一旦用户在问答页面尝试删除一条 QA 记录，由于大纲表中仍有节点的 `question_id` 指向该记录，SQLite 会**直接抛出外键强约束错误并拒绝该删除操作**，导致问答界面的删除逻辑崩溃。
* **修正规格**：
  外键定义规格必须追加 `ON DELETE SET NULL`，确保 QA 被删除时，大纲表自动将对应外键设为 `NULL` 以解除关联：
  ```sql
  question_id INTEGER REFERENCES session_qa_records(id) ON DELETE SET NULL
  ```

### 1.3 大纲节点移动防成环判定规约缺失 (P1 级)
* **规格事实**：
  3.3 节的 `move_outline_node` 仅定义了基本的字段更新，未对树的闭环路径做限制。
* **逻辑硬伤**：
  当用户拖拽节点时，如果无意中把一个父节点拖入它自己的子树中，会导致树形关系成环（A 的父节点是 B，B 的父节点是 A）。前端在解析和递归构建树时会陷入无限死循环，造成浏览器崩溃或栈溢出。
* **修正规格**：
  大纲 API 必须强制定义 **防自环校验规则**：后端在 `move_outline_node` 执行前，必须递归校验 `new_parent_id` 是否属于 `id` 本身或其任何子树节点，若是则直接返回拒绝移动的错误信息。

### 1.4 大纲多视图/多窗口并发编辑下的状态同步通知缺失 (P1 级)
* **规格事实**：
  Tantivy 和 SQLite 后端通过 API 直接存取。
* **逻辑硬伤**：
  由于 Tauri 支持打开多窗口（或者前台包含大纲视图、问答视图等多视图）。当用户在一个视图中修改了大纲的层级，其他视图由于没有接收到状态更新通知，会继续缓存和显示旧的树状结构，当用户再次保存时会导致**覆盖冲突和数据乱序**。
* **修正规格**：
  所有大纲写入/移动/删除 API 规格必须在执行成功后，通过 Tauri **全局事件总线广播变更通知**：
  ```rust
  // 后端广播通知规格
  app_handle.emit("outline:changed", session_id)?;
  ```
  前端 `OutlineContext` 在初始化时注册此监听事件，收到广播后自动重新拉取 `get_outline_tree`，保证多视图数据最终强一致。

### 1.5 大纲节点对 ASR 语音录音输入交互支持的规格遗漏 (P2 级)
* **规格事实**：
  大纲节点 content 和 note 属于普通 markdown 文本域，但未列出任何语音填充设计。
* **逻辑硬伤**：
  现有的调研助手问答界面支持语音录音转文字输入。如果在大纲节点高频录入和编辑时，完全遗漏了 ASR 的语音支持，会导致大纲编辑器的输入体验与系统现有的语音功能脱节。
* **修正规格**：
  规格书交互规范应追加 **“ASR 语音节点编辑支持”**。用户在聚焦节点输入时，支持点击麦克风或使用全局快捷键触发 `asr_config` 中的语音转文字录入，自动将转录文字追加填充进大纲节点的 content/note 中。

### 1.6 大纲导出 Markdown 格式与脑图渲染规范存在冲突 (P2 级)
* **规格事实**：
  在 3.3 节中，定义了 `export_outline(session_id, format: "markdown")` API。同时，3.4 节脑图渲染组件中，使用 `treeToMarkdown` 生成的多级标题形式（`#`、`##` 等）来供给 markmap 脑图插件解析。
* **逻辑硬伤**：
  - 如果大纲导出的 Markdown 作为文本文件输出给用户，使用多级标题会导致排版极其混乱（例如深度为 5 的节点会产生 `##### 节点内容`，这不符合正常文本大纲阅读心智，正常应为缩进无序列表 `- 节点内容`）。
  - 而如果大纲直接导出为缩进无序列表，则会使得 `markmap` 脑图解析失败（`markmap-lib` 默认基于标题层级划分脑图分支效果最佳）。
* **修正规格**：
  在接口规格中明确定义 **两种 Markdown 导出渲染模式**：
  - `export_outline(session_id, format: "markdown_list")`：将大纲转换为**嵌套缩进无序列表**（使用 `-` 与四空格缩进），用于用户复制和导出阅读。
  - `export_outline(session_id, format: "markdown_headings")`：将大纲转换为**多级标题格式**（使用 `#` 级联深度），专门用于 markmap 脑图的渲染输入。

### 1.7 级联物理删除大纲子节点时的孤儿 QA 垃圾堆积漏洞 (P2 级)
* **规格事实**：
  大纲节点表声明了 `parent_id REFERENCES outline_nodes(id) ON DELETE CASCADE`。
* **逻辑硬伤**：
  当用户删除大纲中的一个顶级节点时，SQLite 会级联自动清理其下所有子大纲节点。由于大纲与 QA 的关联是弱外键关系，这些被清理的子大纲节点所关联的 `session_qa_records` 记录不会被删除。这会导致在多次大纲删除和重构操作后，数据库中堆积了大量“在 UI 上不可见、无法通过大纲查询、且不隶属于任何节点”的**孤儿问答数据**，造成垃圾数据在底层单调膨胀。
* **修正规格**：
  在后端 `delete_outline_node` 触发物理删除前，必须在一个事务内执行垃圾回收规则：
  ```sql
  -- 清理即将被级联删除的子节点所关联的、且未被用户标记为收藏/归档的临时 AI 问答记录
  DELETE FROM session_qa_records 
  WHERE id IN (
      SELECT question_id FROM outline_nodes 
      WHERE parent_id = ?1 OR id = ?1
  ) AND is_starred = 0; -- 仅当 QA 未被用户主动星标/归档时清理
  ```

---

## 2. 知识库重构设计 (工作项 A) 规格缺陷

### 2.1 基于 `last_insert_rowid` 向量 Key 生成在 SQLite 下无效 (P0 级)
* **规格事实**：
  2.3.2 节定义 `next_vector_key` 时，通过执行 `UPDATE vector_key_seq SET next_key = next_key + 1`，随后调用 `last_insert_rowid()` 获取最新 Key。
* **技术硬伤**：
  在 SQLite 中，`last_insert_rowid()` **只反映最后一次成功的 `INSERT` 操作**自动分配的行 ID。执行 `UPDATE` **完全不会更新** `last_insert_rowid`。若如此设计，该 API 将永远返回在此之前某次无关 `INSERT` 的旧 ID，导致新向量在摄入时由于 Key 冲突覆盖旧向量。
* **修正规格**：
  序列表改为支持自增列的**单行插入自增模式**：
  ```sql
  CREATE TABLE IF NOT EXISTS vector_key_seq (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      created_at TEXT DEFAULT (datetime('now'))
  );
  ```
  在 Rust 规格中，通过执行 `INSERT` 产生最新 Key：
  ```rust
  pub fn next_vector_key(&self) -> Result<i64, String> {
      self.db.execute("INSERT INTO vector_key_seq DEFAULT VALUES", [])?;
      Ok(self.db.last_insert_rowid())
  }
  ```

### 2.2 偏离克隆参考项目 llm_wiki 的 Step 1 纯本地分析退化 (P1 级)
* **规格事实**：
  2.4.2 节的“两步法摄入”中，将 Step 1 (Analysis) 定义为“纯 Rust 实现（命名实体识别 + TF-IDF 关键词 + 标题分析）”。
* **逻辑硬伤**：
  参考项目 `llm_wiki` 摄入核心在于 **Step 1 (Analysis) 也是通过 LLM 语义提炼进行的**（提炼核心实体、关键概念及与已有 Wiki 的矛盾和交叉关联）。如果我们的规格将其降级为本地纯 Rust 的正则/传统提取，会导致 Step 2 (Generation) 的 LLM 无法接收到高维度的语义关联输入，导致重构后的知识库检索退化。
* **修正规格**：
  在设计规格中，Step 1 必须具备“LLM 深度语义分析”（在有 LLM 配额时，作为主要提取引擎）与“本地 Rust 正则词频提取”（降级引擎）两套配置规格，确保性能对齐 `llm_wiki`。

### 2.3 增量缓存表 `ingest_cache` 的物理模型缺失 (P1 级)
* **规格事实**：
  2.4.3 节定义了“SHA256 增量缓存”，但没有定义支撑缓存所必需的物理模型。
* **逻辑硬伤**：
  如果没有物理表记录“源文件 SHA256 -> 衍生生成的所有 Wiki 文件”的映射关系，系统在判定缓存命中后将无法知道该源文件生成了哪些文档以供后续的检索、更新和同步删除。
* **修正规格**：
  必须在数据库设计 and 迁移小节中追加 `ingest_cache` 表的定义：
  ```sql
  CREATE TABLE IF NOT EXISTS ingest_cache (
      source_identity TEXT PRIMARY KEY,
      sha256          TEXT NOT NULL,
      generated_files TEXT NOT NULL, -- JSON 格式存放生成的 markdown 路径数组
      updated_at      TEXT DEFAULT (datetime('now'))
  );
  ```

---

## 3. 数据库迁移 (第 4 节) 数据割裂缺陷

### 3.1 迁移脚本未能导入已有的存量问答记录 (P1 级)
* **规格事实**：
  迁移规格脚本 (L481-L484) 仅在 `outline_nodes` 中为 Session 插入根节点。
* **逻辑硬伤**：
  如果用户在旧版本中存在大量的 `session_qa_records` 历史数据，迁移后这些数据在大纲编辑器和思维导图页面上**完全不可见**，这会导致严重的“历史资产丢失”和功能割裂。
* **修正规格**：
  修改迁移规格，在为 Session 创建顶级根节点后，**将现有的 `session_qa_records` 转换为大纲根节点下的第一层子节点**，并建立相应关联：

```sql
-- 1. 数据迁移：第一步，为所有无根节点的 Session 创建大纲顶级根节点
INSERT INTO outline_nodes (session_id, content, sort_order)
SELECT id, title, 0.0 FROM research_sessions
WHERE id NOT IN (SELECT session_id FROM outline_nodes WHERE parent_id IS NULL);

-- 2. 数据迁移：第二步，将已有的历史 QA 记录转换并附加为该根节点下的子节点
INSERT INTO outline_nodes (session_id, parent_id, content, note, question_id, sort_order)
SELECT 
    q.session_id, 
    r.id AS parent_id, 
    q.question_text AS content, 
    q.answer_text AS note, 
    q.id AS question_id, 
    CAST(q.sort_order AS REAL)
FROM session_qa_records q
JOIN outline_nodes r ON r.session_id = q.session_id AND r.parent_id IS NULL
WHERE q.id NOT IN (SELECT COALESCE(question_id, 0) FROM outline_nodes);
```
