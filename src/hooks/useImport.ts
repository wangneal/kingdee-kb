/**
 * 可复用的文档导入 Hook
 *
 * 封装文本/文件/文件夹导入的核心逻辑，
 * 自动从后端读取知识编译配置，确保配置加载完成前不执行导入。
 */
import { useCallback, useEffect, useRef, useState } from "react"
import {
  type DirectoryIngestionResult,
  getKbCompilationEnabled,
  type IngestionResult,
  ingestDirectory,
  ingestFile,
  ingestText,
} from "../lib/tauri-commands"

export function useImport() {
  // ── 知识编译配置（等待加载完成后再允许导入） ──
  const [kbCompilationEnabled, setKbCompilationEnabled] = useState(false)
  const [configLoaded, setConfigLoaded] = useState(false)
  const configRef = useRef(false)
  const configLoadedRef = useRef(false)

  useEffect(() => {
    let cancelled = false
    getKbCompilationEnabled()
      .then((enabled) => {
        if (cancelled) return
        setKbCompilationEnabled(enabled)
        configRef.current = enabled
        configLoadedRef.current = true
        setConfigLoaded(true)
      })
      .catch(() => {
        if (cancelled) return
        // 读取失败时使用默认值 false，允许导入继续
        configLoadedRef.current = true
        setConfigLoaded(true)
      })
    return () => {
      cancelled = true
    }
  }, [])

  /** 确保配置已加载，未加载时等待 */
  const ensureConfigLoaded = useCallback(async () => {
    if (configLoadedRef.current) return
    // 最多等待 3 秒
    for (let i = 0; i < 30; i++) {
      await new Promise((r) => setTimeout(r, 100))
      if (configLoadedRef.current) return
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
    configLoaded,
    importFile,
    importDirectory,
    importText,
  }
}
