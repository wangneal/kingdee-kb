import { useState, useEffect, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
  ChevronLeft,
  ChevronRight,
  FileText,
  FileSpreadsheet,
  Loader2,
  Wand2,
  Download,
  CheckCircle2,
  AlertCircle,
} from "lucide-react";
import {
  scanTemplates,
  getTemplateSchema,
  smartFill,
  generateDoc,
  type TemplateInfo,
  type TemplateSchema,
  type SchemaField,
  type SmartFillResult,
  type GeneratedDoc,
} from "../lib/tauri-commands";
import { useProject } from "../contexts/ProjectContext";

type Step = "info" | "fill" | "generate";

const STEPS: { key: Step; label: string }[] = [
  { key: "info", label: "模板信息" },
  { key: "fill", label: "填写字段" },
  { key: "generate", label: "生成文档" },
];

export default function Wizard() {
  const { projectId } = useProject();
  const { templateId } = useParams<{ templateId: string }>();
  const navigate = useNavigate();

  const [currentStep, setCurrentStep] = useState<Step>("info");
  const [template, setTemplate] = useState<TemplateInfo | null>(null);
  const [schema, setSchema] = useState<TemplateSchema | null>(null);
  const [fieldValues, setFieldValues] = useState<Record<string, string>>({});
  const [fillResult, setFillResult] = useState<SmartFillResult | null>(null);
  const [generatedDoc, setGeneratedDoc] = useState<GeneratedDoc | null>(null);
  const [userInput, setUserInput] = useState("");
  const [loading, setLoading] = useState(true);
  const [filling, setFilling] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Load template + schema on mount
  useEffect(() => {
    if (!templateId) {
      navigate("/templates");
      return;
    }

    (async () => {
      try {
        setLoading(true);
        // Find template by ID
        const templates = await scanTemplates();
        const tpl = templates.find((t) => t.id === templateId);
        if (!tpl) {
          setError(`模板未找到: ${templateId}`);
          return;
        }
        setTemplate(tpl);

        // Load schema
        console.log("[Wizard] Calling getTemplateSchema with:", {
          template_id: tpl.id,
          template_name: tpl.name,
          file_path: tpl.file_path,
          phase: tpl.phase,
        });
        const schemaData = await getTemplateSchema(
          tpl.id,
          tpl.name,
          tpl.file_path,
          tpl.phase
        );
        setSchema(schemaData);

        // Initialize field values from defaults
        const defaults: Record<string, string> = {};
        for (const f of schemaData.fields) {
          if (f.default) {
            defaults[f.name] = f.default;
          }
        }
        setFieldValues(defaults);
      } catch (e) {
        console.warn("[Wizard] 加载模板失败:", e);
        setError(String(e));
      } finally {
        setLoading(false);
      }
    })();
  }, [templateId, navigate]);

  const handleFieldChange = (name: string, value: string) => {
    setFieldValues((prev) => ({ ...prev, [name]: value }));
  };

  const handleSmartFill = useCallback(async () => {
    if (!template || !schema) return;
    setFilling(true);
    setError(null);
    try {
      const result = await smartFill({
        template_id: template.id,
        user_input: userInput,
        manual_fields: fieldValues,
        schema_fields: schema.fields,
        project_name: projectId,
      });
      setFillResult(result);
      // Merge AI-filled values
      setFieldValues((prev) => ({ ...prev, ...result.filled_fields }));
    } catch (e) {
      console.warn("[Wizard] 智能填充失败:", e);
      setError(String(e));
    } finally {
      setFilling(false);
    }
  }, [template, schema, userInput, fieldValues]);

  const handleGenerate = useCallback(async () => {
    if (!template || !schema) return;
    setGenerating(true);
    setError(null);
    try {
      const outputPath = template.file_path.replace(
        /([^/\\]+)$/,
        `generated_${Date.now()}.$1`
      );
      const result = await generateDoc({
        template_path: template.file_path,
        output_path: outputPath,
        fields: fieldValues,
        schema_fields: schema.fields,
        project_name: projectId,
      });
      setGeneratedDoc(result);
    } catch (e) {
      console.warn("[Wizard] 生成文档失败:", e);
      setError(String(e));
    } finally {
      setGenerating(false);
    }
  }, [template, schema, fieldValues]);

  const stepIndex = STEPS.findIndex((s) => s.key === currentStep);

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-[#1A6BD8]" />
        <span className="ml-2 text-sm text-neutral-500">加载模板…</span>
      </div>
    );
  }

  if (error && !template) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4">
        <AlertCircle className="h-12 w-12 text-red-400" />
        <p className="text-sm text-red-500">{error}</p>
        <button
          type="button"
          onClick={() => navigate("/templates")}
          className="rounded-md bg-[#1A6BD8] px-4 py-2 text-sm text-white hover:bg-[#1558B0]"
        >
          返回模板列表
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {/* Step indicator */}
      <div className="border-b border-neutral-200 bg-white px-6 py-3">
        <div className="flex items-center gap-2">
          {STEPS.map((step, idx) => (
            <div key={step.key} className="flex items-center">
              <button
                type="button"
                onClick={() => {
                  // Only allow going back or to current step
                  if (idx <= stepIndex) setCurrentStep(step.key);
                }}
                className={`flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium transition-colors ${
                  step.key === currentStep
                    ? "bg-[#1A6BD8] text-white"
                    : idx < stepIndex
                      ? "bg-[#1A6BD8]/10 text-[#1A6BD8] cursor-pointer hover:bg-[#1A6BD8]/20"
                      : "bg-neutral-100 text-neutral-400 cursor-default"
                }`}
              >
                {idx < stepIndex && <CheckCircle2 className="h-3 w-3" />}
                {step.label}
              </button>
              {idx < STEPS.length - 1 && (
                <ChevronRight className="mx-1 h-4 w-4 text-neutral-300" />
              )}
            </div>
          ))}
        </div>
      </div>

      {/* Error banner */}
      {error && (
        <div className="mx-6 mt-4 rounded-md border border-red-200 bg-red-50 px-4 py-2 text-sm text-red-600">
          {error}
        </div>
      )}

      {/* Step content */}
      <div className="flex-1 overflow-auto bg-neutral-50 p-6">
        {currentStep === "info" && template && (
          <StepInfo
            template={template}
            schema={schema}
            onNext={() => setCurrentStep("fill")}
          />
        )}

        {currentStep === "fill" && schema && (
          <StepFill
            schema={schema}
            fieldValues={fieldValues}
            userInput={userInput}
            onFieldChange={handleFieldChange}
            onUserInputChange={setUserInput}
            onSmartFill={handleSmartFill}
            filling={filling}
            fillResult={fillResult}
            onBack={() => setCurrentStep("info")}
            onNext={() => setCurrentStep("generate")}
          />
        )}

        {currentStep === "generate" && template && schema && (
          <StepGenerate
            template={template}
            schema={schema}
            fieldValues={fieldValues}
            onGenerate={handleGenerate}
            generating={generating}
            generatedDoc={generatedDoc}
            onBack={() => setCurrentStep("fill")}
            onDone={() => navigate("/templates")}
          />
        )}
      </div>
    </div>
  );
}

// ── Sub-components ───────────────────────────────────────────────────────────

function StepInfo({
  template,
  schema,
  onNext,
}: {
  template: TemplateInfo;
  schema: TemplateSchema | null;
  onNext: () => void;
}) {
  return (
    <div className="mx-auto max-w-2xl">
      <div className="rounded-lg border border-neutral-200 bg-white p-6">
        {/* Template header */}
        <div className="flex items-start gap-4">
          {template.format === "docx" ? (
            <FileText className="h-12 w-12 shrink-0 text-[#1A6BD8]" />
          ) : (
            <FileSpreadsheet className="h-12 w-12 shrink-0 text-emerald-600" />
          )}
          <div>
            <h2 className="text-lg font-semibold text-neutral-800">
              {template.name}
            </h2>
            <p className="mt-1 text-sm text-neutral-500">
              {template.phase} · {template.filename}
            </p>
            <p className="mt-0.5 text-xs text-neutral-400">
              {(template.file_size / 1024).toFixed(0)} KB · {template.format.toUpperCase()}
            </p>
          </div>
        </div>

        {/* Fields summary */}
        {schema && (
          <div className="mt-6">
            <h3 className="text-sm font-medium text-neutral-700">
              字段列表 ({schema.fields.length})
            </h3>
            <div className="mt-3 space-y-2">
              {schema.fields.map((f) => (
                <div
                  key={f.name}
                  className="flex items-center justify-between rounded-md border border-neutral-100 bg-neutral-50 px-3 py-2"
                >
                  <div>
                    <span className="text-sm font-medium text-neutral-700">
                      {f.name}
                    </span>
                    {f.description && (
                      <span className="ml-2 text-xs text-neutral-400">
                        {f.description}
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2">
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                        f.fill_strategy === "ai"
                          ? "bg-purple-50 text-purple-600"
                          : f.fill_strategy === "user"
                            ? "bg-amber-50 text-amber-600"
                            : "bg-neutral-100 text-neutral-500"
                      }`}
                    >
                      {f.fill_strategy}
                    </span>
                    {f.required && (
                      <span className="text-[10px] text-red-400">*</span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Next button */}
        <div className="mt-6 flex justify-end">
          <button
            type="button"
            onClick={onNext}
            className="flex items-center gap-1.5 rounded-md bg-[#1A6BD8] px-4 py-2 text-sm text-white hover:bg-[#1558B0]"
          >
            下一步
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  );
}

function StepFill({
  schema,
  fieldValues,
  userInput,
  onFieldChange,
  onUserInputChange,
  onSmartFill,
  filling,
  fillResult,
  onBack,
  onNext,
}: {
  schema: TemplateSchema;
  fieldValues: Record<string, string>;
  userInput: string;
  onFieldChange: (name: string, value: string) => void;
  onUserInputChange: (value: string) => void;
  onSmartFill: () => void;
  filling: boolean;
  fillResult: SmartFillResult | null;
  onBack: () => void;
  onNext: () => void;
}) {
  return (
    <div className="mx-auto max-w-2xl">
      <div className="rounded-lg border border-neutral-200 bg-white p-6">
        <h2 className="text-lg font-semibold text-neutral-800 mb-4">
          填写字段
        </h2>

        {/* User input for context */}
        <div className="mb-6">
          <label htmlFor="wizard-user-input" className="block text-sm font-medium text-neutral-700 mb-1.5">
            项目描述（用于 AI 生成上下文）
          </label>
          <textarea
            id="wizard-user-input"
            value={userInput}
            onChange={(e) => onUserInputChange(e.target.value)}
            placeholder="简要描述项目背景、目标、范围等，帮助 AI 生成更准确的内容…"
            rows={3}
            className="w-full rounded-md border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20"
          />
        </div>

        {/* Smart fill button */}
        <div className="mb-6">
          <button
            type="button"
            onClick={onSmartFill}
            disabled={filling}
            className="flex items-center gap-1.5 rounded-md border border-[#1A6BD8] bg-white px-4 py-2 text-sm text-[#1A6BD8] hover:bg-[#1A6BD8]/5 disabled:opacity-50"
          >
            {filling ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Wand2 className="h-4 w-4" />
            )}
            AI 智能填充
          </button>
          {fillResult && (
            <p className="mt-1.5 text-xs text-neutral-500">
              AI 填充了 {fillResult.ai_fields.length} 个字段
              {fillResult.kb_sources.length > 0 && (
                <span>
                  ，引用了 {fillResult.kb_sources.length} 个知识库来源
                </span>
              )}
            </p>
          )}
        </div>

        {/* Field inputs */}
        <div className="space-y-4">
          {schema.fields.map((field) => (
            <FieldInput
              key={field.name}
              field={field}
              value={fieldValues[field.name] ?? ""}
              onChange={(v) => onFieldChange(field.name, v)}
              isAiFilled={fillResult?.ai_fields.includes(field.name) ?? false}
            />
          ))}
        </div>

        {/* Navigation */}
        <div className="mt-6 flex justify-between">
          <button
            type="button"
            onClick={onBack}
            className="flex items-center gap-1.5 rounded-md border border-neutral-200 bg-white px-4 py-2 text-sm text-neutral-600 hover:bg-neutral-50"
          >
            <ChevronLeft className="h-4 w-4" />
            上一步
          </button>
          <button
            type="button"
            onClick={onNext}
            className="flex items-center gap-1.5 rounded-md bg-[#1A6BD8] px-4 py-2 text-sm text-white hover:bg-[#1558B0]"
          >
            下一步
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  );
}

function FieldInput({
  field,
  value,
  onChange,
  isAiFilled,
}: {
  field: SchemaField;
  value: string;
  onChange: (value: string) => void;
  isAiFilled: boolean;
}) {
  const isTextarea =
    field.fill_strategy === "ai" || field.type === "text";

  return (
    <div>
      <label htmlFor={`field-${field.name}`} className="flex items-center gap-1.5 text-sm font-medium text-neutral-700 mb-1.5">
        {field.name}
        {field.required && <span className="text-red-400">*</span>}
        {isAiFilled && (
          <span className="rounded bg-purple-50 px-1.5 py-0.5 text-[10px] text-purple-600">
            AI 生成
          </span>
        )}
      </label>
      {field.description && (
        <p className="text-xs text-neutral-400 mb-1.5">{field.description}</p>
      )}
      {isTextarea ? (
        <textarea
          id={`field-${field.name}`}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          rows={4}
          className={`w-full rounded-md border px-3 py-2 text-sm text-neutral-700 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20 ${
            isAiFilled
              ? "border-purple-200 bg-purple-50/50"
              : "border-neutral-200 bg-white"
          }`}
        />
      ) : (
        <input
          id={`field-${field.name}`}
          type={field.type === "number" ? "number" : "text"}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className={`w-full rounded-md border px-3 py-2 text-sm text-neutral-700 outline-none focus:border-[#1A6BD8] focus:ring-1 focus:ring-[#1A6BD8]/20 ${
            isAiFilled
              ? "border-purple-200 bg-purple-50/50"
              : "border-neutral-200 bg-white"
          }`}
        />
      )}
    </div>
  );
}

function StepGenerate({
  template,
  schema,
  fieldValues,
  onGenerate,
  generating,
  generatedDoc,
  onBack,
  onDone,
}: {
  template: TemplateInfo;
  schema: TemplateSchema;
  fieldValues: Record<string, string>;
  onGenerate: () => void;
  generating: boolean;
  generatedDoc: GeneratedDoc | null;
  onBack: () => void;
  onDone: () => void;
}) {
  const filledCount = schema.fields.filter(
    (f) => fieldValues[f.name]?.trim()
  ).length;
  const totalCount = schema.fields.length;

  return (
    <div className="mx-auto max-w-2xl">
      <div className="rounded-lg border border-neutral-200 bg-white p-6">
        <h2 className="text-lg font-semibold text-neutral-800 mb-4">
          生成文档
        </h2>

        {/* Summary */}
        <div className="mb-6 rounded-md border border-neutral-100 bg-neutral-50 p-4">
          <h3 className="text-sm font-medium text-neutral-700 mb-2">
            填写摘要
          </h3>
          <p className="text-sm text-neutral-600">
            已填写 {filledCount}/{totalCount} 个字段
          </p>
          <p className="text-xs text-neutral-400 mt-1">
            模板: {template.name}
          </p>
        </div>

        {/* Generate button or result */}
        {!generatedDoc ? (
          <div className="text-center">
            <button
              type="button"
              onClick={onGenerate}
              disabled={generating}
              className="inline-flex items-center gap-2 rounded-md bg-[#1A6BD8] px-6 py-3 text-sm font-medium text-white hover:bg-[#1558B0] disabled:opacity-50"
            >
              {generating ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  生成中…
                </>
              ) : (
                <>
                  <Download className="h-4 w-4" />
                  生成文档
                </>
              )}
            </button>
          </div>
        ) : (
          <div className="rounded-md border border-emerald-200 bg-emerald-50 p-4">
            <div className="flex items-center gap-2">
              <CheckCircle2 className="h-5 w-5 text-emerald-600" />
              <h3 className="text-sm font-medium text-emerald-800">
                生成完成
              </h3>
            </div>
            <div className="mt-2 space-y-1 text-sm text-emerald-700">
              <p>填充字段: {generatedDoc.fields_filled}</p>
              <p>AI 生成: {generatedDoc.ai_fields.length} 个</p>
              {generatedDoc.missing_fields.length > 0 && (
                <p className="text-amber-600">
                  未填充: {generatedDoc.missing_fields.length} 个
                </p>
              )}
              <p className="text-xs text-emerald-600 break-all mt-2">
                输出路径: {generatedDoc.output_path}
              </p>
            </div>
          </div>
        )}

        {/* Navigation */}
        <div className="mt-6 flex justify-between">
          <button
            type="button"
            onClick={onBack}
            className="flex items-center gap-1.5 rounded-md border border-neutral-200 bg-white px-4 py-2 text-sm text-neutral-600 hover:bg-neutral-50"
          >
            <ChevronLeft className="h-4 w-4" />
            上一步
          </button>
          {generatedDoc && (
            <button
              type="button"
              onClick={onDone}
              className="rounded-md bg-[#1A6BD8] px-4 py-2 text-sm text-white hover:bg-[#1558B0]"
            >
              完成
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
