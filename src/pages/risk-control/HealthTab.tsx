import { Brain, Download, Loader2, Shield } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { useToast } from "@/components/Toast"
import { useAppError } from "@/contexts/AppErrorContext"
import { formatAppError, parseAppError } from "@/lib/app-error"
import {
  analyzeFitGap,
  exportReport,
  generateRiskReport,
  getProjectHealth,
  type ProjectHealthScore,
  recordHealthMetric,
} from "@/lib/tauri-commands"

export default function HealthTab({ projectId }: { projectId: number | null }) {
  const [health, setHealth] = useState<ProjectHealthScore | null>(null)
  const [loading, setLoading] = useState(false)
  const [aiLoading, setAiLoading] = useState(false)
  const [aiReport, setAiReport] = useState("")
  const [fitGapInput, setFitGapInput] = useState("")
  const [fitGapResult, setFitGapResult] = useState("")
  const [fitGapLoading, setFitGapLoading] = useState(false)
  const [metricType, setMetricType] = useState("attendance")
  const [metricValue, setMetricValue] = useState("")
  const [metricNotes, setMetricNotes] = useState("")
  const [metricSaving, setMetricSaving] = useState(false)
  const toast = useToast()
  const { showLlmKeyError } = useAppError()
  const activeProjectRef = useRef(projectId)

  useEffect(() => {
    activeProjectRef.current = projectId
    setHealth(null)
    setAiReport("")
    setFitGapInput("")
    setFitGapResult("")
    setMetricType("attendance")
    setMetricValue("")
    setMetricNotes("")
    setLoading(false)
    setAiLoading(false)
    setFitGapLoading(false)
    setMetricSaving(false)
  }, [projectId])

  const refresh = useCallback(async () => {
    if (projectId === null) return
    setLoading(true)
    try {
      const result = await getProjectHealth(projectId)
      if (activeProjectRef.current === projectId) setHealth(result)
    } catch (e) {
      toast.error(String(e))
    }
    if (activeProjectRef.current === projectId) setLoading(false)
  }, [projectId, toast.error])

  useEffect(() => {
    refresh()
  }, [refresh])

  const handleAIAnalysis = useCallback(async () => {
    if (!health || aiLoading || projectId === null) return
    setAiLoading(true)
    setAiReport("")
    try {
      const report = await generateRiskReport(projectId, "")
      if (activeProjectRef.current === projectId) setAiReport(report)
    } catch (e) {
      const parsed = parseAppError(e)
      if (parsed?.code === "LLM_INVALID_KEY") {
        showLlmKeyError(parsed)
      } else {
        toast.error(`分析失败: ${formatAppError(e)}`)
      }
    }
    if (activeProjectRef.current === projectId) setAiLoading(false)
  }, [health, aiLoading, projectId, toast, showLlmKeyError])

  const handleRecordMetric = useCallback(async () => {
    if (projectId === null) return
    const value = Number(metricValue)
    if (!Number.isFinite(value) || value < 0 || value > 100) {
      toast.warning("指标值必须是 0 到 100 之间的数字")
      return
    }
    setMetricSaving(true)
    try {
      await recordHealthMetric(projectId, metricType, value, metricNotes.trim())
      if (activeProjectRef.current === projectId) {
        setMetricValue("")
        setMetricNotes("")
        await refresh()
      }
    } catch (e) {
      toast.error(`保存健康指标失败: ${String(e)}`)
    }
    if (activeProjectRef.current === projectId) setMetricSaving(false)
  }, [projectId, metricType, metricValue, metricNotes, refresh, toast])

  const colorClass = (level: string) =>
    level === "unknown"
      ? "text-neutral-500"
      : level === "critical"
        ? "text-red-600"
        : level === "high"
          ? "text-orange-600"
          : level === "medium"
            ? "text-yellow-600"
            : "text-green-600"

  const bgClass = (level: string) =>
    level === "unknown"
      ? "bg-neutral-50 border-neutral-200"
      : level === "critical"
        ? "bg-red-50 border-red-200"
        : level === "high"
          ? "bg-orange-50 border-orange-200"
          : level === "medium"
            ? "bg-yellow-50 border-yellow-200"
            : "bg-green-50 border-green-200"

  if (projectId === null) {
    return (
      <div className="flex flex-col items-center justify-center pt-20">
        <Shield className="mb-3 h-10 w-10 text-neutral-300" />
        <p className="text-sm text-neutral-500">请先在侧边栏选择一个项目</p>
      </div>
    )
  }

  return (
    <div>
      {loading ? (
        <div className="flex justify-center pt-10">
          <Loader2 className="h-5 w-5 animate-spin text-neutral-400" />
        </div>
      ) : health ? (
        <div className="space-y-4">
          {/* 总评分 */}
          <div className={`rounded-lg border p-6 ${bgClass(health.risk_level)}`}>
            <div className="mb-2 flex items-center gap-2">
              <Shield className={`h-5 w-5 ${colorClass(health.risk_level)}`} />
              <span className={`text-lg font-bold ${colorClass(health.risk_level)}`}>
                {health.risk_level === "unknown"
                  ? "暂无评分"
                  : `${health.overall_score.toFixed(0)}/100`}
              </span>
              <span
                className={`rounded px-2 py-0.5 text-xs font-medium ${
                  health.risk_level === "unknown"
                    ? "bg-neutral-100 text-neutral-600"
                    : health.risk_level === "critical"
                      ? "bg-red-100 text-red-700"
                      : health.risk_level === "high"
                        ? "bg-orange-100 text-orange-700"
                        : health.risk_level === "medium"
                          ? "bg-yellow-100 text-yellow-700"
                          : "bg-green-100 text-green-700"
                }`}
              >
                {health.risk_level === "unknown"
                  ? "数据不足"
                  : health.risk_level === "critical"
                    ? "危急"
                    : health.risk_level === "high"
                      ? "高风险"
                      : health.risk_level === "medium"
                        ? "关注"
                        : "健康"}
              </span>
            </div>
            <p className="text-xs text-neutral-600">{health.trend}</p>
            <p className="mt-1 text-[10px] text-neutral-500">
              健康指标完整度 {(health.data_completeness * 100).toFixed(0)}%，共{" "}
              {health.metric_count} 条记录
            </p>
            {health.alert_count > 0 && (
              <p className="mt-1 text-xs font-medium text-red-600">
                ⚠ {health.alert_count} 项指标需要关注
              </p>
            )}
          </div>

          {/* 各维度 */}
          <div className="grid gap-3 sm:grid-cols-2">
            {health.dimensions.map((d) => (
              <div key={d.name} className="rounded-lg border border-neutral-200 bg-white p-4">
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-xs font-medium text-neutral-700">{d.name}</span>
                  <span
                    className={`text-xs font-bold ${
                      !d.has_data
                        ? "text-neutral-400"
                        : d.score >= 50
                          ? "text-red-600"
                          : d.score >= 30
                            ? "text-yellow-600"
                            : "text-green-600"
                    }`}
                  >
                    {d.has_data ? `${d.score.toFixed(0)}/100` : "暂无数据"}
                  </span>
                </div>
                <div className="h-2 rounded-full bg-neutral-100">
                  <div
                    className={`h-full rounded-full transition-all ${
                      !d.has_data
                        ? "bg-neutral-200"
                        : d.score >= 50
                          ? "bg-red-500"
                          : d.score >= 30
                            ? "bg-yellow-500"
                            : "bg-green-500"
                    }`}
                    style={{ width: d.has_data ? `${d.score}%` : "0%" }}
                  />
                </div>
                <p className="mt-1 text-[10px] text-neutral-400">{d.detail}</p>
              </div>
            ))}
          </div>

          <div className="rounded-lg border border-neutral-200 bg-white p-4">
            <h2 className="mb-3 text-sm font-semibold text-neutral-700">录入健康指标</h2>
            <div className="grid gap-3 md:grid-cols-[180px_120px_1fr_auto]">
              <select
                value={metricType}
                onChange={(e) => setMetricType(e.target.value)}
                className="rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
              >
                <option value="attendance">客户关键岗位缺席率</option>
                <option value="data_delay">期初数据延迟</option>
                <option value="issue_count">未解决问题积压</option>
                <option value="sentiment">客户配合度</option>
              </select>
              <input
                type="number"
                min="0"
                max="100"
                value={metricValue}
                onChange={(e) => setMetricValue(e.target.value)}
                placeholder="0-100"
                className="rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
              />
              <input
                value={metricNotes}
                onChange={(e) => setMetricNotes(e.target.value)}
                placeholder="记录依据或备注"
                className="rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
              />
              <button
                type="button"
                onClick={handleRecordMetric}
                disabled={metricSaving || !metricValue.trim()}
                className="flex items-center justify-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
              >
                {metricSaving ? <Loader2 className="h-3 w-3 animate-spin" /> : null}
                保存指标
              </button>
            </div>
            <p className="mt-2 text-[10px] text-neutral-400">
              指标值越高表示风险越高，保存后自动刷新项目健康度。
            </p>
          </div>

          <button
            type="button"
            onClick={handleAIAnalysis}
            disabled={aiLoading}
            className="flex w-full items-center justify-center gap-1.5 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50 transition-colors"
          >
            {aiLoading ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Brain className="h-3.5 w-3.5" />
            )}
            {aiLoading ? "正在检索项目文档并分析..." : "基于项目文档进行 AI 风险分析"}
          </button>
          {aiReport && (
            <div className="mt-2 space-y-2">
              <div className="rounded-lg border border-amber-100 bg-amber-50 p-3 text-xs leading-relaxed text-neutral-700 prose prose-sm prose-amber max-w-none">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{aiReport}</ReactMarkdown>
              </div>
              <button
                type="button"
                onClick={async () => {
                  try {
                    const { save } = await import("@tauri-apps/plugin-dialog")
                    const path = await save({ filters: [{ name: "Markdown", extensions: ["md"] }] })
                    if (path) await exportReport(aiReport, path)
                  } catch (e) {
                    toast.error(`导出失败: ${String(e)}`)
                  }
                }}
                className="flex items-center gap-1 rounded bg-neutral-100 px-2 py-1 text-[10px] text-neutral-500 hover:bg-neutral-200"
              >
                <Download className="h-3 w-3" />
                导出报告
              </button>
            </div>
          )}
        </div>
      ) : (
        <p className="text-center text-sm text-neutral-400">加载失败</p>
      )}

      {/* Fit-Gap 分析 */}
      <div className="mt-6 rounded-lg border border-neutral-200 bg-white p-4">
        <h2 className="mb-3 text-sm font-semibold text-neutral-700">Fit-Gap 差异分析</h2>
        <textarea
          value={fitGapInput}
          onChange={(e) => setFitGapInput(e.target.value)}
          rows={3}
          placeholder="输入需求列表，每行一条，如：&#10;1. 总账模块支持多币种&#10;2. 需要定制化报表引擎"
          className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
        />
        <button
          type="button"
          onClick={async () => {
            if (!fitGapInput.trim()) return
            setFitGapLoading(true)
            try {
              const result = await analyzeFitGap(projectId, fitGapInput)
              if (activeProjectRef.current === projectId) setFitGapResult(result)
            } catch (e) {
              const parsed = parseAppError(e)
              if (parsed?.code === "LLM_INVALID_KEY") {
                showLlmKeyError(parsed)
                if (activeProjectRef.current === projectId) {
                  setFitGapResult("LLM API Key 失效，请配置后重试")
                }
                return
              }
              if (activeProjectRef.current === projectId) {
                setFitGapResult(`分析失败: ${formatAppError(e)}`)
              }
            }
            if (activeProjectRef.current === projectId) setFitGapLoading(false)
          }}
          disabled={fitGapLoading || !fitGapInput.trim()}
          className="mt-2 flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
        >
          {fitGapLoading ? <Loader2 className="h-3 w-3 animate-spin" /> : null}
          {fitGapLoading ? "分析中..." : "开始分析"}
        </button>
        {fitGapResult && (
          <div className="mt-3 space-y-2">
            <div className="rounded-lg border border-neutral-100 bg-neutral-50 p-3">
              <pre className="text-xs leading-relaxed text-neutral-700 whitespace-pre-wrap font-sans">
                {fitGapResult}
              </pre>
            </div>
            <div className="flex justify-end">
              <button
                type="button"
                onClick={async () => {
                  try {
                    const { save } = await import("@tauri-apps/plugin-dialog")
                    const path = await save({ filters: [{ name: "Markdown", extensions: ["md"] }] })
                    if (path) await exportReport(fitGapResult, path)
                  } catch (e) {
                    toast.error(`导出失败: ${String(e)}`)
                  }
                }}
                className="flex items-center gap-1 rounded bg-neutral-100 px-2 py-1 text-[10px] text-neutral-500 hover:bg-neutral-200"
              >
                <Download className="h-3 w-3" />
                导出分析
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
