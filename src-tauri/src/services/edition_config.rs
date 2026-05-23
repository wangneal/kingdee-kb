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

    pub fn init_table(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .map_err(|e| format!("Failed to init config table: {}", e))
    }

    pub fn current(&self) -> Edition {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[EditionConfig] Mutex poisoned: {}", e);
                return Edition::Enterprise;
            }
        };
        let result: Result<String, _> = conn.query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            rusqlite::params![CONFIG_KEY_CURRENT_EDITION],
            |row| row.get(0),
        );
        match result {
            Ok(s) => Edition::from_str(&s).unwrap_or_else(|| {
                eprintln!("[EditionConfig] Unknown edition value: {}", s);
                Edition::Enterprise
            }),
            Err(e) => {
                eprintln!("[EditionConfig] Query failed (table not initialized?): {}", e);
                Edition::Enterprise
            }
        }
    }

    pub fn set(&self, edition: &Edition) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute(
            "INSERT OR REPLACE INTO app_config (key, value) VALUES (?1, ?2)",
            rusqlite::params![CONFIG_KEY_CURRENT_EDITION, edition.as_str()],
        )
        .map_err(|e| format!("Failed to set edition: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_config() -> EditionConfig {
        let conn = Connection::open_in_memory().unwrap();
        let config = EditionConfig::new(conn);
        config.init_table().unwrap();
        config
    }

    #[test]
    fn test_edition_default_is_enterprise() {
        let config = new_config();
        assert_eq!(config.current(), Edition::Enterprise);
    }

    #[test]
    fn test_edition_switch() {
        let config = new_config();
        config.set(&Edition::Flagship).unwrap();
        assert_eq!(config.current(), Edition::Flagship);
        config.set(&Edition::Enterprise).unwrap();
        assert_eq!(config.current(), Edition::Enterprise);
    }
}
