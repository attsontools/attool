# 架构

## 总览

```
┌──────────────────────────────────────────────────────────┐
│  Vue 3 SFC（src/）                                       │
│  ├─ App.vue          主路由壳 + theme + Aria2 工具页       │
│  ├─ shell/*          AppShell / Sidebar / Topbar /        │
│  │                   Dashboard / SettingsModal /           │
│  │                   UpdateBanner / ...                    │
│  ├─ ui/*             Panel / TaskRow / StatPill / Kbd     │
│  ├─ <5 工具>/*       image / network / douyin /            │
│  │                   ecommerce / ai                       │
│  │                   （懒加载）                             │
│  ├─ composables/*    useTheme / useSidebarState /         │
│  │                   useUpdater / usePluginStore...        │
│  ├─ plugins/*        应用管理 / PluginFrame / Host API     │
│  ├─ theme/index.ts   Naive UI overrides（dark + light）    │
│  ├─ styles/*.css     tokens + reset + template-editor     │
│  ├─ types/*          Tool / DownloadTask / Template...    │
│  └─ utils/*          ecommerceTemplate / downloadFormat   │
│                                                          │
│           ↕ invoke() / listen()                          │
│                                                          │
│  Rust（src-tauri/src/）                                  │
│  ├─ build.rs         注入 updater 公钥 → OUT_DIR/*.pem     │
│  ├─ lib.rs           run() + Aria2 下载 + command 注册    │
│  ├─ embedded_runtime / bundled_tools  内置运行时释放与定位 │
│  ├─ clipboard/       系统图片剪贴板写入 + capture 浮层     │
│  ├─ imaging/         压缩/转换/EXIF/OCR/截图（xcap）       │
│  ├─ recording.rs    录屏分块缓存 / 保存 / 清理             │
│  ├─ ecommerce/       模板 render / storage / psd_bridge   │
│  ├─ network/         ping / port / dns                    │
│  ├─ updater/         自研更新：check / verify / download / │
│  │                   apply/{mac,win,linux} + embed scripts │
│  ├─ plugin/          .atpkg 验签 / 安装 registry /         │
│  │                   自定义协议 / Host API / service 代理   │
│  └─ *.rs             douyin·bili·xhs·yt                   │
│                                                          │
│           ↕ Command::new / xcap / reqwest                │
│                                                          │
│  内置运行时 / 系统能力                                     │
│  ├─ aria2c / tesseract    内嵌并释放到版本化缓存           │
│  ├─ psd-bridge sidecar    Python + psd-tools + yt-dlp    │
│  ├─ QuickJS / GStreamer plugins  YouTube / Linux 录屏    │
│  ├─ xcap / core-graphics  屏幕截图                        │
│  ├─ gtk-layer-shell  Wayland capture overlay 定位         │
│  ├─ pkexec           Linux 升级提权（updater）             │
│  └─ open / explorer / xdg-open    打开文件夹              │
└──────────────────────────────────────────────────────────┘
```

## 前后端通信

- **请求-响应：** `invoke<T>('command_name', { arg })` （`@tauri-apps/api/core`）
- **流式事件：** Rust `app_handle.emit("event-name", payload)` → 前端 `listen<T>('event-name', cb)`
  - `download-progress`（aria2 输出解析后推送）、`capture-overlay-init` / `capture-failed`（统一捕获浮层）、`capture-overlay-recording-toggle`（录屏快捷键切换 / 停止）、`recording://completed` / `recording://native-error`（录屏结果与原生错误）、`updater://state`（自研 updater 状态机推送）、`plugin://native-service-event`（Native Service 事件统一转发）等
- **错误：** Rust 命令返回 `Result<T, String>`，前端 `try { await invoke(...) } catch (e)` 捕获

## 状态分层

| 层 | 存储 | 例 |
|---|---|---|
| UI session | Vue ref | 当前选中工具、表单值、loading flags（应用退出即清） |
| 浏览器持久 | `localStorage` | sidebar 折叠状态、上次打开工具、主题、属性面板折叠 |
| 应用持久 | rusqlite（Tauri data dir） | 下载任务历史、模板项目 / 素材库、插件 registry 与管理历史 |
| updater 缓存 | 文件（`app_cache_dir/updater/`） | 已下载但未 apply 的 archive stage |
| 临时 | Tauri runtime State | 进行中的下载子进程句柄（`DownloadTasks`）、录屏分块文件 / Wayland 原生录制 session（`RecordingStore`）、Updater 状态机 `UpdaterState`、插件 registry/runtime 与 Native Service supervisor |

HTTP 与剪贴板数据分别位于 `${app_data_dir}/http/http.sqlite3`、`${app_data_dir}/clipboard/clipboard_history.sqlite3`，但不属于宿主 rusqlite 状态。对应 Native/Data Service 独占数据库、迁移和领域状态；平台只把原数据目录交给已验签 service。停用或卸载插件默认保留数据，平台启动不打开、改写或删除两者数据库。

## 可插拔应用

平台支持从本地包或官方 Catalog 安装签名 `.atpkg`。应用可以由独立仓库发布独立桌面安装包，也可以用同一版本和 `appId` 发布平台插件包；`atcodec`、`attext`、`atgenerator`、`attime`、`atjson` 与带原生 service 的 `athttp`、`atclipboard`、`atvideo` 均已完成生产 Catalog 远程发现和生命周期闭环。

### 包与安装状态

- `.atpkg` 是目标 OS/CPU 专用 ZIP，包含 `manifest.json`、`manifest.sig`、许可证、`ui/` 静态资源，以及可选的目标平台 `nativeService`；包内不允许目录条目、重复文件、未声明文件或不安全相对路径。
- manifest schema v1 固定声明 `appId`、SemVer `version`、publisher、target、Host API 范围、UI 入口、权限和每个文件的 SHA-256；`ui.windows` 可额外声明最多四个签名辅助入口及受控 presentation。原生 service 还必须声明与包版本一致的入口、协议版本、文件 hash 和可选的 `onDemand | background` 生命周期，并绑定 Host API 2.x；Unix 安装后所有受签名清单约束的 `native/` payload 权限统一收紧为 `0700`，确保 service 及其 runtime helper 可执行且不向其他用户开放。平台使用与 updater 分离的 Publisher Key 验签，并拒绝未知字段、错误 target、未知权限和不兼容 Host API。
- 单个普通包文件上限为 100 MiB；声明了 `nativeService` 的包仅对 `native/` 下受 manifest hash 约束的 runtime/service 放宽到 128 MiB，其他路径不随之放宽。
- `src-tauri/src/plugin/install.rs` 把包解压到随机 staging，完成入口健康检查后 rename 到 `app_data_dir/plugins/apps/<appId>/<version>/`，再用 SQLite 事务写入 registry 和成功历史。健康检查 operation 同时持久化待安装包的 `nativeService` 元数据，前端重载后仍能为原生 service 插件建立 Host API 2.x 探针；registry 写入失败会删除已激活目录。
- registry 为每个 `appId` 保存不可变版本目录以及 `current / previous / pending` 关系。停用只更新 registry 并隐藏导航；重新启用前校验入口文件 hash；卸载先把程序包和可选数据移入 staging，registry 提交失败时恢复原目录。

### 隔离运行时与 Host API

- `attool-plugin://<appId>/<path>` 自定义协议只服务已启用且在签名清单中声明的文件，每次读取重新校验 SHA-256，并返回严格 CSP、CORS、`nosniff` 和 `no-store`。协议响应自身带无放行项的 CSP `sandbox`，因此被直接导航为文档时也不能执行脚本或发起导航，只作为 opaque iframe 的静态子资源来源。
- Host 先通过 Tauri command 读取并校验 manifest entry HTML，再把 CSP 与指向签名资源目录的 `<base>` 放到所有不可信标记之前，作为 `sandbox="allow-scripts"` iframe 的 `srcdoc`。iframe 不带 `allow-same-origin`，文档 origin 保持 opaque；这同时隔离父 DOM、Vue 状态和 Tauri commands，不能再假设“自定义协议 origin + allow-same-origin”在所有 WebView 平台都能阻止子 frame 获得 Tauri 初始化脚本。
- iframe 与平台通过 `MessageChannel` 上的 JSON-RPC 2.0 通信。纯前端插件继续使用 Host API 1.x；原生 service 插件使用 Host API 2.x，隔离安装探针同样读取 staged entry 为 opaque `srcdoc`，并按 operation 的 `nativeService` 选择相同 API 版本。握手返回 session ID、correlation ID、应用元数据、主题和已批准能力；请求校验 `appId`、API 版本、消息大小、超时和权限。HTTP 业务帧上限为 8 MiB，请求超时覆盖最长 300 秒网络请求并保留 5 秒传输余量。
- 基础权限为 `host.theme.read`、`host.notification.show`、`host.file.save`；通用桌面能力另以 `host.clipboard.write`、`host.external.open` 和 `host.app.activate` 独立授权，不借用 HTTP 权限。`host.ui.auxiliary` 只允许应用打开自己 manifest 已声明的辅助窗口，64 KiB 激活 payload 通过目标窗口的 `host.ui.activated` 通知交付。HTTP 插件另声明 `host.http.request`、`host.http.stream`、`host.http.data`，分别代理一次请求、长连接和 Data API；主图模板插件声明 `host.template.project`，仅能调用模板方法白名单；剪贴板应用声明 `host.clipboard.history`，只开放六个历史方法和两个设置方法。multipart 与模板文件/目录选择只向插件返回会话级不透明 token，复制、保存和文件选择都由 Host 执行；模板 token 绑定类型且只能消费一次，批处理返回和错误响应拒绝泄漏本地绝对路径。切换工具、停用或卸载会销毁 iframe、消息端口和尚未消费的 token。
- `host.app.activate` 只能激活已安装、启用且可加载的其他动态应用。Host 在 acceptance 响应前把最多 64 KiB 的 JSON payload 加入目标队列，响应后才导航，并在目标主页面握手确认后按 FIFO 通过 `host.app.activated` 交付真实 `sourceAppId`；每个目标最多保留 32 项。未响应的请求从队列撤销，已响应但目标加载或通知失败的内容保留到当前进程内下次可用 session，不因源 iframe 销毁而丢失。

### 原生 Service 与共享数据

- `com.attson.http` 的独立版和插件版都是 Native/Data Service 客户端。service 通过权限受限的 endpoint 文件发布临时 loopback 端口和 UUID token，客户端完成 Host/Data/Service API 握手后才能调用业务方法。
- 平台 supervisor 从已安装且签名校验通过的插件版本启动或复用 service；声明 `background` 的已启用应用在平台启动、安装或启用后即连接，不依赖页面是否打开。并发启动者按 app 收敛到同一连接，不同 app 可并行启动；`service.lock` 和 endpoint 竞争收敛到同一实例，过期租约只在确认进程已失效后清理。Host 保留自己启动的子进程句柄，停用、卸载或插件版本切换前先移除客户端并只终止该 owned service；独立版或其他外部进程不被越权终止，版本不匹配时明确失败。连接状态以 `connecting | connected | unavailable | disconnected`、稳定错误码和 correlation ID 暴露给导航与应用管理。
- Native Service 事件按当前已连接的 app 和 service 版本验收，再路由到该应用的主页面及辅助窗口。保留的 `host.ui.open` 事件只激活已签名且已授权的目标窗口，不作为普通业务事件广播；同一 app/window 的并发打开串行化，握手期间只保留最后一次 activation。
- service 对新客户端独占 `${app_data_dir}/http/http.sqlite3`，首次接管任何已有非空数据库时创建 `http.sqlite3.pre-service-v1.bak`，再执行 schema 迁移和完整性检查。迁移失败返回稳定错误，不回退到宿主内置 HTTP。
- 同一客户端连接允许请求并发分派，因此 `http.cancel` 可以中断仍在执行的 `http.send`；响应写入仍串行化，SSE/WS 保持 2000 条、5 MiB buffer 和 generation 语义。`stream.open` response 前到达的事件由客户端按 generation 暂存并在响应后交付，不能因 RPC/event 时序丢失首帧。
- 宿主不包含 HTTP UI、Store、网络命令或数据库连接。停用/卸载插件不会恢复隐藏的内置实现；历史 `lastTool=http` 只映射到已启用的动态应用入口。

`com.attson.template` 的 standalone 与签名 `.atpkg` 复用同一 Vue 编辑器、`attemplate-service` 和独立 `psd-bridge`，固定原位接管 `${app_data_dir}/ecommerce` 中的模板 SQLite、素材和批量缓存；其他未知应用仍使用 `${app_data_dir}/native-services/<appId>`。首次接管要求宿主 capability，service 持有 owner lock，经完整性校验后把 `migrating` 原子切换为永久 `owned`；失败不改用户数据，成功后宿主旧模板 command 与第二个 service 均拒绝访问，停用或卸载测试包也不恢复旧 owner。Host API 与 Rust 网关共同限制 `template.templates.*`、PSD/素材导入及批量注册、渲染、保存方法，文件与目录路径只在 token 解析后进入 service；公共模板、素材、source 和 output DTO 只暴露受限预览与不透明 ID。Linux x86_64 真实测试包已通过签名安装、PSD 导入、编辑保存、两图批量渲染、选中导出、双客户端共享和路径泄漏检查；五目标 workflow、固定 SDK、生产/测试签名隔离、精确十资产和 Catalog 包布局也已在本地验证。生产 Publisher 授权、两个正式 Release、Catalog 安装/升级/回滚生命周期及宿主内置实现删除仍分别留在 P2、P3。

`com.attson.clipboard` 的独立版与插件版复用 `atclipboard-service`，固定原位接管 `${app_data_dir}/clipboard`，并通过 `host.clipboard.history` 的双层白名单访问。service 以生命周期锁和 `migrating / owned` marker 保证单一 owner；旧设置表为空时沿用历史默认值且不改写数据库，部分缺失或非法设置继续 fail closed。公共图片条目不得返回 asset、数据库或缓存路径，预览只接受 PNG/JPEG/WebP base64 data URL 且整串不超过 2 MiB；列表单页最多 100 条。后台采集、三类恢复、暂停、保留策略、清空未收藏、停启、卸载、重装、升级和首启失败回滚已通过两版生产 Catalog 与真实 500 条旧库验证；Wayland 插件模式使用宿主 `--plugin-window` 命令激活签名 history 窗口。宿主旧 UI、SQLite owner、watcher、领域快捷键、commands 和固定窗口已删除；停用或卸载插件不触发宿主回退。

`com.attson.aria2` 已完成本地双制品与发布链验证：独立 Tauri 与签名 `.atpkg` 复用同一 Vue UI 和 `ataria2-service`，service 通过跨平台文件锁独占 `${app_data_dir}/aria2`、SQLite、固定 aria2 1.37.0 runtime 和取消句柄。首次接管先要求宿主写入 capability，再写 `migrating` marker，并以 SQLite exclusive barrier 等待 marker 前的旧写事务完成；一致快照通过 schema、完整性和行数校验后切换为永久 `owned`。旧库保留只读，宿主旧 owner 的所有读写在 marker 存在时拒绝，写路径取得 SQLite write transaction 后还会再次检查，停用或卸载插件也不恢复旧 owner。Linux x86_64 测试包已通过真实安装、下载和共享历史，五目标 workflow、生产签名校验和 Catalog 包布局校验也已在本地完成；生产 Publisher 授权、正式 Release 和安装/升级/回滚生命周期仍待远端验证。

`com.attson.video` 的独立 Tauri 与签名 `.atpkg` 复用同一 Vue 页面、`atvideo-service` 协议和固定的 yt-dlp 2026.10.21、yt-dlp-ejs 0.8.0、QuickJS-ng 0.17.0 runtime bytes。插件只声明 `host.theme.read`、`host.video.extract`、`host.clipboard.write`、`host.external.open`、`host.app.activate`，Host 与 service 共同限制五个视频方法、平台 host、精确参数和无凭证代理；30 秒平台/runtime 超时外由 standalone client 和 Host 各保留 35 秒响应预算。动态 Video 通过 `host.app.activate` 把有界 URL 批次交给动态 Aria2，Aria2 原子填入输入框但不自动创建任务。生产 Publisher、两个五目标 alpha Release、固定 runtime smoke、精确十资产和 Catalog 包布局均已验签；隔离环境已完成安装、升级、首次启动失败回滚、停启、卸载、重装，以及 B 站解析、复制、外链和 Video 到动态 Aria2 回归。宿主内置视频实现和共享 runtime 在 P3 前继续保留。

### 前端装配与应用外置

- `usePluginStore` 从 Tauri registry 加载插件并生成动态 `Tool`；“应用管理”固定在侧栏平台分组，负责本地包预览、权限确认、安装、启停、卸载、筛选和历史查看。
- 平台不再内置 `codec`、`text`、`generator`、`time`、`json`、`http` 与 `clipboard`；对应插件是这些工具在平台中的唯一入口。停用或卸载插件后，平台不提供回退版内置工具。
- `@attson/plugin-sdk` 定义 manifest、签名 canonical JSON、浏览器 Host bridge 和稳定错误；`atcodec`、`attext`、`atgenerator`、`attime` 与 `atjson` 通过 Vite host alias 在 Standalone Adapter 与 Plugin Adapter 间切换，各自共享独立版和插件版的 Vue UI 与核心逻辑。
- `athttp` 与 `atclipboard` 同样通过 Vite adapter 复用 UI：独立版直接连接本地 service，插件版经 Host API 2.x 转发到相同 service。两者的生产 `0.1.0-alpha.2` 均已通过五目标 Release、Catalog 安装/升级/回滚和真实 Tauri 验收。

### 官方 Catalog、升级与回滚

- `catalog/v1/root.json` 授权 Catalog Signing Key 与各独立应用的 Publisher Key；Root 由离线 Root Key 签名，AT Tool 正式构建只嵌入 Root 公钥。Catalog 与 AT Tool updater 使用相互独立的密钥体系，每个应用使用独立 Publisher Key。
- 平台验证 Root、Catalog、Publisher manifest 签名、目标制品 SHA-256/大小和包内文件 hash。新元数据无效、过期或序列回退时不得覆盖上一份有效缓存；缓存过期后可继续运行已安装应用，但禁止新的远程安装和升级。
- `github.com` 或 `raw.githubusercontent.com` 的规范 HTTPS URL 始终映射到 `gh-proxy.com` 下载；其他 URL 不改写。传输中断时最多执行三次严格 Range 续传，代理响应仍必须通过相同的 Root、Catalog、Publisher 签名、大小、SHA-256 与包内文件 hash 校验，代理不进入信任边界。
- 远程安装与升级先下载到 staging 并登记 `pending`，通过 target、Host API、权限和隔离健康检查后再原子切换 `current`。切换前失败保持旧版本不变；首次运行确认失败恢复 `previous`，并记录稳定错误码与 correlation ID。
- 应用管理合并 Catalog 与本地 registry，提供“应用目录 / 已安装 / 操作记录”。升级只接受严格更高 SemVer；新增权限必须由用户确认完整差集，权限不变或减少不重复授权。

### Catalog 发布链

- 独立应用的一个 tag 由受保护 `prod` Environment 构建五个目标，每个目标同时产生独立安装包和 Publisher 签名 `.atpkg`。普通应用的正式 Release 必须恰好包含十个非空资产；Aria2 还必须附带固定 SHA-256 的 `aria2-1.37.0-source.tar.gz`，共十一项。
- `.github/catalog/apps.json` 是官方应用来源注册表，固定声明 `appId`、仓库、资产前缀、展示信息、Host API、权限和 Publisher secret 名。`.github/workflows/catalog.yml` 只接受注册表中的 `attsontools/*` 公开 Release，通过 `gh-proxy.com` 下载五个 `.atpkg`，逐包核对不可变 Release URL、文件名、目标、版本、Publisher Key、签名、Host API、权限、文件集合与 hash；Aria2 包还必须包含目标对应的 `ataria2-service`、`aria2c`、COPYING 和 Windows OpenSSL notice，Template 包必须包含目标对应的 `attemplate-service`、`psd-bridge` 和 LICENSE，Video 包必须包含目标对应的 `atvideo-service`、`youtube-bridge`、QuickJS、runtime manifest、主许可证和三份第三方许可证，这些应用的额外文件都只允许位于已签名的 `ui/` 目录。
- Catalog 发布器先离线验证已提交的 Root 和上一版 Catalog，再使用 Root 授权且处于有效窗口的 Catalog Signing Key 生成 7 天有效、严格递增的下一序列。首次发布从 `sequence: 1` 开始；已有应用只接受严格更高版本，首次接入的应用追加到 `apps` 且不得改写其他应用。
- `.github/workflows/catalog-renew.yml` 每周一、周四续签 Catalog：递增序列并更新生成与过期时间，但必须逐字保留 `apps` 及其制品信息；续签只能使用 Root 当前授权且处于有效窗口的 Catalog Signing Key。
- 发布和续签共享 `production-catalog` 并发组，且任一 `catalog/*` PR 未合并时都拒绝生成新候选，避免多个候选从同一序列分叉。
- 两个工作流都不生成或修改 Root，也不直接写默认分支；发布创建 `catalog/<app-slug>-<version>` PR，续签创建 `catalog/renew-sequence-<sequence>` PR，由受保护的 `main` 合并后生效。
- `pnpm rotate:catalog-keys` 是通用离线 Root ceremony 入口；生产 overlap 使用三路径快捷入口 `pnpm rotate:catalog-keys:prod -- <Root 离线目录> <新离线目录> <新 transfer 目录>`，自动从已提交 Root 推导下一组 Catalog/Publisher Key ID 与一年有效期。`overlap` 校验当前 Root 签名及 Root 私钥后生成新 Key，并把离线备份、GitHub transfer secrets 与公共 Root 元数据写入两两独立且预先不存在的目录；`status` 只生成递增序列的 Root 与签名，用于把旧 Key 改为 `retired` 或 `revoked`。状态切换必须保留对应角色的 active 替代 Key，`revoked` 为不可恢复终态，任何输出都不得覆盖已有目录。
- 新应用 Publisher 授权使用 `pnpm authorize:publisher:prod -- <app-slug>`。命令从应用注册表和当前年份 Root 离线目录推导 key ID 与输出路径，只新增一个 Publisher Key 并递增 Root sequence，不生成 Catalog Key；私钥备份与可传输的公开 Root/GitHub secrets 分目录写入，失败时清理本次不完整输出。

## 软件更新（v0.8.5 起自研）

**放弃 `tauri-plugin-updater`**（只支持 macOS `.app` / Windows `.msi` / Linux `.AppImage`，不覆盖 attool 的 `.deb` 分发）。替换为一套三平台统一的自研 updater。

### 端到端流程

```
1. useUpdater.check() → invoke('updater_check')
2. Rust check.rs: GET api.github.com/.../releases/latest
                  → semver 比较 → 挑 assetName (按 GOOS/GOARCH)
                  → phase = Available { info }
3. useUpdater.install() → invoke('updater_download')
4. Rust: GET SHA256SUMS + SHA256SUMS.sig
         → verify.rs: Ed25519.verify(pub, sums, sig)
         → lookup expected_sha256 = SHA256SUMS[assetName]
         → download.rs: 流式 GET assetUrl，边下边算 sha
         → phase = Ready
5. useUpdater.relaunch() → invoke('updater_apply')
6. Rust apply/*: 平台专用替换 + 重启 (见下)
```

### 平台专用 apply

| 平台 | 逻辑 |
|---|---|
| macOS | tar 解 `.app.tar.gz` → rename current `.app` 为 `.app.old` → rename 新 `.app` 到位（失败回滚）→ `open <bundle>` 拉起 → app.exit(0) |
| Windows | PowerShell `Expand-Archive` `.exe.zip` → embed `update-windows.bat`（timeout 2s → `copy /y` → `start`）→ `cmd /C start bat` → app.exit(0) |
| Linux | 写 embed `install-linux.sh` 到 `/tmp/attool-update-<pid>.sh` → chmod 755 → 传 `<pid> <staged.tar.gz> <current_exe_path>` → app.exit(0)；脚本内：等父进程死 → tar 解 → 安装 layer-shell 运行库 → mv 覆盖 / 失败走 `pkexec` → `setsid` 拉起 → 清理 tmp |

### 密钥体系

- **Ed25519**（`ed25519-dalek` 校验、Node stdlib `crypto.sign` 签名）
- 私钥 → GitHub Environment `prod` secret `ATTOOL_UPDATE_SIGNING_PRIVATE_KEY`（PKCS8 PEM 整块），**必须离线备份**
- 公钥 → Environment secret `ATTOOL_UPDATE_VERIFY_PUBLIC_KEY`（SPKI PEM 整块）
- 公钥注入：`build.rs` 从 env 读，写到 `$OUT_DIR/verify_public_key.pem`，`updater/keys.rs` 用 `include_str!()` 嵌入
  - **不用 `cargo:rustc-env=`** 因为它是单行指令，多行 PEM 会被截断成第一行
- 空公钥 = updater 全禁用（dev build 场景）

### CI 侧

- `.github/workflows/build.yml` 每 matrix 除 installer 外**打 updater 归档**：macOS `tar -czf .app.tar.gz`、Windows `7z a .exe.zip`、Linux `tar -czf .tar.gz`（`attool` + `libgtk-layer-shell.so.0`）
- release job（`environment: prod`）：`find` 生成 `SHA256SUMS` → `node .github/scripts/sign-checksums.mjs` → `gh release create` 一次性挂全部

### 前端

- `src/composables/useUpdater.ts`：兼容旧 UI 接口（`state / check / install / relaunch / dismiss`）；内部 `invoke() + listen('updater://state')`
- UI：`SettingsModal.vue`（sidebar 齿轮打开）+ `UpdateBanner.vue`（topbar 下）

## 关键模块

### `App.vue`

顶层装配点：

- 导入 `builtinTools[]`，并与 `usePluginStore.navigationTools` 组合成动态导航
- 起 composable：`useSidebarState` / `useLastTool` / `useTheme`
- Naive UI ConfigProvider 切换主题
- 渲染 `<AppShell>` + `<template v-else-if="selectedTool.id === '<id>'">` 分支
- 嵌入 Aria2 工具页（典型 form + list 形态，作为参考实现）
- 注册全局快捷键 `⌘\` / `⌘K`

### `src/composables/`

模式：注入式副作用依赖，便于 Vitest 单测无需 jsdom。

```ts
export function useSidebarState(storage: KVStorage = localStorage) { ... }
export function useTheme(
  storage: KVStorage = localStorage,
  root: ThemeRoot = document.documentElement
) { ... }
```

测试时传入 fake storage / fake root，断言行为与持久化结果。

`usePluginStore` 负责加载已安装应用、合并 Catalog、生成动态导航和驱动安装/升级/停启/卸载；独立应用自己的业务状态不进入宿主 composable。

### `src/components/ecommerce/TemplateTool.vue`

模板编辑器主组件。组合 5 个子组件：

- `TemplateResourcePanel` —— 左侧资源面板，5 个 tab（文字 / 图片 / 素材 / 图层 / 模板）
- `LayerTree` —— 图层树（资源 panel 的"图层"tab 内）
- `TemplateCanvas` —— 中央画布（带选中框 / 手柄 / 拖拽）
- `LayerProperties` —— 右侧属性面板（位置 / 外观 / 字段 / 字体 / 对齐 / 高级）
- `BatchTaskPanel` —— 底部批量任务面板（按图层注入变体 → 1:1 笛卡尔展开）

状态全部在 `TemplateTool.vue` 顶层 `project: ref<TemplateProject>`，下传不下推 —— 子组件只 emit 修改意图，由 `TemplateTool` 调用 `src/utils/ecommerceTemplate.ts` 中的纯函数返回新 project。

### `src/components/plugins/`

- `ApplicationManager.vue` 展示内置工具、Catalog 应用、已安装应用和操作记录，并驱动远程安装、升级、停启与卸载。
- `PluginFrame.vue` 建立隔离 iframe 与 Host API session；带 `nativeService` 的应用先连接 supervisor，再通过 `plugin_native_service_request` 转发请求和 `plugin://native-service-event` 接收事件。
- `pluginHost.ts` 校验 JSON-RPC、权限、超时和消息大小；`pluginProbe.ts` 根据 manifest 选择 Host API 1.x 或 2.x。
- HTTP 编辑器 UI 与业务逻辑位于 `attsontools/athttp`，宿主不复制其组件或 Store。

### 后端模块概览

`lib.rs` 承载主入口 `run()`、Aria2 下载全部逻辑与 command 注册；其余能力按模块拆分：

| 模块 | 职责 | 关键 command |
|---|---|---|
| `lib.rs` | Aria2 下载（正式版启动内置 `aria2c` + 进度事件）、command 注册 | `start_download` / `cancel_download` / `list_download_tasks` / `open_external_url` |
| `clipboard/` | 截图/钉图共用的跨平台系统图片剪贴板写入，以及 Linux capture overlay layer-shell | 无领域 command |
| `imaging/` | 图片压缩 / 转换 / EXIF / OCR / 截图与统一捕获浮层 | `compress_images` / `convert_images` / `read·strip_image_exif` / `ocr_image` / `ocr_capture_overlay` / `capture_screen` / `open·commit·pin_capture_overlay` / `get·set_capture_shortcut` |
| `recording.rs` / `recording_preview.rs` | 录屏快捷键、Web 分块缓存、Wayland 来源准备 / 区域裁剪 / 编码控制、完成结果交接、只读 Range 预览、保存与清理 | `get·set_recording_shortcut` / `prepare·start·pause·resume·stop·cancel_native_recording` / `begin·append·finish_recording_file` / `present_recording_result` / `recording_preview_url` / `save·discard_recording_file` / `default_recording_path` |
| `ecommerce/` | 主图模板：PSD 导入 / 图层渲染 / 批量替换 | `import_psd_template` / `save·load·list·delete_ecommerce_template` / `run_batch_replace_tasks` |
| `network/` | 网络诊断 | `ping_host` / `check_ports` / `resolve_dns` |
| `updater/` | 自研更新：check / verify / download / apply | `updater_get_state` / `updater_check` / `updater_download` / `updater_apply` / `updater_cancel` |
| `plugin/` | 签名包校验、staging/registry、Catalog、隔离资源协议、Host API 与 Native Service supervisor | `plugin_inspect_package` / `plugin_install_package` / `plugin_list_installed` / `plugin_set_enabled` / `plugin_uninstall` / `plugin_connect_native_service` / `plugin_native_service_request` |
| `douyin.rs` / `bilibili.rs` / `xhs.rs` / `youtube.rs` | 视频链接解析；YouTube 通过共享 Python sidecar 调用内置 yt-dlp / EJS / QuickJS | `extract_douyin_video` / `extract_bilibili_video` / `extract_youtube_video` / `resolve_douyin_url` |

### 内置运行时

- `runtime-manifest.json` 固定 Tesseract、tessdata、Aria2、yt-dlp、yt-dlp-ejs、QuickJS-ng 与 Linux GStreamer ABI 的版本和来源校验值。
- CI 在五个发布目标编译前准备并 smoke-test 对应资产；`build.rs` 只在 release 构建中要求资产完整，并把文件生成为 `include_bytes!` 定义嵌入主程序。
- `embedded_runtime.rs` 把各运行时释放到用户缓存的 `attool/runtime/<name>/<version>/`。文件按内容校验，经同目录临时文件原子落盘；损坏文件会自动修复，旧版本目录尽力清理。
- 正式版只使用内置运行时；debug 可通过 `ATTOOL_*` 环境变量指定开发程序，缺省时允许使用系统 `PATH`。
- OCR 始终传入私有 `tessdata` 目录，只接受界面公开的语言组合。YouTube 与 PSD 导入复用同一个 PyInstaller sidecar，但使用独立调用模式和 JSON 输出。
- Linux Wayland 录屏在 `gst::init()` 前设置私有插件目录和 registry；Deb 继续声明 GStreamer 核心 ABI 与基础包依赖。
- 内置组件不自行更新，也不在首次调用时下载；它们只随通过 SHA256SUMS 和 Ed25519 校验的 AT Tool 更新升级。

**截图跨平台策略**（`imaging/capture.rs` + `windows.rs`）：macOS 走 `screencapture` + CoreGraphics 窗口枚举；Linux / Windows 走 `xcap`（`Monitor::capture_image` 截屏、`Window::all` 枚举）。浮层背景帧存为 BMP（无压缩，避免 PNG deflate 的秒级延迟）。Wayland 会话下 xcap 走 xdg-desktop-portal，可能弹权限 / 部分场景失败，窗口枚举降级为空列表（区域选择仍可用）。

截图确认会先把合成 PNG 保存到缓存，再写入系统图片剪贴板；只有剪贴板写入成功才关闭浮层，失败时保留浮层并向用户报错。COSMIC Wayland/XWayland 会话通过独立 X11 owner 发布图片：候选 owner 完成初始化后才替换旧 owner，瞬时启动失败会有限重试，全部失败则保留旧 owner 并返回错误。

**统一截图 / 录屏浮层**（`CaptureOverlay.vue` + `imaging/capture.rs` + `recording.rs`）：浮层默认截图，顶部可切换录屏；截图选区工具栏可通过 `ocr_capture_overlay` 识别不含标注的原始选区，固定使用 `eng+chi_sim`，识别结果在浮层内可编辑，只在用户点击复制时写入文本剪贴板，不提交截图也不关闭浮层。录屏支持区域 / 窗口 / 全屏，区域复用自由框选、移动和八方向缩放。Web MediaRecorder 路径在浮层内保留系统媒体流，用 Canvas 按源像素裁剪后再录制；Linux Wayland 先通过 Portal / PipeWire 准备来源和首帧，用户确认区域后由 GStreamer 按 `videocrop → videoscale → VP8/WebM` 开始编码。倒计时、选区边框和控制条只存在于透明浮层，边框与控制条放在裁剪范围外；停止后通过 `recording://completed` 回到主窗口的预览与保存流程。预览使用仅绑定 `127.0.0.1`、随机令牌保护的只读 HTTP Range 流，不使用 Linux WebKitGTK 无法播放的 Tauri `asset://` 媒体地址，也不把完整录屏读入前端内存。Wayland 原生路径不支持音频，窗口来源不可用时必须禁用，不退化为全屏。

### `src-tauri/src/ecommerce/`

- `mod.rs` —— pub use；register_handlers
- `models.rs` —— TemplateProject / TemplateLayer / TemplateAsset 结构
- `commands.rs` —— `import_psd_template` / `save_ecommerce_template` / `list_ecommerce_templates` / `load_ecommerce_template` / `rename_ecommerce_template` / `delete_ecommerce_template` / `list_template_assets` / `delete_template_asset` / 批量导出系列
- `psd_bridge.rs` —— release 调用安装目录内的 PyInstaller `psd-bridge` sidecar；debug 缺 sidecar 时可回退本机 Python
- `render.rs` —— 把 `TemplateProject` 渲染成 PNG（用于批量导出）
- `storage.rs` —— rusqlite 表创建 / CRUD

### `src-tauri/src/plugin/http_service.rs`

- 只为已安装、已启用、带合法 `nativeService` 的应用启动或连接 service；入口文件仍由安装 registry 的 hash 契约约束。
- 按 app 解析受控数据目录：HTTP 保持 `${app_data_dir}/http`，剪贴板使用 `${app_data_dir}/clipboard`，Aria2 使用 `${app_data_dir}/aria2`，主图模板使用 `${app_data_dir}/ecommerce`，其他应用使用 `${app_data_dir}/native-services/<appId>`；runtime 管理 endpoint/lease、并发启动、owned child 生命周期、版本协商、请求超时和稳定错误码。
- `plugin_connect_native_service` 建立连接，`plugin_native_service_request` 代理 Host API 2.x 请求；service 事件带 `appId` 统一转发给 `PluginFrame`。
- transport 只开放显式领域方法：HTTP 继续映射三项 `host.http.*` 权限，下载七个方法映射到 `host.download.tasks`，模板十三个项目方法映射到 `host.template.project`，抖音解析/抽取、小红书、B 站和 YouTube 五个方法映射到 `host.video.extract`；不提供任意 native service call。视频调用还按方法限制平台 URL host、精确参数字段及无凭证代理，并使用 35 秒 Host 请求预算给 service 内部 30 秒平台/runtime 超时留出稳定错误响应余量；未来 service 新增参数不会自动扩张既有权限。
- 不实现 HTTP、下载或其他应用业务，也不持有业务 SQLite schema；业务状态均由各自版本化 service 拥有。

### `src-tauri/src/updater/`

- `mod.rs` —— 模块声明
- `models.rs` —— `Phase` enum / `ReleaseInfo` / `Snapshot` / `GithubRelease` 结构
- `keys.rs` —— `include_str!(OUT_DIR/verify_public_key.pem)`；`is_enabled()` / `verify_public_key()`
- `check.rs` —— `fetch_latest_release()` + `newer_than_current()` + `expected_asset_name(os, arch)` + `build_release_info()`
- `verify.rs` —— `verify_checksums_signature()` + `lookup_expected_sha256()` + `compute_sha256_hex()`
- `download.rs` —— `fetch_text()` + `download_to_stage()`（流式 + 进度 emit + AtomicBool cancel）
- `state.rs` —— `UpdaterState`：Mutex<Snapshot> + stage_dir + cancel + cached_release + staged_asset
- `commands.rs` —— tauri 命令：`updater_{get_state, check, download, apply, cancel}`
- `apply/{mod, macos, windows, linux}.rs` —— 平台专用替换与重启
- `scripts/{install-linux.sh, update-windows.bat}` —— embed 脚本（`include_str!`）

### Tauri 命令注册

`src-tauri/src/lib.rs` 的 `run()` 在 `invoke_handler![]` 列出全部命令。新增命令必须**同时**：

1. 写 `#[tauri::command]` 函数
2. 在 `invoke_handler![]` 列表追加

否则前端 `invoke('xxx')` 会运行时报错。

## 测试策略

- **逻辑模块** —— Vitest，colocated `*.test.ts`：composable / theme / utils / 插件 Host API、Catalog 与应用管理状态
- **Vue SFC** —— 不写单元测试（不引 jsdom）；改动后人工目视回归
- **后端 Rust** —— 模板模块、updater 模块，以及插件签名、target、路径、hash、原子激活、Native Service supervisor 与协议测试
- **插件** —— Vitest 覆盖动态导航、store 状态和 Host API；真实 Tauri 窗口已验证 Catalog HTTP 的唯一入口、Host API 2.x、停用和重新启用
- **集成** —— HTTP 业务集成测试位于 `athttp`；宿主依赖 `cargo test --lib` 和真实 `pnpm tauri:dev` 流程验证桥接

## 构建与发布

### 本地

- `pnpm build` —— TS 类型检查 + Vite 生产构建（输出 `dist/`）
- `pnpm tauri:build` —— 上一步 + Rust 编译 + 打包

`tauri.conf.json` 的 `bundle.targets` 限制为 `["dmg", "app", "deb", "nsis"]` —— macOS 出 dmg + app、Linux 出 deb、Windows 出 NSIS exe。**关闭 `createUpdaterArtifacts`**：updater archive 由 CI 自己打（不依赖 Tauri signer）。

### CI（`.github/workflows/build.yml`）

两阶段 job 结构：

1. **build matrix（5 平台）**

   每个 runner 跑：
   - `dtolnay/rust-toolchain@stable` 装 target
   - `swatinem/rust-cache@v2` 缓存 Cargo
   - `pnpm/action-setup@v4` + `actions/setup-node@v4` + `pnpm install --frozen-lockfile`
   - `pnpm tauri build --target <triple>` 编译 + 打包
     - env `ATTOOL_UPDATE_VERIFY_PUBLIC_KEY` 从 secret 注入到 `build.rs`（写到 `OUT_DIR/verify_public_key.pem`）
   - `bash .github/scripts/stage-bundles.sh <target> <label> <stage-dir>`：
     - 把 installer 复制到 stage 并规整名（`AT.Tool_<v>_<arch>.{dmg,deb,exe}`）
     - **同时打 updater archive**：macOS `tar -czf .app.tar.gz "AT Tool.app"`、Windows `7z a .exe.zip "AT Tool.exe"`、Linux `tar -czf .tar.gz attool libgtk-layer-shell.so.0`
   - `actions/upload-artifact@v4` 把 stage 全部上传，名 `bundle-<label>`

2. **release job（单 ubuntu-latest，`environment: prod`）** 仅在 tag push 时跑

   - `actions/download-artifact@v4 merge-multiple: true` 把 5 个 bundle artifact 合并到一个目录
   - `find` 生成 `SHA256SUMS`（覆盖所有 dmg / deb / exe / tar.gz / zip）
   - `node .github/scripts/sign-checksums.mjs SHA256SUMS`（读 `ATTOOL_UPDATE_SIGNING_PRIVATE_KEY` env）产 `SHA256SUMS.sig`
   - `gh release create <tag> --title "AT Tool <tag>" --notes "" <all files>` 直接发布正式 release

### 触发条件

| 事件 | 行为 |
|---|---|
| `workflow_dispatch` | 仅跑 build matrix，artifact 14 天有效；不进入 release job |
| push tag `v*` | build matrix → release job，直接创建 published release |

### 代码签名

macOS app bundle 使用 `signingIdentity: "-"` 做 ad-hoc 签名，确保 `.app` 内资源被封存，避免下载后因无 sealed resources 被判定为损坏。未配 macOS Developer ID notarization / Windows Authenticode；用户首次打开仍可能触发 Gatekeeper / SmartScreen 警告，按 OS 说明放行即可。

**Updater 签名与 Gatekeeper 无关**，是独立机制：`ATTOOL_UPDATE_SIGNING_PRIVATE_KEY` 签 `SHA256SUMS`，客户端用编译进二进制的公钥（`build.rs` 写到 `OUT_DIR/verify_public_key.pem`）验签。

## 性能 / 包大小现状

- 前端 bundle 不再包含 Monaco 或 HTTP 业务 chunk；独立应用资源只在安装后由自定义协议加载
- Rust 二进制不再包含宿主 HTTP/SSE/WebSocket 实现或 `tokio-tungstenite`
- 已知优化空间：vite 代码分割（命令面板等惰加载）、删除未引用 Naive UI 组件、调整 Rust LTO

## 扩展指南

参考 `AGENTS.md` 的"加新工具的最小路径"章节。

## 决策记录

### 2026-11-10 独立仓库双制品与隔离插件运行时

工具长期采用独立仓库维护，共享业务核心同时生成独立桌面应用和签名 `.atpkg`；AT Tool 负责安装、隔离 UI、权限代理和公共能力。该方案增加了 SDK 与发布基础设施，但避免编译期组合要求重装平台，也避免所有应用继续共享平台级 Tauri 权限和发布节奏。

### 2026-11-11 Root 离线与 Catalog 受保护 PR 发布

Root 私钥保持离线，日常 Catalog 只使用 Root 授权的独立 Signing Key；AT Tool 主仓从公开 Codec Release 验证制品后生成 Catalog PR，而不直接改 Root 或默认分支。该流程牺牲一步发布速度，换取 Root 信任边界、可审计变更和 Catalog 序列单线推进。

### 2026-11-13 HTTP 共享数据由单一原生 Service 持有

`athttp` 独立版和平台插件统一作为客户端连接同一 Native/Data Service，由 service 独占 SQLite、请求取消和 SSE/WS session；插件通过受权限控制的 Host API 代理访问。该方案增加了 service 分发、版本协商和 supervisor 复杂度，但避免依赖 SQLite WAL 让多个 UI 进程共享数据所有权，也避免把任意网络和本地文件权限交给 iframe。

### 2026-11-14 HTTP 仅保留独立应用入口

生产 Catalog 与 Host API 2.x 生命周期验证完成后，平台完整删除内置 HTTP UI、Store、commands 和数据库连接，只保留动态应用入口与 service 桥接。相比隐藏旧入口或保留只读兼容命令，完整退场避免恢复双数据 owner，也确保停用或卸载插件不会触发不可见回退。

### 2026-11-14 Native Service transport 与领域权限分离

平台复用 app-scoped supervisor、握手和事件 transport，但 Host API 仍以 HTTP、下载等领域权限和精确方法白名单对外，不开放任意 native service 调用。该方案增加双层权限映射和事件路由维护成本，换取 supervisor 复用，同时避免一个原生应用借通用桥访问其他领域能力。

### 2026-11-14 Aria2 接管后永久退权旧 owner

`ataria2-service` 首次验证并迁移旧下载库后写入永久 `owned` marker，AT Tool 的旧命令和事件持久化自此拒绝访问旧库；停用或卸载插件也不自动回退。相比保留可逆双 owner，这一选择要求独立 service 具备完整迁移和恢复能力，但避免两个进程在接管边界后继续写入不同副本而造成历史分叉。

### 2026-11-14 插件文档使用 opaque srcdoc 隔离

插件 entry 由 Host 校验后装入不带 `allow-same-origin` 的 `srcdoc`，签名静态资源继续从自定义协议加载，直接导航协议文档则由 CSP `sandbox` 钝化。相比保留每应用 custom origin 或拆成独立 WebView，该方案需要 CORS 与文档前缀装配，但能跨平台阻断子 frame 获得本地 Tauri 调用身份，同时保留现有 MessageChannel Host API。

### 2026-11-15 后台 Service 与宿主承载辅助窗口

需要页面关闭后持续工作的独立应用使用 `background` Native Service 持有领域状态，签名辅助 UI 仍由宿主以 opaque `srcdoc` 窗口承载并通过受控 activation 通知驱动。相比宿主继续持有业务 owner 或让 service 直接创建原生窗口，该方案增加生命周期与窗口协调契约，但保留单一数据 owner、统一隔离和权限边界。

### 2026-11-15 剪贴板接管后永久退权旧 owner

`atclipboard-service` 在原目录完整性验证成功后写入永久 `owned` marker，宿主旧剪贴板 owner 即使插件停用或卸载也不自动恢复。相比可逆切换，该方案要求独立版与插件版共同承担恢复能力，但避免外部 service 仍运行时宿主重新写入同一 SQLite 和资产目录。

### 2026-11-15 剪贴板仅保留独立应用入口

生产 Catalog、升级回滚和真实旧库生命周期验证完成后，平台完整删除内置剪贴板 UI、SQLite owner、watcher、领域快捷键、commands 与固定窗口，只保留动态应用入口、service 桥接和签名辅助窗口。相比隐藏旧入口或保留兼容命令，完整退场避免宿主再次成为数据 owner，也确保停用或卸载插件不会触发不可见回退。

### 2026-11-16 主图模板接管后永久退权旧 owner

`attemplate-service` 在原目录完成 SQLite、项目 JSON 和素材引用校验后写入永久 `owned` marker，宿主旧模板 command 即使插件停用或卸载也不自动恢复。相比复制旧库或保留可逆双 owner，该方案要求独立版与插件版共同承担恢复能力，但避免模板、素材和批量缓存形成两个可写副本。

### 2026-11-16 跨应用跳转由 Host activation 队列协调

动态应用之间只通过 `host.app.activate` 请求 Host 导航并交付有界 payload，真实来源由当前 session 注入；视频解析继续使用独立领域权限和精确 service 方法白名单。相比复用仅面向同应用辅助窗口的 `host.ui.open`、调用目标 service 或在宿主保留视频专用 handoff，该方案增加两阶段队列状态，但避免源 iframe 提前卸载、跨应用越权和业务特判。

### 2026-11-16 视频应用自持完整解析运行时

`atvideo` 的独立版和插件版共同持有四平台解析 service、专用 YouTube bridge 与固定 QuickJS runtime，不复用宿主或主图模板的 PSD sidecar。相比继续调用宿主命令或立即重写纯 Rust YouTube 提取器，该方案增加独立制品体积和 runtime 发布成本，但保证双制品行为一致，并允许宿主视频实现最终完整退场。
