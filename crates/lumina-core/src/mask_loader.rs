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
    /// G-03: a deterministic range stage was computed straight from the
    /// source frame (no artifact, no model, no cache). A parameter change
    /// implicitly yields a new plane; stale/missing states don't apply.
    ComputedDeterministic,
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
    ///
    /// This is the *copy-wide* one-shot switch (`update_masks`, armed by
    /// `develop`/`export`/`batch --update-masks`, `mask --update-masks` and the
    /// forced `regenerate --module masks`). It overrides the persisted-valid
    /// fastpath for every reachable source mask. A persisted per-mask
    /// [`MaskStatus::Pending`] marker is a *narrower* explicit request: it
    /// re-infers only that mask and is likewise not reported as implicit.
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

        // G-03: deterministic range stages compute straight from the frame —
        // no artifact, no model, no cache, no refresh flag. The plane is a
        // pure function of pixels + parameters, so recomputation is the cache
        // hit (byte-identical); a parameter change is the invalidation.
        if crate::range_masks::is_range_prompt(definition.prompt.as_ref()) {
            let prompt = definition
                .prompt
                .as_ref()
                .expect("range prompt was checked");
            let plane =
                crate::range_masks::evaluate_range_prompt(frame, prompt).map_err(|error| {
                    CoreError::MaskEvaluation {
                        copy_id: key.0.clone(),
                        mask_id: key.1.clone(),
                        reason: error.to_string(),
                    }
                })?;
            planes.insert(key.clone(), plane);
            resolved_sources.insert(key.clone());
            outcomes.push(MaskLoadOutcome {
                copy_id: key.0.clone(),
                mask_id: key.1.clone(),
                from: MaskResolvedFrom::ComputedDeterministic,
            });
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
            // F-100/GUI-GEN-GRANULAR-10: an *implicit* re-inference (the
            // persisted artifact was missing/stale, no explicit refresh was
            // requested) is never silent — surface it loudly so no module is
            // recomputed behind the user's back. An explicit refresh is either
            // the copy-wide `refresh` switch (`--update-masks` /
            // `regenerate --module masks`) or a persisted per-mask `Pending`
            // marker; the collective `regenerate` default uses the latter so it
            // re-infers exactly the stale/missing masks it marked and stays
            // quiet for that deliberately requested work.
            if !ctx.refresh && definition.status != MaskStatus::Pending {
                warnings.push(format!(
                    "mask `{}/{}` was re-inferred during the render because its persisted \
                     artifact was missing or stale; persisted artifacts of other modules are \
                     untouched (use the explicit regeneration action to refresh deliberately)",
                    key.0, key.1
                ));
            }
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
mod tests;
