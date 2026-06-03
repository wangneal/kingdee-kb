import { Check, ChevronsUpDown, FolderKanban, Plus, RefreshCw } from "lucide-react"
import { useMemo, useState } from "react"
import { useProject } from "../contexts/ProjectContext"
import { createProject } from "../lib/project-commands"

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

  const activeProjects = useMemo(
    () => projects.filter((project) => project.status === "active"),
    [projects],
  )

  async function handleCreateProject() {
    const trimmedName = name.trim()
    if (!trimmedName) return
    setCreating(true)
    try {
      const projectId = await createProject(trimmedName)
      await refreshProjects()
      setCurrentProjectId(projectId)
      setName("")
      setOpen(false)
    } finally {
      setCreating(false)
    }
  }

  return (
    <div className="border-b border-neutral-200 p-3">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2 rounded-lg border border-neutral-200 bg-white px-3 py-2 text-left text-sm text-neutral-700 transition-colors hover:border-neutral-300 hover:bg-neutral-50"
        title="切换项目"
      >
        <FolderKanban className="h-4 w-4 shrink-0 text-[#1A6BD8]" />
        <span className="min-w-0 flex-1 truncate font-medium">
          {loading ? "加载项目中" : currentProject?.name ?? "未选择项目"}
        </span>
        <ChevronsUpDown className="h-4 w-4 shrink-0 text-neutral-400" />
      </button>

      {open && (
        <div className="mt-2 rounded-lg border border-neutral-200 bg-white p-1 shadow-sm">
          {activeProjects.map((project) => (
            <button
              key={project.id}
              type="button"
              onClick={() => {
                setCurrentProjectId(project.id)
                setOpen(false)
              }}
              className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-neutral-700 hover:bg-neutral-50"
            >
              <span className="min-w-0 flex-1 truncate">{project.name}</span>
              {project.id === currentProjectId && <Check className="h-4 w-4 text-[#1A6BD8]" />}
            </button>
          ))}

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

          {error && <div className="px-2 pb-1 text-xs text-red-600">{error}</div>}
        </div>
      )}
    </div>
  )
}
