import { useState, useEffect, useMemo } from "react";
import {
  FileText,
  FolderOpen,
  ChevronRight,
  ChevronDown,
  Trash2,
  Tag,
  Hash,
  CheckSquare,
  Square,
  XSquare,
} from "lucide-react";
import {
  listDocuments,
  getDocumentChunks,
  deleteDocument,
  deleteDocumentsBatch,
  type DocumentMeta,
  type ChunkMeta,
} from "../lib/tauri-commands";

interface ProjectGroup {
  project: string;
  documents: DocumentMeta[];
}

/** Parse the tags field: stored as JSON array string like `["tag1","tag2"]` in SQLite. */
function parseTags(tags: string): string[] {
  try {
    const parsed = JSON.parse(tags);
    if (Array.isArray(parsed)) {
      return parsed.filter((t): t is string => typeof t === "string" && t.length > 0);
    }
  } catch {
    // Not valid JSON — treat as plain text
  }
  // Fallback: return as single-element array if non-empty
  return tags.trim().length > 0 ? [tags.trim()] : [];
}

export default function Browse() {
  const [documents, setDocuments] = useState<DocumentMeta[]>([]);
  const [selectedDoc, setSelectedDoc] = useState<DocumentMeta | null>(null);
  const [selectedDocs, setSelectedDocs] = useState<Set<number>>(new Set());
  const [chunks, setChunks] = useState<ChunkMeta[]>([]);
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);
  const [tagFilter, setTagFilter] = useState("");
  const [batchDeleting, setBatchDeleting] = useState(false);

  // Group documents by project
  const projectGroups = useMemo<ProjectGroup[]>(() => {
    const map = new Map<string, DocumentMeta[]>();
    for (const doc of documents) {
      const list = map.get(doc.project) ?? [];
      list.push(doc);
      map.set(doc.project, list);
    }
    return Array.from(map.entries())
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([project, docs]) => ({ project, documents: docs }));
  }, [documents]);

  // Load documents on mount
  useEffect(() => {
    (async () => {
      try {
        const docs = await listDocuments();
        setDocuments(docs);
        // Auto-expand first project
        if (docs.length > 0) {
          setExpandedProjects(new Set([docs[0].project]));
        }
      } catch (e) {
        console.error("Failed to load documents:", e);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  // Load chunks when document is selected
  useEffect(() => {
    if (!selectedDoc) {
      setChunks([]);
      return;
    }
    (async () => {
      try {
        const result = await getDocumentChunks(selectedDoc.id);
        setChunks(result);
      } catch (e) {
        console.error("Failed to load chunks:", e);
      }
    })();
  }, [selectedDoc]);

  // Filter chunks by tag
  const filteredChunks = useMemo(() => {
    if (!tagFilter.trim()) return chunks;
    const lower = tagFilter.toLowerCase();
    return chunks.filter(
      (c) =>
        c.tags?.toLowerCase().includes(lower) ||
        c.section_path?.toLowerCase().includes(lower)
    );
  }, [chunks, tagFilter]);

  const toggleProject = (project: string) => {
    setExpandedProjects((prev) => {
      const next = new Set(prev);
      if (next.has(project)) next.delete(project);
      else next.add(project);
      return next;
    });
  };

  const handleDelete = async (doc: DocumentMeta) => {
    if (!confirm(`确定删除文档「${doc.title}」？`)) return;
    try {
      await deleteDocument(doc.id);
      setDocuments((prev) => prev.filter((d) => d.id !== doc.id));
      if (selectedDoc?.id === doc.id) setSelectedDoc(null);
    } catch (e) {
      console.error("Delete failed:", e);
    }
  };

  /** Toggle a single document in/out of the selection set */
  const toggleDocSelection = (id: number) => {
    setSelectedDocs((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  /** Select / deselect all visible documents */
  const toggleSelectAll = () => {
    if (selectedDocs.size === documents.length) {
      setSelectedDocs(new Set());
    } else {
      setSelectedDocs(new Set(documents.map((d) => d.id)));
    }
  };

  /** Batch-delete all selected documents */
  const handleBatchDelete = async () => {
    if (selectedDocs.size === 0) return;
    const count = selectedDocs.size;
    if (!confirm(`确定要删除选中的 ${count} 个文档吗？此操作不可撤销。`)) return;
    setBatchDeleting(true);
    try {
      const ids = Array.from(selectedDocs);
      await deleteDocumentsBatch(ids);
      setSelectedDocs(new Set());
      if (selectedDoc !== null && ids.includes(selectedDoc.id)) {
        setSelectedDoc(null);
        setChunks([]);
      }
      // Reload document list
      const docs = await listDocuments();
      setDocuments(docs);
    } catch (err) {
      console.error("Batch delete failed:", err);
      alert("批量删除失败：" + err);
    } finally {
      setBatchDeleting(false);
    }
  };

  return (
    <div className="flex h-full">
      {/* Left panel - Document tree */}
      <div className="w-72 shrink-0 border-r border-neutral-200 bg-white overflow-auto">
        <div className="sticky top-0 border-b border-neutral-100 bg-white px-4 py-3">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-semibold text-neutral-700">知识库</h2>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={toggleSelectAll}
                className="text-neutral-400 hover:text-neutral-600"
                title={selectedDocs.size === documents.length ? "取消全选" : "全选"}
              >
                {selectedDocs.size === documents.length && documents.length > 0 ? (
                  <CheckSquare size={15} className="text-blue-500" />
                ) : (
                  <Square size={15} />
                )}
              </button>
              {selectedDocs.size > 0 && (
                <>
                  <span className="text-xs text-blue-600 font-medium">
                    已选 {selectedDocs.size}
                  </span>
                  <button
                    type="button"
                    onClick={handleBatchDelete}
                    disabled={batchDeleting}
                    className="text-red-400 hover:text-red-600 disabled:opacity-50"
                    title="批量删除"
                  >
                    <Trash2 size={15} />
                  </button>
                  <button
                    type="button"
                    onClick={() => setSelectedDocs(new Set())}
                    className="text-neutral-400 hover:text-neutral-600"
                    title="取消选择"
                  >
                    <XSquare size={15} />
                  </button>
                </>
              )}
            </div>
          </div>
          <p className="text-xs text-neutral-400 mt-0.5">
            {documents.length} 篇文档
          </p>
        </div>

        {loading ? (
          <div className="p-4 text-sm text-neutral-400">加载中…</div>
        ) : projectGroups.length === 0 ? (
          <div className="p-4 text-sm text-neutral-400">暂无文档，请先导入</div>
        ) : (
          <div className="py-1">
            {projectGroups.map(({ project, documents: docs }) => (
              <div key={project}>
                {/* Project header */}
                <button
                  type="button"
                  onClick={() => toggleProject(project)}
                  className="flex w-full items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-neutral-500 hover:bg-neutral-50"
                >
                  {expandedProjects.has(project) ? (
                    <ChevronDown className="h-3 w-3" />
                  ) : (
                    <ChevronRight className="h-3 w-3" />
                  )}
                  <FolderOpen className="h-3.5 w-3.5 text-amber-500" />
                  <span className="truncate">{project}</span>
                  <span className="ml-auto text-neutral-400">{docs.length}</span>
                </button>

                {/* Documents */}
                {expandedProjects.has(project) && (
                  <div className="ml-3">
                    {docs.map((doc) => (
                      <div
                        key={doc.id}
                        className={`group flex items-center gap-1.5 px-2 py-1.5 rounded-lg cursor-pointer text-sm transition-colors ${
                          selectedDoc?.id === doc.id
                            ? "bg-blue-50 text-blue-700 font-medium"
                            : "hover:bg-slate-100 text-slate-600"
                        }`}
                        onClick={() => setSelectedDoc(doc)}
                      >
                        {/* checkbox */}
                        <span
                          className="flex-shrink-0"
                          onClick={(e) => {
                            e.stopPropagation();
                            toggleDocSelection(doc.id);
                          }}
                        >
                          {selectedDocs.has(doc.id) ? (
                            <CheckSquare size={15} className="text-blue-500" />
                          ) : (
                            <Square size={15} className="text-slate-300 hover:text-slate-500" />
                          )}
                        </span>
                        <FileText size={14} className="flex-shrink-0" />
                        <span className="truncate flex-1">{doc.title}</span>
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            handleDelete(doc);
                          }}
                          className="opacity-0 group-hover:opacity-100 text-neutral-400 hover:text-red-500 transition-opacity"
                          title="删除"
                        >
                          <Trash2 className="h-3 w-3" />
                        </button>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Right panel - Document preview */}
      <div className="flex-1 overflow-auto bg-neutral-50">
        {!selectedDoc ? (
          <div className="flex h-full items-center justify-center text-neutral-400">
            <div className="text-center">
              <FileText className="mx-auto h-12 w-12 text-neutral-300" />
              <p className="mt-2 text-sm">选择左侧文档查看内容</p>
            </div>
          </div>
        ) : (
          <div className="mx-auto max-w-6xl p-6">
            {/* Document header */}
            <div className="mb-6">
              <h1 className="text-lg font-semibold text-neutral-800">
                {selectedDoc.title}
              </h1>
              <div className="mt-2 flex flex-wrap items-center gap-3 text-xs text-neutral-500">
                <span className="flex items-center gap-1">
                  <FolderOpen className="h-3 w-3" />
                  {selectedDoc.project}
                </span>
                {selectedDoc.source_path && (
                  <span className="flex items-center gap-1">
                    <FileText className="h-3 w-3" />
                    {selectedDoc.source_path}
                  </span>
                )}
                <span className="flex items-center gap-1">
                  <Hash className="h-3 w-3" />
                  {chunks.length} 个片段
                </span>
              </div>
            </div>

            {/* Tag filter */}
            <div className="mb-4 flex items-center gap-2">
              <Tag className="h-4 w-4 text-neutral-400" />
              <input
                type="text"
                placeholder="按标签/章节筛选…"
                value={tagFilter}
                onChange={(e) => setTagFilter(e.target.value)}
                className="rounded-md border border-neutral-200 bg-white px-3 py-1.5 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
              />
              {tagFilter && (
                <span className="text-xs text-neutral-400">
                  {filteredChunks.length}/{chunks.length}
                </span>
              )}
            </div>

            {/* Chunks */}
            <div className="space-y-4">
              {filteredChunks.map((chunk) => (
                <div
                  key={chunk.id}
                  className="rounded-lg border border-neutral-200 bg-white p-4"
                >
                  {/* Chunk metadata */}
                  <div className="mb-2 flex flex-wrap items-center gap-2 text-xs text-neutral-500">
                    {chunk.section_path && (
                      <span className="rounded bg-neutral-100 px-1.5 py-0.5 font-medium">
                        {chunk.section_path}
                      </span>
                    )}
                    {chunk.tags && parseTags(chunk.tags).map((tag) => (
                      <span key={tag} className="rounded bg-[#1A6BD8]/10 px-1.5 py-0.5 text-[#1A6BD8]">
                        {tag}
                      </span>
                    ))}
                    {chunk.line_no != null && (
                      <span>L{chunk.line_no}</span>
                    )}
                  </div>
                  {/* Chunk content */}
                  <pre className="whitespace-pre-wrap text-sm leading-relaxed text-neutral-700 font-sans">
                    {chunk.content}
                  </pre>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
