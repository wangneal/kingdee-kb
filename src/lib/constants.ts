/**
 * 跨模块共享常量。
 *
 * 涉及产品名、品牌色、跨文件 localStorage 键、跨文件 UI 行为时长等
 * 重复出现的字面量必须从此处引用，避免散落在多个文件中导致改名/调整时遗漏。
 *
 * 单文件内部使用的常量（互不引用）仍保持在文件内就近定义。
 *
 * HTML、JSON 配置文件（如 tauri.conf.json、index.html、splashscreen.html）
 * 由于不能 import TS 模块，其中的产品名仍以字面量存在；
 * 修改时请同步更新本文件与以下位置：
 *   - src-tauri/tauri.conf.json（productName / title）
 *   - index.html（<title>）
 *   - public/splashscreen.html（<title> / h1 / 副标题）
 */

// 品牌
export const PRODUCT_NAME = "顾问工作台"
export const PRODUCT_CODE_NAME = "KingdeeKB"

// 侧边栏与主窗口通信用的 localStorage 键（必须保持一致）
export const LS_KEY_SIDEBAR_QUESTION = "kb_sidebar_question"
export const LS_KEY_SIDEBAR_ANSWER = "kb_sidebar_answer"

// 通用 Toast / 状态消息的自动消失时长（毫秒）
export const TOAST_AUTO_DISMISS_MS = 3000
