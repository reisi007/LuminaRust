//! Retouch handlers: the persisted red-eye regions (`red-eye`) and the
//! dust-removal artifacts (`dust-removal`).
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, retouch slice). Pure move: bodies are
//! byte-identical; only the `pub(super)` tokens, the `use` lines and this
//! header are new.

use super::*;
/// G-14: inspect/edit the persisted red-eye regions of one virtual copy.
///
/// LRPAR-G14-REDEYE-AUTO-15 (2.0): `--detect` runs the deterministic,
/// model-free pupil heuristic on the decoded source pixels and lists the
/// candidates read-only; `--detect-apply` persists them explicitly. Detection
/// never runs implicitly and never prefills a recipe on its own.
pub(super) fn red_eye(args: RedEyeArgs) -> Result<(), CliError> {
    if args.detect_apply && !args.detect {
        return Err(CliError::Message(
            "--detect-apply requires --detect (detection is never implicit)".into(),
        ));
    }
    if args.clear && (!args.set.is_empty() || !args.remove.is_empty() || args.detect_apply) {
        return Err(CliError::Message(
            "--clear contradicts --set/--remove/--detect-apply".into(),
        ));
    }
    let wants_mutation =
        !args.set.is_empty() || !args.remove.is_empty() || args.clear || args.detect_apply;
    if args.list && wants_mutation {
        return Err(CliError::Message(
            "--list is read-only; pass no mutation flags with it".into(),
        ));
    }
    // `--set`/`--remove`/`--clear` are explicit edits and always record a
    // history step (unchanged behaviour); `--detect-apply` only writes when it
    // actually changed the persisted regions (idempotent re-application).
    let mut changed = !args.set.is_empty() || !args.remove.is_empty() || args.clear;
    // Detection runs on the decoded source before any recipe stage. It is
    // computed once, before the mutation, so `--detect-apply` persists exactly
    // what `--detect` listed.
    let mut detected: Vec<DetectedRedEye> = Vec::new();
    let mut dropped = 0usize;
    if args.detect {
        let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
        let (frame, _) = decode_input(&args.input, &bytes)?;
        let detection = detect_red_eyes(&frame);
        detected = detection.candidates;
        dropped = detection.dropped;
        if dropped > 0 {
            // Loud, deterministic cap handling: the strongest candidates are
            // kept, the rest are reported — never silently discarded.
            eprintln!(
                "lumina: warning: red-eye detection found {} candidate(s); keeping the \
                 {RED_EYE_MAX_REGIONS} strongest and dropping {dropped}",
                detected.len() + dropped
            );
        }
        info!(
            "red-eye: detected {} candidate(s) ({} dropped) on `{}`",
            detected.len(),
            dropped,
            args.input.display()
        );
    }
    let path = sidecar_path_for(&args.input);
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                args.input.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_id = resolve_mask_copy(&document, args.virtual_copy.as_deref())?;
    let mut actions: Vec<String> = Vec::new();

    if args.clear {
        mask_copy_mut(&mut document, &copy_id)?.recipe.red_eye = None;
        info!("red-eye: cleared stage on copy `{copy_id}`");
        actions.push("red-eye:clear".into());
    } else {
        for spec in &args.set {
            let region = parse_red_eye_region(spec)?;
            let correction = red_eye_mut(&mut document, &copy_id)?;
            match correction
                .regions
                .iter_mut()
                .find(|existing| existing.id == region.id)
            {
                Some(existing) => *existing = region.clone(),
                None => {
                    if correction.regions.len() >= RED_EYE_MAX_REGIONS {
                        return Err(CliError::Message(format!(
                            "red-eye region limit of {RED_EYE_MAX_REGIONS} reached"
                        )));
                    }
                    correction.regions.push(region.clone());
                }
            }
            info!(
                "red-eye: marked `{}` at ({}, {}) r={} on copy `{copy_id}`",
                region.id, region.x, region.y, region.radius
            );
            actions.push(format!("red-eye:set:{}", region.id));
        }
        for id in &args.remove {
            let correction = red_eye_mut(&mut document, &copy_id)?;
            let before = correction.regions.len();
            correction.regions.retain(|region| &region.id != id);
            if correction.regions.len() == before {
                return Err(CliError::Message(format!("unknown red-eye region `{id}`")));
            }
            info!("red-eye: removed `{id}` on copy `{copy_id}`");
            actions.push(format!("red-eye:remove:{id}"));
        }
    }

    if args.detect_apply {
        // Replace only the automatically detected regions; manually marked
        // regions are never touched (deterministic, idempotent apply).
        let before_stage = mask_copy_mut(&mut document, &copy_id)?
            .recipe
            .red_eye
            .clone();
        let applied = {
            let correction = red_eye_mut(&mut document, &copy_id)?;
            correction
                .regions
                .retain(|region| !region.id.starts_with(RED_EYE_DETECT_ID_PREFIX));
            if correction.regions.len() + detected.len() > RED_EYE_MAX_REGIONS {
                return Err(CliError::Message(format!(
                    "red-eye detection would exceed the {RED_EYE_MAX_REGIONS}-region cap: \
                     {} manual region(s) + {} detected; remove regions or clear the stage \
                     (nothing was written)",
                    correction.regions.len(),
                    detected.len()
                )));
            }
            for candidate in &detected {
                let region = candidate.to_region();
                match correction
                    .regions
                    .iter_mut()
                    .find(|existing| existing.id == region.id)
                {
                    Some(existing) => *existing = region,
                    None => correction.regions.push(region),
                }
            }
            correction.regions.len()
        };
        if applied == 0 {
            // A fresh detection that found nothing removes only stale auto
            // regions; an empty stage is stored as absence (identity).
            mask_copy_mut(&mut document, &copy_id)?.recipe.red_eye = None;
        }
        changed |= before_stage != mask_copy_mut(&mut document, &copy_id)?.recipe.red_eye;
        info!(
            "red-eye: applied {} detected region(s) on copy `{copy_id}`",
            detected.len()
        );
        actions.push(format!("detect-apply:{}", detected.len()));
        if dropped > 0 {
            actions.push(format!("detect-dropped:{dropped}"));
        }
    }

    if changed {
        // Loud gate: ranges, ids and the 32-region cap are rejected before
        // anything is written (the sidecar validator re-checks after the edit).
        document
            .validate()
            .map_err(|error| CliError::Message(error.to_string()))?;
        let copy = mask_copy_mut(&mut document, &copy_id)?;
        let final_recipe = copy.recipe.clone();
        let mut id = format!("red-eye-{}", timestamp());
        let mut suffix = 0u32;
        while copy.history.iter().any(|entry| entry.id == id) {
            suffix += 1;
            id = format!("red-eye-{}-{suffix}", timestamp());
        }
        let mut extras = BTreeMap::new();
        extras.insert("step".into(), serde_json::Value::String("red-eye".into()));
        extras.insert(
            "actions".into(),
            serde_json::Value::String(actions.join(",")),
        );
        copy.history.push(HistoryEntry {
            id,
            recipe: final_recipe,
            recorded_at: Some(timestamp()),
            extras,
        });
        save_sidecar(&path, &document)?;
    }
    red_eye_list(&args, &document, &copy_id, &detected, dropped, &actions)
}

/// Mutable access to one virtual copy's red-eye stage, creating an empty
/// (`version = 1`, no regions) stage when none exists.
fn red_eye_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut RedEyeCorrection, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy.recipe.red_eye.get_or_insert_with(|| RedEyeCorrection {
        version: 1,
        regions: Vec::new(),
    }))
}

/// Parses `ID:x,y,radius,desaturate,darken` with normalized values and loud
/// range checks (never clipped).
pub(super) fn parse_red_eye_region(spec: &str) -> Result<RedEyeRegion, CliError> {
    let (id, values) = spec.split_once(':').ok_or_else(|| {
        CliError::Message(format!(
            "invalid red-eye region `{spec}`: expected `ID:x,y,radius,desaturate,darken`"
        ))
    })?;
    if id.is_empty() {
        return Err(CliError::Message(
            "red-eye region id must not be empty".into(),
        ));
    }
    let numbers: Vec<&str> = values.split(',').collect();
    if numbers.len() != 5 {
        return Err(CliError::Message(format!(
            "invalid red-eye region `{spec}`: expected 5 values after the id"
        )));
    }
    let parse = |raw: &str, field: &str| -> Result<f32, CliError> {
        raw.trim().parse::<f32>().map_err(|_| {
            CliError::Message(format!(
                "invalid red-eye {field} `{raw}`: expected a number"
            ))
        })
    };
    let x = parse(numbers[0], "x")?;
    let y = parse(numbers[1], "y")?;
    let radius = parse(numbers[2], "radius")?;
    let desaturate = parse(numbers[3], "desaturate")?;
    let darken = parse(numbers[4], "darken")?;
    if !x.is_finite() || !(0.0..=1.0).contains(&x) {
        return Err(CliError::Message(format!("red-eye x `{x}` out of 0..=1")));
    }
    if !y.is_finite() || !(0.0..=1.0).contains(&y) {
        return Err(CliError::Message(format!("red-eye y `{y}` out of 0..=1")));
    }
    if !radius.is_finite() || radius <= 0.0 || radius > 1.0 {
        return Err(CliError::Message(format!(
            "red-eye radius `{radius}` out of (0, 1]"
        )));
    }
    for (field, value) in [("desaturate", desaturate), ("darken", darken)] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(CliError::Message(format!(
                "red-eye {field} `{value}` out of 0..=1"
            )));
        }
    }
    Ok(RedEyeRegion {
        id: id.into(),
        x,
        y,
        radius,
        desaturate,
        darken,
    })
}

fn red_eye_list(
    args: &RedEyeArgs,
    document: &SidecarDocument,
    copy_id: &str,
    detected: &[DetectedRedEye],
    dropped: usize,
    actions: &[String],
) -> Result<(), CliError> {
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    let correction = copy.recipe.red_eye.as_ref();
    let regions = correction.map(|c| c.regions.as_slice()).unwrap_or(&[]);
    if args.json {
        let detected_json: Vec<serde_json::Value> = detected
            .iter()
            .map(|candidate| {
                serde_json::json!({
                    "id": candidate.id,
                    "x": candidate.x,
                    "y": candidate.y,
                    "radius": candidate.radius,
                    "confidence": candidate.confidence,
                })
            })
            .collect();
        emit(
            true,
            serde_json::json!({
                "command": "red-eye",
                "input": args.input,
                "copy": copy_id,
                "count": regions.len(),
                "red_eye": correction,
                "detected": detected_json,
                "dropped": dropped,
                "actions": actions,
            }),
            "red-eye status listed",
        )
    } else {
        if regions.is_empty() {
            println!("  red-eye: none");
        } else {
            println!("  red-eye: {} region(s)", regions.len());
            for region in regions {
                println!(
                    "    {} x={} y={} radius={} desaturate={} darken={}",
                    region.id, region.x, region.y, region.radius, region.desaturate, region.darken
                );
            }
        }
        if args.detect {
            if detected.is_empty() {
                println!("  red-eye detection: no red pupils found");
            } else {
                println!("  red-eye detection: {} candidate(s)", detected.len());
                for candidate in detected {
                    println!(
                        "    {} x={} y={} radius={} confidence={}",
                        candidate.id,
                        candidate.x,
                        candidate.y,
                        candidate.radius,
                        candidate.confidence
                    );
                }
            }
            if dropped > 0 {
                println!(
                    "  red-eye detection: {dropped} candidate(s) dropped above the \
                     {RED_EYE_MAX_REGIONS}-region cap"
                );
            }
        }
        emit(
            false,
            serde_json::json!({"command":"red-eye","status":"ok"}),
            if actions.is_empty() {
                "red-eye status listed"
            } else {
                "red-eye updated"
            },
        )
    }
}

pub(super) fn dust_removal(args: DustRemovalArgs) -> Result<(), CliError> {
    // Never overwrite the original — or its Lumina bundle files — with the
    // optional render output (REVIEW-CLI-WRITE-1).
    if let Some(output) = &args.render_out {
        reject_protected_output(&args.input, output)?;
    }
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, _raw) = decode_input(&args.input, &bytes)?;

    // Load and validate the repair-region definition.  Region and replacement
    // must have identical dimensions; the region must also match the decoded
    // source frame, because the MVP applies source actions at source resolution.
    let definition = repair_region_input::RepairRegionInput::load(&args.repair_region)
        .map_err(|error| CliError::Message(format!("invalid repair-region input: {error}")))?;
    let kind = definition.source_action_kind().map_err(CliError::Message)?;
    let region_values = repair_region_input::read_region_values(
        &definition.region_path,
        definition.region_width,
        definition.region_height,
    )
    .map_err(CliError::Message)?;
    let replacement_bytes = fs::read(&definition.replacement_path)
        .map_err(|error| io_error(&definition.replacement_path, error))?;
    let replacement_frame = ImageFrame::decode(&replacement_bytes).map_err(|error| {
        CliError::Message(format!("could not decode replacement image: {error}"))
    })?;
    if replacement_frame.width() != definition.region_width
        || replacement_frame.height() != definition.region_height
    {
        return Err(CliError::Message(format!(
            "replacement image {}x{} does not match region {}x{}",
            replacement_frame.width(),
            replacement_frame.height(),
            definition.region_width,
            definition.region_height
        )));
    }
    let region = RepairRegionArtifact {
        id: definition.id.clone(),
        width: definition.region_width,
        height: definition.region_height,
        region: region_values,
        replacement: replacement_frame.pixels().to_vec(),
    };
    region
        .validate()
        .map_err(|error| CliError::Message(format!("invalid repair region: {error}")))?;
    if region.width != frame.width() || region.height != frame.height() {
        return Err(CliError::Message(format!(
            "repair region {}x{} does not match source frame {}x{}; source actions apply at source resolution",
            region.width, region.height, frame.width(), frame.height()
        )));
    }

    // REVIEW-CLI-N2: validate the sidecar and resolve the target copy BEFORE
    // anything is appended to the `.lumina.zdata` bundle. Appending first left
    // orphaned artifact bytes behind whenever the sidecar was missing or the
    // virtual copy did not exist. The recipe stores only a RELATIVE reference
    // (the bundle file name), never an absolute path.
    let sidecar_path = sidecar_path_for(&args.input);
    let mut document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                args.input.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_index = args
        .virtual_copy
        .as_deref()
        .map(|id| {
            document
                .virtual_copies
                .iter()
                .position(|copy| copy.id == id)
                .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`")))
        })
        .transpose()?
        .unwrap_or(0);

    // Persist the artifact bytes into the portable `.lumina.zdata` bundle,
    // appended next to the source.
    let zdata_path = lumina_sidecar::zdata_path_for(&args.input);
    let relative_path = zdata_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repair.zdata")
        .to_string();
    let checksum = region.checksum();
    append_repair_region(&zdata_path, region).map_err(|error| {
        CliError::Message(format!("could not write repair-region bundle: {error}"))
    })?;

    // Record the action spec in the validated virtual copy's recipe.
    let spec = SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind,
        artifact: SourceActionArtifactRef {
            id: definition.id.clone(),
            relative_path: relative_path.clone(),
            checksum: checksum.clone(),
        },
    };
    document.virtual_copies[copy_index]
        .recipe
        .source_actions
        .push(spec);
    document.validate()?;
    save_sidecar(&sidecar_path, &document)?;

    // Optional headless render so the effect is verifiable end-to-end.
    if let Some(output) = &args.render_out {
        let source_actions =
            resolve_source_actions(&document.virtual_copies[copy_index].recipe, &zdata_path)?;
        let rendered = render_frame(
            &frame,
            &RenderContext {
                recipe: &document.virtual_copies[copy_index].recipe,
                camera_white_balance: None,
                source_actions: &source_actions,
                masks: None,
                // F-098-N2: `dust_removal` deliberately does not build a Lensfun
                // corrector. The decoded `RawMetadata` is intentionally discarded
                // here (`let (frame, _raw) = ...`) and the repair-region workflow
                // is a headless, source-resolution verification render without the
                // EXIF scope the corrector requires — `None` keeps the manual model.
                lensfun: None,
                depth: None,
            },
        )?;
        let format = output_format(output)?;
        write_atomically(output, &rendered.frame.encode(format)?)?;
    }

    emit(
        args.json,
        serde_json::json!({
            "command": "dust-removal",
            "input": args.input,
            "virtual_copy": document.virtual_copies[copy_index].id,
            "artifact_id": definition.id,
            "bundle": relative_path,
            "checksum": checksum,
            "status": "ok"
        }),
        "dust removal recorded",
    )
}
