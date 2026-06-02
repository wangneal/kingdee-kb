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
import {
  getWhisperStatus,
  type IngestionResult,
  ingestDirectory,
  ingestFile,
  ingestText,
  listenVideoProgress,
  loadWhisperModel,
  transcribeAndIngestVideo,
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

export default function Import() {
  // Text import state
  const [textTitle, setTextTitle] = useState("")
  const [textContent, setTextContent] = useState("")
  const [textProject, setTextProject] = useState("default")
  const [textCustomProject, setTextCustomProject] = useState("")
  const [textFeedback, setTextFeedback] = useState<ImportFeedback | null>(null)

  // File/folder import state
  const [fileFeedback, setFileFeedback] = useState<ImportFeedback | null>(null)
  const [fileProject, setFileProject] = useState("default")
  const [fileCustomProject, setFileCustomProject] = useState("")
  const [isDragging, setIsDragging] = useState(false)

  // Video transcription state
  const [videoFeedback, setVideoFeedback] = useState<ImportFeedback | null>(null)
  const [videoProject, setVideoProject] = useState("default")
  const [videoCustomProject, setVideoCustomProject] = useState("")
  const [videoGeneratingMinutes, setVideoGeneratingMinutes] = useState(true)
  const [videoResult, setVideoResult] = useState<VideoPipelineResult | null>(null)
  const [whisperReady, setWhisperReady] = useState(false)
  const [whisperLoading, setWhisperLoading] = useState(false)
  const [whisperError, setWhisperError] = useState<string | null>(null)
  const [videoProgress, setVideoProgress] = useState<VideoProgressEvent | null>(null)
  const [copyOk, setCopyOk] = useState<string | null>(null) // "transcript" | "minutes"
  const progressRef = useRef<VideoProgressEvent | null>(null)

  // Check Whisper model status on mount
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
  }, [])

  // Listen for video progress events
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

  // Load Whisper model
  const handleLoadWhisper = useCallback(async () => {
    setWhisperLoading(true)
    setWhisperError(null)
    try {
      await loadWhisperModel("tiny")
      const status = await getWhisperStatus()
      setWhisperReady(status.model_loaded)
    } catch (err) {
      setWhisperReady(false)
      setWhisperError(String(err))
    } finally {
      setWhisperLoading(false)
    }
  }, [])

  // Project options
  const projectOptions = [
    { value: "default", label: "默认" },
    { value: "enterprise", label: "企业版" },
    { value: "flagship", label: "旗舰版" },
    { value: "custom", label: "自定义..." },
  ]

  const getProjectName = (project: string, custom: string) => {
    if (project === "custom") return custom.trim() || "default"
    return project
  }

  // Handle text import
  const handleTextImport = useCallback(async () => {
    if (!textContent.trim() || !textTitle.trim()) return
    setTextFeedback({ status: "loading", message: "正在导入文本…" })
    try {
      const project = getProjectName(textProject, textCustomProject)
      const result = await ingestText(textContent, textTitle, project)
      setTextFeedback({
        status: "success",
        message: `导入成功：${result.title}，共 ${result.chunk_count} 个片段`,
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
  }, [textContent, textTitle, textProject, textCustomProject])

  // Handle file import via dialog
  const handleFileImport = useCallback(async () => {
    setFileFeedback({ status: "loading", message: "正在选择文件…" })
    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: "文档",
            extensions: ["md", "txt", "html", "pdf", "docx", "xlsx", "xls"],
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
      const project = getProjectName(fileProject, fileCustomProject)
      const results: IngestionResult[] = []
      const errors: string[] = []
      for (const path of paths) {
        try {
          const result = await ingestFile(path, project)
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
  }, [fileProject, fileCustomProject])

  // Handle folder import via dialog
  const handleFolderImport = useCallback(async () => {
    setFileFeedback({ status: "loading", message: "正在选择文件夹…" })
    try {
      const selected = await open({
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
      const project = getProjectName(fileProject, fileCustomProject)
      const result = await ingestDirectory(selected, project)
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
  }, [fileProject, fileCustomProject])

  // Handle drag and drop - Tauri native implementation
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
        console.error("Failed to setup drag-drop:", e)
      }
    }

    setupDragDrop()

    return () => {
      if (unlisten) unlisten()
    }
  }, [fileProject])

  const handleFilesDrop = useCallback(
    async (paths: string[]) => {
      setFileFeedback({
        status: "loading",
        message: `正在导入 ${paths.length} 个文件…`,
      })

      const project = getProjectName(fileProject, fileCustomProject)
      const results: IngestionResult[] = []
      const errors: string[] = []
      for (const path of paths) {
        try {
          const result = await ingestFile(path, project)
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
    [fileProject, fileCustomProject],
  )

  const clearFeedback = (type: "text" | "file" | "video") => {
    if (type === "text") setTextFeedback(null)
    else if (type === "file") setFileFeedback(null)
    else setVideoFeedback(null)
  }

  // Handle video transcription
  const handleVideoImport = useCallback(async () => {
    const filePath = await open({
      multiple: false,
      filters: [
        {
          name: "视频/音频文件",
          extensions: ["mp4", "webm", "avi", "mov", "mkv", "flv", "wmv", "m4a", "mp3", "wav"],
        },
      ],
    })
    if (!filePath) return

    const proj = getProjectName(videoProject, videoCustomProject)
    setVideoFeedback({ status: "loading", message: "正在准备..." })
    setVideoResult(null)
    setVideoProgress(null)

    try {
      const result = await transcribeAndIngestVideo(
        filePath as string,
        proj,
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
        message: String(err),
      })
    }
  }, [videoProject, videoCustomProject, videoGeneratingMinutes])

  // Copy text to clipboard
  const copyToClipboard = useCallback(async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopyOk(label)
      setTimeout(() => setCopyOk(null), 2000)
    } catch {
      /* clipboard not available */
    }
  }, [])

  // Export text to file
  const exportToFile = useCallback(async (text: string, filename: string) => {
    const dest = await save({
      defaultPath: filename,
      filters: [{ name: "Markdown", extensions: ["md", "txt"] }],
    })
    if (!dest) return
    try {
      await invoke("export_report", { content: text, filePath: dest })
    } catch (err) {
      console.error("导出失败:", err)
    }
  }, [])

  return (
    <div className="p-6">
      <h1 className="text-lg font-semibold text-neutral-800 mb-6">导入知识</h1>

      {/* Text Import Section */}
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
          <div className="flex items-center gap-2">
            <select
              value={textProject}
              onChange={(e) => setTextProject(e.target.value)}
              className="rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            >
              {projectOptions.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
            {textProject === "custom" && (
              <input
                type="text"
                placeholder="输入项目名称"
                value={textCustomProject}
                onChange={(e) => setTextCustomProject(e.target.value)}
                className="flex-1 rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              />
            )}
          </div>
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

        {/* Text import feedback */}
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

      {/* File/Folder Import Section */}
      <section className="rounded-lg border border-neutral-200 bg-white p-5">
        <h2 className="flex items-center gap-2 text-sm font-medium text-neutral-700 mb-4">
          <FileText className="h-4 w-4 text-[#1A6BD8]" />
          文件导入
        </h2>

        {/* Drag and drop zone */}
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
            支持 Markdown、TXT、HTML、PDF、DOCX、Excel
          </p>
        </div>

        {/* File picker buttons */}
        <div className="flex items-center gap-3 mb-3">
          <select
            value={fileProject}
            onChange={(e) => setFileProject(e.target.value)}
            className="rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
          >
            {projectOptions.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
          {fileProject === "custom" && (
            <input
              type="text"
              placeholder="输入项目名称"
              value={fileCustomProject}
              onChange={(e) => setFileCustomProject(e.target.value)}
              className="flex-1 rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />
          )}
        </div>
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

        {/* File import feedback */}
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
                  </li>
                ))}
              </ul>
            )}
            {fileFeedback.errors && fileFeedback.errors.length > 0 && (
              <div className="mt-2 space-y-1 text-xs text-red-600">
                <p className="font-medium">导入失败的文件：</p>
                {fileFeedback.errors.slice(0, 10).map((e, i) => (
                  <p key={i} className="truncate">
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

      {/* Video/Audio Transcription Section */}
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
          <label className="block text-xs text-neutral-500 mb-1">语音识别引擎</label>
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

        {/* Project selector */}
        <div className="flex items-center gap-2 mb-3">
          <select
            value={videoProject}
            onChange={(e) => setVideoProject(e.target.value)}
            className="rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 outline-none focus:border-purple-400 focus:ring-1 focus:ring-purple-400/20"
          >
            {projectOptions.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
          {videoProject === "custom" && (
            <input
              type="text"
              placeholder="输入项目名称"
              value={videoCustomProject}
              onChange={(e) => setVideoCustomProject(e.target.value)}
              className="flex-1 rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-purple-400 focus:ring-1 focus:ring-purple-400/20"
            />
          )}
        </div>

        {/* Generate minutes toggle */}
        <label className="flex items-center gap-2 mb-3 text-sm text-neutral-700">
          <input
            type="checkbox"
            checked={videoGeneratingMinutes}
            onChange={(e) => setVideoGeneratingMinutes(e.target.checked)}
            className="rounded border-neutral-300 text-purple-600 focus:ring-purple-400"
          />
          自动生成会议纪要
        </label>

        {/* Import button */}
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

        {/* Feedback + Progress */}
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
            {/* Progress bar */}
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

        {/* Transcription result — editable + copy/export */}
        {videoResult && videoResult.transcription.text && (
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

        {/* Meeting minutes — editable + copy/export */}
        {videoResult?.meeting_minutes && (
          <div className="mt-3">
            <div className="flex items-center justify-between mb-1">
              <span className="text-xs font-medium text-neutral-600">
                会议纪要（耗时 {(videoResult.meeting_minutes.generation_time_ms / 1000).toFixed(1)}
                s）
              </span>
              <div className="flex gap-1">
                <button
                  type="button"
                  onClick={() => copyToClipboard(videoResult.meeting_minutes!.minutes, "minutes")}
                  className="flex items-center gap-1 rounded px-2 py-0.5 text-xs text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700 transition-colors"
                  title="复制会议纪要"
                >
                  <Copy className="h-3 w-3" />
                  {copyOk === "minutes" ? "已复制" : "复制"}
                </button>
                <button
                  type="button"
                  onClick={() =>
                    exportToFile(videoResult.meeting_minutes!.minutes, "meeting-minutes.md")
                  }
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
              value={videoResult.meeting_minutes.minutes}
              rows={10}
              className="w-full rounded-md bg-neutral-50 p-3 text-xs text-neutral-600 border border-neutral-200 outline-none resize-y focus:border-purple-300"
            />
          </div>
        )}
      </section>
    </div>
  )
}
