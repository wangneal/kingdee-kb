import {
  AlertCircle,
  AlertTriangle,
  BookOpen,
  Brain,
  CheckCircle,
  Download,
  FileUp,
  Loader2,
  Plus,
  Search,
  Send,
  Shield,
  ShieldAlert,
  Trash2,
} from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { useToast } from "../components/Toast"
import { DEFAULT_SLOT, useAgent } from "../contexts/AgentContext"
import { useProject } from "../contexts/ProjectContext"
import {
  addScopeItem,
  analyzeFitGap,
  type CandidateScopeItem,
  type ContractScopeItem,
  checkScopeCreep,
  confirmScopeItems,
  type DefenseScriptResult,
  deleteScopeItem,
  type DocumentMeta,
  exportReport,
  extractScopeFromDocument,
  generateDefenseScript,
  generateRiskReport,
  getProjectHealth,
  listDocuments,
  listScopeItems,
  type ProjectHealthScore,
  type ScopeCreepResult,
} from "../lib/tauri-commands"

type Tab = "scope" | "health" | "scripts" | "analysis"

export default function RiskControl() {
  const { currentProjectId, currentProject, loading: projectLoading } = useProject()
  const [tab, setTab] = useState<Tab>("scope")
  const activeProjectId = currentProjectId

  const tabs: { key: Tab; label: string; icon: typeof Shield }[] = [
    { key: "scope", label: "需求蔓延警报", icon: AlertTriangle },
    { key: "health", label: "项目健康度", icon: Shield },
    { key: "scripts", label: "防身话术库", icon: BookOpen },
    { key: "analysis", label: "AI 深度分析", icon: Brain },
  ]

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <ShieldAlert className="h-5 w-5 text-amber-600" />
          <h1 className="text-base font-semibold text-neutral-800">双轨风险把控舱</h1>
        </div>
        <p className="text-xs font-medium text-neutral-500">
          当前项目：{projectLoading ? "加载中" : currentProject?.name ?? "未选择项目"}
        </p>
      </div>

      <div className="flex border-b border-neutral-200 bg-white px-6">
        {tabs.map(({ key, label, icon: Icon }) => (
          <button
            key={key}
            type="button"
            onClick={() => setTab(key)}
            className={`flex items-center gap-1.5 border-b-2 px-4 py-2.5 text-xs font-medium transition-colors ${
              tab === key
                ? "border-amber-500 text-amber-700"
                : "border-transparent text-neutral-500 hover:text-neutral-700"
            }`}
          >
            <Icon className="h-3.5 w-3.5" />
            {label}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto p-6">
        {tab === "scope" && <ScopeTab projectId={activeProjectId} />}
        {tab === "health" && <HealthTab projectId={activeProjectId} />}
        {tab === "scripts" && <ScriptsTab projectId={activeProjectId} />}
        {tab === "analysis" && (
          <AnalysisTab projectId={activeProjectId} />
        )}
      </div>
    </div>
  )
}

function ScopeTab({ projectId }: { projectId: number | null }) {
  const [items, setItems] = useState<ContractScopeItem[]>([])
  const [checkResult, setCheckResult] = useState<ScopeCreepResult | null>(null)
  const [loading, setLoading] = useState(false)
  const [newReq, setNewReq] = useState("")
  const [newCat, setNewCat] = useState("")
  const [newDesc, setNewDesc] = useState("")
  const [showExtract, setShowExtract] = useState(false)
  const [documents, setDocuments] = useState<DocumentMeta[]>([])
  const [docLoading, setDocLoading] = useState(false)
  const [extractDocId, setExtractDocId] = useState("")
  const [extractLoading, setExtractLoading] = useState(false)
  const [candidates, setCandidates] = useState<CandidateScopeItem[]>([])
  const [confirmLoading, setConfirmLoading] = useState(false)
  const toast = useToast()
  const activeProjectRef = useRef(projectId)

  useEffect(() => {
    activeProjectRef.current = projectId
    setItems([])
    setCheckResult(null)
    setNewReq("")
    setShowExtract(false)
    setDocuments([])
    setExtractDocId("")
    setCandidates([])
    setLoading(false)
    setDocLoading(false)
    setExtractLoading(false)
    setConfirmLoading(false)
  }, [projectId])

  const refresh = useCallback(async () => {
    if (projectId === null) return
    try {
      const list = await listScopeItems(projectId)
      if (activeProjectRef.current === projectId) setItems(list)
    } catch (e) {
      console.error("加载范围列表失败:", e)
    }
  }, [projectId])

  useEffect(() => {
    refresh()
  }, [refresh])

  // 打开提取对话框时加载文档列表
  useEffect(() => {
    if (!showExtract || projectId === null) {
      if (!showExtract) {
        setExtractDocId("")
        setCandidates([])
      }
      return
    }
    setDocLoading(true)
    listDocuments(projectId)
      .then((result) => {
        if (activeProjectRef.current === projectId) setDocuments(result)
      })
      .catch(() => {
        if (activeProjectRef.current === projectId) setDocuments([])
      })
      .finally(() => {
        if (activeProjectRef.current === projectId) setDocLoading(false)
      })
  }, [showExtract, projectId])

  const handleCheck = async () => {
    if (!newReq.trim() || projectId === null) return
    setLoading(true)
    try {
      const r = await checkScopeCreep(projectId, newReq.trim())
      if (activeProjectRef.current === projectId) setCheckResult(r)
    } catch (e) {
      toast.error(String(e))
    }
    if (activeProjectRef.current === projectId) setLoading(false)
  }

  const handleAdd = async () => {
    if (!newCat.trim() || !newDesc.trim() || projectId === null) return
    await addScopeItem(projectId, newCat.trim(), newDesc.trim(), true, "")
    if (activeProjectRef.current === projectId) {
      setNewCat("")
      setNewDesc("")
      refresh()
    }
  }

  const handleExtract = async () => {
    if (!extractDocId.trim() || projectId === null) return
    setExtractLoading(true)
    setCandidates([])
    try {
      const result = await extractScopeFromDocument(projectId, Number(extractDocId))
      if (activeProjectRef.current === projectId) setCandidates(result)
    } catch (e) {
      toast.error("提取失败: " + String(e))
    }
    if (activeProjectRef.current === projectId) setExtractLoading(false)
  }

  const handleConfirmImport = async () => {
    if (projectId === null || candidates.length === 0) return
    setConfirmLoading(true)
    try {
      await confirmScopeItems(projectId, candidates)
      if (activeProjectRef.current === projectId) {
        setCandidates([])
        setShowExtract(false)
        setExtractDocId("")
        await refresh()
      }
    } catch (e) {
      toast.error("导入失败: " + String(e))
    }
    if (activeProjectRef.current === projectId) setConfirmLoading(false)
  }

  if (projectId === null) {
    return (
      <div className="flex flex-col items-center justify-center pt-20">
        <Search className="mb-3 h-10 w-10 text-neutral-300" />
        <p className="text-sm text-neutral-500">请先在侧边栏选择一个项目</p>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* 范围检查 */}
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <h2 className="mb-3 text-sm font-semibold text-neutral-700">检查新需求是否超范围</h2>
        <div className="flex gap-2">
          <input
            value={newReq}
            onChange={(e) => setNewReq(e.target.value)}
            placeholder="输入新需求描述..."
            className="flex-1 rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
          />
          <button
            type="button"
            onClick={handleCheck}
            disabled={loading || !newReq.trim()}
            className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
          >
            {loading ? <Loader2 className="h-3 w-3 animate-spin" /> : <Send className="h-3 w-3" />}
            检查
          </button>
        </div>
        {checkResult && (
          <div
            className={`mt-3 rounded-lg border p-3 ${
              checkResult.risk_level === "red"
                ? "border-red-200 bg-red-50"
                : checkResult.risk_level === "yellow"
                  ? "border-yellow-200 bg-yellow-50"
                  : "border-green-200 bg-green-50"
            }`}
          >
            <div className="mb-1 flex items-center gap-2">
              {checkResult.risk_level === "red" ? (
                <AlertCircle className="h-4 w-4 text-red-600" />
              ) : checkResult.risk_level === "yellow" ? (
                <AlertTriangle className="h-4 w-4 text-yellow-600" />
              ) : (
                <CheckCircle className="h-4 w-4 text-green-600" />
              )}
              <span
                className={`text-xs font-semibold ${
                  checkResult.risk_level === "red"
                    ? "text-red-700"
                    : checkResult.risk_level === "yellow"
                      ? "text-yellow-700"
                      : "text-green-700"
                }`}
              >
                {checkResult.risk_label}
              </span>
            </div>
            <p className="text-xs text-neutral-600">{checkResult.explanation}</p>
            <p className="mt-1 text-xs font-medium text-neutral-700">
              建议：{checkResult.suggestion}
            </p>
          </div>
        )}
      </div>

      {/* 合同范围列表 */}
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-neutral-700">合同范围定义</h2>
          <div className="flex gap-2">
            <input
              value={newCat}
              onChange={(e) => setNewCat(e.target.value)}
              placeholder="分类"
              className="w-24 rounded-lg border border-neutral-200 px-2 py-1 text-xs outline-none"
            />
            <input
              value={newDesc}
              onChange={(e) => setNewDesc(e.target.value)}
              placeholder="范围描述"
              className="w-40 rounded-lg border border-neutral-200 px-2 py-1 text-xs outline-none"
            />
            <button
              type="button"
              onClick={handleAdd}
              className="flex items-center gap-1 rounded bg-amber-600 px-2 py-1 text-xs text-white hover:bg-amber-700"
            >
              <Plus className="h-3 w-3" />
              添加
            </button>
            <button
              type="button"
              onClick={() => setShowExtract(!showExtract)}
              className="flex items-center gap-1 rounded border border-amber-200 bg-amber-50 px-2 py-1 text-xs text-amber-700 hover:bg-amber-100"
            >
              <FileUp className="h-3 w-3" />
              从文档提取
            </button>
          </div>
        </div>

        {/* 文档提取对话框 */}
        {showExtract && (
          <div className="mb-4 rounded-lg border border-amber-200 bg-amber-50 p-3">
            <div className="flex items-center gap-2">
              {docLoading ? (
                <Loader2 className="h-4 w-4 animate-spin text-amber-600" />
              ) : documents.length === 0 ? (
                <span className="text-xs text-neutral-500">暂无可用文档，请先导入知识库</span>
              ) : (
                <select
                  value={extractDocId}
                  onChange={(e) => setExtractDocId(e.target.value)}
                  className="w-64 rounded-lg border border-amber-200 px-2 py-1 text-xs outline-none focus:border-amber-500"
                >
                  <option value="">请选择文档...</option>
                  {documents.map((doc) => (
                    <option key={doc.id} value={doc.id}>
                      {doc.title}
                    </option>
                  ))}
                </select>
              )}
              <button
                type="button"
                onClick={handleExtract}
                disabled={extractLoading || !extractDocId}
                className="flex items-center gap-1 rounded bg-amber-600 px-3 py-1 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
              >
                {extractLoading ? (
                  <Loader2 className="h-3 w-3 animate-spin" />
                ) : (
                  <Search className="h-3 w-3" />
                )}
                提取
              </button>
            </div>
            {candidates.length > 0 && (
              <div className="mt-3 space-y-2">
                <p className="text-xs font-medium text-amber-800">
                  提取到 {candidates.length} 个候选范围项：
                </p>
                {candidates.map((c) => (
                  <div
                    key={`${c.category}-${c.description}-${c.confidence}`}
                    className="flex items-center justify-between rounded border border-amber-100 bg-white px-3 py-2"
                  >
                    <div className="flex items-center gap-2">
                      <span
                        className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                          c.is_in_scope ? "bg-green-100 text-green-700" : "bg-red-100 text-red-700"
                        }`}
                      >
                        {c.is_in_scope ? "范围内" : "排除"}
                      </span>
                      <span className="text-xs font-medium text-neutral-600">{c.category}</span>
                      <span className="text-xs text-neutral-500">{c.description}</span>
                    </div>
                    <span className="rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
                      置信度 {(c.confidence * 100).toFixed(0)}%
                    </span>
                  </div>
                ))}
                <button
                  type="button"
                  onClick={handleConfirmImport}
                  disabled={confirmLoading}
                  className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
                >
                  {confirmLoading ? (
                    <Loader2 className="h-3 w-3 animate-spin" />
                  ) : (
                    <CheckCircle className="h-3 w-3" />
                  )}
                  确认导入
                </button>
              </div>
            )}
          </div>
        )}

        {items.length === 0 ? (
          <p className="text-xs text-neutral-400">暂无范围定义</p>
        ) : (
          <div className="space-y-1">
            {items.map((item) => (
              <div
                key={item.id}
                className="flex items-center justify-between rounded border border-neutral-100 px-3 py-2"
              >
                <div className="flex items-center gap-2">
                  <span
                    className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                      item.is_in_scope ? "bg-green-100 text-green-700" : "bg-red-100 text-red-700"
                    }`}
                  >
                    {item.is_in_scope ? "范围内" : "排除"}
                  </span>
                  <span className="text-xs font-medium text-neutral-600">{item.category}</span>
                  <span className="text-xs text-neutral-500">{item.description}</span>
                </div>
                <button
                  type="button"
                  onClick={async () => {
                    await deleteScopeItem(projectId, item.id)
                    refresh()
                  }}
                  className="text-neutral-300 hover:text-red-500"
                >
                  <Trash2 className="h-3 w-3" />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

function HealthTab({ projectId }: { projectId: number | null }) {
  const [health, setHealth] = useState<ProjectHealthScore | null>(null)
  const [loading, setLoading] = useState(false)
  const [aiLoading, setAiLoading] = useState(false)
  const [aiReport, setAiReport] = useState("")
  const [fitGapInput, setFitGapInput] = useState("")
  const [fitGapResult, setFitGapResult] = useState("")
  const [fitGapLoading, setFitGapLoading] = useState(false)
  const toast = useToast()
  const activeProjectRef = useRef(projectId)

  useEffect(() => {
    activeProjectRef.current = projectId
    setHealth(null)
    setAiReport("")
    setFitGapInput("")
    setFitGapResult("")
    setLoading(false)
    setAiLoading(false)
    setFitGapLoading(false)
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
  }, [projectId])

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
      toast.error("分析失败: " + String(e))
    }
    if (activeProjectRef.current === projectId) setAiLoading(false)
  }, [health, aiLoading, projectId])

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
                {health.risk_level === "unknown" ? "暂无评分" : `${health.overall_score.toFixed(0)}/100`}
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
              <div className="rounded-lg border border-amber-100 bg-amber-50 p-3 text-xs leading-relaxed text-neutral-700 whitespace-pre-wrap">
                {aiReport}
              </div>
              <button
                type="button"
                onClick={async () => {
                  try {
                    const { save } = await import("@tauri-apps/plugin-dialog")
                    const path = await save({ filters: [{ name: "Markdown", extensions: ["md"] }] })
                    if (path) await exportReport(aiReport, path)
                  } catch (e) {
                    toast.error("导出失败: " + String(e))
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
              if (activeProjectRef.current === projectId) {
                setFitGapResult("分析失败: " + String(e))
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
                    toast.error("导出失败: " + String(e))
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

function ScriptsTab({ projectId }: { projectId: number | null }) {
  const [scenario, setScenario] = useState("")
  const [context, setContext] = useState("")
  const [tone, setTone] = useState("push_back")
  const [result, setResult] = useState<DefenseScriptResult | null>(null)
  const [loading, setLoading] = useState(false)
  const toast = useToast()
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
      toast.error(String(e))
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
        <h2 className="mb-3 text-sm font-semibold text-neutral-700">生成防身话术</h2>
        <div className="space-y-3">
          <div>
            <label className="mb-1 block text-[10px] font-medium text-neutral-500">场景描述</label>
            <textarea
              value={scenario}
              onChange={(e) => setScenario(e.target.value)}
              rows={2}
              placeholder="如：客户要求在合同范围外增加一个全新的报表模块"
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
            />
          </div>
          <div>
            <label className="mb-1 block text-[10px] font-medium text-neutral-500">
              上下文（可选）
            </label>
            <textarea
              value={context}
              onChange={(e) => setContext(e.target.value)}
              rows={2}
              placeholder="补充背景信息..."
              className="w-full rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
            />
          </div>
          <div className="flex items-center gap-3">
            <label className="text-[10px] font-medium text-neutral-500">沟通基调</label>
            <select
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
          {result.scripts.map((s, i) => (
            <div key={i} className="rounded-lg border border-amber-100 bg-amber-50 p-4">
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
                  const md = `# 防身话术\n\n## ${result.scenario_label}\n\n${result.scripts.map((s) => `### ${s.phase}\n\n${s.content}\n\n> 💡 ${s.tip}\n`).join("\n")}`
                  try {
                    const { save } = await import("@tauri-apps/plugin-dialog")
                    const path = await save({ filters: [{ name: "Markdown", extensions: ["md"] }] })
                    if (path) await exportReport(md, path)
                  } catch (e) {
                    toast.error("导出失败: " + String(e))
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

// ─── Analysis Tab: ReAct 深度分析对话 ──────────────────────────────────

function AnalysisTab({ projectId }: { projectId: number | null }) {
  const agent = useAgent()
  const slotId = `risk-analysis:${projectId ?? "none"}`
  const slot = agent.slots.get(slotId) ?? DEFAULT_SLOT
  const { messages, loading, currentTrace } = slot
  const [input, setInput] = useState("")
  const chatEndRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    setInput("")
  }, [projectId])

  // Auto-scroll
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [messages])

  const handleSend = useCallback(async () => {
    const text = input.trim()
    if (!text || loading || projectId === null) return
    setInput("")

    const prompt =
      "请作为 KingdeeKB 双轨风险把控舱中的风控专家分析以下问题，必要时使用知识库搜索、范围蔓延检查、项目健康评分、差异分析或防身话术工具，并给出专业、简洁、可执行的回答。\n\n问题：" +
      text
    await agent.sendMessage(slotId, prompt, {
      projectId,
    })
  }, [input, loading, projectId, agent, slotId])

  return (
    <div className="flex flex-col" style={{ height: "calc(100vh - 12rem)" }}>
      {/* Chat messages */}
      <div className="flex-1 space-y-3 overflow-y-auto rounded-lg border border-neutral-200 bg-white p-4">
        {messages.length === 0 && (
          <div className="flex h-full items-center justify-center">
            <div className="text-center">
              <Brain className="mx-auto mb-2 h-8 w-8 text-amber-400" />
              <p className="text-sm font-medium text-neutral-500">AI 深度风险分析</p>
              <p className="mt-1 text-xs text-neutral-400">
                输入问题开始分析项目风险、范围蔓延、客户沟通策略等
              </p>
            </div>
          </div>
        )}
        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}
          >
            <div
              className={`max-w-[80%] rounded-lg px-3 py-2 text-xs leading-relaxed ${
                msg.role === "user" ? "bg-amber-600 text-white" : "bg-neutral-100 text-neutral-700"
              }`}
            >
              {msg.streaming && !msg.content ? (
                <span className="flex items-center gap-1">
                  <Loader2 className="h-3 w-3 animate-spin" />
                  分析中
                </span>
              ) : (
                <span className="whitespace-pre-wrap">{msg.content}</span>
              )}
            </div>
          </div>
        ))}

        {/* ReAct Trace: thinking + tool calls */}
        {loading && (currentTrace.thinking || currentTrace.toolCalls.length > 0) && (
          <div className="space-y-2 rounded-lg border border-amber-200 bg-amber-50/50 p-3">
            {currentTrace.thinking && (
              <details className="text-xs">
                <summary className="cursor-pointer font-medium text-amber-700 select-none">
                  🤔 思考过程
                </summary>
                <div className="mt-1 whitespace-pre-wrap leading-relaxed text-amber-800 italic">
                  {currentTrace.thinking}
                </div>
              </details>
            )}
            {currentTrace.toolCalls.map((tc, i) => (
              <div
                key={i}
                className="flex items-center gap-2 rounded-md border border-neutral-200 bg-white px-3 py-1.5 text-xs"
              >
                <span className="text-amber-600">🔧</span>
                <span className="font-medium text-neutral-700">{tc.name}</span>
                {tc.args && (
                  <span className="truncate text-neutral-400 max-w-[200px]" title={tc.args}>
                    {tc.args.length > 60 ? tc.args.slice(0, 60) + "…" : tc.args}
                  </span>
                )}
                <span
                  className={`ml-auto rounded px-1.5 py-0.5 text-[10px] font-medium ${
                    tc.result ? "bg-green-100 text-green-700" : "bg-amber-100 text-amber-700"
                  }`}
                >
                  {tc.result ? "完成" : "执行中…"}
                </span>
              </div>
            ))}
          </div>
        )}

        <div ref={chatEndRef} />
      </div>

      {/* Input */}
      <div className="mt-3 flex gap-2">
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault()
              handleSend()
            }
          }}
          placeholder="输入风险分析问题..."
          className="flex-1 rounded-lg border border-neutral-200 px-3 py-2 text-xs outline-none focus:border-amber-500"
          disabled={loading}
        />
        <button
          type="button"
          onClick={handleSend}
          disabled={loading || !input.trim() || projectId === null}
          className="flex items-center gap-1 rounded-lg bg-amber-600 px-3 py-2 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
        >
          {loading ? <Loader2 className="h-3 w-3 animate-spin" /> : <Send className="h-3 w-3" />}
          发送
        </button>
      </div>
    </div>
  )
}
