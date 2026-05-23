use serde::{Deserialize, Serialize};

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
                            questions: vec![
                                "服务注册发现机制？".to_string(),
                            ],
                        },
                    ],
                },
                Section {
                    name: "安全".to_string(),
                    categories: vec![
                        Category {
                            name: "认证".to_string(),
                            questions: vec![
                                "支持的认证方式？".to_string(),
                            ],
                        },
                    ],
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
}
