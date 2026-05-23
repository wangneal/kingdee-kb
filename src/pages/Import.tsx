import { useState, useCallback } from "react";
import {
  Upload,
  FileText,
  FolderOpen,
  ClipboardPaste,
  Loader2,
  CheckCircle2,
  AlertCircle,
  X,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ingestText,
  ingestFile,
  ingestDirectory,
  type IngestionResult,
} from "../lib/tauri-commands";

type ImportStatus = "idle" | "loading" | "success" | "error";

interface ImportFeedback {
  status: ImportStatus;
  message: string;
  results?: IngestionResult[];
}

export default function Import() {
  // Text import state
  const [textTitle, setTextTitle] = useState("");
  const [textContent, setTextContent] = useState("");
  const [textProject, setTextProject] = useState("default");
  const [textFeedback, setTextFeedback] = useState<ImportFeedback | null>(null);

  // File/folder import state
  const [fileFeedback, setFileFeedback] = useState<ImportFeedback | null>(null);
  const [fileProject, setFileProject] = useState("default");
  const [isDragging, setIsDragging] = useState(false);

  // Handle text import
  const handleTextImport = useCallback(async () => {
    if (!textContent.trim() || !textTitle.trim()) return;
    setTextFeedback({ status: "loading", message: "正在导入文本…" });
    try {
      const result = await ingestText(textContent, textTitle, textProject || "default");
      setTextFeedback({
        status: "success",
        message: `导入成功：${result.title}，共 ${result.chunk_count} 个片段`,
        results: [result],
      });
      setTextContent("");
      setTextTitle("");
    } catch (e) {
      setTextFeedback({
        status: "error",
        message: `导入失败：${e}`,
      });
    }
  }, [textContent, textTitle, textProject]);

  // Handle file import via dialog
  const handleFileImport = useCallback(async () => {
    setFileFeedback({ status: "loading", message: "正在选择文件…" });
    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: "文档",
            extensions: ["md", "txt", "pdf", "docx", "html"],
          },
        ],
      });
      if (!selected) {
        setFileFeedback(null);
        return;
      }
      const paths = Array.isArray(selected) ? selected : [selected];
      setFileFeedback({
        status: "loading",
        message: `正在导入 ${paths.length} 个文件…`,
      });
      const results: IngestionResult[] = [];
      for (const path of paths) {
        const result = await ingestFile(path, fileProject || "default");
        results.push(result);
      }
      setFileFeedback({
        status: "success",
        message: `成功导入 ${results.length} 个文件`,
        results,
      });
    } catch (e) {
      setFileFeedback({
        status: "error",
        message: `导入失败：${e}`,
      });
    }
  }, []);

  // Handle folder import via dialog
  const handleFolderImport = useCallback(async () => {
    setFileFeedback({ status: "loading", message: "正在选择文件夹…" });
    try {
      const selected = await open({
        directory: true,
      });
      if (!selected) {
        setFileFeedback(null);
        return;
      }
      setFileFeedback({
        status: "loading",
        message: `正在导入文件夹：${selected}…`,
      });
      const results = await ingestDirectory(selected, fileProject || "default");
      setFileFeedback({
        status: "success",
        message: `成功导入 ${results.length} 个文件`,
        results,
      });
    } catch (e) {
      setFileFeedback({
        status: "error",
        message: `导入失败：${e}`,
      });
    }
  }, []);

  // Handle drag and drop
  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
  }, []);

  const handleDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragging(false);

      const files = Array.from(e.dataTransfer.files);
      if (files.length === 0) return;

      // Note: In Tauri, we need to get the file path from the native drop
      // For now, we'll use the file name as a fallback
      // The actual implementation would need Tauri's drag-drop API
      setFileFeedback({
        status: "loading",
        message: `正在处理 ${files.length} 个文件…`,
      });

      // Tauri provides file paths via the drag-drop event
      // We'll need to use the Tauri event system for proper file paths
      // For now, show a message that drag-drop needs native integration
      setFileFeedback({
        status: "error",
        message: "拖拽导入需要使用文件选择器或文件夹选择器",
      });
    },
    []
  );

  const clearFeedback = (type: "text" | "file") => {
    if (type === "text") setTextFeedback(null);
    else setFileFeedback(null);
  };

  return (
    <div className="mx-auto max-w-3xl p-6">
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
          <input
            type="text"
            placeholder="项目名称（如：星达铜业、default）"
            value={textProject}
            onChange={(e) => setTextProject(e.target.value)}
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
                !textTitle.trim() ||
                !textContent.trim() ||
                textFeedback?.status === "loading"
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
        <button
          type="button"
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
          className={`mb-4 w-full rounded-lg border-2 border-dashed p-8 text-center transition-colors ${
            isDragging
              ? "border-[#1A6BD8] bg-[#1A6BD8]/5"
              : "border-neutral-300 hover:border-neutral-400"
          }`}
        >
          <Upload
            className={`mx-auto h-8 w-8 mb-2 ${
              isDragging ? "text-[#1A6BD8]" : "text-neutral-400"
            }`}
          />
          <p className="text-sm text-neutral-600">
            拖拽文件到此处
          </p>
          <p className="text-xs text-neutral-400 mt-1">
            支持 Markdown、TXT、PDF、DOCX、HTML
          </p>
        </button>

        {/* File picker buttons */}
        <div className="flex items-center gap-3 mb-3">
          <input
            type="text"
            placeholder="项目名称（默认：default）"
            value={fileProject}
            onChange={(e) => setFileProject(e.target.value)}
            className="w-48 rounded-md border border-neutral-200 px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
          />
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
                    {r.title} — {r.chunk_count} 个片段
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
