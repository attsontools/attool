use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
};

use fs2::FileExt;

pub const CLIPBOARD_TAKEN_OVER: &str = "CLIPBOARD_SERVICE_OWNS_DATA";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TakeoverMarkerState {
    Migrating,
    Owned,
}

#[derive(Clone, Debug)]
pub struct LegacyClipboardOwner {
    root_dir: PathBuf,
}

impl LegacyClipboardOwner {
    pub fn new(root_dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root_dir)
            .map_err(|error| format!("创建剪贴板数据目录失败：{error}"))?;
        fs::write(root_dir.join("clipboard-takeover-capability-v1"), b"1\n")
            .map_err(|error| format!("写入剪贴板接管能力标记失败：{error}"))?;
        Ok(Self { root_dir })
    }

    pub fn enter(&self) -> Result<LegacyClipboardGuard, String> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(self.root_dir.join("clipboard-owner.lock"))
            .map_err(|_| CLIPBOARD_TAKEN_OVER.to_string())?;
        file.try_lock_exclusive()
            .map_err(|_| CLIPBOARD_TAKEN_OVER.to_string())?;
        match self.takeover_marker_state()? {
            Some(TakeoverMarkerState::Owned) => {
                return Err(CLIPBOARD_TAKEN_OVER.to_string());
            }
            Some(TakeoverMarkerState::Migrating) => {
                fs::remove_file(self.root_dir.join("clipboard-takeover.json"))
                    .map_err(|_| CLIPBOARD_TAKEN_OVER.to_string())?;
            }
            None => {}
        }
        Ok(LegacyClipboardGuard { file })
    }

    pub fn is_permanently_taken_over(&self) -> bool {
        match self.takeover_marker_state() {
            Ok(Some(TakeoverMarkerState::Owned)) | Err(_) => true,
            Ok(Some(TakeoverMarkerState::Migrating) | None) => false,
        }
    }

    fn takeover_marker_state(&self) -> Result<Option<TakeoverMarkerState>, String> {
        let marker = self.root_dir.join("clipboard-takeover.json");
        if !marker.exists() {
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(&marker).map_err(|_| CLIPBOARD_TAKEN_OVER.to_string())?,
        )
        .map_err(|_| CLIPBOARD_TAKEN_OVER.to_string())?;
        if value
            .get("protocolVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
            || value.get("appId").and_then(serde_json::Value::as_str)
                != Some("com.attson.clipboard")
            || value
                .get("schemaVersion")
                .and_then(serde_json::Value::as_u64)
                != Some(1)
            || value
                .get("nonce")
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err(CLIPBOARD_TAKEN_OVER.to_string());
        }
        match value.get("state").and_then(serde_json::Value::as_str) {
            Some("owned") => Ok(Some(TakeoverMarkerState::Owned)),
            Some("migrating") => Ok(Some(TakeoverMarkerState::Migrating)),
            _ => Err(CLIPBOARD_TAKEN_OVER.to_string()),
        }
    }
}

#[derive(Debug)]
pub struct LegacyClipboardGuard {
    file: File,
}

impl Drop for LegacyClipboardGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::fs::OpenOptions;

    use fs2::FileExt;

    use super::{LegacyClipboardOwner, CLIPBOARD_TAKEN_OVER};

    #[test]
    fn legacy_owner_refuses_access_while_the_service_holds_the_lifecycle_lock() {
        let temporary = tempfile::tempdir().unwrap();
        let owner = LegacyClipboardOwner::new(temporary.path().to_path_buf()).unwrap();
        assert!(temporary
            .path()
            .join("clipboard-takeover-capability-v1")
            .is_file());

        let service_lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(temporary.path().join("clipboard-owner.lock"))
            .unwrap();
        service_lock.lock_exclusive().unwrap();

        assert_eq!(owner.enter().unwrap_err(), CLIPBOARD_TAKEN_OVER);
    }

    #[test]
    fn owned_marker_permanently_disables_the_legacy_owner() {
        let temporary = tempfile::tempdir().unwrap();
        let owner = LegacyClipboardOwner::new(temporary.path().to_path_buf()).unwrap();
        fs::write(
            temporary.path().join("clipboard-takeover.json"),
            br#"{"protocolVersion":1,"appId":"com.attson.clipboard","schemaVersion":1,"state":"owned","nonce":"service"}"#,
        )
        .unwrap();

        assert!(owner.is_permanently_taken_over());
        assert_eq!(owner.enter().unwrap_err(), CLIPBOARD_TAKEN_OVER);
    }

    #[test]
    fn stale_migrating_marker_restores_the_legacy_owner() {
        let temporary = tempfile::tempdir().unwrap();
        let owner = LegacyClipboardOwner::new(temporary.path().to_path_buf()).unwrap();
        let marker = temporary.path().join("clipboard-takeover.json");
        fs::write(
            &marker,
            br#"{"protocolVersion":1,"appId":"com.attson.clipboard","schemaVersion":1,"state":"migrating","nonce":"service"}"#,
        )
        .unwrap();

        assert!(!owner.is_permanently_taken_over());
        drop(owner.enter().unwrap());
        assert!(!marker.exists());
    }
}
