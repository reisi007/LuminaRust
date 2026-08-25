use crate::pipeline::RenderKey;
use blake3::hash;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use thiserror::Error;

/// On-disk half of the folder cache. Only available on targets with a file
/// system so that the portable core keeps compiling for `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub mod disk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheStage {
    Decode,
    Preview,
    Histogram,
    Mask,
    Export,
}

/// The two preview resolutions the folder cache may hold per source and
/// virtual copy. `Standard` is the default that is written when a source is
/// left; `OneToOne` is the inherited, opt-in folder option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreviewKind {
    Standard,
    OneToOne,
}

impl PreviewKind {
    pub const ALL: [PreviewKind; 2] = [Self::Standard, Self::OneToOne];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::OneToOne => "one-to-one",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderCacheSettings {
    #[serde(default = "default_standard_preview")]
    pub standard_preview: bool,
    #[serde(default = "default_one_to_one_preview")]
    pub one_to_one_preview: bool,
}

fn default_standard_preview() -> bool {
    FolderCacheSettings::default().standard_preview
}

fn default_one_to_one_preview() -> bool {
    FolderCacheSettings::default().one_to_one_preview
}

impl Default for FolderCacheSettings {
    fn default() -> Self {
        Self {
            standard_preview: true,
            one_to_one_preview: false,
        }
    }
}

impl FolderCacheSettings {
    /// Whether the effective folder options allow caching `kind`.
    pub fn allows(&self, kind: PreviewKind) -> bool {
        match kind {
            PreviewKind::Standard => self.standard_preview,
            PreviewKind::OneToOne => self.one_to_one_preview,
        }
    }
}

/// Cache identity of one cached preview: source file name plus virtual copy id.
/// `NUL` separates both parts because it cannot occur in a file name, so no
/// source/virtual-copy combination can collide with another one.
pub fn preview_cache_key(source: &str, virtual_copy: &str) -> String {
    format!("{source}\u{0}{virtual_copy}")
}

#[derive(Debug, Default)]
pub struct FolderCache {
    entries: HashMap<String, Vec<u8>>,
}

impl FolderCache {
    /// Folder options are inherited from parent folders; a folder that stores
    /// its own `settings.json` overrides the inherited record completely. The
    /// chain is ordered from the outermost parent to the folder itself.
    pub fn effective_settings(chain: &[FolderCacheSettings]) -> FolderCacheSettings {
        chain
            .iter()
            .copied()
            .fold(FolderCacheSettings::default(), |_, settings| settings)
    }

    pub fn store(&mut self, source_id: impl Into<String>, bytes: Vec<u8>) {
        self.entries.insert(source_id.into(), bytes);
    }

    pub fn get(&self, source_id: &str) -> Option<&[u8]> {
        self.entries.get(source_id).map(Vec::as_slice)
    }

    pub fn prune_orphans(&mut self, live_source_ids: &[String]) {
        self.entries
            .retain(|source_id, _| live_source_ids.contains(source_id));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub stage: CacheStage,
    pub key: String,
    pub bytes: Vec<u8>,
    pub checksum: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CacheError {
    #[error("cache entry exceeds configured size limit")]
    TooLarge,
    #[error("cache operation was cancelled")]
    Cancelled,
    #[error("cache checksum mismatch")]
    Corrupt,
}

#[derive(Debug, Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Default)]
pub struct CacheStore {
    entries: HashMap<(CacheStage, String), CacheEntry>,
    max_bytes: usize,
    used_bytes: usize,
    clock: AtomicU64,
    access: HashMap<(CacheStage, String), u64>,
}
impl CacheStore {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            ..Default::default()
        }
    }
    pub fn get(&mut self, stage: CacheStage, key: &RenderKey) -> Option<Vec<u8>> {
        let id = (stage, key.stage_digest(stage));
        let entry = self.entries.get(&id)?;
        if hash(&entry.bytes).to_hex().as_str() != entry.checksum {
            self.remove_id(&id);
            return None;
        }
        self.clock.fetch_add(1, Ordering::Relaxed);
        self.access.insert(id, self.clock.load(Ordering::Relaxed));
        Some(entry.bytes.clone())
    }
    pub fn put(
        &mut self,
        stage: CacheStage,
        key: &RenderKey,
        bytes: Vec<u8>,
        cancellation: &Cancellation,
    ) -> Result<(), CacheError> {
        if cancellation.is_cancelled() {
            return Err(CacheError::Cancelled);
        }
        if bytes.len() > self.max_bytes {
            return Err(CacheError::TooLarge);
        }
        let id = (stage, key.stage_digest(stage));
        self.remove_id(&id);
        while self.used_bytes + bytes.len() > self.max_bytes {
            let Some(oldest) = self
                .access
                .iter()
                .min_by_key(|(_, stamp)| *stamp)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            self.remove_id(&oldest);
        }
        let entry = CacheEntry {
            stage,
            key: id.1.clone(),
            checksum: hash(&bytes).to_hex().to_string(),
            bytes,
        };
        self.used_bytes += entry.bytes.len();
        self.entries.insert(id.clone(), entry);
        self.clock.fetch_add(1, Ordering::Relaxed);
        self.access.insert(id, self.clock.load(Ordering::Relaxed));
        Ok(())
    }
    pub fn invalidate(&mut self, changed: CacheStage) {
        let stages: Vec<_> = self
            .entries
            .keys()
            .filter(|(stage, _)| invalidated_by(changed, *stage))
            .cloned()
            .collect();
        for id in stages {
            self.remove_id(&id);
        }
    }
    pub fn prune(&mut self, live_keys: &[String]) {
        let stale: Vec<_> = self
            .entries
            .keys()
            .filter(|(_, key)| !live_keys.contains(key))
            .cloned()
            .collect();
        for id in stale {
            self.remove_id(&id);
        }
    }
    fn remove_id(&mut self, id: &(CacheStage, String)) {
        if let Some(entry) = self.entries.remove(id) {
            self.used_bytes -= entry.bytes.len();
        }
        self.access.remove(id);
    }
}
fn invalidated_by(changed: CacheStage, candidate: CacheStage) -> bool {
    match changed {
        CacheStage::Decode => true,
        CacheStage::Mask => matches!(
            candidate,
            CacheStage::Preview | CacheStage::Histogram | CacheStage::Export
        ),
        CacheStage::Preview => false,
        CacheStage::Histogram => false,
        CacheStage::Export => false,
    }
}

#[derive(Debug, Default)]
pub struct StaleTracker {
    generation: AtomicU64,
}
impl StaleTracker {
    pub fn begin(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }
    pub fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{OutputSpec, RenderKey};
    use lumina_sidecar::EditRecipe;
    fn key() -> RenderKey {
        RenderKey::new(
            "s",
            "d",
            "p",
            "vc",
            &EditRecipe::default(),
            vec![],
            OutputSpec {
                profile: "srgb".into(),
                width: 1,
                height: 1,
                format: "png".into(),
            },
        )
    }
    #[test]
    fn invalidation_keeps_decode_for_mask_changes() {
        let mut cache = CacheStore::new(100);
        let stop = Cancellation::default();
        cache
            .put(CacheStage::Decode, &key(), vec![1], &stop)
            .unwrap();
        cache
            .put(CacheStage::Preview, &key(), vec![2], &stop)
            .unwrap();
        cache.invalidate(CacheStage::Mask);
        assert!(cache.get(CacheStage::Decode, &key()).is_some());
        assert!(cache.get(CacheStage::Preview, &key()).is_none());
    }

    #[test]
    fn output_changes_reuse_decode_but_not_preview() {
        let mut cache = CacheStore::new(100);
        let stop = Cancellation::default();
        let first = key();
        let mut second = first.clone();
        second.output.width = 2;
        cache
            .put(CacheStage::Decode, &first, vec![1], &stop)
            .unwrap();
        cache
            .put(CacheStage::Preview, &first, vec![2], &stop)
            .unwrap();
        assert_eq!(cache.get(CacheStage::Decode, &second), Some(vec![1]));
        assert_eq!(cache.get(CacheStage::Preview, &second), None);
    }

    #[test]
    fn crop_changes_reuse_decode_and_mask_but_miss_preview() {
        let mut cache = CacheStore::new(100);
        let stop = Cancellation::default();
        let first = key();
        let mut second = first.clone();
        second.recipe_hash = "crop-changed".into();
        // A crop is downstream of masks; the stage key has the same upstream
        // identity even though the final recipe/render key differs.
        cache
            .put(CacheStage::Decode, &first, vec![1], &stop)
            .unwrap();
        cache.put(CacheStage::Mask, &first, vec![2], &stop).unwrap();
        cache
            .put(CacheStage::Preview, &first, vec![3], &stop)
            .unwrap();
        assert_eq!(cache.get(CacheStage::Decode, &second), Some(vec![1]));
        assert_eq!(cache.get(CacheStage::Mask, &second), Some(vec![2]));
        assert_eq!(cache.get(CacheStage::Preview, &second), None);
    }

    #[test]
    fn source_or_decode_context_change_misses_decode_and_mask() {
        let mut cache = CacheStore::new(100);
        let stop = Cancellation::default();
        let first = key();
        cache
            .put(CacheStage::Decode, &first, vec![1], &stop)
            .unwrap();
        cache.put(CacheStage::Mask, &first, vec![2], &stop).unwrap();
        let mut changed = first.clone();
        changed.source_content_hash = "new-source".into();
        assert_eq!(cache.get(CacheStage::Decode, &changed), None);
        assert_eq!(cache.get(CacheStage::Mask, &changed), None);
        cache
            .put(CacheStage::Decode, &first, vec![1], &stop)
            .unwrap();
        cache.put(CacheStage::Mask, &first, vec![2], &stop).unwrap();
        changed = first.clone();
        changed.decode_version = "new-decoder".into();
        assert_eq!(cache.get(CacheStage::Decode, &changed), None);
        assert_eq!(cache.get(CacheStage::Mask, &changed), None);
    }

    #[test]
    fn mask_artifact_change_misses_mask_and_preview_but_reuses_decode() {
        let mut cache = CacheStore::new(100);
        let stop = Cancellation::default();
        let first = key();
        cache
            .put(CacheStage::Decode, &first, vec![1], &stop)
            .unwrap();
        cache.put(CacheStage::Mask, &first, vec![2], &stop).unwrap();
        cache
            .put(CacheStage::Preview, &first, vec![3], &stop)
            .unwrap();
        let mut changed = first.clone();
        changed.mask_artifact_hashes.push("changed".into());
        assert_eq!(cache.get(CacheStage::Decode, &changed), Some(vec![1]));
        assert_eq!(cache.get(CacheStage::Mask, &changed), None);
        assert_eq!(cache.get(CacheStage::Preview, &changed), None);
    }

    // REVIEW-CORE-SRCACC-1: a changed repair artifact must not serve stale
    // pixels from any downstream stage.
    #[test]
    fn source_action_artifact_change_misses_preview_but_reuses_decode() {
        let mut cache = CacheStore::new(100);
        let stop = Cancellation::default();
        let first = key().with_source_action_hashes(["blake3:old-artifact".to_owned()]);
        cache
            .put(CacheStage::Decode, &first, vec![1], &stop)
            .unwrap();
        cache
            .put(CacheStage::Preview, &first, vec![2], &stop)
            .unwrap();
        let changed = first.with_source_action_hashes(["blake3:new-artifact".to_owned()]);
        assert_eq!(cache.get(CacheStage::Decode, &changed), Some(vec![1]));
        assert_eq!(cache.get(CacheStage::Preview, &changed), None);
    }

    // REVIEW-CORE-EXPORTKEY-1: two encodes that differ only in quality (or
    // dither/seed/bit depth) must not share a preview/export entry.
    #[test]
    fn export_option_change_misses_preview() {
        let mut cache = CacheStore::new(100);
        let stop = Cancellation::default();
        let first = key().with_export_options(crate::ExportOptions {
            quality: 90,
            ..crate::ExportOptions::default()
        });
        cache
            .put(CacheStage::Preview, &first, vec![1], &stop)
            .unwrap();
        let changed = first.clone().with_export_options(crate::ExportOptions {
            quality: 60,
            ..crate::ExportOptions::default()
        });
        assert_eq!(cache.get(CacheStage::Preview, &changed), None);
        assert_eq!(
            cache.get(CacheStage::Preview, &first),
            Some(vec![1]),
            "the original entry stays reachable"
        );
    }

    // REVIEW-CORE-N1: histograms are measured post-crop, so a size change is
    // a histogram cache miss.
    #[test]
    fn output_size_change_misses_histogram_but_reuses_decode() {
        let mut cache = CacheStore::new(200);
        let stop = Cancellation::default();
        let mut first = key();
        first.output.width = 32;
        first.output.height = 32;
        cache
            .put(CacheStage::Decode, &first, vec![1], &stop)
            .unwrap();
        cache
            .put(CacheStage::Histogram, &first, vec![2], &stop)
            .unwrap();
        let mut resized = first.clone();
        resized.output.width = 64;
        resized.output.height = 64;
        assert_eq!(cache.get(CacheStage::Decode, &resized), Some(vec![1]));
        assert_eq!(cache.get(CacheStage::Histogram, &resized), None);
    }
    #[test]
    fn newer_job_makes_older_result_stale() {
        let tracker = StaleTracker::default();
        let old = tracker.begin();
        let new = tracker.begin();
        assert!(!tracker.is_current(old));
        assert!(tracker.is_current(new));
    }
    #[test]
    fn cancelled_and_size_limited_writes_are_not_committed() {
        let mut cache = CacheStore::new(2);
        let stop = Cancellation::default();
        stop.cancel();
        assert_eq!(
            cache.put(CacheStage::Preview, &key(), vec![1], &stop),
            Err(CacheError::Cancelled)
        );
        let active = Cancellation::default();
        assert_eq!(
            cache.put(CacheStage::Preview, &key(), vec![1, 2, 3], &active),
            Err(CacheError::TooLarge)
        );
    }

    #[test]
    fn folder_settings_are_inherited_and_orphans_are_pruned() {
        let settings = FolderCache::effective_settings(&[
            FolderCacheSettings::default(),
            FolderCacheSettings {
                one_to_one_preview: true,
                ..Default::default()
            },
        ]);
        assert!(settings.one_to_one_preview);
        let mut cache = FolderCache::default();
        cache.store("live", vec![1]);
        cache.store("gone", vec![2]);
        cache.prune_orphans(&["live".into()]);
        assert!(cache.get("live").is_some());
        assert!(cache.get("gone").is_none());
    }
}
