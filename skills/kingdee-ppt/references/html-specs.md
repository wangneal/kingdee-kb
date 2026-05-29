# HTML 输出详细规范

> HTML 是第一公民，PPTX 是可选导出。

## HTML 输出特点

| 特点 | 说明 |
|------|------|
| **单文件** | 全部 CSS/JS 内联，零依赖，可直接浏览器打开 |
| **交互式** | 键盘导航、触摸滑动、动画效果、进度条、导航点 |
| **响应式** | `clamp()` 字号自适应，移动端/投影仪完美适配 |
| **品牌一致** | 与 PPTX 共用相同的品牌色系、版式逻辑、内容密度规范 |
| **可分享** | 支持 Vercel 一键部署（在线 URL）、Playwright PDF 导出 |

---

## Phase H3：HTML 生成流程

**第一步：读取必需文件**

```
Read `html-kingdee-template.md` → HTML 结构 + SlidePresentation 类 + 动画触发
Read `html-kingdee-style.md`    → CSS 自定义属性 + 视口锁定 + 响应式断点
Read `html-kingdee-presets.md`  → 按需加载版式 CSS（只复制使用的版式）
If AI品牌logo标注存在 → 使用 lobe-icons CDN SVG
```

**第二步：生成 HTML 文件**

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{演示标题} | 金蝶</title>
  <style>
    /* 1. CSS 自定义属性（品牌色系） */
    /* 2. 视口锁定基础样式 */
    /* 3. 响应式断点 */
    /* 4. 动画与过渡 */
    /* 5. 卡片通用样式 */
    /* 6. 品牌色语义映射 */
    /* 7. 版式 CSS 类（按需加载） */
  </style>
</head>
<body>
  <div class="progress-bar"></div>
  <div class="nav-dots"></div>
  <!-- 幻灯片内容 -->
  <script>
    /* SlidePresentation 类 */
    /* Intersection Observer */
    /* 键盘/触摸/滚轮导航 */
  </script>
</body>
</html>
```

**第三步：浏览器预览**

```
写入 {主题}.html 文件 → 本地 HTTP 服务预览（可选）→ 用户确认 → 交付文件 / 部署 URL
```

---

## HTML 版式类名对照表

| PPTX 版式编号 | HTML CSS 类名 | 结构示例 |
|--------------|---------------|---------|
| 01 封面页 | `.slide-cover` | `<section class="slide slide-cover">` |
| 02 目录页 | `.slide-toc` | `<section class="slide slide-toc">` |
| 03 章节分隔 | `.slide-section` | `<section class="slide slide-section">` |
| 04 要点列表 | `.slide-bullets` | `<section class="slide slide-bullets">` |
| 05 数据卡片 | `.slide-data-cards` | `.data-card-grid` + `.data-card` |
| 11 数据看板 | `.slide-dashboard` | `.dashboard-grid` + `.dashboard-card` |
| 12 Bento Grid | `.slide-bento` | `.bento-grid` + `.bento-main` + `.bento-card` |
| 19 金字塔 | `.slide-pyramid` | `.pyramid-wrapper` + `.pyramid-top/.middle/.bottom` |
| 20 PDCA | `.slide-pdca` | `.pdca-grid` + `.pdca-cell` + `.color-plan/.do/.check/.act` |
| 21 SWOT | `.slide-swot` | `.swot-grid` + `.swot-cell` |
| 22 黄金圈 | `.slide-golden` | `.golden-wrapper` + `.golden-circles` + `.golden-explain` |
| 25 IPD五看 | `.slide-ipd` | `.ipd-grid` + `.ipd-card` |
| 16 金句 | `.slide-quote` | `<section class="slide slide-quote">` |
| 28 悬浮统计 | `.slide-floating-stats` | `.floating-grid` + `.floating-stat` |

---

## HTML 导航功能

| 操作 | 触发方式 |
|------|---------|
| 上一页 | `←` / `↑` / `PageUp` |
| 下一页 | `→` / `↓` / `PageDown` / `Space` |
| 首页 | `Home` |
| 结尾 | `End` |
| 左滑（移动端） | 下一页 |
| 右滑（移动端） | 上一页 |
| 鼠标滚轮 | 下一页/上一页（300ms 防抖） |

---

## HTML 内容密度规范

| 幻灯片类型 | 最大内容 | 版面占比上限 |
|-----------|---------|------------|
| 标题页（封面） | 1 标题 + 1 副标题 + 元信息 | 50% |
| 内容页 | 1 标题 + 4–6 要点 | 70% |
| 特征网格 | 1 标题 + 最多 6 张卡片 | 80% |
| 数据看板 | 3 个数字卡片 | 75% |

---

## HTML 图标使用（Emoji）

与 PPTX 版一致，使用 Microsoft YaHei 字体渲染 emoji：

```html
<span class="icon">📊</span>  <!-- 数据看板 -->
<span class="icon">⚡</span>  <!-- 快速响应 -->
<span class="icon">🤖</span>  <!-- AI/智能体 -->
<span class="icon">🚀</span>  <!-- 起飞/增长 -->
```

---

## HTML 分享与导出

**本地预览**：`python3 -m http.server 8080 --directory .` 或 `npx serve .`

**Vercel 部署**：`vercel --prod`（输出在线 URL）

**PDF 导出**（动画丢失）：
```bash
node scripts/export-pdf.js {主题}.html output.pdf
```

---

## HTML 模式常见情况

| 情况 | 处理方式 |
|------|---------|
| 用户说「HTML幻灯片」/「网页版PPT」/「交互式演示」 | 自动选择 HTML 输出格式 |
| 用户说「传统PPT」/「PPTX文件」 | 选择 PPTX 输出格式 |
| 用户说「不要动画」 | HTML 中移除 `.animate-*` 类，跳过动画触发 |
| 用户说「不要导航点」 | 移除 `.nav-dots` 容器 |
| 用户要求 PDF 导出 | 警告动画丢失，使用 Playwright 截图合并 |
| 用户要求在线分享 | 使用 Vercel CLI 部署，输出 URL |
| 用户上传已有 .pptx 要求转 HTML | 先提取内容，再走 HTML 流程 |
