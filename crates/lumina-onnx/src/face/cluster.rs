//! LRPAR-G12-FACE-20 / FACE-20-S3 — deterministic, model-free face clustering.
//!
//! Stage 3 of the face pipeline is deliberately **local, model-free and
//! deterministic** (FACE-20 §2.1/§2.3): embeddings produced in stage 2 are
//! grouped into person clusters with DBSCAN over cosine distance. There is no
//! training, no online learning and no cloud call. The user later confirms,
//! splits or merges clusters manually — those are pure data operations on the
//! sidecar cluster/person records, implemented here.
//!
//! ## Identity
//!
//! The method name, its version and the calibrated thresholds
//! (`eps`/`min_samples`) are persisted through
//! [`FaceClusteringParams::to_identity`] and are part of the face identity
//! ([`super::face_identity`]). Any threshold or version change therefore makes
//! existing clusters `stale` — they are never silently re-clustered.
//!
//! ## Determinism and permutation invariance
//!
//! Points are ordered by their canonical IEEE-754 bit pattern before DBSCAN
//! runs, and clusters are re-labelled by their canonical member content. The
//! resulting partition is therefore independent of the input order and depends
//! only on the embeddings and the versioned thresholds.
//!
//! ## Image-local only
//!
//! Clustering operates on the detections of **one** source image. There is no
//! cross-catalogue person identity in 2.0 (FACE-20 §4); names are user labels
//! stored per sidecar.

use std::collections::{BTreeMap, BTreeSet};

use lumina_sidecar::{FaceCluster, FaceClusteringIdentity, FacePerson, MAX_FACE_CLUSTER_MEMBERS};

use crate::hash::compute_sha256_hex;
use crate::OnnxError;

/// Clustering method tag persisted in the face identity.
pub const FACE_CLUSTERING_METHOD: &str = "dbscan_cosine";
/// Clustering version persisted in the face identity. Changing the algorithm
/// semantics requires bumping this (never a silent reinterpretation).
pub const FACE_CLUSTERING_VERSION: u32 = 1;
/// Default cosine-distance threshold (`eps`). Start value to be calibrated and
/// re-versioned with real weights (FACE-20 §2.1); part of the identity.
pub const FACE_CLUSTERING_EPS_DEFAULT: f32 = 0.4;
/// Default minimum neighbourhood size (`min_samples`).
///
/// `1` is deliberate for **image-local** clustering: with only a handful of
/// faces per image, `min_samples = 1` means a single face still forms its own
/// cluster instead of being reported as noise. The value is versioned and part
/// of the identity.
pub const FACE_CLUSTERING_MIN_SAMPLES_DEFAULT: usize = 1;

/// Versioned DBSCAN-over-cosine thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceClusteringParams {
    /// Maximum cosine distance (`0..=2`) for two faces to be neighbours.
    pub eps: f32,
    /// Minimum neighbourhood size (self included) for a core point.
    pub min_samples: usize,
}

impl Default for FaceClusteringParams {
    fn default() -> Self {
        Self {
            eps: FACE_CLUSTERING_EPS_DEFAULT,
            min_samples: FACE_CLUSTERING_MIN_SAMPLES_DEFAULT,
        }
    }
}

impl FaceClusteringParams {
    /// Build validated parameters (loud on degenerate values).
    pub fn new(eps: f32, min_samples: usize) -> Result<Self, OnnxError> {
        let params = Self { eps, min_samples };
        params.validate()?;
        Ok(params)
    }

    /// Validate loudly: `eps` finite in `(0, 2]`, `min_samples >= 1`.
    pub fn validate(&self) -> Result<(), OnnxError> {
        if !self.eps.is_finite() || self.eps <= 0.0 || self.eps > 2.0 {
            return Err(OnnxError::InvalidFaceData(format!(
                "face clustering eps must be finite within (0, 2], got {}",
                self.eps
            )));
        }
        if self.min_samples == 0 {
            return Err(OnnxError::InvalidFaceData(
                "face clustering min_samples must be >= 1".into(),
            ));
        }
        Ok(())
    }

    /// Deterministic sidecar clustering identity (method, version, thresholds).
    ///
    /// `eps` is persisted through its exact shortest round-trip decimal
    /// representation (`f32` -> `to_string` -> `f32` is lossless), **not** a
    /// fixed-precision format: a threshold change at any representable
    /// magnitude must flip the identity (and therefore the digest), never be
    /// masked by rounding.
    #[must_use]
    pub fn to_identity(&self) -> FaceClusteringIdentity {
        let mut parameters = BTreeMap::new();
        parameters.insert("eps".into(), self.eps.to_string());
        parameters.insert("min_samples".into(), self.min_samples.to_string());
        FaceClusteringIdentity {
            method: FACE_CLUSTERING_METHOD.into(),
            version: FACE_CLUSTERING_VERSION,
            parameters,
            extras: BTreeMap::new(),
        }
    }
}

fn bit_key(embedding: &[f32]) -> Vec<u32> {
    embedding.iter().map(|value| value.to_bits()).collect()
}

fn validate_embeddings(embeddings: &[Vec<f32>]) -> Result<usize, OnnxError> {
    let Some(first) = embeddings.first() else {
        return Ok(0);
    };
    let dimension = first.len();
    if dimension == 0 {
        return Err(OnnxError::InvalidFaceData(
            "face clustering embedding must not be empty".into(),
        ));
    }
    for (index, embedding) in embeddings.iter().enumerate() {
        if embedding.len() != dimension {
            return Err(OnnxError::InvalidFaceData(format!(
                "face clustering embedding {index} has dimension {}, expected {dimension}",
                embedding.len()
            )));
        }
        if let Some(bad) = embedding.iter().position(|value| !value.is_finite()) {
            return Err(OnnxError::InvalidFaceData(format!(
                "face clustering embedding {index} has a non-finite value at {bad}"
            )));
        }
        let norm = embedding
            .iter()
            .map(|value| f64::from(*value) * f64::from(*value))
            .sum::<f64>();
        if norm <= f64::EPSILON {
            return Err(OnnxError::InvalidFaceData(format!(
                "face clustering embedding {index} has zero norm"
            )));
        }
    }
    Ok(dimension)
}

/// Cosine distance in `0..=2` (identical orientation → `0`, opposite → `2`).
fn cosine_distance(a: &[f32], b: &[f32]) -> f64 {
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = f64::from(*x);
        let y = f64::from(*y);
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denominator = (norm_a.sqrt()) * (norm_b.sqrt());
    // Defensive: `validate_embeddings` rejects zero-norm vectors, so this is
    // unreachable; returning the maximum distance keeps the function total
    // without inventing a similarity.
    if denominator <= 0.0 {
        return 2.0;
    }
    let similarity = (dot / denominator).clamp(-1.0, 1.0);
    1.0 - similarity
}

/// Deterministic DBSCAN over cosine distance.
///
/// Returns one label per input embedding in input order: `Some(cluster_index)`
/// or `None` for noise. Cluster indices are assigned in canonical member-content
/// order, so the result is independent of the input order. An empty input
/// yields an empty result; a single face yields exactly one cluster under the
/// default `min_samples = 1`.
pub fn cluster_embeddings(
    embeddings: &[Vec<f32>],
    params: &FaceClusteringParams,
) -> Result<Vec<Option<usize>>, OnnxError> {
    params.validate()?;
    validate_embeddings(embeddings)?;
    let count = embeddings.len();
    if count == 0 {
        return Ok(Vec::new());
    }

    // Canonical order: bit pattern of the values, then original index as a
    // deterministic tie-break (duplicates are indistinguishable anyway).
    let keys: Vec<Vec<u32>> = embeddings.iter().map(|e| bit_key(e)).collect();
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| keys[a].cmp(&keys[b]).then(a.cmp(&b)));

    let region_query = |point: usize| -> Vec<usize> {
        (0..count)
            .filter(|&other| {
                cosine_distance(&embeddings[point], &embeddings[other]) <= f64::from(params.eps)
            })
            .collect()
    };

    let mut visited = vec![false; count];
    let mut assigned = vec![false; count];
    let mut labels: Vec<Option<usize>> = vec![None; count];
    let mut next_label = 0usize;

    for &point in &order {
        if visited[point] {
            continue;
        }
        visited[point] = true;
        let neighbours = region_query(point);
        if neighbours.len() < params.min_samples {
            labels[point] = None; // noise (may be re-assigned by a later core)
            continue;
        }
        let label = next_label;
        next_label += 1;
        assigned[point] = true;
        labels[point] = Some(label);

        let mut seeds: BTreeSet<usize> = neighbours
            .into_iter()
            .filter(|&other| other != point)
            .collect();
        while let Some(seed) = seeds.pop_first() {
            if !visited[seed] {
                visited[seed] = true;
                let seed_neighbours = region_query(seed);
                if seed_neighbours.len() >= params.min_samples {
                    for other in seed_neighbours {
                        if other != seed {
                            seeds.insert(other);
                        }
                    }
                }
            }
            if !assigned[seed] {
                assigned[seed] = true;
                labels[seed] = Some(label);
            }
        }
    }

    // Canonical relabelling: cluster order is derived from member content, not
    // from the traversal order.
    let mut by_label: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, label) in labels.iter().enumerate() {
        if let Some(label) = label {
            by_label.entry(*label).or_default().push(index);
        }
    }
    let mut canonical: Vec<(Vec<u32>, usize)> = by_label
        .iter()
        .map(|(label, members)| {
            let min_key = members
                .iter()
                .map(|&member| keys[member].clone())
                .min()
                .expect("a cluster always has at least one member");
            (min_key, *label)
        })
        .collect();
    canonical.sort();
    let mut remap = BTreeMap::new();
    for (new_label, (_, old_label)) in canonical.into_iter().enumerate() {
        remap.insert(old_label, new_label);
    }
    Ok(labels
        .into_iter()
        .map(|label| label.map(|label| remap[&label]))
        .collect())
}

/// Stable, content-derived id of a cluster from its member detection ids.
///
/// Never an array position (FACE-20 §4); duplicate member ids are rejected
/// loudly.
pub fn cluster_id_for(detection_ids: &[String]) -> Result<String, OnnxError> {
    if detection_ids.is_empty() {
        return Err(OnnxError::InvalidFaceData(
            "face cluster must have at least one member".into(),
        ));
    }
    if detection_ids.len() > MAX_FACE_CLUSTER_MEMBERS {
        return Err(OnnxError::InvalidFaceData(format!(
            "face cluster exceeds member limit of {MAX_FACE_CLUSTER_MEMBERS}"
        )));
    }
    let mut sorted: Vec<&str> = detection_ids.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    for pair in sorted.windows(2) {
        if pair[0] == pair[1] {
            return Err(OnnxError::InvalidFaceData(format!(
                "face cluster lists detection `{}` twice",
                pair[0]
            )));
        }
    }
    // Length-prefixed join: injective in the member list.
    let mut canonical = String::new();
    for id in &sorted {
        canonical.push_str(&id.len().to_string());
        canonical.push(':');
        canonical.push_str(id);
        canonical.push('|');
    }
    let digest =
        compute_sha256_hex(canonical.as_bytes()).expect("hashing an in-memory buffer cannot fail");
    Ok(format!("cluster-{}", &digest[..32]))
}

/// Convert per-detection cluster labels into sidecar clusters.
///
/// Noise (`None`) members are not part of any cluster. Clusters are ordered by
/// their stable id and members sorted, so the result is deterministic and
/// independent of the detection order. `labels.len()` must match
/// `detection_ids.len()`.
pub fn clusters_from_labels(
    detection_ids: &[String],
    labels: &[Option<usize>],
) -> Result<Vec<FaceCluster>, OnnxError> {
    if detection_ids.len() != labels.len() {
        return Err(OnnxError::InvalidFaceData(format!(
            "face clustering returned {} labels for {} detections",
            labels.len(),
            detection_ids.len()
        )));
    }
    let mut by_label: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (detection_id, label) in detection_ids.iter().zip(labels.iter()) {
        if let Some(label) = label {
            by_label
                .entry(*label)
                .or_default()
                .push(detection_id.clone());
        }
    }
    let mut clusters: Vec<FaceCluster> = by_label
        .into_values()
        .map(|mut members| {
            members.sort();
            let id = cluster_id_for(&members)?;
            Ok(FaceCluster {
                id,
                detection_ids: members,
                extras: BTreeMap::new(),
            })
        })
        .collect::<Result<_, OnnxError>>()?;
    clusters.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(clusters)
}

fn validate_cluster_id(id: &str) -> Result<(), OnnxError> {
    if id.is_empty() || id.trim() != id || id.chars().any(char::is_control) {
        return Err(OnnxError::InvalidFaceData(
            "face cluster id must be non-empty without surrounding whitespace or control characters"
                .into(),
        ));
    }
    Ok(())
}

fn validate_person_name(name: &str) -> Result<(), OnnxError> {
    if name.trim().is_empty()
        || name.trim() != name
        || name.chars().any(char::is_control)
        || name.chars().count() > lumina_sidecar::MAX_FACE_NAME_CHARS
    {
        return Err(OnnxError::InvalidFaceData(
            "face person name must be non-empty, trimmed, without control characters and within \
             the length limit"
                .into(),
        ));
    }
    Ok(())
}

fn find_cluster<'a>(
    clusters: &'a [FaceCluster],
    cluster_id: &str,
) -> Result<&'a FaceCluster, OnnxError> {
    clusters
        .iter()
        .find(|cluster| cluster.id == cluster_id)
        .ok_or_else(|| OnnxError::InvalidFaceData(format!("unknown face cluster `{cluster_id}`")))
}

/// Confirm a cluster as a named person (pure data operation).
///
/// Creates the person when `person_id` is new (already `confirmed`), or appends
/// the cluster to an existing person. Loud refusals: unknown cluster, empty
/// name, invalid id, or a cluster already assigned to a *different* person —
/// never a silent re-assignment of someone else's cluster.
pub fn confirm_person(
    clusters: &[FaceCluster],
    persons: &[FacePerson],
    cluster_id: &str,
    person_id: &str,
    name: &str,
) -> Result<(Vec<FaceCluster>, Vec<FacePerson>), OnnxError> {
    validate_cluster_id(cluster_id)?;
    validate_cluster_id(person_id)?;
    validate_person_name(name)?;
    let cluster = find_cluster(clusters, cluster_id)?;

    let owner = persons
        .iter()
        .find(|person| person.cluster_ids.iter().any(|id| id == &cluster.id));
    if let Some(person) = owner {
        if person.id != person_id {
            return Err(OnnxError::InvalidFaceData(format!(
                "face cluster `{cluster_id}` is already assigned to person `{}`",
                person.id
            )));
        }
    }

    let mut persons = persons.to_vec();
    match persons.iter_mut().find(|person| person.id == person_id) {
        Some(person) => {
            person.name = name.to_owned();
            person.confirmed = true;
            if !person.cluster_ids.iter().any(|id| id == cluster_id) {
                person.cluster_ids.push(cluster_id.to_owned());
                person.cluster_ids.sort();
            }
        }
        None => persons.push(FacePerson {
            id: person_id.to_owned(),
            name: name.to_owned(),
            confirmed: true,
            cluster_ids: vec![cluster_id.to_owned()],
            extras: BTreeMap::new(),
        }),
    }
    persons.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((clusters.to_vec(), persons))
}

/// Split `subset` out of `cluster_id` into a new cluster `new_cluster_id`.
///
/// The source cluster keeps its remaining members and extras; the new cluster
/// is appended with empty extras. Loud refusals: unknown cluster, duplicate
/// `new_cluster_id`, empty subset, unknown subset member, or a subset equal to
/// the whole cluster (a cluster must keep at least one member — use a rename
/// instead).
pub fn split_cluster(
    clusters: &[FaceCluster],
    cluster_id: &str,
    subset: &[String],
    new_cluster_id: &str,
) -> Result<Vec<FaceCluster>, OnnxError> {
    validate_cluster_id(new_cluster_id)?;
    if subset.is_empty() {
        return Err(OnnxError::InvalidFaceData(
            "face cluster split subset must not be empty".into(),
        ));
    }
    if clusters.iter().any(|cluster| cluster.id == new_cluster_id) {
        return Err(OnnxError::InvalidFaceData(format!(
            "face cluster id `{new_cluster_id}` already exists"
        )));
    }
    let source = find_cluster(clusters, cluster_id)?;
    let members: BTreeSet<&str> = source.detection_ids.iter().map(String::as_str).collect();
    let mut subset_set: BTreeSet<&str> = BTreeSet::new();
    for id in subset {
        if !members.contains(id.as_str()) {
            return Err(OnnxError::InvalidFaceData(format!(
                "face cluster `{cluster_id}` does not contain detection `{id}`"
            )));
        }
        if !subset_set.insert(id.as_str()) {
            return Err(OnnxError::InvalidFaceData(format!(
                "face cluster split lists detection `{id}` twice"
            )));
        }
    }
    if subset_set.len() == members.len() {
        return Err(OnnxError::InvalidFaceData(format!(
            "face cluster `{cluster_id}` split would leave the source cluster empty"
        )));
    }

    let mut result = clusters.to_vec();
    let mut new_members: Vec<String> = subset.to_vec();
    new_members.sort();
    new_members.dedup();
    for cluster in &mut result {
        if cluster.id == cluster_id {
            cluster
                .detection_ids
                .retain(|id| !subset_set.contains(id.as_str()));
        }
    }
    result.push(FaceCluster {
        id: new_cluster_id.to_owned(),
        detection_ids: new_members,
        extras: BTreeMap::new(),
    });
    Ok(result)
}

/// Merge two clusters into `merged_id` (pure data operation).
///
/// `merged_id` may name one of the two clusters or a brand-new id. Loud
/// refusals: unknown cluster, merging a cluster with itself, a `merged_id` that
/// already belongs to an unrelated cluster, or conflicting extras keys (no
/// silent metadata loss).
pub fn merge_clusters(
    clusters: &[FaceCluster],
    first_id: &str,
    second_id: &str,
    merged_id: &str,
) -> Result<Vec<FaceCluster>, OnnxError> {
    validate_cluster_id(merged_id)?;
    if first_id == second_id {
        return Err(OnnxError::InvalidFaceData(
            "cannot merge a face cluster with itself".into(),
        ));
    }
    let first = find_cluster(clusters, first_id)?;
    let second = find_cluster(clusters, second_id)?;
    let unrelated = clusters.iter().any(|cluster| {
        cluster.id == merged_id && cluster.id != first_id && cluster.id != second_id
    });
    if unrelated {
        return Err(OnnxError::InvalidFaceData(format!(
            "face cluster id `{merged_id}` already belongs to another cluster"
        )));
    }

    let mut members: Vec<String> = first
        .detection_ids
        .iter()
        .chain(second.detection_ids.iter())
        .cloned()
        .collect();
    members.sort();
    members.dedup();
    if members.len() > MAX_FACE_CLUSTER_MEMBERS {
        return Err(OnnxError::InvalidFaceData(format!(
            "merged face cluster exceeds member limit of {MAX_FACE_CLUSTER_MEMBERS}"
        )));
    }

    let mut extras = first.extras.clone();
    for (key, value) in &second.extras {
        match extras.get(key) {
            Some(existing) if existing != value => {
                return Err(OnnxError::InvalidFaceData(format!(
                    "cannot merge face clusters: conflicting extras key `{key}`"
                )));
            }
            _ => {
                extras.insert(key.clone(), value.clone());
            }
        }
    }

    let merged = FaceCluster {
        id: merged_id.to_owned(),
        detection_ids: members,
        extras,
    };
    let mut result = Vec::with_capacity(clusters.len() - 1);
    let mut placed = false;
    for cluster in clusters {
        if cluster.id == first_id || cluster.id == second_id {
            if !placed {
                result.push(merged.clone());
                placed = true;
            }
        } else {
            result.push(cluster.clone());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(values: &[f32]) -> Vec<f32> {
        let norm = values
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
            .sqrt();
        values.iter().map(|v| (*v as f64 / norm) as f32).collect()
    }

    /// Three well-separated directions: two near each other, one opposite.
    fn separated() -> Vec<Vec<f32>> {
        vec![
            unit(&[1.0, 0.0, 0.0]),
            unit(&[0.99, 0.01, 0.0]),
            unit(&[-1.0, 0.0, 0.0]),
        ]
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(cluster_embeddings(&[], &FaceClusteringParams::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn single_face_forms_one_cluster_under_default() {
        let labels =
            cluster_embeddings(&[unit(&[1.0, 0.0])], &FaceClusteringParams::default()).unwrap();
        assert_eq!(labels, vec![Some(0)]);
    }

    #[test]
    fn single_face_is_noise_when_min_samples_exceeds_neighbourhood() {
        let params = FaceClusteringParams::new(0.4, 2).unwrap();
        let labels = cluster_embeddings(&[unit(&[1.0, 0.0])], &params).unwrap();
        assert_eq!(labels, vec![None]);
    }

    #[test]
    fn separated_points_split_into_two_clusters() {
        let labels = cluster_embeddings(&separated(), &FaceClusteringParams::default()).unwrap();
        assert_eq!(labels[0], labels[1]);
        assert_ne!(labels[0], labels[2]);
        assert!(labels.iter().all(Option::is_some));
    }

    #[test]
    fn clustering_is_stable_and_permutation_invariant() {
        let embeddings = separated();
        let params = FaceClusteringParams::default();
        let base = cluster_embeddings(&embeddings, &params).unwrap();

        // Same input → same labels.
        assert_eq!(base, cluster_embeddings(&embeddings, &params).unwrap());

        // Permuting the input must yield the same partition and the same
        // canonical label numbering.
        let permutation = [2usize, 0, 1];
        let permuted: Vec<Vec<f32>> = permutation.iter().map(|&i| embeddings[i].clone()).collect();
        let permuted_labels = cluster_embeddings(&permuted, &params).unwrap();
        for (new_index, &old_index) in permutation.iter().enumerate() {
            assert_eq!(
                permuted_labels[new_index], base[old_index],
                "canonical labels must survive a permutation"
            );
        }
    }

    #[test]
    fn eps_boundary_is_inclusive() {
        // Orthogonal unit vectors have cosine distance exactly 1.0.
        let a = unit(&[1.0, 0.0]);
        let b = unit(&[0.0, 1.0]);
        let joined = FaceClusteringParams::new(1.0, 1).unwrap();
        let labels = cluster_embeddings(&[a.clone(), b.clone()], &joined).unwrap();
        assert_eq!(labels[0], labels[1], "distance == eps must join");

        let split = FaceClusteringParams::new(0.9, 1).unwrap();
        let labels = cluster_embeddings(&[a, b], &split).unwrap();
        assert_ne!(labels[0], labels[1], "distance > eps must separate");

        // The maximum cosine distance (2.0, opposite vectors) is also inclusive.
        let opposite = unit(&[-1.0, 0.0]);
        let joined = FaceClusteringParams::new(2.0, 1).unwrap();
        let labels = cluster_embeddings(&[unit(&[1.0, 0.0]), opposite.clone()], &joined).unwrap();
        assert_eq!(labels[0], labels[1]);
        let split = FaceClusteringParams::new(1.9, 1).unwrap();
        let labels = cluster_embeddings(&[unit(&[1.0, 0.0]), opposite], &split).unwrap();
        assert_ne!(labels[0], labels[1]);
    }

    #[test]
    fn params_validation_and_identity_are_loud_and_deterministic() {
        assert!(FaceClusteringParams::new(0.0, 1).is_err());
        assert!(FaceClusteringParams::new(f32::NAN, 1).is_err());
        assert!(FaceClusteringParams::new(2.5, 1).is_err());
        assert!(FaceClusteringParams::new(0.4, 0).is_err());
        let identity = FaceClusteringParams::default().to_identity();
        assert_eq!(identity.method, FACE_CLUSTERING_METHOD);
        assert_eq!(identity.version, FACE_CLUSTERING_VERSION);
        assert_eq!(identity.parameters.get("eps").unwrap(), "0.4");
        assert_eq!(identity.parameters.get("min_samples").unwrap(), "1");
    }

    /// The persisted `eps` must round-trip exactly: two thresholds that only
    /// differ below the former `{:.6}` rounding step must still produce
    /// distinct identities, so a real threshold change is never masked (and
    /// therefore never missed as `stale`).
    #[test]
    fn clustering_identity_preserves_exact_eps() {
        let base = FaceClusteringParams::new(0.4, 1).unwrap();
        let nearby = FaceClusteringParams::new(f32::from_bits(0.4f32.to_bits() + 1), 1).unwrap();
        assert_ne!(
            base.eps, nearby.eps,
            "the fixtures must differ below the old 6-decimal step"
        );
        assert_eq!(base.eps.to_string(), "0.4");
        assert_eq!(
            base.to_identity().parameters.get("eps").unwrap(),
            "0.4",
            "the identity must carry the exact round-trip value"
        );
        assert_ne!(
            base.to_identity(),
            nearby.to_identity(),
            "a sub-1e-6 threshold change must flip the clustering identity"
        );
    }

    #[test]
    fn embedding_validation_rejects_inconsistent_inputs() {
        let params = FaceClusteringParams::default();
        assert!(cluster_embeddings(&[vec![1.0, 0.0], vec![1.0]], &params).is_err());
        assert!(cluster_embeddings(&[vec![f32::NAN, 0.0]], &params).is_err());
        assert!(cluster_embeddings(&[vec![0.0, 0.0]], &params).is_err());
    }

    #[test]
    fn clusters_from_labels_are_stable_and_skip_noise() {
        let ids = vec!["face-a".to_string(), "face-b".into(), "face-c".into()];
        let labels = [Some(0), Some(0), None];
        let clusters = clusters_from_labels(&ids, &labels).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].detection_ids, vec!["face-a", "face-b"]);
        assert_eq!(
            clusters[0].id,
            cluster_id_for(&clusters[0].detection_ids).unwrap()
        );
        // Reordering the detections does not change the cluster identity.
        let reordered = clusters_from_labels(
            &["face-c".into(), "face-b".into(), "face-a".into()],
            &[None, Some(0), Some(0)],
        )
        .unwrap();
        assert_eq!(reordered, clusters);
        assert!(clusters_from_labels(&ids, &[Some(0)]).is_err());
    }

    fn cluster(id: &str, members: &[&str]) -> FaceCluster {
        FaceCluster {
            id: id.into(),
            detection_ids: members.iter().map(|m| (*m).into()).collect(),
            extras: BTreeMap::new(),
        }
    }

    #[test]
    fn confirm_person_creates_and_appends() {
        let clusters = vec![cluster("c1", &["f1"]), cluster("c2", &["f2"])];
        let (_, persons) = confirm_person(&clusters, &[], "c1", "p1", "Alex").unwrap();
        assert_eq!(persons.len(), 1);
        assert_eq!(persons[0].name, "Alex");
        assert!(persons[0].confirmed);
        assert_eq!(persons[0].cluster_ids, vec!["c1"]);

        let (_, persons) = confirm_person(&clusters, &persons, "c2", "p1", "Alex").unwrap();
        assert_eq!(persons[0].cluster_ids, vec!["c1", "c2"]);

        // A different person cannot steal an assigned cluster.
        assert!(confirm_person(&clusters, &persons, "c1", "p2", "Sam").is_err());
        // Unknown cluster / empty name are loud too.
        assert!(confirm_person(&clusters, &persons, "c9", "p2", "Sam").is_err());
        assert!(confirm_person(&clusters, &persons, "c2", "p2", "  ").is_err());
    }

    #[test]
    fn split_cluster_moves_a_subset() {
        let clusters = vec![cluster("c1", &["f1", "f2", "f3"])];
        let result = split_cluster(&clusters, "c1", &["f2".into()], "c2").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].detection_ids, vec!["f1", "f3"]);
        assert_eq!(result[1].id, "c2");
        assert_eq!(result[1].detection_ids, vec!["f2"]);

        // Loud failures.
        assert!(split_cluster(&clusters, "c1", &[], "c2").is_err());
        assert!(split_cluster(&clusters, "c1", &["f9".into()], "c2").is_err());
        assert!(split_cluster(
            &clusters,
            "c1",
            &["f1".into(), "f2".into(), "f3".into()],
            "c2"
        )
        .is_err());
        assert!(split_cluster(&clusters, "c1", &["f1".into()], "c1").is_err());
        assert!(split_cluster(&clusters, "c9", &["f1".into()], "c2").is_err());
    }

    #[test]
    fn merge_clusters_unions_members_and_extras() {
        let mut first = cluster("c1", &["f1"]);
        first.extras.insert("note".into(), serde_json::json!("a"));
        let mut second = cluster("c2", &["f2"]);
        second
            .extras
            .insert("origin".into(), serde_json::json!("b"));
        let result = merge_clusters(&[first, second], "c1", "c2", "c1").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "c1");
        assert_eq!(result[0].detection_ids, vec!["f1", "f2"]);
        assert_eq!(result[0].extras.len(), 2);

        // Conflicting extras must not be dropped silently.
        let mut a = cluster("c1", &["f1"]);
        a.extras.insert("k".into(), serde_json::json!(1));
        let mut b = cluster("c2", &["f2"]);
        b.extras.insert("k".into(), serde_json::json!(2));
        assert!(merge_clusters(&[a, b], "c1", "c2", "c1").is_err());

        // Merging with itself / an unrelated target is loud.
        let clusters = vec![
            cluster("c1", &["f1"]),
            cluster("c2", &["f2"]),
            cluster("c3", &["f3"]),
        ];
        assert!(merge_clusters(&clusters, "c1", "c1", "c1").is_err());
        assert!(merge_clusters(&clusters, "c1", "c2", "c3").is_err());
        assert!(merge_clusters(&clusters, "c9", "c2", "c1").is_err());
    }
}
