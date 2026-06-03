//! SQLite product store for generated document management
//!
//! Manages products and product_versions tables with CRUD operations,
//! export capabilities, and project/time filtering.

use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Product metadata (list view)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductMeta {
    pub id: i64,
    pub template_id: String,
    pub template_name: String,
    pub project_id: i64,
    pub status: String,
    pub output_path: String,
    pub field_count: i64,
    pub llm_fields_count: i64,
    pub created_at: String,
}

/// Product version (for regeneration history)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductVersion {
    pub id: i64,
    pub product_id: i64,
    pub version: i64,
    pub input_data: String, // JSON string
    pub output_path: String,
    pub created_at: String,
}

/// SQLite-based product store
pub struct ProductStore {
    db: Connection,
    /// 迁移期间预留
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl ProductStore {
    /// Open or create the product database at the given path
    pub fn new(db_path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create DB directory: {}", e))?;
        }

        let db =
            Connection::open(&db_path).map_err(|e| format!("Failed to open database: {}", e))?;

        // 设置数据库忙超时（5秒），以防并发写入时立即返回 SQLITE_BUSY 错误
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("设置数据库忙超时失败: {}", e))?;

        // Enable WAL mode for better concurrent read performance
        db.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| format!("Failed to set WAL mode: {}", e))?;

        // Enable foreign keys
        db.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("Failed to enable foreign keys: {}", e))?;

        let store = Self { db, db_path };
        store.init_schema()?;
        Ok(store)
    }

    /// Create tables and indexes (idempotent)
    fn init_schema(&self) -> Result<(), String> {
        self.db
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS products (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                template_id TEXT NOT NULL,
                template_name TEXT NOT NULL,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                status TEXT NOT NULL DEFAULT 'completed',
                output_path TEXT NOT NULL,
                field_count INTEGER NOT NULL DEFAULT 0,
                llm_fields_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS product_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                product_id INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
                version INTEGER NOT NULL DEFAULT 1,
                input_data TEXT NOT NULL DEFAULT '{}',
                output_path TEXT NOT NULL,
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_products_project_id ON products(project_id);
            CREATE INDEX IF NOT EXISTS idx_products_created_at ON products(created_at);
            CREATE INDEX IF NOT EXISTS idx_products_template_id ON products(template_id);
            CREATE INDEX IF NOT EXISTS idx_product_versions_product_id ON product_versions(product_id);
            ",
            )
            .map_err(|e| format!("Failed to initialize schema: {}", e))
    }

    // ─── Product operations ───

    /// Create a new product. Returns the product ID.
    pub fn create(
        &self,
        template_id: &str,
        template_name: &str,
        project_id: i64,
        output_path: &str,
        field_count: i64,
        llm_fields_count: i64,
        input_data: &str,
    ) -> Result<i64, String> {
        self.db
            .execute(
                "INSERT INTO products (template_id, template_name, project_id, status, output_path, field_count, llm_fields_count)
                 VALUES (?1, ?2, ?3, 'completed', ?4, ?5, ?6)",
                params![template_id, template_name, project_id, output_path, field_count, llm_fields_count],
            )
            .map_err(|e| format!("Failed to insert product: {}", e))?;

        let product_id = self.db.last_insert_rowid();

        // Create initial version
        self.db
            .execute(
                "INSERT INTO product_versions (product_id, version, input_data, output_path)
                 VALUES (?1, 1, ?2, ?3)",
                params![product_id, input_data, output_path],
            )
            .map_err(|e| format!("Failed to insert product version: {}", e))?;

        Ok(product_id)
    }

    /// Get a product by its ID
    pub fn get(&self, id: i64) -> Result<Option<ProductMeta>, String> {
        self.query_one_product(
            "SELECT id, template_id, template_name, project_id, status, output_path, field_count, llm_fields_count, created_at
             FROM products WHERE id = ?1",
            params![id],
        )
    }

    /// List products, optionally filtered by project. Ordered by created_at DESC.
    pub fn list(
        &self,
        project_id: Option<i64>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<ProductMeta>, String> {
        if let Some(pid) = project_id {
            self.query_products(
                "SELECT id, template_id, template_name, project_id, status, output_path, field_count, llm_fields_count, created_at
                 FROM products WHERE project_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                params![pid, limit.unwrap_or(-1), offset.unwrap_or(0)],
            )
        } else {
            self.query_products(
                "SELECT id, template_id, template_name, project_id, status, output_path, field_count, llm_fields_count, created_at
                 FROM products ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                params![limit.unwrap_or(-1), offset.unwrap_or(0)],
            )
        }
    }

    /// Delete a product and its versions
    /// If project is specified, verify the product belongs to that project before deleting
    pub fn delete(&self, id: i64, project_id: Option<i64>) -> Result<(), String> {
        // Verify project ownership if project is specified
        if let Some(pid) = project_id {
            let product = self
                .get(id)?
                .ok_or_else(|| format!("Product not found: {}", id))?;
            if product.project_id != pid {
                return Err(format!(
                    "Product {} belongs to project '{}', not '{}'",
                    id, product.project_id, pid
                ));
            }
        }

        // Delete versions first (foreign key cascade should handle this, but be explicit)
        self.db
            .execute(
                "DELETE FROM product_versions WHERE product_id = ?1",
                params![id],
            )
            .map_err(|e| format!("Failed to delete product versions: {}", e))?;

        let rows = self
            .db
            .execute("DELETE FROM products WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete product: {}", e))?;

        if rows == 0 {
            return Err(format!("Product not found: {}", id));
        }

        Ok(())
    }

    /// Get all versions for a product, ordered by version DESC
    pub fn get_versions(&self, product_id: i64) -> Result<Vec<ProductVersion>, String> {
        self.query_versions(
            "SELECT id, product_id, version, input_data, output_path, created_at
             FROM product_versions WHERE product_id = ?1 ORDER BY version DESC",
            params![product_id],
        )
    }

    /// Get the latest version of a product
    pub fn get_latest_version(&self, product_id: i64) -> Result<Option<ProductVersion>, String> {
        self.query_one_version(
            "SELECT id, product_id, version, input_data, output_path, created_at
             FROM product_versions WHERE product_id = ?1 ORDER BY version DESC LIMIT 1",
            params![product_id],
        )
    }

    /// Add a new version to an existing product (for regeneration).
    /// Returns the new version number.
    pub fn add_version(
        &self,
        product_id: i64,
        input_data: &str,
        output_path: &str,
    ) -> Result<i64, String> {
        // Get current max version
        let max_version: i64 = self
            .db
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM product_versions WHERE product_id = ?1",
                params![product_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to get max version: {}", e))?;

        let new_version = max_version + 1;

        self.db
            .execute(
                "INSERT INTO product_versions (product_id, version, input_data, output_path)
                 VALUES (?1, ?2, ?3, ?4)",
                params![product_id, new_version, input_data, output_path],
            )
            .map_err(|e| format!("Failed to insert product version: {}", e))?;

        // Update product's output_path to latest version
        self.db
            .execute(
                "UPDATE products SET output_path = ?1 WHERE id = ?2",
                params![output_path, product_id],
            )
            .map_err(|e| format!("Failed to update product output_path: {}", e))?;

        Ok(new_version)
    }

    // ─── Export operations ───

    /// Export a product's output file to a target directory.
    /// If project is specified, verify the product belongs to that project before exporting.
    /// Returns the path of the exported file.
    pub fn export_product(
        &self,
        product_id: i64,
        target_dir: &str,
        project_id: Option<i64>,
    ) -> Result<String, String> {
        let product = self
            .get(product_id)?
            .ok_or_else(|| format!("Product not found: {}", product_id))?;

        // Verify project ownership if project is specified
        if let Some(pid) = project_id {
            if product.project_id != pid {
                return Err(format!(
                    "Product {} belongs to project '{}', not '{}'",
                    product_id, product.project_id, pid
                ));
            }
        }

        let source = PathBuf::from(&product.output_path);
        if !source.exists() {
            return Err(format!("Output file not found: {}", product.output_path));
        }

        let target = PathBuf::from(target_dir);
        std::fs::create_dir_all(&target)
            .map_err(|e| format!("Failed to create target directory: {}", e))?;

        let file_name = source
            .file_name()
            .ok_or_else(|| "Invalid source file name".to_string())?;
        let dest = target.join(file_name);

        std::fs::copy(&source, &dest).map_err(|e| format!("Failed to copy file: {}", e))?;

        Ok(dest.to_string_lossy().to_string())
    }

    /// Export all products for a project to a target directory.
    /// Returns the paths of all exported files.
    pub fn export_all(&self, project_id: i64, target_dir: &str) -> Result<Vec<String>, String> {
        let products = self.list(Some(project_id), None, None)?;
        let mut exported = Vec::new();

        for product in products {
            match self.export_product(product.id, target_dir, Some(project_id)) {
                Ok(path) => exported.push(path),
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to export product {} ({}): {}",
                        product.id, product.template_name, e
                    );
                }
            }
        }

        Ok(exported)
    }

    // ─── Private helpers ───

    fn row_to_product(row: &rusqlite::Row) -> SqlResult<ProductMeta> {
        Ok(ProductMeta {
            id: row.get(0)?,
            template_id: row.get(1)?,
            template_name: row.get(2)?,
            project_id: row.get(3)?,
            status: row.get(4)?,
            output_path: row.get(5)?,
            field_count: row.get(6)?,
            llm_fields_count: row.get(7)?,
            created_at: row.get(8)?,
        })
    }

    fn row_to_version(row: &rusqlite::Row) -> SqlResult<ProductVersion> {
        Ok(ProductVersion {
            id: row.get(0)?,
            product_id: row.get(1)?,
            version: row.get(2)?,
            input_data: row.get(3)?,
            output_path: row.get(4)?,
            created_at: row.get(5)?,
        })
    }

    fn query_one_product(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Option<ProductMeta>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let mut rows = stmt
            .query_map(params, Self::row_to_product)
            .map_err(|e| format!("Failed to query products: {}", e))?;

        match rows.next() {
            Some(Ok(product)) => Ok(Some(product)),
            Some(Err(e)) => Err(format!("Failed to read product row: {}", e)),
            None => Ok(None),
        }
    }

    fn query_products(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<ProductMeta>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map(params, Self::row_to_product)
            .map_err(|e| format!("Failed to query products: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read product row: {}", e))?);
        }
        Ok(results)
    }

    fn query_one_version(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Option<ProductVersion>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let mut rows = stmt
            .query_map(params, Self::row_to_version)
            .map_err(|e| format!("Failed to query versions: {}", e))?;

        match rows.next() {
            Some(Ok(version)) => Ok(Some(version)),
            Some(Err(e)) => Err(format!("Failed to read version row: {}", e)),
            None => Ok(None),
        }
    }

    fn query_versions(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<ProductVersion>, String> {
        let mut stmt = self
            .db
            .prepare(sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map(params, Self::row_to_version)
            .map_err(|e| format!("Failed to query versions: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read version row: {}", e))?);
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::project_store::ProjectStore;
    use std::io::Write;

    fn create_test_store() -> (tempfile::TempDir, ProductStore, i64, i64, i64) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("metadata.db");
        let project_store = ProjectStore::new(&db_path).unwrap();
        let project_a_id = project_store.create_project("project_a", "", "").unwrap();
        let project_b_id = project_store.create_project("project_b", "", "").unwrap();
        let project_batch_id = project_store.create_project("proj_batch", "", "").unwrap();
        let store = ProductStore::new(db_path).unwrap();
        (tmp, store, project_a_id, project_b_id, project_batch_id)
    }

    fn create_dummy_output(dir: &std::path::Path, name: &str) -> String {
        let file_path = dir.join(name);
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "dummy content").unwrap();
        file_path.to_string_lossy().to_string()
    }

    #[test]
    fn test_create_and_get() {
        let (tmp, store, project_a_id, _, _) = create_test_store();
        let output = create_dummy_output(tmp.path(), "test_output.docx");

        let id = store
            .create(
                "tmpl_001",
                "调研报告模板",
                project_a_id,
                &output,
                10,
                3,
                r#"{"项目名称": "测试项目"}"#,
            )
            .unwrap();
        assert!(id > 0);

        let product = store.get(id).unwrap().unwrap();
        assert_eq!(product.template_id, "tmpl_001");
        assert_eq!(product.template_name, "调研报告模板");
        assert_eq!(product.project_id, project_a_id);
        assert_eq!(product.status, "completed");
        assert_eq!(product.field_count, 10);
        assert_eq!(product.llm_fields_count, 3);
    }

    #[test]
    fn test_list_with_project_filter() {
        let (tmp, store, project_a_id, project_b_id, _) = create_test_store();
        let out1 = create_dummy_output(tmp.path(), "out1.docx");
        let out2 = create_dummy_output(tmp.path(), "out2.docx");

        store
            .create("t1", "模板A", project_a_id, &out1, 5, 1, "{}")
            .unwrap();
        store
            .create("t2", "模板B", project_b_id, &out2, 8, 2, "{}")
            .unwrap();

        let all = store.list(None, None, None).unwrap();
        assert_eq!(all.len(), 2);

        let proj_a = store.list(Some(project_a_id), None, None).unwrap();
        assert_eq!(proj_a.len(), 1);
        assert_eq!(proj_a[0].template_name, "模板A");

        let proj_b = store.list(Some(project_b_id), None, None).unwrap();
        assert_eq!(proj_b.len(), 1);
        assert_eq!(proj_b[0].template_name, "模板B");
    }

    #[test]
    fn test_delete() {
        let (tmp, store, project_a_id, _, _) = create_test_store();
        let output = create_dummy_output(tmp.path(), "del_test.docx");

        let id = store
            .create("t1", "待删除", project_a_id, &output, 3, 0, "{}")
            .unwrap();
        assert!(store.get(id).unwrap().is_some());

        store.delete(id, None).unwrap();
        assert!(store.get(id).unwrap().is_none());
    }

    #[test]
    fn test_versions() {
        let (tmp, store, project_a_id, _, _) = create_test_store();
        let out1 = create_dummy_output(tmp.path(), "v1.docx");
        let out2 = create_dummy_output(tmp.path(), "v2.docx");

        let id = store
            .create("t1", "版本测试", project_a_id, &out1, 5, 1, r#"{"v": "1"}"#)
            .unwrap();

        // Add a second version
        let v2 = store.add_version(id, r#"{"v": "2"}"#, &out2).unwrap();
        assert_eq!(v2, 2);

        let versions = store.get_versions(id).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, 2); // DESC order
        assert_eq!(versions[1].version, 1);

        let latest = store.get_latest_version(id).unwrap().unwrap();
        assert_eq!(latest.version, 2);
        assert_eq!(latest.output_path, out2);
    }

    #[test]
    fn test_export_product() {
        let (tmp, store, project_a_id, _, _) = create_test_store();
        let output = create_dummy_output(tmp.path(), "export_me.docx");

        let id = store
            .create("t1", "导出测试", project_a_id, &output, 5, 0, "{}")
            .unwrap();

        let export_dir = tmp.path().join("exported");
        let exported_path = store
            .export_product(id, export_dir.to_str().unwrap(), None)
            .unwrap();

        assert!(std::path::Path::new(&exported_path).exists());
        assert!(exported_path.contains("export_me.docx"));
    }

    #[test]
    fn test_export_all() {
        let (tmp, store, _, _, project_batch_id) = create_test_store();
        let out1 = create_dummy_output(tmp.path(), "batch1.docx");
        let out2 = create_dummy_output(tmp.path(), "batch2.docx");

        store
            .create("t1", "批量A", project_batch_id, &out1, 3, 0, "{}")
            .unwrap();
        store
            .create("t2", "批量B", project_batch_id, &out2, 4, 1, "{}")
            .unwrap();

        let export_dir = tmp.path().join("batch_export");
        let exported = store
            .export_all(project_batch_id, export_dir.to_str().unwrap())
            .unwrap();

        assert_eq!(exported.len(), 2);
        for path in &exported {
            assert!(std::path::Path::new(path).exists());
        }
    }
}
