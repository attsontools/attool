use std::path::PathBuf;

use tauri::State;

use super::{
    models::{
        BatchOutputItem, BatchRunMode, BatchTaskInput, TemplateAsset, TemplateProject,
        TemplateSummary,
    },
    psd_bridge::import_psd,
    render::run_batch_tasks,
    storage::EcommerceStore,
    LegacyTemplateOwner,
};

#[tauri::command]
pub async fn import_psd_template(
    psd_path: String,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<TemplateProject, String> {
    let _owner = owner.enter()?;
    let project = import_psd(PathBuf::from(psd_path).as_path(), store.root_dir())?;
    store.save_template(project)
}

#[tauri::command]
pub async fn list_ecommerce_templates(
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<Vec<TemplateSummary>, String> {
    let _owner = owner.enter()?;
    store.list_templates()
}

#[tauri::command]
pub async fn load_ecommerce_template(
    id: String,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<TemplateProject, String> {
    let _owner = owner.enter()?;
    store.load_template(&id)
}

#[tauri::command]
pub async fn save_ecommerce_template(
    project: TemplateProject,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<TemplateProject, String> {
    let _owner = owner.enter()?;
    store.save_template(project)
}

#[tauri::command]
pub async fn delete_ecommerce_template(
    id: String,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<(), String> {
    let _owner = owner.enter()?;
    store.delete_template(&id)
}

#[tauri::command]
pub async fn rename_ecommerce_template(
    id: String,
    name: String,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<TemplateProject, String> {
    let _owner = owner.enter()?;
    store.rename_template(&id, &name)
}

#[tauri::command]
pub async fn save_template_asset(
    name: String,
    mime_type: String,
    bytes: Vec<u8>,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<TemplateAsset, String> {
    let _owner = owner.enter()?;
    store.save_asset(name, mime_type, bytes)
}

#[tauri::command]
pub async fn import_template_asset_from_path(
    source_path: String,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<TemplateAsset, String> {
    let _owner = owner.enter()?;
    let source = PathBuf::from(&source_path);
    let bytes = std::fs::read(&source).map_err(|error| format!("读取图片失败：{error}"))?;
    let name = source
        .file_name()
        .map(|os| os.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_string());
    store.save_asset(name, String::new(), bytes)
}

#[tauri::command]
pub async fn list_template_assets(
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<Vec<TemplateAsset>, String> {
    let _owner = owner.enter()?;
    store.list_assets()
}

#[tauri::command]
pub async fn delete_template_asset(
    asset_id: String,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<(), String> {
    let _owner = owner.enter()?;
    store.delete_asset(&asset_id)
}

#[tauri::command]
pub async fn run_batch_replace_tasks(
    template_id: String,
    tasks: Vec<BatchTaskInput>,
    mode: Option<BatchRunMode>,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<Vec<BatchOutputItem>, String> {
    let _owner = owner.enter()?;
    run_batch_tasks(&store, &template_id, &tasks, mode.unwrap_or_default())
}

#[tauri::command]
pub async fn save_batch_replace_outputs(
    file_paths: Vec<String>,
    target_dir: String,
    store: State<'_, EcommerceStore>,
    owner: State<'_, LegacyTemplateOwner>,
) -> Result<usize, String> {
    let _owner = owner.enter()?;
    store.save_batch_outputs(&file_paths, &target_dir)
}
