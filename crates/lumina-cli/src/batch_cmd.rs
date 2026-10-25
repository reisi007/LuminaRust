//! `batch` (collection, dedup, per-item isolation, summary), `reindex` and
//! the shared `emit` output helper.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, batch slice). Pure move: bodies are
//! byte-identical; only the `pub(super)`/`pub(crate)` tokens and this header
//! are new.

use super::*;

pub(super) fn reindex(args: IndexArgs) -> Result<(), CliError> {
    let mut files = Vec::new();
    collect_sidecars(&args.input, &mut files)?;
    let mut valid = 0usize;
    let mut invalid: Vec<String> = Vec::new();
    for path in files {
        match load_sidecar(&path) {
            Ok(_) => valid += 1,
            // REVIEW-CLI-N4: corrupt sidecars are never ignored silently —
            // each one is reported and the command exits non-zero so scripts
            // and the future index adapter notice the broken state.
            Err(error) => invalid.push(format!("{}: {error}", path.display())),
        }
    }
    for entry in &invalid {
        eprintln!("warning: invalid sidecar: {entry}");
    }
    let invalid_count = invalid.len();
    let text = format!("reindexed: {valid} valid, {invalid_count} invalid");
    emit(
        args.json,
        serde_json::json!({
            "command":"reindex",
            "input":args.input,
            "sidecars":valid,
            "invalid":invalid_count,
            "errors":invalid,
            "status": if invalid_count == 0 { "ok" } else { "invalid-sidecars" }
        }),
        &text,
    )?;
    if invalid_count != 0 {
        return Err(CliError::Message(format!(
            "reindex found {invalid_count} invalid sidecar(s)"
        )));
    }
    Ok(())
}

pub(super) fn batch(args: BatchArgs) -> Result<(), CliError> {
    if args.jobs == 0 {
        return Err(CliError::Message("--jobs must be greater than zero".into()));
    }
    validate_format(&args.format)?;
    validate_quality(args.quality)?;
    let mut inputs = Vec::new();
    collect_images(&args.input, &mut inputs)?;
    // R2-CLI-11: drop same-file duplicates (hard links / inode aliases reached
    // under two names) so `--jobs > 1` never processes one file twice in
    // parallel and never appends duplicate history entries.
    let inputs = dedup_same_file_inputs(inputs);
    // REVIEW-CLI-BATCHCOLLIDE-1: outputs are name-based inside ONE flat
    // directory, so distinct inputs can map onto the same target file name
    // (`a/x.arw` and `b/x.png` both write `x.png`). Refuse the whole run up
    // front — before the output directory even exists — instead of letting
    // later items silently overwrite earlier ones.
    reject_duplicate_batch_targets(&inputs, &args.format)?;
    lumina_cache::create_dir_all_nofollow(&args.output).map_err(|e| io_error(&args.output, e))?;
    let total = inputs.len();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build()
        .map_err(|e| CliError::Message(e.to_string()))?;
    // R2-CLI-06: per-item progress goes to stderr as items finish; the
    // collected mask warnings of each item are reported in the item JSON
    // below (same channel render/export use).
    let results = pool.install(|| {
        inputs
            .par_iter()
            .enumerate()
            .map(|(index, input)| batch_one(input, index, total, &args))
            .collect::<Vec<_>>()
    });
    let failed = results.iter().filter(|r| r.is_err()).count();
    if args.json {
        let items = results
            .iter()
            .map(|r| match r {
                Ok(v) => serde_json::json!({"status":"ok","input":v.input,"mask_warnings":v.mask_warnings}),
                Err(e) => serde_json::json!({"status":"failed","error":e.to_string()}),
            })
            .collect::<Vec<_>>();
        // R2-CLI-08: serialization of a plain strings/arrays JSON value is
        // practically infallible, but a worker panic must never be the
        // failure mode. Fall back loudly instead of unwrapping.
        let payload = serde_json::to_string(&items).unwrap_or_else(|error| {
            eprintln!("warning: batch summary serialization failed: {error}");
            String::from("[]")
        });
        println!("{payload}");
    } else {
        println!(
            "batch: {} succeeded, {} failed",
            results.len() - failed,
            failed
        );
    }
    if failed != 0 {
        // R2-CLI-07: partial batch failure exits with its own documented code
        // (3) instead of being indistinguishable from a hard runtime error.
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

/// One successfully processed (or resumed/skipped/dry-run) batch item. The
/// collected mask warnings travel with the item so the batch summary can
/// report them like render/export do (R2-CLI-06).
#[derive(Debug, Clone)]
pub(crate) struct BatchItemSuccess {
    input: String,
    mask_warnings: Vec<String>,
}

/// Rejects inputs whose name-based batch targets collide after the output
/// extension is normalized, listing the colliding pair. Runs before any
/// output is written so a rejected batch leaves no partial state.
fn reject_duplicate_batch_targets(inputs: &[PathBuf], format: &str) -> Result<(), CliError> {
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
    for input in inputs {
        let name = input
            .file_name()
            .map(|name| name.to_os_string())
            .ok_or_else(|| CliError::Message("input has no file name".into()))?;
        let target = PathBuf::from(name)
            .with_extension(format_extension(format))
            .to_string_lossy()
            .into_owned();
        match seen.get(&target) {
            Some(first) if first != input => {
                return Err(CliError::Message(format!(
                    "batch output collision: `{}` and `{}` both write `{}` into the output directory; refusing to silently overwrite (mirror the directory structure or split the run)",
                    first.display(),
                    input.display(),
                    target
                )));
            }
            _ => {
                seen.insert(target, input.clone());
            }
        }
    }
    Ok(())
}

/// Processes one batch item. `index`/`total` drive the stderr progress line
/// (R2-CLI-06); the item's mask warnings are returned in
/// [`BatchItemSuccess::mask_warnings`] instead of being discarded.
/// R2-CLI-11: deduplicates batch inputs by filesystem identity so the same
/// underlying file reached under two names (hard link, alias) is processed
/// exactly once — previously `--jobs > 1` ran both names in parallel and the
/// run produced duplicate history entries with last-write-wins outputs.
///
/// Unix identifies files by `(dev, inode)`; other platforms fall back to
/// canonical-path identity (symlink aliases are collapsed there, but hard-link
/// aliases are NOT detectable portably — documented limit). Entries whose
/// metadata cannot be read are kept: the decode step reports them loudly.
pub(super) fn dedup_same_file_inputs(inputs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut kept = Vec::with_capacity(inputs.len());
    #[cfg(unix)]
    {
        use std::collections::BTreeSet;
        use std::os::unix::fs::MetadataExt;
        let mut seen = BTreeSet::new();
        for input in inputs {
            let key = fs::metadata(&input)
                .ok()
                .map(|meta| (meta.dev(), meta.ino()));
            let duplicate = match key {
                Some(key) => !seen.insert(key),
                // Unreadable stat: keep the entry; decoding fails loudly later.
                None => false,
            };
            if !duplicate {
                kept.push(input);
            }
        }
    }
    #[cfg(not(unix))]
    {
        use std::collections::BTreeSet;
        let mut seen = BTreeSet::new();
        for input in inputs {
            let key = fs::canonicalize(&input).unwrap_or_else(|_| input.clone());
            if seen.insert(key) {
                kept.push(input);
            }
        }
    }
    kept
}

pub(super) fn batch_one(
    input: &Path,
    index: usize,
    total: usize,
    args: &BatchArgs,
) -> Result<BatchItemSuccess, CliError> {
    let name = input
        .file_name()
        .ok_or_else(|| CliError::Message("input has no file name".into()))?;
    let label = format!("[batch {}/{}] {}", index + 1, total, name.to_string_lossy());
    let output = args
        .output
        .join(name)
        .with_extension(format_extension(&args.format));
    let status = args
        .output
        .join(format!("{}.status.json", name.to_string_lossy()));
    if args.resume && status.exists() && output.is_file() {
        let state = fs::read_to_string(&status).map_err(|e| io_error(&status, e))?;
        // REVIEW-CLI-N3: resume decides on the PARSED JSON status, not on a
        // substring match of the raw file; a malformed status file counts as
        // "not done" and the item is reprocessed.
        if serde_json::from_str::<BatchStatusFile>(&state)
            .ok()
            .is_some_and(|state| state.status == "ok")
        {
            eprintln!("{label}: skipped (resume)");
            return Ok(BatchItemSuccess {
                input: input.display().to_string(),
                mask_warnings: Vec::new(),
            });
        }
    }
    // R2-CLI-06: this item's collected mask warnings survive the scope of the
    // dry-run guard so they can be reported on the progress line and in the
    // item JSON below.
    let mut last_warnings = Vec::new();
    if !args.dry_run {
        if args.update_masks || args.force_render {
            let sidecar = sidecar_path_for(input);
            let mut document = load_sidecar(&sidecar)?;
            let id = args.virtual_copy.as_deref().unwrap_or("vc-original");
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == id)
                .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`")))?;
            if args.update_masks {
                copy.recipe
                    .options
                    .insert("update_masks".into(), "true".into());
            }
            if args.force_render {
                copy.recipe
                    .options
                    .insert("force_render".into(), "true".into());
            }
            save_sidecar(&sidecar, &document)?;
        }
        let mut last = None;
        for _ in 0..=args.retry {
            // R2-CLI-06: collect this attempt's mask warnings instead of
            // discarding them (`&mut Vec::new()`).
            let mut mask_warnings = Vec::new();
            match process_selected(
                ProcessArgs {
                    input: input.to_path_buf(),
                    output: output.clone(),
                    preset: None,
                    exposure: None,
                    contrast: None,
                    whites: None,
                    blacks: None,
                    highlights: None,
                    shadows: None,
                    auto_tone: false,
                    match_total_exposure: false,
                    target_luminance: 0.5,
                    write_metadata: args.write_metadata,
                },
                args.quality,
                args.virtual_copy.as_deref(),
                args.mask_policy.to_policy(),
                &mut mask_warnings,
            ) {
                // LRPAR-G15-IPTC-S6: a bake-in failure (non-JPEG item, IIM
                // limit) fails THIS item loudly with its reason (`failed`).
                Ok(_) => {
                    last = None;
                    // Keep the warnings of the SUCCESSFUL attempt only.
                    last_warnings = mask_warnings;
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        if let Some(e) = last {
            eprintln!("{label}: failed: {e}");
            return Err(e);
        }
    }
    // R2-CLI-08: an unwritable/serializing status must fail THIS item loudly
    // instead of panicking inside the rayon pool (which would tear down the
    // whole batch). A plain string/number JSON value is practically
    // infallible; the error branch is defense-in-depth.
    let status_payload = serde_json::json!({
        "input": input,
        "output": output,
        "status": if args.dry_run { "dry-run" } else { "ok" }
    });
    let status_bytes = serde_json::to_vec(&status_payload).map_err(|error| {
        CliError::Message(format!(
            "could not serialize batch status for `{}`: {error}",
            input.display()
        ))
    })?;
    write_atomically(&status, &status_bytes)?;
    if args.dry_run {
        eprintln!("{label}: dry-run");
    } else if last_warnings.is_empty() {
        eprintln!("{label}: ok");
    } else {
        eprintln!("{label}: ok ({} mask warning(s))", last_warnings.len());
    }
    Ok(BatchItemSuccess {
        input: input.display().to_string(),
        mask_warnings: last_warnings,
    })
}

pub(super) fn collect_images(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), CliError> {
    Ok(lumina_stages::paths::collect_tree_files(
        path,
        output,
        has_image_extension,
    )?)
}
/// Supported input extensions for batch collection (R2-CLI-01): raster
/// formats plus EVERY RAW extension exported by `lumina_raw::RAW_EXTENSIONS`.
/// Referencing the single shared list here and in [`is_raw_path`] is the whole
/// point of the fix — the previous private 9-extension copy silently skipped
/// RAF/ORF/etc. in batch while single-file decode accepted them.
pub(super) fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lowered = e.to_ascii_lowercase();
            matches!(lowered.as_str(), "png" | "jpg" | "jpeg" | "webp")
                || lumina_raw::is_raw_extension(&lowered)
        })
        .unwrap_or(false)
}

pub(crate) fn emit(json: bool, value: serde_json::Value, text: &str) -> Result<(), CliError> {
    if json {
        println!("{}", value);
    } else {
        println!("{text}");
    }
    Ok(())
}
