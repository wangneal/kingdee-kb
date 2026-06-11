import {
  AlertCircle,
  ArrowRight,
  BookOpen,
  Calendar,
  Check,
  ClipboardList,
  FileText,
  FolderOpen,
  Loader2,
  MessageSquare,
  Package,
  Search,
  ShieldAlert,
} from "lucide-react"
import { useEffect, useState } from "react"
import { useNavigate } from "react-router-dom"
import { useProject } from "../contexts/ProjectContext"
import {
  getProjectPhases,
  type ProjectPhase,
  setCurrentProjectPhase,
} from "../lib/project-commands"
import {
  getStats,
  type KnowledgeStats,
  listProducts,
  type ProductMeta,
} from "../lib/tauri-commands"

export default function Home() {
  const { currentProjectId, currentProject } = useProject()
  const navigate = useNavigate()
  const [stats, setStats] = useState<KnowledgeStats | null>(null)
  const [products, setProducts] = useState<ProductMeta[]>([])
  const [projectPhases, setProjectPhases] = useState<ProjectPhase[]>([])
  const [updatingPhase, setUpdatingPhase] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [dashboardError, setDashboardError] = useState<string | null>(null)
  const [phaseError, setPhaseError] = useState<string | null>(null)

  useEffect(() => {
    ;(async () => {
      setLoading(true)
      setDashboardError(null)
      setPhaseError(null)
      try {
        const loadErrors: string[] = []
        const [statsData, productsData, phasesData] = await Promise.all([
          getStats(currentProjectId).catch((err) => {
            loadErrors.push(`知识统计加载失败：${err}`)
            return null
          }),
          listProducts(currentProjectId).catch((err) => {
            loadErrors.push(`产物列表加载失败：${err}`)
            return []
          }),
          currentProjectId == null
            ? Promise.resolve([])
            : getProjectPhases(currentProjectId).catch((err) => {
                loadErrors.push(`项目阶段加载失败：${err}`)
                return []
              }),
        ])
        setStats(statsData)
        setProducts(productsData)
        setProjectPhases(phasesData)
        setDashboardError(loadErrors.length > 0 ? loadErrors.join("；") : null)
      } catch (e) {
        setStats(null)
        setProducts([])
        setProjectPhases([])
        setDashboardError(`概览加载失败：${e}`)
      } finally {
        setLoading(false)
      }
    })()
  }, [currentProjectId])

  const recentProducts = [...products]
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
    .slice(0, 5)

  const currentPhase =
    projectPhases.find((phase) => phase.status === "current") ??
    projectPhases.find((phase) => phase.phase_key === currentProject?.current_phase)
  const completedPhaseCount = projectPhases.filter((phase) => phase.status === "completed").length
  const progressPercent =
    projectPhases.length === 0 ? 0 : Math.round((completedPhaseCount / projectPhases.length) * 100)

  async function handlePhaseChange(phase: ProjectPhase) {
    if (currentProjectId == null || phase.status === "current") return
    const confirmed = window.confirm(`确认将当前项目阶段切换为“${phase.phase_name}”吗？`)
    if (!confirmed) return
    setUpdatingPhase(phase.phase_key)
    setPhaseError(null)
    try {
      await setCurrentProjectPhase(currentProjectId, phase.phase_key)
      setProjectPhases(await getProjectPhases(currentProjectId))
    } catch (err) {
      setPhaseError(`阶段切换失败：${err}`)
    } finally {
      setUpdatingPhase(null)
    }
  }

  const formatDate = (dateStr: string) => {
    try {
      return new Date(dateStr).toLocaleDateString("zh-CN", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
      })
    } catch {
      return dateStr
    }
  }

  const quickActions = [
    {
      icon: BookOpen,
      label: "浏览知识库",
      description: "查看已导入的文档和知识片段",
      path: "/browse",
      color: "bg-[#1A6BD8]",
    },
    {
      icon: Search,
      label: "检索",
      description: "搜索知识库中的相关内容",
      path: "/search",
      color: "bg-emerald-600",
    },
    {
      icon: FileText,
      label: "AI 生成交付物",
      description: "在对话中调用官方技能生成文档、PPT 和清单",
      path: "/chat",
      color: "bg-violet-600",
    },
    {
      icon: MessageSquare,
      label: "AI 对话",
      description: "基于知识库的智能问答",
      path: "/chat",
      color: "bg-amber-600",
    },
    {
      icon: ClipboardList,
      label: "调研助手",
      description: "语音转录 + 会话管理 + 调研报告",
      path: "/research",
      color: "bg-cyan-600",
    },
    {
      icon: ShieldAlert,
      label: "风险把控",
      description: "范围预警 + 项目健康 + 防身话术",
      path: "/risk",
      color: "bg-red-600",
    },
  ]

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-[#1A6BD8]" />
        <span className="ml-2 text-sm text-neutral-500">加载概览…</span>
      </div>
    )
  }

  return (
    <div className="p-6 w-full">
      {/* 页头 */}
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-neutral-800">概览</h1>
        <p className="mt-1 text-sm text-neutral-500">
          实施顾问AI助手 — 金蝶ERP实施顾问本地知识管理工具
        </p>
      </div>

      {(dashboardError || phaseError) && (
        <div className="mb-6 flex items-start gap-2 rounded-lg border border-red-100 bg-red-50 px-3 py-2 text-xs text-red-600">
          <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>{phaseError ?? dashboardError}</span>
        </div>
      )}

      {/* 统计卡片 */}
      <div className="grid grid-cols-3 gap-4 mb-8">
        <div className="rounded-lg border border-neutral-200 bg-white p-5">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-violet-100">
              <ClipboardList className="h-5 w-5 text-violet-600" />
            </div>
            <div>
              <p className="text-2xl font-semibold text-neutral-800">{projectPhases.length || 7}</p>
              <p className="text-xs text-neutral-500">项目阶段</p>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-neutral-200 bg-white p-5">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-purple-100">
              <Package className="h-5 w-5 text-purple-600" />
            </div>
            <div>
              <p className="text-2xl font-semibold text-neutral-800">{products.length}</p>
              <p className="text-xs text-neutral-500">生成产物</p>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-neutral-200 bg-white p-5">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-emerald-100">
              <BookOpen className="h-5 w-5 text-emerald-600" />
            </div>
            <div>
              <p className="text-2xl font-semibold text-neutral-800">
                {stats?.document_count ?? 0}
              </p>
              <p className="text-xs text-neutral-500">知识库文档</p>
            </div>
          </div>
        </div>
      </div>

      {/* 项目进度 */}
      <div className="mb-8 rounded-lg border border-neutral-200 bg-white p-5">
        <div className="mb-5 flex items-start justify-between gap-4">
          <div>
            <h2 className="text-sm font-semibold text-neutral-700">项目进度</h2>
            <p className="mt-1 text-xs text-neutral-400">
              {currentProject?.name ?? "当前项目"} · 当前阶段：
              <span className="font-medium text-[#1A6BD8]">
                {currentPhase?.phase_name ?? "尚未设置"}
              </span>
            </p>
          </div>
          <div className="text-right">
            <p className="text-2xl font-semibold text-neutral-800">{progressPercent}%</p>
            <p className="text-xs text-neutral-400">
              已完成 {completedPhaseCount}/{projectPhases.length || 7} 个阶段
            </p>
          </div>
        </div>

        <div className="mb-5 h-2 overflow-hidden rounded-full bg-neutral-100">
          <div
            className="h-full rounded-full bg-[#1A6BD8] transition-all"
            style={{ width: `${progressPercent}%` }}
          />
        </div>

        <div className="grid grid-cols-7 gap-2">
          {projectPhases.map((phase) => {
            const isCompleted = phase.status === "completed"
            const isCurrent = phase.status === "current"
            return (
              <button
                key={phase.id}
                type="button"
                onClick={() => void handlePhaseChange(phase)}
                disabled={updatingPhase !== null || isCurrent}
                className="min-w-0 rounded-md py-1 text-center transition-colors hover:bg-neutral-50 disabled:cursor-default disabled:hover:bg-transparent"
                title={
                  isCurrent ? `当前阶段：${phase.phase_name}` : `切换到${phase.phase_name}阶段`
                }
              >
                <div
                  className={`mx-auto flex h-7 w-7 items-center justify-center rounded-full border text-xs ${
                    isCompleted
                      ? "border-[#1A6BD8] bg-[#1A6BD8] text-white"
                      : isCurrent
                        ? "border-[#1A6BD8] bg-[#1A6BD8]/10 text-[#1A6BD8]"
                        : "border-neutral-200 bg-white text-neutral-400"
                  }`}
                >
                  {isCompleted ? <Check className="h-3.5 w-3.5" /> : phase.phase_index + 1}
                </div>
                <p
                  className={`mt-2 truncate text-xs ${
                    isCurrent ? "font-medium text-[#1A6BD8]" : "text-neutral-500"
                  }`}
                  title={phase.phase_name}
                >
                  {phase.phase_name}
                </p>
              </button>
            )
          })}
        </div>
      </div>

      {/* 快捷操作 */}
      <div className="mb-8">
        <h2 className="text-sm font-semibold text-neutral-700 mb-4">快捷操作</h2>
        <div className="grid grid-cols-4 gap-3">
          {quickActions.map((action) => (
            <button
              key={`${action.path}-${action.label}`}
              type="button"
              onClick={() => navigate(action.path)}
              className="group rounded-lg border border-neutral-200 bg-white p-4 text-left transition-all hover:border-[#1A6BD8]/30 hover:shadow-sm"
            >
              <div
                className={`flex h-9 w-9 items-center justify-center rounded-lg ${action.color} mb-3`}
              >
                <action.icon className="h-4 w-4 text-white" />
              </div>
              <p className="text-sm font-medium text-neutral-800">{action.label}</p>
              <p className="text-xs text-neutral-400 mt-0.5">{action.description}</p>
            </button>
          ))}
        </div>
      </div>

      {/* 最近产物 */}
      <div>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-sm font-semibold text-neutral-700">最近产物</h2>
          {products.length > 0 && (
            <button
              type="button"
              onClick={() => navigate("/products")}
              className="flex items-center gap-1 text-xs text-[#1A6BD8] hover:underline"
            >
              查看全部
              <ArrowRight className="h-3 w-3" />
            </button>
          )}
        </div>

        {recentProducts.length === 0 ? (
          <div className="rounded-lg border border-dashed border-neutral-200 bg-white p-8 text-center">
            <Package className="mx-auto h-8 w-8 text-neutral-300" />
            <p className="mt-2 text-sm text-neutral-500">暂无产物</p>
            <p className="text-xs text-neutral-400 mt-1">
              在 AI 对话中说明要生成的交付物，系统会优先调用官方技能
            </p>
            <button
              type="button"
              onClick={() => navigate("/chat")}
              className="mt-3 inline-flex items-center gap-1.5 rounded-md bg-[#1A6BD8] px-3 py-1.5 text-xs text-white hover:bg-[#1558B0]"
            >
              <MessageSquare className="h-3.5 w-3.5" />去 AI 对话
            </button>
          </div>
        ) : (
          <div className="rounded-lg border border-neutral-200 bg-white overflow-hidden">
            {recentProducts.map((product, idx) => (
              <button
                key={product.id}
                type="button"
                onClick={() => navigate("/products")}
                className={`flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-neutral-50 ${
                  idx < recentProducts.length - 1 ? "border-b border-neutral-100" : ""
                }`}
              >
                {product.template_name.endsWith(".xlsx") ||
                product.template_name.endsWith(".xls") ? (
                  <FileText className="h-4 w-4 shrink-0 text-emerald-600" />
                ) : (
                  <FileText className="h-4 w-4 shrink-0 text-[#1A6BD8]" />
                )}
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-neutral-700 truncate">{product.template_name}</p>
                  <p className="text-xs text-neutral-400 flex items-center gap-2">
                    <span className="flex items-center gap-1">
                      <FolderOpen className="h-2.5 w-2.5" />
                      {currentProject?.name ?? `项目 #${product.project_id}`}
                    </span>
                    <span className="flex items-center gap-1">
                      <Calendar className="h-2.5 w-2.5" />
                      {formatDate(product.created_at)}
                    </span>
                  </p>
                </div>
                <ArrowRight className="h-4 w-4 text-neutral-300 shrink-0" />
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
