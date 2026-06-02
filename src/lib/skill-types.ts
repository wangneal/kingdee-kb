/**
 * 技能系统 TypeScript 类型定义
 * 与 Rust services/skill_types.rs 保持一致
 */

export type SkillCategory = "core" | "stage" | "mgmt" | "tool" | "other"

export interface SkillMetadata {
  name?: string
  description?: string
  version?: string
  category: SkillCategory
  phase: string
  icon?: string
  paths: string[]
}

export interface Skill {
  name: string
  location: string
  metadata: SkillMetadata
  body: string
  scripts: string[]
  references: string[]
}

export interface SkillStatsResponse {
  total: number
  by_category: [string, number][]
}

export interface SkillScanResult {
  total: number
  by_category: [string, number][]
}

/** 支撑文件类型 */
export type SkillFileType = "reference" | "script" | "asset" | "config" | "other"

/** 技能支撑文件 */
export interface SkillFile {
  path: string
  name: string
  file_type: SkillFileType
  size: number
  last_modified: number
}

/** _shared 共享资源 */
export interface SharedResource {
  name: string
  path: string
  content?: string
}

/** 技能完整信息（含支撑文件） */
export interface SkillFull {
  skill: Skill
  supporting_files: SkillFile[]
  shared_references: SharedResource[]
}

/** 分类中文标签 */
export const SKILL_CATEGORY_LABELS: Record<string, string> = {
  core: "核心技能",
  stage: "阶段技能",
  mgmt: "管理技能",
  tool: "工具技能",
  other: "其他",
}

/** 分类图标 */
export const SKILL_CATEGORY_ICONS: Record<string, string> = {
  core: "⚙️",
  stage: "📋",
  mgmt: "📊",
  tool: "🔧",
  other: "📦",
}

/** 文件类型标签 */
export const SKILL_FILE_TYPE_LABELS: Record<string, string> = {
  script: "脚本",
  reference: "参考",
  asset: "资源",
  config: "配置",
  other: "其他",
}

/** 文件类型图标 */
export const SKILL_FILE_TYPE_ICONS: Record<string, string> = {
  script: "📜",
  reference: "📖",
  asset: "🖼️",
  config: "⚙️",
  other: "📄",
}

// ─── Phase 2: 触发匹配类型 ──────────────────────────────────

/** 匹配类型 */
export type MatchType = "keyword" | "semantic" | "path"

/** 技能匹配结果 */
export interface SkillMatch {
  skill_id: string
  score: number
  match_type: MatchType
}

/** 触发上下文 */
export interface TriggerContext {
  user_input: string
  accessed_files: string[]
  current_phase?: string
  session_id: string
}

/** 技能提示条目（用于系统提示注入） */
export interface SkillPromptEntry {
  id: string
  name: string
  description: string
  category: string
  phase?: string
  triggers: string[]
}

/** 匹配类型标签 */
export const MATCH_TYPE_LABELS: Record<MatchType, string> = {
  keyword: "关键词匹配",
  semantic: "语义匹配",
  path: "路径匹配",
}

/** 匹配类型图标 */
export const MATCH_TYPE_ICONS: Record<MatchType, string> = {
  keyword: "🔤",
  semantic: "🧠",
  path: "📁",
}

// ─── Phase 3: 脚本执行与模板类型 ──────────────────────────────

/** 执行结果 */
export interface ExecutionResult {
  success: boolean
  output: string
  duration_ms: number
  error?: string
}

/** 模板清单 */
export interface TemplateManifest {
  version: string
  phases: PhaseTemplates[]
}

/** 阶段模板 */
export interface PhaseTemplates {
  phase: string
  templates: Template[]
}

/** 单个模板 */
export interface Template {
  id: string
  name: string
  description: string
  url: string
  size: number
  checksum: string
}

// ─── Phase 4: 图像处理类型 ──────────────────────────────────

/** 图像类型 */
export type ImageType = "text_screenshot" | "flowchart" | "architecture" | "table" | "mixed"

/** OCR 提供商类型 */
export type OcrProviderType = "baidu" | "tencent"

/** LLM 协议类型 */
export type LLMProtocol = "openai" | "anthropic" | "local"

/** API Key 配置 */
export interface ApiKeyConfig {
  id: string
  name: string
  key: string
  is_default: boolean
}

/** 模型配置 */
export interface ModelConfig {
  id: string
  name: string
  is_default: boolean
  is_multimodal: boolean | null
  last_probe_at: string | null
}

/** LLM 供应商配置 */
export interface LLMProviderConfig {
  id: string
  name: string
  protocol: LLMProtocol
  base_url: string
  is_default: boolean
  api_keys: ApiKeyConfig[]
  models: ModelConfig[]
  /** 旧版单个 API Key（向后兼容） */
  api_key: string
  /** 旧版单个模型名称（向后兼容） */
  model: string
  /** 旧版多模态状态（向后兼容） */
  is_multimodal: boolean | null
  /** 旧版探测时间（向后兼容） */
  last_probe_at: string | null
}

/** OCR 供应商配置 */
export interface OcrProviderConfig {
  id: string
  name: string
  provider: OcrProviderType
  api_key: string
  secret_key: string | null
  is_default: boolean
}

/** 供应商探测结果（旧版兼容） */
export interface ProviderProbeResult {
  id: string
  is_multimodal: boolean
}

/** 模型探测结果 */
export interface ModelProbeResult {
  provider_id: string
  model_id: string
  is_multimodal: boolean
}

/** 自动路由结果 */
export interface AutoRouteResult {
  provider_id: string
  model_id: string
  model_name: string
  base_url: string
}

/** 可用模型 */
export interface AvailableModel {
  provider_id: string
  provider_name: string
  model_id: string
  model_name: string
  is_default: boolean
  is_multimodal: boolean | null
}

/** 下一个可用 API Key */
export interface NextApiKeyResult {
  key_id: string
  key_value: string
}

/** 图像处理依赖状态 */
export interface ImageDepsStatus {
  ocr_configured: boolean
  vision_configured: boolean
  ocr_provider: string | null
  llm_multimodal: boolean
}

/** 图像处理结果 */
export interface ImageProcessResult {
  image_type: ImageType
  ocr_text: string | null
  description: string | null
  processing_time_ms: number
}
