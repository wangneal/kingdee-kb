//! 提示词集中管理
//!
//! 所有 AI 系统提示词通过 include_str!() 从 resources/prompts/ 目录嵌入。
//! 编辑提示词只需修改对应的 .md 文件，无需改 Rust 代码。
//!
//! ## 添加新提示词
//! 1. 在 `resources/prompts/` 下创建 `.md` 文件
//! 2. 在本模块中添加 `pub const XXX: &str = include_str!("../../resources/prompts/xxx.md");`
//! 3. 在需要的模块中引用 `super::prompts::XXX`

/// 主 RAG 对话系统提示词 — 金蝶ERP实施顾问知识助手
pub const SYSTEM_PROMPT: &str = include_str!("../../resources/prompts/system_prompt.md");

/// 文档生成系统提示词 — 反模糊结构约束
pub const DOC_GEN_SYSTEM_PROMPT: &str =
    include_str!("../../resources/prompts/doc_gen_system_prompt.md");

/// 调研报告配方系统提示词
pub const RECIPE_INVESTIGATION: &str =
    include_str!("../../resources/prompts/recipe_investigation.md");

/// 周报/月报配方系统提示词
pub const RECIPE_WEEKLY: &str = include_str!("../../resources/prompts/recipe_weekly.md");

/// 业务蓝图配方系统提示词
pub const RECIPE_BLUEPRINT: &str = include_str!("../../resources/prompts/recipe_blueprint.md");

/// PCR（项目变更申请）配方系统提示词
pub const RECIPE_PCR: &str = include_str!("../../resources/prompts/recipe_pcr.md");

/// 上线单配方系统提示词
pub const RECIPE_GO_LIVE: &str = include_str!("../../resources/prompts/recipe_go_live.md");

/// 验收单配方系统提示词
pub const RECIPE_ACCEPTANCE: &str =
    include_str!("../../resources/prompts/recipe_acceptance.md");

/// 会议纪要配方系统提示词
pub const RECIPE_MEETING: &str = include_str!("../../resources/prompts/recipe_meeting.md");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_not_empty() {
        assert!(!SYSTEM_PROMPT.is_empty());
        assert!(!DOC_GEN_SYSTEM_PROMPT.is_empty());
    }

    #[test]
    fn test_all_recipe_prompts_not_empty() {
        assert!(!RECIPE_INVESTIGATION.is_empty());
        assert!(!RECIPE_WEEKLY.is_empty());
        assert!(!RECIPE_BLUEPRINT.is_empty());
        assert!(!RECIPE_PCR.is_empty());
        assert!(!RECIPE_GO_LIVE.is_empty());
        assert!(!RECIPE_ACCEPTANCE.is_empty());
        assert!(!RECIPE_MEETING.is_empty());
    }

    #[test]
    fn test_prompts_contain_chinese_punctuation() {
        // 所有中文提示词应该包含中文标点
        assert!(SYSTEM_PROMPT.contains("，"));
        assert!(SYSTEM_PROMPT.contains("。"));
        assert!(RECIPE_INVESTIGATION.contains("，"));
    }

    /// 回归：所有 8 个常量都通过 include_str! 加载（防漂移到内联字符串）
    #[test]
    fn test_all_constants_use_include_str() {
        // 所有常量都包含 .md 嵌入后会保留的换行/标点模式
        // 同时不含内联字符串特有的 \n 转义符（include_str! 不会转义）
        for (name, content) in [
            ("SYSTEM_PROMPT", SYSTEM_PROMPT),
            ("DOC_GEN_SYSTEM_PROMPT", DOC_GEN_SYSTEM_PROMPT),
            ("RECIPE_INVESTIGATION", RECIPE_INVESTIGATION),
            ("RECIPE_WEEKLY", RECIPE_WEEKLY),
            ("RECIPE_BLUEPRINT", RECIPE_BLUEPRINT),
            ("RECIPE_PCR", RECIPE_PCR),
            ("RECIPE_GO_LIVE", RECIPE_GO_LIVE),
            ("RECIPE_ACCEPTANCE", RECIPE_ACCEPTANCE),
            ("RECIPE_MEETING", RECIPE_MEETING),
        ] {
            assert!(
                !content.contains("\\n"),
                "{} 不应包含 \\n 转义符（应从 .md 加载）",
                name
            );
        }
    }
}
