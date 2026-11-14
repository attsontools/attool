use std::{fs, fs::OpenOptions};

use attool_lib::ecommerce::{EcommerceStore, LegacyTemplateOwner};
use fs2::FileExt;

#[test]
fn legacy_owner_writes_capability_and_enters_without_a_takeover_marker() {
    let directory = tempfile::tempdir().unwrap();

    let owner = LegacyTemplateOwner::new(directory.path().to_path_buf()).unwrap();
    let guard = owner.enter().unwrap();

    assert!(directory
        .path()
        .join("template-takeover-capability-v1")
        .is_file());
    drop(guard);
}

#[test]
fn legacy_owner_rejects_migrating_and_owned_markers() {
    for state in ["migrating", "owned"] {
        let directory = tempfile::tempdir().unwrap();
        let owner = LegacyTemplateOwner::new(directory.path().to_path_buf()).unwrap();
        fs::write(
            directory.path().join("template-takeover.json"),
            format!(r#"{{"protocolVersion":1,"appId":"com.attson.template","state":"{state}"}}"#),
        )
        .unwrap();

        assert_eq!(owner.enter().unwrap_err(), "TEMPLATE_SERVICE_OWNS_DATA");
    }
}

#[test]
fn legacy_owner_rejects_an_active_service_lock_before_touching_data() {
    let directory = tempfile::tempdir().unwrap();
    let owner = LegacyTemplateOwner::new(directory.path().to_path_buf()).unwrap();
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(directory.path().join("template-owner.lock"))
        .unwrap();
    lock.try_lock_exclusive().unwrap();

    assert_eq!(owner.enter().unwrap_err(), "TEMPLATE_SERVICE_OWNS_DATA");
}

#[test]
fn malformed_takeover_marker_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let owner = LegacyTemplateOwner::new(directory.path().to_path_buf()).unwrap();
    fs::write(directory.path().join("template-takeover.json"), b"not-json").unwrap();

    assert_eq!(owner.enter().unwrap_err(), "TEMPLATE_SERVICE_OWNS_DATA");
}

#[test]
fn unknown_takeover_state_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let owner = LegacyTemplateOwner::new(directory.path().to_path_buf()).unwrap();
    fs::write(
        directory.path().join("template-takeover.json"),
        br#"{"protocolVersion":1,"appId":"com.attson.template","state":"future"}"#,
    )
    .unwrap();

    assert_eq!(owner.enter().unwrap_err(), "TEMPLATE_SERVICE_OWNS_DATA");
}

#[test]
fn legacy_store_startup_does_not_recreate_an_owned_database() {
    let directory = tempfile::tempdir().unwrap();
    let owner = LegacyTemplateOwner::new(directory.path().to_path_buf()).unwrap();
    fs::write(
        directory.path().join("template-takeover.json"),
        br#"{"protocolVersion":1,"appId":"com.attson.template","state":"owned"}"#,
    )
    .unwrap();

    EcommerceStore::new_for_legacy(directory.path().to_path_buf(), &owner).unwrap();

    assert!(!directory
        .path()
        .join("ecommerce_templates.sqlite3")
        .exists());
}
