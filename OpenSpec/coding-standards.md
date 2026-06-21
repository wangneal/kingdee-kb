# 编码规范

> AI agent 在编写或审查代码时**必须**遵循本文件及引用的规范全文。
> 遇到具体规则 ID 时，到引用的源文件中读取完整描述（含正例/反例/理由）。

---

## 一、Rust 后端规范

> 完整源文件位于 [`./rust-guidelines/safe-guides/`](./rust-guidelines/safe-guides/)，
> 对应线上版 [Rust 编码规范 V1.0 beta](https://rust-coding-guidelines.github.io/rust-coding-guidelines-zh/)。
>
> 规则编号前缀：**P** = 强制（Must），**G** = 建议（Should）。

### 目录结构

```
rust-guidelines/safe-guides/
├── code_style/
│   ├── naming/         # 2.1 命名（P.NAM.01–09, G.NAM.01–02）
│   ├── fmt/            # 2.2 格式（P.FMT.01–16）
│   └── comments/      # 2.3 注释与文档（P.CMT.01–05, G.CMT.01–03）
└── coding_practice/
    ├── consts/         # 3.1 常量（G.CNS.01–05）
    ├── generic/        # 3.2 泛型（P.GEN.01–05, G.GEN.01–02）
    ├── module/         # 3.3 模块（P.MOD.01–02, G.MOD.01–05）
    ├── data-type/      # 3.4 数据类型（P.TYP.01, G.TYP.01–03 及子目录）
    │   ├── bool/       #   布尔（G.TYP.BOL.01–07）
    │   ├── enum/       #   枚举（G.TYP.ENM.01–07）
    │   ├── float/      #   浮点（G.TYP.FLT.01–05）
    │   ├── int/        #   整数（G.TYP.INT.01–03）
    │   ├── struct/     #   结构体（P.TYP.SCT.01–02, G.TYP.SCT.01–03）
    │   ├── array/      #   数组（G.TYP.ARR.01–03）
    │   ├── slice/      #   切片（P.TYP.SLC.01–02）
    │   ├── tuple/      #   元组（G.TYP.TUP.01）
    │   ├── vec/        #   Vec（P.TYP.VEC.01–02, G.TYP.VEC.01）
    │   ├── char/       #   字符（G.TYP.CHR.01–03）
    │   └── ref/        #   引用
    ├── expr/           # 3.5 表达式（G.EXP.01–06）
    ├── control-flow/   # 3.6 控制流程（P.CTF.01–02, G.CTF.01–04）
    ├── strings/        # 3.7 字符串（P.STR.01–05, G.STR.01–05）
    ├── variables/      # 3.? 变量（P.VAR.01–02, G.VAR.01–04）
    ├── fn-design/      # 函数设计（P.FUD.01–02, G.FUD.01–06）
    ├── macros/         # 宏（P.MAC.01–02 及子目录 decl/ proc/）
    ├── traits/         # Traits（P.TRA.01, P.TRA.BLN.01, P.TRA.OBJ.01–02 及 std-builtin/）
    ├── error-handle/   # 3.12 错误处理（P.ERR.01–02, G.ERR.01–02）
    ├── memory/         # 内存管理（P.MEM.LFT.01–02, P.MEM.SPT.01 及子目录）
    ├── threads/        # 并发（P.MTH.LCK.01, P.MTH.LKF.01–02, G.MTH.LCK.01–04）
    ├── async-await/    # 异步（P.ASY.01, G.ASY.01–05）
    ├── security/       # 安全（P.SEC.01, G.SEC.01）
    ├── collections/    # 集合（P.CLT.01, G.CLT.01）
    ├── io/             # I/O（P.FIO.01, G.FIO.01）
    ├── cargo/          # Cargo（P.CAR.01–04, G.CAR.01–04）
    ├── unsafe_rust/    # Unsafe（P.UNS.01–03 及子目录 ffi/ mem/ raw_ptr/ safe_abstract/ union/）
    └── others/         # 其他（G.OTH.01–02）
```

### 项目级补充规则（Rust）

以下规则不在上述公开规范中，是本项目根据实际踩坑总结的约束。

- **禁止 `#![allow(dead_code)]` 全局抑制**：按需在具体字段/函数上加 `#[allow(dead_code)]`，并附注释说明保留原因。
- **凭据存储使用 `keyring`**：密钥、令牌等敏感数据不得以明文 JSON 写入磁盘，统一通过 `keyring` crate 存入操作系统加密存储。
- **错误处理用 `Result<T, String>` 透传给 Tauri 前端**：内部逻辑用 `anyhow::Result`，Tauri command 边界转为 `Result<T, String>`。
- **静默吞掉错误时必须加 `warn!` 日志**：`Err(_) => return None` → `Err(e) => { warn!("描述: {e}"); return None }`。
- **日志使用 `tracing` 宏**：禁止 `println!` / `dbg!` 进入提交。

---

## 二、React 前端规范

> 完整规范见 [React 规范](https://weihongyu12.github.io/web/docs/specification/code/react/)。
> 以下为完整规则清单，每条均为强制规则。

### 2.1 基本约定

| 编号 | 规则 |
|------|------|
| 1.1 | React 组件文件应使用 `.tsx` 扩展名 |
| 1.2 | 优先使用函数声明或函数表达式定义具名组件；匿名组件可用箭头函数 |
| 1.3 | 避免在 JSX 中引入未使用的 `React`（React 17+ 新 JSX 转换无需引入） |

### 2.2 代码风格

| 编号 | 规则 |
|------|------|
| 2.1.1 | JSX 属性值使用双引号；普通 JS/TS 字符串使用单引号；不需要转义的字符串常量直接使用，不用花括号 |
| 2.1.2 | 布尔属性值为 `true` 时省略值：`<Checkbox checked />` 而非 `<Checkbox checked={true} />` |
| 2.1.3 | 没有子元素的组件使用自闭合标签；自闭合标签斜杠前有一个空格 |
| 2.1.4 | 多行 JSX 必须用括号 `()` 包裹 |
| 2.1.5 | 组件多个属性时每个属性占一行；第一个属性不换行；属性使用 2 空格缩进 |
| 2.2 | JSX 花括号内侧不应有空格：`{name}` 而非 `{ name }` |

### 2.3 组件与 Props

| 编号 | 规则 |
|------|------|
| 3.1 | 在函数组件参数中直接解构 `props` |
| 3.2 | 非必需 props 优先使用 TypeScript 可选链和默认参数设置默认值，而非函数体内 `||` |
| 3.3 | 禁止 `...` props 扩散，除非明确为透传（如 UI 基础组件传递原生 HTML 属性） |
| 3.4 | 列表渲染使用稳定且唯一的标识符作为 `key`，禁止数组索引 |
| 3.5 | 组件名称使用 PascalCase |

### 2.4 State、Hooks 与 Compiler

| 编号 | 规则 |
|------|------|
| 4.1 | 只在顶层调用 Hooks，不在循环、条件或嵌套函数中调用 |
| 4.1b | 只在 React 函数组件或自定义 Hooks 中调用 Hooks |
| 4.2 | `useEffect`、`useCallback`、`useMemo` 等必须包含所有外部依赖项 |
| 4.3 | `useState` 采用对称命名：`[name, setName]` |
| 4.4.1 | 保持组件和 Hooks 纯净：禁止在渲染期间修改 State |
| 4.4.2 | Props 和 State 不可变：不要直接修改，用 `setState` 创建新对象或数组 |

### 2.5 性能优化

| 编号 | 规则 |
|------|------|
| 5.1.1 | 避免在 Props 中创建新对象：提取到组件外部或使用 `useMemo` |
| 5.1.2 | 避免在 Props 中创建新数组：提取到组件外部 |
| 5.1.3 | 避免在 Props 中创建新函数：使用 `useCallback` 或提取到组件外部 |
| 5.2 | 不要在另一个组件的渲染函数内定义组件，避免丢失状态 |

### 2.6 可访问性 (a11y)

| 编号 | 规则 |
|------|------|
| 6.1 | 所有 `<img>` 标签必须有 `alt` 属性；装饰性图片可设为空字符串 |
| 6.2 | `<a>` 标签必须有内容和有效 `href`；否则用 `<button>` 代替 |
| 6.3 | 使用有效的 ARIA 属性和 role |
| 6.4 | 具有点击事件的非交互元素应有 `role` 属性并处理键盘事件；优先直接使用 `<button>` 或 `<a>` |

### 2.7 数据请求 (TanStack Query)

| 编号 | 规则 |
|------|------|
| 7.1 | `useQuery` 和 `useMutation` 的 `queryKey` 和依赖项必须是稳定的 |
| 7.2 | `QueryClient` 实例应在应用顶层创建一次，通过 Context 提供 |
| 7.3 | 查询函数 `queryFn` 必须返回 `Promise` |

### 2.8 安全性

| 编号 | 规则 |
|------|------|
| 8.1 | 避免 `dangerouslySetInnerHTML`；如必须使用，确保内容经过严格消毒 |
| 8.2 | 禁止在 `href` 等属性中使用 `javascript:` 协议 |
| 8.3 | `target="_blank"` 必须同时添加 `rel="noopener noreferrer"` |

### 项目级补充规则（React / TypeScript）

- **共享工具函数提取到 `src/lib/utils.ts`**：两处以上使用的逻辑不内联在组件中。
- **首页组件保持静态 import**：`Home` 页面不用 `React.lazy()`，避免首屏闪烁。
- **Context 中的共享可变单例必须改为工厂函数**：`DEFAULT_SLOT` → `createDefaultSlot()`，防止多消费者共享同一引用。
