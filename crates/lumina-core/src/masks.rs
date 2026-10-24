//! Portable evaluation of the validated sidecar mask DAG.

#[path = "brush_raster.rs"]
mod brush_raster;

use lumina_sidecar::{MaskDefinition, MaskOperation, MaskPrompt, MaskReference, VirtualCopy};
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
    #[error("mask prompt is not a deterministic range stage")]
    NotRangePrompt,
    #[error(
        "range prompt requires source pixels; evaluate via range_masks::evaluate_range_prompt"
    )]
    RangeNeedsPixels,
    #[error("invalid range mask parameters: {reason}")]
    InvalidRange { reason: String },
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
                    // G-03: an AI selection names an automatic matte. It is
                    // only usable from a loaded or inferred plane and never
                    // falls back to the geometric rasterizer — a missing model
                    // stays loudly missing instead of silently painting
                    // geometry.
                    if definition.ai_select.is_some() {
                        return self
                            .sources
                            .get(&(key.0.clone(), key.1.clone()))
                            .cloned()
                            .ok_or_else(|| MaskError::MissingSourcePayload {
                                copy_id: key.0.clone(),
                                mask_id: key.1.clone(),
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
                        // G-03: range stages need source pixels; the loader
                        // computes them from the frame. Direct graph evaluation
                        // without pixels is a loud error, never an empty mask.
                        if crate::range_masks::is_range_prompt(Some(prompt)) {
                            return Err(MaskError::RangeNeedsPixels);
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
            brush_raster::rasterize_brush_marks_into(marks, width, height, &mut values);
        }
        // G-03: range stages need source pixels and cannot be rasterized from
        // geometry alone — the loader evaluates them against the frame.
        MaskPrompt::ColorRange { .. } | MaskPrompt::LuminanceRange { .. } => {
            return Err(MaskError::RangeNeedsPixels);
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
mod tests;
