//! 技能系统核心 — SKILL.md 加载、解析、搜索
//!
//! 参考 opencode 技能系统设计：
//!   - SKILL.md 文件格式：YAML frontmatter + Markdown 内容
//!   - 目录扫描加载
//!   - 按名称/描述匹配

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::services::skill_types::{Skill, SkillCategory, SkillMetadata, SkillPhase};

/// 技能管理器 — 扫描、加载、缓存技能
pub struct SkillManager {
    /// 按 name 索引的技能
    skills: HashMap<String, Skill>,
    /// 技能目录根路径
    skills_dir: PathBuf,
}

impl SkillManager {
    /// 创建技能管理器并扫描指定目录
    pub fn new(skills_dir: PathBuf) -> Self {
        let mut manager = Self {
            skills: HashMap::new(),
            skills_dir,
        };
        manager.scan();
        manager
    }

    /// 扫描技能目录，加载所有 SKILL.md
    pub fn scan(&mut self) {
        self.skills.clear();

        if !self.skills_dir.exists() {
            println!("Skills directory not found: {:?}", self.skills_dir);
            return;
        }

        let entries = match std::fs::read_dir(&self.skills_dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Failed to read skills dir: {}", e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // 跳过 _shared 等特殊目录
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('_') || name.starts_with('.') {
                    continue;
                }
            }
            if let Some(skill) = Self::load_skill_from_dir(&path) {
                println!(
                    "Loaded skill: {} ({:?})",
                    skill.name, skill.metadata.category
                );
                self.skills.insert(skill.name.clone(), skill);
            }
        }

        println!("Loaded {} skills", self.skills.len());
    }

    /// 从技能目录加载单个技能
    fn load_skill_from_dir(dir: &Path) -> Option<Skill> {
        let skill_md = dir.join("SKILL.md");
        if !skill_md.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&skill_md).ok()?;
        let (metadata, body) = Self::parse_skill_md(&content);

        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let location = skill_md.to_string_lossy().to_string();

        Some(Skill {
            name,
            location,
            metadata,
            body,
            scripts: Self::list_scripts(dir),
            references: Self::list_references(dir),
        })
    }

    /// 解析 SKILL.md 的 YAML frontmatter 和 Markdown 正文
    fn parse_skill_md(content: &str) -> (SkillMetadata, String) {
        let mut metadata = SkillMetadata::default();
        let body;

        if let Some(rest) = content.strip_prefix("---") {
            if let Some((frontmatter, rest_body)) = rest.split_once("---") {
                metadata = Self::parse_yaml_frontmatter(frontmatter);
                body = rest_body.trim().to_string();
            } else {
                body = content.to_string();
            }
        } else {
            body = content.to_string();
        }

        (metadata, body)
    }

    /// 简单的 YAML frontmatter 解析（无需引入 serde_yaml 依赖）
    fn parse_yaml_frontmatter(frontmatter: &str) -> SkillMetadata {
        let mut meta = SkillMetadata::default();
        let mut in_description = false;
        let mut description_lines: Vec<String> = Vec::new();

        for line in frontmatter.lines() {
            let trimmed = line.trim();

            if in_description {
                // 描述是多行的，检测下一个 key: value 模式
                if trimmed.contains(':')
                    && !trimmed.starts_with(' ')
                    && !trimmed.starts_with('\t')
                    && !trimmed.starts_with('-')
                    && !trimmed.starts_with('>')
                    && !trimmed.starts_with('|')
                {
                    // 结束描述收集
                    meta.description = Some(description_lines.join(" ").trim().to_string());
                    description_lines.clear();
                    in_description = false;
                } else {
                    description_lines.push(trimmed.to_string());
                    continue;
                }
            }

            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim().to_lowercase();
                let value = value.trim().trim_matches('"').trim_matches('\'');

                match key.as_str() {
                    "name" => {
                        meta.name = Some(value.to_string());
                    }
                    "description" => {
                        if value.starts_with('|') {
                            // 多行描述开始
                            in_description = true;
                            // 如果 | 后面还有内容
                            let rest = value[1..].trim();
                            if !rest.is_empty() {
                                description_lines.push(rest.to_string());
                                #[test]
                                fn debug_parse_kickoff() {
                                    let content =
                                        std::fs::read_to_string("../skills/kickoff-pack/SKILL.md")
                                            .unwrap();
                                    let (meta, _body) = SkillManager::parse_skill_md(&content);
                                    eprintln!("name: {:?}", meta.name);
                                    eprintln!("desc: {:?}", meta.description);
                                    eprintln!("category: {:?}", meta.category);
                                    eprintln!("version: {:?}", meta.version);
                                    assert!(meta.description.unwrap().contains("启动会PPT"));
                                    assert!(matches!(meta.category, SkillCategory::Stage));
                                }
                            }
                        } else if value == ">" {
                            in_description = true;
                        } else {
                            meta.description = Some(value.to_string());
                        }
                    }
                    "version" => {
                        meta.version = Some(value.to_string());
                    }
                    "category" => {
                        meta.category = match value.to_lowercase().as_str() {
                            "core" => SkillCategory::Core,
                            "stage" => SkillCategory::Stage,
                            "mgmt" | "management" => SkillCategory::Management,
                            "tool" => SkillCategory::Tool,
                            _ => SkillCategory::Other(value.to_string()),
                        };
                    }
                    "phase" => {
                        meta.phase = match value.to_lowercase().as_str() {
                            "all" => SkillPhase::All,
                            _ => SkillPhase::Specific(value.to_string()),
                        };
                    }
                    "icon" => {
                        meta.icon = Some(value.to_string());
                    }
                    _ => {
                        // 忽略未知字段
                    }
                }
            }
        }

        // 处理最后的描述
        if in_description && !description_lines.is_empty() {
            meta.description = Some(description_lines.join(" ").trim().to_string());
        }

        // 如果 name 不存在，从 description 推断
        if meta.name.is_none() && meta.description.is_some() {
            meta.name = Some("unknown".to_string());
        }

        meta
    }

    /// 列出技能目录下的脚本文件
    fn list_scripts(dir: &Path) -> Vec<String> {
        let scripts_dir = dir.join("scripts");
        if !scripts_dir.exists() {
            return Vec::new();
        }
        std::fs::read_dir(&scripts_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                if e.path().is_file() {
                    e.file_name().to_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    /// 列出技能目录下的参考文件
    fn list_references(dir: &Path) -> Vec<String> {
        let refs_dir = dir.join("references");
        if !refs_dir.exists() {
            return Vec::new();
        }
        std::fs::read_dir(&refs_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                if e.path().is_file() {
                    e.file_name().to_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    // ─── 查询接口 ──────────────────────────────────────────

    /// 获取所有技能
    pub fn list_all(&self) -> Vec<Skill> {
        let mut skills: Vec<_> = self.skills.values().cloned().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills
    }

    /// 按名称获取技能
    pub fn get(&self, name: &str) -> Option<Skill> {
        self.skills.get(name).cloned()
    }

    /// 按分类筛选
    pub fn list_by_category(&self, category: SkillCategory) -> Vec<Skill> {
        self.skills
            .values()
            .filter(|s| s.metadata.category == category)
            .cloned()
            .collect()
    }

    /// 按阶段筛选
    pub fn list_by_phase(&self, phase: &SkillPhase) -> Vec<Skill> {
        self.skills
            .values()
            .filter(|s| &s.metadata.phase == phase || s.metadata.phase == SkillPhase::All)
            .cloned()
            .collect()
    }

    /// 搜索技能（匹配 name 和 description）
    pub fn search(&self, query: &str) -> Vec<Skill> {
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower.split_whitespace().collect();

        self.skills
            .values()
            .filter(|s| {
                let name_lower = s.name.to_lowercase();
                let desc_lower = s
                    .metadata
                    .description
                    .as_ref()
                    .map(|d| d.to_lowercase())
                    .unwrap_or_default();

                // 精确匹配名称
                name_lower.contains(&query_lower)
                // 关键词匹配描述
                || keywords.iter().any(|kw| desc_lower.contains(*kw))
                // 名称包含关键词
                || keywords.iter().any(|kw| name_lower.contains(*kw))
            })
            .cloned()
            .collect()
    }

    /// 根据用户输入匹配最相关的技能
    pub fn match_best(&self, user_input: &str) -> Option<Skill> {
        let input_lower = user_input.to_lowercase();
        let mut best_score = 0u32;
        let mut best_match: Option<&Skill> = None;
        let mut tied = false;

        let mut skills: Vec<_> = self.skills.values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));

        for skill in skills {
            let mut score = 0u32;
            let skill_name = skill.name.to_lowercase();

            if input_lower.contains(&skill_name) {
                score += 100;
            }

            if let Some(frontmatter_name) = skill.metadata.name.as_ref() {
                let frontmatter_name = frontmatter_name.to_lowercase();
                if frontmatter_name != skill_name && input_lower.contains(&frontmatter_name) {
                    score += 80;
                }
            }

            for alias in skill_aliases(&skill.name) {
                if input_lower.contains(alias) {
                    score += 70;
                }
            }

            for alias in skill_intent_aliases(&skill.name) {
                if input_lower.contains(alias) {
                    score += 85 + alias.chars().count() as u32;
                }
            }

            for part in skill
                .name
                .split('-')
                .filter(|part| part.chars().count() >= 4)
            {
                if input_lower.contains(part) {
                    score += 20;
                }
            }

            if let Some(ref desc) = skill.metadata.description {
                let desc_lower = desc.to_lowercase();
                let has_space = input_lower.contains(' ');

                if has_space {
                    // 英文/混合查询：按空格分词匹配
                    for word in input_lower.split_whitespace() {
                        if word.chars().count() >= 2
                            && !is_generic_skill_word(word)
                            && desc_lower.contains(word)
                        {
                            score += 25;
                        }
                    }
                } else {
                    // 中文查询：检查描述是否包含查询或查询子串
                    if desc_lower.contains(&input_lower) {
                        score += 50;
                    }
                    // 检查 2-4 字子串（滑动窗口）
                    for len in (2..=4).rev() {
                        let chars: Vec<char> = input_lower.chars().collect();
                        if chars.len() >= len {
                            for w in chars.windows(len) {
                                let sub: String = w.iter().collect();
                                if !is_generic_skill_word(&sub) && desc_lower.contains(&sub) {
                                    score += 15;
                                }
                            }
                        }
                    }
                }
                // 查询关键词在描述中连续出现 → 额外加分
                let query_no_space = input_lower.replace(' ', "");
                if query_no_space.chars().count() >= 4 && desc_lower.contains(&query_no_space) {
                    score += 30;
                }
            }

            if score > best_score {
                best_score = score;
                best_match = Some(skill);
                tied = false;
            } else if score == best_score && score > 0 {
                tied = true;
            }
        }

        if best_score >= 20 && !tied {
            best_match.cloned()
        } else {
            None
        }
    }

    /// 获取技能数量
    pub fn count(&self) -> usize {
        self.skills.len()
    }

    /// 获取按分类分组的统计
    pub fn stats(&self) -> HashMap<String, usize> {
        let mut map = HashMap::new();
        for skill in self.skills.values() {
            let cat = skill.metadata.category.to_string();
            *map.entry(cat).or_insert(0) += 1;
        }
        map
    }

    /// 公开的 SKILL.md 解析方法（供 command 层调用）
    pub fn parse_skill_md_public(content: &str) -> (SkillMetadata, String) {
        Self::parse_skill_md(content)
    }

    /// 导入技能：将 SKILL.md 内容写入 skills/<name>/SKILL.md 并重新扫描
    pub fn import_skill(&mut self, name: &str, content: &str) -> Result<String, String> {
        if !is_valid_skill_name(name) || name == "unknown" {
            return Err("无效的技能名".to_string());
        }
        let root = self
            .skills_dir
            .canonicalize()
            .unwrap_or_else(|_| self.skills_dir.clone());
        let dir = self.skills_dir.join(name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {}", e))?;

        let resolved_dir = dir
            .canonicalize()
            .map_err(|e| format!("无法验证目录: {}", e))?;
        if !resolved_dir.starts_with(&root) {
            return Err("技能名不能超出 skills 目录".to_string());
        }

        let skill_md = dir.join("SKILL.md");
        std::fs::write(&skill_md, content).map_err(|e| format!("写入文件失败: {}", e))?;

        // 重新扫描
        self.scan();

        Ok(name.to_string())
    }
}

fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_generic_skill_word(word: &str) -> bool {
    matches!(
        word,
        "skill"
            | "skills"
            | "project"
            | "projects"
            | "generate"
            | "report"
            | "reports"
            | "start"
            | "when"
            | "wants"
            | "user"
            | "kingdee"
            | "implementation"
            | "suite"
    )
}

fn skill_intent_aliases(name: &str) -> &'static [&'static str] {
    match name {
        "acceptance-pack" => &["验收", "验收报告", "交付物", "项目总结", "盘点交付"],
        "blueprint-tools" => &["蓝图", "蓝图设计", "业务流程图", "需求规格"],
        "build-tracker" => &["构建进度", "跟踪构建", "系统配置清单", "配置清单"],
        "change-manager" => &["变更", "变更申请", "需求变更"],
        "claude-req-analysis" => &["需求分析", "客户需求", "梳理客户需求"],
        "data-auditor" => &["数据质量", "数据质量检查", "数据审计"],
        "data-cleaner" => &["数据清洗", "清洗导入数据", "去重", "格式转换"],
        "doc-sanitizer" => &["数据脱敏", "文档敏感信息", "敏感信息清理", "文档脱敏"],
        "doc-tools" => &["word模板", "docx", "填充word", "编辑docx", "文档工具"],
        "drafter-diagram" => &["架构图", "拓扑图"],
        "golive-pack" => &["上线", "上线检查", "准备上线", "切换方案", "初始化清单"],
        "humanizer" => &["去ai味", "去味", "人性化改写", "ai痕迹"],
        "kdclub-ai-product-qa" => &["星空", "苍穹", "多组织核算", "产品问答", "接口"],
        "kickoff-pack" => &["启动会", "启动会ppt", "任命书", "启动材料"],
        "kingdee-ppt" => &["ppt", "演示文稿"],
        "openai-whisper" => &[
            "语音转文字",
            "录音转文本",
            "录音转纪要",
            "会议录音转录",
            "转录",
        ],
        "project-dashboard" => &["项目看板", "项目进度可视化", "看板"],
        "project-init" => &["项目启动", "新建项目", "新建一个项目", "初始化项目"],
        "project-sync" => &["团队文件同步", "拉取最新文件", "项目同步"],
        "qa-root-cause-analysis" => &["根因", "根因分析", "5-why", "5why", "质量复盘", "鱼骨图"],
        "risk-manager" => &["风险", "项目风险", "风险评估"],
        "skill-updater" => &["检查更新", "更新套件", "技能更新"],
        "stakeholder-comms" => &["会议纪要", "会议记录", "录音转纪要"],
        "survey-assistant" => &["调研", "访谈纪要", "需求矩阵", "调研报告", "调研计划"],
        "test-manager" => &["测试用例", "缺陷", "流程测试"],
        "ux-flow-designer" => &[
            "审批流程图",
            "状态图",
            "mermaid时序图",
            "时序图",
            "用户流程",
        ],
        "weekly-report" => &["周报", "工作汇报", "这周工作汇报"],
        _ => &[],
    }
}

fn skill_aliases(name: &str) -> &'static [&'static str] {
    match name {
        "acceptance-pack" => &["验收", "验收报告", "交付验收"],
        "blueprint-tools" => &["蓝图", "业务蓝图", "蓝图设计"],
        "build-tracker" => &["构建跟踪", "配置清单", "实施构建"],
        "change-manager" => &["变更", "变更管理", "变更申请"],
        "claude-req-analysis" => &["需求分析", "需求解析"],
        "data-auditor" => &["数据审计", "数据质量"],
        "data-cleaner" => &["数据清洗", "清洗数据", "去重"],
        "doc-sanitizer" => &["文档脱敏", "脱敏文档"],
        "doc-tools" => &["文档工具", "文档处理"],
        "drafter-diagram" => &["流程图", "架构图", "图表"],
        "golive-pack" => &["上线", "上线切换", "上线方案"],
        "humanizer" => &["去ai味", "去味", "润色", "ai痕迹"],
        "kdclub-ai-product-qa" => &["金蝶社区", "产品问答", "社区问答"],
        "kickoff-pack" => &["启动会", "项目启动", "启动材料"],
        "kingdee-ppt" => &["ppt", "演示文稿", "汇报材料"],
        "openai-whisper" => &["语音转写", "录音转写", "whisper"],
        "project-dashboard" => &["项目看板", "看板"],
        "project-init" => &["项目初始化", "初始化项目"],
        "project-sync" => &["项目同步", "同步项目"],
        "qa-root-cause-analysis" => &["根因分析", "5why", "鱼骨图"],
        "risk-manager" => &["风险", "风险管理", "风险清单"],
        "skill-updater" => &["技能更新", "skill更新"],
        "stakeholder-comms" => &["干系人", "会议纪要", "沟通"],
        "survey-assistant" => &["调研", "调研助手", "访谈"],
        "test-manager" => &["测试", "测试用例", "缺陷"],
        "ux-flow-designer" => &["交互流程", "用户流程", "ux"],
        "weekly-report" => &["周报", "月报", "项目周报"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml_frontmatter_simple() {
        let input = r#"name: test-skill
description: "Use when testing"
version: 1.0"#;

        let meta = SkillManager::parse_yaml_frontmatter(input);
        assert_eq!(meta.name, Some("test-skill".to_string()));
        assert_eq!(meta.description, Some("Use when testing".to_string()));
        assert_eq!(meta.version, Some("1.0".to_string()));
    }

    #[test]
    fn test_parse_yaml_multiline_description() {
        let input = r#"name: test-skill
description: |
  This skill should be used when
  the user wants to test things"#;

        let meta = SkillManager::parse_yaml_frontmatter(input);
        assert_eq!(meta.name, Some("test-skill".to_string()));
        assert!(meta
            .description
            .unwrap()
            .contains("the user wants to test things"));
    }

    #[test]
    fn test_parse_full_skill_md() {
        let content = r#"---
name: brainstorming
description: "在任何创造性工作之前..."
---

# 头脑风暴

通过自然的对话，帮助用户梳理需求。"#;

        let (meta, body) = SkillManager::parse_skill_md(content);
        assert_eq!(meta.name, Some("brainstorming".to_string()));
        assert!(meta.description.unwrap().contains("创造性"));
        assert!(body.contains("头脑风暴"));
        assert!(!body.contains("---"));
    }

    // ─── 集成测试：真实 skills/ 目录加载 ───

    #[test]
    fn integration_load_real_skills() {
        let skills_dir = std::path::PathBuf::from("../skills");
        if !skills_dir.exists() {
            eprintln!("跳过: skills/ 目录不存在");
            return;
        }
        let mgr = SkillManager::new(skills_dir);
        let count = mgr.count();
        eprintln!("\n========== 技能加载报告 ==========");
        eprintln!("总计: {} 个技能", count);
        for (cat, n) in mgr.stats() {
            eprintln!("  {:?}: {}", cat, n);
        }
        assert!(count >= 20, "至少加载 20 个技能，实际: {}", count);
    }

    #[test]
    fn integration_match_user_queries() {
        let skills_dir = std::path::PathBuf::from("../skills");
        if !skills_dir.exists() {
            eprintln!("test_integration_match: skills dir not found, skipping");
            return;
        }
        let mgr = SkillManager::new(skills_dir);

        let test_cases = [
            // 启动阶段
            ("帮我准备启动会材料", "kickoff"),
            ("准备启动会PPT", "kickoff"),
            ("生成任命书", "kickoff"),
            ("项目启动", "project-init"),
            ("新建一个项目", "project-init"),
            ("初始化项目", "project-init"),
            // 周报
            ("生成周报", "weekly"),
            ("写周报", "weekly"),
            ("这周工作汇报", "weekly"),
            // 需求调研
            ("制定调研计划", "survey"),
            ("整理访谈纪要", "survey"),
            ("维护需求矩阵", "survey"),
            ("写调研报告", "survey"),
            // 蓝图设计
            ("画业务流程图", "blueprint"),
            ("做蓝图设计", "blueprint"),
            ("写需求规格", "blueprint"),
            // 构建
            ("跟踪构建进度", "build"),
            ("清洗导入数据", "data-cleaner"),
            ("生成系统配置清单", "build"),
            // 测试
            ("生成测试用例", "test-manager"),
            ("记录缺陷", "test-manager"),
            ("登记一个缺陷", "test-manager"),
            ("流程测试", "test-manager"),
            // 上线
            ("制定切换方案", "golive"),
            ("上线检查", "golive"),
            ("准备上线", "golive"),
            ("生成初始化清单", "golive"),
            // 验收
            ("写验收报告", "acceptance"),
            ("盘点交付物", "acceptance"),
            ("项目总结", "acceptance"),
            // 会议
            ("整理会议纪要", "stakeholder"),
            ("录音转纪要", "stakeholder"),
            ("会议记录", "stakeholder"),
            // 变更
            ("提变更", "change"),
            ("新增变更申请", "change"),
            ("需求变更", "change"),
            // 风险
            ("记录一个风险", "risk"),
            ("识别项目风险", "risk"),
            ("风险评估", "risk"),
            // 质量
            ("分析这个问题的根因", "qa-root"),
            ("5-Why分析", "qa-root"),
            ("质量复盘", "qa-root"),
            ("画鱼骨图", "qa-root"),
            // 工具
            ("这段文字去AI味", "humanizer"),
            ("人性化改写", "humanizer"),
            ("星空里怎么做多组织核算", "kdclub"),
            ("苍穹支持什么接口", "kdclub"),
            ("做PPT", "kingdee-ppt"),
            ("生成一个演示文稿", "kingdee-ppt"),
            ("填充Word模板", "doc-tools"),
            ("编辑docx文件", "doc-tools"),
            ("数据脱敏", "doc-sanitizer"),
            ("文档敏感信息清理", "doc-sanitizer"),
            ("语音转文字", "openai-whisper"),
            ("录音转文本", "openai-whisper"),
            ("会议录音转录", "openai-whisper"),
            ("画个审批流程图", "ux-flow"),
            ("画状态图", "ux-flow"),
            ("画Mermaid时序图", "ux-flow"),
            ("需求分析", "claude-req"),
            ("梳理客户需求", "claude-req"),
            ("画架构图", "drafter"),
            ("画个拓扑图", "drafter"),
            ("数据清洗", "data-cleaner"),
            ("去重", "data-cleaner"),
            ("格式转换", "data-cleaner"),
            ("数据质量检查", "data-auditor"),
            ("数据审计", "data-auditor"),
            ("查看项目看板", "project-dashboard"),
            ("项目进度可视化", "project-dashboard"),
            ("团队文件同步", "project-sync"),
            ("拉取最新文件", "project-sync"),
            ("检查更新", "skill-updater"),
            ("更新套件", "skill-updater"),
        ];

        eprintln!("\n========== skill matching test ==========");
        let mut matched = 0;
        let mut partial = 0;
        let mut missed = 0;
        for (input, expected) in &test_cases {
            match mgr.match_best(input) {
                Some(s) => {
                    let hit = s.name.contains(expected);
                    if hit {
                        eprintln!("  [OK]  '{}' -> {}", input, s.name);
                        matched += 1;
                    } else {
                        eprintln!(
                            "  [~]  '{}' -> {} (expected: *{}*)",
                            input, s.name, expected
                        );
                        partial += 1;
                    }
                }
                None => {
                    eprintln!("  [X]  '{}' -> no match (expected: *{}*)", input, expected);
                    missed += 1;
                }
            }
        }
        let total = test_cases.len();
        let rate = matched as f64 / total as f64 * 100.0;
        eprintln!(
            "\n result: ok={} partial={} miss={} total={} rate={:.0}%",
            matched, partial, missed, total, rate
        );
        assert!(rate >= 75.0, "match rate {:.0}% < 75%", rate);
    }

    #[test]
    fn integration_list_by_category() {
        let skills_dir = std::path::PathBuf::from("../skills");
        if !skills_dir.exists() {
            eprintln!("跳过: skills/ 目录不存在");
            return;
        }
        let mgr = SkillManager::new(skills_dir);

        eprintln!("\n========== 按分类列出 ==========");
        for cat in ["core", "stage", "mgmt", "tool"] {
            let skills = mgr.list_by_category(match cat {
                "core" => crate::services::skill_types::SkillCategory::Core,
                "stage" => crate::services::skill_types::SkillCategory::Stage,
                "mgmt" => crate::services::skill_types::SkillCategory::Management,
                "tool" => crate::services::skill_types::SkillCategory::Tool,
                _ => continue,
            });
            eprintln!("  {}:", cat);
            for s in &skills {
                eprintln!(
                    "    - {} | {}",
                    s.name,
                    s.metadata.description.as_deref().unwrap_or("无描述")
                );
            }
        }
    }
}
