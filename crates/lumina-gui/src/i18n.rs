//! Central UI string table (i18n scaffold).
//!
//! Every user-visible string in the GUI is routed through [`Str`] so that the
//! English UI can later gain a `de` (and other) translations without touching
//! panel code.  Per the F-100 product decision (2026-08-21) the MVP ships
//! English only; the German section names from the SOLL are the reference
//! translation and are kept here as the documented target for a future locale.
//!
//! Mechanism: a single enum of string *keys*.  Each key maps to its current
//! (English) text via [`Str::t`].  Adding a new visible string means adding a
//! variant here; there are intentionally no free-form literals in the panel
//! functions.

/// Stable key for a user-visible string.
///
/// The enum is the single source of truth for which strings exist; the match in
/// [`Str::t`] is the translation table.  Adding a string without wiring it here
/// is a compile error, which keeps the table exhaustive.
///
/// The table covers every panel of the desktop GUI
/// (`feature/platform/cli-gui-wasm.md` § UI-Konventionen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum Str {
    // Module bar
    Library,
    Export,
    // Module bar labels with keyboard shortcut (`{}` is the key letter).
    LibraryShortcut,
    DevelopShortcut,
    ExportTarget,
    ExportFormatLabel,
    ExportQualityLabel,
    ExportChoose,
    ExportUseSuggested,
    ExportRun,
    ExportQualityUnused,
    NoImage,
    NotCurrent,
    Open,
    Refresh,
    Load,
    ChooseFile,
    Copies,
    Sidecar,

    // Panels / sections
    Basic,
    ToneCurve,
    Color,
    Effects,
    Detail,
    Optics,
    Geometry,
    Masking,
    Folders,
    History,
    PresetsSection,
    Navigator,
    Preview,
    Zoom,
    ZoomFit,
    Zoom25,
    Zoom50,
    Zoom75,
    Zoom100,
    ZoomCustom,
    ZoomOneToOne,
    ZoomTwoHundred,
    ZoomFitWidth,
    Histogram,
    HistogramDraft,
    Draft,
    Filmstrip,
    LibraryThumbSize,

    // Basic section
    WhiteBalance,
    Temperature,
    Tint,
    Exposure,
    Contrast,
    Highlights,
    Shadows,
    Whites,
    Blacks,
    Auto,

    // Tone curve (parametric region sliders)
    CurveRegions,
    ToneCurveShadows,
    ToneCurveDarks,
    ToneCurveLights,
    ToneCurveHighlights,

    // Color — HSL mixer
    HslMixer,
    Hue,
    Saturation,
    Luminance,

    // Color — HSL channel names
    HslRed,
    HslOrange,
    HslYellow,
    HslGreen,
    HslCyan,
    HslBlue,
    HslViolet,
    HslMagenta,

    // Color — Color Grading
    ColorGrading,
    GradingShadows,
    GradingMidtones,
    GradingHighlights,
    GradingBalance,

    // Color — Presence (F-094) and Dynamics/Saturation (F-092)
    Presence,
    Texture,
    Clarity,
    Dehaze,
    Vibrance,

    // Effects
    Vignette,
    Amount,
    Midpoint,
    Roundness,
    Feather,
    Grain,
    Size,
    Roughness,
    Seed,

    // Detail
    Sharpening,
    Radius,
    NoiseReduction,

    // Optics (F-098)
    LensCorrection,
    DistortionK1,
    DistortionK2,
    DistortionK3,
    VignetteC0,
    VignetteC1,
    VignetteC2,
    ChromaticRed,
    ChromaticBlue,
    OpticsRequiresLensfun,
    // GUI-OPTICS-1: visible profile status + grouped, self-explanatory
    // manual-correction controls (no silent inactive state).
    OpticsProfileNone,
    OpticsProfilePattern,
    OpticsDistortionGroup,
    OpticsDistortionHint,
    OpticsVignetteGroup,
    OpticsVignetteHint,
    OpticsCaGroup,
    OpticsCaHint,

    // Geometry (F-093 / F-099)
    Crop,
    Rotation,
    MirrorHorizontal,
    MirrorVertical,
    RotateLeft,
    RotateRight,
    Perspective,
    Vertical,
    Horizontal,
    Scale,
    AspectRatio,
    ShiftX,
    ShiftY,
    GeometryRequiresLensfun,

    // Masking
    NewMask,
    SelectMask,
    Invert,
    OfferRecalculation,
    LocalAdjustments,

    // Masking — interactive tools (F-103-N4)
    MaskTool,
    MaskToolBrush,
    MaskToolGradient,
    MaskToolRadial,
    MaskToolNone,
    BrushSize,
    BrushEraser,
    DrawMaskHint,
    Blur,
    Density,

    // Interactions
    BeforeAfter,
    WbEyedropper,
    WbEyedropperActive,
    Cancel,
    Reset,
    MatchExposure,
    SyncSettings,
    MatchSelection,
    ExposureRelative,
    ApplyPreset,
    RenderApply,
    SaveRecipe,
    NotAvailable,
    RenderStateStale,
    RenderStateCurrent,
    FilmstripHint,
    PickWhiteBalanceHint,

    // File-browser status shorthand
    StatusConflict,
    StatusOffline,
    StatusWithout,

    // Status / error literals (F-103-N4/N5 masking & export paths).
    // UI language is English per the F-100 product decision (2026-08-21);
    // every user-visible string from the new masking/export code routes
    // through one of these keys instead of a free-form German literal.
    ReadyForImage,
    PresetNameEmpty,
    NoSidecarLoaded,
    VirtualCopyNotFound,
    MaskNotFound,
    MaskNameEmpty,
    NoImageLoaded,
    MaskNameExists,
    MaskCreated,
    MaskRenamed,
    InvalidLocalAdjustment,
    LocalAdjustmentSaved,
    NoMaskSelected,
    MaskStaleRecalc,
    MaskCurrentNoRecalc,
    ExplicitRecalcRequested,
    IdleQueueFull,
    RecalcRequested,
    ChangePending,
    PreviewCurrent,
    Error,
    AutoToneStale,
    SaveNeedsLocalPath,
    SidecarSaved,
    MaskUnavailable,
    HistoryEntryMissing,
    NoHistory,
    NoPresets,

    // F-009 file-backed user presets (`<name>.lumina-preset.json`).
    PresetsFolder,
    PresetsUnavailable,
    SavePresetFile,
    PresetSaved,
    PresetApplied,

    // Local-adjustment validation & status literals that were previously free-form
    // German strings. UI language is English (F-100); routed through these keys so
    // the panel code carries no literal text.
    RelativeExposureRequiresAutoTone,
    FeatheringMustBeBetween,
    Loaded,
    // GUI-TOAST-OVERLAP-1: transient overlay toast + neighbor-preview cell
    // states (previously free-form German literals painted over thumbnails).
    ToastPreviewReady,
    ToastDismiss,
    NeighborLoading,
    NeighborStale,
    NeighborFailedPattern,

    // Parameterized patterns (use with `format!`); the `{}` placeholder is
    // replaced positionally by the caller.
    ImagesInDirectory,
    DirectoryNotReadable,
    UnknownAdjustment,
    MaskSelected,
    MaskPromptSaved,
    MaskUnavailableLayer,
    InferenceWaiting,

    // GPU present-path routing feedback (R2-GUIMOD-06): the previously silent
    // GPU→CPU fallback is surfaced here as a visible status badge / tooltip
    // instead of only a stderr `log::warn!`.
    CpuFallbackUnsupportedStages,
    CpuFallbackTooltip,

    // Legacy parameterized patterns (use with `format!`)
    // GEN-FILL-02: manual expand beyond image
    ExpandBeyondImage,
    ExpandCanvasLabel,
    ExpandOffsetX,
    ExpandOffsetY,
    ExpandHint,
    ExpandCanvasInvalid,

    HuePattern,
    SatPattern,
    UnsetPattern,

    // LR-01 rating / flag (Library badge + rating section + shortcuts).
    Rating,
    FlagLabel,
    Pick,
    Reject,
    Unflagged,
    InvalidRating,
    RatingSetPattern,
    FlagSetPattern,
    VirtualCopyDuplicatedPattern,

    // LR-PARITY-01 Welle 2: copy/paste settings, color label (6-9), B&W (V),
    // clipping (J), lights out (L), crop mode (R), panel hide (Tab).
    SettingsCopied,
    SettingsPasted,
    ClipboardEmpty,
    ColorLabel,
    ColorRed,
    ColorYellow,
    ColorGreen,
    ColorBlue,
    InvalidColorLabel,
    ColorLabelSetPattern,
    CropModeOn,
    CropModeOff,
    PanelsHiddenOn,
    PanelsHiddenOff,
    LightsOutOn,
    LightsOutOff,
    ClippingOn,
    ClippingOff,
    ClippingDetailPattern,
    BlackWhiteOn,
    BlackWhiteOff,
    SliderResetHint,

    // LR-PARITY-01 Welle 3: filter drawer (`\`), compare/survey (C/N),
    // split (Shift+Y), fullscreen (F), snapshots, stack (Cmd+G),
    // quick develop, import/export shortcuts.
    FilterBar,
    FilterPlaceholder,
    FilterShown,
    FilterHidden,
    QuickDevelop,
    CompareModeCompare,
    CompareModeSurvey,
    CompareOnPattern,
    CompareOff,
    SurveyOn,
    SplitViewOn,
    SplitViewOff,
    FullscreenOn,
    FullscreenOff,
    SnapshotNamePattern,
    SnapshotCreatedPattern,
    InvalidSnapshotName,
    NotSnapshot,
    StackGroupedPattern,
    StackUngrouped,
    QuickDevelopAppliedPattern,
    GotoLibraryImport,
    GotoExport,

    // G-11 overlay/panel comfort (LRPAR-G11-OVERLAYS): tool overlay modes,
    // edit-pin visibility, solo mode, Shift+Tab all-panels toggle.
    OverlayModeLabel,
    OverlayAlways,
    OverlayAuto,
    OverlayNever,
    OverlayModeSetPattern,
    PinVisibilityLabel,
    PinVisibilitySetPattern,
    SoloMode,
    SoloModeOn,
    SoloModeOff,
    AllPanelsHiddenOn,
    AllPanelsHiddenOff,
    SpotOverlayHint,
}
impl Str {
    /// Returns the English text for this key.  This is the only place literals
    /// live; the future `de` locale would be a second match arm selected by a
    /// global locale setting.
    /// Formats a parameterized pattern key by replacing the single `{}`
    /// placeholder with `arg`.  Used for pattern keys such as
    /// [`Str::HuePattern`], [`Str::SatPattern`] and [`Str::UnsetPattern`]; the
    /// future translation table controls word order via the pattern itself.
    pub fn format_arg(self, arg: &str) -> String {
        self.t().replacen("{}", arg, 1)
    }

    pub fn t(self) -> &'static str {
        match self {
            Str::Library => "Library",
            Str::Export => "Export",
            Str::LibraryShortcut => "Library ({})",
            Str::DevelopShortcut => "Develop ({})",
            Str::ExportTarget => "Export to",
            Str::ExportFormatLabel => "Format",
            Str::ExportQualityLabel => "Quality",
            Str::ExportChoose => "Choose…",
            Str::ExportUseSuggested => "Use suggested name",
            Str::ExportRun => "Export",
            Str::ExportQualityUnused => "Quality applies to JPEG / WebP only",
            Str::NoImage => "Drop an image here or load a path",
            Str::NotCurrent => "Not current",
            Str::Open => "Open",
            Str::Refresh => "Refresh",
            Str::Load => "Load",
            Str::ChooseFile => "Choose file",
            Str::Copies => "copies",
            Str::Sidecar => "Sidecar",

            Str::Basic => "Basic",
            Str::ToneCurve => "Tone Curve",
            Str::Color => "Color",
            Str::Effects => "Effects",
            Str::Detail => "Detail",
            Str::Optics => "Optics",
            Str::Geometry => "Geometry",
            Str::Masking => "Masking",
            Str::Folders => "Folders",
            Str::History => "History",
            Str::PresetsSection => "Presets",
            Str::Navigator => "Navigator",
            Str::Preview => "Preview",
            Str::Zoom => "Zoom",
            Str::ZoomFit => "Fit",
            Str::Zoom25 => "25%",
            Str::Zoom50 => "50%",
            Str::Zoom75 => "75%",
            Str::Zoom100 => "100%",
            Str::ZoomCustom => "Custom",
            Str::ZoomOneToOne => "1:1",
            Str::ZoomTwoHundred => "200%",
            Str::ZoomFitWidth => "Fit Width",
            Str::Histogram => "Histogram",
            Str::HistogramDraft => "Draft preview — histogram reflects the low-res draft until the full render completes",
            Str::Draft => "Draft",
            Str::Filmstrip => "Filmstrip",
            Str::LibraryThumbSize => "Thumbnail Size",

            Str::WhiteBalance => "White Balance",
            Str::Temperature => "Temperature",
            Str::Tint => "Tint",
            Str::Exposure => "Exposure",
            Str::Contrast => "Contrast",
            Str::Highlights => "Highlights",
            Str::Shadows => "Shadows",
            Str::Whites => "Whites",
            Str::Blacks => "Blacks",
            Str::Auto => "Auto",

            Str::CurveRegions => "Parametric regions",
            Str::ToneCurveShadows => "Shadows",
            Str::ToneCurveDarks => "Darks",
            Str::ToneCurveLights => "Lights",
            Str::ToneCurveHighlights => "Highlights",

            Str::HslMixer => "HSL / Color Mixer",
            Str::Hue => "Hue",
            Str::Saturation => "Saturation",
            Str::Luminance => "Luminance",

            Str::HslRed => "red",
            Str::HslOrange => "orange",
            Str::HslYellow => "yellow",
            Str::HslGreen => "green",
            Str::HslCyan => "cyan",
            Str::HslBlue => "blue",
            Str::HslViolet => "violet",
            Str::HslMagenta => "magenta",

            Str::ColorGrading => "Color Grading",
            Str::GradingShadows => "Shadows",
            Str::GradingMidtones => "Midtones",
            Str::GradingHighlights => "Highlights",
            Str::GradingBalance => "Balance",

            Str::Presence => "Presence",
            Str::Texture => "Texture",
            Str::Clarity => "Clarity",
            Str::Dehaze => "Dehaze",
            Str::Vibrance => "Vibrance",

            Str::Vignette => "Vignette",
            Str::Amount => "Amount",
            Str::Midpoint => "Midpoint",
            Str::Roundness => "Roundness",
            Str::Feather => "Feather",
            Str::Grain => "Grain",
            Str::Size => "Size",
            Str::Roughness => "Roughness",
            Str::Seed => "Seed",

            Str::Sharpening => "Sharpening",
            Str::Radius => "Radius",
            Str::NoiseReduction => "Noise Reduction",

            Str::LensCorrection => "Lens Correction",
            Str::DistortionK1 => "Distortion K1 (r²)",
            Str::DistortionK2 => "Distortion K2 (r⁴)",
            Str::DistortionK3 => "Distortion K3 (r⁶)",
            Str::VignetteC0 => "Vignette C0 (center)",
            Str::VignetteC1 => "Vignette C1 (mid)",
            Str::VignetteC2 => "Vignette C2 (corners)",
            Str::ChromaticRed => "CA Red (lateral)",
            Str::ChromaticBlue => "CA Blue (lateral)",
            Str::OpticsRequiresLensfun => {
                "Not available: the native Lensfun pipeline stage is disabled in this build."
            }
            Str::OpticsProfileNone => {
                "No lens profile — automatic correction inactive (manual sliders below apply on render)"
            }
            Str::OpticsProfilePattern => "Lens profile: {}",
            Str::OpticsDistortionGroup => "Distortion (radial)",
            Str::OpticsDistortionHint => {
                "Radial distortion, K1·r² + K2·r⁴ + K3·r⁶: negative values correct \
                 barrel distortion, positive values pincushion. Manual model — it \
                 applies on render even without a lens profile."
            }
            Str::OpticsVignetteGroup => "Vignette (light falloff)",
            Str::OpticsVignetteHint => {
                "Vignette correction, C0 + C1·r² + C2·r⁴: brightens darkened \
                 corners. Manual model — it applies on render even without a \
                 lens profile."
            }
            Str::OpticsCaGroup => "Chromatic aberration (lateral)",
            Str::OpticsCaHint => {
                "Lateral chromatic aberration: shifts the red/blue channels \
                 radially to cancel color fringes. Manual model — it applies \
                 on render even without a lens profile."
            }

            Str::Crop => "Crop",
            Str::Rotation => "Rotation",
            Str::MirrorHorizontal => "Mirror Horizontal",
            Str::MirrorVertical => "Mirror Vertical",
            Str::RotateLeft => "Rotate Left 90°",
            Str::RotateRight => "Rotate Right 90°",
            Str::Perspective => "Perspective",
            Str::Vertical => "Vertical",
            Str::Horizontal => "Horizontal",
            Str::Scale => "Scale",
            Str::AspectRatio => "Aspect Ratio",
            Str::ShiftX => "Shift X",
            Str::ShiftY => "Shift Y",
            Str::GeometryRequiresLensfun => {
                "Not available: crop / perspective require the native Lensfun geometry stage, disabled in this build."
            }

            Str::NewMask => "New Mask",
            Str::SelectMask => "Select Mask",
            Str::Invert => "Invert",
            Str::OfferRecalculation => "Recalculation",
            Str::LocalAdjustments => "Local adjustments",

            Str::MaskTool => "Tool",
            Str::MaskToolBrush => "Brush",
            Str::MaskToolGradient => "Linear Gradient",
            Str::MaskToolRadial => "Radial Gradient",
            Str::MaskToolNone => "Off",
            Str::BrushSize => "Brush Size",
            Str::BrushEraser => "Eraser",
            Str::DrawMaskHint => {
                "Drag on the preview to draw the mask; the overlay shows the exact matte."
            }
            Str::Blur => "Blur",
            Str::Density => "Density",

            Str::BeforeAfter => "Before / After (Y)",
            Str::WbEyedropper => "WB Eyedropper",
            Str::WbEyedropperActive => "WB Eyedropper (Esc to cancel)",
            Str::Cancel => "Cancel",
            Str::Reset => "Reset",
            Str::MatchExposure => "Match Total Exposure",
            Str::SyncSettings => "Sync Settings",
            Str::MatchSelection => "Match Total Exposures",
            Str::ExposureRelative => "Exposure relative",
            Str::ApplyPreset => "Create & Apply Preset",
            Str::RenderApply => "Render / Apply",
            Str::SaveRecipe => "Save Recipe / Sidecar",
            Str::NotAvailable => "Not available",
            Str::RenderStateStale => "Render state stale / pending",
            Str::RenderStateCurrent => "Render state current",
            Str::FilmstripHint => "Click a thumbnail to open it",
            Str::PickWhiteBalanceHint => "Click the preview to pick white balance",
            Str::StatusConflict => "Conflict",
            Str::StatusOffline => "Offline",
            Str::StatusWithout => "No sidecar",

            Str::ReadyForImage => "Ready for a PNG, JPEG or WebP",
            Str::PresetNameEmpty => "Preset name must not be empty",
            Str::NoSidecarLoaded => "No sidecar loaded",
            Str::VirtualCopyNotFound => "Virtual copy not found",
            Str::MaskNotFound => "Mask not found",
            Str::MaskNameEmpty => "Mask name must not be empty",
            Str::NoImageLoaded => "No image loaded",
            Str::MaskNameExists => "A mask with this name already exists",
            Str::MaskCreated => "Mask created; recalculation explicitly required",
            Str::MaskRenamed => "Mask renamed; save sidecar",
            Str::InvalidLocalAdjustment => "Invalid local adjustment",
            Str::LocalAdjustmentSaved => {
                "Local mask adjustment saved (pipeline support pending)"
            }
            Str::NoMaskSelected => "No mask selected",
            Str::MaskStaleRecalc => "Mask stale/unavailable; start recalculation?",
            Str::MaskCurrentNoRecalc => "Mask current; no recalculation required",
            Str::ExplicitRecalcRequested => "Explicit recalculation requested",
            Str::IdleQueueFull => "Idle queue is full",
            Str::RecalcRequested => "Recalculation requested; job control required",
            Str::ChangePending => "Change pending",
            Str::PreviewCurrent => "Preview current",
            Str::Error => "Error",
            Str::AutoToneStale => "Auto-Tone stale; recalculation required",
            Str::SaveNeedsLocalPath => {
                "To save, the image must be loaded via a local path"
            }
            Str::SidecarSaved => "Sidecar saved",
            Str::MaskUnavailable => {
                "Warning: mask unavailable; it is not applied in the preview"
            }
            Str::HistoryEntryMissing => "History entry not found",
            Str::NoHistory => "No history entries",
            Str::NoPresets => "No presets saved yet",

            Str::PresetsFolder => "Folder:",
            Str::PresetsUnavailable => {
                "User presets unavailable: the config directory could not be determined"
            }
            Str::SavePresetFile => "Save as preset file",
            Str::PresetSaved => "Preset saved: {}",
            Str::PresetApplied => "Applied preset: {}",

            Str::RelativeExposureRequiresAutoTone => {
                "Relative Exposure requires active Auto-Tone"
            }
            Str::FeatheringMustBeBetween => "Feathering must be between 0 and 1",
            Str::Loaded => "Loaded: {}",
            Str::ToastPreviewReady => "Preview ready",
            Str::ToastDismiss => "Dismiss",
            Str::NeighborLoading => "Preparing preview…",
            Str::NeighborStale => "Stale",
            Str::NeighborFailedPattern => "Error: {}",

            Str::ImagesInDirectory => "{} images in directory",
            Str::DirectoryNotReadable => "Directory not readable: {}",
            Str::UnknownAdjustment => "Unknown adjustment: {}",
            Str::MaskSelected => "Mask selected: {}",
            Str::MaskPromptSaved => "Mask prompt saved: {}",
            Str::MaskUnavailableLayer => {
                "Warning: mask unavailable (layer {}); it is not applied in the preview"
            }
            Str::InferenceWaiting => "Mask {}: background job waiting for inference engine",

            Str::CpuFallbackUnsupportedStages => {
                "Render routed to CPU: GPU does not support all adjustments in this recipe"
            }
            Str::CpuFallbackTooltip => {
                "The GPU present path is unavailable for this recipe, so the preview \
                 falls back to the CPU renderer. The visible pixels are identical."
            }

            Str::ExpandBeyondImage => "Expand beyond image",
            Str::ExpandHint => "When enabled, the canvas can be expanded beyond the original image",
            Str::ExpandCanvasLabel => "Canvas size",
            Str::ExpandOffsetX => "Offset X",
            Str::ExpandOffsetY => "Offset Y",
            Str::ExpandCanvasInvalid => "Invalid canvas",

            Str::HuePattern => "{} Hue",
            Str::SatPattern => "{} Sat",
            Str::UnsetPattern => "{} (unset)",

            Str::Rating => "Rating",
            Str::FlagLabel => "Flag",
            Str::Pick => "Pick",
            Str::Reject => "Reject",
            Str::Unflagged => "Unflagged",
            Str::InvalidRating => "Rating must be 0..=5",
            Str::RatingSetPattern => "Rating set to {}",
            Str::FlagSetPattern => "Flag set to {}",
            Str::VirtualCopyDuplicatedPattern => "Duplicated virtual copy as {}",

            Str::SettingsCopied => {
                "Settings copied from the active copy (paste with Cmd/Ctrl+Shift+V)"
            }
            Str::SettingsPasted => "Settings pasted onto the active copy",
            Str::ClipboardEmpty => {
                "Clipboard empty: copy settings first (Cmd/Ctrl+Shift+C)"
            }
            Str::ColorLabel => "Color Label",
            Str::ColorRed => "Red",
            Str::ColorYellow => "Yellow",
            Str::ColorGreen => "Green",
            Str::ColorBlue => "Blue",
            Str::InvalidColorLabel => "Color label must be 0..=4 (0 = none)",
            Str::ColorLabelSetPattern => "Color label set to {}",
            Str::CropModeOn => "Crop mode on (R): adjust Crop in Geometry, R toggles off",
            Str::CropModeOff => "Crop mode off",
            Str::PanelsHiddenOn => "Side panels hidden (Tab to show)",
            Str::PanelsHiddenOff => "Side panels shown",
            Str::LightsOutOn => "Lights out (L to show chrome)",
            Str::LightsOutOff => "Lights on",
            Str::ClippingOn => "Clipping warnings on (J)",
            Str::ClippingOff => "Clipping warnings off",
            Str::ClippingDetailPattern => "Clipping shadows {}% / highlights {}%",
            Str::BlackWhiteOn => "Black & white treatment on (V)",
            Str::BlackWhiteOff => "Black & white treatment off (color restored)",
            Str::SliderResetHint => "Double-click or Alt-click to reset to default",

            Str::FilterBar => "Filter (\\)",
            Str::FilterPlaceholder => "Name, rating:0-5, flag:pick/reject, label:red/yellow/green/blue/none",
            Str::FilterShown => "Library filter on (\\) — type to filter, Quick Develop below",
            Str::FilterHidden => "Library filter off",
            Str::QuickDevelop => "Quick Develop",
            Str::CompareModeCompare => "Compare",
            Str::CompareModeSurvey => "Survey",
            Str::CompareOnPattern => "Compare view on ({})",
            Str::CompareOff => "Compare view off",
            Str::SurveyOn => "Survey (N): Library grid",
            Str::SplitViewOn => "Split Before/After on (Shift+Y, full-frame Before proxy)",
            Str::SplitViewOff => "Split Before/After off",
            Str::FullscreenOn => "Fullscreen preview on (F)",
            Str::FullscreenOff => "Fullscreen preview off",
            Str::SnapshotNamePattern => "Snapshot {}",
            Str::SnapshotCreatedPattern => "Snapshot saved: {}",
            Str::InvalidSnapshotName => "Snapshot name must not be empty",
            Str::NotSnapshot => "History entry is not a snapshot",
            Str::StackGroupedPattern => "Added to stack {}",
            Str::StackUngrouped => "Removed from stack",
            Str::QuickDevelopAppliedPattern => "Quick develop applied: {}",
            Str::GotoLibraryImport => "Library (import shortcut)",
            Str::GotoExport => "Export (export shortcut)",
            Str::OverlayModeLabel => "Tool overlay",
            Str::OverlayAlways => "Always",
            Str::OverlayAuto => "Auto",
            Str::OverlayNever => "Never",
            Str::OverlayModeSetPattern => "Tool overlay: {}",
            Str::PinVisibilityLabel => "Edit pins",
            Str::PinVisibilitySetPattern => "Edit pins: {}",
            Str::SoloMode => "Solo mode (one section open)",
            Str::SoloModeOn => "Solo mode on (opening a section closes the others)",
            Str::SoloModeOff => "Solo mode off",
            Str::AllPanelsHiddenOn => "All panels hidden (Shift+Tab to show)",
            Str::AllPanelsHiddenOff => "All panels shown",
            Str::SpotOverlayHint => {
                "Overlay/pins follow the global Tool overlay + Edit pins modes (Masking section)"
            }
        }
    }
}
