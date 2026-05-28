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
    await expect(page.getByText("备份恢复")).toBeVisible();
  });

  test("should display project selector", async ({ page }) => {
    await page.goto("/risk");
    const select = page.locator("select");
    await expect(select).toBeVisible();
  });

  test("should display new project button", async ({ page }) => {
    await page.goto("/risk");
    await expect(page.getByText("新建")).toBeVisible();
  });

  test("should show empty state for scope tab when no project selected", async ({ page }) => {
    await page.goto("/risk");
    await expect(page.getByText("请先在顶部选择或创建一个项目")).toBeVisible();
  });

  test("should switch to health tab", async ({ page }) => {
    await page.goto("/risk");
    await page.getByText("项目健康度").click();
    // Should still show empty state since no project selected
    await expect(page.getByText("请先在顶部选择或创建一个项目")).toBeVisible();
  });

  test("should switch to scripts tab", async ({ page }) => {
    await page.goto("/risk");
    await page.getByText("防身话术库").click();
    await expect(page.getByText("生成防身话术")).toBeVisible();
    await expect(page.getByPlaceholder("如：客户要求在合同范围外增加一个全新的报表模块")).toBeVisible();
  });

  test("should switch to backup tab", async ({ page }) => {
    await page.goto("/risk");
    await page.getByText("备份恢复").click();
    await expect(page.getByText("整库备份与恢复")).toBeVisible();
    await expect(page.getByText("导出整库备份")).toBeVisible();
    await expect(page.getByText("导入整库备份")).toBeVisible();
  });
});
