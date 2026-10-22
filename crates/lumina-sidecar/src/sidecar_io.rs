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
    path: PathBuf,
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        // A failed cleanup is deliberately ignored: the next writer can report
        // the lock as stale, and the actual sidecar remains untouched.
        let _ = fs::remove_file(&self.path);
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
    let _lock = acquire_write_lock(path)?;
    save_sidecar_locked(path, document)
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

pub(crate) fn acquire_write_lock(path: &Path) -> Result<WriteLock, SidecarError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    let lock_path = parent.join(format!(
        ".{}.lock",
        filename.as_deref().unwrap_or("sidecar")
    ));
    const STALE_THRESHOLD: Duration = Duration::from_secs(30);
    for iteration in 0..100 {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_) => return Ok(WriteLock { path: lock_path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Crash recovery for a lock cannot identify a process portably;
                // only reclaim locks that are clearly abandoned. The stale
                // decision and the reclaim must be atomic: two processes must
                // not both decide "stale" and have the second delete the fresh
                // lock the first just created (TOCTOU lost update). We achieve
                // atomicity via an atomic rename: the stale lock is moved aside
                // to a unique reclaim path. Only the winner of the rename owns
                // the old lock file and can decide whether it was genuinely
                // stale; a fresh lock is restored instead of silently deleted,
                // producing an explicit Conflict for the contender.
                let stale = fs::metadata(&lock_path)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|time| SystemTime::now().duration_since(time).ok())
                    .is_some_and(|age| age > STALE_THRESHOLD);
                if !stale {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                // Generate a unique reclaim destination in the same directory so
                // the rename stays on the same filesystem and remains atomic.
                let reclaim_path = parent.join(format!(
                    ".{}.lock.reclaim-{}-{}-{}",
                    filename.as_deref().unwrap_or("sidecar"),
                    std::process::id(),
                    iteration,
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ));
                match fs::rename(&lock_path, &reclaim_path) {
                    Ok(()) => {
                        // We now own the old lock file at `reclaim_path`.
                        // Verify it was genuinely stale at the instant of the
                        // atomic rename (its mtime is preserved across rename).
                        let reclaimed_stale = fs::metadata(&reclaim_path)
                            .and_then(|meta| meta.modified())
                            .ok()
                            .and_then(|time| SystemTime::now().duration_since(time).ok())
                            .is_some_and(|age| age > STALE_THRESHOLD);
                        if reclaimed_stale {
                            // Genuinely abandoned -> discard and retry creation.
                            let _ = fs::remove_file(&reclaim_path);
                            continue;
                        } else {
                            // We raced and stole a fresh lock. Restore it
                            // instead of silently deleting it; the contender
                            // must see an explicit Conflict, not a lost update.
                            match fs::rename(&reclaim_path, &lock_path) {
                                Ok(()) => {
                                    thread::sleep(Duration::from_millis(10));
                                    continue;
                                }
                                Err(_) => {
                                    // Another writer already created a new lock
                                    // while we held the stolen fresh file.
                                    // Discard the stolen copy and contend.
                                    let _ = fs::remove_file(&reclaim_path);
                                    thread::sleep(Duration::from_millis(10));
                                    continue;
                                }
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        // Lock vanished between the stale check and the rename
                        // (another reclaimer won). Retry creation immediately.
                        continue;
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                }
            }
            Err(error) => return Err(io_error("creating sidecar lock", &lock_path, error)),
        }
    }
    Err(SidecarError::Conflict(format!(
        "sidecar is locked: `{}`",
        path.display()
    )))
}
