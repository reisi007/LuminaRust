//! MCP-PARITY-C / Impl-A: report substrate of the shared matrix runner.
//!
//! Moved verbatim out of `crates/lumina-cli/src/matrix.rs`
//! (`PairOutcome`, `report_json`, `report_text`, `encode_and_stage`,
//! `write_diff`, `max_abs_diff`, `format_psnr`): the `--json` document is the
//! CLI ≡ MCP parity contract (SOLL §3 — including the `psnr_db` **strings**),
//! so both frontends build it here, once. The staged write keeps the CLI's
//! `.{name}.tmp-*` + fsync + rename scheme so a failing pair leaves neither a
//! half-written golden nor a half-written diff behind.

use crate::error::StageError;
use lumina_core::{ImageFileFormat, ImageFrame};
use std::path::Path;

/// Outcome of one (sample × recipe) pair.
pub(crate) struct PairOutcome {
    pub(crate) sample: String,
    pub(crate) recipe: String,
    pub(crate) status: &'static str,
    /// Golden-relative diagnostic; `None` when no PSNR was computed.
    pub(crate) psnr_db: Option<f64>,
    /// `None` for the `exact` class (PSNR must be infinite).
    pub(crate) min_psnr_db: Option<f64>,
    pub(crate) max_abs_diff: Option<u8>,
    pub(crate) duration_ms: u128,
    pub(crate) golden: std::path::PathBuf,
    pub(crate) output: std::path::PathBuf,
    /// Declared expected route, rendered for the report (always present).
    pub(crate) expected_route: String,
    /// Backend the render port actually used (`"gpu"`/`"cpu"`);
    /// `None` before a successful render.
    pub(crate) route: Option<&'static str>,
    /// Loud CPU routing reasons (empty for a GPU render).
    pub(crate) route_reasons: Vec<String>,
    /// `--require-gpu` mismatch message; folded into `status`/`message` at the
    /// end so it never hides a simultaneous PSNR failure.
    pub(crate) route_error: Option<String>,
    /// Failure reason for the text/JSON report.
    pub(crate) message: Option<String>,
}

impl PairOutcome {
    pub(crate) fn passed(&self) -> bool {
        matches!(
            self.status,
            "pass" | "baseline-created" | "baseline-updated"
        )
    }
}

/// Encodes `frame` as a lossless PNG and atomically publishes it at `target`
/// (same-directory rename; a dropped stage removes the temporary, so no error
/// path leaves a half-written artifact behind).
pub(crate) fn encode_and_stage(frame: &ImageFrame, target: &Path) -> Result<(), StageError> {
    let bytes = frame.encode(ImageFileFormat::Png)?;
    write_bytes_atomically(target, &bytes)
}

/// Publishes `bytes` at `target` atomically: a same-directory temporary file
/// (`.{name}.tmp-*`), `write_all` + `flush` + `sync_all`, then `persist`
/// (= `rename(2)`) over the target.
///
/// The rename is the security-relevant part: it replaces the **directory
/// entry** instead of opening the target, so a symlink at `target` is
/// replaced rather than followed, a hard link is broken instead of rewriting
/// the other name's bytes, and a concurrent reader never observes a
/// truncated/partial file. This is the scheme the golden path
/// ([`encode_and_stage`]) has always used; the full-resolution export and the
/// diff images go through the same function so no matrix artifact is written
/// "through" its target path.
pub(crate) fn write_bytes_atomically(target: &Path, bytes: &[u8]) -> Result<(), StageError> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let filename = target
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "artifact".into());
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| StageError::io(parent, error))?;
    let temporary_path = temporary.path().to_path_buf();
    use std::io::Write as _;
    temporary
        .write_all(bytes)
        .map_err(|error| StageError::io(&temporary_path, error))?;
    temporary
        .flush()
        .map_err(|error| StageError::io(&temporary_path, error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| StageError::io(&temporary_path, error))?;
    temporary
        .persist(target)
        .map_err(|error| StageError::io(target, error.error))?;
    Ok(())
}

/// Writes a diagnostic diff image (`|actual - golden|` per RGB channel, alpha
/// opaque) next to the exports so a failing pair can be inspected visually.
pub(crate) fn write_diff(
    work_dir: &Path,
    name: &str,
    actual: &ImageFrame,
    golden: &ImageFrame,
) -> Result<(), StageError> {
    let target = work_dir.join(format!("{name}.diff.png"));
    let mut pixels = Vec::with_capacity(actual.pixels().len());
    for (index, (a, b)) in actual.pixels().iter().zip(golden.pixels()).enumerate() {
        if index % 4 == 3 {
            pixels.push(255);
        } else {
            pixels.push(a.abs_diff(*b));
        }
    }
    let diff = ImageFrame::new(actual.width(), actual.height(), pixels)?;
    encode_and_stage(&diff, &target)
}

/// Maximum per-channel (RGB) absolute difference between two same-size frames.
pub(crate) fn max_abs_diff(a: &ImageFrame, b: &ImageFrame) -> u8 {
    a.pixels()
        .iter()
        .zip(b.pixels())
        .enumerate()
        .filter(|(index, _)| index % 4 != 3)
        .map(|(_, (x, y))| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

pub(crate) fn format_psnr(value: f64) -> String {
    if value.is_infinite() {
        "inf".to_string()
    } else {
        format!("{value:.2} dB")
    }
}

pub(crate) fn report_text(mode: &str, outcomes: &[PairOutcome], total_ms: u128) -> String {
    let mut text = String::new();
    text.push_str(&format!("matrix ({mode}): {} pair(s)\n", outcomes.len()));
    for pair in outcomes {
        let detail = match (pair.psnr_db, pair.message.as_deref()) {
            (Some(value), _) => format!(
                "psnr={} min={}",
                format_psnr(value),
                pair.min_psnr_db
                    .map(|min| format!("{min} dB"))
                    .unwrap_or_else(|| "inf (exact)".to_string())
            ),
            (None, Some(message)) => message.to_string(),
            (None, None) => "-".to_string(),
        };
        let route = match pair.route {
            Some(actual) => {
                let reasons = if pair.route_reasons.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", pair.route_reasons.join("; "))
                };
                format!("route={actual}{reasons} expected={}", pair.expected_route)
            }
            None => format!("expected={}", pair.expected_route),
        };
        text.push_str(&format!(
            "  [{}] {} / {} ({} ms) {} {} -> {}\n",
            pair.status,
            pair.sample,
            pair.recipe,
            pair.duration_ms,
            route,
            detail,
            pair.output.display()
        ));
    }
    text.push_str(&format!("total: {total_ms} ms"));
    text
}

pub(crate) fn report_json(
    mode: &str,
    recipe_set: &Path,
    golden_dir: &Path,
    comparison_width: u32,
    outcomes: &[PairOutcome],
    total_ms: u128,
) -> serde_json::Value {
    let pairs: Vec<serde_json::Value> = outcomes
        .iter()
        .map(|pair| {
            serde_json::json!({
                "sample": pair.sample,
                "recipe": pair.recipe,
                "status": pair.status,
                "psnr_db": pair.psnr_db.map(|value| {
                    if value.is_infinite() { "inf".to_string() } else { format!("{value:.4}") }
                }),
                "min_psnr_db": pair.min_psnr_db,
                "max_abs_diff": pair.max_abs_diff,
                "duration_ms": pair.duration_ms,
                "expected_route": pair.expected_route,
                "route": pair.route,
                "route_reasons": pair.route_reasons,
                "golden": pair.golden,
                "output": pair.output,
                "message": pair.message,
            })
        })
        .collect();
    let passed = outcomes.iter().filter(|pair| pair.passed()).count();
    serde_json::json!({
        "command": "matrix",
        "mode": mode,
        "recipe_set": recipe_set,
        "golden_dir": golden_dir,
        "comparison_width": comparison_width,
        "pairs_total": outcomes.len(),
        "passed": passed,
        "failed": outcomes.len() - passed,
        "duration_ms": total_ms,
        "pairs": pairs,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::write_bytes_atomically;
    use std::fs;
    use std::os::unix::fs::{symlink, MetadataExt};

    /// SEC-5 (Sicherheitswelle 2a): the export publish must replace the target
    /// **directory entry**, never open the target path.
    ///
    /// Pre-fix the runner used `std::fs::write(&output, &bytes)`, which
    /// (a) followed a symlink at `{sample}__{recipe}.png` and rewrote the file
    /// it pointed at in place (measured: 23 B victim → 49 MB PNG, same inode,
    /// entry still a symlink), and (b) did the same through a hard link, which
    /// destroyed the source image behind that second name. The assertions
    /// below are exactly those two measured outcomes, inverted: the aliased
    /// file stays byte-identical, the entry becomes a regular file with the
    /// new bytes, and the hard-linked source keeps its bytes, inode and its
    /// single remaining name.
    #[test]
    fn export_write_replaces_symlink_and_hardlink_targets() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"\x89PNG\r\n\x1a\nmatrix export bytes (not a real PNG)";
        let victim_original = b"user notes: not an image\n";
        let source_original = b"sample image bytes\n";

        // (1) symlink at the export name -> a foreign file
        let victim = dir.path().join("victim.txt");
        fs::write(&victim, victim_original).unwrap();
        let target = dir.path().join("landscape__g01-identity.png");
        symlink(&victim, &target).unwrap();

        write_bytes_atomically(&target, payload).unwrap();

        assert_eq!(
            fs::read(&victim).unwrap(),
            victim_original,
            "the symlinked victim must stay byte-identical"
        );
        let entry = fs::symlink_metadata(&target).unwrap();
        assert!(
            entry.file_type().is_file(),
            "the export entry must be a regular file, got {:?}",
            entry.file_type()
        );
        assert_eq!(fs::read(&target).unwrap(), payload);

        // (2) hard link at the export name -> the source image itself
        let source = dir.path().join("sample.png");
        fs::write(&source, source_original).unwrap();
        let before = fs::metadata(&source).unwrap();
        let hardlinked = dir.path().join("sample__g01-identity.png");
        fs::hard_link(&source, &hardlinked).unwrap();
        assert_eq!(fs::metadata(&hardlinked).unwrap().ino(), before.ino());

        write_bytes_atomically(&hardlinked, payload).unwrap();

        assert_eq!(
            fs::read(&source).unwrap(),
            source_original,
            "the hard-linked source must stay byte-identical"
        );
        let source_after = fs::metadata(&source).unwrap();
        assert_eq!(source_after.ino(), before.ino());
        assert_eq!(
            source_after.nlink(),
            1,
            "the export must break the hard link, not rewrite the source inode"
        );
        let entry = fs::symlink_metadata(&hardlinked).unwrap();
        assert!(entry.file_type().is_file());
        assert_ne!(entry.ino(), before.ino(), "a fresh inode is published");
        assert_eq!(fs::read(&hardlinked).unwrap(), payload);

        // (3) no staged temporary survives either publish
        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "leftover temporaries: {leftovers:?}");
    }
}
