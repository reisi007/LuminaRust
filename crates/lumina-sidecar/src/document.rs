//! SPLICE-SIDECAR-LIB-90 (Slice 11): the two inherent `impl SidecarDocument`
//! blocks of the sidecar root file, moved verbatim into their own module.
//!
//! One responsibility: the lifecycle, mutation and validation of the persisted
//! document itself — construction and schema-bounded parsing (`new`,
//! `from_json`), the virtual-copy operations (duplicate, rename, sort, delete,
//! restore) and `validate`, plus the source-level IPTC draft mutations that
//! record their own history. The pass-through file I/O of the same type
//! (`load_sidecar`, `save_sidecar`, …) stays in `sidecar_io`.
//!
//! Both blocks are byte-identical to their former place in the root; only this
//! module documentation and the `use` block are new. The methods keep their
//! own visibility, so every `pub` method stays reachable as before
//! (`lumina_sidecar::SidecarDocument`); the struct and all its fields are
//! `pub`, which is why the inherent impl may live in a child module.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::mask_group_ops;
use crate::{
    invalid, validate_adjustments, validate_ai_select, validate_artifact, validate_collection_id,
    validate_collection_name, validate_culling_section, validate_face_analysis,
    validate_generative_ref, validate_history_entry, validate_keyword,
    validate_mask_layer_local_state, validate_metadata_field_id, validate_metadata_field_value,
    validate_metadata_origin, validate_metadata_timestamp, validate_name, validate_prompt,
    validate_relative_path, validate_unique_ids, EditRecipe, Extras, Flag, MaskOperation,
    MaskPrompt, MetadataDraft, MetadataHistoryEntry, SidecarDocument, SidecarError, SourceIdentity,
    VirtualCopy, FORMAT, MAX_COLLECTIONS_PER_DOCUMENT, MAX_KEYWORDS_PER_DOCUMENT,
    MAX_METADATA_HISTORY_ENTRIES, MAX_SIDECAR_BYTES, MAX_VIRTUAL_COPIES, SCHEMA_VERSION,
};

impl SidecarDocument {
    /// LRPAR-G15-IPTC-S1: sets draft fields and records one history entry.
    /// `fields` maps registry field IDs to new values; an empty or
    /// whitespace-only value removes the field ("leer = Feld entfernen").
    /// All-or-nothing per call: any unknown ID or invalid value rejects the
    /// whole call and leaves the document unchanged. Returns `Ok(true)` when
    /// the draft changed (exactly one history entry prepended, `rev =
    /// latest + 1`, `changed` = actually affected IDs in sorted order) and
    /// `Ok(false)` for idempotent no-ops (no entry appended). Recipes,
    /// masks and per-copy edit history are never touched.
    pub fn apply_metadata_draft(
        &mut self,
        fields: &BTreeMap<String, String>,
        origin: &str,
        timestamp: &str,
    ) -> Result<bool, SidecarError> {
        validate_metadata_origin(origin)?;
        validate_metadata_timestamp(timestamp)?;
        for (field, value) in fields {
            validate_metadata_field_value(field, value)?;
        }
        let mut draft = self.metadata.draft.clone();
        let mut changed = BTreeSet::new();
        for (field, value) in fields {
            if value.is_empty() || value.trim().is_empty() {
                if draft.remove(field).is_some() {
                    changed.insert(field.clone());
                }
            } else if draft.get(field).map(String::as_str) != Some(value.as_str()) {
                draft.insert(field.clone(), value.clone());
                changed.insert(field.clone());
            }
        }
        if changed.is_empty() {
            return Ok(false);
        }
        self.metadata.draft = draft;
        self.push_metadata_history(origin, timestamp, changed.into_iter().collect());
        self.validate().map(|()| true)
    }

    /// LRPAR-G15-IPTC-S1: removes the given draft fields (unknown IDs fail
    /// loudly, all-or-nothing). `Ok(false)` when none of the fields was set
    /// (no history entry); otherwise one entry with the removed IDs.
    pub fn clear_metadata_fields(
        &mut self,
        fields: &[&str],
        origin: &str,
        timestamp: &str,
    ) -> Result<bool, SidecarError> {
        validate_metadata_origin(origin)?;
        validate_metadata_timestamp(timestamp)?;
        for field in fields {
            validate_metadata_field_id(field)?;
        }
        let mut removed = BTreeSet::new();
        for field in fields {
            if self.metadata.draft.remove(*field).is_some() {
                removed.insert((*field).to_string());
            }
        }
        if removed.is_empty() {
            return Ok(false);
        }
        self.push_metadata_history(origin, timestamp, removed.into_iter().collect());
        self.validate().map(|()| true)
    }

    /// LRPAR-G15-IPTC-S1: removes every draft field (`--all`; the history
    /// itself is kept and gains one entry listing the removed IDs).
    /// `Ok(false)` when the draft was already empty (history untouched).
    pub fn clear_metadata_draft(
        &mut self,
        origin: &str,
        timestamp: &str,
    ) -> Result<bool, SidecarError> {
        validate_metadata_origin(origin)?;
        validate_metadata_timestamp(timestamp)?;
        if self.metadata.draft.is_empty() {
            return Ok(false);
        }
        let removed: Vec<String> = self.metadata.draft.keys().cloned().collect();
        self.metadata.draft.clear();
        self.push_metadata_history(origin, timestamp, removed);
        self.validate().map(|()| true)
    }

    /// LRPAR-G15-IPTC-S1: explicitly clears the whole draft history (the
    /// only way to empty it; there is no undo). Idempotent: clearing an
    /// already-empty history is a no-op. Draft values are kept.
    pub fn clear_metadata_history(&mut self) {
        self.metadata.history.clear();
    }

    /// Prepends one history entry (`rev = latest + 1`) and enforces the FIFO
    /// cap deterministically (oldest entries fall off the end). Callers have
    /// validated `origin`/`timestamp` and computed a non-empty, sorted
    /// `changed` list.
    fn push_metadata_history(&mut self, origin: &str, timestamp: &str, changed: Vec<String>) {
        let rev = self.metadata.latest_rev() + 1;
        self.metadata.history.insert(
            0,
            MetadataHistoryEntry {
                rev,
                timestamp: timestamp.to_string(),
                origin: origin.to_string(),
                changed,
            },
        );
        self.metadata.history.truncate(MAX_METADATA_HISTORY_ENTRIES);
    }
}

impl SidecarDocument {
    pub fn new(source: SourceIdentity, pipeline_version: impl Into<String>) -> Self {
        Self {
            format: FORMAT.into(),
            schema_version: SCHEMA_VERSION,
            source,
            analysis_fingerprint: None,
            pipeline_version: pipeline_version.into(),
            virtual_copies: vec![VirtualCopy {
                id: "vc-original".into(),
                name: "Original".into(),
                is_default: true,
                rating: 0,
                flag: Flag::Unflagged,
                recipe: EditRecipe::default(),
                mask_library: vec![],
                mask_layers: vec![],
                history: vec![],
                export_records: vec![],
                extras: Extras::new(),
            }],
            deleted_virtual_copies: vec![],
            presets: vec![],
            keywords: vec![],
            collections: vec![],
            metadata: MetadataDraft::default(),
            face: None,
            culling: None,
            stack: None,
            extras: Extras::new(),
        }
    }

    pub fn from_json(json: &str) -> Result<Self, SidecarError> {
        if json.len() > MAX_SIDECAR_BYTES {
            return Err(SidecarError::Invalid("sidecar exceeds size limit".into()));
        }
        let value: Value =
            serde_json::from_str(json).map_err(|e| SidecarError::Json(e.to_string()))?;
        let version = value
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| SidecarError::Invalid("missing schema_version".into()))?;
        // REVIEW-SIDECAR-N2 (documented decision, 2026-08-25): the historical
        // `schema_version` 0 → 1 bump is *migration-only*. The loader never
        // normalizes silently; a v0 document is rejected loudly and points at
        // the explicit migration path (`migrate_json` / `migrate_sidecar_file`),
        // keeping `from_json` and the migration machinery consistent. This
        // matches the pre-alpha rule that incompatible documents fail with a
        // visible error instead of a best-effort interpretation.
        if version == 0 {
            return Err(SidecarError::Invalid(
                "unsupported schema_version 0; explicit migration is required \
                 (see migrate_sidecar_file)"
                    .into(),
            ));
        }
        if version != 1 && version != u64::from(SCHEMA_VERSION) {
            return Err(SidecarError::Invalid(format!(
                "unsupported schema_version {version}; explicit migration is required"
            )));
        }
        let mut document: Self =
            serde_json::from_value(value).map_err(|e| SidecarError::Json(e.to_string()))?;
        document.normalize_legacy_local_adjustments()?;
        document.validate()?;
        Ok(document)
    }

    pub fn duplicate_virtual_copy(
        &mut self,
        source_id: &str,
        new_id: impl Into<String>,
        new_name: impl Into<String>,
    ) -> Result<(), SidecarError> {
        let mut copy = self
            .virtual_copies
            .iter()
            .find(|copy| copy.id == source_id)
            .cloned()
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{source_id}`")))?;
        copy.id = new_id.into();
        copy.name = new_name.into();
        copy.is_default = false;
        if self
            .virtual_copies
            .iter()
            .any(|candidate| candidate.id == copy.id)
            || self
                .deleted_virtual_copies
                .iter()
                .any(|candidate| candidate.id == copy.id)
        {
            return invalid(format!("duplicate virtual copy id `{}`", copy.id));
        }
        // REVIEW-SIDECAR-N4: the document is validated with the copy in place,
        // but a failed validation rolls the insertion back so `self` stays
        // exactly as it was before the rejected call.
        self.virtual_copies.push(copy);
        if let Err(error) = self.validate() {
            self.virtual_copies.pop();
            return Err(error);
        }
        Ok(())
    }

    pub fn rename_virtual_copy(
        &mut self,
        id: &str,
        name: impl Into<String>,
    ) -> Result<(), SidecarError> {
        let name = name.into();
        // REVIEW-SIDECAR-N4: check the incoming value before mutating so an
        // invalid rename cannot leave a mutated document behind.
        validate_name("virtual copy name", &name)?;
        let copy = self
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{id}`")))?;
        copy.name = name;
        self.validate()
    }

    pub fn sort_virtual_copies(&mut self) {
        self.virtual_copies
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    }

    pub fn delete_virtual_copy(&mut self, id: &str) -> Result<(), SidecarError> {
        if id == "vc-original" {
            return invalid("the default virtual copy cannot be deleted");
        }
        let index = self
            .virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{id}`")))?;
        // REVIEW-SIDECAR-N4: validate the post-delete state *before* committing
        // it. Deleting a copy that still owns masks referenced from elsewhere
        // must fail loudly AND leave the document unchanged — previously the
        // copy had already been moved to `deleted_virtual_copies` when
        // validation failed, leaving callers with a mutated-but-invalid
        // document.
        let copy = self.virtual_copies.remove(index);
        if let Err(error) = self.validate() {
            // Roll back to the exact pre-call state.
            self.virtual_copies.insert(index, copy);
            return Err(error);
        }
        self.deleted_virtual_copies.push(copy);
        self.validate()
    }

    pub fn restore_virtual_copy(&mut self, id: &str) -> Result<(), SidecarError> {
        let index = self
            .deleted_virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown deleted virtual copy `{id}`")))?;
        // REVIEW-SIDECAR-N4: same validate-before-commit contract as
        // `delete_virtual_copy`; a rejected restore leaves the copy in the
        // deleted list at its original position.
        let copy = self.deleted_virtual_copies.remove(index);
        self.virtual_copies.push(copy);
        if let Err(error) = self.validate() {
            let copy = self.virtual_copies.pop();
            if let Some(copy) = copy {
                self.deleted_virtual_copies.insert(index, copy);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.format != FORMAT {
            return invalid("format must be `lumina-sidecar`");
        }
        if self.schema_version != 1 && self.schema_version != SCHEMA_VERSION {
            return invalid("unsupported schema_version");
        }
        validate_name("pipeline_version", &self.pipeline_version)?;
        validate_name("source.relative_name", &self.source.relative_name)?;
        validate_relative_path("source.relative_name", &self.source.relative_name)?;
        if !(1..=8).contains(&self.source.orientation) {
            return invalid("source.orientation must be between 1 and 8");
        }
        // G-15 META-MVP (Slice 1): source-level metadata. Empty is the legacy
        // identity; present entries are validated loudly (no silent
        // deduplication, clamping or trimming).
        if self.keywords.len() > MAX_KEYWORDS_PER_DOCUMENT {
            return invalid(format!(
                "keyword list exceeds limit of {MAX_KEYWORDS_PER_DOCUMENT}"
            ));
        }
        {
            let mut seen = BTreeSet::new();
            for keyword in &self.keywords {
                validate_keyword(keyword)?;
                if !seen.insert(keyword) {
                    return invalid(format!("duplicate keyword `{keyword}`"));
                }
            }
        }
        if self.collections.len() > MAX_COLLECTIONS_PER_DOCUMENT {
            return invalid(format!(
                "collection list exceeds limit of {MAX_COLLECTIONS_PER_DOCUMENT}"
            ));
        }
        {
            let mut seen = BTreeSet::new();
            for membership in &self.collections {
                validate_collection_id(&membership.id)?;
                validate_collection_name(&membership.name)?;
                if !seen.insert(&membership.id) {
                    return invalid(format!("duplicate collection id `{}`", membership.id));
                }
            }
        }
        // LRPAR-G15-IPTC-S1: source-level metadata draft (own history,
        // separate from the per-copy edit history below).
        self.metadata.validate()?;
        // LRPAR-G12-FACE-20 (FACE-20-S1): source-level face analysis. Absent is
        // the valid legacy state; a present section is validated loudly,
        // including every detection ↔ embedding ↔ cluster ↔ person cross-link.
        if let Some(face) = &self.face {
            validate_face_analysis(face)?;
        }
        // LRPAR-G09-CULL-25: source-level culling proposal. Absent is the valid
        // "no proposal" state; a present section is validated loudly.
        if let Some(culling) = &self.culling {
            validate_culling_section(culling)?;
        }
        // LRPAR-G15-STACK-15: optional source-level image-stack membership.
        // Absent is the valid "not stacked" state; a present section is
        // validated loudly (version pin, bare same-folder member names,
        // sorted/unique members, cover ∈ members).
        if let Some(stack) = &self.stack {
            stack.validate()?;
        }
        if self.virtual_copies.is_empty() {
            return invalid("at least one virtual copy is required");
        }
        if self.virtual_copies.len() + self.deleted_virtual_copies.len() > MAX_VIRTUAL_COPIES {
            return invalid("sidecar exceeds virtual copy limit");
        }
        let mut copy_ids = BTreeSet::new();
        let mut defaults = 0;
        for copy in &self.virtual_copies {
            validate_name("virtual copy id", &copy.id)?;
            validate_name("virtual copy name", &copy.name)?;
            validate_name("recipe_version", &copy.recipe.recipe_version)?;
            validate_adjustments(&copy.recipe)?;
            // LR-01: star ratings are 0..=5 (0 = unrated); anything else is a
            // schema violation, never silently clamped.
            if copy.rating > 5 {
                return invalid(format!(
                    "virtual copy `{}` rating must be 0..=5, got {}",
                    copy.id, copy.rating
                ));
            }
            if !copy_ids.insert(&copy.id) {
                return invalid(format!("duplicate virtual copy id `{}`", copy.id));
            }
            defaults += usize::from(copy.is_default);
            let mut mask_ids = BTreeSet::new();
            for mask in &copy.mask_library {
                validate_name("mask id", &mask.id)?;
                validate_name("mask name", &mask.name)?;
                validate_name("mask rescaling_method", &mask.rescaling_method)?;
                validate_name("mask generator_version", &mask.generator_version)?;
                // REVIEW-SIDECAR-N3: a zero-sized inference resolution can
                // never back a valid matte; reject it at the schema boundary.
                if mask.inference_resolution.width == 0 || mask.inference_resolution.height == 0 {
                    return invalid(format!(
                        "mask `{}/{}` inference_resolution must be non-zero",
                        copy.id, mask.id
                    ));
                }
                if !mask_ids.insert(&mask.id) {
                    return invalid(format!(
                        "duplicate mask id `{}` in copy `{}`",
                        mask.id, copy.id
                    ));
                }
                if let Some(a) = &mask.artifact {
                    validate_artifact(a)?;
                }
                validate_prompt(&mask.prompt)?;
                validate_ai_select(&mask.ai_select)?;
                // G-03: an AI selection names an automatic source matte, so it
                // is only meaningful on `source` nodes. Derived nodes combine
                // already-resolved mattes; stamping them `ai_select` would
                // silently change their meaning.
                if mask.ai_select.is_some() && mask.operation != MaskOperation::Source {
                    return invalid(format!(
                        "mask `{}/{}` carries ai_select on a non-source operation; ai_select requires `source`",
                        copy.id, mask.id
                    ));
                }
                // G-03: a range prompt is a deterministic source stage and
                // likewise never lives on a derived node.
                if mask.operation != MaskOperation::Source
                    && matches!(
                        mask.prompt,
                        Some(MaskPrompt::ColorRange { .. })
                            | Some(MaskPrompt::LuminanceRange { .. })
                    )
                {
                    return invalid(format!(
                        "mask `{}/{}` carries a range prompt on a non-source operation; range prompts require `source`",
                        copy.id, mask.id
                    ));
                }
                let arity_is_valid = match mask.operation {
                    MaskOperation::Source => mask.references.is_empty(),
                    MaskOperation::Invert => mask.references.len() == 1,
                    MaskOperation::Subtract => mask.references.len() == 2,
                    MaskOperation::Union | MaskOperation::Intersect => mask.references.len() >= 2,
                };
                if !arity_is_valid {
                    return invalid(format!(
                        "mask `{}/{}` operation `{}` has invalid input arity ({})",
                        copy.id,
                        mask.id,
                        serde_json::to_string(&mask.operation).unwrap_or_default(),
                        mask.references.len()
                    ));
                }
                for reference in &mask.references {
                    validate_name("mask reference copy_id", &reference.copy_id)?;
                    validate_name("mask reference mask_id", &reference.mask_id)?;
                }
            }
            for export in &copy.export_records {
                validate_name("export id", &export.id)?;
                validate_relative_path("export relative_path", &export.relative_path)?;
                validate_name("export format", &export.format)?;
            }
            validate_unique_ids("mask layer", &copy.mask_layers, |layer| &layer.id)?;
            validate_unique_ids("history entry", &copy.history, |entry| &entry.id)?;
            validate_unique_ids("export record", &copy.export_records, |export| &export.id)?;
            for layer in &copy.mask_layers {
                validate_name("mask layer id", &layer.id)?;
                validate_mask_layer_local_state(layer)
                    .map_err(|error| SidecarError::Invalid(error.to_string()))?;
                // REVIEW-SIDECAR-N3: local adjustment parameters were persisted
                // without any finite/range validation. feather/blur are
                // radii-like quantities (>= 0), density is a normalized
                // 0..=1 opacity. Defaults (0/0/1) of every existing valid
                // sidecar satisfy these bounds.
                if !layer.feather.is_finite() || layer.feather < 0.0 {
                    return invalid(format!(
                        "mask layer `{}` feather must be finite and >= 0",
                        layer.id
                    ));
                }
                if !layer.blur.is_finite() || layer.blur < 0.0 {
                    return invalid(format!(
                        "mask layer `{}` blur must be finite and >= 0",
                        layer.id
                    ));
                }
                if !layer.density.is_finite() || !(0.0..=1.0).contains(&layer.density) {
                    return invalid(format!(
                        "mask layer `{}` density must be finite within 0..=1",
                        layer.id
                    ));
                }
            }
            for entry in &copy.history {
                validate_name("history entry id", &entry.id)?;
                validate_history_entry(entry)?;
            }
        }
        for copy in &self.deleted_virtual_copies {
            validate_name("deleted virtual copy id", &copy.id)?;
            validate_name("deleted virtual copy name", &copy.name)?;
            validate_name("recipe_version", &copy.recipe.recipe_version)?;
            validate_adjustments(&copy.recipe)?;
            if !copy_ids.insert(&copy.id) {
                return invalid(format!("duplicate virtual copy id `{}`", copy.id));
            }
        }
        validate_unique_ids("preset", &self.presets, |preset| &preset.id)?;
        for preset in &self.presets {
            validate_name("preset id", &preset.id)?;
        }
        for copy in &self.virtual_copies {
            for layer in &copy.mask_layers {
                let target = self
                    .virtual_copies
                    .iter()
                    .find(|candidate| candidate.id == layer.mask.copy_id)
                    .ok_or_else(|| {
                        SidecarError::Invalid(format!(
                            "mask layer `{}` references unknown copy `{}`",
                            layer.id, layer.mask.copy_id
                        ))
                    })?;
                if !target
                    .mask_library
                    .iter()
                    .any(|mask| mask.id == layer.mask.mask_id)
                {
                    return invalid(format!(
                        "mask layer `{}` references unknown mask `{}/{}`",
                        layer.id, layer.mask.copy_id, layer.mask.mask_id
                    ));
                }
            }
        }
        if defaults != 1 {
            return invalid("exactly one default virtual copy is required");
        }
        if !self
            .virtual_copies
            .iter()
            .any(|c| c.id == "vc-original" && c.is_default)
        {
            return invalid("`vc-original` must be the default virtual copy");
        }
        for copy in &self.virtual_copies {
            if let Some(ge) = &copy.recipe.generative_edit {
                if ge.version != 1 {
                    return invalid("generative_edit.version must be 1");
                }
                let expand = ge.effective_expand();
                if expand && ge.canvas.is_none() {
                    return invalid("generative_edit.expand_beyond_image requires canvas");
                }
                if !expand && ge.canvas.is_some() {
                    // When expand is false, canvas must be None or equal to source (but we don't have source dims here)
                    // For test, any canvas with expand false is considered invalid if canvas is Some
                    // The strict rule: expand false => canvas must be None
                    // To keep lenient, we only reject when canvas is Some and expand is Some(false) explicitly
                    if ge.expand_beyond_image == Some(false) {
                        return invalid(
                            "generative_edit.canvas present but expand_beyond_image is false",
                        );
                    }
                }
                if let Some(canvas) = &ge.canvas {
                    canvas.validate()?;
                }
                ge.validate_edit_extras()?;
                if let Some(link) = &ge.artifact {
                    validate_generative_ref(link)?;
                }
            }
        }
        mask_group_ops::validate_mask_graph(self, &copy_ids)?;
        mask_group_ops::validate_mask_groups(self)
    }
}
