import { useState, useCallback, useMemo } from "react";
import { Search as SearchIcon, FileText, Hash, Tag, Loader2, AlertCircle, RefreshCw } from "lucide-react";
import {
  hybridSearch,
  type HybridSearchResult,
} from "../lib/tauri-commands";
import { useProject } from "../contexts/ProjectContext";

function highlightText(text: string, query: string): React.ReactNode {
  if (!query.trim()) return text;
  const words = query
    .split(/\s+/)
    .filter(Boolean)
    .map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (words.length === 0) return text;
  const regex = new RegExp(`(${words.join("|")})`, "gi");
  const parts = text.split(regex);
  return parts.map((part) =>
    regex.test(part) ? (
      <mark key={part} className="bg-yellow-200 text-neutral-800 rounded-sm px-0.5">
        {part}
      </mark>
    ) : (
      part
    )
  );
}

export default function Search() {
  const { projectId } = useProject();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<HybridSearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [searched, setSearched] = useState(false);
  const [tagFilter, setTagFilter] = useState("");
  const [searchError, setSearchError] = useState<string | null>(null);

  const handleSearch = useCallback(
    async (e?: React.FormEvent) => {
      e?.preventDefault();
      if (!query.trim()) return;
      setLoading(true);
      setSearched(true);
      setSearchError(null);
      try {
        const res = await hybridSearch(query.trim(), projectId, 30);
        setResults(res);
        setSearchError(null);
      } catch (err) {
        console.error("Search failed:", err);
        setResults([]);
        setSearchError("搜索服务暂时不可用，请稍后重试");
      } finally {
        setLoading(false);
      }
    },
    [query]
  );

  const filteredResults = useMemo(() => {
    if (!tagFilter.trim()) return results;
    const lower = tagFilter.toLowerCase();
    return results.filter(
      (r) =>
        r.section_path?.toLowerCase().includes(lower) ||
        r.source.toLowerCase().includes(lower)
    );
  }, [results, tagFilter]);

  return (
      <div className="p-6">
      <h1 className="text-lg font-semibold text-neutral-800 mb-4">知识检索</h1>

      {/* Search form */}
      <form onSubmit={handleSearch} className="mb-6">
        <div className="flex gap-2">
          <div className="relative flex-1">
            <SearchIcon className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-neutral-400" />
            <input
              type="text"
              placeholder="输入关键词或自然语言问题…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="w-full rounded-lg border border-neutral-200 bg-white py-2.5 pl-10 pr-4 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-2 focus:ring-[#1A6BD8]/20"
            />
          </div>
          <button
            type="submit"
            disabled={loading || !query.trim()}
            className="rounded-lg bg-[#1A6BD8] px-5 py-2.5 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            {loading ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              "搜索"
            )}
          </button>
        </div>
      </form>

      {/* Tag filter */}
      {results.length > 0 && (
        <div className="mb-4 flex items-center gap-2">
          <Tag className="h-4 w-4 text-neutral-400" />
          <input
            type="text"
            placeholder="按来源/章节筛选…"
            value={tagFilter}
            onChange={(e) => setTagFilter(e.target.value)}
            className="rounded-md border border-neutral-200 bg-white px-3 py-1.5 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
          />
          {tagFilter && (
            <span className="text-xs text-neutral-400">
              {filteredResults.length}/{results.length}
            </span>
          )}
        </div>
      )}

      {/* Results */}
      {loading ? (
        <div className="flex items-center justify-center py-12 text-neutral-400">
          <Loader2 className="mr-2 h-5 w-5 animate-spin" />
          <span className="text-sm">检索中…</span>
        </div>
      ) : searched && searchError ? (
        <div className="py-12 text-center">
          <AlertCircle className="mx-auto mb-3 h-8 w-8 text-red-400" />
          <p className="text-sm text-red-600 mb-3">{searchError}</p>
          <button
            type="button"
            onClick={() => handleSearch()}
            className="inline-flex items-center gap-1.5 rounded-lg bg-[#1A6BD8] px-4 py-2 text-sm font-medium text-white hover:bg-[#1558B0] transition-colors"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            重试
          </button>
        </div>
      ) : searched && filteredResults.length === 0 ? (
        <div className="py-12 text-center">
          <SearchIcon className="mx-auto mb-3 h-8 w-8 text-neutral-300" />
          <p className="text-sm text-neutral-500 mb-1">未找到相关结果</p>
          <p className="text-xs text-neutral-400">您可以尝试不同的关键词或导入相关文档</p>
        </div>
      ) : (
        <div className="space-y-3">
          {filteredResults.map((result) => (
            <div
              key={result.chunk_id}
              className="rounded-lg border border-neutral-200 bg-white p-4 transition-shadow hover:shadow-sm"
            >
              {/* Header: title + score */}
              <div className="mb-2 flex items-start justify-between gap-3">
                <div className="flex items-center gap-2 min-w-0">
                  <FileText className="h-4 w-4 shrink-0 text-[#1A6BD8]" />
                  <h3 className="text-sm font-medium text-neutral-800 truncate">
                    {result.title}
                  </h3>
                </div>
                <span
                  className={`shrink-0 rounded-full px-2 py-0.5 text-xs font-medium ${
                    result.score >= 0.8
                      ? "bg-green-100 text-green-700"
                      : result.score >= 0.5
                      ? "bg-yellow-100 text-yellow-700"
                      : "bg-neutral-100 text-neutral-500"
                  }`}
                >
                  {(result.score * 100).toFixed(0)}%
                </span>
              </div>

              {/* Source annotation */}
              <div className="mb-2 flex flex-wrap items-center gap-2 text-xs text-neutral-500">
                <span className="flex items-center gap-1">
                  <Hash className="h-3 w-3" />
                  {result.project}
                </span>
                {result.section_path && (
                  <span className="rounded bg-neutral-100 px-1.5 py-0.5">
                    {result.section_path}
                  </span>
                )}
                <span className="text-neutral-400">{result.source}</span>
              </div>

              {/* Content with keyword highlighting */}
              <pre className="whitespace-pre-wrap text-sm leading-relaxed text-neutral-600 font-sans">
                {highlightText(result.content, query)}
              </pre>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
