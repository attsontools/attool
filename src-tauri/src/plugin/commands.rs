use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ed25519_dalek::VerifyingKey;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use super::{
    auxiliary_window::{AuxiliaryWindowError, AuxiliaryWindowRuntime},
    catalog::{CatalogError, CatalogInstallPlan, CatalogRuntime, CatalogSnapshot},
    frame_server::{FrameAssetReader, FrameSessionDescriptor, FrameSessionRuntime},
    http_service::{NativeServiceError, NativeServiceRuntime},
    install::{
        InstallError, InstalledPlugin, PluginAsset, PluginHistoryRecord, PluginStore,
        PreparedPlugin,
    },
    package::{inspect_package, HostEnvironment, InspectedPackage, PackageError},
    remote_install::{
        download_artifact, permission_diff, validate_accepted_permissions,
        validate_package_against_catalog, OperationSnapshot, OperationStage, RemoteInstallError,
    },
};

const P0_TEST_PUBLIC_KEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

#[derive(Clone)]
pub struct PluginRuntime {
    store: Arc<Mutex<PluginStore>>,
    environment: HostEnvironment,
    trusted_keys: Arc<HashMap<String, VerifyingKey>>,
    client: reqwest::Client,
    operations: Arc<Mutex<HashMap<String, OperationSnapshot>>>,
    pending: Arc<Mutex<HashMap<String, PendingCatalogInstall>>>,
    frame_sessions: FrameSessionRuntime,
}

struct PendingCatalogInstall {
    prepared: PreparedPlugin,
    mode: CatalogOperationMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogOperationMode {
    Install,
    Update,
}

impl CatalogOperationMode {
    fn action(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInstallResult {
    pub operation: OperationSnapshot,
    pub installed: InstalledPlugin,
}

impl PluginRuntime {
    pub fn p0(store: PluginStore) -> Result<Self, String> {
        let key = VerifyingKey::from_bytes(&P0_TEST_PUBLIC_KEY)
            .map_err(|error| format!("invalid P0 plugin public key: {error}"))?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            environment: HostEnvironment::current("2.0.0"),
            trusted_keys: Arc::new(HashMap::from([("attool-p0-test".to_string(), key)])),
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .redirect(reqwest::redirect::Policy::limited(3))
                .build()
                .map_err(|error| format!("cannot create plugin download client: {error}"))?,
            operations: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            frame_sessions: FrameSessionRuntime::default(),
        })
    }

    fn inspect(&self, path: PathBuf) -> Result<InspectedPackage, PluginCommandError> {
        inspect_package(&path, &self.environment, &self.trusted_keys).map_err(Into::into)
    }

    fn install(&self, path: PathBuf) -> Result<InstalledPlugin, PluginCommandError> {
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .install(&path, &self.environment, &self.trusted_keys)
            .map_err(Into::into)
    }

    fn list(&self) -> Result<Vec<InstalledPlugin>, PluginCommandError> {
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .list()
            .map_err(Into::into)
    }

    fn history(&self) -> Result<Vec<PluginHistoryRecord>, PluginCommandError> {
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .history()
            .map_err(Into::into)
    }

    fn set_enabled(&self, app_id: String, enabled: bool) -> Result<(), PluginCommandError> {
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .set_enabled(&app_id, enabled)
            .map_err(Into::into)
    }

    fn uninstall(&self, app_id: String, delete_data: bool) -> Result<(), PluginCommandError> {
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .uninstall(&app_id, delete_data)
            .map_err(Into::into)
    }

    pub(super) fn read_asset(
        &self,
        app_id: &str,
        relative: &str,
    ) -> Result<PluginAsset, PluginCommandError> {
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .read_asset(app_id, relative)
            .map_err(Into::into)
    }

    fn read_entry_html(
        &self,
        app_id: &str,
        entry: Option<&str>,
    ) -> Result<String, PluginCommandError> {
        let asset = self
            .store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .read_entry(app_id, entry)?;
        entry_html(asset)
    }

    pub(super) fn read_probe_asset(
        &self,
        operation_id: &str,
        relative: &str,
    ) -> Result<PluginAsset, PluginCommandError> {
        let pending = self.pending.lock().map_err(|_| {
            PluginCommandError::new(
                "PLUGIN_OPERATION_LOCK_FAILED",
                "Plugin operation lock failed",
            )
        })?;
        let operation = pending.get(operation_id).ok_or_else(|| {
            PluginCommandError::new(
                "PLUGIN_OPERATION_NOT_FOUND",
                "Plugin operation was not found",
            )
        })?;
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .read_prepared_asset(&operation.prepared, relative)
            .map_err(Into::into)
    }

    fn read_probe_entry_html(
        &self,
        operation_id: &str,
        entry: Option<&str>,
    ) -> Result<String, PluginCommandError> {
        let pending = self.pending.lock().map_err(|_| {
            PluginCommandError::new(
                "PLUGIN_OPERATION_LOCK_FAILED",
                "Plugin operation lock failed",
            )
        })?;
        let operation = pending.get(operation_id).ok_or_else(|| {
            PluginCommandError::new(
                "PLUGIN_OPERATION_NOT_FOUND",
                "Plugin operation was not found",
            )
        })?;
        let asset = self
            .store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .read_prepared_entry(&operation.prepared, entry)?;
        entry_html(asset)
    }

    fn create_frame_session(
        &self,
        app_id: String,
        entry: Option<String>,
    ) -> Result<FrameSessionDescriptor, PluginCommandError> {
        let entry = {
            let store = self.store.lock().map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?;
            let plugin = store
                .list()?
                .into_iter()
                .find(|plugin| plugin.app_id == app_id)
                .ok_or_else(|| {
                    PluginCommandError::new("PLUGIN_NOT_FOUND", "Installed plugin was not found")
                })?;
            let entry = entry.unwrap_or(plugin.entry.clone());
            if entry != plugin.entry && !plugin.windows.values().any(|window| window.entry == entry)
            {
                return Err(PluginCommandError::new(
                    "PLUGIN_ENTRY_NOT_DECLARED",
                    "Plugin UI entry is not declared as a signed window",
                ));
            }
            entry
        };
        let runtime = self.clone();
        let reader: FrameAssetReader =
            Arc::new(move |relative| runtime.read_asset(&app_id, relative));
        self.frame_sessions.start(&entry, reader)
    }

    fn create_probe_frame_session(
        &self,
        operation_id: String,
        entry: Option<String>,
    ) -> Result<FrameSessionDescriptor, PluginCommandError> {
        let entry = {
            let pending = self.pending.lock().map_err(|_| {
                PluginCommandError::new(
                    "PLUGIN_OPERATION_LOCK_FAILED",
                    "Plugin operation lock failed",
                )
            })?;
            let operation = pending.get(&operation_id).ok_or_else(|| {
                PluginCommandError::new(
                    "PLUGIN_OPERATION_NOT_FOUND",
                    "Plugin operation was not found",
                )
            })?;
            let package = &operation.prepared.package;
            let entry = entry.unwrap_or(package.entry.clone());
            if entry != package.entry
                && !package.windows.values().any(|window| window.entry == entry)
            {
                return Err(PluginCommandError::new(
                    "PLUGIN_ENTRY_NOT_DECLARED",
                    "Plugin UI entry is not declared as a signed window",
                ));
            }
            entry
        };
        let runtime = self.clone();
        let reader: FrameAssetReader =
            Arc::new(move |relative| runtime.read_probe_asset(&operation_id, relative));
        self.frame_sessions.start(&entry, reader)
    }

    fn revoke_frame_session(&self, session_id: &str) {
        self.frame_sessions.revoke(session_id);
    }

    async fn start_catalog_install(
        &self,
        plan: CatalogInstallPlan,
        accepted_permissions: Vec<String>,
        app: &AppHandle,
    ) -> Result<OperationSnapshot, PluginCommandError> {
        validate_accepted_permissions(&accepted_permissions, &plan.permissions)?;
        self.start_catalog_operation(plan, app, CatalogOperationMode::Install)
            .await
    }

    async fn start_catalog_update(
        &self,
        plan: CatalogInstallPlan,
        accepted_added_permissions: Vec<String>,
        app: &AppHandle,
    ) -> Result<OperationSnapshot, PluginCommandError> {
        let current = self
            .store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .validate_upgrade_target(&plan.app_id, &plan.version)?;
        let difference = permission_diff(&current.permissions, &plan.permissions);
        validate_accepted_permissions(&accepted_added_permissions, &difference.added)?;
        self.start_catalog_operation(plan, app, CatalogOperationMode::Update)
            .await
    }

    async fn start_catalog_operation(
        &self,
        plan: CatalogInstallPlan,
        app: &AppHandle,
        mode: CatalogOperationMode,
    ) -> Result<OperationSnapshot, PluginCommandError> {
        let operation_id = Uuid::new_v4().to_string();
        let correlation_id = Uuid::new_v4().to_string();
        let mut snapshot = OperationSnapshot::new(
            operation_id.clone(),
            correlation_id.clone(),
            mode.action(),
            &plan,
        );
        snapshot.final_available_version = self
            .store
            .lock()
            .map_err(|_| {
                PluginCommandError::with_correlation(
                    "PLUGIN_STORE_LOCK_FAILED",
                    "Plugin store lock failed",
                    &correlation_id,
                )
            })?
            .list()?
            .into_iter()
            .find(|installed| installed.app_id == plan.app_id)
            .map(|installed| installed.version);
        self.reserve_operation(snapshot.clone())?;
        let _ = app.emit("plugin://operation-state", snapshot);

        let download_path = self
            .store
            .lock()
            .map_err(|_| {
                PluginCommandError::with_correlation(
                    "PLUGIN_STORE_LOCK_FAILED",
                    "Plugin store lock failed",
                    &correlation_id,
                )
            })?
            .catalog_download_path(&operation_id, &plan.artifact.file_name);
        let runtime = self.clone();
        let event_app = app.clone();
        let progress_operation_id = operation_id.clone();
        let total_bytes = plan.artifact.size;
        let mut last_progress = Instant::now()
            .checked_sub(Duration::from_millis(150))
            .unwrap_or_else(Instant::now);
        if let Err(error) = download_artifact(
            &self.client,
            &plan.artifact,
            &download_path,
            move |downloaded| {
                if downloaded != total_bytes && last_progress.elapsed() < Duration::from_millis(150)
                {
                    return;
                }
                last_progress = Instant::now();
                if let Err(error) =
                    runtime.update_operation(&event_app, &progress_operation_id, |state| {
                        state.downloaded_bytes = downloaded;
                        Ok(())
                    })
                {
                    eprintln!("cannot update plugin download progress: {error:?}");
                }
            },
        )
        .await
        {
            self.remove_download(&operation_id);
            return Err(self.fail_operation(app, &operation_id, error.code, error.message));
        }

        if let Err(error) = self.update_operation(app, &operation_id, |state| {
            state.advance_to(OperationStage::Verify)
        }) {
            self.remove_download(&operation_id);
            return Err(self.fail_operation(app, &operation_id, &error.code, error.message));
        }
        let prepare_runtime = self.clone();
        let prepare_plan = plan.clone();
        let prepare_operation_id = operation_id.clone();
        let prepared = match tauri::async_runtime::spawn_blocking(move || {
            prepare_runtime.prepare_catalog_package(
                &download_path,
                &prepare_operation_id,
                &prepare_plan,
                mode,
            )
        })
        .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                self.remove_download(&operation_id);
                return Err(self.fail_operation(
                    app,
                    &operation_id,
                    "PLUGIN_WORKER_FAILED",
                    format!("Plugin verification worker failed: {error}"),
                ));
            }
        };
        self.remove_download(&operation_id);
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                return Err(self.fail_operation(app, &operation_id, &error.code, error.message));
            }
        };

        if let Err(error) = self.update_operation(app, &operation_id, |state| {
            state.advance_to(OperationStage::Staging)?;
            state.entry = Some(prepared.package.entry.clone());
            state.windows = prepared.package.windows.clone();
            state.native_service = prepared.package.native_service.clone();
            Ok(())
        }) {
            if let Ok(store) = self.store.lock() {
                store.discard_prepared(prepared);
            }
            return Err(self.fail_operation(app, &operation_id, &error.code, error.message));
        }
        if mode == CatalogOperationMode::Update {
            if let Err(error) = self
                .store
                .lock()
                .map_err(|_| {
                    PluginCommandError::with_correlation(
                        "PLUGIN_STORE_LOCK_FAILED",
                        "Plugin store lock failed",
                        &correlation_id,
                    )
                })?
                .mark_pending_upgrade(&prepared, &operation_id, &correlation_id)
            {
                if let Ok(store) = self.store.lock() {
                    store.discard_prepared(prepared);
                }
                return Err(self.fail_operation(app, &operation_id, &error.code, error.message));
            }
        }
        match self.pending.lock() {
            Ok(mut pending) => {
                pending.insert(
                    operation_id.clone(),
                    PendingCatalogInstall { prepared, mode },
                );
            }
            Err(_) => {
                if let Ok(store) = self.store.lock() {
                    if mode == CatalogOperationMode::Update {
                        let _ = store.clear_pending_upgrade(&plan.app_id, &operation_id);
                    }
                    store.discard_prepared(prepared);
                }
                return Err(self.fail_operation(
                    app,
                    &operation_id,
                    "PLUGIN_OPERATION_LOCK_FAILED",
                    "Plugin operation lock failed",
                ));
            }
        }
        if let Err(error) = self.update_operation(app, &operation_id, |state| {
            state.advance_to(OperationStage::HealthCheck)
        }) {
            let pending = self
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&operation_id));
            if let Some(pending) = pending {
                if let Ok(store) = self.store.lock() {
                    if pending.mode == CatalogOperationMode::Update {
                        let _ = store.clear_pending_upgrade(&plan.app_id, &operation_id);
                    }
                    store.discard_prepared(pending.prepared);
                }
            }
            return Err(self.fail_operation(app, &operation_id, &error.code, error.message));
        }
        self.operation(&operation_id)
    }

    fn prepare_catalog_package(
        &self,
        package_path: &std::path::Path,
        operation_id: &str,
        plan: &CatalogInstallPlan,
        mode: CatalogOperationMode,
    ) -> Result<PreparedPlugin, PluginCommandError> {
        let keys = HashMap::from([(plan.publisher_key_id.clone(), plan.publisher_key)]);
        let inspected = inspect_package(package_path, &self.environment, &keys)?;
        validate_package_against_catalog(&inspected, plan)?;
        let store = self.store.lock().map_err(|_| {
            PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
        })?;
        match mode {
            CatalogOperationMode::Install => {
                store.prepare_install(package_path, &self.environment, &keys, operation_id)
            }
            CatalogOperationMode::Update => {
                store.prepare_upgrade(package_path, &self.environment, &keys, operation_id)
            }
        }
        .map_err(Into::into)
    }

    fn complete_catalog_install(
        &self,
        operation_id: &str,
        app: &AppHandle,
    ) -> Result<CatalogInstallResult, PluginCommandError> {
        let correlation_id = self.operation(operation_id)?.correlation_id;
        let mut pending_operations = self.pending.lock().map_err(|_| {
            PluginCommandError::with_correlation(
                "PLUGIN_OPERATION_LOCK_FAILED",
                "Plugin operation lock failed",
                &correlation_id,
            )
        })?;
        if !pending_operations.contains_key(operation_id) {
            return Err(PluginCommandError::with_correlation(
                "PLUGIN_OPERATION_NOT_PENDING",
                "Plugin operation is not waiting for health check",
                &correlation_id,
            ));
        }
        self.update_operation(app, operation_id, |state| {
            state.advance_to(OperationStage::Activate)
        })?;
        let store = self.store.lock().map_err(|_| {
            PluginCommandError::with_correlation(
                "PLUGIN_STORE_LOCK_FAILED",
                "Plugin store lock failed",
                &correlation_id,
            )
        })?;
        let pending = pending_operations
            .remove(operation_id)
            .expect("pending operation was checked while locked");
        drop(pending_operations);
        let app_id = pending.prepared.package.app_id.clone();
        let activation = match pending.mode {
            CatalogOperationMode::Install => store.activate_prepared_with_context(
                pending.prepared,
                Some(operation_id),
                Some(&correlation_id),
            ),
            CatalogOperationMode::Update => {
                store.activate_prepared_upgrade(pending.prepared, operation_id, &correlation_id)
            }
        };
        if activation
            .as_ref()
            .is_err_and(|error| error.code != "PLUGIN_ACTIVATION_CLEANUP_FAILED")
            && pending.mode == CatalogOperationMode::Update
        {
            let _ = store.clear_pending_upgrade(&app_id, operation_id);
        }
        drop(store);
        let installed = match activation {
            Ok(installed) => installed,
            Err(error) => {
                return Err(self.fail_operation(app, operation_id, &error.code, error.message));
            }
        };
        self.update_operation(app, operation_id, |state| {
            state.advance_to(OperationStage::Complete)?;
            state.final_available_version = Some(installed.version.clone());
            Ok(())
        })?;
        Ok(CatalogInstallResult {
            operation: self.operation(operation_id)?,
            installed,
        })
    }

    fn fail_health_check(
        &self,
        operation_id: &str,
        app: &AppHandle,
    ) -> Result<OperationSnapshot, PluginCommandError> {
        let correlation_id = self.operation(operation_id)?.correlation_id;
        let mut pending_operations = self.pending.lock().map_err(|_| {
            PluginCommandError::with_correlation(
                "PLUGIN_OPERATION_LOCK_FAILED",
                "Plugin operation lock failed",
                &correlation_id,
            )
        })?;
        if !pending_operations.contains_key(operation_id) {
            return Err(PluginCommandError::with_correlation(
                "PLUGIN_OPERATION_NOT_PENDING",
                "Plugin operation is not waiting for health check",
                &correlation_id,
            ));
        }
        let store = self.store.lock().map_err(|_| {
            PluginCommandError::with_correlation(
                "PLUGIN_STORE_LOCK_FAILED",
                "Plugin store lock failed",
                &correlation_id,
            )
        })?;
        if pending_operations
            .get(operation_id)
            .is_some_and(|pending| pending.mode == CatalogOperationMode::Update)
        {
            let app_id = pending_operations
                .get(operation_id)
                .expect("pending operation was checked while locked")
                .prepared
                .package
                .app_id
                .clone();
            store.clear_pending_upgrade(&app_id, operation_id)?;
        }
        let pending = pending_operations
            .remove(operation_id)
            .expect("pending operation was checked while locked");
        drop(pending_operations);
        store.discard_prepared(pending.prepared);
        drop(store);
        self.fail_operation(
            app,
            operation_id,
            "PLUGIN_HEALTH_CHECK_FAILED",
            "Plugin did not complete the isolated Host API handshake",
        );
        self.operation(operation_id)
    }

    fn operation(&self, operation_id: &str) -> Result<OperationSnapshot, PluginCommandError> {
        if let Some(operation) = self
            .operations
            .lock()
            .map_err(|_| {
                PluginCommandError::new(
                    "PLUGIN_OPERATION_LOCK_FAILED",
                    "Plugin operation lock failed",
                )
            })?
            .get(operation_id)
            .cloned()
        {
            return Ok(operation);
        }
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .operation(operation_id)?
            .ok_or_else(|| {
                PluginCommandError::new(
                    "PLUGIN_OPERATION_NOT_FOUND",
                    "Plugin operation was not found",
                )
            })
    }

    fn latest_operation(&self) -> Result<Option<OperationSnapshot>, PluginCommandError> {
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .latest_operation()
            .map_err(Into::into)
    }

    fn reserve_operation(&self, snapshot: OperationSnapshot) -> Result<(), PluginCommandError> {
        let operation_id = snapshot.operation_id.clone();
        let mut operations = self.operations.lock().map_err(|_| {
            PluginCommandError::with_correlation(
                "PLUGIN_OPERATION_LOCK_FAILED",
                "Plugin operation lock failed",
                &snapshot.correlation_id,
            )
        })?;
        if operations.values().any(|operation| {
            operation.app_id == snapshot.app_id
                && !matches!(
                    operation.stage,
                    OperationStage::Complete | OperationStage::RolledBack | OperationStage::Error
                )
        }) {
            return Err(PluginCommandError::with_correlation(
                "PLUGIN_INSTALL_IN_PROGRESS",
                "Another install operation is already active for this application",
                &snapshot.correlation_id,
            ));
        }
        operations.insert(snapshot.operation_id.clone(), snapshot);
        let reserved = operations
            .get(&operation_id)
            .cloned()
            .expect("reserved operation exists");
        drop(operations);
        if let Err(error) = self
            .store
            .lock()
            .map_err(|_| {
                PluginCommandError::with_correlation(
                    "PLUGIN_STORE_LOCK_FAILED",
                    "Plugin store lock failed",
                    &reserved.correlation_id,
                )
            })?
            .save_operation(&reserved)
        {
            if let Ok(mut operations) = self.operations.lock() {
                operations.remove(&reserved.operation_id);
            }
            return Err(error.into());
        }
        Ok(())
    }

    fn update_operation(
        &self,
        app: &AppHandle,
        operation_id: &str,
        update: impl FnOnce(&mut OperationSnapshot) -> Result<(), RemoteInstallError>,
    ) -> Result<(), PluginCommandError> {
        let mut operations = self.operations.lock().map_err(|_| {
            PluginCommandError::new(
                "PLUGIN_OPERATION_LOCK_FAILED",
                "Plugin operation lock failed",
            )
        })?;
        let state = operations.get_mut(operation_id).ok_or_else(|| {
            PluginCommandError::new(
                "PLUGIN_OPERATION_NOT_FOUND",
                "Plugin operation was not found",
            )
        })?;
        update(state)?;
        let snapshot = state.clone();
        drop(operations);
        if let Ok(store) = self.store.lock() {
            if let Err(error) = store.save_operation(&snapshot) {
                eprintln!("cannot persist plugin operation: {error}");
            }
        }
        let _ = app.emit("plugin://operation-state", snapshot);
        Ok(())
    }

    fn fail_operation(
        &self,
        app: &AppHandle,
        operation_id: &str,
        code: &str,
        message: impl Into<String>,
    ) -> PluginCommandError {
        let internal_message = message.into();
        let message = operation_error_message(code).to_string();
        let previous = self.operation(operation_id).ok();
        if let Err(error) = self.update_operation(app, operation_id, |state| {
            state.advance_to(OperationStage::Error)?;
            state.error_code = Some(code.to_string());
            state.message = Some(message.clone());
            Ok(())
        }) {
            eprintln!("cannot update failed plugin operation: {error:?}");
        }
        if let Some(previous) = previous.filter(|state| {
            !matches!(
                state.stage,
                OperationStage::RolledBack | OperationStage::Error
            ) && (state.stage != OperationStage::Complete || code.starts_with("PLUGIN_ROLLBACK"))
        }) {
            let failed = self
                .operation(operation_id)
                .unwrap_or_else(|_| previous.clone());
            if let Ok(store) = self.store.lock() {
                if let Err(error) = store.record_catalog_failure(
                    &previous.app_id,
                    &previous.version,
                    &previous.action,
                    &previous.operation_id,
                    &previous.correlation_id,
                    code,
                    &failed.steps,
                    failed.final_available_version.as_deref(),
                ) {
                    eprintln!("cannot record plugin operation failure: {error}");
                }
            }
        }
        let correlation_id = self
            .operation(operation_id)
            .map(|operation| operation.correlation_id)
            .unwrap_or_default();
        eprintln!("plugin operation {operation_id} failed: {internal_message}");
        PluginCommandError::with_correlation(code, message, &correlation_id)
    }

    fn remove_download(&self, operation_id: &str) {
        if let Ok(store) = self.store.lock() {
            store.remove_catalog_download(operation_id);
        }
    }

    fn confirm_plugin_launch(
        &self,
        app_id: &str,
        version: &str,
    ) -> Result<bool, PluginCommandError> {
        self.store
            .lock()
            .map_err(|_| {
                PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
            })?
            .confirm_current_launch(app_id, version)
            .map_err(Into::into)
    }

    fn rollback_plugin_launch(
        &self,
        app_id: &str,
        version: &str,
        app: &AppHandle,
    ) -> Result<Option<InstalledPlugin>, PluginCommandError> {
        let store = self.store.lock().map_err(|_| {
            PluginCommandError::new("PLUGIN_STORE_LOCK_FAILED", "Plugin store lock failed")
        })?;
        let operation_id = store.launch_operation_id(app_id, version)?;
        let rollback = store.rollback_current_launch(app_id, version);
        drop(store);
        let rollback = match rollback {
            Ok(rollback) => rollback,
            Err(error) => {
                if let Some(operation_id) = operation_id {
                    if let Err(update_error) = self.update_operation(app, &operation_id, |state| {
                        state.final_available_version = None;
                        Ok(())
                    }) {
                        eprintln!("cannot update failed rollback operation: {update_error:?}");
                    }
                    return Err(self.fail_operation(
                        app,
                        &operation_id,
                        &error.code,
                        error.message,
                    ));
                }
                return Err(error.into());
            }
        };
        let Some(rollback) = rollback else {
            return Ok(None);
        };
        self.update_operation(app, &rollback.operation_id, |state| {
            state.advance_to(OperationStage::RolledBack)?;
            state.message = Some("Plugin was restored to the previous version".to_string());
            state.final_available_version = Some(rollback.installed.version.clone());
            Ok(())
        })?;
        Ok(Some(rollback.installed))
    }
}

fn operation_error_message(code: &str) -> &'static str {
    match code {
        "PLUGIN_DOWNLOAD_HTTP_FAILED" => "Application download failed",
        "PLUGIN_DOWNLOAD_INTERRUPTED" => "Application download was interrupted",
        "PLUGIN_DOWNLOAD_SIZE_MISMATCH" => "Downloaded application size is invalid",
        "PLUGIN_DOWNLOAD_HASH_MISMATCH" => "Downloaded application integrity check failed",
        "PLUGIN_CATALOG_MANIFEST_MISMATCH" => "Application package does not match the Catalog",
        "PLUGIN_HEALTH_CHECK_FAILED" => "Application health check failed",
        "PLUGIN_ACTIVATION_FAILED" | "PLUGIN_ACTIVATION_CLEANUP_FAILED" => {
            "Application activation failed"
        }
        "PLUGIN_REGISTRY_WRITE_FAILED" => "Application registry update failed",
        "PLUGIN_ROLLBACK_FAILED" | "PLUGIN_ROLLBACK_COMPENSATION_FAILED" => {
            "Automatic rollback failed; manual recovery is required"
        }
        _ => "Application operation failed",
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandError {
    pub code: String,
    pub message: String,
    pub correlation_id: String,
}

impl PluginCommandError {
    pub(super) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            correlation_id: Uuid::new_v4().to_string(),
        }
    }

    fn with_correlation(
        code: impl Into<String>,
        message: impl Into<String>,
        correlation_id: &str,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            correlation_id: correlation_id.to_string(),
        }
    }
}

impl From<CatalogError> for PluginCommandError {
    fn from(error: CatalogError) -> Self {
        Self::new(error.code, error.message)
    }
}

impl From<PackageError> for PluginCommandError {
    fn from(error: PackageError) -> Self {
        Self::new(error.code, error.message)
    }
}

impl From<InstallError> for PluginCommandError {
    fn from(error: InstallError) -> Self {
        Self::new(error.code, error.message)
    }
}

impl From<RemoteInstallError> for PluginCommandError {
    fn from(error: RemoteInstallError) -> Self {
        Self::new(error.code, error.message)
    }
}

impl From<NativeServiceError> for PluginCommandError {
    fn from(error: NativeServiceError) -> Self {
        Self::new(error.code, error.message)
    }
}

impl From<AuxiliaryWindowError> for PluginCommandError {
    fn from(error: AuxiliaryWindowError) -> Self {
        Self::new(error.code, error.message)
    }
}

fn is_background_plugin(plugin: &InstalledPlugin) -> bool {
    plugin
        .native_service
        .as_ref()
        .is_some_and(|service| service.lifecycle == "background")
}

fn should_connect_background(plugin: &InstalledPlugin) -> bool {
    plugin.enabled && is_background_plugin(plugin)
}

async fn restore_native_service(
    native_services: &NativeServiceRuntime,
    app_id: &str,
    was_connected: bool,
) {
    if !was_connected && !native_services.background_service_enabled(app_id) {
        return;
    }
    if let Err(error) = native_services.connect(app_id).await {
        eprintln!(
            "cannot restore native service {app_id} after failed plugin operation [{}]: {}",
            error.code, error.message
        );
    }
}

async fn connect_native_service_observed(native_services: &NativeServiceRuntime, app_id: &str) {
    if let Err(error) = native_services.connect(app_id).await {
        let correlation_id = native_services
            .status_for(app_id)
            .and_then(|status| status.correlation_id)
            .unwrap_or_default();
        eprintln!(
            "native service {app_id} is unavailable after committed plugin state [{}] correlation_id={}: {}",
            error.code, correlation_id, error.message
        );
    }
}

fn close_auxiliary_windows_observed(
    auxiliary_windows: &AuxiliaryWindowRuntime,
    app: &AppHandle,
    app_id: &str,
) {
    if let Err(error) = auxiliary_windows.close_for_app(app, app_id) {
        eprintln!(
            "cannot close auxiliary windows for {app_id} after committed plugin state [{}]: {}",
            error.code, error.message
        );
    }
}

fn join_error(error: tauri::Error) -> PluginCommandError {
    PluginCommandError::new(
        "PLUGIN_WORKER_FAILED",
        format!("Plugin worker failed: {error}"),
    )
}

fn entry_html(asset: PluginAsset) -> Result<String, PluginCommandError> {
    if asset.content_type != "text/html; charset=utf-8" {
        return Err(PluginCommandError::new(
            "PLUGIN_ENTRY_NOT_HTML",
            "Plugin UI entry must be an HTML document",
        ));
    }
    String::from_utf8(asset.bytes).map_err(|_| {
        PluginCommandError::new(
            "PLUGIN_ENTRY_ENCODING_INVALID",
            "Plugin UI entry must be valid UTF-8",
        )
    })
}

#[tauri::command]
pub async fn plugin_inspect_package(
    path: String,
    runtime: State<'_, PluginRuntime>,
) -> Result<InspectedPackage, PluginCommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.inspect(PathBuf::from(path)))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugin_install_package(
    path: String,
    runtime: State<'_, PluginRuntime>,
    native_services: State<'_, NativeServiceRuntime>,
    auxiliary_windows: State<'_, AuxiliaryWindowRuntime>,
    app: AppHandle,
) -> Result<InstalledPlugin, PluginCommandError> {
    let runtime = runtime.inner().clone();
    let package_path = PathBuf::from(path);
    let inspect_runtime = runtime.clone();
    let inspect_path = package_path.clone();
    let app_id =
        tauri::async_runtime::spawn_blocking(move || inspect_runtime.inspect(inspect_path))
            .await
            .map_err(join_error)??
            .app_id;
    let replacing = runtime
        .list()?
        .into_iter()
        .any(|plugin| plugin.app_id == app_id);
    let was_connected = native_services.is_connected(&app_id);
    if replacing {
        native_services.disconnect(&app_id).await?;
        if let Err(error) = auxiliary_windows.close_for_app(&app, &app_id) {
            restore_native_service(&native_services, &app_id, was_connected).await;
            return Err(error.into());
        }
    }
    let worker = runtime.clone();
    let installed = match tauri::async_runtime::spawn_blocking(move || worker.install(package_path))
        .await
        .map_err(join_error)?
    {
        Ok(installed) => installed,
        Err(error) => {
            restore_native_service(&native_services, &app_id, was_connected).await;
            return Err(error.into());
        }
    };
    if replacing {
        close_auxiliary_windows_observed(&auxiliary_windows, &app, &app_id);
    }
    if was_connected || should_connect_background(&installed) {
        connect_native_service_observed(&native_services, &installed.app_id).await;
    }
    Ok(installed)
}

#[tauri::command]
pub async fn plugin_list_installed(
    runtime: State<'_, PluginRuntime>,
) -> Result<Vec<InstalledPlugin>, PluginCommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.list())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugin_read_entry_html(
    app_id: String,
    entry: Option<String>,
    runtime: State<'_, PluginRuntime>,
) -> Result<String, PluginCommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.read_entry_html(&app_id, entry.as_deref()))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugin_read_probe_entry_html(
    operation_id: String,
    entry: Option<String>,
    runtime: State<'_, PluginRuntime>,
) -> Result<String, PluginCommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime.read_probe_entry_html(&operation_id, entry.as_deref())
    })
    .await
    .map_err(join_error)?
}

#[tauri::command]
pub async fn plugin_create_frame_session(
    app_id: String,
    entry: Option<String>,
    runtime: State<'_, PluginRuntime>,
) -> Result<FrameSessionDescriptor, PluginCommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.create_frame_session(app_id, entry))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugin_create_probe_frame_session(
    operation_id: String,
    entry: Option<String>,
    runtime: State<'_, PluginRuntime>,
) -> Result<FrameSessionDescriptor, PluginCommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime.create_probe_frame_session(operation_id, entry)
    })
    .await
    .map_err(join_error)?
}

#[tauri::command]
pub async fn plugin_revoke_frame_session(
    session_id: String,
    runtime: State<'_, PluginRuntime>,
) -> Result<(), PluginCommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.revoke_frame_session(&session_id))
        .await
        .map_err(join_error)
}

#[tauri::command]
pub async fn plugin_list_history(
    runtime: State<'_, PluginRuntime>,
) -> Result<Vec<PluginHistoryRecord>, PluginCommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.history())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugin_set_enabled(
    app_id: String,
    enabled: bool,
    runtime: State<'_, PluginRuntime>,
    native_services: State<'_, NativeServiceRuntime>,
    auxiliary_windows: State<'_, AuxiliaryWindowRuntime>,
    app: AppHandle,
) -> Result<(), PluginCommandError> {
    let was_connected = native_services.is_connected(&app_id);
    if !enabled {
        native_services.disconnect(&app_id).await?;
        if let Err(error) = auxiliary_windows.close_for_app(&app, &app_id) {
            restore_native_service(&native_services, &app_id, was_connected).await;
            return Err(error.into());
        }
    }
    let updated_app_id = app_id.clone();
    let runtime = runtime.inner().clone();
    let worker = runtime.clone();
    let update = tauri::async_runtime::spawn_blocking(move || worker.set_enabled(app_id, enabled))
        .await
        .map_err(join_error)
        .and_then(|result| result);
    if let Err(error) = update {
        if !enabled {
            restore_native_service(&native_services, &updated_app_id, was_connected).await;
        }
        return Err(error);
    }
    if enabled {
        let plugin = runtime
            .list()?
            .into_iter()
            .find(|plugin| plugin.app_id == updated_app_id);
        if plugin.as_ref().is_some_and(should_connect_background) {
            connect_native_service_observed(&native_services, &updated_app_id).await;
        }
    } else {
        close_auxiliary_windows_observed(&auxiliary_windows, &app, &updated_app_id);
    }
    Ok(())
}

#[tauri::command]
pub async fn plugin_uninstall(
    app_id: String,
    delete_data: bool,
    runtime: State<'_, PluginRuntime>,
    native_services: State<'_, NativeServiceRuntime>,
    auxiliary_windows: State<'_, AuxiliaryWindowRuntime>,
    app: AppHandle,
) -> Result<(), PluginCommandError> {
    let was_connected = native_services.is_connected(&app_id);
    native_services.disconnect(&app_id).await?;
    if let Err(error) = auxiliary_windows.close_for_app(&app, &app_id) {
        restore_native_service(&native_services, &app_id, was_connected).await;
        return Err(error.into());
    }
    let restore_app_id = app_id.clone();
    let runtime = runtime.inner().clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || runtime.uninstall(app_id, delete_data))
            .await
            .map_err(join_error)
            .and_then(|result| result);
    if let Err(error) = result {
        restore_native_service(&native_services, &restore_app_id, was_connected).await;
        return Err(error);
    }
    close_auxiliary_windows_observed(&auxiliary_windows, &app, &restore_app_id);
    Ok(())
}

#[tauri::command]
pub async fn plugin_catalog_refresh(
    runtime: State<'_, CatalogRuntime>,
) -> Result<CatalogSnapshot, PluginCommandError> {
    runtime.refresh().await.map_err(Into::into)
}

#[tauri::command]
pub async fn plugin_catalog_list(
    runtime: State<'_, CatalogRuntime>,
) -> Result<Option<CatalogSnapshot>, PluginCommandError> {
    runtime.list().await.map_err(Into::into)
}

#[tauri::command]
pub async fn plugin_install_catalog_app(
    app_id: String,
    accepted_permissions: Vec<String>,
    runtime: State<'_, PluginRuntime>,
    catalog: State<'_, CatalogRuntime>,
    app: AppHandle,
) -> Result<OperationSnapshot, PluginCommandError> {
    let plan = catalog.install_plan(&app_id).await?;
    runtime
        .start_catalog_install(plan, accepted_permissions, &app)
        .await
}

#[tauri::command]
pub async fn plugin_update_catalog_app(
    app_id: String,
    accepted_added_permissions: Vec<String>,
    runtime: State<'_, PluginRuntime>,
    catalog: State<'_, CatalogRuntime>,
    app: AppHandle,
) -> Result<OperationSnapshot, PluginCommandError> {
    let plan = catalog.install_plan(&app_id).await?;
    runtime
        .start_catalog_update(plan, accepted_added_permissions, &app)
        .await
}

#[tauri::command]
pub async fn plugin_complete_catalog_install(
    operation_id: String,
    runtime: State<'_, PluginRuntime>,
    native_services: State<'_, NativeServiceRuntime>,
    auxiliary_windows: State<'_, AuxiliaryWindowRuntime>,
    app: AppHandle,
) -> Result<CatalogInstallResult, PluginCommandError> {
    let app_id = runtime.operation(&operation_id)?.app_id;
    let was_connected = native_services.is_connected(&app_id);
    native_services.disconnect(&app_id).await?;
    if let Err(error) = auxiliary_windows.close_for_app(&app, &app_id) {
        restore_native_service(&native_services, &app_id, was_connected).await;
        return Err(error.into());
    }
    let runtime = runtime.inner().clone();
    let worker = runtime.clone();
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        worker.complete_catalog_install(&operation_id, &worker_app)
    })
    .await
    .map_err(join_error)?;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            restore_native_service(&native_services, &app_id, was_connected).await;
            return Err(error);
        }
    };
    if was_connected || should_connect_background(&result.installed) {
        connect_native_service_observed(&native_services, &app_id).await;
    }
    close_auxiliary_windows_observed(&auxiliary_windows, &app, &app_id);
    Ok(result)
}

#[tauri::command]
pub async fn plugin_fail_catalog_install(
    operation_id: String,
    runtime: State<'_, PluginRuntime>,
    app: AppHandle,
) -> Result<OperationSnapshot, PluginCommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.fail_health_check(&operation_id, &app))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugin_get_operation(
    operation_id: String,
    runtime: State<'_, PluginRuntime>,
) -> Result<OperationSnapshot, PluginCommandError> {
    runtime.operation(&operation_id)
}

#[tauri::command]
pub async fn plugin_get_latest_operation(
    runtime: State<'_, PluginRuntime>,
) -> Result<Option<OperationSnapshot>, PluginCommandError> {
    runtime.latest_operation()
}

#[tauri::command]
pub async fn plugin_confirm_plugin_launch(
    app_id: String,
    version: String,
    runtime: State<'_, PluginRuntime>,
) -> Result<bool, PluginCommandError> {
    runtime.confirm_plugin_launch(&app_id, &version)
}

#[tauri::command]
pub async fn plugin_rollback_plugin_launch(
    app_id: String,
    version: String,
    runtime: State<'_, PluginRuntime>,
    native_services: State<'_, NativeServiceRuntime>,
    auxiliary_windows: State<'_, AuxiliaryWindowRuntime>,
    app: AppHandle,
) -> Result<Option<InstalledPlugin>, PluginCommandError> {
    let current = runtime
        .list()?
        .into_iter()
        .find(|plugin| plugin.app_id == app_id);
    let was_connected = native_services.is_connected(&app_id);
    native_services.disconnect(&app_id).await?;
    if let Err(error) = auxiliary_windows.close_for_app(&app, &app_id) {
        restore_native_service(&native_services, &app_id, was_connected).await;
        return Err(error.into());
    }
    let rolled_back = match runtime.rollback_plugin_launch(&app_id, &version, &app) {
        Ok(rolled_back) => rolled_back,
        Err(error) => {
            restore_native_service(&native_services, &app_id, was_connected).await;
            return Err(error);
        }
    };
    if was_connected
        || rolled_back
            .as_ref()
            .or(current.as_ref())
            .is_some_and(should_connect_background)
    {
        connect_native_service_observed(&native_services, &app_id).await;
    }
    close_auxiliary_windows_observed(&auxiliary_windows, &app, &app_id);
    Ok(rolled_back)
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{Read, Write},
        net::TcpStream,
    };

    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};
    use zip::{write::SimpleFileOptions, ZipWriter};

    use super::{
        operation_error_message, CatalogOperationMode, PendingCatalogInstall, PluginRuntime,
        P0_TEST_PUBLIC_KEY,
    };
    use crate::plugin::install::PluginStore;
    use crate::plugin::remote_install::{OperationSnapshot, OperationStage};

    fn operation(operation_id: &str, stage: OperationStage) -> OperationSnapshot {
        OperationSnapshot {
            action: "install".to_string(),
            operation_id: operation_id.to_string(),
            correlation_id: format!("correlation-{operation_id}"),
            app_id: "com.attson.codec".to_string(),
            name: "编码转换".to_string(),
            version: "1.0.0".to_string(),
            publisher_id: "com.attson".to_string(),
            publisher_name: "Attson".to_string(),
            publisher_key_id: "codec-key".to_string(),
            entry: None,
            windows: std::collections::BTreeMap::new(),
            permissions: vec!["host.theme.read".to_string()],
            native_service: None,
            stage,
            steps: vec![stage.as_str().to_string()],
            downloaded_bytes: 0,
            total_bytes: 1,
            error_code: None,
            message: None,
            final_available_version: None,
        }
    }

    fn signed_test_package(directory: &std::path::Path) -> std::path::PathBuf {
        const SEED: [u8; 32] = [
            0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
            0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03,
            0x1c, 0xae, 0x7f, 0x60,
        ];
        let path = directory.join("frame-test.atpkg");
        let signing_key = SigningKey::from_bytes(&SEED);
        let license = b"P0 test license";
        let entry = b"<!doctype html><title>Frame test</title>";
        let script = b"globalThis.frameTest = true;";
        let sha256 = |bytes: &[u8]| hex::encode(Sha256::digest(bytes));
        let manifest = format!(
            concat!(
                "{{\"appId\":\"com.attson.frame-test\",",
                "\"files\":{{\"LICENSE\":\"{}\",\"ui/app.js\":\"{}\",\"ui/index.html\":\"{}\"}},",
                "\"hostApi\":{{\"max\":\"1.x\",\"min\":\"1.0.0\"}},",
                "\"name\":\"Frame test\",",
                "\"permissions\":[\"host.theme.read\"],",
                "\"publisher\":{{\"id\":\"com.attson\",\"keyId\":\"attool-p0-test\",\"name\":\"Attson\"}},",
                "\"schemaVersion\":1,",
                "\"target\":{{\"arch\":\"{}\",\"os\":\"{}\"}},",
                "\"ui\":{{\"entry\":\"ui/index.html\"}},",
                "\"version\":\"0.1.0-alpha.0\"}}"
            ),
            sha256(license),
            sha256(script),
            sha256(entry),
            std::env::consts::ARCH,
            std::env::consts::OS,
        );
        let signature = base64::engine::general_purpose::STANDARD
            .encode(signing_key.sign(manifest.as_bytes()).to_bytes());
        let mut archive = ZipWriter::new(File::create(&path).unwrap());
        for (name, bytes) in [
            ("manifest.json", manifest.as_bytes()),
            ("manifest.sig", signature.as_bytes()),
            ("LICENSE", license.as_slice()),
            ("ui/index.html", entry.as_slice()),
            ("ui/app.js", script.as_slice()),
        ] {
            archive
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        path
    }

    fn get(url: &str) -> String {
        let parsed = url::Url::parse(url).unwrap();
        let address = format!("{}:{}", parsed.host_str().unwrap(), parsed.port().unwrap());
        let mut stream = TcpStream::connect(&address).unwrap();
        write!(
            stream,
            "GET {} HTTP/1.1\r\nHost: {address}\r\n\r\n",
            parsed.path()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn p0_runtime_trusts_the_sdk_test_key() {
        let directory = tempfile::tempdir().unwrap();
        let runtime =
            PluginRuntime::p0(PluginStore::open(directory.path().join("plugins")).unwrap())
                .unwrap();
        let seed = [
            0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
            0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03,
            0x1c, 0xae, 0x7f, 0x60,
        ];

        assert_eq!(
            SigningKey::from_bytes(&seed).verifying_key().to_bytes(),
            P0_TEST_PUBLIC_KEY
        );
        assert!(runtime.trusted_keys.contains_key("attool-p0-test"));
    }

    #[test]
    fn installed_and_probe_frames_share_verified_session_delivery() {
        let installed_directory = tempfile::tempdir().unwrap();
        let installed_package = signed_test_package(installed_directory.path());
        let installed_runtime = PluginRuntime::p0(
            PluginStore::open(installed_directory.path().join("plugins")).unwrap(),
        )
        .unwrap();
        installed_runtime.install(installed_package).unwrap();
        let installed = installed_runtime
            .create_frame_session("com.attson.frame-test".to_string(), None)
            .unwrap();
        assert!(get(&installed.url).contains("<title>Frame test</title>"));
        installed_runtime.revoke_frame_session(&installed.session_id);

        let probe_directory = tempfile::tempdir().unwrap();
        let probe_package = signed_test_package(probe_directory.path());
        let probe_runtime =
            PluginRuntime::p0(PluginStore::open(probe_directory.path().join("plugins")).unwrap())
                .unwrap();
        let prepared = probe_runtime
            .store
            .lock()
            .unwrap()
            .prepare_install(
                &probe_package,
                &probe_runtime.environment,
                &probe_runtime.trusted_keys,
                "probe-operation",
            )
            .unwrap();
        probe_runtime.pending.lock().unwrap().insert(
            "probe-operation".to_string(),
            PendingCatalogInstall {
                prepared,
                mode: CatalogOperationMode::Install,
            },
        );
        let probe = probe_runtime
            .create_probe_frame_session("probe-operation".to_string(), None)
            .unwrap();
        assert!(get(&probe.url).contains("<title>Frame test</title>"));
        probe_runtime.revoke_frame_session(&probe.session_id);
    }

    #[test]
    fn rejects_a_second_active_install_for_the_same_application() {
        let directory = tempfile::tempdir().unwrap();
        let runtime =
            PluginRuntime::p0(PluginStore::open(directory.path().join("plugins")).unwrap())
                .unwrap();
        runtime
            .reserve_operation(operation("operation-1", OperationStage::Download))
            .unwrap();

        let error = runtime
            .reserve_operation(operation("operation-2", OperationStage::Download))
            .unwrap_err();

        assert_eq!(error.code, "PLUGIN_INSTALL_IN_PROGRESS");
        assert_eq!(error.correlation_id, "correlation-operation-2");
    }

    #[test]
    fn allows_active_installs_for_different_applications() {
        let directory = tempfile::tempdir().unwrap();
        let runtime =
            PluginRuntime::p0(PluginStore::open(directory.path().join("plugins")).unwrap())
                .unwrap();
        runtime
            .reserve_operation(operation("operation-1", OperationStage::Download))
            .unwrap();
        let mut other = operation("operation-2", OperationStage::Download);
        other.app_id = "com.attson.other".to_string();

        runtime.reserve_operation(other).unwrap();
    }

    #[test]
    fn failed_operation_persistence_does_not_block_a_retry() {
        let directory = tempfile::tempdir().unwrap();
        let store_root = directory.path().join("plugins");
        let runtime = PluginRuntime::p0(PluginStore::open(store_root.clone()).unwrap()).unwrap();
        let connection = rusqlite::Connection::open(store_root.join("registry.sqlite3")).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_operation_insert
                 BEFORE INSERT ON plugin_operations
                 BEGIN SELECT RAISE(ABORT, 'forced failure'); END;",
            )
            .unwrap();

        let error = runtime
            .reserve_operation(operation("operation-1", OperationStage::Download))
            .unwrap_err();
        assert_eq!(error.code, "PLUGIN_REGISTRY_WRITE_FAILED");

        connection
            .execute_batch("DROP TRIGGER reject_operation_insert;")
            .unwrap();
        runtime
            .reserve_operation(operation("operation-2", OperationStage::Download))
            .unwrap();
    }

    #[test]
    fn operation_events_use_stable_public_error_messages() {
        assert_eq!(
            operation_error_message("PLUGIN_ROLLBACK_FAILED"),
            "Automatic rollback failed; manual recovery is required"
        );
        assert_eq!(
            operation_error_message("UNEXPECTED_INTERNAL_PATH_ERROR"),
            "Application operation failed"
        );
    }
}
