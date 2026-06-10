import { test, expect } from "@playwright/test";
import { mockTauriApis } from "./mocks/tauri-mock";

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page);
});

test.describe("Import page", () => {
  test("should display the import heading", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByRole("heading", { name: "导入知识" })).toBeVisible();
  });

  test("should display text import section", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByText("粘贴文本导入")).toBeVisible();
  });

  test("should have title input", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByPlaceholder("文档标题")).toBeVisible();
  });

  test("should have content textarea", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByPlaceholder("粘贴文本内容…")).toBeVisible();
  });

  test("should have import button", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByRole("button", { name: "导入" })).toBeVisible();
  });

  test("import button should be disabled when fields are empty", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByRole("button", { name: "导入" })).toBeDisabled();
  });

  test("should display file import section", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByText("文件导入")).toBeVisible();
    await expect(page.getByText("拖拽文件到此处")).toBeVisible();
  });

  test("should have file and folder picker buttons", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByRole("button", { name: "选择文件", exact: true })).toBeVisible();
    await expect(page.getByText("选择文件夹")).toBeVisible();
  });

  test("should display video transcription section", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByText("视频/音频转写")).toBeVisible();
  });

  test("should show whisper model status", async ({ page }) => {
    await page.goto("/import");
    await expect(page.getByText(/Whisper \(本地\)/)).toBeVisible();
  });

  test("should follow the global project instead of showing a local project selector", async ({
    page,
  }) => {
    await page.goto("/import");
    await expect(page.locator("select")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "导入" })).toBeVisible();
  });
});
