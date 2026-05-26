//! 应用状态管理
//!
//! 在 Arc<Mutex<>> 中持有所有服务，以便从 Tauri 命令中线程安全地访问。
//!
//! 注意：AppState 字段较多（22个）是 Tauri 框架的典型模式。
//! 所有命令通过 `State<'_, AppState>` 访问，拆分为子状态会增加大量胶水代码。

use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};
use crate::services::audio_capture::AudioCapture;
use crate::services::whisper_service::WhisperService;
use crate::AsrConfigStore;
use crate::services::desensitize::Desensitizer;
use crate::services::edition_config::EditionConfig;
use crate::services::embedding::{EmbeddingService, ModelManager};
use crate::services::llm_service::LLMService;
use crate::services::bm25_service::BM25Service;
use crate::services::metadata::MetadataStore;
use crate::services::product_store::ProductStore;
use crate::services::react_agent::ReActAgent;
use crate::services::rig_agent::RigAgent;
use crate::services::research_indexer::ResearchIndexer;
use crate::services::research_session::ResearchSessionStore;
use crate::services::risk_control::RiskControlStore;
use crate::services::tool_registry::{ToolRegistry, SearchKnowledgeTool, GenerateDocTool, CheckScopeCreepTool, AnalyzeFitGapTool, GetProjectHealthTool, GenerateDefenseScriptTool, ExtractBlueprintTool, RecommendQuestionsTool};
use crate::services::question_tool::{self, PendingQuestions};
use crate::services::vector_index::VectorIndex;
use std::path::PathBuf;
use rusqlite::Connection;

/// 创建一个配置了金蝶实施 AI 工具的 ToolRegistry。
///
/// 在 `AppState::new()` 和 `AppState::minimal()` 之间共享，避免代码重复。
fn create_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(SearchKnowledgeTool));
    registry.register(Box::new(GenerateDocTool));
    registry.register(Box::new(CheckScopeCreepTool));
    registry.register(Box::new(AnalyzeFitGapTool));
    registry.register(Box::new(GetProjectHealthTool));
    registry.register(Box::new(GenerateDefenseScriptTool));
    registry.register(Box::new(ExtractBlueprintTool));
    registry.register(Box::new(RecommendQuestionsTool));
    registry
}

/// 所有 Tauri 命令共享的全局应用状态
pub struct AppState {
    /// 应用数据目录（~/.kingdee-kb/）
    pub data_dir: PathBuf,
    /// 嵌入模型管理器（下载、初始化、状态）
    pub model_manager: Arc<Mutex<ModelManager>>,
    /// 文本 → 向量嵌入服务
    pub embedding: Arc<Mutex<EmbeddingService>>,
    /// 用于相似性搜索的 HNSW 向量索引
    pub vector_index: Arc<Mutex<VectorIndex>>,
    /// 用于分块↔向量映射的 SQLite 元数据存储
    pub metadata: Arc<Mutex<MetadataStore>>,
    /// BM25 全文搜索服务（tantivy + jieba）
    pub bm25: Arc<Mutex<BM25Service>>,
    /// 用于 RAG 查询的 LLM 服务（OpenAI 兼容 API）
    pub llm: LLMService,
    /// 用于生成文档管理的产品存储
    pub products: Arc<Mutex<ProductStore>>,
    /// 嵌入模型的下载进度（0–100）。由后台线程更新。
    pub download_progress: Arc<AtomicU32>,
    /// 版本配置（企业版 / 旗舰版）
    pub edition_config: EditionConfig,
    /// 研究大纲索引器
    pub research_indexer: ResearchIndexer,
    /// 研究会话存储
    pub research_session_store: ResearchSessionStore,
    /// 风险控制存储（需求蔓延警报/爆雷预警/话术库）
    pub risk_control_store: RiskControlStore,
    /// 数据脱敏器（本地敏感信息过滤）
    pub desensitizer: Desensitizer,
    /// ReAct Agent（推理引擎 — 旧实现，保留兼容）
    #[allow(dead_code)]
    pub react_agent: ReActAgent,
    /// Rig Agent（新推理引擎 — 基于 rig 的原生 function calling）
    pub rig_agent: RigAgent,
    /// 问题工具的待处理问题（跨进程状态）
    pub pending_questions: PendingQuestions,
    /// Whisper 语音转录服务（延迟加载模型）
    pub whisper_service: Arc<Mutex<WhisperService>>,
    /// 音频捕获（麦克风录音）
    pub audio_capture: Arc<Mutex<AudioCapture>>,
    /// 在线 ASR 配置（腾讯/讯飞）
    pub asr_config: Arc<Mutex<AsrConfigStore>>,
}

impl AppState {
    /// 使用给定的数据目录（~/.kingdee-kb/）初始化所有服务
    pub fn new(data_dir: &std::path::Path) -> Result<Self, String> {
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

        // 初始化 MetadataStore（如果不存在则创建）
        let metadata = MetadataStore::new(db_path.clone())?;

        // 初始化 BM25Service（tantivy + jieba 全文索引）
        let bm25_index_dir = data_dir.join("bm25_index");
        let bm25 = BM25Service::new(bm25_index_dir)?;

        // 初始化 ProductStore（如果不存在则创建）
        let products_db_path = data_dir.join("products.db");
        let products = ProductStore::new(products_db_path)?;

        // 初始化 EditionConfig（共享 metadata.db 的 app_config 表）
        let edition_config = {
            let conn = Connection::open(&db_path)
                .map_err(|e| format!("Failed to open DB for EditionConfig: {}", e))?;
            let config = EditionConfig::new(conn);
            config.init_table()?;
            config
        };

        // 初始化 ResearchIndexer
        let research_indexer = {
            let indexer = ResearchIndexer::new(&db_path)?;
            indexer.init_tables()?;
            indexer
        };

        // 初始化 ResearchSessionStore（共享 metadata.db）
        let research_session_store = ResearchSessionStore::new(&db_path)?;

        // 初始化 RiskControlStore（共享 metadata.db）
        let risk_control_store = RiskControlStore::new(&db_path)?;

        let desensitizer = Desensitizer::new();

        // 初始化 ReAct Agent
        let tool_registry = Arc::new(create_tool_registry());
        let react_agent = ReActAgent::new(tool_registry);
        let pending_questions = question_tool::create_pending_questions();

        let whisper_service = WhisperService::new();
        let audio_capture = AudioCapture::new(data_dir);
        let asr_config = AsrConfigStore::new(&db_path);

        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            model_manager: Arc::new(Mutex::new(model_manager)),
            embedding: Arc::new(Mutex::new(embedding)),
            vector_index: Arc::new(Mutex::new(vector_index)),
            metadata: Arc::new(Mutex::new(metadata)),
            bm25: Arc::new(Mutex::new(bm25)),
            llm: LLMService::new(data_dir),
            products: Arc::new(Mutex::new(products)),
            download_progress: Arc::new(AtomicU32::new(0)),
            edition_config,
            research_indexer,
            research_session_store,
            risk_control_store,
            desensitizer,
            whisper_service: Arc::new(Mutex::new(whisper_service)),
            audio_capture: Arc::new(Mutex::new(audio_capture)),
            asr_config: Arc::new(Mutex::new(asr_config)),
            react_agent,
            rig_agent: RigAgent,
            pending_questions,
        })
    }

    /// 当完整初始化失败时创建最小 AppState。
    ///
    /// 尝试单独初始化每个服务。如果服务失败，
    /// 使用内存存根以便应用仍可启动（依赖该服务的命令在运行时将返回错误）。
    pub fn minimal(data_dir: &std::path::Path) -> Self {
        let metadata = MetadataStore::new(data_dir.join("metadata.db"))
            .expect("Fatal: cannot create metadata DB — app cannot function without it");

        let vector_index = VectorIndex::new(data_dir.join("index"))
            .expect("Fatal: cannot create vector index — app cannot function without it");

        let bm25 = BM25Service::new(data_dir.join("bm25_index"))
            .expect("Fatal: cannot create BM25 index — app cannot function without it");

        let products = ProductStore::new(data_dir.join("products.db"))
            .expect("Fatal: cannot create product store — app cannot function without it");

        let db_path = data_dir.join("metadata.db");

        let edition_config = {
            let conn = Connection::open(&db_path)
                .expect("Fatal: cannot open DB for EditionConfig");
            let config = EditionConfig::new(conn);
            config.init_table().expect("Fatal: cannot init config table");
            config
        };

        let research_indexer = {
            let indexer = ResearchIndexer::new(&db_path)
                .expect("Fatal: cannot create ResearchIndexer");
            indexer.init_tables().expect("Fatal: cannot init research tables");
            indexer
        };

        let research_session_store = ResearchSessionStore::new(&db_path)
            .expect("Fatal: cannot create ResearchSessionStore");

        let risk_control_store = RiskControlStore::new(&db_path)
            .expect("Fatal: cannot create RiskControlStore");

        let desensitizer = Desensitizer::new();

        // 初始化 ReAct Agent（最小化）
        let tool_registry = Arc::new(create_tool_registry());
        let react_agent = ReActAgent::new(tool_registry);
        let pending_questions = question_tool::create_pending_questions();

        let whisper_service = WhisperService::new();
        let audio_capture = AudioCapture::new(data_dir);

        Self {
            data_dir: data_dir.to_path_buf(),
            model_manager: Arc::new(Mutex::new(ModelManager::new(data_dir.join("models")))),
            embedding: Arc::new(Mutex::new(EmbeddingService::empty())),
            vector_index: Arc::new(Mutex::new(vector_index)),
            metadata: Arc::new(Mutex::new(metadata)),
            bm25: Arc::new(Mutex::new(bm25)),
            llm: LLMService::new(data_dir),
            products: Arc::new(Mutex::new(products)),
            download_progress: Arc::new(AtomicU32::new(0)),
            edition_config,
            research_indexer,
            research_session_store,
            risk_control_store,
            desensitizer,
            react_agent,
            rig_agent: RigAgent,
            pending_questions,
            whisper_service: Arc::new(Mutex::new(whisper_service)),
            audio_capture: Arc::new(Mutex::new(audio_capture)),
            asr_config: Arc::new(Mutex::new(AsrConfigStore::new(&db_path))),
        }
    }
}
