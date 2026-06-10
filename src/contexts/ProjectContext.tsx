import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react"
import { ensureDefaultProject, listProjects, type ProjectSummary } from "../lib/project-commands"

const STORAGE_KEY = "kingdee_kb_active_project"

interface ProjectContextValue {
  projectId: string | undefined
  setProjectId: (id: string | undefined) => void
  currentProjectId: number | null
  setCurrentProjectId: (id: number | null) => void
  currentProject: ProjectSummary | null
  projects: ProjectSummary[]
  loading: boolean
  error: string | null
  refreshProjects: () => Promise<void>
}

const ProjectContext = createContext<ProjectContextValue | null>(null)

export function ProjectProvider({ children }: { children: ReactNode }) {
  const [currentProjectId, setCurrentProjectIdState] = useState<number | null>(() => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY)
      if (!raw) return null
      const parsed = Number(raw)
      return Number.isFinite(parsed) ? parsed : null
    } catch {
      return null
    }
  })
  const [projects, setProjects] = useState<ProjectSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const ensuredDefaultProject = useRef(false)

  const projectId = currentProjectId == null ? undefined : String(currentProjectId)

  const currentProject = useMemo(
    () => projects.find((project) => project.id === currentProjectId) ?? null,
    [currentProjectId, projects],
  )

  const setProjectId = useCallback((id: string | undefined) => {
    const parsed = id == null ? null : Number(id)
    setCurrentProjectIdState(Number.isFinite(parsed) ? parsed : null)
  }, [])

  const setCurrentProjectId = useCallback((id: number | null) => {
    setCurrentProjectIdState(id)
    try {
      if (id != null) localStorage.setItem(STORAGE_KEY, String(id))
      else localStorage.removeItem(STORAGE_KEY)
    } catch (storageError) {
      console.warn("保存当前项目失败", storageError)
    }
  }, [])

  const refreshProjects = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      let defaultId: number | null = null
      if (!ensuredDefaultProject.current) {
        defaultId = await ensureDefaultProject()
        ensuredDefaultProject.current = true
      }

      let nextProjects = await listProjects()
      if (!nextProjects.some((project) => project.status === "active")) {
        defaultId = await ensureDefaultProject()
        ensuredDefaultProject.current = true
        nextProjects = await listProjects()
      }

      setProjects(nextProjects)
      setCurrentProjectIdState((previousId) => {
        if (
          previousId != null &&
          nextProjects.some((project) => project.id === previousId && project.status === "active")
        ) {
          return previousId
        }
        return nextProjects.find((project) => project.status === "active")?.id ?? defaultId
      })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      setError(message)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refreshProjects()
  }, [refreshProjects])

  useEffect(() => {
    try {
      if (currentProjectId != null) localStorage.setItem(STORAGE_KEY, String(currentProjectId))
      else localStorage.removeItem(STORAGE_KEY)
    } catch (storageError) {
      console.warn("同步当前项目失败", storageError)
    }
  }, [currentProjectId])

  return (
    <ProjectContext.Provider
      value={{
        projectId,
        setProjectId,
        currentProjectId,
        setCurrentProjectId,
        currentProject,
        projects,
        loading,
        error,
        refreshProjects,
      }}
    >
      {children}
    </ProjectContext.Provider>
  )
}

export function useProject(): ProjectContextValue {
  const ctx = useContext(ProjectContext)
  if (!ctx) throw new Error("useProject must be used within ProjectProvider")
  return ctx
}
