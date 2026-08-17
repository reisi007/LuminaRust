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
        Self {
            source_content_hash: source_content_hash.into(),
            decode_version: decode_version.into(),
            pipeline_version: pipeline_version.into(),
            virtual_copy_id: virtual_copy_id.into(),
            recipe_hash,
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
            hasher.update(self.recipe_hash.as_bytes());
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
        assert!(Pipeline::default().validate());
    }
}
