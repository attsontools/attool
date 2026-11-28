# AT Tool Review Profile

## 业务域

- 项目名称：AT Tool（仓库 `attool`）。
- 项目定位：基于 Tauri 2 + Vue 3 的个人桌面工具箱，提供内置工具与可按需安装的隔离应用；HTTP 调试由官方独立应用提供。
- 主要调用方：安装在 macOS、Windows、Linux 上的单个终端用户；前端通过 Tauri command 调用同进程 Rust 后端。
- 核心业务对象：工具注册项、插件 registry/Catalog、跨应用 activation、剪贴板历史、截图/录屏产物、主图模板、AI 会话、更新任务。
- 关键状态流转：更新器 `idle → checking → up-to-date|available|error → downloading → verifying → ready → applying`；插件安装 `download → verify → staging → health-check → activate → complete|rolled-back|error`；跨应用 activation `accept → commit|discard → target delivery`。

## 关键约定

- 前端新增工具必须保持 `App.vue` 为路由壳；业务逻辑放独立组件/模块，工具注册、图标与渲染分支保持一致。
- Tauri command 新增、删除或改名时，Rust command 定义、`invoke_handler` 注册、前端调用名和序列化字段必须同步；不得形成只编译单侧的契约。
- SQLite 持久化结构必须兼容用户已有本地数据库；字段新增需要迁移或兼容读取，禁止通过删库、清表或静默丢弃旧数据解决升级问题。
- 自研 updater 的 Ed25519 签名、SHA256 校验和平台 apply 链路不得绕过、降级或恢复 `tauri-plugin-updater` / `tauri-plugin-process`；空公钥仅表示 dev build 禁用更新。
- 发布资产命名、平台架构映射、校验清单和 updater archive 内容必须在客户端选择逻辑、stage 脚本与 CI release job 间完全一致。
- 插件 Host API session、Native Service supervisor、AI 流式会话和取消句柄必须可收口；错误、停用、卸载或窗口退出后不得残留监听器、无主进程或“进行中”状态。
- UI 颜色、圆角和字号必须使用已有语义 token；明暗主题下都要可读，不在组件中新增散装 hex、`rgba(...)` 或未定义圆角。
- 纯逻辑模块按 colocated Vitest 覆盖；需要 DOM/storage 的逻辑通过依赖注入测试，不引入 jsdom。
- 不新增依赖，除非需求明确且现有能力不能完成；依赖或 native runtime 变化必须同时审查锁文件、打包体积和五平台构建影响。
- 不把已弃用的“电商图片处理”前端功能加回；主图模板是该场景的现行入口。
- 不把 HTTP 内置 UI、Store、旧 Tauri commands 或宿主 SQLite owner 加回；HTTP 业务属于 `attsontools/athttp`，平台只保留 Host API 2.x 与 service supervisor。
- 不把 Aria2 内置 UI、下载 commands、宿主 SQLite owner、下载事件或 bundled runtime 加回；平台只保留动态入口、Host activation、Host API 2.x、service supervisor、包校验与旧库 capability marker。

## 高风险路径

- `src-tauri/src/updater/**`、`src-tauri/build.rs`：更新信任链、下载校验、进程替换和提权，错误可导致无法启动或执行未验证二进制。
- `.github/workflows/build.yml`、`.github/scripts/stage-bundles.sh`、`.github/scripts/sign-checksums.mjs`：五平台构建、签名和公开发布链路。
- `src-tauri/tauri.conf.json`、`src-tauri/capabilities/**`：桌面权限、可访问文件范围、窗口能力和打包内容。
- `src-tauri/src/lib.rs`：应用初始化、共享状态和全部 Tauri command 注册入口。
- `src-tauri/src/**/storage.rs`、`src-tauri/src/plugin/**`：用户本地数据、插件 registry/Catalog、安装回滚和历史兼容。
- `src-tauri/src/clipboard/**`、`src-tauri/src/imaging/**`、`src-tauri/src/recording/**`：剪贴板、屏幕内容、文件写入、全局快捷键和跨平台 native 行为。
- `src-tauri/src/plugin/http_service.rs`、`src/components/plugins/**`：隔离边界、Host API 权限、Native Service 生命周期、HTTP 数据目录和事件转发。
- `src-tauri/src/ai/**`、`src/components/ai/**`：第三方模型端点、API key、流式输出和会话存储。
- `.github/scripts/*runtime*`、`src-tauri/runtime-manifest.json`：内置二进制来源、哈希、目标平台与正式包完整性。

## 忽略路径

- `node_modules/**`、`src-tauri/target/**`、`dist/**`：依赖或构建产物。
- `src-tauri/gen/**`、`src-tauri/binaries/**`：生成或本地构建产物；若 manifest/脚本改变其来源，审查来源文件。
- `docs/tasks/**`、`docs/superpowers/**`、`.workflow-plan/**`：本地过程稿，不属于交付 diff。
- `.playwright-mcp/**`、`.superpowers/**`、`*.local*`：本地工具与个人配置。

## 独立发布范围

AT Tool 前端、Rust 后端、sidecar、内置 runtime 和 apply 脚本随同一桌面应用版本发布。Catalog 应用由各自仓库独立发布，平台只消费签名 `.atpkg` 契约。

## 领域术语

- Native/Data Service：随 `com.attson.http` 包分发、独占 HTTP SQLite 与网络 session 的本地进程；平台通过 Host API 2.x 代理访问。
- HTTP 数据目录：`${app_data_dir}/http`，升级前后保持原路径；停用/卸载默认保留，宿主启动不得打开或迁移其 SQLite。
- updater archive：应用内升级使用的压缩包，不等于给用户手工安装的 dmg/deb/nsis 产物。
- PSD sidecar：正式包内置的 PyInstaller 程序与运行时，不要求最终用户安装 Python。
- capture overlay：截图与录屏共用的全屏浮层窗口；截图需要桌面帧，录屏源选择不应强制先截图。

## 重点检查项

### 业务约束

- 用户输入和本地历史默认不得被隐式改写、清空或迁移丢失；尤其不得通过删除 HTTP 数据目录解决插件升级或退场问题。
- 取消、失败和窗口关闭必须清除任务/session/listener 状态；成功与失败路径都检查资源释放和 UI 状态复位。
- 剪贴板、截图、录屏、AI 凭证和经 Host API 代理的 HTTP 凭证属于敏感本地数据；不得新增日志、错误信息或非预期远端泄露。

### 架构边界

- `App.vue` 只承担工具注册、路由壳和平台装配；新业务进入独立组件/composable/Rust module。
- 内置前端 Store 与 Rust session/storage 各自拥有明确状态；独立应用状态不得复制回宿主。HTTP 数据只能由 Native/Data Service 持有。
- OS 分支必须用 `cfg`/目标依赖隔离，并保留 macOS、Windows、Linux 的编译路径；Linux 同时考虑 X11 与 Wayland。

### 数据与兼容性

- 审查 SQLite 表/列变化、旧 JSON 反序列化、默认字段和历史数据；新枚举值需考虑旧版本数据和回滚读取。
- 窗口 label、Tauri event 名、command 名、环境变量名和持久化字段属于跨前后端契约。
- 插件 registry、pending/current/previous 版本、Host API 版本、权限和 native service metadata 必须保持一致；失败激活不得覆盖上一可用版本。

### 外部依赖

- GitHub Releases、Catalog、Ollama、AI provider、插件包和视频站点均为不可信外部输入；检查签名/hash、超时、取消、大小限制和错误暴露。
- 外部命令与 sidecar 路径必须来自受控安装目录或明确的 debug 环境变量；参数不得经 shell 拼接造成注入。
- updater 下载的清单、签名与单文件哈希必须在 apply 前全部验证，失败应停止而非继续安装。

### 测试重点

- 纯函数先覆盖正常、边界、错误与不变性；store 测试从公共方法观察持久化、底层调用和最终状态。
- Rust 变更覆盖平台无关单测；平台专用行为至少提供目标 CI 编译、脚本测试或真实平台证据。
- UI/SFC 样式变更需要暗色与浅色目视证据；截图/录屏/更新器变化需要真实桌面流程证据。

## 触发关键词检查

- `invoke(`、`#[tauri::command]`、`generate_handler!`：核对命令名、参数/返回字段和注册是否一致。
- `CREATE TABLE`、`ALTER TABLE`、序列化 enum/struct：核对旧库、旧记录、迁移、回滚和默认值。
- `Command::new`、sidecar、PowerShell、`.bat`、`.sh`：核对路径、参数注入、退出码、清理和平台差异。
- `ATTOOL_UPDATE_*`、`SHA256SUMS`、`verify`、`apply`：核对信任链没有跳过或静默降级。
- `listen`、`spawn`、`service.lock`、`nativeService`、cancel token：核对权限、重复启动、停用/卸载、owned child 边界和异常释放。
- `clipboard`、`capture`、`recording`、`apiKey`、`token`：核对敏感内容不会进入日志、历史或非预期远端。
- `setTimeout`、debounce、worker、buffer：核对卸载清理、竞态、防陈旧结果和容量上限。

## Verification Evidence

```bash
corepack pnpm test
corepack pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
```

- 运行条件：使用 `packageManager` 锁定的 pnpm；Rust/native 测试需要对应系统开发库；完整桌面包还需要 runtime 与 PSD sidecar 构建环境。
- 常见环境失败：Linux 缺 GTK/WebKit/GStreamer/xcap 依赖、dev build 缺 updater 公钥、未准备 sidecar/runtime；应与 TypeScript、Vitest 或业务断言失败区分。
- 无法本地验证时需要的证据：五平台 CI 编译结果、目标平台真实操作截图/日志、release staged 文件清单、签名校验和 runtime smoke 输出。

## Risk & Compatibility

- Breaking change 判断：Tauri command/event/window label、SQLite schema、持久化 JSON、环境变量语义、更新资产名或压缩包布局不兼容既有客户端/数据即为 breaking change。
- 迁移要求：本地数据库和设置必须原地升级且保留用户数据；无法兼容时需要明确迁移、备份、失败回滚和用户提示。
- 发布影响：平台脚本、runtime、sidecar、签名或配置变更必须经目标矩阵构建；tag 发布会直接创建公开 release，不把未验证行为留给发版时发现。
- 安全/权限：重点关注 updater 信任链、Tauri capability、asset protocol、本地文件路径、命令执行、剪贴板/屏幕内容和 API key 泄露。
- 性能/容量：关注插件 Host API 8 MiB 帧、Native Service 请求并发、录屏分块落盘、图片批处理和下载并发；避免主线程或 Tauri async runtime 被阻塞。

## 跨仓库联动

- HTTP 契约联动 `attsontools/athttp` 与 `attsontools/attool-plugin-sdk`；平台发布另依赖 GitHub Environment `prod` 中的 updater 公私钥和 GitHub Releases 资产约定。

## Review Routing

- 默认 owner：NEEDS_INPUT: 仓库没有 CODEOWNERS 或已记录的模块 owner。
- 需要额外评审的情况：更新器/签名、Tauri 权限、SQLite 破坏性迁移、跨平台 native 捕获、正式发布工作流变化。
- 升级路径：NEEDS_INPUT: 未记录 TL、架构、安全或发布负责人。
- 应 request changes 的情况：破坏 updater 信任链、可能丢失本地数据、泄露敏感内容、单平台可编译但破坏其他发布目标、前后端 command 契约不一致。
- 可接受残余风险：NEEDS_INPUT: 未记录项目 owner 接受残余风险的标准。

## Verdict 调整

- updater 信任链绕过、本地数据破坏、敏感信息泄露、公开 release 资产错误或确定的平台启动失败，一律升级为 Block。
- 平台专用改动缺少目标平台证据时至少为 Needs attention；若 CI/编译路径已确定失败则为 Block。

## Code Pointers

- `AGENTS.md`：工程约定、目录地图、更新器与发布流程的事实来源。
- `docs/spec/overview.md`：各工具的现行用户行为与明确范围外事项。
- `docs/spec/architecture.md`：前后端模块、SQLite、worker、store 和测试边界。
- `docs/spec/ui-design-system.md`：token、主题、布局和交互约束。
- `src/App.vue`、`src/composables/**`：前端装配、共享状态和持久化入口。
- `src-tauri/src/lib.rs`、`src-tauri/src/**/commands.rs`：Tauri 初始化与 command 契约。
- `src-tauri/src/**/storage.rs`：本地数据库所有权与迁移。
- `src-tauri/src/plugin/**`、`src/components/plugins/**`：应用安装、隔离运行时、Host API 和 Native Service supervisor。
- `.github/workflows/build.yml`、`.github/scripts/**`：五平台打包、runtime、签名和发布。

## 维护规则

- 不把单次 review finding 直接提升为长期规则；需要项目文档、重复事故或 owner 确认后再沉淀。
- 验证命令、目录结构、状态流转、owner、发布资产或跨仓库联动变化时同步更新本 profile。
- 不写通用代码风格或泛泛最佳实践；只保留 AT Tool 特有、能改变 finding 判断的规则。
