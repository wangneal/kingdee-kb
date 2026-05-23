import { useState, useEffect, useMemo } from "react";
import {
  Package,
  FolderOpen,
  ChevronRight,
  ChevronDown,
  Download,
  Trash2,
  FileText,
  FileSpreadsheet,
  Loader2,
  Calendar,
  Hash,
  CheckCircle2,
  AlertCircle,
} from "lucide-react";
import {
  listProducts,
  exportProduct,
  deleteProduct,
  type ProductMeta,
} from "../lib/tauri-commands";

interface ProjectGroup {
  project: string;
  products: ProductMeta[];
}

export default function Products() {
  const [products, setProducts] = useState<ProductMeta[]>([]);
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(new Set());
  const [expandedProduct, setExpandedProduct] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [exporting, setExporting] = useState<number | null>(null);
  const [deleting, setDeleting] = useState<number | null>(null);
  const [exportDialog, setExportDialog] = useState<ProductMeta | null>(null);
  const [exportDir, setExportDir] = useState("");
  const [exportResult, setExportResult] = useState<string | null>(null);

  // Group products by project
  const projectGroups = useMemo<ProjectGroup[]>(() => {
    const map = new Map<string, ProductMeta[]>();
    for (const product of products) {
      const list = map.get(product.project) ?? [];
      list.push(product);
      map.set(product.project, list);
    }
    return Array.from(map.entries())
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([project, prods]) => ({
        project,
        products: prods.sort(
          (a, b) =>
            new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
        ),
      }));
  }, [products]);

  // Load products on mount
  useEffect(() => {
    (async () => {
      try {
        const prods = await listProducts();
        setProducts(prods);
        // Auto-expand first project
        if (prods.length > 0) {
          setExpandedProjects(new Set([prods[0].project]));
        }
      } catch (e) {
        console.error("Failed to load products:", e);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const toggleProject = (project: string) => {
    setExpandedProjects((prev) => {
      const next = new Set(prev);
      if (next.has(project)) next.delete(project);
      else next.add(project);
      return next;
    });
  };

  const handleExport = async () => {
    if (!exportDialog || !exportDir.trim()) return;
    setExporting(exportDialog.id);
    setExportResult(null);
    try {
      const result = await exportProduct(exportDialog.id, exportDir.trim());
      setExportResult(result);
    } catch (e) {
      console.error("Export failed:", e);
      setExportResult(`导出失败: ${e}`);
    } finally {
      setExporting(null);
    }
  };

  const handleDelete = async (product: ProductMeta) => {
    if (
      !confirm(
        `确定删除产物「${product.template_name}」？\n输出路径: ${product.output_path}`
      )
    )
      return;
    setDeleting(product.id);
    try {
      await deleteProduct(product.id);
      setProducts((prev) => prev.filter((p) => p.id !== product.id));
      if (expandedProduct === product.id) setExpandedProduct(null);
    } catch (e) {
      console.error("Delete failed:", e);
    } finally {
      setDeleting(null);
    }
  };

  const closeExportDialog = () => {
    setExportDialog(null);
    setExportDir("");
    setExportResult(null);
  };

  const formatDate = (dateStr: string) => {
    try {
      return new Date(dateStr).toLocaleDateString("zh-CN", {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return dateStr;
    }
  };

  const getStatusBadge = (status: string) => {
    switch (status.toLowerCase()) {
      case "completed":
      case "success":
        return (
          <span className="flex items-center gap-1 rounded-full bg-emerald-50 px-2 py-0.5 text-[10px] font-medium text-emerald-600">
            <CheckCircle2 className="h-2.5 w-2.5" />
            完成
          </span>
        );
      case "failed":
      case "error":
        return (
          <span className="flex items-center gap-1 rounded-full bg-red-50 px-2 py-0.5 text-[10px] font-medium text-red-600">
            <AlertCircle className="h-2.5 w-2.5" />
            失败
          </span>
        );
      default:
        return (
          <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-[10px] font-medium text-neutral-500">
            {status}
          </span>
        );
    }
  };

  return (
    <div className="flex h-full">
      {/* Left panel - Product tree */}
      <div className="w-72 shrink-0 border-r border-neutral-200 bg-white overflow-auto">
        <div className="sticky top-0 border-b border-neutral-100 bg-white px-4 py-3">
          <h2 className="text-sm font-semibold text-neutral-700 flex items-center gap-2">
            <Package className="h-4 w-4 text-[#1A6BD8]" />
            产物管理
          </h2>
          <p className="text-xs text-neutral-400 mt-0.5">
            {products.length} 个产物
          </p>
        </div>

        {loading ? (
          <div className="flex items-center justify-center p-8">
            <Loader2 className="h-5 w-5 animate-spin text-[#1A6BD8]" />
            <span className="ml-2 text-sm text-neutral-400">加载中…</span>
          </div>
        ) : projectGroups.length === 0 ? (
          <div className="flex flex-col items-center justify-center p-8 text-center">
            <Package className="h-10 w-10 text-neutral-300 mb-3" />
            <p className="text-sm text-neutral-500">暂无产物</p>
            <p className="text-xs text-neutral-400 mt-1">
              请先通过文档生成向导创建产物
            </p>
          </div>
        ) : (
          <div className="py-1">
            {projectGroups.map(({ project, products: prods }) => (
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
                  <span className="truncate flex-1 text-left">{project}</span>
                  <span className="text-neutral-400">{prods.length}</span>
                </button>

                {/* Products */}
                {expandedProjects.has(project) && (
                  <div className="ml-3">
                    {prods.map((product) => (
                      <button
                        type="button"
                        key={product.id}
                        onClick={() =>
                          setExpandedProduct(
                            expandedProduct === product.id ? null : product.id
                          )
                        }
                        className={`group flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm transition-colors ${
                          expandedProduct === product.id
                            ? "bg-[#1A6BD8]/10 text-[#1A6BD8]"
                            : "text-neutral-600 hover:bg-neutral-50"
                        }`}
                      >
                        {product.template_name.endsWith(".xlsx") ||
                        product.template_name.endsWith(".xls") ? (
                          <FileSpreadsheet className="h-3.5 w-3.5 shrink-0 text-emerald-600" />
                        ) : (
                          <FileText className="h-3.5 w-3.5 shrink-0 text-[#1A6BD8]" />
                        )}
                        <div className="flex-1 min-w-0">
                          <span className="truncate block">
                            {product.template_name}
                          </span>
                          <span className="text-[10px] text-neutral-400 block">
                            {formatDate(product.created_at)}
                          </span>
                        </div>
                        {getStatusBadge(product.status)}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Right panel - Product preview */}
      <div className="flex-1 overflow-auto bg-neutral-50">
        {expandedProduct === null ? (
          <div className="flex h-full items-center justify-center text-neutral-400">
            <div className="text-center">
              <Package className="mx-auto h-12 w-12 text-neutral-300" />
              <p className="mt-2 text-sm">选择左侧产物查看详情</p>
            </div>
          </div>
        ) : (() => {
          const selected = products.find((p) => p.id === expandedProduct);
          if (!selected) return null;
          return (
            <ProductPreview
              product={selected}
              onExport={(p) => setExportDialog(p)}
              onDelete={handleDelete}
              deleting={deleting}
              formatDate={formatDate}
              getStatusBadge={getStatusBadge}
            />
          );
        })()
}
      </div>

      {/* Export dialog */}
      {exportDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
          <div className="w-full max-w-md rounded-lg bg-white p-6 shadow-xl">
            <h3 className="text-lg font-semibold text-neutral-800 mb-4">
              导出产物
            </h3>
            <p className="text-sm text-neutral-600 mb-2">
              模板: {exportDialog.template_name}
            </p>
            <p className="text-xs text-neutral-400 mb-4 break-all">
              源路径: {exportDialog.output_path}
            </p>

            <label
              htmlFor="export-dir"
              className="block text-sm font-medium text-neutral-700 mb-1.5"
            >
              导出目标目录
            </label>
            <input
              id="export-dir"
              type="text"
              value={exportDir}
              onChange={(e) => setExportDir(e.target.value)}
              placeholder="例如: C:\Users\...\导出目录"
              className="w-full rounded-md border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
            />

            {exportResult && (
              <div
                className={`mt-3 rounded-md p-3 text-sm ${
                  exportResult.startsWith("导出失败")
                    ? "bg-red-50 text-red-600 border border-red-200"
                    : "bg-emerald-50 text-emerald-600 border border-emerald-200"
                }`}
              >
                {exportResult}
              </div>
            )}

            <div className="mt-6 flex justify-end gap-2">
              <button
                type="button"
                onClick={closeExportDialog}
                className="rounded-md border border-neutral-200 bg-white px-4 py-2 text-sm text-neutral-600 hover:bg-neutral-50"
              >
                取消
              </button>
              <button
                type="button"
                onClick={handleExport}
                disabled={exporting === exportDialog.id || !exportDir.trim()}
                className="flex items-center gap-1.5 rounded-md bg-[#1A6BD8] px-4 py-2 text-sm text-white hover:bg-[#1558B0] disabled:opacity-50"
              >
                {exporting === exportDialog.id ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Download className="h-4 w-4" />
                )}
                导出
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Product Preview Component ─────────────────────────────────────────────────

function ProductPreview({
  product,
  onExport,
  onDelete,
  deleting,
  formatDate,
  getStatusBadge,
}: {
  product: ProductMeta;
  onExport: (p: ProductMeta) => void;
  onDelete: (p: ProductMeta) => void;
  deleting: number | null;
  formatDate: (d: string) => string;
  getStatusBadge: (s: string) => React.ReactNode;
}) {
  return (
    <div className="mx-auto max-w-full px-6 py-6">
      {/* Product header */}
      <div className="rounded-lg border border-neutral-200 bg-white p-6">
        <div className="flex items-start gap-4">
          {product.template_name.endsWith(".xlsx") ||
          product.template_name.endsWith(".xls") ? (
            <FileSpreadsheet className="h-12 w-12 shrink-0 text-emerald-600" />
          ) : (
            <FileText className="h-12 w-12 shrink-0 text-[#1A6BD8]" />
          )}
          <div className="flex-1 min-w-0">
            <h2 className="text-lg font-semibold text-neutral-800">
              {product.template_name}
            </h2>
            <div className="mt-2 flex flex-wrap items-center gap-3 text-xs text-neutral-500">
              <span className="flex items-center gap-1">
                <FolderOpen className="h-3 w-3" />
                {product.project}
              </span>
              <span className="flex items-center gap-1">
                <Calendar className="h-3 w-3" />
                {formatDate(product.created_at)}
              </span>
              <span className="flex items-center gap-1">
                <Hash className="h-3 w-3" />
                {product.field_count} 个字段
              </span>
              {getStatusBadge(product.status)}
            </div>
          </div>
        </div>

        {/* Stats */}
        <div className="mt-6 grid grid-cols-3 gap-4">
          <div className="rounded-md bg-neutral-50 p-3 text-center">
            <p className="text-2xl font-semibold text-[#1A6BD8]">
              {product.field_count}
            </p>
            <p className="text-xs text-neutral-500">总字段数</p>
          </div>
          <div className="rounded-md bg-neutral-50 p-3 text-center">
            <p className="text-2xl font-semibold text-purple-600">
              {product.llm_fields_count}
            </p>
            <p className="text-xs text-neutral-500">AI 生成字段</p>
          </div>
          <div className="rounded-md bg-neutral-50 p-3 text-center">
            <p className="text-2xl font-semibold text-emerald-600">
              {product.field_count - product.llm_fields_count}
            </p>
            <p className="text-xs text-neutral-500">用户填写字段</p>
          </div>
        </div>

        {/* Output path */}
        <div className="mt-4 rounded-md border border-neutral-100 bg-neutral-50 p-3">
          <p className="text-xs font-medium text-neutral-500 mb-1">输出路径</p>
          <p className="text-sm text-neutral-700 break-all font-mono">
            {product.output_path}
          </p>
        </div>

        {/* Actions */}
        <div className="mt-6 flex items-center gap-3">
          <button
            type="button"
            onClick={() => onExport(product)}
            className="flex items-center gap-1.5 rounded-md bg-[#1A6BD8] px-4 py-2 text-sm text-white hover:bg-[#1558B0]"
          >
            <Download className="h-4 w-4" />
            导出
          </button>
          <button
            type="button"
            onClick={() => onDelete(product)}
            disabled={deleting === product.id}
            className="flex items-center gap-1.5 rounded-md border border-red-200 bg-white px-4 py-2 text-sm text-red-600 hover:bg-red-50 disabled:opacity-50"
          >
            {deleting === product.id ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Trash2 className="h-4 w-4" />
            )}
            删除
          </button>
        </div>
      </div>
    </div>
  );
}
