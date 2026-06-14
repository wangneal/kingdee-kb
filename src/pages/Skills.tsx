import { open } from "@tauri-apps/plugin-dialog"
import {
  BookOpen,
  ChevronDown,
  Code,
  ExternalLink,
  FileText,
  FolderOpen,
  Loader2,
  RefreshCw,
  Search,
  Upload,
  X,
  Zap,
} from "lucide-react"
import { useCallback, useEffect, useState } from "react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import {
  executeSkillScript,
  getSkillFull,
  getSkillStats,
  importSkill,
  listSkills,
  readSkillFile,
  rescanSkills,
  searchSkills,
} from "@/lib/skill-commands"
import type {
  ExecutionResult,
  Skill,
  SkillFile,
  SkillFull,
  SkillStatsResponse,
} from "@/lib/skill-types"
import {
  SKILL_CATEGORY_LABELS,
  SKILL_FILE_TYPE_ICONS,
  SKILL_FILE_TYPE_LABELS,
} from "@/lib/skill-types"

const CATEGORY_ORDER = ["core", "stage", "mgmt", "tool"]

export default function Skills() {
  const [skills, setSkills] = useState<Skill[]>([])
  const [stats, setStats] = useState<SkillStatsResponse | null>(null)
  const [query, setQuery] = useState("")
  const [loading, setLoading] = useState(true)
  const [scanning, setScanning] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [detail, setDetail] = useState<Skill | null>(null)
  const [expandedCards, setExpandedCards] = useState<Set<string>>(new Set())
  const [skillFull, setSkillFull] = useState<SkillFull | null>(null)
  const [fileContent, setFileContent] = useState<string | null>(null)
  const [loadingFile, setLoadingFile] = useState(false)

  // Script Execution state
  const [executingScript, setExecutingScript] = useState<string | null>(null)
  const [scriptResult, setScriptResult] = useState<ExecutionResult | null>(null)

  const refresh = useCallback(async (q?: string) => {
    setLoading(true)
    setError(null)
    try {
      const [skillList, skillStats] = await Promise.all([
        q ? searchSkills(q) : listSkills(),
        getSkillStats().catch(() => null),
      ])
      setSkills(skillList)
      setStats(skillStats)
    } catch (e) {
      setError(String(e))
    }
    setLoading(false)
  }, [])

  useEffect(() => {
    refresh()
  }, [refresh])

  const handleSearch = useCallback(() => refresh(query), [query, refresh])
  const handleRescan = useCallback(async () => {
    setScanning(true)
    setError(null)
    try {
      await rescanSkills()
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setScanning(false)
    }
  }, [refresh])

  const handleImport = useCallback(async () => {
    setError(null)
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "技能包 ZIP", extensions: ["zip"] }],
      })
      if (!selected) return
      await importSkill(selected as string)
      await refresh()
      setError(null)
    } catch (e) {
      setError(String(e))
    }
  }, [refresh])

  const handleCardClick = useCallback(async (name: string) => {
    setError(null)
    try {
      const full = await getSkillFull(name)
      setSkillFull(full)
      setDetail(full?.skill ?? null)
      setFileContent(null)
    } catch (e) {
      setError(`加载技能详情失败：${String(e)}`)
    }
  }, [])

  // Script Execution handler
  const handleExecuteScript = useCallback(async (skillId: string, scriptPath: string) => {
    setExecutingScript(scriptPath)
    setScriptResult(null)
    try {
      const result = await executeSkillScript(skillId, scriptPath, [])
      setScriptResult(result)
    } catch (e) {
      setScriptResult({
        success: false,
        output: "",
        duration_ms: 0,
        error: String(e),
      })
    }
    setExecutingScript(null)
  }, [])

  const handleCardExpand = useCallback((name: string, e: React.MouseEvent) => {
    e.stopPropagation()
    setExpandedCards((prev) => {
      const next = new Set(prev)
      next.has(name) ? next.delete(name) : next.add(name)
      return next
    })
  }, [])

  // ── Detail Overlay ──
  if (detail && skillFull) {
    const filesByType = skillFull.supporting_files.reduce(
      (acc, f) => {
        const type = f.file_type
        if (!acc[type]) acc[type] = []
        acc[type].push(f)
        return acc
      },
      {} as Record<string, SkillFile[]>,
    )

    const fileTypes = ["script", "reference", "asset", "config", "other"] as const

    return (
      <div className="flex h-full flex-col">
        <div className="flex h-12 items-center justify-between border-b border-neutral-200 px-6 shrink-0">
          <h2 className="text-sm font-semibold text-neutral-800">
            {detail.metadata.icon || "📄"} {detail.name}
          </h2>
          <button
            type="button"
            onClick={() => {
              setDetail(null)
              setSkillFull(null)
              setFileContent(null)
            }}
            className="flex h-7 w-7 items-center justify-center rounded-md text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-6">
          {/* Body */}
          <div
            className="prose prose-sm prose-neutral max-w-4xl
            prose-headings:text-neutral-800
            prose-h2:mt-6 prose-h2:mb-3 prose-h2:border-b prose-h2:border-neutral-200 prose-h2:pb-2
            prose-p:text-neutral-600
            prose-code:rounded prose-code:bg-neutral-100 prose-code:px-1.5 prose-code:py-0.5 prose-code:text-xs prose-code:text-neutral-700
            prose-pre:rounded-lg prose-pre:bg-neutral-100 prose-pre:text-neutral-700
            prose-table:text-xs prose-table:border-collapse prose-th:border prose-th:border-neutral-200 prose-th:bg-neutral-50 prose-th:px-2 prose-th:py-1 prose-td:border prose-td:border-neutral-200 prose-td:px-2 prose-td:py-1
            prose-a:text-[#1A6BD8] prose-a:no-underline hover:prose-a:underline
            prose-strong:text-neutral-700
            prose-li:text-neutral-600
            prose-blockquote:border-l-[#1A6BD8] prose-blockquote:text-neutral-500
            prose-img:rounded-lg"
          >
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{detail.body || "*暂无内容*"}</ReactMarkdown>
          </div>

          {/* Supporting Files */}
          {skillFull.supporting_files.length > 0 && (
            <div className="mt-6">
              <h3 className="flex items-center gap-2 text-sm font-semibold text-neutral-700 mb-3">
                <FolderOpen className="h-4 w-4" />
                支撑文件 ({skillFull.supporting_files.length})
              </h3>
              {fileTypes.map((type) => {
                const files = filesByType[type]
                if (!files || files.length === 0) return null
                return (
                  <div key={type} className="mb-3">
                    <p className="text-xs font-medium text-neutral-500 mb-1">
                      {SKILL_FILE_TYPE_ICONS[type]} {SKILL_FILE_TYPE_LABELS[type]} ({files.length})
                    </p>
                    <div className="flex flex-wrap gap-1.5">
                      {files.map((f) => (
                        <div key={f.path} className="flex items-center gap-1">
                          <button
                            type="button"
                            onClick={async () => {
                              setLoadingFile(true)
                              setFileContent(null)
                              try {
                                const content = await readSkillFile(detail.name, f.path)
                                setFileContent(`// ${f.name}\n${content}`)
                              } catch (e) {
                                setFileContent(`读取失败: ${e}`)
                              }
                              setLoadingFile(false)
                            }}
                            className="flex items-center gap-1 rounded-md border border-neutral-200 bg-white px-2 py-1 text-xs text-neutral-600 hover:border-[#1A6BD8]/30 hover:bg-blue-50/50 transition-colors"
                          >
                            <FileText className="h-3 w-3 text-neutral-400" />
                            {f.name}
                            <span className="text-neutral-300 text-[10px]">
                              {f.size > 1024 ? `${(f.size / 1024).toFixed(1)}KB` : `${f.size}B`}
                            </span>
                          </button>
                          {type === "script" && (
                            <button
                              type="button"
                              onClick={(e) => {
                                e.stopPropagation()
                                handleExecuteScript(detail.name, f.path)
                              }}
                              disabled={executingScript === f.path}
                              className="flex items-center gap-1 rounded-md bg-green-500 px-2 py-1 text-xs text-white hover:bg-green-600 disabled:opacity-50"
                            >
                              {executingScript === f.path ? (
                                <Loader2 className="h-3 w-3 animate-spin" />
                              ) : (
                                <Zap className="h-3 w-3" />
                              )}
                              运行
                            </button>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )
              })}
            </div>
          )}

          {/* File Content Viewer */}
          {(fileContent || loadingFile) && (
            <div className="mt-4 rounded-lg border border-neutral-200 bg-neutral-50 p-4">
              <div className="flex items-center justify-between mb-2">
                <span className="text-xs font-medium text-neutral-500">文件内容</span>
                <button
                  type="button"
                  onClick={() => setFileContent(null)}
                  className="text-xs text-neutral-400 hover:text-neutral-600"
                >
                  关闭
                </button>
              </div>
              {loadingFile ? (
                <div className="flex items-center gap-2 text-xs text-neutral-400">
                  <Loader2 className="h-3 w-3 animate-spin" />
                  加载中...
                </div>
              ) : (
                <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all text-xs text-neutral-700 font-mono">
                  {fileContent}
                </pre>
              )}
            </div>
          )}

          {/* Script Execution Result */}
          {scriptResult && (
            <div
              className={`mt-4 rounded-lg border p-4 ${scriptResult.success ? "border-green-200 bg-green-50" : "border-red-200 bg-red-50"}`}
            >
              <div className="flex items-center justify-between mb-2">
                <span
                  className={`text-xs font-medium ${scriptResult.success ? "text-green-700" : "text-red-700"}`}
                >
                  {scriptResult.success ? "✅ 执行成功" : "❌ 执行失败"}
                </span>
                <div className="flex items-center gap-2">
                  <span className="text-[10px] text-neutral-400">{scriptResult.duration_ms}ms</span>
                  <button
                    type="button"
                    onClick={() => setScriptResult(null)}
                    className="text-xs text-neutral-400 hover:text-neutral-600"
                  >
                    关闭
                  </button>
                </div>
              </div>
              {scriptResult.output && (
                <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all text-xs text-neutral-700 font-mono bg-white rounded p-2 mb-2">
                  {scriptResult.output}
                </pre>
              )}
              {scriptResult.error && (
                <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all text-xs text-red-600 font-mono bg-white rounded p-2">
                  {scriptResult.error}
                </pre>
              )}
            </div>
          )}

          {/* Shared Resources */}
          {skillFull.shared_references.length > 0 && (
            <div className="mt-4">
              <h3 className="flex items-center gap-2 text-sm font-semibold text-neutral-700 mb-2">
                <BookOpen className="h-4 w-4" />
                共享资源 ({skillFull.shared_references.length})
              </h3>
              <div className="flex flex-wrap gap-1.5">
                {skillFull.shared_references.map((r) => (
                  <span
                    key={r.path}
                    className="inline-flex items-center gap-1 rounded-full bg-neutral-100 px-2 py-0.5 text-[11px] text-neutral-500"
                  >
                    📄 {r.name}
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    )
  }

  // ── Detail Overlay Fallback ──
  if (detail && !skillFull) {
    return (
      <div className="flex h-full flex-col">
        <div className="flex h-12 items-center justify-between border-b border-neutral-200 px-6 shrink-0">
          <h2 className="text-sm font-semibold text-neutral-800">
            {detail.metadata.icon || "📄"} {detail.name}
          </h2>
          <button
            type="button"
            onClick={() => setDetail(null)}
            className="flex h-7 w-7 items-center justify-center rounded-md text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-6">
          <div
            className="prose prose-sm prose-neutral max-w-4xl
            prose-headings:text-neutral-800
            prose-h2:mt-6 prose-h2:mb-3 prose-h2:border-b prose-h2:border-neutral-200 prose-h2:pb-2
            prose-p:text-neutral-600
            prose-code:rounded prose-code:bg-neutral-100 prose-code:px-1.5 prose-code:py-0.5 prose-code:text-xs prose-code:text-neutral-700
            prose-pre:rounded-lg prose-pre:bg-neutral-100 prose-pre:text-neutral-700
            prose-a:text-[#1A6BD8] prose-a:no-underline hover:prose-a:underline
            prose-strong:text-neutral-700
            prose-li:text-neutral-600
            prose-blockquote:border-l-[#1A6BD8] prose-blockquote:text-neutral-500
            prose-img:rounded-lg"
          >
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{detail.body || "*暂无内容*"}</ReactMarkdown>
          </div>
        </div>
      </div>
    )
  }

  // ── Card Grid ──
  const grouped = CATEGORY_ORDER.flatMap((cat) =>
    skills.filter((s) => s.metadata.category === cat),
  ).concat(skills.filter((s) => !CATEGORY_ORDER.includes(s.metadata.category)))

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6 shrink-0">
        <div className="flex items-center gap-2">
          <Zap className="h-5 w-5 text-[#1A6BD8]" />
          <h1 className="text-base font-semibold text-neutral-800">技能体系</h1>
          {stats && <span className="text-xs text-neutral-400">{stats.total} 个</span>}
        </div>
        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-neutral-400" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSearch()}
              placeholder="搜索..."
              className="h-8 w-36 rounded-md border border-neutral-200 bg-white pl-8 pr-2 text-xs placeholder:text-neutral-400 focus:border-[#1A6BD8] focus:outline-none"
            />
          </div>
          <button
            type="button"
            onClick={handleImport}
            className="flex h-8 items-center gap-1 rounded-md bg-[#1A6BD8] px-3 text-xs text-white hover:bg-[#1555B0]"
          >
            <Upload className="h-3.5 w-3.5" />
            导入
          </button>
          <button
            type="button"
            onClick={handleRescan}
            disabled={scanning}
            className="flex h-8 items-center gap-1 rounded-md border border-neutral-200 px-2.5 text-xs text-neutral-600 hover:bg-neutral-50 disabled:opacity-50"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${scanning ? "animate-spin" : ""}`} />
          </button>
        </div>
      </div>

      {error && (
        <div className="mx-6 mt-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-600">
          {error}
        </div>
      )}

      {/* 卡片网格 */}
      <div className="flex-1 overflow-y-auto p-6">
        {loading ? (
          <div className="flex items-center justify-center py-20">
            <Loader2 className="h-6 w-6 animate-spin text-neutral-300" />
          </div>
        ) : grouped.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-20 text-neutral-400">
            <Zap className="h-12 w-12 text-neutral-200" />
            <p className="mt-3 text-sm">没有找到技能</p>
            <p className="mt-1 text-xs">点击「导入」添加技能 ZIP 包</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {grouped.map((skill) => (
              <SkillCard
                key={skill.name}
                skill={skill}
                expanded={expandedCards.has(skill.name)}
                onClick={() => handleCardClick(skill.name)}
                onExpand={(e) => handleCardExpand(skill.name, e)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

// ── 技能卡片 ──
function SkillCard({
  skill,
  expanded,
  onClick,
  onExpand,
}: {
  skill: Skill
  expanded: boolean
  onClick: () => void
  onExpand: (e: React.MouseEvent) => void
}) {
  const cat = skill.metadata.category
  const catLabel = SKILL_CATEGORY_LABELS[cat] || cat
  const hasExtras = skill.scripts.length > 0 || skill.references.length > 0

  return (
    <div className="group rounded-lg border border-neutral-200 bg-white p-4 transition-all hover:border-[#1A6BD8]/30 hover:shadow-sm">
      {/* 页头 */}
      <div className="flex items-start justify-between mb-2">
        <span className="text-2xl">{skill.metadata.icon || "📄"}</span>
        <span className="rounded-full bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
          {catLabel}
        </span>
      </div>

      {/* 名称 */}
      <h3 className="mb-1 text-sm font-semibold text-neutral-800">{skill.name}</h3>

      {/* 描述 */}
      {skill.metadata.description && (
        <p className="mb-3 text-xs leading-relaxed text-neutral-500 line-clamp-2">
          {skill.metadata.description}
        </p>
      )}

      {/* 展开切换 */}
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={onExpand}
          className="flex items-center gap-1 text-[11px] text-neutral-400 hover:text-neutral-600"
        >
          <ChevronDown className={`h-3 w-3 transition-transform ${expanded ? "rotate-180" : ""}`} />
          详情
        </button>
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation()
            onClick()
          }}
          className="flex items-center gap-1 text-[11px] text-[#1A6BD8] opacity-0 transition-opacity group-hover:opacity-100 hover:underline"
        >
          查看 <ExternalLink className="h-3 w-3" />
        </button>
      </div>

      {/* 展开详情 */}
      {expanded && (
        <div className="mt-3 border-t border-neutral-100 pt-3 text-xs text-neutral-500">
          {skill.metadata.version && <p className="mb-1">版本: {skill.metadata.version}</p>}
          {hasExtras && (
            <div className="flex gap-3 mt-2">
              {skill.scripts.length > 0 && (
                <span className="flex items-center gap-1">
                  <Code className="h-3 w-3" />
                  {skill.scripts.length} 脚本
                </span>
              )}
              {skill.references.length > 0 && (
                <span className="flex items-center gap-1">
                  <BookOpen className="h-3 w-3" />
                  {skill.references.length} 参考
                </span>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
