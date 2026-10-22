//! SPLICE-SIDECAR-LIB-90 (Slice 9): the sidecar I/O block of the sidecar root
//! file, moved verbatim into its own module.
//!
//! One responsibility: reading and writing the persisted sidecar document —
//! the size-bounded load, the validated atomic save, the compare-and-swap
//! save, the revision digest, the atomic byte write and the recovery sweep
//! for orphaned atomic-write temporaries. Every writer of a bundle file
//! serializes against the same per-target write lock, so a plain save can
//! never race an in-flight compare-and-swap into a lost update.
//!
//! The public symbols keep their names and stay reachable under the same
//! paths — the root re-exports them, so `lumina_sidecar::SidecarDocument`,
//! `lumina_sidecar::RecoveryReport`, `lumina_sidecar::load_sidecar`,
//! `lumina_sidecar::save_sidecar`, `lumina_sidecar::write_atomically`,
//! `lumina_sidecar::save_sidecar_if_unchanged`,
//! `lumina_sidecar::document_revision` and `lumina_sidecar::recover_sidecar`
//! stay resolvable. The crate-internal write lock (`acquire_write_lock`) is
//! re-exported by the root for the `zdata` writers of the `.lumina.zdata`
//! bundle.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
// Bare `thread::sleep` is used only by the native file-lock retry loop below.
use std::thread;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

// `io_error` lives in the crate root and is private there; a child module does
// not inherit the parent's item names unqualified, so it is imported here.
// Its visibility is unchanged (`fn`, private in the root).
use crate::io_error;
// The source-level IPTC draft of the document; its schema and helpers live in
// the `metadata` module (Slice 7).
use crate::metadata::MetadataDraft;
// `migrate_sidecar_file` is referenced only by the intra-doc link of the moved
// `save_sidecar` doc comment; the import keeps that link resolvable.
#[allow(unused_imports)]
use crate::migrate_sidecar_file;
use crate::{
    AnalysisFingerprint, CollectionMembership, CullingSection, Extras, FaceAnalysis, Preset,
    SidecarError, SourceIdentity, StackMembership, VirtualCopy, MAX_SIDECAR_BYTES, TEMP_SWEEP_AGE,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarDocument {
    pub format: String,
    pub schema_version: u32,
    pub source: SourceIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_fingerprint: Option<AnalysisFingerprint>,
    pub pipeline_version: String,
    pub virtual_copies: Vec<VirtualCopy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_virtual_copies: Vec<VirtualCopy>,
    pub presets: Vec<Preset>,
    /// G-15 META-MVP (Slice 1): source-level keywords describing the image
    /// content (shared by all virtual copies, like mask artefacts). Additive:
    /// absent in older sidecars reads as empty and serializes back absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// G-15 META-MVP (Slice 1): source-level static collection memberships.
    /// Additive: absent in older sidecars reads as empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<CollectionMembership>,
    /// LRPAR-G15-IPTC-S1: source-level IPTC metadata draft (draft map plus
    /// its own provenance history, separate from the per-copy edit
    /// history). Additive: absent in older sidecars reads as the empty
    /// draft and an empty draft serializes back absent.
    #[serde(default, skip_serializing_if = "MetadataDraft::is_empty")]
    pub metadata: MetadataDraft,
    /// LRPAR-G12-FACE-20 (FACE-20-S1): optional source-level face analysis
    /// (detections, embeddings, clusters, person labels plus their identity
    /// and status). Additive: absent in older sidecars reads as `None` and an
    /// absent section serializes back absent (no schema bump, no migration).
    /// Shared by every virtual copy — local adjustments stay per copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FaceAnalysis>,
    /// LRPAR-G09-CULL-25: optional source-level culling proposal (assisted
    /// culling). Additive: absent is the valid "no proposal" state and
    /// serializes back absent. The proposal never writes rating/flag/label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub culling: Option<CullingSection>,
    /// LRPAR-G15-STACK-15: optional source-level image-stack membership.
    /// Additive schema-v2 field: absent is the legitimate "not stacked" state
    /// and serializes back absent (legacy documents stay byte-stable).
    /// Shared by all virtual copies — a stack is per source image, never a
    /// recipe/mask/history mutation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<StackMembership>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// The result of cleaning up files left behind by an interrupted atomic write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryReport {
    pub removed_temporary_files: usize,
}

/// REVIEW-SIDECAR-CAS-1 / REVIEW-SIDECAR-ZDATA-1: the per-target write lock is
/// shared by every writer of a bundle file (JSON sidecar *and* `.lumina.zdata`)
/// so plain saves, compare-and-swap saves and read-modify-write appends all
/// serialize against each other. `pub(crate)` because `zdata` reuses it for the
/// `.zdata.lock`.
pub(crate) struct WriteLock {
    // Keep the file handle (and therefore its OS advisory lock) alive for the
    // full critical section. The rendezvous file is intentionally persistent:
    // unlinking it while another process has opened it can split contenders
    // across two inodes. The OS releases the advisory lock when the process
    // exits, so the persistent file does not preserve stale lock ownership.
    _file: fs::File,
}

/// Cooperative lock for a sidecar and any related files published with it.
///
/// Hold this across the complete read/check/publish transaction, and use
/// [`SidecarWriteLock::save`] instead of [`save_sidecar`] while it is held.
/// This serializes Lumina writers that use the sidecar lock protocol; it does
/// not prevent processes that mutate files without honoring the lock, and it
/// depends on the filesystem honoring OS advisory locks. The OS releases the
/// advisory lock on process exit; its stable rendezvous file is intentionally
/// retained to prevent contenders locking different inodes.
#[must_use = "dropping the lock before publication reopens the check/write race"]
pub struct SidecarWriteLock {
    path: PathBuf,
    _lock: WriteLock,
}

impl SidecarWriteLock {
    /// Atomically save the sidecar protected by this lock.
    pub fn save(&self, document: &SidecarDocument) -> Result<(), SidecarError> {
        save_sidecar_locked(&self.path, document)
    }
}

pub fn load_sidecar(path: &Path) -> Result<SidecarDocument, SidecarError> {
    // A temporary file is never a sidecar.  Remove only files with the exact
    // tempfile prefix used by save_sidecar; a partial file must not become a
    // valid document after a crash.
    recover_sidecar(path)?;
    // REVIEW-SIDECAR-N5: the size limit is enforced *before* the file is read
    // (metadata gate) and a second time on the bounded read itself, so an
    // oversized or sparse file can never be pulled into memory in full. The
    // metadata/open race is covered by the `take` bound, not by the stat.
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            SidecarError::Missing(path.display().to_string())
        } else {
            io_error("reading sidecar metadata", path, error)
        }
    })?;
    if metadata.len() > MAX_SIDECAR_BYTES as u64 {
        return Err(SidecarError::Invalid(format!(
            "sidecar exceeds size limit of {MAX_SIDECAR_BYTES} bytes: `{}`",
            path.display()
        )));
    }
    let mut file = fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            SidecarError::Missing(path.display().to_string())
        } else {
            io_error("opening", path, error)
        }
    })?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(MAX_SIDECAR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("reading", path, error))?;
    if bytes.len() > MAX_SIDECAR_BYTES {
        return Err(SidecarError::Invalid(format!(
            "sidecar exceeds size limit of {MAX_SIDECAR_BYTES} bytes: `{}`",
            path.display()
        )));
    }
    let json = String::from_utf8(bytes).map_err(|_| {
        SidecarError::Json(format!("sidecar is not valid UTF-8: `{}`", path.display()))
    })?;
    SidecarDocument::from_json(&json)
}

/// Validates before writing and replaces the destination only after the complete
/// temporary file has been flushed and synced. Output and sidecar are not a
/// two-file transaction (the architecture explicitly leaves that out of v1).
///
/// REVIEW-SIDECAR-CAS-1: this is a *serialized* write — it takes the same
/// per-sidecar write lock as [`save_sidecar_if_unchanged`] and
/// [`migrate_sidecar_file`], so a plain save can no longer race an in-flight
/// compare-and-swap into a lost update. Contract for all writers: every JSON
/// sidecar write must go through `save_sidecar`, `save_sidecar_if_unchanged`
/// or `migrate_sidecar_file`; direct filesystem writes bypass conflict
/// detection entirely and are not supported.
pub fn save_sidecar(path: &Path, document: &SidecarDocument) -> Result<(), SidecarError> {
    acquire_sidecar_write_lock(path)?.save(document)
}

/// Acquires the same cooperative lock used by [`save_sidecar`]. Multi-file
/// transactions can hold it across ownership checks and artifact publication,
/// then call [`SidecarWriteLock::save`] to persist without reacquiring the
/// non-reentrant lock.
pub fn acquire_sidecar_write_lock(path: &Path) -> Result<SidecarWriteLock, SidecarError> {
    Ok(SidecarWriteLock {
        path: path.to_path_buf(),
        _lock: acquire_write_lock(path)?,
    })
}

/// The actual atomic replace. Caller must hold the sidecar's write lock; the
/// lock deliberately lives with the public wrappers so the check-then-write
/// window of the compare-and-swap path stays inside one critical section.
fn save_sidecar_locked(path: &Path, document: &SidecarDocument) -> Result<(), SidecarError> {
    let json = document.to_json()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    let filename = filename.as_deref().unwrap_or("sidecar");
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| io_error("creating temporary file", parent, error))?;
    let temporary_path = temporary.path().to_path_buf();
    let result = (|| {
        // tempfile creates private files (0600 on Unix). Preserve the mode
        // only when replacing an existing regular file; aliases still get
        // replaced by persist/rename, never followed.
        #[cfg(unix)]
        if let Ok(metadata) = std::fs::symlink_metadata(path) {
            if metadata.file_type().is_file() {
                use std::os::unix::fs::PermissionsExt as _;
                temporary
                    .as_file()
                    .set_permissions(std::fs::Permissions::from_mode(
                        metadata.permissions().mode() & 0o7777,
                    ))
                    .map_err(|error| {
                        io_error("setting temporary file mode", &temporary_path, error)
                    })?;
            }
        }
        temporary
            .write_all(json.as_bytes())
            .map_err(|error| io_error("writing temporary file", &temporary_path, error))?;
        temporary
            .flush()
            .map_err(|error| io_error("flushing temporary file", &temporary_path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| io_error("syncing temporary file", &temporary_path, error))?;
        temporary
            .persist(path)
            .map_err(|error| io_error("renaming temporary file", path, error.error))?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| io_error("syncing sidecar directory", parent, error))?;
        Ok(())
    })();
    result
}

/// Write `bytes` to `path` atomically: a temporary file is created in the same
/// directory, fully written, flushed and synced, then renamed over the target.
///
/// Both the CLI and the GUI reuse this so the atomic-write semantics (and the
/// `.{name}.tmp-*` naming) are identical everywhere; only the sidecar write
/// additionally syncs the parent directory afterwards (see [`save_sidecar`]).
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), SidecarError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    let filename = filename.as_deref().unwrap_or("artifact");
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| io_error("creating temporary file", parent, error))?;
    let temporary_path = temporary.path().to_path_buf();
    let result = (|| {
        #[cfg(unix)]
        if let Ok(metadata) = std::fs::symlink_metadata(path) {
            if metadata.file_type().is_file() {
                use std::os::unix::fs::PermissionsExt as _;
                temporary
                    .as_file()
                    .set_permissions(std::fs::Permissions::from_mode(
                        metadata.permissions().mode() & 0o7777,
                    ))
                    .map_err(|error| {
                        io_error("setting temporary file mode", &temporary_path, error)
                    })?;
            }
        }
        temporary
            .write_all(bytes)
            .map_err(|error| io_error("writing temporary file", &temporary_path, error))?;
        temporary
            .flush()
            .map_err(|error| io_error("flushing temporary file", &temporary_path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| io_error("syncing temporary file", &temporary_path, error))?;
        temporary
            .persist(path)
            .map_err(|error| io_error("renaming temporary file", path, error.error))?;
        Ok(())
    })();
    result
}

#[cfg(all(test, unix))]
#[test]
fn atomic_export_preserves_regular_target_mode_at_path_and_name() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("named-export.png");
    for mode in [0o644, 0o600] {
        std::fs::write(&target, b"old").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode)).unwrap();
        let before = std::fs::symlink_metadata(&target).unwrap();
        write_atomically(&target, b"new").unwrap();
        let after = std::fs::symlink_metadata(&target).unwrap();
        assert_eq!(after.permissions().mode() & 0o7777, mode);
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_ne!(after.ino(), before.ino());
    }
}

pub fn save_sidecar_if_unchanged(
    path: &Path,
    document: &SidecarDocument,
    expected_revision: Option<&str>,
) -> Result<String, SidecarError> {
    let _lock = acquire_write_lock(path)?;
    if let Some(expected) = expected_revision {
        if !path.exists() {
            return Err(SidecarError::Conflict(format!(
                "sidecar disappeared: `{}`",
                path.display()
            )));
        }
        let current = document_revision(&load_sidecar(path)?)?;
        if current != expected {
            return Err(SidecarError::Conflict(path.display().to_string()));
        }
    } else if path.exists() {
        return Err(SidecarError::Conflict(format!(
            "sidecar already exists: `{}`; an expected revision is required",
            path.display()
        )));
    }
    // Already holding the lock: use the unlocked inner write (the public
    // `save_sidecar` would deadlock on the non-reentrant lock).
    save_sidecar_locked(path, document)?;
    document_revision(document)
}

pub fn document_revision(document: &SidecarDocument) -> Result<String, SidecarError> {
    let json = document.to_json()?;
    Ok(blake3::hash(json.as_bytes()).to_hex().to_string())
}

/// Remove orphaned atomic-write temporaries.  The destination is never
/// touched, and temporary contents are never parsed or promoted.
///
/// REVIEW-SIDECAR-TMP-1: only temporaries that are demonstrably older than
/// [`TEMP_SWEEP_AGE`] are removed. A live writer keeps its temporary fresh, so
/// a concurrent reader running recovery can no longer delete the temporary out
/// from under an in-flight save (which previously made the writer's atomic
/// rename fail). A temporary whose modification time cannot be determined is
/// deliberately *kept*: an un-swept leftover is harmless clutter, while
/// deleting a live writer's temporary loses data. Leftovers younger than the
/// threshold are swept by a later recovery once they have aged past it.
pub fn recover_sidecar(path: &Path) -> Result<RecoveryReport, SidecarError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    // Keep the prefix narrow: unrelated temporary files must survive recovery.
    let prefix = format!(".{}.tmp-", filename.as_deref().unwrap_or("sidecar"));
    let entries = fs::read_dir(parent)
        .map_err(|error| io_error("reading recovery directory", parent, error))?;
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("reading recovery entry", parent, error))?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(&prefix) || !entry.path().is_file() {
            continue;
        }
        // REVIEW-SIDECAR-TMP-1: skip temporaries that may belong to a live
        // writer (fresh modification time or undeterminable metadata).
        let orphaned = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > TEMP_SWEEP_AGE);
        if !orphaned {
            continue;
        }
        fs::remove_file(entry.path())
            .map_err(|error| io_error("removing orphaned temporary file", &entry.path(), error))?;
        removed += 1;
    }
    Ok(RecoveryReport {
        removed_temporary_files: removed,
    })
}

/// Acquires the per-target OS advisory lock on a stable rendezvous file.
///
/// Never unlink the rendezvous file: a process that has opened an unlinked
/// inode can otherwise acquire it while another process locks a replacement.
/// The kernel releases the lock on normal close and process termination, so
/// crash recovery does not depend on timestamps or a stale-file reclaimer.
pub(crate) fn acquire_write_lock(path: &Path) -> Result<WriteLock, SidecarError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    let lock_path = parent.join(format!(
        ".{}.lock",
        filename.as_deref().unwrap_or("sidecar")
    ));
    for _ in 0..100 {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| io_error("opening sidecar lock", &lock_path, error))?;
        match file.try_lock() {
            Ok(()) => return Ok(WriteLock { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                // Retry briefly, then report an explicit conflict rather than
                // waiting indefinitely behind a long-running writer.
                thread::sleep(Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(io_error("locking sidecar", &lock_path, error));
            }
        }
    }
    Err(SidecarError::Conflict(format!(
        "sidecar is locked: `{}`",
        path.display()
    )))
}
