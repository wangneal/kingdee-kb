import { test, expect } from "@playwright/test";
import { mockTauriApis } from "./mocks/tauri-mock";

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page);
});

test.describe("Settings page", () => {
  test("should display the settings heading", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("设置", { exact: true }).first()).toBeVisible();
  });

  test("should display LLM config section", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("LLM 配置")).toBeVisible();
    await expect(page.getByText("配置大语言模型 API 连接参数")).toBeVisible();
  });

  test("should display API Key field", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("API Key", { exact: true })).toBeVisible();
    await expect(page.getByPlaceholder("sk-...")).toBeVisible();
  });

  test("should display Endpoint field", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("Endpoint")).toBeVisible();
    await expect(page.getByPlaceholder("https://api.openai.com/v1")).toBeVisible();
  });

  test("should display Model field", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("Model").first()).toBeVisible();
    await expect(page.getByPlaceholder("gpt-4o")).toBeVisible();
  });

  test("should have save and test buttons", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByRole("button", { name: "保存配置" }).first()).toBeVisible();
    await expect(page.getByText("测试连接")).toBeVisible();
  });

  test("should display Embedding model section", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("Embedding 模型")).toBeVisible();
  });

  test("should display storage stats section", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("存储统计")).toBeVisible();
    await expect(page.getByText("文档数")).toBeVisible();
    await expect(page.getByText("分块数")).toBeVisible();
    await expect(page.getByText("数据库", { exact: true })).toBeVisible();
  });

  test("should display not configured warning", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("未配置 API Key，AI 对话功能将不可用")).toBeVisible();
  });

  test("should display desensitization section", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("数据脱敏配置")).toBeVisible();
  });

  test("should display ASR config section", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("语音识别服务配置")).toBeVisible();
  });

  test("should display database backup section", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("整库备份")).toBeVisible();
  });
});
