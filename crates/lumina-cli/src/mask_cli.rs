//! Mask command and mask-library state transitions, extracted verbatim from `main.rs` for SPLICE-CLI-MAIN-89.

use super::*;

pub(crate) fn preflight_masks(
    input: &Path,
    virtual_copy: Option<&str>,
    update: bool,
    policy: MaskPolicy,
) -> Result<(), CliError> {
    let path = sidecar_path_for(input);
    let document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let id = virtual_copy.unwrap_or("vc-original");
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`")))?;
    let root = input.parent().unwrap_or_else(|| Path::new("."));
    let missing = copy
        .mask_library
        .iter()
        .filter(|mask| {
            !matches!(mask.status, MaskStatus::Valid)
                || mask.artifact.as_ref().is_none_or(|artifact| {
                    artifact_status(root, artifact) != ArtifactStatus::Available
                })
        })
        .count();
    if missing == 0 {
        return Ok(());
    }
    if policy == MaskPolicy::Strict {
        return Err(CliError::Message(format!(
            "strict mask policy: {missing} mask(s) are missing or unavailable for `{id}`; command aborted"
        )));
    }
    // Harmonized default (REVIEW-CLI-EXPORTMASK-1): warn-and-continue. An
    // explicit `--update-masks` is honoured by the render itself — masks are
    // re-inferred when an engine is available and the command fails loudly
    // when none is; it never silently succeeds with stale pixels.
    if update {
        eprintln!(
            "warning: {missing} mask(s) for `{id}` are missing or unavailable; --update-masks will re-infer them during the render and fail loudly if no inference engine is installed"
        );
    } else {
        eprintln!(
            "warning: {missing} mask(s) for `{id}` are missing or unavailable; they will not be applied (use --update-masks when an inference engine is installed)"
        );
    }
    Ok(())
}

/// Persists the ONE-SHOT `--update-masks` request into the named virtual
/// copy's recipe options — the same channel develop/batch/mask use. The next
/// render through `process_selected` consumes it and removes it from the
/// persisted recipe again (REVIEW-CLI-MASKFLAG-1).
pub(crate) fn mark_masks_pending_refresh(
    input: &Path,
    virtual_copy: Option<&str>,
) -> Result<(), CliError> {
    let path = sidecar_path_for(input);
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let id = virtual_copy.unwrap_or("vc-original");
    let Some(copy) = document
        .virtual_copies
        .iter_mut()
        .find(|copy| copy.id == id)
    else {
        return Err(CliError::Message(format!("unknown virtual copy `{id}`")));
    };
    copy.recipe
        .options
        .insert("update_masks".into(), "true".into());
    save_sidecar(&path, &document)?;
    Ok(())
}

pub(crate) fn mask(args: MaskArgs) -> Result<(), CliError> {
    let path = sidecar_path_for(&args.input);
    let mut document = load_sidecar(&path)?;
    let wants_mutation = args.update_masks
        || args.add_ai_select.is_some()
        || args.add_luminance_range
        || args.add_color_range
        || args.combine.is_some()
        || args.duplicate.is_some()
        || args.attach_layer.is_some()
        || args.show_layer.is_some()
        || args.hide_layer.is_some()
        || args.local_layer.is_some()
        || !args.set_local_adjustments.is_empty()
        || !args.reset_local_adjustments.is_empty();
    if args.list && !wants_mutation {
        return mask_list(&args, &document);
    }
    if !wants_mutation {
        // Historical behaviour: without flags the command reports status.
        return mask_list(&args, &document);
    }
    let copy_id = resolve_mask_copy(&document, args.virtual_copy.as_deref())?;
    let local_set_specs = &args.set_local_adjustments;
    let local_reset_specs = &args.reset_local_adjustments;
    let local_transaction = if local_set_specs.is_empty() && local_reset_specs.is_empty() {
        None
    } else {
        Some(mask_local::LocalAdjustmentTransaction::capture(
            &document,
            &copy_id,
            args.local_layer.as_deref(),
        )?)
    };
    let mut actions: Vec<String> = Vec::new();
    // Decode once for every mutation that mints a mask definition
    // (dimensions for the geometry context); a loud error instead of
    // zero-sized geometry.
    let frame_dims = if args.add_ai_select.is_some()
        || args.add_luminance_range
        || args.add_color_range
        || args.combine.is_some()
    {
        let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
        let (frame, _) = decode_input(&args.input, &bytes)?;
        Some((frame.width(), frame.height()))
    } else {
        None
    };
    if let Some(kind) = args.add_ai_select.as_deref() {
        let name = require_mask_name(args.name.as_deref())?;
        mask_add_ai(
            &mut document,
            &copy_id,
            kind,
            name,
            args.detail.as_deref(),
            frame_dims,
        )?;
        info!("mask: added ai-select `{kind}` as `{name}` on copy `{copy_id}`");
        actions.push(format!("add-ai-select:{name}"));
    }
    if args.add_luminance_range {
        let name = require_mask_name(args.name.as_deref())?;
        let min = args.range_min.ok_or_else(|| {
            CliError::Message("--add-luminance-range requires --range-min".into())
        })?;
        let max = args.range_max.ok_or_else(|| {
            CliError::Message("--add-luminance-range requires --range-max".into())
        })?;
        let feather = args.feather.unwrap_or(0.0);
        mask_add_luminance(&mut document, &copy_id, name, min, max, feather, frame_dims)?;
        info!("mask: added luminance-range `{name}` on copy `{copy_id}`");
        actions.push(format!("add-luminance-range:{name}"));
    }
    if args.add_color_range {
        let name = require_mask_name(args.name.as_deref())?;
        let feather = args.feather.unwrap_or(0.0);
        mask_add_color(
            &mut document,
            &copy_id,
            name,
            args.hue_center,
            args.hue_width,
            args.sat_min,
            args.sat_max,
            args.lum_min,
            args.lum_max,
            feather,
            frame_dims,
        )?;
        info!("mask: added color-range `{name}` on copy `{copy_id}`");
        actions.push(format!("add-color-range:{name}"));
    }
    if let Some(op) = args.combine.as_deref() {
        let name = require_mask_name(args.name.as_deref())?;
        let inputs = args
            .inputs
            .as_deref()
            .ok_or_else(|| CliError::Message("--combine requires --inputs <ref,...>".into()))?;
        mask_combine(&mut document, &copy_id, op, name, inputs, frame_dims)?;
        info!("mask: combined `{op}` as `{name}` on copy `{copy_id}`");
        actions.push(format!("combine:{name}"));
    }
    if let Some(source) = args.duplicate.as_deref() {
        let name = require_mask_name(args.name.as_deref())?;
        mask_duplicate(&mut document, &copy_id, source, name)?;
        info!("mask: duplicated `{source}` as `{name}` on copy `{copy_id}`");
        actions.push(format!("duplicate:{name}"));
    }
    if let Some(target) = args.attach_layer.as_deref() {
        mask_attach_layer(&mut document, &copy_id, target)?;
        info!("mask: attached layer for `{target}` on copy `{copy_id}`");
        actions.push(format!("attach-layer:{target}"));
    }
    if let Some(layer) = args.show_layer.as_deref() {
        mask_set_layer_visible(&mut document, &copy_id, layer, true)?;
        info!("mask: layer `{layer}` visible on copy `{copy_id}`");
        actions.push(format!("show-layer:{layer}"));
    }
    if let Some(layer) = args.hide_layer.as_deref() {
        mask_set_layer_visible(&mut document, &copy_id, layer, false)?;
        info!("mask: layer `{layer}` hidden on copy `{copy_id}`");
        actions.push(format!("hide-layer:{layer}"));
    }
    mask_local::apply_local_adjustment_flags(
        &mut document,
        &copy_id,
        args.local_layer.as_deref(),
        local_set_specs,
        local_reset_specs,
        &mut actions,
    )?;
    if args.update_masks {
        let copies = if let Some(id) = args.virtual_copy.as_deref() {
            document
                .virtual_copies
                .iter_mut()
                .filter(|copy| copy.id == id)
                .collect::<Vec<_>>()
        } else {
            document.virtual_copies.iter_mut().collect::<Vec<_>>()
        };
        if args.virtual_copy.is_some() && copies.is_empty() {
            return Err(CliError::Message("unknown virtual copy".into()));
        }
        for copy in copies {
            for mask in &mut copy.mask_library {
                mask.status = lumina_sidecar::MaskStatus::Pending;
            }
            // GUI-GEN-GRANULAR-10 / M2: arm the ONE-SHOT explicit refresh in
            // the recipe as well. Without it the next render would re-infer the
            // pending masks with `refresh == false` and report the deliberately
            // requested work as an implicit re-inference. `process_selected`
            // consumes and removes the flag after the successful render
            // (REVIEW-CLI-MASKFLAG-1).
            copy.recipe
                .options
                .insert("update_masks".into(), "true".into());
        }
        info!("mask: marked masks pending (update_masks)");
        actions.push("update-masks".into());
    }
    if let Some(transaction) = local_transaction.as_ref() {
        transaction.append_history_entry(&mut document, &copy_id, &actions)?;
    }
    // Loud gate: arity, unknown references, cycles, ranges and ai_select
    // placement are rejected before anything is written.
    document.validate()?;
    if let Some(transaction) = local_transaction.as_ref() {
        save_sidecar_if_unchanged(&path, &document, Some(transaction.expected_revision()))?;
    } else {
        save_sidecar(&path, &document)?;
    }
    emit(
        args.json,
        serde_json::json!({"command":"mask", "input":args.input, "copy":copy_id, "actions":actions, "status":"ok"}),
        &format!("mask updated: {}", actions.join(", ")),
    )
}

/// Lists every mask library entry and layer with its status (G-03). Read-only:
/// the sidecar is never written.
fn mask_list(args: &MaskArgs, document: &SidecarDocument) -> Result<(), CliError> {
    let copies: Vec<&lumina_sidecar::VirtualCopy> = match args.virtual_copy.as_deref() {
        Some(id) => document
            .virtual_copies
            .iter()
            .filter(|copy| copy.id == id)
            .collect(),
        None => document.virtual_copies.iter().collect(),
    };
    if args.virtual_copy.is_some() && copies.is_empty() {
        return Err(CliError::Message("unknown virtual copy".into()));
    }
    if args.json {
        let copies_json = copies
            .iter()
            .map(|copy| {
                serde_json::json!({
                    "id": copy.id,
                    "name": copy.name,
                    "masks": copy.mask_library.iter().map(|mask| serde_json::json!({
                        "id": mask.id,
                        "name": mask.name,
                        "operation": format!("{:?}", mask.operation).to_lowercase(),
                        "status": format!("{:?}", mask.status).to_lowercase(),
                        "ai_select": mask.ai_select.as_ref().map(|select| serde_json::json!({
                            "kind": select.kind.as_str(),
                            "detail": select.detail,
                        })),
                        "prompt": mask.prompt.as_ref().map(prompt_kind),
                    })).collect::<Vec<_>>(),
                    "layers": copy.mask_layers.iter().map(|layer| serde_json::json!({
                        "id": layer.id,
                        "mask": format!("{}/{}", layer.mask.copy_id, layer.mask.mask_id),
                        "visible": layer.visible,
                        "local_adjustments": layer.local_adjustments,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        emit(
            true,
            serde_json::json!({"command":"mask", "input":args.input, "copies":copies_json, "status":"ok"}),
            "mask status listed",
        )
    } else {
        for copy in &copies {
            println!("copy: {} [{}]", copy.name, copy.id);
            for mask in &copy.mask_library {
                println!(
                    "  mask: {} [{}] op={} status={}{}{}",
                    mask.name,
                    mask.id,
                    format!("{:?}", mask.operation).to_lowercase(),
                    format!("{:?}", mask.status).to_lowercase(),
                    mask.ai_select.as_ref().map_or(String::new(), |select| {
                        format!(
                            " ai={}{}",
                            select.kind.as_str(),
                            select
                                .detail
                                .as_deref()
                                .map_or(String::new(), |d| format!(":{d}"))
                        )
                    }),
                    mask.prompt
                        .as_ref()
                        .map_or(String::new(), |p| format!(" prompt={}", prompt_kind(p))),
                );
            }
            for layer in &copy.mask_layers {
                println!(
                    "  layer: {} -> {}/{} visible={} local={}",
                    layer.id,
                    layer.mask.copy_id,
                    layer.mask.mask_id,
                    layer.visible,
                    layer
                        .local_adjustments
                        .as_ref()
                        .map(|adjustments| adjustments.to_string())
                        .unwrap_or_else(|| "none".into())
                );
            }
        }
        emit(
            false,
            serde_json::json!({"command":"mask", "input":args.input, "status":"ok"}),
            "mask status listed",
        )
    }
}

fn prompt_kind(prompt: &MaskPrompt) -> &'static str {
    match prompt {
        MaskPrompt::Box { .. } => "box",
        MaskPrompt::Brush { .. } => "brush",
        MaskPrompt::Polygon { .. } => "polygon",
        MaskPrompt::Ellipse { .. } => "ellipse",
        MaskPrompt::Gradient { .. } => "gradient",
        MaskPrompt::ColorRange { .. } => "color-range",
        MaskPrompt::LuminanceRange { .. } => "luminance-range",
    }
}

fn unix_now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Parses a mask reference as `mask-id` (same copy) or `copy-id/mask-id`.
/// Anything else is a loud error — never a guess.
fn parse_mask_ref(value: &str, default_copy: &str) -> Result<(String, String), CliError> {
    let parts: Vec<&str> = value.split('/').collect();
    match parts.as_slice() {
        [mask] if !mask.trim().is_empty() => Ok((default_copy.into(), (*mask).into())),
        [copy, mask] if !copy.trim().is_empty() && !mask.trim().is_empty() => {
            Ok(((*copy).into(), (*mask).into()))
        }
        _ => Err(CliError::Message(format!(
            "invalid mask reference `{value}`: expected `mask-id` or `copy-id/mask-id`"
        ))),
    }
}

fn stable_mask_id(parts: &[&str]) -> String {
    let joined = parts.join("\0");
    format!("mask-{}", blake3::hash(joined.as_bytes()).to_hex())
}

fn mask_library_contains(document: &SidecarDocument, copy_id: &str, mask_id: &str) -> bool {
    document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .is_some_and(|copy| copy.mask_library.iter().any(|mask| mask.id == mask_id))
}

fn new_source_mask(
    document: &SidecarDocument,
    id: &str,
    name: &str,
    frame_dims: Option<(u32, u32)>,
    status: MaskStatus,
) -> MaskDefinition {
    let (width, height) = frame_dims.unwrap_or((0, 0));
    MaskDefinition {
        id: id.into(),
        name: name.into(),
        source_fingerprint: SourceFingerprint {
            content_hash: document.source.content_hash.clone(),
            byte_length: document.source.byte_length,
            extras: BTreeMap::new(),
        },
        decode_context: DecodeFingerprint {
            decoder: "cli-mask".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        geometry_context: GeometryFingerprint {
            width,
            height,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        model: ModelIdentity {
            name: "unavailable".into(),
            version: "pending".into(),
            hash: "pending".into(),
            extras: BTreeMap::new(),
        },
        inference_resolution: Resolution {
            width,
            height,
            extras: BTreeMap::new(),
        },
        preprocessing: Preprocessing {
            name: "pending".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status,
        created_at: unix_now(),
        generator_version: env!("CARGO_PKG_VERSION").into(),
        error_text: None,
        artifact: None,
        operation: MaskOperation::Source,
        references: vec![],
        prompt: None,
        ai_select: None,
        extras: BTreeMap::new(),
    }
}

fn mask_add_ai(
    document: &mut SidecarDocument,
    copy_id: &str,
    kind: &str,
    name: &str,
    detail: Option<&str>,
    frame_dims: Option<(u32, u32)>,
) -> Result<(), CliError> {
    let kind = AiSelectKind::parse(kind).ok_or_else(|| {
        CliError::Message(format!(
            "unknown ai-select kind `{kind}`: expected subject|sky|background|objects|people"
        ))
    })?;
    if let Some(detail) = detail {
        if detail.len() > 64
            || detail.trim().is_empty()
            || detail != detail.trim()
            || detail.chars().any(|c| c.is_control())
        {
            return Err(CliError::Message(
                "detail must be trimmed, non-empty, free of control characters and at most 64 chars".into(),
            ));
        }
    }
    let id = stable_mask_id(&["ai", kind.as_str(), name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let mut mask = new_source_mask(document, &id, name, frame_dims, MaskStatus::Pending);
    mask.ai_select = Some(AiSelect {
        kind,
        detail: detail.map(str::to_string),
        extras: BTreeMap::new(),
    });
    mask_copy_mut(document, copy_id)?.mask_library.push(mask);
    Ok(())
}

fn mask_add_luminance(
    document: &mut SidecarDocument,
    copy_id: &str,
    name: &str,
    min: f32,
    max: f32,
    feather: f32,
    frame_dims: Option<(u32, u32)>,
) -> Result<(), CliError> {
    let id = stable_mask_id(&["luminance-range", name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let mut mask = new_source_mask(document, &id, name, frame_dims, MaskStatus::Valid);
    mask.prompt = Some(MaskPrompt::LuminanceRange {
        min,
        max,
        feather,
        transformation: PromptTransform::default(),
    });
    mask_copy_mut(document, copy_id)?.mask_library.push(mask);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn mask_add_color(
    document: &mut SidecarDocument,
    copy_id: &str,
    name: &str,
    hue_center: Option<f32>,
    hue_width: Option<f32>,
    sat_min: Option<f32>,
    sat_max: Option<f32>,
    lum_min: Option<f32>,
    lum_max: Option<f32>,
    feather: f32,
    frame_dims: Option<(u32, u32)>,
) -> Result<(), CliError> {
    let (hue_center, hue_width, sat_min, sat_max, lum_min, lum_max) = (
        hue_center
            .ok_or_else(|| CliError::Message("--add-color-range requires --hue-center".into()))?,
        hue_width
            .ok_or_else(|| CliError::Message("--add-color-range requires --hue-width".into()))?,
        sat_min.ok_or_else(|| CliError::Message("--add-color-range requires --sat-min".into()))?,
        sat_max.ok_or_else(|| CliError::Message("--add-color-range requires --sat-max".into()))?,
        lum_min.ok_or_else(|| CliError::Message("--add-color-range requires --lum-min".into()))?,
        lum_max.ok_or_else(|| CliError::Message("--add-color-range requires --lum-max".into()))?,
    );
    let id = stable_mask_id(&["color-range", name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let mut mask = new_source_mask(document, &id, name, frame_dims, MaskStatus::Valid);
    mask.prompt = Some(MaskPrompt::ColorRange {
        hue_center,
        hue_width,
        sat_min,
        sat_max,
        lum_min,
        lum_max,
        feather,
        transformation: PromptTransform::default(),
    });
    mask_copy_mut(document, copy_id)?.mask_library.push(mask);
    Ok(())
}

fn mask_combine(
    document: &mut SidecarDocument,
    copy_id: &str,
    op: &str,
    name: &str,
    inputs: &str,
    frame_dims: Option<(u32, u32)>,
) -> Result<(), CliError> {
    let operation = match op.trim().to_ascii_lowercase().as_str() {
        "union" | "add" => MaskOperation::Union,
        "intersect" => MaskOperation::Intersect,
        "subtract" | "sub" => MaskOperation::Subtract,
        "invert" | "inv" => MaskOperation::Invert,
        _ => {
            return Err(CliError::Message(format!(
                "unknown combine op `{op}`: expected union|intersect|subtract|invert"
            )));
        }
    };
    let refs: Vec<(String, String)> = inputs
        .split(',')
        .map(|part| parse_mask_ref(part.trim(), copy_id))
        .collect::<Result<_, _>>()?;
    let arity_ok = match operation {
        MaskOperation::Invert => refs.len() == 1,
        MaskOperation::Subtract => refs.len() == 2,
        MaskOperation::Union | MaskOperation::Intersect => refs.len() >= 2,
        MaskOperation::Source => false,
    };
    if !arity_ok {
        return Err(CliError::Message(format!(
            "combine op `{op}` needs {} input(s), got {}",
            match operation {
                MaskOperation::Invert => "exactly 1",
                MaskOperation::Subtract => "exactly 2",
                _ => "at least 2",
            },
            refs.len()
        )));
    }
    for (ref_copy, ref_mask) in &refs {
        if !mask_library_contains(document, ref_copy, ref_mask) {
            return Err(CliError::Message(format!(
                "combine input references unknown mask `{ref_copy}/{ref_mask}`"
            )));
        }
    }
    // Canonical op key (not the raw alias): `union` and `add` name the same
    // node, so re-running with either spelling hits the loud duplicate.
    let op_key = match operation {
        MaskOperation::Union => "union",
        MaskOperation::Intersect => "intersect",
        MaskOperation::Subtract => "subtract",
        MaskOperation::Invert => "invert",
        MaskOperation::Source => "source",
    };
    let id = stable_mask_id(&["combine", op_key, name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let references = refs
        .into_iter()
        .map(|(ref_copy, ref_mask)| MaskReference {
            copy_id: ref_copy,
            mask_id: ref_mask,
            extras: BTreeMap::new(),
        })
        .collect();
    let mut mask = new_source_mask(document, &id, name, frame_dims, MaskStatus::Pending);
    mask.operation = operation;
    mask.references = references;
    // A derived node inherits usability only through the loader blessing pass;
    // `Pending` keeps it honest until every input resolves.
    mask_copy_mut(document, copy_id)?.mask_library.push(mask);
    Ok(())
}

fn mask_duplicate(
    document: &mut SidecarDocument,
    copy_id: &str,
    source: &str,
    name: &str,
) -> Result<(), CliError> {
    let (ref_copy, ref_mask) = parse_mask_ref(source, copy_id)?;
    let template = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == ref_copy)
        .and_then(|copy| copy.mask_library.iter().find(|mask| mask.id == ref_mask))
        .cloned()
        .ok_or_else(|| CliError::Message(format!("unknown mask `{ref_copy}/{ref_mask}`")))?;
    // Only source payloads duplicate cleanly; a derived node is rebuilt with
    // `--combine` against the same inputs instead of aliasing them silently.
    if template.operation != MaskOperation::Source {
        return Err(CliError::Message(format!(
            "cannot duplicate derived mask `{ref_copy}/{ref_mask}`; use --combine to rebuild it"
        )));
    }
    let id = stable_mask_id(&["duplicate", &ref_copy, &ref_mask, name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let mut duplicated = template;
    duplicated.id = id;
    duplicated.name = name.into();
    duplicated.created_at = unix_now();
    duplicated.generator_version = env!("CARGO_PKG_VERSION").into();
    mask_copy_mut(document, copy_id)?
        .mask_library
        .push(duplicated);
    Ok(())
}

fn mask_attach_layer(
    document: &mut SidecarDocument,
    copy_id: &str,
    target: &str,
) -> Result<(), CliError> {
    let (ref_copy, ref_mask) = parse_mask_ref(target, copy_id)?;
    if !mask_library_contains(document, &ref_copy, &ref_mask) {
        return Err(CliError::Message(format!(
            "cannot attach unknown mask `{ref_copy}/{ref_mask}`"
        )));
    }
    let layer_id = format!("layer-{ref_mask}");
    let copy = mask_copy_mut(document, copy_id)?;
    if copy.mask_layers.iter().any(|layer| layer.id == layer_id) {
        return Err(CliError::Message(format!(
            "copy `{copy_id}` already has layer `{layer_id}`"
        )));
    }
    copy.mask_layers.push(MaskLayer {
        id: layer_id,
        mask: MaskReference {
            copy_id: ref_copy,
            mask_id: ref_mask,
            extras: BTreeMap::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        visible: true,
        local_adjustments: None,
        extras: BTreeMap::new(),
    });
    Ok(())
}

fn mask_set_layer_visible(
    document: &mut SidecarDocument,
    copy_id: &str,
    layer_id: &str,
    visible: bool,
) -> Result<(), CliError> {
    let layer = mask_copy_mut(document, copy_id)?
        .mask_layers
        .iter_mut()
        .find(|layer| layer.id == layer_id)
        .ok_or_else(|| {
            CliError::Message(format!("unknown layer `{layer_id}` on copy `{copy_id}`"))
        })?;
    layer.visible = visible;
    Ok(())
}
