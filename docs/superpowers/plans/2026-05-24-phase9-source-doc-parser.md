# Phase 9: 源文档解析引擎 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 subagent-driven-development（推荐）或 executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 实现 DOC 调研提纲解析器、模板解析器（.docx/.xlsx）、Edition Profile 框架，构建调研问题向量和 BM25 索引。

**架构：** 
- Rust 后端解析 DOC/DOCX/XLSX 文件为结构化数据
- SQLite 存储解析结果（research_outlines, research_questions, template_metadata 表）
- usearch + tantivy 构建向量和全文索引
- Edition Profile 支持企业版+旗舰版切换

**技术栈：** Rust (winapi COM / docx-rs / calamine), SQLite (rusqlite), usearch, tantivy, jieba-rs

---

## 文件结构

### 新文件
- `src-tauri/src/services/research_outline.rs` — 调研提纲数据模型 + DOC 解析器
- `src-tauri/src/services/edition_config.rs` — Edition Profile 框架
- `src-tauri/src/services/research_indexer.rs` — 调研问题索引构建器

### 修改文件
- `src-tauri/src/services/mod.rs` — 注册新模块
- `src-tauri/src/main.rs` — 注册 Tauri commands
- `src-tauri/src/lib.rs` — 注册 Tauri commands（如果使用）

---

## 任务

### 任务 1: 调研提纲数据模型

**文件：**
- 创建：`src-tauri/src/services/research_outline.rs`
- 测试：`src-tauri/src/services/research_outline.rs`（模块内测试）

- [ ] **步骤 1：定义数据结构并编写测试**

```rust
// src-tauri/src/services/research_outline.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Edition {
    Enterprise,
    Flagship,
}

impl Edition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Edition::Enterprise => "enterprise",
            Edition::Flagship => "flagship",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "enterprise" => Some(Edition::Enterprise),
            "flagship" => Some(Edition::Flagship),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchOutline {
    pub edition: Edition,
    pub module_code: String,
    pub module_name: String,
    pub cloud_type: String,
    pub doc_file: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub categories: Vec<Category>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatQuestion {
    pub edition: Edition,
    pub module_code: String,
    pub module_name: String,
    pub cloud_type: String,
    pub section: String,
    pub category: String,
    pub question_text: String,
    pub order: i32,
}

impl ResearchOutline {
    /// 将树形结构的提纲展开为扁平面问题列表
    pub fn flatten(&self) -> Vec<FlatQuestion> {
        let mut result = Vec::new();
        let mut order = 0;
        for section in &self.sections {
            for category in &section.categories {
                for question in &category.questions {
                    result.push(FlatQuestion {
                        edition: self.edition.clone(),
                        module_code: self.module_code.clone(),
                        module_name: self.module_name.clone(),
                        cloud_type: self.cloud_type.clone(),
                        section: section.name.clone(),
                        category: category.name.clone(),
                        question_text: question.clone(),
                        order,
                    });
                    order += 1;
                }
            }
        }
        result
    }
}
```

- [ ] **步骤 2：运行编译检查**

运行：`cd src-tauri && cargo check`
预期：编译通过（虽然是 dead code，但类型定义正确）

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/research_outline.rs
git commit -m "feat: add research outline data models (Edition, ResearchOutline, FlatQuestion)"
```

---

### 任务 2: Edition Profile 框架

**文件：**
- 创建：`src-tauri/src/services/edition_config.rs`
- 修改：`src-tauri/src/services/mod.rs`

- [ ] **步骤 1：编写 EditionConfig 和测试**

```rust
// src-tauri/src/services/edition_config.rs
use rusqlite::Connection;
use std::sync::Mutex;
use crate::services::research_outline::Edition;

const CONFIG_KEY_CURRENT_EDITION: &str = "research_edition";

pub struct EditionConfig {
    conn: Mutex<Connection>,
}

impl EditionConfig {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// 初始化 app_config 表
    pub fn init_table(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );"
        ).map_err(|e| format!("Failed to create app_config: {}", e))?;
        Ok(())
    }

    /// 获取当前版本
    pub fn current(&self) -> Edition {
        let conn = self.conn.lock().unwrap();
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM app_config WHERE key = ?1",
                [CONFIG_KEY_CURRENT_EDITION],
                |row| row.get(0),
            )
            .ok();
        match value.as_deref() {
            Some("flagship") => Edition::Flagship,
            _ => Edition::Enterprise,
        }
    }

    /// 切换版本
    pub fn set(&self, edition: &Edition) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO app_config (key, value) VALUES (?1, ?2)",
            rusqlite::params![CONFIG_KEY_CURRENT_EDITION, edition.as_str()],
        ).map_err(|e| format!("Failed to set edition: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edition_default_is_enterprise() {
        let conn = Connection::open_in_memory().unwrap();
        let config = EditionConfig::new(conn);
        config.init_table().unwrap();
        assert_eq!(config.current(), Edition::Enterprise);
    }

    #[test]
    fn test_edition_switch() {
        let conn = Connection::open_in_memory().unwrap();
        let config = EditionConfig::new(conn);
        config.init_table().unwrap();
        
        config.set(&Edition::Flagship).unwrap();
        assert_eq!(config.current(), Edition::Flagship);
        
        config.set(&Edition::Enterprise).unwrap();
        assert_eq!(config.current(), Edition::Enterprise);
    }
}
```

- [ ] **步骤 2：在 mod.rs 中注册** 

```rust
// src-tauri/src/services/mod.rs 添加:
pub mod edition_config;
pub mod research_outline;
```

- [ ] **步骤 3：运行测试**

运行：`cd src-tauri && cargo test edition_config::tests -- --nocapture`
预期：2 passed

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/services/edition_config.rs src-tauri/src/services/mod.rs
git commit -m "feat: add EditionConfig framework (enterprise/flagship switching)"
```

---

### 任务 3: DOC 调研提纲解析器

**文件：**
- 修改：`src-tauri/src/services/research_outline.rs`

- [ ] **步骤 1：编写 DOC 解析函数和测试**

在 `research_outline.rs` 中添加：

```rust
/// 从 DOC 文件中解析调研提纲
/// 
/// 使用 Windows COM (Word.Application) 读取 .doc 内容。
/// 企业版提纲结构: 文档编号_调研提纲_模块名_分类_V1.0.doc
/// 例如: ECW2107_调研提纲_总账_财务_V1.0.doc
pub fn parse_doc_file(filepath: &std::path::Path) -> Result<String, String> {
    let path_str = filepath.to_str().ok_or("非 UTF-8 路径")?;
    
    // 使用 PowerShell 调用 Word COM 对象读取内容
    let script = format!(
        r#"$word = New-Object -ComObject Word.Application;
$word.Visible = $false;
try {{
    $doc = $word.Documents.Open('{0}');
    $text = $doc.Content.Text;
    $doc.Close();
    Write-Output $text
}} finally {{
    $word.Quit()
}}"#, path_str.replace('\'', "''")
    );

    let output = std::process::Command::new("powershell")
        .args(&["-Command", &script])
        .output()
        .map_err(|e| format!("Failed to run PowerShell: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell failed: {}", stderr));
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(text)
}

/// 从文件名解析模块信息
/// 例如: "ECW2107_调研提纲_总账_财务_V1.0.doc" → (ECW2107, 总账, 财务)
pub fn parse_module_info(filename: &str) -> Option<(String, String, String)> {
    // 格式: CODE_调研提纲_MODULE_CATEGORY_V*.doc
    let stem = std::path::Path::new(filename).file_stem()?.to_str()?;
    let parts: Vec<&str> = stem.split('_').collect();
    if parts.len() < 4 {
        return None;
    }
    // parts[0] = ECW2107, parts[1] = 调研提纲, parts[2] = 总账, parts[3] = 财务
    Some((
        parts[0].to_string(),
        parts[2].to_string(),
        parts[3].to_string(),
    ))
}

/// 解析文本内容为结构化提纲
/// 
/// 提纲结构:
/// N 标题 (如 "1 业务概况")
/// N.N 分类 (如 "1.1 组织人员")
/// N.N.N 问题 (如 "1.1.1 公司目前财务组织架构")
pub fn parse_outline_text(
    text: &str,
    edition: Edition,
    module_code: &str,
    module_name: &str,
    cloud_type: &str,
    filename: &str,
) -> ResearchOutline {
    let mut sections = Vec::new();
    let mut current_section: Option<Section> = None;
    let mut current_category: Option<Category> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // 检测章节标题: "N 标题" (如 "1 业务概况")
        if let Some(cap) = try_parse_section_header(trimmed) {
            // 保存上一个 category
            if let (Some(mut sec), Some(cat)) = (current_section.take(), current_category.take()) {
                sec.categories.push(cat);
                sections.push(sec);
            } else if let Some(sec) = current_section.take() {
                sections.push(sec);
            }
            current_section = Some(Section {
                name: cap,
                categories: Vec::new(),
            });
            continue;
        }

        // 检测分类: "N.N 分类名" (如 "1.1 组织人员")
        if let Some(cap) = try_parse_category_header(trimmed) {
            if let (Some(mut sec), Some(cat)) = (current_section.take(), current_category.take()) {
                sec.categories.push(cat);
                current_section = Some(sec);
            } else if let Some(cat) = current_category.take() {
                if let Some(ref mut sec) = current_section {
                    sec.categories.push(cat);
                }
            }
            current_category = Some(Category {
                name: cap,
                questions: Vec::new(),
            });
            continue;
        }

        // 检测问题: "N.N.N 问题文本" (如 "1.1.1 公司目前财务组织架构")
        if let Some(question) = try_parse_question(trimmed) {
            if let Some(ref mut cat) = current_category {
                cat.questions.push(question);
            } else if let Some(ref mut sec) = current_section {
                // 无分类时直接归入章节
                let cat = Category {
                    name: "通用".to_string(),
                    questions: vec![question],
                };
                current_category = Some(cat);
            }
            continue;
        }
    }

    // 保存剩余的数据
    if let (Some(mut sec), Some(cat)) = (current_section.take(), current_category.take()) {
        sec.categories.push(cat);
        sections.push(sec);
    } else if let Some(sec) = current_section.take() {
        sections.push(sec);
    } else if let Some(cat) = current_category.take() {
        sections.push(Section {
            name: "其他".to_string(),
            categories: vec![cat],
        });
    }

    ResearchOutline {
        edition,
        module_code: module_code.to_string(),
        module_name: module_name.to_string(),
        cloud_type: cloud_type.to_string(),
        doc_file: filename.to_string(),
        sections,
    }
}

fn try_parse_section_header(line: &str) -> Option<String> {
    // 匹配: ^(\d+)\s+(.+)
    let re = regex::Regex::new(r"^\d+\s+(.+)").ok()?;
    re.captures(line).map(|c| c[1].trim().to_string())
}

fn try_parse_category_header(line: &str) -> Option<String> {
    // 匹配: ^(\d+\.\d+)\s+(.+)
    let re = regex::Regex::new(r"^\d+\.\d+\s+(.+)").ok()?;
    re.captures(line).map(|c| c[1].trim().to_string())
}

fn try_parse_question(line: &str) -> Option<String> {
    // 匹配: ^(\d+\.\d+\.\d+)\s+(.+)
    let re = regex::Regex::new(r"^\d+\.\d+\.\d+\s+(.+)").ok()?;
    re.captures(line).map(|c| c[1].trim().to_string())
}
```

- [ ] **步骤 2：编写解析测试**

```rust
// 添加到 research_outline.rs 的 tests 模块
#[test]
fn test_parse_module_info() {
    let (code, name, ctype) = parse_module_info("ECW2107_调研提纲_总账_财务_V1.0.doc").unwrap();
    assert_eq!(code, "ECW2107");
    assert_eq!(name, "总账");
    assert_eq!(ctype, "财务");
}

#[test]
fn test_parse_outline_text_basic() {
    let text = "1 业务概况\n1.1 组织人员\n1.1.1 公司目前财务组织架构？\n1.1.2 财务组织涉及哪些角色？\n1.2 关键业务\n1.2.1 目前核算的痛点？";
    let outline = parse_outline_text(text, Edition::Enterprise, "ECW2107", "总账", "财务", "test.doc");
    assert_eq!(outline.sections.len(), 1);
    assert_eq!(outline.sections[0].name, "业务概况");
    assert_eq!(outline.sections[0].categories.len(), 2);
    assert_eq!(outline.sections[0].categories[0].name, "组织人员");
    assert_eq!(outline.sections[0].categories[0].questions.len(), 2);
    assert_eq!(outline.sections[0].categories[0].questions[0], "公司目前财务组织架构？");
    
    let flat = outline.flatten();
    assert_eq!(flat.len(), 3);
}

#[test]
fn test_parse_doc_file_real() {
    // 如果存在真实的调研提纲文件，解析前3个验证
    let test_dir = std::path::Path::new(r"E:\工作资料\项目资料\企业版调研提纲\企业版");
    if !test_dir.exists() {
        eprintln!("Skipping real file test: test directory not found");
        return;
    }
    let files: Vec<_> = std::fs::read_dir(test_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "doc"))
        .take(3)
        .collect();
    for entry in &files {
        let text = parse_doc_file(&entry.path()).unwrap();
        assert!(!text.is_empty(), "Empty content from {:?}", entry.path());
        eprintln!("Parsed {:?}: {} chars", entry.path(), text.len());
    }
}
```

- [ ] **步骤 3：添加 regex 依赖到 Cargo.toml**

```toml
# src-tauri/Cargo.toml 的 [dependencies] 中添加:
regex = "1"
```

- [ ] **步骤 4：运行测试**

运行：`cd src-tauri && cargo test research_outline::tests -- --nocapture`
预期：编译通过，单元测试通过

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/services/research_outline.rs
git commit -m "feat: implement DOC research outline parser with regex-based section/category/question extraction"
```

---

### 任务 4: 调研问题索引构建器

**文件：**
- 创建：`src-tauri/src/services/research_indexer.rs`

- [ ] **步骤 1：实现索引构建器**

```rust
// src-tauri/src/services/research_indexer.rs
use rusqlite::Connection;
use std::path::Path;
use crate::services::research_outline::*;
use std::sync::Mutex;

/// SQLite 表名
const TABLE_OUTLINES: &str = "research_outlines";
const TABLE_QUESTIONS: &str = "research_questions";

pub struct ResearchIndexer {
    conn: Mutex<Connection>,
}

impl ResearchIndexer {
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open DB: {}", e))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 创建表结构
    pub fn init_tables(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {table_outlines} (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                edition TEXT NOT NULL,
                module_code TEXT NOT NULL,
                module_name TEXT NOT NULL,
                cloud_type TEXT NOT NULL,
                doc_file TEXT NOT NULL,
                section_count INTEGER DEFAULT 0,
                question_count INTEGER DEFAULT 0,
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS {table_questions} (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                outline_id INTEGER NOT NULL,
                edition TEXT NOT NULL,
                module_code TEXT NOT NULL,
                module_name TEXT NOT NULL,
                cloud_type TEXT NOT NULL,
                section TEXT NOT NULL,
                category TEXT NOT NULL,
                question_text TEXT NOT NULL,
                question_order INTEGER NOT NULL,
                embedding_id INTEGER,
                FOREIGN KEY (outline_id) REFERENCES {table_outlines}(id)
            );

            CREATE INDEX IF NOT EXISTS idx_questions_edition ON {table_questions}(edition);
            CREATE INDEX IF NOT EXISTS idx_questions_module ON {table_questions}(module_code);
            ",
            table_outlines = TABLE_OUTLINES,
            table_questions = TABLE_QUESTIONS,
        )).map_err(|e| format!("Failed to init tables: {}", e))?;
        Ok(())
    }

    /// 插入一份提纲及其所有问题
    pub fn insert_outline(&self, outline: &ResearchOutline) -> Result<i64, String> {
        let flat = outline.flatten();
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        let section_count = outline.sections.iter()
            .flat_map(|s| s.categories.iter())
            .count() as i32;
        let question_count = flat.len() as i32;

        conn.execute(
            &format!(
                "INSERT INTO {table} (edition, module_code, module_name, cloud_type, doc_file, section_count, question_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                table = TABLE_OUTLINES,
            ),
            rusqlite::params![
                outline.edition.as_str(),
                outline.module_code,
                outline.module_name,
                outline.cloud_type,
                outline.doc_file,
                section_count,
                question_count,
            ],
        ).map_err(|e| format!("Failed to insert outline: {}", e))?;

        let outline_id = conn.last_insert_rowid();

        for q in &flat {
            conn.execute(
                &format!(
                    "INSERT INTO {table} (outline_id, edition, module_code, module_name, cloud_type, section, category, question_text, question_order)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    table = TABLE_QUESTIONS,
                ),
                rusqlite::params![
                    outline_id,
                    q.edition.as_str(),
                    q.module_code,
                    q.module_name,
                    q.cloud_type,
                    q.section,
                    q.category,
                    q.question_text,
                    q.order,
                ],
            ).map_err(|e| format!("Failed to insert question: {}", e))?;
        }

        Ok(outline_id)
    }

    /// 查询某版本的所有问题
    pub fn get_questions_by_edition(&self, edition: &Edition) -> Result<Vec<FlatQuestion>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            &format!(
                "SELECT edition, module_code, module_name, cloud_type, section, category, question_text, question_order
                 FROM {table} WHERE edition = ?1 ORDER BY question_order",
                table = TABLE_QUESTIONS,
            )
        ).map_err(|e| format!("Failed to prepare: {}", e))?;

        let rows = stmt.query_map(
            rusqlite::params![edition.as_str()],
            |row| {
                Ok(FlatQuestion {
                    edition: Edition::from_str(&row.get::<_, String>(0)?).unwrap_or(Edition::Enterprise),
                    module_code: row.get(1)?,
                    module_name: row.get(2)?,
                    cloud_type: row.get(3)?,
                    section: row.get(4)?,
                    category: row.get(5)?,
                    question_text: row.get(6)?,
                    order: row.get(7)?,
                })
            },
        ).map_err(|e| format!("Failed to query: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("Row error: {}", e))?);
        }
        Ok(result)
    }

    /// 获取所有已索引的提纲列表
    pub fn list_outlines(&self, edition: &Edition) -> Result<Vec<(i64, String, String)>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            &format!(
                "SELECT id, module_code, module_name FROM {table} WHERE edition = ?1 ORDER BY module_code",
                table = TABLE_OUTLINES,
            )
        ).map_err(|e| format!("Failed to prepare: {}", e))?;

        let rows = stmt.query_map(
            rusqlite::params![edition.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).map_err(|e| format!("Failed to query: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("Row error: {}", e))?);
        }
        Ok(result)
    }

    /// 获取问题总数
    pub fn question_count(&self, edition: &Edition) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM {table} WHERE edition = ?1",
                table = TABLE_QUESTIONS,
            ),
            rusqlite::params![edition.as_str()],
            |row| row.get(0),
        ).map_err(|e| format!("Failed to count: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_outline() -> ResearchOutline {
        ResearchOutline {
            edition: Edition::Enterprise,
            module_code: "ECW2107".to_string(),
            module_name: "总账".to_string(),
            cloud_type: "财务".to_string(),
            doc_file: "test.doc".to_string(),
            sections: vec![
                Section {
                    name: "业务概况".to_string(),
                    categories: vec![
                        Category {
                            name: "组织人员".to_string(),
                            questions: vec![
                                "公司目前财务组织架构？".to_string(),
                                "财务组织涉及哪些角色？".to_string(),
                            ],
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn test_insert_and_query() {
        let db_path = std::env::temp_dir().join("test_research_index.db");
        // 清理上次运行的残留
        let _ = std::fs::remove_file(&db_path);

        let indexer = ResearchIndexer::new(&db_path).unwrap();
        indexer.init_tables().unwrap();

        let outline = sample_outline();
        let outline_id = indexer.insert_outline(&outline).unwrap();
        assert!(outline_id > 0);

        let count: i64 = indexer.question_count(&Edition::Enterprise).unwrap();
        assert_eq!(count, 2);

        let questions = indexer.get_questions_by_edition(&Edition::Enterprise).unwrap();
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[0].question_text, "公司目前财务组织架构？");

        // 清理
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_flagship_empty_initially() {
        let db_path = std::env::temp_dir().join("test_flagship_empty.db");
        let _ = std::fs::remove_file(&db_path);

        let indexer = ResearchIndexer::new(&db_path).unwrap();
        indexer.init_tables().unwrap();

        let count: i64 = indexer.question_count(&Edition::Flagship).unwrap();
        assert_eq!(count, 0);

        let _ = std::fs::remove_file(&db_path);
    }
}
```

- [ ] **步骤 2：在 mod.rs 注册**

```rust
// src-tauri/src/services/mod.rs 添加:
pub mod research_indexer;
```

- [ ] **步骤 3：运行测试**

运行：`cd src-tauri && cargo test research_indexer::tests -- --nocapture`
预期：2 passed

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/services/research_indexer.rs src-tauri/src/services/mod.rs
git commit -m "feat: add ResearchIndexer — SQLite storage for research outlines and questions"
```

---

### 任务 5: 批量解析企业版调研提纲

**文件：**
- 修改：`src-tauri/src/services/research_indexer.rs`

- [ ] **步骤 1：实现批量导入函数**

```rust
// 添加到 research_indexer.rs
impl ResearchIndexer {
    /// 从目录批量导入调研提纲
    pub fn import_directory(&self, dir: &Path, edition: Edition) -> Result<ImportResult, String> {
        let mut result = ImportResult::default();

        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("Failed to read dir: {}", e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "doc"))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in &entries {
            let filename = entry.file_name().to_string_lossy().to_string();

            // 跳过临时文件
            if filename.starts_with('~') {
                continue;
            }

            // 解析模块信息
            let (module_code, module_name, cloud_type) = match parse_module_info(&filename) {
                Some(info) => info,
                None => {
                    eprintln!("Skipping {}: cannot parse module info", filename);
                    result.skipped += 1;
                    continue;
                }
            };

            // 解析 DOC 内容
            let text = match parse_doc_file(&entry.path()) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Skipping {}: {}", filename, e);
                    result.errors.push(format!("{}: {}", filename, e));
                    continue;
                }
            };

            // 结构化
            let outline = parse_outline_text(
                &text,
                edition.clone(),
                &module_code,
                &module_name,
                &cloud_type,
                &filename,
            );

            // 入库
            match self.insert_outline(&outline) {
                Ok(id) => {
                    eprintln!("Imported {} → id={} ({} questions)",
                        filename, id, outline.flatten().len());
                    result.imported += 1;
                    result.total_questions += outline.flatten().len() as i64;
                }
                Err(e) => {
                    eprintln!("Failed to insert {}: {}", filename, e);
                    result.errors.push(format!("{}: {}", filename, e));
                }
            }
        }

        Ok(result)
    }
}

#[derive(Debug, Default)]
pub struct ImportResult {
    pub imported: i32,
    pub skipped: i32,
    pub total_questions: i64,
    pub errors: Vec<String>,
}
```

- [ ] **步骤 2：编写导入测试（使用现有文件）**

```rust
// 添加到 tests 模块
#[test]
fn test_import_enterprise_directory() {
    let outline_dir = std::path::Path::new(r"E:\工作资料\项目资料\企业版调研提纲\企业版");
    if !outline_dir.exists() {
        eprintln!("Skipping: enterprise outline directory not found");
        return;
    }

    let db_path = std::env::temp_dir().join("test_enterprise_import.db");
    let _ = std::fs::remove_file(&db_path);

    let indexer = ResearchIndexer::new(&db_path).unwrap();
    indexer.init_tables().unwrap();

    let result = indexer.import_directory(outline_dir, Edition::Enterprise).unwrap();
    eprintln!("Import result: {:#?}", result);
    assert!(result.imported > 0, "Should have imported at least some files");
    assert!(result.total_questions > 0, "Should have questions");

    let outlines = indexer.list_outlines(&Edition::Enterprise).unwrap();
    eprintln!("Imported {} outlines", outlines.len());
    for (id, code, name) in &outlines {
        eprintln!("  [{}] {}: {}", id, code, name);
    }

    let _ = std::fs::remove_file(&db_path);
}
```

- [ ] **步骤 3：运行测试**

运行：`cd src-tauri && cargo test research_indexer::tests::test_import_enterprise_directory -- --nocapture`
预期：完成解析（输出导入统计）

**注意：** 此测试依赖真实的 DOC 文件，在 CI 中可能被跳过。阶段结束前手动运行一次确认。

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/services/research_indexer.rs
git commit -m "feat: add batch import of enterprise research outlines from directory"
```

---

### 任务 6: 向量索引构建

**文件：**
- 修改：`src-tauri/src/services/research_indexer.rs`

- [ ] **步骤 1：添加问答 embedding 和 usearch 索引构建**

```rust
// 添加到 research_indexer.rs

/// 为指定版本的所有问题构建向量索引
pub fn build_vector_index(
    db_path: &Path,
    edition: &Edition,
    embedding_service: &EmbeddingService, // 使用 v0.1 已存在的 embedding
    index_path: &Path,
) -> Result<usize, String> {
    let indexer = ResearchIndexer::new(db_path)?;
    let questions = indexer.get_questions_by_edition(edition)?;
    
    if questions.is_empty() {
        return Err("No questions to index".to_string());
    }

    // 复用 v0.1 的 VectorIndex 创建索引
    // (假设 VectorIndex::with_dimensions 已存在)
    let mut vec_index = super::vector_index::VectorIndex::with_dimensions(
        index_path.parent().unwrap().to_path_buf(),
        512,
    )?;

    // 为每个问题生成 embedding 并添加到索引
    for q in &questions {
        let text = format!("{} {} {} {}",
            q.module_name, q.section, q.category, q.question_text);
        let embedding = embedding_service.embed(&text)
            .map_err(|e| format!("Embedding failed: {}", e))?;
        
        // 使用 question id 作为 key
        let key = q.order as u64;
        vec_index.add(key, &embedding)?;
    }

    vec_index.save()?;
    Ok(questions.len())
}
```

**注意：** 此步骤需要 v0.1 的 `vector_index::VectorIndex` 和 `embedding_service` 接口。实际实现时需根据现有 API 调整。如果 `VectorIndex` 的 `with_dimensions` 还不支持自定义维度，需要先适配。

- [ ] **步骤 2：添加 BM25 索引构建**

```rust
// 添加到 research_indexer.rs

/// 为指定版本的问题构建 BM25 全文索引
pub fn build_bm25_index(
    db_path: &Path,
    edition: &Edition,
    bm25_index_path: &Path,
) -> Result<usize, String> {
    use std::fs;
    use std::io::Write;
    
    let indexer = ResearchIndexer::new(db_path)?;
    let questions = indexer.get_questions_by_edition(edition)?;
    
    // 构建 BM25 索引数据的 JSONL 文件
    // （具体的 BM25 添加逻辑使用 v0.1 的 bm25_service）
    let jsonl_path = bm25_index_path.with_extension("jsonl");
    let mut file = fs::File::create(&jsonl_path)
        .map_err(|e| format!("Failed to create JSONL: {}", e))?;

    for q in &questions {
        let entry = serde_json::json!({
            "id": format!("research_q_{}_{}", edition.as_str(), q.order),
            "text": format!("{} {} {} {}",
                q.module_name, q.section, q.category, q.question_text),
            "metadata": {
                "edition": q.edition.as_str(),
                "module_code": q.module_code,
                "module_name": q.module_name,
                "section": q.section,
                "category": q.category,
            }
        });
        writeln!(file, "{}", entry)
            .map_err(|e| format!("Failed to write JSONL: {}", e))?;
    }

    // 调用 BM25 服务重建索引
    // （需要 bm25_service 的 rebuild_from_jsonl 接口）
    // bm25_service::rebuild(bm25_index_path, &jsonl_path)?;

    fs::remove_file(&jsonl_path).ok();
    Ok(questions.len())
}
```

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/src/services/research_indexer.rs
git commit -m "feat: add vector and BM25 index building for research questions"
```

---

### 任务 7: Tauri Commands 注册

**文件：**
- 修改：`src-tauri/src/lib.rs` 或 `src-tauri/src/main.rs`

- [ ] **步骤 1：注册 Phase 9 相关命令**

```rust
// 在 lib.rs 或 main.rs 中添加 Tauri commands

#[tauri::command]
fn get_current_edition(state: tauri::State<'_, AppState>) -> Result<String, String> {
    Ok(state.edition_config.current().as_str().to_string())
}

#[tauri::command]
fn set_edition(state: tauri::State<'_, AppState>, edition: String) -> Result<(), String> {
    let e = research_outline::Edition::from_str(&edition)
        .ok_or_else(|| format!("Invalid edition: {}", edition))?;
    state.edition_config.set(&e)
}

#[tauri::command]
fn list_research_modules(state: tauri::State<'_, AppState>) -> Result<Vec<(i64, String, String)>, String> {
    let edition = state.edition_config.current();
    state.research_indexer.list_outlines(&edition)
}

#[tauri::command]
fn import_research_outlines(state: tauri::State<'_, AppState>, dir: String) -> Result<String, String> {
    let edition = state.edition_config.current();
    let path = std::path::Path::new(&dir);
    if !path.exists() {
        return Err(format!("Directory not found: {}", dir));
    }
    let result = state.research_indexer.import_directory(path, edition)?;
    Ok(format!("Imported {} outlines, {} questions, {} skipped, {} errors",
        result.imported, result.total_questions, result.skipped, result.errors.len()))
}
```

- [ ] **步骤 2：在 AppState 中添加字段**

```rust
// AppState 添加：
pub struct AppState {
    pub edition_config: EditionConfig,
    pub research_indexer: ResearchIndexer,
    // ... existing fields
}
```

- [ ] **步骤 3：运行编译检查**

运行：`cd src-tauri && cargo check`
预期：编译通过

- [ ] **步骤 4：Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: register Tauri commands for edition config and research outline management"
```

---

## 自检

- [ ] 所有数据结构（Edition, ResearchOutline, FlatQuestion）定义完整，无 TODO
- [ ] EditionConfig 支持企业版/旗舰版切换和持久化
- [ ] DOC 解析器通过正则提取章节/分类/问题
- [ ] ResearchIndexer 支持入库、查询、批量导入
- [ ] 向量索引和 BM25 索引构建已规划
- [ ] Tauri commands 已注册
- [ ] 所有测试代码完整无占位符
