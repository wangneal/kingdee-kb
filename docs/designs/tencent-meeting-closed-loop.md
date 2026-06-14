# 腾讯会议 × AI 助手闭环设计

> 版本：v3  
> 状态：待实现  
> 核心修订：明确会议落库、转写、AI 纪要、知识库入库、活动日志都必须遵守项目隔离语义。

---

## 1. 结论

### 1.1 落库要不要关联项目？

**要关联项目。**

KingdeeKB 的核心数据模型是“项目化工作台”：知识库文档、原始资料、调研会话、产物、风险数据都以 `project_id` 隔离。腾讯会议不是单独的全局业务域，它产生的是项目沟通证据、调研材料、会议纪要和待办，因此会议本地落库必须纳入同一套项目边界。

允许存在一种例外：**MCP 同步缓存可以短暂未归属项目**。原因是从腾讯会议拉到的历史会议不一定能立即判断属于哪个项目。未归属会议只允许用于列表展示、搜索、补全和后续手动关联，不允许自动写入项目目录、知识库、产物或活动日志。

### 1.2 AI 生成的会议纪要要不要关联项目？

**必须关联项目。**

会议纪要是项目交付物和项目证据，不是普通聊天回复。只要系统执行“生成会议纪要”“落盘”“同步待办”“入库知识库”“登记产物”中的任一动作，就必须有明确 `project_id`。

项目来源按优先级确定：

1. 用户在自然语言中明确指定项目；
2. 会议记录已经有 `project_id`；
3. 当前前端 `ProjectContext.currentProjectId` 或 Agent 的 `active_project_id`；
4. 仍无法确定时，系统必须询问用户选择项目，不能默认写入任意项目。

---

## 2. 已核对的现状

| 范围 | 现状 | 设计影响 |
|------|------|----------|
| 路由 | `/meetings` 在 `src/App.tsx` 中指向 `src/pages/Meetings.tsx` | 会议管理页是唯一主入口 |
| 项目上下文 | `ProjectContext` 持有当前项目，localStorage key 为 `kingdee_kb_active_project` | 前端会议操作应传入当前项目 |
| Agent 上下文 | `RigAgent::run` 会解析 `active_project_id`，并在系统提示词中注入“当前项目” | 自然语言预约和纪要生成默认归属当前项目 |
| 项目存储 | `projects` 是核心表，其他业务表大多 `project_id NOT NULL REFERENCES projects(id)` | 会议纪要和入库结果不应脱离项目 |
| 原始资料 | `raw_sources.project_id` 非空，按项目唯一标识 | 会议转写作为原始资料入库必须带项目 |
| 产物 | `products.project_id` 非空 | 会议纪要作为产物登记必须带项目 |
| 会议页 | `Meetings.tsx` 已读取 `currentProjectId` 但当前未使用，列表直接拉 MCP | 需要改为“同步 MCP → 本地缓存 → 按项目展示” |
| 腾讯会议 MCP | 已有配置、预约、查询、取消、转写、官方 AI 纪要封装 | 复用现有 MCP 客户端和 Tauri command |
| 视频转写 | 已支持视频转写、入库、可选生成纪要，但纪要 prompt 硬编码 | 应改为同一套纪要生成服务 |
| stakeholder-comms | 技能要求写入 `00_项目管理/会议纪要/` 和活动日志 | 需要由会议闭环提供项目根目录和落盘路径 |

备注：项目规则要求阅读 `docs/superpowers/plans/2026-06-01-kingdeekb-technical-spec.md`，当前仓库中该路径不存在。本文基于实际存在的 `docs/ARCHITECTURE.md`、路由、项目存储、会议页、MCP 客户端、视频转写和技能文件重写。

---

## 3. 设计原则

1. **项目隔离优先**  
   任何会改变项目资产的动作必须带 `project_id`，包括写文件、入库、登记产物、追加活动日志。

2. **缓存与资产分层**  
   会议列表缓存可以未归属；会议转写、纪要和待办是项目资产，必须归属。

3. **单一路径生成纪要**  
   腾讯会议转写、视频导入、手动粘贴转写都调用同一个 `meeting_minutes_service`，避免会议页和视频页各自维护 prompt。

4. **不保留旧兼容路径**  
   项目未发布，重写时直接替换硬编码 prompt 和重复 UI，不保留双协议、旧入口或 feature flag。

5. **先同步本地，再展示和处理**  
   MCP 是外部数据源，本地 SQLite 是应用状态源。页面列表、自动同步、Agent 查询都应读取本地会议表。

---

## 4. 目标用户场景

### 场景 A：自然语言预约会议

```
顾问：明天下午 3 点和 XX 客户开 1 小时需求确认会
AI：已预约腾讯会议，会议号 123 456 789，加入链接：https://...
    已关联到当前项目「XX 集团 ERP 实施」。
```

处理规则：

- Agent 使用当前 `active_project_id`；
- 调腾讯会议 MCP 创建会议；
- 将 MCP 返回结果写入 `meetings.project_id = active_project_id`；
- 如果用户明确说了其他项目，先解析并校验项目存在且未归档。

### 场景 B：同步历史会议

```
顾问：同步最近 30 天腾讯会议
AI：已同步 18 场会议，其中 12 场已自动匹配到当前项目，6 场待关联。
```

处理规则：

- MCP 拉取历史会议；
- 本地 upsert；
- 能根据当前操作上下文或已有关联判断项目时写 `project_id`；
- 无法判断时 `project_id = NULL`，状态为 `unlinked`，只能展示和待关联。

### 场景 C：生成项目会议纪要

```
顾问：把今天下午客户需求确认会整理成会议纪要
AI：已拉取转写并生成纪要。
    纪要已保存到：00_项目管理/会议纪要/2026-06-14_客户需求确认会.md
    待办 3 条已同步到活动日志。
```

处理规则：

- 先定位会议；
- 如果会议无 `project_id`，使用当前项目并提示确认，或要求用户选择项目；
- 拉转写；
- 调统一纪要生成服务；
- 写入 `meeting_minutes`、项目目录、`raw_sources`、`products`、活动日志。

### 场景 D：会议结束自动生成纪要

```
系统：检测到当前项目 2 场会议已结束且有转写，已生成纪要并写入项目目录。
```

处理规则：

- 定时任务只处理 `project_id IS NOT NULL` 的 ended 会议；
- `project_id IS NULL` 的 ended 会议不自动生成纪要，只在 Home/Meetings 提示“待关联”；
- 单场失败不影响其他会议。

### 场景 E：视频导入生成纪要

```
操作：在当前项目导入会议录像，勾选“自动生成会议纪要”
系统：视频转写入库 → 统一纪要生成服务 → 项目目录落盘 → 产物登记
```

处理规则：

- 视频导入入口已有 `projectId` 参数；
- 不再走 `video_transcriber.rs` 的硬编码 prompt；
- 与腾讯会议转写共用 `meeting_minutes_service`。

---

## 5. 数据模型

### 5.1 会议表

```sql
CREATE TABLE IF NOT EXISTS meetings (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id         INTEGER REFERENCES projects(id) ON DELETE SET NULL,
  meeting_id         TEXT NOT NULL UNIQUE,
  meeting_code       TEXT,
  subject            TEXT NOT NULL,
  host_user_id       TEXT,
  invitees_json      TEXT NOT NULL DEFAULT '[]',
  start_time         TEXT NOT NULL,
  end_time           TEXT,
  duration_minutes   INTEGER,
  status             TEXT NOT NULL,
  link_status        TEXT NOT NULL DEFAULT 'unlinked',
  source             TEXT NOT NULL DEFAULT 'tencent_mcp',
  raw_payload_json   TEXT NOT NULL DEFAULT '{}',
  created_at         TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at         TEXT NOT NULL DEFAULT (datetime('now')),
  CHECK (status IN ('scheduled', 'ongoing', 'ended', 'cancelled')),
  CHECK (link_status IN ('linked', 'unlinked', 'ignored')),
  FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_meetings_project_time ON meetings(project_id, start_time);
CREATE INDEX IF NOT EXISTS idx_meetings_status ON meetings(status);
CREATE INDEX IF NOT EXISTS idx_meetings_link_status ON meetings(link_status);
CREATE INDEX IF NOT EXISTS idx_meetings_code ON meetings(meeting_code);
```

说明：

- `project_id` 允许为空，仅表示“同步缓存尚未归属项目”；
- `link_status = linked` 时 `project_id` 必须非空，由服务层校验；
- 会议被取消时保留本地记录，状态置为 `cancelled`。

### 5.2 转写表

```sql
CREATE TABLE IF NOT EXISTS meeting_transcripts (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  meeting_id        INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  project_id        INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  record_file_id    TEXT,
  transcript_text   TEXT NOT NULL,
  transcript_raw    TEXT NOT NULL DEFAULT '{}',
  raw_source_id     INTEGER REFERENCES raw_sources(id) ON DELETE SET NULL,
  fetched_at        TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE(meeting_id)
);

CREATE INDEX IF NOT EXISTS idx_meeting_transcripts_project ON meeting_transcripts(project_id);
```

说明：

- 转写是项目资料，`project_id` 非空；
- 转写可以登记到 `raw_sources`，用于项目源数据管理和后续重编译。

### 5.3 纪要表

```sql
CREATE TABLE IF NOT EXISTS meeting_minutes (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  meeting_id         INTEGER NOT NULL UNIQUE REFERENCES meetings(id) ON DELETE CASCADE,
  project_id         INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  transcript_id      INTEGER REFERENCES meeting_transcripts(id) ON DELETE SET NULL,
  content_md         TEXT NOT NULL,
  official_minutes   TEXT,
  decisions_json     TEXT NOT NULL DEFAULT '[]',
  todos_json         TEXT NOT NULL DEFAULT '[]',
  file_path          TEXT NOT NULL,
  product_id         INTEGER REFERENCES products(id) ON DELETE SET NULL,
  generator          TEXT NOT NULL DEFAULT 'stakeholder-comms',
  model_used         TEXT,
  generated_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_meeting_minutes_project ON meeting_minutes(project_id, generated_at);
```

说明：

- 纪要必须归属项目；
- `file_path` 是项目目录内路径；
- `product_id` 用于在“产物管理”页展示会议纪要。

### 5.4 项目目录约定

会议纪要文件写入当前项目根目录：

```text
{project_root}/
└── 00_项目管理/
    ├── 活动日志.md
    └── 会议纪要/
        └── 2026-06-14_客户需求确认会.md
```

如果当前系统还没有显式项目根目录字段，本次实现应先落到应用数据目录下的项目资产区，并在数据库记录绝对路径；后续项目目录能力补齐后再统一迁移。不要把文件写到仓库根目录。

---

## 6. 服务拆分

### 6.1 `meeting_store.rs`

负责 SQLite 表和 DAO。

关键方法：

```rust
pub fn init(&self) -> Result<(), String>;
pub fn upsert_from_tencent(&self, payload: TencentMeetingUpsert, project_id: Option<i64>) -> Result<i64, String>;
pub fn list(&self, filter: MeetingFilter) -> Result<Vec<Meeting>, String>;
pub fn get(&self, id: i64) -> Result<Option<Meeting>, String>;
pub fn get_by_tencent_id(&self, meeting_id: &str) -> Result<Option<Meeting>, String>;
pub fn link_project(&self, meeting_id: i64, project_id: i64) -> Result<(), String>;
pub fn unlink_project(&self, meeting_id: i64) -> Result<(), String>;
pub fn list_ended_without_minutes(&self, project_id: Option<i64>, limit: u32) -> Result<Vec<Meeting>, String>;
pub fn save_transcript(&self, input: SaveTranscript) -> Result<i64, String>;
pub fn save_minutes(&self, input: SaveMinutes) -> Result<i64, String>;
```

服务层校验：

- `link_project` 必须调用 `ProjectStore::ensure_project_active`；
- `save_transcript` 和 `save_minutes` 必须要求 `project_id`；
- 会议 `project_id` 与转写、纪要 `project_id` 必须一致。

### 6.2 `meeting_minutes_service.rs`

负责统一纪要生成和项目资产写入。

输入：

```rust
pub struct GenerateMeetingMinutesInput {
    pub project_id: i64,
    pub meeting_id: Option<i64>,
    pub title: String,
    pub start_time: Option<String>,
    pub transcript: String,
    pub official_minutes: Option<String>,
    pub source: MeetingMinutesSource,
}
```

输出：

```rust
pub struct GenerateMeetingMinutesOutput {
    pub content_md: String,
    pub decisions_json: String,
    pub todos_json: String,
    pub file_path: String,
    pub product_id: Option<i64>,
    pub raw_source_id: Option<i64>,
}
```

职责：

1. 校验项目存在且未归档；
2. 调 `stakeholder-comms` 或同源提示词生成纪要；
3. 生成规范文件名；
4. 写入 `00_项目管理/会议纪要/`；
5. 将转写登记为 `raw_sources`；
6. 将纪要登记为 `products`；
7. 将待办追加到活动日志；
8. 返回完整路径和数据库关联 ID。

### 6.3 `meeting_sync.rs`

负责自动同步。

流程：

```text
定时触发
  → 拉取 MCP 未开始/进行中/已结束会议
  → upsert 到 meetings
  → 查询 project_id 非空且 ended 且未生成纪要的会议
  → 拉转写
  → meeting_minutes_service.generate()
  → save_transcript + save_minutes
  → emit 前端通知
```

约束：

- 不处理未关联项目的会议；
- 不修改用户手动标记为 `ignored` 的会议；
- 同一会议只生成一次，手动重生成走显式 command。

### 6.4 Agent 工具

新增或改造以下工具：

| 工具 | 项目语义 |
|------|----------|
| `tencent_schedule_meeting` | 参数含 `project_id`，默认当前项目 |
| `tencent_list_meetings` | 默认按当前项目过滤，可请求“全部未关联” |
| `tencent_cancel_meeting` | 取消前必须二次确认 |
| `tencent_get_meeting` | 返回会议和项目归属 |
| `tencent_fetch_transcript` | 若会议未关联项目，先要求选择项目 |
| `generate_meeting_minutes` | 必填 `project_id`，统一生成并落盘 |

Agent 提示词必须强调：

- 用户在当前项目内说“开会”“拉转写”“生成纪要”，默认归属当前项目；
- 不能把未关联会议自动写入默认项目；
- 取消会议属于外部写操作，必须确认；
- 生成纪要后要返回文件路径和项目名。

---

## 7. Tauri API

新增 command：

| Command | 说明 |
|---------|------|
| `sync_tencent_meetings` | 从 MCP 同步会议到本地，可传 `projectId` 作为默认归属 |
| `list_meetings` | 按 `projectId/status/linkStatus/query` 查询本地会议 |
| `get_meeting_with_assets` | 获取会议、转写、纪要、项目归属 |
| `link_meeting_to_project` | 将会议关联到项目 |
| `unlink_meeting_from_project` | 取消项目关联，仅允许未生成纪要或先显式删除资产 |
| `ignore_unlinked_meeting` | 标记未归属会议不再提醒 |
| `fetch_meeting_transcript` | 拉取转写并保存，必填或可解析 `projectId` |
| `generate_meeting_minutes` | 生成纪要并落盘，必填 `projectId` |
| `regenerate_meeting_minutes` | 显式重生成纪要，保留版本或覆盖由产品规则决定 |
| `list_recent_meeting_minutes` | Home 展示最近纪要 |

现有 MCP 透传 command 保留给调试和低层封装，但 UI 和 Agent 不再直接依赖它们完成业务闭环。

---

## 8. 前端改造

### 8.1 `Meetings.tsx`

当前 `/meetings` 是主入口，必须改为本地业务视图：

- 列表读取 `list_meetings({ projectId: currentProjectId })`；
- 提供“同步腾讯会议”按钮，调用 `sync_tencent_meetings({ projectId: currentProjectId })`；
- 新增“未关联会议”筛选；
- 会议详情显示项目归属；
- 未关联会议提供“关联到当前项目”和“选择项目”；
- “同步转写 + 智能纪要”拆为两个清晰动作：
  - “同步转写”
  - “生成项目纪要”
- 生成纪要按钮在无项目归属时禁用并提示先关联；
- 详情展示纪要文件路径、产物 ID、raw source ID。

### 8.2 `Home.tsx`

- 今日会议卡片读取当前项目会议；
- 增加“待关联会议”提醒；
- 增加“最近生成纪要”列表。

### 8.3 `ResearchAssistant.tsx`

- 删除旧的腾讯会议线上转写 UI；
- 保留必要的 localStorage 桥接；
- 提供跳转 `/meetings` 的轻入口；
- 不再独立维护会议 MCP 轮询、状态和生成纪要逻辑。

### 8.4 `Import.tsx`

- 视频导入时已经持有 `projectId`；
- 勾选“自动生成会议纪要”后调用 `generate_meeting_minutes` command；
- 展示统一返回的纪要路径和产物信息；
- 删除前端对硬编码纪要 prompt 的依赖。

---

## 9. 文件和知识库写入规则

### 9.1 会议纪要文件

命名：

```text
YYYY-MM-DD_会议主题.md
```

文件内容至少包含：

```markdown
# 会议纪要：{会议主题}

- 项目：{项目名称}
- 时间：{开始时间} - {结束时间}
- 来源：腾讯会议 / 视频导入 / 手动转写
- 会议号：{meeting_code}

## 结论摘要

## 关键决策

## 待办事项

## 风险与关注点

## 详细记录
```

### 9.2 知识库入库

- 转写全文作为 `raw_sources` 记录；
- 可选将纪要正文作为普通文档入库，便于检索；
- 所有入库必须带同一个 `project_id`；
- `identity` 建议使用 `meeting:{meeting_id}:transcript` 和 `meeting:{meeting_id}:minutes`。

### 9.3 活动日志

待办同步目标：

```text
{project_root}/00_项目管理/活动日志.md
```

如果项目根目录不可用，则先写应用数据目录中的项目资产区，并在 UI 提示“项目目录未绑定”。不能写仓库根目录的 `CLAUDE.md` 或 `AGENTS.md`。

---

## 10. 实施计划

### 阶段 1：项目化会议存储

- 新建 `meeting_store.rs`；
- 在 `AppState` 初始化并注册；
- 新建 meeting commands；
- 前端新增 TS 类型和 wrappers；
- `Meetings.tsx` 切换到本地会议列表。

验收：

- `sync_tencent_meetings` 能拉取并 upsert；
- `/meetings` 按当前项目显示会议；
- 未归属会议单独可见。

### 阶段 2：统一纪要服务

- 新建 `meeting_minutes_service.rs`；
- 改视频转写纪要生成路径；
- 实现转写保存、纪要落盘、产物登记、活动日志追加；
- 移除 `video_transcriber.rs` 硬编码 prompt。

验收：

- 腾讯会议和视频导入生成的纪要结构一致；
- 数据库 `meeting_transcripts`、`meeting_minutes`、`raw_sources`、`products` 关联同一项目；
- 无项目时不能生成纪要。

### 阶段 3：Agent 与自动同步

- 注册腾讯会议 Agent 工具；
- 更新系统提示词项目归属规则；
- 新建 `meeting_sync.rs` 定时任务；
- Home 展示最近纪要和待关联会议。

验收：

- 自然语言预约写入当前项目；
- 自然语言生成纪要能落盘并返回路径；
- 自动同步只处理已关联项目会议；
- 取消会议有二次确认。

### 阶段 4：清理重复入口

- 删除 `ResearchAssistant.tsx` 旧腾讯会议转写 UI；
- 保留桥接和跳转；
- 更新用户文档。

验收：

- 腾讯会议只有 `/meetings` 主入口；
- 调研助手不再维护独立 MCP 状态。

---

## 11. 风险与处理

| 风险 | 处理 |
|------|------|
| MCP 历史会议无法自动判断项目 | 允许未归属缓存，生成资产前必须选择项目 |
| 当前项目与会议真实项目不一致 | 生成纪要前展示项目名；已有关联优先于当前项目 |
| 自动同步误写项目目录 | 自动任务只处理 `project_id IS NOT NULL` 且 `link_status='linked'` |
| 项目根目录未定义 | 写应用数据目录项目资产区，记录绝对路径，不写仓库根 |
| 官方 AI 纪要和本地纪要冲突 | 官方纪要只作为 `official_minutes` 输入和参考，本地纪要以统一服务输出为准 |
| 视频导入改造影响现有流程 | 保持 Tauri command 对外签名，内部替换为统一服务 |

---

## 12. 验收清单

- [ ] 会议同步后能关联到当前项目；
- [ ] 未关联会议不会自动生成纪要；
- [ ] 生成会议纪要必须有 `project_id`；
- [ ] 会议转写保存到 `meeting_transcripts`，且 `project_id` 非空；
- [ ] 会议纪要保存到 `meeting_minutes`，且 `project_id` 非空；
- [ ] 纪要文件写入项目资产目录；
- [ ] 纪要产物登记到 `products`；
- [ ] 转写或纪要可进入 `raw_sources`；
- [ ] 待办写入项目活动日志；
- [ ] `/meetings` 是唯一会议管理主入口；
- [ ] 视频导入和腾讯会议共用同一纪要生成服务；
- [ ] `cargo check` 通过；
- [ ] `npx tsc --noEmit` 通过。

---

## 13. 不在本次范围

| 项 | 原因 |
|----|------|
| 多腾讯会议账号 | 当前产品是单顾问单账号 |
| 日历冲突检测 | 腾讯会议 MCP 不提供完整日历语义 |
| 自动识别客户项目的复杂规则 | 先使用当前项目、显式选择和手动关联 |
| 录制文件下载 | 当前 MCP 封装只覆盖转写和智能纪要 |
| 项目目录绑定能力重构 | 本设计只定义会议闭环如何使用项目目录 |
