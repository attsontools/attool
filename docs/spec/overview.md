# 概述

## 是什么

attool 是一个个人使用的桌面工具集合，运行在 Tauri 2 上。前端 Vue 3 + Naive UI，后端 Rust。所有数据本地存储（rusqlite），不联网（除工具自身职责如下载，以及自研 updater 检查 GitHub Releases）。

## 当前应用形态

平台保留现有内置工具，并新增固定的“应用管理”入口。前端在 `src/App.vue` 注册 `builtinTools[]`，多数内置工具按需加载；运行时再合并已安装且启用的插件应用，无需重新安装或重启平台。

### 应用管理与独立应用

- “应用管理”（`id: app-management`）位于侧栏“平台”分组，展示内置工具和已安装插件，支持本地 `.atpkg` 预览、权限确认、安装、停用、启用、卸载、筛选和安装历史。
- 平台只接受签名有效、文件 hash 完整、target 匹配且 Host API 兼容的包。Host 校验插件 entry 后把它装入 opaque `srcdoc` sandbox，签名静态资源由 `attool-plugin://<appId>` 提供；公共桌面能力只能通过版本化 Host API 调用。
- Codec、Text、Generator、Time 与 JSON 均为从平台完全外置的双制品应用：`atcodec`、`attext`、`atgenerator`、`attime`、`atjson` 仓库分别生成独立 Tauri 安装包和对应插件包。平台不再包含五者的内置实现，安装的插件是唯一入口。
- 官方 Catalog 由 AT Tool 主仓维护并签名，平台可按当前 OS/架构远程安装和主动升级多个官方应用；新版本先进入 `pending`，健康检查与原子切换通过后成为 `current`，失败时保留或恢复上一可用版本。
- HTTP 已完全外置为 `com.attson.http`：`athttp` 独立版和官方 Catalog 插件复用同一 UI、请求核心与 `${app_data_dir}/http/http.sqlite3`，由单一 Native/Data Service 提供请求、长连接和数据 API。平台只保留 Host API 2.x 桥接和 service supervisor，不再包含内置 HTTP UI、Store、Tauri commands 或 SQLite owner。
- 插件平台支持页面关闭后继续连接的 `background` Native Service，以及由 manifest 声明、宿主隔离承载的底部面板和居中对话框辅助窗口。后台连接失败不会阻止平台启动，应用入口和管理页会显示稳定错误码与关联 ID。

下面详列两个“重后端”内置工具与 HTTP 独立应用，其余见汇总表。

### Aria2 下载（`id: aria2`）

- 使用正式包内置的 Aria2 1.37.0，支持单服务器多连接、文件分片、批量 URL 提交、实时进度回传；最终用户无需安装 `aria2c`
- 任务状态：`queued | running | completed | failed | cancelled`
- 进度通过 Tauri event `download-progress` 流式推送给前端；任务历史存 SQLite
- Tauri 命令：`start_download` / `cancel_download` / `list_download_tasks` / `open_download_folder` 等
- 该工具直接内嵌在 `App.vue`，是"表单 + 列表"形态的参考实现

### 主图模板（`id: template`）

- PSD 导入（正式包内置 `psd-bridge` sidecar，包含 Python 运行时与 `psd-tools`）
- 图层支持图片 / 文本 / 形状 / 组，可拖拽、改尺寸、重排序
- 字段占位：图层名标 `{{field_name}}` 后，批量任务中按行替换
- 批量任务：对单个图层注入多组变体，按笛卡尔展开生成所有结果，渲染成 PNG
- 前端 `src/components/ecommerce/TemplateTool.vue`（下挂 5 子组件），后端 `src-tauri/src/ecommerce/`

### HTTP 请求（独立应用 `com.attson.http`）

- 通过官方 Catalog 安装；启用后动态工具 ID 为 `plugin:com.attson.http`，停用或卸载后入口消失，平台不提供内置回退或自动安装。
- 提供 HTTP / SSE / WebSocket、多 tab、历史、环境变量、集合、OpenAPI、cURL、multipart 与响应预览；具体 UI、请求核心和业务测试由 `attsontools/athttp` 维护。
- 插件 iframe 使用 Host API 2.x 的 `host.http.request`、`host.http.stream`、`host.http.data` 权限连接 Native/Data Service。平台只负责包验签、权限、service 启动/复用、请求代理和事件转发。
- service 独占 `${app_data_dir}/http/http.sqlite3`，沿用升级前数据和迁移备份。平台启动不打开、迁移、清理或删除该数据库。
- SSE / WebSocket 消息只驻 service 内存 buffer（2000 条 / 5 MB），不写历史数据库；关闭 tab 后清理。
- 历史持久值 `attool.lastTool=http` 会映射到动态应用 ID；目标应用不可用时留在首页，不静默恢复旧实现。

### 其余工具

| 工具（id） | 能力 | 后端 |
|---|---|---|
| 剪贴板（`clipboard`） | 历史记录、图片/文本预览、Paste 风格恢复、快捷键自定义、独立浮窗；`atclipboard` 双制品和原位接管已通过本地签名包与真实旧库验证，生产 Catalog 替代前仍保留受 owner guard 约束的内置实现 | `atclipboard-service`（单一 owner）+ 宿主 `clipboard/`（兼容 guard，待退场） |
| 视频链接抽取（`douyin`） | 抖音 / B站 / 小红书 / YouTube 链接与文案解析；YouTube 使用内置 yt-dlp + EJS + QuickJS | `douyin/bilibili/xhs/youtube.rs` |
| 图片（`image`） | 统一捕获工作台（截图 / 录屏，截图选区支持标注与快捷 OCR）、压缩、格式转换、EXIF、OCR；OCR 内置 Tesseract 与中英日语言模型 | `imaging/`（含 xcap 截图）+ `recording.rs` |
| 网络（`network`） | URL 分解、Ping、端口、DNS | `network/` |

## 不在范围内

- 联网同步 / 多设备 / 账号体系
- 浅色/深色之外的多主题 / 国际化
- 移动端 / Web 端
- HTTP 工具的代码生成、脚本、cookie jar
- OpenAPI YAML / Swagger 2.0 完整兼容、复杂 schema 示例生成
- SSE / WS 消息落库回放、自动重连策略、WS 二进制**发送**、WS 自定义 subprotocol 协商细节（`subprotocols` 只做透传）

## 平台支持

通过 GitHub Actions 矩阵构建（`.github/workflows/build.yml`）：

| 平台 | Runner | Target | 用户下载 | Updater 归档 |
|---|---|---|---|---|
| macOS Apple Silicon | `macos-14` | `aarch64-apple-darwin` | `_arm64.dmg` | `_arm64.app.tar.gz` |
| macOS Intel | `macos-14`（cross-compile） | `x86_64-apple-darwin` | `_amd64.dmg` | `_amd64.app.tar.gz` |
| Linux x64 | `ubuntu-24.04` | `x86_64-unknown-linux-gnu` | `_amd64.deb` | `_amd64.tar.gz` |
| Linux ARM64 | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `_arm64.deb` | `_arm64.tar.gz` |
| Windows x64 | `windows-latest` | `x86_64-pc-windows-msvc` | `_amd64.exe`（NSIS） | `_amd64.exe.zip` |

> macOS Intel runner（`macos-13`）在 GitHub 公共池容量极差（曾经排队数小时不动），所以 `macos-x64` 改在 `macos-14` 上 cross-compile。不依赖紧缺 runner，5-10 分钟全 matrix 跑完。

打 `v*` tag 自动建 GitHub Release（直接 publish，不再走草稿）；`workflow_dispatch` 手动跑则只上传 14 天有效的 artifact。macOS app bundle 仅做 ad-hoc 签名来封存资源，未做 Developer ID 公证；Windows 未做 Authenticode。用户首次打开仍可能触发 Gatekeeper / SmartScreen。**Updater 归档由自研 Ed25519 密钥统一签名 SHA256SUMS**（由 `ATTOOL_UPDATE_SIGNING_PRIVATE_KEY` 注入），跟 Gatekeeper / SmartScreen 无关。

## 运行时依赖

正式包随主程序内置并按版本释放以下应用运行时，最终用户无需另行安装：

- Tesseract 5.5.3，以及 `eng` / `chi_sim` / `chi_tra` / `jpn` 语言模型
- Aria2 1.37.0
- `psd-bridge` sidecar 自带的 Python、`psd-tools`、yt-dlp 2026.10.21 与 yt-dlp-ejs 0.8.0
- QuickJS-ng 0.17.0，用于 yt-dlp 的 YouTube JavaScript challenge
- Linux Wayland 录屏所需的 GStreamer 插件与 libvpx；GStreamer 1.0 ABI 由 Deb 依赖保证

这些文件释放到用户缓存中的版本化目录；缺失或损坏时从主程序内嵌内容自动重建。正式版不从系统 `PATH` 回退查找上述程序，也不在首次使用时联网下载组件。

仍由系统提供的边界：

- `pkexec`（Linux in-app 升级提权，一般随桌面环境自带）
- PowerShell / `cmd`、`open` / `xdg-open`、`screencapture`、`ping`、桌面 Portal / PipeWire
- 可选的本地 Ollama 服务
- 网络访问 GitHub API（`api.github.com/repos/attson/attool/releases/latest`）和 Releases 静态资源，仅用于应用更新；可在 Settings 中关闭"启动时自动检查更新"
