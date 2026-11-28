mod bilibili;
mod bundled_tools;
mod clipboard;
#[cfg(target_os = "linux")]
mod capture_command;
mod updater;
mod douyin;
mod embedded_runtime;
#[cfg(target_os = "linux")]
mod gpu_linux;
pub mod ai;
pub mod ecommerce;
pub mod imaging;
pub mod network;
mod recording;
mod recording_preview;
mod plugin;
mod xhs;
mod youtube;

use ecommerce::{EcommerceStore, LegacyTemplateOwner};
use serde::Serialize;
use std::{fs, path::Path};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;
use tauri::{AppHandle, Manager, State, WindowEvent};

/// 启动时全局快捷键注册的结果。注册失败(如快捷键被系统/其它应用占用)时记录错误文案,
/// 供前端启动后查询并提示用户。
#[derive(Default)]
struct ShortcutRegisterState {
    capture: std::sync::Mutex<Option<String>>,
    recording: std::sync::Mutex<Option<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutRegisterErrors {
    capture: Option<String>,
    recording: Option<String>,
}

#[tauri::command]
fn get_shortcut_register_errors(state: State<'_, ShortcutRegisterState>) -> ShortcutRegisterErrors {
    ShortcutRegisterErrors {
        capture: state.capture.lock().ok().and_then(|g| g.clone()),
        recording: state.recording.lock().ok().and_then(|g| g.clone()),
    }
}

fn ensure_aria2_takeover_capability(app_data_dir: &Path) -> Result<(), String> {
    if !app_data_dir.join("aria2-tasks.sqlite3").is_file() {
        return Ok(());
    }
    let service_data_dir = app_data_dir.join("aria2");
    fs::create_dir_all(&service_data_dir)
        .map_err(|error| format!("创建 Aria2 服务数据目录失败：{error}"))?;
    let capability = service_data_dir.join("takeover-capability-v1");
    if fs::read(&capability).ok().as_deref() == Some(b"1\n") {
        return Ok(());
    }
    fs::write(capability, b"1\n").map_err(|error| format!("写入 Aria2 接管能力标记失败：{error}"))
}

#[cfg(test)]
mod aria2_takeover_capability_tests {
    use super::ensure_aria2_takeover_capability;

    #[test]
    fn does_not_create_aria2_data_without_a_legacy_database() {
        let directory = tempfile::tempdir().unwrap();

        ensure_aria2_takeover_capability(directory.path()).unwrap();

        assert!(!directory.path().join("aria2").exists());
    }

    #[test]
    fn writes_an_idempotent_capability_without_modifying_the_legacy_database() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("aria2-tasks.sqlite3");
        let original = b"legacy-download-history";
        std::fs::write(&database, original).unwrap();
        let before = std::fs::metadata(&database).unwrap();

        ensure_aria2_takeover_capability(directory.path()).unwrap();
        ensure_aria2_takeover_capability(directory.path()).unwrap();

        assert_eq!(std::fs::read(&database).unwrap(), original);
        let after = std::fs::metadata(&database).unwrap();
        assert_eq!(after.len(), before.len());
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
        assert_eq!(
            std::fs::read(directory.path().join("aria2/takeover-capability-v1")).unwrap(),
            b"1\n"
        );
    }

    #[test]
    fn rejects_startup_when_the_capability_directory_cannot_be_created() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("aria2-tasks.sqlite3"), b"legacy").unwrap();
        std::fs::write(directory.path().join("aria2"), b"not-a-directory").unwrap();

        let error = ensure_aria2_takeover_capability(directory.path()).unwrap_err();

        assert!(error.starts_with("创建 Aria2 服务数据目录失败："));
    }
}

const DOUYIN_MOBILE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.0 Mobile/15E148 Safari/604.1";

#[tauri::command]
async fn extract_douyin_video(url: String) -> Result<douyin::DouyinVideoInfo, String> {
    let video_id = douyin::extract_video_id(&url)
        .ok_or_else(|| "URL 中未找到 video id".to_string())?;

    let share_url = format!("https://www.iesdouyin.com/share/video/{video_id}/");
    let client = reqwest::Client::builder()
        .user_agent(DOUYIN_MOBILE_UA)
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|error| format!("构造 HTTP 客户端失败：{error}"))?;

    let response = client
        .get(&share_url)
        .header("Accept-Language", "zh-CN,zh;q=0.9")
        .send()
        .await
        .map_err(|error| format!("网络错误：{error}"))?;

    if !response.status().is_success() {
        return Err(format!("网络错误：HTTP {}", response.status()));
    }

    let body = response
        .text()
        .await
        .map_err(|error| format!("读取响应失败：{error}"))?;

    let raw_json = douyin::extract_payload_json(&body)?;
    let root = douyin::parse_render_data(&raw_json)?;
    let raw_mp4 =
        douyin::find_mp4_url(&root).ok_or_else(|| "未在页面数据中找到视频 URL".to_string())?;
    let title = douyin::find_title(&root, &video_id);
    let (mp4_url, resolved_no_wm) = douyin::derive_watermark_removed_url(&raw_mp4);
    Ok(douyin::DouyinVideoInfo {
        mp4_url,
        title,
        has_watermark: !resolved_no_wm,
    })
}

#[tauri::command]
async fn resolve_douyin_url(url: String) -> Result<String, String> {
    if !(url.starts_with("https://v.douyin.com/") || url.starts_with("http://v.douyin.com/")) {
        return Err("仅支持 v.douyin.com 短链".to_string());
    }

    let client = reqwest::Client::builder()
        .user_agent(DOUYIN_MOBILE_UA)
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|error| format!("构造 HTTP 客户端失败：{error}"))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("请求失败：{error}"))?;

    let final_url = response.url().to_string();
    Ok(canonicalize_douyin_url(&final_url))
}

fn canonicalize_douyin_url(url: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"/video/(\d+)").unwrap());
    if let Some(cap) = re.captures(url) {
        return format!("https://www.douyin.com/video/{}", &cap[1]);
    }
    url.to_string()
}

#[cfg(test)]
mod douyin_tests {
    use super::canonicalize_douyin_url;

    #[test]
    fn extracts_id_from_iesdouyin_share_url() {
        assert_eq!(
            canonicalize_douyin_url(
                "https://www.iesdouyin.com/share/video/7650383172730932515/?region=CN&mid=xxx"
            ),
            "https://www.douyin.com/video/7650383172730932515"
        );
    }

    #[test]
    fn extracts_id_from_douyin_video_url() {
        assert_eq!(
            canonicalize_douyin_url("https://www.douyin.com/video/7650383172730932515"),
            "https://www.douyin.com/video/7650383172730932515"
        );
    }

    #[test]
    fn returns_original_when_no_video_id_found() {
        assert_eq!(
            canonicalize_douyin_url("https://www.douyin.com/note/xxx"),
            "https://www.douyin.com/note/xxx"
        );
    }
}

#[tauri::command]
async fn extract_xhs_note(url: String) -> Result<xhs::XhsNoteInfo, String> {
    xhs::extract_note(&url).await
}

#[tauri::command]
async fn extract_bilibili_video(url: String) -> Result<bilibili::BiliVideoInfo, String> {
    bilibili::extract_video(&url).await
}

#[tauri::command]
async fn extract_youtube_video(
    url: String,
    proxy: Option<String>,
) -> Result<youtube::YoutubeVideoInfo, String> {
    youtube::extract_video(&url, proxy.as_deref()).await
}

#[tauri::command]
async fn open_external_url(url: String) -> Result<(), String> {
    let (program, args) = external_url_command(std::env::consts::OS, &url)?;
    let mut command = std::process::Command::new(program);
    command.args(args);

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开链接失败：{error}"))
}

fn external_url_command(target_os: &str, raw_url: &str) -> Result<(String, Vec<String>), String> {
    let url = url::Url::parse(raw_url).map_err(|_| "链接格式无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("仅支持不含凭证的 http/https 链接".to_string());
    }
    let url = url.to_string();
    match target_os {
        "macos" => Ok(("open".to_string(), vec![url])),
        "windows" => Ok((
            "rundll32.exe".to_string(),
            vec!["url.dll,FileProtocolHandler".to_string(), url],
        )),
        "linux" => Ok(("xdg-open".to_string(), vec![url])),
        _ => Err("当前系统不支持打开外部链接".to_string()),
    }
}

#[cfg(test)]
mod external_url_tests {
    use super::external_url_command;

    #[test]
    fn windows_opener_does_not_pass_plugin_urls_through_cmd() {
        let url = "https://example.com/watch?v=1&calc.exe";
        let (program, args) = external_url_command("windows", url).unwrap();

        assert_ne!(program.to_ascii_lowercase(), "cmd.exe");
        assert_ne!(program.to_ascii_lowercase(), "cmd");
        assert_eq!(args.last().map(String::as_str), Some(url));
    }

    #[test]
    fn external_url_command_rejects_unsafe_urls() {
        for url in [
            "file:///etc/passwd",
            "https://user:secret@example.com/video",
            "not a url",
        ] {
            assert!(external_url_command("linux", url).is_err());
        }
    }
}

fn open_folder(folder: &str) -> Result<(), String> {
    let path = Path::new(folder);
    if !path.exists() {
        return Err(format!("保存目录不存在：{folder}"));
    }

    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "macos")]
    command.arg(path);

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("explorer");
        command.arg(path);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .spawn()
        .map_err(|error| format!("打开保存目录失败：{error}"))?;
    Ok(())
}

pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        {
            let _ = app.set_activation_policy(ActivationPolicy::Regular);
        }
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn run() {
    #[cfg(target_os = "linux")]
    if clipboard::image_writer::run_x11_clipboard_owner_if_requested() {
        return;
    }

    #[cfg(target_os = "linux")]
    let external_command = capture_command::requested_command();
    #[cfg(target_os = "linux")]
    if let Some(request) = external_command.as_ref() {
        if capture_command::forward_if_running(request) {
            return;
        }
    }

    // Linux/WebKitGTK: 启动前探测 GPU 环境,必要时兜底禁用 DMABUF 渲染器,
    // 避免驱动版本不一致等场景下 WebKit 在 C 层 SIGABRT。见 gpu_linux 模块。
    #[cfg(target_os = "linux")]
    gpu_linux::maybe_disable_dmabuf();

    tauri::Builder::default()
        .register_uri_scheme_protocol("attool-plugin", |context, request| {
            let Some(runtime) = context
                .app_handle()
                .try_state::<plugin::commands::PluginRuntime>()
            else {
                return plugin::protocol::unavailable_response();
            };
            plugin::protocol::handle_plugin_request(&runtime, request)
        })
        .register_uri_scheme_protocol("attool-plugin-probe", |context, request| {
            let Some(runtime) = context
                .app_handle()
                .try_state::<plugin::commands::PluginRuntime>()
            else {
                return plugin::protocol::unavailable_response();
            };
            plugin::protocol::handle_probe_request(&runtime, request)
        })
        .on_permission_request(|_, kind| match kind {
            tauri::webview::PermissionKind::DisplayCapture => {
                tauri::webview::PermissionResponse::Allow
            }
            _ => tauri::webview::PermissionResponse::Default,
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                    #[cfg(target_os = "macos")]
                    {
                        let _ = window
                            .app_handle()
                            .set_activation_policy(ActivationPolicy::Accessory);
                    }
                }
            }
        })
        .setup(move |app| {
            #[cfg(not(debug_assertions))]
            bundled_tools::notices()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;

            #[cfg(target_os = "linux")]
            match clipboard::layer_shell::configure_capture_overlay(app.handle()) {
                Ok(true) => eprintln!("截图录屏浮层已启用 Wayland layer-shell Overlay 层"),
                Ok(false) => {}
                Err(error) => eprintln!("{error}"),
            }

            #[cfg(target_os = "linux")]
            if let Err(error) = capture_command::start_listener(app.handle()) {
                eprintln!("{error}");
            }

            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            ensure_aria2_takeover_capability(&app_data_dir)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            let ecommerce_dir = app_data_dir.join("ecommerce");
            let ecommerce_owner = LegacyTemplateOwner::new(ecommerce_dir.clone())
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            let ecommerce_store = EcommerceStore::new_for_legacy(ecommerce_dir, &ecommerce_owner)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            app.manage(ecommerce_owner);
            app.manage(ecommerce_store);
            let shortcut_errors = ShortcutRegisterState::default();
            if let Err(error) = imaging::capture::register_capture_shortcut(app.handle()) {
                eprintln!("{error}");
                if let Ok(mut slot) = shortcut_errors.capture.lock() {
                    *slot = Some(error);
                }
            }
            if let Err(error) = recording::register_recording_shortcut(app.handle()) {
                eprintln!("{error}");
                if let Ok(mut slot) = shortcut_errors.recording.lock() {
                    *slot = Some(error);
                }
            }
            app.manage(shortcut_errors);

            let ai_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?
                .join("ai");
            let ai_store = std::sync::Arc::new(
                ai::storage::AiStore::new(ai_dir)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?,
            );
            app.manage(ai_store);
            app.manage(std::sync::Arc::new(ai::session::AiSessionState::new()));
            let ai_http_client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, format!("build reqwest client: {error}")))?;
            app.manage(ai_http_client);

            let updater_stage_dir = app
                .path()
                .app_cache_dir()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?
                .join("updater");
            app.manage(updater::state::UpdaterState::new(updater_stage_dir));
            app.manage(recording::RecordingStore::default());
            let plugin_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?
                .join("plugins");
            let plugin_store = plugin::install::PluginStore::open(plugin_dir)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            let plugin_runtime = plugin::commands::PluginRuntime::p0(plugin_store.clone())
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            app.manage(plugin_runtime);
            app.manage(plugin::auxiliary_window::AuxiliaryWindowRuntime::new(
                plugin_store.clone(),
            ));
            let plugin_native_service_runtime = plugin::http_service::NativeServiceRuntime::new(
                plugin_store,
                app.path()
                    .app_data_dir()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?,
                app.handle().clone(),
            );
            let background_runtime = plugin_native_service_runtime.clone();
            app.manage(plugin_native_service_runtime);
            tauri::async_runtime::spawn(async move {
                for (app_id, result) in background_runtime.start_background_services().await {
                    if let Err(error) = result {
                        let correlation_id = background_runtime
                            .status_for(&app_id)
                            .and_then(|status| status.correlation_id)
                            .unwrap_or_default();
                        eprintln!(
                            "background native service {app_id} failed [{}] correlation_id={}: {}",
                            error.code, correlation_id, error.message
                        );
                    }
                }
            });
            let catalog_runtime = plugin::catalog::CatalogRuntime::production(
                app.path()
                    .app_data_dir()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?
                    .join("plugins")
                    .join("catalog.sqlite3"),
            )
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            app.manage(catalog_runtime);

            let show_item =
                MenuItem::with_id(app, "show-main", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show-main" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            #[cfg(target_os = "linux")]
            if let Some(request) = external_command {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
                capture_command::schedule_initial_command(app.handle(), request);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_shortcut_register_errors,
            open_external_url,
            resolve_douyin_url,
            extract_douyin_video,
            extract_xhs_note,
            extract_bilibili_video,
            extract_youtube_video,
            recording::begin_recording_file,
            recording::append_recording_chunk,
            recording::finish_recording_file,
            recording::cancel_recording_file,
            recording::save_recording_file,
            recording::discard_recording_file,
            recording::recording_preview_url,
            recording::present_recording_result,
            recording::default_recording_path,
            recording::open_recording_folder,
            recording::get_recording_shortcut,
            recording::set_recording_shortcut,
            recording::get_native_recording_capabilities,
            recording::prepare_native_recording,
            recording::start_native_recording,
            recording::pause_native_recording,
            recording::resume_native_recording,
            recording::stop_native_recording,
            recording::cancel_native_recording,
            ai::commands::ai_list_providers,
            ai::commands::ai_upsert_provider,
            ai::commands::ai_delete_provider,
            ai::commands::ai_list_models,
            ai::commands::ai_upsert_model,
            ai::commands::ai_delete_model,
            ai::commands::ai_fetch_provider_models,
            ai::commands::ai_list_sessions,
            ai::commands::ai_create_session,
            ai::commands::ai_get_session,
            ai::commands::ai_update_session,
            ai::commands::ai_delete_session,
            ai::commands::ai_send,
            ai::commands::ai_cancel,
            ai::commands::ai_retry,
            ai::commands::ai_export_session,
            ai::commands::ai_export_config,
            ai::commands::ai_import_config,
            ai::commands::ai_save_attachment,
            updater::commands::updater_get_state,
            updater::commands::updater_check,
            updater::commands::updater_download,
            updater::commands::updater_apply,
            updater::commands::updater_cancel,
            plugin::commands::plugin_inspect_package,
            plugin::commands::plugin_install_package,
            plugin::commands::plugin_list_installed,
            plugin::commands::plugin_read_entry_html,
            plugin::commands::plugin_read_probe_entry_html,
            plugin::commands::plugin_create_frame_session,
            plugin::commands::plugin_create_probe_frame_session,
            plugin::commands::plugin_revoke_frame_session,
            plugin::commands::plugin_list_history,
            plugin::commands::plugin_set_enabled,
            plugin::commands::plugin_uninstall,
            plugin::commands::plugin_catalog_refresh,
            plugin::commands::plugin_catalog_list,
            plugin::commands::plugin_install_catalog_app,
            plugin::commands::plugin_update_catalog_app,
            plugin::commands::plugin_complete_catalog_install,
            plugin::commands::plugin_fail_catalog_install,
            plugin::commands::plugin_get_operation,
            plugin::commands::plugin_get_latest_operation,
            plugin::commands::plugin_get_latest_operations,
            plugin::commands::plugin_confirm_plugin_launch,
            plugin::commands::plugin_rollback_plugin_launch,
            plugin::auxiliary_window::plugin_open_auxiliary_window,
            plugin::auxiliary_window::plugin_get_auxiliary_activation,
            plugin::auxiliary_window::plugin_ack_auxiliary_activation,
            plugin::http_service::plugin_connect_native_service,
            plugin::http_service::plugin_disconnect_native_service,
            plugin::http_service::plugin_list_native_service_statuses,
            plugin::http_service::plugin_native_service_request,
            ecommerce::commands::import_psd_template,
            ecommerce::commands::list_ecommerce_templates,
            ecommerce::commands::load_ecommerce_template,
            ecommerce::commands::save_ecommerce_template,
            ecommerce::commands::delete_ecommerce_template,
            ecommerce::commands::rename_ecommerce_template,
            ecommerce::commands::save_template_asset,
            ecommerce::commands::list_template_assets,
            ecommerce::commands::delete_template_asset,
            ecommerce::commands::import_template_asset_from_path,
            ecommerce::commands::run_batch_replace_tasks,
            ecommerce::commands::save_batch_replace_outputs,
            imaging::commands::compress_images,
            imaging::commands::convert_images,
            imaging::commands::read_image_exif,
            imaging::commands::strip_image_exif,
            imaging::commands::write_binary_file,
            imaging::commands::ocr_image,
            imaging::commands::ocr_capture_overlay,
            imaging::commands::capture_screen,
            imaging::commands::get_capture_shortcut,
            imaging::commands::set_capture_shortcut,
            imaging::commands::open_system_shortcut_settings,
            imaging::commands::open_capture_overlay,
            imaging::commands::mark_capture_overlay_ready,
            imaging::commands::set_capture_overlay_input_region,
            imaging::commands::close_capture_overlay,
            imaging::commands::commit_capture_overlay,
            imaging::commands::pin_capture_overlay,
            imaging::commands::copy_pin_image,
            imaging::commands::copy_pin_to,
            network::commands::ping_host,
            network::commands::check_ports,
            network::commands::resolve_dns
        ])
        .run(tauri::generate_context!())
        .expect("error while running AT Tool");
}
