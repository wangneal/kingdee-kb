// 部分字段当前未被全部命令使用，保留以备未来扩展
#![allow(dead_code)]
//! 应用状态管理
//!
//! 在 Arc<Mutex<>> 中持有所有服务，以便从 Tauri 命令中线程安全地访问。
//!
//! 所有命令通过 `State<'_, AppState>` 访问，拆分为子状态会增加大量胶水代码。

use crate::services::analysis_cache::AnalysisCacheStore;
use crate::services::audio_capture::AudioCapture;
use crate::services::bm25_service::BM25Service;
use crate::services::desensitize::{Desensitizer, SensitiveKeywordStore};
use crate::services::embedding::{EmbeddingService, ModelManager};
use crate::services::image_processor::ImageProcessor;
use crate::services::ingest_cache::IngestCacheStore;
use crate::services::ingestion_queue::IngestionQueue;
use crate::services::knowledge_graph::GraphStore;
use crate::services::llm_providers::LLMProviderManager;
use crate::services::llm_service::LLMService;
use crate::services::meeting_store::MeetingStore;
use crate::services::metadata::MetadataStore;
use crate::services::outline::OutlineStore;
use crate::services::product_store::ProductStore;
use crate::services::project_store::ProjectStore;
use crate::services::question_tool::{self, PendingQuestions};
use crate::services::raw_source::RawSourceStore;
use crate::services::rerank::RerankerService;
use crate::services::research_session::ResearchSessionStore;
use crate::services::rig_agent::RigAgent;
use crate::services::risk_control::RiskControlStore;
use crate::services::signal_writer::SignalWriter;
use crate::services::skill_manager::SkillManager;
use crate::services::template_manager::TemplateManager;
use crate::services::vector_index::VectorIndex;
use crate::services::whisper_service::WhisperService;
use crate::services::wiki_page::WikiPageStore;
use crate::AsrConfigStore;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KbRecompileFailure {
    pub source_id: i64,
    pub title: String,
    pub error: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KbRecompileStatus {
    pub status: String,
    pub project_id: Option<i64>,
    pub force: bool,
    pub retried: usize,
    pub succeeded: usize,
    pub failed: Vec<KbRecompileFailure>,
    pub completed_source_keys: Vec<String>,
    pub message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl Default for KbRecompileStatus {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            project_id: None,
            force: false,
            retried: 0,
            succeeded: 0,
            failed: Vec::new(),
            completed_source_keys: Vec::new(),
            message: None,
            started_at: None,
            finished_at: None,
        }
    }
}

/// 所有 Tauri 命令共享的全局应用状态
pub struct AppState {
    /// 应用数据目录（~/.kingdee-kb/）
    pub data_dir: PathBuf,
    /// 嵌入模型管理器（下载、初始化、状态）
    pub model_manager: Arc<RwLock<ModelManager>>,
    /// 文本 → 向量嵌入服务
    pub embedding: Arc<RwLock<EmbeddingService>>,
    /// 用于相似性搜索的 HNSW 向量索引
    pub vector_index: Arc<RwLock<VectorIndex>>,
    /// 用于分块↔向量映射的 SQLite 元数据存储
    pub metadata: Arc<Mutex<MetadataStore>>,
    /// 统一项目管理存储（projects / project_phases）
    pub project_store: Arc<Mutex<ProjectStore>>,
    /// BM25 全文搜索服务（tantivy + jieba）
    pub bm25: Arc<RwLock<BM25Service>>,
    /// 用于 RAG 查询的 LLM 服务（OpenAI 兼容 API）
    pub llm: LLMService,
    /// 用于生成文档管理的产品存储
    pub products: Arc<Mutex<ProductStore>>,
    /// 原始导入文件管理（raw_sources 表）
    pub raw_sources: Arc<Mutex<RawSourceStore>>,
    /// 维基页面管理（wiki_pages 表）
    pub wiki_pages: Arc<Mutex<WikiPageStore>>,
    /// 知识图谱存储（knowledge_graph 表）
    pub graph_store: Arc<Mutex<GraphStore>>,
    /// 分析缓存管理（analysis_cache 表）
    pub analysis_cache: Arc<Mutex<AnalysisCacheStore>>,
    /// 摄入缓存管理（ingest_cache 表）
    pub ingest_cache_store: Arc<Mutex<IngestCacheStore>>,
    /// 嵌入模型的下载进度（0–100）。由后台线程更新。
    pub download_progress: Arc<AtomicU32>,
    /// 研究会话存储
    pub research_session_store: ResearchSessionStore,
    /// 研究大纲节点存储
    pub outline_store: Arc<Mutex<OutlineStore>>,
    /// 风险控制存储（需求蔓延警报/爆雷预警/话术库）
    pub risk_control_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
    /// 数据脱敏器（本地敏感信息过滤）
    pub desensitizer: Arc<Desensitizer>,
    /// 敏感词持久化存储（sensitive_keywords 表）
    pub sensitive_keyword_store: Arc<Mutex<SensitiveKeywordStore>>,
    /// Rig Agent（新推理引擎 — 基于 rig 的原生 function calling）
    pub rig_agent: RigAgent,
    /// 问题工具的待处理问题（跨进程状态）
    pub pending_questions: PendingQuestions,
    /// Whisper 语音转录服务（延迟加载模型）
    pub whisper_service: Arc<RwLock<WhisperService>>,
    /// 音频捕获（麦克风录音）
    pub audio_capture: Arc<RwLock<AudioCapture>>,
    /// 在线 ASR 配置（腾讯/讯飞）
    pub asr_config: Arc<RwLock<AsrConfigStore>>,
    /// 技能管理器（SKILL.md 加载/搜索/匹配）
    pub skill_manager: Arc<tokio::sync::Mutex<SkillManager>>,
    /// 信号写入器（技能系统事件记录）
    pub signal_writer: Arc<RwLock<SignalWriter>>,
    /// 模板管理器（Gitee 模板下载和缓存）
    pub template_manager: Arc<Mutex<TemplateManager>>,
    /// 图像处理器（OCR + 多模态 LLM）
    pub image_processor: Arc<RwLock<ImageProcessor>>,
    /// LLM 供应商管理器
    pub llm_providers: Arc<RwLock<LLMProviderManager>>,
    /// 持久化摄入队列（JSON 文件 + 崩溃恢复）
    pub ingest_queue: Mutex<IngestionQueue>,
    /// 知识编译后台任务状态（跨页面恢复）
    pub kb_recompile_status: Arc<Mutex<KbRecompileStatus>>,
    /// Agent 会话取消标志（session_id → cancel flag）
    pub cancel_flags: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>,
    /// Cross-Encoder Reranker（精排服务，延迟懒加载）
    pub reranker: RwLock<Option<Arc<RerankerService>>>,
    /// 会议存储服务（meetings / meeting_transcripts / meeting_minutes）
    pub meeting_store: Arc<Mutex<MeetingStore>>,
}

impl AppState {
    /// 使用给定的数据目录（~/.kingdee-kb/）初始化所有服务
    pub fn new(data_dir: &std::path::Path, skill_manager: SkillManager) -> Result<Self, String> {
        let model_dir = data_dir.join("models");
        let index_dir = data_dir.join("index");
        let db_path = data_dir.join("metadata.db");

        // 初始化 ModelManager（模型下载延迟）
        let model_manager = ModelManager::new(model_dir);

        // 初始化 EmbeddingService（空 - 模型尚未加载）
        let embedding = EmbeddingService::empty();

        // 初始化 VectorIndex（创建或从磁盘加载）
        let index_path = index_dir.join("vectors.usearch");
        let vector_index = if index_path.exists() {
            VectorIndex::load(index_path)
                .unwrap_or_else(|_| VectorIndex::new(index_dir).expect("Failed to create index"))
        } else {
            VectorIndex::new(index_dir)?
        };

        // 初始化 ProjectStore（共享 metadata.db）
        let project_store = {
            let store = ProjectStore::new(&db_path)?;
            store.ensure_default_project()?;
            Arc::new(Mutex::new(store))
        };

        // 初始化 MetadataStore（依赖 projects 表提供 project_id 外键）
        let metadata = MetadataStore::new(db_path.clone())?;

        // 初始化 BM25Service（懒加载——首次搜索时初始化）
        let bm25_index_dir = data_dir.join("bm25_index");
        let bm25 = BM25Service::empty(bm25_index_dir);

        // 初始化 ProductStore（共享 metadata.db）
        let products = ProductStore::new(db_path.clone())?;

        // 初始化 RawSourceStore（共享 metadata.db）
        let raw_source_conn =
            Connection::open(&db_path).map_err(|e| format!("打开原始文件数据库失败: {}", e))?;
        let raw_source_store = RawSourceStore::new(raw_source_conn);
        raw_source_store.ensure_table()?;
        let raw_sources = Arc::new(Mutex::new(raw_source_store));

        // 初始化 WikiPageStore（共享 metadata.db）
        let wiki_pages = {
            let conn =
                Connection::open(&db_path).map_err(|e| format!("打开维基页面数据库失败: {}", e))?;
            let store = WikiPageStore::new(conn);
            store.ensure_table()?;
            Arc::new(Mutex::new(store))
        };

        // 初始化 GraphStore（共享 metadata.db）
        let graph_store = {
            let conn =
                Connection::open(&db_path).map_err(|e| format!("打开知识图谱数据库失败: {}", e))?;
            let store = GraphStore::new(conn);
            store.ensure_table()?;
            Arc::new(Mutex::new(store))
        };

        // 初始化 AnalysisCacheStore（共享 metadata.db）
        let analysis_cache = {
            let conn =
                Connection::open(&db_path).map_err(|e| format!("打开分析缓存数据库失败: {}", e))?;
            let store = AnalysisCacheStore::new(conn);
            store.ensure_table()?;
            Arc::new(Mutex::new(store))
        };

        // 初始化 IngestCacheStore（共享 metadata.db）
        let ingest_cache_store = {
            let conn =
                Connection::open(&db_path).map_err(|e| format!("打开摄入缓存数据库失败: {}", e))?;
            let store = IngestCacheStore::new(conn);
            store.ensure_table()?;
            Arc::new(Mutex::new(store))
        };

        // 初始化 ResearchSessionStore（共享 metadata.db）
        let research_session_store = ResearchSessionStore::new(&db_path)?;

        // 初始化 OutlineStore（共享 metadata.db）
        let outline_store = {
            let store = OutlineStore::new(&db_path)?;
            Arc::new(Mutex::new(store))
        };

        // 初始化 RiskControlStore（共享 metadata.db）
        let risk_control_store =
            Arc::new(tokio::sync::Mutex::new(RiskControlStore::new(&db_path)?));

        let desensitizer = Arc::new(Desensitizer::new());

        // 敏感词持久化存储：读取已有敏感词加载到内存脱敏器
        let sensitive_keyword_store = {
            let store = SensitiveKeywordStore::new(&db_path)?;
            match store.list() {
                Ok(keywords) => {
                    if !keywords.is_empty() {
                        desensitizer.add_typed_keywords(&keywords);
                        tracing::info!("已加载 {} 个持久化敏感词", keywords.len());
                    }
                }
                Err(e) => tracing::warn!("加载持久化敏感词失败: {}", e),
            }
            Arc::new(Mutex::new(store))
        };

        let pending_questions = question_tool::create_pending_questions();

        let whisper_service = WhisperService::new();
        let audio_capture = AudioCapture::new(data_dir);
        let asr_config = AsrConfigStore::new(&db_path);

        // 初始化 SignalWriter（技能系统事件记录）
        let signals_path = data_dir.join("signals.jsonl");
        let signal_writer = SignalWriter::new(signals_path)
            .map_err(|e| format!("Failed to create SignalWriter: {}", e))?;

        // 初始化 TemplateManager（模板下载和缓存）
        let template_cache_dir = data_dir.join("templates");
        let template_manager = TemplateManager::new(template_cache_dir, String::new());

        // 初始化 LLM 供应商管理器
        let llm_providers = Arc::new(RwLock::new(LLMProviderManager::new(
            &data_dir.to_path_buf(),
        )));
        // ImageProcessor 必须先于 seed 任务创建（Arc 包装），便于 seed 完成后回填
        // 修复前：ImageProcessor 在 seed 之前用空配置快照，seed 完成后无法感知，永远空配置
        // 修复后：先以空配置创建，seed 完成后由 seed 任务调 update_llm_config 回填
        let image_processor = Arc::new(RwLock::new(ImageProcessor::new(
            String::new(),
            String::new(),
            String::new(),
        )));
        // 已配置用户：llm_providers.load() 已加载文件，立即从内存回填到 ImageProcessor
        {
            if let Ok(mgr) = llm_providers.read() {
                if let Some(provider) = mgr.get_default_provider() {
                    if let Ok(mut proc) = image_processor.write() {
                        proc.update_llm_config(
                            provider.get_default_key_value(),
                            provider.base_url.clone(),
                            provider.get_default_model_name(),
                        );
                    }
                }
            }
        }
        // 首次启动时异步 seed OpenCode Zen 默认供应商
        // 分离到独立方法是为了让 LLMProviderManager::new() 在无 tokio runtime 的测试环境也能工作
        //
        // 必须用 tauri::async_runtime::spawn 而非 tokio::spawn：
        // AppState::new 由 Tauri 的 setup 同步钩子调用，
        // 此时 tokio 运行时尚未初始化，tokio::spawn 会直接 panic
        // （"there is no reactor running"）。Tauri 自带 runtime，
        // 在事件循环启动后由框架统一管理，可靠且不依赖全局 runtime 状态。
        {
            let seed_arc = llm_providers.clone();
            let img_arc = image_processor.clone();
            tauri::async_runtime::spawn(async move {
                LLMProviderManager::seed_default_async(&seed_arc).await;
                // seed 完成后回填 ImageProcessor：解决"首启时 ImageProcessor 永远拿不到默认配置"
                if let Ok(mgr) = seed_arc.read() {
                    if let Some(provider) = mgr.get_default_provider() {
                        if let Ok(mut proc) = img_arc.write() {
                            proc.update_llm_config(
                                provider.get_default_key_value(),
                                provider.base_url.clone(),
                                provider.get_default_model_name(),
                            );
                        }
                    }
                }
            });
        }

        // 初始化 LLM 服务（从 LLMProviderManager 获取配置并传入数据脱敏器）
        let llm = LLMService::with_desensitizer(llm_providers.clone(), desensitizer.clone());

        // 初始化持久化摄入队列
        let ingest_queue = Mutex::new(IngestionQueue::new(data_dir));

        // 初始化 MeetingStore（共享 metadata.db）
        let meeting_store = {
            let store = MeetingStore::new(&db_path)?;
            store.ensure_table()?;
            Arc::new(Mutex::new(store))
        };

        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            model_manager: Arc::new(RwLock::new(model_manager)),
            embedding: Arc::new(RwLock::new(embedding)),
            vector_index: Arc::new(RwLock::new(vector_index)),
            metadata: Arc::new(Mutex::new(metadata)),
            project_store,
            bm25: Arc::new(RwLock::new(bm25)),
            llm,
            products: Arc::new(Mutex::new(products)),
            raw_sources,
            wiki_pages,
            graph_store,
            analysis_cache,
            ingest_cache_store,
            download_progress: Arc::new(AtomicU32::new(0)),
            research_session_store,
            outline_store,
            risk_control_store,
            desensitizer,
            sensitive_keyword_store,
            whisper_service: Arc::new(RwLock::new(whisper_service)),
            audio_capture: Arc::new(RwLock::new(audio_capture)),
            asr_config: Arc::new(RwLock::new(asr_config)),
            rig_agent: RigAgent,
            pending_questions,
            skill_manager: Arc::new(tokio::sync::Mutex::new(skill_manager)),
            signal_writer: Arc::new(RwLock::new(signal_writer)),
            template_manager: Arc::new(Mutex::new(template_manager)),
            image_processor,
            llm_providers,
            ingest_queue,
            kb_recompile_status: Arc::new(Mutex::new(KbRecompileStatus::default())),
            cancel_flags: Arc::new(RwLock::new(HashMap::new())),
            reranker: RwLock::new(None),
            meeting_store,
        })
    }

    /// 当完整初始化失败时创建最小 AppState。
    ///
    /// 尝试单独初始化每个服务。如果服务失败，
    /// 使用内存存根以便应用仍可启动（依赖该服务的命令在运行时将返回错误）。
    pub fn minimal(data_dir: &std::path::Path) -> Self {
        let db_path = data_dir.join("metadata.db");

        let project_store = {
            let store = ProjectStore::new(&db_path).expect("Fatal: cannot create project store");
            store
                .ensure_default_project()
                .expect("Fatal: cannot create default project");
            Arc::new(Mutex::new(store))
        };

        let metadata = MetadataStore::new(db_path.clone())
            .expect("Fatal: cannot create metadata DB — app cannot function without it");

        let vector_index = VectorIndex::new(data_dir.join("index"))
            .expect("Fatal: cannot create vector index — app cannot function without it");

        let bm25 = BM25Service::empty(data_dir.join("bm25_index"));

        let products = ProductStore::new(db_path.clone())
            .expect("Fatal: cannot create product store — app cannot function without it");

        // ResearchSessionStore / RiskControlStore / OutlineStore 都是非核心辅助服务：
        // 磁盘库创建失败时退化为内存库，保证主流程不中断；内存库数据不持久化。
        let research_session_store = match ResearchSessionStore::new(&db_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("ResearchSessionStore 初始化失败（非致命，降级到内存库）: {}", e);
                // 降级到内存库：当前会话可用，重启后数据丢失（仅辅助服务）
                ResearchSessionStore::new_in_memory()
                    .expect("Fatal: cannot create in-memory ResearchSessionStore")
            }
        };

        let outline_store = Arc::new(Mutex::new(match OutlineStore::new(&db_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("OutlineStore 初始化失败（非致命，降级到内存库）: {}", e);
                OutlineStore::new_in_memory().expect("Fatal: cannot create in-memory OutlineStore")
            }
        }));

        let risk_control_store = Arc::new(tokio::sync::Mutex::new(
            match RiskControlStore::new(&db_path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("RiskControlStore 初始化失败（非致命，降级到内存库）: {}", e);
                    RiskControlStore::new_in_memory()
                        .expect("Fatal: cannot create in-memory RiskControlStore")
                }
            },
        ));

        let desensitizer = Arc::new(Desensitizer::new());

        // 敏感词持久化：minimal 模式下尝试加载已有敏感词，DB 不可用时用内存库保证可启动
        let sensitive_keyword_store = match SensitiveKeywordStore::new(&db_path) {
            Ok(store) => {
                if let Ok(keywords) = store.list() {
                    desensitizer.add_typed_keywords(&keywords);
                }
                Arc::new(Mutex::new(store))
            }
            Err(e) => {
                tracing::error!("minimal 模式敏感词存储初始化失败: {}", e);
                Arc::new(Mutex::new(
                    SensitiveKeywordStore::with_conn(
                        Connection::open_in_memory().expect("Fatal: 无法创建敏感词内存库"),
                    )
                    .expect("Fatal: 无法初始化敏感词内存库"),
                    ))
                }
            };

        let pending_questions = question_tool::create_pending_questions();

        let whisper_service = WhisperService::new();
        let audio_capture = AudioCapture::new(data_dir);

        let llm_providers = Arc::new(RwLock::new(LLMProviderManager::new(
            &data_dir.to_path_buf(),
        )));
        let llm = LLMService::with_desensitizer(llm_providers.clone(), desensitizer.clone());

        let ingest_queue = Mutex::new(IngestionQueue::new(data_dir));

        Self {
            data_dir: data_dir.to_path_buf(),
            model_manager: Arc::new(RwLock::new(ModelManager::new(data_dir.join("models")))),
            embedding: Arc::new(RwLock::new(EmbeddingService::empty())),
            vector_index: Arc::new(RwLock::new(vector_index)),
            metadata: Arc::new(Mutex::new(metadata)),
            project_store,
            bm25: Arc::new(RwLock::new(bm25)),
            llm,
            products: Arc::new(Mutex::new(products)),
            raw_sources: Arc::new(Mutex::new({
                let conn =
                    Connection::open(&db_path).expect("Fatal: cannot open DB for RawSourceStore");
                let store = RawSourceStore::new(conn);
                store
                    .ensure_table()
                    .expect("Fatal: cannot init raw_sources table");
                store
            })),
            wiki_pages: Arc::new(Mutex::new({
                let conn =
                    Connection::open(&db_path).expect("Fatal: cannot open DB for WikiPageStore");
                let store = WikiPageStore::new(conn);
                store
                    .ensure_table()
                    .expect("Fatal: cannot init wiki_pages table");
                store
            })),
            graph_store: Arc::new(Mutex::new({
                let conn =
                    Connection::open(&db_path).expect("Fatal: cannot open DB for GraphStore");
                let store = GraphStore::new(conn);
                store
                    .ensure_table()
                    .expect("Fatal: cannot init knowledge_graph table");
                store
            })),
            analysis_cache: Arc::new(Mutex::new({
                let conn = Connection::open(&db_path)
                    .expect("Fatal: cannot open DB for AnalysisCacheStore");
                let store = AnalysisCacheStore::new(conn);
                store
                    .ensure_table()
                    .expect("Fatal: cannot init analysis_cache table");
                store
            })),
            ingest_cache_store: Arc::new(Mutex::new({
                let conn =
                    Connection::open(&db_path).expect("Fatal: cannot open DB for IngestCacheStore");
                let store = IngestCacheStore::new(conn);
                store
                    .ensure_table()
                    .expect("Fatal: cannot init ingest_cache table");
                store
            })),
            download_progress: Arc::new(AtomicU32::new(0)),
            research_session_store,
            outline_store,
            risk_control_store,
            desensitizer,
            sensitive_keyword_store,
            rig_agent: RigAgent,
            pending_questions,
            whisper_service: Arc::new(RwLock::new(whisper_service)),
            audio_capture: Arc::new(RwLock::new(audio_capture)),
            asr_config: Arc::new(RwLock::new(AsrConfigStore::new(&db_path))),
            skill_manager: Arc::new(tokio::sync::Mutex::new(SkillManager::new(
                data_dir.join("skills"),
            ))),
            signal_writer: Arc::new(RwLock::new(
                SignalWriter::new(data_dir.join("signals.jsonl")).unwrap_or_else(|_| {
                    // 降级到临时目录
                    let temp = std::env::temp_dir().join("kingdee-kb-signals.jsonl");
                    SignalWriter::new(temp).expect("Failed to create fallback SignalWriter")
                }),
            )),
            template_manager: Arc::new(Mutex::new(TemplateManager::new(
                data_dir.join("templates"),
                String::new(),
            ))),
            image_processor: Arc::new(RwLock::new(ImageProcessor::new(
                String::new(),
                String::new(),
                String::new(),
            ))),
            llm_providers,
            ingest_queue,
            kb_recompile_status: Arc::new(Mutex::new(KbRecompileStatus::default())),
            cancel_flags: Arc::new(RwLock::new(HashMap::new())),
            reranker: RwLock::new(None),
            meeting_store: Arc::new(Mutex::new(
                MeetingStore::new(&db_path).expect("Fatal: cannot init MeetingStore"),
            )),
        }
    }
}

impl AppState {
    /// 获取或异步懒加载 Reranker 服务
    pub fn get_or_init_reranker(&self) -> Option<Arc<RerankerService>> {
        {
            let read = self.reranker.read().ok()?;
            if let Some(ref r) = *read {
                return Some(r.clone());
            }
        }

        let mut write = self.reranker.write().ok()?;
        if write.is_none() {
            tracing::info!("开始后台懒加载 Reranker 模型 (BAAI/bge-reranker-v2-m3)");
            match RerankerService::try_new(10) {
                Ok(r) => {
                    *write = Some(Arc::new(r));
                    tracing::info!("Reranker 模型加载成功");
                }
                Err(e) => {
                    tracing::error!("Reranker 模型懒加载失败: {}", e);
                }
            }
        }
        write.clone()
    }

    /// 确保 BM25 全文搜索服务已初始化（幂等安全）
    /// 类似于 ensure_embedding_ready()，用于调用 get_or_init_bm25 的公共入口。
    pub fn ensure_bm25_ready(&self) {
        if let Err(e) = self.get_or_init_bm25() {
            tracing::error!("BM25 初始化失败: {}", e);
        }
    }

    /// 确保 BM25 全文搜索服务已初始化（幂等安全）
    pub fn get_or_init_bm25(&self) -> Result<(), String> {
        {
            let read = self.bm25.read().map_err(|e| e.to_string())?;
            if read.is_ready() {
                return Ok(());
            }
        }
        let mut write = self.bm25.write().map_err(|e| e.to_string())?;
        write.ensure_initialized()
    }

    /// 注册一个取消标志，返回共享的 AtomicBool 传入 agent 循环。
    pub fn register_cancel_flag(&self, session_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut flags) = self.cancel_flags.write() {
            flags.insert(session_id.to_string(), flag.clone());
        }
        flag
    }

    /// 取消指定会话的 agent 流。
    pub fn cancel_agent_session(&self, session_id: &str) {
        if let Ok(flags) = self.cancel_flags.read() {
            if let Some(flag) = flags.get(session_id) {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    /// 移除已完成会话的取消标志（防止内存泄漏）。
    pub fn remove_cancel_flag(&self, session_id: &str) {
        if let Ok(mut flags) = self.cancel_flags.write() {
            flags.remove(session_id);
        }
    }

    /// 确保 embedding 模型已加载（懒加载）。
    /// 合并自 ingestion.rs 和 search_llm.rs 的重复实现。
    pub fn ensure_embedding_ready(&self) {
        let emb = match self.embedding.read() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("[LazyLoad] Embedding 锁中毒: {}", e);
                return;
            }
        };
        if emb.is_ready() {
            return;
        }
        drop(emb);

        let mut mm = match self.model_manager.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        if !mm.is_ready() {
            if let Err(e) = mm.init() {
                tracing::error!("[LazyLoad] 模型初始化失败: {}", e);
                return;
            }
        }
        if let Some(model) = mm.take_model() {
            let mut emb = match self.embedding.write() {
                Ok(g) => g,
                Err(e) => {
                    tracing::error!("[LazyLoad] Embedding 锁中毒: {}", e);
                    return;
                }
            };
            emb.set_model(model);
        }
    }

    /// 检查并释放空闲超时的本地 Embedding 模型。
    ///
    /// 如果本地模型空闲时间超过 `timeout_secs` 秒，则释放模型内存。
    /// 下次使用时 `ensure_embedding_ready()` 会从磁盘缓存重新加载。
    /// 返回 true 表示已释放，false 表示未释放。
    pub fn unload_idle_embedding(&self, timeout_secs: u64) -> bool {
        // 检查是否有本地模型且已空闲超时
        let should_unload = match self.embedding.read() {
            Ok(emb) => emb.has_local_model() && emb.idle_seconds() >= timeout_secs,
            Err(_) => return false,
        };

        if !should_unload {
            return false;
        }

        // 释放 EmbeddingService 中的模型
        match self.embedding.write() {
            Ok(mut emb) => {
                // 二次检查（避免在获取写锁期间被其他线程更新）
                if !emb.has_local_model() || emb.idle_seconds() < timeout_secs {
                    return false;
                }
                emb.unload();
            }
            Err(_) => return false,
        }

        // 重置 ModelManager 以便下次懒加载
        match self.model_manager.write() {
            Ok(mut mm) => mm.reset_for_reload(),
            Err(_) => {}
        }

        true
    }
}
