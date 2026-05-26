//! Integration test: batch-generate .schema.yaml files for all templates.
//!
//! Run with: cargo test --test generate_schemas -- --nocapture

/// Generate schema sidecar files for all templates in the default template directory.
#[test]
fn generate_all_schema_yaml_files() {
    // Use the default template directory (~/.kingdee-kb/templates)
    let home = dirs::home_dir().expect("Cannot find home directory");
    let template_dir = home.join(".kingdee-kb").join("templates");

    if !template_dir.exists() {
        eprintln!("Template directory not found at: {}", template_dir.display());
        eprintln!("Skipping schema generation - no templates to process.");
        return;
    }

    println!("Template directory: {}", template_dir.display());
    println!("Generating schema files...");

    // Force regenerate all schemas
    let result = kingdee_kb_lib::services::template_schema::batch_generate_schemas(
        &template_dir,
        true, // force overwrite
    );

    match result {
        Ok((generated, skipped, errors)) => {
            println!("=== Schema Generation Complete ===");
            println!("Generated: {}", generated);
            println!("Skipped (already existed): {}", skipped);

            if errors.is_empty() {
                println!("Errors: 0");
            } else {
                println!("Errors: {}", errors.len());
                for err in &errors {
                    eprintln!("  - {}", err);
                }
            }

            if generated > 0 || !errors.is_empty() {
                println!("Check the template directory for new .schema.yaml files.");
            }
        }
        Err(e) => {
            eprintln!("Schema generation failed: {}", e);
            panic!("Schema generation failed: {}", e);
        }
    }
}
