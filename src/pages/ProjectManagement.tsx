import { open } from "@tauri-apps/plugin-dialog"
import {
  FileUp,
  FolderKanban,
  Loader2,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  Trash2,
} from "lucide-react"
import { useCallback, useEffect, useState } from "react"
import { useProject } from "../contexts/ProjectContext"
import { getImportDialogDefaultPath } from "../lib/dialog-options"
import {
  createRawSource,
  enqueueIngestion,
  getProject,
  getProjectPhases,
  type IngestionQueueItem,
  listIngestionQueue,
  listRawSources,
  type Project,
  type ProjectPhase,
  type ProjectProduct,
  processIngestionQueue,
  type RawSource,
  retryFailedIngestions,
  softDeleteRawSource,
  updateProject,
  updateProjectPhasePlan,
  listProjectProducts,
  addProjectProduct,
  deleteProjectProduct,
} from "../lib/project-commands"

type Tab = "details" | "phases" | "sources" | "queue"

export default function ProjectManagement() {
  const { currentProjectId, refreshProjects } = useProject()
  const [tab, setTab] = useState<Tab>("details")
  const [project, setProject] = useState<Project | null>(null)
  const [phases, setPhases] = useState<ProjectPhase[]>([])
  const [sources, setSources] = useState<RawSource[]>([])
  const [queue, setQueue] = useState<IngestionQueueItem[]>([])
  const [products, setProducts] = useState<ProjectProduct[]>([])
  const [busy, setBusy] = useState(false)
  const [loading, setLoading] = useState(true)
  const [message, setMessage] = useState("")

  const refresh = useCallback(async () => {
    if (currentProjectId == null) {
      setProject(null)
      setPhases([])
      setSources([])
      setQueue([])
      setProducts([])
      setLoading(false)
      return
    }
    setLoading(true)
    try {
      const [nextProject, nextPhases, nextSources, nextQueue, nextProducts] = await Promise.all([
        getProject(currentProjectId),
        getProjectPhases(currentProjectId),
        listRawSources(currentProjectId),
        listIngestionQueue(),
        listProjectProducts(currentProjectId),
      ])
      setProject(nextProject)
      setPhases(nextPhases)
      setSources(nextSources)
      setQueue(nextQueue.filter((item) => item.project_id === currentProjectId))
      setProducts(nextProducts)
    } finally {
      setLoading(false)
    }
  }, [currentProjectId])

  useEffect(() => {
    void refresh()
  }, [refresh])

  async function run(action: () => Promise<boolean>, success: string) {
    setBusy(true)
    setMessage("")
    try {
      const completed = await action()
      if (!completed) return
      await refresh()
      setMessage(success)
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy(false)
    }
  }

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-neutral-500">
        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
        正在加载项目管理数据...
      </div>
    )
  }

  if (currentProjectId == null || !project) {
    return <div className="p-6 text-sm text-neutral-500">当前没有可管理的项目。</div>
  }

  const tabs: Array<[Tab, string]> = [
    ["details", "项目详情"],
    ["phases", "阶段计划"],
    ["sources", "原始资料"],
    ["queue", "摄入队列"],
  ]

  return (
    <div className="w-full p-6">
      <div className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-neutral-800">项目管理</h1>
          <p className="mt-1 text-sm text-neutral-500">{project.name}</p>
        </div>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={loading}
          className="rounded-md border border-neutral-200 p-2 text-neutral-500 hover:bg-white"
          title="刷新"
        >
          <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
        </button>
      </div>

      <div className="mb-5 flex gap-1 rounded-lg border border-neutral-200 bg-white p-1">
        {tabs.map(([key, label]) => (
          <button
            key={key}
            type="button"
            onClick={() => setTab(key)}
            className={`rounded-md px-4 py-2 text-sm ${
              tab === key ? "bg-[#1A6BD8] text-white" : "text-neutral-600 hover:bg-neutral-50"
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      {message && (
        <div className="mb-4 rounded-md border border-neutral-200 bg-white px-4 py-3 text-sm text-neutral-600">
          {message}
        </div>
      )}

      {tab === "details" && (
        <>
          <ProjectDetails
            project={project}
            busy={busy}
            onSave={(name, clientName, description) =>
              run(async () => {
                await updateProject(project.id, name, clientName, description)
                await refreshProjects()
                return true
              }, "项目详情已保存")
            }
          />
          <ProductVersions
            products={products}
            busy={busy}
            onAdd={(productName, productVersion) =>
              run(async () => {
                await addProjectProduct(project.id, productName, productVersion)
                return true
              }, "产品版本已添加")
            }
            onDelete={(productId) =>
              run(async () => {
                if (!window.confirm("确认删除该产品版本吗？")) return false
                await deleteProjectProduct(project.id, productId)
                return true
              }, "产品版本已删除")
            }
          />
        </>
      )}
      {tab === "phases" && (
        <PhasePlans
          phases={phases}
          busy={busy}
          onSave={(phaseKey, start, end) =>
            run(async () => {
              await updateProjectPhasePlan(project.id, phaseKey, start || null, end || null)
              return true
            }, "阶段计划已保存")
          }
        />
      )}
      {tab === "sources" && (
        <RawSources
          sources={sources}
          busy={busy}
          onImport={() =>
            run(async () => {
              const defaultPath = await getImportDialogDefaultPath()
              const path = await open({
                title: "选择原始资料文件",
                defaultPath,
                multiple: false,
                directory: false,
              })
              if (!path) return false
              const identity = path.split(/[\\/]/).pop() ?? "source"
              await createRawSource(project.id, identity, path)
              return true
            }, "原始资料已导入")
          }
          onQueue={(identity) =>
            run(async () => {
              await enqueueIngestion(project.id, identity)
              return true
            }, "已加入摄入队列")
          }
          onDelete={(id) =>
            run(async () => {
              if (!window.confirm("确认移除这条原始资料记录吗？")) return false
              await softDeleteRawSource(id)
              return true
            }, "原始资料已移除")
          }
        />
      )}
      {tab === "queue" && (
        <QueuePanel
          items={queue}
          busy={busy}
          onProcess={() =>
            run(async () => {
              await processIngestionQueue(project.id)
              return true
            }, "摄入队列处理完成")
          }
          onRetry={() =>
            run(async () => {
              await retryFailedIngestions(project.id)
              return true
            }, "失败任务已重置为待处理")
          }
        />
      )}
    </div>
  )
}

function ProjectDetails({
  project,
  busy,
  onSave,
}: {
  project: Project
  busy: boolean
  onSave: (name: string, clientName: string, description: string) => Promise<void>
}) {
  const [name, setName] = useState(project.name)
  const [clientName, setClientName] = useState(project.client_name)
  const [description, setDescription] = useState(project.description)
  useEffect(() => {
    setName(project.name)
    setClientName(project.client_name)
    setDescription(project.description)
  }, [project])
  return (
    <section className="space-y-4 rounded-lg border border-neutral-200 bg-white p-5">
      <Field label="项目名称" value={name} onChange={setName} />
      <Field label="客户名称" value={clientName} onChange={setClientName} />
      <label className="block text-sm text-neutral-600">
        项目描述
        <textarea
          value={description}
          onChange={(event) => setDescription(event.target.value)}
          className="mt-1 min-h-28 w-full rounded-md border border-neutral-200 px-3 py-2 outline-none focus:border-[#1A6BD8]"
        />
      </label>
      <ActionButton
        busy={busy}
        icon={Save}
        label="保存项目详情"
        onClick={() => onSave(name, clientName, description)}
      />
    </section>
  )
}

function ProductVersions({
  products,
  busy,
  onAdd,
  onDelete,
}: {
  products: ProjectProduct[]
  busy: boolean
  onAdd: (productName: string, productVersion: string) => Promise<void>
  onDelete: (productId: number) => Promise<void>
}) {
  const [productName, setProductName] = useState("")
  const [productVersion, setProductVersion] = useState("")

  function handleAdd() {
    const name = productName.trim()
    const version = productVersion.trim()
    if (!name || !version) return
    void onAdd(name, version).then(() => {
      setProductName("")
      setProductVersion("")
    })
  }

  return (
    <section className="mt-4 space-y-3 rounded-lg border border-neutral-200 bg-white p-5">
      <h2 className="text-sm font-semibold text-neutral-700">产品版本</h2>
      {products.length === 0 ? (
        <p className="text-xs text-neutral-400">暂无产品版本</p>
      ) : (
        <div className="space-y-1">
          {products.map((p) => (
            <div
              key={p.id}
              className="flex items-center justify-between rounded border border-neutral-100 px-3 py-2"
            >
              <div className="flex items-center gap-2">
                <span className="text-xs font-medium text-neutral-600">{p.product_name}</span>
                <span className="rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
                  {p.product_version}
                </span>
              </div>
              <button
                type="button"
                disabled={busy}
                onClick={() => void onDelete(p.id)}
                className="text-neutral-300 hover:text-red-500 disabled:opacity-50"
                title="删除产品版本"
              >
                <Trash2 className="h-3 w-3" />
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="flex items-center gap-2">
        <input
          value={productName}
          onChange={(e) => setProductName(e.target.value)}
          placeholder="产品名称"
          className="flex-1 rounded-md border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
        />
        <input
          value={productVersion}
          onChange={(e) => setProductVersion(e.target.value)}
          placeholder="版本号"
          className="w-32 rounded-md border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
        />
        <button
          type="button"
          disabled={busy || !productName.trim() || !productVersion.trim()}
          onClick={handleAdd}
          className="flex items-center gap-1 rounded bg-[#1A6BD8] px-2 py-1.5 text-xs text-white hover:bg-[#1558B0] disabled:opacity-50"
        >
          <Plus className="h-3 w-3" />
          添加
        </button>
      </div>
    </section>
  )
}

function PhasePlans({
  phases,
  busy,
  onSave,
}: {
  phases: ProjectPhase[]
  busy: boolean
  onSave: (phaseKey: string, start: string, end: string) => Promise<void>
}) {
  return (
    <section className="overflow-hidden rounded-lg border border-neutral-200 bg-white">
      {phases.map((phase) => (
        <PhaseRow key={phase.id} phase={phase} busy={busy} onSave={onSave} />
      ))}
    </section>
  )
}

function PhaseRow({
  phase,
  busy,
  onSave,
}: {
  phase: ProjectPhase
  busy: boolean
  onSave: (phaseKey: string, start: string, end: string) => Promise<void>
}) {
  const [start, setStart] = useState(phase.planned_start ?? "")
  const [end, setEnd] = useState(phase.planned_end ?? "")
  return (
    <div className="grid grid-cols-[1fr_160px_160px_90px] items-center gap-3 border-b border-neutral-100 px-4 py-3 last:border-b-0">
      <div>
        <p className="text-sm font-medium text-neutral-700">{phase.phase_name}</p>
        <p className="text-xs text-neutral-400">{phase.status}</p>
      </div>
      <input
        type="date"
        value={start}
        onChange={(event) => setStart(event.target.value)}
        className="rounded-md border border-neutral-200 px-2 py-1.5 text-sm"
      />
      <input
        type="date"
        value={end}
        onChange={(event) => setEnd(event.target.value)}
        className="rounded-md border border-neutral-200 px-2 py-1.5 text-sm"
      />
      <button
        type="button"
        disabled={busy}
        onClick={() => void onSave(phase.phase_key, start, end)}
        className="rounded-md border border-neutral-200 px-3 py-1.5 text-xs hover:bg-neutral-50 disabled:opacity-50"
      >
        保存
      </button>
    </div>
  )
}

function RawSources({
  sources,
  busy,
  onImport,
  onQueue,
  onDelete,
}: {
  sources: RawSource[]
  busy: boolean
  onImport: () => Promise<void>
  onQueue: (identity: string) => Promise<void>
  onDelete: (id: number) => Promise<void>
}) {
  return (
    <section>
      <ActionButton busy={busy} icon={FileUp} label="导入原始资料" onClick={onImport} />
      <div className="mt-4 overflow-hidden rounded-lg border border-neutral-200 bg-white">
        {sources.length === 0 ? (
          <p className="p-8 text-center text-sm text-neutral-400">暂无原始资料</p>
        ) : (
          sources.map((source) => (
            <div
              key={source.id}
              className="flex items-center gap-3 border-b border-neutral-100 px-4 py-3 last:border-b-0"
            >
              <FolderKanban className="h-4 w-4 text-[#1A6BD8]" />
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm text-neutral-700">{source.identity}</p>
                <p className="truncate text-xs text-neutral-400">{source.original_path}</p>
              </div>
              <button
                type="button"
                disabled={busy}
                onClick={() => void onQueue(source.identity)}
                className="rounded p-2 text-neutral-400 hover:bg-neutral-50 hover:text-[#1A6BD8]"
                title="加入摄入队列"
              >
                <Play className="h-4 w-4" />
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => void onDelete(source.id)}
                className="rounded p-2 text-neutral-400 hover:bg-neutral-50 hover:text-red-500"
                title="移除原始资料"
              >
                <Trash2 className="h-4 w-4" />
              </button>
            </div>
          ))
        )}
      </div>
    </section>
  )
}

function QueuePanel({
  items,
  busy,
  onProcess,
  onRetry,
}: {
  items: IngestionQueueItem[]
  busy: boolean
  onProcess: () => Promise<void>
  onRetry: () => Promise<void>
}) {
  return (
    <section>
      <div className="flex gap-2">
        <ActionButton busy={busy} icon={Play} label="处理待执行任务" onClick={onProcess} />
        <ActionButton
          busy={busy}
          icon={RotateCcw}
          label="重试失败任务"
          onClick={onRetry}
          secondary
        />
      </div>
      <div className="mt-4 overflow-hidden rounded-lg border border-neutral-200 bg-white">
        {items.length === 0 ? (
          <p className="p-8 text-center text-sm text-neutral-400">队列为空</p>
        ) : (
          items.map((item) => (
            <div
              key={item.id}
              className="flex items-center gap-3 border-b border-neutral-100 px-4 py-3 last:border-b-0"
            >
              <span
                className={`h-2 w-2 rounded-full ${item.status === "done" ? "bg-green-500" : item.status === "failed" ? "bg-red-500" : "bg-amber-500"}`}
              />
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm text-neutral-700">{item.source_identity}</p>
                <p className="text-xs text-neutral-400">
                  {item.status} · 重试 {item.retry_count}/3
                </p>
                {item.error_message && (
                  <p className="truncate text-xs text-red-500">{item.error_message}</p>
                )}
              </div>
            </div>
          ))
        )}
      </div>
    </section>
  )
}

function Field({
  label,
  value,
  onChange,
}: {
  label: string
  value: string
  onChange: (value: string) => void
}) {
  return (
    <label className="block text-sm text-neutral-600">
      {label}
      <input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="mt-1 w-full rounded-md border border-neutral-200 px-3 py-2 outline-none focus:border-[#1A6BD8]"
      />
    </label>
  )
}

function ActionButton({
  busy,
  icon: Icon,
  label,
  onClick,
  secondary = false,
}: {
  busy: boolean
  icon: typeof Save
  label: string
  onClick: () => Promise<void>
  secondary?: boolean
}) {
  return (
    <button
      type="button"
      disabled={busy}
      onClick={() => void onClick()}
      className={`inline-flex items-center gap-2 rounded-md px-3 py-2 text-sm disabled:opacity-50 ${secondary ? "border border-neutral-200 bg-white text-neutral-600 hover:bg-neutral-50" : "bg-[#1A6BD8] text-white hover:bg-[#1558B0]"}`}
    >
      {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Icon className="h-4 w-4" />}
      {label}
    </button>
  )
}
