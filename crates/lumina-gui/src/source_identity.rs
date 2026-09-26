//! GUI source identity: how the GUI decides **what a source file is**.
//!
//! Two things live here, both of which the rest of the GUI must not re-derive:
//!
//! 1. **Construction** of a persisted [`SourceIdentity`] ([`build_source_identity`],
//!    shared verbatim by the loaded-app path in [`crate::present`] and the
//!    "fresh sidecar for a selection target" path
//!    ([`selection_source_identity`])). Before the extraction the two were
//!    copy-pasted field-for-field, so a decode/geometry field could drift
//!    between them silently; one constructor cannot drift from itself.
//!
//! 2. **The process-wide whole-file content memo** ([`content_hash`] /
//!    [`source_fingerprint_of`]), the `THUMB-HASH-PERF-35` fix. The thumbnail
//!    scheduler asks for a source's *exact* content identity once per visible
//!    cell **per frame**, and the answer used to be "read and BLAKE3-hash the
//!    whole 12 MB RAW" every single time — ~115 ms per *lookup* for a 12.3 MB
//!    CR3, and 0.79 s per *frame* at three visible cells (two full reads per
//!    cell) against 0.69 s of decode. The memo keys the expensive full-file
//!    identity on the cheap stat quadruple
//!    **`(path, mtime, ctime, len)`**, so an unchanged file is hashed once
//!    instead of once per frame. It changes **no identity value** — only how
//!    often the same value is recomputed.
//!
//!    **`ctime` is load bearing, not decoration.** Do not "simplify" this key
//!    back to `(path, mtime, len)`: the committed regression
//!    `thumbnail_source_replacement_invalidates_cached_and_pending_state`
//!    restores both the byte length *and* the mtime and still requires a new
//!    content identity, so the shorter key would serve stale pixels. See the
//!    [`FileStatKey`] doc for the full argument.
//!
//! The normative contract (cache key, why it is sound, what happens on a miss,
//! and the "a genuinely changed source must still yield a new identity"
//! invariant) is `feature/platform/cli-gui-wasm.md` § *Quell-Identitäts-Cache im
//! UI-Thread*. The residual risk — a platform that exposes no `ctime` — is
//! named there, not hidden here.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::SystemTime;

use lumina_core::ImageFrame;
use lumina_sidecar::{DecodeFingerprint, GeometryFingerprint, SourceFingerprint, SourceIdentity};

use crate::decoder_identity;

// ---- 1. Persisted source identity (shared construction) ----

/// Build the [`SourceIdentity`] every Lumina source is described by.
///
/// `bytes` is the *exact* source content; `None` (a dropped image with no bytes
/// resident) keeps the long-standing `blake3:unknown` / `byte_length: 0`
/// placeholder, so a source that was never fully read still gets a stable,
/// visibly-incomplete identity instead of a silent zero.
///
/// `raw_format` is derived from `relative_name`. That is identical to deriving
/// it from the app's `source_name`: the loaded-app path substitutes the
/// extension-less literal `"dropped-image"` only when `source_name` is empty,
/// and an empty name has no extension either — both yield `"raster"`.
pub(crate) fn build_source_identity(
    relative_name: String,
    bytes: Option<&[u8]>,
    frame: &ImageFrame,
    orientation: u8,
    source_is_raw: bool,
) -> SourceIdentity {
    SourceIdentity {
        content_hash: bytes
            .map(|bytes| format!("blake3:{}", blake3::hash(bytes).to_hex()))
            .unwrap_or_else(|| "blake3:unknown".into()),
        byte_length: bytes.map_or(0, |bytes| bytes.len() as u64),
        raw_format: Path::new(&relative_name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("raster")
            .to_ascii_uppercase(),
        decode_fingerprint: DecodeFingerprint {
            decoder: decoder_identity(source_is_raw).into(),
            version: if source_is_raw {
                lumina_raw::libraw_decode_version()
            } else {
                env!("CARGO_PKG_VERSION").into()
            },
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: frame.width,
            height: frame.height,
            orientation,
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        relative_name,
        modified_at: None,
        extras: BTreeMap::new(),
        orientation,
    }
}

/// Source identity for a freshly created selection sidecar, mirroring
/// [`crate::LuminaApp::source_identity`] without requiring loaded-app state.
pub(crate) fn selection_source_identity(
    name: &str,
    bytes: &[u8],
    frame: &ImageFrame,
    orientation: u8,
    source_is_raw: bool,
) -> SourceIdentity {
    build_source_identity(
        name.to_string(),
        Some(bytes),
        frame,
        orientation,
        source_is_raw,
    )
}

// ---- 2. THUMB-HASH-PERF-35: the process-wide whole-file identity memo ----

/// Chunk size of the streaming whole-file hash (unchanged from the pre-cache
/// implementation, so the read pattern and the digest are bit-identical).
const HASH_CHUNK: usize = 64 * 1024;

/// Upper bound of memoized entries. The memo is a pure optimization: exceeding
/// the bound drops it wholesale so a long session cannot grow without limit.
/// A drop costs one re-hash, never a wrong hit.
const MAX_MEMO_ENTRIES: usize = 4096;

/// Cheap, mutation-sensitive cache key: `(path, mtime, ctime, len)`.
///
/// The originally proposed key `(path, mtime, len)` is **not sound here**, and
/// the existing suite says so explicitly: the committed regression
/// `thumbnail_source_replacement_invalidates_cached_and_pending_state` writes
/// new content, restores the original byte length *and* the original mtime, and
/// asserts the content identity still changes ("content identity must change
/// even with identical mtime/length"). `(path, mtime, len)` would serve a stale
/// identity there, so the key additionally carries the inode **change time**
/// (`ctime`).
///
/// `ctime` is the kernel-maintained "last metadata-or-content change"
/// timestamp. A user *can* set mtime back with `utimensat`/`touch -r`, but
/// cannot set `ctime` back — every write and every `set_modified` bumps it. It
/// also catches three further changes that leave length *and* mtime untouched.
/// All four were verified on this platform, each yielding `len_same=true`,
/// `mtime_same=true`, `ctime_same=false`:
///
/// * an in-place rewrite with the byte count preserved and mtime restored;
/// * a **symlink swap** (`path → a` becomes `path → b`, both targets with the
///   same length and mtime — `stat` follows the link, so path, length and mtime
///   all agree while the content is now `b`);
/// * a **hardlink write-through** (the same inode mutated through its other
///   name — `st_ino` is *identical* here, so inode would not have helped);
/// * **path reuse** (delete + recreate at the same path with the same length
///   and a restored mtime).
///
/// It costs **no** extra syscall, because it comes out of the `stat` already
/// performed. On non-Unix targets there is no `ctime`; the key then degrades to
/// the stat triple and the residual risk in the SOLL applies in full.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FileStatKey {
    path: PathBuf,
    len: u64,
    mtime: SystemTime,
    /// Unix inode change time; `None` where the platform does not expose it.
    ctime: Option<(i64, i64)>,
}

/// Read the stat quadruple that makes up the cache key.
///
/// Returns `None` when the platform/filesystem cannot supply a usable
/// modification stamp — such a file is **never** memoized, because "unknown
/// last write" must not be read as "unchanged".
fn stat_key(path: &Path, metadata: &std::fs::Metadata) -> Option<FileStatKey> {
    Some(FileStatKey {
        path: path.to_path_buf(),
        len: metadata.len(),
        mtime: metadata.modified().ok()?,
        ctime: change_time(metadata),
    })
}

/// The kernel-maintained inode change time, where the platform exposes it.
#[cfg(unix)]
fn change_time(metadata: &std::fs::Metadata) -> Option<(i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.ctime(), metadata.ctime_nsec()))
}

/// No `ctime` on this platform: the key degrades to `(path, mtime, len)` and
/// the SOLL's documented residual risk applies in full.
#[cfg(not(unix))]
fn change_time(_metadata: &std::fs::Metadata) -> Option<(i64, i64)> {
    None
}

/// One memoized identity plus the proof that it was really computed: the
/// number of whole-file hashes spent on *this* key.
///
/// The counter is per key, not global, so a test on one temp file can assert an
/// exact hash count without any other test's file activity (the suite runs
/// multi-threaded) perturbing it. It is incremented on a miss and never on a
/// hit, so "no re-hash on an unchanged file" is provable by an exact count
/// rather than by a wall-clock guess.
#[derive(Debug, Clone)]
struct MemoEntry {
    content_hash: String,
    hashes: u64,
}

/// The memo. `const`-initialized, so no `OnceLock` and no lazy-init race.
static MEMO: Mutex<BTreeMap<FileStatKey, MemoEntry>> = Mutex::new(BTreeMap::new());

/// Exact BLAKE3 content hash (`"blake3:<hex>"`) of the whole file at `path`.
///
/// Errors keep the classes the callers already distinguish: `NotFound` for a
/// missing path, everything else as the underlying I/O error. Neither is ever
/// memoized.
pub(crate) fn content_hash(path: &Path) -> std::io::Result<String> {
    resolve(path).map(|(_, content_hash)| content_hash)
}

/// The live [`SourceFingerprint`] (content hash + byte length) of the whole
/// source file, memoized exactly like [`content_hash`].
///
/// This is what the fail-closed sidecar/source check compares against the
/// persisted `document.source`. Before the memo it was `std::fs::read(path)` +
/// hash on the UI thread — the second full-file read of the same per-frame
/// chain. The value is unchanged: BLAKE3 over 64-KB chunks has the same digest
/// as the one-shot hash (pinned by `chunked_hash_equals_one_shot_hash` in
/// `tests::identity_cache`) and `byte_length` comes from the same `stat` the
/// read would have produced.
pub(crate) fn source_fingerprint_of(path: &Path) -> std::io::Result<SourceFingerprint> {
    resolve(path).map(|(len, content_hash)| SourceFingerprint {
        content_hash,
        byte_length: len,
        extras: BTreeMap::new(),
    })
}

/// Test-only: how many whole-file hashes were spent on `path`'s *current* stat
/// key, or `None` when the file is not memoized (missing/unreadable, or a
/// filesystem that reports no mtime — every one of which must re-attempt rather
/// than serve a remembered identity).
#[cfg(test)]
pub(crate) fn hash_count(path: &Path) -> Option<u64> {
    let key = current_key(path).ok().flatten()?;
    hash_count_for(&key)
}

/// Test-only: the hash count stored under one **specific** key.
///
/// Needed because [`Self::hash_count`] only ever looks at the file's *current*
/// key: an entry wrongly filed under a stale key would be invisible to it, and
/// the TOCTOU guard test would pass whether the guard exists or not.
#[cfg(test)]
pub(crate) fn hash_count_for(key: &FileStatKey) -> Option<u64> {
    locked().get(key).map(|entry| entry.hashes)
}

/// The memo key for `path` as of right now.
///
/// `Ok(None)` = the stat succeeded but the filesystem reports no usable
/// modification stamp, so the path is deliberately not memoizable. Used by the
/// post-read re-stat ([`stamp_survived_read`]), the test-only [`hash_count`]
/// and the TOCTOU guard test.
pub(crate) fn current_key(path: &Path) -> std::io::Result<Option<FileStatKey>> {
    let metadata = std::fs::metadata(path)?;
    Ok(stat_key(path, &metadata))
}

/// Whether the pre-read and post-read stamps agree, i.e. whether the digest just
/// computed is trustworthy as a memo entry. Pure, and exposed for the guard
/// test: the ~100 ms race window itself is not reproducible on demand, the
/// decision is.
pub(crate) fn stamp_survived(before: Option<&FileStatKey>, after: Option<&FileStatKey>) -> bool {
    before.is_some() && before == after
}

/// Look the file up, hashing it on a miss.
///
/// The `Mutex` is never held across file I/O: `stat` and the full read/hash run
/// outside the guard, which only ever covers a `BTreeMap` lookup or insert.
/// Two threads racing on the same cold file may therefore both hash it — that
/// is duplicated work, not a data race, and the result is the same value.
/// Returns `(byte length, content hash)`. A miss reads and hashes the whole
/// file; a hit is a map lookup.
fn resolve(path: &Path) -> std::io::Result<(u64, String)> {
    let metadata = std::fs::metadata(path)?;
    let len = metadata.len();
    // No usable modification stamp → no memo entry: "unknown last write" is
    // never "unchanged".
    let key = stat_key(path, &metadata);
    if let Some(key) = key.as_ref() {
        if let Some(entry) = locked().get(key) {
            return Ok((len, entry.content_hash.clone()));
        }
    }
    // R3-LOG-1 / THUMB-HASH-PERF-35: the one wall-clock read of the heavy pass.
    // The SOLL makes this line the manual counting seam for a `RUST_LOG=trace`
    // acceptance run; `emit` only evaluates the closure when the level is on,
    // so the format string costs nothing in normal operation.
    let stopwatch = crate::timing::Stopwatch::now();
    let content_hash = hash_whole_file(path)?;
    crate::timing::emit(|| {
        crate::timing::source_identity_hashed_line(path, len, stopwatch.elapsed_ms())
    });
    if let Some(key) = key.as_ref() {
        store(path, key, &content_hash);
    }
    Ok((len, content_hash))
}

/// Whether `path` still carries exactly `key` right now.
fn stamp_survived_read(path: &Path, key: &FileStatKey) -> bool {
    stamp_survived(Some(key), current_key(path).ok().flatten().as_ref())
}

/// A poisoned lock must not turn a pure memo into a panic cascade: the map
/// holds derived strings only, so recovering the guard is sound and the result
/// is at worst a miss.
fn locked() -> MutexGuard<'static, BTreeMap<FileStatKey, MemoEntry>> {
    MEMO.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Record one successful full hash, spending a hash on the key.
///
/// **The single write path into the memo, and the TOCTOU guard lives here.**
/// The key was taken before a read that costs ~100 ms; a file mutated inside
/// that window produces a torn digest that belongs to no key at all. Storing
/// it under the pre-read stamp would be a `(key, value)` inconsistency, so
/// this re-stats (one syscall, cold path only) and **refuses** a stamp that no
/// longer describes the file — the next lookup simply re-hashes. `crate`-visible
/// so the guard can be tested directly: the race window itself is not
/// reproducible on demand, but "a stale stamp is refused" is.
pub(crate) fn store(path: &Path, key: &FileStatKey, content_hash: &str) {
    if !stamp_survived_read(path, key) {
        log::debug!(
            "GUI source identity: file changed while it was being hashed, nothing memoized path={}",
            path.display()
        );
        return;
    }
    let mut memo = locked();
    if memo.len() >= MAX_MEMO_ENTRIES {
        log::debug!(
            "GUI source identity memo full, dropping {MAX_MEMO_ENTRIES} entries (next miss re-hashes)"
        );
        memo.clear();
    }
    let hashes = memo.get(key).map_or(0, |entry| entry.hashes);
    memo.insert(
        key.clone(),
        MemoEntry {
            content_hash: content_hash.to_owned(),
            hashes: hashes + 1,
        },
    );
}

/// Read and BLAKE3-hash the complete file. The streaming hasher and the chunk
/// size are the pre-cache ones, so the digest is unchanged.
fn hash_whole_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; HASH_CHUNK];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                hasher.update(&buffer[..read]);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(format!("blake3:{}", hasher.finalize().to_hex()))
}
