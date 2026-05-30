//! 技能触发引擎 — 关键词匹配 + 语义相似度
//!
//! 参考 Claude Code 技能系统的 when_to_use 机制：
//!   - 从 SKILL.md 的 description 和 body 提取触发关键词
//!   - 构建倒排索引实现快速匹配
//!   - 支持中英文混合查询

use std::collections::HashMap;

use crate::services::skill_types::{Skill, extract_triggers_from_body};

/// 技能匹配结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillMatch {
    pub skill_id: String,
    pub score: f64,
    pub match_type: MatchType,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MatchType {
    Keyword,
    Semantic,
    Path,
}

/// 触发上下文
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TriggerContext {
    pub user_input: String,
    pub accessed_files: Vec<String>,
    pub current_phase: Option<String>,
    pub session_id: String,
}

/// 技能触发引擎
pub struct SkillTriggerEngine {
    /// when_to_use 文本 → 技能 ID
    when_to_use_map: HashMap<String, String>,
    /// 关键词 → 技能 ID 列表
    keyword_map: HashMap<String, Vec<String>>,
    /// 技能别名 → 技能 ID
    alias_map: HashMap<String, String>,
    /// 技能 ID → 描述文本（用于相似度计算）
    description_map: HashMap<String, String>,
    /// 路径模式 → 技能 ID 列表
    path_map: HashMap<String, Vec<String>>,
}

impl SkillTriggerEngine {
    /// 从技能列表构建触发引擎
    pub fn new(skills: &[Skill]) -> Self {
        let mut when_to_use_map = HashMap::new();
        let mut keyword_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut alias_map = HashMap::new();
        let mut description_map = HashMap::new();
        let mut path_map: HashMap<String, Vec<String>> = HashMap::new();

        for skill in skills {
            let skill_id = skill.name.clone();

            // 从 description 提取触发文本
            if let Some(ref desc) = skill.metadata.description {
                when_to_use_map.insert(desc.clone(), skill_id.clone());
                description_map.insert(skill_id.clone(), desc.clone());

                // 提取关键词建立索引
                let keywords = Self::extract_keywords(desc);
                for keyword in keywords {
                    keyword_map
                        .entry(keyword)
                        .or_default()
                        .push(skill_id.clone());
                }
            }

            // 从 SKILL.md body 提取触发条件
            let trigger_keywords = extract_triggers_from_body(&skill.body);
            for keyword in trigger_keywords {
                keyword_map
                    .entry(keyword)
                    .or_default()
                    .push(skill_id.clone());
            }

            // 建立别名索引
            for alias in Self::get_skill_aliases(&skill.name) {
                alias_map.insert(alias.to_string(), skill_id.clone());
            }

            // 建立路径索引（从 frontmatter 的 paths 字段）
            for path_pattern in &skill.metadata.paths {
                path_map
                    .entry(path_pattern.to_lowercase())
                    .or_default()
                    .push(skill_id.clone());
            }
        }

        Self {
            when_to_use_map,
            keyword_map,
            alias_map,
            description_map,
            path_map,
        }
    }

    /// 根据用户输入匹配技能
    pub fn match_by_input(&self, input: &str) -> Vec<SkillMatch> {
        let input_lower = input.to_lowercase();
        let mut scores: HashMap<String, f64> = HashMap::new();

        // 1. 精确别名匹配（最高权重）
        for (alias, skill_id) in &self.alias_map {
            if input_lower.contains(&alias.to_lowercase()) {
                *scores.entry(skill_id.clone()).or_insert(0.0) += 10.0;
            }
        }

        // 2. 关键词匹配
        let input_keywords = Self::extract_keywords(&input_lower);
        for keyword in &input_keywords {
            if let Some(skill_ids) = self.keyword_map.get(keyword) {
                for skill_id in skill_ids {
                    *scores.entry(skill_id.clone()).or_insert(0.0) += 1.0;
                }
            }
        }

        // 3. 语义相似度（基于 when_to_use 文本）
        for (trigger_text, skill_id) in &self.when_to_use_map {
            let similarity = Self::compute_similarity(&input_lower, &trigger_text.to_lowercase());
            if similarity > 0.3 {
                *scores.entry(skill_id.clone()).or_insert(0.0) += similarity * 3.0;
            }
        }

        // 4. 描述文本包含匹配
        for (skill_id, desc) in &self.description_map {
            let desc_lower = desc.to_lowercase();
            if desc_lower.contains(&input_lower) || input_lower.contains(&desc_lower) {
                *scores.entry(skill_id.clone()).or_insert(0.0) += 5.0;
            }
        }

        // 排序并返回
        let mut matches: Vec<SkillMatch> = scores
            .into_iter()
            .map(|(id, score)| SkillMatch {
                skill_id: id,
                score,
                match_type: if score > 5.0 {
                    MatchType::Semantic
                } else {
                    MatchType::Keyword
                },
            })
            .collect();
        matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        matches
    }

    /// 根据文件路径匹配技能（条件激活）
    ///
    /// 使用从 SKILL.md frontmatter 的 paths 字段构建的索引。
    pub fn match_by_paths(&self, accessed_files: &[String]) -> Vec<SkillMatch> {
        let mut matches = Vec::new();

        for file_path in accessed_files {
            let path_lower = file_path.to_lowercase();

            // 遍历路径模式索引
            for (pattern, skill_ids) in &self.path_map {
                if path_lower.contains(pattern) {
                    for skill_id in skill_ids {
                        matches.push(SkillMatch {
                            skill_id: skill_id.clone(),
                            score: 1.0,
                            match_type: MatchType::Path,
                        });
                    }
                }
            }
        }

        matches
    }

    /// 提取中英文关键词
    fn extract_keywords(text: &str) -> Vec<String> {
        let mut keywords = Vec::new();

        // 英文单词
        let en_regex = regex::Regex::new(r"[a-zA-Z]+").unwrap();
        for mat in en_regex.find_iter(text) {
            let word = mat.as_str().to_lowercase();
            if word.len() >= 2 {
                keywords.push(word);
            }
        }

        // 中文字符（单字 + 2-gram）
        let chars: Vec<char> = text.chars().filter(|c| !c.is_ascii()).collect();

        // 中文 2-gram
        for window in chars.windows(2) {
            let bigram: String = window.iter().collect();
            keywords.push(bigram);
        }

        // 中文 3-gram（更精确的匹配）
        for window in chars.windows(3) {
            let trigram: String = window.iter().collect();
            keywords.push(trigram);
        }

        keywords
    }

    /// 计算两个文本的相似度（基于词汇重叠）
    fn compute_similarity(text1: &str, text2: &str) -> f64 {
        let keywords1: std::collections::HashSet<String> =
            Self::extract_keywords(text1).into_iter().collect();
        let keywords2: std::collections::HashSet<String> =
            Self::extract_keywords(text2).into_iter().collect();

        if keywords1.is_empty() || keywords2.is_empty() {
            return 0.0;
        }

        let intersection = keywords1.intersection(&keywords2).count();
        let union = keywords1.union(&keywords2).count();

        if union == 0 {
            0.0
        } else {
            intersection as f64 / union as f64
        }
    }

    /// 获取技能的别名列表
    fn get_skill_aliases(name: &str) -> Vec<&str> {
        match name {
            "acceptance-pack" => vec!["验收", "验收报告", "交付验收", "交付物盘点"],
            "blueprint-tools" => vec!["蓝图", "业务蓝图", "蓝图设计", "流程分析", "需求规格"],
            "build-tracker" => vec!["构建跟踪", "配置清单", "实施构建", "数据清洗导入"],
            "change-manager" => vec!["变更", "变更管理", "变更申请", "需求变更"],
            "claude-req-analysis" => vec!["需求分析", "需求解析", "客户需求"],
            "data-auditor" => vec!["数据审计", "数据质量", "数据检查"],
            "data-cleaner" => vec!["数据清洗", "清洗数据", "去重", "格式转换"],
            "doc-sanitizer" => vec!["文档脱敏", "脱敏文档", "敏感信息"],
            "doc-tools" => vec!["文档工具", "文档处理", "填充模板", "OOXML"],
            "drafter-diagram" => vec!["流程图", "架构图", "图表", "拓扑图"],
            "golive-pack" => vec!["上线", "上线切换", "上线方案", "切换方案", "上线检查"],
            "humanizer" => vec!["去ai味", "去味", "润色", "ai痕迹", "人性化"],
            "kdclub-ai-product-qa" => vec!["金蝶社区", "产品问答", "社区问答", "星空", "苍穹"],
            "kickoff-pack" => vec!["启动会", "项目启动", "启动材料", "任命书"],
            "kingdee-ppt" => vec!["ppt", "演示文稿", "汇报材料", "幻灯片"],
            "openai-whisper" => vec!["语音转写", "录音转写", "whisper", "语音转文字"],
            "project-dashboard" => vec!["项目看板", "看板", "项目概览", "可视化"],
            "project-init" => vec!["项目初始化", "初始化项目", "新建项目"],
            "project-sync" => vec!["项目同步", "同步项目", "团队协同"],
            "qa-root-cause-analysis" => vec!["根因分析", "5why", "鱼骨图", "8D报告", "Pareto"],
            "risk-manager" => vec!["风险", "风险管理", "风险清单", "风险评估"],
            "skill-updater" => vec!["技能更新", "skill更新", "检查更新", "更新套件"],
            "stakeholder-comms" => vec!["干系人", "会议纪要", "沟通", "会议记录"],
            "survey-assistant" => vec!["调研", "调研助手", "访谈", "需求矩阵"],
            "test-manager" => vec!["测试", "测试用例", "缺陷", "流程测试"],
            "ux-flow-designer" => vec!["交互流程", "用户流程", "ux", "Mermaid"],
            "weekly-report" => vec!["周报", "月报", "项目周报", "工作汇报"],
            _ => vec![],
        }
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
            body: String::new(),
            scripts: Vec::new(),
            references: Vec::new(),
        }
    }

    #[test]
    fn test_keyword_matching() {
        let skills = vec![
            create_test_skill("weekly-report", "生成周报 双周周报 工作汇报"),
            create_test_skill("kickoff-pack", "启动会 启动会PPT 任命书"),
        ];

        let engine = SkillTriggerEngine::new(&skills);
        let matches = engine.match_by_input("生成周报");

        assert!(!matches.is_empty());
        assert_eq!(matches[0].skill_id, "weekly-report");
    }

    #[test]
    fn test_alias_matching() {
        let skills = vec![create_test_skill(
            "humanizer",
            "AI文案去味 24种模式检测",
        )];

        let engine = SkillTriggerEngine::new(&skills);
        let matches = engine.match_by_input("这段文字去AI味");

        assert!(!matches.is_empty());
        assert_eq!(matches[0].skill_id, "humanizer");
    }

    #[test]
    fn test_path_matching() {
        let skills = vec![Skill {
            name: "kickoff-pack".to_string(),
            location: "/skills/kickoff-pack/SKILL.md".to_string(),
            metadata: SkillMetadata {
                name: Some("kickoff-pack".to_string()),
                description: Some("启动阶段文档包".to_string()),
                version: Some("1.0".to_string()),
                category: SkillCategory::Stage,
                phase: SkillPhase::Specific("启动".to_string()),
                icon: None,
                paths: vec!["01_启动".to_string(), "kickoff".to_string()],
            },
            body: String::new(),
            scripts: Vec::new(),
            references: Vec::new(),
        }];
        let engine = SkillTriggerEngine::new(&skills);

        let files = vec!["01_启动阶段/启动会PPT.pptx".to_string()];
        let matches = engine.match_by_paths(&files);

        assert!(!matches.is_empty());
        assert_eq!(matches[0].skill_id, "kickoff-pack");
    }
}
