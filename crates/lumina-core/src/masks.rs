//! Portable evaluation of the validated sidecar mask DAG.

use lumina_sidecar::{MaskDefinition, MaskOperation, MaskReference, VirtualCopy};
use std::collections::BTreeMap;
use thiserror::Error;

const MAX: u32 = u16::MAX as u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskPlane {
    pub width: u32,
    pub height: u32,
    pub values: Vec<u16>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MaskError {
    #[error("invalid mask plane dimensions {width}x{height} for {length} values")]
    InvalidPlane {
        width: u32,
        height: u32,
        length: usize,
    },
    #[error("mask `{copy_id}/{mask_id}` has no source payload")]
    MissingSourcePayload { copy_id: String, mask_id: String },
    #[error("mask `{copy_id}/{mask_id}` is not defined")]
    MissingMask { copy_id: String, mask_id: String },
    #[error("mask graph contains a cycle at `{copy_id}/{mask_id}`")]
    Cycle { copy_id: String, mask_id: String },
    #[error("mask `{copy_id}/{mask_id}` operation has invalid input arity ({actual})")]
    InvalidArity {
        copy_id: String,
        mask_id: String,
        actual: usize,
    },
    #[error("mask dimensions differ: expected {expected_width}x{expected_height}, got {actual_width}x{actual_height}")]
    DimensionMismatch {
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
}

impl MaskPlane {
    pub fn new(width: u32, height: u32, values: Vec<u16>) -> Result<Self, MaskError> {
        let expected = (width as usize).saturating_mul(height as usize);
        if values.len() != expected {
            return Err(MaskError::InvalidPlane {
                width,
                height,
                length: values.len(),
            });
        }
        Ok(Self {
            width,
            height,
            values,
        })
    }
}

pub struct MaskGraph<'a> {
    definitions: BTreeMap<(String, String), &'a MaskDefinition>,
    sources: BTreeMap<(String, String), MaskPlane>,
}

impl<'a> MaskGraph<'a> {
    pub fn new(copies: &'a [VirtualCopy], sources: BTreeMap<(String, String), MaskPlane>) -> Self {
        let definitions = copies
            .iter()
            .flat_map(|copy| {
                copy.mask_library
                    .iter()
                    .map(move |mask| ((copy.id.clone(), mask.id.clone()), mask))
            })
            .collect();
        Self {
            definitions,
            sources,
        }
    }

    pub fn evaluate(&self, root: &MaskReference) -> Result<MaskPlane, MaskError> {
        self.evaluate_node(
            &(root.copy_id.clone(), root.mask_id.clone()),
            &mut Vec::new(),
        )
    }

    fn evaluate_node(
        &self,
        key: &(String, String),
        stack: &mut Vec<(String, String)>,
    ) -> Result<MaskPlane, MaskError> {
        let definition = self
            .definitions
            .get(key)
            .ok_or_else(|| MaskError::MissingMask {
                copy_id: key.0.clone(),
                mask_id: key.1.clone(),
            })?;
        if stack.contains(key) {
            return Err(MaskError::Cycle {
                copy_id: key.0.clone(),
                mask_id: key.1.clone(),
            });
        }
        stack.push(key.clone());
        let result = (|| {
            let count = definition.references.len();
            match definition.operation {
                MaskOperation::Source => {
                    if count != 0 {
                        return Err(MaskError::InvalidArity {
                            copy_id: key.0.clone(),
                            mask_id: key.1.clone(),
                            actual: count,
                        });
                    }
                    self.sources
                        .get(&(key.0.clone(), key.1.clone()))
                        .cloned()
                        .ok_or_else(|| MaskError::MissingSourcePayload {
                            copy_id: key.0.clone(),
                            mask_id: key.1.clone(),
                        })
                }
                MaskOperation::Invert => {
                    if count != 1 {
                        return Err(MaskError::InvalidArity {
                            copy_id: key.0.clone(),
                            mask_id: key.1.clone(),
                            actual: count,
                        });
                    }
                    let mut plane = self.evaluate_reference(&definition.references[0], stack)?;
                    plane
                        .values
                        .iter_mut()
                        .for_each(|value| *value = u16::MAX - *value);
                    Ok(plane)
                }
                operation => {
                    if (matches!(operation, MaskOperation::Subtract) && count != 2)
                        || (matches!(operation, MaskOperation::Union | MaskOperation::Intersect)
                            && count < 2)
                    {
                        return Err(MaskError::InvalidArity {
                            copy_id: key.0.clone(),
                            mask_id: key.1.clone(),
                            actual: count,
                        });
                    }
                    let mut result = self.evaluate_reference(&definition.references[0], stack)?;
                    for reference in &definition.references[1..] {
                        let other = self.evaluate_reference(reference, stack)?;
                        ensure_dimensions(&result, &other)?;
                        match operation {
                            MaskOperation::Union => result
                                .values
                                .iter_mut()
                                .zip(other.values)
                                .for_each(|(a, b)| *a = (*a).max(b)),
                            MaskOperation::Intersect => result
                                .values
                                .iter_mut()
                                .zip(other.values)
                                .for_each(|(a, b)| *a = (*a).min(b)),
                            MaskOperation::Subtract => result
                                .values
                                .iter_mut()
                                .zip(other.values)
                                .for_each(|(a, b)| {
                                    *a = (((*a as u32) * (MAX - b as u32) + MAX / 2) / MAX) as u16
                                }),
                            _ => unreachable!(),
                        }
                    }
                    Ok(result)
                }
            }
        })();
        stack.pop();
        result
    }

    fn evaluate_reference(
        &self,
        reference: &MaskReference,
        stack: &mut Vec<(String, String)>,
    ) -> Result<MaskPlane, MaskError> {
        self.evaluate_node(
            &(reference.copy_id.clone(), reference.mask_id.clone()),
            stack,
        )
    }
}

fn ensure_dimensions(expected: &MaskPlane, actual: &MaskPlane) -> Result<(), MaskError> {
    if (expected.width, expected.height) != (actual.width, actual.height) {
        Err(MaskError::DimensionMismatch {
            expected_width: expected.width,
            expected_height: expected.height,
            actual_width: actual.width,
            actual_height: actual.height,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::{
        CoordinateSystem, DecodeFingerprint, Extras, GeometryFingerprint, MaskStatus,
        ModelIdentity, Preprocessing, Resolution, SourceFingerprint,
    };

    fn definition(
        id: &str,
        operation: MaskOperation,
        references: Vec<MaskReference>,
    ) -> MaskDefinition {
        MaskDefinition {
            id: id.into(),
            name: id.into(),
            source_fingerprint: SourceFingerprint {
                content_hash: "h".into(),
                byte_length: 1,
                extras: Extras::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "d".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            geometry_context: GeometryFingerprint {
                width: 2,
                height: 1,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            model: ModelIdentity {
                name: "m".into(),
                version: "1".into(),
                hash: "h".into(),
                extras: Extras::new(),
            },
            inference_resolution: Resolution {
                width: 2,
                height: 1,
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
            operation,
            references,
            extras: Extras::new(),
        }
    }
    fn reference(id: &str) -> MaskReference {
        MaskReference {
            copy_id: "vc".into(),
            mask_id: id.into(),
            extras: Extras::new(),
        }
    }
    fn graph(definitions: Vec<MaskDefinition>, sources: &[(&str, Vec<u16>)]) -> MaskGraph<'static> {
        let copy = Box::leak(Box::new(VirtualCopy {
            id: "vc".into(),
            name: "VC".into(),
            is_default: true,
            recipe: Default::default(),
            mask_library: definitions,
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        }));
        let payloads = sources
            .iter()
            .map(|(id, values)| {
                (
                    ("vc".into(), (*id).into()),
                    MaskPlane::new(2, 1, values.clone()).unwrap(),
                )
            })
            .collect();
        MaskGraph::new(std::slice::from_ref(copy), payloads)
    }

    #[test]
    fn evaluates_all_operations_and_subtract_rounding() {
        let refs = |ids: &[&str]| ids.iter().map(|id| reference(id)).collect();
        let graph = graph(
            vec![
                definition("a", MaskOperation::Source, vec![]),
                definition("b", MaskOperation::Source, vec![]),
                definition("u", MaskOperation::Union, refs(&["a", "b"])),
                definition("i", MaskOperation::Intersect, refs(&["a", "b"])),
                definition("s", MaskOperation::Subtract, refs(&["a", "b"])),
                definition("n", MaskOperation::Invert, refs(&["a"])),
            ],
            &[("a", vec![0, 32768]), ("b", vec![65535, 16384])],
        );
        assert_eq!(
            graph.evaluate(&reference("u")).unwrap().values,
            vec![65535, 32768]
        );
        assert_eq!(
            graph.evaluate(&reference("i")).unwrap().values,
            vec![0, 16384]
        );
        assert_eq!(
            graph.evaluate(&reference("s")).unwrap().values,
            vec![0, 24576]
        );
        assert_eq!(
            graph.evaluate(&reference("n")).unwrap().values,
            vec![65535, 32767]
        );
    }

    #[test]
    fn resolves_cross_copy_and_reports_failures() {
        let source = definition("source", MaskOperation::Source, vec![]);
        let mut derived = definition(
            "derived",
            MaskOperation::Invert,
            vec![MaskReference {
                copy_id: "other".into(),
                mask_id: "source".into(),
                extras: Extras::new(),
            }],
        );
        derived.id = "derived".into();
        let other = Box::leak(Box::new(VirtualCopy {
            id: "other".into(),
            name: "Other".into(),
            is_default: false,
            recipe: Default::default(),
            mask_library: vec![source],
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        }));
        let target = Box::leak(Box::new(VirtualCopy {
            id: "vc".into(),
            name: "VC".into(),
            is_default: true,
            recipe: Default::default(),
            mask_library: vec![derived],
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        }));
        let copies = Box::leak(Box::new(vec![(*target).clone(), (*other).clone()]));
        let graph = MaskGraph::new(
            copies,
            BTreeMap::from([(
                ("other".into(), "source".into()),
                MaskPlane::new(1, 1, vec![7]).unwrap(),
            )]),
        );
        assert_eq!(
            graph.evaluate(&reference("derived")).unwrap().values,
            vec![65528]
        );
        assert!(matches!(
            MaskPlane::new(2, 1, vec![1]),
            Err(MaskError::InvalidPlane { .. })
        ));
        assert!(matches!(
            MaskGraph::new(std::slice::from_ref(target), BTreeMap::new())
                .evaluate(&reference("derived")),
            Err(MaskError::MissingMask { .. })
        ));
    }

    #[test]
    fn reports_missing_source_payload() {
        let graph = graph(
            vec![definition("source", MaskOperation::Source, vec![])],
            &[],
        );

        assert_eq!(
            graph.evaluate(&reference("source")),
            Err(MaskError::MissingSourcePayload {
                copy_id: "vc".into(),
                mask_id: "source".into(),
            })
        );
    }

    #[test]
    fn detects_cycle_during_evaluation() {
        let refs = |id: &str| vec![reference(id)];
        let graph = graph(
            vec![
                definition("a", MaskOperation::Invert, refs("b")),
                definition("b", MaskOperation::Invert, refs("a")),
            ],
            &[],
        );

        assert_eq!(
            graph.evaluate(&reference("a")),
            Err(MaskError::Cycle {
                copy_id: "vc".into(),
                mask_id: "a".into(),
            })
        );
    }

    #[test]
    fn reports_dimensions_of_referenced_masks() {
        let mut definitions = vec![
            definition("a", MaskOperation::Source, vec![]),
            definition("b", MaskOperation::Source, vec![]),
            definition(
                "union",
                MaskOperation::Union,
                vec![reference("a"), reference("b")],
            ),
        ];
        let copy = Box::leak(Box::new(VirtualCopy {
            id: "vc".into(),
            name: "VC".into(),
            is_default: true,
            recipe: Default::default(),
            mask_library: std::mem::take(&mut definitions),
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        }));
        let graph = MaskGraph::new(
            std::slice::from_ref(copy),
            BTreeMap::from([
                (
                    ("vc".into(), "a".into()),
                    MaskPlane::new(2, 1, vec![1, 2]).unwrap(),
                ),
                (
                    ("vc".into(), "b".into()),
                    MaskPlane::new(1, 2, vec![3, 4]).unwrap(),
                ),
            ]),
        );

        assert_eq!(
            graph.evaluate(&reference("union")),
            Err(MaskError::DimensionMismatch {
                expected_width: 2,
                expected_height: 1,
                actual_width: 1,
                actual_height: 2,
            })
        );
    }

    #[test]
    fn rejects_invalid_arity_during_evaluation() {
        let graph = graph(
            vec![definition(
                "subtract",
                MaskOperation::Subtract,
                vec![reference("a")],
            )],
            &[],
        );

        assert_eq!(
            graph.evaluate(&reference("subtract")),
            Err(MaskError::InvalidArity {
                copy_id: "vc".into(),
                mask_id: "subtract".into(),
                actual: 1,
            })
        );
    }

    #[test]
    fn subtract_handles_u16_boundaries_without_overflow() {
        let graph = graph(
            vec![
                definition("a", MaskOperation::Source, vec![]),
                definition("b", MaskOperation::Source, vec![]),
                definition(
                    "subtract",
                    MaskOperation::Subtract,
                    vec![reference("a"), reference("b")],
                ),
            ],
            &[("a", vec![0, u16::MAX]), ("b", vec![u16::MAX, 0])],
        );

        assert_eq!(
            graph.evaluate(&reference("subtract")).unwrap().values,
            vec![0, u16::MAX]
        );
    }
}
