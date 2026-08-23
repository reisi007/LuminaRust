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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    NewCopy,
    Source,
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
    Navigator,
    Preview,
    Zoom,
    ZoomFit,
    ZoomOneToOne,
    ZoomTwoHundred,
    ZoomFitWidth,
    Histogram,
    Filmstrip,

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

    // Geometry (F-093 / F-099)
    Crop,
    Rotation,
    MirrorHorizontal,
    MirrorVertical,
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
    ExposureRelative,
    ApplyPreset,
    RenderApply,
    SaveRecipe,
    Preset,
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

    // Local-adjustment validation & status literals that were previously free-form
    // German strings. UI language is English (F-100); routed through these keys so
    // the panel code carries no literal text.
    RelativeExposureRequiresAutoTone,
    FeatheringMustBeBetween,
    Loaded,

    // Parameterized patterns (use with `format!`); the `{}` placeholder is
    // replaced positionally by the caller.
    ImagesInDirectory,
    DirectoryNotReadable,
    UnknownAdjustment,
    MaskSelected,
    MaskPromptSaved,
    MaskUnavailableLayer,
    InferenceWaiting,

    // Legacy parameterized patterns (use with `format!`)
    HuePattern,
    SatPattern,
    UnsetPattern,
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
            Str::NewCopy => "Duplicate copy",
            Str::Source => "Source",
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
            Str::Navigator => "Navigator",
            Str::Preview => "Preview",
            Str::Zoom => "Zoom",
            Str::ZoomFit => "Fit",
            Str::ZoomOneToOne => "1:1",
            Str::ZoomTwoHundred => "200%",
            Str::ZoomFitWidth => "Fit Width",
            Str::Histogram => "Histogram",
            Str::Filmstrip => "Filmstrip",

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
            Str::DistortionK1 => "Distortion k1",
            Str::DistortionK2 => "Distortion k2",
            Str::DistortionK3 => "Distortion k3",
            Str::VignetteC0 => "Vignette c0",
            Str::VignetteC1 => "Vignette c1",
            Str::VignetteC2 => "Vignette c2",
            Str::ChromaticRed => "CA Red",
            Str::ChromaticBlue => "CA Blue",
            Str::OpticsRequiresLensfun => {
                "Not available: the native Lensfun pipeline stage is disabled in this build."
            }

            Str::Crop => "Crop",
            Str::Rotation => "Rotation",
            Str::MirrorHorizontal => "Mirror Horizontal",
            Str::MirrorVertical => "Mirror Vertical",
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
            Str::ExposureRelative => "Exposure relative",
            Str::ApplyPreset => "Create & Apply Preset",
            Str::RenderApply => "Render / Apply",
            Str::SaveRecipe => "Save Recipe / Sidecar",
            Str::Preset => "Preset",
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

            Str::RelativeExposureRequiresAutoTone => {
                "Relative Exposure requires active Auto-Tone"
            }
            Str::FeatheringMustBeBetween => "Feathering must be between 0 and 1",
            Str::Loaded => "Loaded: {}",

            Str::ImagesInDirectory => "{} images in directory",
            Str::DirectoryNotReadable => "Directory not readable: {}",
            Str::UnknownAdjustment => "Unknown adjustment: {}",
            Str::MaskSelected => "Mask selected: {}",
            Str::MaskPromptSaved => "Mask prompt saved: {}",
            Str::MaskUnavailableLayer => {
                "Warning: mask unavailable (layer {}); it is not applied in the preview"
            }
            Str::InferenceWaiting => "Mask {}: background job waiting for inference engine",

            Str::HuePattern => "{} Hue",
            Str::SatPattern => "{} Sat",
            Str::UnsetPattern => "{} (unset)",
        }
    }
}
