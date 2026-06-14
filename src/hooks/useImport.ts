/**
 * 可复用的文档导入 Hook
 *
 * 封装文本/文件/文件夹导入的核心逻辑，
 * 知识编译开关状态从全局 KbCompilationContext 读取（跨页面同步），
 * 并在配置加载完成前阻塞导入操作。
 */
import { useCallback, useRef } from "react"
import {
  type DirectoryIngestionResult,
  type IngestionResult,
  ingestDirectory,
  ingestFile,
  ingestText,
} from "../lib/tauri-commands"
import { useKbCompilation } from "../contexts/KbCompilationContext"

export function useImport() {
  // 知识编译开关由全局 Context 管理（Import.tsx 与 Settings.tsx 共享同步）
  const { enabled: kbCompilationEnabled, loading } = useKbCompilation()
  // ref 随最新值更新，供异步回调闭包读取，避免回调依赖重渲染
  const configRef = useRef(false)
  configRef.current = kbCompilationEnabled
  const loadingRef = useRef(true)
  loadingRef.current = loading

  /** 确保配置已加载，未加载时等待（最多 3 秒） */
  const ensureConfigLoaded = useCallback(async () => {
    if (!loadingRef.current) return
    for (let i = 0; i < 30; i++) {
      await new Promise((r) => setTimeout(r, 100))
      if (!loadingRef.current) return
    }
  }, [])

  /** 导入文件 */
  const importFile = useCallback(
    async (filePath: string, projectId?: number): Promise<IngestionResult> => {
      await ensureConfigLoaded()
      if (projectId == null) throw new Error("请先选择项目")
      return ingestFile(filePath, projectId, configRef.current)
    },
    [ensureConfigLoaded],
  )

  /** 导入文件夹 */
  const importDirectory = useCallback(
    async (dirPath: string, projectId?: number): Promise<DirectoryIngestionResult> => {
      await ensureConfigLoaded()
      if (projectId == null) throw new Error("请先选择项目")
      return ingestDirectory(dirPath, projectId, configRef.current)
    },
    [ensureConfigLoaded],
  )

  /** 导入文本 */
  const importText = useCallback(
    async (text: string, title: string, projectId?: number): Promise<IngestionResult> => {
      await ensureConfigLoaded()
      if (projectId == null) throw new Error("请先选择项目")
      return ingestText(text, title, projectId, configRef.current)
    },
    [ensureConfigLoaded],
  )

  return {
    /** 知识编译开关是否启用（配置加载后有效） */
    kbCompilationEnabled,
    /** 配置是否已从后端加载完成 */
    configLoaded: !loading,
    importFile,
    importDirectory,
    importText,
  }
}
