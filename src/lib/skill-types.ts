/**
 * 技能系统 TypeScript 类型定义
 * 与 Rust services/skill_types.rs 保持一致
 */

export type SkillCategory = "core" | "stage" | "mgmt" | "tool" | string;

export interface SkillMetadata {
  name?: string;
  description?: string;
  version?: string;
  category: SkillCategory;
  phase: "all" | string;
  icon?: string;
}

export interface Skill {
  name: string;
  location: string;
  metadata: SkillMetadata;
  body: string;
  scripts: string[];
  references: string[];
}

export interface SkillStatsResponse {
  total: number;
  by_category: [string, number][];
}

export interface SkillScanResult {
  total: number;
  by_category: [string, number][];
}

/** 分类中文标签 */
export const SKILL_CATEGORY_LABELS: Record<string, string> = {
  core: "核心技能",
  stage: "阶段技能",
  mgmt: "管理技能",
  tool: "工具技能",
  other: "其他",
};

/** 分类图标 */
export const SKILL_CATEGORY_ICONS: Record<string, string> = {
  core: "⚙️",
  stage: "📋",
  mgmt: "📊",
  tool: "🔧",
  other: "📦",
};
