/**
 * 大纲树渲染组件
 *
 * 递归渲染大纲节点树，处理空状态、加载状态和错误状态。
 */
import { useOutline } from "@/contexts/OutlineContext"
import OutlineNode from "./OutlineNode"

interface OutlineTreeProps {
  sessionId: number
}

export default function OutlineTree({ sessionId: _sessionId }: OutlineTreeProps) {
  const { tree, isLoading, error } = useOutline()

  // 加载状态
  if (isLoading) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-center">
        <div className="mb-3 h-5 w-5 animate-spin rounded-full border-2 border-neutral-300 border-t-[#1A6BD8]" />
        <p className="text-xs text-neutral-400">加载大纲中...</p>
      </div>
    )
  }

  // 错误状态
  if (error) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-center">
        <p className="text-xs text-red-500">加载失败：{error}</p>
      </div>
    )
  }

  // 空状态
  if (tree.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-center">
        <p className="text-sm text-neutral-400">暂无大纲节点，点击 + 添加</p>
      </div>
    )
  }

  // 渲染根节点列表，子节点由 OutlineNode 递归渲染
  return (
    <div className="outline-tree">
      {tree.map((node) => (
        <OutlineNode key={node.id} node={node} depth={0} />
      ))}
    </div>
  )
}
