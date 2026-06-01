import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom"
import { useProject } from "../contexts/ProjectContext";
import {
  BookOpen,
  Search,
  FileEdit,
  MessageSquare,
  Package,
  FileText,
  FolderOpen,
  Calendar,
  ArrowRight,
  Loader2,
  ClipboardList,
  ShieldAlert,
} from "lucide-react";
import {
  getStats,
  listProducts,
  scanTemplates,
  type KnowledgeStats,
  type ProductMeta,
  type TemplateInfo,
} from "../lib/tauri-commands";

export default function Home() {
  const { projectId } = useProject();
  const navigate = useNavigate();
  const [stats, setStats] = useState<KnowledgeStats | null>(null);
  const [products, setProducts] = useState<ProductMeta[]>([]);
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    (async () => {
      try {
        const [statsData, productsData, templatesData] = await Promise.all([
          getStats(projectId).catch(() => null),
          listProducts(projectId).catch(() => []),
          scanTemplates().catch(() => []),
        ]);
        setStats(statsData);
        setProducts(productsData);
        setTemplates(templatesData);
      } catch (e) {
        console.error("Failed to load dashboard data:", e);
      } finally {
        setLoading(false);
      }
    })();
  }, [projectId]);

  const recentProducts = products
    .sort(
      (a, b) =>
        new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
    )
    .slice(0, 5);

  const formatDate = (dateStr: string) => {
    try {
      return new Date(dateStr).toLocaleDateString("zh-CN", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return dateStr;
    }
  };

  const quickActions = [
    {
      icon: BookOpen,
      label: "浏览知识库",
      description: "查看已导入的文档和知识片段",
      path: "/browse",
      color: "bg-[#1A6BD8]",
    },
    {
      icon: Search,
      label: "检索",
      description: "搜索知识库中的相关内容",
      path: "/search",
      color: "bg-emerald-600",
    },
    {
      icon: FileEdit,
      label: "生成文档",
      description: "使用模板生成实施文档",
      path: "/templates",
      color: "bg-purple-600",
    },
    {
      icon: MessageSquare,
      label: "AI 对话",
      description: "基于知识库的智能问答",
      path: "/chat",
      color: "bg-amber-600",
    },
    {
      icon: ClipboardList,
      label: "调研助手",
      description: "语音转录 + 会话管理 + 蓝图导出",
      path: "/research",
      color: "bg-cyan-600",
    },
    {
      icon: ShieldAlert,
      label: "风险把控",
      description: "范围预警 + 项目健康 + 防身话术",
      path: "/risk",
      color: "bg-red-600",
    },
  ];

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-[#1A6BD8]" />
        <span className="ml-2 text-sm text-neutral-500">加载概览…</span>
      </div>
    );
  }

  return (
    <div className="p-6 w-full">
      {/* Header */}
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-neutral-800">概览</h1>
        <p className="mt-1 text-sm text-neutral-500">
          实施顾问AI助手 — 金蝶ERP实施顾问本地知识管理工具
        </p>
      </div>

      {/* Stats cards */}
      <div className="grid grid-cols-3 gap-4 mb-8">
        <div className="rounded-lg border border-neutral-200 bg-white p-5">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-[#1A6BD8]/10">
              <FileEdit className="h-5 w-5 text-[#1A6BD8]" />
            </div>
            <div>
              <p className="text-2xl font-semibold text-neutral-800">
                {templates.length}
              </p>
              <p className="text-xs text-neutral-500">模板数量</p>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-neutral-200 bg-white p-5">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-purple-100">
              <Package className="h-5 w-5 text-purple-600" />
            </div>
            <div>
              <p className="text-2xl font-semibold text-neutral-800">
                {products.length}
              </p>
              <p className="text-xs text-neutral-500">生成产物</p>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-neutral-200 bg-white p-5">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-emerald-100">
              <BookOpen className="h-5 w-5 text-emerald-600" />
            </div>
            <div>
              <p className="text-2xl font-semibold text-neutral-800">
                {stats?.document_count ?? 0}
              </p>
              <p className="text-xs text-neutral-500">知识库文档</p>
            </div>
          </div>
        </div>
      </div>

      {/* Quick actions */}
      <div className="mb-8">
        <h2 className="text-sm font-semibold text-neutral-700 mb-4">
          快捷操作
        </h2>
        <div className="grid grid-cols-4 gap-3">
          {quickActions.map((action) => (
            <button
              key={action.path}
              type="button"
              onClick={() => navigate(action.path)}
              className="group rounded-lg border border-neutral-200 bg-white p-4 text-left transition-all hover:border-[#1A6BD8]/30 hover:shadow-sm"
            >
              <div
                className={`flex h-9 w-9 items-center justify-center rounded-lg ${action.color} mb-3`}
              >
                <action.icon className="h-4 w-4 text-white" />
              </div>
              <p className="text-sm font-medium text-neutral-800">
                {action.label}
              </p>
              <p className="text-xs text-neutral-400 mt-0.5">
                {action.description}
              </p>
            </button>
          ))}
        </div>
      </div>

      {/* Recent products */}
      <div>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-sm font-semibold text-neutral-700">
            最近产物
          </h2>
          {products.length > 0 && (
            <button
              type="button"
              onClick={() => navigate("/products")}
              className="flex items-center gap-1 text-xs text-[#1A6BD8] hover:underline"
            >
              查看全部
              <ArrowRight className="h-3 w-3" />
            </button>
          )}
        </div>

        {recentProducts.length === 0 ? (
          <div className="rounded-lg border border-dashed border-neutral-200 bg-white p-8 text-center">
            <Package className="mx-auto h-8 w-8 text-neutral-300" />
            <p className="mt-2 text-sm text-neutral-500">暂无产物</p>
            <p className="text-xs text-neutral-400 mt-1">
              前往文档生成页面创建您的第一个产物
            </p>
            <button
              type="button"
              onClick={() => navigate("/templates")}
              className="mt-3 inline-flex items-center gap-1.5 rounded-md bg-[#1A6BD8] px-3 py-1.5 text-xs text-white hover:bg-[#1558B0]"
            >
              <FileEdit className="h-3.5 w-3.5" />
              去生成文档
            </button>
          </div>
        ) : (
          <div className="rounded-lg border border-neutral-200 bg-white overflow-hidden">
            {recentProducts.map((product, idx) => (
              <button
                key={product.id}
                type="button"
                onClick={() => navigate("/products")}
                className={`flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-neutral-50 ${
                  idx < recentProducts.length - 1
                    ? "border-b border-neutral-100"
                    : ""
                }`}
              >
                {product.template_name.endsWith(".xlsx") ||
                product.template_name.endsWith(".xls") ? (
                  <FileText className="h-4 w-4 shrink-0 text-emerald-600" />
                ) : (
                  <FileText className="h-4 w-4 shrink-0 text-[#1A6BD8]" />
                )}
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-neutral-700 truncate">
                    {product.template_name}
                  </p>
                  <p className="text-xs text-neutral-400 flex items-center gap-2">
                    <span className="flex items-center gap-1">
                      <FolderOpen className="h-2.5 w-2.5" />
                      {product.project}
                    </span>
                    <span className="flex items-center gap-1">
                      <Calendar className="h-2.5 w-2.5" />
                      {formatDate(product.created_at)}
                    </span>
                  </p>
                </div>
                <ArrowRight className="h-4 w-4 text-neutral-300 shrink-0" />
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
