/**
 * Tauri API mock for Playwright E2E testing.
 *
 * Injects a mock `window.__TAURI_INTERNALS__` before the app loads so that
 * all `invoke()` calls from `@tauri-apps/api/core` return realistic mock data
 * instead of trying to reach the Rust backend.
 */

import type { Page } from "@playwright/test"

interface MockOptions {
  responses?: Record<string, unknown>
  sequences?: Record<string, unknown[]>
}

/** Map of Tauri command name → mock return value */
const MOCK_RESPONSES: Record<string, unknown> = {
  // Knowledge base
  get_stats: { document_count: 42, chunk_count: 256, db_path: "/data/kingdee.db" },
  list_documents: [],
  get_document_chunks: [],
  delete_document: null,
  delete_documents_batch: 0,
  list_wiki_pages: [],
  get_wiki_page: null,
  approve_wiki_page: null,
  get_graph_neighbors: [],
  get_kb_compilation_enabled: false,
  recompile_failed_kb_sources: { retried: 0, succeeded: 0, failed: [] },
  "plugin:path|resolve_directory": "C:\\Users\\Test\\Documents",

  // Projects
  ensure_default_project: 1,
  list_projects: [
    {
      id: 1,
      name: "默认项目",
      client_name: "",
      description: "",
      current_phase: "startup",
      status: "active",
      document_count: 0,
      product_count: 0,
      created_at: "2026-01-01 00:00:00",
    },
  ],
  create_project: 2,
  archive_project: null,
  restore_project: null,

  // Products
  list_products: [],

  // Templates
  scan_templates: [
    {
      id: "tpl_charter",
      name: "项目章程",
      filename: "项目章程.docx",
      phase: "启动阶段",
      phase_index: 1,
      format: "docx",
      file_path: "/templates/启动阶段/项目章程.docx",
      relative_path: "启动阶段/项目章程.docx",
      file_size: 20480,
    },
    {
      id: "tpl_requirements",
      name: "需求规格说明书",
      filename: "需求规格说明书.docx",
      phase: "需求阶段",
      phase_index: 2,
      format: "docx",
      file_path: "/templates/需求阶段/需求规格说明书.docx",
      relative_path: "需求阶段/需求规格说明书.docx",
      file_size: 40960,
    },
    {
      id: "tpl_tracker",
      name: "问题跟踪表",
      filename: "问题跟踪表.xlsx",
      phase: "测试阶段",
      phase_index: 5,
      format: "xlsx",
      file_path: "/templates/测试阶段/问题跟踪表.xlsx",
      relative_path: "测试阶段/问题跟踪表.xlsx",
      file_size: 15360,
    },
  ],

  // Search
  hybrid_search: [],
  bm25_search: [],

  // Ingestion
  ingest_text: {
    document_id: 1,
    title: "测试文档",
    sha256: "abc123",
    is_duplicate: false,
    chunk_count: 5,
    vector_count: 5,
  },
  ingest_file: {
    document_id: 2,
    title: "测试文件",
    sha256: "def456",
    is_duplicate: false,
    chunk_count: 3,
    vector_count: 3,
  },
  ingest_directory: { imported: [], errors: [] },
  extract_file_text: {
    file_path: "/test.txt",
    title: "测试",
    text: "测试内容",
    char_count: 4,
  },

  // LLM
  is_llm_configured: false,
  get_llm_config: {
    provider: "openai",
    api_key: "",
    base_url: "https://api.openai.com/v1",
    model: "gpt-4o",
    temperature: 0.7,
    max_tokens: 4096,
  },
  set_llm_config: null,
  test_llm_connection: "连接成功",
  rag_query: { answer: "测试回答", sources: [], llm_available: false },
  rag_query_stream: [],
  count_tokens: 10,
  start_chat_stream: null,

  // Chat memory
  save_chat_memory: null,

  // Agent
  agent_chat: "session_123",
  answer_question: null,

  // Embedding model
  init_model: true,
  get_model_status: true,
  get_download_progress: 0,
  get_embedding_model_config: {},
  set_embedding_model_config: true,

  // Template schema / wizard
  extract_template_fields: [],
  get_template_schema: {
    template: { id: "tpl_charter", name: "项目章程", format: "docx", phase: "启动阶段" },
    fields: [],
  },
  smart_fill: { filled_fields: {}, ai_fields: [], missing_fields: [], kb_sources: [] },
  generate_doc: {
    output_path: "/output/test.docx",
    fields_filled: 0,
    user_fields: [],
    ai_fields: [],
    missing_fields: [],
    missing_fields_detail: [],
  },
  get_deliverable_recipe: {
    name: "项目章程",
    template_id: "tpl_charter",
    phase: "启动阶段",
    description: "项目启动阶段的核心文档",
    field_overrides: {},
    system_prompt: "",
  },
  export_product: "/output/test.docx",
  delete_product: null,

  // Research sessions
  create_research_session: 1,
  list_research_sessions: [],
  get_research_session: null,
  update_research_session: null,
  delete_research_session: null,
  add_qa_record: 1,
  update_qa_record: null,
  delete_qa_record: null,
  get_session_records: [],
  export_session_csv: "question,answer\n",
  export_session_markdown: "# QA Records\n",
  reorder_qa_records: null,

  // Whisper
  load_whisper_model: null,
  get_whisper_status: { model_loaded: false, model_size: "tiny", language: "zh" },
  start_whisper_recording: null,
  stop_whisper_recording: { text: "", segments: [], confidence: 0, processing_time_ms: 0 },

  // Video transcription
  transcribe_video_file: {
    video_path: "/test.mp4",
    text: "测试转写内容",
    segments: [],
    confidence: 0.95,
    extraction_time_ms: 100,
    transcription_time_ms: 200,
    duration_secs: 60,
  },
  transcribe_and_ingest_video: {
    transcription: {
      video_path: "/test.mp4",
      text: "测试转写内容",
      segments: [],
      confidence: 0.95,
      extraction_time_ms: 100,
      transcription_time_ms: 200,
      duration_secs: 60,
    },
    ingestion_document_id: 1,
    meeting_minutes: null,
  },
  generate_meeting_minutes_from_transcript: {
    minutes: "# 会议纪要\n\n测试内容",
    generation_time_ms: 100,
  },

  // Risk control
  list_scope_items: [],
  add_scope_item: 1,
  delete_scope_item: null,
  check_scope_creep: {
    risk_level: "green",
    risk_label: "安全",
    explanation: "该需求在合同范围内",
    matched_items: [],
    suggestion: "可以继续推进",
  },
  get_project_health: {
    overall_score: 75,
    risk_level: "critical",
    dimensions: [
      { name: "范围控制", score: 20, weight: 0.3, detail: "范围稳定" },
      { name: "进度管理", score: 15, weight: 0.25, detail: "进度正常" },
    ],
    trend: "稳定",
    alert_count: 0,
  },
  record_health_metric: 1,
  generate_risk_report: "项目风险分析报告：整体风险可控。",
  generate_defense_script: {
    scenario_label: "需求蔓延场景",
    scripts: [
      { phase: "初次沟通", content: "感谢您的建议，我们会认真评估。", tip: "保持专业态度" },
    ],
  },
  extract_scope_from_document: [],
  confirm_scope_items: 0,
  export_database: null,
  import_database: {
    db_size_bytes: 1024000,
    document_count: 15,
    chunk_count: 8,
  },

  // Blueprint / Fit-Gap / Desensitize
  extract_blueprint: "## 蓝图摘要\n\n测试蓝图内容",
  analyze_fit_gap: "## Fit-Gap 分析\n\n测试分析结果",
  desensitize_text: { safe_text: "测试文本", mapping: {} },
  add_sensitive_keyword: null,
  list_sensitive_keywords: [],
  remove_sensitive_keyword: true,

  // Export
  export_report: "/output/report.md",

  // ASR
  list_asr_providers: [],
  recognize_audio_with_provider: {
    text: "",
    is_final: true,
    confidence: 0,
    processing_time_ms: 0,
    segments: null,
  },
  save_asr_config: null,
  get_asr_config_status: { tencent_configured: false, xfyun_configured: false },
}

/**
 * Inject Tauri mocks into a Playwright page.
 * Must be called BEFORE `page.goto()`.
 */
export async function mockTauriApis(page: Page, options: MockOptions = {}): Promise<void> {
  await page.addInitScript(
    ({ defaults, overrides, sequenceOverrides }) => {
      const mocks = { ...defaults, ...overrides }
      const sequences: Record<string, unknown[]> = { ...sequenceOverrides }
      const calls: Record<string, Record<string, unknown>[]> = {}

      // Mock __TAURI_INTERNALS__ which is used by @tauri-apps/api/core's invoke()
      const tauriInternals = {
        invoke: (cmd: string, args?: Record<string, unknown>) => {
          calls[cmd] = [...(calls[cmd] ?? []), args ?? {}]
          if (cmd in sequences) {
            const sequence = sequences[cmd]
            const value = sequence.length > 1 ? sequence.shift() : sequence[0]
            return Promise.resolve(value)
          }
          if (cmd in mocks) {
            return Promise.resolve(mocks[cmd])
          }
          console.warn(`[Tauri Mock] Unhandled command: ${cmd}`)
          return Promise.resolve(null)
        },
        transformCallback: (_cb?: (...args: unknown[]) => unknown) => {
          return `callback_${Date.now()}`
        },
        plugins: {
          dialog: {
            open: () => Promise.resolve(null),
            save: () => Promise.resolve(null),
          },
          opener: {},
          globalShortcut: {},
        },
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { windowLabel: "main", label: "main" },
        },
        convertFileSrc: (filePath: string, protocol = "asset") =>
          `${protocol}://localhost/${filePath}`,
      }

      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        value: tauriInternals,
        configurable: true,
        writable: true,
      })
      Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
        value: tauriInternals,
        configurable: true,
        writable: true,
      })
      Object.defineProperty(globalThis, "__TAURI_MOCK_CALLS__", {
        value: calls,
        configurable: true,
        writable: true,
      })

      // Mock @tauri-apps/plugin-dialog open/save
      // These are loaded as separate modules, so we need to intercept at a lower level
      // The __TAURI_EVENT_PLUGIN_INTERNALS__ pattern handles event listeners
      Object.defineProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__", {
        value: {
          registerListener: () => Promise.resolve(() => {}),
        },
        configurable: true,
        writable: true,
      })
    },
    {
      defaults: MOCK_RESPONSES,
      overrides: options.responses ?? {},
      sequenceOverrides: options.sequences ?? {},
    },
  )
}
