//! Generative canvas/edit value types of the persisted recipe vocabulary.
//!
//! Pure move out of `crates/lumina-sidecar/src/value_types.rs` (file-size
//! ceiling): the items below are byte-identical to their former place; only
//! this module documentation and the `use` block are new. `value_types.rs`
//! re-exports every name, so `lumina_sidecar::GenerativeEdit`,
//! `lumina_sidecar::GenerativeCanvas`, … keep their paths.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::invalid;
use crate::{Extras, GenerativeArtifactRef, SidecarError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerativeCanvas {
    pub output_width: u32,
    pub output_height: u32,
    pub source_offset_x: i32,
    pub source_offset_y: i32,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerativeEdit {
    pub version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<GenerativeCanvas>,
    /// GEN-ZDATA-LINK-1 (GEN-EXPAND-1 `generative_canvas`, zdata `kind = 2`):
    /// link to the full composited canvas record. Additive schema-v2 field;
    /// absent (`None`) means no persisted canvas yet (`missing`, never a
    /// silent fallback). Part of the recipe identity (see
    /// [`GenerativeArtifactRef`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<GenerativeArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_generative_content: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_fill_transparent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand_beyond_image: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

impl GenerativeEdit {
    pub fn effective_keep(&self) -> bool {
        self.keep_generative_content.unwrap_or(true)
    }
    pub fn effective_expand(&self) -> bool {
        self.expand_beyond_image.unwrap_or(false)
    }

    /// GEN-ONNX-1 Welle 2a: the persisted optional negative prompt.
    ///
    /// Additive schema field (top-level JSON `"negative_prompt"`). It is stored
    /// in the flattened [`Self::extras`] map rather than a typed struct field:
    /// a new field would break every `GenerativeEdit` struct literal outside
    /// this wave's scope (notably `lumina-gui`, Welle 2b). The JSON shape is
    /// still the additive top-level field the SOLL documents, and a present
    /// value that is neither a string nor `null` is rejected loudly by
    /// [`Self::validate_edit_extras`]/the document validator — never silently
    /// ignored.
    #[must_use]
    pub fn negative_prompt(&self) -> Option<&str> {
        self.extras
            .get(GENERATIVE_NEGATIVE_PROMPT_KEY)
            .and_then(Value::as_str)
    }

    /// Sets (or clears) the additive negative prompt.
    pub fn set_negative_prompt(&mut self, value: Option<String>) {
        match value {
            Some(value) => {
                self.extras.insert(
                    GENERATIVE_NEGATIVE_PROMPT_KEY.to_owned(),
                    Value::String(value),
                );
            }
            None => {
                self.extras.remove(GENERATIVE_NEGATIVE_PROMPT_KEY);
            }
        }
    }

    /// Loud validation of the additive extras-backed generative fields.
    ///
    /// `negative_prompt` must be a string or `null`; any other type is a hard
    /// error (F2 lesson: an unknown/mistyped field is never silently ignored).
    pub fn validate_edit_extras(&self) -> Result<(), SidecarError> {
        if let Some(value) = self.extras.get(GENERATIVE_NEGATIVE_PROMPT_KEY) {
            if !value.is_null() && !value.is_string() {
                return invalid("generative_edit.negative_prompt must be a string or null");
            }
        }
        Ok(())
    }
}

/// GEN-ONNX-1 Welle 2a: additive `GenerativeEdit` extras key of the optional
/// negative prompt (top-level JSON `"negative_prompt"`).
pub const GENERATIVE_NEGATIVE_PROMPT_KEY: &str = "negative_prompt";
impl GenerativeCanvas {
    pub fn validate_with_source(
        &self,
        source_width: u32,
        source_height: u32,
    ) -> Result<(), SidecarError> {
        self.validate()?;
        if self.output_width <= source_width && self.output_height <= source_height {
            return invalid("expand canvas must be larger than source (output_* > source_*)");
        }
        let ox = self.source_offset_x as i64;
        let oy = self.source_offset_y as i64;
        let sw = source_width as i64;
        let sh = source_height as i64;
        let ow = self.output_width as i64;
        let oh = self.output_height as i64;
        if ox + sw < 0 || oy + sh < 0 || ox >= ow || oy >= oh {
            return invalid("canvas source_offset out of bounds");
        }
        if ox + sw > ow || oy + sh > oh {
            return invalid("canvas source_offset out of bounds");
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.output_width == 0 || self.output_height == 0 {
            return invalid("generative canvas output dimensions must be > 0");
        }
        Ok(())
    }
}
