//! `inspect` (source metadata + sidecar/copy report plus its report types),
//! the opt-in export metadata bake-in helpers, the shared output
//! format/path-safety/atomic-write helpers and the active-copy resolver used by
//! the AI command slices.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, inspect slice). Pure move: bodies are
//! byte-identical; only the `pub(super)`/`pub(crate)` tokens and this header
//! are new.

use super::*;

/// LRPAR-G15-IPTC-S6: outcome of the opt-in metadata bake-in (SOLL §7).
/// `None` without the flag (no record at all); `Some` carries the additive,
/// optional `ExportRecord.metadata_written` payload (`{iim, xmp, status}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MetadataWritten {
    iim: bool,
    xmp: bool,
    /// `"written"` (IIM+XMP spliced) or `"empty"` (empty draft, plain export).
    status: &'static str,
}

impl MetadataWritten {
    pub(crate) fn json(self) -> serde_json::Value {
        serde_json::json!({"iim": self.iim, "xmp": self.xmp, "status": self.status})
    }
}

/// LRPAR-G15-IPTC-S6: merge the source-level draft (+ the routed `keywords`,
/// SOLL §4) into `lumina-iptc` values. Empty/whitespace-only fields stay
/// absent: the tag is omitted on write, never written empty.
fn draft_to_iptc(document: &SidecarDocument) -> IptcMetadata {
    let get = |id: &str| document.metadata.get(id).map(|value| value.to_string());
    IptcMetadata {
        title: get("title"),
        headline: get("headline"),
        description: get("description"),
        copyright_notice: get("copyright_notice"),
        creator: get("creator"),
        credit: get("credit"),
        source: get("source"),
        city: get("city"),
        state_province: get("state_province"),
        country: get("country"),
        date_created: get("date_created"),
        keywords: document.keywords.clone(),
    }
}

/// LRPAR-G15-IPTC-S6: splice draft+keywords (IIM+XMP) into already-encoded
/// JPEG bytes (SOLL §7: post-encode, pixel bytes verbatim). An empty draft
/// keeps the export plain with a loud warning (`"empty"`, never a silent
/// no-op); a Sidecar-valid value over its IIM octet limit fails loudly
/// (field + limit named, no silent truncation). No EXIF write, no adoption
/// of embedded source metadata — only Lumina drafts.
pub(crate) fn bake_metadata_into_jpeg(
    document: &SidecarDocument,
    encoded: &mut Vec<u8>,
    output: &Path,
) -> Result<MetadataWritten, CliError> {
    let meta = draft_to_iptc(document);
    if meta.is_empty() {
        eprintln!(
            "warning: no IPTC draft or keywords for `{}`; exporting without embedded metadata (metadata_written: empty)",
            output.display()
        );
        info!(
            "export without metadata for `{}` (empty draft, metadata_written: empty)",
            output.display()
        );
        return Ok(MetadataWritten {
            iim: false,
            xmp: false,
            status: "empty",
        });
    }
    let spliced = embed_metadata(encoded, &meta).map_err(|error| {
        CliError::Message(format!(
            "metadata bake-in for `{}` failed: {error}",
            output.display()
        ))
    })?;
    *encoded = spliced;
    info!(
        "export with IPTC metadata for `{}` (metadata_written: written)",
        output.display()
    );
    Ok(MetadataWritten {
        iim: true,
        xmp: true,
        status: "written",
    })
}

/// LRPAR-G15-IPTC-S6: append the additive, optional `metadata_written` record
/// (`{iim, xmp, status}` under `extras`) to the copy's export history. Only
/// called with the flag — without it no record is written at all.
pub(crate) fn push_metadata_export_record(
    copy: &mut lumina_sidecar::VirtualCopy,
    output: &Path,
    format: ImageFileFormat,
    written: &MetadataWritten,
) {
    let relative_path = output
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| output.display().to_string());
    let mut extras = BTreeMap::new();
    extras.insert("metadata_written".to_string(), written.json());
    copy.export_records.push(ExportRecord {
        id: format!("export-{}", timestamp()),
        relative_path,
        format: format.default_extension().to_string(),
        exported_at: Some(now_rfc3339_utc()),
        extras,
    });
}

/// One virtual copy as reported by `inspect` (R2-CLI-03): rendered either as
/// JSON or free text from the same data so the two outputs cannot drift.
struct InspectCopy {
    id: String,
    name: String,
    auto_tone: bool,
    matching: bool,
    target_luminance: f64,
    /// LRPAR-G01-BASIC: Develop treatment (`color` | `bw`, absent = `color`).
    treatment: String,
    /// LRPAR-G01-BASIC: Develop profile (absent = `default`).
    profile: String,
}

/// Sidecar state behind an `inspect` report; the invalid variant carries the
/// error that fails the command AFTER the report has been printed.
enum InspectSidecarState {
    Valid {
        source: String,
        copies: Vec<InspectCopy>,
    },
    Missing,
    Invalid(lumina_sidecar::SidecarError),
}

/// R2-CLI-03/-04: shows the source's RAW metadata (via the metadata-only
/// LibRaw path — no full-pixel decode for four EXIF lines) plus the sidecar
/// status and every virtual copy incl. auto-tone/matching state. Free text is
/// the default output; `--json` prints one machine-readable JSON object (the
/// SOLL "JSON-Status"). An invalid sidecar keeps the historical behaviour:
/// the report is still printed, then the command fails loudly.
pub(super) fn inspect(args: InspectArgs) -> Result<(), CliError> {
    // Metadata-only pass (R2-CLI-04): open + unpack + size finalisation;
    // demosaic, colour processing, memory image and promotion never run.
    let raw_metadata = if is_raw_path(&args.input) {
        Some(lumina_raw::read_metadata(&args.input)?)
    } else {
        None
    };

    let path = sidecar_path_for(&args.input);
    let sidecar_state = match load_sidecar(&path) {
        Ok(document) => InspectSidecarState::Valid {
            source: document.source.relative_name.clone(),
            copies: document
                .virtual_copies
                .iter()
                .map(|copy| InspectCopy {
                    id: copy.id.clone(),
                    name: copy.name.clone(),
                    auto_tone: copy.recipe.auto_features.enable_auto_tone,
                    matching: copy.recipe.auto_features.match_total_exposure,
                    target_luminance: copy.recipe.auto_features.target_luminance,
                    treatment: copy.recipe.treatment().to_string(),
                    profile: copy.recipe.develop_profile().to_string(),
                })
                .collect(),
        },
        Err(lumina_sidecar::SidecarError::Missing(_)) => InspectSidecarState::Missing,
        Err(error) => InspectSidecarState::Invalid(error),
    };

    if args.json {
        let raw_json = raw_metadata.as_ref().map(|metadata| {
            serde_json::json!({
                "width": metadata.width,
                "height": metadata.height,
                "orientation": metadata.orientation,
                "camera_make": metadata.camera_make,
                "camera_model": metadata.camera_model,
                "iso": metadata.iso,
                "shutter": metadata.shutter,
                "aperture": metadata.aperture,
                "lens": metadata.lens,
            })
        });
        let sidecar_json = match &sidecar_state {
            InspectSidecarState::Valid { source, copies } => serde_json::json!({
                "path": path,
                "status": "valid",
                "source": source,
                "virtual_copies": copies.iter().map(|copy| serde_json::json!({
                    "id": copy.id,
                    "name": copy.name,
                    "auto_tone": copy.auto_tone,
                    "match_total_exposure": copy.matching,
                    "target_luminance": copy.target_luminance,
                    "treatment": copy.treatment,
                    "profile": copy.profile,
                })).collect::<Vec<_>>(),
            }),
            InspectSidecarState::Missing => serde_json::json!({
                "path": path,
                "status": "missing",
                "virtual_copies": [{
                    "id": "vc-original",
                    "name": "Original",
                    "auto_tone": false,
                    "match_total_exposure": false,
                    "target_luminance": 0.5,
                }],
            }),
            InspectSidecarState::Invalid(_) => {
                serde_json::json!({"path": path, "status": "invalid"})
            }
        };
        println!(
            "{}",
            serde_json::json!({
                "command": "inspect",
                "input": args.input,
                "raw": raw_json,
                "sidecar": sidecar_json,
            })
        );
    } else {
        if let Some(metadata) = &raw_metadata {
            println!(
                "raw: {}x{} orientation {}",
                metadata.width, metadata.height, metadata.orientation
            );
            println!(
                "camera: {} {}",
                metadata.camera_make.as_deref().unwrap_or("unknown"),
                metadata.camera_model.as_deref().unwrap_or("unknown")
            );
            println!(
                "iso: {:?}, shutter: {:?}, aperture: {:?}, lens: {:?}",
                metadata.iso, metadata.shutter, metadata.aperture, metadata.lens
            );
        }
        match &sidecar_state {
            InspectSidecarState::Valid { source, copies } => {
                println!("sidecar: valid ({})", path.display());
                println!("source: {source}");
                for copy in copies {
                    println!("virtual-copy: {} [{}]", copy.name, copy.id);
                    println!(
                        "auto-tone: {} matching: {} target-luminance: {}",
                        copy.auto_tone, copy.matching, copy.target_luminance
                    );
                    println!("treatment: {} profile: {}", copy.treatment, copy.profile);
                }
            }
            InspectSidecarState::Missing => {
                println!("sidecar: missing ({})", path.display());
                println!("virtual-copy: Original [vc-original] (default)");
            }
            InspectSidecarState::Invalid(_) => {
                println!("sidecar: invalid ({})", path.display());
            }
        }
    }
    match sidecar_state {
        InspectSidecarState::Invalid(error) => Err(error.into()),
        _ => Ok(()),
    }
}

pub(crate) fn output_format(path: &Path) -> Result<ImageFileFormat, CliError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ImageFileFormat::from_extension(&extension).ok_or_else(|| {
        CliError::Message(format!(
            "unsupported output extension `.{extension}`; use png, jpg, jpeg, or webp"
        ))
    })
}

pub(crate) fn validate_format(format: &str) -> Result<(), CliError> {
    parse_output_format(format).map(|_| ())
}

/// Parses the `--format` value into an [`ImageFileFormat`] or returns the same
/// loud usage error `validate_format` produced. Single source so callers that
/// must actually encode in the requested format (e.g. `denoise --render`) do
/// not re-implement the mapping (M1).
pub(super) fn parse_output_format(format: &str) -> Result<ImageFileFormat, CliError> {
    ImageFileFormat::from_extension(format).ok_or_else(|| {
        CliError::Message(format!(
            "unsupported format `{format}`; use png, jpg, jpeg, or webp"
        ))
    })
}

pub(crate) fn format_extension(format: &str) -> &'static str {
    match format.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "jpg",
        "webp" => "webp",
        _ => "png",
    }
}

pub(crate) fn validate_quality(quality: u8) -> Result<(), CliError> {
    if (1..=100).contains(&quality) {
        Ok(())
    } else {
        Err(CliError::Message("quality must be in 1..=100".into()))
    }
}

pub(super) fn reject_same_path(input: &Path, output: &Path) -> Result<(), CliError> {
    if lumina_sidecar::paths_resolve_equal(input, output).map_err(|error| io_error(input, error))? {
        return Err(CliError::Message(
            "input and output resolve to the same path; refusing to overwrite the original".into(),
        ));
    }
    Ok(())
}

/// REVIEW-CLI-WRITE-1: refuses an output path that would clobber the original
/// source, one of its Lumina bundle files (`<input>.lumina.json`,
/// `<input>.lumina.zdata`) or a hard link to them. Path equality covers
/// canonical aliases (including not-yet-existing targets, resolved against
/// their parent directory); `(dev, inode)` identity additionally catches hard
/// links, which canonicalization cannot see.
pub(crate) fn reject_protected_output(input: &Path, output: &Path) -> Result<(), CliError> {
    reject_same_path(input, output)?;
    let output_resolved = resolve_candidate(output).map_err(|error| io_error(output, error))?;
    let protected: Vec<(&str, PathBuf)> = vec![
        ("sidecar", lumina_sidecar::sidecar_path_for(input)),
        (
            "mask/source-action bundle",
            lumina_sidecar::zdata_path_for(input),
        ),
    ];
    for (kind, target) in protected {
        let target_resolved =
            resolve_candidate(&target).map_err(|error| io_error(&target, error))?;
        if target_resolved == output_resolved {
            return Err(CliError::Message(format!(
                "output `{}` would overwrite the Lumina {kind} `{}`; refusing (non-destructive guarantee)",
                output.display(),
                target.display()
            )));
        }
        // Hard-link alias: the same bundle file under a different directory
        // entry. Canonicalization cannot see it, so `(dev, inode)` identity is
        // checked in addition (CLI-GUARD-HARDLINK-1).
        if paths_are_same_file(&target, output).map_err(|error| io_error(&target, error))? {
            return Err(CliError::Message(format!(
                "output `{}` is a hard link to the Lumina {kind} `{}`; refusing to overwrite the bundle (non-destructive guarantee)",
                output.display(),
                target.display()
            )));
        }
    }
    if paths_are_same_file(input, output).map_err(|error| io_error(input, error))? {
        return Err(CliError::Message(format!(
            "output `{}` is a hard link to the input `{}`; refusing to overwrite the original",
            output.display(),
            input.display()
        )));
    }
    Ok(())
}

/// Resolves `path` to a comparable identity: existing paths are canonicalized,
/// missing ones are resolved against their canonical parent directory (the
/// same convention as `lumina_sidecar::paths_resolve_equal`).
fn resolve_candidate(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        fs::canonicalize(path)
    } else {
        let parent = fs::canonicalize(path.parent().unwrap_or_else(|| Path::new(".")))?;
        Ok(parent.join(path.file_name().unwrap_or_default()))
    }
}

/// Unix: true when both paths refer to the same underlying file via
/// `(dev, inode)` identity — this catches hard links between distinct paths.
#[cfg(unix)]
fn paths_are_same_file(a: &Path, b: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    if !(a.exists() && b.exists()) {
        return Ok(false);
    }
    let (meta_a, meta_b) = (fs::metadata(a)?, fs::metadata(b)?);
    Ok(meta_a.dev() == meta_b.dev() && meta_a.ino() == meta_b.ino())
}

/// Non-unix fallback: no portable inode identity exists; only path equality
/// (checked separately above) applies.
#[cfg(not(unix))]
fn paths_are_same_file(_a: &Path, _b: &Path) -> std::io::Result<bool> {
    Ok(false)
}

pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    // Delegate to the shared atomic-write helper in `lumina-sidecar` so the
    // CLI and GUI use identical atomic-write semantics.
    lumina_sidecar::write_atomically(path, bytes)?;
    Ok(())
}

/// REVIEW-CLI-N6: staged write for the export/sidecar two-artifact sequence.
///
/// The encoded bytes are written into a temporary file inside the target's
/// directory — same `.{name}.tmp-*` scheme, flush and fsync steps as
/// [`lumina_sidecar::write_atomically`] — but the target name is NOT yet
/// taken. [`StagedArtifact::commit`] later renames the temporary into place
/// (a same-directory rename, atomic per POSIX). Dropping an uncommitted stage
/// deletes the temporary, so every error path between `stage` and `commit`
/// leaves neither artifact behind: this is what lets `process_selected` order
/// the sequence as *stage export → save sidecar → commit export* and still
/// roll back to "nothing changed" when the sidecar save fails.
#[derive(Debug)]
pub(crate) struct StagedArtifact {
    pub(crate) temporary: tempfile::NamedTempFile,
    pub(crate) target: PathBuf,
}

impl StagedArtifact {
    /// Stage `bytes` for `target`: create, fill, flush and fsync a temporary
    /// file in `target`'s parent directory so a later `commit` is a
    /// same-directory (same-filesystem) rename. Fails before any sidecar
    /// mutation could happen in the caller's sequence.
    pub(crate) fn stage(target: &Path, bytes: &[u8]) -> Result<Self, CliError> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        let filename = target
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "artifact".into());
        let mut temporary = tempfile::Builder::new()
            .prefix(&format!(".{filename}.tmp-"))
            .tempfile_in(parent)
            .map_err(|error| io_error(parent, error))?;
        let temporary_path = temporary.path().to_path_buf();
        temporary
            .write_all(bytes)
            .map_err(|error| io_error(&temporary_path, error))?;
        temporary
            .flush()
            .map_err(|error| io_error(&temporary_path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| io_error(&temporary_path, error))?;
        Ok(Self {
            temporary,
            target: target.to_path_buf(),
        })
    }

    /// Publish the staged bytes by renaming them over the target path. The
    /// staged file must not outlive this call either way: on success it has
    /// been renamed, on failure the `PersistError` keeps nothing and the
    /// dropped `NamedTempFile` removes the temporary.
    pub(crate) fn commit(self) -> Result<(), CliError> {
        self.temporary
            .persist(&self.target)
            .map_err(|error| io_error(&self.target, error.error))?;
        Ok(())
    }
}
