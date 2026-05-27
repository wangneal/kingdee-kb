# KingdeeKB 开发进度

## 当前目标
- 修复搜索质量问题：ingestion 流程中 BM25 索引遗漏 Bug → 混合搜索退化为纯向量搜索

## 已完成

### 1. lib.rs 重构（已提交 e5df2ab, dd52ae7）
- 2318 行 → ~170 行，拆分为 10 个命令模块
- 使用 Rust `mod` + `pub use`，非 `include!()`

### 2. 模板字段提取扩展（已提交）
- `template_docx.rs`：三种提取模式（brace/xxxx/sdt）+ `FieldInfo.source` + `extract_context()` UTF-8 安全
- `template_xlsx.rs`：XXXX + 花括号提取 + `source` 字段
- 模板分析结果：31/66 模板有 XXXX 占位符，0 个花括号；SDT/FormFields 非输入字段

### 3. BM25 索引遗漏修复 + 搜索质量优化（已提交 cb77cf4）
**根因**：`ingest_text`/`ingest_file`/`ingest_directory` 只调用 embedding + vector_index + metadata，完全未调用 `BM25Service.add_chunk()`，导致 BM25 索引永远为空，混合搜索退化为纯向量搜索。

**修改文件清单**：

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/services/ingestion.rs` | 4个函数签名增加 `bm25: &Arc<Mutex<BM25Service>>`；`ingest_text` 中收集 BM25 数据到 Vec → 释放 idx+meta lock → `add_chunks()` → `commit()`；修复 `ingest_dir_recursive` 中 `dir→dir_path` 变量名、`ingest_file` 调用缺少 bm25 参数、缩进不一致；添加 `is_temp_file()` 过滤 `~$` 前缀文件和 Thumbs.db |
| `src-tauri/src/commands/ingestion.rs` | 3个 Tauri command 传递 `&state.bm25`；修复文件格式损坏 |
| `src-tauri/src/commands/media.rs` | 第195行 `ingest_text` 调用增加 `&state.bm25` |
| `src-tauri/src/services/memory.rs` | 增加 `BM25Service` import；`save_chat_memory` 签名增加 `bm25` 参数 |
| `src-tauri/src/commands/search_llm.rs` | `save_chat_memory` 调用增加 `&state.bm25`；清理 unused `MetadataStore` import |
| `src-tauri/src/services/bm25_service.rs` | 添加 `KINGDEE_DOMAIN_WORDS` 常量（~60 金蝶/ERP 术语）；`JiebaTokenizer::with_domain_dict()` 替代 `with_default_dict()`；`BM25Service::new()` 使用 `with_domain_dict()` |
| `src-tauri/src/services/hybrid_search.rs` | 加权 RRF 融合：新增 `VECTOR_WEIGHT=2.0` 和 `BM25_WEIGHT=1.0` 常量；`rrf_fuse()` 使用权重计算得分 |
| `src-tauri/src/commands/core.rs` | 清理 unused `Arc` import |
| `src-tauri/src/commands/embedding.rs` | 清理 unused `Manager` import |
| `src-tauri/src/commands/risk_blueprint.rs` | 清理 unused `AppHandle` import |

**BM25 写入策略**：
- 在 chunk 循环内收集 BM25 数据到 `Vec<(i64, String, String, Option<String>, String)>`
- 在 idx+meta lock 释放后调用 `add_chunks()` 批量写入，避免同时持有 3 个锁导致死锁
- 所有 chunk 处理完后调用 `bm25.commit()` 刷新 tantivy 索引

**搜索质量优化**：
- jieba 领域词典：编译内置 ~60 金蝶/ERP 术语（科目、凭证、辅助核算等），改善中文分词精度
- 加权 RRF：向量搜索 weight=2.0（语义相似度主导），BM25 weight=1.0（关键词补充），KB QA 场景下语义匹配更重要
- 临时文件过滤：`is_temp_file()` 跳过 `~$` 前缀 Office 锁文件和 Thumbs.db

**编译测试结果**：
- 0 编译错误，0 警告
- 156 单元测试通过，5 集成测试通过

**git 提交**：
- `996a662` — feat: 模板字段提取扩展
- `cb77cf4` — fix: BM25 搜索质量修复

## 所有 DEVLOG 任务已完成 ✅

## 关键设计决策
- SDT/FormFields 在金蝶模板中非输入字段，不需要专门解析
- XXXX 后字段名：紧跟中文字符，最多 10 个中文字符（超过视为标题）
- 花括号优先级高于 XXXX
- 修复 byte boundary panic：用 char 索引替代 byte 索引
- **搜索质量根因**：不是模型降级问题，而是 BM25 索引从未被写入数据
- **BM25 写入策略**：批量收集→释放锁→批量写入→commit，避免死锁
- **Orphan 文档（0 chunks）**：BM25 中不会有数据，不需要额外清理

## BM25Service 接口参考
```rust
// 构造
BM25Service::new(index_dir: PathBuf) -> Result<Self, String>

// 单条写入
add_chunk(&self, chunk_id: i64, title: &str, content: &str, section_path: Option<&str>, project: &str) -> Result<(), String>

// 批量写入
add_chunks(&self, chunks: &[(i64, String, String, Option<String>, String)]) -> Result<(), String>

// 刷新索引（必须调用才能搜索到新数据）
commit(&self) -> Result<(), String>

// 搜索
search(&self, query: &str, project_id: Option<&str>, top_k: u32) -> Result<Vec<BM25SearchResult>, String>

// 删除
remove_chunk(&self, chunk_id: i64) -> Result<(), String>
remove_project(&self, project: &str) -> Result<(), String>
```

## AppState 参考
```rust
pub struct AppState {
    pub embedding: Arc<Mutex<EmbeddingService>>,
    pub vector_index: Arc<Mutex<VectorIndex>>,
    pub metadata: Arc<Mutex<MetadataStore>>,
    pub bm25: Arc<Mutex<BM25Service>>,  // 已存在
    pub llm: LLMService,
    pub data_dir: PathBuf,
    // ...
}
```