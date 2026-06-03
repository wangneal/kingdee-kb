/**
 * 大纲状态管理上下文
 *
 * 提供大纲节点的完整 CRUD、树形结构构建、展开/收起、
 * 选择、撤销/重做，以及 Tauri 事件自动刷新。
 */
import { listen } from "@tauri-apps/api/event"
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react"
import {
  createOutlineNode,
  deleteOutlineNode,
  getOutlineTree,
  importMarkdownOutline,
  moveOutlineNode,
  type OutlineNode,
  updateOutlineNode,
} from "../lib/outline-commands"

// ── 导出类型 ──────────────────────────────────────────────────────────────

/** 树节点（扩展 OutlineNode，增加子节点列表） */
export interface TreeNode extends OutlineNode {
  children: TreeNode[]
}

/** 撤销/重做操作类型 */
type UndoOpType = "create" | "delete" | "update" | "move"

/** 撤销/重做条目 */
interface UndoEntry {
  type: UndoOpType
  /** 操作前的节点快照（用于撤销恢复） */
  before: OutlineNode | null
  /** 操作后的节点快照（用于重做恢复） */
  after: OutlineNode | null
  /** update 操作的旧字段值 */
  oldFields?: Partial<OutlineNode>
  /** update 操作的新字段值 */
  newFields?: Partial<OutlineNode>
  /** move 操作的旧父节点 ID */
  oldParentId?: number | null
  /** move 操作的旧排序权重 */
  oldSortOrder?: number
  /** move 操作的新父节点 ID */
  newParentId?: number | null
  /** move 操作的新排序权重 */
  newSortOrder?: number
}

/** 上下文值接口 */
interface OutlineContextValue {
  // ── 状态 ──
  /** 平铺节点列表 */
  nodes: OutlineNode[]
  /** 树形结构（用于渲染） */
  tree: TreeNode[]
  /** 当前选中节点 ID */
  selectedNodeId: number | null
  /** 已展开节点 ID 集合 */
  expandedNodeIds: Set<number>
  /** 是否正在加载 */
  isLoading: boolean
  /** 错误信息 */
  error: string | null

  // ── 会话管理 ──
  /** 加载指定会话的大纲 */
  loadOutline: (sessionId: number) => Promise<void>

  // ── CRUD 操作（自动持久化） ──
  /** 创建节点（返回 null 表示失败） */
  createNode: (parentId: number | null, content?: string) => Promise<OutlineNode | null>
  /** 更新节点字段 */
  updateNode: (
    id: number,
    fields: Partial<
      Pick<
        OutlineNode,
        "content" | "notes" | "tags" | "collapsed" | "completed" | "marker" | "priority" | "note"
      >
    >,
  ) => Promise<void>
  /** 删除节点（子树删除） */
  deleteNode: (id: number) => Promise<void>
  /** 移动节点到新的父节点下 */
  moveNode: (id: number, newParentId: number | null, newSortOrder: number) => Promise<void>
  /** 从 Markdown 同步导入大纲 */
  importMarkdown: (markdown: string) => Promise<void>

  // ── 选择 ──
  selectNode: (id: number | null) => void

  // ── 展开/收起 ──
  toggleExpand: (id: number) => void
  expandAll: () => void
  collapseAll: () => void

  // ── 撤销/重做 ──
  undo: () => void
  redo: () => void
  canUndo: boolean
  canRedo: boolean
}

// ── 辅助函数 ──────────────────────────────────────────────────────────────

/** 从平铺节点列表构建树结构 */
function buildTree(nodes: OutlineNode[]): TreeNode[] {
  // 排序：根节点优先，然后按 sort_order
  const sorted = [...nodes].sort((a, b) => {
    const aRoot = a.parent_id === null ? 0 : 1
    const bRoot = b.parent_id === null ? 0 : 1
    if (aRoot !== bRoot) return aRoot - bRoot
    if (a.parent_id !== b.parent_id) return (a.parent_id ?? 0) - (b.parent_id ?? 0)
    return a.sort_order - b.sort_order
  })

  // 构建 id → TreeNode 映射
  const map = new Map<number, TreeNode>()
  for (const node of sorted) {
    map.set(node.id, { ...node, children: [] })
  }

  // 挂载子节点到父节点
  const roots: TreeNode[] = []
  for (const node of sorted) {
    const treeNode = map.get(node.id)
    if (!treeNode) continue
    if (node.parent_id === null) {
      roots.push(treeNode)
    } else {
      const parent = map.get(node.parent_id)
      if (parent) {
        parent.children.push(treeNode)
      } else {
        // 父节点不存在（数据异常），作为根节点处理
        roots.push(treeNode)
      }
    }
  }

  return roots
}

/** 收集树中所有节点 ID */
function collectAllIds(nodes: OutlineNode[]): number[] {
  return nodes.map((n) => n.id)
}

/** 判断节点是否是指定节点的后代 */
function isDescendantOf(node: OutlineNode, ancestorId: number, allNodes: OutlineNode[]): boolean {
  let current = node
  const map = new Map(allNodes.map((n) => [n.id, n]))

  while (current.parent_id !== null) {
    if (current.parent_id === ancestorId) return true
    const parent = map.get(current.parent_id)
    if (!parent) break
    current = parent
  }
  return false
}

// ── 上下文 ────────────────────────────────────────────────────────────────

const OutlineContext = createContext<OutlineContextValue | null>(null)

/** 大纲上下文 Hook */
export function useOutline(): OutlineContextValue {
  const ctx = useContext(OutlineContext)
  if (!ctx) throw new Error("useOutline must be used within OutlineProvider")
  return ctx
}

// ── 常量 ──────────────────────────────────────────────────────────────────

const MAX_UNDO_ENTRIES = 50
const DEBOUNCE_EVENT_MS = 300
const DEBOUNCE_SAVE_MS = 800

// ── Provider ──────────────────────────────────────────────────────────────

export function OutlineProvider({ children }: { children: ReactNode }) {
  // ── 核心状态 ──
  const [nodes, setNodes] = useState<OutlineNode[]>([])
  const [selectedNodeId, setSelectedNodeId] = useState<number | null>(null)
  const [expandedNodeIds, setExpandedNodeIds] = useState<Set<number>>(new Set())
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // ── 会话与撤销/重做状态（ref 避免闭包过期） ──
  const currentSessionIdRef = useRef<number | null>(null)
  const undoStackRef = useRef<UndoEntry[]>([])
  const redoStackRef = useRef<UndoEntry[]>([])
  const revisionRef = useRef(0)
  const isUndoRedoRef = useRef(false)

  // ── 定时器 ──
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // ── 派生值 ──
  const tree = buildTree(nodes)
  const canUndo = undoStackRef.current.length > 0
  const canRedo = redoStackRef.current.length > 0

  // ── 刷新大纲（从后端重新加载） ──
  const refreshOutline = useCallback(async () => {
    const sessionId = currentSessionIdRef.current
    if (sessionId === null) return

    try {
      const fetched = await getOutlineTree(sessionId)
      setNodes(fetched)
      // 默认展开所有节点
      setExpandedNodeIds(new Set(collectAllIds(fetched)))
      setError(null)
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      setError(msg)
      console.error("[OutlineContext] 刷新大纲失败:", msg)
    }
  }, [])

  // ── 加载大纲 ──
  const loadOutline = useCallback(async (sessionId: number) => {
    currentSessionIdRef.current = sessionId
    setIsLoading(true)
    setError(null)
    // 切换会话时清空撤销/重做栈
    undoStackRef.current = []
    redoStackRef.current = []

    try {
      const fetched = await getOutlineTree(sessionId)
      setNodes(fetched)
      setExpandedNodeIds(new Set(collectAllIds(fetched)))
      setSelectedNodeId(null)
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      setError(msg)
      console.error("[OutlineContext] 加载大纲失败:", msg)
    } finally {
      setIsLoading(false)
    }
  }, [])

  // ── 推入撤销条目 ──
  const pushUndo = useCallback((entry: UndoEntry) => {
    undoStackRef.current.push(entry)
    // 超过上限时移除最旧的条目
    if (undoStackRef.current.length > MAX_UNDO_ENTRIES) {
      undoStackRef.current.shift()
    }
    // 新操作时清空重做栈
    redoStackRef.current = []
  }, [])

  // ── 标记撤销/重做修订 ──
  const markRevision = useCallback(() => {
    isUndoRedoRef.current = true
    revisionRef.current += 1

    // 800ms 防抖后自动持久化
    if (saveTimerRef.current) {
      clearTimeout(saveTimerRef.current)
    }
    saveTimerRef.current = setTimeout(() => {
      refreshOutline()
      // 持久化完成后恢复外部事件监听
      isUndoRedoRef.current = false
    }, DEBOUNCE_SAVE_MS)
  }, [refreshOutline])

  // ── CRUD 操作 ──

  const createNode = useCallback(
    async (parentId: number | null, content = ""): Promise<OutlineNode | null> => {
      const sessionId = currentSessionIdRef.current
      if (sessionId === null) return null

      try {
        const newNode = await createOutlineNode(sessionId, parentId, content)
        setNodes((prev) => [...prev, newNode])
        setExpandedNodeIds((prev) => {
          const next = new Set(prev)
          next.add(newNode.id)
          return next
        })
        pushUndo({ type: "create", before: null, after: newNode })
        return newNode
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        setError(msg)
        console.error("[OutlineContext] 创建节点失败:", msg)
        return null
      }
    },
    [pushUndo],
  )

  const updateNode = useCallback(
    async (
      id: number,
      fields: Partial<
        Pick<
          OutlineNode,
          "content" | "notes" | "tags" | "collapsed" | "completed" | "marker" | "priority" | "note"
        >
      >,
    ) => {
      try {
        const updated = await updateOutlineNode(
          id,
          fields.content,
          fields.notes,
          fields.tags,
          fields.collapsed,
          fields.completed,
          fields.marker,
          fields.priority,
          fields.note,
        )
        setNodes((prev) => prev.map((n) => (n.id === id ? updated : n)))

        // 记录撤销条目
        const oldNode = nodes.find((n) => n.id === id)
        if (oldNode) {
          pushUndo({
            type: "update",
            before: oldNode,
            after: updated,
            oldFields: fields,
            newFields: fields,
          })
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        setError(msg)
        console.error("[OutlineContext] 更新节点失败:", msg)
      }
    },
    [nodes, pushUndo],
  )

  const deleteNode = useCallback(
    async (id: number) => {
      const sessionId = currentSessionIdRef.current
      if (sessionId === null) return

      // 保存被删除节点的完整快照（含子节点）用于撤销
      const deletedNodes = nodes.filter((n) => n.id === id || isDescendantOf(n, id, nodes))

      try {
        await deleteOutlineNode(id, sessionId)
        setNodes((prev) => prev.filter((n) => n.id !== id && !isDescendantOf(n, id, prev)))
        if (selectedNodeId === id || deletedNodes.some((n) => n.id === selectedNodeId)) {
          setSelectedNodeId(null)
        }

        // 为每个被删除的节点记录撤销条目
        for (const node of deletedNodes) {
          pushUndo({ type: "delete", before: node, after: null })
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        setError(msg)
        console.error("[OutlineContext] 删除节点失败:", msg)
      }
    },
    [nodes, selectedNodeId, pushUndo],
  )

  const moveNode = useCallback(
    async (id: number, newParentId: number | null, newSortOrder: number) => {
      const oldNode = nodes.find((n) => n.id === id)
      if (!oldNode) return

      try {
        const moved = await moveOutlineNode(id, newParentId, newSortOrder)
        setNodes((prev) => prev.map((n) => (n.id === id ? moved : n)))
        pushUndo({
          type: "move",
          before: oldNode,
          after: moved,
          oldParentId: oldNode.parent_id,
          oldSortOrder: oldNode.sort_order,
          newParentId,
          newSortOrder,
        })
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        setError(msg)
        console.error("[OutlineContext] 移动节点失败:", msg)
        await refreshOutline()
      }
    },
    [nodes, pushUndo, refreshOutline],
  )

  const importMarkdown = useCallback(
    async (markdown: string) => {
      const sessionId = currentSessionIdRef.current
      if (sessionId === null) return

      try {
        await importMarkdownOutline(sessionId, markdown)
        await refreshOutline()
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        setError(msg)
        console.error("[OutlineContext] 导入大纲失败:", msg)
      }
    },
    [refreshOutline],
  )

  // ── 选择 ──

  const selectNode = useCallback((id: number | null) => {
    setSelectedNodeId(id)
  }, [])

  // ── 展开/收起 ──

  const toggleExpand = useCallback((id: number) => {
    setExpandedNodeIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }, [])

  const expandAll = useCallback(() => {
    setExpandedNodeIds(new Set(collectAllIds(nodes)))
  }, [nodes])

  const collapseAll = useCallback(() => {
    setExpandedNodeIds(new Set())
  }, [])

  // ── 撤销/重做 ──

  const undo = useCallback(() => {
    const entry = undoStackRef.current.pop()
    if (!entry) return

    isUndoRedoRef.current = true

    switch (entry.type) {
      case "create": {
        // 撤销创建 → 删除节点
        const afterNode = entry.after
        if (afterNode) {
          const sid = currentSessionIdRef.current
          if (sid !== null) {
            deleteOutlineNode(afterNode.id, sid).catch(console.error)
          }
          setNodes((prev) => prev.filter((n) => n.id !== afterNode.id))
        }
        break
      }
      case "delete": {
        // 撤销删除 → 重新创建节点
        const beforeNode = entry.before
        if (beforeNode) {
          const sid = currentSessionIdRef.current
          if (sid !== null) {
            createOutlineNode(sid, beforeNode.parent_id, beforeNode.content)
              .then(async (restored) => {
                // 恢复其他字段
                await updateOutlineNode(
                  restored.id,
                  undefined,
                  beforeNode.notes,
                  beforeNode.tags,
                  beforeNode.collapsed,
                  beforeNode.completed,
                  beforeNode.marker,
                  beforeNode.priority,
                  beforeNode.note,
                )
                // 持久化后刷新以获取正确的排序
                await refreshOutline()
              })
              .catch(console.error)
          }
          // 立即更新本地状态
          setNodes((prev) => [...prev, beforeNode])
        }
        break
      }
      case "update": {
        // 撤销更新 → 恢复旧字段值
        const beforeNode = entry.before
        const oldFlds = entry.oldFields
        if (beforeNode && oldFlds) {
          updateOutlineNode(
            beforeNode.id,
            oldFlds.content,
            oldFlds.notes,
            oldFlds.tags,
            oldFlds.collapsed,
            oldFlds.completed,
            oldFlds.marker,
            oldFlds.priority,
            oldFlds.note,
          ).catch(console.error)
          setNodes((prev) => prev.map((n) => (n.id === beforeNode.id ? beforeNode : n)))
        }
        break
      }
      case "move": {
        // 撤销移动 → 恢复旧位置
        const beforeNode = entry.before
        const oldPid = entry.oldParentId
        const oldSort = entry.oldSortOrder
        if (beforeNode && oldPid !== undefined && oldSort !== undefined) {
          moveOutlineNode(beforeNode.id, oldPid, oldSort).catch(console.error)
          setNodes((prev) =>
            prev.map((n) =>
              n.id === beforeNode.id ? { ...n, parent_id: oldPid, sort_order: oldSort } : n,
            ),
          )
        }
        break
      }
    }

    redoStackRef.current.push(entry)
    markRevision()
  }, [refreshOutline, markRevision])

  const redo = useCallback(() => {
    const entry = redoStackRef.current.pop()
    if (!entry) return

    isUndoRedoRef.current = true

    switch (entry.type) {
      case "create": {
        // 重做创建 → 重新创建节点
        const afterNode = entry.after
        if (afterNode) {
          const sid = currentSessionIdRef.current
          if (sid !== null) {
            createOutlineNode(sid, afterNode.parent_id, afterNode.content)
              .then(async (restored) => {
                await updateOutlineNode(
                  restored.id,
                  undefined,
                  afterNode.notes,
                  afterNode.tags,
                  afterNode.collapsed,
                  afterNode.completed,
                  afterNode.marker,
                  afterNode.priority,
                  afterNode.note,
                )
                await refreshOutline()
              })
              .catch(console.error)
          }
          setNodes((prev) => [...prev, afterNode])
        }
        break
      }
      case "delete": {
        // 重做删除 → 再次删除
        const beforeNode = entry.before
        if (beforeNode) {
          const sid = currentSessionIdRef.current
          if (sid !== null) {
            deleteOutlineNode(beforeNode.id, sid).catch(console.error)
          }
          setNodes((prev) => prev.filter((n) => n.id !== beforeNode.id))
        }
        break
      }
      case "update": {
        // 重做更新 → 应用新字段值
        const afterNode = entry.after
        const newFlds = entry.newFields
        if (afterNode && newFlds) {
          updateOutlineNode(
            afterNode.id,
            newFlds.content,
            newFlds.notes,
            newFlds.tags,
            newFlds.collapsed,
            newFlds.completed,
            newFlds.marker,
            newFlds.priority,
            newFlds.note,
          ).catch(console.error)
          setNodes((prev) => prev.map((n) => (n.id === afterNode.id ? afterNode : n)))
        }
        break
      }
      case "move": {
        // 重做移动 → 再次移动到新位置
        const afterNode = entry.after
        const newPid = entry.newParentId
        const newSort = entry.newSortOrder
        if (afterNode && newPid !== undefined && newSort !== undefined) {
          moveOutlineNode(afterNode.id, newPid, newSort).catch(console.error)
          setNodes((prev) =>
            prev.map((n) =>
              n.id === afterNode.id ? { ...n, parent_id: newPid, sort_order: newSort } : n,
            ),
          )
        }
        break
      }
    }

    undoStackRef.current.push(entry)
    markRevision()
  }, [refreshOutline, markRevision])

  // ── Tauri 事件监听（outline:changed） ──
  useEffect(() => {
    let unlistenFn: (() => void) | null = null

    const unlisten = listen<number>("outline:changed", (event) => {
      // 忽略由当前撤销/重做触发的事件
      if (isUndoRedoRef.current) return

      if (event.payload === currentSessionIdRef.current) {
        if (debounceTimerRef.current) {
          clearTimeout(debounceTimerRef.current)
        }
        debounceTimerRef.current = setTimeout(() => {
          refreshOutline()
        }, DEBOUNCE_EVENT_MS)
      }
    })

    unlisten.then((fn) => {
      unlistenFn = fn
    })

    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current)
      }
      unlistenFn?.()
    }
  }, [refreshOutline])

  // ── 键盘快捷键（Ctrl+Z / Ctrl+Shift+Z） ──
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "z" && !e.shiftKey) {
        e.preventDefault()
        undo()
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "z" && e.shiftKey) {
        e.preventDefault()
        redo()
      }
    }
    window.addEventListener("keydown", handler)
    return () => window.removeEventListener("keydown", handler)
  }, [undo, redo])

  // ── 清理定时器 ──
  useEffect(() => {
    return () => {
      if (saveTimerRef.current) {
        clearTimeout(saveTimerRef.current)
      }
    }
  }, [])

  // ── 上下文值 ──

  const ctx: OutlineContextValue = {
    nodes,
    tree,
    selectedNodeId,
    expandedNodeIds,
    isLoading,
    error,
    loadOutline,
    createNode,
    updateNode,
    deleteNode,
    moveNode,
    importMarkdown,
    selectNode,
    toggleExpand,
    expandAll,
    collapseAll,
    undo,
    redo,
    canUndo,
    canRedo,
  }

  return <OutlineContext.Provider value={ctx}>{children}</OutlineContext.Provider>
}
