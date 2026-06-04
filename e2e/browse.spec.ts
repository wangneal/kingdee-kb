import { expect, test } from "@playwright/test"
import { mockTauriApis } from "./mocks/tauri-mock"

test.describe("Browse page", () => {
  test("should display the wiki page list heading", async ({ page }) => {
    await mockTauriApis(page)
    await page.goto("/browse")
    await expect(page.getByRole("heading", { name: "知识页面" })).toBeVisible()
  })

  test("should show empty state when no wiki pages", async ({ page }) => {
    await mockTauriApis(page)
    await page.goto("/browse")
    await expect(page.getByText("暂无 Wiki 页面")).toBeVisible()
  })

  test("should display wiki page count", async ({ page }) => {
    await mockTauriApis(page)
    await page.goto("/browse")
    await expect(page.getByText("0 页")).toBeVisible()
  })

  test("should show placeholder on right panel", async ({ page }) => {
    await mockTauriApis(page)
    await page.goto("/browse")
    await expect(page.getByText("选择一个 Wiki 页面查看内容")).toBeVisible()
  })

  test("should refresh wiki list after text import succeeds", async ({ page }) => {
    await mockTauriApis(page, {
      responses: {
        get_kb_compilation_enabled: true,
        ingest_text: {
          document_id: 1,
          title: "测试文档",
          sha256: "abc123",
          is_duplicate: false,
          chunk_count: 5,
          vector_count: 5,
          kb_analysis_engine: "rust",
        },
      },
      sequences: {
        list_wiki_pages: [
          [],
          [],
          [{ id: 10, slug: "test-doc", title: "测试文档", page_type: "summary" }],
        ],
      },
    })
    await page.addInitScript(() => {
      localStorage.setItem("kingdee_kb_active_project", "1")
    })

    await page.goto("/browse")
    await page.click("text=导入文档", { button: "right" })
    await page.getByRole("menuitem", { name: "导入文档" }).click()
    await page.getByPlaceholder("文档标题").fill("测试文档")
    await page.getByPlaceholder("粘贴文本内容...").fill("这是一段测试知识内容。")
    await page.getByRole("button", { name: "导入" }).click()

    await expect(page.getByRole("button", { name: "测试文档 summary" })).toBeVisible()
    await expect(page.getByText("1 页")).toBeVisible()
  })

  test("should recompile failed kb sources and refresh wiki list", async ({ page }) => {
    await mockTauriApis(page, {
      responses: {
        recompile_failed_kb_sources: {
          retried: 1,
          succeeded: 1,
          failed: [],
        },
      },
      sequences: {
        list_wiki_pages: [
          [],
          [{ id: 20, slug: "retry-doc", title: "重编译文档", page_type: "summary" }],
        ],
      },
    })
    await page.addInitScript(() => {
      localStorage.setItem("kingdee_kb_active_project", "1")
    })

    await page.goto("/browse")
    await page.getByRole("button", { name: "重编译失败项" }).click()

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => Reflect.get(globalThis, "__TAURI_MOCK_CALLS__"))
        const recompileCalls = Reflect.get(calls, "recompile_failed_kb_sources") as
          | Record<string, unknown>[]
          | undefined
        return recompileCalls?.at(-1)?.projectId
      })
      .toBe(1)
    await expect(page.getByText("重编译完成：成功 1/1 项")).toBeVisible()
    await expect(page.getByRole("button", { name: "重编译文档 summary" })).toBeVisible()
  })

  test("should open folder picker with a stable start directory", async ({ page }) => {
    await mockTauriApis(page)
    await page.addInitScript(() => {
      localStorage.setItem("kingdee_kb_active_project", "1")
    })

    await page.goto("/browse")
    await page.click("text=导入文档", { button: "right" })
    await page.getByRole("menuitem", { name: "导入文档" }).click()
    await page.getByRole("button", { name: "选择文件夹" }).click()
    await page.getByRole("button", { name: "点击选择文件夹" }).click()

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => Reflect.get(globalThis, "__TAURI_MOCK_CALLS__"))
        const dialogCalls = Reflect.get(calls, "plugin:dialog|open") as
          | Record<string, unknown>[]
          | undefined
        return dialogCalls?.at(-1)
      })
      .toEqual({
        options: {
          defaultPath: "C:\\Users\\Test\\Documents",
          directory: true,
          title: "选择要导入的文件夹",
        },
      })
  })
})
