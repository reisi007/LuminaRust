//! Explicit, bounded installation of one user-approved model package.

use crate::{hash::compute_sha256_hex, OnnxError};
use reqwest::{blocking::Client, redirect::Policy, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_ALLOWED_BYTES: u64 = 20 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct PackageDescriptor {
    pub model_id: String,
    pub version: String,
    pub filename: String,
    pub artifact_url: String,
    pub provenance_url: String,
    pub license_id: String,
    pub license_text: String,
    pub sha256: String,
    pub max_bytes: u64,
}

/// Consent is bound to the exact license identifier and text digest shown to
/// the user, not a reusable boolean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseAcceptance {
    license_id: String,
    text_sha256: String,
}

impl LicenseAcceptance {
    /// Call only after displaying this exact license text and identifier and
    /// receiving affirmative user action for this package.
    pub fn accept_exact(license_id: &str, license_text: &str) -> Self {
        Self {
            license_id: license_id.to_owned(),
            text_sha256: digest(license_text.as_bytes()),
        }
    }
}

pub struct DownloadResponse {
    pub content_length: Option<u64>,
    pub body: Box<dyn Read>,
}

pub trait ArtifactTransport {
    fn open(&self, url: &str) -> Result<DownloadResponse, String>;
}

pub struct HttpsTransport {
    client: Client,
}

impl HttpsTransport {
    pub fn new() -> Result<Self, OnnxError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|error| invalid(format!("HTTPS client: {error}")))?;
        Ok(Self { client })
    }

    /// HTTPS is checked here as well as by descriptor validation: callers
    /// cannot use this transport directly to issue a cleartext request.
    pub fn open(&self, url: &str) -> Result<DownloadResponse, String> {
        let parsed = Url::parse(url).map_err(|error| error.to_string())?;
        validate_https_url(&parsed)?;
        let response = self
            .client
            .get(parsed)
            .send()
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP status {}", response.status()));
        }
        Ok(DownloadResponse {
            content_length: response.content_length(),
            body: Box::new(response),
        })
    }
}

impl ArtifactTransport for HttpsTransport {
    fn open(&self, url: &str) -> Result<DownloadResponse, String> {
        HttpsTransport::open(self, url)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct InstalledMetadata {
    model_id: String,
    version: String,
    filename: String,
    artifact_url: String,
    provenance_url: String,
    license_id: String,
    sha256: String,
    max_bytes: u64,
}

/// Install one explicitly consented package into `<root>/<model-id>/<version>`.
/// A complete directory is published using a no-replace atomic directory
/// rename; a prior or concurrently installed package is never overwritten.
pub fn install<T: ArtifactTransport>(
    package: &PackageDescriptor,
    consent: &LicenseAcceptance,
    root: &Path,
    transport: &T,
) -> Result<PathBuf, OnnxError> {
    validate(package)?;
    if consent.license_id != package.license_id
        || consent.text_sha256 != digest(package.license_text.as_bytes())
    {
        return Err(invalid(
            "explicit consent does not match exact license text",
        ));
    }

    fs::create_dir_all(root).map_err(io_error)?;
    let destination = root.join(&package.model_id).join(&package.version);
    let package_parent = destination
        .parent()
        .ok_or_else(|| invalid("package destination has no parent"))?;
    if let Some(matches) = destination_status(&destination, package)? {
        return if matches {
            Ok(destination)
        } else {
            Err(invalid(
                "model version destination is occupied by another package",
            ))
        };
    }
    fs::create_dir_all(package_parent).map_err(io_error)?;

    let mut response = match transport.open(&package.artifact_url) {
        Ok(response) => response,
        Err(error) => {
            remove_empty_directory(package_parent);
            return Err(invalid(error));
        }
    };
    if response
        .content_length
        .is_some_and(|length| length > package.max_bytes)
    {
        remove_empty_directory(package_parent);
        return Err(invalid("declared Content-Length exceeds package size cap"));
    }

    let stage = match create_staging_directory(root, &package.model_id, &package.version) {
        Ok(stage) => stage,
        Err(error) => {
            remove_empty_directory(package_parent);
            return Err(error);
        }
    };
    let result = write_staged_package(package, &stage, &mut response);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&stage);
        remove_empty_directory(package_parent);
        return Err(error);
    }

    match rename_directory_noreplace(&stage, &destination) {
        Ok(()) => Ok(destination),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_dir_all(&stage);
            if destination_status(&destination, package)? == Some(true) {
                Ok(destination)
            } else {
                Err(invalid("concurrent package install occupied destination"))
            }
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&stage);
            remove_empty_directory(package_parent);
            Err(io_error(error))
        }
    }
}

/// `Some(true)` is the identical installed package; `Some(false)` is an
/// occupied but different/unsafe path; `None` is absent. `symlink_metadata`
/// intentionally does not follow symlinks, and all errors except NotFound fail
/// closed before a download is attempted.
fn destination_status(
    destination: &Path,
    package: &PackageDescriptor,
) -> Result<Option<bool>, OnnxError> {
    let metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    if !metadata.file_type().is_dir() {
        return Ok(Some(false));
    }
    installed_package_matches(destination, package).map(Some)
}

fn write_staged_package(
    package: &PackageDescriptor,
    stage: &Path,
    response: &mut DownloadResponse,
) -> Result<(), OnnxError> {
    let artifact_path = stage.join(&package.filename);
    let mut artifact = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&artifact_path)
        .map_err(io_error)?;
    copy_bounded(&mut response.body, &mut artifact, package.max_bytes).map_err(io_error)?;
    artifact.sync_all().map_err(io_error)?;
    let actual =
        compute_sha256_hex(File::open(&artifact_path).map_err(io_error)?).map_err(io_error)?;
    if actual != package.sha256[7..] {
        return Err(invalid("artifact SHA-256 mismatch"));
    }

    let metadata = InstalledMetadata {
        model_id: package.model_id.clone(),
        version: package.version.clone(),
        filename: package.filename.clone(),
        artifact_url: package.artifact_url.clone(),
        provenance_url: package.provenance_url.clone(),
        license_id: package.license_id.clone(),
        sha256: package.sha256.clone(),
        max_bytes: package.max_bytes,
    };
    write_new_synced(
        &stage.join("metadata.json"),
        &serde_json::to_vec_pretty(&metadata)
            .map_err(|error| invalid(format!("serialize package metadata: {error}")))?,
    )?;
    write_new_synced(&stage.join("LICENSE"), package.license_text.as_bytes())?;
    File::open(stage)
        .and_then(|directory| directory.sync_all())
        .map_err(io_error)?;
    Ok(())
}

fn write_new_synced(path: &Path, contents: &[u8]) -> Result<(), OnnxError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error)?;
    file.write_all(contents).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

fn installed_package_matches(
    directory: &Path,
    package: &PackageDescriptor,
) -> Result<bool, OnnxError> {
    let expected_metadata = InstalledMetadata {
        model_id: package.model_id.clone(),
        version: package.version.clone(),
        filename: package.filename.clone(),
        artifact_url: package.artifact_url.clone(),
        provenance_url: package.provenance_url.clone(),
        license_id: package.license_id.clone(),
        sha256: package.sha256.clone(),
        max_bytes: package.max_bytes,
    };
    let metadata: InstalledMetadata = match fs::read(directory.join("metadata.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| invalid(format!("invalid installed metadata: {error}")))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(io_error(error)),
    };
    if metadata != expected_metadata
        || fs::read(directory.join("LICENSE")).map_err(io_error)? != package.license_text.as_bytes()
    {
        return Ok(false);
    }
    let artifact = File::open(directory.join(&package.filename)).map_err(io_error)?;
    Ok(compute_sha256_hex(artifact).map_err(io_error)? == package.sha256[7..])
}

fn validate(package: &PackageDescriptor) -> Result<(), OnnxError> {
    validate_path_component("model id", &package.model_id)?;
    validate_path_component("version", &package.version)?;
    if package.filename.is_empty()
        || package.filename == "."
        || package.filename == ".."
        || package.filename.contains('/')
        || package.filename.contains('\\')
        || package.filename.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(invalid("unsafe artifact filename"));
    }

    for value in [&package.artifact_url, &package.provenance_url] {
        let url = Url::parse(value).map_err(|_| invalid("malformed source URL"))?;
        validate_https_url(&url).map_err(invalid)?;
    }
    if !valid_spdx_label(&package.license_id) || package.license_text.trim().is_empty() {
        return Err(invalid("missing or malformed SPDX/license metadata"));
    }
    if !package.sha256.strip_prefix("sha256:").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err(invalid("hash must be pinned lowercase sha256:<64 hex>"));
    }
    if package.max_bytes == 0 || package.max_bytes > MAX_ALLOWED_BYTES {
        return Err(invalid("invalid or excessive max_bytes"));
    }
    Ok(())
}

fn validate_path_component(label: &str, value: &str) -> Result<(), OnnxError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(invalid(format!("unsafe {label}")));
    }
    Ok(())
}

fn valid_spdx_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
        && value
            .split(['+', '.'])
            .all(|part| !part.is_empty() && part.bytes().any(|byte| byte.is_ascii_alphanumeric()))
}

fn validate_https_url(url: &Url) -> Result<(), String> {
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("URL must be credential-free HTTPS without fragment".into());
    }
    Ok(())
}

fn copy_bounded(reader: &mut impl Read, writer: &mut impl Write, cap: u64) -> io::Result<()> {
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("artifact size overflow"))?;
        if total > cap {
            return Err(io::Error::other("artifact exceeds size cap"));
        }
        writer.write_all(&buffer[..count])?;
    }
    Ok(())
}

fn create_staging_directory(
    root: &Path,
    model_id: &str,
    version: &str,
) -> Result<PathBuf, OnnxError> {
    for nonce in 0..1000u32 {
        let path = root.join(format!(
            ".{model_id}.{version}.{}.{}.stage",
            std::process::id(),
            nonce
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(error)),
        }
    }
    Err(invalid("unable to reserve package staging directory"))
}

fn remove_empty_directory(path: &Path) {
    let _ = fs::remove_dir(path);
}

#[cfg(target_os = "linux")]
fn rename_directory_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let source = std::ffi::CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("NUL in staging path"))?;
    let destination = std::ffi::CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("NUL in destination path"))?;
    // SAFETY: both C strings are NUL-terminated and live through the syscall.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn rename_directory_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let source = std::ffi::CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("NUL in staging path"))?;
    let destination = std::ffi::CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("NUL in destination path"))?;
    // SAFETY: both C strings are NUL-terminated and live through the syscall.
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn rename_directory_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    // On platforms without a stable no-replace directory rename API, fail
    // closed rather than risk replacing an existing package.
    let _ = (source, destination);
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace directory rename is unavailable on this platform",
    ))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn invalid(message: impl Into<String>) -> OnnxError {
    OnnxError::InvalidManifest(message.into())
}

fn io_error(error: io::Error) -> OnnxError {
    invalid(format!("model installation I/O: {error}"))
}

#[cfg(test)]
#[path = "model_download_tests.rs"]
mod tests;
