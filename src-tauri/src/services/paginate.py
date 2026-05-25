import sys

def paginate_metadata(path):
    with open(path, 'r', encoding='utf-8') as f:
        content = f.read()

    old = '''    /// Get all documents, optionally filtered by project
    pub fn get_documents(&self, project: Option<&str>) -> Result<Vec<DocumentMeta>, String> {
        if let Some(proj) = project {
            self.query_documents(
                "SELECT id, title, source_path, sha256, created_at, project
                 FROM documents WHERE project = ?1 ORDER BY created_at DESC",
                params![proj],
            )
        } else {
            self.query_documents(
                "SELECT id, title, source_path, sha256, created_at, project
                 FROM documents ORDER BY created_at DESC",
                [],
            )
        }
    }'''

    new = '''    /// Get all documents, optionally filtered by project
    pub fn get_documents(&self, project: Option<&str>, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<DocumentMeta>, String> {
        if let Some(proj) = project {
            self.query_documents(
                "SELECT id, title, source_path, sha256, created_at, project
                 FROM documents WHERE project = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                params![proj, limit.unwrap_or(-1), offset.unwrap_or(0)],
            )
        } else {
            self.query_documents(
                "SELECT id, title, source_path, sha256, created_at, project
                 FROM documents ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                params![limit.unwrap_or(-1), offset.unwrap_or(0)],
            )
        }
    }'''

    if old in content:
        content = content.replace(old, new)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(content)
        print('SUCCESS: metadata.rs get_documents paginated')
    else:
        print('WARNING: Old pattern not found in metadata.rs')

def paginate_product_store(path):
    with open(path, 'r', encoding='utf-8') as f:
        content = f.read()

    old = '''    pub fn list(&self, project: Option<&str>) -> Result<Vec<ProductMeta>, String> {
        if let Some(proj) = project {
            self.query_products(
                "SELECT id, template_id, template_name, project, status, output_path, field_count, llm_fields_count, created_at
                 FROM products WHERE project = ?1 ORDER BY created_at DESC",
                params![proj],
            )
        } else {
            self.query_products(
                "SELECT id, template_id, template_name, project, status, output_path, field_count, llm_fields_count, created_at
                 FROM products ORDER BY created_at DESC",
                [],
            )
        }
    }'''

    new = '''    pub fn list(&self, project: Option<&str>, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<ProductMeta>, String> {
        if let Some(proj) = project {
            self.query_products(
                "SELECT id, template_id, template_name, project, status, output_path, field_count, llm_fields_count, created_at
                 FROM products WHERE project = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                params![proj, limit.unwrap_or(-1), offset.unwrap_or(0)],
            )
        } else {
            self.query_products(
                "SELECT id, template_id, template_name, project, status, output_path, field_count, llm_fields_count, created_at
                 FROM products ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                params![limit.unwrap_or(-1), offset.unwrap_or(0)],
            )
        }
    }'''

    if old in content:
        content = content.replace(old, new)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(content)
        print('SUCCESS: product_store.rs list paginated')
    else:
        print('WARNING: Old pattern not found in product_store.rs')


def paginate_research_session(path):
    with open(path, 'r', encoding='utf-8') as f:
        content = f.read()

    old = '''    pub fn list_sessions(&self) -> Result<Vec<ResearchSession>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, edition, module_code, interviewee, session_date, status, created_at, updated_at
                 FROM research_sessions ORDER BY updated_at DESC",
            )
            .map_err(|e| format!("Failed to prepare list: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ResearchSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    edition: row.get(2)?,
                    module_code: row.get(3)?,
                    interviewee: row.get(4)?,
                    session_date: row.get(5)?,
                    status: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })
            .map_err(|e| format!("Failed to query sessions: {}", e))?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row.map_err(|e| format!("Failed to read session: {}", e))?);
        }
        Ok(sessions)
    }'''

    new = '''    pub fn list_sessions(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<ResearchSession>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, edition, module_code, interviewee, session_date, status, created_at, updated_at
                 FROM research_sessions ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2",
            )
            .map_err(|e| format!("Failed to prepare list: {}", e))?;

        let rows = stmt
            .query_map(params![limit.unwrap_or(-1), offset.unwrap_or(0)], |row| {
                Ok(ResearchSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    edition: row.get(2)?,
                    module_code: row.get(3)?,
                    interviewee: row.get(4)?,
                    session_date: row.get(5)?,
                    status: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })
            .map_err(|e| format!("Failed to query sessions: {}", e))?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row.map_err(|e| format!("Failed to read session: {}", e))?);
        }
        Ok(sessions)
    }'''

    if old in content:
        content = content.replace(old, new)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(content)
        print('SUCCESS: research_session.rs list_sessions paginated')
    else:
        print('WARNING: Old pattern not found for list_sessions')


def paginate_risk_control(path):
    with open(path, 'r', encoding='utf-8') as f:
        content = f.read()

    old = '''    pub fn list_scope_items(&self) -> Result<Vec<ContractScopeItem>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT id, category, description, is_in_scope, detail, created_at FROM contract_scope_items ORDER BY category, id")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt.query_map([], |row| {
            Ok(ContractScopeItem {
                id: row.get(0)?,
                category: row.get(1)?,
                description: row.get(2)?,
                is_in_scope: row.get::<_, i32>(3)? != 0,
                detail: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query: {}", e))?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(items)
    }'''

    new = '''    pub fn list_scope_items(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<ContractScopeItem>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT id, category, description, is_in_scope, detail, created_at FROM contract_scope_items ORDER BY category, id LIMIT ?1 OFFSET ?2")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt.query_map(params![limit.unwrap_or(-1), offset.unwrap_or(0)], |row| {
            Ok(ContractScopeItem {
                id: row.get(0)?,
                category: row.get(1)?,
                description: row.get(2)?,
                is_in_scope: row.get::<_, i32>(3)? != 0,
                detail: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query: {}", e))?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(items)
    }'''

    if old in content:
        content = content.replace(old, new)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(content)
        print('SUCCESS: risk_control.rs list_scope_items paginated')
    else:
        print('WARNING: Old pattern not found for list_scope_items')


if __name__ == '__main__':
    base = r'E:\projects\kingdee\KingdeeKB\src-tauri\src\services'
    paginate_metadata(f'{base}\\metadata.rs')
    paginate_product_store(f'{base}\\product_store.rs')
    paginate_research_session(f'{base}\\research_session.rs')
    paginate_risk_control(f'{base}\\risk_control.rs')
