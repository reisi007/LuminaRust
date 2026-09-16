//! Duplicate / series similarity within one explicit selection.
//!
//! The comparison is **only** ever over the signatures the caller passed in a
//! single [`group_similar`] call — this crate never scans a directory, never
//! expands a selection and never groups across folders automatically
//! (decisions §2/§9). The signature is content-only (dHash + luminance
//! histogram), so ordering is deterministic and no wall clock/file path enters.

use lumina_core::ImageFrame;
use serde::{Deserialize, Serialize};

use crate::config::CullConfig;
use crate::signals::{luma_plane, LumaPlane};
use crate::CullError;

/// Number of luminance bins in the similarity histogram.
pub const SIMILARITY_HISTOGRAM_BINS: usize = 32;
/// Target width of the dHash grid (9 columns → 8 horizontal comparisons).
const DHASH_WIDTH: u32 = 9;
/// Target height of the dHash grid (8 rows → 64 bits).
const DHASH_HEIGHT: u32 = 8;

/// Content-only similarity signature of one image.
#[derive(Debug, Clone, PartialEq)]
pub struct SimilaritySignature {
    /// 64-bit difference hash (row-major `y * 8 + x`).
    pub perceptual_hash: u64,
    /// Normalized luminance histogram (fractions, sum `≈ 1`).
    pub histogram: [f32; SIMILARITY_HISTOGRAM_BINS],
    /// Number of luminance samples used.
    pub sample_count: u64,
}

impl SimilaritySignature {
    /// The zero signature of a frame with no pixels (not produced for empty
    /// frames; used only as a neutral value).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            perceptual_hash: 0,
            histogram: [0.0; SIMILARITY_HISTOGRAM_BINS],
            sample_count: 0,
        }
    }
}

/// One image offered to [`group_similar`].
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarityCandidate {
    /// Content-only signature.
    pub signature: SimilaritySignature,
    /// Intrinsic keep-worthiness used **only** to pick a group's best member.
    pub intrinsic_score: f64,
}

/// Whether a group is near-identical or merely a burst/series resemblance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimilarityKind {
    /// Near-duplicate (tight hash and histogram thresholds).
    Duplicate,
    /// Series/burst resemblance (looser thresholds).
    Series,
}

/// A non-singleton similarity group over the explicit selection. `members`
/// holds selection indices in ascending order; `best` is the member kept as
/// the group's representative (highest `intrinsic_score`, lowest index on a
/// tie).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimilarityGroup {
    /// Ascending selection indices of the group members.
    pub members: Vec<usize>,
    /// Group severity.
    pub kind: SimilarityKind,
    /// Representative member index.
    pub best: usize,
}

/// Hamming distance between two 64-bit perceptual hashes.
#[must_use]
pub fn hash_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Total-variation distance (`0.5 * L1`) between two normalized histograms in
/// `0..=1`. `0` = identical distributions.
#[must_use]
pub fn histogram_distance(
    a: &[f32; SIMILARITY_HISTOGRAM_BINS],
    b: &[f32; SIMILARITY_HISTOGRAM_BINS],
) -> f64 {
    0.5 * a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (f64::from(*x) - f64::from(*y)).abs())
        .sum::<f64>()
}

/// 64-bit difference hash of a frame (downscaled to a 9x8 luminance grid).
pub fn perceptual_hash(frame: &ImageFrame) -> Result<u64, CullError> {
    let plane = luma_plane(frame)?;
    Ok(perceptual_hash_of_plane(&plane))
}

fn perceptual_hash_of_plane(plane: &LumaPlane) -> u64 {
    let grid = resize_box(plane, DHASH_WIDTH, DHASH_HEIGHT);
    let mut hash = 0u64;
    for y in 0..DHASH_HEIGHT {
        for x in 0..DHASH_WIDTH - 1 {
            let left = grid[(y * DHASH_WIDTH + x) as usize];
            let right = grid[(y * DHASH_WIDTH + x + 1) as usize];
            if left > right {
                hash |= 1u64 << (y * (DHASH_WIDTH - 1) + x);
            }
        }
    }
    hash
}

/// Full content-only signature of a frame.
pub fn similarity_signature(frame: &ImageFrame) -> Result<SimilaritySignature, CullError> {
    let plane = luma_plane(frame)?;
    let mut histogram = [0u32; SIMILARITY_HISTOGRAM_BINS];
    for value in &plane.values {
        let bin = ((*value as f64) * SIMILARITY_HISTOGRAM_BINS as f64) as usize;
        histogram[bin.min(SIMILARITY_HISTOGRAM_BINS - 1)] += 1;
    }
    let sample_count: u64 = histogram.iter().map(|count| u64::from(*count)).sum();
    let histogram = if sample_count == 0 {
        [0.0; SIMILARITY_HISTOGRAM_BINS]
    } else {
        let mut normalized = [0f32; SIMILARITY_HISTOGRAM_BINS];
        for (index, count) in histogram.iter().enumerate() {
            normalized[index] = *count as f32 / sample_count as f32;
        }
        normalized
    };
    Ok(SimilaritySignature {
        perceptual_hash: perceptual_hash_of_plane(&plane),
        histogram,
        sample_count,
    })
}

/// Groups near-duplicate/series images within the explicit selection.
///
/// Deterministic: pairs are visited in ascending `(i, j)` order, components
/// are emitted by ascending first member, and the group kind is the most
/// severe edge in the component (`Duplicate` wins over `Series`).
#[must_use]
pub fn group_similar(
    candidates: &[SimilarityCandidate],
    config: &CullConfig,
) -> Vec<SimilarityGroup> {
    let count = candidates.len();
    if count < 2 {
        return Vec::new();
    }
    let mut union = UnionFind::new(count);
    let mut duplicate_edges: Vec<(usize, usize)> = Vec::new();

    for i in 0..count {
        for j in (i + 1)..count {
            let a = &candidates[i].signature;
            let b = &candidates[j].signature;
            let hash = hash_distance(a.perceptual_hash, b.perceptual_hash);
            let histogram = histogram_distance(&a.histogram, &b.histogram);
            let is_duplicate = hash <= config.duplicate_hash_distance
                && histogram <= config.duplicate_histogram_distance;
            let is_series = hash <= config.series_hash_distance
                && histogram <= config.series_histogram_distance;
            if !is_duplicate && !is_series {
                continue;
            }
            union.union(i, j);
            if is_duplicate {
                duplicate_edges.push((i, j));
            }
        }
    }

    // A component is a near-duplicate group if *any* of its edges was a tight
    // (duplicate) match. Flags are resolved against the final roots so later
    // unions can never orphan a flag under a stale root index.
    let mut duplicate_component = vec![false; count];
    for (i, _) in duplicate_edges {
        let root = union.find(i);
        duplicate_component[root] = true;
    }

    // Collect components in ascending first-member order without hashing.
    let mut roots: Vec<usize> = Vec::new();
    for index in 0..count {
        let root = union.find(index);
        if !roots.contains(&root) {
            roots.push(root);
        }
    }

    let mut groups = Vec::new();
    for root in roots {
        let members: Vec<usize> = (0..count)
            .filter(|index| union.find(*index) == root)
            .collect();
        if members.len() < 2 {
            continue;
        }
        let kind = if duplicate_component[root] {
            SimilarityKind::Duplicate
        } else {
            SimilarityKind::Series
        };
        let best = members
            .iter()
            .copied()
            .reduce(|current, candidate| {
                let current_score = candidates[current].intrinsic_score;
                let candidate_score = candidates[candidate].intrinsic_score;
                if candidate_score > current_score {
                    candidate
                } else {
                    current
                }
            })
            .expect("non-empty component");
        groups.push(SimilarityGroup {
            members,
            kind,
            best,
        });
    }
    groups
}

/// Deterministic box-average resize of a luminance plane to `out_w x out_h`.
fn resize_box(plane: &LumaPlane, out_w: u32, out_h: u32) -> Vec<f32> {
    debug_assert!(out_w > 0 && out_h > 0);
    let mut output = vec![0f32; (out_w * out_h) as usize];
    for oy in 0..out_h {
        let y0 = (f64::from(oy) * f64::from(plane.height) / f64::from(out_h)).floor() as u32;
        let y1 = (((f64::from(oy + 1) * f64::from(plane.height) / f64::from(out_h)).ceil() as u32)
            .max(y0 + 1))
        .min(plane.height);
        for ox in 0..out_w {
            let x0 = (f64::from(ox) * f64::from(plane.width) / f64::from(out_w)).floor() as u32;
            let x1 = (((f64::from(ox + 1) * f64::from(plane.width) / f64::from(out_w)).ceil()
                as u32)
                .max(x0 + 1))
            .min(plane.width);
            let mut sum = 0f64;
            let mut n = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += f64::from(plane.at(x.min(plane.width - 1), y));
                    n += 1;
                }
            }
            output[(oy * out_w + ox) as usize] = if n == 0 { 0.0 } else { (sum / n as f64) as f32 };
        }
    }
    output
}

/// Minimal deterministic union-find (union by size, root-minimizing).
struct UnionFind {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl UnionFind {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..count).collect(),
            size: vec![1; count],
        }
    }

    fn find(&mut self, mut value: usize) -> usize {
        while self.parent[value] != value {
            self.parent[value] = self.parent[self.parent[value]];
            value = self.parent[value];
        }
        value
    }

    /// Returns the new root. The **smaller root index** wins ties so component
    /// numbering is independent of union order.
    fn union(&mut self, a: usize, b: usize) -> usize {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a == root_b {
            return root_a;
        }
        let (large, small) = if self.size[root_a] >= self.size[root_b] {
            (root_a, root_b)
        } else {
            (root_b, root_a)
        };
        // Keep the smaller index as root for a stable, order-independent root.
        let (new_root, child) = if large < small {
            (large, small)
        } else {
            (small, large)
        };
        self.parent[child] = new_root;
        self.size[new_root] += self.size[child];
        new_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(hash: u64, bin0: f32, bin1: f32) -> SimilaritySignature {
        let mut histogram = [0f32; SIMILARITY_HISTOGRAM_BINS];
        histogram[0] = bin0;
        histogram[1] = bin1;
        SimilaritySignature {
            perceptual_hash: hash,
            histogram,
            sample_count: 100,
        }
    }

    fn solid_frame(value: u8) -> ImageFrame {
        let pixels = (0..16).flat_map(|_| [value, value, value, 255]).collect();
        ImageFrame::new(4, 4, pixels).expect("exact buffer")
    }

    #[test]
    fn histogram_distance_is_bounded_and_zero_for_identical() {
        let a = signature(0, 1.0, 0.0);
        assert_eq!(histogram_distance(&a.histogram, &a.histogram), 0.0);
        let disjoint = signature(0, 0.0, 1.0);
        assert!((histogram_distance(&a.histogram, &disjoint.histogram) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn duplicate_and_series_thresholds_are_distinct() {
        let config = CullConfig::default();

        let duplicate = vec![
            SimilarityCandidate {
                signature: signature(0, 1.0, 0.0),
                intrinsic_score: 0.5,
            },
            SimilarityCandidate {
                signature: signature(0b11, 0.98, 0.02),
                intrinsic_score: 0.9,
            },
            SimilarityCandidate {
                signature: signature(u64::MAX, 0.0, 0.0),
                intrinsic_score: 0.5,
            },
        ];
        let groups = group_similar(&duplicate, &config);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members, vec![0, 1]);
        assert_eq!(groups[0].kind, SimilarityKind::Duplicate);
        assert_eq!(groups[0].best, 1, "higher intrinsic score becomes best");

        let series = vec![
            SimilarityCandidate {
                signature: signature(0, 1.0, 0.0),
                intrinsic_score: 0.4,
            },
            SimilarityCandidate {
                signature: signature(0b11_1111_1111, 0.8, 0.2),
                intrinsic_score: 0.6,
            },
        ];
        let groups = group_similar(&series, &config);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, SimilarityKind::Series);
    }

    #[test]
    fn identical_frames_have_equal_signatures_and_solid_frames_hash_to_zero() {
        let flat = solid_frame(128);
        let flat_signature = similarity_signature(&flat).expect("signature");
        assert_eq!(flat_signature.perceptual_hash, 0);
        assert_eq!(
            flat_signature,
            similarity_signature(&flat).expect("signature")
        );
        assert_eq!(perceptual_hash(&flat).expect("hash"), 0);
    }
}
