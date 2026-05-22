import { useState, useEffect, useMemo } from "react";
import {
  FileText,
  FolderOpen,
  ChevronRight,
  ChevronDown,
  Trash2,
  Tag,
  Hash,
} from "lucide-react";
import {
  listDocuments,
  getDocumentChunks,
  deleteDocument,
  type DocumentMeta,
  type ChunkMeta,
} from "../lib/tauri-commands";

interface ProjectGroup {
  project: string;
  documents: DocumentMeta[];
}

export default function Browse() {
  const [documents, setDocuments] = useState<DocumentMeta[]>([]);
  const [selectedDoc, setSelectedDoc] = useState<DocumentMeta | null>(null);
  const [chunks, setChunks] = useState<ChunkMeta[]>([]);
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);
  const [tagFilter, setTagFilter] = useState("");

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

  return (
    <div className="flex h-full">
      {/* Left panel - Document tree */}
      <div className="w-72 shrink-0 border-r border-neutral-200 bg-white overflow-auto">
        <div className="sticky top-0 border-b border-neutral-100 bg-white px-4 py-3">
          <h2 className="text-sm font-semibold text-neutral-700">知识库</h2>
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
                      <button
                        type="button"
                        key={doc.id}
                        onClick={() => setSelectedDoc(doc)}
                        className={`group flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm transition-colors ${
                          selectedDoc?.id === doc.id
                            ? "bg-[#1A6BD8]/10 text-[#1A6BD8]"
                            : "text-neutral-600 hover:bg-neutral-50"
                        }`}
                      >
                        <FileText className="h-3.5 w-3.5 shrink-0" />
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
                      </button>
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
          <div className="mx-auto max-w-3xl p-6">
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
                    {chunk.tags && (
                      <span className="rounded bg-[#1A6BD8]/10 px-1.5 py-0.5 text-[#1A6BD8]">
                        {chunk.tags}
                      </span>
                    )}
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
