# 阶段五：知识图谱 + 图检索 — 规格设计

> **版本**: v1.0
> **日期**: 2026-06-02
> **状态**: 确认后进入开发
> **前提**: 50+ wiki_pages 数据（种子数据已就绪）
> **参考**: `docs/superpowers/plans/2026-06-01-kb-refactor-design-decisions.md`

## 设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 图存储 | SQLite 递归 CTE | 规模 ≤5000 节点，无新依赖 |
| wikilink 编辑器 | 独立前端组件 | 不与大纲编辑器耦合 |
| 实现范围 | 全实现，分步走 | wikilink → 图构建 → 图检索 |

---

## 步骤 1：wikilink 编辑器

### 后端

**现有数据**：wiki_pages 表已有 `wikilinks TEXT NOT NULL DEFAULT '[]'` 字段，存储 JSON 数组 `["slug1", "slug2"]`。

**新增 API**：

```rust
// 获取某页面的 wikilink 候选列表（按标题/标签搜索其他页面）
fn search_wikilink_candidates(project: &str, query: &str, limit: i64) -> Vec<WikiPageRef>

// 添加 wikilink（追加 slug 到 wikilinks JSON 数组，去重）
fn add_wikilink(page_id: i64, target_slug: &str) -> Result<WikiPage>

// 移除 wikilink（从数组中删除 slug）
fn remove_wikilink(page_id: i64, target_slug: &str) -> Result<WikiPage>

// 获取 wikilink 目标页面详情（批量查询被引页面的标题/slug/type）
fn get_wikilink_targets(page_id: i64) -> Vec<WikiLinkTarget>

// 获取反向链接（哪些页面引用了当前页面）
fn get_backlinks(project: &str, slug: &str) -> Vec<WikiPageRef>
```

**WikiLinkTarget 结构**：
```rust
pub struct WikiLinkTarget {
    pub slug: String,
    pub title: String,
    pub page_type: String,
    pub page_status: String,
}
```

**WikiPageRef 结构**：
```rust
pub struct WikiPageRef {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub page_type: String,
}
```

### 前端

**独立组件**：`src/components/wiki/WikiLinkEditor.tsx`

布局：
```
┌─ wikilinks ─────────────────────────────────┐
│ 已链接页面：                                    │
│  [x] 总账管理模块蓝图 (blueprint)               │
│  [x] 应收管理 Fit-Gap (fitgap)                │
│  [+ 添加链接]                                  │
│                                               │
│ 反向链接：                                      │
│  被 3 个页面引用                                │
│  [财务管理系统概述] [应付管理...] [预算编制...]  │
└──────────────────────────────────────────────┘
```

交互：
- 点击 `[+ 添加链接]` 弹出搜索框，输入关键词搜索页面
- 搜索结果显示标题 + page_type 标签
- 点击选中后追加到 wikilinks
- 点击 `[x]` 移除链接
- 反向链接区域只读展示

---

## 步骤 2：4 信号知识图谱构建

### 4 信号定义

| 信号 | 数据源 | 权重 | 说明 |
|------|--------|------|------|
| **S1: wikilink** | `wiki_pages.wikilinks` | 1.0 | 用户/LLM 显式定义的页面间引用 |
| **S2: tags 共现** | `wiki_pages.tags` | 0.6 | 共享同一 tag 的页面自动关联 |
| **S3: sources 共源** | `wiki_pages.sources` | 0.4 | 引用同一 raw_source 的页面关联 |
| **S4: 共引关系** | wikilink 反向索引 | 0.3 | A→C 且 B→C，则 A 和 B 间接关联 |

### SQLite 图存储

**新增表**：`knowledge_graph`

```sql
CREATE TABLE knowledge_graph (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project     TEXT NOT NULL,
    source_slug TEXT NOT NULL,           -- 源页面 slug
    target_slug TEXT NOT NULL,           -- 目标页面 slug
    signal      TEXT NOT NULL CHECK(signal IN ('wikilink','tag','source','co_citation')),
    weight      REAL NOT NULL DEFAULT 1.0, -- 信号强度 [0, 1]
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project, source_slug, target_slug, signal)
);

CREATE INDEX idx_kg_source ON knowledge_graph(project, source_slug);
CREATE INDEX idx_kg_target ON knowledge_graph(project, target_slug);
```

### 图构建

```rust
fn build_knowledge_graph(project: &str) -> Result<usize, String>
```

流程：
1. 清空该项目旧图数据
2. **S1 wikilink**: 遍历 wiki_pages，解析 wikilinks JSON → 写入边，weight=1.0
3. **S2 tag 共现**: 遍历 tags，共享 1+ tag 的两两页面 → 写入边，weight=0.6
4. **S3 source 共源**: 遍历 sources，共享同一 source 的两两页面 → 写入边，weight=0.4
5. **S4 co_citation**: 统计被引次数，一个页面同时被 N 个页面引用时，这些引用者两两关联 → weight=0.3

### 增量更新

当 wiki_pages 被创建/更新/删除时，触发对应信号的增量更新：
- wikilink 变更 → 删除旧边 + 插入新边（仅 S1）
- tags/sources 变更 → 重建该页面的 S2/S3 边
- 删除页面 → 清空该页面的所有边

### 图查询

```rust
// 递归查询：从 seed 页面出发，沿边扩展 N 层
fn traverse_graph(project: &str, seed_slug: &str, max_depth: i64, min_weight: f64) -> Vec<GraphPath>

// 获取某页面的邻居（直接相连的页面）
fn get_neighbors(project: &str, slug: &str) -> Vec<GraphNeighbor>

// 统计图信息
fn get_graph_stats(project: &str) -> GraphStats
```

**GraphPath 结构**：
```rust
pub struct GraphPath {
    pub source_slug: String,
    pub target_slug: String,
    pub target_title: String,
    pub depth: i64,
    pub signals: Vec<String>,  // 关联的信号类型列表
    pub combined_weight: f64,  // 组合权重
}
```

---

## 步骤 3：图扩展检索管道

### 检索流程

```
输入: 当前页面的 slug
  → 1. 查询 knowledge_graph 获取 1 跳邻居
  → 2. 按 combined_weight 排序，取 top K
  → 3. 拼接 BM25 关键词检索结果（可选）
  → 4. 输出去重合并的推荐页面列表
```

### API

```rust
// 图扩展检索：给定页面，推荐相关页面
fn graph_expand_search(
    project: &str,
    slug: &str,
    max_depth: i64,     // 默认 2
    max_results: i64,    // 默认 10
    min_weight: f64,     // 默认 0.3
) -> Vec<GraphRecommendation>
```

**GraphRecommendation**：
```rust
pub struct GraphRecommendation {
    pub slug: String,
    pub title: String,
    pub page_type: String,
    pub combined_weight: f64,
    pub depth: i64,
    pub paths: Vec<String>,    // 关联路径说明 ["通过 wikilink 关联", "通过 tag 共现关联"]
    pub matched_signals: Vec<String>,
}
```

### 展示

在 wiki_page 详情页底部添加"相关页面"区域：
```
┌─ 相关页面（图扩展）─────────────────────────────┐
│ 📌 总账管理模块蓝图    (wikilink · 0.95)       │
│ 📌 应收管理 Fit-Gap    (wikilink · 0.82)       │
│ 📌 会计科目体系设计决策  (tag 共现 · 0.60)     │
│ 📌 财务系统参数配置      (共引 · 0.45)          │
└─────────────────────────────────────────────────┘
```

---

## SQLite 递归 CTE 示例

2 层图扩展查询：
```sql
WITH RECURSIVE graph_walk AS (
    -- 种子节点
    SELECT target_slug, 1 AS depth, signal, weight
    FROM knowledge_graph
    WHERE project = ? AND source_slug = ? AND weight >= ?
    UNION
    -- 递归扩展
    SELECT kg.target_slug, gw.depth + 1, kg.signal, kg.weight
    FROM graph_walk gw
    JOIN knowledge_graph kg ON kg.source_slug = gw.target_slug
        AND kg.project = ? AND kg.weight >= ?
    WHERE gw.depth < ?
)
SELECT target_slug, MAX(depth) as depth, 
       GROUP_CONCAT(DISTINCT signal) as signals,
       AVG(weight) as avg_weight
FROM graph_walk
GROUP BY target_slug
ORDER BY avg_weight DESC
LIMIT ?;
```

---

## DB 变更清单

| 操作 | 说明 |
|------|------|
| `CREATE TABLE knowledge_graph` | 新增图边表 |
| `CREATE INDEX idx_kg_source` | 源节点索引 |
| `CREATE INDEX idx_kg_target` | 目标节点索引 |

## 前端新增文件

| 文件 | 说明 |
|------|------|
| `src/components/wiki/WikiLinkEditor.tsx` | wikilink 编辑器组件 |
| `src/components/wiki/WikiLinkSearch.tsx` | wikilink 候选搜索弹窗 |
| `src/components/wiki/GraphRecommendations.tsx` | 图扩展推荐展示组件 |

## 命令注册

| Rust 命令 | 说明 |
|-----------|------|
| `search_wikilink_candidates` | 搜索 wikilink 候选页面 |
| `add_wikilink` | 添加 wikilink |
| `remove_wikilink` | 移除 wikilink |
| `get_wikilink_targets` | 获取 wikilink 目标详情 |
| `get_backlinks` | 获取反向链接 |
| `build_knowledge_graph` | 构建/重建知识图谱 |
| `traverse_graph` | 递归图遍历 |
| `get_neighbors` | 获取直接邻居 |
| `get_graph_stats` | 图统计信息 |
| `graph_expand_search` | 图扩展检索 |
