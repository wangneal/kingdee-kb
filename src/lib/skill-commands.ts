/**
 * 技能系统 Tauri 命令封装
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  Skill,
  SkillStatsResponse,
  SkillScanResult,
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
