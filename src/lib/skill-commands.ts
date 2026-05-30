/**
 * 技能系统 Tauri 命令封装
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  Skill,
  SkillFull,
  SkillFile,
  SharedResource,
  SkillStatsResponse,
  SkillScanResult,
  SkillMatch,
  TriggerContext,
  SkillPromptEntry,
  ExecutionResult,
  TemplateManifest,
  ImageDepsStatus,
  ImageProcessResult,
  LLMProviderConfig,
  OcrProviderConfig,
  ProviderProbeResult,
} from "./skill-types";

/** 列出所有技能 */
export async function listSkills(): Promise<Skill[]> {
  return invoke("list_skills");
}

/** 按名称获取技能详情 */
export async function getSkill(name: string): Promise<Skill | null> {
  return invoke("get_skill", { name });
}

/** 搜索技能 */
export async function searchSkills(query: string): Promise<Skill[]> {
  return invoke("search_skills", { query });
}

/** 获取技能统计 */
export async function getSkillStats(): Promise<SkillStatsResponse> {
  return invoke("get_skill_stats");
}

/** 重新扫描技能目录 */
export async function rescanSkills(): Promise<SkillScanResult> {
  return invoke("rescan_skills");
}

/** 匹配最适合的技能 */
export async function matchSkill(input: string): Promise<Skill | null> {
  return invoke("match_skill", { input });
}

/** 导入新技能：选择 SKILL.md 文件，复制到 skills/ 目录 */
export async function importSkill(filePath: string): Promise<string> {
  return invoke("import_skill", { file_path: filePath });
}

/** 获取技能完整信息（含支撑文件和共享资源） */
export async function getSkillFull(name: string): Promise<SkillFull | null> {
  return invoke("get_skill_full", { name });
}

/** 获取所有共享资源 */
export async function listSharedResources(): Promise<SharedResource[]> {
  return invoke("list_shared_resources");
}

/** 读取技能支撑文件内容 */
export async function readSkillFile(
  skillName: string,
  relativePath: string
): Promise<string> {
  return invoke("read_skill_file", { skill_name: skillName, relative_path: relativePath });
}

/** 获取技能支撑文件列表 */
export async function listSkillFiles(name: string): Promise<SkillFile[]> {
  return invoke("list_skill_files", { name });
}

// ─── Phase 2: 触发匹配命令 ──────────────────────────────────

/** 触发技能匹配（使用完整触发上下文） */
export async function triggerSkillMatch(context: TriggerContext): Promise<SkillMatch[]> {
  return invoke("trigger_skill_match", { context });
}

/** 匹配多个候选技能 */
export async function matchSkillCandidates(
  input: string,
  limit?: number
): Promise<SkillMatch[]> {
  return invoke("match_skill_candidates", { input, limit });
}

/** 生成技能列表系统提示 */
export async function getSkillListPrompt(): Promise<string> {
  return invoke("get_skill_list_prompt");
}

/** 获取技能摘要列表（用于前端展示和提示注入） */
export async function getSkillPromptEntries(): Promise<SkillPromptEntry[]> {
  return invoke("get_skill_prompt_entries");
}

// ─── Phase 3: 脚本执行与模板命令 ──────────────────────────────

/** 执行技能脚本 */
export async function executeSkillScript(
  skillId: string,
  scriptPath: string,
  arguments_: string[]
): Promise<ExecutionResult> {
  return invoke("execute_skill_script", {
    skill_id: skillId,
    script_path: scriptPath,
    arguments: arguments_,
  });
}

/** 获取模板清单 */
export async function getTemplateManifest(): Promise<TemplateManifest | null> {
  return invoke("get_template_manifest");
}

/** 保存模板清单 */
export async function saveTemplateManifest(manifest: TemplateManifest): Promise<void> {
  return invoke("save_template_manifest", { manifest });
}

// ─── Phase 4: 图像处理命令 ──────────────────────────────────

/** 检查图像处理依赖状态 */
export async function checkImageDeps(): Promise<ImageDepsStatus> {
  return invoke("check_image_deps");
}

/** 探测当前 LLM 是否支持多模态 */
export async function probeLlmMultimodal(): Promise<boolean> {
  return invoke("probe_llm_multimodal");
}

/** 保存图像处理 API 配置 */
export async function saveImageConfig(config: {
  ocr_provider?: string;
  ocr_api_key?: string;
  ocr_secret_key?: string;
  vision_fallback_api_key?: string;
  vision_fallback_base_url?: string;
  vision_fallback_model?: string;
}): Promise<void> {
  return invoke("save_image_config", config);
}

/** 处理单张图片 */
export async function processImage(imagePath: string): Promise<ImageProcessResult> {
  return invoke("process_image", { image_path: imagePath });
}

// ─── LLM 供应商管理命令 ──────────────────────────────────

/** LLM 供应商创建/更新参数 */
export type LLMProviderInput = {
  id: string;
  name: string;
  protocol: string;
  apiKey: string;
  baseUrl: string;
  model: string;
};

/** 获取所有 LLM 供应商 */
export async function listLLMProviders(): Promise<LLMProviderConfig[]> {
  return invoke("list_llm_providers");
}

/** 添加 LLM 供应商 */
export async function addLLMProvider(provider: LLMProviderInput): Promise<void> {
  return invoke("add_llm_provider", provider);
}

/** 更新 LLM 供应商 */
export async function updateLLMProvider(provider: LLMProviderInput): Promise<void> {
  return invoke("update_llm_provider", provider);
}

/** 删除 LLM 供应商 */
export async function deleteLLMProvider(id: string): Promise<void> {
  return invoke("delete_llm_provider", { id });
}

/** 设置默认 LLM 供应商 */
export async function setDefaultLLMProvider(id: string): Promise<void> {
  return invoke("set_default_llm_provider", { id });
}

/** 探测单个供应商的多模态能力 */
export async function probeProviderMultimodal(id: string): Promise<boolean> {
  return invoke("probe_provider_multimodal", { id });
}

/** 批量探测所有供应商的多模态能力 */
export async function probeAllProviders(): Promise<ProviderProbeResult[]> {
  return invoke("probe_all_providers");
}

/** 获取 OCR 配置 */
export async function getOcrConfig(): Promise<OcrProviderConfig | null> {
  return invoke("get_ocr_config");
}

/** 保存 OCR 配置 */
export async function saveOcrConfig(config: {
  id: string;
  name: string;
  provider: string;
  api_key: string;
  secret_key?: string;
}): Promise<void> {
  return invoke("save_ocr_config", config);
}

/** 清除 OCR 配置 */
export async function clearOcrConfig(): Promise<void> {
  return invoke("clear_ocr_config");
}
