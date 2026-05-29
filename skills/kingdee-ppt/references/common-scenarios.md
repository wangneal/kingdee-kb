# 常见情况处理

| 情况 | 处理方式 |
|------|---------|
| 用户未指定输出格式 | 触发 Phase F，询问 HTML vs PPTX，选择后再进入 Phase 0 |
| 用户直接提供完整内容 | 若格式已明确，直接进入 Phase 0 内容分析；否则先走 Phase F |
| 用户需求模糊（如"做个PPT介绍产品"） | 先触发 Phase F 选格式，再触发 Phase 0.5 设计方向顾问 |
| 用户明确指定模型 | 跳过 Phase 0 扫描，直接采用指定模型版式 |
| 用户说「不要思维模型」 | Phase 0 结果全部忽略，按标准版式处理 |
| 用户说「直接生成」/「不用确认」 | 若格式未明确，先触发 Phase F；然后快速模式：Phase 0+1+2+H 合并输出，末尾一个确认问题 |
| 用户只要内容脚本不要文件 | 完成 Phase 2 后停止 |
| 用户说「不要封面/结尾」 | 记住偏好，大纲中删除对应页 |
| 章节超过4个 | 建议合并或设附录，目录页最多4章 |
| 单页内容过多 | 拆为2页，用承接标题「（一）」「（二）」 |
| 检测到4种以上模型 | 保留主要3种，其余降级为标准版式，大纲中注明 |
| 用户上传已有 .pptx | 提取内容后重排为金蝶风格 HTML deck |
| 用户提供数据要求图表 | 收集数据后在 HTML 中用 CSS/Canvas 绘制，PPTX 导出时转为图片 |
| 内容含 AI 品牌/大模型关键词 | 大纲标注 `[logo: slug]`，HTML 中用 lobe-icons SVG |
| 用户说「不要 logo」 | 跳过 logo 拉取，不调用 CDN |
| 用户中途要求切换输出格式 | 若未进入 Phase H，回退到 Phase F 重新选择；若已进入 Phase H，评估调整成本 |
| **OUTPUT_FORMAT=pptx 且用户要求导出 PPTX** | 进入 Phase X，执行 `scripts/export_deck_pptx.mjs` |
| **用户说「可编辑PPTX」/「要PPT文件」** | 设置 OUTPUT_FORMAT=pptx，Phase H 必须按 html2pptx 4 条硬约束执行 |
| **html2pptx 导出失败** | 提示 HTML 不合规，列出具体错误，建议修改 HTML 或输出 PDF |
| **OUTPUT_FORMAT=html 且用户只要 HTML** | 交付 HTML deck，可浏览器演讲或部署 Vercel，跳过 Phase X |
| **用户要 PDF** | 用 Playwright 截图合并导出 PDF（动画丢失），不依赖 OUTPUT_FORMAT |
