import {
  AlertCircle,
  AlertTriangle,
  CheckCircle,
  FileText,
  FileUp,
  Loader2,
  Plus,
  Search,
  Send,
  Trash2,
  X,
} from "lucide-react"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { useToast } from "@/components/Toast"
import { useAppError } from "@/contexts/AppErrorContext"
import { formatAppError, parseAppError } from "@/lib/app-error"
import {
  addScopeItem,
  type CandidateScopeItem,
  type ContractScopeItem,
  type ContractScopeProgressEvent,
  checkScopeCreep,
  confirmScopeItems,
  type DocumentMeta,
  deleteScopeItem,
  extractScopeFromDocument,
  listDocuments,
  listenContractScopeProgress,
  listScopeItems,
} from "@/lib/tauri-commands"

export default function ScopeTab({ projectId }: { projectId: number | null }) {
  const [items, setItems] = useState<ContractScopeItem[]>([])
  const [checkResult, setCheckResult] = useState<ScopeCreepResult | null>(null)
  const [loading, setLoading] = useState(false)
  const [newReq, setNewReq] = useState("")
  const [newCat, setNewCat] = useState("")
  const [newDesc, setNewDesc] = useState("")
  const [showExtract, setShowExtract] = useState(false)
  const [documents, setDocuments] = useState<DocumentMeta[]>([])
  const [docSearch, setDocSearch] = useState("")
  const [scopeFilter, setScopeFilter] = useState("")
  const [docLoading, setDocLoading] = useState(false)
  const [extractDocId, setExtractDocId] = useState("")
  const [extractError, setExtractError] = useState("")
  const [extractLoading, setExtractLoading] = useState(false)
  const [candidates, setCandidates] = useState<CandidateScopeItem[]>([])
  const [confirmLoading, setConfirmLoading] = useState(false)
  const [addLoading, setAddLoading] = useState(false)
  const [deletingId, setDeletingId] = useState<number | null>(null)
  const [scopeProgress, setScopeProgress] = useState<ContractScopeProgressEvent | null>(null)
  const toast = useToast()
  const { showLlmKeyError } = useAppError()
  const activeProjectRef = useRef(projectId)

  useEffect(() => {
    activeProjectRef.current = projectId
    setItems([])
    setCheckResult(null)
    setNewReq("")
    setShowExtract(false)
    setDocuments([])
    setDocSearch("")
    setScopeFilter("")
    setExtractDocId("")
    setExtractError("")
    setCandidates([])
    setLoading(false)
    setDocLoading(false)
    setExtractLoading(false)
    setConfirmLoading(false)
    setAddLoading(false)
    setDeletingId(null)
    setScopeProgress(null)
  }, [projectId])

  const filteredDocuments = useMemo(() => {
    const query = docSearch.trim().toLowerCase()
    const list = [...documents].sort(
      (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
    )
    if (!query) return list
    return list.filter((doc) => {
      const sourcePath = doc.source_path ?? ""
      return (
        doc.title.toLowerCase().includes(query) ||
        sourcePath.toLowerCase().includes(query) ||
        String(doc.id).includes(query)
      )
    })
  }, [docSearch, documents])

  const selectedDocument = useMemo(
    () => documents.find((doc) => String(doc.id) === extractDocId) ?? null,
    [documents, extractDocId],
  )

  const filteredScopeItems = useMemo(() => {
    const query = scopeFilter.trim().toLowerCase()
    if (!query) return items
    return items.filter((item) =>
      [item.category, item.description, item.detail]
        .filter(Boolean)
        .some((value) => value.toLowerCase().includes(query)),
    )
  }, [items, scopeFilter])

  const refresh = useCallback(async () => {
    if (projectId === null) return
    try {
      const list = await listScopeItems(projectId)
      if (activeProjectRef.current === projectId) setItems(list)
    } catch (e) {
      if (activeProjectRef.current === projectId) {
        toast.error(`加载范围列表失败: ${String(e)}`)
      }
    }
  }, [projectId, toast])

  useEffect(() => {
    refresh()
  }, [refresh])

  // 监听合同范围提取进度事件
  useEffect(() => {
    let unlisten: (() => void) | null = null
    listenContractScopeProgress((event) => {
      if (
        activeProjectRef.current === projectId &&
        event.project_id === projectId &&
        event.doc_id === Number(extractDocId)
      ) {
        setScopeProgress(event)
        if (event.step === "done") {
          setTimeout(() => {
            if (activeProjectRef.current === projectId) setScopeProgress(null)
          }, 3000)
        }
      }
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      if (unlisten) unlisten()
    }
  }, [projectId, extractDocId])

  // 监听文档导入事件
  useEffect(() => {
    let listenFn: (() => void) | null = null
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<{ project_id: number }>("document-imported", (event) => {
        if (event.payload.project_id === projectId) {
          toast.info("检测到新文档导入，建议在「需求蔓延警报」中检查是否超出合同范围")
        }
      }).then((fn) => {
        listenFn = fn
      })
    })
    return () => {
      if (listenFn) listenFn()
    }
  }, [projectId, toast])

  // 打开提取对话框时加载文档列表
  useEffect(() => {
    if (!showExtract || projectId === null) {
      if (!showExtract) {
        setExtractDocId("")
        setDocSearch("")
        setExtractError("")
        setCandidates([])
      }
      return
    }
    setDocLoading(true)
    listDocuments(projectId)
      .then((result) => {
        if (activeProjectRef.current === projectId) {
          setDocuments(result)
          setExtractError("")
        }
      })
      .catch((error) => {
        if (activeProjectRef.current === projectId) {
          setDocuments([])
          setExtractError(`加载文档列表失败: ${String(error)}`)
        }
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
      const parsed = parseAppError(e)
      if (parsed?.code === "LLM_INVALID_KEY") {
        showLlmKeyError(parsed)
      } else {
        toast.error(formatAppError(e))
      }
    }
    if (activeProjectRef.current === projectId) setLoading(false)
  }

  const handleAdd = async () => {
    if (!newCat.trim() || !newDesc.trim() || projectId === null) return
    setAddLoading(true)
    try {
      await addScopeItem(projectId, newCat.trim(), newDesc.trim(), true, "")
      if (activeProjectRef.current === projectId) {
        setNewCat("")
        setNewDesc("")
        await refresh()
      }
    } catch (e) {
      toast.error(`添加范围失败: ${String(e)}`)
    } finally {
      if (activeProjectRef.current === projectId) setAddLoading(false)
    }
  }

  const handleDelete = async (item: ContractScopeItem) => {
    if (projectId === null || deletingId !== null) return
    if (!window.confirm(`确认删除范围"${item.description}"？`)) return
    setDeletingId(item.id)
    try {
      await deleteScopeItem(projectId, item.id)
      if (activeProjectRef.current === projectId) await refresh()
    } catch (e) {
      toast.error(`删除范围失败: ${String(e)}`)
    } finally {
      if (activeProjectRef.current === projectId) setDeletingId(null)
    }
  }

  const handleExtract = async () => {
    if (!extractDocId.trim() || projectId === null) return
    setExtractLoading(true)
    setCandidates([])
    setExtractError("")
    try {
      const result = await extractScopeFromDocument(projectId, Number(extractDocId))
      if (activeProjectRef.current === projectId) {
        setCandidates(result)
        if (result.length === 0) {
          setExtractError(
            "未提取到候选范围项。请确认文档中包含实施范围、交付物或排除项等明确条款。",
          )
        }
      }
    } catch (e) {
      const parsed = parseAppError(e)
      if (parsed?.code === "LLM_INVALID_KEY") {
        showLlmKeyError(parsed)
        setExtractError("LLM API Key 失效，请配置后重试")
        return
      }
      const message = `提取失败: ${formatAppError(e)}`
      setExtractError(message)
      toast.error(message)
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
      toast.error(`导入失败: ${String(e)}`)
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
              disabled={addLoading || !newCat.trim() || !newDesc.trim()}
              className="flex items-center gap-1 rounded bg-amber-600 px-2 py-1 text-xs text-white hover:bg-amber-700 disabled:opacity-50"
            >
              {addLoading ? (
                <Loader2 className="h-3 w-3 animate-spin" />
              ) : (
                <Plus className="h-3 w-3" />
              )}
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
            <div className="flex items-start gap-3">
              <div className="min-w-0 flex-1">
                <div className="relative">
                  <Search className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-amber-500" />
                  <input
                    value={docSearch}
                    onChange={(e) => setDocSearch(e.target.value)}
                    placeholder="搜索文档名称、路径或 ID"
                    className="w-full rounded-lg border border-amber-200 bg-white py-1.5 pl-8 pr-8 text-xs outline-none focus:border-amber-500"
                  />
                  {docSearch.trim() && (
                    <button
                      type="button"
                      onClick={() => setDocSearch("")}
                      className="absolute right-2 top-1.5 rounded p-0.5 text-amber-500 hover:bg-amber-100"
                      title="清空搜索"
                    >
                      <X className="h-3.5 w-3.5" />
                    </button>
                  )}
                </div>

                {extractError ? (
                  <p className="mt-2 text-xs text-red-600">{extractError}</p>
                ) : docLoading ? (
                  <div className="mt-2 flex items-center gap-2 text-xs text-amber-700">
                    <Loader2 className="h-4 w-4 animate-spin" />
                    加载文档列表...
                  </div>
                ) : documents.length === 0 ? (
                  <p className="mt-2 text-xs text-neutral-500">暂无可用文档，请先导入知识库</p>
                ) : filteredDocuments.length === 0 ? (
                  <p className="mt-2 text-xs text-neutral-500">没有匹配的文档</p>
                ) : (
                  <div className="mt-2 max-h-48 space-y-1 overflow-y-auto rounded-lg border border-amber-100 bg-white p-1">
                    {filteredDocuments.map((doc) => {
                      const selected = String(doc.id) === extractDocId
                      return (
                        <button
                          key={doc.id}
                          type="button"
                          onClick={() => setExtractDocId(String(doc.id))}
                          className={`flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left transition-colors ${
                            selected
                              ? "bg-amber-100 text-amber-900"
                              : "text-neutral-600 hover:bg-amber-50"
                          }`}
                        >
                          <FileText className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600" />
                          <span className="min-w-0 flex-1">
                            <span className="block truncate text-xs font-medium">{doc.title}</span>
                            <span className="block truncate text-[10px] text-neutral-400">
                              {doc.source_path ?? `文档 ID ${doc.id}`}
                            </span>
                          </span>
                        </button>
                      )
                    })}
                  </div>
                )}

                {selectedDocument && (
                  <p className="mt-2 truncate text-[10px] text-amber-700">
                    已选择：{selectedDocument.title}
                  </p>
                )}
              </div>

              <button
                type="button"
                onClick={handleExtract}
                disabled={docLoading || extractLoading || !extractDocId}
                className="flex shrink-0 items-center gap-1 rounded bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50"
              >
                {extractLoading ? (
                  <Loader2 className="h-3 w-3 animate-spin" />
                ) : (
                  <Search className="h-3 w-3" />
                )}
                提取
              </button>
            </div>
            {scopeProgress && extractLoading && (
              <div className="mt-2 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2">
                <div className="flex items-center gap-2">
                  <Loader2 className="h-3.5 w-3.5 animate-spin text-amber-600" />
                  <span className="text-xs font-medium text-amber-800">
                    {scopeProgress.message}
                  </span>
                </div>
                {scopeProgress.total > 0 && (
                  <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-amber-200">
                    <div
                      className="h-full rounded-full bg-amber-600 transition-all duration-300"
                      style={{
                        width: `${Math.min(
                          scopeProgress.step === "done" || scopeProgress.step === "merging"
                            ? 100
                            : (scopeProgress.current / scopeProgress.total) * 100,
                          100,
                        )}%`,
                      }}
                    />
                  </div>
                )}
              </div>
            )}
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

        {items.length > 0 && (
          <div className="mb-3">
            <div className="relative max-w-md">
              <Search className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-neutral-400" />
              <input
                value={scopeFilter}
                onChange={(e) => setScopeFilter(e.target.value)}
                placeholder="搜索分类、描述或明细"
                className="w-full rounded-lg border border-neutral-200 py-1.5 pl-8 pr-8 text-xs outline-none focus:border-amber-500"
              />
              {scopeFilter.trim() && (
                <button
                  type="button"
                  onClick={() => setScopeFilter("")}
                  className="absolute right-2 top-1.5 rounded p-0.5 text-neutral-400 hover:bg-neutral-100"
                  title="清空搜索"
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              )}
            </div>
          </div>
        )}

        {items.length === 0 ? (
          <p className="text-xs text-neutral-400">暂无范围定义</p>
        ) : filteredScopeItems.length === 0 ? (
          <p className="text-xs text-neutral-400">没有匹配的范围定义</p>
        ) : (
          <div className="space-y-1">
            {filteredScopeItems.map((item) => (
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
                  onClick={() => void handleDelete(item)}
                  disabled={deletingId !== null}
                  className="text-neutral-300 hover:text-red-500 disabled:opacity-50"
                  title="删除范围"
                >
                  {deletingId === item.id ? (
                    <Loader2 className="h-3 w-3 animate-spin" />
                  ) : (
                    <Trash2 className="h-3 w-3" />
                  )}
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
