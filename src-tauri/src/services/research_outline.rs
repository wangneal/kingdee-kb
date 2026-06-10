use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

// SAFE: hardcoded regex patterns — documented outline structure formats
static RE_SECTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\s+(.+)").unwrap());
static RE_CATEGORY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\.\d+\s+(.+)").unwrap());
static RE_QUESTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+\.\d+\.\d+)\s+(.+)").unwrap());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Edition {
    Enterprise,
    Flagship,
}

impl Edition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Edition::Enterprise => "enterprise",
            Edition::Flagship => "flagship",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "enterprise" => Some(Edition::Enterprise),
            "flagship" => Some(Edition::Flagship),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchOutline {
    pub edition: Edition,
    pub module_code: String,
    pub module_name: String,
    pub cloud_type: String,
    pub doc_file: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub categories: Vec<Category>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatQuestion {
    pub edition: Edition,
    pub module_code: String,
    pub module_name: String,
    pub cloud_type: String,
    pub section: String,
    pub category: String,
    pub question_text: String,
    pub order: i32,
}

impl ResearchOutline {
    pub fn flatten(&self) -> Vec<FlatQuestion> {
        let mut result = Vec::new();
        let mut order = 0;
        for section in &self.sections {
            for category in &section.categories {
                for question in &category.questions {
                    result.push(FlatQuestion {
                        edition: self.edition.clone(),
                        module_code: self.module_code.clone(),
                        module_name: self.module_name.clone(),
                        cloud_type: self.cloud_type.clone(),
                        section: section.name.clone(),
                        category: category.name.clone(),
                        question_text: question.clone(),
                        order,
                    });
                    order += 1;
                }
            }
        }
        result
    }
}

pub fn parse_doc_file(filepath: &std::path::Path) -> Result<String, String> {
    let path_str = filepath
        .to_str()
        .ok_or("Invalid file path: non-UTF-8 characters")?;
    let escaped = path_str.replace('\'', "''");
    let script = format!(
        concat!(
            "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; ",
            "$OutputEncoding = [System.Text.Encoding]::UTF8; ",
            "$word = New-Object -ComObject Word.Application; ",
            "$word.Visible = $false; ",
            "$doc = $null; ",
            "try {{ ",
            "$doc = $word.Documents.Open('{path}'); ",
            "$content = $doc.Content.Text; ",
            "[Console]::Out.Write($content); ",
            "}} finally {{ ",
            "if ($doc -ne $null) {{ $doc.Close($false) | Out-Null; [System.Runtime.Interopservices.Marshal]::ReleaseComObject($doc) | Out-Null; }} ",
            "$word.Quit(); ",
            "[System.Runtime.Interopservices.Marshal]::ReleaseComObject($word) | Out-Null; ",
            "[System.GC]::Collect(); ",
            "[System.GC]::WaitForPendingFinalizers(); ",
            "}}",
        ),
        path = escaped
    );
    let output = std::process::Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(&script)
        .output()
        .map_err(|e| format!("Failed to execute PowerShell: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell error: {}", stderr));
    }
    let text =
        String::from_utf8(output.stdout).map_err(|e| format!("DOC 输出不是有效 UTF-8: {}", e))?;
    if text.trim().is_empty() {
        return Err("No content read from DOC file".to_string());
    }
    Ok(text)
}

pub fn parse_module_info(filename: &str) -> Option<(String, String, String)> {
    let re = Regex::new(r"^(\w+)_调研提纲_(.+?)_(.+?)_V\d+\.\d+\.docx?$").ok()?;
    let caps = re.captures(filename)?;
    Some((
        caps.get(1)?.as_str().to_string(),
        caps.get(2)?.as_str().to_string(),
        caps.get(3)?.as_str().to_string(),
    ))
}

fn clean_line(line: &str) -> String {
    line.chars()
        .filter(|&c| c != '\u{7}' && (c.is_ascii_graphic() || c.is_whitespace() || !c.is_ascii()))
        .collect()
}

pub fn parse_outline_text(
    text: &str,
    edition: Edition,
    module_code: &str,
    module_name: &str,
    cloud_type: &str,
    filename: &str,
) -> ResearchOutline {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = normalized.lines().map(clean_line).collect();

    let section_re = &RE_SECTION;
    let cat_re = &RE_CATEGORY;
    let q_re = &RE_QUESTION;

    let mut sections: Vec<Section> = Vec::new();
    let mut raw_questions: Vec<(String, String, String)> = Vec::new();
    let mut labels: Vec<String> = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.len() < 2 {
            continue;
        }
        if let Some(caps) = section_re.captures(trimmed) {
            sections.push(Section {
                name: caps
                    .get(1)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default(),
                categories: Vec::new(),
            });
        } else if let Some(caps) = cat_re.captures(trimmed) {
            if let Some(s) = sections.last_mut() {
                s.categories.push(Category {
                    name: caps
                        .get(1)
                        .map(|m| m.as_str().trim().to_string())
                        .unwrap_or_default(),
                    questions: Vec::new(),
                });
            } else {
                sections.push(Section {
                    name: String::new(),
                    categories: vec![Category {
                        name: caps
                            .get(1)
                            .map(|m| m.as_str().trim().to_string())
                            .unwrap_or_default(),
                        questions: Vec::new(),
                    }],
                });
            }
        } else if let Some(caps) = q_re.captures(trimmed) {
            let full_prefix = caps
                .get(1)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let q_text = caps
                .get(2)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            let section_part = full_prefix.split('.').next().unwrap_or("1").to_string();
            let cat_part: String = full_prefix
                .rsplit('.')
                .skip(1)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(".");
            raw_questions.push((section_part, cat_part, q_text));
        } else {
            labels.push(trimmed.to_string());
        }
    }

    if sections.is_empty() && !raw_questions.is_empty() {
        let mut unique_cats: Vec<String> = Vec::new();
        for (_, ref cp, _) in &raw_questions {
            if !unique_cats.contains(cp) {
                unique_cats.push(cp.clone());
            }
        }

        let mut label_iter = labels.into_iter();
        let first_label = label_iter.next().filter(|l| l.len() > 2);
        sections.push(Section {
            name: first_label.unwrap_or_default(),
            categories: Vec::new(),
        });

        for cp in &unique_cats {
            let cat_name = label_iter.next().unwrap_or_else(|| {
                let parts: Vec<&str> = cp.split('.').collect();
                if parts.len() >= 2 {
                    format!("类别{}", parts[1])
                } else {
                    String::new()
                }
            });
            sections[0].categories.push(Category {
                name: cat_name,
                questions: Vec::new(),
            });
        }

        for (_, cat_part, q_text) in &raw_questions {
            let cat_idx = unique_cats.iter().position(|c| c == cat_part);
            if let Some(ci) = cat_idx {
                if sections[0].categories.len() > ci {
                    sections[0].categories[ci].questions.push(q_text.clone());
                }
            }
        }
    } else {
        for (sec_part, cat_part, q_text) in &raw_questions {
            let sec_idx = sec_part.parse::<usize>().unwrap_or(1).saturating_sub(1);
            if sec_idx < sections.len() {
                let cat_num_in_sec: usize = cat_part
                    .rsplit('.')
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(1)
                    .saturating_sub(1);
                if cat_num_in_sec < sections[sec_idx].categories.len() {
                    sections[sec_idx].categories[cat_num_in_sec]
                        .questions
                        .push(q_text.clone());
                }
            }
        }
    }

    ResearchOutline {
        edition,
        module_code: module_code.to_string(),
        module_name: module_name.to_string(),
        cloud_type: cloud_type.to_string(),
        doc_file: filename.to_string(),
        sections,
    }
}

/// 大纲解析辅助函数（迁移期间预留）
#[allow(dead_code)]
fn try_parse_section_header(line: &str) -> Option<String> {
    let re = Regex::new(r"^\d+\s+(.+)").ok()?;
    let caps = re.captures(line)?;
    Some(caps.get(1)?.as_str().trim().to_string())
}

#[allow(dead_code)]
fn try_parse_category_header(line: &str) -> Option<String> {
    let re = Regex::new(r"^\d+\.\d+\s+(.+)").ok()?;
    let caps = re.captures(line)?;
    Some(caps.get(1)?.as_str().trim().to_string())
}

#[allow(dead_code)]
fn try_parse_question(line: &str) -> Option<String> {
    let re = Regex::new(r"^\d+\.\d+\.\d+\s+(.+)").ok()?;
    let caps = re.captures(line)?;
    Some(caps.get(1)?.as_str().trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
                            questions: vec!["服务注册发现机制？".to_string()],
                        },
                    ],
                },
                Section {
                    name: "安全".to_string(),
                    categories: vec![Category {
                        name: "认证".to_string(),
                        questions: vec!["支持的认证方式？".to_string()],
                    }],
                },
            ],
        }
    }

    #[test]
    fn test_edition_as_str() {
        assert_eq!(Edition::Enterprise.as_str(), "enterprise");
        assert_eq!(Edition::Flagship.as_str(), "flagship");
    }

    #[test]
    fn test_edition_from_str() {
        assert_eq!(Edition::from_str("enterprise"), Some(Edition::Enterprise));
        assert_eq!(Edition::from_str("flagship"), Some(Edition::Flagship));
        assert_eq!(Edition::from_str("other"), None);
        assert_eq!(Edition::from_str(""), None);
    }

    #[test]
    fn test_edition_serde_roundtrip() {
        let json = serde_json::to_string(&Edition::Enterprise).unwrap();
        assert_eq!(json, "\"enterprise\"");
        let deserialized: Edition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Edition::Enterprise);
    }

    #[test]
    fn test_flatten_empty_sections() {
        let outline = ResearchOutline {
            edition: Edition::Flagship,
            module_code: "ISS".to_string(),
            module_name: "智能服务平台".to_string(),
            cloud_type: "私有云".to_string(),
            doc_file: "ISS_research.md".to_string(),
            sections: vec![],
        };
        let flat = outline.flatten();
        assert!(flat.is_empty());
    }

    #[test]
    fn test_flatten_preserves_metadata() {
        let outline = sample_outline();
        let flat = outline.flatten();
        assert_eq!(flat.len(), 4);
        for q in &flat {
            assert_eq!(q.edition, Edition::Enterprise);
            assert_eq!(q.module_code, "BOS");
            assert_eq!(q.module_name, "基础平台");
            assert_eq!(q.cloud_type, "公有云");
        }
    }

    #[test]
    fn test_flatten_order_is_sequential() {
        let outline = sample_outline();
        let flat = outline.flatten();
        for (i, q) in flat.iter().enumerate() {
            assert_eq!(q.order, i as i32);
        }
    }

    #[test]
    fn test_flatten_content_correctness() {
        let flat = sample_outline().flatten();
        assert_eq!(flat[0].section, "架构");
        assert_eq!(flat[0].category, "部署架构");
        assert_eq!(flat[0].question_text, "支持的部署模式有哪些？");
        assert_eq!(flat[0].order, 0);

        assert_eq!(flat[1].section, "架构");
        assert_eq!(flat[1].category, "部署架构");
        assert_eq!(flat[1].question_text, "高可用方案如何？");
        assert_eq!(flat[1].order, 1);

        assert_eq!(flat[2].section, "架构");
        assert_eq!(flat[2].category, "微服务");
        assert_eq!(flat[2].question_text, "服务注册发现机制？");
        assert_eq!(flat[2].order, 2);

        assert_eq!(flat[3].section, "安全");
        assert_eq!(flat[3].category, "认证");
        assert_eq!(flat[3].question_text, "支持的认证方式？");
        assert_eq!(flat[3].order, 3);
    }

    #[test]
    fn test_serde_roundtrip() {
        let outline = sample_outline();
        let json = serde_json::to_string_pretty(&outline).unwrap();
        let deserialized: ResearchOutline = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.edition, outline.edition);
        assert_eq!(deserialized.module_code, outline.module_code);
        assert_eq!(deserialized.sections.len(), outline.sections.len());
        assert_eq!(deserialized.flatten().len(), outline.flatten().len());
    }

    #[test]
    fn test_try_parse_section_header() {
        assert_eq!(
            try_parse_section_header("1 业务概况"),
            Some("业务概况".to_string())
        );
        assert_eq!(
            try_parse_section_header("2 技术架构"),
            Some("技术架构".to_string())
        );
        assert_eq!(
            try_parse_section_header("10 安全"),
            Some("安全".to_string())
        );
        assert_eq!(try_parse_section_header("  1  带空格的章节"), None);
        assert_eq!(try_parse_section_header("1.1 分类"), None);
        assert_eq!(try_parse_section_header("无序文本"), None);
        assert_eq!(try_parse_section_header(""), None);
    }

    #[test]
    fn test_try_parse_category_header() {
        assert_eq!(
            try_parse_category_header("1.1 组织人员"),
            Some("组织人员".to_string())
        );
        assert_eq!(
            try_parse_category_header("2.3 数据存储"),
            Some("数据存储".to_string())
        );
        assert_eq!(
            try_parse_category_header("10.20 安全策略"),
            Some("安全策略".to_string())
        );
        assert_eq!(try_parse_category_header("1 章节"), None);
        assert_eq!(try_parse_category_header("1.1.1 问题"), None);
        assert_eq!(try_parse_category_header(""), None);
    }

    #[test]
    fn test_try_parse_question() {
        assert_eq!(
            try_parse_question("1.1.1 公司目前财务组织架构？"),
            Some("公司目前财务组织架构？".to_string())
        );
        assert_eq!(
            try_parse_question("2.3.1 使用什么数据库？"),
            Some("使用什么数据库？".to_string())
        );
        assert_eq!(
            try_parse_question("10.20.5 安全策略如何审计"),
            Some("安全策略如何审计".to_string())
        );
        assert_eq!(try_parse_question("1 章节"), None);
        assert_eq!(try_parse_question("1.1 分类"), None);
        assert_eq!(try_parse_question(""), None);
    }

    #[test]
    fn test_parse_module_info() {
        let result = parse_module_info("ECW2107_调研提纲_总账_财务_V1.0.doc");
        assert_eq!(
            result,
            Some((
                "ECW2107".to_string(),
                "总账".to_string(),
                "财务".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_module_info_with_long_version() {
        let result = parse_module_info("BOS123_调研提纲_基础平台_公有云_V10.20.doc");
        assert_eq!(
            result,
            Some((
                "BOS123".to_string(),
                "基础平台".to_string(),
                "公有云".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_module_info_invalid() {
        assert_eq!(parse_module_info("random_file.txt"), None);
        assert_eq!(parse_module_info(""), None);
        assert_eq!(parse_module_info("ECW2107_no_match.doc"), None);
    }

    #[test]
    fn test_parse_outline_text_basic() {
        let text = "1 业务概况\n\
                    1.1 组织人员\n\
                    1.1.1 公司目前财务组织架构？\n\
                    1.1.2 财务人员数量及分工？\n\
                    1.2 信息系统\n\
                    1.2.1 目前使用什么财务系统？\n\
                    2 技术架构\n\
                    2.1 数据存储\n\
                    2.1.1 使用什么数据库？\n";
        let outline = parse_outline_text(
            text,
            Edition::Enterprise,
            "ECW2107",
            "总账",
            "财务",
            "test.doc",
        );
        assert_eq!(outline.edition, Edition::Enterprise);
        assert_eq!(outline.module_code, "ECW2107");
        assert_eq!(outline.module_name, "总账");
        assert_eq!(outline.cloud_type, "财务");
        assert_eq!(outline.doc_file, "test.doc");
        assert_eq!(outline.sections.len(), 2);
        assert_eq!(outline.sections[0].name, "业务概况");
        assert_eq!(outline.sections[0].categories.len(), 2);
        assert_eq!(outline.sections[0].categories[0].name, "组织人员");
        assert_eq!(outline.sections[0].categories[0].questions.len(), 2);
        assert_eq!(
            outline.sections[0].categories[0].questions[0],
            "公司目前财务组织架构？"
        );
        assert_eq!(
            outline.sections[0].categories[1].questions[0],
            "目前使用什么财务系统？"
        );
        assert_eq!(outline.sections[1].name, "技术架构");
        assert_eq!(outline.sections[1].categories[0].name, "数据存储");

        let flat = outline.flatten();
        assert_eq!(flat.len(), 4);
        assert_eq!(flat[0].section, "业务概况");
        assert_eq!(flat[0].category, "组织人员");
        assert_eq!(flat[0].question_text, "公司目前财务组织架构？");
        assert_eq!(flat[0].order, 0);
        assert_eq!(flat[2].section, "业务概况");
        assert_eq!(flat[2].category, "信息系统");
        assert_eq!(flat[2].question_text, "目前使用什么财务系统？");
        assert_eq!(flat[2].order, 2);
        assert_eq!(flat[3].section, "技术架构");
        assert_eq!(flat[3].category, "数据存储");
        assert_eq!(flat[3].question_text, "使用什么数据库？");
        assert_eq!(flat[3].order, 3);
    }

    #[test]
    fn test_parse_outline_text_skips_unmatched_lines() {
        let text = "这是一段前言\n\
                    1 正式章节\n\
                    一些描述文字\n\
                    1.1 分类\n\
                    1.1.1 问题内容\n\
                    结尾备注\n";
        let outline = parse_outline_text(text, Edition::Flagship, "M", "N", "C", "f.doc");
        assert_eq!(outline.sections.len(), 1);
        assert_eq!(outline.sections[0].categories.len(), 1);
        assert_eq!(outline.sections[0].categories[0].questions.len(), 1);
    }

    #[test]
    fn test_parse_outline_text_empty() {
        let outline = parse_outline_text("", Edition::Enterprise, "M", "N", "C", "f.doc");
        assert!(outline.sections.is_empty());
        assert!(outline.flatten().is_empty());
    }

    #[test]
    fn test_parse_outline_text_no_question_category() {
        let text = "1 章节\n1.1 分类\n没有编号的文本\n1.1.1 问题\n";
        let outline = parse_outline_text(text, Edition::Enterprise, "M", "N", "C", "f.doc");
        assert_eq!(outline.sections.len(), 1);
        assert_eq!(outline.sections[0].categories.len(), 1);
        assert_eq!(outline.sections[0].categories[0].questions.len(), 1);
    }

    #[test]
    fn test_parse_doc_file_real() {
        let candidate_paths = [r"E:\工作资料\项目资料\企业版调研提纲\企业版"];
        let dir = std::path::Path::new(candidate_paths[0]);
        if !dir.exists() {
            eprintln!(
                "Skipping test_parse_doc_file_real: directory not found at {:?}",
                dir
            );
            return;
        }
        let entries: Vec<_> = match std::fs::read_dir(dir) {
            Ok(e) => e.filter_map(|e| e.ok()).collect(),
            Err(_) => return,
        };
        let doc_files: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "doc" || ext == "docx")
                    .unwrap_or(false)
            })
            .collect();
        if doc_files.is_empty() {
            eprintln!("Skipping test_parse_doc_file_real: no doc files found");
            return;
        }
        for entry in doc_files.iter().take(3) {
            let path = entry.path();
            let result = parse_doc_file(&path);
            assert!(result.is_ok(), "Failed to parse {:?}: {:?}", path, result);
            let text = result.unwrap();
            assert!(!text.trim().is_empty(), "Empty content from {:?}", path);
            let filename = path.file_name().unwrap().to_str().unwrap_or("");
            let info = parse_module_info(filename);
            assert!(
                info.is_some(),
                "Cannot parse module info from filename: {}",
                filename
            );
            let (code, mod_name, cloud) = info.unwrap();
            let outline = parse_outline_text(
                &text,
                Edition::Enterprise,
                &code,
                &mod_name,
                &cloud,
                filename,
            );
            eprintln!("Sections: {}", outline.sections.len());
            let flat = outline.flatten();
            eprintln!(
                "OK: {} → {} sections, {} questions",
                filename,
                outline.sections.len(),
                flat.len()
            );
        }
    }

    #[test]
    fn test_parse_doc_file_nonexistent() {
        let result = parse_doc_file(std::path::Path::new(r"C:\nonexistent_file_12345.doc"));
        assert!(result.is_err());
    }
}
