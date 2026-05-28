import { test, expect } from "@playwright/test";
import { mockTauriApis } from "./mocks/tauri-mock";

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page);
});

test.describe("Home page", () => {
  test("should display the overview heading", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByRole("heading", { name: "概览" })).toBeVisible();
  });

  test("should display stats cards", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("模板数量")).toBeVisible();
    await expect(page.getByText("生成产物", { exact: true })).toBeVisible();
    await expect(page.getByText("知识库文档")).toBeVisible();
  });

  test("should display quick action buttons", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("浏览知识库")).toBeVisible();
    await expect(page.getByText("检索").first()).toBeVisible();
    await expect(page.getByRole("button", { name: "生成文档 使用模板生成实施文档" })).toBeVisible();
    await expect(page.getByRole("button", { name: "AI 对话 基于知识库的智能问答" })).toBeVisible();
    await expect(page.getByRole("button", { name: "调研助手 语音转录 + 会话管理 + 蓝图导出" })).toBeVisible();
    await expect(page.getByRole("button", { name: "风险把控 范围预警 + 项目健康 + 防身话术" })).toBeVisible();
  });

  test("should display recent products section", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("最近产物")).toBeVisible();
    await expect(page.getByText("暂无产物")).toBeVisible();
  });

  test("should navigate to browse page via quick action", async ({ page }) => {
    await page.goto("/");
    await page.getByText("浏览知识库").click();
    await expect(page).toHaveURL("/browse");
  });

  test("should navigate to search page via quick action", async ({ page }) => {
    await page.goto("/");
    await page.getByText("检索").first().click();
    await expect(page).toHaveURL("/search");
  });

  test("should navigate to chat page via quick action", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "AI 对话 基于知识库的智能问答" }).click();
    await expect(page).toHaveURL("/chat");
  });
});
