import { invoke } from "@tauri-apps/api/core"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import { open, save } from "@tauri-apps/plugin-dialog"
import {
  AlertCircle,
  CheckCircle2,
  ClipboardPaste,
  Copy,
  Download,
  FileAudio,
  FileText,
  FolderOpen,
  Loader2,
  Upload,
  Video,
  X,
} from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { useProject } from "../contexts/ProjectContext"
import { getImportDialogDefaultPath } from "../lib/dialog-options"
import {
  checkFfmpegStatus,
  getKbCompilationEnabled,
  getWhisperStatus,
  type IngestionResult,
  ingestDirectory,
  ingestFile,
  ingestText,
  listenVideoProgress,
  loadWhisperModel,
  setKbCompilationEnabled,
  transcribeAndIngestVideo,
  type FfmpegStatus,
  type VideoPipelineResult,
  type VideoProgressEvent,
} from "../lib/tauri-commands"

type ImportStatus = "idle" | "loading" | "success" | "error"

interface ImportFeedback {
  status: ImportStatus
  message: string
  results?: IngestionResult[]
  errors?: FileError[]
}

interface FileError {
  path: string
  error: string
}

function formatImportSuccess(result: IngestionResult): string {
  const engineSuffix = result.kb_analysis_engine === "rust" ? "，快速分析模式（非 LLM）" : ""
  const suffix = result.kb_compilation_error ? `，知识编译失败：${result.kb_compilation_error}` : ""
  return `导入成功：${result.title}，共 ${result.chunk_count} 个片段${engineSuffix}${suffix}`
}

export default function Import() {
  const { currentProjectId } = useProject()
  // 文本导入状态
  const [textTitle, setTextTitle] = useState("")
  const [textContent, setTextContent] = useState("")
  const [textFeedback, setTextFeedback] = useState<ImportFeedback | null>(null)

  // 文件和文件夹导入状态
  const [fileFeedback, setFileFeedback] = useState<ImportFeedback | null>(null)
  const [isDragging, setIsDragging] = useState(false)

  // 视频转写状态
  const [videoFeedback, setVideoFeedback] = useState<ImportFeedback | null>(null)
  const [videoGeneratingMinutes, setVideoGeneratingMinutes] = useState(true)
  const [videoResult, setVideoResult] = useState<VideoPipelineResult | null>(null)
  const [whisperReady, setWhisperReady] = useState(false)
  const [whisperLoading, setWhisperLoading] = useState(false)
  const [whisperError, setWhisperError] = useState<string | null>(null)
  const [ffmpegStatus, setFfmpegStatus] = useState<FfmpegStatus | null>(null)
  const [videoProgress, setVideoProgress] = useState<VideoProgressEvent | null>(null)
  const [copyOk, setCopyOk] = useState<string | null>(null) // 转写稿或会议纪要
  const progressRef = useRef<VideoProgressEvent | null>(null)
  const [kbCompilationEnabled, setKbCompilationEnabledState] = useState(false)
  const [kbCompilationSaving, setKbCompilationSaving] = useState(false)

  // 初始化 Whisper 和知识编译状态
  useEffect(() => {
    getWhisperStatus()
      .then((status) => {
        setWhisperReady(status.model_loaded)
        setWhisperError(null)
      })
      .catch((err) => {
        setWhisperReady(false)
        setWhisperError(String(err))
      })

    checkFfmpegStatus()
      .then(setFfmpegStatus)
      .catch(() => setFfmpegStatus(null))

    getKbCompilationEnabled()
      .then(setKbCompilationEnabledState)
      .catch(() => setKbCompilationEnabledState(false))
  }, [])

  // 监听视频处理进度
  useEffect(() => {
    let unlisten: (() => void) | null = null
    listenVideoProgress((event) => {
      progressRef.current = event
      setVideoProgress(event)
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      if (unlisten) unlisten()
    }
  }, [])

  // 加载 Whisper 模型
  const handleLoadWhisper = useCallback(async () => {
    setWhisperLoading(true)
    setWhisperError(null)
    try {
      await loadWhisperModel("base")
      const status = await getWhisperStatus()
      setWhisperReady(status.model_loaded)
    } catch (err) {
      setWhisperReady(false)
      setWhisperError(String(err))
    } finally {
      setWhisperLoading(false)
    }
  }, [])

  const handleKbCompilationToggle = useCallback(async (enabled: boolean) => {
    setKbCompilationEnabledState(enabled)
    setKbCompilationSaving(true)
    try {
      await setKbCompilationEnabled(enabled)
    } catch {
      setKbCompilationEnabledState(!enabled)
    } finally {
      setKbCompilationSaving(false)
    }
  }, [])

  // 文本导入
  const handleTextImport = useCallback(async () => {
    if (!textContent.trim() || !textTitle.trim()) return
    setTextFeedback({ status: "loading", message: "正在导入文本…" })
    try {
      if (currentProjectId == null) throw new Error("当前项目未就绪")
      const result = await ingestText(
        textContent,
        textTitle,
        currentProjectId,
        kbCompilationEnabled,
      )
      setTextFeedback({
        status: "success",
        message: formatImportSuccess(result),
        results: [result],
      })
      setTextContent("")
      setTextTitle("")
    } catch (e) {
      setTextFeedback({
        status: "error",
        message: `导入失败：${e}`,
      })
    }
  }, [currentProjectId, textContent, textTitle, kbCompilationEnabled])

  // 通过对话框导入文件
  const handleFileImport = useCallback(async () => {
    setFileFeedback({ status: "loading", message: "正在选择文件…" })
    try {
      const defaultPath = await getImportDialogDefaultPath()
      const selected = await open({
        title: "选择要导入的文件",
        defaultPath,
        multiple: true,
        filters: [
          {
            name: "文档",
            extensions: ["md", "txt", "html", "pdf", "docx", "xlsx", "xls", "vsdx", "vsd"],
          },
        ],
      })
      if (!selected) {
        setFileFeedback(null)
        return
      }
      const paths = Array.isArray(selected) ? selected : [selected]
      setFileFeedback({
        status: "loading",
        message: `正在导入 ${paths.length} 个文件…`,
      })
      if (currentProjectId == null) throw new Error("当前项目未就绪")
      const results: IngestionResult[] = []
      const errors: string[] = []
      for (const path of paths) {
        try {
          const result = await ingestFile(path, currentProjectId, kbCompilationEnabled)
          results.push(result)
        } catch (err) {
          const filename = path.split(/[\\/]/).pop() || path
          errors.push(`${filename}: ${err}`)
        }
      }
      if (results.length > 0) {
        setFileFeedback({
          status: "success",
          message:
            errors.length > 0
              ? `成功导入 ${results.length} 个文件，${errors.length} 个失败`
              : `成功导入 ${results.length} 个文件`,
          results,
        })
      } else {
        setFileFeedback({
          status: "error",
          message:
            errors.length > 0
              ? `导入失败：${errors[0]}`
              : "没有文件被成功导入（文件夹中未找到支持的文件格式）",
        })
      }
    } catch (e) {
      setFileFeedback({
        status: "error",
        message: `导入失败：${e}`,
      })
    }
  }, [currentProjectId, kbCompilationEnabled])

  // 通过对话框导入文件夹
  const handleFolderImport = useCallback(async () => {
    setFileFeedback({ status: "loading", message: "正在选择文件夹…" })
    try {
      const defaultPath = await getImportDialogDefaultPath()
      const selected = await open({
        title: "选择要导入的文件夹",
        defaultPath,
        directory: true,
      })
      if (!selected) {
        setFileFeedback(null)
        return
      }
      setFileFeedback({
        status: "loading",
        message: `正在导入文件夹：${selected}…`,
      })
      if (currentProjectId == null) throw new Error("当前项目未就绪")
      const result = await ingestDirectory(selected, currentProjectId, kbCompilationEnabled)
      const { imported, errors } = result
      if (imported.length > 0) {
        setFileFeedback({
          status: "success",
          message:
            errors.length > 0
              ? `成功导入 ${imported.length} 个文件，${errors.length} 个失败`
              : `成功导入 ${imported.length} 个文件`,
          results: imported,
          errors,
        })
      } else if (errors.length > 0) {
        setFileFeedback({
          status: "error",
          message: `导入失败：${errors.length} 个文件均未成功。例如：${errors[0].error}`,
          errors,
        })
      } else {
        setFileFeedback({
          status: "error",
          message: "未找到支持的文件格式",
        })
      }
    } catch (e) {
      setFileFeedback({
        status: "error",
        message: `导入失败：${e}`,
      })
    }
  }, [currentProjectId, kbCompilationEnabled])

  const handleFilesDrop = useCallback(
    async (paths: string[]) => {
      setFileFeedback({
        status: "loading",
        message: `正在导入 ${paths.length} 个文件…`,
      })

      if (currentProjectId == null) {
        setFileFeedback({ status: "error", message: "当前项目未就绪" })
        return
      }
      const results: IngestionResult[] = []
      const errors: string[] = []
      for (const path of paths) {
        try {
          const result = await ingestFile(path, currentProjectId, kbCompilationEnabled)
          results.push(result)
        } catch (err) {
          const filename = path.split(/[\\/]/).pop() || path
          errors.push(`${filename}: ${err}`)
        }
      }

      if (results.length > 0) {
        setFileFeedback({
          status: "success",
          message:
            errors.length > 0
              ? `成功导入 ${results.length} 个文件，${errors.length} 个失败`
              : `成功导入 ${results.length} 个文件`,
          results,
        })
      } else {
        setFileFeedback({
          status: "error",
          message: errors.length > 0 ? `导入失败：${errors[0]}` : "没有文件被成功导入",
        })
      }
    },
    [currentProjectId, kbCompilationEnabled],
  )

  // Tauri 原生拖拽导入
  useEffect(() => {
    let unlisten: (() => void) | null = null

    const setupDragDrop = async () => {
      try {
        const webview = getCurrentWebview()
        unlisten = await webview.onDragDropEvent((event) => {
          if (event.payload.type === "over") {
            setIsDragging(true)
          } else if (event.payload.type === "drop") {
            setIsDragging(false)
            const paths = event.payload.paths
            if (paths && paths.length > 0) {
              handleFilesDrop(paths)
            }
          } else if (event.payload.type === "leave") {
            setIsDragging(false)
          }
        })
      } catch (e) {
        console.error("设置拖拽导入失败:", e)
      }
    }

    setupDragDrop()

    return () => {
      if (unlisten) unlisten()
    }
  }, [handleFilesDrop])

  const clearFeedback = (type: "text" | "file" | "video") => {
    if (type === "text") setTextFeedback(null)
    else if (type === "file") setFileFeedback(null)
    else setVideoFeedback(null)
  }

  // 视频转写导入
  const handleVideoImport = useCallback(async () => {
    setVideoFeedback({ status: "loading", message: "正在选择视频/音频文件…" })
    try {
      const defaultPath = await getImportDialogDefaultPath()
      const filePath = await open({
        title: "选择要转写的视频或音频文件",
        defaultPath,
        multiple: false,
        filters: [
          {
            name: "视频/音频文件",
            extensions: ["mp4", "webm", "avi", "mov", "mkv", "flv", "wmv", "m4a", "mp3", "wav"],
          },
        ],
      })
      if (!filePath) {
        setVideoFeedback(null)
        return
      }

      if (currentProjectId == null) {
        setVideoFeedback({ status: "error", message: "当前项目未就绪" })
        return
      }
      setVideoFeedback({ status: "loading", message: "正在准备..." })
      setVideoResult(null)
      setVideoProgress(null)

      const result = await transcribeAndIngestVideo(
        filePath as string,
        currentProjectId,
        videoGeneratingMinutes,
      )
      setVideoFeedback({
        status: "success",
        message: `转写完成！${result.transcription.duration_secs.toFixed(0)}秒视频 → ${result.transcription.text.length}字。已入库知识库。${result.meeting_minutes ? " 会议纪要已生成。" : ""}`,
      })
      setVideoResult(result)
    } catch (err) {
      setVideoFeedback({
        status: "error",
        message: `视频/音频处理失败：${err}`,
      })
    }
  }, [currentProjectId, videoGeneratingMinutes])

  // 复制文本
  const copyToClipboard = useCallback(async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopyOk(label)
      setTimeout(() => setCopyOk(null), 2000)
    } catch (err) {
      setVideoFeedback({ status: "error", message: `复制失败：${err}` })
    }
  }, [])

  // 导出文本
  const exportToFile = useCallback(async (text: string, filename: string) => {
    try {
      const dest = await save({
        defaultPath: filename,
        filters: [{ name: "Markdown", extensions: ["md", "txt"] }],
      })
      if (!dest) return
      await invoke("export_report", { content: text, filePath: dest })
      setVideoFeedback({ status: "success", message: `已导出到：${dest}` })
    } catch (err) {
      setVideoFeedback({ status: "error", message: `导出失败：${err}` })
    }
  }, [])

  const meetingMinutes = videoResult?.meeting_minutes ?? null

  return (
    <div className="p-6">
      <h1 className="text-lg font-semibold text-neutral-800 mb-6">导入知识</h1>

      <section className="mb-6 rounded-lg border border-neutral-200 bg-white px-5 py-4">
        <label className="flex items-center justify-between gap-4">
          <span>
            <span className="block text-sm font-medium text-neutral-700">知识编译</span>
            <span className="mt-1 block text-xs text-neutral-500">
              开启后导入会调用 LLM 生成 Wiki 候选页面，失败不会影响普通入库。
            </span>
          </span>
          <input
            type="checkbox"
            checked={kbCompilationEnabled}
            disabled={kbCompilationSaving}
            onChange={(e) => handleKbCompilationToggle(e.target.checked)}
            className="h-4 w-4 rounded border-neutral-300 text-[#1A6BD8] focus:ring-[#1A6BD8]"
          />
        </label>
      </section>

      {/* 文本导入区 */}
      <section className="mb-8 rounded-lg border border-neutral-200 bg-white p-5">
        <h2 className="flex items-center gap-2 text-sm font-medium text-neutral-700 mb-4">
          <ClipboardPaste className="h-4 w-4 text-[#1A6BD8]" />
          粘贴文本导入
        </h2>

        <div className="space-y-3">
          <input
            type="text"
            placeholder="文档标题"
            value={textTitle}
            onChange={(e) => setTextTitle(e.target.value)}
            className="w-full rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
          />
          <textarea
            placeholder="粘贴文本内容…"
            value={textContent}
            onChange={(e) => setTextContent(e.target.value)}
            rows={6}
            className="w-full rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20 resize-y"
          />
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleTextImport}
              disabled={
                !textTitle.trim() || !textContent.trim() || textFeedback?.status === "loading"
              }
              className="rounded-md bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {textFeedback?.status === "loading" ? (
                <Loader2 className="h-4 w-4 animate-spin inline mr-1" />
              ) : (
                <Upload className="h-4 w-4 inline mr-1" />
              )}
              导入
            </button>
            {textFeedback && (
              <button
                type="button"
                onClick={() => clearFeedback("text")}
                className="text-neutral-400 hover:text-neutral-600"
              >
                <X className="h-4 w-4" />
              </button>
            )}
          </div>
        </div>

        {/* 文本导入反馈 */}
        {textFeedback && (
          <div
            className={`mt-3 rounded-md p-3 text-sm ${
              textFeedback.status === "success"
                ? "bg-green-50 text-green-700"
                : textFeedback.status === "error"
                  ? "bg-red-50 text-red-700"
                  : "bg-blue-50 text-blue-700"
            }`}
          >
            <div className="flex items-center gap-2">
              {textFeedback.status === "success" ? (
                <CheckCircle2 className="h-4 w-4" />
              ) : textFeedback.status === "error" ? (
                <AlertCircle className="h-4 w-4" />
              ) : (
                <Loader2 className="h-4 w-4 animate-spin" />
              )}
              {textFeedback.message}
            </div>
          </div>
        )}
      </section>

      {/* 文件和文件夹导入区 */}
      <section className="rounded-lg border border-neutral-200 bg-white p-5">
        <h2 className="flex items-center gap-2 text-sm font-medium text-neutral-700 mb-4">
          <FileText className="h-4 w-4 text-[#1A6BD8]" />
          文件导入
        </h2>

        {/* 拖拽区域 */}
        <div
          className={`mb-4 w-full rounded-lg border-2 border-dashed p-8 text-center transition-colors ${
            isDragging
              ? "border-[#1A6BD8] bg-[#1A6BD8]/5"
              : "border-neutral-300 hover:border-neutral-400"
          }`}
        >
          <Upload
            className={`mx-auto h-8 w-8 mb-2 ${isDragging ? "text-[#1A6BD8]" : "text-neutral-400"}`}
          />
          <p className="text-sm text-neutral-600">拖拽文件到此处</p>
          <p className="text-xs text-neutral-400 mt-1">
            支持 Markdown、TXT、HTML、PDF、DOCX、Excel、Visio
          </p>
        </div>

        {/* 文件选择按钮 */}
        <div className="flex gap-3">
          <button
            type="button"
            onClick={handleFileImport}
            disabled={fileFeedback?.status === "loading"}
            className="flex items-center gap-2 rounded-md border border-neutral-200 bg-white px-4 py-2.5 text-sm font-medium text-neutral-700 hover:bg-neutral-50 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            <FileText className="h-4 w-4" />
            选择文件
          </button>
          <button
            type="button"
            onClick={handleFolderImport}
            disabled={fileFeedback?.status === "loading"}
            className="flex items-center gap-2 rounded-md border border-neutral-200 bg-white px-4 py-2.5 text-sm font-medium text-neutral-700 hover:bg-neutral-50 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            <FolderOpen className="h-4 w-4" />
            选择文件夹
          </button>
          {fileFeedback && (
            <button
              type="button"
              onClick={() => clearFeedback("file")}
              className="text-neutral-400 hover:text-neutral-600"
            >
              <X className="h-4 w-4" />
            </button>
          )}
        </div>

        {/* 文件导入反馈 */}
        {fileFeedback && (
          <div
            className={`mt-4 rounded-md p-3 text-sm ${
              fileFeedback.status === "success"
                ? "bg-green-50 text-green-700"
                : fileFeedback.status === "error"
                  ? "bg-red-50 text-red-700"
                  : "bg-blue-50 text-blue-700"
            }`}
          >
            <div className="flex items-center gap-2">
              {fileFeedback.status === "success" ? (
                <CheckCircle2 className="h-4 w-4" />
              ) : fileFeedback.status === "error" ? (
                <AlertCircle className="h-4 w-4" />
              ) : (
                <Loader2 className="h-4 w-4 animate-spin" />
              )}
              {fileFeedback.message}
            </div>
            {fileFeedback.results && fileFeedback.results.length > 0 && (
              <ul className="mt-2 space-y-1 text-xs">
                {fileFeedback.results.map((r) => (
                  <li key={r.document_id} className="flex items-center gap-1">
                    <CheckCircle2 className="h-3 w-3" />
                    {r.title}
                    {r.is_duplicate
                      ? ` — 已存在（${r.chunk_count} 个片段）`
                      : ` — ${r.chunk_count} 个片段`}
                    {r.kb_analysis_engine === "rust" ? "，快速分析模式（非 LLM）" : ""}
                    {r.kb_compilation_error ? `，知识编译失败：${r.kb_compilation_error}` : ""}
                  </li>
                ))}
              </ul>
            )}
            {fileFeedback.errors && fileFeedback.errors.length > 0 && (
              <div className="mt-2 space-y-1 text-xs text-red-600">
                <p className="font-medium">导入失败的文件：</p>
                {fileFeedback.errors.slice(0, 10).map((e) => (
                  <p key={`${e.path}:${e.error}`} className="truncate">
                    ⚠ {e.path}: {e.error}
                  </p>
                ))}
                {fileFeedback.errors.length > 10 && (
                  <p className="text-neutral-500">
                    …还有 {fileFeedback.errors.length - 10} 个文件未显示
                  </p>
                )}
              </div>
            )}
          </div>
        )}
      </section>

      {/* 视频和音频转写区 */}
      <section className="mt-8 rounded-lg border border-purple-200 bg-white p-5">
        <h2 className="flex items-center gap-2 text-sm font-medium text-neutral-700 mb-4">
          <Video className="h-4 w-4 text-purple-600" />
          视频/音频转写
        </h2>

        <p className="text-xs text-neutral-500 mb-3">
          导入录屏或音频文件，自动提取音频并转写为文字，支持入库和生成会议纪要。
        </p>

        {/* Whisper 模型状态 */}
        <div className="mb-3">
          <span className="block text-xs text-neutral-500 mb-1">语音识别引擎</span>
          <div className="flex items-center gap-2">
            <span className="rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 bg-neutral-50">
              Whisper (本地) {whisperReady ? "✓" : "⚠ 未加载"}
            </span>
            {!whisperReady && (
              <button
                type="button"
                onClick={handleLoadWhisper}
                disabled={whisperLoading}
                className="rounded border border-purple-300 px-2 py-1 text-xs text-purple-600 hover:bg-purple-50 disabled:opacity-50 transition-colors"
              >
                {whisperLoading ? (
                  <span className="flex items-center gap-1">
                    <Loader2 className="h-3 w-3 animate-spin" />
                    加载中...
                  </span>
                ) : (
                  "加载模型"
                )}
              </button>
            )}
          </div>
          {whisperError && (
            <p className="text-[10px] text-red-500 truncate max-w-lg mt-1" title={whisperError}>
              错误: {whisperError}
            </p>
          )}
        </div>

        {/* FFmpeg 状态 */}
        <div className="mb-3">
          <span className="block text-xs text-neutral-500 mb-1">音频提取引擎</span>
          <span className="inline-block rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 bg-neutral-50">
            FFmpeg {ffmpegStatus?.available ? "✓ 已就绪" : "○ 首次转写时自动下载（约 80MB）"}
          </span>
        </div>

        {/* 生成会议纪要开关 */}
        <label className="flex items-center gap-2 mb-3 text-sm text-neutral-700">
          <input
            type="checkbox"
            checked={videoGeneratingMinutes}
            onChange={(e) => setVideoGeneratingMinutes(e.target.checked)}
            className="rounded border-neutral-300 text-purple-600 focus:ring-purple-400"
          />
          自动生成会议纪要
        </label>

        {/* 导入按钮 */}
        <button
          type="button"
          onClick={handleVideoImport}
          disabled={!whisperReady || videoFeedback?.status === "loading"}
          className="flex items-center gap-2 rounded-md bg-purple-600 px-4 py-2.5 text-sm font-medium text-white hover:bg-purple-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {videoFeedback?.status === "loading" ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <FileAudio className="h-4 w-4" />
          )}
          选择视频/音频文件
        </button>

        {/* 反馈和进度 */}
        {videoFeedback && (
          <div
            className={`mt-4 rounded-md p-3 text-sm ${
              videoFeedback.status === "success"
                ? "bg-green-50 text-green-700"
                : videoFeedback.status === "error"
                  ? "bg-red-50 text-red-700"
                  : "bg-purple-50 text-purple-700"
            }`}
          >
            <div className="flex items-center gap-2">
              {videoFeedback.status === "success" ? (
                <CheckCircle2 className="h-4 w-4" />
              ) : videoFeedback.status === "error" ? (
                <AlertCircle className="h-4 w-4" />
              ) : (
                <Loader2 className="h-4 w-4 animate-spin" />
              )}
              {videoFeedback.status === "loading" && videoProgress
                ? videoProgress.message
                : videoFeedback.message}
              {videoFeedback && videoFeedback.status !== "loading" && (
                <button
                  type="button"
                  onClick={() => clearFeedback("video")}
                  className="ml-auto text-current opacity-50 hover:opacity-100"
                >
                  <X className="h-4 w-4" />
                </button>
              )}
            </div>
            {/* 进度条 */}
            {videoFeedback.status === "loading" && videoProgress && (
              <div className="mt-2">
                <div className="h-1.5 rounded-full bg-purple-200 overflow-hidden">
                  <div
                    className="h-full bg-purple-600 rounded-full transition-all duration-300"
                    style={{ width: `${Math.min(videoProgress.progress, 100)}%` }}
                  />
                </div>
              </div>
            )}
          </div>
        )}

        {/* 转写结果：可编辑、复制、导出 */}
        {videoResult?.transcription.text && (
          <div className="mt-4">
            <div className="flex items-center justify-between mb-1">
              <span className="text-xs font-medium text-neutral-600">
                转写结果（{videoResult.transcription.text.length}字，耗时{" "}
                {(
                  (videoResult.transcription.extraction_time_ms +
                    videoResult.transcription.transcription_time_ms) /
                  1000
                ).toFixed(1)}
                s）
              </span>
              <div className="flex gap-1">
                <button
                  type="button"
                  onClick={() => copyToClipboard(videoResult.transcription.text, "transcript")}
                  className="flex items-center gap-1 rounded px-2 py-0.5 text-xs text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700 transition-colors"
                  title="复制转写文本"
                >
                  <Copy className="h-3 w-3" />
                  {copyOk === "transcript" ? "已复制" : "复制"}
                </button>
                <button
                  type="button"
                  onClick={() => exportToFile(videoResult.transcription.text, "transcript.txt")}
                  className="flex items-center gap-1 rounded px-2 py-0.5 text-xs text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700 transition-colors"
                  title="导出转写文本"
                >
                  <Download className="h-3 w-3" />
                  导出
                </button>
              </div>
            </div>
            <textarea
              readOnly
              value={videoResult.transcription.text}
              rows={8}
              className="w-full rounded-md bg-neutral-50 p-3 text-xs text-neutral-600 border border-neutral-200 outline-none resize-y focus:border-purple-300"
            />
          </div>
        )}

        {/* 会议纪要：可编辑、复制、导出 */}
        {meetingMinutes && (
          <div className="mt-3">
            <div className="flex items-center justify-between mb-1">
              <span className="text-xs font-medium text-neutral-600">
                会议纪要（耗时 {(meetingMinutes.generation_time_ms / 1000).toFixed(1)}
                s）
              </span>
              <div className="flex gap-1">
                <button
                  type="button"
                  onClick={() => copyToClipboard(meetingMinutes.minutes, "minutes")}
                  className="flex items-center gap-1 rounded px-2 py-0.5 text-xs text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700 transition-colors"
                  title="复制会议纪要"
                >
                  <Copy className="h-3 w-3" />
                  {copyOk === "minutes" ? "已复制" : "复制"}
                </button>
                <button
                  type="button"
                  onClick={() => exportToFile(meetingMinutes.minutes, "meeting-minutes.md")}
                  className="flex items-center gap-1 rounded px-2 py-0.5 text-xs text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700 transition-colors"
                  title="导出会议纪要"
                >
                  <Download className="h-3 w-3" />
                  导出
                </button>
              </div>
            </div>
            <textarea
              readOnly
              value={meetingMinutes.minutes}
              rows={10}
              className="w-full rounded-md bg-neutral-50 p-3 text-xs text-neutral-600 border border-neutral-200 outline-none resize-y focus:border-purple-300"
            />
            {meetingMinutes.file_path && (
              <p className="text-[10px] text-neutral-400 truncate mt-1" title={meetingMinutes.file_path}>
                <FileText className="inline h-3 w-3 mr-0.5" />
                已保存：{meetingMinutes.file_path}
              </p>
            )}
          </div>
        )}
      </section>
    </div>
  )
}
