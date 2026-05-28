import { test, expect } from "@playwright/test";
import { mockTauriApis } from "./mocks/tauri-mock";

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page);
});

test.describe("Browse page", () => {
  test("should display the knowledge base heading", async ({ page }) => {
    await page.goto("/browse");
    await expect(page.getByRole("heading", { name: "知识库" })).toBeVisible();
  });

  test("should show empty state when no documents", async ({ page }) => {
    await page.goto("/browse");
    await expect(page.getByText("暂无文档，请先导入")).toBeVisible();
  });

  test("should display document count", async ({ page }) => {
    await page.goto("/browse");
    await expect(page.getByText("0 篇文档")).toBeVisible();
  });

  test("should show placeholder on right panel", async ({ page }) => {
    await page.goto("/browse");
    await expect(page.getByText("选择左侧文档查看内容")).toBeVisible();
  });
});
