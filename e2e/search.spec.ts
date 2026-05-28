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
});
