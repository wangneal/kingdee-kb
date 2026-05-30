//! 技能类型定义
//!
//! 参考 Claude Code 技能系统的数据结构，兼容 SKILL.md 格式：
//!   Skill { name, description, location, content }
//!   SkillFile { path, name, file_type, size }
//!   SkillFull { skill, supporting_files, shared_references }

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

/// 技能完整信息（含支撑文件）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFull {
    /// 基础技能信息
    pub skill: Skill,
    /// 支撑文件列表（scripts/, references/, assets/）
    pub supporting_files: Vec<SkillFile>,
    /// 关联的共享资源
    pub shared_references: Vec<SharedResource>,
}

/// 技能支撑文件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFile {
    /// 相对于技能目录的路径
    pub path: String,
    /// 文件名
    pub name: String,
    /// 文件类型
    pub file_type: SkillFileType,
    /// 文件大小（bytes）
    pub size: u64,
    /// 最后修改时间（Unix 时间戳 ms）
    pub last_modified: u64,
}

/// 支撑文件类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillFileType {
    /// 参考文档（references/*.md）
    Reference,
    /// 脚本文件（scripts/*.py, *.sh, *.js）
    Script,
    /// 资源文件（assets/*）
    Asset,
    /// 配置文件（*.json, *.yaml）
    Config,
    /// 其他文件
    Other,
}

/// _shared 共享资源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedResource {
    /// 资源名称
    pub name: String,
    /// 相对于 _shared/ 目录的路径
    pub path: String,
    /// 资源内容（文本文件）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
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
    /// 路径匹配模式（用于条件激活）
    /// 例如：["01_启动", "kickoff"] 匹配包含这些字符串的文件路径
    #[serde(default)]
    pub paths: Vec<String>,
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SkillPhase {
    /// 全阶段适用
    #[serde(rename = "all")]
    #[default]
    All,
    /// 特定阶段
    #[serde(untagged)]
    Specific(String),
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

// ─── 共用解析函数 ───

/// 解析 SKILL.md 的 YAML frontmatter 和 Markdown 正文
///
/// 格式：
/// ```markdown
/// ---
/// name: skill-name
/// description: |
///   多行描述
///   第二行
/// version: 1.0
/// category: tool
/// phase: all
/// ---
///
/// 正文内容...
/// ```
pub fn parse_skill_md(content: &str) -> (SkillMetadata, String) {
    let mut metadata = SkillMetadata::default();
    let body;

    if let Some(rest) = content.strip_prefix("---") {
        if let Some((frontmatter, rest_body)) = rest.split_once("---") {
            metadata = parse_yaml_frontmatter(frontmatter);
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
///
/// 支持：
/// - 单行值：`key: value`
/// - 多行值：`key: |` 或 `key: >`
/// - 引号值：`key: "value"` 或 `key: 'value'`
pub fn parse_yaml_frontmatter(frontmatter: &str) -> SkillMetadata {
    let mut meta = SkillMetadata::default();
    let mut in_description = false;
    let mut description_lines: Vec<String> = Vec::new();
    let mut in_paths = false;

    for line in frontmatter.lines() {
        let trimmed = line.trim();

        // 处理 paths 列表格式
        if in_paths {
            if trimmed.starts_with('-') {
                // 列表项：`- value`
                let item = trimmed
                    .trim_start_matches('-')
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                if !item.is_empty() {
                    meta.paths.push(item.to_string());
                }
                continue;
            } else if trimmed.contains(':') && !trimmed.starts_with(' ') && !trimmed.starts_with('\t') {
                // 新的 key: value，结束 paths 收集
                in_paths = false;
            } else {
                // 忽略空行或其他格式
                continue;
            }
        }

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
                    if let Some(rest) = value.strip_prefix('|') {
                        // 多行描述开始
                        in_description = true;
                        // 如果 | 后面还有内容
                        let rest = rest.trim();
                        if !rest.is_empty() {
                            description_lines.push(rest.to_string());
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
                "paths" => {
                    if value.is_empty() {
                        // 空值，可能是列表格式的开始
                        in_paths = true;
                    } else {
                        // 逗号分隔格式
                        meta.paths = value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
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

    meta
}

/// 从 SKILL.md body 提取触发关键词
///
/// 扫描 "触发条件" 或 "触发词" 段落，提取以 `-` 开头的触发词。
pub fn extract_triggers_from_body(body: &str) -> Vec<String> {
    let mut triggers = Vec::new();
    let mut in_trigger_section = false;

    for line in body.lines() {
        let trimmed = line.trim();

        // 检测触发条件段落
        if trimmed.contains("触发条件") || trimmed.contains("触发词") {
            in_trigger_section = true;
            continue;
        }

        // 检测段落结束
        if in_trigger_section && (trimmed.starts_with("## ") || trimmed.starts_with("### ")) {
            in_trigger_section = false;
            continue;
        }

        // 提取触发词
        if in_trigger_section && trimmed.starts_with('-') {
            let trigger = trimmed
                .trim_start_matches('-')
                .trim()
                .trim_matches('\u{201c}')  // "
                .trim_matches('\u{201d}')  // "
                .trim_matches('"');
            if !trigger.is_empty() {
                triggers.push(trigger.to_string());
            }
        }
    }

    triggers
}
