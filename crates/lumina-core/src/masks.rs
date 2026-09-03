//! Portable evaluation of the validated sidecar mask DAG.

use lumina_sidecar::{
    BrushMarkSign, MaskDefinition, MaskOperation, MaskPrompt, MaskReference, VirtualCopy,
};
use std::cell::Cell;
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
    #[error("mask rasterization of {required} pixels exceeds the memory budget limit {limit}")]
    MemoryBudgetExceeded { required: u64, limit: u64 },
    #[error("mask layer density must be finite and in 0..=1, got {value}")]
    InvalidDensity { value: String },
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
    /// REVIEW-MASK-N1: number of node bodies actually executed by the most
    /// recent [`MaskGraph::evaluate`] call. Shared subtrees execute once and
    /// are served from the per-call memo; this counter makes that observable
    /// (tests, diagnostics) without changing evaluation semantics.
    evaluated_node_count: Cell<usize>,
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
            evaluated_node_count: Cell::new(0),
        }
    }

    /// Diagnostics for [`REVIEW-MASK-N1`]: how many node bodies the last
    /// [`Self::evaluate`] call actually computed. With a DAG of shared
    /// subtrees this stays at the number of *distinct* nodes instead of
    /// growing exponentially with the number of sharing paths.
    pub fn last_evaluated_node_count(&self) -> usize {
        self.evaluated_node_count.get()
    }

    pub fn evaluate(&self, root: &MaskReference) -> Result<MaskPlane, MaskError> {
        // REVIEW-MASK-N1: memoize successful subtree results **per evaluate
        // call**. A plane is a pure function of its `(copy_id, mask_id)`
        // definition and the loaded sources, so caching `Ok` results inside
        // one traversal is sound (a memo hit can never be on the current
        // cycle-detection stack: only completed nodes are cached). Scoping
        // the memo to a single `evaluate` invocation keeps every call fully
        // deterministic and independent of prior calls.
        self.evaluated_node_count.set(0);
        let mut memo = BTreeMap::new();
        let plane = self.evaluate_node(
            &(root.copy_id.clone(), root.mask_id.clone()),
            &mut Vec::new(),
            &mut memo,
        )?;
        Ok(plane)
    }

    fn evaluate_node(
        &self,
        key: &(String, String),
        stack: &mut Vec<(String, String)>,
        memo: &mut BTreeMap<(String, String), MaskPlane>,
    ) -> Result<MaskPlane, MaskError> {
        if let Some(plane) = memo.get(key) {
            return Ok(plane.clone());
        }
        self.evaluated_node_count
            .set(self.evaluated_node_count.get() + 1);
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
                    // A prompt-source node can be evaluated without a model.
                    // If an inferred (loaded) plane exists it takes precedence
                    // (the matte can be recomputed and the prompt stays);
                    // otherwise fall back to the deterministic, model-free
                    // geometric rasterizer (F-079).
                    if let Some(prompt) = &definition.prompt {
                        if let Some(plane) = self.sources.get(&(key.0.clone(), key.1.clone())) {
                            return Ok(plane.clone());
                        }
                        let width = definition.geometry_context.width;
                        let height = definition.geometry_context.height;
                        return rasterize_prompt(prompt, width, height);
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
                    let mut plane =
                        self.evaluate_reference(&definition.references[0], stack, memo)?;
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
                    let mut result =
                        self.evaluate_reference(&definition.references[0], stack, memo)?;
                    for reference in &definition.references[1..] {
                        let other = self.evaluate_reference(reference, stack, memo)?;
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
        match result {
            Ok(plane) => {
                memo.insert(key.clone(), plane.clone());
                Ok(plane)
            }
            Err(error) => Err(error),
        }
    }

    fn evaluate_reference(
        &self,
        reference: &MaskReference,
        stack: &mut Vec<(String, String)>,
        memo: &mut BTreeMap<(String, String), MaskPlane>,
    ) -> Result<MaskPlane, MaskError> {
        self.evaluate_node(
            &(reference.copy_id.clone(), reference.mask_id.clone()),
            stack,
            memo,
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

/// Deterministic, model-free geometric rasterization of a user-guided prompt
/// source (F-079) into a `u16` mask plane at the requested dimensions.
///
/// The result is a pure function of the prompt and the dimensions: no RNG, no
/// wall-clock, no address dependence, so two calls with identical inputs always
/// produce byte-identical output. This is the geometric fallback matte used
/// when no inferred (model) plane is loaded; SAM 2 / network inference is a
/// separate concern (F-082).
pub fn rasterize_prompt(
    prompt: &MaskPrompt,
    width: u32,
    height: u32,
) -> Result<MaskPlane, MaskError> {
    // REVIEW-MASK-ZERO-1: a zero-dimension target would panic downstream
    // (`chunks_exact_mut(0)` in the per-prompt rasterizers) and can never
    // yield a meaningful matte. Reject it explicitly instead.
    if width == 0 || height == 0 {
        return Err(MaskError::InvalidPlane {
            width,
            height,
            length: 0,
        });
    }
    let w = width as usize;
    let h = height as usize;
    let budget = crate::memory::MemoryBudget::from_env();
    let required = budget
        .check_mask(width as u64, height as u64)
        .map_err(|error| MaskError::MemoryBudgetExceeded {
            required: error.required(),
            limit: error.limit(),
        })?;
    let _ = required;
    let mut values = vec![0u16; w.saturating_mul(h)];
    match prompt {
        MaskPrompt::Box { rect, .. } => {
            // Normalized [0,1] -> pixel; paint the inclusive rectangle interior
            // (hard edges are fine and deterministic).
            let x0 = (rect.x * width as f32).floor() as i64;
            let y0 = (rect.y * height as f32).floor() as i64;
            let x1 = ((rect.x + rect.width) * width as f32).ceil() as i64;
            let y1 = ((rect.y + rect.height) * height as f32).ceil() as i64;
            for y in y0.max(0)..y1.min(h as i64) {
                for x in x0.max(0)..x1.min(w as i64) {
                    values[y as usize * w + x as usize] = u16::MAX;
                }
            }
        }
        MaskPrompt::Ellipse { center, radii, .. } => {
            for (y, row) in values.chunks_exact_mut(w).enumerate() {
                let ny = (y as f32 + 0.5) / height as f32;
                for (x, pixel) in row.iter_mut().enumerate() {
                    let nx = (x as f32 + 0.5) / width as f32;
                    let dx = (nx - center.x) / radii.x;
                    let dy = (ny - center.y) / radii.y;
                    if dx * dx + dy * dy <= 1.0 {
                        *pixel = u16::MAX;
                    }
                }
            }
        }
        MaskPrompt::Polygon { points, .. } => {
            for (y, row) in values.chunks_exact_mut(w).enumerate() {
                let ny = (y as f32 + 0.5) / height as f32;
                for (x, pixel) in row.iter_mut().enumerate() {
                    let nx = (x as f32 + 0.5) / width as f32;
                    if point_in_polygon(nx, ny, points) {
                        *pixel = u16::MAX;
                    }
                }
            }
        }
        MaskPrompt::Gradient {
            angle_deg,
            start,
            end,
            ..
        } => {
            let rad = angle_deg.to_radians();
            let (dx, dy) = (rad.cos(), rad.sin());
            // Normalize the projection parameter `t` to [0,1] across the image
            // using the four extreme pixel centres, so the outermost columns
            // (or corners for diagonal angles) map exactly to `start`/`end`.
            let mut s_min = f32::INFINITY;
            let mut s_max = f32::NEG_INFINITY;
            for (cx, cy) in [
                (0.5 / width as f32, 0.5 / height as f32),
                (1.0 - 0.5 / width as f32, 0.5 / height as f32),
                (0.5 / width as f32, 1.0 - 0.5 / height as f32),
                (1.0 - 0.5 / width as f32, 1.0 - 0.5 / height as f32),
            ] {
                let s = cx * dx + cy * dy;
                s_min = s_min.min(s);
                s_max = s_max.max(s);
            }
            let span = (s_max - s_min).max(f32::EPSILON);
            for (y, row) in values.chunks_exact_mut(w).enumerate() {
                let ny = (y as f32 + 0.5) / height as f32;
                for (x, pixel) in row.iter_mut().enumerate() {
                    let nx = (x as f32 + 0.5) / width as f32;
                    let s = nx * dx + ny * dy;
                    let t = ((s - s_min) / span).clamp(0.0, 1.0);
                    let g = (start + t * (end - start)).clamp(0.0, 1.0);
                    *pixel = (g * (u16::MAX as f32) + 0.5) as u16;
                }
            }
        }
        MaskPrompt::Brush { marks, .. } => {
            // Start at zero; paint marks in order so later marks override
            // earlier ones (a negative mark erases a positive one).
            for (y, row) in values.chunks_exact_mut(w).enumerate() {
                let ny = (y as f32 + 0.5) / height as f32;
                for (x, pixel) in row.iter_mut().enumerate() {
                    let nx = (x as f32 + 0.5) / width as f32;
                    let mut value = 0u16;
                    for mark in marks {
                        let ddx = nx - mark.x;
                        let ddy = ny - mark.y;
                        if ddx * ddx + ddy * ddy <= mark.radius * mark.radius {
                            value = if matches!(mark.sign, BrushMarkSign::Positive) {
                                u16::MAX
                            } else {
                                0
                            };
                        }
                    }
                    *pixel = value;
                }
            }
        }
    }
    MaskPlane::new(width, height, values)
}

/// Even-odd point-in-polygon test (classic PNPOLY), all coordinates
/// normalized to `0..=1`.
fn point_in_polygon(px: f32, py: f32, points: &[lumina_sidecar::Point2]) -> bool {
    let mut inside = false;
    let n = points.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        let j = if i == 0 { n - 1 } else { i - 1 };
        let (xi, yi) = (points[i].x, points[i].y);
        let (xj, yj) = (points[j].x, points[j].y);
        let intersects = ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi);
        if intersects {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::{
        BrushMark, BrushMarkSign, CoordinateSystem, DecodeFingerprint, Extras, GeometryFingerprint,
        MaskPrompt, MaskStatus, ModelIdentity, NormalizedRect, Point2, Preprocessing,
        PromptTransform, Resolution, SourceFingerprint,
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
            prompt: None,
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
            rating: 0,
            flag: lumina_sidecar::Flag::Unflagged,
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
            rating: 0,
            flag: lumina_sidecar::Flag::Unflagged,
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
            rating: 0,
            flag: lumina_sidecar::Flag::Unflagged,
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
            rating: 0,
            flag: lumina_sidecar::Flag::Unflagged,
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

    // =====================================================================
    // F-079: deterministic geometric rasterization of prompt sources.
    // =====================================================================

    #[test]
    fn rasterize_box_fills_rectangle_interior() {
        let prompt = MaskPrompt::Box {
            rect: NormalizedRect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            transformation: PromptTransform::default(),
        };
        let plane = rasterize_prompt(&prompt, 4, 4).unwrap();
        assert_eq!((plane.width, plane.height), (4, 4));
        // Outside corners are zero.
        assert_eq!(plane.values[0], 0);
        assert_eq!(plane.values[15], 0);
        // The central 2x2 block (x,y in {1,2}) is fully on.
        for idx in [5, 6, 9, 10] {
            assert_eq!(plane.values[idx], u16::MAX, "box interior at {idx}");
        }
    }

    #[test]
    fn rasterize_ellipse_fills_interior() {
        let prompt = MaskPrompt::Ellipse {
            center: Point2 { x: 0.5, y: 0.5 },
            radii: Point2 { x: 0.4, y: 0.4 },
            transformation: PromptTransform::default(),
        };
        let plane = rasterize_prompt(&prompt, 16, 16).unwrap();
        // Centre pixel inside the ellipse.
        assert_eq!(plane.values[8 * 16 + 8], u16::MAX);
        // Far corner outside the ellipse.
        assert_eq!(plane.values[0], 0);
    }

    #[test]
    fn rasterize_polygon_fills_interior() {
        let prompt = MaskPrompt::Polygon {
            points: vec![
                Point2 { x: 0.0, y: 1.0 },
                Point2 { x: 1.0, y: 1.0 },
                Point2 { x: 0.5, y: 0.0 },
            ],
            transformation: PromptTransform::default(),
        };
        let plane = rasterize_prompt(&prompt, 10, 10).unwrap();
        // Bottom-left corner sits inside the upward triangle.
        assert_eq!(plane.values[9 * 10], u16::MAX);
        // Top-centre near the apex is outside.
        assert_eq!(plane.values[5], 0);
    }

    #[test]
    fn rasterize_gradient_is_monotonic_and_maps_endpoints() {
        let prompt = MaskPrompt::Gradient {
            angle_deg: 0.0,
            start: 0.0,
            end: 1.0,
            transformation: PromptTransform::default(),
        };
        let width = 8u32;
        let plane = rasterize_prompt(&prompt, width, 1).unwrap();
        // Leftmost/rightmost columns map to the endpoints.
        assert_eq!(plane.values[0], 0);
        assert_eq!(plane.values[(width - 1) as usize], u16::MAX);
        // Monotonic non-decreasing left to right.
        for i in 1..width as usize {
            assert!(
                plane.values[i] >= plane.values[i - 1],
                "gradient not monotonic at {i}: {} < {}",
                plane.values[i],
                plane.values[i - 1]
            );
        }
        // Reversed gradient is monotonic non-increasing.
        let reversed = MaskPrompt::Gradient {
            angle_deg: 0.0,
            start: 1.0,
            end: 0.0,
            transformation: PromptTransform::default(),
        };
        let plane = rasterize_prompt(&reversed, width, 1).unwrap();
        assert_eq!(plane.values[0], u16::MAX);
        assert_eq!(plane.values[(width - 1) as usize], 0);
        for i in 1..width as usize {
            assert!(plane.values[i] <= plane.values[i - 1]);
        }
    }

    #[test]
    fn rasterize_brush_paints_positive_and_erases_negative() {
        let prompt = MaskPrompt::Brush {
            marks: vec![
                BrushMark {
                    x: 0.5,
                    y: 0.5,
                    radius: 0.4,
                    sign: BrushMarkSign::Positive,
                },
                BrushMark {
                    x: 0.2,
                    y: 0.5,
                    radius: 0.2,
                    sign: BrushMarkSign::Negative,
                },
            ],
            resolution: (32, 32),
            transformation: PromptTransform::default(),
        };
        let plane = rasterize_prompt(&prompt, 32, 32).unwrap();
        // Centre is covered by the positive mark -> max.
        assert_eq!(plane.values[16 * 32 + 16], u16::MAX);
        // Far corner is untouched -> zero.
        assert_eq!(plane.values[0], 0);
        // A pixel near the negative mark (left side) is erased to zero even
        // though it would be inside the large positive disk.
        let negative_pixel = (16usize) * 32 + (6usize); // normalized x ~0.2
        assert_eq!(plane.values[negative_pixel], 0);
    }

    #[test]
    fn rasterize_prompt_is_deterministic() {
        let prompt = MaskPrompt::Gradient {
            angle_deg: 30.0,
            start: 0.1,
            end: 0.9,
            transformation: PromptTransform::default(),
        };
        let a = rasterize_prompt(&prompt, 12, 7).unwrap();
        let b = rasterize_prompt(&prompt, 12, 7).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.values, b.values);
    }

    #[test]
    fn source_with_prompt_evaluates_geometric_matte_without_loaded_plane() {
        // A Source node carrying a Box prompt must evaluate to the deterministic
        // geometric matte even when no inferred (model) plane is loaded.
        let mut def = definition("box-source", MaskOperation::Source, vec![]);
        def.prompt = Some(MaskPrompt::Box {
            rect: NormalizedRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            transformation: PromptTransform::default(),
        });
        // geometry_context in the test helper is 2x1, so the full-image box
        // rasterizes to an all-max 2x1 plane.
        let graph = graph(vec![def], &[]);
        let plane = graph.evaluate(&reference("box-source")).unwrap();
        assert_eq!(plane.values, vec![u16::MAX, u16::MAX]);
    }

    #[test]
    fn source_with_prompt_and_loaded_plane_prefers_loaded_plane() {
        // When an inferred (loaded) plane is present for a prompt node, it takes
        // precedence: the matte can be recomputed and the user's prompt stays.
        let mut def = definition("box-source", MaskOperation::Source, vec![]);
        def.prompt = Some(MaskPrompt::Box {
            rect: NormalizedRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            transformation: PromptTransform::default(),
        });
        let graph = graph(vec![def], &[("box-source", vec![7, 11])]);
        assert_eq!(
            graph.evaluate(&reference("box-source")).unwrap().values,
            vec![7, 11]
        );
    }

    #[test]
    fn source_with_prompt_falls_back_when_no_loaded_plane() {
        let mut def = definition("ellipse-source", MaskOperation::Source, vec![]);
        def.prompt = Some(MaskPrompt::Ellipse {
            center: Point2 { x: 0.5, y: 0.5 },
            radii: Point2 { x: 0.5, y: 0.5 },
            transformation: PromptTransform::default(),
        });
        let graph = graph(vec![def], &[]);
        let plane = graph.evaluate(&reference("ellipse-source")).unwrap();
        // Full-image ellipse -> all max.
        assert_eq!(plane.values, vec![u16::MAX, u16::MAX]);
    }

    // =====================================================================
    // REVIEW-MASK-ZERO-1: zero-dimension targets must be MaskErrors.
    // =====================================================================

    #[test]
    fn rasterize_prompt_rejects_zero_dimensions_with_mask_error() {
        let prompt = MaskPrompt::Box {
            rect: NormalizedRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            transformation: PromptTransform::default(),
        };
        for (width, height) in [(0u32, 5u32), (5, 0), (0, 0)] {
            assert_eq!(
                rasterize_prompt(&prompt, width, height),
                Err(MaskError::InvalidPlane {
                    width,
                    height,
                    length: 0
                }),
                "width={width} height={height}"
            );
        }
        // The guard runs before the per-prompt match: every prompt kind is
        // covered by the same rejection.
        let ellipse = MaskPrompt::Brush {
            marks: vec![],
            resolution: (4, 4),
            transformation: PromptTransform::default(),
        };
        assert!(matches!(
            rasterize_prompt(&ellipse, 0, 4),
            Err(MaskError::InvalidPlane { .. })
        ));
    }

    #[test]
    fn graph_evaluates_zero_dimension_prompt_source_as_error_not_panic() {
        let mut def = definition("zero-source", MaskOperation::Source, vec![]);
        def.prompt = Some(MaskPrompt::Box {
            rect: NormalizedRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            transformation: PromptTransform::default(),
        });
        def.geometry_context.width = 0;
        def.geometry_context.height = 0;
        let graph = graph(vec![def], &[]);
        assert!(matches!(
            graph.evaluate(&reference("zero-source")),
            Err(MaskError::InvalidPlane { .. })
        ));
    }

    // =====================================================================
    // REVIEW-MASK-N1: per-call memoization of shared subtrees.
    // =====================================================================

    #[test]
    fn memoized_dag_executes_each_shared_subtree_once() {
        // Diamond with a doubly-shared leaf:
        //   d = union(a, b); a = invert(s); b = invert(s)
        // Without memoization the leaf `s` executes twice (once under `a`,
        // once under `b`); with memoization every node body runs exactly
        // once and the result is unchanged.
        let refs = |ids: &[&str]| ids.iter().map(|id| reference(id)).collect();
        let graph = graph(
            vec![
                definition("s", MaskOperation::Source, vec![]),
                definition("a", MaskOperation::Invert, refs(&["s"])),
                definition("b", MaskOperation::Invert, refs(&["s"])),
                definition("d", MaskOperation::Union, refs(&["a", "b"])),
            ],
            &[("s", vec![100, 65_000])],
        );
        let plane = graph.evaluate(&reference("d")).unwrap();
        assert_eq!(
            plane.values,
            vec![u16::MAX - 100, u16::MAX - 65_000],
            "union of two identical inversions must equal one inversion"
        );
        assert_eq!(
            graph.last_evaluated_node_count(),
            4,
            "distinct nodes s/a/b/d must each execute exactly once"
        );
        // A second evaluation on the same graph is independent (per-call
        // memo) and executes the same number of node bodies again.
        let again = graph.evaluate(&reference("d")).unwrap();
        assert_eq!(again.values, plane.values);
        assert_eq!(graph.last_evaluated_node_count(), 4);
    }

    #[test]
    fn deep_sharing_stays_linear_under_memoization() {
        // A chain where every level references the SAME base mask twice:
        //   m0 = source; m{k} = union(m{k-1}, m{k-1})
        // Without memoization this is exponential in k; with memoization the
        // executed-node count is exactly k + 1 (m0..mk).
        let levels = 12;
        let mut definitions = vec![definition("m0", MaskOperation::Source, vec![])];
        for k in 1..=levels {
            definitions.push(definition(
                &format!("m{k}"),
                MaskOperation::Union,
                vec![
                    reference(&format!("m{}", k - 1)),
                    reference(&format!("m{}", k - 1)),
                ],
            ));
        }
        let payloads = [("m0", vec![1u16, 65_000])];
        let graph = graph(definitions, &payloads);
        let plane = graph.evaluate(&reference(&format!("m{levels}"))).unwrap();
        assert_eq!(plane.values, payloads[0].1);
        assert_eq!(
            graph.last_evaluated_node_count(),
            levels as usize + 1,
            "memoization must keep the doubly-referencing chain linear"
        );
    }
}
