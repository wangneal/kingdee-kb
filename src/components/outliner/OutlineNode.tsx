/**
 * 大纲节点组件
 *
 * 只读节点展示：选择、展开/收起、键盘导航、完成状态切换、语音按钮。
 * 内容编辑请使用右侧 Markdown 编辑器。
 */

import {
  CheckCircle,
  ChevronDown,
  ChevronRight,
  Circle,
  GripVertical,
  Mic,
  Trash2,
} from "lucide-react"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { useAudio } from "../../contexts/AudioContext"
import { type TreeNode, useOutline } from "../../contexts/OutlineContext"

interface OutlineNodeProps {
  node: TreeNode
  depth: number
}

export default function OutlineNode({ node, depth }: OutlineNodeProps) {
  const {
    nodes,
    tree,
    selectedNodeId,
    expandedNodeIds,
    selectNode,
    toggleExpand,
    createNode,
    updateNode,
    deleteNode,
    moveNode,
  } = useOutline()

  const { status, startAudioRecording, stopAudioRecording } = useAudio()
  const [isRecording, setRecording] = useState(false)
  const containerRef = useRef<HTMLDivElement>(null)

  const isSelected = selectedNodeId === node.id
  const isExpanded = expandedNodeIds.has(node.id)
  const hasChildren = node.children.length > 0

  // ── 辅助：获取可见节点的扁平列表（用于键盘导航） ──
  const getVisibleNodeIds = useCallback((): number[] => {
    const ids: number[] = []
    const walk = (nodes: TreeNode[]) => {
      for (const n of nodes) {
        ids.push(n.id)
        if (expandedNodeIds.has(n.id) && n.children.length > 0) {
          walk(n.children)
        }
      }
    }
    walk(tree)
    return ids
  }, [tree, expandedNodeIds])

  // ── 选择 ──
  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      selectNode(node.id)
    },
    [node.id, selectNode],
  )

  // ── 展开/收起 ──
  const handleToggle = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      toggleExpand(node.id)
    },
    [node.id, toggleExpand],
  )

  // ── 完成状态切换 ──
  const handleToggleCompleted = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      updateNode(node.id, { completed: !node.completed })
    },
    [node.id, node.completed, updateNode],
  )

  // ── 删除节点 ──
  const handleDelete = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      if (
        window.confirm(`确认删除节点"${node.content}"？${hasChildren ? "子节点也将被删除。" : ""}`)
      ) {
        deleteNode(node.id)
      }
    },
    [node.id, node.content, hasChildren, deleteNode],
  )

  // ── 语音输入 ──
  const handleVoice = useCallback(
    async (e: React.MouseEvent) => {
      e.stopPropagation()
      if (status === "idle" || status === "error") {
        await startAudioRecording()
        setRecording(true)
      } else if (status === "recording") {
        setRecording(false)
        const text = await stopAudioRecording()
        if (text) {
          const newContent = node.content ? `${node.content}\n${text}` : text
          await updateNode(node.id, { content: newContent })
        }
      }
    },
    [status, startAudioRecording, stopAudioRecording, node.id, node.content, updateNode],
  )

  // ── 键盘导航 ──
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      const visibleIds = getVisibleNodeIds()
      const currentIdx = visibleIds.indexOf(node.id)

      switch (e.key) {
        case "ArrowUp": {
          e.preventDefault()
          if (currentIdx > 0) {
            selectNode(visibleIds[currentIdx - 1])
          }
          break
        }
        case "ArrowDown": {
          e.preventDefault()
          if (currentIdx < visibleIds.length - 1) {
            selectNode(visibleIds[currentIdx + 1])
          }
          break
        }
        case "ArrowLeft": {
          e.preventDefault()
          if (isExpanded && hasChildren) {
            toggleExpand(node.id)
          }
          break
        }
        case "ArrowRight": {
          e.preventDefault()
          if (!isExpanded && hasChildren) {
            toggleExpand(node.id)
          }
          break
        }
        case "Enter": {
          e.preventDefault()
          createNode(node.parent_id, "").then((newNode) => {
            if (newNode) selectNode(newNode.id)
          })
          break
        }
        case "Tab": {
          e.preventDefault()
          if (e.shiftKey) {
            if (node.parent_id !== null) {
              const parentNode = nodes.find((n) => n.id === node.parent_id)
              if (parentNode) {
                const targetSiblings = [...nodes]
                  .filter((n) => n.parent_id === parentNode.parent_id && n.id !== node.id)
                  .sort((a, b) => a.sort_order - b.sort_order || a.id - b.id)
                const parentIndex = targetSiblings.findIndex((n) => n.id === parentNode.id)
                const nextSibling = parentIndex >= 0 ? targetSiblings[parentIndex + 1] : undefined
                const newOrder = !nextSibling
                  ? parentNode.sort_order + 1
                  : (parentNode.sort_order + nextSibling.sort_order) / 2
                moveNode(node.id, parentNode.parent_id, newOrder)
              }
            }
          } else {
            const siblings = [...nodes]
              .filter((n) => n.parent_id === node.parent_id)
              .sort((a, b) => a.sort_order - b.sort_order || a.id - b.id)
            const idx = siblings.findIndex((n) => n.id === node.id)
            if (idx > 0) {
              const prevSibling = siblings[idx - 1]
              const targetChildren = [...nodes]
                .filter((n) => n.parent_id === prevSibling.id && n.id !== node.id)
                .sort((a, b) => a.sort_order - b.sort_order || a.id - b.id)
              const lastChild = targetChildren[targetChildren.length - 1]
              moveNode(node.id, prevSibling.id, lastChild ? lastChild.sort_order + 1 : 1)
            }
          }
          break
        }
        case "Delete":
        case "Backspace": {
          e.preventDefault()
          if (
            window.confirm(
              `确认删除节点"${node.content}"？${hasChildren ? "子节点也将被删除。" : ""}`,
            )
          ) {
            deleteNode(node.id)
          }
          break
        }
      }
    },
    [
      node,
      isExpanded,
      hasChildren,
      getVisibleNodeIds,
      selectNode,
      toggleExpand,
      createNode,
      moveNode,
      deleteNode,
      nodes,
    ],
  )

  // ── 选中时自动聚焦 ──
  useEffect(() => {
    if (isSelected && containerRef.current) {
      containerRef.current.focus()
    }
  }, [isSelected])

  // ── 解析标签 ──
  const tags = useMemo(() => {
    try {
      return JSON.parse(node.tags) as string[]
    } catch {
      return []
    }
  }, [node.tags])

  return (
    <div>
      <div
        ref={containerRef}
        role="treeitem"
        aria-selected={isSelected}
        aria-expanded={hasChildren ? isExpanded : undefined}
        tabIndex={isSelected ? 0 : -1}
        onClick={handleClick}
        onKeyDown={handleKeyDown}
        className={`group flex items-center gap-1 rounded-md px-2 py-1 text-sm transition-colors outline-none cursor-pointer ${
          isSelected ? "bg-[#1A6BD8]/10 text-[#1A6BD8]" : "text-neutral-700 hover:bg-neutral-100"
        } ${node.completed ? "opacity-60" : ""}`}
        style={{ paddingLeft: `${depth * 20 + 8}px` }}
      >
        {/* 展开/收起按钮 */}
        {hasChildren ? (
          <button
            type="button"
            onClick={handleToggle}
            className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-neutral-400 hover:text-neutral-600 transition-colors"
          >
            {isExpanded ? (
              <ChevronDown className="h-3.5 w-3.5" />
            ) : (
              <ChevronRight className="h-3.5 w-3.5" />
            )}
          </button>
        ) : (
          <span className="h-4 w-4 shrink-0" />
        )}

        {/* 拖拽手柄 */}
        <GripVertical className="h-3.5 w-3.5 shrink-0 cursor-grab text-neutral-300 opacity-0 group-hover:opacity-100 transition-opacity" />

        {/* 节点标题（只读） */}
        <span
          className={`min-w-0 flex-1 truncate ${
            node.completed ? "line-through text-neutral-400" : ""
          }`}
        >
          {node.content || <span className="italic text-neutral-300">空节点</span>}
        </span>

        {/* 标签 */}
        {tags.length > 0 && (
          <div className="flex shrink-0 gap-1">
            {tags.map((tag) => (
              <span
                key={tag}
                className="rounded bg-blue-50 px-1.5 py-0.5 text-[10px] text-blue-600"
              >
                {tag}
              </span>
            ))}
          </div>
        )}

        {/* 完成状态按钮 */}
        <button
          type="button"
          onClick={handleToggleCompleted}
          className="shrink-0 rounded p-0.5 text-neutral-300 hover:text-green-500 transition-colors"
          title={node.completed ? "标记为未完成" : "标记为已完成"}
        >
          {node.completed ? (
            <CheckCircle className="h-4 w-4 text-green-500" />
          ) : (
            <Circle className="h-4 w-4" />
          )}
        </button>

        {/* 语音按钮 */}
        <button
          type="button"
          onClick={handleVoice}
          disabled={status === "transcribing"}
          className={`shrink-0 rounded p-0.5 transition-all ${
            isRecording
              ? "text-red-500 opacity-100 animate-pulse"
              : status === "transcribing"
                ? "text-amber-400 opacity-100"
                : "text-neutral-300 opacity-0 group-hover:opacity-100 hover:text-[#1A6BD8]"
          }`}
          title={isRecording ? "点击停止录音" : status === "transcribing" ? "转录中..." : "语音输入"}
        >
          <Mic className="h-3.5 w-3.5" />
        </button>

        {/* 删除按钮 */}
        <button
          type="button"
          onClick={handleDelete}
          className="shrink-0 rounded p-0.5 text-neutral-300 opacity-0 group-hover:opacity-100 hover:text-red-500 transition-all"
          title="删除节点"
        >
          <Trash2 className="h-3.5 w-3.5" />
        </button>
      </div>

      {/* 子节点递归 */}
      {hasChildren && isExpanded && (
        <div role="group">
          {node.children.map((child) => (
            <OutlineNode key={child.id} node={child} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  )
}
