import { test, expect } from "@playwright/test";
import { mockTauriApis } from "./mocks/tauri-mock";

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page);
});

test.describe("Templates page", () => {
  test("should display phase sidebar heading", async ({ page }) => {
    await page.goto("/templates");
    await expect(page.getByRole("heading", { name: "项目阶段" })).toBeVisible();
  });

  test("should display template count", async ({ page }) => {
    await page.goto("/templates");
    await expect(page.getByText("3 个模板")).toBeVisible();
  });

  test("should display all project phases in sidebar", async ({ page }) => {
    await page.goto("/templates");
    const sidebar = page.locator("div.w-56");
    await expect(sidebar.getByRole("button", { name: "项目管理" })).toBeVisible();
    await expect(sidebar.getByRole("button", { name: /启动阶段/ })).toBeVisible();
    await expect(sidebar.getByRole("button", { name: /需求阶段/ })).toBeVisible();
    await expect(sidebar.getByRole("button", { name: /方案阶段/ })).toBeVisible();
    await expect(sidebar.getByRole("button", { name: /构建阶段/ })).toBeVisible();
    await expect(sidebar.getByRole("button", { name: /测试阶段/ })).toBeVisible();
    await expect(sidebar.getByRole("button", { name: /上线阶段/ })).toBeVisible();
    await expect(sidebar.getByRole("button", { name: /验收阶段/ })).toBeVisible();
  });

  test("should display template cards", async ({ page }) => {
    await page.goto("/templates");
    await expect(page.getByText("项目章程")).toBeVisible();
  });

  test("should display format badges", async ({ page }) => {
    await page.goto("/templates");
    const badges = page.getByText("docx", { exact: true });
    await expect(badges.first()).toBeVisible();
  });

  test("should be able to click a phase and filter templates", async ({ page }) => {
    await page.goto("/templates");
    // Click on "测试阶段" in the sidebar
    const sidebar = page.locator("div.w-56");
    await sidebar.getByRole("button", { name: /测试阶段/ }).click();
    await expect(page.getByText("问题跟踪表")).toBeVisible();
    // 项目章程 should not be visible (it's in 启动阶段)
    await expect(page.getByText("项目章程")).not.toBeVisible();
  });

  test("should navigate to wizard when template is clicked", async ({ page }) => {
    await page.goto("/templates");
    await page.getByText("项目章程").click();
    await expect(page).toHaveURL(/\/wizard\/tpl_charter/);
  });

  test("wizard should generate document under current project", async ({ page }) => {
    await page.goto("/wizard/tpl_charter");
    await page.getByRole("button", { name: "下一步" }).click();
    await page.getByRole("button", { name: "下一步" }).click();
    await page.getByRole("button", { name: "生成文档" }).last().click();

    await expect.poll(async () => {
      const calls = await page.evaluate(() => Reflect.get(globalThis, "__TAURI_MOCK_CALLS__"));
      const generateDocCalls = Reflect.get(calls, "generate_doc") as Record<string, unknown>[] | undefined;
      const lastCall = generateDocCalls?.at(-1);
      const request = Reflect.get(lastCall ?? {}, "request") as Record<string, unknown> | undefined;
      return request?.project_id;
    }).toBe(1);
  });

  test("wizard should smart fill with current project id", async ({ page }) => {
    await page.goto("/wizard/tpl_charter");
    await page.getByRole("button", { name: "下一步" }).click();
    await page.getByRole("button", { name: "AI 智能填充" }).click();

    await expect.poll(async () => {
      const calls = await page.evaluate(() => Reflect.get(globalThis, "__TAURI_MOCK_CALLS__"));
      const smartFillCalls = Reflect.get(calls, "smart_fill") as Record<string, unknown>[] | undefined;
      const lastCall = smartFillCalls?.at(-1);
      const request = Reflect.get(lastCall ?? {}, "request") as Record<string, unknown> | undefined;
      return request?.project_id;
    }).toBe(1);
  });
});
