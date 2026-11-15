use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
};

use fs2::FileExt;

pub const TEMPLATE_TAKEN_OVER: &str = "TEMPLATE_SERVICE_OWNS_DATA";

#[derive(Clone, Debug)]
pub struct LegacyTemplateOwner {
    root_dir: PathBuf,
}

impl LegacyTemplateOwner {
    pub fn new(root_dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root_dir).map_err(|error| format!("创建模板数据目录失败：{error}"))?;
        fs::write(root_dir.join("template-takeover-capability-v1"), b"1\n")
            .map_err(|error| format!("写入模板接管能力标记失败：{error}"))?;
        Ok(Self { root_dir })
    }

    pub fn enter(&self) -> Result<LegacyTemplateGuard, String> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(self.root_dir.join("template-owner.lock"))
            .map_err(|_| TEMPLATE_TAKEN_OVER.to_string())?;
        file.try_lock_exclusive()
            .map_err(|_| TEMPLATE_TAKEN_OVER.to_string())?;
        if self.service_has_claimed_data() {
            return Err(TEMPLATE_TAKEN_OVER.to_string());
        }
        Ok(LegacyTemplateGuard { file })
    }

    fn service_has_claimed_data(&self) -> bool {
        let marker = self.root_dir.join("template-takeover.json");
        if !marker.exists() {
            return false;
        }
        let Ok(bytes) = fs::read(marker) else {
            return true;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return true;
        };
        if value
            .get("protocolVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
            || value.get("appId").and_then(serde_json::Value::as_str) != Some("com.attson.template")
        {
            return true;
        }
        true
    }
}

#[derive(Debug)]
pub struct LegacyTemplateGuard {
    file: File,
}

impl Drop for LegacyTemplateGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
