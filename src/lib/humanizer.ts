/**
 * AI内容去味模块
 * 基于 Wikipedia "Signs of AI writing" 检测 24 种 AI 写作模式
 *
 * 原始实现：humanizer/SKILL.md + README.md
 */

// ─── 类型定义 ──────────────────────────────────────────────

export interface AIPattern {
  id: string
  name: string
  category: "content" | "language" | "style" | "communication" | "filler"
  description: string
  regex: RegExp
  suggestion: string
}

export interface HumanizeResult {
  original: string
  humanized: string
  detectedPatterns: {
    id: string
    name: string
    matches: string[]
  }[]
  score: number // 0-100, 越高越像AI
  suggestions: string[]
}

// ─── 24种AI写作模式 ──────────────────────────────────────────────

export const AI_PATTERNS: AIPattern[] = [
  // Content Patterns (1-6)
  {
    id: "significance_inflation",
    name: "重要性夸大",
    category: "content",
    description: '使用"标志性时刻"、"里程碑"等夸大词汇',
    regex:
      /marking a pivotal moment|landmark|milestone|groundbreaking|revolutionary|transformative|game-changer|paradigm shift/gi,
    suggestion: "使用具体事实替代夸大描述",
  },
  {
    id: "notability_name_dropping",
    name: "名人堆砌",
    category: "content",
    description: "无上下文地列举名人或来源",
    regex:
      /(?:according to|as stated by|experts say|research shows|studies indicate)(?:\s+[^,.]+){0,2}(?:,\s*(?:according to|as stated by|experts say|research shows|studies indicate))+/gi,
    suggestion: "提供具体来源和上下文",
  },
  {
    id: "superficial_ing_analyses",
    name: "肤浅-ing分析",
    category: "content",
    description: "使用symbolizing、reflecting等表面分析",
    regex:
      /symbolizing|reflecting|embodying|representing|signifying|illustrating|demonstrating|showcasing/gi,
    suggestion: "提供更深入的分析",
  },
  {
    id: "promotional_language",
    name: "促销语言",
    category: "content",
    description: "使用夸张的形容词",
    regex:
      /nestled within|nestled in|breathtaking|stunning|magnificent|exquisite|pristine|serene|vibrant|lush|picturesque|captivating|enchanting|mesmerizing/gi,
    suggestion: "使用更中性的描述",
  },
  {
    id: "vague_attributions",
    name: "模糊归因",
    category: "content",
    description: '使用"专家认为"等模糊归因',
    regex:
      /experts believe|it is widely believed|many experts|some experts|it is generally accepted|it is commonly understood/gi,
    suggestion: "提供具体来源",
  },
  {
    id: "formulaic_challenges",
    name: "公式化挑战",
    category: "content",
    description: '使用"尽管面临挑战"等公式化表达',
    regex:
      /despite (?:the )?challenges?|despite (?:the )?obstacles?|despite (?:the )?difficulties?|in the face of adversity|overcoming adversity/gi,
    suggestion: "具体描述挑战和解决方案",
  },

  // Language Patterns (7-12)
  {
    id: "ai_vocabulary",
    name: "AI词汇",
    category: "language",
    description: "使用典型AI词汇",
    regex:
      /\b(additionally|moreover|furthermore|consequently|nevertheless|nonetheless|notably|significantly|importantly|essentially|fundamentally|arguably|undeniably|undoubtedly)\b/gi,
    suggestion: "使用更简单的连接词",
  },
  {
    id: "copula_avoidance",
    name: "系动词回避",
    category: "language",
    description: '用"serves as"替代"is"',
    regex: /serves as|acts as|functions as|operates as|works as|plays the role of/gi,
    suggestion: '直接使用"is"',
  },
  {
    id: "negative_parallelisms",
    name: "否定平行结构",
    category: "language",
    description: '使用"不仅仅是X，而是Y"结构',
    regex:
      /it'?s not just .{1,50}, it'?s|not only .{1,50} but also|more than just .{1,50}, it'?s/gi,
    suggestion: "使用更直接的表达",
  },
  {
    id: "rule_of_three",
    name: "三连法则",
    category: "language",
    description: "强制将观点分三点",
    regex: /(?:\w+,\s*){2,}\w+\s+and\s+\w+|(?:\w+,\s*){2,}\w+\s*,?\s*and\s+\w+/gi,
    suggestion: "根据实际需要决定要点数量",
  },
  {
    id: "synonym_cycling",
    name: "同义词循环",
    category: "language",
    description: "过度使用同义词替换",
    regex:
      /(?:crucial|vital|essential|important|significant|critical|paramount|imperative)(?:\s+\w+){0,5}(?:crucial|vital|essential|important|significant|critical|paramount|imperative)/gi,
    suggestion: "保持术语一致性",
  },
  {
    id: "false_ranges",
    name: "虚假范围",
    category: "language",
    description: "使用无意义的范围描述",
    regex: /from .{1,30} to .{1,30}|ranging from .{1,30} to .{1,30}/gi,
    suggestion: "使用具体数据",
  },

  // Style Patterns (13-18)
  {
    id: "em_dash_overuse",
    name: "破折号滥用",
    category: "style",
    description: "过度使用破折号",
    regex: /—/g,
    suggestion: "减少破折号使用，使用逗号或句号",
  },
  {
    id: "boldface_overuse",
    name: "粗体滥用",
    category: "style",
    description: "过度使用粗体",
    regex: /\*\*[^*]+\*\*/g,
    suggestion: "减少粗体使用",
  },
  {
    id: "inline_header_lists",
    name: "内联标题列表",
    category: "style",
    description: "使用内联标题格式",
    regex: /^(?:\*\*[^*]+\*\*:\s*|#{1,6}\s+.+:\s*)/gm,
    suggestion: "使用更自然的段落结构",
  },
  {
    id: "title_case_headings",
    name: "标题大写",
    category: "style",
    description: "标题使用大写格式",
    regex: /^[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*$/gm,
    suggestion: "使用句子大小写",
  },
  {
    id: "emoji_decoration",
    name: "表情符号装饰",
    category: "style",
    description: "过度使用表情符号",
    regex:
      /[\u{1F600}-\u{1F64F}\u{1F300}-\u{1F5FF}\u{1F680}-\u{1F6FF}\u{1F1E0}-\u{1F1FF}\u{2600}-\u{26FF}\u{2700}-\u{27BF}]/gu,
    suggestion: "减少表情符号使用",
  },
  {
    id: "curly_quotation_marks",
    name: "弯引号",
    category: "style",
    description: "使用弯引号而非直引号",
    regex: /[""'']/g,
    suggestion: "使用直引号",
  },

  // Communication Patterns (19-21)
  {
    id: "chatbot_artifacts",
    name: "聊天机器人痕迹",
    category: "communication",
    description: "使用聊天机器人常用语",
    regex:
      /I hope this helps|I'm happy to help|Let me know if|Feel free to|Don't hesitate to|Please let me know/gi,
    suggestion: "删除这些客套话",
  },
  {
    id: "cutoff_disclaimers",
    name: "截止声明",
    category: "communication",
    description: '使用"细节有限"等声明',
    regex:
      /while details are limited|while information is limited|as of my knowledge cutoff|based on my training data/gi,
    suggestion: "直接提供信息",
  },
  {
    id: "sycophantic_tone",
    name: "谄媚语气",
    category: "communication",
    description: '使用"好问题"等谄媚表达',
    regex:
      /great question|excellent question|that's a wonderful|what a great|that's an interesting/gi,
    suggestion: "直接回答问题",
  },

  // Filler and Hedging (22-24)
  {
    id: "filler_phrases",
    name: "填充短语",
    category: "filler",
    description: "使用冗长短语",
    regex:
      /in order to|due to the fact that|for the purpose of|with regard to|in the event that|in light of the fact that/gi,
    suggestion: "使用更简洁的表达",
  },
  {
    id: "excessive_hedging",
    name: "过度对冲",
    category: "filler",
    description: "使用过多对冲词",
    regex:
      /could potentially possibly|might perhaps|may possibly|it is possible that|there is a chance that/gi,
    suggestion: "使用更确定的表达",
  },
  {
    id: "generic_conclusions",
    name: "通用结论",
    category: "filler",
    description: "使用通用结论",
    regex:
      /the future looks bright|the possibilities are endless|only time will tell|the journey continues|this is just the beginning/gi,
    suggestion: "提供具体结论",
  },
]

// ─── 检测函数 ──────────────────────────────────────────────

export function detectAIPatterns(text: string): HumanizeResult["detectedPatterns"] {
  const detected: HumanizeResult["detectedPatterns"] = []

  for (const pattern of AI_PATTERNS) {
    const matches: string[] = []
    let match: RegExpExecArray | null

    // 重置正则状态
    pattern.regex.lastIndex = 0

    while ((match = pattern.regex.exec(text)) !== null) {
      matches.push(match[0])
      if (!pattern.regex.global) break
    }

    if (matches.length > 0) {
      detected.push({
        id: pattern.id,
        name: pattern.name,
        matches: [...new Set(matches)], // 去重
      })
    }
  }

  return detected
}

export function calculateAIScore(detectedPatterns: HumanizeResult["detectedPatterns"]): number {
  if (detectedPatterns.length === 0) return 0

  // 每个模式类别权重
  const weights: Record<string, number> = {
    content: 5,
    language: 4,
    style: 3,
    communication: 2,
    filler: 1,
  }

  let totalWeight = 0
  let detectedWeight = 0

  for (const pattern of AI_PATTERNS) {
    totalWeight += weights[pattern.category] || 1
  }

  for (const detected of detectedPatterns) {
    const pattern = AI_PATTERNS.find((p) => p.id === detected.id)
    if (pattern) {
      detectedWeight += (weights[pattern.category] || 1) * detected.matches.length
    }
  }

  // 计算得分，最高100
  return Math.min(100, Math.round((detectedWeight / totalWeight) * 100 * 2))
}

// ─── 去味函数 ──────────────────────────────────────────────

export function humanizeText(text: string): string {
  let result = text

  // 1. 替换AI词汇为更简单的替代
  const aiVocabularyReplacements: [RegExp, string][] = [
    [/\badditionally\b/gi, "also"],
    [/\bmoreover\b/gi, "also"],
    [/\bfurthermore\b/gi, "also"],
    [/\bconsequently\b/gi, "so"],
    [/\bnevertheless\b/gi, "but"],
    [/\bnonetheless\b/gi, "but"],
    [/\bnotably\b/gi, ""],
    [/\bsignificantly\b/gi, ""],
    [/\bimportantly\b/gi, ""],
    [/\bessentially\b/gi, ""],
    [/\bfundamentally\b/gi, ""],
    [/\barguably\b/gi, ""],
    [/\bundeniably\b/gi, ""],
    [/\bundoubtedly\b/gi, ""],
  ]

  for (const [regex, replacement] of aiVocabularyReplacements) {
    result = result.replace(regex, replacement)
  }

  // 2. 替换系动词
  result = result.replace(/\bserves as\b/gi, "is")
  result = result.replace(/\bacts as\b/gi, "is")
  result = result.replace(/\bfunctions as\b/gi, "is")
  result = result.replace(/\boperates as\b/gi, "is")

  // 3. 替换模糊归因
  result = result.replace(/\bexperts believe\b/gi, "")
  result = result.replace(/\bit is widely believed\b/gi, "")
  result = result.replace(/\bresearch shows\b/gi, "")

  // 4. 替换填充短语
  result = result.replace(/\bin order to\b/gi, "to")
  result = result.replace(/\bdue to the fact that\b/gi, "because")
  result = result.replace(/\bfor the purpose of\b/gi, "to")
  result = result.replace(/\bwith regard to\b/gi, "about")
  result = result.replace(/\bin the event that\b/gi, "if")
  result = result.replace(/\bin light of the fact that\b/gi, "because")

  // 5. 替换聊天机器人痕迹
  result = result.replace(/\bI hope this helps\.?\s*/gi, "")
  result = result.replace(/\bI'm happy to help\.?\s*/gi, "")
  result = result.replace(/\bLet me know if[^.]*\.\s*/gi, "")
  result = result.replace(/\bFeel free to[^.]*\.\s*/gi, "")
  result = result.replace(/\bDon't hesitate to[^.]*\.\s*/gi, "")

  // 6. 替换谄媚语气
  result = result.replace(/\bgreat question[!.]*\s*/gi, "")
  result = result.replace(/\bexcellent question[!.]*\s*/gi, "")
  result = result.replace(/\bthat's a wonderful[^.]*\.\s*/gi, "")

  // 7. 清理多余空格
  result = result.replace(/\s{2,}/g, " ")
  result = result.replace(/\n{3,}/g, "\n\n")

  return result.trim()
}

// ─── 主函数 ──────────────────────────────────────────────

export function humanize(text: string): HumanizeResult {
  const detectedPatterns = detectAIPatterns(text)
  const score = calculateAIScore(detectedPatterns)
  const humanized = humanizeText(text)

  // 收集建议
  const suggestions: string[] = []
  for (const detected of detectedPatterns) {
    const pattern = AI_PATTERNS.find((p) => p.id === detected.id)
    if (pattern) {
      suggestions.push(`${pattern.name}: ${pattern.suggestion}`)
    }
  }

  return {
    original: text,
    humanized,
    detectedPatterns,
    score,
    suggestions: [...new Set(suggestions)], // 去重
  }
}

// ─── 便捷函数 ──────────────────────────────────────────────

export function isAIText(text: string, threshold: number = 50): boolean {
  const result = humanize(text)
  return result.score >= threshold
}

export function getPatternSummary(): { category: string; count: number }[] {
  const categories: Record<string, number> = {}
  for (const pattern of AI_PATTERNS) {
    categories[pattern.category] = (categories[pattern.category] || 0) + 1
  }
  return Object.entries(categories).map(([category, count]) => ({
    category,
    count,
  }))
}
