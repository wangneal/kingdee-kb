import { Cpu } from "lucide-react"
import { useCallback, useState } from "react"
import { useKbCompilation } from "@/contexts/KbCompilationContext"
import { TOAST_AUTO_DISMISS_MS } from "@/lib/constants"

export default function KnowledgeCompilationCard() {
  const { enabled, loading, saving, setEnabled } = useKbCompilation()
  const [message, setMessage] = useState<string | null>(null)

  const handleToggle = useCallback(
    async (next: boolean) => {
      setMessage(null)
      try {
        await setEnabled(next)
        setMessage(next ? "已开启知识编译" : "已关闭知识编译")
        setTimeout(() => setMessage(null), TOAST_AUTO_DISMISS_MS)
      } catch (error) {
        setMessage(`保存失败：${error instanceof Error ? error.message : String(error)}`)
      }
    },
    [setEnabled],
  )

  return (
    <section className="rounded-xl border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-5 py-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h2 className="text-sm font-semibold text-neutral-700">知识编译</h2>
            <p className="mt-0.5 text-xs text-neutral-400">
              导入后尝试使用 LLM 生成 Wiki 候选页面；LLM 不可用或 30
              秒超时时，会降级为快速分析模式（非 LLM）。
            </p>
          </div>
          <span
            className={`flex shrink-0 items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium ${
              enabled ? "bg-green-50 text-green-700" : "bg-neutral-100 text-neutral-500"
            }`}
          >
            <span
              className={`h-1.5 w-1.5 rounded-full ${enabled ? "bg-green-500" : "bg-neutral-400"}`}
            />
            {enabled ? "已开启" : "已关闭"}
          </span>
        </div>
      </div>
      <div className="flex items-center justify-between gap-4 p-5">
        <div className="flex items-center gap-2 text-xs text-neutral-500">
          <Cpu className="h-4 w-4 text-neutral-400" />
          <span>{enabled ? "优先 LLM，失败时自动降级" : "仅执行普通入库，不生成 Wiki 候选"}</span>
        </div>
        <label className="flex items-center gap-2 text-xs text-neutral-600">
          <input
            type="checkbox"
            checked={enabled}
            disabled={loading || saving}
            onChange={(event) => handleToggle(event.target.checked)}
            className="h-4 w-4 rounded border-neutral-300 text-[#1A6BD8] focus:ring-[#1A6BD8]"
          />
          开启
        </label>
      </div>
      {message && (
        <div className="border-t border-neutral-100 px-5 py-2 text-xs text-neutral-500">
          {message}
        </div>
      )}
    </section>
  )
}
