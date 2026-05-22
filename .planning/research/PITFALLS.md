# Pitfalls Research

**Domain:** 本地 RAG 桌面知识管理工具（金蝶ERP实施顾问）
**Researched:** 2026-05-23
**Confidence:** HIGH

---

## Critical Pitfalls

### Pitfall 1: Embedding 模型中文语义退化 —— 英文模型处理中文文档

**What goes wrong:**
`all-MiniLM-L6-v2` 是在英文语料（215M 英文问答对 + 10亿训练对）上训练的。对中文文本进行向量化时，模型将中文字符映射到英文语义空间，导致：
- 语义相近的中文句子在向量空间中距离过远（"金蝶云星空"与"金蝶苍穹"相似度偏低）
- ERP 专业术语（"期货点价""二开""PCR"）的向量表示质量极差，模型从未见过这些中文组合
- 检索召回率大幅下降：用户查询"客户要做期货点价"可能返回无关内容

**Why it happens:**
Sentence-Transformers 官方文档明确标注该模型是 "all-round model tuned for many use-cases" 且基于 `nreimers/MiniLM-L6-H384-uncased`（Uncased = 英文小写化）。模型 tokenizer 对中文字符采用字节级 BPE 编码，每个中文字被拆成 2-3 个 byte token，语义信息被稀释。

**How to avoid:**
1. **立即动作**：在技术验证阶段用中文 ERP 样本做检索精度评估（Precision@5），如果 < 0.6 则必须换模型
2. **替代方案**：考虑 `BAAI/bge-small-zh-v1.5`（512维，中文专用，MTEB 中文榜 Top-5）或 `shibing624/text2vec-base-chinese`（768维），两者都支持 ONNX 导出
3. **最小改动方案**：如果坚决用 all-MiniLM-L6-v2，至少要用中文语料做 domain-adaptive fine-tuning（用金蝶文档对训练），否则召回率无法保证

**Warning signs:**
- 中文查询返回不相关英文语义的结果
- 相同含义的中文表达（如"期货点价"和"点价交易"）余弦相似度 < 0.6
- 用户反馈"搜不到想要的内容"——首先怀疑 embedding 模型

**Phase to address:**
Phase 2（技术验证：embedding + ChromaDB 跑通）——必须在投入大量分块/索引工作前验证

**Sources:**
- Sentence-Transformers 官方模型卡片 (HIGH): https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2
- Context7 `/huggingface/sentence-transformers` docs (HIGH): max sequence length 256, 384 dim
- RAG 10 Failure Patterns (MEDIUM): https://unimon.co.th/en/blog/rag-implementation-failure-patterns

---

### Pitfall 2: ChromaDB 无 WAL 模式导致 16 分钟卡死 + 崩溃后数据库损坏

**What goes wrong:**
ChromaDB 嵌入式模式的 SQLite 后端默认使用 DELETE journal_mode（而非 WAL），导致：
1. **多进程/多线程并发写冲突**：两个 PersistentClient 同时初始化同一路径时，第二个会卡住最长 1000 秒（`busy_timeout` 设置为 1000s，疑似单位错误，正常应为 5s）
2. **异常退出后数据库损坏**：应用崩溃、OOM Kill、强制关机时 DELETE 模式下正在进行的事务会导致 SQLite 文件损坏（`database disk image is malformed`）
3. **文件锁定无法关闭**：ChromaDB 没有显式 `close()` 方法，只能通过 gc 释放资源，导致知识库路径被锁定无法移动/删除/备份

**Why it happens:**
ChromaDB 的 Rust SQLite 层（`rust/sqlite/src/config.rs`）在构建连接时遗漏了 `PRAGMA journal_mode=WAL` 设置。此外 `busy_timeout` 被设为 1000 秒而非 5 秒（Community Issue #7040 确认）。WAL 模式在 NFS/SMB 网络文件系统上有锁兼容问题，但对于本地桌面应用完全适用。

**How to avoid:**
1. **启动时强制设置 WAL**：在应用启动脚本中执行 `sqlite3 chroma.sqlite3 'PRAGMA journal_mode=WAL;'`（必须在应用启动前，因为运行中执行会触发 busy_timeout）
2. **实现优雅关闭**：监听 SIGTERM/窗口关闭事件，显式调用 `client._system.stop()` + `gc.collect()` 后再退出
3. **启动时完整性检查**：每次启动运行 `PRAGMA integrity_check;`，失败时自动从备份恢复
4. **定期自动备份**：每次知识库变更后创建 chroma.sqlite3 副本（ChromaDB 官方推荐文件系统备份方式）
5. **单进程架构**：确保同一应用实例只创建一个 PersistentClient（ChromaDB 是线程安全但不进程安全）

**Warning signs:**
- 应用启动时卡在 splash screen 超过 10 秒
- 日志中出现 `sqlite3.OperationalError: unable to open database file`
- 日志中出现 `OSError: [Errno 24] Too many open files`
- ChromaDB 查询报 `InternalError: Error executing plan: Internal error: Error finding id`（损坏后症状）

**Phase to address:**
Phase 2（技术验证）——验证 WAL 设置和启动完整性检查逻辑；Phase 4（数据持久化）——实现自动备份和恢复

**Sources:**
- ChromaDB Issue #7040 "PersistentClient second-opener hangs ~16 minutes" (HIGH): https://github.com/chroma-core/chroma/issues/7040
- ChromaDB Issue #5868 "Unable to Close Persistent Client - SQLite corruption" (HIGH): https://github.com/chroma-core/chroma/issues/5868
- ChromaDB Issue #4039 "unable to open database file" (HIGH): https://github.com/chroma-core/chroma/issues/4039
- schema.ai "Collection Metadata Corruption After Unclean Shutdown" (MEDIUM): https://schema.ai/technologies/chroma/insights/collection-metadata-corruption-unclean-shutdown

---

### Pitfall 3: 中文分块策略失效 —— 英文分隔符无法切割中文

**What goes wrong:**
LangChain 的 `RecursiveCharacterTextSplitter` 默认分隔符列表 `["\n\n", "\n", " ", ""]` 和句号分隔符 `"."`（英文句点+空格）无法识别中文标点符号（`。！？；，`）。对中文文档的分块实际退化为固定长度切割：
- 在句子中间切断（"标准产品没有内置的期货点价模块\n需要通过二开" 可能在"通过"处被切断）
- ERP 术语被拆分到两个 chunk（"期货点价"在第一块结尾，"模块"在第二块开头）
- 检索时返回"半截"答案

**Why it happens:**
`RecursiveCharacterTextSplitter` 的 `separators` 参数默认值针对英文设计。中文的自然语义边界由 `。！？` 标记，但这些不在默认分隔符优先级列表中。

**How to avoid:**
1. **使用中文优化分隔符列表**：
```python
separators = [
    "\n## ",    # Markdown H2
    "\n### ",   # Markdown H3
    "\n\n",     # 段落
    "\n",       # 换行
    "。",       # 中文句号
    "！",       # 感叹号
    "？",       # 问号
    "；",       # 分号
    "，",       # 逗号
    ".",        # 英文句号（兜底）
    " ",        # 空格（兜底）
]
```
2. **chunk_size 以 token 数而非字符数计算**：all-MiniLM-L6-v2 最大输入仅 256 tokens，chunk_size 必须 ≤ 200 tokens（留余量给 overlap）
3. **重叠设为 15-20%**：ERP 文档术语密集，低重叠率会导致术语被边界切断
4. **SPEC 中的递归分块策略是正确的**（H2→段落→句子），但需要将 Rust 侧的 splitter 配置为中文感知

**Warning signs:**
- 向量检索返回的内容以半句话开头或结尾
- 同一术语频繁出现在不同 chunk 的边界处
- 用户反馈"检索到的内容不完整"

**Phase to address:**
Phase 3（分块引擎）——实现中文感知分隔符；Phase 2（技术验证）——用中文样本验证分块质量

**Sources:**
- LangChain Semantic Chunker 中文指南 (MEDIUM): https://langchain.cadn.net.cn/python/docs/how_to/semantic-chunker/index.html
- RAG Chunking 实战 (MEDIUM): https://tool.lu/article/7sa/detail
- 掘金 "RAG系统的分块策略工程2026" (MEDIUM): https://juejin.cn/post/7635674038564519988
- RAG落地实战之文本切分4种策略全解析 (MEDIUM): https://developer.volcengine.com/articles/7541274990247854134

---

### Pitfall 4: Tauri WebView2 冷启动白屏 —— Windows 用户感知体验极差

**What goes wrong:**
Tauri 应用在 Windows 上启动时，WebView2 初始化需要 2-20 秒（取决于机器性能和 WebView2 版本）：
- 这段时间窗口显示纯白色背景，用户以为应用卡死
- 某些 Windows 10 系统（WebView2 未预装或版本老旧）启动延迟更严重
- 管理员权限运行时会进一步增加延迟（Community Issue #13727 确认 20+ 秒）

**Why it happens:**
WebView2 是一个独立进程（msedgewebview2.exe），Tauri 需要等待它启动并加载 HTML。本身不是 Tauri 的问题，而是 WebView2 的运行时初始化开销。Windows 7/10 旧系统上 WebView2 可能需要先安装/更新 Evergreen Runtime。

**How to avoid:**
1. **使用原生 Splash Screen**（Tauri 2.x 支持 `splashscreen` 配置）：在 WebView2 初始化期间显示应用 Logo/品牌色窗口（原生渲染，不依赖 WebView）
2. **Tauri 2.x 的 WebView2 安装**：配置 `webviewInstallMode` 为 `fixedRuntime` 捆绑特定版本，避免首次启动下载
3. **最小化初始化 HTML**：第一个加载的 HTML 应该极简（<10KB），避免加载 React/TailwindCSS 等大 bundle
4. **Lazy load 前端框架**：Splash screen 后渐进式加载 React 和主界面
5. **避免管理员权限**：不请求 `run as admin`，WebView2 在管理员模式下初始化显著变慢

**Warning signs:**
- 双击 .exe 后 3+ 秒才看到任何 UI
- 用户报告"点了没反应，以为没安装成功"
- 任务管理器中 msedgewebview2.exe 长时间未出现

**Phase to address:**
Phase 1（项目脚手架 + Tauri 配置）——配置 splash screen 和 WebView2 fixed runtime

**Sources:**
- Tauri Issue #13727 "Slow startup on Windows (over 20 seconds)" (HIGH): https://github.com/tauri-apps/tauri/issues/13727
- Tauri Discussion #10822 "How to speed up loading" (HIGH): https://github.com/tauri-apps/tauri/discussions/10822
- Tauri 2.x WebView2 文档 (HIGH): https://v2.tauri.app/distribute/windows-installer

---

### Pitfall 5: RRFR 融合参数固化 —— k=60 不是银弹

**What goes wrong:**
SPEC.md 中 RRFR 的 k 值固定为 60：`score = sum(1 / (rank + 60))`。这个值来自学术论文的推荐，但：
- k 值越小，排名靠前的结果权重越大（更容易受单一检索方法影响）
- k 值越大，排名差异被"抹平"（不同位置的结果分数接近）
- 对于 ERP 场景，BM25 返回的精确关键词匹配（如 "PCR-003"）在低 k 值下可能被向量结果的微弱排名优势淹没
- 向量检索返回 30 条 + BM25 返回 30 条 = 60 条候选，但用户只需要 top 5

**Why it happens:**
RRFR 是无监督融合算法，不依赖分数绝对值，只看排名。这是它的优点也是缺点：无法区分"排名第 1 相关性 0.95"和"排名第 1 相关性 0.5"。

**How to avoid:**
1. **在评估集上网格搜索 k 值**：测试 k ∈ {10, 30, 60, 100, 200}，选 Recall@5 最高的
2. **考虑加权 RRFR**：给 BM25 结果更高的权重（ERP 场景下精确术语匹配很重要）：
   `score = α * sum(1/(k + rank_bm25)) + (1-α) * sum(1/(k + rank_vector))`，α ∈ [0.5, 0.7]
3. **实现去重逻辑**：同一个文档在向量和 BM25 结果中都出现时，取较高的 rank
4. **添加重排序（Re-ranking）**：用 Cross-Encoder 对融合后的 Top-20 做精排，然后再取 Top-5

**Warning signs:**
- 精确搜索 "PCR-003" 返回无关内容（向量检索主导）
- 语义搜索"期货点价"遗漏包含"点价交易"但无"期货"词的结果（BM25 主导）
- 用户反馈"明明有这个文档但搜不出来"

**Phase to address:**
Phase 5（检索引擎）——实现可配置的 RRFR + 评估脚本

**Sources:**
- RAG 混合检索实战 (MEDIUM): https://blog.csdn.net/2401_84526799/article/details/160903983
- RAG Hybrid Search 策略 (MEDIUM): https://www.cnblogs.com/pass-ion/p/19572891
- 混合检索与重排序实战 (MEDIUM): https://blog.csdn.net/Crown_22/article/details/160411539

---

### Pitfall 6: 中文 Token 计数失准 —— 上下文窗口被悄悄撑爆

**What goes wrong:**
tiktoken（OpenAI 官方 tokenizer）对中文的 token 计数与英文有本质差异：
- 1 个英文字符 ≈ 0.25 token，1 个中文字符 ≈ 1.5-2 tokens
- SPEC.md 设定的 `max_tokens = 4096` 是给 LLM 的总配额，但构造 prompt 时如果以字符数估算，5000 中文字符的 context 实际消耗 ~8000 tokens，直接超出限制
- 超出上下文窗口时，OpenAI API 会自动截断（从开头截断），导致系统 prompt 被部分丢失
- 或者 API 直接返回错误（Anthropic 更严格）

**Why it happens:**
中文一个字通常对应 1-2 个 BPE token，而英文一个单词对应 1-3 个 token。开发时容易用"中文字符数 ≈ 1 token"做估算，导致实际用量翻倍。

**How to avoid:**
1. **使用 tiktoken 精确计数**：`tiktoken.get_encoding("cl100k_base").encode(text)` 返回真实 token 数
2. **动态调整 context 大小**：根据检索结果的 token 数动态截断，而非固定 top_k
3. **预留 token 预算**：
   - 系统 prompt: ~200 tokens
   - 用户 prompt 模板: ~100 tokens
   - 检索上下文: ~3000 tokens（中文约 2000 字符）
   - LLM 回答: ~800 tokens
4. **实现 TokenCountingContext**：在 Rust 侧用 tiktoken-rs crate 做实时计数，前端组装前就截断

**Warning signs:**
- 用户长查询后 LLM 返回不完整回答或截断内容
- OpenAI API 返回 `finish_reason: "length"` 而非 "stop"
- 日志显示 `max_tokens` 设置值远小于实际使用量

**Phase to address:**
Phase 6（LLM 集成）——实现 token 计数和动态窗口管理

**Sources:**
- tiktoken 官方文档 (HIGH): https://github.com/openai/tiktoken
- OpenAI Token 使用指南 (HIGH): https://zhuanlan.zhihu.com/p/626593576
- CSDN "tiktoken 统计 Token" (MEDIUM): https://blog.csdn.net/uncle_ll/article/details/159614529

---

### Pitfall 7: ChromaDB 版本迁移陷阱 —— 升级导致知识库不可用

**What goes wrong:**
ChromaDB 跨大版本升级（如 0.4.x → 0.5.x → 1.x）需要执行 Schema 迁移（migration）。迁移：
- 是不可逆的（一旦升级无法降级）
- 失败会导致数据完全不可用
- 需要用户主动触发（或在应用启动时自动执行，用户无感知）
- 某些版本之间迁移不兼容（如 0.4.x 到 1.x 的 embedding queue 格式变更）

**Why it happens:**
ChromaDB 是一个快速迭代的开源项目。从 0.4 到 1.x，底层存储格式经历了多次重大变更。v1.x 开始使用新的 YAML 配置文件，`IS_PERSISTENT` 等环境变量也有变化。如果应用打包了特定版本的 ChromaDB，用户升级应用时可能面临数据库不兼容。

**How to avoid:**
1. **锁定 ChromaDB 版本**：在 Cargo.toml 中固定依赖版本（如 `chromadb = "=1.x.x"`）
2. **升级前强制备份**：在启动时检测版本变化，自动创建 `chroma.sqlite3.backup`
3. **迁移验证**：设置 `migrations="validate"` 在开发/CI 中检查兼容性
4. **应用内迁移流程**：
   - 检测现有数据库版本
   - 如果与应用要求的版本不一致，弹出确认对话框
   - 用户确认后执行 `chroma backup → upgrade → validate`
5. **降级策略**：如果迁移失败，从备份恢复并提示用户手动处理

**Warning signs:**
- 应用升级后启动报 `MigrationError`
- ChromaDB 日志中大量 migration 相关错误
- 用户反馈"升级后知识库打不开了"

**Phase to address:**
Phase 2（技术验证）——确定 ChromaDB 锁定版本；Phase 7（应用生命周期）——实现升级流程

**Sources:**
- ChromaDB 官方 Schema Migrations 文档 (HIGH): https://www.mintlify.com/chroma-core/chroma/operations/migrations
- ChromaDB Issue #6654 "IS_PERSISTENT defaults to False — silent data loss" (HIGH): https://github.com/chroma-core/chroma/issues/6654

---

### Pitfall 8: API Key 明文存储 —— 配置文件被窃取后密钥泄露

**What goes wrong:**
SPEC.md 计划将 API Key 存储在 `~/.kingdee-kb/config.json` 的明文 JSON 文件中：
- 任何能访问用户文件系统的程序都可以读取
- 恶意软件可以直接窃取 API Key
- 用户备份/迁移配置文件时可能通过不安全渠道传输
- 前端 JavaScript 可以访问 Rust 侧传回的 API Key（XSS 风险）
- 如果知识包配置了 GitHub URL，攻击者可以构造恶意知识包读取本地文件

**Why it happens:**
开发者倾向于简单方案："用户自己保护的 API Key，我们只是存在本地"。但桌面应用有更安全的存储方案可用。用户对"本地存储 = 安全"有错误认知。

**How to avoid:**
1. **使用 OS 原生凭据存储**：
   - Windows: Credential Manager（通过 `keyring` crate）
   - macOS: Keychain（通过 `keyring` crate 或 `security` CLI）
   - Linux: Secret Service / GNOME Keyring / KWallet
2. **推荐 Tauri 插件**：`tauri-plugin-keyring-store`（基于 keyring crate，跨平台 OS 凭据存储）或 `tauri-plugin-configurate`（支持 keyring 字段标记）
3. **API Key 不经过前端**：Rust 侧直接从 Keyring 读取 API Key 发起 LLM 请求，前端只发送查询文本，不持有 APK Key
4. **config.json 只存非敏感配置**：模型名称、endpoint URL、主题偏好等
5. **敏感字段加密兜底**：对不支持 Keyring 的环境（如无 GUI 的 Linux），使用 `tauri-plugin-store` 的加密 JSON 文件 + 应用密钥

**Warning signs:**
- 用户打开 `config.json` 直接看到明文 API Key
- 前端代码中出现 `apiKey` 变量
- 日志中打印了 API Key（务必脱敏）

**Phase to address:**
Phase 1（项目脚手架）——集成 keyring 插件；Phase 6（LLM 集成）——API 调用路径不经过前端

**Sources:**
- tauri-plugin-keyring-store (HIGH): https://github.com/s00d/tauri-plugin-keyring-store
- tauri-plugin-configurate (HIGH): https://github.com/Crysta1221/tauri-plugin-configurate
- DEV Community "Storing API Key Securely in Tauri" (HIGH): https://dev.to/hiyoyok/storing-a-gemini-api-key-securely-in-a-tauri-app-dont-hardcode-it-4cdk
- DEV Community "Jira Time Tracker with Tauri - API Tokens Securely" (HIGH): https://dev.to/jorrygo_dev/building-a-jira-time-tracker-with-tauri-how-i-stored-api-tokens-securely-46aj

---

### Pitfall 9: IPC 大数据传输冻结 UI —— 索引进度实时更新失效

**What goes wrong:**
Tauri 的 IPC 桥（invoke/emit）在传输大数据时存在严重性能问题：
- 120K 文件扫描场景：70MB JSON 通过 IPC 桥 = UI 冻结 47 秒（真实案例，AssetHoard 项目）
- 即使使用 chunked streaming（80 次 IPC 调用，每次 625KB），总耗时增加到 45 秒
- ChromaDB 查询返回大量向量结果时，JSON 序列化/反序列化耗时显著
- 嵌入式 ChromaDB 在 Rust 侧运行（不是独立服务），embedding 计算时 UI 线程可能被阻塞

**Why it happens:**
Tauri IPC 每次调用涉及 Rust JSON 序列化 → 跨进程传输 → JavaScript JSON 解析，三者都在单线程上执行。当知识库包含数千个文档时，单次检索返回的完整结果集可能达到数 MB。

**How to avoid:**
1. **数据不下桥**：Rust 侧处理重型数据（扫描文件、嵌入计算、ChromaDB 查询），前端只接收展示所需的最小数据集
2. **进度事件替代数据传输**：用 Tauri event system 发送轻量进度更新（`{current: 450, total: 1000}`，~100 bytes），而非传输处理完的结果
3. **分页查询**：检索结果分页返回（每页 10-20 条），前端按需加载更多
4. **Web Worker 处理前端 JSON 解析**：避免在主线程解析大数据
5. **嵌入计算在后台线程**：使用 Rust `tokio::spawn_blocking` 或在独立线程池中运行嵌入模型推理

**Warning signs:**
- 大量文件导入时窗口无响应（> 2 秒无 UI 更新）
- 任务管理器显示单个 CPU 核心 100%
- 用户报告"导入多文件时应用假死"

**Phase to address:**
Phase 4（数据持久化）——实现异步索引 + 进度事件；Phase 3（分块引擎）——流式处理管道

**Sources:**
- AssetHoard Tauri 性能优化经验 (HIGH): https://assethoard.com/blog/when-120000-files-meet-tauri
- Tauri Issue #4197 "Transfer rate from backend is very slow" (HIGH): https://github.com/tauri-apps/tauri/issues/4197
- Tauri 2.x 文档 — IPC 和 Events (HIGH): https://v2.tauri.app/

---

### Pitfall 10: ERP 多项目知识混淆 —— 不同客户的知识互相污染

**What goes wrong:**
ERP 实施顾问通常同时服务多个客户（星达铜业、某某制造、某某餐饮等），每个项目的术语和解决方案可能相似但细节不同：
- "期货点价"在星达铜业是二开方案，在另一个项目可能是标准功能变通
- 跨项目检索时返回其他客户的信息，可能导致错误的实施建议
- 知识库没有项目级别的隔离机制，用户手动整理标签容易遗漏

**Why it happens:**
向量检索基于语义相似性，无法区分上下文差异。"采购模块改造"在项目 A 和项目 B 中向量距离很近，但业务上下文截然不同。

**How to avoid:**
1. **强制项目标签**：每条知识入库时必须关联项目（`project: "星达铜业"`），ChromaDB 查询时用 `where={"project": current_project}` 过滤
2. **默认项目隔离**：检索默认只搜索当前选中项目，提供"跨项目搜索"作为高级选项
3. **命名空间机制**：用 ChromaDB 的 Collection 按项目分库（`collection_kingdee_knowledge_project_xingda`），检索时指定目标 collection
4. **术语词典**：在系统 prompt 中注入当前项目的术语对照表（"本项目中的'点价'指……"），帮助 LLM 正确理解上下文
5. **来源标注强制**：每个检索结果必须显示所属项目名，防止用户误用其他项目的信息

**Warning signs:**
- 用户报告"建议方案不适用于当前项目"
- 检索结果列表中混入了不同客户的内容
- 顾问说"这是上个项目的方案，但这次不适用"

**Phase to address:**
Phase 4（数据持久化）——元数据 Schema 包含项目隔离字段；Phase 5（检索引擎）——实现项目级过滤

**Sources:**
- Golden-Retriever: Agentic RAG for Industrial KB (MEDIUM): https://arxiv.org/html/2408.00798v1
- RAG-Driven Data Quality Governance for ERP (MEDIUM): https://huggingface.co/papers/2511.16700
- Odoo ERP RAG 实践 (MEDIUM): https://dev.to/harideevagan/how-i-built-a-rag-powered-conversational-assistant-for-odoo-erp-3pjn

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| 用英文 embedding 模型直接处中文 | 免去找/转换模型的工作量 | 检索召回率低30-40%，用户不满 | 仅 Phase 2 技术验证阶段 |
| API Key 存明文 config.json | 实现最快（30分钟） | 安全审计失败，用户数据泄露风险 | 仅 Phase 2 阶段，Phase 6 前必须迁移到 Keyring |
| ChromaDB 不配置 WAL / 不备份 | 跳过 5 行配置代码 | 崩溃后数据库损坏，所有知识丢失 | 永远不可接受 — 这是数据损毁风险 |
| RRFR k=60 写死 | 不用调参 | 检索质量不可控，用户场景差异大 | Phase 5 评估阶段，之后再固化 |
| 固定 top_k=5 不做动态截断 | 实现简单 | 长查询时上下文窗口溢出 | Phase 6 之前临时可用 |
| 不分项目隔离（全库搜索） | 不需要用户输入项目标签 | 跨项目知识污染，给出错误建议 | 仅当 MVP 只有 1 个项目时 |
| 不分页，一次传全部检索结果 | 减少一次 IPC 调用 | 大量结果时 UI 卡顿 | 仅在知识库 < 100 条时 |
| 固定 chunk_size=512（字符计） | 实现简单 | Embedding 模型截断导致语义丢失 | 永远不可 — 必须以 token 计 |

---

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| ChromaDB PersistentClient | 多处创建 client 实例（进程不安全） | 全局单例，通过 Arc<RwLock<>> 共享 |
| OpenAI Chat API | 用字符数估算 token 控制上下文 | 用 tiktoken-rs 精确计数后动态截断 |
| ONNX Runtime (embedding) | 每次推理都重新加载模型 | 模型在应用生命周期内常驻内存 |
| Tauri WebView2 | 发布时不捆绑 WebView2 Runtime | `webviewInstallMode: fixedRuntime` 捆绑或使用 `offline` 安装器 |
| Tauri IPC | 传输大型数据结构（数组/文件内容） | 流式事件报告进度，数据保持在 Rust 侧 |
| BM25 检索引擎 | 单次查询扫描全量文档 | 提前构建倒排索引，查询时 O(1) 查找 |
| GitHub 知识包导入 | git clone 默认 depth=full | 使用 `git clone --depth=1` 减少下载量 |
| SQLite (ChromaDB) | 不设置 WAL 模式 | 启动时 PRAGMA journal_mode=WAL |

---

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Embedding 模型首次下载 | 首次启动白屏 1-3 分钟（下载 ~90MB） | 安装包内预置模型文件，避免运行时下载 | 首次安装（每个用户） |
| ChromaDB 冷启动 | 应用启动卡 5-15 秒（加载 HNSW 索引） | lazy-load：仅连接，首次查询时才加载索引 | 知识库 > 10000 chunks |
| 全量重新索引 | 每次添加文件都重建整个索引 | 增量索引：只对新文件做 embedding + insert | 知识库 > 1000 条 |
| 嵌入计算阻塞 UI | 批量导入时窗口无响应 | `tokio::spawn_blocking` + 进度事件通知 | 导入 > 50 个文件 |
| ChromaDB 查询逐条 serialization | 大量结果时 IPC 传输慢 | 分页返回 + Rust 侧做 top_k 截断 | 检索结果 > 50 条 |
| 大型 Markdown 文件全量加载 | 打开 10MB .md 文件卡死 | 虚拟滚动 + 流式读取 | 单文件 > 500KB |
| WebView2 内存泄漏 | 长时间运行后内存持续增长 | 定期重启 WebView（Tauri 2.x 支持 webview 重启） | 连续运行 > 24h |

---

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| API Key 明文存磁盘 | 恶意软件/物理访问窃取密钥 | OS Keyring 存储，API Key 不经过前端 |
| 知识库内容无加密 | 物理访问磁盘可读取所有知识 | SQLCipher 加密 ChromaDB（可选，非 MVP 必须） |
| Prompt 注入（通过知识库内容） | 恶意构建的知识包 content 可劫持 LLM 行为 | 检索上下文与用户查询分开发送，系统 prompt 置顶不可覆盖 |
| LLM 请求无超时 | 网络问题导致 UI 永久加载 | 30 秒超时 + 用户可取消 |
| 配置文件无校验 | config.json 损坏导致应用崩溃 | 启动时 JSON Schema 验证 + 损坏时回退默认值 |
| 日志输出 API Key | 调试日志在控制台泄露密钥 | 配置脱敏中间件，API Key 写入前替换为 `***` |
| git clone 执行任意代码 | 恶意知识包仓库包含 post-checkout hooks | git clone 时不执行 hooks（`-c core.hooksPath=/dev/null`） |

---

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| 无检索相关度展示 | 用户无法判断结果可信度 | 显示相关度分数/条形图 + 来源标注 |
| AI 回答无法追溯到原文 | 用户无法验证 AI 是否"编造" | 回答中内联引用 + 点击跳转到原文 |
| 知识添加后索引完成无反馈 | 用户刷新页面发现搜不到刚添加的内容 | 显示实时索引进度条 + 完成后通知 |
| 全库搜索无项目标签 | ERP 顾问误用其他项目方案 | 默认项目隔离，标签过滤优先 |
| 首次启动无引导 | 用户面对空界面不知道做什么 | 显示示例知识 + "拖入文件开始"引导 |
| 嵌入模型下载无进度 | 用户以为应用卡死 | 下载进度条 + 体积预估 + 取消按钮 |
| 错误消息不友好 | 技术报错用户看不懂 | 用户侧展示友好提示，技术详情记日志 |

---

## "Looks Done But Isn't" Checklist

- [ ] **知识导入:** 拖入 .md 文件后索引进度条正常显示，不会因为批量文件导致 UI 冻结 — 验证: 导入 20 个 500KB .md 文件
- [ ] **检索功能:** 中文查询返回中文结果，不因英文 embedding 模型而导致召回失效 — 验证: 用 10 个中文 ERP 查询做 Precision@5 测试
- [ ] **ChromaDB 持久化:** 强制关闭应用后重启，知识库完整可用，数据不损坏 — 验证: 入库 100 条后 `taskkill /f`，重启检查
- [ ] **API Key 安全:** config.json 中不包含明文 API Key，前端 JS 无法读取 API Key — 验证: 检查 `~/.kingdee-kb/config.json` 文件内容
- [ ] **LLM 回答:** 超过上下文窗口限制时不报错，而是优雅截断 + 提示用户 — 验证: 检索 20+ 条相关结果触发超窗
- [ ] **多项目隔离:** 切换项目后检索结果仅来自当前项目 — 验证: 创建 2 个项目各 20 条知识，分别检索
- [ ] **冷启动性能:** 首次启动（含 WebView2 + 模型加载）在 10 秒内完成 — 验证: 在 Windows 10 旧机器上测试
- [ ] **崩溃恢复:** 异常退出后 ChromaDB 自动执行 integrity_check 并恢复 — 验证: 模拟应用崩溃后重启

---

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| ChromaDB SQLite 损坏 | MEDIUM | 1. 运行 `PRAGMA integrity_check` 2. 如失败: `sqlite3 chroma.sqlite3 '.recover' \| sqlite3 recovered.db` 3. 替换原文件 4. 如果恢复也失败: 从最近备份恢复 5. 最坏情况: 清空数据库，重新索引原始文件 |
| API Key 泄露 | LOW | 1. 立即在 OpenAI/Anthropic 控制台吊销 Key 2. 生成新 Key 3. 在应用中重新输入 |
| 嵌入模型下载失败 | LOW | 1. 降级到在线 API embedding（如 text-embedding-3-small）2. 用户手动放置模型文件到指定目录 |
| 迁移失败导致数据不可用 | HIGH | 1. 从 `chroma.sqlite3.backup` 恢复 2. 降级应用版本 3. 如果无备份: 重新索引所有 .md/.txt 文件 |
| WebView2 无法启动 | MEDIUM | 1. 检测 WebView2 安装状态 2. 提示用户手动安装 3. 作为最后手段: 降级到 Tauri 1.x 的 Edge WebView |

---

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Embedding 中文退化 (P1) | Phase 2 - 技术验证 | 中文 ERP 样本 Precision@5 ≥ 0.6 |
| ChromaDB 无 WAL (P2) | Phase 2 - 技术验证 | 模拟崩溃后数据完整性测试 |
| 中文分块失效 (P3) | Phase 3 - 分块引擎 | 分块边界检查：不在句子中间切割 |
| Tauri 冷启动白屏 (P4) | Phase 1 - 项目脚手架 | Splash screen < 500ms 出现 |
| RRFR 参数固化 (P5) | Phase 5 - 检索引擎 | 网格搜索最优 k 值 |
| Token 计数失准 (P6) | Phase 6 - LLM 集成 | 中文 3000 字符上下文 < 4000 tokens |
| ChromaDB 版本迁移 (P7) | Phase 7 - 应用生命周期 | 升级测试：旧库 → 新版本可用 |
| API Key 明文存储 (P8) | Phase 1 + Phase 6 | config.json 无明文 Key |
| IPC 大数据冻结 UI (P9) | Phase 4 - 数据持久化 | 批量导入 50 文件 UI 保持 60fps |
| ERP 多项目混淆 (P10) | Phase 4 + Phase 5 | 跨项目检索返回正确隔离结果 |

---

## Sources

### HIGH Confidence
- **Sentence-Transformers 官方模型卡片**: https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2
- **Context7 ChromaDB Cookbook**: https://cookbook.chromadb.dev/ (via Context7 /websites/cookbook_chromadb_dev)
- **Context7 Tauri 2.x 文档**: https://v2.tauri.app/ (via Context7 /websites/v2_tauri_app)
- **Context7 HuggingFace Transformers**: https://huggingface.co/docs/transformers (via Context7 /huggingface/transformers)
- **Context7 Sentence-Transformers**: https://sbert.net/ (via Context7 /huggingface/sentence-transformers)
- **ChromaDB Issue #7040** (PersistentClient 16分钟hung): https://github.com/chroma-core/chroma/issues/7040
- **ChromaDB Issue #5868** (SQLite 损坏/锁): https://github.com/chroma-core/chroma/issues/5868
- **ChromaDB Issue #6654** (IS_PERSISTENT 数据静默丢失): https://github.com/chroma-core/chroma/issues/6654
- **ChromaDB Issue #4039** (无法打开数据库文件): https://github.com/chroma-core/chroma/issues/4039
- **Tauri Issue #13727** (Windows 20秒慢启动): https://github.com/tauri-apps/tauri/issues/13727
- **Tauri Discussion #10822** (WebView2 慢加载): https://github.com/tauri-apps/tauri/discussions/10822
- **Tauri Issue #4197** (IPC 大数据传输极慢): https://github.com/tauri-apps/tauri/issues/4197
- **Chromium Schema 迁移文档**: https://www.mintlify.com/chroma-core/chroma/operations/migrations
- **tauri-plugin-keyring-store**: https://github.com/s00d/tauri-plugin-keyring-store
- **tauri-plugin-configurate**: https://github.com/Crysta1221/tauri-plugin-configurate
- **AssetHoard Tauri 120K 文件性能优化**: https://assethoard.com/blog/when-120000-files-meet-tauri

### MEDIUM Confidence
- **RAG 10 Failure Patterns**: https://unimon.co.th/en/blog/rag-implementation-failure-patterns
- **Schema.ai ChromaDB 损坏恢复**: https://schema.ai/technologies/chroma/insights/collection-metadata-corruption-unclean-shutdown
- **RAG Chunking 实战 (tool.lu)**: https://tool.lu/article/7sa/detail
- **RAG 分块策略工程 (掘金)**: https://juejin.cn/post/7635674038564519988
- **RAG 混合检索实战 (CSDN)**: https://blog.csdn.net/2401_84526799/article/details/160903983
- **Hybrid Search 策略 (博客园)**: https://www.cnblogs.com/pass-ion/p/19572891
- **tiktoken Token 指南 (知乎)**: https://zhuanlan.zhihu.com/p/626593576
- **DEV Community Tauri API Key 安全**: https://dev.to/hiyoyok/storing-a-gemini-api-key-securely-in-a-tauri-app-dont-hardcode-it-4cdk
- **Golden-Retriever Agentic RAG (arXiv)**: https://arxiv.org/html/2408.00798v1
- **RAG for ERP Data Quality (HuggingFace)**: https://huggingface.co/papers/2511.16700
- **LlamaIndex SemanticSplitter 中文问题**: https://developer.volcengine.com/articles/7541274990247854134
- **RAG 中文分块实战 (博客园)**: https://www.cnblogs.com/theseventhson/p/18279980

---

*Pitfalls research for: KingdeeKB (本地 RAG 桌面知识管理工具)*
*Researched: 2026-05-23*
*Confidence: HIGH — 所有 Critical Pitfalls 均由官方文档、GitHub Issues、或生产环境验证报告支撑*
