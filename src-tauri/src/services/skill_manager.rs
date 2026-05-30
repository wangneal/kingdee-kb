//! 技能系统核心 — SKILL.md 加载、解析、搜索
//!
//! 参考 opencode 技能系统设计：
//!   - SKILL.md 文件格式：YAML frontmatter + Markdown 内容
//!   - 目录扫描加载
//!   - 按名称/描述匹配

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::services::skill_loader::SkillLoader;
use crate::services::skill_trigger::{SkillTriggerEngine, SkillMatch, TriggerContext};
use crate::services::prompt_assembler::PromptAssembler;
use crate::services::skill_types::{
    Skill, SkillCategory, SkillFile, SkillFull, SkillMetadata, SkillPhase, SharedResource,
    parse_skill_md,
};

/// 技能管理器 — 扫描、加载、缓存、触发匹配
pub struct SkillManager {
    /// 按 name 索引的技能
    skills: HashMap<String, Skill>,
    /// 技能目录根路径
    skills_dir: PathBuf,
    /// 触发引擎（懒加载）
    trigger_engine: Option<SkillTriggerEngine>,
    /// 提示组装器
    prompt_assembler: PromptAssembler,
}

impl SkillManager {
    /// 创建技能管理器并扫描指定目录
    pub fn new(skills_dir: PathBuf) -> Self {
        let mut manager = Self {
            skills: HashMap::new(),
            skills_dir,
            trigger_engine: None,
            prompt_assembler: PromptAssembler::new(128000), // 默认 128K 上下文窗口
        };
        manager.scan();
        manager.init_trigger_engine();
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

        // 重建触发引擎索引
        self.init_trigger_engine();
    }

    /// 从技能目录加载单个技能
    fn load_skill_from_dir(dir: &Path) -> Option<Skill> {
        let skill_md = dir.join("SKILL.md");
        if !skill_md.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&skill_md).ok()?;
        let (metadata, body) = parse_skill_md(&content);

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
    ///
    /// 委托给触发引擎进行统一的匹配逻辑。
    pub fn match_best(&self, user_input: &str) -> Option<Skill> {
        let engine = self.trigger_engine.as_ref()?;

        let matches = engine.match_by_input(user_input);
        let best = matches.first()?;

        // 返回分数足够高的匹配
        if best.score >= 2.0 {
            self.skills.get(&best.skill_id).cloned()
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

    /// 获取技能目录路径
    pub fn get_skill_dir(&self, skill_id: &str) -> PathBuf {
        self.skills_dir.join(skill_id)
    }

    /// 获取技能根目录路径
    pub fn get_skills_dir(&self) -> PathBuf {
        self.skills_dir.clone()
    }

    /// 公开的 SKILL.md 解析方法（供 command 层调用）
    pub fn parse_skill_md_public(content: &str) -> (SkillMetadata, String) {
        parse_skill_md(content)
    }

    // ─── 触发引擎接口 ──────────────────────────────────────

    /// 初始化触发引擎
    fn init_trigger_engine(&mut self) {
        let skills: Vec<Skill> = self.skills.values().cloned().collect();
        self.trigger_engine = Some(SkillTriggerEngine::new(&skills));
    }

    /// 根据用户输入匹配最佳技能（使用触发引擎）
    pub fn match_best_skill(&self, context: &TriggerContext) -> Option<SkillMatch> {
        let engine = self.trigger_engine.as_ref()?;

        let mut all_matches = engine.match_by_input(&context.user_input);

        // 合并路径匹配
        let path_matches = engine.match_by_paths(&context.accessed_files);
        all_matches.extend(path_matches);

        // 按分数排序
        all_matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        all_matches.into_iter().next()
    }

    /// 匹配多个候选技能
    pub fn match_candidates(&self, user_input: &str, limit: usize) -> Vec<SkillMatch> {
        let engine = match &self.trigger_engine {
            Some(e) => e,
            None => return Vec::new(),
        };

        let mut matches = engine.match_by_input(user_input);
        matches.truncate(limit);
        matches
    }

    /// 生成技能列表系统提示
    pub fn build_skill_list_prompt(&self) -> String {
        let skills: Vec<Skill> = self.skills.values().cloned().collect();
        self.prompt_assembler.build_skill_list_prompt(&skills)
    }

    /// 获取技能摘要列表（用于前端展示和提示注入）
    pub fn get_skill_prompt_entries(
        &self,
    ) -> Vec<crate::services::prompt_assembler::SkillPromptEntry> {
        let skills: Vec<Skill> = self.skills.values().cloned().collect();
        self.prompt_assembler.build_skill_summaries(&skills)
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

    /// 获取技能完整信息（含支撑文件和共享资源）
    pub fn get_skill_full(&self, name: &str) -> Option<SkillFull> {
        let skill_dir = self.skills_dir.join(name);
        if !skill_dir.exists() {
            return None;
        }
        let shared = self.load_shared();
        SkillLoader::load_skill_full(&skill_dir, &shared)
    }

    /// 获取所有共享资源
    pub fn get_shared_resources(&self) -> Vec<SharedResource> {
        self.load_shared()
    }

    /// 读取技能目录下的指定文件（安全路径验证）
    pub fn read_skill_file(&self, skill_name: &str, relative_path: &str) -> Result<String, String> {
        let skill_dir = self.skills_dir.join(skill_name);
        if !skill_dir.exists() {
            return Err(format!("技能 '{}' 不存在", skill_name));
        }
        SkillLoader::read_skill_file(&skill_dir, relative_path)
    }

    /// 获取技能的支撑文件列表
    pub fn get_skill_files(&self, name: &str) -> Vec<SkillFile> {
        let skill_dir = self.skills_dir.join(name);
        if !skill_dir.exists() {
            return Vec::new();
        }
        SkillLoader::load_supporting_files(&skill_dir)
    }

    /// 内部：加载共享资源（懒加载，不缓存）
    fn load_shared(&self) -> Vec<SharedResource> {
        SkillLoader::load_shared_resources(&self.skills_dir)
    }
}

fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::skill_types::{parse_skill_md, parse_yaml_frontmatter};

    #[test]
    fn test_parse_yaml_frontmatter_simple() {
        let input = r#"name: test-skill
description: "Use when testing"
version: 1.0"#;

        let meta = parse_yaml_frontmatter(input);
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

        let meta = parse_yaml_frontmatter(input);
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

        let (meta, body) = parse_skill_md(content);
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
