//! SPLICE-SIDECAR-LIB-90 (Slice 8): the artifact status block of the sidecar
//! root file, moved verbatim into its own module.
//!
//! One responsibility: the visible status of a persisted artifact — the
//! source file against its pinned identity (`source_status`) and the bundle
//! files against their structural and codec checks (`artifact_status`,
//! `generative_artifact_status`). A missing or corrupt artifact is always
//! reported; nothing is silently accepted.
//!
//! The public symbols keep their names and stay reachable under the same
//! paths — the root re-exports them, so `lumina_sidecar::source_status`,
//! `lumina_sidecar::artifact_status`, `lumina_sidecar::GenerativeArtifactStatus`
//! and `lumina_sidecar::generative_artifact_status` stay resolvable. The two
//! `artifact_status` variants are cfg-gated (`zdata` / not `zdata`) and keep
//! their condition. The crate-internal helpers `basic_artifact_file_check`
//! and `starts_with_zdata_magic` stay private in this module, because no call
//! site outside the moved block uses them.

use std::fs;
use std::path::{Path, PathBuf};

// `io_error` lives in the crate root and is private there; a child module does
// not inherit the parent's item names unqualified, so it is imported here.
// Its visibility is unchanged (`fn`, private in the root).
use crate::io_error;
#[cfg(feature = "zdata")]
use crate::load_zdata;
use crate::{
    ArtifactReference, ArtifactStatus, GenerativeArtifactRef, SidecarError, SourceIdentity,
    SourceStatus,
};

pub fn source_status(path: &Path, source: &SourceIdentity) -> Result<SourceStatus, SidecarError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SourceStatus::Missing)
        }
        Err(error) => return Err(io_error("reading source", path, error)),
    };
    let metadata =
        fs::metadata(path).map_err(|error| io_error("reading source metadata", path, error))?;
    let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    if metadata.len() == source.byte_length && hash == source.content_hash {
        Ok(SourceStatus::Unchanged)
    } else {
        Ok(SourceStatus::SourceChanged)
    }
}

/// Container magic of `.lumina.zdata` bundles. Duplicated from the
/// feature-gated `zdata` module so the structural checks below work in every
/// build.
const ZDATA_MAGIC: [u8; 8] = *b"LUMZDATA";

/// REVIEW-SIDECAR-FOLLOWUP-1: every format name used for `.lumina.zdata`
/// payloads embeds "zdata" (`"zdata"`, `"zdata-mask"`, `"lumina-zdata"`), so a
/// substring match covers all producer spellings without maintaining a list.
/// A reference declaring such a format must carry the container magic; that
/// errs toward visibility (reporting corrupt) instead of silently treating a
/// mislabeled file as an unverifiable opaque payload.
fn format_declares_zdata(format: &str) -> bool {
    format.contains("zdata")
}

/// Outcome of the build-independent file checks shared by both
/// `artifact_status` variants.
enum BasicArtifactCheck {
    /// Absent or not a regular file.
    Missing,
    /// Exists but can never be an artifact this crate persists.
    Corrupt,
    /// Passed the structural checks; payload verification may follow.
    Usable(PathBuf),
}

/// Existence/size floor applied identically with and without the `zdata`
/// feature (REVIEW-SIDECAR-STATUS-1 + REVIEW-SIDECAR-FOLLOWUP-1):
///
/// * absent or non-regular paths are `Missing`,
/// * files smaller than the 8-byte container magic — including empty ones —
///   are `Corrupt`; previously a non-empty <8-byte file slipped through the
///   failed magic read as `Available`,
/// * everything else is handed on with its resolved path.
fn basic_artifact_file_check(
    bundle_root: &Path,
    artifact: &ArtifactReference,
) -> BasicArtifactCheck {
    let path = bundle_root.join(&artifact.relative_path);
    let Ok(metadata) = fs::metadata(&path) else {
        return BasicArtifactCheck::Missing;
    };
    if !metadata.is_file() {
        return BasicArtifactCheck::Missing;
    }
    if metadata.len() < ZDATA_MAGIC.len() as u64 {
        return BasicArtifactCheck::Corrupt;
    }
    BasicArtifactCheck::Usable(path)
}

/// Checks whether the referenced artifact is usable.
///
/// REVIEW-SIDECAR-STATUS-1 / REVIEW-SIDECAR-FOLLOWUP-1: existence alone is no
/// longer sufficient. The check performs, in order:
///
/// 1. `Missing` when the path is absent or not a regular file,
/// 2. `Corrupt` for files smaller than the 8-byte container magic (an empty
///    or undersized artifact is never valid),
/// 3. `Corrupt` when the declared format names a `.lumina.zdata` payload but
///    the file lacks the `LUMZDATA` magic — such a file is mislabeled, not an
///    opaque payload,
/// 4. format-aware verification: files beginning with the `LUMZDATA` container
///    magic are parsed and their per-record BLAKE3 checksums verified eagerly;
///    any parse/checksum/size failure yields `Corrupt`,
/// 5. `Available` otherwise.
///
/// Known limitation (documented SOLL gap, not a silent fallback): opaque
/// non-container payloads cannot be deep-verified because this crate owns no
/// parser for them; they are reported as `Available` once they pass checks
/// 1–2. The reference's declared resolution (`width`/`height`) describes the
/// mask plane and has no generic mapping onto bundle records either; see
/// `feature/architecture/sidecar.md` ("Artefaktstatus-Prüfung") for why this
/// validation belongs to the consuming pipeline loader, not this function.
/// The deep (codec) verification variant is compiled when the
/// `.lumina.zdata` codec is available, i.e. with the `zdata` feature; the
/// structural-only variant below takes over without it so `artifact_status`
/// keeps a definition in every build configuration.
#[cfg(feature = "zdata")]
pub fn artifact_status(bundle_root: &Path, artifact: &ArtifactReference) -> ArtifactStatus {
    let path = match basic_artifact_file_check(bundle_root, artifact) {
        BasicArtifactCheck::Missing => return ArtifactStatus::Missing,
        BasicArtifactCheck::Corrupt => return ArtifactStatus::Corrupt,
        BasicArtifactCheck::Usable(path) => path,
    };
    let has_magic = starts_with_zdata_magic(&path);
    if !has_magic && format_declares_zdata(&artifact.format) {
        return ArtifactStatus::Corrupt;
    }
    if has_magic {
        return match load_zdata(&path) {
            Ok(_) => ArtifactStatus::Available,
            // An oversized/truncated/corrupt container must not count as
            // available just because the file exists.
            Err(_) => ArtifactStatus::Corrupt,
        };
    }
    ArtifactStatus::Available
}

/// Non-zdata build: without the container codec a magic-bearing file cannot
/// be deep-verified here; it
/// counts as usable once it passes the structural checks (documented
/// limitation, not a silent fallback — the same rules as the `zdata` build
/// apply up to the eager checksum pass).
#[cfg(not(feature = "zdata"))]
pub fn artifact_status(bundle_root: &Path, artifact: &ArtifactReference) -> ArtifactStatus {
    let path = match basic_artifact_file_check(bundle_root, artifact) {
        BasicArtifactCheck::Missing => return ArtifactStatus::Missing,
        BasicArtifactCheck::Corrupt => return ArtifactStatus::Corrupt,
        BasicArtifactCheck::Usable(path) => path,
    };
    if !starts_with_zdata_magic(&path) && format_declares_zdata(&artifact.format) {
        return ArtifactStatus::Corrupt;
    }
    ArtifactStatus::Available
}

/// Reads at most 8 bytes and reports whether the file begins with the zdata
/// container magic (`LUMZDATA`), so only containers this crate owns get the
/// (comparatively expensive) full parse+checksum verification. Compiled
/// without the `zdata` feature too: the magic check itself needs only
/// [`std::fs`] and keeps both `artifact_status` builds consistent.
fn starts_with_zdata_magic(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 8];
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    file.read_exact(&mut magic).is_ok() && magic == ZDATA_MAGIC
}

/// GEN-EXPAND-CACHE-1: combined status of a persisted generative artifact.
///
/// Unlike the generic [`ArtifactStatus`] this also verifies the stored
/// generative identity against the current one. A record generated for a
/// different source/recipe/seed/canvas (or a legacy record without a pinned
/// identity) is reported as [`GenerativeArtifactStatus::Stale`] instead of
/// `Available` — the caller must never serve it silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerativeArtifactStatus {
    /// Bundle record exists, passed every checksum check and its pinned
    /// identity matches the current generative identity.
    Available,
    /// The bundle file does not exist (or is not a regular file).
    Missing,
    /// The bundle exists but is unusable (bad magic/version/checksum).
    Corrupt,
    /// The bundle is intact but was produced for a different generative
    /// identity (or carries no verifiable identity at all).
    Stale,
}

/// GEN-EXPAND-CACHE-1: identity-verified status of a generative bundle link.
///
/// `current_identity` is the digest of the generative operation the caller is
/// about to render (for the core cache this is the
/// `GenerativeCacheKey::digest()` of the current source/recipe/seed/canvas).
/// The function never returns `Available` for a mismatching or unverifiable
/// identity, so a stale persisted canvas can never be served silently.
pub fn generative_artifact_status(
    bundle_root: &Path,
    link: &GenerativeArtifactRef,
    current_identity: &str,
) -> GenerativeArtifactStatus {
    match link.artifact_status(bundle_root) {
        ArtifactStatus::Missing => GenerativeArtifactStatus::Missing,
        ArtifactStatus::Corrupt => GenerativeArtifactStatus::Corrupt,
        ArtifactStatus::Available => {
            if link.identity() == Some(current_identity) {
                GenerativeArtifactStatus::Available
            } else {
                GenerativeArtifactStatus::Stale
            }
        }
    }
}
