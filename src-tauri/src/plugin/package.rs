use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::Path,
};

use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{
    de::{MapAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 1024;
const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_NATIVE_FILE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct HostEnvironment {
    pub os: String,
    pub arch: String,
    pub host_api_version: Version,
}

impl HostEnvironment {
    pub fn current(host_api_version: &str) -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            host_api_version: Version::parse(host_api_version).expect("valid host API version"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectedPackage {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub publisher_id: String,
    pub publisher_name: String,
    pub publisher_key_id: String,
    pub target_os: String,
    pub target_arch: String,
    pub host_api_min: String,
    pub host_api_max: String,
    pub entry: String,
    pub windows: BTreeMap<String, AuxiliaryWindow>,
    pub permissions: Vec<String>,
    pub native_service: Option<NativeService>,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeService {
    pub entry: String,
    pub version: String,
    pub sha256: String,
    pub protocol_version: u32,
    #[serde(default = "default_native_service_lifecycle")]
    pub lifecycle: String,
}

fn default_native_service_lifecycle() -> String {
    "onDemand".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuxiliaryWindow {
    pub entry: String,
    pub presentation: String,
}

#[derive(Debug)]
pub struct PackageError {
    pub code: &'static str,
    pub message: String,
}

impl PackageError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PackageError {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    app_id: String,
    name: String,
    version: String,
    publisher: Publisher,
    target: Target,
    host_api: HostApi,
    ui: Ui,
    permissions: Vec<String>,
    #[serde(default)]
    native_service: Option<NativeService>,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Publisher {
    id: String,
    name: String,
    key_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    os: String,
    arch: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostApi {
    min: String,
    max: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ui {
    entry: String,
    #[serde(default, deserialize_with = "deserialize_auxiliary_windows")]
    windows: BTreeMap<String, AuxiliaryWindow>,
}

fn deserialize_auxiliary_windows<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, AuxiliaryWindow>, D::Error>
where
    D: Deserializer<'de>,
{
    struct AuxiliaryWindowsVisitor;

    impl<'de> Visitor<'de> for AuxiliaryWindowsVisitor {
        type Value = BTreeMap<String, AuxiliaryWindow>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an object of auxiliary window declarations")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut windows = BTreeMap::new();
            while let Some((window_id, window)) = map.next_entry::<String, AuxiliaryWindow>()? {
                if windows.insert(window_id.clone(), window).is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate window ID: {window_id}"
                    )));
                }
            }
            Ok(windows)
        }
    }

    deserializer.deserialize_map(AuxiliaryWindowsVisitor)
}

pub fn inspect_package(
    path: &Path,
    environment: &HostEnvironment,
    trusted_keys: &HashMap<String, VerifyingKey>,
) -> Result<InspectedPackage, PackageError> {
    let file = File::open(path).map_err(|error| {
        PackageError::new(
            "PLUGIN_PACKAGE_READ_FAILED",
            format!("Cannot open package: {error}"),
        )
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        PackageError::new(
            "PLUGIN_PACKAGE_INVALID",
            format!("Invalid package archive: {error}"),
        )
    })?;
    let manifest_bytes = read_entry(&mut archive, "manifest.json", MAX_MANIFEST_BYTES)?;
    let signature_bytes = read_entry(&mut archive, "manifest.sig", MAX_SIGNATURE_BYTES)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        PackageError::new(
            "PLUGIN_MANIFEST_INVALID",
            format!("Invalid manifest: {error}"),
        )
    })?;
    let manifest_value: Value = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        PackageError::new(
            "PLUGIN_MANIFEST_INVALID",
            format!("Invalid manifest JSON: {error}"),
        )
    })?;

    verify_archive_layout(&mut archive, &manifest)?;
    validate_manifest(&manifest, environment)?;
    verify_signature(&manifest, &manifest_value, &signature_bytes, trusted_keys)?;
    verify_files(&mut archive, &manifest)?;

    Ok(InspectedPackage {
        app_id: manifest.app_id,
        name: manifest.name,
        version: manifest.version,
        publisher_id: manifest.publisher.id,
        publisher_name: manifest.publisher.name,
        publisher_key_id: manifest.publisher.key_id,
        target_os: manifest.target.os,
        target_arch: manifest.target.arch,
        host_api_min: manifest.host_api.min,
        host_api_max: manifest.host_api.max,
        entry: manifest.ui.entry,
        windows: manifest.ui.windows,
        permissions: manifest.permissions,
        native_service: manifest.native_service,
        files: manifest.files,
    })
}

pub fn extract_package(
    path: &Path,
    destination: &Path,
    environment: &HostEnvironment,
    trusted_keys: &HashMap<String, VerifyingKey>,
) -> Result<InspectedPackage, PackageError> {
    let package = inspect_package(path, environment, trusted_keys)?;
    if destination.exists() {
        return Err(PackageError::new(
            "PLUGIN_STAGING_CONFLICT",
            "Plugin staging destination already exists",
        ));
    }
    fs::create_dir_all(destination).map_err(|error| {
        PackageError::new(
            "PLUGIN_STAGING_FAILED",
            format!("Cannot create staging directory: {error}"),
        )
    })?;

    let extraction = (|| {
        let file = File::open(path).map_err(|error| {
            PackageError::new(
                "PLUGIN_PACKAGE_READ_FAILED",
                format!("Cannot reopen package: {error}"),
            )
        })?;
        let mut archive = ZipArchive::new(file).map_err(|error| {
            PackageError::new(
                "PLUGIN_PACKAGE_INVALID",
                format!("Invalid package archive: {error}"),
            )
        })?;
        for relative in package.files.keys() {
            let bytes = read_entry(
                &mut archive,
                relative,
                package_file_limit(relative, package.native_service.is_some()),
            )?;
            let output = destination.join(relative);
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    PackageError::new(
                        "PLUGIN_STAGING_FAILED",
                        format!("Cannot create package directory: {error}"),
                    )
                })?;
            }
            fs::write(&output, bytes).map_err(|error| {
                PackageError::new(
                    "PLUGIN_STAGING_FAILED",
                    format!("Cannot write staged package file: {error}"),
                )
            })?;
        }
        Ok(())
    })();

    if let Err(error) = extraction {
        let _ = fs::remove_dir_all(destination);
        return Err(error);
    }
    if let Some(service) = &package.native_service {
        if let Err(error) = set_native_payload_permissions(destination, package.files.keys()) {
            let _ = fs::remove_dir_all(destination);
            return Err(error);
        }
        debug_assert!(package.files.contains_key(&service.entry));
    }
    Ok(package)
}

fn set_native_payload_permissions<'a>(
    destination: &Path,
    files: impl Iterator<Item = &'a String>,
) -> Result<(), PackageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        for relative in files.filter(|relative| relative.starts_with("native/")) {
            let path = destination.join(relative);
            let mut permissions = fs::metadata(&path)
                .map_err(|error| {
                    PackageError::new(
                        "PLUGIN_STAGING_FAILED",
                        format!("Cannot inspect native payload permissions: {error}"),
                    )
                })?
                .permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).map_err(|error| {
                PackageError::new(
                    "PLUGIN_STAGING_FAILED",
                    format!("Cannot mark native payload executable: {error}"),
                )
            })?;
        }
    }
    #[cfg(not(unix))]
    let _ = (destination, files);
    Ok(())
}

fn verify_archive_layout(
    archive: &mut ZipArchive<File>,
    manifest: &Manifest,
) -> Result<(), PackageError> {
    let mut allowed = manifest
        .files
        .keys()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    allowed.insert("manifest.json");
    allowed.insert("manifest.sig");
    let mut seen = HashSet::new();

    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            PackageError::new(
                "PLUGIN_PACKAGE_INVALID",
                format!("Cannot inspect package entry: {error}"),
            )
        })?;
        let name = entry.name().to_string();
        if entry.is_dir() || validate_package_path(&name).is_err() {
            return Err(PackageError::new(
                "PLUGIN_FILE_PATH_INVALID",
                format!("Invalid package entry: {name}"),
            ));
        }
        if !seen.insert(name.clone()) {
            return Err(PackageError::new(
                "PLUGIN_FILE_DUPLICATE",
                format!("Duplicate package entry: {name}"),
            ));
        }
        if !allowed.contains(name.as_str()) {
            return Err(PackageError::new(
                "PLUGIN_FILE_UNDECLARED",
                format!("Undeclared package file: {name}"),
            ));
        }
    }
    Ok(())
}

fn validate_manifest(
    manifest: &Manifest,
    environment: &HostEnvironment,
) -> Result<(), PackageError> {
    if manifest.schema_version != 1 {
        return Err(PackageError::new(
            "PLUGIN_MANIFEST_SCHEMA_UNSUPPORTED",
            "Only manifest schema version 1 is supported",
        ));
    }
    let app_id_pattern =
        regex::Regex::new(r"^[a-z0-9]+(?:[.-][a-z0-9]+)+$").expect("app ID pattern is valid");
    if !app_id_pattern.is_match(&manifest.app_id) {
        return Err(PackageError::new(
            "PLUGIN_APP_ID_INVALID",
            "Package appId is invalid",
        ));
    }
    Version::parse(&manifest.version).map_err(|_| {
        PackageError::new(
            "PLUGIN_VERSION_INVALID",
            "Package version must be a valid semantic version",
        )
    })?;
    if manifest.permissions.iter().any(|permission| {
        !matches!(
            permission.as_str(),
            "host.theme.read"
                | "host.file.save"
                | "host.notification.show"
                | "host.http.request"
                | "host.http.stream"
                | "host.http.data"
                | "host.download.tasks"
                | "host.template.project"
                | "host.ui.auxiliary"
                | "host.clipboard.history"
                | "host.app.activate"
                | "host.clipboard.write"
                | "host.external.open"
                | "host.video.extract"
        )
    }) {
        return Err(PackageError::new(
            "PLUGIN_PERMISSION_UNKNOWN",
            "Package requests an unknown permission",
        ));
    }
    if manifest.target.os != environment.os || manifest.target.arch != environment.arch {
        return Err(PackageError::new(
            "PLUGIN_TARGET_MISMATCH",
            format!(
                "Package target {}/{} does not match host {}/{}",
                manifest.target.os, manifest.target.arch, environment.os, environment.arch
            ),
        ));
    }
    let host_min = Version::parse(&manifest.host_api.min).map_err(|_| {
        PackageError::new(
            "PLUGIN_HOST_API_UNSUPPORTED",
            "Invalid minimum Host API version",
        )
    })?;
    let supported_major = host_min.major == 1 || host_min.major == 2;
    if manifest.host_api.max != format!("{}.x", host_min.major)
        || !supported_major
        || environment.host_api_version.major < host_min.major
        || (environment.host_api_version.major == host_min.major
            && environment.host_api_version < host_min)
    {
        return Err(PackageError::new(
            "PLUGIN_HOST_API_UNSUPPORTED",
            "Package requires an unsupported Host API version",
        ));
    }
    if !manifest.files.contains_key(&manifest.ui.entry) {
        return Err(PackageError::new(
            "PLUGIN_ENTRY_MISSING",
            "UI entry is not declared in manifest files",
        ));
    }
    let window_id_pattern =
        regex::Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("window ID pattern is valid");
    if manifest.ui.windows.len() > 4
        || manifest.ui.windows.iter().any(|(window_id, window)| {
            !window_id_pattern.is_match(window_id)
                || validate_package_path(&window.entry).is_err()
                || !manifest.files.contains_key(&window.entry)
                || !matches!(
                    window.presentation.as_str(),
                    "bottomPanel" | "centeredDialog"
                )
        })
    {
        return Err(PackageError::new(
            "PLUGIN_UI_WINDOW_INVALID",
            "Auxiliary window declaration is invalid",
        ));
    }
    if let Some(service) = &manifest.native_service {
        let service_hash = manifest.files.get(&service.entry);
        if validate_package_path(&service.entry).is_err()
            || service.version != manifest.version
            || service.protocol_version != 1
            || service_hash != Some(&service.sha256)
            || !matches!(service.lifecycle.as_str(), "onDemand" | "background")
        {
            return Err(PackageError::new(
                "PLUGIN_NATIVE_SERVICE_INVALID",
                "Native service identity does not match the signed package",
            ));
        }
    }
    let requests_native_service = manifest.permissions.iter().any(|permission| {
        permission.starts_with("host.http.")
            || permission == "host.download.tasks"
            || permission == "host.template.project"
            || permission == "host.clipboard.history"
            || permission == "host.video.extract"
    });
    if (host_min.major == 2) != manifest.native_service.is_some()
        || (requests_native_service && manifest.native_service.is_none())
    {
        return Err(PackageError::new(
            "PLUGIN_NATIVE_SERVICE_REQUIRED",
            "Native services and their Host permissions require Host API 2.x",
        ));
    }
    if manifest
        .native_service
        .as_ref()
        .is_some_and(|service| service.lifecycle == "background")
        && !requests_native_service
    {
        return Err(PackageError::new(
            "PLUGIN_NATIVE_SERVICE_REQUIRED",
            "Background native services require a Native Service Host permission",
        ));
    }
    Ok(())
}

fn verify_signature(
    manifest: &Manifest,
    manifest_value: &Value,
    signature_bytes: &[u8],
    trusted_keys: &HashMap<String, VerifyingKey>,
) -> Result<(), PackageError> {
    let key = trusted_keys
        .get(&manifest.publisher.key_id)
        .ok_or_else(|| {
            PackageError::new(
                "PLUGIN_PUBLISHER_UNTRUSTED",
                "Package publisher key is not trusted",
            )
        })?;
    let signature_text = std::str::from_utf8(signature_bytes).map_err(|_| {
        PackageError::new("PLUGIN_SIGNATURE_INVALID", "Package signature is not UTF-8")
    })?;
    let signature_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_text.trim())
        .map_err(|_| {
            PackageError::new(
                "PLUGIN_SIGNATURE_INVALID",
                "Package signature is not valid base64",
            )
        })?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| {
        PackageError::new(
            "PLUGIN_SIGNATURE_INVALID",
            "Package signature has an invalid length",
        )
    })?;
    let canonical = canonical_json(manifest_value).map_err(|error| {
        PackageError::new(
            "PLUGIN_MANIFEST_INVALID",
            format!("Cannot canonicalize manifest: {error}"),
        )
    })?;
    key.verify_strict(canonical.as_bytes(), &signature)
        .map_err(|_| PackageError::new("PLUGIN_SIGNATURE_INVALID", "Package signature is invalid"))
}

fn verify_files(archive: &mut ZipArchive<File>, manifest: &Manifest) -> Result<(), PackageError> {
    for (path, expected_hash) in &manifest.files {
        validate_package_path(path)?;
        let bytes = read_entry(
            archive,
            path,
            package_file_limit(path, manifest.native_service.is_some()),
        )?;
        let actual_hash = hex::encode(Sha256::digest(&bytes));
        if &actual_hash != expected_hash {
            return Err(PackageError::new(
                "PLUGIN_FILE_HASH_MISMATCH",
                format!("Package file hash does not match: {path}"),
            ));
        }
    }
    Ok(())
}

fn package_file_limit(path: &str, has_native_service: bool) -> u64 {
    if has_native_service && path.starts_with("native/") {
        MAX_NATIVE_FILE_BYTES
    } else {
        MAX_FILE_BYTES
    }
}

fn validate_package_path(path: &str) -> Result<(), PackageError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(PackageError::new(
            "PLUGIN_FILE_PATH_INVALID",
            format!("Unsafe package path: {path}"),
        ));
    }
    Ok(())
}

fn read_entry(
    archive: &mut ZipArchive<File>,
    name: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, PackageError> {
    let mut entry = archive.by_name(name).map_err(|_| {
        PackageError::new(
            "PLUGIN_FILE_MISSING",
            format!("Package file is missing: {name}"),
        )
    })?;
    if entry.is_dir() || entry.size() > max_bytes {
        return Err(PackageError::new(
            "PLUGIN_FILE_INVALID",
            format!("Package file is invalid or too large: {name}"),
        ));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes).map_err(|error| {
        PackageError::new(
            "PLUGIN_PACKAGE_READ_FAILED",
            format!("Cannot read package file {name}: {error}"),
        )
    })?;
    Ok(bytes)
}

fn canonical_json(value: &Value) -> Result<String, serde_json::Error> {
    match value {
        Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), canonical_value(value)))
                .collect::<BTreeMap<_, _>>();
            serde_json::to_string(&sorted)
        }
        _ => serde_json::to_string(&canonical_value(value)),
    }
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), canonical_value(value)))
                .collect::<BTreeMap<_, _>>();
            serde_json::to_value(sorted).expect("JSON value is serializable")
        }
        value => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs::{File, OpenOptions},
        io::Write,
    };

    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};
    use zip::{write::SimpleFileOptions, ZipWriter};

    use super::{
        canonical_json, extract_package, inspect_package, package_file_limit, HostEnvironment,
        MAX_FILE_BYTES, MAX_NATIVE_FILE_BYTES,
    };

    const TEST_KEY_SEED: [u8; 32] = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ];

    #[test]
    fn larger_file_limit_is_scoped_to_native_service_payloads() {
        assert_eq!(
            package_file_limit("native/aria2c", true),
            MAX_NATIVE_FILE_BYTES
        );
        assert_eq!(package_file_limit("native/aria2c", false), MAX_FILE_BYTES);
        assert_eq!(package_file_limit("ui/large.bin", true), MAX_FILE_BYTES);
    }

    fn sha256(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    fn package_with_index(
        archived_index: &[u8],
        hashed_index: &[u8],
        app_id: &str,
        version: &str,
        permissions_json: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf, SigningKey) {
        package_with_index_host_api(
            archived_index,
            hashed_index,
            app_id,
            version,
            permissions_json,
            1,
        )
    }

    fn package_with_index_host_api(
        archived_index: &[u8],
        hashed_index: &[u8],
        app_id: &str,
        version: &str,
        permissions_json: &str,
        host_api_major: u64,
    ) -> (tempfile::TempDir, std::path::PathBuf, SigningKey) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("codec.atpkg");
        let signing_key = SigningKey::from_bytes(&TEST_KEY_SEED);
        let license = b"P0 test license";
        let manifest = format!(
            concat!(
                "{{\"appId\":\"{}\",",
                "\"files\":{{\"LICENSE\":\"{}\",\"ui/index.html\":\"{}\"}},",
                "\"hostApi\":{{\"max\":\"{}.x\",\"min\":\"{}.0.0\"}},",
                "\"name\":\"编码转换\",",
                "\"permissions\":{},",
                "\"publisher\":{{\"id\":\"com.attson\",\"keyId\":\"attool-p0-test\",\"name\":\"Attson\"}},",
                "\"schemaVersion\":1,",
                "\"target\":{{\"arch\":\"{}\",\"os\":\"{}\"}},",
                "\"ui\":{{\"entry\":\"ui/index.html\"}},",
                "\"version\":\"{}\"}}"
            ),
            app_id,
            sha256(license),
            sha256(hashed_index),
            host_api_major,
            host_api_major,
            permissions_json,
            std::env::consts::ARCH,
            std::env::consts::OS,
            version
        );
        let signature = signing_key.sign(manifest.as_bytes());
        let signature = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());

        let file = File::create(&path).unwrap();
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, bytes) in [
            ("manifest.json", manifest.as_bytes()),
            ("manifest.sig", signature.as_bytes()),
            ("LICENSE", license.as_slice()),
            ("ui/index.html", archived_index),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();

        (directory, path, signing_key)
    }

    fn valid_package() -> (tempfile::TempDir, std::path::PathBuf, SigningKey) {
        let index = b"<!doctype html><title>Codec</title>";
        package_with_index(
            index,
            index,
            "com.attson.codec",
            "0.1.0-alpha.0",
            "[\"host.theme.read\"]",
        )
    }

    fn native_service_package(
        service_version: &str,
        protocol_version: u32,
        declared_hash: Option<&str>,
        host_api_min: &str,
        permissions: serde_json::Value,
    ) -> (tempfile::TempDir, std::path::PathBuf, SigningKey) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("http.atpkg");
        let signing_key = SigningKey::from_bytes(&TEST_KEY_SEED);
        let index = b"<!doctype html><title>HTTP</title>";
        let service = b"native-service";
        let runtime_helper = b"runtime-helper";
        let service_hash = sha256(service);
        let declared_hash = declared_hash.unwrap_or(&service_hash);
        let manifest = serde_json::json!({
            "appId": "com.attson.http",
            "files": {
                "native/athttp-service": service_hash,
                "native/runtime-helper": sha256(runtime_helper),
                "ui/index.html": sha256(index)
            },
            "hostApi": {
                "max": format!("{}.x", host_api_min.split('.').next().unwrap()),
                "min": host_api_min
            },
            "name": "HTTP request",
            "nativeService": {
                "entry": "native/athttp-service",
                "protocolVersion": protocol_version,
                "sha256": declared_hash,
                "version": service_version
            },
            "permissions": permissions,
            "publisher": { "id": "com.attson", "keyId": "attool-p0-test", "name": "Attson" },
            "schemaVersion": 1,
            "target": { "arch": std::env::consts::ARCH, "os": std::env::consts::OS },
            "ui": { "entry": "ui/index.html" },
            "version": "0.1.0-alpha.0"
        });
        let manifest = canonical_json(&manifest).unwrap();
        let signature = base64::engine::general_purpose::STANDARD
            .encode(signing_key.sign(manifest.as_bytes()).to_bytes());
        let file = File::create(&path).unwrap();
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, bytes) in [
            ("manifest.json", manifest.as_bytes()),
            ("manifest.sig", signature.as_bytes()),
            ("ui/index.html", index.as_slice()),
            ("native/athttp-service", service.as_slice()),
            ("native/runtime-helper", runtime_helper.as_slice()),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        (directory, path, signing_key)
    }

    fn clipboard_package(
        lifecycle: &str,
        windows: serde_json::Value,
        permissions: serde_json::Value,
    ) -> (tempfile::TempDir, std::path::PathBuf, SigningKey) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("clipboard.atpkg");
        let signing_key = SigningKey::from_bytes(&TEST_KEY_SEED);
        let index = b"<!doctype html><title>Clipboard</title>";
        let history = b"<!doctype html><title>History</title>";
        let preview = b"<!doctype html><title>Preview</title>";
        let service = b"clipboard-service";
        let service_hash = sha256(service);
        let manifest = serde_json::json!({
            "appId": "com.attson.clipboard",
            "files": {
                "native/atclipboard-service": service_hash,
                "ui/history.html": sha256(history),
                "ui/index.html": sha256(index),
                "ui/preview.html": sha256(preview)
            },
            "hostApi": { "max": "2.x", "min": "2.0.0" },
            "name": "Clipboard",
            "nativeService": {
                "entry": "native/atclipboard-service",
                "lifecycle": lifecycle,
                "protocolVersion": 1,
                "sha256": service_hash,
                "version": "0.1.0-alpha.0"
            },
            "permissions": permissions,
            "publisher": { "id": "com.attson", "keyId": "attool-p0-test", "name": "Attson" },
            "schemaVersion": 1,
            "target": { "arch": std::env::consts::ARCH, "os": std::env::consts::OS },
            "ui": { "entry": "ui/index.html", "windows": windows },
            "version": "0.1.0-alpha.0"
        });
        let manifest = canonical_json(&manifest).unwrap();
        let signature = base64::engine::general_purpose::STANDARD
            .encode(signing_key.sign(manifest.as_bytes()).to_bytes());
        let file = File::create(&path).unwrap();
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, bytes) in [
            ("manifest.json", manifest.as_bytes()),
            ("manifest.sig", signature.as_bytes()),
            ("ui/index.html", index.as_slice()),
            ("ui/history.html", history.as_slice()),
            ("ui/preview.html", preview.as_slice()),
            ("native/atclipboard-service", service.as_slice()),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        (directory, path, signing_key)
    }

    #[test]
    fn inspects_a_valid_signed_package() {
        let (_directory, path, signing_key) = valid_package();
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);
        let environment = HostEnvironment::current("1.0.0");

        let package = inspect_package(&path, &environment, &keys).unwrap();

        assert_eq!(package.app_id, "com.attson.codec");
        assert_eq!(package.version, "0.1.0-alpha.0");
        assert_eq!(package.entry, "ui/index.html");
        assert_eq!(package.permissions, vec!["host.theme.read"]);
    }

    #[test]
    fn inspects_a_signed_native_service_package() {
        let (_directory, path, signing_key) = native_service_package(
            "0.1.0-alpha.0",
            1,
            None,
            "2.0.0",
            serde_json::json!(["host.http.request", "host.http.stream", "host.http.data"]),
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let package = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys).unwrap();

        let service = package.native_service.unwrap();
        assert_eq!(service.entry, "native/athttp-service");
        assert_eq!(service.version, package.version);
        assert_eq!(service.protocol_version, 1);
        assert_eq!(service.sha256, package.files["native/athttp-service"]);
    }

    #[test]
    fn accepts_download_permissions_for_a_host_api_two_native_service() {
        let (_directory, path, signing_key) = native_service_package(
            "0.1.0-alpha.0",
            1,
            None,
            "2.0.0",
            serde_json::json!(["host.download.tasks"]),
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let package = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys).unwrap();

        assert_eq!(package.permissions, ["host.download.tasks"]);
    }

    #[test]
    fn accepts_template_permissions_for_a_host_api_two_native_service() {
        let (_directory, path, signing_key) = native_service_package(
            "0.1.0-alpha.0",
            1,
            None,
            "2.0.0",
            serde_json::json!(["host.template.project"]),
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let package = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys).unwrap();

        assert_eq!(package.permissions, ["host.template.project"]);
    }

    #[test]
    fn accepts_general_app_permissions_without_a_native_service() {
        let index = b"<!doctype html><title>Video</title>";
        let permissions = "[\"host.app.activate\",\"host.clipboard.write\",\"host.external.open\"]";
        let (_directory, path, signing_key) = package_with_index(
            index,
            index,
            "com.attson.video",
            "0.1.0-alpha.0",
            permissions,
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let package = inspect_package(&path, &HostEnvironment::current("1.0.0"), &keys).unwrap();

        assert_eq!(
            package.permissions,
            [
                "host.app.activate",
                "host.clipboard.write",
                "host.external.open"
            ]
        );
    }

    #[test]
    fn accepts_video_extract_only_with_a_host_api_two_native_service() {
        let (_directory, path, signing_key) = native_service_package(
            "0.1.0-alpha.0",
            1,
            None,
            "2.0.0",
            serde_json::json!(["host.video.extract"]),
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);
        let package = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys).unwrap();
        assert_eq!(package.permissions, ["host.video.extract"]);

        let index = b"<!doctype html><title>Video</title>";
        let (_directory, path, signing_key) = package_with_index(
            index,
            index,
            "com.attson.video",
            "0.1.0-alpha.0",
            "[\"host.video.extract\"]",
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);
        let error = inspect_package(&path, &HostEnvironment::current("1.0.0"), &keys)
            .expect_err("video extraction requires a native service");
        assert_eq!(error.code, "PLUGIN_NATIVE_SERVICE_REQUIRED");
    }

    #[test]
    fn inspects_background_service_and_signed_auxiliary_windows() {
        let (_directory, path, signing_key) = clipboard_package(
            "background",
            serde_json::json!({
                "history": { "entry": "ui/history.html", "presentation": "bottomPanel" },
                "preview": { "entry": "ui/preview.html", "presentation": "centeredDialog" }
            }),
            serde_json::json!(["host.ui.auxiliary", "host.clipboard.history"]),
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let package = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys).unwrap();

        assert_eq!(package.native_service.unwrap().lifecycle, "background");
        assert_eq!(package.windows["history"].entry, "ui/history.html");
        assert_eq!(package.windows["history"].presentation, "bottomPanel");
        assert_eq!(package.windows["preview"].presentation, "centeredDialog");
        assert_eq!(
            package.permissions,
            ["host.ui.auxiliary", "host.clipboard.history"]
        );
    }

    #[test]
    fn rejects_invalid_auxiliary_window_declarations() {
        for windows in [
            serde_json::json!({
                "History": { "entry": "ui/history.html", "presentation": "bottomPanel" }
            }),
            serde_json::json!({
                "history": { "entry": "ui/history.html", "presentation": "floating" }
            }),
            serde_json::json!({
                "history": { "entry": "ui/unsigned.html", "presentation": "bottomPanel" }
            }),
            serde_json::json!({
                "one": { "entry": "ui/history.html", "presentation": "bottomPanel" },
                "two": { "entry": "ui/history.html", "presentation": "bottomPanel" },
                "three": { "entry": "ui/history.html", "presentation": "bottomPanel" },
                "four": { "entry": "ui/history.html", "presentation": "bottomPanel" },
                "five": { "entry": "ui/history.html", "presentation": "bottomPanel" }
            }),
        ] {
            let (_directory, path, signing_key) = clipboard_package(
                "background",
                windows,
                serde_json::json!(["host.ui.auxiliary", "host.clipboard.history"]),
            );
            let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

            let error = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys)
                .expect_err("invalid auxiliary windows must be rejected");

            assert_eq!(error.code, "PLUGIN_UI_WINDOW_INVALID");
        }
    }

    #[test]
    fn rejects_duplicate_auxiliary_window_ids_in_the_manifest_json() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("duplicate-window.atpkg");
        let signing_key = SigningKey::from_bytes(&TEST_KEY_SEED);
        let index = b"<!doctype html><title>Clipboard</title>";
        let history = b"<!doctype html><title>History</title>";
        let preview = b"<!doctype html><title>Preview</title>";
        let manifest = format!(
            concat!(
                "{{\"appId\":\"com.attson.clipboard\",",
                "\"files\":{{\"ui/history.html\":\"{}\",\"ui/index.html\":\"{}\",\"ui/preview.html\":\"{}\"}},",
                "\"hostApi\":{{\"max\":\"1.x\",\"min\":\"1.0.0\"}},",
                "\"name\":\"Clipboard\",",
                "\"permissions\":[\"host.theme.read\"],",
                "\"publisher\":{{\"id\":\"com.attson\",\"keyId\":\"attool-p0-test\",\"name\":\"Attson\"}},",
                "\"schemaVersion\":1,",
                "\"target\":{{\"arch\":\"{}\",\"os\":\"{}\"}},",
                "\"ui\":{{\"entry\":\"ui/index.html\",\"windows\":{{",
                "\"history\":{{\"entry\":\"ui/history.html\",\"presentation\":\"bottomPanel\"}},",
                "\"history\":{{\"entry\":\"ui/preview.html\",\"presentation\":\"centeredDialog\"}}",
                "}}}},",
                "\"version\":\"0.1.0-alpha.0\"}}"
            ),
            sha256(history),
            sha256(index),
            sha256(preview),
            std::env::consts::ARCH,
            std::env::consts::OS
        );
        let manifest_value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        let canonical = canonical_json(&manifest_value).unwrap();
        let signature = base64::engine::general_purpose::STANDARD
            .encode(signing_key.sign(canonical.as_bytes()).to_bytes());
        let file = File::create(&path).unwrap();
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, bytes) in [
            ("manifest.json", manifest.as_bytes()),
            ("manifest.sig", signature.as_bytes()),
            ("ui/index.html", index.as_slice()),
            ("ui/history.html", history.as_slice()),
            ("ui/preview.html", preview.as_slice()),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys)
            .expect_err("duplicate auxiliary window IDs must be rejected");

        assert_eq!(error.code, "PLUGIN_MANIFEST_INVALID");
        assert!(error.message.contains("duplicate window ID"));
    }

    #[test]
    fn rejects_background_lifecycle_without_a_native_domain_permission() {
        for (lifecycle, permissions, expected_code) in [
            (
                "always",
                serde_json::json!(["host.ui.auxiliary", "host.clipboard.history"]),
                "PLUGIN_NATIVE_SERVICE_INVALID",
            ),
            (
                "background",
                serde_json::json!(["host.ui.auxiliary"]),
                "PLUGIN_NATIVE_SERVICE_REQUIRED",
            ),
        ] {
            let (_directory, path, signing_key) = clipboard_package(
                lifecycle,
                serde_json::json!({
                    "history": { "entry": "ui/history.html", "presentation": "bottomPanel" }
                }),
                permissions,
            );
            let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

            let error = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys)
                .expect_err("invalid background lifecycle must be rejected");

            assert_eq!(error.code, expected_code);
        }
    }

    #[test]
    fn rejects_a_native_service_with_mismatched_identity() {
        for (version, protocol, hash) in [
            ("0.1.1", 1, None),
            ("0.1.0-alpha.0", 2, None),
            ("0.1.0-alpha.0", 1, Some("0")),
        ] {
            let (_directory, path, signing_key) = native_service_package(
                version,
                protocol,
                hash,
                "2.0.0",
                serde_json::json!(["host.http.request", "host.http.stream", "host.http.data"]),
            );
            let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

            let error = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys)
                .expect_err("native service identity must match its package");

            assert_eq!(error.code, "PLUGIN_NATIVE_SERVICE_INVALID");
        }
    }

    #[test]
    fn rejects_http_permissions_without_a_native_service() {
        let index = b"<!doctype html><title>HTTP</title>";
        let (_directory, path, signing_key) = package_with_index(
            index,
            index,
            "com.attson.http",
            "0.1.0-alpha.0",
            "[\"host.http.request\"]",
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys)
            .expect_err("HTTP Host permissions require a signed native service");

        assert_eq!(error.code, "PLUGIN_NATIVE_SERVICE_REQUIRED");
    }

    #[test]
    fn rejects_host_api_two_without_a_native_service_like_the_sdk() {
        let index = b"<!doctype html><title>Codec</title>";
        let (_directory, path, signing_key) = package_with_index_host_api(
            index,
            index,
            "com.attson.codec",
            "0.1.0-alpha.0",
            "[\"host.theme.read\"]",
            2,
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys)
            .expect_err("Host API 2 packages require a signed native service");

        assert_eq!(error.code, "PLUGIN_NATIVE_SERVICE_REQUIRED");
    }

    #[test]
    fn rejects_a_native_service_bound_to_host_api_one() {
        let (_directory, path, signing_key) = native_service_package(
            "0.1.0-alpha.0",
            1,
            None,
            "1.0.0",
            serde_json::json!(["host.theme.read"]),
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys)
            .expect_err("native services require Host API 2");

        assert_eq!(error.code, "PLUGIN_NATIVE_SERVICE_REQUIRED");
    }

    #[cfg(unix)]
    #[test]
    fn extraction_marks_native_service_payload_executables() {
        use std::os::unix::fs::PermissionsExt;

        let (directory, path, signing_key) = native_service_package(
            "0.1.0-alpha.0",
            1,
            None,
            "2.0.0",
            serde_json::json!(["host.http.request", "host.http.stream", "host.http.data"]),
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);
        let destination = directory.path().join("staged");

        extract_package(
            &path,
            &destination,
            &HostEnvironment::current("2.0.0"),
            &keys,
        )
        .unwrap();

        for relative in ["native/athttp-service", "native/runtime-helper"] {
            let mode = std::fs::metadata(destination.join(relative))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0, "{relative} must be executable");
        }
    }

    #[test]
    fn rejects_an_invalid_signature() {
        let (_directory, path, _signing_key) = valid_package();
        let different_key = SigningKey::from_bytes(&[7; 32]);
        let keys = HashMap::from([("attool-p0-test".to_string(), different_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("1.0.0"), &keys)
            .expect_err("signature must fail");

        assert_eq!(error.code, "PLUGIN_SIGNATURE_INVALID");
    }

    #[test]
    fn rejects_a_modified_package_file() {
        let original = b"<!doctype html><title>Codec</title>";
        let tampered = b"<!doctype html><title>Tampered</title>";
        let (_directory, path, signing_key) = package_with_index(
            tampered,
            original,
            "com.attson.codec",
            "0.1.0-alpha.0",
            "[\"host.theme.read\"]",
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("1.0.0"), &keys)
            .expect_err("hash must fail");

        assert_eq!(error.code, "PLUGIN_FILE_HASH_MISMATCH");
    }

    #[test]
    fn rejects_a_package_for_another_target() {
        let (_directory, path, signing_key) = valid_package();
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);
        let mut environment = HostEnvironment::current("1.0.0");
        environment.arch = "unsupported-test-arch".to_string();

        let error = inspect_package(&path, &environment, &keys).expect_err("target must fail");

        assert_eq!(error.code, "PLUGIN_TARGET_MISMATCH");
    }

    #[test]
    fn accepts_a_legacy_host_api_package_on_a_v2_host() {
        let (_directory, path, signing_key) = valid_package();
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let package = inspect_package(&path, &HostEnvironment::current("2.0.0"), &keys).unwrap();

        assert_eq!(package.host_api_max, "1.x");
    }

    #[test]
    fn rejects_a_host_api_package_newer_than_the_host() {
        let (_directory, path, signing_key) = native_service_package(
            "0.1.0-alpha.0",
            1,
            None,
            "2.0.0",
            serde_json::json!(["host.http.request", "host.http.stream", "host.http.data"]),
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("1.0.0"), &keys)
            .expect_err("Host API must fail");

        assert_eq!(error.code, "PLUGIN_HOST_API_UNSUPPORTED");
    }

    #[test]
    fn rejects_a_signed_manifest_with_an_invalid_app_id() {
        let index = b"<!doctype html><title>Codec</title>";
        let (_directory, path, signing_key) = package_with_index(
            index,
            index,
            "../codec",
            "0.1.0-alpha.0",
            "[\"host.theme.read\"]",
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("1.0.0"), &keys)
            .expect_err("appId must fail");

        assert_eq!(error.code, "PLUGIN_APP_ID_INVALID");
    }

    #[test]
    fn rejects_a_signed_manifest_with_an_unknown_permission() {
        let index = b"<!doctype html><title>Codec</title>";
        let (_directory, path, signing_key) = package_with_index(
            index,
            index,
            "com.attson.codec",
            "0.1.0-alpha.0",
            "[\"host.process.spawn\"]",
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("1.0.0"), &keys)
            .expect_err("permission must fail");

        assert_eq!(error.code, "PLUGIN_PERMISSION_UNKNOWN");
    }

    #[test]
    fn rejects_an_undeclared_archive_file() {
        let (_directory, path, signing_key) = valid_package();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut archive = ZipWriter::new_append(file).unwrap();
        archive
            .start_file("hidden/payload.js", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"unexpected").unwrap();
        archive.finish().unwrap();
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("1.0.0"), &keys)
            .expect_err("undeclared file must fail");

        assert_eq!(error.code, "PLUGIN_FILE_UNDECLARED");
    }

    #[test]
    fn rejects_a_signed_manifest_with_an_invalid_version() {
        let index = b"<!doctype html><title>Codec</title>";
        let (_directory, path, signing_key) = package_with_index(
            index,
            index,
            "com.attson.codec",
            "../../../outside",
            "[\"host.theme.read\"]",
        );
        let keys = HashMap::from([("attool-p0-test".to_string(), signing_key.verifying_key())]);

        let error = inspect_package(&path, &HostEnvironment::current("1.0.0"), &keys)
            .expect_err("invalid version must fail");

        assert_eq!(error.code, "PLUGIN_VERSION_INVALID");
    }
}
