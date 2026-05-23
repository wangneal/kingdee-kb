//! Template directory scanner
//!
//! Scans 实施方法论V10.0交付物模板/ directory and builds a registry
//! of all templates organized by the 8 implementation phases.

use serde::{Deserialize, Serialize};
use std::path::Path;
use walkdir::WalkDir;

/// Supported template file formats (v0.2: docx + xlsx only)
const SUPPORTED_EXTENSIONS: &[&str] = &["docx", "xlsx"];

/// Phase names in the Kingdee implementation methodology
pub const PHASE_NAMES: &[&str] = &[
    "项目管理",
    "启动阶段",
    "需求阶段",
    "方案阶段",
    "构建阶段",
    "测试阶段",
    "上线阶段",
    "验收阶段",
];

/// Template metadata returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateInfo {
    /// Unique ID (SHA256 of relative path, first 12 chars)
    pub id: String,
    /// Display name (filename without extension and prefix numbers)
    pub name: String,
    /// Full filename with extension
    pub filename: String,
    /// Phase name (e.g., "项目管理", "启动阶段")
    pub phase: String,
    /// Phase index 0-7
    pub phase_index: u8,
    /// File format: "docx" or "xlsx"
    pub format: String,
    /// Absolute file path
    pub file_path: String,
    /// Relative path from template root
    pub relative_path: String,
    /// File size in bytes
    pub file_size: u64,
}

/// Scan the template directory and return all templates sorted by phase.
///
/// - Skips temp files (`~$` prefix)
/// - Skips unsupported formats (.pptx, .doc, .xls)
/// - Organizes by 8 phases based on directory name
pub fn scan_templates(root_dir: &Path) -> Result<Vec<TemplateInfo>, String> {
    if !root_dir.exists() {
        // Auto-create the templates directory so the app can start cleanly
        std::fs::create_dir_all(root_dir)
            .map_err(|e| format!("Failed to create template directory {}: {}", root_dir.display(), e))?;
        return Ok(Vec::new());
    }

    let mut templates = Vec::new();

    for entry in WalkDir::new(root_dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            // Skip hidden directories
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.')
        })
    {
        let entry = entry.map_err(|e| format!("Walk error: {}", e))?;

        if !entry.file_type().is_file() {
            continue;
        }

        let filename = entry.file_name().to_string_lossy().to_string();

        // Skip temp files (~$ prefix from Word)
        if filename.starts_with("~$") {
            continue;
        }

        // Get extension
        let ext = Path::new(&filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Only support docx/xlsx (v0.2)
        if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        // Determine phase from parent directory name
        let relative_path = entry
            .path()
            .strip_prefix(root_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| filename.clone());

        let phase_info = extract_phase(&relative_path);
        let (phase_name, phase_index) = phase_info.unwrap_or(("未分类".to_string(), 8));

        // Generate ID from relative path
        let id = generate_id(&relative_path);

        // Extract display name
        let display_name = extract_display_name(&filename);

        // File size
        let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);

        templates.push(TemplateInfo {
            id,
            name: display_name,
            filename,
            phase: phase_name,
            phase_index,
            format: ext,
            file_path: entry.path().to_string_lossy().to_string(),
            relative_path,
            file_size,
        });
    }

    // Sort by phase_index, then by filename
    templates.sort_by(|a, b| {
        a.phase_index
            .cmp(&b.phase_index)
            .then_with(|| a.filename.cmp(&b.filename))
    });

    Ok(templates)
}

/// Extract phase name and index from the relative path.
///
/// Directory names follow the pattern: `{index}{phase_name}` (e.g., "0项目管理", "1启动阶段")
fn extract_phase(relative_path: &str) -> Option<(String, u8)> {
    let first_component = relative_path.split(std::path::MAIN_SEPARATOR).next()?;

    for (i, &phase) in PHASE_NAMES.iter().enumerate() {
        if first_component.contains(phase) {
            return Some((phase.to_string(), i as u8));
        }
    }

    // Try to extract from directory name pattern like "0项目管理"
    if let Some(first_char) = first_component.chars().next() {
        if let Some(digit) = first_char.to_digit(10) {
            let phase_name: String = first_component.chars().skip(1).collect();
            if PHASE_NAMES.contains(&phase_name.as_str()) {
                return Some((phase_name, digit as u8));
            }
        }
    }

    None
}

/// Generate a short unique ID from the relative path.
fn generate_id(relative_path: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(relative_path.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    hash[..12].to_string()
}

/// Extract a clean display name from filename.
///
/// Removes prefix numbers like "01", "02" and suffixes like "_模板（for V10.0）".
fn extract_display_name(filename: &str) -> String {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);

    // Remove leading digits and whitespace (e.g., "01项目调研计划" → "项目调研计划")
    let name = stem.trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ');

    // Remove common suffixes
    let name = name
        .split("_模板")
        .next()
        .unwrap_or(name)
        .split("_Template")
        .next()
        .unwrap_or(name);

    // Clean up trailing whitespace and special chars
    let name = name.trim().trim_end_matches('_').trim();

    if name.is_empty() {
        stem.to_string()
    } else {
        name.to_string()
    }
}

/// Template index structure for templates.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateIndex {
    /// Version of the index format
    pub version: String,
    /// Total template count
    pub total_count: usize,
    /// Templates grouped by phase (category)
    pub categories: Vec<TemplateCategory>,
    /// Flat list of all templates
    pub templates: Vec<TemplateInfo>,
}

/// A category (phase) in the template index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateCategory {
    /// Phase name (e.g., "项目管理")
    pub name: String,
    /// Phase index (0-7)
    pub index: u8,
    /// Number of templates in this phase
    pub count: usize,
    /// Template IDs in this phase
    pub template_ids: Vec<String>,
}

/// Build a complete template index as JSON string.
///
/// Scans the template directory, groups templates by phase,
/// and produces a structured JSON index.
pub fn build_templates_json(template_dir: &Path) -> Result<String, String> {
    let templates = scan_templates(template_dir)?;

    // Group by phase
    let mut categories: Vec<TemplateCategory> = Vec::new();
    for phase_idx in 0..PHASE_NAMES.len() {
        let phase_name = PHASE_NAMES[phase_idx];
        let ids: Vec<String> = templates
            .iter()
            .filter(|t| t.phase_index == phase_idx as u8)
            .map(|t| t.id.clone())
            .collect();

        if !ids.is_empty() {
            categories.push(TemplateCategory {
                name: phase_name.to_string(),
                index: phase_idx as u8,
                count: ids.len(),
                template_ids: ids,
            });
        }
    }

    // Handle uncategorized templates
    let uncategorized: Vec<String> = templates
        .iter()
        .filter(|t| t.phase_index >= PHASE_NAMES.len() as u8)
        .map(|t| t.id.clone())
        .collect();

    if !uncategorized.is_empty() {
        categories.push(TemplateCategory {
            name: "未分类".to_string(),
            index: 8,
            count: uncategorized.len(),
            template_ids: uncategorized,
        });
    }

    let index = TemplateIndex {
        version: "1.0".to_string(),
        total_count: templates.len(),
        categories,
        templates,
    };

    serde_json::to_string_pretty(&index).map_err(|e| format!("JSON serialization error: {}", e))
}

/// Write templates.json to a file and return the JSON string.
pub fn write_templates_json(template_dir: &Path, output_path: &Path) -> Result<String, String> {
    let json = build_templates_json(template_dir)?;

    // Create parent directory if needed
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    std::fs::write(output_path, &json)
        .map_err(|e| format!("Failed to write {}: {}", output_path.display(), e))?;

    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_phase() {
        assert_eq!(
            extract_phase("0项目管理/01风险跟踪记录表.xlsx"),
            Some(("项目管理".to_string(), 0))
        );
        assert_eq!(
            extract_phase("3方案阶段/02会议纪要.docx"),
            Some(("方案阶段".to_string(), 3))
        );
        assert_eq!(extract_phase("unknown/file.docx"), None);
    }

    #[test]
    fn test_extract_display_name() {
        assert_eq!(
            extract_display_name("01项目调研计划_模板（for V10.0）.docx"),
            "项目调研计划"
        );
        assert_eq!(
            extract_display_name("02会议纪要_模板 （for V10.0）.docx"),
            "会议纪要"
        );
        assert_eq!(
            extract_display_name("06调研报告_模板（for V10.0）.docx"),
            "调研报告"
        );
    }

    #[test]
    fn test_generate_id() {
        let id = generate_id("0项目管理/01风险跟踪记录表.xlsx");
        assert_eq!(id.len(), 12);
        // Same path should produce same ID
        assert_eq!(id, generate_id("0项目管理/01风险跟踪记录表.xlsx"));
        // Different path should produce different ID
        assert_ne!(id, generate_id("1启动阶段/01项目通讯录.xlsx"));
    }
}
