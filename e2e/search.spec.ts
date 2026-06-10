import { test, expect } from "@playwright/test";
import { mockTauriApis } from "./mocks/tauri-mock";

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page);
});

test.describe("Search page", () => {
  test("should display the search heading", async ({ page }) => {
    await page.goto("/search");
    await expect(page.getByRole("heading", { name: "知识检索" })).toBeVisible();
  });

  test("should have a search input", async ({ page }) => {
    await page.goto("/search");
    const input = page.getByPlaceholder("输入关键词或自然语言问题…");
    await expect(input).toBeVisible();
  });

  test("should have a search button", async ({ page }) => {
    await page.goto("/search");
    const button = page.getByRole("button", { name: "搜索" });
    await expect(button).toBeVisible();
    await expect(button).toBeDisabled();
  });

  test("search button should be enabled when input has text", async ({ page }) => {
    await page.goto("/search");
    const input = page.getByPlaceholder("输入关键词或自然语言问题…");
    const button = page.getByRole("button", { name: "搜索" });
    await input.fill("测试关键词");
    await expect(button).toBeEnabled();
  });

  test("should perform search and show empty result message", async ({ page }) => {
    await page.goto("/search");
    const input = page.getByPlaceholder("输入关键词或自然语言问题…");
    await input.fill("测试搜索");
    await page.getByRole("button", { name: "搜索" }).click();
    await expect(page.getByText("未找到相关结果")).toBeVisible();
  });

  test("should search within the current project from the search page", async ({ page }) => {
    await page.goto("/search");
    await page.getByPlaceholder("输入关键词或自然语言问题…").fill("测试搜索");
    await page.getByRole("button", { name: "搜索" }).click();

    await expect.poll(async () => {
      const calls = await page.evaluate(() => Reflect.get(globalThis, "__TAURI_MOCK_CALLS__"));
      const hybridSearchCalls = Reflect.get(calls, "hybrid_search") as Record<string, unknown>[] | undefined;
      return hybridSearchCalls?.at(-1)?.projectId;
    }).toBe(1);
  });
});
