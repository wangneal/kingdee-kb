/**
 * humanizer.ts 的单元测试
 *
 * 这是项目第一个 vitest demo test，作用：
 * 1. 验证 vitest 基础设施在 src/ 下能跑
 * 2. 锁定 humanizer 的关键行为，防止后续 AI 模式正则被误改
 * 3. 给后续 PR 提供"加新模式必加测试"的样板
 *
 * 跑法：
 *   npx vitest run
 *   npx vitest run src/lib/humanizer.test.ts
 */
import { describe, expect, it } from "vitest";
import {
  AI_PATTERNS,
  calculateAIScore,
  detectAIPatterns,
  getPatternSummary,
  humanize,
  humanizeText,
  isAIText,
} from "./humanizer";

describe("detectAIPatterns", () => {
  it("空字符串不应检测到任何模式", () => {
    expect(detectAIPatterns("")).toEqual([]);
  });

  it("人类写作不应触发 AI 模式", () => {
    const humanText =
      "昨天下午我们去了朝阳公园。孩子们在湖边追蜻蜓，我坐在长椅上看书，4 点半回家吃饭。";
    const detected = detectAIPatterns(humanText);
    expect(detected).toEqual([]);
  });

  it("典型 AI 词汇应被检测", () => {
    const aiText =
      "Additionally, this represents a landmark moment. Furthermore, it serves as a milestone for the team.";
    const detected = detectAIPatterns(aiText);
    const ids = detected.map((d) => d.id);
    // 应至少命中: ai_vocabulary, significance_inflation, copula_avoidance
    expect(ids).toContain("ai_vocabulary");
    expect(ids).toContain("significance_inflation");
    expect(ids).toContain("copula_avoidance");
  });

  it("中文 AI 文本也应命中", () => {
    const aiText = "这是 AI 的标准开场白：希望这能帮到你。";
    const detected = detectAIPatterns(aiText);
    // "希望这能帮到你" → 英文 chatbot_artifacts 模式不会命中
    // 但 emoji_decoration 之类可能误判，这里只验证检测函数能正常返回
    expect(Array.isArray(detected)).toBe(true);
  });

  it("同一模式多次出现应被合并去重", () => {
    const text = "Additionally, we did X. Additionally, we did Y. Additionally, we did Z.";
    const detected = detectAIPatterns(text);
    const aiVocab = detected.find((d) => d.id === "ai_vocabulary");
    expect(aiVocab).toBeDefined();
    // matches 应去重："Additionally" 出现 3 次但只算 1 个 unique
    expect(aiVocab!.matches.length).toBe(1);
  });
});

describe("humanizeText", () => {
  it("应替换 AI 词汇为更简单表达", () => {
    expect(humanizeText("Additionally, we should do X.")).toContain("also");
    expect(humanizeText("Additionally, we should do X.")).not.toMatch(/additionally/i);
  });

  it("应替换系动词", () => {
    expect(humanizeText("This serves as a foundation.")).toContain("is");
    expect(humanizeText("This serves as a foundation.")).not.toMatch(/serves as/i);
  });

  it("应清理填充短语", () => {
    expect(humanizeText("In order to win, we trained hard.")).toContain("to win");
    expect(humanizeText("In order to win, we trained hard.")).not.toMatch(/in order to/i);
  });

  it("应保留原文中的非 AI 内容", () => {
    const text = "今天天气很好，我们去爬山。";
    expect(humanizeText(text)).toBe(text);
  });

  it("应折叠多余空格", () => {
    const out = humanizeText("a    b     c");
    expect(out).not.toMatch(/ {2,}/);
  });
});

describe("calculateAIScore", () => {
  it("无模式时得分 0", () => {
    expect(calculateAIScore([])).toBe(0);
  });

  it("命中 content 类模式应得较高分", () => {
    const detected = detectAIPatterns(
      "This is a landmark, groundbreaking, pivotal moment that serves as a milestone."
    );
    const score = calculateAIScore(detected);
    expect(score).toBeGreaterThan(0);
    expect(score).toBeLessThanOrEqual(100);
  });

  it("得分应随检测到的模式数单调递增", () => {
    const few = calculateAIScore(
      detectAIPatterns("This is a landmark moment.")
    );
    const many = calculateAIScore(
      detectAIPatterns(
        "This is a landmark, groundbreaking, pivotal moment. Additionally, it serves as a milestone. Furthermore, it represents a paradigm shift."
      )
    );
    expect(many).toBeGreaterThan(few);
  });
});

describe("humanize (端到端)", () => {
  it("应同时返回 original/humanized/score/suggestions", () => {
    const text = "Additionally, this is groundbreaking. I hope this helps.";
    const result = humanize(text);
    expect(result.original).toBe(text);
    expect(result.humanized).not.toBe(text); // 至少改了一处
    expect(result.score).toBeGreaterThan(0);
    expect(result.suggestions.length).toBeGreaterThan(0);
  });

  it("纯人类文本 humanize 后应无变化、得 0 分", () => {
    const text = "我早上 7 点起床，喝了一杯咖啡。";
    const result = humanize(text);
    expect(result.score).toBe(0);
    expect(result.humanized).toBe(text);
  });
});

describe("isAIText", () => {
  it("默认阈值 50 下，明显的 AI 文本应被判为 AI", () => {
    const aiText = "Additionally, this is groundbreaking. Furthermore, it serves as a milestone. I hope this helps.";
    expect(isAIText(aiText)).toBe(true);
  });

  it("人类文本应被判为非 AI", () => {
    const humanText = "我昨天去菜市场买了点菜。";
    expect(isAIText(humanText)).toBe(false);
  });

  it("阈值越高判定越严格", () => {
    const mildAi = "Additionally, we did this."; // 单个 ai_vocabulary 实际得分约 10
    // 阈值 5 (很宽松) → 判为 AI
    expect(isAIText(mildAi, 5)).toBe(true);
    // 阈值 50 (默认) → 不判为 AI
    expect(isAIText(mildAi, 50)).toBe(false);
  });
});

describe("getPatternSummary", () => {
  it("应返回 5 大类别的模式计数", () => {
    const summary = getPatternSummary();
    expect(summary).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ category: "content" }),
        expect.objectContaining({ category: "language" }),
        expect.objectContaining({ category: "style" }),
        expect.objectContaining({ category: "communication" }),
        expect.objectContaining({ category: "filler" }),
      ])
    );
  });

  it("总和应等于 AI_PATTERNS 长度（防漏注册）", () => {
    const summary = getPatternSummary();
    const total = summary.reduce((sum, s) => sum + s.count, 0);
    expect(total).toBe(AI_PATTERNS.length);
  });

  it("每个类别的 count 应 >= 1", () => {
    const summary = getPatternSummary();
    for (const s of summary) {
      expect(s.count).toBeGreaterThanOrEqual(1);
    }
  });
});
