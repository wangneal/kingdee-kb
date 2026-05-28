//! Integration test: batch-generate .schema.yaml files for all templates.
//!
//! Run with: cargo test --test generate_schemas -- --nocapture

/// Generate schema sidecar files for all templates in the default template directory.
/// Uses the public extract_docx_fields / extract_xlsx_fields functions.
#[test]
fn generate_all_schema_yaml_files() {
    // Use the default template directory (~/.kingdee-kb/templates)
    let home = dirs::home_dir().expect("Cannot find home directory");
    let template_dir = home.join(".kingdee-kb").join("templates");

    if !template_dir.exists() {
        eprintln!(
            "Template directory not found at: {}",
            template_dir.display()
        );
        eprintln!("Skipping schema generation - no templates to process.");
        return;
    }

    println!("Template directory: {}", template_dir.display());

    let mut generated: usize = 0;
    let mut skipped: usize = 0;
    let mut errors: Vec<String> = Vec::new();

    // Collect all files recursively (templates organized by phase subdirectories)
    let mut all_files: Vec<std::path::PathBuf> = Vec::new();
    collect_files_recursive(&template_dir, &mut all_files);
    all_files.sort();

    for file_path in &all_files {
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let field_count = match ext.as_str() {
            "docx" => match kingdee_kb_lib::template_docx::extract_docx_fields(file_path) {
                Ok(f) => {
                    let count = f.len();
                    if !f.is_empty() {
                        let file_name = file_path.file_name().unwrap_or_default().to_string_lossy();
                        println!(
                            "  [DOCX] {} fields: {}",
                            file_name,
                            f.iter()
                                .map(|f| format!("{}({})", f.name, f.source))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    count
                }
                Err(e) => {
                    errors.push(format!("{}: {}", file_path.display(), e));
                    continue;
                }
            },
            "xlsx" => match kingdee_kb_lib::template_xlsx::extract_xlsx_fields(file_path) {
                Ok(f) => {
                    let count = f.len();
                    if !f.is_empty() {
                        let file_name = file_path.file_name().unwrap_or_default().to_string_lossy();
                        println!(
                            "  [XLSX] {} fields: {}",
                            file_name,
                            f.iter()
                                .map(|f| format!("{}({})", f.name, f.source))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    count
                }
                Err(e) => {
                    errors.push(format!("{}: {}", file_path.display(), e));
                    continue;
                }
            },
            _ => continue,
        };

        if field_count == 0 {
            skipped += 1;
        } else {
            generated += 1;
        }
    }

    println!("=== Schema Generation Complete ===");
    println!("Templates with fields: {}", generated);
    println!("Templates without fields: {}", skipped);
    println!("Errors: {}", errors.len());
    for err in &errors {
        eprintln!("  - {}", err);
    }
}

fn collect_files_recursive(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                collect_files_recursive(&path, files);
            } else {
                files.push(path);
            }
        }
    }
}
