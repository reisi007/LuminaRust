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
    HistogramShowOriginal,
    HistogramOriginalBadge,
    HistogramOriginalOn,
    HistogramOriginalOff,
    HistogramDeltaPattern,
    Draft,
    Filmstrip,
    LibraryThumbSize,
    LibrarySortName,
    LibrarySortDate,
    LibrarySortCustom,
    LibrarySortReorderedPattern,
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
    // G-02: per-channel tone curve (channel selector + point editor)
    ToneCurveChannel,
    ToneCurveChannelMaster,
    ToneCurveChannelRed,
    ToneCurveChannelGreen,
    ToneCurveChannelBlue,
    ToneCurvePoints,
    ToneCurveAddPoint,
    ToneCurveRemovePoint,
    ToneCurvePointInput,
    ToneCurvePointOutput,
    ToneCurveInvalidPattern,
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
    GradingBlending,
    // G-02: Point Color (F-090b)
    PointColor,
    PointColorAdd,
    PointColorRemove,
    PointColorHueCenter,
    PointColorRange,
    PointColorHueShift,
    PointColorSatShift,
    PointColorLumShift,
    PointColorFull,
    PointColorRangePattern,
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
    LensProfile,
    DistortionK1,
    DistortionK2,
    DistortionK3,
    VignetteC0,
    VignetteC1,
    VignetteC2,
    ChromaticRed,
    ChromaticBlue,
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
    // Lens Blur (G-05): depth-bokeh controls, optics-adjacent. The section
    // lives inside Optics so the 8-section F-100 order stays intact.
    LensBlur,
    LensBlurEnable,
    LensBlurAmount,
    LensBlurFocalNear,
    LensBlurFocalFar,
    LensBlurBokeh,
    LensBlurBokehRound,
    LensBlurBokehElliptical,
    LensBlurBokehHexagonal,
    LensBlurFocusRect,
    LensBlurStatusPattern,
    LensBlurHint,

    // Geometry (F-093 / F-099)
    Crop,
    Rotation,
    Straighten,
    Aspect,
    ClearCrop,
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
    LensfunAutoPattern,

    // Upright (LRPAR-G06-UPRIGHT-15)
    Upright,
    UprightAnalyze,
    UprightEnable,
    UprightClear,
    UprightStatusPattern,
    UprightFresh,
    UprightStale,
    UprightNone,
    UprightHint,

    // Red Eye (G-14 / LRPAR-G14-REDEYE-15)
    RedEye,
    RedEyeHint,
    RedEyePickMode,
    RedEyeRegion,
    RedEyeRemove,
    RedEyeClear,
    RedEyeDesaturate,
    RedEyeDarken,
    RedEyeRadius,
    RedEyeCountPattern,
    RedEyeRadiusDefault,
    RedEyeDetect,
    RedEyeApplyDetected,

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
    BrushSoftness,
    BrushFlow,
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
    /// LRPAR-G08-PREVIOUS: cross-image Previous button in the filmstrip
    /// action row. Deliberately NOT [`Str::Previous`] (the G-01 panel-local
    /// section undo) — the two actions differ (Vorbild-Übernahme vs.
    /// Sektions-Undo) and must never share a label.
    PreviousImage,
    ExposureRelative,
    ApplyPreset,
    RenderApply,
    SaveRecipe,
    NotAvailable,
    RenderStateStale,
    RenderStateCurrent,
    FilmstripHint,
    /// UX-SLICE-1 (UXG-09): "n of N" counter in the filmstrip header. Two
    /// `{}` placeholders: selected strip entries, total strip entries.
    FilmstripCounter,
    /// UX-SLICE-1 (UXG-09): honest empty-strip text (never a fake thumbnail).
    FilmstripEmpty,
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
    /// UX-SLICE-1 (P5): Library empty-state title (replaces the bare
    /// `Library` heading that duplicated the module-bar label) and the CTA
    /// hooked onto the existing folder-open path.
    LibraryEmptyTitle,
    OpenFolder,
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
    BrushRadiusInvalid,
    BrushSoftnessInvalid,
    BrushFlowInvalid,
    BrushMarkInvalid,
    BrushStrokeEmpty,
    MaskOrderInvalid,
    DuplicateSourceOnly,
    MaskResolutionMismatchPattern,
    MaskLivePlaneMissing,
    MaskLivePlaneRebuildPattern,
    IdleQueueFull,
    RecalcRequested,
    /// GUI-GEN-GRANULAR-10 (F-100): collective regeneration action and its outcomes.
    RegenerateStale,
    NothingStale,
    RegeneratedStale,
    ChangePending,
    PreviewCurrent,
    Error,
    /// KITTEST-COVERAGE-STATES-1: close button of the error popup dialog.
    ErrorDialogClose,
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
    // GEN-ONNX-1 Welle 2b: auto-fill toggle + the explicit generation action.
    AutoFillTransparent,
    GenerateCanvas,
    ExpandDragFrameHint,
    ExpandCropToImage,

    HuePattern,
    SatPattern,
    LumPattern,
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

    // F-100 Klickbarkeit (GUI-CLICK-ALL-17, User-Vorgabe 2026-09-17): every
    // keyboard-bound view/tool action also has a clickable button. The labels
    // name the action and its shortcut so the manual test finds them; the
    // tooltip repeats the shortcut via `ShortcutHint`.
    ViewToolbarCrop,
    ViewToolbarClipping,
    ViewToolbarLightsOut,
    ViewToolbarPanels,
    ViewToolbarAllPanels,
    ViewToolbarFullscreen,
    ViewToolbarSplit,
    ShortcutHint,
    DuplicateCopy,
    CopySettings,
    PasteSettings,
    SnapshotButton,
    StackGroup,
    StackUngroup,

    // G-09 Library parity (LRPAR-G09-LIB): Grid/Loupe/Compare/Survey views,
    // keyboard navigation, folder/catalog management with sidecar company.
    LibraryGridOn,
    LoupeOn,
    FolderExistsPattern,
    FolderCreatedPattern,
    FolderRenamedPattern,
    FolderDeletedPattern,
    ImageMovedPattern,
    ImageDeletedPattern,
    CompanionMoveFailedPattern,
    CompareBefore,
    CompareAfter,
    SelectionCountPattern,

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

    // G-03 masking parity (LRPAR-G03-MASK): AI-select kinds, range stages,
    // combinators, Show + color overlay, mask-list eye.
    ShowOverlay,
    OverlayColor,
    MaskEye,
    AiSelectLabel,
    AiSubject,
    AiSky,
    AiBackground,
    AiObjects,
    AiPeople,
    DetailLabel,
    AddAiMask,
    LuminanceRange,
    ColorRange,
    AddRange,
    CombineLabel,
    CombineAdd,
    CombineSubtract,
    DuplicateMask,
    OtherMask,
    MaskStatusLabel,
    // LRPAR-G03-MASKGROUP-03: group panel (Copy vs. Duplicate, collapsible
    // groups, member reorder, shared offsets, loud deletion).
    MaskGroupsLabel,
    MaskPin,
    MaskVisible,
    MaskHidden,
    RenameMask,
    MoveMaskUp,
    MoveMaskDown,
    GroupMembersLabel,
    GroupSelected,
    DuplicateGroup,
    Ungroup,
    GroupActive,
    GroupFeatherOffset,
    GroupDensityOffset,
    GroupApplyOffsets,
    GroupOffsetsApplied,
    MaskGroupedPattern,
    MaskGroupDissolved,
    MaskDeletedPattern,
    DeleteMaskButton,
    MaskGroupNotFound,
    SelectMasksToGroup,

    // G-16 power-shortcut rest (LRPAR-G16-POWER): Shift+double-click auto
    // white/black point, Alt+slider masking preview, S softproof preview.
    SoftproofOn,
    SoftproofOff,
    SoftproofToggle,
    AutoEndpointAppliedPattern,
    MaskingPreviewPattern,

    // G-15 metadata MVP (LRPAR-G15-META-MVP, Slice 3 GUI): keywords,
    // static/smart collections, batch bar (Library drawer).
    KeywordsSection,
    KeywordInputHint,
    AddKeyword,
    KeywordAddedPattern,
    KeywordRemovedPattern,
    KeywordUnchangedPattern,
    CollectionsSection,
    CollectionIdHint,
    CollectionNameHint,
    AddToCollection,
    CollectionJoinedPattern,
    CollectionUnchangedPattern,
    CollectionLeftPattern,
    InvalidCollectionAssignment,
    StaticCollections,
    AllImages,
    SmartSection,
    SmartIdHint,
    SmartNameHint,
    SmartRuleValueHint,
    PushRule,
    CombineAnd,
    CombineOr,
    CombineNot,
    ClearStack,
    RuleStackPattern,
    CreateSmart,
    SmartNeedsSingleRule,
    SmartDuplicateId,
    SmartUnknownId,
    SmartCreatedPattern,
    SmartDeletedPattern,
    CatalogPathHint,
    LoadCatalog,
    SaveCatalog,
    InvalidSmartCatalogFormat,
    InvalidSmartCatalogVersion,
    SmartCatalogLoadedPattern,
    SmartCatalogSavedPattern,
    BatchSection,
    BatchValueHint,
    BatchApply,
    BatchAppliedPattern,
    NoImagesSelected,

    // G-01 develop basis (LRPAR-G01-BASIC): treatment/profile selectors,
    // per-panel Previous/Reset, reset-sliders-automatically, original
    // reference line.
    Treatment,
    TreatmentColor,
    TreatmentBlackWhite,
    TreatmentSetPattern,
    Profile,
    ProfileSetPattern,
    InvalidTreatment,
    InvalidProfile,
    Previous,
    SectionPreviousPattern,
    SectionResetPattern,
    SectionPreviousUnavailable,
    ResetSlidersAutomatically,
    ResetSlidersOn,
    ResetSlidersOff,
    ResetSlidersDropped,
    HistogramEditedPattern,
    // LRPAR-G15-IPTC-S8: Library Metadata panel (right column).
    MetadataSection,
    MetadataDraftSection,
    MetadataFieldTitle,
    MetadataFieldHeadline,
    MetadataFieldDescription,
    MetadataFieldCopyrightNotice,
    MetadataFieldCreator,
    MetadataFieldCredit,
    MetadataFieldSource,
    MetadataFieldCity,
    MetadataFieldStateProvince,
    MetadataFieldCountry,
    MetadataFieldDateCreated,
    MetadataDateHint,
    MetadataSaveDraft,
    MetadataClearDraft,
    MetadataDraftSaved,
    MetadataDraftUnchanged,
    MetadataDraftCleared,
    MetadataEmbeddedSection,
    MetadataEmbeddedUnavailable,
    MetadataEmbeddedUnreadablePattern,
    MetadataHistoryEntryPattern,
    MetadataHistoryClear,
    MetadataHistoryClearedPattern,
    MetadataPresetSection,
    MetadataPresetApply,
    MetadataPresetChooseHint,
    MetadataPresetAppliedPattern,
    MetadataPresetUnchangedPattern,
    MetadataPresetDialog,
    MetadataPresetVarRequiredPattern,
    MetadataUnknownFieldPattern,
    MetadataEmbeddedValuePattern,
    MetadataEmbeddedKeywordsPattern,
    MetadataEmbeddedNoKeywords,
    MetadataPresetVarHint,
    MetadataPresetConfirm,
    MetadataSyncSection,
    MetadataSyncButton,
    MetadataSyncAppliedPattern,
    MetadataSyncFailedPattern,
    MetadataSyncNeedsFields,
    MetadataNoSidecarPattern,
    MetadataPresetBatchPattern,
    MetadataNoPresetDir,
    // KITTEST-COVERAGE-STATES-1: Library metadata clipboard (own copy/paste
    // system, separate from the Develop settings clipboard).
    MetadataCopyDraft,
    MetadataPasteDraft,
    MetadataCopiedPattern,
    MetadataNothingToPaste,

    // LRPAR-G14-DENOISE-IMPL-20 (GUI slice): Detail-section AI denoise
    // controls, status badge and the loud policy surfaces.
    DenoiseAi,
    DenoiseEnable,
    DenoiseStrength,
    DenoisePreserveDetail,
    DenoiseModelPattern,
    DenoiseModelHint,
    DenoiseStatusInactive,
    DenoiseStatusReady,
    DenoiseStatusUnavailable,
    DenoiseStatusStale,
    DenoiseStatusMissing,
    DenoiseStatusCorrupt,
    DenoisePolicy,
    DenoisePolicyWarn,
    DenoisePolicyStrict,
    DenoiseNotReadyWarning,
    DenoiseReasonPattern,
    DenoiseNoReason,
    DenoiseExportUnsupported,
    DenoiseExportStrictPattern,

    // LRPAR-G09-CULL-25 (GUI slice): Library assisted-culling badges, filter
    // and the explicit adopt/clear actions.
    CullingSection,
    CullingKeep,
    CullingReview,
    CullingReject,
    CullingStale,
    CullingNoProposal,
    CullingProposalPattern,
    CullingReasonsPattern,
    CullingStalePattern,
    CullingAnalyze,
    CullingClear,
    CullingFilterHint,
    CullingAdoptedPattern,
    CullingClearedPattern,
    CullingNoIdentity,

    // LRPAR-G12-FACE-20 (S5): Library People view (clusters, persons, explicit
    // naming, status warnings). No map/GPS strings exist by design.
    FacePeople,
    FaceNoAnalysis,
    FaceStatusValid,
    FaceStatusStale,
    FaceStatusMissing,
    FaceStatusCorrupt,
    FaceStatusPattern,
    FaceNoAnalysisHint,
    FaceFilterLabel,
    FaceFilterHint,
    FaceStaleWarning,
    FaceMissingWarning,
    FaceClusterPattern,
    FaceSelectCluster,
    FaceNameHint,
    FaceConfirm,
    FaceNameRequired,
    FaceConfirmDonePattern,
    FaceSplitHint,
    FaceSplit,
    FaceSplitDone,
    FaceMergeHint,
    FaceMerge,
    FaceMergeDone,
    FaceCrops,
    FaceUseAsMask,
    FaceMaskCreated,
    FaceMaskNeedsValidPattern,
    FaceDetectionNotFound,

    // LRPAR-G13-MERGE-15 (GUI slice): HDR/panorama actions, job state and the
    // visible merge-bundle status.
    MergeSection,
    MergeHdr,
    MergePano,
    MergeSelectionPattern,
    MergeStatusPattern,
    MergeBundlePattern,
    MergeStatusNone,
    MergeStatusOk,
    MergeStatusStale,
    MergeStatusMissing,
    MergeStatusUnsupported,
    MergeRunningPattern,
    MergeDonePattern,
    MergeCurrentPattern,
    MergeFailed,
    MergeAlreadyRunning,
    MergeNeedsSelection,
}
// LRPAR-G09-SORT-09: the Library strings live in `i18n_library.rs`.
use crate::i18n_library::library_text;
// LRPAR-G03-MASKGROUP-03: the masking strings live in `i18n_masking.rs`.
use crate::i18n_masking::masking_text;
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
            // UX-SLICE-1 (mapper P3): the export destination label is unique
            // ("Destination") instead of the duplicate "Export to"/"Export".
            Str::ExportTarget => "Destination",
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
            Str::HistogramShowOriginal => "Show original",
            Str::HistogramOriginalBadge => "Original — unedited decode",
            Str::HistogramOriginalOn => "Original histogram on (unedited decode)",
            Str::HistogramOriginalOff => "Original histogram off",
            Str::HistogramDeltaPattern => "Δ vs edited — mean {} L1 {}",
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
            Str::ToneCurveChannel => "Channel",
            Str::ToneCurveChannelMaster => "Master",
            Str::ToneCurveChannelRed => "Red",
            Str::ToneCurveChannelGreen => "Green",
            Str::ToneCurveChannelBlue => "Blue",
            Str::ToneCurvePoints => "Point curve",
            Str::ToneCurveAddPoint => "Add point",
            Str::ToneCurveRemovePoint => "Remove",
            Str::ToneCurvePointInput => "Input {}",
            Str::ToneCurvePointOutput => "Output {}",
            Str::ToneCurveInvalidPattern => "Tone curve: invalid points ({}) — not saved",

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
            Str::GradingBlending => "Blending",
            Str::PointColor => "Point Color",
            Str::PointColorAdd => "Add color",
            Str::PointColorRemove => "Remove",
            Str::PointColorHueCenter => "Hue center",
            Str::PointColorRange => "Range",
            Str::PointColorHueShift => "Hue shift",
            Str::PointColorSatShift => "Saturation shift",
            Str::PointColorLumShift => "Luminance shift",
            Str::PointColorFull => "Point Color: entry limit (8) reached",
            Str::PointColorRangePattern => "Point Color: {} out of range — not saved",

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
            Str::LensProfile => "Lens Profile",
            Str::DistortionK1 => "Distortion K1 (r²)",
            Str::DistortionK2 => "Distortion K2 (r⁴)",
            Str::DistortionK3 => "Distortion K3 (r⁶)",
            Str::VignetteC0 => "Vignette C0 (center)",
            Str::VignetteC1 => "Vignette C1 (mid)",
            Str::VignetteC2 => "Vignette C2 (corners)",
            Str::ChromaticRed => "CA Red (lateral)",
            Str::ChromaticBlue => "CA Blue (lateral)",
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
            Str::LensBlur => "Lens Blur",
            Str::LensBlurEnable => "Enable lens blur",
            Str::LensBlurAmount => "Blur amount",
            Str::LensBlurFocalNear => "Focal range near",
            Str::LensBlurFocalFar => "Focal range far",
            Str::LensBlurBokeh => "Bokeh shape",
            Str::LensBlurBokehRound => "Round",
            Str::LensBlurBokehElliptical => "Elliptical",
            Str::LensBlurBokehHexagonal => "Hexagonal",
            Str::LensBlurFocusRect => "Focus rectangle (x, y, w, h)",
            Str::LensBlurStatusPattern => "Status: {}",
            Str::LensBlurHint => {
                "Depth bokeh: the focus rectangle stays sharp, the focal \
                 range sets the sharp depth band, the amount sets the blur \
                 strength. Without an external depth map a deterministic \
                 focus-distance heuristic applies; a referenced but missing \
                 depth map aborts the render loudly."
            }

            Str::Crop => "Crop",
            Str::Rotation => "Rotation",
            Str::Straighten => "Straighten",
            Str::Aspect => "Aspect",
            Str::ClearCrop => "Clear Crop",
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
            Str::LensfunAutoPattern => "Lensfun auto: {}",

            Str::Upright => "Upright",
            Str::UprightAnalyze => "Analyze",
            Str::UprightEnable => "Apply analysis",
            Str::UprightClear => "Clear upright",
            Str::UprightStatusPattern => "Upright analysis: {}",
            Str::UprightFresh => "fresh",
            Str::UprightStale => "stale",
            Str::UprightNone => "none",
            Str::UprightHint => {
                "Classic, model-free line analysis. Disabling keeps the manual perspective."
            }

            Str::RedEye => "Red Eye",
            Str::RedEyeHint => "Click the preview to mark a red pupil. Regions are persisted explicitly.",
            Str::RedEyePickMode => "Mark region",
            Str::RedEyeRegion => "Region {}",
            Str::RedEyeRemove => "Remove",
            Str::RedEyeClear => "Clear all",
            Str::RedEyeDesaturate => "Desaturate",
            Str::RedEyeDarken => "Darken",
            Str::RedEyeRadius => "Radius",
            Str::RedEyeCountPattern => "{} region(s)",
            Str::RedEyeRadiusDefault => "0.05",
            Str::RedEyeDetect => "Detect pupils",
            Str::RedEyeApplyDetected => "Apply detected",

            Str::BeforeAfter => "Before / After (Y)",
            Str::WbEyedropper => "WB Eyedropper",
            Str::WbEyedropperActive => "WB Eyedropper (Esc to cancel)",
            Str::Cancel => "Cancel",
            Str::Reset => "Reset",
            Str::MatchExposure => "Match Total Exposure",
            Str::SyncSettings => "Sync Settings",
            Str::MatchSelection => "Match Total Exposures",
            Str::PreviousImage => "Previous Image",
            Str::ExposureRelative => "Exposure relative",
            Str::ApplyPreset => "Create & Apply Preset",
            Str::RenderApply => "Render / Apply",
            Str::SaveRecipe => "Save Recipe / Sidecar",
            Str::NotAvailable => "Not available",
            Str::RenderStateStale => "Stale",
            Str::RenderStateCurrent => "Render state current: {}",
            Str::FilmstripHint => "Click a thumbnail to open it",
            Str::FilmstripCounter => "{} of {}",
            Str::FilmstripEmpty => "No images in this folder",
            Str::PickWhiteBalanceHint => "Click the preview to pick white balance",
            Str::StatusConflict => "Conflict",
            Str::StatusOffline => "Offline",
            Str::StatusWithout => "No sidecar",

            Str::ReadyForImage => "Ready for a PNG, JPEG or WebP",
            Str::LibraryEmptyTitle => "No images",
            Str::OpenFolder => "Open Folder",
            Str::PresetNameEmpty => "Preset name must not be empty",
            Str::IdleQueueFull => "Idle queue is full",
            Str::RecalcRequested => "Recalculation requested; job control required",
            Str::RegenerateStale => "Regenerate Stale / Missing",
            Str::NothingStale => "Nothing stale or missing",
            Str::RegeneratedStale => "Regenerated: {}",
            Str::ChangePending => "Change pending",
            Str::PreviewCurrent => "Preview current",
            Str::Error => "Error",
            Str::ErrorDialogClose => "Close",
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
            Str::AutoFillTransparent => "Auto-fill transparent pixels (after lens)",
            Str::GenerateCanvas => "Generate",
            Str::ExpandDragFrameHint =>
                "Drag a frame: the preview shows the expand frame when active",
            Str::ExpandCropToImage => "Crop to image — no expand.",

            Str::HuePattern => "{} Hue",
            Str::SatPattern => "{} Sat",
            Str::LumPattern => "{} Lum",
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

            Str::FilterBar | Str::FilterPlaceholder | Str::FilterShown => library_text(self),
            Str::FilterHidden | Str::QuickDevelop | Str::CompareModeCompare => library_text(self),
            Str::CompareModeSurvey | Str::CompareOnPattern | Str::CompareOff => library_text(self),
            Str::SurveyOn | Str::QuickDevelopAppliedPattern => library_text(self),
            Str::StackGroupedPattern | Str::StackUngrouped => library_text(self),
            Str::StackGroup | Str::StackUngroup => library_text(self),
            Str::LibrarySortName | Str::LibrarySortDate => library_text(self),
            Str::LibrarySortCustom | Str::LibrarySortReorderedPattern => library_text(self),
            Str::SplitViewOn => "Split Before/After on (Shift+Y, full-frame Before proxy)",
            Str::SplitViewOff => "Split Before/After off",
            Str::FullscreenOn => "Fullscreen preview on (F)",
            Str::FullscreenOff => "Fullscreen preview off",
            Str::SnapshotNamePattern => "Snapshot {}",
            Str::SnapshotCreatedPattern => "Snapshot saved: {}",
            Str::InvalidSnapshotName => "Snapshot name must not be empty",
            Str::NotSnapshot => "History entry is not a snapshot",
            Str::GotoLibraryImport => "Library (import shortcut)",
            Str::GotoExport => "Export (export shortcut)",
            Str::ViewToolbarCrop => "Crop (R)",
            Str::ViewToolbarClipping => "Clipping (J)",
            Str::ViewToolbarLightsOut => "Lights Out (L)",
            Str::ViewToolbarPanels => "Panels (Tab)",
            Str::ViewToolbarAllPanels => "All Panels (Shift+Tab)",
            Str::ViewToolbarFullscreen => "Fullscreen (F)",
            Str::ViewToolbarSplit => "Split (Shift+Y)",
            Str::ShortcutHint => "Keyboard shortcut: {}",
            Str::DuplicateCopy => "Duplicate Copy",
            Str::CopySettings => "Copy Settings",
            Str::PasteSettings => "Paste Settings",
            Str::SnapshotButton => "Snapshot",
            Str::LibraryGridOn => "Library grid (G)",
            Str::LoupeOn => "Loupe (E): single image",
            Str::FolderExistsPattern => "Folder already exists: {}",
            Str::FolderCreatedPattern => "Folder created: {}",
            Str::FolderRenamedPattern => "Folder renamed: {}",
            Str::FolderDeletedPattern => "Folder deleted: {}",
            Str::ImageMovedPattern => "Image moved: {}",
            Str::ImageDeletedPattern => "Image deleted: {}",
            Str::CompanionMoveFailedPattern => "Companion move failed: {}",
            Str::CompareBefore => "Before",
            Str::CompareAfter => "After",
            Str::SelectionCountPattern => "{} selected",
            Str::NewMask | Str::SelectMask | Str::Invert | Str::OfferRecalculation => {
                masking_text(self)
            }
            Str::LocalAdjustments | Str::MaskTool | Str::MaskToolBrush => masking_text(self),
            Str::MaskToolGradient | Str::MaskToolRadial | Str::MaskToolNone => masking_text(self),
            Str::BrushSize | Str::BrushSoftness | Str::BrushFlow | Str::BrushEraser => {
                masking_text(self)
            }
            Str::DrawMaskHint | Str::Blur | Str::Density => masking_text(self),
            Str::NoSidecarLoaded | Str::VirtualCopyNotFound | Str::MaskNotFound => {
                masking_text(self)
            }
            Str::MaskNameEmpty | Str::NoImageLoaded | Str::MaskNameExists => masking_text(self),
            Str::MaskCreated | Str::MaskRenamed | Str::InvalidLocalAdjustment => {
                masking_text(self)
            }
            Str::LocalAdjustmentSaved | Str::NoMaskSelected | Str::MaskStaleRecalc => {
                masking_text(self)
            }
            Str::MaskCurrentNoRecalc | Str::ExplicitRecalcRequested => masking_text(self),
            Str::BrushRadiusInvalid | Str::BrushSoftnessInvalid | Str::BrushFlowInvalid
            | Str::BrushMarkInvalid | Str::BrushStrokeEmpty | Str::MaskOrderInvalid
            | Str::DuplicateSourceOnly | Str::MaskResolutionMismatchPattern
            | Str::MaskLivePlaneMissing | Str::MaskLivePlaneRebuildPattern => masking_text(self),
            Str::OverlayModeLabel => "Tool overlay",
            Str::OverlayAlways => "Always",
            Str::OverlayAuto => "Auto",
            Str::PinVisibilitySetPattern => "Edit pins: {}",
            Str::OverlayNever | Str::OverlayModeSetPattern | Str::PinVisibilityLabel => masking_text(self),
            Str::SpotOverlayHint | Str::ShowOverlay | Str::OverlayColor | Str::MaskEye => masking_text(self),
            Str::AiSelectLabel | Str::AiSubject | Str::AiSky | Str::AiBackground => masking_text(self),
            Str::AiObjects | Str::AiPeople | Str::DetailLabel | Str::AddAiMask => masking_text(self),
            Str::LuminanceRange | Str::ColorRange | Str::AddRange | Str::CombineLabel => masking_text(self),
            Str::CombineAdd | Str::CombineSubtract | Str::DuplicateMask => masking_text(self),
            Str::OtherMask | Str::MaskStatusLabel | Str::SoftproofOn | Str::SoftproofOff => masking_text(self),
            Str::SoftproofToggle | Str::AutoEndpointAppliedPattern => masking_text(self),
            Str::MaskingPreviewPattern | Str::SoloMode | Str::SoloModeOn => masking_text(self),
            Str::SoloModeOff | Str::AllPanelsHiddenOn | Str::AllPanelsHiddenOff => masking_text(self),
            Str::MaskGroupsLabel | Str::MaskPin | Str::MaskVisible | Str::MaskHidden => masking_text(self),
            Str::RenameMask | Str::MoveMaskUp | Str::MoveMaskDown => masking_text(self),
            Str::GroupMembersLabel | Str::GroupSelected => masking_text(self),
            Str::DuplicateGroup | Str::Ungroup | Str::GroupActive => masking_text(self),
            Str::GroupFeatherOffset | Str::GroupDensityOffset | Str::GroupApplyOffsets => masking_text(self),
            Str::GroupOffsetsApplied | Str::MaskGroupedPattern | Str::MaskGroupDissolved => masking_text(self),
            Str::MaskDeletedPattern | Str::DeleteMaskButton => masking_text(self),
            Str::MaskGroupNotFound | Str::SelectMasksToGroup => masking_text(self),
            Str::KeywordsSection => "Keywords",
            Str::KeywordInputHint => "New keyword…",
            Str::AddKeyword => "Add keyword",
            Str::KeywordAddedPattern => "Keyword added: {}",
            Str::KeywordRemovedPattern => "Keyword removed: {}",
            Str::KeywordUnchangedPattern => "Keyword unchanged: {}",
            Str::CollectionsSection => "Collections",
            Str::CollectionIdHint => "id (or id=name)",
            Str::CollectionNameHint => "Display name",
            Str::AddToCollection => "Add to collection",
            Str::CollectionJoinedPattern => "Added to collection: {}",
            Str::CollectionUnchangedPattern => "Collection unchanged: {}",
            Str::CollectionLeftPattern => "Removed from collection: {}",
            Str::InvalidCollectionAssignment => "invalid collection assignment `{}`: expected `id=name`",
            Str::StaticCollections => "Static collections",
            Str::AllImages => "All images",
            Str::SmartSection => "Smart collections",
            Str::SmartIdHint => "smart id",
            Str::SmartNameHint => "Display name",
            Str::SmartRuleValueHint => "rule value",
            Str::PushRule => "Push rule",
            Str::CombineAnd => "And",
            Str::CombineOr => "Or",
            Str::CombineNot => "Not",
            Str::ClearStack => "Clear",
            Str::RuleStackPattern => "{} rule(s) on the stack",
            Str::CreateSmart => "Create smart collection",
            Str::SmartNeedsSingleRule => "smart collection needs exactly one combined rule on the stack",
            Str::SmartDuplicateId => "smart collection id already exists: {}",
            Str::SmartUnknownId => "unknown smart collection: {}",
            Str::SmartCreatedPattern => "Smart collection created: {}",
            Str::SmartDeletedPattern => "Smart collection deleted: {}",
            Str::CatalogPathHint => "catalog path…",
            Str::LoadCatalog => "Load catalog",
            Str::SaveCatalog => "Save catalog",
            Str::InvalidSmartCatalogFormat => "invalid smart catalog format: {}",
            Str::InvalidSmartCatalogVersion => "unsupported smart catalog version: {}",
            Str::SmartCatalogLoadedPattern => "Smart catalog loaded: {} collection(s)",
            Str::SmartCatalogSavedPattern => "Smart catalog saved: {} collection(s)",
            Str::BatchSection => "Batch (selection)",
            Str::BatchValueHint => "value",
            Str::BatchApply => "Apply to selection",
            Str::BatchAppliedPattern => "Batch applied to {} image(s)",
            Str::NoImagesSelected => "No images selected",
            Str::Treatment => "Treatment",
            Str::TreatmentColor => "Color",
            Str::TreatmentBlackWhite => "Black & White",
            Str::TreatmentSetPattern => "Treatment: {}",
            Str::Profile => "Profile",
            Str::ProfileSetPattern => "Profile: {}",
            Str::InvalidTreatment => "Unknown treatment (expected Color or Black & White)",
            Str::InvalidProfile => "Unknown profile",
            Str::Previous => "Previous",
            Str::SectionPreviousPattern => "Previous restored ({})",
            Str::SectionResetPattern => "{} reset",
            Str::SectionPreviousUnavailable => "No saved state for this section yet",
            Str::ResetSlidersAutomatically => "Reset sliders automatically",
            Str::ResetSlidersOn => "Reset sliders automatically on",
            Str::ResetSlidersOff => "Reset sliders automatically off",
            Str::ResetSlidersDropped => "Pending edit discarded (reset sliders automatically)",
            Str::HistogramEditedPattern => "Edited mean {} (Δ {}, L1 {})",
            Str::MetadataSection => "Metadata",
            Str::MetadataDraftSection => "Metadata draft",
            Str::MetadataFieldTitle => "Title",
            Str::MetadataFieldHeadline => "Headline",
            Str::MetadataFieldDescription => "Description",
            Str::MetadataFieldCopyrightNotice => "Copyright",
            Str::MetadataFieldCreator => "Creator",
            Str::MetadataFieldCredit => "Credit",
            Str::MetadataFieldSource => "Source",
            Str::MetadataFieldCity => "City",
            Str::MetadataFieldStateProvince => "State / Province",
            Str::MetadataFieldCountry => "Country",
            Str::MetadataFieldDateCreated => "Date created",
            Str::MetadataDateHint => "YYYY-MM-DD",
            Str::MetadataSaveDraft => "Save draft",
            Str::MetadataClearDraft => "Clear draft",
            Str::MetadataDraftSaved => "Draft saved",
            Str::MetadataDraftUnchanged => "Draft unchanged",
            Str::MetadataDraftCleared => "Draft cleared",
            Str::MetadataEmbeddedSection => "Embedded (read-only)",
            Str::MetadataEmbeddedUnavailable => {
                "Embedded: unavailable (not JPEG / no IIM/XMP)"
            }
            Str::MetadataEmbeddedUnreadablePattern => "Embedded IPTC unreadable: {}",
            Str::MetadataHistoryEntryPattern => "rev {} | {} | {} | {}",
            Str::MetadataHistoryClear => "Clear history",
            Str::MetadataHistoryClearedPattern => "History cleared ({} entries removed)",
            Str::MetadataPresetSection => "Meta preset",
            Str::MetadataPresetApply => "Apply preset",
            Str::MetadataPresetChooseHint => "Choose a preset…",
            Str::MetadataPresetAppliedPattern => "Meta preset applied: {}",
            Str::MetadataPresetUnchangedPattern => "Meta preset unchanged: {}",
            Str::MetadataPresetDialog => "Preset variables (all required)",
            Str::MetadataPresetVarRequiredPattern => "Value required for {}",
            Str::MetadataUnknownFieldPattern => "Unknown metadata field: {}",
            Str::MetadataEmbeddedValuePattern => "Embedded: {}",
            Str::MetadataEmbeddedKeywordsPattern => "Embedded keywords: {}",
            Str::MetadataEmbeddedNoKeywords => "Embedded keywords: none",
            Str::MetadataPresetVarHint => "Value…",
            Str::MetadataPresetConfirm => "Apply",
            Str::MetadataSyncSection => "Sync to selection",
            Str::MetadataSyncButton => "Sync to selection",
            Str::MetadataSyncAppliedPattern => "Metadata sync applied to {} image(s)",
            Str::MetadataSyncFailedPattern => "Metadata sync failed for {} image(s): {}",
            Str::MetadataSyncNeedsFields => {
                "Metadata sync needs at least one field (refusing a silent transfer-all)"
            }
            Str::MetadataNoSidecarPattern => "No sidecar for {} (open the image first)",
            Str::MetadataPresetBatchPattern => "Meta preset {} applied to {} image(s)",
            Str::MetadataNoPresetDir => {
                "Meta presets unavailable: the config directory could not be determined"
            }
            Str::MetadataCopyDraft => "Copy metadata",
            Str::MetadataPasteDraft => "Paste metadata",
            Str::MetadataCopiedPattern => "Metadata copied ({} field(s))",
            Str::MetadataNothingToPaste => {
                "Nothing to paste: copy metadata from an image first"
            }
            Str::DenoiseAi => "AI Denoise",
            Str::DenoiseEnable => "Enable AI Denoise",
            Str::DenoiseStrength => "AI Denoise Strength",
            Str::DenoisePreserveDetail => "Preserve Detail",
            Str::DenoiseModelPattern => "Model: {}",
            Str::DenoiseModelHint => {
                "Model identity is part of the persisted recipe; changing it invalidates the artifact."
            }
            Str::DenoiseStatusInactive => "AI Denoise Inactive",
            Str::DenoiseStatusReady => "AI Denoise Ready",
            Str::DenoiseStatusUnavailable => "AI Denoise Unavailable",
            Str::DenoiseStatusStale => "AI Denoise Stale",
            Str::DenoiseStatusMissing => "AI Denoise Missing",
            Str::DenoiseStatusCorrupt => "AI Denoise Corrupt",
            Str::DenoisePolicy => "On missing artifact",
            Str::DenoisePolicyWarn => "Warn and use manual NR",
            Str::DenoisePolicyStrict => "Stop the render",
            Str::DenoiseNotReadyWarning => {
                "AI Denoise has no usable artifact — manual noise reduction applies (never silent)."
            }
            Str::DenoiseReasonPattern => "Status detail: {}",
            Str::DenoiseNoReason => "no diagnostic available",
            Str::DenoiseExportUnsupported => {
                "Export refused: a verified AI Denoise artifact cannot be applied by the shared export path yet."
            }
            Str::DenoiseExportStrictPattern => {
                "Export refused: AI Denoise is {} and the policy is stop-on-missing."
            }
            Str::CullingSection => "Assisted Culling",
            Str::CullingKeep => "Keep",
            Str::CullingReview => "Review",
            Str::CullingReject => "Reject",
            Str::CullingStale => "Stale",
            Str::CullingNoProposal => "No proposal",
            Str::CullingProposalPattern => "Proposal: {}",
            Str::CullingReasonsPattern => "Reasons: {}",
            Str::CullingStalePattern => "Outdated/unusable: {}",
            Str::CullingAnalyze => "Analyze & adopt",
            Str::CullingClear => "Clear proposal",
            Str::CullingFilterHint => "Filter: cull:keep|review|reject|none|stale",
            Str::CullingAdoptedPattern => "Culling proposal recorded for {}",
            Str::CullingClearedPattern => "Culling proposal cleared for {}",
            Str::CullingNoIdentity => "Culling analysis needs a loaded image and a sidecar",
            Str::FacePeople => "People",
            Str::FaceNoAnalysis => "No face analysis",
            Str::FaceStatusValid => "Valid",
            Str::FaceStatusStale => "Stale",
            Str::FaceStatusMissing => "Missing",
            Str::FaceStatusCorrupt => "Corrupt",
            Str::FaceStatusPattern => "Face analysis: {}",
            Str::FaceNoAnalysisHint => "No face analysis for this image",
            Str::FaceFilterLabel => "Person filter",
            Str::FaceFilterHint => "Filter clusters by person name",
            Str::FaceStaleWarning => {
                "This face analysis is outdated (source/decode/model context changed) — re-run it explicitly."
            }
            Str::FaceMissingWarning => {
                "This face analysis references a missing artifact — no automatic re-run."
            }
            Str::FaceClusterPattern => "{} ",
            Str::FaceSelectCluster => "Select a cluster to name, split or merge it",
            Str::FaceNameHint => "Person name",
            Str::FaceConfirm => "Confirm person",
            Str::FaceNameRequired => "A person name is required (no automatic naming)",
            Str::FaceConfirmDonePattern => "Person {} confirmed",
            Str::FaceSplitHint => "Detection ids to split (comma separated)",
            Str::FaceSplit => "Split cluster",
            Str::FaceSplitDone => "Cluster split",
            Str::FaceMergeHint => "Other cluster id to merge into the selected one",
            Str::FaceMerge => "Merge clusters",
            Str::FaceMergeDone => "Clusters merged",
            Str::FaceCrops => "Faces in cluster",
            Str::FaceUseAsMask => "Use face as mask",
            Str::FaceMaskCreated => "Face mask source created",
            Str::FaceMaskNeedsValidPattern => {
                "A face mask source needs a valid face analysis (status: {}) — re-run it explicitly"
            }
            Str::FaceDetectionNotFound => {
                "The selected face detection does not exist in this analysis"
            }
            Str::MergeSection => "HDR / Panorama Merge",
            Str::MergeHdr => "Merge HDR (Cmd/Ctrl+H)",
            Str::MergePano => "Merge Panorama (Cmd/Ctrl+M)",
            Str::MergeSelectionPattern => "{} source(s) selected",
            Str::MergeStatusPattern => "Merge: {}",
            Str::MergeBundlePattern => "Merge bundle: {}",
            Str::MergeStatusNone => "no merge bundle",
            Str::MergeStatusOk => "ok",
            Str::MergeStatusStale => "stale",
            Str::MergeStatusMissing => "missing",
            Str::MergeStatusUnsupported => "unsupported",
            Str::MergeRunningPattern => "{} running…",
            Str::MergeDonePattern => "merged {}",
            Str::MergeCurrentPattern => "merge already current: {}",
            Str::MergeFailed => "merge failed (see the log for the loud reason)",
            Str::MergeAlreadyRunning => "A merge job is already running",
            Str::MergeNeedsSelection => {
                "Select at least {} images in the filmstrip to merge"
            }
        }
    }
}
