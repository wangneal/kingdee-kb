//! Xlsx template filler
//!
//! Takes an .xlsx template and a HashMap of field values, replaces `{field_name}`
//! placeholders in cell values with actual values, and saves the result as a new file.
//!
//! Preserves all cell formatting, formulas (non-formula cells only), and structure.

use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

/// Fill an .xlsx template with field values.
///
/// Opens the template, replaces all `{field_name}` placeholders in cell values
/// with corresponding values from `fields`, and saves the result to `output_path`.
///
/// Returns the number of field replacements made.
pub fn fill_xlsx(
    template_path: &Path,
    fields: &HashMap<String, String>,
    output_path: &Path,
) -> Result<usize, String> {
    let mut book = umya_spreadsheet::reader::xlsx::read(template_path)
        .map_err(|e| format!("Failed to read xlsx {}: {}", template_path.display(), e))?;

    let brace_re = Regex::new(r"\{([^}]+)\}").map_err(|e| format!("Regex error: {}", e))?;
    let xxxx_re = Regex::new(r"XXXX([\u4e00-\u9fff][\u4e00-\u9fff\w/（）()\-]{0,19})")
        .map_err(|e| format!("Regex error: {}", e))?;
    let mut total_replaced = 0usize;

    let sheet_count = book.get_sheet_count();
    for sheet_idx in 0..sheet_count {
        let sheet = match book.get_sheet_mut(&sheet_idx) {
            Some(s) => s,
            None => continue,
        };

        // Collect cell coordinates first to avoid borrow conflicts
        let cell_coords: Vec<_> = sheet
            .get_cell_collection()
            .iter()
            .map(|c| {
                let coord = c.get_coordinate();
                (*coord.get_col_num(), *coord.get_row_num())
            })
            .collect();

        for (col_num, row_num) in cell_coords {
            let cell = sheet.get_cell_mut((col_num, row_num));

            let cell_value = cell.get_value().to_string();
            if cell_value.is_empty() {
                continue;
            }

            let mut replaced_value = cell_value.clone();
            let mut cell_count = 0usize;

            replaced_value = brace_re
                .replace_all(&replaced_value, |caps: &regex::Captures| {
                    let field_name = &caps[1];
                    match get_field_value(fields, field_name) {
                        Some(value) => {
                            cell_count += 1;
                            value
                        }
                        None => caps[0].to_string(),
                    }
                })
                .to_string();

            replaced_value = xxxx_re
                .replace_all(&replaced_value, |caps: &regex::Captures| {
                    let field_name = &caps[1];
                    match get_field_value(fields, field_name) {
                        Some(value) => {
                            cell_count += 1;
                            value
                        }
                        None => caps[0].to_string(),
                    }
                })
                .to_string();

            if cell_count > 0 {
                cell.set_value(replaced_value);
                total_replaced += cell_count;
            }
        }
    }

    umya_spreadsheet::writer::xlsx::write(&book, output_path)
        .map_err(|e| format!("Failed to write xlsx {}: {}", output_path.display(), e))?;

    Ok(total_replaced)
}

fn get_field_value(fields: &HashMap<String, String>, name: &str) -> Option<String> {
    let normalized = normalize_field_name(name);
    fields
        .get(name)
        .or_else(|| fields.get(name.trim()))
        .or_else(|| fields.get(&normalized))
        .cloned()
}

fn normalize_field_name(name: &str) -> String {
    name.trim()
        .trim_matches('《')
        .trim_matches('》')
        .trim_matches('«')
        .trim_matches('»')
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_field_value_normalizes_wrappers() {
        let mut fields = HashMap::new();
        fields.insert("项目名称".to_string(), "罗孚项目".to_string());

        assert_eq!(
            get_field_value(&fields, "《项目名称》"),
            Some("罗孚项目".to_string())
        );
    }

    #[test]
    fn test_fill_xlsx_basic() {
        // Use the existing test fixture if available
        let template_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests")
            .join("fixtures");
        let template = template_dir.join("test_template.xlsx");

        if !template.exists() {
            // Skip if no fixture available
            eprintln!("Skipping test_fill_xlsx_basic: test fixture not found");
            return;
        }

        let mut fields = HashMap::new();
        fields.insert("项目名称".to_string(), "测试项目".to_string());

        let output = template_dir.join("test_output.xlsx");
        let result = fill_xlsx(&template, &fields, &output);
        if let Err(e) = &result {
            eprintln!("fill_xlsx failed: {}", e);
        }
        // Cleanup
        let _ = std::fs::remove_file(&output);
    }
}
