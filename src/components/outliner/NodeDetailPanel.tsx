/**
 * 节点详情侧边面板
 *
 * 展示选中节点的详细信息：备注、标签、关联问答、元数据、导出。
 */

import { Download, FileText, Hash, Link2, List } from "lucide-react"
import { useCallback, useEffect, useMemo, useState } from "react"
import { type TreeNode, useOutline } from "../../contexts/OutlineContext"
import { exportOutline } from "../../lib/outline-commands"
import { useToast } from "../Toast"

interface NodeDetailPanelProps {
  node: TreeNode | null
  sessionId: number
}

export default function NodeDetailPanel({ node, sessionId }: NodeDetailPanelProps) {
  const { updateNode } = useOutline()
  const toast = useToast()

  // ── 备注编辑状态 ──
  const [notes, setNotes] = useState(node?.notes ?? "")
  const [tagsInput, setTagsInput] = useState("")

  // 同步选中节点变化
  useEffect(() => {
    if (node) {
      setNotes(node.notes ?? "")
      // 解析 JSON 标签数组为逗号分隔字符串
      try {
        const parsed = JSON.parse(node.tags) as string[]
        setTagsInput(parsed.join(", "))
      } catch {
        setTagsInput("")
      }
    }
  }, [node])

  // ── 解析标签 ──
  const tags = useMemo(() => {
    if (!node) return []
    try {
      return JSON.parse(node.tags) as string[]
    } catch {
      return []
    }
  }, [node])

  // ── 保存备注 ──
  const handleNotesBlur = useCallback(async () => {
    if (!node) return
    if (notes !== (node.notes ?? "")) {
      await updateNode(node.id, { notes })
    }
  }, [node, notes, updateNode])

  // ── 保存标签 ──
  const handleTagsBlur = useCallback(async () => {
    if (!node) return
    const parsed = tagsInput
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean)
    const json = JSON.stringify(parsed)
    if (json !== node.tags) {
      await updateNode(node.id, { tags: json })
    }
  }, [node, tagsInput, updateNode])

  // ── 导出 ──
  const handleExport = useCallback(
    async (format: string) => {
      try {
        const content = await exportOutline(sessionId, format)
        // 复制到剪贴板
        await navigator.clipboard.writeText(content)
        toast.success("已复制到剪贴板")
      } catch (err) {
        toast.error(`导出失败: ${String(err)}`)
      }
    },
    [sessionId, toast],
  )

  // ── 无选中节点时的占位 ──
  if (!node) {
    return (
      <div className="flex h-full flex-col items-center justify-center p-6 text-center">
        <FileText className="mb-3 h-8 w-8 text-neutral-200" />
        <p className="text-xs text-neutral-400">选择一个节点查看详情</p>
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col overflow-y-auto p-4">
      {/* 节点标题 */}
      <div className="mb-4">
        <h3 className="text-sm font-semibold text-neutral-800 line-clamp-2">
          {node.content || "空节点"}
        </h3>
        {node.completed && (
          <span className="mt-1 inline-block rounded bg-green-100 px-1.5 py-0.5 text-[10px] text-green-700">
            已完成
          </span>
        )}
      </div>

      {/* 备注 */}
      <div className="mb-4">
        <label htmlFor="node-notes" className="mb-1 block text-xs font-medium text-neutral-600">
          备注
        </label>
        <textarea
          id="node-notes"
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          onBlur={handleNotesBlur}
          placeholder="添加备注..."
          rows={4}
          className="w-full resize-none rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
        />
      </div>

      {/* 标签 */}
      <div className="mb-4">
        <label htmlFor="node-tags" className="mb-1 block text-xs font-medium text-neutral-600">
          标签 <span className="text-neutral-400">（逗号分隔）</span>
        </label>
        <input
          id="node-tags"
          value={tagsInput}
          onChange={(e) => setTagsInput(e.target.value)}
          onBlur={handleTagsBlur}
          placeholder="标签1, 标签2, ..."
          className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
        />
        {tags.length > 0 && (
          <div className="mt-1.5 flex flex-wrap gap-1">
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
      </div>

      {/* Q&A 关联 */}
      <div className="mb-4 rounded-lg border border-neutral-200 bg-neutral-50 p-3">
        <div className="flex items-center gap-2">
          <Link2 className="h-3.5 w-3.5 text-neutral-400" />
          <span className="text-xs font-medium text-neutral-600">问答关联</span>
        </div>
        {node.question_id ? (
          <p className="mt-1 text-[10px] text-neutral-500">已关联问答记录 #{node.question_id}</p>
        ) : (
          <p className="mt-1 text-[10px] text-neutral-400">暂未关联问答记录</p>
        )}
      </div>

      {/* 节点信息 */}
      <div className="mb-4 space-y-1 text-[10px] text-neutral-400">
        <div className="flex items-center gap-1.5">
          <Hash className="h-3 w-3" />
          <span>ID: {node.id}</span>
        </div>
        <div>创建: {node.created_at}</div>
        <div>更新: {node.updated_at}</div>
      </div>

      {/* 导出按钮 */}
      <div className="mt-auto border-t border-neutral-200 pt-4">
        <p className="mb-2 text-xs font-medium text-neutral-600">导出大纲</p>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => handleExport("markdown_list")}
            className="flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-neutral-200 px-3 py-2 text-xs text-neutral-600 hover:bg-neutral-50 transition-colors"
          >
            <List className="h-3.5 w-3.5" />
            列表
          </button>
          <button
            type="button"
            onClick={() => handleExport("markdown_headings")}
            className="flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-neutral-200 px-3 py-2 text-xs text-neutral-600 hover:bg-neutral-50 transition-colors"
          >
            <Download className="h-3.5 w-3.5" />
            标题
          </button>
        </div>
      </div>
    </div>
  )
}
