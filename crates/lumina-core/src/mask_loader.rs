//! Intelligent mask-loading decision layer (F-048 / F-051).
//!
//! This module sits **above** the raw `.lumina.zdata` tile loader. Given the
//! recipe's mask definitions, the already-loaded zdata planes, the current
//! source identity and an optional re-inference backend, it decides for every
//! reachable source mask whether to:
//!
//! * **use a confirmably valid persisted artifact** (preferred — no inference);
//! * **re-infer via the ONNX adapter** because the artifact is missing,
//!   outdated (source changed, model changed) or a refresh was requested;
//! * **fall back to a cached (possibly stale) artifact** when the model is
//!   unavailable (F-051: render still succeeds, but a warning is surfaced).
//!
//! Additionally (REVIEW-SIDECAR-LOADER-RES), every decoded persisted plane is
//! checked against the resolution declared in its own artifact record
//! ([`lumina_sidecar::ArtifactReference`]). `artifact_status` deliberately
//! does not validate reference width/height against bundle records (see
//! `feature/architecture/sidecar.md`, „Artefaktstatus-Prüfung"); this loading
//! path can do so soundly because it knows the mask identity and holds the
//! fully decoded plane. A plane whose dimensions contradict its own record is
//! corrupt: it is reported as [`MaskStatus::Corrupt`] in the returned copies
//! and routed through re-inference / cache-with-warning / hard error — never
//! silently resampled and never served as confirmably valid.
//!
//! When neither a cached artifact nor a model is available, it reports a clear
//! error instead of silently serving a stale or empty mask (F-051).
//!
//! `lumina-core` never couples to a concrete model: the re-inference path is
//! injected through the platform-neutral [`MaskInference`] trait, which the
//! native `lumina-onnx` adapter implements.

use crate::masks::MaskPlane;
use crate::{CoreError, ImageFrame, MaskPolicy};
use lumina_sidecar::{
    DecodeFingerprint, MaskDefinition, MaskOperation, MaskReference, MaskStatus, ModelIdentity,
    VirtualCopy,
};
use std::collections::{BTreeMap, BTreeSet};

/// Key under which the ONNX adapter (`lumina-onnx`) persists a deterministic
/// SHA-256 digest over a model's input spec ([`ModelInputSpec`], i.e.
/// inference resolution, channel layout, tensor name/format and input
/// normalization) inside [`ModelIdentity::extras`].
///
/// This is a **cross-crate contract** with `lumina_onnx::manifest::
/// INPUT_SPEC_DIGEST_KEY`; the two string literals MUST stay identical.
/// `lumina-core` intentionally does not depend on `lumina-onnx` (platform-
/// neutral domain crate, see `Agents.md` architecture boundaries), so the key
/// is mirrored here rather than imported. ai-masks.md lists
/// "Vorverarbeitung und Inferenzauflösung" explicitly as mask-identity
/// components; the digest lets the decision layer detect a changed inference
/// contract even when name/version/hash are untouched (R2-ONNX-01).
const INPUT_SPEC_DIGEST_KEY: &str = "input_spec_digest";

/// Re-inference surface for a subject mask.
///
/// Implemented by the ONNX adapter (`lumina-onnx`); `lumina-core` is never
/// coupled to a concrete model or native dependency. The decision layer calls
/// [`MaskInference::infer`] only when re-inference is actually required.
pub trait MaskInference {
    /// Whether the model artifact/weights required for inference are present.
    /// A `false` result means re-inference is impossible, so the decision layer
    /// must fall back to a cached artifact (F-051) or fail.
    fn is_available(&self) -> bool;
    /// Re-infer a subject matte from `frame`. Must never silently fall back on
    /// a missing or mismatched artifact — it returns an error instead.
    fn infer(&self, frame: &ImageFrame) -> Result<MaskPlane, CoreError>;
}

/// How a source mask was resolved by the decision layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskResolvedFrom {
    /// A confirmably valid persisted artifact was loaded and used (no inference).
    LoadedPersisted,
    /// The model was available and the matte was (re-)inferred.
    ReInferred,
    /// No model was available; a cached (possibly stale) artifact was used.
    CachedUnavailable,
}

/// Per-mask resolution outcome for diagnostics / surfacing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskLoadOutcome {
    pub copy_id: String,
    pub mask_id: String,
    pub from: MaskResolvedFrom,
}

/// Inputs to the mask-loading decision layer.
#[derive(Clone)]
pub struct MaskLoadContext<'a> {
    /// All virtual copies (the decision layer resolves cross-copy references).
    pub copies: &'a [VirtualCopy],
    /// The active virtual copy whose `mask_layers` drive reachability.
    pub active_copy_id: &'a str,
    /// Current source content hash (BLAKE3 `blake3:…` form).
    pub source_hash: &'a str,
    /// Current decode fingerprint of the source.
    pub decode_context: &'a DecodeFingerprint,
    /// Planes already loaded from the `.lumina.zdata` tile container, keyed by
    /// `(copy_id, mask_id)`. The decision layer reads from here; it never
    /// touches the filesystem.
    pub loaded_planes: BTreeMap<(String, String), MaskPlane>,
    /// Optional re-inference backend. `None` means no model is wired at all
    /// (equivalent to "unavailable").
    pub inference: Option<&'a dyn MaskInference>,
    /// Identity of the configured model (only set when `inference` is `Some`).
    /// Used to detect a model change against the persisted mask identity.
    pub model_identity: Option<&'a ModelIdentity>,
    /// Force re-inference even when a confirmably valid persisted mask exists.
    pub refresh: bool,
    /// How the subsequent render stage treats unresolved layers. The
    /// decision layer itself always fails hard when no model **and** no cache
    /// exist (F-051), independent of this policy.
    pub policy: MaskPolicy,
}

/// Result of the mask-loading decision layer.
#[derive(Debug, Clone)]
pub struct MaskLoadResult {
    /// Planes keyed by `(copy_id, mask_id)` for every resolved source mask.
    pub planes: BTreeMap<(String, String), MaskPlane>,
    /// Copies with statuses updated: every *resolved* (usable) mask is marked
    /// `Valid`; unreachable or unresolved masks keep their previous status.
    pub copies: Vec<VirtualCopy>,
    /// Human-readable warnings (model-unavailable / cached-fallback, etc.).
    pub warnings: Vec<String>,
    /// `true` if at least one mask required the model but it was unavailable.
    pub model_unavailable: bool,
    /// Per-mask resolution outcomes (for diagnostics / surfacing).
    pub outcomes: Vec<MaskLoadOutcome>,
}

/// Run the mask-loading decision layer (F-048 / F-051).
///
/// For every source mask reachable from the active copy's `mask_layers`, the
/// function decides whether to load the persisted plane, re-infer via the model,
/// or fall back to a cached (stale) plane. The returned [`MaskLoadResult`] is
/// consumed directly by `render_frame` via [`crate::MaskContext`].
///
/// `frame` is only used if re-inference is required.
pub fn resolve_mask_planes(
    ctx: MaskLoadContext<'_>,
    frame: &ImageFrame,
) -> Result<MaskLoadResult, CoreError> {
    let reachable = reachable_definitions(ctx.copies, ctx.active_copy_id);
    let mut planes: BTreeMap<(String, String), MaskPlane> = BTreeMap::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut model_unavailable = false;
    let mut outcomes: Vec<MaskLoadOutcome> = Vec::new();
    let mut resolved_sources: BTreeSet<(String, String)> = BTreeSet::new();
    // REVIEW-SIDECAR-LOADER-RES: keys whose decoded plane contradicts the
    // resolution declared in its own artifact record (see below). Successful
    // re-inference removes a key again (the fresh matte replaces the corrupt
    // artifact); everything left in the set is surfaced as `Corrupt`.
    let mut dimension_mismatched: BTreeSet<(String, String)> = BTreeSet::new();

    for key in &reachable {
        let Some(definition) = find_definition(ctx.copies, &reference_from_key(key)) else {
            continue;
        };
        // Only source masks carry their own plane; derived (union/invert/…)
        // definitions are resolved in the blessing pass below.
        if definition.operation != MaskOperation::Source {
            continue;
        }

        let persisted = ctx.loaded_planes.get(key).cloned();
        let artifact_present = persisted.is_some() && definition.artifact.is_some();
        // REVIEW-SIDECAR-LOADER-RES: compare the *decoded* plane dimensions
        // with the reference resolution declared by the artifact record. The
        // zdata loader hands planes through unscaled, so any difference means
        // payload and declaration disagree → the artifact is corrupt. Without
        // an artifact record there is nothing to compare against and the
        // pre-existing behavior applies unchanged.
        let dimension_mismatch = persisted
            .as_ref()
            .zip(definition.artifact.as_ref())
            .is_some_and(|(plane, artifact)| {
                plane.width != artifact.width || plane.height != artifact.height
            });
        if dimension_mismatch {
            dimension_mismatched.insert(key.clone());
        }
        let source_ok = definition.source_fingerprint.content_hash == ctx.source_hash;
        let decode_ok = decode_context_matches(&definition.decode_context, ctx.decode_context);
        let identity_ok = model_identity_matches(&definition.model, ctx.model_identity);
        let valid = definition.status == MaskStatus::Valid
            && artifact_present
            && !dimension_mismatch
            && source_ok
            && decode_ok
            && identity_ok;

        if valid && !ctx.refresh {
            // F-048 (1)(2): a confirmably valid persisted mask is preferred.
            planes.insert(key.clone(), persisted.unwrap());
            resolved_sources.insert(key.clone());
            outcomes.push(MaskLoadOutcome {
                copy_id: key.0.clone(),
                mask_id: key.1.clone(),
                from: MaskResolvedFrom::LoadedPersisted,
            });
            continue;
        }

        // The persisted artifact is missing, outdated, corrupt (dimension
        // mismatch) or a refresh was requested. Try re-inference first.
        let model_present = ctx
            .inference
            .map(|backend| backend.is_available())
            .unwrap_or(false);
        if model_present {
            // F-048 (3): re-infer via the ONNX adapter (covers refresh, stale
            // source, changed model, and missing persisted artifact).
            let plane =
                ctx.inference
                    .unwrap()
                    .infer(frame)
                    .map_err(|error| CoreError::MaskInference {
                        reason: format!(
                            "re-inference of mask `{}/{}` failed: {}",
                            key.0, key.1, error
                        ),
                    })?;
            planes.insert(key.clone(), plane);
            resolved_sources.insert(key.clone());
            // The freshly inferred matte replaces the corrupt artifact; the
            // layer is resolved again and is not left marked `Corrupt`.
            dimension_mismatched.remove(key);
            outcomes.push(MaskLoadOutcome {
                copy_id: key.0.clone(),
                mask_id: key.1.clone(),
                from: MaskResolvedFrom::ReInferred,
            });
            continue;
        }

        // F-051: the model is unavailable (no backend wired, or its weights are
        // missing). Fall back to a cached artifact if one exists.
        model_unavailable = true;
        if let Some(plane) = persisted {
            // F-051 (1): use the cached (possibly stale) artifact; mark it used.
            // REVIEW-SIDECAR-LOADER-RES: when the plane contradicts its own
            // artifact record, say so explicitly instead of only hinting at
            // staleness — the cause must stay visible (kein stiller Fallback).
            if let Some(artifact) = definition.artifact.as_ref() {
                if plane.width != artifact.width || plane.height != artifact.height {
                    warnings.push(format!(
                        "mask `{}/{}` artifact dimensions {}x{} contradict the declared {}x{}; \
                         marked Corrupt (no silent resample)",
                        key.0, key.1, plane.width, plane.height, artifact.width, artifact.height
                    ));
                }
            }
            planes.insert(key.clone(), plane);
            resolved_sources.insert(key.clone());
            outcomes.push(MaskLoadOutcome {
                copy_id: key.0.clone(),
                mask_id: key.1.clone(),
                from: MaskResolvedFrom::CachedUnavailable,
            });
            warnings.push(format!(
                "mask `{}/{}` used from cache because the inference model is unavailable \
                 (persisted status {:?}); the result may be stale",
                key.0, key.1, definition.status
            ));
            continue;
        }

        // F-051 (2): no cached artifact and no model — a clear hard error,
        // never a silent fallback or an empty mask.
        return Err(CoreError::MaskUnavailable {
            copy_id: key.0.clone(),
            mask_id: key.1.clone(),
            status: "model-unavailable".into(),
        });
    }

    // Blessing pass: a derived definition (union/invert/…) is usable only when
    // every source mask in its dependency closure was resolved.
    let mut blessed: BTreeSet<(String, String)> = resolved_sources.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for key in &reachable {
            if blessed.contains(key) {
                continue;
            }
            let Some(definition) = find_definition(ctx.copies, &reference_from_key(key)) else {
                continue;
            };
            if definition.references.is_empty() {
                continue;
            }
            let all_sources_blessed = definition.references.iter().all(|reference| {
                blessed.contains(&(reference.copy_id.clone(), reference.mask_id.clone()))
            });
            if all_sources_blessed {
                blessed.insert(key.clone());
                changed = true;
            }
        }
    }

    // Clone the copies and mark every reachable, blessed mask as `Valid` so the
    // downstream `render_frame` evaluation accepts it. The original sidecar is
    // never mutated by this in-memory rewrite.
    let mut copies: Vec<VirtualCopy> = ctx.copies.to_vec();
    for copy in &mut copies {
        for mask in &mut copy.mask_library {
            let key = (copy.id.clone(), mask.id.clone());
            // REVIEW-SIDECAR-LOADER-RES: a plane whose dimensions contradict
            // its own artifact record is corrupt. It wins over the blessing
            // pass so a dimension-mismatched artifact that had to be served
            // from cache (F-051) is never reported as confirmably `Valid`.
            if dimension_mismatched.contains(&key) {
                mask.status = MaskStatus::Corrupt;
            } else if reachable.contains(&key) && blessed.contains(&key) {
                mask.status = MaskStatus::Valid;
            }
        }
    }

    Ok(MaskLoadResult {
        planes,
        copies,
        warnings,
        model_unavailable,
        outcomes,
    })
}

/// All `(copy_id, mask_id)` definitions reachable from the active copy's
/// `mask_layers`, following `references` recursively. The result is a DAG
/// closure (the sidecar guarantees a cycle-free graph).
fn reachable_definitions(
    copies: &[VirtualCopy],
    active_copy_id: &str,
) -> BTreeSet<(String, String)> {
    let Some(active) = copies.iter().find(|copy| copy.id == active_copy_id) else {
        return BTreeSet::new();
    };
    let mut work: Vec<MaskReference> = active
        .mask_layers
        .iter()
        .map(|layer| layer.mask.clone())
        .collect();
    let mut reachable = BTreeSet::new();
    let mut seen = BTreeSet::new();
    while let Some(reference) = work.pop() {
        let key = (reference.copy_id.clone(), reference.mask_id.clone());
        if !seen.insert(key.clone()) {
            continue;
        }
        if find_definition(copies, &reference).is_some() {
            reachable.insert(key.clone());
            if let Some(definition) = find_definition(copies, &reference) {
                for reference in &definition.references {
                    work.push(reference.clone());
                }
            }
        }
    }
    reachable
}

/// Finds the `MaskDefinition` referenced by `reference` across all copies
/// (mirrors `MaskGraph`'s `(copy_id, mask_id)` keying).
fn find_definition<'a>(
    copies: &'a [VirtualCopy],
    reference: &MaskReference,
) -> Option<&'a MaskDefinition> {
    copies
        .iter()
        .find(|copy| copy.id == reference.copy_id)
        .and_then(|copy| {
            copy.mask_library
                .iter()
                .find(|mask| mask.id == reference.mask_id)
        })
}

fn reference_from_key(key: &(String, String)) -> MaskReference {
    MaskReference {
        copy_id: key.0.clone(),
        mask_id: key.1.clone(),
        extras: Default::default(),
    }
}

fn decode_context_matches(actual: &DecodeFingerprint, expected: &DecodeFingerprint) -> bool {
    actual.decoder == expected.decoder
        && actual.version == expected.version
        && actual.parameters == expected.parameters
}

fn model_identity_matches(model: &ModelIdentity, configured: Option<&ModelIdentity>) -> bool {
    match configured {
        Some(configured) => {
            // Legacy identity: every persisted mask identity carries at least
            // name/version/hash, and they must all agree.
            if !(model.name == configured.name
                && model.version == configured.version
                && model.hash == configured.hash)
            {
                return false;
            }
            // R2-ONNX-01: the persisted mask identity may additionally carry a
            // deterministic digest over the model's input spec
            // (`INPUT_SPEC_DIGEST_KEY` in `extras`; see ai-masks.md
            // "Vorverarbeitung und Inferenzauflösung"). When the inference
            // contract changes (resolution, normalization, layout, tensor
            // name/format) the digest flips even though name/version/hash are
            // identical — without this check a stale cached mask would silently
            // stay `valid`.
            //
            // Rule (matches the producer's additive-optional contract):
            // * both present  → must match;
            // * only the persisted mask carries it → the configured context is
            //   older/foreign (incomparable) → treat as changed;
            // * only the configured context carries it → the persisted mask
            //   predates the digest feature (or is synthetic); fall back to the
            //   legacy name/version/hash behaviour already checked above so old
            //   sidecars and test fixtures stay valid;
            // * neither present → keep the legacy name/version/hash behaviour.
            match (
                model.extras.get(INPUT_SPEC_DIGEST_KEY),
                configured.extras.get(INPUT_SPEC_DIGEST_KEY),
            ) {
                (Some(a), Some(b)) => a == b,
                (Some(_), None) => false,
                (None, Some(_)) | (None, None) => true,
            }
        }
        // REVIEW-MASK-N3: without a configured expected model identity the
        // persisted artifact's model context CANNOT be confirmed. Per F-048
        // ("kann Gültigkeit nicht bestätigt werden → gilt als fehlend") and
        // the sidecar mask rules (Modellname/-version/-hash gehören zur
        // Gültigkeit), an unconfirmable artifact is treated as stale, not as
        // valid — a mask produced by a foreign/unknown model must never be
        // served silently as confirmably valid (`LoadedPersisted`). The
        // decision layer's F-051 path then either re-infers (model present),
        // serves it from cache WITH a stale warning, or fails hard.
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::masks::MaskPlane;
    use lumina_sidecar::{
        ArtifactReference, CoordinateSystem, EditRecipe, Extras, GeometryFingerprint,
        Preprocessing, Resolution, SourceFingerprint,
    };
    use std::collections::BTreeMap;

    /// Deterministic test backend. `infer` always returns a uniform plane of
    /// `value`, which lets a test distinguish a re-inferred plane from a
    /// persisted one (a different `value`).
    struct FakeInference {
        available: bool,
        value: u16,
    }

    impl MaskInference for FakeInference {
        fn is_available(&self) -> bool {
            self.available
        }
        fn infer(&self, _frame: &ImageFrame) -> Result<MaskPlane, CoreError> {
            MaskPlane::new(4, 4, vec![self.value; 16]).map_err(|error| CoreError::MaskInference {
                reason: error.to_string(),
            })
        }
    }

    /// Backend whose `infer` always fails, to assert that a failed re-inference
    /// is surfaced as a hard error and never silently falls back to a cached
    /// (stale) plane (F-051 / F-050: "keine stillen Fallbacks").
    struct FailingInference {
        available: bool,
    }

    impl MaskInference for FailingInference {
        fn is_available(&self) -> bool {
            self.available
        }
        fn infer(&self, _frame: &ImageFrame) -> Result<MaskPlane, CoreError> {
            Err(CoreError::MaskInference {
                reason: "simulated re-inference failure".into(),
            })
        }
    }

    /// Like [`resolve_one`] but lets the caller specify the *context's* decode
    /// fingerprint independently of the definition's, so decode-context-change
    /// scenarios can be exercised.
    #[allow(clippy::too_many_arguments)]
    fn resolve_single(
        definition: MaskDefinition,
        loaded_plane: Option<u16>,
        inference: Option<&dyn MaskInference>,
        model_identity: Option<ModelIdentity>,
        refresh: bool,
        source_hash: &str,
        ctx_decode: DecodeFingerprint,
    ) -> Result<MaskLoadResult, CoreError> {
        let mut copy = copy_with("vc", vec![definition]);
        copy.mask_layers = vec![layer_for("vc", "subject")];
        let loaded_planes = loaded_plane
            .map(|value| {
                BTreeMap::from([(
                    ("vc".into(), "subject".into()),
                    MaskPlane::new(4, 4, vec![value; 16]).unwrap(),
                )])
            })
            .unwrap_or_default();
        let model_ref = model_identity.as_ref();
        resolve_mask_planes(
            MaskLoadContext {
                copies: &[copy],
                active_copy_id: "vc",
                source_hash,
                decode_context: &ctx_decode,
                loaded_planes,
                inference,
                model_identity: model_ref,
                refresh,
                policy: MaskPolicy::Warn,
            },
            &frame(),
        )
    }

    /// REVIEW-SIDECAR-LOADER-RES: like [`resolve_one`] but takes an explicit
    /// [`MaskPlane`], so a test controls the *decoded* plane dimensions
    /// independently of the artifact record (always declared 4x4).
    fn resolve_with_plane(
        definition: MaskDefinition,
        plane: Option<MaskPlane>,
        inference: Option<&dyn MaskInference>,
        model_identity: Option<ModelIdentity>,
    ) -> Result<MaskLoadResult, CoreError> {
        let mut copy = copy_with("vc", vec![definition]);
        copy.mask_layers = vec![layer_for("vc", "subject")];
        let loaded_planes = plane
            .map(|value| BTreeMap::from([(("vc".into(), "subject".into()), value)]))
            .unwrap_or_default();
        let model_ref = model_identity.as_ref();
        resolve_mask_planes(
            MaskLoadContext {
                copies: &[copy],
                active_copy_id: "vc",
                source_hash: "src",
                decode_context: &decode_context(),
                loaded_planes,
                inference,
                model_identity: model_ref,
                refresh: false,
                policy: MaskPolicy::Warn,
            },
            &frame(),
        )
    }

    /// Status of the single `vc`/`subject` mask in the result copies.
    fn persisted_status(result: &MaskLoadResult) -> MaskStatus {
        result
            .copies
            .iter()
            .find(|copy| copy.id == "vc")
            .and_then(|copy| copy.mask_library.iter().find(|mask| mask.id == "subject"))
            .map(|mask| mask.status.clone())
            .unwrap()
    }

    const PERSISTED_VALUE: u16 = 32768;
    const INFERRED_VALUE: u16 = 12345;

    fn decode_context() -> DecodeFingerprint {
        DecodeFingerprint {
            decoder: "image".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        }
    }

    fn model_identity() -> ModelIdentity {
        ModelIdentity {
            name: "BiRefNet".into(),
            version: "1.0.0".into(),
            hash: "h".into(),
            extras: Extras::new(),
        }
    }

    fn source_definition(
        _copy_id: &str,
        id: &str,
        status: MaskStatus,
        source_hash: &str,
        model: ModelIdentity,
        decode: DecodeFingerprint,
        with_artifact: bool,
    ) -> MaskDefinition {
        MaskDefinition {
            id: id.into(),
            name: id.into(),
            source_fingerprint: SourceFingerprint {
                content_hash: source_hash.into(),
                byte_length: 1,
                extras: Extras::new(),
            },
            decode_context: decode,
            geometry_context: GeometryFingerprint {
                width: 4,
                height: 4,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            model,
            inference_resolution: Resolution {
                width: 4,
                height: 4,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "p".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status,
            created_at: "now".into(),
            generator_version: "g".into(),
            error_text: None,
            artifact: with_artifact.then(|| ArtifactReference {
                relative_path: "x.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: "c".into(),
                width: 4,
                height: 4,
                channels: "u16".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            }),
            operation: MaskOperation::Source,
            references: vec![],
            prompt: None,
            extras: Extras::new(),
        }
    }

    fn copy_with(copy_id: &str, definitions: Vec<MaskDefinition>) -> VirtualCopy {
        VirtualCopy {
            id: copy_id.into(),
            name: copy_id.into(),
            is_default: copy_id == "vc",
            recipe: EditRecipe::default(),
            mask_library: definitions,
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        }
    }

    fn layer_for(copy_id: &str, mask_id: &str) -> lumina_sidecar::MaskLayer {
        use lumina_sidecar::MaskReference;
        lumina_sidecar::MaskLayer {
            id: format!("layer-{mask_id}"),
            mask: MaskReference {
                copy_id: copy_id.into(),
                mask_id: mask_id.into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
        }
    }

    fn frame() -> ImageFrame {
        ImageFrame::new(4, 4, vec![100; 64]).unwrap()
    }

    /// Build a single active copy `vc` whose only layer references `subject`.
    fn resolve_one(
        definition: MaskDefinition,
        loaded_plane: Option<u16>,
        inference: Option<&dyn MaskInference>,
        model_identity: Option<ModelIdentity>,
        refresh: bool,
        source_hash: &str,
    ) -> Result<MaskLoadResult, CoreError> {
        let mut copy = copy_with("vc", vec![definition]);
        copy.mask_layers = vec![layer_for("vc", "subject")];
        let loaded_planes = loaded_plane
            .map(|value| {
                BTreeMap::from([(
                    ("vc".into(), "subject".into()),
                    MaskPlane::new(4, 4, vec![value; 16]).unwrap(),
                )])
            })
            .unwrap_or_default();
        let model_ref = model_identity.as_ref();
        resolve_mask_planes(
            MaskLoadContext {
                copies: &[copy],
                active_copy_id: "vc",
                source_hash,
                decode_context: &decode_context(),
                loaded_planes,
                inference,
                model_identity: model_ref,
                refresh,
                policy: MaskPolicy::Warn,
            },
            &frame(),
        )
    }

    #[test]
    fn valid_persisted_mask_is_preferred_without_reinference() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let result = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(model_identity()),
            false,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes.len(), 1);
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::LoadedPersisted);
        assert_eq!(
            result
                .planes
                .get(&("vc".into(), "subject".into()))
                .unwrap()
                .values,
            vec![PERSISTED_VALUE; 16]
        );
        assert!(!result.model_unavailable);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn refresh_forces_reinference_even_when_valid() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let result = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(model_identity()),
            true,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
        assert_eq!(
            result
                .planes
                .get(&("vc".into(), "subject".into()))
                .unwrap()
                .values,
            vec![INFERRED_VALUE; 16]
        );
    }

    #[test]
    fn stale_mask_is_reinferred_when_model_available() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Stale,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let result = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(model_identity()),
            false,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    }

    #[test]
    fn changed_source_is_reinferred_when_model_available() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "OLD-SRC",
            model_identity(),
            decode_context(),
            true,
        );
        // Current source hash differs → persisted mask is not confirmable.
        let result = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(model_identity()),
            false,
            "NEW-SRC",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    }

    /// Build a [`ModelIdentity`] whose `extras` optionally carries the
    /// `INPUT_SPEC_DIGEST_KEY` digest — mirroring what the ONNX adapter's
    /// `to_model_identity` writes. `None` produces a legacy identity without
    /// the key (an old sidecar before R2-ONNX-01).
    fn identity_with_digest(digest: Option<&str>) -> ModelIdentity {
        let mut extras = Extras::new();
        if let Some(digest) = digest {
            extras.insert(
                INPUT_SPEC_DIGEST_KEY.to_owned(),
                serde_json::Value::String(digest.to_owned()),
            );
        }
        ModelIdentity {
            name: "BiRefNet".into(),
            version: "1.0.0".into(),
            hash: "h".into(),
            extras,
        }
    }

    // R2-ONNX-01 — the decision layer [`model_identity_matches`] must honor the
    // persisted input-spec digest in `extras` exactly as the producer writes
    // it, closing the stale-detection hole where a normalization/resolution
    // change kept a cached mask `valid` despite changed inference semantics.

    #[test]
    fn same_input_spec_digest_matches() {
        // Identical digests → the persisted mask stays confirmably valid.
        assert!(model_identity_matches(
            &identity_with_digest(Some("sha256:aaaa")),
            Some(&identity_with_digest(Some("sha256:aaaa"))),
        ));
    }

    #[test]
    fn changed_input_spec_digest_does_not_match() {
        // Only the inference contract (digest) differs; name/version/hash are
        // identical — this is exactly the stale hole R2-ONNX-01 closes.
        assert!(!model_identity_matches(
            &identity_with_digest(Some("sha256:aaaa")),
            Some(&identity_with_digest(Some("sha256:bbbb"))),
        ));
    }

    #[test]
    fn digest_present_on_one_side_only() {
        // Persisted mask carries a digest the configured context lacks → the
        // context is older/foreign (incomparable) → treat as changed.
        assert!(!model_identity_matches(
            &identity_with_digest(Some("sha256:aaaa")),
            Some(&identity_with_digest(None)),
        ));
        // The persisted mask predates the digest feature (or is synthetic) but
        // the configured context carries one → fall back to the legacy
        // name/version/hash comparison (already checked above) → still matches.
        assert!(model_identity_matches(
            &identity_with_digest(None),
            Some(&identity_with_digest(Some("sha256:aaaa"))),
        ));
    }

    #[test]
    fn legacy_identity_without_digest_still_matches() {
        // Old sidecars (no digest key) fall back to the legacy comparison.
        assert!(model_identity_matches(
            &identity_with_digest(None),
            Some(&identity_with_digest(None)),
        ));
        // ...and a legacy identity still fails when name/version/hash differ.
        assert!(!model_identity_matches(
            &ModelIdentity {
                name: "Other".into(),
                version: "1.0.0".into(),
                hash: "h".into(),
                extras: Extras::new(),
            },
            Some(&identity_with_digest(None)),
        ));
    }

    #[test]
    fn changed_input_spec_digest_triggers_reinference() {
        // End-to-end: a persisted mask whose digest differs from the currently
        // configured model must be re-inferred, never served as
        // `LoadedPersisted`.
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            identity_with_digest(Some("sha256:OLD")),
            decode_context(),
            true,
        );
        let result = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(identity_with_digest(Some("sha256:NEW"))),
            false,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    }

    #[test]
    fn same_input_spec_digest_prefers_persisted() {
        // End-to-end mirror of the above: identical digest keeps the persisted
        // plane (no re-inference).
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            identity_with_digest(Some("sha256:SAME")),
            decode_context(),
            true,
        );
        let result = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(identity_with_digest(Some("sha256:SAME"))),
            false,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::LoadedPersisted);
    }

    #[test]
    fn changed_model_is_reinferred_when_available() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            // Persisted for a different model identity.
            ModelIdentity {
                name: "OldModel".into(),
                version: "0.9".into(),
                hash: "old".into(),
                extras: Extras::new(),
            },
            decode_context(),
            true,
        );
        let result = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(model_identity()),
            false,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    }

    #[test]
    fn missing_persisted_plane_is_reinferred_when_model_available() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        // No loaded plane (artifact file missing) → re-infer.
        let result = resolve_one(
            definition,
            None,
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(model_identity()),
            false,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    }

    #[test]
    fn model_unavailable_uses_cached_stale_mask_with_warning() {
        // No model wired at all; a persisted (stale) plane exists.
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Stale,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let result =
            resolve_one(definition, Some(PERSISTED_VALUE), None, None, false, "src").unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
        assert!(result.model_unavailable);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("model is unavailable"));
        assert!(result.warnings[0].contains("stale"));
        assert_eq!(
            result
                .planes
                .get(&("vc".into(), "subject".into()))
                .unwrap()
                .values,
            vec![PERSISTED_VALUE; 16]
        );
    }

    // REVIEW-MASK-N3: without a configured expected model identity the
    // persisted artifact's model context cannot be confirmed — it must be
    // treated as stale (cache-with-warning / re-inference / hard error),
    // never as confirmably valid.
    #[test]
    fn unconfirmable_model_identity_is_never_loaded_persisted() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        // No model wired at all: the otherwise perfectly matching artifact
        // may not silently pass as `LoadedPersisted`. The F-051 path serves
        // it from cache WITH an explicit staleness warning instead.
        let result = resolve_one(
            definition.clone(),
            Some(PERSISTED_VALUE),
            None,
            None,
            false,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
        assert!(result.model_unavailable);
        assert_eq!(result.warnings.len(), 1);
        assert!(!result.warnings.is_empty());

        // With an available model but no configured identity to compare
        // against, validity is likewise unconfirmable → re-inference.
        let result = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            None,
            false,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
        assert!(!result.model_unavailable);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn model_unavailable_without_cache_is_a_hard_error() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Missing,
            "src",
            model_identity(),
            decode_context(),
            false,
        );
        // No model and no persisted plane → hard error, never a silent fallback.
        let error = resolve_one(definition, None, None, None, false, "src").unwrap_err();
        assert!(matches!(
            error,
            CoreError::MaskUnavailable {
                ref copy_id,
                ref mask_id,
                ref status,
            } if copy_id == "vc" && mask_id == "subject" && status == "model-unavailable"
        ));
    }

    #[test]
    fn model_unavailable_with_missing_plane_is_a_hard_error_even_if_status_valid() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        // Status says Valid, but the artifact cannot be loaded and no model is
        // available. Validity cannot be confirmed → treated as missing → error.
        let error = resolve_one(definition, None, None, None, false, "src").unwrap_err();
        assert!(matches!(
            error,
            CoreError::MaskUnavailable {
                ref status,
                ..
            } if status == "model-unavailable"
        ));
    }

    #[test]
    fn cross_copy_source_is_resolved() {
        // Active copy `vc` references a source mask in `other`.
        let source = source_definition(
            "other",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let mut copy = copy_with("vc", vec![]);
        copy.mask_layers = vec![layer_for("other", "subject")];
        let other = copy_with("other", vec![source]);
        let loaded_planes = BTreeMap::from([(
            ("other".into(), "subject".into()),
            MaskPlane::new(4, 4, vec![PERSISTED_VALUE; 16]).unwrap(),
        )]);
        let result = resolve_mask_planes(
            MaskLoadContext {
                copies: &[copy, other],
                active_copy_id: "vc",
                source_hash: "src",
                decode_context: &decode_context(),
                loaded_planes,
                inference: Some(&FakeInference {
                    available: true,
                    value: INFERRED_VALUE,
                }),
                model_identity: Some(model_identity()).as_ref(),
                refresh: false,
                policy: MaskPolicy::Warn,
            },
            &frame(),
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::LoadedPersisted);
        assert!(result
            .planes
            .contains_key(&("other".into(), "subject".into())));
    }

    #[test]
    fn blessing_pass_resolves_source_and_derived_masks() {
        // A source mask plus a derived (invert) mask referencing it. Both must
        // be resolved by the decision layer when the source is reachable and
        // valid: the source is loaded, and the derived mask is "blessed"
        // (marked Valid) because its only dependency was resolved.
        let source = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let derived = MaskDefinition {
            id: "inverted".into(),
            name: "inverted".into(),
            source_fingerprint: SourceFingerprint {
                content_hash: "src".into(),
                byte_length: 1,
                extras: Extras::new(),
            },
            decode_context: decode_context(),
            geometry_context: GeometryFingerprint {
                width: 4,
                height: 4,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            model: model_identity(),
            inference_resolution: Resolution {
                width: 4,
                height: 4,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "p".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Valid,
            created_at: "now".into(),
            generator_version: "g".into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Invert,
            references: vec![MaskReference {
                copy_id: "vc".into(),
                mask_id: "subject".into(),
                extras: Extras::new(),
            }],
            prompt: None,
            extras: Extras::new(),
        };
        let mut copy = copy_with("vc", vec![source, derived]);
        copy.mask_layers = vec![layer_for("vc", "subject"), layer_for("vc", "inverted")];
        let loaded_planes = BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(4, 4, vec![PERSISTED_VALUE; 16]).unwrap(),
        )]);
        let result = resolve_mask_planes(
            MaskLoadContext {
                copies: &[copy],
                active_copy_id: "vc",
                source_hash: "src",
                decode_context: &decode_context(),
                loaded_planes,
                inference: Some(&FakeInference {
                    available: true,
                    value: INFERRED_VALUE,
                }),
                model_identity: Some(model_identity()).as_ref(),
                refresh: false,
                policy: MaskPolicy::Warn,
            },
            &frame(),
        )
        .unwrap();

        // The source plane is loaded and resolved.
        assert!(result.planes.contains_key(&("vc".into(), "subject".into())));
        assert_eq!(
            result
                .planes
                .get(&("vc".into(), "subject".into()))
                .unwrap()
                .values,
            vec![PERSISTED_VALUE; 16]
        );
        // The source resolution is reported as loaded-from-persisted.
        assert!(result
            .outcomes
            .iter()
            .any(|o| o.mask_id == "subject" && o.from == MaskResolvedFrom::LoadedPersisted));
        // The derived mask is blessed (no own plane, but marked Valid) so the
        // downstream MaskGraph evaluation can resolve it.
        let blessed = result
            .copies
            .iter()
            .flat_map(|c| c.mask_library.iter())
            .find(|m| m.id == "inverted")
            .expect("derived mask present in result copies");
        assert_eq!(blessed.status, MaskStatus::Valid);
    }

    // --- F-050: comprehensive invalidation / re-inference coverage ----------

    #[test]
    fn decode_context_change_is_reinferred_when_model_available() {
        // The persisted mask was decoded with a different LibRaw/decode
        // fingerprint than the current source; it is therefore not confirmable
        // and must be re-inferred rather than loaded.
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let mut ctx_decode = decode_context();
        ctx_decode.version = "2".into();
        let result = resolve_single(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(model_identity()),
            false,
            "src",
            ctx_decode,
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
        assert_eq!(
            result
                .planes
                .get(&("vc".into(), "subject".into()))
                .unwrap()
                .values,
            vec![INFERRED_VALUE; 16]
        );
    }

    #[test]
    fn decode_context_change_falls_back_to_cache_when_model_unavailable() {
        // Decode context changed but no model is wired: the (possibly stale)
        // cached plane is used with a warning (F-051), never a silent drop.
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let mut ctx_decode = decode_context();
        ctx_decode.version = "2".into();
        let result = resolve_single(
            definition,
            Some(PERSISTED_VALUE),
            None,
            None,
            false,
            "src",
            ctx_decode,
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
        assert!(result.model_unavailable);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("model is unavailable"));
    }

    #[test]
    fn corrupt_status_is_reinferred_when_model_available() {
        // A mask marked `Corrupt` is not confirmable; with a model it is
        // re-inferred (no reuse of the corrupt artifact).
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Corrupt,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let result = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(model_identity()),
            false,
            "src",
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
    }

    #[test]
    fn corrupt_status_falls_back_to_cache_when_model_unavailable() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Corrupt,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let result =
            resolve_one(definition, Some(PERSISTED_VALUE), None, None, false, "src").unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
        assert!(result.model_unavailable);
        assert!(result.warnings[0].contains("Corrupt"));
    }

    #[test]
    fn inference_failure_is_hard_error_not_silent_fallback() {
        // The mask is `Stale` (re-inference required) and a cached plane exists,
        // but re-inference fails. F-051 forbids silently falling back to the
        // stale cache — the failure is surfaced as a hard error.
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Stale,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let error = resolve_one(
            definition,
            Some(PERSISTED_VALUE),
            Some(&FailingInference { available: true }),
            Some(model_identity()),
            false,
            "src",
        )
        .unwrap_err();
        assert!(matches!(error, CoreError::MaskInference { .. }));
    }

    #[test]
    fn inference_failure_without_cache_is_hard_error() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let error = resolve_one(
            definition,
            None,
            Some(&FailingInference { available: true }),
            Some(model_identity()),
            false,
            "src",
        )
        .unwrap_err();
        assert!(matches!(error, CoreError::MaskInference { .. }));
    }

    // --- REVIEW-SIDECAR-LOADER-RES: loading-path resolution validation ------

    /// A decoded plane whose dimensions match the declared artifact record
    /// loads normally: `LoadedPersisted`, no warning, confirmably valid.
    #[test]
    fn matching_dimensions_load_persisted_normally() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let result = resolve_with_plane(
            definition,
            Some(MaskPlane::new(4, 4, vec![PERSISTED_VALUE; 16]).unwrap()),
            None,
            Some(model_identity()),
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::LoadedPersisted);
        assert_eq!(
            result
                .planes
                .get(&("vc".into(), "subject".into()))
                .unwrap()
                .values,
            vec![PERSISTED_VALUE; 16]
        );
        assert!(result.warnings.is_empty());
        assert_eq!(persisted_status(&result), MaskStatus::Valid);
    }

    /// Decoded 2x8 against a declared 4x4 record: the artifact cannot be
    /// confirmed → with a model available it is re-inferred (never served as
    /// confirmably valid and never silently resampled to the declared size).
    #[test]
    fn dimension_mismatch_is_reinferred_when_model_available() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let result = resolve_with_plane(
            definition,
            Some(MaskPlane::new(2, 8, vec![PERSISTED_VALUE; 16]).unwrap()),
            Some(&FakeInference {
                available: true,
                value: INFERRED_VALUE,
            }),
            Some(model_identity()),
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::ReInferred);
        assert_eq!(
            result
                .planes
                .get(&("vc".into(), "subject".into()))
                .unwrap()
                .values,
            vec![INFERRED_VALUE; 16]
        );
        // The fresh matte resolved the layer again — it is not left marked
        // `Corrupt`, because the corrupt persisted artifact was replaced.
        assert_eq!(persisted_status(&result), MaskStatus::Valid);
    }

    /// Same mismatch without a model (F-051): the cached plane is used with an
    /// explicit dimension-mismatch warning and reported as `Corrupt` in the
    /// result copies — visible, not a silent fallback or resample.
    #[test]
    fn dimension_mismatch_marks_corrupt_when_cached_without_model() {
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            true,
        );
        let result = resolve_with_plane(
            definition,
            Some(MaskPlane::new(8, 2, vec![PERSISTED_VALUE; 16]).unwrap()),
            None,
            Some(model_identity()),
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
        assert!(result.model_unavailable);
        // The plane is passed through byte-identical — no resampling.
        assert_eq!(
            result
                .planes
                .get(&("vc".into(), "subject".into()))
                .unwrap()
                .values,
            vec![PERSISTED_VALUE; 16]
        );
        assert!(result.warnings.iter().any(|warning| {
            warning.contains("8x2")
                && warning.contains("4x4")
                && warning.contains("Corrupt")
                && warning.contains("no silent resample")
        }));
        assert_eq!(persisted_status(&result), MaskStatus::Corrupt);
    }

    /// Without an artifact record there is no declared reference resolution to
    /// compare against: the pre-existing behavior applies unchanged (artifact
    /// absent ⇒ validity unconfirmable ⇒ F-051 paths). No dimension check and
    /// no `Corrupt` override is invented from thin air.
    #[test]
    fn missing_artifact_reference_keeps_existing_behavior() {
        // Plane present but `artifact: None`.
        let definition = source_definition(
            "vc",
            "subject",
            MaskStatus::Valid,
            "src",
            model_identity(),
            decode_context(),
            false,
        );
        let result = resolve_with_plane(
            definition,
            Some(MaskPlane::new(4, 4, vec![PERSISTED_VALUE; 16]).unwrap()),
            None,
            Some(model_identity()),
        )
        .unwrap();
        assert_eq!(result.outcomes[0].from, MaskResolvedFrom::CachedUnavailable);
        assert!(result.model_unavailable);
        // Only the F-051 cache warning — no dimension-mismatch warning.
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("model is unavailable"));
        // Existing blessing semantics apply untouched.
        assert_eq!(persisted_status(&result), MaskStatus::Valid);
    }
}
