import { test, expect } from "@playwright/test";
import { mockTauriApis } from "./mocks/tauri-mock";

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page);
});

test.describe("Chat page", () => {
  test("should display the AI assistant heading", async ({ page }) => {
    await page.goto("/chat");
    await expect(page.getByRole("heading", { name: "AI 助手" })).toBeVisible();
  });

  test("should have a chat input textarea", async ({ page }) => {
    await page.goto("/chat");
    await expect(page.getByPlaceholder("输入问题，或先添加文档/图片附件...")).toBeVisible();
  });

  test("should have a clear chat button", async ({ page }) => {
    await page.goto("/chat");
    await expect(page.getByText("清空对话")).toBeVisible();
  });

  test("should display empty state message", async ({ page }) => {
    await page.goto("/chat");
    await expect(page.getByText("输入问题开始对话")).toBeVisible();
  });

  test("should display LLM not configured warning", async ({ page }) => {
    await page.goto("/chat");
    await expect(page.getByText("LLM 未配置，请先在设置中填写 API Key")).toBeVisible();
  });

  test("should show round count", async ({ page }) => {
    await page.goto("/chat");
    await expect(page.getByText("0 轮对话")).toBeVisible();
  });

  test("send button should be disabled when empty", async ({ page }) => {
    await page.goto("/chat");
    // The send button is the last button in the input bar
    const sendBtn = page.locator('button[type="button"]').last();
    await expect(sendBtn).toBeDisabled();
  });
});
