/**
 * ImportFeedback — 导入反馈提示区域
 *
 * 展示成功/失败/加载中的状态信息。
 */

import { AlertCircle, CheckCircle2, Loader2 } from "lucide-react"
import type { ImportStatus } from "./types"

interface ImportFeedbackProps {
  status: ImportStatus
  message: string
}

export default function ImportFeedback({ status, message }: ImportFeedbackProps) {
  const bgClass =
    status === "success"
      ? "bg-green-50 text-green-700"
      : status === "error"
        ? "bg-red-50 text-red-700"
        : "bg-blue-50 text-blue-700"

  const icon =
    status === "success" ? (
      <CheckCircle2 className="h-4 w-4 shrink-0" />
    ) : status === "error" ? (
      <AlertCircle className="h-4 w-4 shrink-0" />
    ) : (
      <Loader2 className="h-4 w-4 shrink-0 animate-spin" />
    )

  return (
    <div className={`mt-4 flex items-center gap-2 rounded-md p-3 text-sm ${bgClass}`}>
      {icon}
      <span>{message}</span>
    </div>
  )
}
