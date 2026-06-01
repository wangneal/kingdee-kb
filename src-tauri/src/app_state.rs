// 迁移期间部分字段暂未使用
#![allow(dead_code)]
//! 应用状态管理
//!
//! 在 Arc<Mutex<>> 中持有所有服务，以便从 Tauri 命令中线程安全地访问。
//!
//! 注意：AppState 字段较多（22个）是 Tauri 框架的典型模式。
//! 所有命令通过 `State<'_, AppState>` 访问，拆分为子状态会增加大量胶水代码。

use crate::services::audio_capture::AudioCapture;
use crate::services::bm25_service::BM25Service;
use crate::services::desensitize::Desensitizer;
use crate::services::edition_config::EditionConfig;
use crate::services::embedding::{EmbeddingService, ModelManager};
use crate::services::image_processor::ImageProcessor;
use crate::services::llm_providers::LLMProviderManager;
use crate::services::llm_service::LLMService;
use crate::services::metadata::MetadataStore;
use crate::services::product_store::ProductStore;
use crate::services::question_tool::{self, PendingQuestions};
use crate::services::research_indexer::ResearchIndexer;
use crate::services::research_session::ResearchSessionStore;
use crate::services::rig_agent::RigAgent;
use crate::services::risk_control::RiskControlStore;
use crate::services::signal_writer::SignalWriter;
use crate::services::skill_manager::SkillManager;
use crate::services::template_manager::TemplateManager;
use crate::services::vector_index::VectorIndex;
use crate::services::whisper_service::WhisperService;
use crate::AsrConfigStore;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex};

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
    pub risk_control_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
    /// 数据脱敏器（本地敏感信息过滤）
    pub desensitizer: Arc<Desensitizer>,
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
    /// 技能管理器（SKILL.md 加载/搜索/匹配）
    pub skill_manager: Arc<Mutex<SkillManager>>,
    /// 信号写入器（技能系统事件记录）
    pub signal_writer: Arc<Mutex<SignalWriter>>,
    /// 模板管理器（Gitee 模板下载和缓存）
    pub template_manager: Arc<Mutex<TemplateManager>>,
    /// 图像处理器（OCR + 多模态 LLM）
    pub image_processor: Arc<Mutex<ImageProcessor>>,
    /// LLM 供应商管理器
    pub llm_providers: Arc<Mutex<LLMProviderManager>>,
    /// Agent 会话取消标志（session_id → cancel flag）
    pub cancel_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
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
        let risk_control_store =
            Arc::new(tokio::sync::Mutex::new(RiskControlStore::new(&db_path)?));

        let desensitizer = Arc::new(Desensitizer::new());

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
        let llm_providers = Arc::new(Mutex::new(LLMProviderManager::new(&data_dir.to_path_buf())));

        // 初始化 ImageProcessor（图像处理，从 LLMProviderManager 获取配置）
        let image_processor = {
            let mgr = llm_providers.lock().map_err(|e| e.to_string())?;
            let (api_key, base_url, model) = mgr
                .get_default_provider()
                .map(|p| {
                    (
                        p.get_default_key_value(),
                        p.base_url.clone(),
                        p.get_default_model_name(),
                    )
                })
                .unwrap_or_default();
            ImageProcessor::new(api_key, base_url, model)
        };

        // 初始化 LLM 服务（从 LLMProviderManager 获取配置并传入数据脱敏器）
        let llm = LLMService::with_desensitizer(llm_providers.clone(), desensitizer.clone());

        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            model_manager: Arc::new(Mutex::new(model_manager)),
            embedding: Arc::new(Mutex::new(embedding)),
            vector_index: Arc::new(Mutex::new(vector_index)),
            metadata: Arc::new(Mutex::new(metadata)),
            bm25: Arc::new(Mutex::new(bm25)),
            llm,
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
            rig_agent: RigAgent,
            pending_questions,
            skill_manager: Arc::new(Mutex::new(skill_manager)),
            signal_writer: Arc::new(Mutex::new(signal_writer)),
            template_manager: Arc::new(Mutex::new(template_manager)),
            image_processor: Arc::new(Mutex::new(image_processor)),
            llm_providers,
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
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
            let conn = Connection::open(&db_path).expect("Fatal: cannot open DB for EditionConfig");
            let config = EditionConfig::new(conn);
            config
                .init_table()
                .expect("Fatal: cannot init config table");
            config
        };

        let research_indexer = {
            let indexer =
                ResearchIndexer::new(&db_path).expect("Fatal: cannot create ResearchIndexer");
            indexer
                .init_tables()
                .expect("Fatal: cannot init research tables");
            indexer
        };

        // ResearchSessionStore 和 RiskControlStore 不是核心服务，失败时用内存兜底
        let research_session_store = match ResearchSessionStore::new(&db_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "WARNING: ResearchSessionStore init failed (non-fatal): {}",
                    e
                );
                // 用内存数据库兜底，避免 crash
                ResearchSessionStore::new_in_memory()
                    .expect("Fatal: cannot create in-memory ResearchSessionStore")
            }
        };

        let risk_control_store = Arc::new(tokio::sync::Mutex::new(
            match RiskControlStore::new(&db_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("WARNING: RiskControlStore init failed (non-fatal): {}", e);
                    RiskControlStore::new_in_memory()
                        .expect("Fatal: cannot create in-memory RiskControlStore")
                }
            },
        ));

        let desensitizer = Arc::new(Desensitizer::new());

        let pending_questions = question_tool::create_pending_questions();

        let whisper_service = WhisperService::new();
        let audio_capture = AudioCapture::new(data_dir);

        let llm_providers = Arc::new(Mutex::new(LLMProviderManager::new(&data_dir.to_path_buf())));
        let llm = LLMService::with_desensitizer(llm_providers.clone(), desensitizer.clone());

        Self {
            data_dir: data_dir.to_path_buf(),
            model_manager: Arc::new(Mutex::new(ModelManager::new(data_dir.join("models")))),
            embedding: Arc::new(Mutex::new(EmbeddingService::empty())),
            vector_index: Arc::new(Mutex::new(vector_index)),
            metadata: Arc::new(Mutex::new(metadata)),
            bm25: Arc::new(Mutex::new(bm25)),
            llm,
            products: Arc::new(Mutex::new(products)),
            download_progress: Arc::new(AtomicU32::new(0)),
            edition_config,
            research_indexer,
            research_session_store,
            risk_control_store,
            desensitizer,
            rig_agent: RigAgent,
            pending_questions,
            whisper_service: Arc::new(Mutex::new(whisper_service)),
            audio_capture: Arc::new(Mutex::new(audio_capture)),
            asr_config: Arc::new(Mutex::new(AsrConfigStore::new(&db_path))),
            skill_manager: Arc::new(Mutex::new(SkillManager::new(data_dir.join("skills")))),
            signal_writer: Arc::new(Mutex::new(
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
            image_processor: Arc::new(Mutex::new(ImageProcessor::new(
                String::new(),
                String::new(),
                String::new(),
            ))),
            llm_providers,
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl AppState {
    /// 注册一个取消标志，返回共享的 AtomicBool 传入 agent 循环。
    pub fn register_cancel_flag(&self, session_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut flags) = self.cancel_flags.lock() {
            flags.insert(session_id.to_string(), flag.clone());
        }
        flag
    }

    /// 取消指定会话的 agent 流。
    pub fn cancel_agent_session(&self, session_id: &str) {
        if let Ok(flags) = self.cancel_flags.lock() {
            if let Some(flag) = flags.get(session_id) {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    /// 移除已完成会话的取消标志（防止内存泄漏）。
    pub fn remove_cancel_flag(&self, session_id: &str) {
        if let Ok(mut flags) = self.cancel_flags.lock() {
            flags.remove(session_id);
        }
    }
}
