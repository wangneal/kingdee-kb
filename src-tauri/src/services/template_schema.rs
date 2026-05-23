//! YAML schema generator for templates
//!
//! Auto-generates a minimal YAML sidecar schema for each template,
//! with all fields defaulting to `fill_strategy: "user"` and `required: true`.

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::template_docx::FieldInfo as DocxField;
use super::template_xlsx::XlsxFieldInfo;

/// YAML schema structure for a template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateSchema {
    /// Template metadata
    pub template: SchemaTemplateMeta,
    /// Field definitions
    pub fields: Vec<SchemaField>,
}

/// Template metadata in the schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaTemplateMeta {
    /// Template ID
    pub id: String,
    /// Template name
    pub name: String,
    /// File format
    pub format: String,
    /// Phase
    pub phase: String,
}

/// Field definition in the schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaField {
    /// Field name (matches the `{field_name}` placeholder)
    pub name: String,
    /// Field type: "text", "number", "date"
    #[serde(rename = "type")]
    pub field_type: String,
    /// Fill strategy: "user" (manual), "ai" (LLM-generated), "kb" (from knowledge base)
    pub fill_strategy: String,
    /// Whether the field is required
    pub required: bool,
    /// Optional default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Cell references (xlsx only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_refs: Option<Vec<String>>,
}

/// Generate a YAML schema from extracted docx fields.
pub fn generate_schema_from_docx(
    template_id: &str,
    template_name: &str,
    phase: &str,
    fields: &[DocxField],
) -> TemplateSchema {
    let schema_fields: Vec<SchemaField> = fields
        .iter()
        .map(|f| SchemaField {
            name: f.name.clone(),
            field_type: f.field_type.clone(),
            fill_strategy: "user".to_string(),
            required: true,
            default: None,
            description: None,
            cell_refs: None,
        })
        .collect();

    TemplateSchema {
        template: SchemaTemplateMeta {
            id: template_id.to_string(),
            name: template_name.to_string(),
            format: "docx".to_string(),
            phase: phase.to_string(),
        },
        fields: schema_fields,
    }
}

/// Generate a YAML schema from extracted xlsx fields.
pub fn generate_schema_from_xlsx(
    template_id: &str,
    template_name: &str,
    phase: &str,
    fields: &[XlsxFieldInfo],
) -> TemplateSchema {
    let schema_fields: Vec<SchemaField> = fields
        .iter()
        .map(|f| SchemaField {
            name: f.name.clone(),
            field_type: f.field_type.clone(),
            fill_strategy: "user".to_string(),
            required: true,
            default: None,
            description: None,
            cell_refs: Some(f.cell_refs.clone()),
        })
        .collect();

    TemplateSchema {
        template: SchemaTemplateMeta {
            id: template_id.to_string(),
            name: template_name.to_string(),
            format: "xlsx".to_string(),
            phase: phase.to_string(),
        },
        fields: schema_fields,
    }
}

/// Serialize a schema to YAML string.
pub fn schema_to_yaml(schema: &TemplateSchema) -> Result<String, String> {
    serde_yaml::to_string(schema).map_err(|e| format!("YAML serialization error: {}", e))
}

/// Write a schema YAML sidecar file next to the template.
///
/// Creates `{template_path}.schema.yaml` alongside the template file.
pub fn write_schema_sidecar(template_path: &Path, schema: &TemplateSchema) -> Result<(), String> {
    let sidecar_path = template_path.with_extension("schema.yaml");
    let yaml = schema_to_yaml(schema)?;
    std::fs::write(&sidecar_path, yaml)
        .map_err(|e| format!("Failed to write schema {}: {}", sidecar_path.display(), e))?;
    Ok(())
}
