import { Download, Loader2, Send } from "lucide-react"
import { useEffect, useRef, useState } from "react"
import { useToast } from "@/components/Toast"
import { useAppError } from "@/contexts/AppErrorContext"
import { formatAppError, parseAppError } from "@/lib/app-error"
import { type DefenseScriptResult, exportReport, generateDefenseScript } from "@/lib/tauri-commands"

export default function ScriptsTab({ projectId }: { projectId: number | null }) {
  const [scenario, setScenario] = useState("")
  const [context, setContext] = useState("")
  const [tone, setTone] = useState("push_back")
  const [result, setResult] = useState<DefenseScriptResult | null>(null)
  const [loading, setLoading] = useState(false)
  const toast = useToast()
  const { showLlmKeyError } = useAppError()
  const activeProjectRef = useRef(projectId)

  useEffect(() => {
    activeProjectRef.current = projectId
    setScenario("")
    setContext("")
    setResult(null)
    setLoading(false)
  }, [projectId])

  const handleGenerate = async () => {
    if (!scenario.trim() || projectId === null) return
    setLoading(true)
    try {
      const r = await generateDefenseScript(projectId, {
        scenario: scenario.trim(),
        context: context.trim(),
        tone,
      })
      if (activeProjectRef.current === projectId) setResult(r)
    } catch (e) {
      const parsed = parseAppError(e)
      if (parsed?.code === "LLM_INVALID_KEY") {
        showLlmKeyError(parsed)
      } else {
        toast.error(formatAppError(e))
      }
    }
    if (activeProjectRef.current === projectId) setLoading(false)
  }

  return (
    <div className="space-y-4">
      {projectId === null && (
        <div className="rounded-lg border border-neutral-200 bg-neutral-50 p-4 text-xs text-neutral-500">
          请先在侧边栏选择一个项目
        </div>
      )}
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <h2 className="mb-3 text-sm font-semibold text-neutral-700">生成沟通话术</h2>
        <div className="space-y-3">
          <div>
            <label
              htmlFor="risk-script-scenario"
              className="mb-1 block text-[10px] font-medium text-neutral-500"
            >
              场景描述
            </label>
            <textarea
              id="risk-script-scenario"
              value={scenario}
              onChange={(e) => setScenario(e.target.value)}
              rows={2}
              placeholder="如：客户要求在合同范围外增加一个全新的报表模块"
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
            />
          </div>
          <div>
            <label
              htmlFor="risk-script-context"
              className="mb-1 block text-[10px] font-medium text-neutral-500"
            >
              上下文（可选）
            </label>
            <textarea
              id="risk-script-context"
              value={context}
              onChange={(e) => setContext(e.target.value)}
              rows={2}
              placeholder="补充背景信息..."
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
            />
          </div>
          <div className="flex items-center gap-3">
            <label htmlFor="risk-script-tone" className="text-[10px] font-medium text-neutral-500">
              沟通基调
            </label>
            <select
              id="risk-script-tone"
              value={tone}
              onChange={(e) => setTone(e.target.value)}
              className="rounded-lg border border-neutral-200 px-2 py-1 text-xs outline-none"
            >
              <option value="push_back">委婉拒绝</option>
              <option value="guide">引导说服</option>
              <option value="escalate">升级讨论</option>
            </select>
            <button
              type="button"
              onClick={handleGenerate}
              disabled={loading || !scenario.trim() || projectId === null}
              className="ml-auto flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
            >
              {loading ? (
                <Loader2 className="h-3 w-3 animate-spin" />
              ) : (
                <Send className="h-3 w-3" />
              )}
              生成话术
            </button>
          </div>
        </div>
      </div>

      {result && (
        <div className="space-y-3">
          <p className="text-xs font-semibold text-neutral-700">{result.scenario_label}</p>
          {result.scripts.map((s) => (
            <div
              key={`${s.phase}-${s.content}-${s.tip}`}
              className="rounded-lg border border-amber-100 bg-amber-50 p-4"
            >
              <span className="mb-1 inline-block rounded bg-amber-200 px-2 py-0.5 text-[10px] font-medium text-amber-800">
                {s.phase}
              </span>
              <p className="text-sm leading-relaxed text-neutral-700">{s.content}</p>
              <p className="mt-1 text-[10px] italic text-amber-700">💡 {s.tip}</p>
            </div>
          ))}
          {result && (
            <div className="flex justify-end pt-1">
              <button
                type="button"
                onClick={async () => {
                  const md = `# 沟通话术\n\n## ${result.scenario_label}\n\n${result.scripts.map((s) => `### ${s.phase}\n\n${s.content}\n\n> 💡 ${s.tip}\n`).join("\n")}`
                  try {
                    const { save } = await import("@tauri-apps/plugin-dialog")
                    const path = await save({ filters: [{ name: "Markdown", extensions: ["md"] }] })
                    if (path) await exportReport(md, path)
                  } catch (e) {
                    toast.error(`导出失败: ${String(e)}`)
                  }
                }}
                className="flex items-center gap-1 rounded bg-neutral-100 px-2 py-1 text-[10px] text-neutral-500 hover:bg-neutral-200"
              >
                <Download className="h-3 w-3" />
                导出话术
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
