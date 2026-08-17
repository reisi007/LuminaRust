use blake3::Hasher;
use lumina_sidecar::EditRecipe;
use serde_json::Value;

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
    AutoAnalysis,
    Adjustments,
    Masks,
    Crop,
    Output,
}

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
                    PipelineStage::Masks,
                    PipelineFormat::Rgba8Srgb,
                    PipelineFormat::Rgba8Srgb,
                ),
                (
                    PipelineStage::Crop,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderKey {
    pub source_content_hash: String,
    pub decode_version: String,
    pub pipeline_version: String,
    pub virtual_copy_id: String,
    pub recipe_hash: String,
    mask_recipe_hash: String,
    pub mask_artifact_hashes: Vec<String>,
    pub output_profile: String,
    pub output_width: u32,
    pub output_height: u32,
    pub output_format: String,
}

impl RenderKey {
    pub fn new(
        source_content_hash: impl Into<String>,
        decode_version: impl Into<String>,
        pipeline_version: impl Into<String>,
        virtual_copy_id: impl Into<String>,
        recipe: &EditRecipe,
        mask_artifact_hashes: Vec<String>,
        output_profile: impl Into<String>,
        output_width: u32,
        output_height: u32,
        output_format: impl Into<String>,
    ) -> Self {
        let recipe_bytes = serde_json::to_vec(recipe).expect("EditRecipe is serializable");
        let recipe_hash = blake3::hash(&recipe_bytes).to_hex().to_string();
        let mut mask_recipe = recipe.clone();
        // Geometry is downstream of source-sized masks; keep it in the full
        // recipe hash but remove it from the mask identity.
        mask_recipe.geometry = None;
        mask_recipe.lens_correction = None;
        mask_recipe.perspective = None;
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
            output_profile: output_profile.into(),
            output_width,
            output_height,
            output_format: output_format.into(),
        }
    }

    pub fn digest(&self) -> String {
        self.digest_for("render")
    }

    pub fn stage_digest(&self, stage: crate::cache::CacheStage) -> String {
        match stage {
            crate::cache::CacheStage::Decode => self.digest_for("decode"),
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
        if scope != "decode" {
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
        }
        if matches!(scope, "render") {
            hasher.update(self.output_profile.as_bytes());
            hasher.update(&[0]);
            hasher.update(self.output_format.as_bytes());
            hasher.update(&self.output_width.to_le_bytes());
            hasher.update(&self.output_height.to_le_bytes());
        }
        hasher.finalize().to_hex().to_string()
    }

    pub fn recipe_value(recipe: &EditRecipe) -> Value {
        serde_json::to_value(recipe).expect("EditRecipe is serializable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn key_changes_for_every_render_input() {
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([("exposure".into(), 1.0)]),
            ..Default::default()
        };
        let key = RenderKey::new(
            "source",
            "decode-1",
            "pipeline-1",
            "vc",
            &recipe,
            vec!["mask".into()],
            "sRGB",
            10,
            20,
            "png",
        );
        let mut changed = key.clone();
        changed.output_width += 1;
        assert_ne!(key.digest(), changed.digest());
        changed = key.clone();
        changed.mask_artifact_hashes.push("other".into());
        assert_ne!(key.digest(), changed.digest());
        assert_ne!(
            key.recipe_hash,
            RenderKey::new(
                "source",
                "decode-1",
                "pipeline-1",
                "vc",
                &Default::default(),
                vec!["mask".into()],
                "sRGB",
                10,
                20,
                "png"
            )
            .recipe_hash
        );
    }
    #[test]
    fn pipeline_order_and_formats_are_explicit() {
        let pipeline = Pipeline::default();
        let stages = pipeline.stages();
        assert_eq!(
            stages
                .iter()
                .map(|(stage, _, _)| *stage)
                .collect::<Vec<_>>(),
            vec![
                PipelineStage::Decode,
                PipelineStage::SourceActions,
                PipelineStage::AutoAnalysis,
                PipelineStage::Adjustments,
                PipelineStage::Masks,
                PipelineStage::Crop,
                PipelineStage::Output,
            ]
        );
        assert!(pipeline.validate());
    }

    #[test]
    fn downstream_recipe_options_do_not_change_decode_or_mask_digest() {
        let mut first_recipe = EditRecipe::default();
        first_recipe
            .options
            .insert("crop".into(), "original".into());
        first_recipe
            .options
            .insert("output".into(), "100x100".into());
        let mut second_recipe = first_recipe.clone();
        second_recipe.options.insert("crop".into(), "square".into());
        second_recipe
            .options
            .insert("output".into(), "200x200".into());
        let first = RenderKey::new(
            "source",
            "decode",
            "pipeline",
            "vc",
            &first_recipe,
            vec![],
            "srgb",
            100,
            100,
            "png",
        );
        let second = RenderKey::new(
            "source",
            "decode",
            "pipeline",
            "vc",
            &second_recipe,
            vec![],
            "srgb",
            200,
            200,
            "png",
        );
        assert_eq!(
            first.stage_digest(crate::cache::CacheStage::Decode),
            second.stage_digest(crate::cache::CacheStage::Decode)
        );
        assert_eq!(
            first.stage_digest(crate::cache::CacheStage::Mask),
            second.stage_digest(crate::cache::CacheStage::Mask)
        );
        assert_ne!(first.digest(), second.digest());
    }

    #[test]
    fn geometry_changes_render_but_not_source_sized_mask_digest() {
        let base = EditRecipe::default();
        let mut cropped = base.clone();
        cropped.geometry = Some(lumina_sidecar::Geometry {
            version: 1,
            crop: Some(lumina_sidecar::Crop::Aspect {
                preset: lumina_sidecar::AspectPreset::OneToOne,
            }),
            rotation_degrees: 90.0,
            mirror_horizontal: true,
            mirror_vertical: false,
        });
        let first = RenderKey::new(
            "source",
            "decode",
            "pipeline",
            "vc",
            &base,
            vec![],
            "srgb",
            10,
            20,
            "png",
        );
        let second = RenderKey::new(
            "source",
            "decode",
            "pipeline",
            "vc",
            &cropped,
            vec![],
            "srgb",
            20,
            10,
            "png",
        );
        assert_eq!(
            first.stage_digest(crate::cache::CacheStage::Decode),
            second.stage_digest(crate::cache::CacheStage::Decode)
        );
        assert_eq!(
            first.stage_digest(crate::cache::CacheStage::Mask),
            second.stage_digest(crate::cache::CacheStage::Mask)
        );
        assert_ne!(first.digest(), second.digest());
    }

    #[test]
    fn sharpening_render_scale_changes_render_only() {
        let recipe = EditRecipe {
            sharpening: Some(lumina_sidecar::Sharpening {
                version: 1,
                amount: 1.0,
                radius: 2.0,
                detail: 0.5,
                masking: 0.0,
            }),
            ..Default::default()
        };
        let small = RenderKey::new(
            "source",
            "decode",
            "pipeline",
            "vc",
            &recipe,
            vec![],
            "srgb",
            100,
            100,
            "png",
        );
        let large = RenderKey::new(
            "source",
            "decode",
            "pipeline",
            "vc",
            &recipe,
            vec![],
            "srgb",
            200,
            200,
            "png",
        );
        assert_eq!(
            small.stage_digest(crate::cache::CacheStage::Decode),
            large.stage_digest(crate::cache::CacheStage::Decode)
        );
        assert_eq!(
            small.stage_digest(crate::cache::CacheStage::Mask),
            large.stage_digest(crate::cache::CacheStage::Mask)
        );
        assert_ne!(
            small.stage_digest(crate::cache::CacheStage::Preview),
            large.stage_digest(crate::cache::CacheStage::Preview)
        );
    }

    #[test]
    fn lens_and_perspective_change_render_not_decode_or_mask_digest() {
        let base = EditRecipe::default();
        let mut changed = base.clone();
        changed.lens_correction = Some(lumina_sidecar::LensCorrection {
            version: 1,
            profile: Some("wide-light".into()),
            distortion_k1: Some(0.0),
            distortion_k2: Some(0.0),
            distortion_k3: Some(0.0),
            vignette_c0: Some(1.0),
            vignette_c1: Some(0.0),
            vignette_c2: Some(0.0),
            ca_red: Some(0.0),
            ca_blue: Some(0.0),
        });
        changed.perspective = Some(lumina_sidecar::Perspective {
            version: 1,
            vertical: 0.2,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        });
        let a = RenderKey::new("s", "d", "p", "v", &base, vec![], "sRGB", 10, 10, "png");
        let b = RenderKey::new("s", "d", "p", "v", &changed, vec![], "sRGB", 10, 10, "png");
        assert_eq!(
            a.stage_digest(crate::cache::CacheStage::Decode),
            b.stage_digest(crate::cache::CacheStage::Decode)
        );
        assert_eq!(
            a.stage_digest(crate::cache::CacheStage::Mask),
            b.stage_digest(crate::cache::CacheStage::Mask)
        );
        assert_ne!(a.digest(), b.digest());
    }
}
