import { test, expect } from "@playwright/test";
import { mockTauriApis } from "./mocks/tauri-mock";

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page);
});

test.describe("Risk Control page", () => {
  test("should display the heading", async ({ page }) => {
    await page.goto("/risk");
    await expect(page.getByRole("heading", { name: "双轨风险把控舱" })).toBeVisible();
  });

  test("should display all tabs", async ({ page }) => {
    await page.goto("/risk");
    await expect(page.getByText("需求蔓延警报")).toBeVisible();
    await expect(page.getByText("项目健康度")).toBeVisible();
    await expect(page.getByText("防身话术库")).toBeVisible();
    await expect(page.getByText("AI 深度分析")).toBeVisible();
  });

  test("should follow the global project instead of showing a risk project selector", async ({ page }) => {
    await page.goto("/risk");
    await expect(page.getByText("当前项目：默认项目")).toBeVisible();
    await expect(page.locator("select")).toHaveCount(0);
    await expect(page.getByText("新建")).toHaveCount(0);

    const calls = await page.evaluate(() => globalThis.__TAURI_MOCK_CALLS__);
    expect(calls.list_risk_projects).toBeUndefined();
    expect(calls.list_scope_items?.[0]).toEqual({ projectId: 1 });
  });

  test("should load scope tab for the current global project", async ({ page }) => {
    await page.goto("/risk");
    await expect(page.getByText("检查新需求是否超范围")).toBeVisible();
  });

  test("should switch to health tab", async ({ page }) => {
    await page.goto("/risk");
    await page.getByText("项目健康度").click();
    await expect(page.getByText("75/100")).toBeVisible();
  });

  test("should switch to scripts tab", async ({ page }) => {
    await page.goto("/risk");
    await page.getByText("防身话术库").click();
    await expect(page.getByText("生成防身话术")).toBeVisible();
    await expect(page.getByPlaceholder("如：客户要求在合同范围外增加一个全新的报表模块")).toBeVisible();
  });

});
