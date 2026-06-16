//! Xlsx field placeholder extractor
//!
//! Parses .xlsx files to find field placeholders in cells.
//! Supports two Kingdee template formats:
//!   1. `{field_name}` — brace-style placeholders
//!   2. `XXXX字段名` — Kingdee XXXX-style placeholders
//!
//! Extracts field names with their cell references (sheet, row, col).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Field placeholder extracted from an xlsx template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XlsxFieldInfo {
    /// Field name (e.g., "项目名称")
    pub name: String,
    /// Inferred field type: "text", "number", "date"
    pub field_type: String,
    /// Cell references where this field appears: vec of "Sheet1!A1"
    pub cell_refs: Vec<String>,
    /// Number of occurrences
    pub count: usize,
    /// Source format: "brace", "xxxx"
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "brace".to_string()
}

/// Extract all field placeholders from an .xlsx file.
///
/// Strategy:
/// 1. Read workbook with umya-spreadsheet
/// 2. Iterate all sheets and cells
/// 3. Apply regex patterns for `{name}` and `XXXX名称` on each cell value
/// 4. Return deduplicated field list with cell references
pub fn extract_xlsx_fields(file_path: &Path) -> Result<Vec<XlsxFieldInfo>, String> {
    let book = umya_spreadsheet::reader::xlsx::read(file_path)
        .map_err(|e| format!("Failed to read xlsx {}: {}", file_path.display(), e))?;

    let brace_re = Regex::new(r"\{([^}]+)\}").map_err(|e| format!("Regex error: {}", e))?;
    let xxxx_re = Regex::new(r"XXXX([\u4e00-\u9fff][\u4e00-\u9fff\w/（）()\-]{0,19})").unwrap();

    let mut fields: BTreeMap<String, XlsxFieldInfo> = BTreeMap::new();

    let sheet_count = book.get_sheet_count();
    for sheet_idx in 0..sheet_count {
        let sheet = match book.get_sheet(&sheet_idx) {
            Some(s) => s,
            None => continue,
        };

        let sheet_name = sheet.get_name().to_string();

        // Iterate all cells in this sheet
        let cells = sheet.get_cell_collection();
        for cell in &cells {
            let cell_value = cell.get_value();
            if cell_value.is_empty() {
                continue;
            }

            let coord = cell.get_coordinate();
            let col_num = *coord.get_col_num();
            let row_num = *coord.get_row_num();
            let cell_ref = format!("{}{}", col_to_letter(col_num), row_num);
            let full_ref = format!("{}!{}", sheet_name, cell_ref);

            // 1. Brace-style: {field_name}
            for cap in brace_re.captures_iter(&cell_value) {
                let field_name = cap[1].trim().to_string();
                if field_name.is_empty() {
                    continue;
                }
                let entry = fields
                    .entry(field_name.clone())
                    .or_insert_with(|| XlsxFieldInfo {
                        name: field_name.clone(),
                        field_type: infer_field_type(&field_name),
                        cell_refs: Vec::new(),
                        count: 0,
                        source: "brace".to_string(),
                    });

                entry.cell_refs.push(full_ref.clone());
                entry.count += 1;
            }

            // 2. Kingdee XXXX-style: XXXX字段名
            for cap in xxxx_re.captures_iter(&cell_value) {
                let field_name = cap[1].trim().to_string();
                // Filter: skip empty, too long (>30 bytes / >10 chars), or already found via brace
                if field_name.is_empty() || field_name.len() > 30 || field_name.chars().count() > 10
                {
                    continue;
                }
                if fields.contains_key(&field_name) {
                    // Already exists, just add cell ref
                    if let Some(entry) = fields.get_mut(&field_name) {
                        entry.cell_refs.push(full_ref.clone());
                        entry.count += 1;
                    }
                    continue;
                }
                let entry = fields
                    .entry(field_name.clone())
                    .or_insert_with(|| XlsxFieldInfo {
                        name: field_name.clone(),
                        field_type: infer_field_type(&field_name),
                        cell_refs: Vec::new(),
                        count: 0,
                        source: "xxxx".to_string(),
                    });

                entry.cell_refs.push(full_ref.clone());
                entry.count += 1;
            }
        }
    }

    Ok(fields.into_values().collect())
}

/// Convert a 1-based column number to Excel letter (1=A, 2=B, ..., 26=Z, 27=AA).
fn col_to_letter(mut col: u32) -> String {
    let mut result = String::new();
    while col > 0 {
        col -= 1;
        result.insert(0, (b'A' + (col % 26) as u8) as char);
        col /= 26;
    }
    result
}

/// Infer field type from field name (same heuristic as docx).
fn infer_field_type(name: &str) -> String {
    let name_lower = name.to_lowercase();

    if name_lower.contains("日期")
        || name_lower.contains("时间")
        || name_lower.contains("date")
        || name_lower.contains("time")
    {
        return "date".to_string();
    }

    if name_lower.contains("数量")
        || name_lower.contains("金额")
        || name_lower.contains("价格")
        || name_lower.contains("比例")
        || name_lower.contains("百分")
        || name_lower.contains("天数")
        || name_lower.contains("number")
        || name_lower.contains("amount")
    {
        return "number".to_string();
    }

    "text".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_col_to_letter() {
        assert_eq!(col_to_letter(1), "A");
        assert_eq!(col_to_letter(2), "B");
        assert_eq!(col_to_letter(26), "Z");
        assert_eq!(col_to_letter(27), "AA");
        assert_eq!(col_to_letter(28), "AB");
    }
}
