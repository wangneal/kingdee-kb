# 知识库重构与调研大纲脑图编辑器 — 实现计划 (PLAN) 专项评审报告

针对实现计划 [2026-06-01-kb-refactor-research-outliner-plan.md](file:///E:/projects/kingdee/KingdeeKB/docs/superpowers/plans/2026-06-01-kb-refactor-research-outliner-plan.md) 进行了细致的代码和执行步骤层级审查，我们发现了若干影响**多线程编译、数据一致性、算法安全与级联逻辑**的计划执行硬伤：

---

## 1. 知识库基础设施重构 (阶段一) 计划缺陷

### 1.1 `last_insert_rowid()` 在 `UPDATE` 机制下无法获取自增 Key (P0 级)
* **计划实现 (L119-L126)**：
  在 `next_vector_key` 方法中，执行 `UPDATE vector_key_seq SET next_key = next_key + 1`，随后尝试通过 `self.db.last_insert_rowid()` 返回最新 Key。
* **致命缺陷**：
  SQLite 中 `last_insert_rowid()` 仅受 `INSERT` 驱动，执行 `UPDATE` 完全无法更新此 ID。这会导致所有新生成的 chunk 使用同一个旧的向量 Key，发生严重的向量覆盖和冲突。
* **建议整改**：
  改为**空行插入自增模式**：
  ```rust
  pub fn next_vector_key(&self) -> Result<i64, String> {
      self.db.execute("INSERT INTO vector_key_seq DEFAULT VALUES", [])
          .map_err(|e| format!("生成向量Key失败: {}", e))?;
      Ok(self.db.last_insert_rowid())
  }
  ```

### 1.2 `RwLock` 包装 `!Sync` 连接引发 Rust 编译错误 (P0 级)
* **计划实现 (L477)**：
  在 `app_state.rs` 中声明 `pub outline_store: RwLock<OutlineStore>`，其中 `OutlineStore` 持有 `Connection`。
* **致命缺陷**：
  在 Rust 中，`rusqlite::Connection` 并不实现 `Sync`（线程安全）。将其放入 `RwLock` 会导致 `RwLock<OutlineStore>` 也不是 `Sync`，使得 Tauri 全局共享的 `AppState` 编译失败。
* **建议整改**：
  由于数据库写锁本身互斥，在 `app_state.rs` 中应将 `outline_store` 的包装方式改为 `Mutex`：
  ```rust
  pub outline_store: Mutex<OutlineStore>,
  ```

---

## 2. 调研脑图编辑器设计 (阶段二) 计划缺陷

### 2.1 大纲移动时双链表断裂风险 (P1 级)
* **计划实现 (L418-L424)**：
  在 `move_node` 中执行单行更新：
  ```rust
  pub fn move_node(&self, id: i64, new_parent_id: Option<i64>, new_left_id: Option<i64>) -> Result<(), String> {
      self.db.execute("UPDATE outline_nodes SET parent_id = ?1, left_id = ?2 WHERE id = ?3", ...)
  }
  ```
* **致命缺陷**：
  移动节点时仅修改当前节点的 `left_id`，但原位置后继节点的 `left_id`（失去了前驱）和新位置后继节点的 `left_id`（插了新前驱）并没有在事务中同步修改。这会导致链表发生多处断裂，在依据 `left_id` 重组树时会导致大量节点丢失。
* **建议整改**：
  废除 `left_id` 设计，完全在 `REAL`（浮点数）类型的 `sort_order` 字段上使用 **Fractional Indexing** 算法实现 $O(1)$ 的排序位置更新。

### 2.2 防自环判定缺失引发前端构建死循环 (P1 级)
* **计划实现**：
  在 Rust 接口中无限制地直接更新父节点，没有在后端校验父子树的合法性。
* **致命缺陷**：
  一旦用户误将父节点拖入自身的子孙节点下，数据库会出现回路。前端 `buildTree` 在进行递归解析时将陷入无限死循环，造成浏览器栈溢出崩溃。
* **建议整改**：
  后端接口更新前，递归判断新父节点是否属于当前节点的子树，若是则返回错误。

### 2.3 外键自动级联删除与手动 Rust 递归删除冲突冗余 (P2 级)
* **计划实现 (L426-L440)**：
  在 `delete_node` 中在 Rust 层查出所有子节点手动递归执行删除，最后删除顶级节点。
* **致命缺陷**：
  由于在表结构中已经声明了 `parent_id REFERENCES outline_nodes(id) ON DELETE CASCADE`。当删除顶级节点时，SQLite 底层会自动级联清理子节点。Rust 层的递归删除不仅属于冗余的高开销操作，而且还会因为外键级联删除了节点，导致 Rust 后续的 `DELETE` 语句找不到数据而抛出报错，破坏正常的删除事务。
* **建议整改**：
  利用外键优势，直接在 Rust 中执行单次物理删除，由 SQLite 底层自动完成全部子节点的递归清理：
  ```rust
  pub fn delete_node(&self, id: i64) -> Result<(), String> {
      self.db.execute("DELETE FROM outline_nodes WHERE id = ?1", params![id])?;
      Ok(())
  }
  ```

---

## 3. 历史问答数据集成割裂 (第 4 节) 计划缺陷

### 3.1 迁移脚本未能导入已有的存量问答记录 (P1 级)
* **计划实现 (L481-L484)**：
  迁移脚本仅在 `outline_nodes` 中为 Session 插入根节点。
* **致命缺陷**：
  这会导致老 Session 中已有的 `session_qa_records` 数据在新的大纲编辑器和思维导图页面上完全不可见，造成严重的历史数据孤立割裂。
* **建议整改**：
  修改迁移脚本，在为 Session 创建顶级根节点后，**自动将现有的 `session_qa_records` 转换为大纲根节点下的第一层子叶子节点**（关联对应的外键 `question_id`）。
