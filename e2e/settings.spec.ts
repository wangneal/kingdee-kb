import { test, expect } from "@playwright/test"
import { mockTauriApis } from "./mocks/tauri-mock"

test.beforeEach(async ({ page }) => {
  await mockTauriApis(page)
})

test.describe("Settings page", () => {
  test("should display the settings heading", async ({ page }) => {
    await page.goto("/settings")
    await expect(page.getByText("设置", { exact: true }).first()).toBeVisible()
  })

  test("should display LLM config section", async ({ page }) => {
    await page.goto("/settings")
    await expect(page.getByText("LLM 供应商")).toBeVisible()
    await expect(page.getByText("管理大语言模型供应商配置")).toBeVisible()
  })

  test("should open provider dialog with API Key field", async ({ page }) => {
    await page.goto("/settings")
    await page.getByRole("button", { name: "添加供应商" }).click()
    const providerForm = page.locator("form").first()
    await expect(providerForm.getByText("API Key", { exact: true })).toBeVisible()
    await expect(providerForm.getByPlaceholder("sk-...")).toBeVisible()
  })

  test("should open provider dialog with Endpoint field", async ({ page }) => {
    await page.goto("/settings")
    await page.getByRole("button", { name: "添加供应商" }).click()
    await expect(page.getByText("Endpoint URL")).toBeVisible()
    await expect(page.getByPlaceholder("https://api.openai.com/v1")).toBeVisible()
  })

  test("should open provider dialog with model list field", async ({ page }) => {
    await page.goto("/settings")
    await page.getByRole("button", { name: "添加供应商" }).click()
    await expect(page.getByText("模型列表（每行一个，第一行为默认模型）")).toBeVisible()
  })

  test("should fill provider preset values", async ({ page }) => {
    await page.goto("/settings")
    await page.getByRole("button", { name: "添加供应商" }).click()
    const providerForm = page.locator("form").first()
    await providerForm.locator("select").first().selectOption("deepseek")
    await expect(providerForm.getByLabel("供应商名称")).toHaveValue("DeepSeek")
    await expect(providerForm.locator("select").nth(1)).toHaveValue("openai")
    await expect(providerForm.getByLabel("Endpoint URL")).toHaveValue("https://api.deepseek.com/v1")
    await expect(providerForm.locator("textarea")).toHaveValue(/deepseek-chat/)
  })

  test("should have add and probe buttons", async ({ page }) => {
    await page.goto("/settings")
    await expect(page.getByRole("button", { name: "添加供应商" })).toBeVisible()
    await expect(page.getByRole("button", { name: "多模态检测" })).toBeVisible()
  })

  test("should display Embedding model section", async ({ page }) => {
    await page.goto("/settings")
    await expect(page.getByText("Embedding 模型")).toBeVisible()
  })

  test("should display storage stats section", async ({ page }) => {
    await page.goto("/settings")
    await page.getByRole("button", { name: "数据管理" }).click()
    await expect(page.getByText("存储统计")).toBeVisible()
    await expect(page.getByText("文档数")).toBeVisible()
    await expect(page.getByText("分块数")).toBeVisible()
    await expect(page.getByText("数据库", { exact: true })).toBeVisible()
  })

  test("should display empty provider state", async ({ page }) => {
    await page.goto("/settings")
    await expect(page.getByText("暂无供应商配置")).toBeVisible()
  })

  test("should display desensitization section", async ({ page }) => {
    await page.goto("/settings")
    await page.getByRole("button", { name: "数据管理" }).click()
    await expect(page.getByText("数据脱敏配置")).toBeVisible()
  })

  test("should display ASR config section", async ({ page }) => {
    await page.goto("/settings")
    await page.getByRole("button", { name: "集成服务" }).click()
    await expect(page.getByText("语音识别服务配置")).toBeVisible()
  })

  test("should display database backup section", async ({ page }) => {
    await page.goto("/settings")
    await page.getByRole("button", { name: "数据管理" }).click()
    await expect(page.getByText("整库备份")).toBeVisible()
  })
})
