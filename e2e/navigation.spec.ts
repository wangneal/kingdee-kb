import { test, expect } from "@playwright/test";
import { mockTauriApis } from "./mocks/tauri-mock";

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page);
});

test.describe("Navigation", () => {
  test("should display sidebar with logo", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("实施顾问AI助手", { exact: true })).toBeVisible();
  });

  test("should have all navigation links", async ({ page }) => {
    await page.goto("/");
    const sidebar = page.locator("aside");
    await expect(sidebar.getByText("概览")).toBeVisible();
    await expect(sidebar.getByText("知识浏览")).toBeVisible();
    await expect(sidebar.getByText("检索")).toBeVisible();
    await expect(sidebar.getByText("AI 对话")).toBeVisible();
    await expect(sidebar.getByText("调研助手")).toBeVisible();
    await expect(sidebar.getByText("风险把控")).toBeVisible();
    await expect(sidebar.getByText("文档生成")).toBeVisible();
    await expect(sidebar.getByText("产物管理")).toBeVisible();
    await expect(sidebar.getByText("导入")).toBeVisible();
    await expect(sidebar.getByText("设置")).toBeVisible();
  });

  test("should navigate to browse page", async ({ page }) => {
    await page.goto("/");
    await page.locator("aside").getByText("知识浏览").click();
    await expect(page).toHaveURL("/browse");
    await expect(page.getByRole("heading", { name: "知识库" })).toBeVisible();
  });

  test("should navigate to search page", async ({ page }) => {
    await page.goto("/");
    await page.locator("aside").getByText("检索").click();
    await expect(page).toHaveURL("/search");
    await expect(page.getByRole("heading", { name: "知识检索" })).toBeVisible();
  });

  test("should navigate to chat page", async ({ page }) => {
    await page.goto("/");
    await page.locator("aside").getByText("AI 对话").click();
    await expect(page).toHaveURL("/chat");
    await expect(page.getByRole("heading", { name: "AI 助手" })).toBeVisible();
  });

  test("should navigate to risk control page", async ({ page }) => {
    await page.goto("/");
    await page.locator("aside").getByText("风险把控").click();
    await expect(page).toHaveURL("/risk");
    await expect(page.getByRole("heading", { name: "双轨风险把控舱" })).toBeVisible();
  });

  test("should navigate to templates page", async ({ page }) => {
    await page.goto("/");
    await page.locator("aside").getByText("文档生成").click();
    await expect(page).toHaveURL("/templates");
    await expect(page.getByRole("heading", { name: "项目阶段" })).toBeVisible();
  });

  test("should navigate to import page", async ({ page }) => {
    await page.goto("/");
    await page.locator("aside").getByText("导入").click();
    await expect(page).toHaveURL("/import");
    await expect(page.getByRole("heading", { name: "导入知识" })).toBeVisible();
  });

  test("should navigate to settings page", async ({ page }) => {
    await page.goto("/");
    await page.locator("aside").getByText("设置").click();
    await expect(page).toHaveURL("/settings");
    await expect(page.getByText("设置", { exact: true }).first()).toBeVisible();
  });

  test("should highlight active navigation item", async ({ page }) => {
    await page.goto("/settings");
    const settingsLink = page.locator("aside a", { hasText: "设置" });
    // The active link should have the blue background class
    await expect(settingsLink).toHaveClass(/bg-\[#1A6BD8\]/);
  });
});
