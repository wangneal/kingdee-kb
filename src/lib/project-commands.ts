import { invoke } from "@tauri-apps/api/core"

export interface Project {
  id: number
  name: string
  client_name: string
  description: string
  status: string
  current_phase: string
  created_at: string
  updated_at: string
}

export interface ProjectPhase {
  id: number
  project_id: number
  phase_key: string
  phase_name: string
  phase_index: number
  status: string
  planned_start: string | null
  planned_end: string | null
  actual_start: string | null
  actual_end: string | null
}

export interface ProjectSummary {
  id: number
  name: string
  client_name: string
  current_phase: string
  status: string
  document_count: number
  wiki_count: number
  product_count: number
  risk_count: number
  created_at: string
}

export async function ensureDefaultProject(): Promise<number> {
  return invoke("ensure_default_project")
}

export async function createProject(
  name: string,
  clientName?: string,
  description?: string,
): Promise<number> {
  return invoke("create_project", {
    name,
    clientName: clientName ?? null,
    description: description ?? null,
  })
}

export async function listProjects(): Promise<ProjectSummary[]> {
  return invoke("list_projects")
}

export async function getProject(projectId: number): Promise<Project | null> {
  return invoke("get_project", { projectId })
}

export async function getProjectPhases(projectId: number): Promise<ProjectPhase[]> {
  return invoke("get_project_phases", { projectId })
}

export async function archiveProject(projectId: number): Promise<void> {
  return invoke("archive_project", { projectId })
}

export async function ensureProjectActive(projectId: number): Promise<void> {
  return invoke("ensure_project_active", { projectId })
}
