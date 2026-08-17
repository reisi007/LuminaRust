//! Shared eframe application for the native and browser MVP.

use eframe::egui;
use lumina_core::{
    analyze_tone, match_total_exposure, suggest_auto_tone, tone_fingerprint, AutoToneConfig,
    ImageFrame, RenderKey,
};
use lumina_raw::RawError;
use lumina_sidecar::{AnalysisFingerprint, EditRecipe, Preset};
#[cfg(not(target_arch = "wasm32"))]
use lumina_sidecar::{
    ArtifactStatus, CoordinateSystem, DecodeFingerprint, GeometryFingerprint, HistoryEntry,
    MaskDefinition, MaskLayer, MaskOperation, MaskReference, MaskStatus, ModelIdentity,
    Preprocessing, Resolution, SidecarDocument, SourceFingerprint, SourceIdentity, SourceStatus,
};
#[cfg(not(target_arch = "wasm32"))]
use serde_json::Value;
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

/// Work which may be performed when the GUI has no interactive input.
///
/// Queueing is deliberately separate from mask status: a missing/pending mask
/// is never inserted here implicitly.  The caller must enqueue it as the
/// result of an explicit user action (or a future CLI/GUI command).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdleTask {
    MaskInference { mask_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedIdleTask {
    id: u64,
    priority: u8,
    task: IdleTask,
}

/// Small, bounded priority queue for work that is safe to defer until idle.
#[derive(Debug, Clone)]
pub struct IdleQueue {
    capacity: usize,
    next_id: u64,
    tasks: Vec<QueuedIdleTask>,
}

impl IdleQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next_id: 0,
            tasks: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Enqueue only explicit requests. Returns a stable cancellation handle.
    pub fn enqueue(&mut self, task: IdleTask, priority: u8) -> Option<u64> {
        if self.tasks.len() >= self.capacity {
            return None;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.tasks.push(QueuedIdleTask { id, priority, task });
        Some(id)
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|task| task.id != id);
        self.tasks.len() != before
    }

    /// Takes the highest-priority task. Equal priorities retain FIFO order.
    pub fn pop_next(&mut self) -> Option<(u64, IdleTask)> {
        let index = self
            .tasks
            .iter()
            .enumerate()
            .max_by_key(|(_, task)| task.priority)?
            .0;
        let task = self.tasks.remove(index);
        Some((task.id, task.task))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GuiError {
    #[error("{0}")]
    Core(#[from] lumina_core::CoreError),
    #[error("{0}")]
    Sidecar(#[from] lumina_sidecar::SidecarError),
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    Raw(#[from] RawError),
}

pub struct LuminaApp {
    original: Option<ImageFrame>,
    preview: Option<ImageFrame>,
    source_bytes: Option<Vec<u8>>,
    source_is_raw: bool,
    raw_orientation: u8,
    source_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    path: String,
    #[cfg(not(target_arch = "wasm32"))]
    directory: String,
    #[cfg(not(target_arch = "wasm32"))]
    entries: Vec<FileBrowserEntry>,
    #[cfg(not(target_arch = "wasm32"))]
    selected_entry: Option<usize>,
    recipe: EditRecipe,
    texture: Option<egui::TextureHandle>,
    status: String,
    error: Option<String>,
    render_key: Option<RenderKey>,
    tone_analysis: Option<lumina_core::ToneAnalysis>,
    #[cfg(not(target_arch = "wasm32"))]
    document: Option<SidecarDocument>,
    #[cfg(not(target_arch = "wasm32"))]
    virtual_copy_id: String,
    #[cfg(not(target_arch = "wasm32"))]
    selected_mask_id: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    mask_name_input: String,
    preset_name: String,
    preset_fields: BTreeMap<String, bool>,
    preset_relative_exposure: bool,
    idle_queue: IdleQueue,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
struct FileBrowserEntry {
    path: PathBuf,
    name: String,
    sidecar_path: PathBuf,
    has_sidecar: bool,
    source_status: SourceStatus,
    conflict: bool,
    virtual_copies: usize,
    missing_models: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileBrowserEntry {
    fn status_label(&self) -> &'static str {
        if self.conflict {
            "Konflikt"
        } else if matches!(self.source_status, SourceStatus::Missing) {
            "Offline"
        } else if self.has_sidecar {
            "Sidecar"
        } else {
            "Ohne"
        }
    }
    fn is_offline(&self) -> bool {
        matches!(self.source_status, SourceStatus::Missing)
    }
}

impl LuminaApp {
    pub fn new(_ctx: egui::Context) -> Self {
        Self {
            original: None,
            preview: None,
            source_bytes: None,
            source_is_raw: false,
            raw_orientation: 1,
            source_name: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            path: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            directory: ".".into(),
            #[cfg(not(target_arch = "wasm32"))]
            entries: Vec::new(),
            selected_entry: None,
            recipe: EditRecipe::default(),
            texture: None,
            status: "Bereit für ein PNG, JPEG oder WebP".into(),
            error: None,
            render_key: None,
            tone_analysis: None,
            #[cfg(not(target_arch = "wasm32"))]
            document: None,
            #[cfg(not(target_arch = "wasm32"))]
            virtual_copy_id: "vc-original".into(),
            #[cfg(not(target_arch = "wasm32"))]
            selected_mask_id: None,
            mask_name_input: String::new(),
            preset_name: String::new(),
            preset_fields: BTreeMap::from([
                ("exposure".into(), true),
                ("contrast".into(), true),
                ("highlights".into(), false),
                ("shadows".into(), false),
            ]),
            preset_relative_exposure: false,
            idle_queue: IdleQueue::new(32),
        }
    }

    pub fn recipe(&self) -> &EditRecipe {
        &self.recipe
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_file(&mut self, path: impl Into<String>) {
        self.path = path.into();
        // Populate the file browser with the directory containing the opened file.
        if let Some(parent) = Path::new(&self.path).parent() {
            self.directory = parent.display().to_string();
        }
        self.list_directory();
        self.load_path();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_directory(&mut self, directory: impl Into<String>) {
        self.directory = directory.into();
        self.list_directory();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn list_directory(&mut self) {
        let directory = std::path::PathBuf::from(self.directory.trim());
        self.entries.clear();
        match std::fs::read_dir(&directory) {
            Ok(dir_entries) => {
                for entry in dir_entries.flatten() {
                    if let Some(scanned) = Self::scan_entry(&entry.path()) {
                        self.entries.push(scanned);
                    }
                }
                // Also pick up orphan sidecars whose source file is missing.
                // After deleting the source, read_dir won't list it, but the
                // .lumina.json sidecar still exists on disk.
                if let Ok(sidecar_entries) = std::fs::read_dir(&directory) {
                    for entry in sidecar_entries.flatten() {
                        let path = entry.path();
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.ends_with(".lumina.json") {
                                if let Some(source_name) = name.strip_suffix(".lumina.json") {
                                    let source_path = directory.join(source_name);
                                    if !self.entries.iter().any(|e| e.path == source_path) {
                                        if let Some(scanned) = Self::scan_entry(&source_path) {
                                            self.entries.push(scanned);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                self.entries.sort_by(|a, b| a.name.cmp(&b.name));
                self.status = format!("{} Bilder im Verzeichnis", self.entries.len());
            }
            Err(error) => {
                self.status = format!("Verzeichnis nicht lesbar: {error}");
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn entries(&self) -> &[FileBrowserEntry] {
        &self.entries
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scan_entry(path: &Path) -> Option<FileBrowserEntry> {
        if !is_supported_image(path) {
            return None;
        }
        let sidecar_path = lumina_sidecar::sidecar_path_for(path);
        let has_sidecar = sidecar_path.is_file();
        let mut virtual_copies = 0usize;
        let mut missing_models = 0usize;
        let source_status = if path.is_file() {
            match lumina_sidecar::load_sidecar(&sidecar_path) {
                Ok(document) => {
                    virtual_copies = document.virtual_copies.len();
                    let bundle_root = path.parent().unwrap_or_else(|| Path::new("."));
                    for copy in &document.virtual_copies {
                        for mask in &copy.mask_library {
                            let artifact_missing =
                                mask.artifact.as_ref().map_or(false, |artifact| {
                                    lumina_sidecar::artifact_status(bundle_root, artifact)
                                        == ArtifactStatus::Missing
                                });
                            if matches!(
                                mask.status,
                                MaskStatus::Missing
                                    | MaskStatus::Pending
                                    | MaskStatus::Stale
                                    | MaskStatus::Corrupt
                            ) || artifact_missing
                            {
                                missing_models += 1;
                            }
                        }
                    }
                    lumina_sidecar::source_status(path, &document.source)
                        .unwrap_or(SourceStatus::Unchanged)
                }
                Err(_) => SourceStatus::Unchanged,
            }
        } else {
            SourceStatus::Missing
        };
        let conflict = has_sidecar && !matches!(source_status, SourceStatus::Unchanged);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        Some(FileBrowserEntry {
            path: path.to_path_buf(),
            name,
            sidecar_path,
            has_sidecar,
            source_status,
            conflict,
            virtual_copies,
            missing_models,
        })
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn idle_queue(&self) -> &IdleQueue {
        &self.idle_queue
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn preview(&self) -> Option<&ImageFrame> {
        self.preview.as_ref()
    }
    pub fn render_key(&self) -> Option<&RenderKey> {
        self.render_key.as_ref()
    }
    pub fn tone_analysis(&self) -> Option<lumina_core::ToneAnalysis> {
        self.tone_analysis
    }

    pub fn create_preset(&self, name: impl Into<String>) -> Result<Preset, GuiError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io("Presetname darf nicht leer sein".into()));
        }
        let mut recipe = EditRecipe::default();
        for (field, selected) in &self.preset_fields {
            if *selected {
                if let Some(value) = self.recipe.adjustments.get(field) {
                    recipe.adjustments.insert(field.clone(), *value);
                }
            }
        }
        if self.preset_relative_exposure {
            if !self.recipe.auto_features.enable_auto_tone {
                return Err(GuiError::Io(
                    "Relative Exposure erfordert aktives Auto-Tone".into(),
                ));
            }
            recipe
                .options
                .insert("exposure_semantics".into(), "relative".into());
        } else {
            recipe
                .options
                .insert("exposure_semantics".into(), "absolute".into());
        }
        Ok(Preset {
            id: format!("preset-{}", blake3::hash(name.as_bytes()).to_hex()),
            name,
            recipe,
            extras: BTreeMap::new(),
        })
    }

    pub fn apply_preset(&mut self, preset: &Preset) -> Result<(), GuiError> {
        if preset
            .recipe
            .options
            .get("exposure_semantics")
            .map(String::as_str)
            == Some("relative")
            && !self.recipe.auto_features.enable_auto_tone
        {
            return Err(GuiError::Io(
                "Relative Exposure erfordert aktives Auto-Tone".into(),
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let previous = self.recipe.clone();
        for (key, value) in &preset.recipe.adjustments {
            let value = if key == "exposure"
                && preset
                    .recipe
                    .options
                    .get("exposure_semantics")
                    .map(String::as_str)
                    == Some("relative")
            {
                self.recipe.adjustments.get(key).copied().unwrap_or(0.0) + value
            } else {
                *value
            };
            self.recipe.adjustments.insert(key.clone(), value);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(document) = &mut self.document {
            if let Some(copy) = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == self.virtual_copy_id)
            {
                let id = format!("history-{}", copy.history.len() + 1);
                copy.history.push(HistoryEntry {
                    id,
                    recipe: previous,
                    recorded_at: None,
                    extras: BTreeMap::new(),
                });
            }
        }
        self.render()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn duplicate_virtual_copy(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<(), GuiError> {
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io("Kein Sidecar geladen".into()));
        };
        document.duplicate_virtual_copy(&self.virtual_copy_id, id, name)?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn select_virtual_copy(&mut self, id: &str) -> Result<(), GuiError> {
        let Some(document) = &self.document else {
            return Err(GuiError::Io("Kein Sidecar geladen".into()));
        };
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == id)
            .ok_or_else(|| GuiError::Io("Virtuelle Kopie nicht gefunden".into()))?;
        self.virtual_copy_id = copy.id.clone();
        self.recipe = copy.recipe.clone();
        self.selected_mask_id = copy
            .mask_layers
            .first()
            .map(|layer| layer.mask.mask_id.clone());
        self.render()
    }

    /// Select a mask from the active copy's library and make it the active layer.
    /// The matte is only referenced; no payload is copied or modified.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn select_mask(&mut self, mask_id: &str) -> Result<(), GuiError> {
        self.ensure_document_loaded()?;
        let document = self.document.as_mut().expect("document was ensured");
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| GuiError::Io("Virtuelle Kopie nicht gefunden".into()))?;
        if !copy.mask_library.iter().any(|mask| mask.id == mask_id) {
            return Err(GuiError::Io("Maske nicht gefunden".into()));
        }
        if let Some(layer) = copy.mask_layers.first_mut() {
            layer.mask = MaskReference {
                copy_id: copy.id.clone(),
                mask_id: mask_id.into(),
                extras: BTreeMap::new(),
            };
        } else {
            copy.mask_layers.push(MaskLayer {
                id: "layer-1".into(),
                mask: MaskReference {
                    copy_id: copy.id.clone(),
                    mask_id: mask_id.into(),
                    extras: BTreeMap::new(),
                },
                inverted: false,
                feather: 0.0,
                blur: 0.0,
                density: 1.0,
                extras: BTreeMap::new(),
            });
        }
        self.selected_mask_id = Some(mask_id.into());
        self.render_key = None;
        self.status = format!("Maske ausgewählt: {mask_id}");
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn selected_mask_id(&self) -> Option<&str> {
        self.selected_mask_id.as_deref()
    }

    /// Create a pending library entry. Inference is deliberately not started here.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_mask(&mut self, name: impl Into<String>) -> Result<String, GuiError> {
        self.ensure_document_loaded()?;
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io("Maskenname darf nicht leer sein".into()));
        }
        let id = format!("mask-{}", blake3::hash(name.as_bytes()).to_hex());
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io("Kein Bild geladen".into()))?
            .clone();
        let source_hash = self
            .source_bytes
            .as_ref()
            .map(|b| format!("blake3:{}", blake3::hash(b).to_hex()))
            .unwrap_or_else(|| "blake3:unknown".into());
        let source_byte_length = self.source_bytes.as_ref().map_or(0, |b| b.len() as u64);
        let document = self.document.as_mut().expect("document was ensured");
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| GuiError::Io("Virtuelle Kopie nicht gefunden".into()))?;
        if copy.mask_library.iter().any(|mask| mask.id == id) {
            return Err(GuiError::Io(
                "Eine Maske mit diesem Namen existiert bereits".into(),
            ));
        }
        copy.mask_library.push(MaskDefinition {
            id: id.clone(),
            name,
            source_fingerprint: SourceFingerprint {
                content_hash: source_hash,
                byte_length: source_byte_length,
                extras: BTreeMap::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "pending".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_context: GeometryFingerprint {
                width: frame.width,
                height: frame.height,
                orientation: self.raw_orientation,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            model: ModelIdentity {
                name: "unavailable".into(),
                version: "pending".into(),
                hash: "pending".into(),
                extras: BTreeMap::new(),
            },
            inference_resolution: Resolution {
                width: frame.width,
                height: frame.height,
                extras: BTreeMap::new(),
            },
            preprocessing: Preprocessing {
                name: "pending".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Pending,
            created_at: "pending".into(),
            generator_version: env!("CARGO_PKG_VERSION").into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Source,
            references: vec![],
            extras: BTreeMap::new(),
        });
        self.select_mask(&id)?;
        self.status = "Maske angelegt; Neuberechnung ausdrücklich erforderlich".into();
        Ok(id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn rename_mask(&mut self, mask_id: &str, name: impl Into<String>) -> Result<(), GuiError> {
        self.ensure_document_loaded()?;
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io("Maskenname darf nicht leer sein".into()));
        }
        let copy = self.active_copy_mut()?;
        let mask = copy
            .mask_library
            .iter_mut()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io("Maske nicht gefunden".into()))?;
        mask.name = name;
        self.status = "Maske umbenannt; Sidecar speichern".into();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_inverted(&mut self, inverted: bool) -> Result<(), GuiError> {
        let layer = self.active_layer_mut()?;
        layer.inverted = inverted;
        self.render_key = None;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_feather(&mut self, feather: f32) -> Result<(), GuiError> {
        if !feather.is_finite() || !(0.0..=1.0).contains(&feather) {
            return Err(GuiError::Io(
                "Feathering muss zwischen 0 und 1 liegen".into(),
            ));
        }
        self.active_layer_mut()?.feather = feather;
        self.render_key = None;
        Ok(())
    }

    /// Store a local adjustment as declarative layer metadata. Applying it to pixels
    /// requires the not-yet-implemented masked core pipeline; it is never baked in.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_local_adjustment(&mut self, key: &str, value: f64) -> Result<(), GuiError> {
        if !matches!(key, "exposure" | "contrast" | "highlights" | "shadows") || !value.is_finite()
        {
            return Err(GuiError::Io("Ungültige lokale Anpassung".into()));
        }
        self.active_layer_mut()?
            .extras
            .insert(format!("adjustment_{key}"), Value::from(value));
        self.status =
            "Lokale Maskenanpassung gespeichert (Pipeline-Unterstützung ausstehend)".into();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn offer_mask_recalculation(&mut self) -> Result<bool, GuiError> {
        let mask_id = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io("Keine Maske ausgewählt".into()))?;
        let mask = self
            .active_copy_mut()?
            .mask_library
            .iter()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io("Maske nicht gefunden".into()))?;
        let offered = !matches!(mask.status, MaskStatus::Valid);
        self.status = if offered {
            "Maske veraltet/nicht verfügbar; Neuberechnung starten?"
        } else {
            "Maske aktuell; keine Neuberechnung erforderlich"
        }
        .into();
        Ok(offered)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn mark_mask_for_recalculation(&mut self) -> Result<(), GuiError> {
        let mask_id = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io("Keine Maske ausgewählt".into()))?;
        let mask = self
            .active_copy_mut()?
            .mask_library
            .iter_mut()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io("Maske nicht gefunden".into()))?;
        mask.status = MaskStatus::Pending;
        mask.error_text = Some("Explizite Neuberechnung angefordert".into());
        let queued = self.idle_queue.enqueue(
            IdleTask::MaskInference {
                mask_id: mask_id.clone(),
            },
            100,
        );
        if queued.is_none() {
            return Err(GuiError::Io("Idle-Warteschlange ist voll".into()));
        }
        self.status = "Neuberechnung angefordert; Jobsteuerung erforderlich".into();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_document_loaded(&mut self) -> Result<(), GuiError> {
        if self.document.is_none() {
            let frame = self
                .original
                .as_ref()
                .ok_or_else(|| GuiError::Io("Kein Bild geladen".into()))?
                .clone();
            self.document = Some(SidecarDocument::new(
                self.source_identity(&frame),
                "raster-mvp-1",
            ));
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn active_copy_mut(&mut self) -> Result<&mut lumina_sidecar::VirtualCopy, GuiError> {
        self.document
            .as_mut()
            .and_then(|d| {
                d.virtual_copies
                    .iter_mut()
                    .find(|c| c.id == self.virtual_copy_id)
            })
            .ok_or_else(|| GuiError::Io("Virtuelle Kopie nicht gefunden".into()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn active_layer_mut(&mut self) -> Result<&mut MaskLayer, GuiError> {
        self.active_copy_mut()?
            .mask_layers
            .first_mut()
            .ok_or_else(|| GuiError::Io("Keine Maske ausgewählt".into()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_adjustment_to_selection(
        paths: &[std::path::PathBuf],
        key: &str,
        value: f64,
    ) -> Result<usize, GuiError> {
        if !matches!(key, "exposure" | "contrast" | "highlights" | "shadows") {
            return Err(GuiError::Io(format!("Unbekannte Anpassung: {key}")));
        }
        let mut changed = 0;
        for path in paths {
            let sidecar_path = lumina_sidecar::sidecar_path_for(path);
            let mut document = lumina_sidecar::load_sidecar(&sidecar_path)?;
            let Some(copy) = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.is_default)
            else {
                continue;
            };
            copy.recipe.adjustments.insert(key.into(), value);
            copy.history.push(HistoryEntry {
                id: format!("selection-{changed}"),
                recipe: copy.recipe.clone(),
                recorded_at: None,
                extras: BTreeMap::new(),
            });
            lumina_sidecar::save_sidecar(&sidecar_path, &document)?;
            changed += 1;
        }
        Ok(changed)
    }

    pub fn load_bytes(&mut self, bytes: Vec<u8>, name: impl Into<String>) -> Result<(), GuiError> {
        let name = name.into();
        let source_is_raw = is_raw_name(&name);
        let (frame, orientation) = if source_is_raw {
            let image = lumina_raw::decode_bytes(&bytes, &name)?;
            (image.frame, image.metadata.orientation)
        } else {
            (ImageFrame::decode(&bytes)?, 1)
        };
        self.source_name = name;
        self.source_bytes = Some(bytes);
        self.source_is_raw = source_is_raw;
        self.raw_orientation = orientation;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.document = None;
            self.virtual_copy_id = "vc-original".into();
            self.selected_mask_id = None;
        }
        self.original = Some(frame);
        self.recipe = EditRecipe::default();
        self.error = None;
        self.status = format!("Geladen: {}", self.source_name);
        self.render()
    }

    pub fn set_adjustment(&mut self, name: &str, value: f64) {
        self.recipe.adjustments.insert(name.into(), value);
        self.render_key = None;
        self.tone_analysis = None;
        self.status = "Änderung ausstehend".into();
        self.error = None;
    }

    pub fn auto_tone(&mut self) -> Result<(), GuiError> {
        let Some(frame) = &self.original else {
            return Ok(());
        };
        let config = AutoToneConfig {
            target_luminance: self.recipe.auto_features.target_luminance,
            ..Default::default()
        };
        let result = suggest_auto_tone(frame, config)?;
        self.recipe
            .adjustments
            .insert("exposure".into(), result.exposure);
        self.recipe
            .adjustments
            .insert("contrast".into(), result.contrast);
        self.recipe.auto_features.enable_auto_tone = true;
        self.recipe.auto_features.auto_exposure = Some(result.exposure);
        self.recipe.auto_features.auto_contrast = Some(result.contrast);
        self.recipe.auto_features.analysis_fingerprint = Some(AnalysisFingerprint {
            algorithm: "tone-rgba8-rec709".into(),
            version: "1".into(),
            input_fingerprint: tone_fingerprint(frame, config),
            extras: BTreeMap::new(),
        });
        self.render()
    }

    pub fn match_total_exposure(&mut self, target: f64) -> Result<(), GuiError> {
        let Some(frame) = &self.preview else {
            return Ok(());
        };
        let value = match_total_exposure(frame, target)?;
        let exposure = self
            .recipe
            .adjustments
            .get("exposure")
            .copied()
            .unwrap_or(0.0)
            + value;
        self.recipe.adjustments.insert("exposure".into(), exposure);
        self.recipe.auto_features.match_total_exposure = true;
        self.recipe.auto_features.target_luminance = target;
        self.recipe.auto_features.matched_exposure = Some(value);
        self.render()
    }

    pub fn reset(&mut self) {
        self.recipe = EditRecipe::default();
        if self.original.is_some() {
            let _ = self.render();
        }
    }

    pub fn render(&mut self) -> Result<(), GuiError> {
        let Some(original) = &self.original else {
            self.status = "Kein Bild geladen".into();
            return Ok(());
        };
        let mut preview = original.clone();
        preview.apply_recipe(&self.recipe)?;
        let source_hash = self
            .source_bytes
            .as_ref()
            .map(|bytes| format!("blake3:{}", blake3::hash(bytes).to_hex()))
            .unwrap_or_else(|| "blake3:unknown".into());
        let copy_id = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.virtual_copy_id.clone()
            }
            #[cfg(target_arch = "wasm32")]
            {
                "vc-original".to_owned()
            }
        };
        let mask_hashes = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.document
                    .as_ref()
                    .and_then(|d| {
                        d.virtual_copies
                            .iter()
                            .find(|c| c.id == self.virtual_copy_id)
                    })
                    .map(|c| {
                        c.mask_library
                            .iter()
                            .filter_map(|m| m.artifact.as_ref().map(|a| a.checksum.clone()))
                            .collect()
                    })
                    .unwrap_or_default()
            }
            #[cfg(target_arch = "wasm32")]
            {
                Vec::new()
            }
        };
        self.render_key = Some(RenderKey::new(
            source_hash,
            env!("CARGO_PKG_VERSION"),
            "raster-mvp-1",
            copy_id,
            &self.recipe,
            mask_hashes,
            "sRGB",
            preview.width,
            preview.height,
            "rgba8",
        ));
        self.tone_analysis = Some(analyze_tone(&preview));
        self.preview = Some(preview);
        self.error = None;
        self.status = if self.active_mask_needs_attention() {
            "Warnung: Maske nicht verfügbar; sie wird in der Vorschau nicht angewendet".into()
        } else {
            "Vorschau aktuell".into()
        };
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn active_mask_needs_attention(&self) -> bool {
        let Some(document) = &self.document else {
            return false;
        };
        let Some(copy) = document
            .virtual_copies
            .iter()
            .find(|c| c.id == self.virtual_copy_id)
        else {
            return false;
        };
        copy.mask_layers.iter().any(|layer| {
            let Some(mask) = copy
                .mask_library
                .iter()
                .find(|m| m.id == layer.mask.mask_id)
            else {
                return true;
            };
            if !matches!(mask.status, MaskStatus::Valid) {
                return true;
            }
            let Some(artifact) = &mask.artifact else {
                return true;
            };
            lumina_sidecar::artifact_status(
                Path::new(&self.path)
                    .parent()
                    .unwrap_or_else(|| Path::new(".")),
                artifact,
            ) != ArtifactStatus::Available
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn active_mask_needs_attention(&self) -> bool {
        false
    }

    fn show_error(&mut self, error: impl ToString) {
        let message = error.to_string();
        self.status = "Fehler".into();
        self.error = Some(message);
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        if let Some(frame) = &self.preview {
            let size = [frame.width as usize, frame.height as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
            self.texture =
                Some(ctx.load_texture("lumina-preview", image, egui::TextureOptions::LINEAR));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn source_identity(&self, frame: &ImageFrame) -> SourceIdentity {
        SourceIdentity {
            relative_name: if self.source_name.is_empty() {
                "dropped-image".into()
            } else {
                self.source_name.clone()
            },
            content_hash: self
                .source_bytes
                .as_ref()
                .map(|bytes| format!("blake3:{}", blake3::hash(bytes).to_hex()))
                .unwrap_or_else(|| "blake3:unknown".into()),
            byte_length: self
                .source_bytes
                .as_ref()
                .map_or(0, |bytes| bytes.len() as u64),
            modified_at: None,
            raw_format: Path::new(&self.source_name)
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or("raster")
                .to_ascii_uppercase(),
            orientation: self.raw_orientation,
            decode_fingerprint: DecodeFingerprint {
                decoder: decoder_identity(self.source_is_raw).into(),
                version: env!("CARGO_PKG_VERSION").into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: frame.width,
                height: frame.height,
                orientation: self.raw_orientation,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            extras: BTreeMap::new(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_path(&mut self) {
        let path = std::path::PathBuf::from(self.path.trim());
        match std::fs::read(&path) {
            Ok(bytes) => {
                if let Err(error) = self.load_bytes(
                    bytes,
                    path.file_name().and_then(|v| v.to_str()).unwrap_or("image"),
                ) {
                    self.show_error(error);
                } else if let Ok(document) =
                    lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&path))
                {
                    let candidate = document.virtual_copies[0].recipe.clone();
                    self.document = Some(document);
                    let config = AutoToneConfig {
                        target_luminance: candidate.auto_features.target_luminance,
                        ..Default::default()
                    };
                    let fingerprint =
                        tone_fingerprint(self.original.as_ref().expect("loaded frame"), config);
                    let valid = candidate
                        .auto_features
                        .analysis_fingerprint
                        .as_ref()
                        .is_some_and(|stored| is_current_tone_analysis(stored, &fingerprint));
                    self.recipe = candidate;
                    let stale_auto_tone = self.recipe.auto_features.enable_auto_tone && !valid;
                    if stale_auto_tone {
                        clear_stale_auto_tone(&mut self.recipe);
                        self.status = "Auto-Tone veraltet; Neuberechnung erforderlich".into();
                    }
                    if let Err(error) = self.render() {
                        self.show_error(error);
                    } else if stale_auto_tone {
                        self.status = "Auto-Tone veraltet; Neuberechnung erforderlich".into();
                    }
                }
            }
            Err(error) => self.show_error(GuiError::Io(format!("{}: {}", path.display(), error))),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_sidecar(&mut self) {
        if self.path.trim().is_empty() {
            self.show_error("Zum Speichern muss das Bild über einen lokalen Pfad geladen werden");
            return;
        }
        let path = std::path::PathBuf::from(self.path.trim());
        let Some(frame) = &self.original else {
            self.show_error("Kein Bild geladen");
            return;
        };
        let mut document = self
            .document
            .take()
            .unwrap_or_else(|| SidecarDocument::new(self.source_identity(frame), "raster-mvp-1"));
        document.source = self.source_identity(frame);
        let Some(copy) = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
        else {
            self.show_error("Virtuelle Kopie nicht gefunden");
            self.document = Some(document);
            return;
        };
        copy.recipe = self.recipe.clone();
        if let Err(error) =
            lumina_sidecar::save_sidecar(&lumina_sidecar::sidecar_path_for(&path), &document)
        {
            self.show_error(error);
        } else {
            self.status = "Sidecar gespeichert".into();
        }
        self.document = Some(document);
        self.list_directory();
        self.status = "Sidecar gespeichert".into();
    }

    fn draw_preview(&mut self, ui: &mut egui::Ui) {
        if let Some(texture) = &self.texture {
            let available = ui.available_size();
            let image = egui::Image::from_texture(texture).fit_to_fraction(available);
            ui.add(image);
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("Bild hierher ziehen oder einen Pfad laden");
            });
        }
    }

    fn draw_histogram(&self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Histogramm");
        if let Some(analysis) = self.tone_analysis {
            ui.label(format!(
                "Mittel {:.3}  Median {:.3}",
                analysis.mean, analysis.median
            ));
            ui.label(format!(
                "P01 {:.3}  P99 {:.3}  ({} Samples)",
                analysis.p01, analysis.p99, analysis.sample_count
            ));
            let width = ui.available_width().max(40.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 2.0, egui::Color32::from_gray(35));
            let left = rect.left() + rect.width() * analysis.p01 as f32;
            let right = rect.left() + rect.width() * analysis.p99 as f32;
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(left, rect.top()),
                    egui::pos2(right.max(left + 1.0), rect.bottom()),
                ),
                2.0,
                egui::Color32::LIGHT_GRAY,
            );
        } else {
            ui.label("Nicht aktuell");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn is_supported_image(path: &Path) -> bool {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "webp") => true,
        _ => is_raw_name(&path.display().to_string()),
    }
}

fn is_raw_name(name: &str) -> bool {
    matches!(
        std::path::Path::new(name)
            .extension()
            .and_then(|v| v.to_str())
            .map(|v| v.to_ascii_lowercase())
            .as_deref(),
        Some(
            "cr2"
                | "cr3"
                | "nef"
                | "arw"
                | "dng"
                | "orf"
                | "raf"
                | "rw2"
                | "crw"
                | "pef"
                | "srw"
                | "3fr"
                | "iiq"
                | "rwl"
                | "mos"
                | "erf"
                | "kdc"
                | "x3f"
        )
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_stale_auto_tone(recipe: &mut EditRecipe) {
    recipe.auto_features.auto_exposure = None;
    recipe.auto_features.auto_contrast = None;
    recipe.adjustments.remove("exposure");
    recipe.adjustments.remove("contrast");
}

#[cfg(not(target_arch = "wasm32"))]
fn is_current_tone_analysis(stored: &AnalysisFingerprint, input_fingerprint: &str) -> bool {
    stored.input_fingerprint == input_fingerprint
}

#[cfg(not(target_arch = "wasm32"))]
fn decoder_identity(source_is_raw: bool) -> &'static str {
    if source_is_raw {
        "libraw"
    } else {
        "image"
    }
}

impl eframe::App for LuminaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(not(target_arch = "wasm32"))]
        if !ctx.input(|input| input.pointer.any_down()) {
            // A queue item is consumed only while idle.  The current build has
            // no ONNX adapter, so consuming it records the hand-off point and
            // leaves inference itself to the future adapter.
            if let Some((_id, IdleTask::MaskInference { mask_id })) = self.idle_queue.pop_next() {
                self.status = format!("Maske {mask_id}: Hintergrundjob wartet auf Inferenz-Engine");
            }
        }
        for file in ctx.input(|input| input.raw.dropped_files.clone()) {
            if let Some(bytes) = file.bytes {
                if let Err(error) = self.load_bytes(bytes.to_vec(), file.name) {
                    self.show_error(error);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = file.path {
                self.path = path.display().to_string();
                self.load_path();
            }
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Lumina");
                ui.separator();
                ui.label(&self.status);
            });
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::RED, error);
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        egui::SidePanel::left("browser")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.heading("Datei-Browser");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.directory);
                    if ui.button("Öffnen").clicked() {
                        self.list_directory();
                    }
                });
                if ui.button("Aktualisieren").clicked() {
                    self.list_directory();
                }
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut path_to_open = None;
                    for entry in &self.entries {
                        let selected = self.path == entry.path.display().to_string();
                        let label = format!(
                            "{}  [{}] Kopien:{} Masken:{}",
                            entry.name,
                            entry.status_label(),
                            entry.virtual_copies,
                            entry.missing_models
                        );
                        if ui.selectable_label(selected, label).clicked() {
                            path_to_open = Some(entry.path.display().to_string());
                        }
                    }
                    if let Some(path) = path_to_open {
                        self.open_file(path);
                    }
                });
            });
        // Lightroom places the Develop controls on the right; the left side is
        // reserved for the browser and the central area for the navigator.
        egui::SidePanel::right("controls")
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Entwickeln");
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.path);
                        if ui.button("Load").clicked() {
                            self.load_path();
                        }
                    });
                    if ui.button("Datei auswählen").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            self.path = path.display().to_string();
                            self.load_path();
                        }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    ui.label("Web: Bild per Drag-and-drop laden");
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(document) = self.document.clone() {
                    let copy_count = document.virtual_copies.len();
                    let copy_options: Vec<(String, String)> = document
                        .virtual_copies
                        .iter()
                        .map(|copy| (copy.id.clone(), copy.name.clone()))
                        .collect();
                    let missing_masks = document
                        .virtual_copies
                        .iter()
                        .flat_map(|copy| &copy.mask_library)
                        .filter(|mask| !matches!(mask.status, lumina_sidecar::MaskStatus::Valid))
                        .count();
                    let source_status = lumina_sidecar::source_status(
                        std::path::Path::new(&self.path),
                        &document.source,
                    )
                    .ok();
                    ui.separator();
                    ui.label(format!("Sidecar: {} Kopien", copy_count));
                    if let Some(source_status) = source_status {
                        ui.label(format!("Quelle: {:?}", source_status));
                    }
                    let mut selected = self.virtual_copy_id.clone();
                    egui::ComboBox::from_label("Virtuelle Kopie")
                        .selected_text(selected.clone())
                        .show_ui(ui, |ui| {
                            for (id, name) in &copy_options {
                                ui.selectable_value(&mut selected, id.clone(), name);
                            }
                        });
                    if selected != self.virtual_copy_id {
                        let _ = self.select_virtual_copy(&selected);
                    }
                    if ui.button("Kopie duplizieren").clicked() {
                        let id = format!("vc-{}", copy_count + 1);
                        if let Err(error) = self.duplicate_virtual_copy(id, "Neue Kopie") {
                            self.show_error(error);
                        }
                    }
                    ui.label(format!("Masken: {} nicht verfügbar/aktuell", missing_masks));
                    ui.separator();
                    ui.collapsing("Maskierung", |ui| {
                        let mask_options: Vec<(String, String)> = document
                            .virtual_copies
                            .iter()
                            .find(|copy| copy.id == self.virtual_copy_id)
                            .map(|copy| {
                                copy.mask_library
                                    .iter()
                                    .map(|mask| (mask.id.clone(), mask.name.clone()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let mut selected_mask = self.selected_mask_id.clone().unwrap_or_default();
                        egui::ComboBox::from_label("Maske auswählen")
                            .selected_text(
                                mask_options
                                    .iter()
                                    .find(|(id, _)| id == &selected_mask)
                                    .map(|(_, name)| name.as_str())
                                    .unwrap_or("Keine"),
                            )
                            .show_ui(ui, |ui| {
                                for (id, name) in &mask_options {
                                    ui.selectable_value(&mut selected_mask, id.clone(), name);
                                }
                            });
                        if selected_mask != self.selected_mask_id.clone().unwrap_or_default()
                            && !selected_mask.is_empty()
                        {
                            if let Err(error) = self.select_mask(&selected_mask) {
                                self.show_error(error);
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut self.mask_name_input);
                            if ui.button("Maske anlegen").clicked() {
                                if let Err(error) = self.create_mask(self.mask_name_input.clone()) {
                                    self.show_error(error);
                                } else {
                                    self.mask_name_input.clear();
                                }
                            }
                        });
                        if self.selected_mask_id.is_some() {
                            let mut inverted = document
                                .virtual_copies
                                .iter()
                                .find(|copy| copy.id == self.virtual_copy_id)
                                .and_then(|copy| copy.mask_layers.first())
                                .is_some_and(|layer| layer.inverted);
                            if ui.checkbox(&mut inverted, "Invertieren").changed() {
                                if let Err(error) = self.set_mask_inverted(inverted) {
                                    self.show_error(error);
                                }
                            }
                            let mut feather = document
                                .virtual_copies
                                .iter()
                                .find(|copy| copy.id == self.virtual_copy_id)
                                .and_then(|copy| copy.mask_layers.first())
                                .map_or(0.0, |layer| layer.feather);
                            if ui
                                .add(egui::Slider::new(&mut feather, 0.0..=1.0).text("Feathering"))
                                .changed()
                            {
                                if let Err(error) = self.set_mask_feather(feather) {
                                    self.show_error(error);
                                }
                            }
                            if ui.button("Neuberechnung anbieten").clicked() {
                                if let Err(error) =
                                    self.offer_mask_recalculation().and_then(|offered| {
                                        if offered {
                                            self.mark_mask_for_recalculation()
                                        } else {
                                            Ok(())
                                        }
                                    })
                                {
                                    self.show_error(error);
                                }
                            }
                            ui.label("Lokale Anpassungen der ausgewählten Maske");
                            for (key, label) in
                                [("exposure", "Belichtung"), ("contrast", "Kontrast")]
                            {
                                let stored = document
                                    .virtual_copies
                                    .iter()
                                    .find(|copy| copy.id == self.virtual_copy_id)
                                    .and_then(|copy| copy.mask_layers.first())
                                    .and_then(|layer| {
                                        layer.extras.get(&format!("adjustment_{key}"))
                                    })
                                    .and_then(Value::as_f64)
                                    .unwrap_or(0.0);
                                let range = if key == "exposure" {
                                    -10.0..=10.0
                                } else {
                                    -1.0..=1.0
                                };
                                let mut value = stored;
                                if ui
                                    .add(egui::Slider::new(&mut value, range).text(label))
                                    .changed()
                                {
                                    if let Err(error) = self.set_mask_local_adjustment(key, value) {
                                        self.show_error(error);
                                    }
                                }
                            }
                        }
                    });
                }
                let mut exposure = self
                    .recipe
                    .adjustments
                    .get("exposure")
                    .copied()
                    .unwrap_or(0.0);
                let mut contrast = self
                    .recipe
                    .adjustments
                    .get("contrast")
                    .copied()
                    .unwrap_or(0.0);
                if ui
                    .add(egui::Slider::new(&mut exposure, -10.0..=10.0).text("Exposure"))
                    .changed()
                {
                    self.set_adjustment("exposure", exposure);
                }
                if ui.button("Auto-Tone").clicked() {
                    if let Err(error) = self.auto_tone() {
                        self.show_error(error);
                    }
                }
                if ui.button("Match Total Exposure").clicked() {
                    if let Err(error) = self.match_total_exposure(0.5) {
                        self.show_error(error);
                    }
                }
                if ui
                    .add(egui::Slider::new(&mut contrast, -1.0..=1.0).text("Contrast"))
                    .changed()
                {
                    self.set_adjustment("contrast", contrast);
                }
                ui.horizontal(|ui| {
                    if ui.button("Reset").clicked() {
                        self.reset();
                    }
                    if ui.button("Render / Apply").clicked() {
                        if let Err(error) = self.render() {
                            self.show_error(error);
                        }
                    }
                });
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button("Save Recipe / Sidecar").clicked() {
                    self.save_sidecar();
                }
                ui.separator();
                ui.heading("Preset");
                ui.text_edit_singleline(&mut self.preset_name);
                for field in ["exposure", "contrast", "highlights", "shadows"] {
                    let selected = self.preset_fields.entry(field.into()).or_insert(false);
                    ui.checkbox(selected, field);
                }
                ui.checkbox(&mut self.preset_relative_exposure, "Exposure relativ");
                if ui.button("Preset erstellen und anwenden").clicked() {
                    match self
                        .create_preset(self.preset_name.clone())
                        .and_then(|preset| self.apply_preset(&preset))
                    {
                        Ok(()) => self.status = "Preset angewendet, neuer History-Schritt".into(),
                        Err(error) => self.show_error(error),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    ui.label("Browser-Dateispeichern ist im MVP noch nicht implementiert.");
                }
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.update_texture(ctx);
            self.draw_preview(ui);
            self.draw_histogram(ui);
            if let Some(key) = &self.render_key {
                ui.label(format!("Renderstand: {}", &key.digest()[..12]));
            } else {
                ui.colored_label(egui::Color32::YELLOW, "Renderstand veraltet / ausstehend");
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let canvas = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("lumina_canvas"))
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("Lumina canvas was not found"))?;
    wasm_bindgen_futures::spawn_local(async {
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(LuminaApp::new(cc.egui_ctx.clone())))),
            )
            .await;
        if let Err(error) = result {
            web_sys::console::error_1(&error);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::ImageFileFormat;
    use lumina_sidecar::{
        CoordinateSystem, DecodeFingerprint, GeometryFingerprint, MaskDefinition, MaskOperation,
        MaskStatus, ModelIdentity, Preprocessing, Resolution, SourceFingerprint, SourceStatus,
    };
    fn new_app() -> LuminaApp {
        LuminaApp::new(egui::Context::default())
    }
    fn png() -> Vec<u8> {
        ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }
    #[test]
    fn recipe_change_and_render() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("exposure", 1.0);
        app.render().unwrap();
        assert_eq!(app.recipe().adjustments["exposure"], 1.0);
        assert_eq!(app.preview().unwrap().pixels[0], 20);
    }
    #[test]
    fn auto_and_matching_use_core_and_persist_recipe_state() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.auto_tone().unwrap();
        assert!(app.recipe().auto_features.enable_auto_tone);
        app.match_total_exposure(0.5).unwrap();
        assert!(app.recipe().auto_features.match_total_exposure);
        assert!(app.recipe().auto_features.matched_exposure.is_some());
    }
    #[test]
    fn reset_restores_original_preview() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("contrast", 1.0);
        app.render().unwrap();
        app.reset();
        assert!(app.recipe().adjustments.is_empty());
        assert_eq!(app.preview().unwrap().pixels[0], 10);
    }

    #[test]
    fn render_key_is_invalidated_until_render() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        assert!(app.render_key().is_some());
        app.set_adjustment("exposure", 1.0);
        assert!(app.render_key().is_none());
        app.render().unwrap();
        assert!(app.render_key().is_some());
        assert_eq!(app.tone_analysis().unwrap().sample_count, 2);
    }

    #[test]
    fn preset_requires_name_and_validates_relative_exposure() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        assert!(app.create_preset("").is_err());
        app.preset_relative_exposure = true;
        assert!(app.create_preset("relative").is_err());
        app.recipe.auto_features.enable_auto_tone = true;
        let preset = app.create_preset("relative").unwrap();
        assert_eq!(preset.recipe.options["exposure_semantics"], "relative");
    }
    #[test]
    fn decode_error_is_visible() {
        let mut app = new_app();
        let result = app.load_bytes(vec![1, 2, 3], "bad.png");
        assert!(result.is_err());
        app.show_error(result.unwrap_err());
        assert_eq!(app.status(), "Fehler");
        assert!(app.error().is_some());
    }

    #[test]
    fn stale_auto_tone_clears_active_adjustments_but_keeps_status_state() {
        let mut recipe = EditRecipe::default();
        recipe.auto_features.enable_auto_tone = true;
        recipe.auto_features.auto_exposure = Some(1.25);
        recipe.auto_features.auto_contrast = Some(-0.2);
        recipe.adjustments.insert("exposure".into(), 1.25);
        recipe.adjustments.insert("contrast".into(), -0.2);
        recipe.adjustments.insert("highlights".into(), -0.5);

        clear_stale_auto_tone(&mut recipe);

        assert!(recipe.auto_features.enable_auto_tone);
        assert!(recipe.auto_features.auto_exposure.is_none());
        assert!(recipe.auto_features.auto_contrast.is_none());
        assert!(!recipe.adjustments.contains_key("exposure"));
        assert!(!recipe.adjustments.contains_key("contrast"));
        assert_eq!(recipe.adjustments["highlights"], -0.5);
    }

    #[test]
    fn stale_auto_tone_validation_checks_input_fingerprint() {
        let frame = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
        let input = tone_fingerprint(&frame, AutoToneConfig::default());
        let valid = AnalysisFingerprint {
            algorithm: "tone-rgba8-rec709".into(),
            version: "1".into(),
            input_fingerprint: input.clone(),
            extras: BTreeMap::new(),
        };
        assert!(is_current_tone_analysis(&valid, &input));
        for stored_input in [input.as_str(), "wrong"] {
            let stored = AnalysisFingerprint {
                algorithm: "arbitrary-pre-mvp-label".into(),
                version: "arbitrary-pre-mvp-value".into(),
                input_fingerprint: stored_input.into(),
                extras: BTreeMap::new(),
            };
            assert_eq!(
                is_current_tone_analysis(&stored, &input),
                stored_input == input
            );
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn sidecar_decoder_identity_distinguishes_raw_from_raster() {
        assert_eq!(decoder_identity(true), "libraw");
        assert_eq!(decoder_identity(false), "image");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_png(path: &Path) {
        let png = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        std::fs::write(path, png).unwrap();
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gui_writes_sidecar_and_restores_recipe_on_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.set_adjustment("exposure", 1.5);
        app.save_sidecar();
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        assert!(sidecar.is_file(), "Sidecar muss geschrieben werden");
        let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        assert_eq!(
            document.virtual_copies[0].recipe.adjustments["exposure"],
            1.5
        );

        let mut reopened = new_app();
        reopened.open_file(source.display().to_string());
        assert_eq!(reopened.recipe().adjustments["exposure"], 1.5);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gui_persists_virtual_copies_across_save_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.set_adjustment("contrast", 0.3);
        app.save_sidecar();
        app.duplicate_virtual_copy("vc-2", "Kopie 2").unwrap();
        app.save_sidecar();
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        assert_eq!(document.virtual_copies.len(), 2);
        assert!(document
            .virtual_copies
            .iter()
            .any(|copy| copy.id == "vc-2" && copy.name == "Kopie 2"));
        assert_eq!(
            document.virtual_copies[0].recipe.adjustments["contrast"],
            0.3
        );

        let mut reopened = new_app();
        reopened.open_file(source.display().to_string());
        assert_eq!(reopened.entries().len(), 1);
        let reloaded = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        assert_eq!(reloaded.virtual_copies.len(), 2);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn file_browser_index_reports_sidecar_and_copy_count() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.save_sidecar();
        app.duplicate_virtual_copy("vc-2", "Zwei").unwrap();
        app.duplicate_virtual_copy("vc-3", "Drei").unwrap();
        app.save_sidecar();
        app.set_directory(directory.path().display().to_string());
        let entry = app
            .entries()
            .iter()
            .find(|e| e.name == "photo.png")
            .unwrap();
        assert!(entry.has_sidecar);
        assert_eq!(entry.virtual_copies, 3);
        assert!(!entry.conflict);
        assert!(!entry.is_offline());
        assert_eq!(entry.status_label(), "Sidecar");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn file_browser_detects_offline_source_and_conflict() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.save_sidecar();
        app.set_directory(directory.path().display().to_string());
        let entry = app
            .entries()
            .iter()
            .find(|e| e.name == "photo.png")
            .unwrap();
        assert!(!entry.is_offline());
        assert!(!entry.conflict);

        std::fs::remove_file(&source).unwrap();
        app.set_directory(directory.path().display().to_string());
        let entry = app
            .entries()
            .iter()
            .find(|e| e.name == "photo.png")
            .unwrap();
        assert!(entry.is_offline());
        assert!(entry.conflict);
        assert_eq!(entry.source_status, SourceStatus::Missing);
        assert_eq!(entry.status_label(), "Konflikt");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn file_browser_reports_missing_mask_models() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.save_sidecar();
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        let mut document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        document.virtual_copies[0]
            .mask_library
            .push(MaskDefinition {
                id: "m1".into(),
                name: "subject".into(),
                source_fingerprint: SourceFingerprint {
                    content_hash: "blake3:x".into(),
                    byte_length: 1,
                    extras: BTreeMap::new(),
                },
                decode_context: DecodeFingerprint {
                    decoder: "libraw".into(),
                    version: "1".into(),
                    parameters: BTreeMap::new(),
                    extras: BTreeMap::new(),
                },
                geometry_context: GeometryFingerprint {
                    width: 2,
                    height: 1,
                    orientation: 1,
                    pixel_aspect_ratio: 1.0,
                    extras: BTreeMap::new(),
                },
                model: ModelIdentity {
                    name: "birefnet".into(),
                    version: "1".into(),
                    hash: "h".into(),
                    extras: BTreeMap::new(),
                },
                inference_resolution: Resolution {
                    width: 2,
                    height: 1,
                    extras: BTreeMap::new(),
                },
                preprocessing: Preprocessing {
                    name: "std".into(),
                    version: "1".into(),
                    parameters: BTreeMap::new(),
                    extras: BTreeMap::new(),
                },
                rescaling_method: "bilinear".into(),
                rescaling_parameters: BTreeMap::new(),
                coordinate_system: CoordinateSystem::SourceOriented,
                status: MaskStatus::Missing,
                created_at: "2026-01-01T00:00:00Z".into(),
                generator_version: "g".into(),
                error_text: None,
                artifact: None,
                operation: MaskOperation::Source,
                references: vec![],
                extras: BTreeMap::new(),
            });
        lumina_sidecar::save_sidecar(&sidecar, &document).unwrap();
        app.set_directory(directory.path().display().to_string());
        let entry = app
            .entries()
            .iter()
            .find(|e| e.name == "photo.png")
            .unwrap();
        assert_eq!(entry.missing_models, 1);
        assert_eq!(entry.has_sidecar, true);

        let reloaded = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        assert_eq!(reloaded.virtual_copies[0].mask_library.len(), 1);
        assert_eq!(
            reloaded.virtual_copies[0].mask_library[0].status,
            MaskStatus::Missing
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn mask_selection_and_name_roundtrip_through_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        let id = app.create_mask("Subject").unwrap();
        app.rename_mask(&id, "Main subject").unwrap();
        app.save_sidecar();

        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
        assert_eq!(
            document.virtual_copies[0].mask_library[0].name,
            "Main subject"
        );
        assert_eq!(document.virtual_copies[0].mask_layers[0].mask.mask_id, id);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn mask_layer_parameters_are_non_destructive_and_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.create_mask("Subject").unwrap();
        app.set_mask_inverted(true).unwrap();
        app.set_mask_feather(0.25).unwrap();
        app.save_sidecar();

        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
        let layer = &document.virtual_copies[0].mask_layers[0];
        assert!(layer.inverted);
        assert_eq!(layer.feather, 0.25);
        assert_eq!(
            document.virtual_copies[0].mask_library[0].status,
            MaskStatus::Pending
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn stale_mask_offers_recalculation_without_running_inference() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.create_mask("Subject").unwrap();
        assert!(app.offer_mask_recalculation().unwrap());
        assert!(app.status().contains("Neuberechnung"));
        app.mark_mask_for_recalculation().unwrap();
        assert!(app.status().contains("angefordert"));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn local_mask_adjustments_roundtrip_through_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.create_mask("Subject").unwrap();
        app.set_mask_local_adjustment("exposure", 1.25).unwrap();
        app.set_mask_local_adjustment("contrast", -0.35).unwrap();
        app.save_sidecar();

        let mut reopened = new_app();
        reopened.open_file(source.display().to_string());
        let layer = &reopened.document.as_ref().unwrap().virtual_copies[0].mask_layers[0];
        assert_eq!(layer.extras["adjustment_exposure"].as_f64(), Some(1.25));
        assert_eq!(layer.extras["adjustment_contrast"].as_f64(), Some(-0.35));
    }

    #[test]
    fn idle_queue_is_bounded_prioritized_and_cancellable() {
        let mut queue = IdleQueue::new(2);
        let low = queue
            .enqueue(
                IdleTask::MaskInference {
                    mask_id: "low".into(),
                },
                1,
            )
            .unwrap();
        queue
            .enqueue(
                IdleTask::MaskInference {
                    mask_id: "high".into(),
                },
                9,
            )
            .unwrap();
        assert!(queue
            .enqueue(
                IdleTask::MaskInference {
                    mask_id: "full".into()
                },
                9
            )
            .is_none());
        assert!(queue.cancel(low));
        assert_eq!(
            queue.pop_next().unwrap().1,
            IdleTask::MaskInference {
                mask_id: "high".into()
            }
        );
        assert!(queue.is_empty());
    }
}
