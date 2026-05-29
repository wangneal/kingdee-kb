import { useState, useEffect, useCallback } from "react";
import {
  Search,
  Loader2,
  RefreshCw,
  Zap,
  Upload,
  X,
  ExternalLink,
  BookOpen,
  Code,
  ChevronDown,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  listSkills,
  getSkill,
  searchSkills,
  getSkillStats,
  rescanSkills,
  importSkill,
} from "../lib/skill-commands";
import type { Skill, SkillStatsResponse } from "../lib/skill-types";
import { SKILL_CATEGORY_LABELS } from "../lib/skill-types";

const CATEGORY_ORDER = ["core", "stage", "mgmt", "tool"];

export default function Skills() {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [stats, setStats] = useState<SkillStatsResponse | null>(null);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [detail, setDetail] = useState<Skill | null>(null);
  const [expandedCards, setExpandedCards] = useState<Set<string>>(new Set());

  const refresh = useCallback(async (q?: string) => {
    setLoading(true);
    setError(null);
    try {
      const [skillList, skillStats] = await Promise.all([
        q ? searchSkills(q) : listSkills(),
        getSkillStats().catch(() => null),
      ]);
      setSkills(skillList);
      setStats(skillStats);
    } catch (e) {
      setError(String(e));
    }
    setLoading(false);
  }, []);

  useEffect(() => { refresh(); }, []);

  const handleSearch = useCallback(() => refresh(query), [query, refresh]);
  const handleRescan = useCallback(async () => {
    setScanning(true);
    try { await rescanSkills(); await refresh(); }
    catch (e) { setError(String(e)); }
    setScanning(false);
  }, [refresh]);

  const handleImport = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "SKILL.md", extensions: ["md"] }],
    });
    if (!selected) return;
    try {
      await importSkill(selected as string);
      await refresh();
      setError(null);
    } catch (e) { setError(String(e)); }
  }, [refresh]);

  const handleCardClick = useCallback(async (name: string) => {
    setDetail(await getSkill(name));
  }, []);

  const handleCardExpand = useCallback((name: string, e: React.MouseEvent) => {
    e.stopPropagation();
    setExpandedCards(prev => {
      const next = new Set(prev);
      next.has(name) ? next.delete(name) : next.add(name);
      return next;
    });
  }, []);

  // ── Detail Overlay ──
  if (detail) {
    return (
      <div className="flex h-full flex-col">
        <div className="flex h-12 items-center justify-between border-b border-neutral-200 px-6 shrink-0">
          <h2 className="text-sm font-semibold text-neutral-800">
            {detail.metadata.icon || "📄"} {detail.name}
          </h2>
          <button
            onClick={() => setDetail(null)}
            className="flex h-7 w-7 items-center justify-center rounded-md text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-6">
          <div className="prose prose-sm prose-neutral max-w-4xl
            prose-headings:text-neutral-800
            prose-h2:mt-6 prose-h2:mb-3 prose-h2:border-b prose-h2:border-neutral-200 prose-h2:pb-2
            prose-p:text-neutral-600
            prose-code:rounded prose-code:bg-neutral-100 prose-code:px-1.5 prose-code:py-0.5 prose-code:text-xs prose-code:text-neutral-700
            prose-pre:rounded-lg prose-pre:bg-neutral-100 prose-pre:text-neutral-700
            prose-table:text-xs prose-table:border-collapse prose-th:border prose-th:border-neutral-200 prose-th:bg-neutral-50 prose-th:px-2 prose-th:py-1 prose-td:border prose-td:border-neutral-200 prose-td:px-2 prose-td:py-1
            prose-a:text-[#1A6BD8] prose-a:no-underline hover:prose-a:underline
            prose-strong:text-neutral-700
            prose-li:text-neutral-600
            prose-blockquote:border-l-[#1A6BD8] prose-blockquote:text-neutral-500
            prose-img:rounded-lg"> 
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {detail.body || "*暂无内容*"}
            </ReactMarkdown>
          </div>
        </div>
      </div>
    );
  }

  // ── Card Grid ──
  const grouped = CATEGORY_ORDER
    .flatMap(cat => skills.filter(s => s.metadata.category === cat))
    .concat(skills.filter(s => !CATEGORY_ORDER.includes(s.metadata.category)));

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6 shrink-0">
        <div className="flex items-center gap-2">
          <Zap className="h-5 w-5 text-[#1A6BD8]" />
          <h1 className="text-base font-semibold text-neutral-800">技能体系</h1>
          {stats && <span className="text-xs text-neutral-400">{stats.total} 个</span>}
        </div>
        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-neutral-400" />
            <input
              type="text" value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSearch()}
              placeholder="搜索..."
              className="h-8 w-36 rounded-md border border-neutral-200 bg-white pl-8 pr-2 text-xs placeholder:text-neutral-400 focus:border-[#1A6BD8] focus:outline-none"
            />
          </div>
          <button onClick={handleImport}
            className="flex h-8 items-center gap-1 rounded-md bg-[#1A6BD8] px-3 text-xs text-white hover:bg-[#1555B0]"
          ><Upload className="h-3.5 w-3.5" />导入</button>
          <button onClick={handleRescan} disabled={scanning}
            className="flex h-8 items-center gap-1 rounded-md border border-neutral-200 px-2.5 text-xs text-neutral-600 hover:bg-neutral-50 disabled:opacity-50"
          ><RefreshCw className={`h-3.5 w-3.5 ${scanning ? "animate-spin" : ""}`} /></button>
        </div>
      </div>

      {error && (
        <div className="mx-6 mt-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-600">{error}</div>
      )}

      {/* Card Grid */}
      <div className="flex-1 overflow-y-auto p-6">
        {loading ? (
          <div className="flex items-center justify-center py-20">
            <Loader2 className="h-6 w-6 animate-spin text-neutral-300" />
          </div>
        ) : grouped.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-20 text-neutral-400">
            <Zap className="h-12 w-12 text-neutral-200" />
            <p className="mt-3 text-sm">没有找到技能</p>
            <p className="mt-1 text-xs">点击「导入」添加 SKILL.md 文件</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {grouped.map(skill => (
              <SkillCard
                key={skill.name}
                skill={skill}
                expanded={expandedCards.has(skill.name)}
                onClick={() => handleCardClick(skill.name)}
                onExpand={(e) => handleCardExpand(skill.name, e)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Skill Card ──
function SkillCard({
  skill,
  expanded,
  onClick,
  onExpand,
}: {
  skill: Skill;
  expanded: boolean;
  onClick: () => void;
  onExpand: (e: React.MouseEvent) => void;
}) {
  const cat = skill.metadata.category;
  const catLabel = SKILL_CATEGORY_LABELS[cat] || cat;
  const hasExtras = skill.scripts.length > 0 || skill.references.length > 0;

  return (
    <div
      className="group cursor-pointer rounded-lg border border-neutral-200 bg-white p-4 transition-all hover:border-[#1A6BD8]/30 hover:shadow-sm"
      onClick={onClick}
    >
      {/* Header */}
      <div className="flex items-start justify-between mb-2">
        <span className="text-2xl">{skill.metadata.icon || "📄"}</span>
        <span className="rounded-full bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
          {catLabel}
        </span>
      </div>

      {/* Name */}
      <h3 className="mb-1 text-sm font-semibold text-neutral-800">{skill.name}</h3>

      {/* Description */}
      {skill.metadata.description && (
        <p className="mb-3 text-xs leading-relaxed text-neutral-500 line-clamp-2">
          {skill.metadata.description}
        </p>
      )}

      {/* Expand toggle */}
      <div className="flex items-center justify-between">
        <button
          onClick={onExpand}
          className="flex items-center gap-1 text-[11px] text-neutral-400 hover:text-neutral-600"
        >
          <ChevronDown className={`h-3 w-3 transition-transform ${expanded ? "rotate-180" : ""}`} />
          详情
        </button>
        <button
          onClick={(e) => { e.stopPropagation(); onClick(); }}
          className="flex items-center gap-1 text-[11px] text-[#1A6BD8] opacity-0 transition-opacity group-hover:opacity-100 hover:underline"
        >
          查看 <ExternalLink className="h-3 w-3" />
        </button>
      </div>

      {/* Expanded detail */}
      {expanded && (
        <div className="mt-3 border-t border-neutral-100 pt-3 text-xs text-neutral-500">
          {skill.metadata.version && (
            <p className="mb-1">版本: {skill.metadata.version}</p>
          )}
          {hasExtras && (
            <div className="flex gap-3 mt-2">
              {skill.scripts.length > 0 && (
                <span className="flex items-center gap-1"><Code className="h-3 w-3" />{skill.scripts.length} 脚本</span>
              )}
              {skill.references.length > 0 && (
                <span className="flex items-center gap-1"><BookOpen className="h-3 w-3" />{skill.references.length} 参考</span>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
