//! Shared eframe application for the native and browser MVP.

mod filmstrip;
mod i18n;
mod slider;
mod theme;

use eframe::egui;
#[cfg(not(target_arch = "wasm32"))]
use lumina_core::cache::disk::DiskFolderCache;
#[cfg(not(target_arch = "wasm32"))]
use lumina_core::cache::PreviewKind;
#[cfg(not(target_arch = "wasm32"))]
use lumina_core::MaskPolicy;
use lumina_core::{
    analyze_tone, export_image, masks::rasterize_prompt, match_total_exposure_masked, render_frame,
    suggest_auto_tone, tone_fingerprint, AutoToneConfig, ExportOptions, ImageFileFormat,
    ImageFrame, MaskContext, MaskLayerResult, MaskPlane, OutputSpec, RenderContext, RenderKey,
};
use lumina_raw::RawError;
#[cfg(not(target_arch = "wasm32"))]
use lumina_sidecar::{
    load_zdata, zdata_path_for, ArtifactStatus, BrushMark, BrushMarkSign, CoordinateSystem,
    DecodeFingerprint, GeometryFingerprint, HistoryEntry, MaskDefinition, MaskLayer, MaskOperation,
    MaskPrompt, MaskReference, MaskStatus, ModelIdentity, Point2, Preprocessing, PromptTransform,
    Resolution, SidecarDocument, SourceFingerprint, SourceIdentity, SourceStatus,
};
use lumina_sidecar::{
    AnalysisFingerprint, ColorGrading, ColorGradingRange, CurveChannels, CurvePoint, Curves,
    EditRecipe, Effects, Geometry, Grain, HslAdjustments, HslChannel, LensCorrection,
    NoiseReduction, Perspective, Preset, Sharpening, Vignette,
};
#[cfg(not(target_arch = "wasm32"))]
use serde_json::Value;
use slider::{identity_spec, lr_slider, percent_spec, SliderAction, SliderSpec};
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

use filmstrip::{downscale_rgba, ThumbnailManager, THUMBNAIL_MAX_DIM};
use i18n::Str;
use theme::apply_lightroom_dark;

/// Work which may be performed when the GUI has no interactive input.
///
/// Queueing is deliberately separate from mask status: a missing/pending mask
/// is never inserted here implicitly.  The caller must enqueue it as the
/// result of an explicit user action (or a future CLI/GUI command).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdleTask {
    MaskInference {
        mask_id: String,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Thumbnail {
        source: PathBuf,
        name: String,
    },
}

/// Top-level module selected in the module bar (Library / Develop / Export).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Library,
    Develop,
    Export,
}

/// Active interactive masking tool (F-103-N4). `None` means the preview accepts
/// the ordinary click/eyedropper interactions; any other variant arms the
/// preview for a drag gesture that builds a [`MaskPrompt`] for the selected
/// mask.  The tool only chooses *how* the drag is interpreted; persistence goes
/// through the existing sidecar paths.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskTool {
    #[default]
    None,
    Brush,
    LinearGradient,
    Radial,
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
    camera_white_balance: Option<[f32; 4]>,
    source_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    path: String,
    #[cfg(not(target_arch = "wasm32"))]
    directory: String,
    #[cfg(not(target_arch = "wasm32"))]
    entries: Vec<FileBrowserEntry>,
    recipe: EditRecipe,
    texture: Option<egui::TextureHandle>,
    status: String,
    error: Option<String>,
    render_key: Option<RenderKey>,
    tone_analysis: Option<lumina_core::ToneAnalysis>,
    /// Effective mask layers of the last [`Self::render`] (F-041): the
    /// measurement domain of `Match Total Exposure` is the rendered preview
    /// weighted by these planes. Empty on wasm32 (no mask context, documented
    /// post-MVP state) and whenever the render produced no layers.
    render_mask_layers: Vec<MaskLayerResult>,
    #[cfg(not(target_arch = "wasm32"))]
    document: Option<SidecarDocument>,
    #[cfg(not(target_arch = "wasm32"))]
    virtual_copy_id: String,
    #[cfg(not(target_arch = "wasm32"))]
    selected_mask_id: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    mask_name_input: String,
    #[cfg(not(target_arch = "wasm32"))]
    mask_tool: MaskTool,
    /// Normalized brush radius (0..=1 in source space). Driven by a slider.
    #[cfg(not(target_arch = "wasm32"))]
    brush_radius: f32,
    /// When true, brush marks use the negative (eraser) sign.
    #[cfg(not(target_arch = "wasm32"))]
    brush_eraser: bool,
    /// Marks accumulated during an in-progress brush drag (cleared on release).
    #[cfg(not(target_arch = "wasm32"))]
    pending_brush_marks: Vec<BrushMark>,
    /// Drag start/current normalized points for gradient/radial gestures.
    #[cfg(not(target_arch = "wasm32"))]
    drag_start: Option<Point2>,
    #[cfg(not(target_arch = "wasm32"))]
    drag_current: Option<Point2>,
    /// True while a mask-tool drag is in progress (drives the live overlay).
    #[cfg(not(target_arch = "wasm32"))]
    drawing: bool,
    preset_name: String,
    preset_fields: BTreeMap<String, bool>,
    preset_relative_exposure: bool,
    idle_queue: IdleQueue,
    /// Active top-level module (Library / Develop / Export).
    active_module: Module,
    /// Export module UI state (F-103-N5). The target path is chosen via a
    /// native save dialog; the format/quality drive the shared export path.
    export_path: String,
    export_format: ImageFileFormat,
    export_quality: u8,
    /// Before/After toggle state. Never mutates the recipe.
    before_after: bool,
    /// White-balance eyedropper armed state.
    wb_pick_mode: bool,
    /// Generated filmstrip thumbnail textures.
    thumbnails: ThumbnailManager,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct FileBrowserEntry {
    path: PathBuf,
    name: String,
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
            Str::StatusConflict.t()
        } else if self.is_offline() {
            Str::StatusOffline.t()
        } else if self.has_sidecar {
            Str::Sidecar.t()
        } else {
            Str::StatusWithout.t()
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
            camera_white_balance: None,
            source_name: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            path: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            directory: ".".into(),
            #[cfg(not(target_arch = "wasm32"))]
            entries: Vec::new(),
            recipe: EditRecipe::default(),
            texture: None,
            status: Str::ReadyForImage.t().into(),
            error: None,
            render_key: None,
            tone_analysis: None,
            render_mask_layers: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            document: None,
            #[cfg(not(target_arch = "wasm32"))]
            virtual_copy_id: "vc-original".into(),
            #[cfg(not(target_arch = "wasm32"))]
            selected_mask_id: None,
            #[cfg(not(target_arch = "wasm32"))]
            mask_name_input: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            mask_tool: MaskTool::None,
            #[cfg(not(target_arch = "wasm32"))]
            brush_radius: 0.05,
            #[cfg(not(target_arch = "wasm32"))]
            brush_eraser: false,
            #[cfg(not(target_arch = "wasm32"))]
            pending_brush_marks: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            drag_start: None,
            #[cfg(not(target_arch = "wasm32"))]
            drag_current: None,
            #[cfg(not(target_arch = "wasm32"))]
            drawing: false,
            preset_name: String::new(),
            preset_fields: BTreeMap::from([
                ("exposure".into(), true),
                ("contrast".into(), true),
                ("highlights".into(), false),
                ("shadows".into(), false),
            ]),
            preset_relative_exposure: false,
            idle_queue: IdleQueue::new(32),
            active_module: Module::Develop,
            export_path: String::new(),
            export_format: ImageFileFormat::Png,
            export_quality: 90,
            before_after: false,
            wb_pick_mode: false,
            thumbnails: ThumbnailManager::new(),
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
                self.status = Str::ImagesInDirectory.format_arg(&self.entries.len().to_string());
            }
            Err(error) => {
                self.status = Str::DirectoryNotReadable.format_arg(&error.to_string());
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
                            let artifact_missing = mask.artifact.as_ref().is_some_and(|artifact| {
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
            return Err(GuiError::Io(Str::PresetNameEmpty.t().to_string()));
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
                    Str::RelativeExposureRequiresAutoTone.t().to_string(),
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
                Str::RelativeExposureRequiresAutoTone.t().to_string(),
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
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        document.duplicate_virtual_copy(&self.virtual_copy_id, id, name)?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn select_virtual_copy(&mut self, id: &str) -> Result<(), GuiError> {
        let Some(document) = &self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
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
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        if !copy.mask_library.iter().any(|mask| mask.id == mask_id) {
            return Err(GuiError::Io(Str::MaskNotFound.t().to_string()));
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
        self.status = Str::MaskSelected.format_arg(mask_id);
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
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        let id = format!("mask-{}", blake3::hash(name.as_bytes()).to_hex());
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
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
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        if copy.mask_library.iter().any(|mask| mask.id == id) {
            return Err(GuiError::Io(Str::MaskNameExists.t().to_string()));
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
            prompt: None,
            extras: BTreeMap::new(),
        });
        self.select_mask(&id)?;
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn rename_mask(&mut self, mask_id: &str, name: impl Into<String>) -> Result<(), GuiError> {
        self.ensure_document_loaded()?;
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        let copy = self.active_copy_mut()?;
        let mask = copy
            .mask_library
            .iter_mut()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        mask.name = name;
        self.status = Str::MaskRenamed.t().into();
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
            return Err(GuiError::Io(Str::FeatheringMustBeBetween.t().to_string()));
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
            return Err(GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()));
        }
        self.active_layer_mut()?
            .extras
            .insert(format!("adjustment_{key}"), Value::from(value));
        self.status = Str::LocalAdjustmentSaved.t().to_string();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn offer_mask_recalculation(&mut self) -> Result<bool, GuiError> {
        let mask_id = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        let mask = self
            .active_copy_mut()?
            .mask_library
            .iter()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        let offered = !matches!(mask.status, MaskStatus::Valid);
        self.status = if offered {
            Str::MaskStaleRecalc.t()
        } else {
            Str::MaskCurrentNoRecalc.t()
        }
        .into();
        Ok(offered)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn mark_mask_for_recalculation(&mut self) -> Result<(), GuiError> {
        let mask_id = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        let mask = self
            .active_copy_mut()?
            .mask_library
            .iter_mut()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        mask.status = MaskStatus::Pending;
        mask.error_text = Some(Str::ExplicitRecalcRequested.t().to_string());
        let queued = self.idle_queue.enqueue(
            IdleTask::MaskInference {
                mask_id: mask_id.clone(),
            },
            100,
        );
        if queued.is_none() {
            return Err(GuiError::Io(Str::IdleQueueFull.t().to_string()));
        }
        self.status = Str::RecalcRequested.t().into();
        Ok(())
    }

    // ---- F-103-N4: interactive mask tools (Brush / Linear / Radial) ----

    /// Arm or disarm an interactive masking tool. Disarming returns the preview
    /// to its ordinary click/eyedropper behaviour and cancels any in-progress
    /// drag.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_tool(&mut self, tool: MaskTool) {
        self.mask_tool = tool;
        self.pending_brush_marks.clear();
        self.drag_start = None;
        self.drag_current = None;
        self.drawing = false;
    }

    /// Set the normalized brush radius. Rejected (no state change) if not finite
    /// or outside the open-closed `(0, 1]` range.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_brush_radius(&mut self, radius: f32) -> Result<(), GuiError> {
        if !radius.is_finite() || !(0.0..=1.0).contains(&radius) || radius <= 0.0 {
            return Err(GuiError::Io(
                "Brush radius must be finite and within (0, 1]".into(),
            ));
        }
        self.brush_radius = radius;
        Ok(())
    }

    /// Toggle the brush eraser (negative) sign.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_brush_eraser(&mut self, eraser: bool) {
        self.brush_eraser = eraser;
    }

    /// Set the blur of the selected mask layer (0..=1).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_blur(&mut self, blur: f32) -> Result<(), GuiError> {
        if !blur.is_finite() || !(0.0..=1.0).contains(&blur) {
            return Err(GuiError::Io("Blur must be between 0 and 1".into()));
        }
        self.active_layer_mut()?.blur = blur;
        self.render_key = None;
        Ok(())
    }

    /// Set the density of the selected mask layer (0..=1).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_density(&mut self, density: f32) -> Result<(), GuiError> {
        if !density.is_finite() || !(0.0..=1.0).contains(&density) {
            return Err(GuiError::Io("Density must be between 0 and 1".into()));
        }
        self.active_layer_mut()?.density = density;
        self.render_key = None;
        Ok(())
    }

    /// Returns the active virtual copy's source dimensions, used as the brush
    /// prompt resolution and overlay rasterization size.
    #[cfg(not(target_arch = "wasm32"))]
    fn image_dims(&self) -> Result<(u32, u32), GuiError> {
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
        Ok((frame.width, frame.height))
    }

    /// Ensure a mask is selected; create a default one if the active copy has
    /// none yet so a drawn prompt always has a home.
    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_selected_mask(&mut self) -> Result<String, GuiError> {
        if let Some(id) = self.selected_mask_id.clone() {
            return Ok(id);
        }
        let count = self
            .document
            .as_ref()
            .and_then(|d| {
                d.virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
            })
            .map_or(0, |c| c.mask_library.len());
        self.create_mask(format!("Mask {}", count + 1))
    }

    /// Persist a finished [`MaskPrompt`] onto the selected mask and write the
    /// sidecar. A hand-drawn prompt mask is complete without a model — the
    /// geometric rasterizer (F-079) supplies the matte — so it is marked
    /// `Valid` (the file browser would otherwise report a phantom "missing
    /// model"). No silent fallback: a missing sidecar/document is a hard error.
    #[cfg(not(target_arch = "wasm32"))]
    fn apply_mask_prompt(&mut self, prompt: MaskPrompt) -> Result<(), GuiError> {
        let mask_id = self.ensure_selected_mask()?;
        let document = self
            .document
            .as_mut()
            .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        let mask = copy
            .mask_library
            .iter_mut()
            .find(|mask| mask.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        mask.prompt = Some(prompt);
        mask.status = MaskStatus::Valid;
        mask.error_text = None;
        self.render_key = None;
        self.save_sidecar();
        self.status = Str::MaskPromptSaved.format_arg(&mask_id);
        Ok(())
    }

    /// Finalize a brush stroke. An empty stroke is a hard error and writes
    /// nothing (no silent fallback). Every mark is validated against the F-079
    /// prompt rules before persistence.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn commit_brush_stroke(&mut self, marks: Vec<BrushMark>) -> Result<(), GuiError> {
        if marks.is_empty() {
            return Err(GuiError::Io("A brush mask needs at least one mark".into()));
        }
        for mark in &marks {
            if !mark.x.is_finite()
                || !mark.y.is_finite()
                || !mark.radius.is_finite()
                || !(0.0..=1.0).contains(&mark.x)
                || !(0.0..=1.0).contains(&mark.y)
                || !(0.0..=1.0).contains(&mark.radius)
                || mark.radius <= 0.0
            {
                return Err(GuiError::Io(
                    "Brush marks must have finite normalized coordinates within 0..=1 and a positive radius".into(),
                ));
            }
        }
        let (w, h) = self.image_dims()?;
        let prompt = MaskPrompt::Brush {
            marks,
            resolution: (w, h),
            transformation: PromptTransform::default(),
        };
        self.apply_mask_prompt(prompt)
    }

    /// Build a linear-gradient prompt from a drag (start→end, normalized 0..=1).
    ///
    /// Behaviour (documented): both endpoints are clamped to `0..=1` before use,
    /// so a drag that leaves the image still yields a well-defined angle from
    /// the clamped segment. The drag *direction* sets `angle_deg`
    /// (`atan2(dy, dx)`, normalized to `[0, 360)`); `start`/`end` are the matte
    /// values (1.0 → 0.0) along that axis, matching the F-079 geometric
    /// rasterizer (`start` + t·(end−start) across the normalized projection).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn gradient_prompt_from_drag(a: Point2, b: Point2) -> MaskPrompt {
        let a = Point2 {
            x: a.x.clamp(0.0, 1.0),
            y: a.y.clamp(0.0, 1.0),
        };
        let b = Point2 {
            x: b.x.clamp(0.0, 1.0),
            y: b.y.clamp(0.0, 1.0),
        };
        let dx = (b.x - a.x) as f64;
        let dy = (b.y - a.y) as f64;
        let mut angle = dy.atan2(dx).to_degrees();
        if angle < 0.0 {
            angle += 360.0;
        }
        MaskPrompt::Gradient {
            angle_deg: angle as f32,
            start: 1.0,
            end: 0.0,
            transformation: PromptTransform::default(),
        }
    }

    /// Finalize a linear-gradient drag. A zero-length drag (the two endpoints
    /// coincide within tolerance) is rejected.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn commit_gradient(&mut self, a: Point2, b: Point2) -> Result<(), GuiError> {
        let a = Point2 {
            x: a.x.clamp(0.0, 1.0),
            y: a.y.clamp(0.0, 1.0),
        };
        let b = Point2 {
            x: b.x.clamp(0.0, 1.0),
            y: b.y.clamp(0.0, 1.0),
        };
        if (b.x - a.x).abs() < 1e-4 && (b.y - a.y).abs() < 1e-4 {
            return Err(GuiError::Io("Drag a gradient across the image".into()));
        }
        self.apply_mask_prompt(Self::gradient_prompt_from_drag(a, b))
    }

    /// Build a radial-gradient (ellipse) prompt from a drag. The drag defines
    /// the ellipse bounding box: `center` is the segment midpoint and `radii`
    /// are half the absolute (clamped) deltas, clamped to `(0, 1]`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn ellipse_prompt_from_drag(a: Point2, b: Point2) -> MaskPrompt {
        let a = Point2 {
            x: a.x.clamp(0.0, 1.0),
            y: a.y.clamp(0.0, 1.0),
        };
        let b = Point2 {
            x: b.x.clamp(0.0, 1.0),
            y: b.y.clamp(0.0, 1.0),
        };
        let center = Point2 {
            x: (a.x + b.x) / 2.0,
            y: (a.y + b.y) / 2.0,
        };
        let rx = ((b.x - a.x).abs() / 2.0).clamp(1e-3, 1.0);
        let ry = ((b.y - a.y).abs() / 2.0).clamp(1e-3, 1.0);
        MaskPrompt::Ellipse {
            center,
            radii: Point2 { x: rx, y: ry },
            transformation: PromptTransform::default(),
        }
    }

    /// Finalize a radial-gradient drag. A zero-size drag (both radii below
    /// tolerance) is rejected.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn commit_radial(&mut self, a: Point2, b: Point2) -> Result<(), GuiError> {
        let rx = ((b.x - a.x).abs() / 2.0).clamp(1e-3, 1.0);
        let ry = ((b.y - a.y).abs() / 2.0).clamp(1e-3, 1.0);
        if rx < 1e-4 && ry < 1e-4 {
            return Err(GuiError::Io("Drag a radial mask across the image".into()));
        }
        self.apply_mask_prompt(Self::ellipse_prompt_from_drag(a, b))
    }

    /// Finish the in-progress mask-tool drag, dispatching to the right commit
    /// based on the active tool. Errors are surfaced as visible [`GuiError`]s.
    #[cfg(not(target_arch = "wasm32"))]
    fn finish_drawing(&mut self) {
        let tool = self.mask_tool;
        let start = self.drag_start;
        let end = self.drag_current;
        let marks = std::mem::take(&mut self.pending_brush_marks);
        self.drawing = false;
        self.drag_start = None;
        self.drag_current = None;
        let result = match tool {
            MaskTool::None => return,
            MaskTool::Brush => self.commit_brush_stroke(marks),
            MaskTool::LinearGradient => match (start, end) {
                (Some(a), Some(b)) => self.commit_gradient(a, b),
                _ => Ok(()),
            },
            MaskTool::Radial => match (start, end) {
                (Some(a), Some(b)) => self.commit_radial(a, b),
                _ => Ok(()),
            },
        };
        if let Err(error) = result {
            self.show_error(error);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_document_loaded(&mut self) -> Result<(), GuiError> {
        if self.document.is_none() {
            let frame = self
                .original
                .as_ref()
                .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
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
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn active_layer_mut(&mut self) -> Result<&mut MaskLayer, GuiError> {
        self.active_copy_mut()?
            .mask_layers
            .first_mut()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_adjustment_to_selection(
        paths: &[std::path::PathBuf],
        key: &str,
        value: f64,
    ) -> Result<usize, GuiError> {
        if !matches!(key, "exposure" | "contrast" | "highlights" | "shadows") {
            return Err(GuiError::Io(Str::UnknownAdjustment.format_arg(key)));
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
        let (frame, orientation, camera_white_balance) = if source_is_raw {
            let image = lumina_raw::decode_bytes(&bytes, &name)?;
            (
                image.frame,
                image.metadata.orientation,
                Some(image.metadata.camera_white_balance),
            )
        } else {
            (ImageFrame::decode(&bytes)?, 1, None)
        };
        self.source_name = name;
        self.source_bytes = Some(bytes);
        self.source_is_raw = source_is_raw;
        self.raw_orientation = orientation;
        self.camera_white_balance = camera_white_balance;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.document = None;
            self.virtual_copy_id = "vc-original".into();
            self.selected_mask_id = None;
        }
        self.original = Some(frame);
        self.recipe = EditRecipe::default();
        self.error = None;
        self.status = Str::Loaded.format_arg(&self.source_name);
        self.render()
    }

    pub fn set_adjustment(&mut self, name: &str, value: f64) {
        self.recipe.adjustments.insert(name.into(), value);
        self.render_key = None;
        self.tone_analysis = None;
        self.status = Str::ChangePending.t().into();
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
        // F-041: measure the final visible domain — the rendered preview
        // (post crop/geometry, same frame that is displayed) weighted by the
        // effective mask planes of the last render. wasm32 renders without
        // mask layers, so the empty slice keeps the raster measurement.
        let mask_planes: Vec<MaskPlane> = self
            .render_mask_layers
            .iter()
            .map(|layer| layer.plane.clone())
            .collect();
        let value = match_total_exposure_masked(frame, target, &mask_planes)?;
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
            self.status = Str::NoImageLoaded.t().into();
            return Ok(());
        };
        let render_key_source = original.clone();
        // Mask artifact planes loaded from the optional `.lumina.zdata` sidecar
        // (native only).  Missing or unreadable zdata is not a hard error:
        // affected layers are reported through the `MaskPolicy::Warn` path.
        #[cfg(not(target_arch = "wasm32"))]
        let masks_context = {
            let planes = self.load_mask_planes();
            match &self.document {
                Some(document) => document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .map(|_| MaskContext {
                        copies: &document.virtual_copies,
                        active_copy_id: &self.virtual_copy_id,
                        planes,
                        policy: MaskPolicy::Warn,
                    }),
                None => None,
            }
        };
        #[cfg(target_arch = "wasm32")]
        let masks_context: Option<MaskContext<'_>> = None;
        let output = render_frame(
            &render_key_source,
            &RenderContext {
                recipe: &self.recipe,
                camera_white_balance: self.camera_white_balance,
                source_actions: &[],
                masks: masks_context,
                #[cfg(feature = "lensfun")]
                lensfun: None,
            },
        )?;
        let preview = output.frame;
        let mask_warnings = output.mask_warnings;
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
            if self.source_is_raw {
                lumina_raw::libraw_decode_version()
            } else {
                env!("CARGO_PKG_VERSION").into()
            },
            "raster-mvp-1",
            copy_id,
            &self.recipe,
            mask_hashes,
            OutputSpec {
                profile: "sRGB".into(),
                width: preview.width,
                height: preview.height,
                format: "rgba8".into(),
            },
        ));
        self.tone_analysis = Some(analyze_tone(&preview));
        self.preview = Some(preview);
        self.render_mask_layers = output.mask_layers;
        self.error = None;
        self.status = if !mask_warnings.is_empty() {
            let layers: Vec<&str> = mask_warnings
                .iter()
                .filter_map(|w| w.split('`').nth(1))
                .collect();
            if layers.is_empty() {
                Str::MaskUnavailable.t().to_string()
            } else {
                Str::MaskUnavailableLayer.format_arg(&layers.join(", "))
            }
        } else {
            Str::PreviewCurrent.t().to_string()
        };
        Ok(())
    }

    /// Loads mask artifact planes from the optional `.lumina.zdata` sidecar for
    /// the active virtual copy (native only).  Missing/unreadable zdata yields
    /// an empty map; affected layers are handled by the `MaskPolicy::Warn`
    /// path in [`render_frame`].
    #[cfg(not(target_arch = "wasm32"))]
    fn load_mask_planes(&self) -> BTreeMap<(String, String), MaskPlane> {
        let mut planes = BTreeMap::new();
        let Some(document) = &self.document else {
            return planes;
        };
        let Some(copy) = document
            .virtual_copies
            .iter()
            .find(|c| c.id == self.virtual_copy_id)
        else {
            return planes;
        };
        let zdata_path = zdata_path_for(Path::new(&self.path));
        if !zdata_path.exists() {
            return planes;
        }
        let Ok(container) = load_zdata(&zdata_path) else {
            return planes;
        };
        for mask in copy
            .mask_library
            .iter()
            .filter(|m| matches!(m.status, MaskStatus::Valid))
        {
            if let Ok(tile) = container.tile(&mask.id, 0, 0) {
                if let Ok(plane) = MaskPlane::new(tile.width, tile.height, tile.values) {
                    planes.insert((copy.id.clone(), mask.id.clone()), plane);
                }
            }
        }
        planes
    }

    #[cfg(target_arch = "wasm32")]
    fn load_mask_planes(&self) -> BTreeMap<(String, String), MaskPlane> {
        BTreeMap::new()
    }

    fn show_error(&mut self, error: impl ToString) {
        let message = error.to_string();
        self.status = Str::Error.t().into();
        self.error = Some(message);
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        // Before/After shows the original (never the recipe) so the toggle can
        // never mutate the recipe — it only swaps which frame is displayed.
        let frame = if self.before_after {
            self.original.as_ref()
        } else {
            self.preview.as_ref()
        };
        if let Some(frame) = frame {
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
                version: if self.source_is_raw {
                    lumina_raw::libraw_decode_version()
                } else {
                    env!("CARGO_PKG_VERSION").into()
                },
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
                        self.status = Str::AutoToneStale.t().into();
                    }
                    if let Err(error) = self.render() {
                        self.show_error(error);
                    } else if stale_auto_tone {
                        self.status = Str::AutoToneStale.t().into();
                    }
                }
            }
            Err(error) => self.show_error(GuiError::Io(format!("{}: {}", path.display(), error))),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_sidecar(&mut self) {
        if self.path.trim().is_empty() {
            self.show_error(Str::SaveNeedsLocalPath.t());
            return;
        }
        let path = std::path::PathBuf::from(self.path.trim());
        let Some(frame) = &self.original else {
            self.show_error(Str::NoImageLoaded.t());
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
            self.show_error(Str::VirtualCopyNotFound.t());
            self.document = Some(document);
            return;
        };
        copy.recipe = self.recipe.clone();
        if let Err(error) =
            lumina_sidecar::save_sidecar(&lumina_sidecar::sidecar_path_for(&path), &document)
        {
            self.show_error(error);
        } else {
            self.status = Str::SidecarSaved.t().into();
        }
        self.document = Some(document);
        self.list_directory();
        self.status = Str::SidecarSaved.t().into();
    }

    // ---- F-103-N5: Export module -------------------------------------------
    //
    // The export path is byte-identical to the CLI: it renders the current
    // recipe through the *same* `lumina_core::export_image` function (render +
    // encode) and writes the artifact through the *same* `lumina_sidecar::
    // write_atomically` helper. No encode logic is duplicated in the GUI.

    /// Export the currently loaded image to `output` using the shared render +
    /// encode chain. The output file extension is forced to the format's
    /// canonical extension so the chosen format (not a typed extension) is
    /// authoritative — mirroring the CLI's `output.with_extension(...)`.
    ///
    /// The original source is never overwritten: if `self.path` is set and
    /// resolves equal to `output`, the export is rejected as a [`GuiError`]
    /// (no silent fallback). The artifact is written atomically; the declarative
    /// recipe/sidecar is left untouched by the export itself (the user saves
    /// the recipe explicitly via "Save Recipe / Sidecar").
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_to(&mut self, output: PathBuf) -> Result<(), GuiError> {
        let Some(original) = self.original.as_ref() else {
            return Err(GuiError::Io(
                "No image loaded; open or drop an image first".into(),
            ));
        };
        // Never overwrite the original source.
        if !self.path.trim().is_empty() {
            let source = Path::new(&self.path);
            if lumina_sidecar::paths_resolve_equal(source, &output)
                .map_err(|error| GuiError::Io(error.to_string()))?
            {
                return Err(GuiError::Io(
                    "input and output resolve to the same path; refusing to overwrite the original"
                        .into(),
                ));
            }
        }
        let format = self.export_format;
        let quality = self.export_quality;
        let options = ExportOptions {
            format,
            quality,
            dither: false,
            ..Default::default()
        };
        options.validate().map_err(GuiError::Core)?;
        let output = output.with_extension(format.default_extension());
        // Build the identical render context used by `render()` (what the user
        // currently sees) so the export matches the preview.
        let masks_context = {
            let planes = self.load_mask_planes();
            self.document.as_ref().and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
                    .map(|_| MaskContext {
                        copies: &document.virtual_copies,
                        active_copy_id: &self.virtual_copy_id,
                        planes,
                        policy: MaskPolicy::Warn,
                    })
            })
        };
        let context = RenderContext {
            recipe: &self.recipe,
            camera_white_balance: self.camera_white_balance,
            source_actions: &[],
            masks: masks_context,
            #[cfg(feature = "lensfun")]
            lensfun: None,
        };
        let encoded = export_image(original, &context, options).map_err(GuiError::Core)?;
        lumina_sidecar::write_atomically(&output, &encoded).map_err(GuiError::Sidecar)?;
        self.error = None;
        self.status = format!(
            "Exported {} ({:?} @ q{})",
            output.display(),
            format,
            quality
        );
        Ok(())
    }

    /// Suggested export file name derived from the source name and the selected
    /// format (e.g. `photo.jpg` from `photo.png`). Used to prefill the save
    /// dialog and the path field.
    #[cfg(not(target_arch = "wasm32"))]
    fn suggested_export_name(&self) -> String {
        let base = if self.source_name.is_empty() {
            "export".to_string()
        } else {
            self.source_name.clone()
        };
        let stem = Path::new(&base)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "export".to_string());
        format!("{}.{}", stem, self.export_format.default_extension())
    }

    /// Draw the Export module controls (native only). Under wasm32 the module is
    /// shown as a clear capability hint instead (no file-system export).
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_export_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(Str::Export.t());
        ui.label(Str::ExportTarget.t());
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.export_path);
            if ui.button(Str::ExportChoose.t()).clicked() {
                let suggested = self.suggested_export_name();
                if let Some(path) = rfd::FileDialog::new().set_file_name(&suggested).save_file() {
                    self.export_path = path.display().to_string();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(Str::ExportFormatLabel.t());
            egui::ComboBox::from_label("")
                .selected_text(format_label(self.export_format))
                .show_ui(ui, |ui| {
                    for (candidate, label) in [
                        (ImageFileFormat::Png, "PNG"),
                        (ImageFileFormat::Jpeg, "JPEG"),
                        (ImageFileFormat::WebP, "WebP"),
                    ] {
                        if ui
                            .selectable_label(self.export_format == candidate, label)
                            .clicked()
                        {
                            self.export_format = candidate;
                        }
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(Str::ExportQualityLabel.t());
            let mut quality = self.export_quality as f64;
            if ui
                .add(egui::Slider::new(&mut quality, 1.0..=100.0).show_value(true))
                .changed()
            {
                self.export_quality = quality as u8;
            }
        });
        if self.export_format == ImageFileFormat::Png {
            ui.label(Str::ExportQualityUnused.t());
        }
        ui.horizontal(|ui| {
            if ui.button(Str::ExportUseSuggested.t()).clicked() {
                self.export_path = self.suggested_export_name();
            }
            if ui.button(Str::ExportRun.t()).clicked() {
                let path = PathBuf::from(self.export_path.trim());
                if path.as_os_str().is_empty() {
                    self.show_error(GuiError::Io("Choose an export target first".into()));
                } else if let Err(error) = self.export_to(path) {
                    self.show_error(error);
                }
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        ui.label(&self.status);
    }

    fn draw_preview(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        if let Some(texture) = &self.texture {
            let size = texture.size();
            let (tw, th) = (size[0] as f32, size[1] as f32);
            let scale = (available.x / tw).min(available.y / th).min(1.0);
            let draw = egui::vec2(tw * scale, th * scale);
            let rect = egui::Rect::from_min_max(
                egui::pos2(
                    ((available.x - draw.x) / 2.0).max(0.0),
                    ((available.y - draw.y) / 2.0).max(0.0),
                ),
                egui::pos2(
                    ((available.x + draw.x) / 2.0).min(available.x),
                    ((available.y + draw.y) / 2.0).min(available.y),
                ),
            );
            // A mask tool arms the preview for a drag gesture; otherwise it stays
            // a plain click target (so the WB eyedropper still works).
            #[cfg(not(target_arch = "wasm32"))]
            let sense = if self.mask_tool != MaskTool::None {
                egui::Sense::drag()
            } else {
                egui::Sense::click()
            };
            #[cfg(target_arch = "wasm32")]
            let sense = egui::Sense::click();
            let response = ui.allocate_rect(rect, sense);
            ui.put(rect, egui::Image::from_texture(texture));
            if self.wb_pick_mode && response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let nx = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                    let ny = ((pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
                    self.pick_white_balance_at(nx as f64, ny as f64);
                }
            }
            if self.wb_pick_mode {
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(2.0_f32, crate::theme::ACCENT),
                    egui::StrokeKind::Middle,
                );
            }
            #[cfg(not(target_arch = "wasm32"))]
            self.handle_mask_tool_drag(&response, rect);
            #[cfg(not(target_arch = "wasm32"))]
            self.draw_mask_overlay(ui, rect);
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(Str::NoImage.t());
            });
        }
    }

    /// Map a pointer position to normalized (0..=1) source coordinates using the
    /// same image-rect mapping as the WB eyedropper: the displayed preview rect
    /// maps 1:1 onto the source frame's normalized space (respecting the frame
    /// dimensions after orientation), clamped to the image bounds.
    #[cfg(not(target_arch = "wasm32"))]
    fn to_normalized(pos: egui::Pos2, rect: egui::Rect) -> (f32, f32) {
        let nx = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
        let ny = ((pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
        (nx, ny)
    }

    /// Drive an interactive mask-tool drag on the preview widget.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_mask_tool_drag(&mut self, response: &egui::Response, rect: egui::Rect) {
        if self.mask_tool == MaskTool::None || self.wb_pick_mode {
            return;
        }
        let Some(pos) = response.interact_pointer_pos() else {
            return;
        };
        let (nx, ny) = Self::to_normalized(pos, rect);
        if response.drag_started() {
            self.drawing = true;
            self.drag_start = Some(Point2 { x: nx, y: ny });
            self.drag_current = Some(Point2 { x: nx, y: ny });
            if self.mask_tool == MaskTool::Brush {
                self.pending_brush_marks.clear();
                self.pending_brush_marks.push(BrushMark {
                    x: nx,
                    y: ny,
                    radius: self.brush_radius,
                    sign: if self.brush_eraser {
                        BrushMarkSign::Negative
                    } else {
                        BrushMarkSign::Positive
                    },
                });
            }
        } else if response.dragged() {
            self.drag_current = Some(Point2 { x: nx, y: ny });
            if self.mask_tool == MaskTool::Brush {
                if let Some(last) = self.pending_brush_marks.last() {
                    let dist = ((last.x - nx).powi(2) + (last.y - ny).powi(2)).sqrt();
                    if dist > self.brush_radius * 0.5 {
                        self.pending_brush_marks.push(BrushMark {
                            x: nx,
                            y: ny,
                            radius: self.brush_radius,
                            sign: if self.brush_eraser {
                                BrushMarkSign::Negative
                            } else {
                                BrushMarkSign::Positive
                            },
                        });
                    }
                }
            }
        }
        if response.drag_stopped() {
            self.finish_drawing();
        }
    }

    /// Draw the currently relevant mask as a translucent overlay on the preview:
    /// the in-progress drag (live) or the selected mask's saved prompt. The
    /// F-079 geometric rasterizer produces the matte; it is painted as a
    /// translucent tint over the source rect so the user sees exactly what the
    /// pipeline will evaluate.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_mask_overlay(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(prompt) = self.current_overlay_prompt() else {
            return;
        };
        let (w, h) = self.image_dims().unwrap_or((1, 1));
        // Cap the rasterization so live drags stay smooth on large sources.
        let max_dim = 1024u32;
        let (rw, rh) = if w.max(h) > max_dim {
            let s = max_dim as f32 / w.max(h) as f32;
            (
                (w as f32 * s).round().max(1.0) as u32,
                (h as f32 * s).round().max(1.0) as u32,
            )
        } else {
            (w, h)
        };
        let Ok(plane) = rasterize_prompt(&prompt, rw, rh) else {
            return;
        };
        let mut pixels = vec![0u8; plane.values.len() * 4];
        for (i, value) in plane.values.iter().enumerate() {
            let alpha = (*value as f32 / u16::MAX as f32 * 0.45 * 255.0) as u8;
            pixels[i * 4..i * 4 + 4].copy_from_slice(&[80, 160, 255, alpha]);
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [plane.width as usize, plane.height as usize],
            &pixels,
        );
        let texture =
            ui.ctx()
                .load_texture("lumina-mask-overlay", image, egui::TextureOptions::NEAREST);
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    /// The prompt to display in the overlay: the live in-progress gesture while
    /// drawing, otherwise the selected mask's saved prompt (if any).
    #[cfg(not(target_arch = "wasm32"))]
    fn current_overlay_prompt(&self) -> Option<MaskPrompt> {
        if self.drawing && self.mask_tool != MaskTool::None {
            let (start, end) = (self.drag_start?, self.drag_current?);
            return match self.mask_tool {
                MaskTool::Brush => {
                    if self.pending_brush_marks.is_empty() {
                        None
                    } else {
                        let (w, h) = self.image_dims().unwrap_or((1, 1));
                        Some(MaskPrompt::Brush {
                            marks: self.pending_brush_marks.clone(),
                            resolution: (w, h),
                            transformation: PromptTransform::default(),
                        })
                    }
                }
                MaskTool::LinearGradient => Some(Self::gradient_prompt_from_drag(start, end)),
                MaskTool::Radial => Some(Self::ellipse_prompt_from_drag(start, end)),
                MaskTool::None => None,
            };
        }
        let id = self.selected_mask_id.as_ref()?;
        let document = self.document.as_ref()?;
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)?;
        let mask = copy.mask_library.iter().find(|mask| mask.id == *id)?;
        mask.prompt.clone()
    }

    /// Histogram of the *currently displayed* render state (original while
    /// Before/After is held, otherwise the last preview).
    fn current_analysis(&self) -> Option<lumina_core::ToneAnalysis> {
        if self.before_after {
            self.original.as_ref().map(analyze_tone)
        } else {
            self.tone_analysis
        }
    }

    fn draw_histogram(&self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading(Str::Histogram.t());
        if let Some(analysis) = self.current_analysis() {
            ui.label(format!(
                "Mean {:.3}  Median {:.3}",
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
            ui.label(Str::NotCurrent.t());
        }
    }

    fn mark_dirty(&mut self) {
        self.render_key = None;
        self.tone_analysis = None;
        self.error = None;
    }

    /// Documented default for a flat adjustment key (identity is 0; WB Kelvin 6500).
    pub fn default_for_adjustment(key: &str) -> f64 {
        match key {
            "wb_temperature" => 6500.0,
            _ => 0.0,
        }
    }

    /// Reset exactly one flat adjustment to its documented default. Never resets
    /// the whole recipe (that is [`Self::reset`]).
    pub fn reset_single_adjustment(&mut self, key: &str) {
        let default = Self::default_for_adjustment(key);
        self.recipe.adjustments.insert(key.to_owned(), default);
        self.mark_dirty();
        self.status = format!("Reset {key}");
    }

    /// Toggle Before/After. Deliberately does not touch the recipe.
    pub fn toggle_before_after(&mut self) {
        self.before_after = !self.before_after;
    }

    /// Derive a deterministic WB (temperature, tint) from a picked sRGB point so
    /// the channel means become neutral. `None` for non-positive channels.
    pub fn white_balance_from_point(r: f64, g: f64, b: f64) -> Option<(f64, f64)> {
        if r <= 0.0 || g <= 0.0 || b <= 0.0 {
            return None;
        }
        let l = (r + g + b) / 3.0;
        let gr = l / r;
        let gg = l / g;
        let gb = l / b;
        // Pipeline gains: R = 1 - warmth*0.35, G = 1 - tint*0.20, B = 1 + warmth*0.35.
        let warmth = (((1.0 - gr) + (gb - 1.0)) / 2.0) / 0.35;
        let tint = (1.0 - gg) / 0.20;
        let temperature = (6500.0 + warmth * 5500.0).clamp(1500.0, 12000.0);
        let tint = tint.clamp(-1.0, 1.0);
        Some((temperature, tint))
    }

    /// Set the `wb_temperature`/`wb_tint` recipe fields from a picked point
    /// (Core F-036-N1 path: `render_frame` applies them via the sRGB model).
    pub fn set_white_balance_from_point(&mut self, r: f64, g: f64, b: f64) -> Result<(), GuiError> {
        let Some((temp, tint)) = Self::white_balance_from_point(r, g, b) else {
            return Err(GuiError::Io(
                "Cannot derive white balance from this point".into(),
            ));
        };
        self.recipe
            .adjustments
            .insert("wb_temperature".into(), temp);
        self.recipe.adjustments.insert("wb_tint".into(), tint);
        self.wb_pick_mode = false;
        self.mark_dirty();
        self.status = "White balance set from picked point".into();
        if self.original.is_some() {
            self.render()?;
        }
        Ok(())
    }

    fn pick_white_balance_at(&mut self, nx: f64, ny: f64) {
        let Some(frame) = &self.original else {
            return;
        };
        let x = ((nx * frame.width as f64) as u32).min(frame.width.saturating_sub(1));
        let y = ((ny * frame.height as f64) as u32).min(frame.height.saturating_sub(1));
        let idx = ((y * frame.width + x) * 4) as usize;
        let px = &frame.pixels[idx..idx + 4];
        let r = px[0] as f64 / 255.0;
        let g = px[1] as f64 / 255.0;
        let b = px[2] as f64 / 255.0;
        if let Err(e) = self.set_white_balance_from_point(r, g, b) {
            self.show_error(e);
        }
    }

    /// One horizontal Lightroom-style adjustment row bound to a flat recipe key.
    fn adjustment_slider(&mut self, ui: &mut egui::Ui, key: &str, label: &str, spec: SliderSpec) {
        let mut v = self
            .recipe
            .adjustments
            .get(key)
            .copied()
            .unwrap_or(Self::default_for_adjustment(key));
        match lr_slider(ui, label, &mut v, spec) {
            SliderAction::Changed => self.set_adjustment(key, v),
            SliderAction::ResetRequested => self.reset_single_adjustment(key),
            SliderAction::Nothing => {}
        }
    }

    // ---- Develop panel sections (fixed F-100 order) ----

    fn draw_basic(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Basic.t(), |ui| {
            ui.label(Str::WhiteBalance.t());
            self.adjustment_slider(
                ui,
                "wb_temperature",
                Str::Temperature.t(),
                identity_spec(1500.0..=12000.0, 6500.0, 50.0),
            );
            self.adjustment_slider(ui, "wb_tint", Str::Tint.t(), percent_spec(-1.0..=1.0, 0.0));
            if self.wb_pick_mode {
                ui.horizontal(|ui| {
                    if ui.button(Str::WbEyedropperActive.t()).clicked() {
                        self.wb_pick_mode = false;
                    }
                    if ui.button(Str::Cancel.t()).clicked() {
                        self.wb_pick_mode = false;
                    }
                });
                ui.label(Str::PickWhiteBalanceHint.t());
            } else if ui.button(Str::WbEyedropper.t()).clicked() {
                self.wb_pick_mode = true;
            }
            ui.separator();
            self.adjustment_slider(
                ui,
                "exposure",
                Str::Exposure.t(),
                identity_spec(-10.0..=10.0, 0.0, 0.1),
            );
            self.adjustment_slider(
                ui,
                "contrast",
                Str::Contrast.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(
                ui,
                "highlights",
                Str::Highlights.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(
                ui,
                "shadows",
                Str::Shadows.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(ui, "whites", Str::Whites.t(), percent_spec(-1.0..=1.0, 0.0));
            self.adjustment_slider(ui, "blacks", Str::Blacks.t(), percent_spec(-1.0..=1.0, 0.0));
            if ui.button(Str::Auto.t()).clicked() {
                if let Err(e) = self.auto_tone() {
                    self.show_error(e);
                }
            }
        });
    }

    fn draw_tone_curve(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::ToneCurve.t(), |ui| {
            ui.label(Str::CurveRegions.t());
            let (mut s, mut d, mut l, mut h) = tone_curve_regions(&self.recipe);
            let spec = percent_spec(-1.0..=1.0, 0.0);
            let mut changed = false;
            for (val, label) in [
                (&mut s, Str::ToneCurveShadows),
                (&mut d, Str::ToneCurveDarks),
                (&mut l, Str::ToneCurveLights),
                (&mut h, Str::ToneCurveHighlights),
            ] {
                match lr_slider(ui, label.t(), val, spec) {
                    SliderAction::Changed | SliderAction::ResetRequested => changed = true,
                    SliderAction::Nothing => {}
                }
            }
            if changed {
                self.recipe.curves = Some(build_tone_curve(s, d, l, h));
                self.mark_dirty();
            }
        });
    }

    fn draw_color(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Color.t(), |ui| {
            ui.label(Str::HslMixer.t());
            let mut hsl = self.recipe.hsl.clone().unwrap_or_default();
            let spec = percent_spec(-1.0..=1.0, 0.0);
            let channels = [
                (Str::HslRed, "red"),
                (Str::HslOrange, "orange"),
                (Str::HslYellow, "yellow"),
                (Str::HslGreen, "green"),
                (Str::HslCyan, "cyan"),
                (Str::HslBlue, "blue"),
                (Str::HslViolet, "violet"),
                (Str::HslMagenta, "magenta"),
            ];
            let mut changed = false;
            for (label, key) in channels {
                ui.label(label.t());
                let slot = hsl_channel_mut(&mut hsl, key);
                for (field, label) in [
                    (&mut slot.hue, Str::Hue),
                    (&mut slot.saturation, Str::Saturation),
                    (&mut slot.luminance, Str::Luminance),
                ] {
                    match lr_slider(ui, label.t(), field, spec) {
                        SliderAction::Changed | SliderAction::ResetRequested => changed = true,
                        SliderAction::Nothing => {}
                    }
                }
            }
            ui.separator();
            ui.label(Str::ColorGrading.t());
            let mut cg = self.recipe.color_grading.clone().unwrap_or(ColorGrading {
                version: 1,
                shadows: ColorGradingRange {
                    hue_degrees: 0.0,
                    saturation: 0.0,
                },
                midtones: ColorGradingRange {
                    hue_degrees: 0.0,
                    saturation: 0.0,
                },
                highlights: ColorGradingRange {
                    hue_degrees: 0.0,
                    saturation: 0.0,
                },
                balance: 0.0,
            });
            let mut changed_cg = false;
            for (range, label) in [
                (&mut cg.shadows, Str::GradingShadows),
                (&mut cg.midtones, Str::GradingMidtones),
                (&mut cg.highlights, Str::GradingHighlights),
            ] {
                self.color_grading_range_slider(ui, range, label, &mut changed_cg);
            }
            let mut balance = cg.balance;
            match lr_slider(
                ui,
                Str::GradingBalance.t(),
                &mut balance,
                percent_spec(-1.0..=1.0, 0.0),
            ) {
                SliderAction::Changed | SliderAction::ResetRequested => {
                    cg.balance = balance;
                    changed_cg = true;
                }
                SliderAction::Nothing => {}
            }
            if changed {
                self.recipe.hsl = Some(hsl);
                self.mark_dirty();
            }
            if changed_cg {
                self.recipe.color_grading = Some(cg);
                self.mark_dirty();
            }
        });
    }

    fn color_grading_range_slider(
        &mut self,
        ui: &mut egui::Ui,
        range: &mut ColorGradingRange,
        label: Str,
        changed: &mut bool,
    ) {
        let mut hue = range.hue_degrees;
        if matches!(
            lr_slider(
                ui,
                &Str::HuePattern.format_arg(label.t()),
                &mut hue,
                identity_spec(0.0..=360.0, 0.0, 1.0)
            ),
            SliderAction::Changed | SliderAction::ResetRequested
        ) {
            range.hue_degrees = hue;
            *changed = true;
        }
        let mut sat = range.saturation;
        if matches!(
            lr_slider(
                ui,
                &Str::SatPattern.format_arg(label.t()),
                &mut sat,
                identity_spec(0.0..=1.0, 0.0, 0.01)
            ),
            SliderAction::Changed | SliderAction::ResetRequested
        ) {
            range.saturation = sat;
            *changed = true;
        }
    }

    fn draw_effects(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Effects.t(), |ui| {
            ui.label(Str::Vignette.t());
            let mut effects = self.recipe.effects.clone().unwrap_or(Effects {
                vignette: Some(Vignette {
                    version: 1,
                    amount: 0.0,
                    midpoint: 0.5,
                    roundness: 0.0,
                    feather: 0.0,
                }),
                grain: Some(Grain {
                    version: 1,
                    amount: 0.0,
                    size: 0.0,
                    roughness: 0.0,
                    seed: 0,
                }),
            });
            let mut changed = false;
            if let Some(v) = &mut effects.vignette {
                let mut amount = v.amount;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Amount.t(),
                        &mut amount,
                        percent_spec(-1.0..=1.0, 0.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    v.amount = amount;
                    changed = true;
                }
                let mut midpoint = v.midpoint;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Midpoint.t(),
                        &mut midpoint,
                        identity_spec(0.0..=1.0, 0.5, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    v.midpoint = midpoint;
                    changed = true;
                }
                let mut roundness = v.roundness;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Roundness.t(),
                        &mut roundness,
                        percent_spec(-1.0..=1.0, 0.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    v.roundness = roundness;
                    changed = true;
                }
                let mut feather = v.feather;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Feather.t(),
                        &mut feather,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    v.feather = feather;
                    changed = true;
                }
            }
            ui.label(Str::Grain.t());
            if let Some(g) = &mut effects.grain {
                let mut amount = g.amount;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Amount.t(),
                        &mut amount,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    g.amount = amount;
                    changed = true;
                }
                let mut size = g.size;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Size.t(),
                        &mut size,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    g.size = size;
                    changed = true;
                }
                let mut roughness = g.roughness;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Roughness.t(),
                        &mut roughness,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    g.roughness = roughness;
                    changed = true;
                }
                let mut seed = g.seed as f64;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Seed.t(),
                        &mut seed,
                        identity_spec(0.0..=1_000_000.0, 0.0, 1.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    g.seed = seed as u64;
                    changed = true;
                }
            }
            if changed {
                self.recipe.effects = Some(effects);
                self.mark_dirty();
            }
        });
    }

    fn draw_detail(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Detail.t(), |ui| {
            ui.label(Str::Sharpening.t());
            let mut sh = self.recipe.sharpening.unwrap_or(Sharpening {
                version: 1,
                amount: 0.0,
                radius: 0.5,
                detail: 0.0,
                masking: 0.0,
            });
            let mut changed = false;
            let mut amount = sh.amount;
            if matches!(
                lr_slider(
                    ui,
                    Str::Amount.t(),
                    &mut amount,
                    identity_spec(0.0..=3.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                sh.amount = amount;
                changed = true;
            }
            let mut radius = sh.radius;
            if matches!(
                lr_slider(
                    ui,
                    Str::Radius.t(),
                    &mut radius,
                    identity_spec(0.1..=10.0, 0.5, 0.1)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                sh.radius = radius;
                changed = true;
            }
            let mut detail = sh.detail;
            if matches!(
                lr_slider(
                    ui,
                    Str::Detail.t(),
                    &mut detail,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                sh.detail = detail;
                changed = true;
            }
            let mut masking = sh.masking;
            if matches!(
                lr_slider(
                    ui,
                    Str::Masking.t(),
                    &mut masking,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                sh.masking = masking;
                changed = true;
            }
            if changed {
                self.recipe.sharpening = Some(sh);
                self.mark_dirty();
            }
            ui.label(Str::NoiseReduction.t());
            let mut nr = self.recipe.noise_reduction.unwrap_or(NoiseReduction {
                version: 1,
                luminance: 0.0,
                color: 0.0,
            });
            let mut changed_nr = false;
            let mut lum = nr.luminance;
            if matches!(
                lr_slider(
                    ui,
                    Str::Luminance.t(),
                    &mut lum,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                nr.luminance = lum;
                changed_nr = true;
            }
            let mut col = nr.color;
            if matches!(
                lr_slider(
                    ui,
                    Str::Color.t(),
                    &mut col,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                nr.color = col;
                changed_nr = true;
            }
            if changed_nr {
                self.recipe.noise_reduction = Some(nr);
                self.mark_dirty();
            }
        });
    }

    fn draw_optics(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Optics.t(), |ui| {
            if cfg!(feature = "lensfun") {
                ui.label(Str::LensCorrection.t());
                let mut lc = self
                    .recipe
                    .lens_correction
                    .clone()
                    .unwrap_or(LensCorrection {
                        version: 1,
                        profile: None,
                        distortion_k1: None,
                        distortion_k2: None,
                        distortion_k3: None,
                        vignette_c0: None,
                        vignette_c1: None,
                        vignette_c2: None,
                        ca_red: None,
                        ca_blue: None,
                    });
                let mut changed = false;
                for (field, label, spec) in [
                    (
                        &mut lc.distortion_k1,
                        Str::DistortionK1,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.distortion_k2,
                        Str::DistortionK2,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.distortion_k3,
                        Str::DistortionK3,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.vignette_c0,
                        Str::VignetteC0,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.vignette_c1,
                        Str::VignetteC1,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.vignette_c2,
                        Str::VignetteC2,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.ca_red,
                        Str::ChromaticRed,
                        identity_spec(-0.05..=0.05, 0.0, 0.001),
                    ),
                    (
                        &mut lc.ca_blue,
                        Str::ChromaticBlue,
                        identity_spec(-0.05..=0.05, 0.0, 0.001),
                    ),
                ] {
                    let current = field.as_ref().copied();
                    if let Some(c) = current {
                        let mut v = c;
                        match lr_slider(ui, label.t(), &mut v, spec) {
                            SliderAction::Changed | SliderAction::ResetRequested => {
                                *field = Some(v);
                                changed = true;
                            }
                            SliderAction::Nothing => {}
                        }
                    } else {
                        ui.label(Str::UnsetPattern.format_arg(label.t()));
                    }
                }
                if changed {
                    self.recipe.lens_correction = Some(lc);
                    self.mark_dirty();
                }
            } else {
                ui.label(Str::OpticsRequiresLensfun.t());
                ui.label(Str::NotAvailable.t());
            }
        });
    }

    fn draw_geometry(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Geometry.t(), |ui| {
            if cfg!(feature = "lensfun") {
                ui.label(Str::Crop.t());
                let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
                    version: 1,
                    crop: None,
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                });
                let mut changed = false;
                let mut rotation = geo.rotation_degrees;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Rotation.t(),
                        &mut rotation,
                        identity_spec(-180.0..=180.0, 0.0, 1.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    geo.rotation_degrees = rotation;
                    changed = true;
                }
                let mut mh = geo.mirror_horizontal;
                if ui.checkbox(&mut mh, Str::MirrorHorizontal.t()).changed() {
                    geo.mirror_horizontal = mh;
                    changed = true;
                }
                let mut mv = geo.mirror_vertical;
                if ui.checkbox(&mut mv, Str::MirrorVertical.t()).changed() {
                    geo.mirror_vertical = mv;
                    changed = true;
                }
                ui.label(Str::Perspective.t());
                let mut persp = self.recipe.perspective.unwrap_or(Perspective {
                    version: 1,
                    vertical: 0.0,
                    horizontal: 0.0,
                    rotation: 0.0,
                    scale: 1.0,
                    aspect_ratio: 1.0,
                    shift_x: 0.0,
                    shift_y: 0.0,
                });
                let mut changed_p = false;
                for (field, label, spec) in [
                    (
                        &mut persp.vertical,
                        Str::Vertical,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut persp.horizontal,
                        Str::Horizontal,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut persp.rotation,
                        Str::Rotation,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut persp.scale,
                        Str::Scale,
                        identity_spec(0.1..=10.0, 1.0, 0.01),
                    ),
                    (
                        &mut persp.aspect_ratio,
                        Str::AspectRatio,
                        identity_spec(0.1..=10.0, 1.0, 0.01),
                    ),
                    (
                        &mut persp.shift_x,
                        Str::ShiftX,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut persp.shift_y,
                        Str::ShiftY,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                ] {
                    let mut v = *field;
                    match lr_slider(ui, label.t(), &mut v, spec) {
                        SliderAction::Changed | SliderAction::ResetRequested => {
                            *field = v;
                            changed_p = true;
                        }
                        SliderAction::Nothing => {}
                    }
                }
                if changed {
                    self.recipe.geometry = Some(geo);
                    self.mark_dirty();
                }
                if changed_p {
                    self.recipe.perspective = Some(persp);
                    self.mark_dirty();
                }
            } else {
                ui.label(Str::GeometryRequiresLensfun.t());
                ui.label(Str::NotAvailable.t());
            }
        });
    }

    fn draw_masking(&mut self, ui: &mut egui::Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        ui.collapsing(Str::Masking.t(), |ui| {
            let Some(document) = self.document.clone() else {
                return;
            };
            let mask_options: Vec<(String, String)> = document
                .virtual_copies
                .iter()
                .find(|c| c.id == self.virtual_copy_id)
                .map(|c| {
                    c.mask_library
                        .iter()
                        .map(|m| (m.id.clone(), m.name.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let mut selected_mask = self.selected_mask_id.clone().unwrap_or_default();
            egui::ComboBox::from_label(Str::SelectMask.t())
                .selected_text(
                    mask_options
                        .iter()
                        .find(|(id, _)| id == &selected_mask)
                        .map(|(_, name)| name.as_str())
                        .unwrap_or("None"),
                )
                .show_ui(ui, |ui| {
                    for (id, name) in &mask_options {
                        ui.selectable_value(&mut selected_mask, id.clone(), name);
                    }
                });
            if selected_mask != self.selected_mask_id.clone().unwrap_or_default()
                && !selected_mask.is_empty()
            {
                if let Err(e) = self.select_mask(&selected_mask) {
                    self.show_error(e);
                }
            }
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.mask_name_input);
                if ui.button(Str::NewMask.t()).clicked() {
                    if let Err(e) = self.create_mask(self.mask_name_input.clone()) {
                        self.show_error(e);
                    } else {
                        self.mask_name_input.clear();
                    }
                }
            });
            // F-103-N4: interactive mask tools. The tool only picks how a drag on
            // the preview is interpreted; persistence goes through the sidecar.
            ui.separator();
            ui.label(Str::MaskTool.t());
            ui.horizontal_wrapped(|ui| {
                for (tool, label) in [
                    (MaskTool::Brush, Str::MaskToolBrush),
                    (MaskTool::LinearGradient, Str::MaskToolGradient),
                    (MaskTool::Radial, Str::MaskToolRadial),
                ] {
                    if ui
                        .selectable_label(self.mask_tool == tool, label.t())
                        .clicked()
                    {
                        self.set_mask_tool(tool);
                    }
                }
                if ui
                    .selectable_label(self.mask_tool == MaskTool::None, Str::MaskToolNone.t())
                    .clicked()
                {
                    self.set_mask_tool(MaskTool::None);
                }
            });
            if self.mask_tool == MaskTool::Brush {
                let mut radius = self.brush_radius;
                if ui
                    .add(egui::Slider::new(&mut radius, 0.005..=1.0).text(Str::BrushSize.t()))
                    .changed()
                {
                    if let Err(e) = self.set_brush_radius(radius) {
                        self.show_error(e);
                    }
                }
                ui.checkbox(&mut self.brush_eraser, Str::BrushEraser.t());
            }
            ui.label(Str::DrawMaskHint.t());
            if self.selected_mask_id.is_some() {
                let mut inverted = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .is_some_and(|layer| layer.inverted);
                if ui.checkbox(&mut inverted, Str::Invert.t()).changed() {
                    if let Err(e) = self.set_mask_inverted(inverted) {
                        self.show_error(e);
                    }
                }
                let mut feather = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .map_or(0.0, |layer| layer.feather);
                if ui
                    .add(egui::Slider::new(&mut feather, 0.0..=1.0).text(Str::Feather.t()))
                    .changed()
                {
                    if let Err(e) = self.set_mask_feather(feather) {
                        self.show_error(e);
                    }
                }
                let mut blur = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .map_or(0.0, |layer| layer.blur);
                if ui
                    .add(egui::Slider::new(&mut blur, 0.0..=1.0).text(Str::Blur.t()))
                    .changed()
                {
                    if let Err(e) = self.set_mask_blur(blur) {
                        self.show_error(e);
                    }
                }
                let mut density = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .map_or(1.0, |layer| layer.density);
                if ui
                    .add(egui::Slider::new(&mut density, 0.0..=1.0).text(Str::Density.t()))
                    .changed()
                {
                    if let Err(e) = self.set_mask_density(density) {
                        self.show_error(e);
                    }
                }
                if ui.button(Str::OfferRecalculation.t()).clicked() {
                    if let Err(e) = self.offer_mask_recalculation().and_then(|offered| {
                        if offered {
                            self.mark_mask_for_recalculation()
                        } else {
                            Ok(())
                        }
                    }) {
                        self.show_error(e);
                    }
                }
                ui.label(Str::LocalAdjustments.t());
                for (key, label) in [("exposure", Str::Exposure), ("contrast", Str::Contrast)] {
                    let stored = document
                        .virtual_copies
                        .iter()
                        .find(|c| c.id == self.virtual_copy_id)
                        .and_then(|c| c.mask_layers.first())
                        .and_then(|layer| layer.extras.get(&format!("adjustment_{key}")))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let range = if key == "exposure" {
                        -10.0..=10.0
                    } else {
                        -1.0..=1.0
                    };
                    let mut value = stored;
                    if ui
                        .add(egui::Slider::new(&mut value, range).text(label.t()))
                        .changed()
                    {
                        if let Err(e) = self.set_mask_local_adjustment(key, value) {
                            self.show_error(e);
                        }
                    }
                }
            }
        });
        #[cfg(target_arch = "wasm32")]
        ui.collapsing(Str::Masking.t(), |ui| {
            ui.label(Str::NotAvailable.t());
        });
    }

    fn draw_file_browser(&mut self, ui: &mut egui::Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.heading(Str::Library.t());
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.directory);
                if ui.button(Str::Open.t()).clicked() {
                    self.list_directory();
                }
            });
            if ui.button(Str::Refresh.t()).clicked() {
                self.list_directory();
            }
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut path_to_open = None;
                for entry in &self.entries {
                    let selected = self.path == entry.path.display().to_string();
                    let label = format!(
                        "{}  [{}] {}:{} {}:{}",
                        entry.name,
                        entry.status_label(),
                        Str::Copies.t(),
                        entry.virtual_copies,
                        Str::Masking.t(),
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
        }
        #[cfg(target_arch = "wasm32")]
        {
            ui.heading(Str::Library.t());
            ui.label(Str::NotAvailable.t());
        }
    }

    /// The full Develop control stack: the eight F-100 sections in fixed order,
    /// then the preset manager and the global render/save actions.  Every
    /// adjustment uses [`lr_slider`] so the F-100 reset/scroll/scale rules apply.
    fn draw_develop_panel(&mut self, ui: &mut egui::Ui) {
        self.draw_basic(ui);
        self.draw_tone_curve(ui);
        self.draw_color(ui);
        self.draw_effects(ui);
        self.draw_detail(ui);
        self.draw_optics(ui);
        self.draw_geometry(ui);
        self.draw_masking(ui);
        ui.separator();
        ui.collapsing(Str::Preset.t(), |ui| {
            ui.text_edit_singleline(&mut self.preset_name);
            for field in ["exposure", "contrast", "highlights", "shadows"] {
                let selected = self.preset_fields.entry(field.into()).or_insert(false);
                ui.checkbox(selected, field);
            }
            ui.checkbox(
                &mut self.preset_relative_exposure,
                Str::ExposureRelative.t(),
            );
            if ui.button(Str::ApplyPreset.t()).clicked() {
                match self
                    .create_preset(self.preset_name.clone())
                    .and_then(|preset| self.apply_preset(&preset))
                {
                    Ok(()) => self.status = "Preset applied, new history step".into(),
                    Err(error) => self.show_error(error),
                }
            }
        });
        ui.separator();
        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.path);
                if ui.button(Str::Load.t()).clicked() {
                    self.load_path();
                }
            });
            if ui.button(Str::ChooseFile.t()).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.path = path.display().to_string();
                    self.load_path();
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            ui.label(Str::NotAvailable.t());
        }
        if ui.button(Str::MatchExposure.t()).clicked() {
            if let Err(error) = self.match_total_exposure(0.5) {
                self.show_error(error);
            }
        }
        ui.horizontal(|ui| {
            if ui.button(Str::Reset.t()).clicked() {
                self.reset();
            }
            if ui.button(Str::RenderApply.t()).clicked() {
                if let Err(error) = self.render() {
                    self.show_error(error);
                }
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        if ui.button(Str::SaveRecipe.t()).clicked() {
            self.save_sidecar();
        }
    }

    /// Library-module sidecar / virtual-copy manager (native only).  Mask editing
    /// lives in the Develop panel's Masking section; here the user picks which
    /// source copy to work on and can duplicate it.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_library_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(Str::Source.t());
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
            let source_status =
                lumina_sidecar::source_status(std::path::Path::new(&self.path), &document.source)
                    .ok();
            ui.separator();
            ui.label(format!(
                "{}: {} {}",
                Str::Sidecar.t(),
                Str::Copies.t(),
                copy_count
            ));
            if let Some(source_status) = source_status {
                ui.label(format!("Source: {:?}", source_status));
            }
            let mut selected = self.virtual_copy_id.clone();
            egui::ComboBox::from_label(Str::Copies.t())
                .selected_text(selected.clone())
                .show_ui(ui, |ui| {
                    for (id, name) in &copy_options {
                        ui.selectable_value(&mut selected, id.clone(), name);
                    }
                });
            if selected != self.virtual_copy_id {
                let _ = self.select_virtual_copy(&selected);
            }
            if ui.button(Str::NewCopy.t()).clicked() {
                let id = format!("vc-{}", copy_count + 1);
                if let Err(error) = self.duplicate_virtual_copy(id, "New copy") {
                    self.show_error(error);
                }
            }
            ui.label(format!(
                "{}: {} {}",
                Str::Masking.t(),
                missing_masks,
                Str::NotAvailable.t()
            ));
        } else {
            ui.label(Str::NoImage.t());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn draw_filmstrip(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.heading(Str::Filmstrip.t());
        ui.label(Str::FilmstripHint.t());
        let entries: Vec<FileBrowserEntry> = self.entries.clone();
        let mut open_target: Option<String> = None;
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for entry in &entries {
                    let tex = self.thumbnails.get(&entry.name).cloned();
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(110.0, 84.0), egui::Sense::click());
                    if let Some(texture) = tex {
                        ui.put(
                            rect,
                            egui::Image::from_texture(&texture).max_size(rect.size()),
                        );
                    } else {
                        ui.painter()
                            .rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                        ui.put(rect, egui::Label::new(&entry.name));
                    }
                    if resp.clicked() {
                        open_target = Some(entry.path.display().to_string());
                    }
                }
            });
        });
        if let Some(target) = open_target {
            self.open_file(target);
        }
        for entry in &entries {
            self.ensure_thumbnail(ctx, entry);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_thumbnail(&mut self, ctx: &egui::Context, entry: &FileBrowserEntry) {
        if self.thumbnails.get(&entry.name).is_some() {
            return;
        }
        if self.thumbnails.probed(&entry.name) {
            return;
        }
        if let Ok(cache) = DiskFolderCache::for_image(entry.path.as_path()) {
            // Use the headless-testable cache probe; on a hit, load and display
            // the stored preview.  A miss enqueues a background thumbnail job
            // (no silent fallback to a wrong/sized-up image).
            if filmstrip::filmstrip_preview_cached(&cache, &entry.name, "vc-original") {
                if let Ok(Some(bytes)) =
                    cache.load_preview(&entry.name, "vc-original", PreviewKind::Standard)
                {
                    if let Ok(frame) = ImageFrame::decode(&bytes) {
                        let tex = self.make_thumbnail_texture(ctx, &frame, &entry.name);
                        self.thumbnails.insert(&entry.name, tex);
                        self.thumbnails.mark_probed(&entry.name);
                        return;
                    }
                }
            }
        }
        self.thumbnails.mark_probed(&entry.name);
        let _ = self.idle_queue.enqueue(
            IdleTask::Thumbnail {
                source: entry.path.clone(),
                name: entry.name.clone(),
            },
            50,
        );
    }

    fn make_thumbnail_texture(
        &self,
        ctx: &egui::Context,
        frame: &ImageFrame,
        key: &str,
    ) -> egui::TextureHandle {
        let size = [frame.width as usize, frame.height as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
        ctx.load_texture(format!("thumb-{key}"), image, egui::TextureOptions::LINEAR)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn generate_thumbnail(&mut self, ctx: &egui::Context, source: &Path, name: &str) {
        let bytes = match std::fs::read(source) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("Thumbnail failed: {e}");
                return;
            }
        };
        let is_raw = is_raw_name(name);
        let frame = if is_raw {
            match lumina_raw::decode_bytes(&bytes, name) {
                Ok(img) => img.frame,
                Err(e) => {
                    self.status = format!("Thumbnail decode failed: {e}");
                    return;
                }
            }
        } else {
            match ImageFrame::decode(&bytes) {
                Ok(f) => f,
                Err(e) => {
                    self.status = format!("Thumbnail decode failed: {e}");
                    return;
                }
            }
        };
        let (small, w, h) =
            downscale_rgba(&frame.pixels, frame.width, frame.height, THUMBNAIL_MAX_DIM);
        let small_frame = match ImageFrame::new(w, h, small) {
            Ok(f) => f,
            Err(_) => return,
        };
        let context = RenderContext {
            recipe: &EditRecipe::default(),
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            #[cfg(feature = "lensfun")]
            lensfun: None,
        };
        let preview = match render_frame(&small_frame, &context) {
            Ok(o) => o.frame,
            Err(_) => small_frame,
        };
        let png = match preview.encode(ImageFileFormat::Png) {
            Ok(p) => p,
            Err(_) => return,
        };
        if let Ok(cache) = DiskFolderCache::for_image(source) {
            let _ = cache.store_preview(name, "vc-original", PreviewKind::Standard, &png);
        }
        let tex = self.make_thumbnail_texture(ctx, &preview, name);
        self.thumbnails.insert(name, tex);
    }
}

/// The four Lightroom parametric tone-curve regions (Shadows, Darks, Lights,
/// Highlights) as the GUI's source of truth.  They are persisted as a master
/// [`Curves`] point list via [`build_tone_curve`]; the read-back keeps the
/// slider values stable for typical (unclamped) adjustments.
fn tone_curve_regions(recipe: &EditRecipe) -> (f64, f64, f64, f64) {
    let base: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let points = recipe
        .curves
        .as_ref()
        .map(|c| c.master.clone())
        .unwrap_or_default();
    let mut out = (0.0f64, 0.0, 0.0, 0.0);
    let vals: [f64; 4] = std::array::from_fn(|i| {
        let bx = base[i];
        let out_v = points.get(i).map(|p| p.output as f64).unwrap_or(bx);
        (out_v - bx).clamp(-1.0, 1.0)
    });
    out.0 = vals[0];
    out.1 = vals[1];
    out.2 = vals[2];
    out.3 = vals[3];
    out
}

/// Persist the four region values as a master [`Curves`] point list.  Outputs
/// stay in `[0,1]` so the render pipeline never sees an out-of-range control
/// point; extreme region values are clamped (a documented MVP simplification).
fn build_tone_curve(shadows: f64, darks: f64, lights: f64, highlights: f64) -> Curves {
    let base: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let deltas = [shadows, darks, lights, highlights];
    let master: Vec<CurvePoint> = base
        .iter()
        .zip(deltas.iter())
        .map(|(bx, d)| CurvePoint {
            input: *bx as f32,
            output: ((bx + d).clamp(0.0, 1.0)) as f32,
        })
        .collect();
    Curves {
        version: 1,
        master,
        channels: CurveChannels::default(),
    }
}

/// Mutable reference to one HSL mixer channel, creating the `Option` slot on
/// first use so the GUI never has to special-case `None`.
fn hsl_channel_mut<'a>(hsl: &'a mut HslAdjustments, ch: &str) -> &'a mut HslChannel {
    let slot = match ch {
        "red" => &mut hsl.red,
        "orange" => &mut hsl.orange,
        "yellow" => &mut hsl.yellow,
        "green" => &mut hsl.green,
        "cyan" => &mut hsl.cyan,
        "blue" => &mut hsl.blue,
        "violet" => &mut hsl.violet,
        "magenta" => &mut hsl.magenta,
        _ => &mut hsl.red,
    };
    slot.get_or_insert_with(HslChannel::default)
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

/// Human-readable label for an [`ImageFileFormat`] used by the Export panel.
fn format_label(format: ImageFileFormat) -> &'static str {
    match format {
        ImageFileFormat::Png => "PNG",
        ImageFileFormat::Jpeg => "JPEG",
        ImageFileFormat::WebP => "WebP",
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
        // Apply the Lumina dark theme once per frame. `egui` only re-applies the
        // fields that changed, so this is cheap and keeps the Lightroom feeling
        // consistent across modules.
        apply_lightroom_dark(ctx);

        // Keyboard: `Y` toggles Before/After (which never mutates the recipe);
        // `Esc` cancels an armed white-balance eyedropper.
        if ctx.input(|i| i.key_pressed(egui::Key::Y)) {
            self.toggle_before_after();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.wb_pick_mode = false;
        }

        // Consume idle tasks only while there is no interactive pointer input.
        // Thumbnails are generated here; mask inference is handed to the (still
        // missing) ONNX adapter and only records the hand-off point.
        #[cfg(not(target_arch = "wasm32"))]
        if !ctx.input(|input| input.pointer.any_down()) {
            if let Some((_id, task)) = self.idle_queue.pop_next() {
                match task {
                    IdleTask::MaskInference { mask_id } => {
                        self.status = Str::InferenceWaiting.format_arg(&mask_id);
                    }
                    IdleTask::Thumbnail { source, name } => {
                        self.generate_thumbnail(ctx, &source, &name);
                    }
                }
            }
        }

        // Dropped files (path or bytes) load a new source.
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

        // Top: brand + status/error.
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

        // Top: module bar (Library / Develop / Export) + the Before/After toggle,
        // then the histogram of the currently displayed render state.
        egui::TopBottomPanel::top("modules").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (module, label) in [
                    (Module::Library, Str::Library),
                    (Module::Develop, Str::Develop),
                    (Module::Export, Str::Export),
                ] {
                    if ui
                        .selectable_label(self.active_module == module, label.t())
                        .clicked()
                    {
                        self.active_module = module;
                    }
                }
                ui.separator();
                if ui.button(Str::BeforeAfter.t()).clicked() {
                    self.toggle_before_after();
                }
            });
            self.draw_histogram(ui);
        });

        // Left: the Library file browser. Develop/Export leave the left edge to
        // the large navigator/preview working area.
        #[cfg(not(target_arch = "wasm32"))]
        if self.active_module == Module::Library {
            egui::SidePanel::left("browser")
                .resizable(true)
                .default_width(260.0)
                .show(ctx, |ui| self.draw_file_browser(ui));
        }

        // Right: Develop controls (eight sections), the Library sidecar/copy
        // manager, or nothing extra for Export (placeholder shown centrally).
        egui::SidePanel::right("controls")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| match self.active_module {
                Module::Develop => self.draw_develop_panel(ui),
                Module::Library => {
                    #[cfg(not(target_arch = "wasm32"))]
                    self.draw_library_panel(ui);
                    #[cfg(target_arch = "wasm32")]
                    {
                        ui.label(Str::NotAvailable.t());
                    }
                }
                Module::Export => {
                    #[cfg(not(target_arch = "wasm32"))]
                    self.draw_export_panel(ui);
                    #[cfg(target_arch = "wasm32")]
                    ui.label(Str::NotAvailable.t());
                }
            });

        // Bottom: filmstrip in Library/Develop. Native builds show generated
        // thumbnails (miss -> background job); the wasm build shows placeholders
        // only, since in-browser RAW/file IO is a documented native capability.
        let show_filmstrip = matches!(self.active_module, Module::Library | Module::Develop);
        if show_filmstrip {
            #[cfg(not(target_arch = "wasm32"))]
            egui::TopBottomPanel::bottom("filmstrip").show(ctx, |ui| self.draw_filmstrip(ctx, ui));
            #[cfg(target_arch = "wasm32")]
            egui::TopBottomPanel::bottom("filmstrip").show(ctx, |ui| {
                ui.heading(Str::Filmstrip.t());
                ui.label(Str::NotAvailable.t());
            });
        }

        // Central: the large preview/navigator. The Export module shows the
        // current render (what will be exported); the controls live in the
        // right-side Export panel. Under wasm32 (no file-system export) the
        // module is a clear capability hint.
        egui::CentralPanel::default().show(ctx, |ui| match self.active_module {
            Module::Export => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.horizontal(|ui| {
                        ui.label(Str::Navigator.t());
                        ui.separator();
                        ui.label(Str::Preview.t());
                    });
                    self.update_texture(ctx);
                    self.draw_preview(ui);
                    if let Some(key) = &self.render_key {
                        ui.label(format!(
                            "{}: {}",
                            Str::RenderStateCurrent.t(),
                            &key.digest()[..12]
                        ));
                    } else {
                        ui.colored_label(egui::Color32::YELLOW, Str::RenderStateStale.t());
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    ui.centered_and_justified(|ui| ui.label(Str::NotAvailable.t()));
                }
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.label(Str::Navigator.t());
                    ui.separator();
                    ui.label(Str::Preview.t());
                });
                self.update_texture(ctx);
                self.draw_preview(ui);
                if let Some(key) = &self.render_key {
                    ui.label(format!(
                        "{}: {}",
                        Str::RenderStateCurrent.t(),
                        &key.digest()[..12]
                    ));
                } else {
                    ui.colored_label(egui::Color32::YELLOW, Str::RenderStateStale.t());
                }
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
        BrushMark, BrushMarkSign, CoordinateSystem, DecodeFingerprint, GeometryFingerprint,
        MaskDefinition, MaskOperation, MaskPrompt, MaskStatus, ModelIdentity, Point2,
        Preprocessing, PromptTransform, Resolution, SourceFingerprint, SourceStatus,
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
        assert_eq!(app.status(), Str::Error.t());
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
        assert!(sidecar.is_file(), "Sidecar must be written");
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
        app.duplicate_virtual_copy("vc-2", "Copy 2").unwrap();
        app.save_sidecar();
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        assert_eq!(document.virtual_copies.len(), 2);
        assert!(document
            .virtual_copies
            .iter()
            .any(|copy| copy.id == "vc-2" && copy.name == "Copy 2"));
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
        app.duplicate_virtual_copy("vc-2", "Two").unwrap();
        app.duplicate_virtual_copy("vc-3", "Three").unwrap();
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
        assert_eq!(entry.status_label(), Str::Sidecar.t());
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
        assert_eq!(entry.status_label(), "Conflict");
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
                prompt: None,
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
        assert!(entry.has_sidecar);

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
        assert!(app.status().contains("recalculation"));
        app.mark_mask_for_recalculation().unwrap();
        assert!(app.status().contains("requested"));
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

    // ---- F-103-N2: single-control reset semantics + display scaling ----

    #[test]
    fn slider_reset_only_this_control_keeps_other_adjustments() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("exposure", 2.0);
        app.set_adjustment("contrast", 0.5);
        app.set_adjustment("wb_temperature", 7000.0);
        // Resetting one control must not touch the others or the whole recipe.
        app.reset_single_adjustment("exposure");
        assert_eq!(app.recipe().adjustments["exposure"], 0.0);
        assert_eq!(app.recipe().adjustments["contrast"], 0.5);
        assert_eq!(app.recipe().adjustments["wb_temperature"], 7000.0);
        assert!(!app.recipe().adjustments.is_empty());
    }

    #[test]
    fn display_scale_percent_maps_internal_domain() {
        // `-1..=1` is shown as `-100..+100`; Exposure/Kelvin stay identity.
        let percent = crate::slider::percent_spec(-1.0..=1.0, 0.0);
        assert_eq!(percent.scale, crate::slider::DisplayScale::Percent);
        assert_eq!(
            crate::slider::to_display(-1.0, crate::slider::DisplayScale::Percent),
            -100.0
        );
        assert_eq!(
            crate::slider::to_display(0.5, crate::slider::DisplayScale::Percent),
            50.0
        );
        assert_eq!(
            crate::slider::from_display(-100.0, crate::slider::DisplayScale::Percent),
            -1.0
        );
        let identity = crate::slider::identity_spec(-10.0..=10.0, 0.0, 0.1);
        assert_eq!(identity.scale, crate::slider::DisplayScale::Identity);
        assert_eq!(
            crate::slider::to_display(2.5, crate::slider::DisplayScale::Identity),
            2.5
        );
    }

    // ---- F-103-N3: Before/After + white-balance eyedropper ----

    #[test]
    fn before_after_toggle_does_not_mutate_recipe() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("exposure", 1.5);
        let snapshot = app.recipe().adjustments.clone();
        app.toggle_before_after();
        assert!(app.before_after);
        // The toggle only swaps the displayed frame; the recipe is untouched.
        assert_eq!(app.recipe().adjustments, snapshot);
        assert_eq!(app.recipe().adjustments["exposure"], 1.5);
        app.toggle_before_after();
        assert!(!app.before_after);
        assert_eq!(app.recipe().adjustments["exposure"], 1.5);
    }

    #[test]
    fn white_balance_eyedropper_sets_recipe_fields() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        // A colored picked point sets both WB fields and disarms the picker.
        app.set_white_balance_from_point(1.0, 0.5, 0.25).unwrap();
        assert!(app.recipe().adjustments.contains_key("wb_temperature"));
        assert!(app.recipe().adjustments.contains_key("wb_tint"));
        let temp = app.recipe().adjustments["wb_temperature"];
        assert!((1500.0..=12000.0).contains(&temp));
        assert!(!app.wb_pick_mode);
        // A neutral grey point is the documented default (6500 K, tint 0).
        app.set_white_balance_from_point(0.5, 0.5, 0.5).unwrap();
        assert_eq!(app.recipe().adjustments["wb_temperature"], 6500.0);
        assert_eq!(app.recipe().adjustments["wb_tint"], 0.0);
        // Non-positive channels cannot derive a white balance.
        assert!(app.set_white_balance_from_point(0.0, 0.5, 0.5).is_err());
    }

    // ---- Filmstrip helpers (headless) ----

    #[test]
    fn filmstrip_downscale_keeps_small_images_and_shrinks_large() {
        let small = vec![1u8, 2, 3, 255, 4, 5, 6, 255];
        let (out, w, h) = crate::filmstrip::downscale_rgba(&small, 2, 1, 160);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, small);

        let big = vec![0u8; (4 * 320 * 200) as usize];
        let (out2, w2, h2) = crate::filmstrip::downscale_rgba(&big, 320, 200, 160);
        assert_eq!((w2, h2), (160, 100));
        assert_eq!(out2.len(), 4 * 160 * 100);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn filmstrip_cache_miss_then_hit_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let name = source.file_name().unwrap().to_string_lossy().to_string();
        let cache = lumina_core::cache::disk::DiskFolderCache::for_image(&source).unwrap();
        // No preview on disk yet -> miss (no silent fallback to a wrong image).
        assert!(!crate::filmstrip::filmstrip_preview_cached(
            &cache,
            &name,
            "vc-original"
        ));
        let thumbnail = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        cache
            .store_preview(
                &name,
                "vc-original",
                lumina_core::cache::PreviewKind::Standard,
                &thumbnail,
            )
            .unwrap();
        // After storing, the same probe is a cache hit.
        assert!(crate::filmstrip::filmstrip_preview_cached(
            &cache,
            &name,
            "vc-original"
        ));
    }

    // ---- F-103-N4: interactive mask tools (Brush / Linear / Radial) ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn brush_marks_roundtrip_through_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        let id = app.create_mask("Subject").unwrap();
        let marks = vec![
            BrushMark {
                x: 0.2,
                y: 0.3,
                radius: 0.05,
                sign: BrushMarkSign::Positive,
            },
            BrushMark {
                x: 0.5,
                y: 0.6,
                radius: 0.05,
                sign: BrushMarkSign::Positive,
            },
        ];
        app.commit_brush_stroke(marks).unwrap();

        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        let mask = document.virtual_copies[0]
            .mask_library
            .iter()
            .find(|m| m.id == id)
            .unwrap();
        assert_eq!(
            mask.prompt,
            Some(MaskPrompt::Brush {
                marks: vec![
                    BrushMark {
                        x: 0.2,
                        y: 0.3,
                        radius: 0.05,
                        sign: BrushMarkSign::Positive,
                    },
                    BrushMark {
                        x: 0.5,
                        y: 0.6,
                        radius: 0.05,
                        sign: BrushMarkSign::Positive,
                    },
                ],
                resolution: (2, 1),
                transformation: PromptTransform::default(),
            })
        );
        // The active layer references the same mask.
        assert_eq!(document.virtual_copies[0].mask_layers[0].mask.mask_id, id);

        // The prompt survives a reopen.
        let mut reopened = new_app();
        reopened.open_file(source.display().to_string());
        let reloaded = reopened.document.as_ref().unwrap();
        let reloaded_mask = reloaded.virtual_copies[0]
            .mask_library
            .iter()
            .find(|m| m.id == id)
            .unwrap();
        assert!(reloaded_mask.prompt.is_some());
        assert_eq!(reloaded_mask.status, MaskStatus::Valid);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn empty_brush_stroke_is_visible_error_and_writes_no_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.create_mask("Subject").unwrap();
        // An empty stroke is rejected by the commit path (returns Err, no write).
        let result = app.commit_brush_stroke(vec![]);
        assert!(result.is_err());

        // The interactive UI path surfaces it as a visible GuiError via
        // `finish_drawing` (which the preview drag-stop calls) and writes
        // nothing to disk.
        app.set_mask_tool(MaskTool::Brush);
        app.pending_brush_marks.clear();
        app.drag_start = Some(Point2 { x: 0.3, y: 0.3 });
        app.drag_current = Some(Point2 { x: 0.3, y: 0.3 });
        app.finish_drawing();
        assert_eq!(app.status(), Str::Error.t());
        assert!(app.error().is_some());

        // No sidecar was written (the empty stroke never persisted).
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        assert!(!sidecar.is_file());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gradient_coordinate_calculation_from_drag() {
        // Helper exposes the prompt-building math directly.
        let p = LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 0.1, y: 0.5 },
            Point2 { x: 0.9, y: 0.5 },
        );
        // Horizontal drag -> 0 degrees; start/end are the documented matte values.
        match p {
            MaskPrompt::Gradient {
                angle_deg,
                start,
                end,
                ..
            } => {
                assert!((angle_deg - 0.0).abs() < 1e-3);
                assert_eq!(start, 1.0);
                assert_eq!(end, 0.0);
            }
            _ => panic!("expected gradient prompt"),
        }

        match LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 0.5, y: 0.1 },
            Point2 { x: 0.5, y: 0.9 },
        ) {
            MaskPrompt::Gradient { angle_deg, .. } => {
                assert!((angle_deg - 90.0).abs() < 1e-3, "vertical drag -> 90°")
            }
            _ => panic!("expected gradient prompt"),
        }

        match LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 0.9, y: 0.5 },
            Point2 { x: 0.1, y: 0.5 },
        ) {
            MaskPrompt::Gradient { angle_deg, .. } => {
                assert!((angle_deg - 180.0).abs() < 1e-3, "right-to-left -> 180°")
            }
            _ => panic!("expected gradient prompt"),
        }

        match LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 0.9, y: 0.9 },
            Point2 { x: 0.1, y: 0.1 },
        ) {
            MaskPrompt::Gradient { angle_deg, .. } => assert!(
                (angle_deg - 225.0).abs() < 1e-3,
                "up-left -> 225° (negative direction normalized to [0,360))"
            ),
            _ => panic!("expected gradient prompt"),
        }

        // Points outside 0..=1 are clamped before the angle is computed.
        match LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 2.0, y: 0.5 },
            Point2 { x: -1.0, y: 0.5 },
        ) {
            // After clamping: (1.0,0.5)->(0.0,0.5) -> dx=-1.0 -> 180°.
            MaskPrompt::Gradient { angle_deg, .. } => {
                assert!((angle_deg - 180.0).abs() < 1e-3)
            }
            _ => panic!("expected gradient prompt"),
        }

        // A zero-length drag is rejected by the commit path.
        let mut app = new_app();
        assert!(app
            .commit_gradient(Point2 { x: 0.5, y: 0.5 }, Point2 { x: 0.5001, y: 0.5 })
            .is_err());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn ellipse_generated_from_center_and_radii() {
        match LuminaApp::ellipse_prompt_from_drag(
            Point2 { x: 0.2, y: 0.2 },
            Point2 { x: 0.8, y: 0.6 },
        ) {
            MaskPrompt::Ellipse { center, radii, .. } => {
                assert!((center.x - 0.5).abs() < 1e-6);
                assert!((center.y - 0.4).abs() < 1e-6);
                assert!((radii.x - 0.3).abs() < 1e-6);
                assert!((radii.y - 0.2).abs() < 1e-6);
            }
            _ => panic!("expected ellipse prompt"),
        }

        // A gradient/radial prompt also persists through the sidecar.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        let id = app.create_mask("Sky").unwrap();
        app.commit_gradient(Point2 { x: 0.1, y: 0.5 }, Point2 { x: 0.9, y: 0.5 })
            .unwrap();
        app.create_mask("Sun").unwrap();
        app.commit_radial(Point2 { x: 0.2, y: 0.2 }, Point2 { x: 0.8, y: 0.6 })
            .unwrap();

        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
        let sky = document.virtual_copies[0]
            .mask_library
            .iter()
            .find(|m| m.id == id)
            .unwrap();
        assert!(matches!(sky.prompt, Some(MaskPrompt::Gradient { .. })));

        let radial = document.virtual_copies[0]
            .mask_library
            .iter()
            .find(|m| m.name == "Sun")
            .unwrap();
        assert!(matches!(radial.prompt, Some(MaskPrompt::Ellipse { .. })));
    }

    // ---- F-103-N5: shared export path is byte-identical to the CLI ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gui_export_is_byte_identical_to_shared_export_path() {
        // The GUI export module must produce the exact same bytes as the CLI's
        // shared `lumina_core::export_image` (render + encode) for the same
        // source frame, recipe and export options. PNG is used for the byte
        // comparison because its encoder is deterministic (see
        // feature/platform/cli-gui-wasm.md, "Desktop-GUI / Export-Determinismus").
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let bytes = std::fs::read(&source).unwrap();
        let recipe = EditRecipe {
            adjustments: std::collections::BTreeMap::from([("exposure".into(), 0.7)]),
            ..Default::default()
        };

        // CLI-style shared path: decode + render + encode via the single shared
        // function, with the same neutral context the GUI uses (no masks, no
        // source actions, no white balance for a raster PNG).
        let frame = ImageFrame::decode(&bytes).unwrap();
        let context = RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            #[cfg(feature = "lensfun")]
            lensfun: None,
        };
        let options = ExportOptions {
            format: ImageFileFormat::Png,
            quality: 90,
            dither: false,
            ..Default::default()
        };
        let cli_bytes = export_image(&frame, &context, options).unwrap();

        // GUI path: load the same bytes, set the same recipe, then export via the
        // module's own `export_to` (which internally calls `export_image`).
        let mut app = new_app();
        app.load_bytes(bytes.clone(), "photo.png").unwrap();
        app.set_adjustment("exposure", 0.7);
        app.render().unwrap();
        app.export_format = ImageFileFormat::Png;
        app.export_quality = 90;
        let out = directory.path().join("photo_export.png");
        app.export_to(out.clone()).unwrap();
        let gui_bytes = std::fs::read(&out).unwrap();

        assert_eq!(
            cli_bytes, gui_bytes,
            "GUI and CLI/shared export paths must be byte-identical"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gui_jpeg_export_is_functional_and_byte_identical_to_shared_path() {
        // JPEG is functionally validated (deterministic within one encoder
        // version, see feature/platform/cli-gui-wasm.md,
        // "Desktop-GUI / Export-Determinismus"): the GUI JPEG export equals the
        // shared `export_image` JPEG export and decodes to the same dimensions
        // as the source.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let bytes = std::fs::read(&source).unwrap();

        let frame = ImageFrame::decode(&bytes).unwrap();
        let context = RenderContext {
            recipe: &EditRecipe::default(),
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            #[cfg(feature = "lensfun")]
            lensfun: None,
        };
        let options = ExportOptions {
            format: ImageFileFormat::Jpeg,
            quality: 85,
            dither: false,
            ..Default::default()
        };
        let cli_bytes = export_image(&frame, &context, options).unwrap();

        let mut app = new_app();
        app.load_bytes(bytes.clone(), "photo.png").unwrap();
        app.export_format = ImageFileFormat::Jpeg;
        app.export_quality = 85;
        let out = directory.path().join("photo_export.jpg");
        app.export_to(out.clone()).unwrap();
        let gui_bytes = std::fs::read(&out).unwrap();

        // Both paths use the identical image encoder call, so the bytes match.
        assert_eq!(
            cli_bytes, gui_bytes,
            "JPEG GUI and shared export must match"
        );
        // And the decoded JPEG has the same pixel dimensions as the source.
        let decoded = ImageFrame::decode(&gui_bytes).unwrap();
        assert_eq!((decoded.width, decoded.height), (frame.width, frame.height));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn export_rejects_same_path_as_gui_error() {
        // Exporting onto the source file is rejected (non-destructive contract);
        // nothing is written.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.set_adjustment("exposure", 0.3);
        let result = app.export_to(source.clone());
        assert!(result.is_err(), "exporting onto the source must fail");
        // No export artifact was created with the source's name.
        assert!(!source.with_extension("jpg").exists());
        assert!(source.is_file());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn export_preserves_original_bytes_unchanged() {
        // The original source file is byte-for-byte untouched by an export.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let before = blake3::hash(&std::fs::read(&source).unwrap());
        let mut app = new_app();
        app.open_file(source.display().to_string());
        app.set_adjustment("contrast", 0.4);
        app.render().unwrap();
        let out = directory.path().join("photo_out.png");
        app.export_to(out.clone()).unwrap();
        let after = blake3::hash(&std::fs::read(&source).unwrap());
        assert_eq!(
            before, after,
            "original must be byte-identical after export"
        );
        assert!(out.is_file(), "export artifact was written");
    }

    #[test]
    fn export_options_validate_quality_range() {
        // Quality must be in 1..=100; 0 and 101 are rejected, 1 and 100 ok.
        assert!(ExportOptions {
            format: ImageFileFormat::Png,
            quality: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(ExportOptions {
            format: ImageFileFormat::Png,
            quality: 101,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(ExportOptions {
            format: ImageFileFormat::Png,
            quality: 1,
            ..Default::default()
        }
        .validate()
        .is_ok());
        assert!(ExportOptions {
            format: ImageFileFormat::Png,
            quality: 100,
            ..Default::default()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn image_format_from_extension_parses_known_and_rejects_unknown() {
        assert_eq!(
            ImageFileFormat::from_extension("png"),
            Some(ImageFileFormat::Png)
        );
        assert_eq!(
            ImageFileFormat::from_extension("JPG"),
            Some(ImageFileFormat::Jpeg)
        );
        assert_eq!(
            ImageFileFormat::from_extension("jpeg"),
            Some(ImageFileFormat::Jpeg)
        );
        assert_eq!(
            ImageFileFormat::from_extension("webp"),
            Some(ImageFileFormat::WebP)
        );
        assert_eq!(ImageFileFormat::from_extension("tiff"), None);
        assert_eq!(ImageFileFormat::from_extension(""), None);
        assert_eq!(
            ImageFileFormat::default_extension(ImageFileFormat::Jpeg),
            "jpg"
        );
        assert_eq!(
            ImageFileFormat::default_extension(ImageFileFormat::Png),
            "png"
        );
    }
}
