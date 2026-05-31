//! 系统提示组装器 — 将技能列表注入系统提示
//!
//! 参考 Claude Code 技能系统的提示注入机制：
//!   - 按使用频率排序技能列表
//!   - 控制 token 预算（上下文窗口的 1%）
//!   - 支持压缩模式

use crate::services::skill_types::{extract_triggers_from_body, Skill};

/// 系统提示组装器
pub struct PromptAssembler {
    /// 技能列表的 token 预算
    token_budget: usize,
}

/// 技能提示条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillPromptEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub phase: Option<String>,
    pub triggers: Vec<String>,
}

impl PromptAssembler {
    /// 创建提示组装器
    pub fn new(context_window_size: usize) -> Self {
        Self {
            // 技能列表占上下文窗口的 1%
            token_budget: context_window_size / 100,
        }
    }

    /// 生成技能列表注入系统提示的文本
    pub fn build_skill_list_prompt(&self, skills: &[Skill]) -> String {
        let mut prompt = String::from("# Available Skills\n\n");
        let mut used_tokens = 0;

        // 按分类排序
        let mut sorted_skills: Vec<&Skill> = skills.iter().collect();
        sorted_skills.sort_by(|a, b| {
            let cat_ord = a
                .metadata
                .category
                .to_string()
                .cmp(&b.metadata.category.to_string());
            if cat_ord == std::cmp::Ordering::Equal {
                a.name.cmp(&b.name)
            } else {
                cat_ord
            }
        });

        let mut current_category = String::new();

        for skill in sorted_skills {
            let cat = skill.metadata.category.to_string();

            // 分类标题
            if cat != current_category {
                prompt.push_str(&format!("## {}\n\n", Self::category_title(&cat)));
                current_category = cat;
                used_tokens += Self::estimate_tokens(&format!("## {}\n\n", current_category));
            }

            let skill_text = self.format_skill_entry(skill);
            let tokens = Self::estimate_tokens(&skill_text);

            if used_tokens + tokens > self.token_budget {
                // 尝试压缩
                let compressed = self.format_skill_entry_compressed(skill);
                let compressed_tokens = Self::estimate_tokens(&compressed);

                if used_tokens + compressed_tokens > self.token_budget {
                    break; // 超出预算，跳过
                }

                prompt.push_str(&compressed);
                used_tokens += compressed_tokens;
            } else {
                prompt.push_str(&skill_text);
                used_tokens += tokens;
            }
        }

        prompt
    }

    /// 生成技能摘要列表（用于前端展示）
    pub fn build_skill_summaries(&self, skills: &[Skill]) -> Vec<SkillPromptEntry> {
        skills
            .iter()
            .map(|s| SkillPromptEntry {
                id: s.name.clone(),
                name: s.metadata.name.clone().unwrap_or_else(|| s.name.clone()),
                description: s.metadata.description.clone().unwrap_or_default(),
                category: s.metadata.category.to_string(),
                phase: match &s.metadata.phase {
                    crate::services::skill_types::SkillPhase::All => Some("all".to_string()),
                    crate::services::skill_types::SkillPhase::Specific(p) => Some(p.clone()),
                },
                triggers: extract_triggers_from_body(&s.body),
            })
            .collect()
    }

    /// 格式化单个技能条目（完整版）
    fn format_skill_entry(&self, skill: &Skill) -> String {
        let mut entry = format!(
            "- **{}**: {}",
            skill.name,
            Self::truncate(&skill.metadata.description.clone().unwrap_or_default(), 100)
        );

        let triggers = extract_triggers_from_body(&skill.body);
        if !triggers.is_empty() {
            entry.push_str(&format!(" [{}]", triggers.join(", ")));
        }

        entry.push('\n');
        entry
    }

    /// 格式化单个技能条目（压缩版）
    fn format_skill_entry_compressed(&self, skill: &Skill) -> String {
        format!(
            "- {}: {}\n",
            skill.name,
            Self::truncate(&skill.metadata.description.clone().unwrap_or_default(), 50)
        )
    }

    /// 分类标题映射
    fn category_title(category: &str) -> &str {
        match category {
            "core" => "Core Skills",
            "stage" => "Phase Skills",
            "mgmt" | "management" => "Management Skills",
            "tool" => "Tool Skills",
            _ => "Other Skills",
        }
    }

    /// 截断文本（UTF-8 安全）
    fn truncate(text: &str, max_len: usize) -> String {
        if text.len() <= max_len {
            text.to_string()
        } else {
            // 找到不超过 max_len 的最后一个字符边界
            let mut end = max_len;
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...", &text[..end])
        }
    }

    /// 粗略估算 token 数量
    fn estimate_tokens(text: &str) -> usize {
        // 中文约 1.5 字符/token，英文约 4 字符/token
        let chinese_chars = text.chars().filter(|c| !c.is_ascii()).count();
        let ascii_chars = text.len() - chinese_chars;
        (chinese_chars as f64 / 1.5 + ascii_chars as f64 / 4.0) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::skill_types::{SkillCategory, SkillMetadata, SkillPhase};

    fn create_test_skill(name: &str, description: &str) -> Skill {
        Skill {
            name: name.to_string(),
            location: format!("/skills/{}/SKILL.md", name),
            metadata: SkillMetadata {
                name: Some(name.to_string()),
                description: Some(description.to_string()),
                version: Some("1.0".to_string()),
                category: SkillCategory::Tool,
                phase: SkillPhase::All,
                icon: None,
                paths: Vec::new(),
            },
            body: "## 触发条件\n- \"test\" \"testing\"".to_string(),
            scripts: Vec::new(),
            references: Vec::new(),
        }
    }

    #[test]
    fn test_build_prompt() {
        let skills = vec![
            create_test_skill("skill-a", "Description A"),
            create_test_skill("skill-b", "Description B"),
        ];

        let assembler = PromptAssembler::new(100000);
        let prompt = assembler.build_skill_list_prompt(&skills);

        assert!(prompt.contains("Available Skills"));
        assert!(prompt.contains("skill-a"));
        assert!(prompt.contains("skill-b"));
    }

    #[test]
    fn test_token_budget() {
        let skills: Vec<Skill> = (0..100)
            .map(|i| create_test_skill(&format!("skill-{}", i), &format!("Description {}", i)))
            .collect();

        // 很小的 token 预算
        let assembler = PromptAssembler::new(1000);
        let prompt = assembler.build_skill_list_prompt(&skills);

        // 应该截断，不会包含所有 100 个技能
        let count = prompt.matches("skill-").count();
        assert!(count < 100, "Expected truncation, but got {} skills", count);
    }

    #[test]
    fn test_extract_triggers() {
        let body = r#"## 触发条件
- "生成周报" "周报" "双周周报"
- "工作汇报"

## 工作流
Step 1: ..."#;

        let triggers = extract_triggers_from_body(body);
        assert_eq!(triggers.len(), 2);
        assert!(triggers[0].contains("生成周报"));
    }
}
