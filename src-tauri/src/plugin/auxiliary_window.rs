use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

use super::install::PluginStore;

const MAX_ACTIVATION_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct AuxiliaryWindowRuntime {
    store: Arc<Mutex<PluginStore>>,
    coordinator: AuxiliaryOpenCoordinator,
    owners: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Clone)]
struct AuxiliaryOpenCoordinator {
    pending: Arc<Mutex<HashMap<WindowIdentity, AuxiliaryActivation>>>,
    locks: Arc<Mutex<HashMap<WindowIdentity, Arc<AsyncMutex<()>>>>>,
}

struct AuxiliaryOpenAttempt {
    coordinator: AuxiliaryOpenCoordinator,
    identity: WindowIdentity,
    activation_id: String,
    _guard: OwnedMutexGuard<()>,
    committed: bool,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct WindowIdentity {
    app_id: String,
    window_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliaryActivation {
    pub activation_id: String,
    pub app_id: String,
    pub window_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliaryOpenResult {
    pub opened: bool,
    pub activation_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliaryWindowError {
    pub code: String,
    pub message: String,
}

impl AuxiliaryWindowError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl AuxiliaryOpenCoordinator {
    fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn begin(
        &self,
        identity: WindowIdentity,
        activation: AuxiliaryActivation,
    ) -> Result<AuxiliaryOpenAttempt, AuxiliaryWindowError> {
        let open_lock = {
            let mut locks = self.locks.lock().map_err(|_| state_lock_error())?;
            Arc::clone(
                locks
                    .entry(identity.clone())
                    .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
            )
        };
        let guard = open_lock.lock_owned().await;
        self.pending
            .lock()
            .map_err(|_| state_lock_error())?
            .insert(identity.clone(), activation.clone());
        Ok(AuxiliaryOpenAttempt {
            coordinator: self.clone(),
            identity,
            activation_id: activation.activation_id,
            _guard: guard,
            committed: false,
        })
    }

    fn latest(
        &self,
        identity: &WindowIdentity,
    ) -> Result<Option<AuxiliaryActivation>, AuxiliaryWindowError> {
        self.pending
            .lock()
            .map_err(|_| state_lock_error())
            .map(|pending| pending.get(identity).cloned())
    }

    fn acknowledge(
        &self,
        identity: &WindowIdentity,
        activation_id: &str,
    ) -> Result<bool, AuxiliaryWindowError> {
        let mut pending = self.pending.lock().map_err(|_| state_lock_error())?;
        if pending
            .get(identity)
            .is_some_and(|activation| activation.activation_id == activation_id)
        {
            pending.remove(identity);
            return Ok(true);
        }
        Ok(false)
    }

    fn remove_if_current(&self, identity: &WindowIdentity, activation_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            if pending
                .get(identity)
                .is_some_and(|activation| activation.activation_id == activation_id)
            {
                pending.remove(identity);
            }
        }
    }

    fn clear_app(&self, app_id: &str) -> Result<(), AuxiliaryWindowError> {
        self.pending
            .lock()
            .map_err(|_| state_lock_error())?
            .retain(|identity, _| identity.app_id != app_id);
        Ok(())
    }
}

impl AuxiliaryOpenAttempt {
    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for AuxiliaryOpenAttempt {
    fn drop(&mut self) {
        if !self.committed {
            self.coordinator
                .remove_if_current(&self.identity, &self.activation_id);
        }
    }
}

impl AuxiliaryWindowRuntime {
    pub fn new(store: PluginStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            coordinator: AuxiliaryOpenCoordinator::new(),
            owners: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn open(
        &self,
        app: &AppHandle,
        app_id: &str,
        window_id: &str,
        payload: Value,
    ) -> Result<AuxiliaryOpenResult, AuxiliaryWindowError> {
        if serde_json::to_vec(&payload)
            .map(|bytes| bytes.len() > MAX_ACTIVATION_BYTES)
            .unwrap_or(true)
        {
            return Err(AuxiliaryWindowError::new(
                "HOST_UI_PAYLOAD_TOO_LARGE",
                "Auxiliary window payload is too large",
            ));
        }
        let identity = WindowIdentity {
            app_id: app_id.to_string(),
            window_id: window_id.to_string(),
        };
        let plugin = self
            .store
            .lock()
            .map_err(|_| state_lock_error())?
            .list()
            .map_err(|error| AuxiliaryWindowError::new(error.code, error.message))?
            .into_iter()
            .find(|plugin| plugin.app_id == app_id)
            .ok_or_else(|| {
                AuxiliaryWindowError::new(
                    "HOST_UI_PLUGIN_NOT_FOUND",
                    "Installed application was not found",
                )
            })?;
        if !plugin.enabled {
            return Err(AuxiliaryWindowError::new(
                "HOST_UI_PLUGIN_DISABLED",
                "Installed application is disabled",
            ));
        }
        if !plugin
            .permissions
            .iter()
            .any(|permission| permission == "host.ui.auxiliary")
        {
            return Err(AuxiliaryWindowError::new(
                "HOST_PERMISSION_DENIED",
                "Application is not allowed to open auxiliary windows",
            ));
        }
        let declaration = plugin.windows.get(window_id).ok_or_else(|| {
            AuxiliaryWindowError::new(
                "HOST_UI_WINDOW_NOT_DECLARED",
                "Auxiliary window is not declared by this application",
            )
        })?;
        let activation = AuxiliaryActivation {
            activation_id: Uuid::new_v4().to_string(),
            app_id: app_id.to_string(),
            window_id: window_id.to_string(),
            payload,
        };
        let attempt = self.coordinator.begin(identity, activation.clone()).await?;

        let label = auxiliary_window_label(app_id, window_id);
        let (window, created) = match app.get_webview_window(&label) {
            Some(window) => (window, false),
            None => {
                let app_id_query = percent_encoding::utf8_percent_encode(
                    app_id,
                    percent_encoding::NON_ALPHANUMERIC,
                );
                let window_id_query = percent_encoding::utf8_percent_encode(
                    window_id,
                    percent_encoding::NON_ALPHANUMERIC,
                );
                let url = format!(
                    "index.html?pluginAppId={app_id_query}&pluginWindowId={window_id_query}"
                );
                let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
                    .title(format!("{} - {}", plugin.name, window_id))
                    .visible(false)
                    .decorations(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .resizable(true)
                    .build()
                    .map_err(|error| {
                        AuxiliaryWindowError::new(
                            "HOST_UI_WINDOW_CREATE_FAILED",
                            format!("Cannot create auxiliary window: {error}"),
                        )
                    })?;
                (window, true)
            }
        };
        let opened = (|| -> Result<(), AuxiliaryWindowError> {
            position_window(app, &window, &declaration.presentation)?;
            window.show().map_err(|error| {
                AuxiliaryWindowError::new(
                    "HOST_UI_WINDOW_SHOW_FAILED",
                    format!("Cannot show auxiliary window: {error}"),
                )
            })?;
            let _ = window.set_focus();
            app.emit_to(&label, "plugin://auxiliary-activation", activation.clone())
                .map_err(|error| {
                    AuxiliaryWindowError::new(
                        "HOST_UI_WINDOW_ACTIVATE_FAILED",
                        format!("Cannot activate auxiliary window: {error}"),
                    )
                })?;
            if created {
                self.owners
                    .lock()
                    .map_err(|_| state_lock_error())?
                    .insert(label.clone(), app_id.to_string());
            }
            Ok(())
        })();
        if let Err(error) = opened {
            if created {
                let _ = window.close();
                if let Ok(mut owners) = self.owners.lock() {
                    owners.remove(&label);
                }
            }
            return Err(error);
        }
        attempt.commit();
        Ok(AuxiliaryOpenResult {
            opened: true,
            activation_id: activation.activation_id,
        })
    }

    pub fn latest_activation(
        &self,
        app_id: &str,
        window_id: &str,
    ) -> Result<Option<AuxiliaryActivation>, AuxiliaryWindowError> {
        self.coordinator.latest(&WindowIdentity {
            app_id: app_id.to_string(),
            window_id: window_id.to_string(),
        })
    }

    pub fn acknowledge(
        &self,
        app_id: &str,
        window_id: &str,
        activation_id: &str,
    ) -> Result<bool, AuxiliaryWindowError> {
        let identity = WindowIdentity {
            app_id: app_id.to_string(),
            window_id: window_id.to_string(),
        };
        self.coordinator.acknowledge(&identity, activation_id)
    }

    pub fn close_for_app(&self, app: &AppHandle, app_id: &str) -> Result<(), AuxiliaryWindowError> {
        let labels = self
            .owners
            .lock()
            .map_err(|_| state_lock_error())?
            .iter()
            .filter(|(_, owner)| owner.as_str() == app_id)
            .map(|(label, _)| label.clone())
            .collect::<Vec<_>>();
        let mut first_error = None;
        for label in labels {
            if let Some(window) = app.get_webview_window(&label) {
                if let Err(error) = window.close() {
                    first_error.get_or_insert_with(|| {
                        AuxiliaryWindowError::new(
                            "HOST_UI_WINDOW_CLOSE_FAILED",
                            format!("Cannot close auxiliary window: {error}"),
                        )
                    });
                    continue;
                }
            }
            self.owners
                .lock()
                .map_err(|_| state_lock_error())?
                .remove(&label);
        }
        self.coordinator.clear_app(app_id)?;
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn route_native_event(&self, app: &AppHandle, app_id: &str, event: Value) {
        if event.get("type").and_then(Value::as_str) == Some("host.ui.open") {
            let Some(window_id) = event.get("windowId").and_then(Value::as_str) else {
                return;
            };
            let payload = event.get("payload").cloned().unwrap_or_else(|| json!({}));
            let runtime = self.clone();
            let app = app.clone();
            let app_id = app_id.to_string();
            let window_id = window_id.to_string();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = runtime.open(&app, &app_id, &window_id, payload).await {
                    eprintln!(
                        "cannot open auxiliary window {app_id}/{window_id} [{}]: {}",
                        error.code, error.message
                    );
                }
            });
            return;
        }
        let envelope = json!({ "appId": app_id, "event": event });
        if let Err(error) = app.emit_to("main", "plugin://native-service-event", &envelope) {
            eprintln!("cannot emit native service event for {app_id} to main: {error}");
        }
        let labels = self
            .owners
            .lock()
            .map(|owners| {
                owners
                    .iter()
                    .filter(|(_, owner)| owner.as_str() == app_id)
                    .map(|(label, _)| label.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for label in labels {
            if app.get_webview_window(&label).is_some() {
                if let Err(error) = app.emit_to(&label, "plugin://native-service-event", &envelope)
                {
                    eprintln!("cannot emit native service event for {app_id} to {label}: {error}");
                }
            }
        }
    }
}

fn auxiliary_window_label(app_id: &str, window_id: &str) -> String {
    let digest = hex::encode(Sha256::digest(format!("{app_id}\0{window_id}").as_bytes()));
    format!("plugin-aux-{}", &digest[..24])
}

fn position_window(
    app: &AppHandle,
    window: &WebviewWindow,
    presentation: &str,
) -> Result<(), AuxiliaryWindowError> {
    let monitor = app
        .primary_monitor()
        .map_err(|error| {
            AuxiliaryWindowError::new(
                "HOST_UI_MONITOR_UNAVAILABLE",
                format!("Cannot inspect primary monitor: {error}"),
            )
        })?
        .or_else(|| app.available_monitors().ok()?.into_iter().next())
        .ok_or_else(|| {
            AuxiliaryWindowError::new(
                "HOST_UI_MONITOR_UNAVAILABLE",
                "No monitor is available for the auxiliary window",
            )
        })?;
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let width = work_area.size.width as f64 / scale;
    let height = work_area.size.height as f64 / scale;
    let x = work_area.position.x as f64 / scale;
    let y = work_area.position.y as f64 / scale;
    let (window_width, window_height, window_x, window_y) = match presentation {
        "bottomPanel" => {
            let panel_height = 260.0_f64.min(height);
            (width, panel_height, x, y + height - panel_height)
        }
        "centeredDialog" => {
            let dialog_width = (width * 0.78).max(900.0).min(width);
            let dialog_height = (height * 0.76).max(620.0).min(height);
            (
                dialog_width,
                dialog_height,
                x + (width - dialog_width) / 2.0,
                y + (height - dialog_height) / 2.0,
            )
        }
        _ => {
            return Err(AuxiliaryWindowError::new(
                "HOST_UI_PRESENTATION_INVALID",
                "Auxiliary window presentation is invalid",
            ));
        }
    };
    window
        .set_size(LogicalSize::new(window_width, window_height))
        .and_then(|_| window.set_position(LogicalPosition::new(window_x, window_y)))
        .map_err(|error| {
            AuxiliaryWindowError::new(
                "HOST_UI_WINDOW_LAYOUT_FAILED",
                format!("Cannot position auxiliary window: {error}"),
            )
        })
}

fn state_lock_error() -> AuxiliaryWindowError {
    AuxiliaryWindowError::new(
        "HOST_UI_STATE_LOCK_FAILED",
        "Auxiliary window state lock failed",
    )
}

#[tauri::command]
pub async fn plugin_open_auxiliary_window(
    app_id: String,
    window_id: String,
    payload: Value,
    runtime: State<'_, AuxiliaryWindowRuntime>,
    app: AppHandle,
) -> Result<AuxiliaryOpenResult, AuxiliaryWindowError> {
    runtime.open(&app, &app_id, &window_id, payload).await
}

#[tauri::command]
pub fn plugin_get_auxiliary_activation(
    app_id: String,
    window_id: String,
    runtime: State<'_, AuxiliaryWindowRuntime>,
) -> Result<Option<AuxiliaryActivation>, AuxiliaryWindowError> {
    runtime.latest_activation(&app_id, &window_id)
}

#[tauri::command]
pub fn plugin_ack_auxiliary_activation(
    app_id: String,
    window_id: String,
    activation_id: String,
    runtime: State<'_, AuxiliaryWindowRuntime>,
) -> Result<bool, AuxiliaryWindowError> {
    runtime.acknowledge(&app_id, &window_id, &activation_id)
}

#[cfg(test)]
mod tests {
    use super::{
        auxiliary_window_label, AuxiliaryActivation, AuxiliaryOpenCoordinator,
        AuxiliaryWindowRuntime, WindowIdentity,
    };
    use futures_util::poll;
    use std::task::Poll;
    use tokio::time::{timeout, Duration};

    fn activation(app_id: &str, window_id: &str, activation_id: &str) -> AuxiliaryActivation {
        AuxiliaryActivation {
            activation_id: activation_id.to_string(),
            app_id: app_id.to_string(),
            window_id: window_id.to_string(),
            payload: serde_json::json!({ "activationId": activation_id }),
        }
    }

    #[test]
    fn window_labels_are_stable_and_scoped_to_the_full_identity() {
        let history = auxiliary_window_label("com.attson.clipboard", "history");
        assert_eq!(
            history,
            auxiliary_window_label("com.attson.clipboard", "history")
        );
        assert_ne!(
            history,
            auxiliary_window_label("com.attson.clipboard", "preview")
        );
        assert_ne!(
            history,
            auxiliary_window_label("com.example.clipboard", "history")
        );
        assert!(history.starts_with("plugin-aux-"));
    }

    #[test]
    fn acknowledgement_cannot_remove_a_newer_activation() {
        let directory = tempfile::tempdir().unwrap();
        let store =
            crate::plugin::install::PluginStore::open(directory.path().join("plugins")).unwrap();
        let runtime = AuxiliaryWindowRuntime::new(store);
        let identity = WindowIdentity {
            app_id: "com.attson.clipboard".to_string(),
            window_id: "history".to_string(),
        };
        runtime.coordinator.pending.lock().unwrap().insert(
            identity,
            AuxiliaryActivation {
                activation_id: "newer".to_string(),
                app_id: "com.attson.clipboard".to_string(),
                window_id: "history".to_string(),
                payload: serde_json::json!({ "itemId": "2" }),
            },
        );

        assert!(!runtime
            .acknowledge("com.attson.clipboard", "history", "older")
            .unwrap());
        assert_eq!(
            runtime
                .latest_activation("com.attson.clipboard", "history")
                .unwrap()
                .unwrap()
                .activation_id,
            "newer"
        );
    }

    #[test]
    fn clearing_one_app_keeps_other_apps_pending_activations() {
        let coordinator = AuxiliaryOpenCoordinator::new();
        let clipboard = WindowIdentity {
            app_id: "com.attson.clipboard".to_string(),
            window_id: "history".to_string(),
        };
        let other = WindowIdentity {
            app_id: "com.example.clipboard".to_string(),
            window_id: "history".to_string(),
        };
        let mut pending = coordinator.pending.lock().unwrap();
        pending.insert(
            clipboard.clone(),
            activation("com.attson.clipboard", "history", "clipboard"),
        );
        pending.insert(
            other.clone(),
            activation("com.example.clipboard", "history", "other"),
        );
        drop(pending);

        coordinator.clear_app("com.attson.clipboard").unwrap();

        assert!(coordinator.latest(&clipboard).unwrap().is_none());
        assert_eq!(
            coordinator.latest(&other).unwrap().unwrap().activation_id,
            "other"
        );
    }

    #[tokio::test]
    async fn failed_open_attempt_removes_its_pending_activation() {
        let coordinator = AuxiliaryOpenCoordinator::new();
        let identity = WindowIdentity {
            app_id: "com.attson.clipboard".to_string(),
            window_id: "history".to_string(),
        };
        let attempt = coordinator
            .begin(
                identity.clone(),
                activation("com.attson.clipboard", "history", "failed"),
            )
            .await
            .unwrap();

        assert_eq!(
            coordinator
                .latest(&identity)
                .unwrap()
                .unwrap()
                .activation_id,
            "failed"
        );
        drop(attempt);
        assert!(coordinator.latest(&identity).unwrap().is_none());
    }

    #[tokio::test]
    async fn same_window_opens_serialize_and_the_latest_activation_wins() {
        let coordinator = AuxiliaryOpenCoordinator::new();
        let identity = WindowIdentity {
            app_id: "com.attson.clipboard".to_string(),
            window_id: "history".to_string(),
        };
        let first = coordinator
            .begin(
                identity.clone(),
                activation("com.attson.clipboard", "history", "first"),
            )
            .await
            .unwrap();
        let mut second = Box::pin(coordinator.begin(
            identity.clone(),
            activation("com.attson.clipboard", "history", "second"),
        ));

        assert!(matches!(poll!(second.as_mut()), Poll::Pending));
        first.commit();
        let second = timeout(Duration::from_secs(1), second)
            .await
            .expect("second open should continue after the first completes")
            .unwrap();
        second.commit();

        assert_eq!(
            coordinator
                .latest(&identity)
                .unwrap()
                .unwrap()
                .activation_id,
            "second"
        );
    }

    #[tokio::test]
    async fn different_window_opens_do_not_share_a_global_lock() {
        let coordinator = AuxiliaryOpenCoordinator::new();
        let history = WindowIdentity {
            app_id: "com.attson.clipboard".to_string(),
            window_id: "history".to_string(),
        };
        let preview = WindowIdentity {
            app_id: "com.attson.clipboard".to_string(),
            window_id: "preview".to_string(),
        };
        let history_attempt = coordinator
            .begin(
                history,
                activation("com.attson.clipboard", "history", "history"),
            )
            .await
            .unwrap();

        let preview_attempt = timeout(
            Duration::from_secs(1),
            coordinator.begin(
                preview,
                activation("com.attson.clipboard", "preview", "preview"),
            ),
        )
        .await
        .expect("an unrelated window should not wait for the history window")
        .unwrap();

        preview_attempt.commit();
        history_attempt.commit();
    }
}
