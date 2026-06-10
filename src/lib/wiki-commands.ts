/**
 * 维基链接 Tauri 命令封装
 *
 * 对应 Rust 后端 commands::wiki_page 模块中的 wikilink 相关命令。
 */
import { invoke } from "@tauri-apps/api/core"

// ── 类型定义（匹配 Rust WikiPage / WikiPageBrief / WikiLinkTarget 结构体） ──

/** 维基页面完整记录 */
export interface WikiPage {
  id: number
  project_id: number
  slug: string
  title: string
  page_type: string
  content: string
  content_candidate: string | null
  candidate_status: string | null
  sources_candidate: string | null
  frontmatter: string
  sources: string
  /** JSON 数组格式的链接目标 slug 列表 */
  wikilinks: string
  tags: string
  page_metadata: string
  candidate_version: number | null
  page_status: string
  version: number
  created_at: string
  updated_at: string
}

/** wiki_pages 简略信息，用于搜索候选和反向链接 */
export interface WikiPageBrief {
  id: number
  slug: string
  title: string
  page_type: string
}

/** wikilink 目标详情 */
export interface WikiLinkTarget {
  slug: string
  title: string
  page_type: string
  page_status: string
}

// ── 命令封装 ─────────────────────────────────────────────────────────────

/**
 * 搜索 wikilink 候选页面（按标题模糊搜索，排除自身）
 */
export async function searchWikilinkCandidates(
  projectId: number,
  query: string,
  excludeSlug: string,
  limit?: number,
): Promise<WikiPageBrief[]> {
  return invoke("search_wikilink_candidates", {
    projectId,
    query,
    excludeSlug,
    limit: limit ?? 20,
  })
}

/**
 * 添加 wikilink（追加 slug 到页面的 wikilinks JSON 数组，去重）
 */
export async function addWikilink(pageId: number, targetSlug: string): Promise<WikiPage> {
  return invoke("add_wikilink", { pageId, targetSlug })
}

/**
 * 移除 wikilink（从页面的 wikilinks JSON 数组中删除 slug）
 */
export async function removeWikilink(pageId: number, targetSlug: string): Promise<WikiPage> {
  return invoke("remove_wikilink", { pageId, targetSlug })
}

/**
 * 获取 wikilink 目标页面详情（按项目过滤，批量查询被引页面的标题/slug/type/status）
 */
export async function getWikilinkTargets(
  projectId: number,
  slugs: string[],
): Promise<WikiLinkTarget[]> {
  return invoke("get_wikilink_targets", { projectId, slugs })
}

/**
 * 获取反向链接（哪些页面引用了当前页面）
 */
export async function getBacklinks(projectId: number, slug: string): Promise<WikiPageBrief[]> {
  return invoke("get_backlinks", { projectId, slug })
}

// ── 知识图谱命令 ──────────────────────────────────────────────────────────

/** 图扩展检索推荐结果 */
export interface GraphRecommendation {
  slug: string
  title: string
  page_type: string
  combined_weight: number
  depth: number
  paths: string[]
  matched_signals: string[]
}

/**
 * 图扩展检索：给定页面，推荐相关页面。
 * 使用递归遍历获取多跳邻居，按组合权重排序，返回 top K。
 */
export async function graphExpandSearch(
  projectId: number,
  slug: string,
  maxDepth?: number,
  maxResults?: number,
  minWeight?: number,
): Promise<GraphRecommendation[]> {
  return invoke("graph_expand_search", {
    projectId,
    slug,
    maxDepth,
    maxResults,
    minWeight,
  })
}
