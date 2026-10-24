use blake3::Hasher;
use lumina_sidecar::EditRecipe;
use serde_json::Value;
mod local_state;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineFormat {
    EncodedSource,
    LinearProPhotoRgb,
    Rgba8Srgb,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    Decode,
    SourceActions,
    SpotHeal,
    AutoAnalysis,
    Adjustments,
    Masks,
    Crop,
    Output,
}

/// GEN-PIPELINE-DECOUPLE: decoupled geometry sub-stages executed inside the
/// `Crop` pipeline stage (the former 5-in-1 `apply_geometry`, now five
/// independently callable stages). The order is normative per
/// `feature/product/generative-expand.md` §Pipeline-Platzierung:
/// `LensCorrection(F-098) → GenerativeEdit(auto-fill) → Perspective(F-099) →
/// GenerativeEdit(expand) → Crop(F-093, incl. rotation/mirror)`.
///
/// `Pipeline::default()` intentionally keeps the coarse top-level stages
/// above: the sub-stages all operate `Rgba8Srgb → Rgba8Srgb` and share one
/// cache/RenderKey scope, so splitting them into top-level stages would churn
/// every `RenderKey`/`stage_digest` consumer without cache benefit. The render
/// entry points ([`crate::render_frame_from_base`]) implement exactly this
/// order via `apply_lens_stage` → `composite_auto_fill` →
/// `apply_perspective_stage` → `composite_expand` → `apply_crop_stage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryStage {
    LensCorrection,
    AutoFillTransparent,
    Perspective,
    GenerativeExpand,
    Crop,
}

/// Normative geometry sub-stage order. [`crate::render_frame_from_base`]
/// executes the stages in exactly this sequence.
pub const GEOMETRY_STAGE_ORDER: [GeometryStage; 5] = [
    GeometryStage::LensCorrection,
    GeometryStage::AutoFillTransparent,
    GeometryStage::Perspective,
    GeometryStage::GenerativeExpand,
    GeometryStage::Crop,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    stages: Vec<(PipelineStage, PipelineFormat, PipelineFormat)>,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self {
            stages: vec![
                (
                    PipelineStage::Decode,
                    PipelineFormat::EncodedSource,
                    PipelineFormat::Rgba8Srgb,
                ),
                (
                    PipelineStage::SourceActions,
                    PipelineFormat::Rgba8Srgb,
                    PipelineFormat::Rgba8Srgb,
                ),
                (
                    PipelineStage::SpotHeal,
                    PipelineFormat::Rgba8Srgb,
                    PipelineFormat::Rgba8Srgb,
                ),
                (
                    PipelineStage::AutoAnalysis,
                    PipelineFormat::Rgba8Srgb,
                    PipelineFormat::Rgba8Srgb,
                ),
                (
                    PipelineStage::Adjustments,
                    PipelineFormat::Rgba8Srgb,
                    PipelineFormat::Rgba8Srgb,
                ),
                (
                    PipelineStage::Crop,
                    PipelineFormat::Rgba8Srgb,
                    PipelineFormat::Rgba8Srgb,
                ),
                (
                    PipelineStage::Masks,
                    PipelineFormat::Rgba8Srgb,
                    PipelineFormat::Rgba8Srgb,
                ),
                (
                    PipelineStage::Output,
                    PipelineFormat::Rgba8Srgb,
                    PipelineFormat::Output,
                ),
            ],
        }
    }
}

impl Pipeline {
    pub fn stages(&self) -> &[(PipelineStage, PipelineFormat, PipelineFormat)] {
        &self.stages
    }
    pub fn validate(&self) -> bool {
        self.stages.windows(2).all(|pair| pair[0].2 == pair[1].1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceAction {
    DustRemoval { artifact_hash: String },
    AiReplacement { artifact_hash: String },
}

/// Output parameters that participate in the render identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSpec {
    pub profile: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderKey {
    pub source_content_hash: String,
    pub decode_version: String,
    pub pipeline_version: String,
    pub virtual_copy_id: String,
    pub recipe_hash: String,
    mask_recipe_hash: String,
    pub mask_artifact_hashes: Vec<String>,
    /// MASK-LOCAL-P0/P1.1: canonical ordered mask-layer digest including relative WB.
    mask_local_state_digest: Option<String>,
    /// REVIEW-CORE-SRCACC-1: checksums of the resolved source-action
    /// (repair-region) artifacts that the render actually applied. The recipe
    /// hash covers the persisted *references*; these hashes cover the resolved
    /// runtime artifacts so that changed repair artifacts cannot be served
    /// from stale cached pixels. Empty when no source actions were applied.
    pub source_action_artifact_hashes: Vec<String>,
    /// PERF-GUI-1: source-space ROI `(x, y, w, h)` that was cropped **before**
    /// the base stage (the interactive preview crop). It participates ONLY in
    /// the [`CacheStage::Base`] digest (`with_base_roi`), never in the render
    /// digest: the final render identity already distinguishes outputs by
    /// dimensions/profile, while the cached demosaiced base must additionally
    /// separate two windows of the same source with equal size but different
    /// offset. `None` (the default) keeps every existing digest unchanged.
    base_roi: Option<[u32; 4]>,
    pub output: OutputSpec,
    /// REVIEW-CORE-EXPORTKEY-1: full export parameters beyond
    /// [`OutputSpec`] (bit depth, quality, dithering, dither seed). They shape
    /// the encoded preview/export bytes, so two renders that differ only in
    /// quality/dither/seed/bit depth must never share a cache entry.
    /// `None` means "no encoder parameters attached" (a plain frame render)
    /// and is distinguished from every `Some(_)` in the digest. Attach via
    /// [`RenderKey::with_export_options`].
    pub export_options: Option<crate::ExportOptions>,
}

impl RenderKey {
    /// GEN-RENDERKEY-LINK-1: the persisted generative link fields
    /// (`GenerativeEdit::artifact`, `EditRecipe::spot_removals` including each
    /// `SpotRemoval::artifact`) are part of the render identity *generically*:
    /// `recipe_hash` is BLAKE3 over the full serialized recipe, so every link
    /// field (id, checksum, dimensions, format/channels/data_version) changes
    /// the hash and invalidates preview/export. The mask stage digest keeps
    /// these fields too (only geometry/lens/perspective are stripped there),
    /// so a link swap also invalidates cached mask evaluations. No separate
    /// link digest exists — none is needed. The decode digest deliberately
    /// stays stable (decode is upstream of SpotHeal/GenerativeEdit). A
    /// missing/corrupt bundle record is reported visibly downstream, never
    /// silently re-rendered or skipped.
    pub fn new(
        source_content_hash: impl Into<String>,
        decode_version: impl Into<String>,
        pipeline_version: impl Into<String>,
        virtual_copy_id: impl Into<String>,
        recipe: &EditRecipe,
        mask_artifact_hashes: Vec<String>,
        output: OutputSpec,
    ) -> Self {
        let recipe_bytes = serde_json::to_vec(recipe).expect("EditRecipe is serializable");
        let recipe_hash = blake3::hash(&recipe_bytes).to_hex().to_string();
        let mut mask_recipe = recipe.clone();
        // Geometry is downstream of source-sized masks; keep it in the full
        // recipe hash but remove it from the mask identity.
        mask_recipe.geometry = None;
        mask_recipe.lens_correction = None;
        mask_recipe.perspective = None;
        mask_recipe.upright = None;
        for key in [
            "crop",
            "rotation",
            "mirror",
            "geometry",
            "output",
            "output_profile",
            "output_width",
            "output_height",
            "output_format",
        ] {
            mask_recipe.options.remove(key);
            mask_recipe.extras.remove(key);
        }
        let mask_recipe_bytes =
            serde_json::to_vec(&mask_recipe).expect("EditRecipe is serializable");
        let mask_recipe_hash = blake3::hash(&mask_recipe_bytes).to_hex().to_string();
        Self {
            source_content_hash: source_content_hash.into(),
            decode_version: decode_version.into(),
            pipeline_version: pipeline_version.into(),
            virtual_copy_id: virtual_copy_id.into(),
            recipe_hash,
            mask_recipe_hash,
            mask_artifact_hashes,
            mask_local_state_digest: None,
            // REVIEW-CORE-SRCACC-1 / REVIEW-CORE-EXPORTKEY-1: neutral defaults
            // keep the existing constructor call sites compiling. Callers that
            // apply source-action artifacts or encode with explicit export
            // options MUST attach them via the `with_*` builders so the
            // digest covers them.
            source_action_artifact_hashes: Vec::new(),
            base_roi: None,
            output,
            export_options: None,
        }
    }

    /// Attaches the full encoder parameters (REVIEW-CORE-EXPORTKEY-1) so they
    /// participate in the render-scope digest. Every caller that encodes a
    /// rendered frame into preview/export bytes with explicit options must
    /// attach them here; otherwise cache hits can serve pixels encoded with a
    /// different quality/dither/seed/bit depth.
    #[must_use]
    pub fn with_export_options(mut self, options: crate::ExportOptions) -> Self {
        self.export_options = Some(options);
        self
    }

    /// Attaches the checksums of the resolved source-action artifacts
    /// (REVIEW-CORE-SRCACC-1) so changed repair artifacts invalidate every
    /// downstream cache stage instead of serving stale pixels.
    #[must_use]
    pub fn with_source_action_hashes<I: IntoIterator<Item = String>>(mut self, hashes: I) -> Self {
        self.source_action_artifact_hashes = hashes.into_iter().collect();
        self
    }

    /// PERF-GUI-1: attaches the pre-crop source ROI so it participates in the
    /// [`CacheStage::Base`] digest. Like `with_export_options` this is opt-in:
    /// call sites that do not cache a demosaiced base keep `None` and produce
    /// byte-identical digests to before.
    #[must_use]
    pub fn with_base_roi(mut self, roi: Option<[u32; 4]>) -> Self {
        self.base_roi = roi;
        self
    }

    pub fn digest(&self) -> String {
        self.digest_for("render")
    }

    pub fn stage_digest(&self, stage: crate::cache::CacheStage) -> String {
        match stage {
            crate::cache::CacheStage::Decode => self.digest_for("decode"),
            crate::cache::CacheStage::Base => self.digest_for("base"),
            crate::cache::CacheStage::Mask => self.digest_for("mask"),
            crate::cache::CacheStage::Histogram => self.digest_for("histogram"),
            crate::cache::CacheStage::Preview | crate::cache::CacheStage::Export => {
                self.digest_for("render")
            }
        }
    }

    fn digest_for(&self, scope: &str) -> String {
        let mut hasher = Hasher::new();
        hasher.update(scope.as_bytes());
        hasher.update(&[0]);
        for value in [
            &self.source_content_hash,
            &self.decode_version,
            &self.pipeline_version,
            &self.virtual_copy_id,
        ] {
            hasher.update(value.as_bytes());
            hasher.update(&[0]);
        }
        if scope == "base" {
            // PERF-GUI-1: the demosaiced/pre-adjustment base stage identity.
            // It sits between decode and adjustments, so — like "decode" — it
            // deliberately excludes the whole recipe (any slider change must
            // reuse the base), but unlike "decode" it covers the resolved
            // source-action artifacts composited INTO the base, the pre-crop
            // ROI window and the resulting frame geometry.
            for value in &self.source_action_artifact_hashes {
                hasher.update(value.as_bytes());
                hasher.update(&[0]);
            }
            match self.base_roi {
                None => {
                    hasher.update(&[0]);
                }
                Some([x, y, w, h]) => {
                    hasher.update(&[1]);
                    hasher.update(&x.to_le_bytes());
                    hasher.update(&y.to_le_bytes());
                    hasher.update(&w.to_le_bytes());
                    hasher.update(&h.to_le_bytes());
                }
            }
            hasher.update(&self.output.width.to_le_bytes());
            hasher.update(&self.output.height.to_le_bytes());
        }
        if scope != "decode" && scope != "base" {
            // Crop and output are downstream of masks.  They are deliberately
            // not part of the mask identity: changing either must reuse the
            // decoded source and any source-sized matte.
            let recipe_hash = if scope == "mask" {
                &self.mask_recipe_hash
            } else {
                &self.recipe_hash
            };
            hasher.update(recipe_hash.as_bytes());
            hasher.update(&[0]);
            for value in &self.mask_artifact_hashes {
                hasher.update(value.as_bytes());
                hasher.update(&[0]);
            }
            local_state::update_digest(&mut hasher, self.mask_local_state_digest.as_deref());
            // REVIEW-CORE-SRCACC-1: resolved source-action artifacts composite
            // right after decode and before every downstream stage, so every
            // non-decode stage digest covers their checksums. A changed repair
            // artifact therefore invalidates mask/preview/histogram/export
            // entries instead of serving stale pixels.
            for value in &self.source_action_artifact_hashes {
                hasher.update(value.as_bytes());
                hasher.update(&[0]);
            }
        }
        if scope == "histogram" {
            // REVIEW-CORE-N1: a histogram is measured on the post-crop frame,
            // so two renders that differ only in output size produce different
            // histograms. Without the geometry in the stage key, a cached
            // histogram of one size could be served for another. Profile and
            // format stay render-only (they cannot change luminance bins).
            hasher.update(&self.output.width.to_le_bytes());
            hasher.update(&self.output.height.to_le_bytes());
        }
        if matches!(scope, "render") {
            hasher.update(self.output.profile.as_bytes());
            hasher.update(&[0]);
            hasher.update(self.output.format.as_bytes());
            hasher.update(&self.output.width.to_le_bytes());
            hasher.update(&self.output.height.to_le_bytes());
            // REVIEW-CORE-EXPORTKEY-1: full encoder identity in the render
            // digest. The leading marker byte separates "no encoder options"
            // from every attached option set.
            match &self.export_options {
                None => {
                    hasher.update(&[0]);
                }
                Some(options) => {
                    hasher.update(&[1]);
                    hasher.update(options.format.default_extension().as_bytes());
                    hasher.update(&[0]);
                    hasher.update(&[match options.bit_depth {
                        crate::BitDepth::Eight => 8u8,
                        crate::BitDepth::Sixteen => 16u8,
                    }]);
                    hasher.update(&[options.quality]);
                    hasher.update(&[u8::from(options.dither)]);
                    hasher.update(&options.seed.to_le_bytes());
                }
            }
        }
        hasher.finalize().to_hex().to_string()
    }

    pub fn recipe_value(recipe: &EditRecipe) -> Value {
        serde_json::to_value(recipe).expect("EditRecipe is serializable")
    }
}

#[cfg(test)]
mod tests;
