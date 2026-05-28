//! Deliverable recipes — pre-configured fill strategies for key ERP deliverables
//!
//! Each recipe maps a template type to customized field fill strategies and a
//! domain-specific system prompt. These override the default schema strategies
//! to produce higher-quality output for standard deliverable formats.
//!
//! The 7 key deliverables:
//! 1. 调研报告  (Requirements Investigation Report)
//! 2. 周报/月报 (Weekly/Monthly Progress Report)
//! 3. 业务蓝图  (Business Blueprint)
//! 4. PCR        (Project Change Request)
//! 5. 上线单     (Go-Live Checklist)
//! 6. 验收单     (Acceptance Sign-off)
//! 7. 会议纪要   (Meeting Minutes)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::template_schema::SchemaField;

// ─── Types ───

/// A deliverable recipe: pre-configured fill strategy overrides + system prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliverableRecipe {
    /// Human-readable recipe name (e.g., "调研报告")
    pub name: String,
    /// Template ID this recipe applies to
    pub template_id: String,
    /// Phase this deliverable belongs to (e.g., "L2-调研")
    pub phase: String,
    /// Brief description of what this deliverable is for
    pub description: String,
    /// Per-field fill strategy overrides (key: field_name, value: strategy)
    /// If a field is listed here, it overrides the schema's default fill_strategy.
    pub field_overrides: HashMap<String, FieldOverride>,
    /// System prompt tailored to this deliverable type
    pub system_prompt: String,
}

/// Override for a single field's fill strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldOverride {
    /// The fill strategy to use: "ai", "kb", "user", "default"
    pub strategy: String,
    /// Optional hint/guidance for the LLM when filling this field
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

// ─── Recipe Registry ───

/// Get all registered deliverable recipes.
///
/// Returns a vec of 7 pre-configured recipes for the standard ERP deliverables.
pub fn all_recipes() -> Vec<DeliverableRecipe> {
    vec![
        recipe_investigation_report(),
        recipe_weekly_monthly_report(),
        recipe_business_blueprint(),
        recipe_pcr(),
        recipe_go_live(),
        recipe_acceptance(),
        recipe_meeting_minutes(),
    ]
}

/// Look up a recipe by template_id.
///
/// Returns `None` if no recipe matches the given template ID.
pub fn get_recipe_by_template_id(template_id: &str) -> Option<DeliverableRecipe> {
    all_recipes()
        .into_iter()
        .find(|r| r.template_id == template_id)
}

/// Look up a recipe by name (partial match).
///
/// Returns all recipes whose name contains the given substring.
pub fn get_recipes_by_name(name: &str) -> Vec<DeliverableRecipe> {
    all_recipes()
        .into_iter()
        .filter(|r| r.name.contains(name))
        .collect()
}

/// Apply a recipe's field overrides to a list of schema fields.
///
/// For each field in `fields`, if the recipe has an override for that field name,
/// the field's `fill_strategy` is replaced with the override's strategy.
/// Returns a new Vec with the overrides applied.
pub fn apply_recipe_overrides(
    fields: &[SchemaField],
    recipe: &DeliverableRecipe,
) -> Vec<SchemaField> {
    fields
        .iter()
        .map(|f| {
            if let Some(ovr) = recipe.field_overrides.get(&f.name) {
                let mut f = f.clone();
                f.fill_strategy = ovr.strategy.clone();
                // If the override has a hint and the field has no description, use the hint
                if f.description.is_none() {
                    if let Some(ref hint) = ovr.hint {
                        f.description = Some(hint.clone());
                    }
                }
                f
            } else {
                f.clone()
            }
        })
        .collect()
}

// ─── Recipe Definitions ───

/// 1. 调研报告 — Requirements Investigation Report
fn recipe_investigation_report() -> DeliverableRecipe {
    let mut overrides = HashMap::new();

    overrides.insert(
        "调研背景".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("基于项目背景和行业特点，描述本次调研的背景和目的".to_string()),
        },
    );
    overrides.insert(
        "企业概况".to_string(),
        FieldOverride {
            strategy: "kb".to_string(),
            hint: Some("从知识库中提取企业基本信息，如行业、规模、组织架构".to_string()),
        },
    );
    overrides.insert(
        "业务现状".to_string(),
        FieldOverride {
            strategy: "kb".to_string(),
            hint: Some("从知识库中提取企业当前业务流程和系统使用情况".to_string()),
        },
    );
    overrides.insert(
        "问题分析".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("基于调研结果，总结企业在流程、系统、数据等方面的主要问题".to_string()),
        },
    );
    overrides.insert(
        "建议方案".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("针对发现的问题，给出金蝶ERP解决方案建议".to_string()),
        },
    );

    DeliverableRecipe {
        name: "调研报告".to_string(),
        template_id: "investigation_report".to_string(),
        phase: "L2-调研".to_string(),
        description: "项目调研阶段输出，记录企业现状、问题分析和建议方案".to_string(),
        field_overrides: overrides,
        system_prompt: "你是一位资深的金蝶ERP实施顾问，正在撰写项目调研报告。\n\
             【输出结构约束】\n\
             1.【现有流程 As-Is】— 客户当前业务操作模式（必须有具体流程描述）\n\
             2.【系统方案 To-Be】— 金蝶标准解决方案（必须含系统路径/单据类型）\n\
             3.【差异分析】— 逐条标注 Fit(标准配置) 或 Gap(需评估)\n\
             4.【实施建议】— 具体的配置参数、业务规则或操作步骤\n\
             【禁止】\n\
             - 禁止「实现高效管理」「优化流程」等无具体操作的套话\n\
             - 禁止编造不存在的系统功能或二开方案\n\
             - 不确定的内容写「待确认」，不得用模糊表述填充"
            .to_string(),
    }
}

/// 2. 周报/月报 — Weekly/Monthly Progress Report
fn recipe_weekly_monthly_report() -> DeliverableRecipe {
    let mut overrides = HashMap::new();

    overrides.insert(
        "本期工作内容".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("总结本周/月的主要工作内容和进展".to_string()),
        },
    );
    overrides.insert(
        "完成情况".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("描述各任务的完成百分比和关键里程碑达成情况".to_string()),
        },
    );
    overrides.insert(
        "问题与风险".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("列出当前遇到的问题和潜在风险，及应对措施".to_string()),
        },
    );
    overrides.insert(
        "下期计划".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("规划下一阶段的工作重点和目标".to_string()),
        },
    );

    DeliverableRecipe {
        name: "周报/月报".to_string(),
        template_id: "weekly_monthly_report".to_string(),
        phase: "全程".to_string(),
        description: "项目过程中的定期进度汇报文档".to_string(),
        field_overrides: overrides,
        system_prompt: "你是一位金蝶ERP项目经理，正在撰写项目进度报告。\n\
            报告需要清晰地展示项目进展、问题和计划。\n\
            \n\
            写作要求：\n\
            - 条理清晰，使用编号列表\n\
            - 量化指标（完成百分比、工时等）\n\
            - 问题描述要包含影响和应对措施\n\
            - 下期计划要有时间节点"
            .to_string(),
    }
}

/// 3. 业务蓝图 — Business Blueprint
fn recipe_business_blueprint() -> DeliverableRecipe {
    let mut overrides = HashMap::new();

    overrides.insert(
        "业务流程设计".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("基于金蝶ERP最佳实践，设计目标业务流程".to_string()),
        },
    );
    overrides.insert(
        "系统功能映射".to_string(),
        FieldOverride {
            strategy: "kb".to_string(),
            hint: Some("从知识库中提取金蝶ERP相关模块和功能配置".to_string()),
        },
    );
    overrides.insert(
        "数据迁移方案".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("规划历史数据的迁移策略、范围和方法".to_string()),
        },
    );
    overrides.insert(
        "集成方案".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("描述与其他系统的集成接口和数据流向".to_string()),
        },
    );
    overrides.insert(
        "组织架构设计".to_string(),
        FieldOverride {
            strategy: "kb".to_string(),
            hint: Some("从知识库中提取企业组织架构和权限设计".to_string()),
        },
    );

    DeliverableRecipe {
        name: "业务蓝图".to_string(),
        template_id: "business_blueprint".to_string(),
        phase: "L3-蓝图".to_string(),
        description: "蓝图阶段核心交付物，描述ERP系统的目标业务流程和功能设计".to_string(),
        field_overrides: overrides,
        system_prompt: "你是一位资深的金蝶ERP业务架构师，正在设计业务蓝图。\n\
             【四段硬结构】\n\
             1.【现有流程 As-Is】— 客户当前操作模式，必须有具体业务场景描述\n\
             2.【标准流程 To-Be】— 金蝶标准方案，必须含系统路径（如：财务会计→总账→凭证录入）\n\
             3.【差异配置点】— 按「配置路径: 配置值」列出具体配置项\n\
             4.【对应单据类型】— 涉及的 ERP 单据及编码规则\n\
             【禁止】\n\
             - 禁止「实现高效管理」「优化流程」等无具体操作的表述\n\
             - 不确定的内容写「[待确认]」，不得空洞填充"
            .to_string(),
    }
}

/// 4. PCR — Project Change Request
fn recipe_pcr() -> DeliverableRecipe {
    let mut overrides = HashMap::new();

    overrides.insert(
        "变更描述".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("清晰描述变更的内容、范围和触发原因".to_string()),
        },
    );
    overrides.insert(
        "影响分析".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("分析变更对进度、成本、质量、资源的影响".to_string()),
        },
    );
    overrides.insert(
        "风险评估".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("评估变更带来的风险和应对措施".to_string()),
        },
    );
    overrides.insert(
        "实施方案".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("制定变更实施的步骤、资源和时间计划".to_string()),
        },
    );

    DeliverableRecipe {
        name: "PCR（项目变更申请）".to_string(),
        template_id: "pcr".to_string(),
        phase: "全程".to_string(),
        description: "项目变更管理文档，记录变更请求的影响分析和审批".to_string(),
        field_overrides: overrides,
        system_prompt: "你是一位金蝶ERP项目经理，正在撰写项目变更申请（PCR）。\n\
            PCR需要客观分析变更的必要性、影响和实施方案。\n\
            \n\
            写作要求：\n\
            - 变更描述要具体，明确变更前后的差异\n\
            - 影响分析要量化（工期天数、成本金额等）\n\
            - 风险评估要分级（高/中/低）并给出应对措施\n\
            - 实施方案要有明确的步骤和责任人"
            .to_string(),
    }
}

/// 5. 上线单 — Go-Live Checklist
fn recipe_go_live() -> DeliverableRecipe {
    let mut overrides = HashMap::new();

    overrides.insert(
        "上线条件检查".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("列出系统上线前必须满足的各项条件和检查结果".to_string()),
        },
    );
    overrides.insert(
        "数据准备情况".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("描述主数据、期初数据的准备进度和质量".to_string()),
        },
    );
    overrides.insert(
        "用户培训情况".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("总结用户培训的完成率和掌握程度".to_string()),
        },
    );
    overrides.insert(
        "应急预案".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("制定上线失败时的回退方案和应急联系人".to_string()),
        },
    );

    DeliverableRecipe {
        name: "上线单".to_string(),
        template_id: "go_live".to_string(),
        phase: "L5-上线".to_string(),
        description: "系统上线前的检查清单，确认各项准备就绪".to_string(),
        field_overrides: overrides,
        system_prompt: "你是一位金蝶ERP实施顾问，正在准备系统上线检查单。\n\
            上线单是系统正式切换前的最后关口，需要逐项确认各项条件。\n\
            \n\
            写作要求：\n\
            - 检查项要全面，覆盖数据、功能、性能、安全等方面\n\
            - 每项要有明确的通过标准和实际状态\n\
            - 应急预案要具体，包含回退步骤和联系方式\n\
            - 语言简洁明确，适合作为检查清单使用"
            .to_string(),
    }
}

/// 6. 验收单 — Acceptance Sign-off
fn recipe_acceptance() -> DeliverableRecipe {
    let mut overrides = HashMap::new();

    overrides.insert(
        "验收标准".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("列出项目验收的各项标准和指标".to_string()),
        },
    );
    overrides.insert(
        "验收结果".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("描述各项验收标准的达成情况".to_string()),
        },
    );
    overrides.insert(
        "遗留问题".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("记录验收过程中发现的遗留问题和处理计划".to_string()),
        },
    );
    overrides.insert(
        "项目总结".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("总结项目实施的整体成果、经验教训".to_string()),
        },
    );

    DeliverableRecipe {
        name: "验收单".to_string(),
        template_id: "acceptance".to_string(),
        phase: "L6-验收".to_string(),
        description: "项目验收阶段的正式签署文档，确认项目交付物满足要求".to_string(),
        field_overrides: overrides,
        system_prompt: "你是一位金蝶ERP项目经理，正在准备项目验收文档。\n\
            验收单是项目的正式收尾文档，需要全面总结项目成果。\n\
            \n\
            写作要求：\n\
            - 验收标准要与合同/需求对应\n\
            - 验收结果要有数据支撑\n\
            - 遗留问题要明确责任人和解决时间\n\
            - 项目总结要客观，既肯定成果也总结教训"
            .to_string(),
    }
}

/// 7. 会议纪要 — Meeting Minutes
fn recipe_meeting_minutes() -> DeliverableRecipe {
    let mut overrides = HashMap::new();

    overrides.insert(
        "会议议题".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("列出本次会议讨论的主要议题".to_string()),
        },
    );
    overrides.insert(
        "讨论内容".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("记录各议题的讨论要点和各方观点".to_string()),
        },
    );
    overrides.insert(
        "决议事项".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("总结会议达成的共识和决策".to_string()),
        },
    );
    overrides.insert(
        "待办事项".to_string(),
        FieldOverride {
            strategy: "ai".to_string(),
            hint: Some("列出后续行动项，包含责任人和截止时间".to_string()),
        },
    );

    DeliverableRecipe {
        name: "会议纪要".to_string(),
        template_id: "meeting_minutes".to_string(),
        phase: "全程".to_string(),
        description: "项目相关会议的正式记录，包含议题、决议和待办事项".to_string(),
        field_overrides: overrides,
        system_prompt: "你是一位金蝶ERP项目助理，正在整理会议纪要。\n\
             【三明治结构】\n\
             1.【会议结论】— 本次会议达成的核心结论（逐条列出）\n\
             2.【确认免责事项】— 客户/厂商明确的免责范围或边界条件\n\
             3.【待办行动计划】— 必须含「负责人、截止日期、交付物」三要素\n\
             【禁止】\n\
             - 禁止模糊表述（如「继续推进」「进一步讨论」）\n\
             - 每项待办必须明确到人×时间×产出"
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_recipes_count() {
        assert_eq!(all_recipes().len(), 7);
    }

    #[test]
    fn test_get_recipe_by_template_id() {
        let recipe = get_recipe_by_template_id("investigation_report");
        assert!(recipe.is_some());
        assert_eq!(recipe.unwrap().name, "调研报告");
    }

    #[test]
    fn test_get_recipe_by_template_id_not_found() {
        assert!(get_recipe_by_template_id("nonexistent").is_none());
    }

    #[test]
    fn test_get_recipes_by_name() {
        let results = get_recipes_by_name("报告");
        assert!(!results.is_empty());
    }

    #[test]
    fn test_apply_recipe_overrides() {
        let fields = vec![
            SchemaField {
                name: "调研背景".to_string(),
                field_type: "text".to_string(),
                fill_strategy: "user".to_string(),
                required: true,
                default: None,
                description: None,
                cell_refs: None,
            },
            SchemaField {
                name: "其他字段".to_string(),
                field_type: "text".to_string(),
                fill_strategy: "user".to_string(),
                required: false,
                default: None,
                description: None,
                cell_refs: None,
            },
        ];

        let recipe = get_recipe_by_template_id("investigation_report").unwrap();
        let overridden = apply_recipe_overrides(&fields, &recipe);

        // 调研背景 should be overridden to "ai"
        assert_eq!(overridden[0].fill_strategy, "ai");
        // 其他字段 should remain "user"
        assert_eq!(overridden[1].fill_strategy, "user");
    }
}
