import type { VerificationReport } from "../contexts/AgentContext";

/** 置信度徽章组件 — 在 AI 回答底部展示验证结果 */
export function VerificationBadge({ report }: { report: VerificationReport }) {
  if (!report) return null;

  const { level, overall_confidence, suggested_labels } = report;

  const levelConfig = {
    Confirmed: { icon: "🟢", label: "已确认", bg: "bg-green-50", border: "border-green-200", text: "text-green-700" },
    NeedsReview: { icon: "🟡", label: "需核查", bg: "bg-yellow-50", border: "border-yellow-200", text: "text-yellow-700" },
    Suspected: { icon: "🔴", label: "疑似幻觉", bg: "bg-red-50", border: "border-red-200", text: "text-red-700" },
    Failed: { icon: "🔴", label: "验证失败", bg: "bg-red-50", border: "border-red-200", text: "text-red-700" },
  };

  const cfg = levelConfig[level];
  const confidencePct = Math.round(overall_confidence * 100);

  return (
    <div className={`mt-3 rounded-lg border ${cfg.bg} ${cfg.border} px-3 py-2 text-xs ${cfg.text}`}>
      <div className="flex items-center justify-between">
        <span className="font-medium">
          {cfg.icon} {cfg.label}
        </span>
        <span className="opacity-75">置信度 {confidencePct}%</span>
      </div>
      {suggested_labels.length > 0 && (
        <div className="mt-1 space-y-0.5">
          {suggested_labels.map((label, i) => (
            <div key={i} className="opacity-80">• {label}</div>
          ))}
        </div>
      )}
    </div>
  );
}
