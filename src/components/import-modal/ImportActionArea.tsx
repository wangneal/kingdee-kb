/**
 * ImportActionArea — 文件/文件夹通用的选择按钮区域
 *
 * 渲染一个虚线边框的点击选择区域，支持 loading 状态。
 */

import { Loader2 } from "lucide-react"
import type { LucideIcon } from "lucide-react"

interface ImportActionAreaProps {
  /** 支持的格式说明文本 */
  hint?: string
  /** 按钮图标 */
  icon: LucideIcon
  /** 按钮文字 */
  buttonText: string
  /** 点击回调 */
  onClick: () => void
  /** 是否加载中 */
  loading: boolean
}

export default function ImportActionArea({
  hint,
  icon: Icon,
  buttonText,
  onClick,
  loading,
}: ImportActionAreaProps) {
  return (
    <div className="space-y-3">
      {hint && <p className="text-xs text-neutral-500">{hint}</p>}
      <button
        type="button"
        onClick={onClick}
        disabled={loading}
        className="flex w-full items-center justify-center gap-2 rounded-lg border-2 border-dashed border-neutral-200 px-4 py-8 text-sm text-neutral-500 transition-colors hover:border-[#1A6BD8] hover:text-[#1A6BD8] disabled:opacity-50"
      >
        {loading ? (
          <Loader2 className="h-5 w-5 animate-spin" />
        ) : (
          <Icon className="h-5 w-5" />
        )}
        {buttonText}
      </button>
    </div>
  )
}
