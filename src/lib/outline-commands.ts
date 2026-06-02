/**
 * 大纲节点 Tauri 命令封装
 *
 * 对应 Rust 后端 commands::outline 模块中的 7 个命令。
 */
import { invoke } from "@tauri-apps/api/core"

// ── 类型定义（匹配 Rust OutlineNode 结构体） ────────────────────────────

/** 大纲节点完整记录 */
export interface OutlineNode {
  id: number
  session_id: number
  parent_id: number | null
  content: string
  sort_order: number
  question_id: number | null
  notes: string
  /** JSON 数组格式的标签列表 */
  tags: string
  /** 是否折叠（前端树节点展开/收起状态） */
  collapsed: boolean
  /** 是否已完成 */
  completed: boolean
  /** 节点标记（如图标标识） */
  marker: string
  /** 优先级 */
  priority: string
  /** 备注（轻量单行备注） */
  note: string
  created_at: string
  updated_at: string
}

/** 大纲统计信息 */
export interface OutlineStats {
  total_nodes: number
  depth: number
  max_children: number
}

// ── 命令封装 ─────────────────────────────────────────────────────────────

/**
 * 创建大纲节点
 *
 * - parentId 为 null 时创建根节点
 * - parentId 为数字时创建子节点
 */
export async function createOutlineNode(
  sessionId: number,
  parentId: number | null,
  content: string,
): Promise<OutlineNode> {
  return invoke("create_outline_node", { sessionId, parentId, content })
}

/**
 * 更新大纲节点（部分更新，仅更新传入的字段）
 */
export async function updateOutlineNode(
  id: number,
  content?: string,
  notes?: string,
  tags?: string,
  collapsed?: boolean,
  completed?: boolean,
  marker?: string,
  priority?: string,
  note?: string,
): Promise<OutlineNode> {
  return invoke("update_outline_node", {
    id,
    content: content ?? null,
    notes: notes ?? null,
    tags: tags ?? null,
    collapsed: collapsed ?? null,
    completed: completed ?? null,
    marker: marker ?? null,
    priority: priority ?? null,
    note: note ?? null,
  })
}

/**
 * 删除大纲节点及其所有后代（子树删除）
 *
 * 需要 sessionId 用于发送变更事件通知。
 */
export async function deleteOutlineNode(id: number, sessionId: number): Promise<void> {
  return invoke("delete_outline_node", { id, sessionId })
}

/**
 * 移动大纲节点到新的父节点下
 *
 * 包含环检测：不允许将节点移动到其后代节点下。
 */
export async function moveOutlineNode(
  id: number,
  newParentId: number | null,
  newSortOrder: number,
): Promise<OutlineNode> {
  return invoke("move_outline_node", { id, newParentId, newSortOrder })
}

/**
 * 获取指定 session 的完整大纲树（平铺列表）
 *
 * 返回按 parent_id 和 sort_order 排序的节点数组。
 */
export async function getOutlineTree(sessionId: number): Promise<OutlineNode[]> {
  return invoke("get_outline_tree", { sessionId })
}

/**
 * 导出大纲为指定格式的字符串
 *
 * - "markdown_list"：缩进的无序列表
 * - "markdown_headings"：## / ### / #### 层级标题
 */
export async function exportOutline(sessionId: number, format: string): Promise<string> {
  return invoke("export_outline", { sessionId, format })
}

/**
 * 获取大纲统计信息
 */
export async function getOutlineStats(sessionId: number): Promise<OutlineStats> {
  return invoke("get_outline_stats", { sessionId })
}
