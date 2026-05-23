use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

use crate::services::research_outline::{
    parse_doc_file, parse_module_info, parse_outline_text, Edition, FlatQuestion, ResearchOutline,
};

pub struct ResearchIndexer {
    conn: Mutex<Connection>,
}

impl ResearchIndexer {
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn init_tables(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS research_outlines (
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

            CREATE TABLE IF NOT EXISTS research_questions (
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
                FOREIGN KEY (outline_id) REFERENCES research_outlines(id)
            );

            CREATE INDEX IF NOT EXISTS idx_questions_edition ON research_questions(edition);
            CREATE INDEX IF NOT EXISTS idx_questions_module ON research_questions(module_code);",
        )
        .map_err(|e| format!("Failed to init tables: {}", e))
    }

    pub fn insert_outline(&self, outline: &ResearchOutline) -> Result<i64, String> {
        let flat = outline.flatten();
        let section_count: i64 = outline.sections.iter().map(|s| s.categories.len() as i64).sum();
        let question_count = flat.len() as i64;

        let mut conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start transaction: {}", e))?;

        tx.execute(
            "INSERT INTO research_outlines (edition, module_code, module_name, cloud_type, doc_file, section_count, question_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                outline.edition.as_str(),
                outline.module_code,
                outline.module_name,
                outline.cloud_type,
                outline.doc_file,
                section_count,
                question_count,
            ],
        )
        .map_err(|e| format!("Failed to insert outline: {}", e))?;

        let outline_id = tx.last_insert_rowid();

        for q in &flat {
            tx.execute(
                "INSERT INTO research_questions (outline_id, edition, module_code, module_name, cloud_type, section, category, question_text, question_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
            )
            .map_err(|e| format!("Failed to insert question: {}", e))?;
        }

        tx.commit().map_err(|e| format!("Failed to commit transaction: {}", e))?;
        Ok(outline_id)
    }

    pub fn get_questions_by_edition(&self, edition: &Edition) -> Result<Vec<FlatQuestion>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare(
                "SELECT edition, module_code, module_name, cloud_type, section, category, question_text, question_order
                 FROM research_questions
                 WHERE edition = ?1
                 ORDER BY question_order",
            )
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let rows = stmt
            .query_map(rusqlite::params![edition.as_str()], |row| {
                let edition_str: String = row.get(0)?;
                let edition = Edition::from_str(&edition_str)
                    .ok_or_else(|| {
                        rusqlite::Error::InvalidParameterName(
                            format!("invalid edition value in database: {}", edition_str)
                        )
                    })?;
                Ok(FlatQuestion {
                    edition,
                    module_code: row.get(1)?,
                    module_name: row.get(2)?,
                    cloud_type: row.get(3)?,
                    section: row.get(4)?,
                    category: row.get(5)?,
                    question_text: row.get(6)?,
                    order: row.get(7)?,
                })
            })
            .map_err(|e| format!("Failed to query questions: {}", e))?;

        let mut questions = Vec::new();
        for row in rows {
            questions.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(questions)
    }

    pub fn list_outlines(&self, edition: &Edition) -> Result<Vec<(i64, String, String)>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, module_code, module_name
                 FROM research_outlines
                 WHERE edition = ?1
                 ORDER BY id",
            )
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let rows = stmt
            .query_map(rusqlite::params![edition.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| format!("Failed to query outlines: {}", e))?;

        let mut outlines = Vec::new();
        for row in rows {
            outlines.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(outlines)
    }

    pub fn question_count(&self, edition: &Edition) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.query_row(
            "SELECT COUNT(*) FROM research_questions WHERE edition = ?1",
            rusqlite::params![edition.as_str()],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count questions: {}", e))
    }
}

pub struct ImportResult {
    pub imported: i32,
    pub skipped: i32,
    pub total_questions: i64,
    pub errors: Vec<String>,
}

impl ResearchIndexer {
    pub fn import_directory(&self, dir: &Path, edition: Edition) -> Result<ImportResult, String> {
        let mut result = ImportResult {
            imported: 0,
            skipped: 0,
            total_questions: 0,
            errors: Vec::new(),
        };

        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("Failed to read directory {:?}: {}", dir, e))?
            .filter_map(|e| e.ok())
            .collect();

        entries.sort_by_key(|e| e.file_name());

        for entry in &entries {
            let path = entry.path();

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "doc" && ext != "docx" {
                continue;
            }

            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if filename.starts_with('~') {
                continue;
            }

            let (module_code, module_name, cloud_type) = match parse_module_info(filename) {
                Some(info) => info,
                None => {
                    result.skipped += 1;
                    continue;
                }
            };

            let text = match parse_doc_file(&path) {
                Ok(t) => t,
                Err(e) => {
                    result.errors.push(format!("Failed to parse {}: {}", filename, e));
                    continue;
                }
            };

            let outline = parse_outline_text(
                &text,
                edition.clone(),
                &module_code,
                &module_name,
                &cloud_type,
                filename,
            );

            match self.insert_outline(&outline) {
                Ok(_) => {
                    result.imported += 1;
                    result.total_questions += outline.flatten().len() as i64;
                }
                Err(e) => {
                    result.errors.push(format!("Failed to insert {}: {}", filename, e));
                }
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::research_outline::{Category, Section};

    fn sample_outline() -> ResearchOutline {
        ResearchOutline {
            edition: Edition::Enterprise,
            module_code: "BOS".to_string(),
            module_name: "基础平台".to_string(),
            cloud_type: "公有云".to_string(),
            doc_file: "BOS_research.md".to_string(),
            sections: vec![
                Section {
                    name: "架构".to_string(),
                    categories: vec![
                        Category {
                            name: "部署架构".to_string(),
                            questions: vec![
                                "支持的部署模式有哪些？".to_string(),
                                "高可用方案如何？".to_string(),
                            ],
                        },
                        Category {
                            name: "微服务".to_string(),
                            questions: vec![
                                "服务注册发现机制？".to_string(),
                            ],
                        },
                    ],
                },
                Section {
                    name: "安全".to_string(),
                    categories: vec![
                        Category {
                            name: "认证".to_string(),
                            questions: vec![
                                "支持的认证方式？".to_string(),
                            ],
                        },
                    ],
                },
            ],
        }
    }

    fn new_indexer() -> ResearchIndexer {
        let conn = Connection::open_in_memory().unwrap();
        let indexer = ResearchIndexer {
            conn: Mutex::new(conn),
        };
        indexer.init_tables().unwrap();
        indexer
    }

    #[test]
    fn test_insert_and_query() {
        let indexer = new_indexer();
        let outline = sample_outline();
        let outline_id = indexer.insert_outline(&outline).unwrap();
        assert!(outline_id > 0);

        let count = indexer.question_count(&Edition::Enterprise).unwrap();
        assert_eq!(count, 4);

        let questions = indexer.get_questions_by_edition(&Edition::Enterprise).unwrap();
        assert_eq!(questions.len(), 4);
        assert_eq!(questions[0].question_text, "支持的部署模式有哪些？");
        assert_eq!(questions[1].question_text, "高可用方案如何？");
        assert_eq!(questions[2].question_text, "服务注册发现机制？");
        assert_eq!(questions[3].question_text, "支持的认证方式？");
    }

    #[test]
    fn test_flagship_empty_initially() {
        let indexer = new_indexer();
        let count = indexer.question_count(&Edition::Flagship).unwrap();
        assert_eq!(count, 0);

        let questions = indexer.get_questions_by_edition(&Edition::Flagship).unwrap();
        assert_eq!(questions.len(), 0);

        let outlines = indexer.list_outlines(&Edition::Flagship).unwrap();
        assert_eq!(outlines.len(), 0);
    }

    #[test]
    fn test_import_enterprise_directory() {
        let dir = Path::new(r"E:\工作资料\项目资料\企业版调研提纲\企业版");
        if !dir.exists() {
            eprintln!("Skipping test_import_enterprise_directory: directory not found at {:?}", dir);
            return;
        }
        let indexer = new_indexer();
        let result = indexer.import_directory(dir, Edition::Enterprise).unwrap();
        eprintln!(
            "Import result: imported={}, skipped={}, total_questions={}, errors={:?}",
            result.imported, result.skipped, result.total_questions, result.errors
        );
        assert!(result.imported > 0, "Expected at least 1 imported file, got {}", result.imported);
        assert!(result.total_questions > 0, "Expected total_questions > 0, got {}", result.total_questions);
        let total_in_db = indexer.question_count(&Edition::Enterprise).unwrap();
        assert_eq!(total_in_db, result.total_questions);
    }
}
