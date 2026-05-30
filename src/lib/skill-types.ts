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
  paths: string[];
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

/** 支撑文件类型 */
export type SkillFileType = "reference" | "script" | "asset" | "config" | "other";

/** 技能支撑文件 */
export interface SkillFile {
  path: string;
  name: string;
  file_type: SkillFileType;
  size: number;
  last_modified: number;
}

/** _shared 共享资源 */
export interface SharedResource {
  name: string;
  path: string;
  content?: string;
}

/** 技能完整信息（含支撑文件） */
export interface SkillFull {
  skill: Skill;
  supporting_files: SkillFile[];
  shared_references: SharedResource[];
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

/** 文件类型标签 */
export const SKILL_FILE_TYPE_LABELS: Record<string, string> = {
  script: "脚本",
  reference: "参考",
  asset: "资源",
  config: "配置",
  other: "其他",
};

/** 文件类型图标 */
export const SKILL_FILE_TYPE_ICONS: Record<string, string> = {
  script: "📜",
  reference: "📖",
  asset: "🖼️",
  config: "⚙️",
  other: "📄",
};

// ─── Phase 2: 触发匹配类型 ──────────────────────────────────

/** 匹配类型 */
export type MatchType = "keyword" | "semantic" | "path";

/** 技能匹配结果 */
export interface SkillMatch {
  skill_id: string;
  score: number;
  match_type: MatchType;
}

/** 触发上下文 */
export interface TriggerContext {
  user_input: string;
  accessed_files: string[];
  current_phase?: string;
  session_id: string;
}

/** 技能提示条目（用于系统提示注入） */
export interface SkillPromptEntry {
  id: string;
  name: string;
  description: string;
  category: string;
  phase?: string;
  triggers: string[];
}

/** 匹配类型标签 */
export const MATCH_TYPE_LABELS: Record<MatchType, string> = {
  keyword: "关键词匹配",
  semantic: "语义匹配",
  path: "路径匹配",
};

/** 匹配类型图标 */
export const MATCH_TYPE_ICONS: Record<MatchType, string> = {
  keyword: "🔤",
  semantic: "🧠",
  path: "📁",
};

// ─── Phase 3: 脚本执行与模板类型 ──────────────────────────────

/** 执行结果 */
export interface ExecutionResult {
  success: boolean;
  output: string;
  duration_ms: number;
  error?: string;
}

/** 模板清单 */
export interface TemplateManifest {
  version: string;
  phases: PhaseTemplates[];
}

/** 阶段模板 */
export interface PhaseTemplates {
  phase: string;
  templates: Template[];
}

/** 单个模板 */
export interface Template {
  id: string;
  name: string;
  description: string;
  url: string;
  size: number;
  checksum: string;
}

// ─── Phase 4: 图像处理类型 ──────────────────────────────────

/** 图像类型 */
export type ImageType = "text_screenshot" | "flowchart" | "architecture" | "table" | "mixed";

/** OCR 提供商 */
export type OcrProvider = "baidu" | "tencent" | "tesseract";

/** 多模态 LLM 提供商 */
export type VisionProvider = "gpt4v" | "qwen_vl" | "glm4v" | "claude";

/** 图像处理依赖状态 */
export interface ImageDepsStatus {
  ocr_configured: boolean;
  vision_configured: boolean;
  ocr_provider: OcrProvider | null;
  vision_provider: VisionProvider | null;
}

/** 图像处理结果 */
export interface ImageProcessResult {
  image_type: ImageType;
  ocr_text: string | null;
  description: string | null;
  processing_time_ms: number;
}
