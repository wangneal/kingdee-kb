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

export interface RawSource {
  id: number
  project_id: number
  identity: string
  original_path: string
  storage_path: string
  sha256: string
  file_size: number | null
  mime_type: string | null
  status: string
  created_at: string
  deleted_at: string | null
}

export interface IngestionQueueItem {
  id: string
  project_id: number
  source_identity: string
  status: string
  retry_count: number
  error_message: string | null
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

export async function updateProject(
  projectId: number,
  name: string,
  clientName: string,
  description: string,
): Promise<void> {
  return invoke("update_project", { projectId, name, clientName, description })
}

export async function updateProjectPhasePlan(
  projectId: number,
  phaseKey: string,
  plannedStart: string | null,
  plannedEnd: string | null,
): Promise<void> {
  return invoke("update_project_phase_plan", { projectId, phaseKey, plannedStart, plannedEnd })
}

export async function createRawSource(
  projectId: number,
  identity: string,
  sourcePath: string,
): Promise<RawSource> {
  return invoke("create_raw_source", { projectId, identity, sourcePath, mimeType: null })
}

export async function listRawSources(projectId: number): Promise<RawSource[]> {
  return invoke("list_raw_sources", { projectId })
}

export async function softDeleteRawSource(id: number): Promise<void> {
  return invoke("soft_delete_raw_source", { id })
}

export async function enqueueIngestion(projectId: number, sourceIdentity: string): Promise<string> {
  return invoke("enqueue_ingestion", { projectId, sourceIdentity })
}

export async function listIngestionQueue(): Promise<IngestionQueueItem[]> {
  return invoke("list_ingestion_queue")
}

export async function retryFailedIngestions(projectId: number): Promise<void> {
  return invoke("retry_project_failed_ingestions", { projectId })
}

export async function processIngestionQueue(projectId: number): Promise<string[]> {
  return invoke("process_project_ingestion_queue", { projectId })
}

export async function archiveProject(projectId: number): Promise<void> {
  return invoke("archive_project", { projectId })
}

export async function restoreProject(projectId: number): Promise<void> {
  return invoke("restore_project", { projectId })
}

export async function setCurrentProjectPhase(projectId: number, phaseKey: string): Promise<void> {
  return invoke("set_current_project_phase", { projectId, phaseKey })
}

export async function ensureProjectActive(projectId: number): Promise<void> {
  return invoke("ensure_project_active", { projectId })
}

// ─── 产品版本管理 ───

export interface ProjectProduct {
  id: number
  project_id: number
  product_name: string
  product_version: string
}

export async function listProjectProducts(projectId: number): Promise<ProjectProduct[]> {
  return invoke("list_project_products", { projectId })
}

export async function addProjectProduct(
  projectId: number,
  productName: string,
  productVersion: string,
): Promise<number> {
  return invoke("add_project_product", { projectId, productName, productVersion })
}

export async function deleteProjectProduct(projectId: number, productId: number): Promise<void> {
  return invoke("delete_project_product", { projectId, productId })
}
