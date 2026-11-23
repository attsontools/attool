# UI 设计系统

**风格定位：** Mono Dark（默认）+ Light（可切），单一 emerald accent，Linear / Vercel 一类的开发者工具审美。

## Token 系统

所有颜色 / 圆角 / 字号 / 间距通过 CSS 变量定义在 `src/styles/tokens.css`，**禁止在组件里写散装 hex / 圆角值**。

### 颜色（dark / light 一一对应）

| Token | Dark | Light | 用途 |
|---|---|---|---|
| `--bg-base` | `#0A0A0B` | `#FAFAF9` | 应用底色、内容区 |
| `--bg-elevated` | `#131316` | `#FFFFFF` | 面板、卡片、sidebar |
| `--bg-elev-2` | `#18181C` | `#F4F4F5` | hover、次级面板、表单内嵌 |
| `--bg-overlay` | `#1D1D22` | `#FFFFFF` | 模态、下拉、popover |
| `--line` | `#1F1F23` | `#E4E4E7` | 默认 1px 分隔线 |
| `--line-strong` | `#28282D` | `#D4D4D8` | 输入框 / 强分组 |
| `--text` | `#EDEDED` | `#18181B` | 正文 |
| `--text-muted` | `#8B8B8B` | `#71717A` | 元信息 |
| `--text-faint` | `#6A6A6F` | `#A1A1AA` | 占位、面包屑分隔符 |
| `--accent` | `#34D399` | `#10B981` | 主按钮、focus、活跃菜单、进度条 |
| `--accent-hover` | `#4ADE80` | `#059669` | accent hover |
| `--accent-pressed` | `#16A34A` | `#047857` | accent pressed |
| `--accent-fg` | `#0A0A0B` | `#FFFFFF` | accent 上的前景文字 |
| `--accent-soft` | rgba(52,211,153,.18) | rgba(16,185,129,.12) | badge / focus 光晕 / 活跃 sidebar |
| `--warning` | `#F5B94B` | `#D97706` | 警告 |
| `--error` | `#F87171` | `#DC2626` | 错误 |
| `--info` | `#60A5FA` | `#2563EB` | 信息 |
| `--canvas-bg` | `var(--bg-base)` | `#F4F4F5` | 模板编辑器画布 wrap 底色 |
| `--canvas-checker` | `var(--bg-elev-2)` | `#E5E7EB` | 模板编辑器透明区棋盘格 |

切换机制：在 `<html>` 上写 `data-theme="light"`，`[data-theme="light"]` 选择器全量覆写。

### 圆角

| Token | 值 | 用途 |
|---|---|---|
| `--radius-sm` | 4px | 标签内嵌 |
| `--radius` | 6px | 按钮、输入框 |
| `--radius-md` | 8px | 卡片、面板 |
| `--radius-lg` | 12px | 模态、大型 popover |
| `--radius-pill` | 999px | 徽标、tag |

**严禁出现 14 / 18 / 24 px 的 `border-radius`。**

### 排版

```
--font-sans: -apple-system, "SF Pro Text", "PingFang SC", "Inter", "Segoe UI", sans-serif;
--font-mono: "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace;
```

字号阶梯：

| Token | 值 | 用途 |
|---|---|---|
| `--fs-xxs` | 11px | 分组 LABEL、kbd |
| `--fs-xs` | 12px | 元信息、tag、表单 label |
| `--fs-sm` | 12.5px | 表单输入文本 |
| `--fs-md` | 13px | 正文（默认） |
| `--fs-lg` | 15px | 区块标题 |
| `--fs-xl` | 17px | 页面标题 |
| `--fs-2xl` | 22px | 大型展示标题（首页问候） |

**数字字段（速度、ETA、百分比、坐标、尺寸）走 `--font-mono` + `font-feature-settings: "tnum"`。** 使用 `.mono` 或 `.tnum` 工具类，或在 scoped style 里手动声明。

### 阴影 / 动效

- 默认无阴影；层级靠背景明度差表达
- `--shadow-pop` 用于 modal / popover；`--shadow-canvas` 用于模板画布
- `--motion-fast: 120ms ease`（hover / focus）
- `--motion-mid: 180ms ease`（布局过渡如 sidebar 折叠）

## 主题切换

- Composable：`src/composables/useTheme.ts`，注入 `KVStorage` + `ThemeRoot`，默认 `localStorage` + `document.documentElement`
- 状态持久化：`localStorage.attool.theme = 'dark' | 'light'`，默认 `dark`
- Naive UI：`<n-config-provider :theme="..." :theme-overrides="...">` 在 `darkTheme + darkOverrides` 与 `null + lightOverrides` 之间切换
- 入口：sidebar 底栏的太阳/月亮按钮（折叠态隐藏）

## 外壳骨架

```
┌──────────┬────────────────────────────────────────────┐
│  brand   │                                            │
│  ⌘K      │                                            │
│ ● Aria2  │   <slot />                                 │
│ ○ 模板   │                                            │
│ ○ 剪贴板  │                                            │
│ ○ ...    │                                            │
│ v0.1 ☀ ‹ │                                            │
└──────────┴────────────────────────────────────────────┘
```

`AppShell` 用 `height: 100vh` 锁定视口，内容区自身滚动，sidebar 不再随内容一起滚走；工具列表不再分「已就绪 / 规划中」标题，全部工具在一个 `.nav` 容器里连续排列，容器自身可垂直滚动，工具再多也不会撑破外壳或推走底部按钮。宿主不再渲染重复的顶栏，搜索、主题和设置入口统一收在 sidebar；插件应用填满内容区，品牌顶栏由插件仅在 standalone 模式展示。

| 部件 | 文件 | 关键行为 |
|---|---|---|
| AppShell | `src/components/shell/AppShell.vue` | sidebar + banner slot + content slot；外壳 `height: 100vh`，插件内容支持 full-bleed |
| Sidebar | `src/components/shell/Sidebar.vue` | 220 ↔ 56 折叠（`useSidebarState`），无分组标题（v0.8.6+），活跃项 emerald 高亮，⌘K 打开命令面板，底部 theme toggle + collapse toggle |
| Dashboard | `src/components/shell/Dashboard.vue` | 首屏：欢迎语 + 上次使用 + 快速入口；首次启动落在这里，之后恢复 `useLastTool` |
| BrandMark | `src/components/shell/BrandMark.vue` | 26px emerald 方块 "A" |
| ToolIcon | `src/components/shell/ToolIcon.vue` | 内联 SVG 图标库（`download / layout / clipboard / type / wifi / hash`），`stroke="currentColor"` 让色彩随父容器流转 |

## UI 原子组件

| 组件 | 用途 |
|---|---|
| `Panel` | 卡片容器（替代多数 NCard）。带 title slot + right slot + body slot；body 可加 `flush` 去 padding |
| `TaskRow` | Aria2 任务行：标题 + url + 速度/ETA/% + 3px 进度条 + 取消/打开按钮 |
| `Kbd` | 键盘快捷键标签（如 `⌘K`） |

## Naive UI 主题策略

整体 wrapped in `<n-config-provider :theme="naiveTheme" :theme-overrides="naiveOverrides">`，theme module 在 `src/theme/index.ts` 导出 `darkOverrides` 和 `lightOverrides`，分别覆盖：

- `common`（primary / bg / text / border / divider / input / radius / font）
- `Card`（radius / color / borderColor）
- `Button`（radius / height）
- `Input`（radius / color / border / focus 光晕）
- `Tag`（radius=999px）
- `Modal`（color）
- `Slider`（fillColor / railColor / handleColor）

少数复杂组件**不使用 Naive UI**（自写更可控）：sidebar / dashboard / panel / task-row / kbd / brand-mark / tool-icon / layer 系列。

## 多工具通用 UI 模式

平台内置工具复用同一套外壳与原子组件，形成一致的页面骨架：

- **工具页容器**：`Panel`（`title` + 右上 `#right` slot）分区；多子功能的内置工具（图片 / 网络）用顶部 tab 切分 pane。
- **快捷键录制**：图片截图页和录屏页提供"修改 / 恢复默认"按钮 + 键盘录制框（`keydown` 捕获组合键），注册失败时就地红色 alert 提示。剪贴板快捷键与错误状态由独立应用设置页维护。
- **多窗口**：统一捕获浮层（`capture-overlay`，透明全屏，顶部切换截图 / 录屏；截图支持选区 + 标注，录屏支持区域 / 窗口 / 全屏与区域调整）和截图钉图窗（`capture-pin-*`）由宿主持有。插件辅助窗口由签名 manifest 声明为 `bottomPanel` 或 `centeredDialog`；剪贴板 history/preview 也走该通用路径，由宿主决定尺寸、位置、置顶和无边框样式，窗口内容仍由 `PluginFrame` 的主题通知和 opaque iframe 隔离。
- **插件应用容器**：`PluginFrame` 填满宿主工具内容区，不额外套卡片；loading、连接和错误状态由宿主统一呈现。插件内部设计由对应应用仓库维护，但必须通过 Host API 主题通知跟随当前 dark / light 主题。

## 模板编辑器画布

主图模板是工具之一，其编辑器是本项目最复杂的 UI，四列布局：

```
[rail 56px] [resource 220-280] [canvas 弹性] [props 240-320]
```

- rail：图标按钮垂直排列；活跃态走 `accent-soft + accent`
- resource panel：根据 tab（文字 / 图片 / 素材 / 图层 / 模板）切换右侧内容
- canvas：棋盘格透明背景（`--canvas-bg` × `--canvas-checker` 12px 格子）；模板自身 1px line + `--shadow-canvas`
- 选中图层：1.5px emerald 实线 + 3px `--accent-soft` 外发光，四角 8px emerald 小手柄
- 浮动图层 toolbar：`--bg-overlay` + `--shadow-pop`
- props 面板**默认折叠**（`localStorage.attool.template.propsCollapsed`），通过 canvas card header 的 ‹/› 按钮切换；折叠时 grid 从 4 列变 3 列

## 快捷键

**应用内 · 全局**（webview 焦点时）：

| 键位 | 行为 |
|---|---|
| `⌘\` / `Ctrl\` | 切换 sidebar 折叠 |
| `⌘K` / `CtrlK` | 命令面板：搜索内置工具与已启用的独立应用 |

**系统全局**（任意窗口；截图/录屏由宿主注册，剪贴板由 `atclipboard-service` 注册）：

| 默认键位 | 行为 |
|---|---|
| `CommandOrControl+Shift+A` | 截图（触发选区浮层） |
| `CommandOrControl+Shift+R` | 录屏（打开来源选择器；录制中再次按下停止） |
| `CommandOrControl+Alt+V` | 打开剪贴板历史面板 |

宿主启动时注册截图与录屏快捷键；失败信息记录在 `ShortcutRegisterState`，前端通过 `ShortcutErrorNotifier.vue` 和对应工具页提示。剪贴板快捷键由 background service 持有，状态通过 `clipboard.settings.get` 交给独立应用显示，宿主不重复注册。

## 不允许

- 在组件里写散装 hex / `rgba(...)`（必须走 token）
- 出现 14 / 18 / 24 px 的圆角
- 在 `<style scoped>` 里硬编码 dark 色值（让 `[data-theme=light]` 自动接管）
- 引入新图标库 / 字体包（图标走 `ToolIcon.vue` 内联 SVG，字体走系统栈）
