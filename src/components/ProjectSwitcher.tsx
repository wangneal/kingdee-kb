import {
  Archive,
  Check,
  ChevronsUpDown,
  FolderKanban,
  Plus,
  RefreshCw,
  RotateCcw,
  Trash2,
} from "lucide-react"
import { useMemo, useState } from "react"
import { DestructiveConfirmDialog } from "@/components/DestructiveConfirmDialog"
import { useProject } from "@/contexts/ProjectContext"
import {
  archiveProject,
  createProject,
  deleteProject,
  restoreProject,
} from "@/lib/project-commands"

export default function ProjectSwitcher() {
  const {
    currentProjectId,
    setCurrentProjectId,
    currentProject,
    projects,
    loading,
    error,
    refreshProjects,
  } = useProject()
  const [open, setOpen] = useState(false)
  const [creating, setCreating] = useState(false)
  const [name, setName] = useState("")
  const [actionError, setActionError] = useState<string | null>(null)
  // 硬删除二次确认：null = 不在删除流程；{id, name} = 正在确认删除该项目
  const [deletingProject, setDeletingProject] = useState<{ id: number; name: string } | null>(null)

  const activeProjects = useMemo(
    () => projects.filter((project) => project.status === "active"),
    [projects],
  )
  const archivedProjects = useMemo(
    () => projects.filter((project) => project.status === "archived"),
    [projects],
  )

  async function handleCreateProject() {
    const trimmedName = name.trim()
    if (!trimmedName) return
    setActionError(null)
    setCreating(true)
    try {
      const projectId = await createProject(trimmedName)
      await refreshProjects()
      setCurrentProjectId(projectId)
      setName("")
      setOpen(false)
    } catch (err) {
      setActionError(`新建项目失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setCreating(false)
    }
  }

  async function handleArchiveProject(projectId: number, projectName: string) {
    if (!window.confirm(`确认归档项目“${projectName}”吗？归档后将不再显示在项目切换列表中。`)) {
      return
    }
    setActionError(null)
    try {
      await archiveProject(projectId)
      await refreshProjects()
    } catch (err) {
      setActionError(`归档项目失败：${err instanceof Error ? err.message : String(err)}`)
    }
  }

  async function handleRestoreProject(projectId: number) {
    setActionError(null)
    try {
      await restoreProject(projectId)
      await refreshProjects()
    } catch (err) {
      setActionError(`恢复项目失败：${err instanceof Error ? err.message : String(err)}`)
    }
  }

  async function handleConfirmDelete() {
    if (!deletingProject) return
    const { id } = deletingProject
    const wasCurrent = id === currentProjectId
    setActionError(null)
    setDeletingProject(null)
    try {
      await deleteProject(id)
      await refreshProjects()
      // 删的是当前项目时，切换到首个剩余 active 项目
      if (wasCurrent) {
        const next = projects.find((p) => p.status === "active" && p.id !== id)
        if (next) {
          setCurrentProjectId(next.id)
        }
      }
    } catch (err) {
      setActionError(`删除项目失败：${err instanceof Error ? err.message : String(err)}`)
    }
  }

  return (
    <div className="border-b border-neutral-200 p-3">
      <button
        type="button"
        onClick={() => {
          setActionError(null)
          setOpen((value) => !value)
        }}
        className="flex w-full items-center gap-2 rounded-lg border border-neutral-200 bg-white px-3 py-2 text-left text-sm text-neutral-700 transition-colors hover:border-neutral-300 hover:bg-neutral-50"
        title="切换项目"
      >
        <FolderKanban className="h-4 w-4 shrink-0 text-[#1A6BD8]" />
        <span className="min-w-0 flex-1 truncate font-medium">
          {loading ? "加载项目中" : (currentProject?.name ?? "未选择项目")}
        </span>
        <ChevronsUpDown className="h-4 w-4 shrink-0 text-neutral-400" />
      </button>

      {open && (
        <div className="mt-2 rounded-lg border border-neutral-200 bg-white p-1 shadow-sm">
          {activeProjects.map((project) => (
            <div
              key={project.id}
              className="group flex items-center rounded-md hover:bg-neutral-50"
            >
              <button
                type="button"
                onClick={() => {
                  setCurrentProjectId(project.id)
                  setOpen(false)
                }}
                className="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5 text-left text-sm text-neutral-700"
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate">{project.name}</span>
                  <span className="block truncate text-[10px] text-neutral-400">
                    {project.document_count} 篇资料 · {project.product_count} 个产物
                  </span>
                </span>
                {project.id === currentProjectId && <Check className="h-4 w-4 text-[#1A6BD8]" />}
              </button>
              {activeProjects.length > 1 && project.name !== "默认项目" && (
                <button
                  type="button"
                  onClick={() => void handleArchiveProject(project.id, project.name)}
                  className="mr-1 rounded p-1 text-neutral-300 opacity-0 transition-opacity hover:bg-neutral-100 hover:text-red-500 group-hover:opacity-100"
                  title={`归档项目“${project.name}”`}
                >
                  <Archive className="h-3.5 w-3.5" />
                </button>
              )}
            </div>
          ))}

          {archivedProjects.length > 0 && (
            <>
              <div className="my-1 border-t border-neutral-100" />
              <p className="px-2 py-1 text-[10px] font-medium text-neutral-400">已归档项目</p>
              {archivedProjects.map((project) => (
                <div key={project.id} className="flex items-center rounded-md px-2 py-1 text-xs">
                  <span className="min-w-0 flex-1 truncate text-neutral-400">{project.name}</span>
                  <button
                    type="button"
                    onClick={() => void handleRestoreProject(project.id)}
                    className="rounded p-1 text-neutral-400 hover:bg-neutral-100 hover:text-[#1A6BD8]"
                    title={`恢复项目“${project.name}”`}
                  >
                    <RotateCcw className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setActionError(null)
                      setDeletingProject({ id: project.id, name: project.name })
                    }}
                    className="ml-1 rounded p-1 text-neutral-400 hover:bg-red-50 hover:text-red-600"
                    title={`硬删除项目“${project.name}”（不可恢复）`}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>
              ))}
            </>
          )}

          <DestructiveConfirmDialog
            open={deletingProject}
            title={deletingProject ? `硬删除项目“${deletingProject.name}”` : ""}
            message="此操作不可撤销，将永久删除该项目的所有文档、Wiki 候选、向量索引、BM25 索引和物理文件。"
            hint={
              deletingProject
                ? `请输入项目名 \`${deletingProject.name}\` 以确认：`
                : undefined
            }
            onConfirm={() => void handleConfirmDelete()}
            onCancel={() => setDeletingProject(null)}
          />

          <div className="my-1 border-t border-neutral-100" />

          <div className="flex items-center gap-1 px-1 pb-1">
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") void handleCreateProject()
              }}
              className="min-w-0 flex-1 rounded-md border border-neutral-200 px-2 py-1.5 text-xs outline-none focus:border-[#1A6BD8]"
              placeholder="新项目名称"
            />
            <button
              type="button"
              onClick={() => void handleCreateProject()}
              disabled={creating || !name.trim()}
              className="rounded-md p-1.5 text-neutral-500 hover:bg-neutral-100 disabled:opacity-40"
              title="新建项目"
            >
              <Plus className="h-4 w-4" />
            </button>
            <button
              type="button"
              onClick={() => void refreshProjects()}
              className="rounded-md p-1.5 text-neutral-500 hover:bg-neutral-100"
              title="刷新项目"
            >
              <RefreshCw className="h-4 w-4" />
            </button>
          </div>

          {(actionError || error) && (
            <div className="px-2 pb-1 text-xs text-red-600">{actionError || error}</div>
          )}
        </div>
      )}
    </div>
  )
}
