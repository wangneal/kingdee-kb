//! 技能类型定义
//!
//! 参考 opencode 技能系统的数据结构：
//!   Skill { name, description, location, content }

use serde::{Deserialize, Serialize};

/// 技能实例
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// 技能唯一标识（目录名）
    pub name: String,
    /// SKILL.md 文件路径
    pub location: String,
    /// 元数据（YAML frontmatter）
    pub metadata: SkillMetadata,
    /// Markdown 正文
    pub body: String,
    /// 脚本文件列表
    pub scripts: Vec<String>,
    /// 参考文件列表
    pub references: Vec<String>,
}

/// 技能元数据（从 SKILL.md frontmatter 解析）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub category: SkillCategory,
    pub phase: SkillPhase,
    pub icon: Option<String>,
}

/// 技能分类
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillCategory {
    /// 核心技能
    #[serde(rename = "core")]
    Core,
    /// 阶段技能
    #[serde(rename = "stage")]
    Stage,
    /// 管理技能
    #[serde(rename = "mgmt")]
    Management,
    /// 工具技能
    #[serde(rename = "tool")]
    Tool,
    /// 其他
    #[serde(untagged)]
    Other(String),
}

impl Default for SkillCategory {
    fn default() -> Self {
        SkillCategory::Other("unknown".to_string())
    }
}

impl std::fmt::Display for SkillCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillCategory::Core => write!(f, "core"),
            SkillCategory::Stage => write!(f, "stage"),
            SkillCategory::Management => write!(f, "mgmt"),
            SkillCategory::Tool => write!(f, "tool"),
            SkillCategory::Other(s) => write!(f, "{}", s),
        }
    }
}

/// 技能适用的项目阶段
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillPhase {
    /// 全阶段适用
    #[serde(rename = "all")]
    All,
    /// 特定阶段
    #[serde(untagged)]
    Specific(String),
}

impl Default for SkillPhase {
    fn default() -> Self {
        SkillPhase::All
    }
}

/// 技能搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSearchResult {
    pub skill: Skill,
    pub relevance: f64,
}

/// 技能统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStats {
    pub total: usize,
    pub by_category: Vec<(String, usize)>,
}
