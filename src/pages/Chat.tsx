import { useState, useCallback, useRef, useEffect, type Dispatch, type SetStateAction } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useNavigate } from "react-router-dom";
import {
  Send,
  Trash2,
  Loader2,
  AlertCircle,
  Brain,
  Paperclip,
  X,
  FileText,
  Image as ImageIcon,
  StopCircle,
  RefreshCw,
  Settings,
  ChevronDown,
  ChevronUp,
  BookOpen,
} from "lucide-react";
import {
  useAgent,
  DEFAULT_SLOT,
  buildAgentHistory,
  type AgentMessage,
  type RAGSource,
} from "../contexts/AgentContext";
import {
  isLLMConfigured,
  extractFileText,
  ingestFile,
  saveChatMemory,
} from "../lib/tauri-commands";
import { listLLMProviders, processImage } from "../lib/skill-commands";
import type { LLMProviderConfig } from "../lib/skill-types";

interface ChatAttachment {
  id: string;
  path: string;
  name: string;
  kind: "document" | "image" | "unsupported";
  status: "ready" | "ingesting" | "parsed" | "ingested" | "error";
  documentId?: number;
  extractedText?: string;
  charCount?: number;
  error?: string;
}

function nextId(): string {
  return crypto.randomUUID();
}

const CHAT_STORAGE_KEY = "kingdee_kb_chat_history";
const MAX_STORED_MESSAGES = 500;
const CHAT_ATTACHMENT_PROJECT = "chat-attachments";
const ACTIVE_PROJECT_KEY = "kingdee_kb_active_project";
const MAX_ATTACHMENT_PROMPT_CHARS = 12000;
const DOCUMENT_EXTENSIONS = new Set([
  "md",
  "txt",
  "text",
  "markdown",
  "html",
  "htm",
  "pdf",
  "doc",
  "docx",
  "xlsx",
  "xls",
]);
const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "webp", "bmp", "gif"]);

function loadChatHistory(): AgentMessage[] {
  try {
    const raw = localStorage.getItem(CHAT_STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return (parsed as AgentMessage[]).filter((m) => m.id && m.role);
  } catch {
    return [];
  }
}

function saveChatHistory(messages: AgentMessage[]) {
  try {
    const clean = messages.map((m) => ({ ...m, streaming: false }));
    const trimmed = clean.length > MAX_STORED_MESSAGES
      ? clean.slice(clean.length - MAX_STORED_MESSAGES)
      : clean;
    localStorage.setItem(CHAT_STORAGE_KEY, JSON.stringify(trimmed));
  } catch {
    // ignore
  }
}

function summarizeToolArgs(args: string): string {
  if (!args.trim()) return "";
  try {
    const parsed = JSON.parse(args) as Record<string, unknown>;
    const parts: string[] = [];
    for (const key of ["skill_name", "script", "action", "name_or_query", "template_id", "project_name"]) {
      const value = parsed[key];
      if (typeof value === "string") parts.push(`${key}: ${value}`);
    }
    if (Array.isArray(parsed.args)) parts.push(`args: ${parsed.args.length} 项`);
    if (Array.isArray(parsed.input_files)) parts.push(`input_files: ${parsed.input_files.length} 个文件`);
    return parts.join("\n") || `参数 ${args.length} 字符`;
  } catch {
    return args.length > 240 ? `参数 ${args.length} 字符` : args;
  }
}

export default function Chat() {
  const agent = useAgent();
  const navigate = useNavigate();
  const slot = agent.slots.get("chat") ?? DEFAULT_SLOT;
  const { messages, loading, currentTrace } = slot;

  const [input, setInput] = useState("");
  const [attachments, setAttachments] = useState<ChatAttachment[]>([]);
  const [attaching, setAttaching] = useState(false);
  const [llmReady, setLlmReady] = useState<boolean | null>(null);
  const [providers, setProviders] = useState<LLMProviderConfig[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState<string>("");
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const lastInputRef = useRef<{ text: string; attachments: ChatAttachment[] } | null>(null);

  // Load chat history from localStorage into slot on mount
  const didLoadRef = useRef(false);
  useEffect(() => {
    if (didLoadRef.current) return;
    didLoadRef.current = true;
    if (slot.messages.length === 0) {
      const history = loadChatHistory();
      if (history.length > 0) {
        agent.updateMessages("chat", () => history);
      }
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Check LLM on mount
  useEffect(() => {
    isLLMConfigured()
      .then(setLlmReady)
      .catch(() => setLlmReady(false));
  }, []);

  // Load LLM providers on mount
  useEffect(() => {
    listLLMProviders()
      .then((fetchedProviders) => {
        setProviders(fetchedProviders);
        // Pre-select default provider
        const defaultProvider = fetchedProviders.find((p) => p.is_default);
        if (defaultProvider) {
          setSelectedProviderId(defaultProvider.id);
        } else if (fetchedProviders.length > 0) {
          setSelectedProviderId(fetchedProviders[0].id);
        }
      })
      .catch((err) => {
        console.warn("[Chat] Failed to load LLM providers:", err);
      });
  }, []);

  // Auto-scroll
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, currentTrace]);

  // Close dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setDropdownOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if ((!text && attachments.length === 0) || loading || attaching) return;

    // Check if LLM is configured before sending
    if (llmReady === false) {
      alert("尚未配置 AI 模型，请前往【设置 → AI 模型】添加 LLM 供应商");
      return;
    }

    // Preserve input for retry before clearing
    lastInputRef.current = { text: input, attachments: [...attachments] };

    setAttaching(true);
    const preparedAttachments = await prepareAttachmentsForSend(attachments, setAttachments);
    setAttaching(false);

    const attachmentPrompt = buildAttachmentPrompt(preparedAttachments);
    const outboundText = [text, attachmentPrompt].filter(Boolean).join("\n\n");
    const visibleText = [text || "请分析附件", buildAttachmentDisplay(preparedAttachments)]
      .filter(Boolean)
      .join("\n\n");

    setInput("");
    setAttachments([]);

    const projectId = localStorage.getItem(ACTIVE_PROJECT_KEY) || undefined;
    const history = buildAgentHistory(messages);
    await agent.sendMessage("chat", outboundText, {
      displayText: visibleText,
      history,
      projectId,
      providerId: selectedProviderId || undefined,
    });
  }, [input, attachments, loading, attaching, messages, agent, selectedProviderId, llmReady]);

  // Retry last failed message
  const handleRetry = useCallback(async () => {
    if (loading || !lastInputRef.current) return;
    const { text, attachments: prevAttachments } = lastInputRef.current;
    setInput(text);
    setAttachments(prevAttachments);
    // Use a short delay to allow state to update, then trigger send
    setTimeout(() => {
      inputRef.current?.focus();
    }, 50);
  }, [loading]);

  // Navigate to settings section
  const handleNavigateSettings = useCallback((section?: string) => {
    navigate(section ? `/settings?section=${section}` : "/settings");
  }, [navigate]);

  const handleAttach = useCallback(async () => {
    if (loading || attaching) return;
    try {
      const selected = await open({
        multiple: true,
        directory: false,
        filters: [
          {
            name: "文档和图片",
            extensions: [
              "md",
              "txt",
              "pdf",
              "doc",
              "docx",
              "xlsx",
              "xls",
              "html",
              "htm",
              "png",
              "jpg",
              "jpeg",
              "webp",
              "bmp",
              "gif",
            ],
          },
        ],
      });
      if (!selected) return;

      const paths = Array.isArray(selected) ? selected : [selected];
      const next = paths.map(createAttachment);
      setAttachments((prev) => [...prev, ...next]);
    } catch (err) {
      agent.updateMessages("chat", (prev) => [
        ...prev,
        {
          id: nextId(),
          role: "assistant",
          content: `附件选择失败：${String(err)}`,
          error: true,
        },
      ]);
    }
  }, [loading, attaching, agent]);

  const removeAttachment = useCallback((id: string) => {
    setAttachments((prev) => prev.filter((a) => a.id !== id));
  }, []);

  // Cancel running agent stream
  const handleCancel = useCallback(async () => {
    await agent.cancelSession("chat");
  }, [agent]);

  // Answer a pending clarification question
  const handleClarify = useCallback(async (questionId: string, answer: string) => {
    await agent.answerClarification("chat", questionId, answer);
  }, [agent]);

  // Save to localStorage debounced
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    saveTimeoutRef.current = setTimeout(() => saveChatHistory(messages), 500);
    return () => { if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current); };
  }, [messages]);

  // Save chat memory when session completes (loading transitions true → false)
  const prevLoadingRef = useRef(loading);
  useEffect(() => {
    if (prevLoadingRef.current && !loading && messages.length > 0) {
      const lastMsg = messages[messages.length - 1];
      if (lastMsg.role === "assistant" && !lastMsg.error) {
        const conversation = messages
          .filter((m) => !m.error)
          .map((m) => ({ role: m.role, content: m.content }));
        saveChatMemory(conversation).catch((e) =>
          console.warn("[Chat] Failed to save chat memory:", e)
        );
      }
    }
    prevLoadingRef.current = loading;
  }, [loading, messages]);

  const handleClear = useCallback(() => {
    agent.clearSlot("chat");
    localStorage.removeItem(CHAT_STORAGE_KEY);
  }, [agent]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend]
  );

  const selectedProvider = providers.find((p) => p.id === selectedProviderId);

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex h-14 items-center justify-between border-b border-neutral-200 px-6">
        <div className="flex items-center gap-2">
          <Brain className="h-5 w-5 text-amber-600" />
          <h1 className="text-base font-semibold text-neutral-800">AI 助手</h1>
          <span className="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-medium text-amber-700">Agent</span>
          <span className="text-xs text-neutral-400">
            {messages.filter((m) => m.role === "user").length} 轮对话
          </span>
        </div>
        <button type="button" onClick={handleClear}
          className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium text-neutral-500 hover:bg-neutral-100 transition-colors">
          <Trash2 className="h-3.5 w-3.5" />
          清空对话
        </button>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-4">
        <div className="mx-auto max-w-3xl space-y-4">
          {messages.length === 0 && !loading ? (
            <div className="flex flex-col items-center justify-center pt-20 text-center">
              <div className="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-amber-50">
                <Brain className="h-8 w-8 text-amber-300" />
              </div>
              <p className="text-sm font-medium text-neutral-500">输入问题开始对话</p>
              <p className="mt-1 text-xs text-neutral-400">Agent 可以搜索知识库、生成文档、分析风险</p>
              {llmReady === false && (
                <div className="mt-4 inline-flex items-center gap-1.5 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700">
                  <AlertCircle className="h-3.5 w-3.5" />
                  LLM 未配置，请先在设置中填写 API Key
                </div>
              )}
            </div>
          ) : (
            messages.map((msg) => (
              <MessageBubble
                key={msg.id}
                message={msg}
                onClarify={handleClarify}
                onRetry={handleRetry}
                onNavigateSettings={handleNavigateSettings}
              />
            ))
          )}

          {/* ReAct Trace (while loading) */}
          {loading && (currentTrace.thinking || currentTrace.toolCalls.length > 0) && (
            <div className="space-y-2 border-l-2 border-amber-200 pl-4">
              {currentTrace.thinking && (
                <div className="text-xs text-amber-700 italic leading-relaxed">
                  🤔 {currentTrace.thinking}
                </div>
              )}
              {currentTrace.toolCalls.map((tc, i) => (
                <div key={i}>
                  <details className="rounded-lg border border-amber-200 bg-amber-50 text-xs" open={i === currentTrace.toolCalls.length - 1}>
                    <summary className="cursor-pointer px-3 py-2 font-medium text-amber-800">
                      🔧 {tc.name}
                    </summary>
                    {tc.args && (
                      <pre className="max-h-60 overflow-auto border-t border-amber-200 bg-white/70 px-3 py-2 font-mono text-[11px] leading-relaxed text-amber-950 whitespace-pre-wrap break-words">
                        {summarizeToolArgs(tc.args)}
                      </pre>
                    )}
                  </details>
                  {tc.result && (
                    <div className="rounded-b-lg border border-green-200 border-t-0 bg-green-50 px-3 py-2 text-xs text-green-700">
                      工具执行完成
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Input bar */}
      <div className="border-t border-neutral-200 bg-white p-4">
        <div className="mx-auto max-w-3xl">
          {attachments.length > 0 && (
            <div className="mb-3 flex flex-wrap gap-2">
              {attachments.map((attachment) => (
                <AttachmentChip
                  key={attachment.id}
                  attachment={attachment}
                  onRemove={() => removeAttachment(attachment.id)}
                />
              ))}
            </div>
          )}
          <div className="flex items-end gap-2">
            <button
              type="button"
              onClick={handleAttach}
              disabled={loading || attaching}
              title="添加附件"
              className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border border-neutral-200 bg-white text-neutral-500 hover:bg-neutral-50 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {attaching ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Paperclip className="h-4 w-4" />
              )}
            </button>

            {/* Model Selector */}
            {providers.length > 0 && (
              <div ref={dropdownRef} className="relative shrink-0">
                <button
                  type="button"
                  onClick={() => setDropdownOpen(!dropdownOpen)}
                  disabled={loading}
                  className="flex h-10 items-center gap-1.5 rounded-lg border border-neutral-200 bg-white px-3 text-xs font-medium text-neutral-700 hover:bg-neutral-50 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  title="选择模型"
                >
                  <Brain className="h-3.5 w-3.5 text-amber-600" />
                  <span className="max-w-[120px] truncate">
                    {selectedProvider?.model || "选择模型"}
                  </span>
                  {selectedProvider?.is_multimodal && (
                    <span className="rounded bg-blue-100 px-1 py-0.5 text-[9px] font-medium text-blue-700">
                      多模态
                    </span>
                  )}
                   <svg
                     className={`h-3 w-3 text-neutral-400 transition-transform ${dropdownOpen ? "rotate-180" : ""}`}
                     fill="none"
                     viewBox="0 0 24 24"
                     stroke="currentColor"
                     aria-hidden="true"
                   >
                     <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
                   </svg>
                </button>

                {dropdownOpen && (
                  <div className="absolute bottom-full left-0 z-50 mb-1 w-64 rounded-lg border border-neutral-200 bg-white py-1 shadow-lg">
                    <div className="px-3 py-1.5 text-[10px] font-medium text-neutral-400 uppercase tracking-wider">
                      选择模型
                    </div>
                    {providers.map((provider) => (
                      <button
                        key={provider.id}
                        type="button"
                        onClick={() => {
                          setSelectedProviderId(provider.id);
                          setDropdownOpen(false);
                        }}
                        className={`flex w-full items-center justify-between px-3 py-2 text-left text-xs hover:bg-neutral-50 transition-colors ${
                          provider.id === selectedProviderId ? "bg-amber-50 text-amber-700" : "text-neutral-700"
                        }`}
                      >
                        <div className="flex flex-col gap-0.5 min-w-0">
                          <span className="font-medium truncate">{provider.name}</span>
                          <span className="text-[10px] text-neutral-400 truncate">{provider.model}</span>
                        </div>
                        <div className="flex items-center gap-1.5 shrink-0">
                          {provider.is_multimodal && (
                            <span className="rounded bg-blue-100 px-1 py-0.5 text-[9px] font-medium text-blue-700">
                              多模态
                            </span>
                          )}
                          {provider.is_default && (
                            <span className="rounded bg-green-100 px-1 py-0.5 text-[9px] font-medium text-green-700">
                              默认
                            </span>
                          )}
                           {provider.id === selectedProviderId && (
                             <svg className="h-3.5 w-3.5 text-amber-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" aria-hidden="true">
                               <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
                             </svg>
                           )}
                        </div>
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}

            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入问题，或先添加文档/图片附件..."
              rows={1}
              disabled={loading}
              className="flex-1 resize-none rounded-lg border border-neutral-200 bg-white px-4 py-2.5 text-sm text-neutral-700 placeholder-neutral-400 outline-none focus:border-amber-500 focus:ring-2 focus:ring-amber-500/20 disabled:opacity-50"
            />
            <button
              type="button"
              onClick={handleSend}
              disabled={loading || attaching || (!input.trim() && attachments.length === 0)}
              className="flex h-10 w-10 items-center justify-center rounded-lg bg-amber-600 text-white hover:bg-amber-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {loading ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Send className="h-4 w-4" />
              )}
            </button>
            {loading && (
              <button
                type="button"
                onClick={handleCancel}
                title="取消生成"
                className="flex h-10 w-10 items-center justify-center rounded-lg bg-red-500 text-white hover:bg-red-600 transition-colors"
              >
                <StopCircle className="h-4 w-4" />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function createAttachment(path: string): ChatAttachment {
  const name = path.split(/[\\/]/).pop() || path;
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  const kind = DOCUMENT_EXTENSIONS.has(ext)
    ? "document"
    : IMAGE_EXTENSIONS.has(ext)
      ? "image"
      : "unsupported";
  return {
    id: nextId(),
    path,
    name,
    kind,
    status: kind === "unsupported" ? "error" : "ready",
    error:
      kind === "unsupported"
        ? "当前格式暂不支持内容解析。"
        : undefined,
  };
}

async function prepareAttachmentsForSend(
  attachments: ChatAttachment[],
  setAttachments: Dispatch<SetStateAction<ChatAttachment[]>>
): Promise<ChatAttachment[]> {
  const prepared = [...attachments];

  for (let i = 0; i < prepared.length; i++) {
    const attachment = prepared[i];

    // Process images via OCR/vision
    if (attachment.kind === "image" && !attachment.extractedText && attachment.status !== "error") {
      setAttachments((prev) =>
        prev.map((a) =>
          a.id === attachment.id ? { ...a, status: "ingesting", error: undefined } : a
        )
      );

      try {
        const result = await processImage(attachment.path);
        const text = result.ocr_text || result.description || "";
        prepared[i] = {
          ...attachment,
          status: "parsed",
          extractedText: text,
          charCount: text.length,
          error: undefined,
        };
        setAttachments((prev) =>
          prev.map((a) => (a.id === attachment.id ? prepared[i] : a))
        );
      } catch (err) {
        prepared[i] = {
          ...attachment,
          status: "error",
          error: `图片处理失败：${String(err)}`,
        };
        setAttachments((prev) =>
          prev.map((a) => (a.id === attachment.id ? prepared[i] : a))
        );
      }
      continue;
    }

    // Skip non-documents or already processed
    if (
      attachment.kind !== "document" ||
      attachment.extractedText ||
      attachment.status === "error"
    ) {
      continue;
    }

    setAttachments((prev) =>
      prev.map((a) =>
        a.id === attachment.id ? { ...a, status: "ingesting", error: undefined } : a
      )
    );

    try {
      const extracted = await extractFileText(attachment.path);
      prepared[i] = {
        ...attachment,
        status: "parsed",
        extractedText: extracted.text,
        charCount: extracted.char_count,
      };
      setAttachments((prev) =>
        prev.map((a) => (a.id === attachment.id ? prepared[i] : a))
      );

      try {
        const result = await ingestFile(attachment.path, CHAT_ATTACHMENT_PROJECT);
        prepared[i] = {
          ...prepared[i],
          status: "ingested",
          documentId: result.document_id,
          error: undefined,
        };
        setAttachments((prev) =>
          prev.map((a) => (a.id === attachment.id ? prepared[i] : a))
        );
      } catch (ingestErr) {
        prepared[i] = {
          ...prepared[i],
          status: "parsed",
          error: `已解析，入库失败：${String(ingestErr)}`,
        };
        setAttachments((prev) =>
          prev.map((a) => (a.id === attachment.id ? prepared[i] : a))
        );
      }
    } catch (err) {
      prepared[i] = {
        ...attachment,
        status: "error",
        error: normalizeAttachmentError(String(err), attachment.name),
      };
      setAttachments((prev) =>
        prev.map((a) => (a.id === attachment.id ? prepared[i] : a))
      );
    }
  }

  return prepared;
}

function normalizeAttachmentError(error: string, filename: string): string {
  const ext = filename.split(".").pop()?.toLowerCase();
  if (ext === "pdf") {
    return `PDF 文本解析失败：${error}。如果这是扫描件或图片型 PDF，需要 OCR/多模态模型；如果是加密 PDF，请先另存为可复制文本的 PDF。`;
  }
  if (ext === "doc") {
    return `DOC 文本解析失败：${error}。.doc 需要本机安装 Microsoft Word 才能解析。`;
  }
  return error;
}

function buildAttachmentDisplay(attachments: ChatAttachment[]): string {
  if (attachments.length === 0) return "";
  return [
    "附件：",
    ...attachments.map((a) => {
      const status = a.status === "ingested" && a.documentId
        ? `已入库 #${a.documentId}`
        : a.status === "parsed"
          ? (a.error ?? "已解析")
        : a.kind === "image" && a.extractedText
          ? "已识别"
        : a.error
          ? a.error
          : a.status;
      return `- ${a.name}（${status}）`;
    }),
  ].join("\n");
}

function buildAttachmentPrompt(attachments: ChatAttachment[]): string {
  if (attachments.length === 0) return "";
  const documents = attachments.filter((a) =>
    a.kind === "document" &&
    (a.status === "parsed" || a.status === "ingested") &&
    Boolean(a.extractedText)
  );
  const processedImages = attachments.filter((a) =>
    a.kind === "image" &&
    (a.status === "parsed") &&
    Boolean(a.extractedText)
  );
  const unprocessedImages = attachments.filter((a) =>
    a.kind === "image" && !a.extractedText
  );
  const failed = attachments.filter((a) => a.status === "error" || a.kind === "unsupported");
  const lines = ["【本轮附件】"];

  if (documents.length > 0) {
    lines.push("以下文档已解析。优先基于摘录回答；如果摘录不足，再调用 search-knowledge 检索这些附件内容：");
    for (const doc of documents) {
      const fullText = doc.extractedText ?? "";
      const excerpt = truncateForPrompt(fullText, MAX_ATTACHMENT_PROMPT_CHARS);
      const omitted = Math.max(0, (doc.charCount ?? fullText.length) - excerpt.length);
      lines.push(`\n--- 附件：${doc.name} ---`);
      lines.push(`document_id=${doc.documentId ?? "unknown"}，project=${CHAT_ATTACHMENT_PROJECT}，字符数=${doc.charCount ?? fullText.length}`);
      lines.push(excerpt || "（未提取到文本）");
      if (omitted > 0) {
        lines.push(`（后续还有约 ${omitted} 字未直接放入上下文，可用 search-knowledge 继续检索）`);
      }
    }
  }
  if (processedImages.length > 0) {
    lines.push("以下图片已通过 OCR/视觉模型解析，优先基于解析内容回答：");
    for (const image of processedImages) {
      const fullText = image.extractedText ?? "";
      const excerpt = truncateForPrompt(fullText, MAX_ATTACHMENT_PROMPT_CHARS);
      lines.push(`\n--- 图片：${image.name} ---`);
      lines.push(`字符数=${image.charCount ?? fullText.length}`);
      lines.push(excerpt || "（未提取到文本）");
    }
  }
  if (unprocessedImages.length > 0) {
    lines.push("以下图片未能解析内容，回答时不要声称已读取其内容：");
    for (const image of unprocessedImages) {
      lines.push(`- ${image.name}，path=${image.path}`);
    }
  }
  if (failed.length > 0) {
    lines.push("以下附件未成功解析，回答时不要声称已读取其内容：");
    for (const item of failed) {
      lines.push(`- ${item.name}，原因：${item.error ?? "不支持"}`);
    }
  }

  return lines.join("\n");
}

function truncateForPrompt(text: string, maxChars: number): string {
  if (text.length <= maxChars) return text;
  return `${text.slice(0, maxChars)}\n\n[附件内容过长，已截断]`;
}

function AttachmentChip({
  attachment,
  onRemove,
}: {
  attachment: ChatAttachment;
  onRemove: () => void;
}) {
  const preview = attachment.kind === "image" ? convertFileSrc(attachment.path) : undefined;
  const statusText =
    attachment.status === "ingesting"
      ? "入库中"
      : attachment.status === "parsed"
        ? "已解析"
      : attachment.status === "ingested"
        ? attachment.kind === "image"
          ? "图片"
          : "已入库"
        : attachment.status === "error"
          ? "失败"
          : "待入库";

  return (
    <div className="group flex max-w-[260px] items-center gap-2 rounded-lg border border-neutral-200 bg-neutral-50 px-2.5 py-2">
      {preview ? (
        <img src={preview} alt="" className="h-8 w-8 rounded object-cover" />
      ) : attachment.kind === "document" ? (
        <FileText className="h-4 w-4 shrink-0 text-amber-600" />
      ) : (
        <ImageIcon className="h-4 w-4 shrink-0 text-neutral-500" />
      )}
      <div className="min-w-0 flex-1">
        <div className="truncate text-xs font-medium text-neutral-700">{attachment.name}</div>
        <div className={`text-[10px] ${
          attachment.status === "error" || attachment.kind === "unsupported"
            ? "text-red-500"
            : "text-neutral-400"
        }`}>
          {statusText}
        </div>
      </div>
      {attachment.status === "ingesting" ? (
        <Loader2 className="h-3.5 w-3.5 animate-spin text-neutral-400" />
      ) : (
        <button
          type="button"
          onClick={onRemove}
          title="移除附件"
          className="rounded p-1 text-neutral-400 hover:bg-neutral-200 hover:text-neutral-700"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      )}
    </div>
  );
}

// ── Message Bubble Component ──────────────────────────────────────────────

function MessageBubble({
  message,
  onClarify,
  onRetry,
  onNavigateSettings,
}: {
  message: AgentMessage;
  onClarify: (questionId: string, answer: string) => void;
  onRetry: () => void;
  onNavigateSettings: (section?: string) => void;
}) {
  const isUser = message.role === "user";
  const [freeInput, setFreeInput] = useState("");

  const isLLMError = message.error && /未配置|api.?key|llm|模型|unauthorized|401/i.test(message.content);
  const isTimeoutError = message.error && /超时|timeout/i.test(message.content);

  return (
    <div className={`flex ${isUser ? "justify-end" : "justify-start"}`}>
      <div className={`max-w-[80%] ${isUser ? "" : "w-full"}`}>
        {/* Avatar */}
        <div className={`mb-1 flex items-center gap-1.5 text-xs ${
          isUser ? "justify-end text-neutral-400" : "text-amber-600"
        }`}>
          <span className="flex h-5 w-5 items-center justify-center rounded-full bg-neutral-100 text-[10px]">
            {isUser ? "👤" : "🤖"}
          </span>
          <span className="font-medium">{isUser ? "你" : "AI 助手"}</span>
        </div>

        {/* Message */}
        <div className={`rounded-2xl px-4 py-3 text-sm leading-relaxed ${
          isUser
            ? "bg-amber-600 text-white rounded-tr-md"
            : message.error
            ? "bg-red-50 text-red-700 border border-red-200 rounded-tl-md"
            : "bg-white text-neutral-700 border border-neutral-200 rounded-tl-md shadow-sm"
        }`}>
          {isUser ? (
            <div className="whitespace-pre-wrap">{message.content}</div>
          ) : (
            <div className="prose prose-sm max-w-none prose-headings:text-neutral-800 prose-a:text-amber-600 prose-code:bg-neutral-100 prose-code:px-1 prose-code:rounded prose-pre:bg-neutral-900 prose-pre:text-neutral-100">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {message.content.replace(/^\n+/, "")}
              </ReactMarkdown>
              {message.streaming && (
                <span className="ml-1 inline-block h-3.5 w-1.5 animate-pulse bg-amber-500 rounded-sm align-middle" />
              )}
            </div>
          )}

          {/* Error action buttons */}
          {message.error && !message.streaming && (
            <div className="mt-3 flex flex-wrap gap-2 border-t border-red-200 pt-3">
              {(isLLMError || isTimeoutError) && (
                <button
                  type="button"
                  onClick={() => onNavigateSettings("llm")}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-red-300 bg-white px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-50 transition-colors"
                >
                  <Settings className="h-3 w-3" />
                  去设置
                </button>
              )}
              <button
                type="button"
                onClick={onRetry}
                className="inline-flex items-center gap-1.5 rounded-lg border border-red-300 bg-white px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-50 transition-colors"
              >
                <RefreshCw className="h-3 w-3" />
                重试
              </button>
            </div>
          )}

          {/* RAG Sources */}
          {!isUser && !message.error && message.sources && message.sources.length > 0 && (
            <SourcesDisplay sources={message.sources} />
          )}

          {/* Clarification options */}
          {message.clarification && !message.clarificationAnswered && (
            <div className="mt-3 border-t border-neutral-100 pt-3 space-y-2">
              {/* Single choice: clickable buttons */}
              {message.clarification.mode === "single_choice" && (
                <div className="flex flex-wrap gap-2">
                  {message.clarification.options.map((opt) => (
                    <button
                      key={opt}
                      type="button"
                      onClick={() => onClarify(message.clarification!.question_id, opt)}
                      className="rounded-lg border border-amber-200 bg-amber-50 px-3 py-1.5 text-xs font-medium text-amber-700 hover:bg-amber-100 transition-colors"
                    >
                      {opt}
                    </button>
                  ))}
                </div>
              )}

              {/* Multi choice: checkboxes + confirm */}
              {message.clarification.mode === "multi_choice" && (
                <MultiChoiceOptions
                  options={message.clarification.options}
                  questionId={message.clarification.question_id}
                  onConfirm={onClarify}
                />
              )}

              {/* Free input: text box + submit */}
              {message.clarification.mode === "free_input" && (
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={freeInput}
                    onChange={(e) => setFreeInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && freeInput.trim()) {
                        onClarify(message.clarification!.question_id, freeInput.trim());
                        setFreeInput("");
                      }
                    }}
                    placeholder="输入你的回答..."
                    className="flex-1 rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-xs outline-none focus:border-amber-500"
                  />
                  <button
                    type="button"
                    onClick={() => {
                      if (freeInput.trim()) {
                        onClarify(message.clarification!.question_id, freeInput.trim());
                        setFreeInput("");
                      }
                    }}
                    disabled={!freeInput.trim()}
                    className="rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50 transition-colors"
                  >
                    发送
                  </button>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/** Collapsible RAG sources display */
function SourcesDisplay({ sources }: { sources: RAGSource[] }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="mt-3 border-t border-neutral-100 pt-3">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-1.5 text-xs text-neutral-500 hover:text-neutral-700 transition-colors"
      >
        <BookOpen className="h-3 w-3" />
        <span className="font-medium">参考来源 ({sources.length})</span>
        {expanded ? (
          <ChevronUp className="h-3 w-3 ml-auto" />
        ) : (
          <ChevronDown className="h-3 w-3 ml-auto" />
        )}
      </button>
      {expanded && (
        <div className="mt-2 space-y-1.5">
          {sources.map((src, i) => (
            <div
              key={i}
              className="rounded-lg border border-neutral-100 bg-neutral-50 px-3 py-2"
            >
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs font-medium text-neutral-700 truncate">
                  {src.title}
                </span>
                {src.score > 0 && (
                  <span className="shrink-0 rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-medium text-amber-700">
                    {(src.score * 100).toFixed(0)}%
                  </span>
                )}
              </div>
              {src.section_path && (
                <div className="mt-0.5 text-[10px] text-neutral-400 truncate">
                  {src.section_path}
                </div>
              )}
              {src.content_snippet && (
                <div className="mt-1 text-[11px] text-neutral-500 line-clamp-2 leading-relaxed">
                  {src.content_snippet}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** Multi-choice checkbox group with confirm button */
function MultiChoiceOptions({
  options,
  questionId,
  onConfirm,
}: {
  options: string[];
  questionId: string;
  onConfirm: (questionId: string, answer: string) => void;
}) {
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const toggle = (opt: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(opt)) next.delete(opt);
      else next.add(opt);
      return next;
    });
  };

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-2">
        {options.map((opt) => (
          <button
            key={opt}
            type="button"
            onClick={() => toggle(opt)}
            className={`rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors ${
              selected.has(opt)
                ? "border-amber-500 bg-amber-100 text-amber-800"
                : "border-neutral-200 bg-white text-neutral-600 hover:border-amber-200"
            }`}
          >
            {selected.has(opt) ? "✓ " : ""}{opt}
          </button>
        ))}
      </div>
      <button
        type="button"
        onClick={() => onConfirm(questionId, Array.from(selected).join(", "))}
        disabled={selected.size === 0}
        className="rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700 disabled:opacity-50 transition-colors"
      >
        确认选择 ({selected.size})
      </button>
    </div>
  );
}
