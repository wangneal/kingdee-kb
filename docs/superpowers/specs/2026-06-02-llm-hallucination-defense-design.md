# KingdeeKB LLM 防幻觉体系设计

> **版本**: v1.0
> **日期**: 2026-06-02
> **范围**: 跨场景 LLM 输出验证层 + RAG 检索增强
> **前置文档**: `2026-06-01-kingdeekb-technical-spec.md`, `2026-06-01-llm-wiki-research-report.md`

---

## 1. 现状分析

### 1.1 现有防护措施

| 措施 | 位置 | 方式 |
|------|------|------|
| 来源标注指令 | `system_prompt.md` | LLM 指令要求标注源文档 |
| 反编造指令 | `system_prompt.md`, `doc_gen_system_prompt.md` | 提示词级别的「禁止编造」约束 |
| 结构约束 | 各 Recipe prompt | 四段式/三段式输出结构强制 |
| 混合检索 | `hybrid_search.rs` | BM25 + 向量 RRF 融合 (k=60) |
| 上下文围栏 | `build_user_prompt()` | `<context>` Hermes 风格隔离 |
| 步骤验证器 | `harness/verifier.rs` | 关键词/空结果检查 |
| Plan-State 验证 | `planner.rs` | 计划执行状态机校验 |
| 脱敏还原 | `llm_service.rs` | 敏感数据脱敏后送 LLM |

### 1.2 现有缺口

| 缺口 | 影响 | 涉及场景 |
|------|------|----------|
| **无生成后验证** | LLM 编造的内容直接输出 | 全部 |
| **无引用校验** | 声称的「来源:xxx.md」可能不存在 | Chat/搜索 |
| **无事实一致性检查** | 回答可能和检索结果矛盾 | 搜索问答 |
| **无置信度估算** | 用户不知道回答是否可靠 | 全部 |
| **无跨场景验证层** | 每个场景自己处理，标准不统一 | 全部 |
| **检索缺少多轮/分解** | 复杂问题可能遗漏关键信息 | 搜索/调研/文档生成 |
| **无 rerank** | 向量+BM25 融合后直接送 LLM，噪声多 | 搜索 |

---

## 2. 架构总览

```
┌────────────────────────────────────────────────────────────────────┐
│                     LLM 生成层 (已有)                               │
│  Chat/Agent · 搜索问答 · 文档生成 · 调研 · 风控报告 · 知识编译     │
└──────────────────────────┬─────────────────────────────────────────┘
                           ↓
┌────────────────────────────────────────────────────────────────────┐
│            验证层 (Verification Layer) — 本设计新增                  │
│                                                                    │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐ │
│  │ 预处理验证    │  │ 生成中约束    │  │ 生成后校验               │ │
│  │ (Pre-flight)  │  │ (In-flight)  │  │ (Post-hoc)              │ │
│  ├──────────────┤  ├──────────────┤  ├──────────────────────────┤ │
│  │ • 检索质量评估│  │ • 结构化输出  │  │ • 引用存在性校验         │ │
│  │ • 查询分解    │  │   模板       │  │ • 事实一致性检查         │ │
│  │ • 缺失检测    │  │ • 内联引用   │  │ • 内部矛盾检测           │ │
│  └──────────────┘  │   强制       │  │ • 不确定性标记           │ │
│                     └──────────────┘  │ • 自一致性格令           │ │
│                                        └──────────────────────────┘ │
└──────────────────────────────────┬──────────────────────────────────┘
                                   ↓
                      ┌────────────────────┐
                      │ 最终输出 & 置信度展示 │
                      └────────────────────┘

┌────────────────────────────────────────────────────────────────────┐
│            检索增强 (RAG Enhancement) — 本设计辅助                    │
│                                                                    │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐ │
│  │ 查询增强      │  | 检索优化      │  │ 上下文工程               │ │
│  ├──────────────┤  ├──────────────┤  ├──────────────────────────┤ │
│  │ • 查询分解    │  │ • Cross-     │  │ • 智能 chunk 选择        │ │
│  │ • 查询扩展    │  │   Encoder    │  │ • 动态 token 分配        │ │
│  │ • 多角度检索  │  │   Rerank    │  │ • 分层上下文窗口         │ │
│  └──────────────┘  └──────────────┘  └──────────────────────────┘ │
└────────────────────────────────────────────────────────────────────┘
```

---

## 3. 验证层 (Verification Layer) — 核心

### 3.1 设计原则

- **跨场景统一抽象**: 所有 AI 场景共享同一个 `VerificationPipeline` trait
- **无侵入集成**: 验证层包装在现有 LLM 调用之外，不改动已有业务逻辑
- **渐进式失败**: 验证不通过 → 自动重生成 → 仍不通过 → 带上验证报告输出
- **可观测**: 每次验证产生 `VerificationReport`，可在 UI 展示

### 3.2 核心类型

```rust
/// 验证结果等级
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationLevel {
    /// 完全通过 — 可以信赖
    Confirmed,
    /// 建议核查 — 存在不确定性，但可能正确
    NeedsReview,
    /// 检测到幻觉 — 已修正或标记
    Suspected,
    /// 验证失败 — 无法生成可信内容
    Failed,
}

/// 验证检查项结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check_name: String,
    pub passed: bool,
    pub confidence: f32,           // 0.0 ~ 1.0
    pub detail: String,
    pub evidence: Vec<String>,
}

/// 完整的验证报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub level: VerificationLevel,
    pub checks: Vec<CheckResult>,
    pub overall_confidence: f32,
    pub corrected_output: Option<String>,  // 如果修正过，这里存放修正版
    pub suggested_labels: Vec<String>,     // ["待确认", "需核实数据来源"]
}
```

### 3.3 验证管线 (Pipeline)

```
生成的文本 + 检索结果源
        │
        ▼
┌─────────────────────────────────┐
│ 1. 引用存在性校验                  │ ← 检查所有 [来源:X] 是否真实存在于检索结果中
│    CitationExistenceChecker      │
└────────────┬────────────────────┘
        │
        ▼
┌─────────────────────────────────┐
│ 2. 事实一致性检查                  │ ← NLI 风格：回答是否蕴含于检索文档中
│    FactualConsistencyChecker     │
└────────────┬────────────────────┘
        │
        ▼
┌─────────────────────────────────┐
│ 3. 内部矛盾检测                    │ ← 回答自身是否前后矛盾
│    SelfContradictionChecker      │
└────────────┬────────────────────┘
        │
        ▼
┌─────────────────────────────────┐
│ 4. 不确定性标记                    │ ← 低置信度词检测（"可能""不确定"替代）
│    UncertaintyMarker             │
└────────────┬────────────────────┘
        │
        ▼
┌─────────────────────────────────┐
│ 5. LLM 自一致性验证 (可选)         │ ← 重新生成 -> 对比是否一致
│    SelfConsistencyVerifier       │
└────────────┬────────────────────┘
        │
        ▼
     VerificationReport
```

### 3.4 各校验器的详细设计

#### 3.4.1 引用存在性校验 (CitationExistenceChecker)

**功能**: 解析 LLM 输出中的来源引用标记，验证每个引用的文档/chunk 在检索结果中真实存在。

**输入**: LLM 回答文本 + 检索结果 (`Vec<HybridSearchResult>`)

**方法**:
```
1. 正则匹配回答中的 [来源：xxx.md] 或 [src:N] 标记
2. 对每个引用标记，在检索结果中查找匹配的 title/section_path
3. 找不到的标记 → 标记为疑似幻觉
4. 统计覆盖率 (%) → 回答中有多少事实有来源支撑
```

**输出**: `CheckResult { check_name: "citation_existence", passed, confidence, detail, evidence }`

#### 3.4.2 事实一致性检查 (FactualConsistencyChecker)

**功能**: 判断 LLM 生成的回答是否与检索到的知识库内容在事实上一致，而非矛盾。

**方法 (两种策略)**:
```
策略 A — 基于 chunk 的逐句验证 (精确模式):
  1. 将回答分句
  2. 每个句子与最相关的检索 chunk 局部进行比较
  3. 检查是否出现「知识库不存在的信息」或「与知识库矛盾的信息」

策略 B — LLM 判官 (灵活模式，可选):
  1. 将回答 + 检索源文档一起发给 LLM
  2. Prompt: "以下回答是否完全基于提供的文档？有无编造或矛盾？"
  3. 返回 structured verdict
```

**默认走策略 A**（成本低，无附加 LLM 调用），检出疑似问题时升级到策略 B。

#### 3.4.3 内部矛盾检测 (SelfContradictionChecker)

**功能**: 检测回答自身是否存在逻辑矛盾（如前面说「不支持」后面说「可以配置」）。

**方法**: 将回答分句后两两比较矛盾关系。使用轻量关键词 + NLI 混合策略。

#### 3.4.4 不确定性标记 (UncertaintyMarker)

**功能**: 识别 LLM 在表达不确定、猜测或推测时是否缺少明确的置信度提示。

**检测模式**:
```
- 无具体来源的断言（没有 [来源:X] 后缀的陈述句）
- 使用「可能」「应该」「一般来说」等软化词但未标注可信度
- 回答中包含数据/数字但未注明来源
```
**自动添加**: 在低置信度段落前插入 `🟡 [待确认]` 或 `🔴 [需核实]` 标签。

#### 3.4.5 LLM 自一致性验证 (SelfConsistencyVerifier)

**功能**: 通过多次独立生成并对比结果一致性来评估可信度（可选，按场景配置）。

**方法**:
```
1. 用相同 prompt 和上下文重新生成 2-3 次 (temperature=0.3)
2. 计算语义相似度（提取关键事实后比较）
3. 高度一致 → 高置信度
4. 显著差异 → 低置信度，标注争议点
```

**成本**: 增加 2-3 倍 LLM 调用。仅在**文档生成**和**调研报告**等一次性重要产物的场景启用。

### 3.5 验证管线编排

```rust
pub struct VerificationPipeline {
    checkers: Vec<Box<dyn Checker>>,
    policy: VerificationPolicy,
}

pub trait Checker: Send + Sync {
    fn name(&self) -> &str;
    async fn check(&self, input: &VerificationInput) -> CheckResult;
    fn required_sources(&self) -> Vec<SourceType>;
}

pub struct VerificationInput {
    pub generated_text: String,
    pub retrieved_chunks: Vec<HybridSearchResult>,
    pub query: String,
    pub scenario: ScenarioType,
    pub llm_service: Option<LLMService>,  // 用于策略B和自一致性验证
}

pub enum ScenarioType {
    Chat,
    SearchQA,
    DocGen,
    Research,
    RiskReport,
    KnowledgeCompilation,
}
```

---

## 4. RAG 检索增强 (RAG Enhancement) — 辅助

### 4.1 Cross-Encoder Rerank

在现有 BM25+向量 RRF 融合之后，增加一个跨编码器精排层。

```
用户查询 → Embedding → 向量搜索 (TOP 200)
                                              → RRF 融合 (k=60) → TOP 30 → Cross-Encoder Rerank → TOP 10 → LLM
用户查询 → BM25 → 全文搜索 (TOP 200)
```

**实现**:
- 使用 `fastembed` 的 Cross-Encoder 模型（如 `ms-marco-MiniLM-L-6-v2`，ONNX 格式）
- 对 RRF 融合后的 TOP 30 进行逐对打分
- 返回精排后的 TOP 10 给 LLM
- 延迟增量: ~100-300ms (本地 ONNX 推理)

### 4.2 查询分解 (Query Decomposition)

将复杂问题拆解为多个原子子问题，分别检索后合并结果。

**适用场景**: Chat（rig_agent 的 tool call）和搜索问答

```
输入: "金蝶云星空和 K/3 WISE 的应收管理模块有什么区别？"
         ↓
 分解: ["金蝶云星空应收管理模块功能",
        "K/3 WISE 应收管理模块功能",
        "金蝶云星空 vs K/3 WISE 应收差异"]
         ↓
 分别检索 → 合并去重 → 统一送 LLM
```

**方法**: 用 LLM 对用户查询进行分解（一次小的 LLM 调用），然后将子查询并行检索。

### 4.3 缺失检测与补充检索

在首轮检索后，评估检索结果是否足以回答问题。如果不足，自动触发补充检索。

```
1. 首轮检索 → 检查结果数量/相关性
2. 如果 TOP 3 分数低于阈值 → 缺信息
3. 自动用查询扩展/同义词改写重试
4. 合并两轮结果
```

---

## 5. 场景集成

### 5.1 Chat / Agent 场景

**集成点**: `rig_agent.rs` → `llm_service.rag_query_rig()` 之后

| 阶段 | 措施 | 模式 |
|------|------|------|
| 预处理 | 查询分解（复杂问题） | 可选增强 |
| 检索 | Cross-Encoder Rerank | 增强 |
| 生成中 | 结构化输出模板 + 内联引用强制 | 约束 |
| 生成后 | 引用校验 + 一致性检查 + 不确定性标记 | **必选** |
| 输出 | 置信度标签展示 | 前端 |

### 5.2 搜索问答场景

**集成点**: `search_llm.rs` 命令

| 阶段 | 措施 |
|------|------|
| 检索 | Cross-Encoder Rerank + 缺失检测 |
| 生成后 | 引用校验 + 一致性检查 |
| 输出 | 无引用断言必须标记待确认 |

### 5.3 文档生成场景

**集成点**: `doc_generator.rs` + `template_*.rs`

| 阶段 | 措施 |
|------|------|
| 预处理 | 查询分解 + 多源检索 |
| 生成后 | 引用校验 + 一致性检查 + **自一致性验证** |
| 输出 | 校验报告写入文档元数据 |

### 5.4 调研场景

**集成点**: `research_session.rs` 的 AI 辅助生成

| 阶段 | 措施 |
|------|------|
| 生成后 | 引用校验 + 不确定性标记 |
| 输出 | 每个回答标明依据来源 |

### 5.5 风控报告场景

**集成点**: `risk_control.rs`

| 阶段 | 措施 |
|------|------|
| 生成后 | 引用校验 + 事实一致性检查 |

### 5.6 知识编译场景 (计划中)

**集成点**: Wiki 两步摄入管道

| 阶段 | 措施 |
|------|------|
| 编译中 | 验证 pass 作为第三步 |
| 编译后 | 引用校验 + 一致性检查 + 自一致性验证 |

---

## 6. 数据结构变更

### 6.1 新增 SQLite 表

```sql
-- 验证日志 - 记录每次验证结果，用于分析和调试
CREATE TABLE verification_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scenario TEXT NOT NULL,           -- chat/search/doc_gen/research
    query TEXT,                       -- 用户原始查询
    verification_level TEXT NOT NULL, -- confirmed/needs_review/suspected/failed
    overall_confidence REAL,
    checks_json TEXT NOT NULL,        -- CheckResult 数组的 JSON
    corrected_output TEXT,            -- 修正后的输出（如果有）
    original_output TEXT,             -- 修正前的输出
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 验证缓存 - 避免对相同高频查询重复验证
CREATE TABLE verification_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    query_hash TEXT NOT NULL,          -- SHA256(query + context_hash)
    result_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL
);
```

### 6.2 RAGSource 扩展

```rust
// 在现有 RAGSource 基础上扩展字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGSourceV2 {
    pub title: String,
    pub section_path: Option<String>,
    pub content_snippet: String,
    pub score: f32,
    pub rerank_score: Option<f32>,    // Cross-Encoder 精排分
    pub is_cited: bool,               // 是否被回答引用
    pub chunk_id: i64,                // 关联到 chunks 表
}
```

---

## 7. 置信度展示 (前端)

### 7.1 交互模式

在 Chat / 搜索问答等场景的输出底部，增加置信度指示器：

```
┌──────────────────────────────────────┐
│ 根据金蝶云星空的应收管理模块，系统     │
│ 支持按客户维度进行账龄分析，配置路径：  │
│ 应收管理 → 账龄分析 → 方案设置...     │
│                                      │
│ ─── 验证报告 ─────────── ✅ 已确认 ── │
│ ✓ 引用来源验证通过 (2/2)             │
│ ✓ 与知识库正文一致                   │
│ ✓ 未检测到内部矛盾                   │
│ ────────────────────────────────── │
│ 🟢 高置信度 · 基于知识库 2 篇文档     │
└──────────────────────────────────────┘
```

### 7.2 三种状态

| 状态 | 图标 | 含义 | 建议操作 |
|------|------|------|----------|
| Confirmed | 🟢 | 所有验证通过 | 可直接使用 |
| NeedsReview | 🟡 | 部分验证不确定 | 建议人工核查 |
| Suspected | 🔴 | 检测到可能幻觉 | 必须核查后再使用 |

---

## 8. 实现计划

### Phase 1: 验证层基础设施 (预计 3-5 天)

| 任务 | 文件 | 说明 |
|------|------|------|
| 定义核心类型 | `services/verification/types.rs` | `VerificationLevel`, `CheckResult`, `VerificationReport`, `VerificationInput` |
| 实现引用校验器 | `services/verification/citation.rs` | `CitationExistenceChecker` |
| 实现事实一致性检查 | `services/verification/consistency.rs` | `FactualConsistencyChecker` |
| 实现矛盾检测 | `services/verification/contradiction.rs` | `SelfContradictionChecker` |
| 实现不确定性标记 | `services/verification/uncertainty.rs` | `UncertaintyMarker` |
| 实现验证管线 | `services/verification/pipeline.rs` | `VerificationPipeline` 编排 |
| 注册模块 | `services/mod.rs` | 新增模块注册 |

### Phase 2: 场景集成 (预计 2-3 天)

| 任务 | 说明 |
|------|------|
| Chat/Agent 集成 | 在 `rag_query_rig` 返回后调用验证管线 |
| 搜索问答集成 | 在 `search_llm` 命令中调用 |
| 文档生成集成 | 在 `doc_generator` 生成后调用 |
| 调研场景集成 | 在 AI 辅助回答后调用 |
| 前端展示 | 验证报告渲染组件 |

### Phase 3: RAG 增强 (预计 2-3 天)

| 任务 | 文件 | 说明 |
|------|------|------|
| Cross-Encoder Rerank | `services/rerank.rs` | 集成 fastembed cross-encoder |
| 查询分解 | `services/query_decomposition.rs` | LLM 驱动的查询分解 |
| 缺失检测 | `services/hybrid_search.rs` | 检索质量评估与补充检索 |

### Phase 4: 自一致性验证 + 缓存 (预计 2 天)

| 任务 | 说明 |
|------|------|
| 自一致性验证 | LLM 多轮生成对比 |
| 验证缓存 | 避免重复验证 |
| 验证日志查询 | 分析验证数据分布 |

---

## 9. 权衡与约束

| 决策 | 权衡 |
|------|------|
| 验证层默认走策略 A（基于规则的 chunk 比较） | 速度更快成本更低，但覆盖面不如 LLM 判官；策略 B 作为降级选项 |
| 引用校验仅验证「存在性」，不验证「正确性」 | 存在性可自动验证，正确性需 LLM 判官；分阶段实现 |
| 自一致性验证仅在文档生成场景默认开启 | 成本考虑（2-3x LLM 调用），其他场景可选配置 |
| Cross-Encoder 使用本地 ONNX 模型 | 延迟 100-300ms，无需 GPU；精度略低于云端模型但无额外成本 |
| 验证报告默认在前端折叠展示 | 不干扰主流使用，高级用户可展开查看详情 |

---

## 10. 与现有系统的关系

- **不破坏已有功能**: 验证层作为可插拔组件，配置关闭时行为完全不变
- **不修改现有 prompt**: 验证层独立于 prompt 设计，二者互补
- **验证日志可查询**: 帮助识别高频幻觉模式，指导检索优化
- **验证缓存**: 对相同 query+context 避免重复计算
