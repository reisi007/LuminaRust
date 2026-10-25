// FILE-SIZE-1000-ALL-EXCEPTIONS-100: this file is *included* into the crate
// root (`include!("app_struct.rs")` in `lib.rs`), so `LuminaApp` is still
// defined in module `crate`: every field stays private and remains readable by
// the child modules that carry the split `impl LuminaApp` blocks. A
// `mod app_struct;` would move the type into a child module and force
// `pub(crate)` onto all fields — a wider internal API for zero behavioural
// gain. The struct below is byte-identical to its `lib.rs` text at HEAD.

pub struct LuminaApp {
    original: Option<ImageFrame>,
    preview: Option<ImageFrame>,
    /// Post-global/post-geometry frame captured by the core before local mask
    /// recipes. It is the only valid source for the local WB picker.
    effective_source_stage: Option<ImageFrame>,
    /// Render identity that produced `effective_source_stage`; a mismatch is
    /// reported as stale instead of silently sampling an older stage.
    effective_source_stage_digest: Option<String>,
    source_bytes: Option<Vec<u8>>,
    source_is_raw: bool,
    raw_orientation: u8,
    camera_white_balance: Option<[f32; 4]>,
    /// EXIF lens identity of the loaded source (G-06): snapshot from the
    /// decoder metadata at load time (`None` for raster sources / missing
    /// EXIF). Drives the Lensfun auto-profile resolution (status line +
    /// render corrector). Reset on every source change (REVIEW-GUI-N3).
    loaded_lens_identity: Option<LensIdentity>,
    /// Cached Lensfun auto-corrector for the loaded source (G-06, only
    /// with the `lensfun` feature): rebuilt when the identity/dimensions
    /// key changes, reused across preview/export renders so the system DB
    /// is not re-loaded per slider tick. `None` = no profile applies
    /// (manual model) or no EXIF. Never crosses threads (preview,
    /// navigator and export render on the UI thread; thumbnails render
    /// without auto-lens by design — documented in the G-06 status).
    #[cfg(feature = "lensfun")]
    lensfun_cache: Option<CachedLensCorrector>,
    source_name: String,
    path: String,
    directory: String,
    entries: Vec<FileBrowserEntry>,
    recipe: EditRecipe,
    /// GEN-ONNX-1 Welle 2b: the generative canvas artifacts the render hook
    /// feeds into the shared pipeline for the current source/copy. Populated by
    /// the explicit `Generative Expand → Generieren` action (fixture model
    /// pre-integration) and/or by loading the persisted `generative_canvas`
    /// record for the current identity. Every active role without a matching
    /// artifact is a loud render error — never a silent "as if not generated".
    generative_artifacts: GenerativeArtifacts,
    /// GEN-ONNX-1 Welle 2b: per-role status (`valid`/`stale`/`missing`/
    /// `corrupt`) recorded by the resolver for the current source/recipe, so the
    /// Generative panel can name the precise cause. Indexed by
    /// [`generative_role_index`].
    generative_role_status: [GenerativeRoleStatus; 2],
    /// Memo key (resolved source hash + recipe digest) for which
    /// [`Self::generative_artifacts`] were last resolved. Recomputing the
    /// generative input frames is a full pipeline head, so an unchanged
    /// source/recipe reuses the resolved canvases without re-hashing the frames.
    /// A stale key never serves a canvas: the per-role identity digest is still
    /// compared on every resolve.
    generative_memo: Option<String>,
    texture: Option<egui::TextureHandle>,
    /// R2-GUIMOD-02: identity of the pixels currently held by
    /// [`Self::texture`] — `(preview generation, before_after, [w, h])`.
    /// The CPU present path re-uploads only when this differs from what would
    /// be displayed, instead of rebuilding a full-screen [`egui::ColorImage`]
    /// and re-creating the texture on every repaint (mousemoves over panels
    /// used to pay a full-frame memcpy + upload per frame).
    texture_identity: Option<(u64, bool, [usize; 2])>,
    /// KITTEST-COVERAGE-OVERLAYS-1: retained texture for the CPU mask-overlay
    /// matte painted by [`Self::draw_mask_overlay`]. Previously the matte was
    /// uploaded with a per-frame `load_texture` into a local handle that was
    /// dropped at the end of the same frame; egui then queued the texture `set`
    /// and its `free` in one `TexturesDelta`, the backend processed `set` before
    /// `free`, and the paint sampled an already-freed texture → the overlay was
    /// invisible. Keeping the handle alive (exactly like [`Self::texture`] and
    /// the navigator/thumbnail textures) means no per-frame alloc/free and the
    /// matte actually reaches the screen.
    mask_overlay_texture: Option<egui::TextureHandle>,
    /// GUI-NAV-RECT-1: overview texture of the FULL source for the navigator
    /// viewport (never the ROI-cropped preview texture) + its cache key
    /// `(path, full_w, full_h)`. Rebuilt only on source change.
    navigator_texture: Option<egui::TextureHandle>,
    navigator_texture_key: Option<(String, u32, u32)>,
    /// GUI-NAV-RECT-1: cached downscaled full-frame overview render (current
    /// recipe) for the zoomed navigator + its key
    /// `(path, full_w, full_h, recipe_digest)`. Recomputed only when source
    /// or recipe change; at Fit the preview texture is reused instead.
    navigator_overview: Option<ImageFrame>,
    navigator_overview_key: Option<(String, u32, u32, String)>,
    /// R2-GUIMOD-02: bumped whenever `self.preview` receives new content.
    /// Part of [`Self::texture_identity`]; together with the Before/After flag
    /// and pixel size it gates the CPU texture upload to actual content
    /// changes. A monotonic counter is deliberate: draft and full renders of
    /// the same source can produce identical render keys while their pixels
    /// differ (masks are skipped for drafts), so key-based identity would be
    /// unsound.
    ///
    /// INVARIANT: every site that assigns `self.preview` MUST bump this
    /// counter — otherwise the CPU present path keeps serving the previous
    /// upload (`texture_identity` still matches). The only production
    /// assignment today is `render_from`; a new source clears `preview`
    /// implicitly by resetting render state before the next render bumps the
    /// generation again.
    preview_generation: u64,
    status: String,
    error: Option<String>,
    /// KITTEST-COVERAGE-STATES-1: whether the current `error` should be shown
    /// as the popup dialog (explicit user-action failure) or only as the
    /// header banner (background decode/listing failure). Reset by
    /// [`LuminaApp::show_error_banner`], set by [`LuminaApp::show_error`].
    error_dialog: bool,
    /// Crash-Fix Runde 2 (F5): message of the last draft-render failure that
    /// was surfaced. A repeated identical failure in a following draft tick is
    /// downgraded to one `warn!` instead of re-emitting `error!` and re-arming
    /// the dialog every frame (45 MP drag ticks made that a log/abort storm).
    /// Cleared once a draft render succeeds ([`LuminaApp::clear_draft_error_dedup`]).
    draft_error_dedup: Option<String>,
    /// Crash-Fix Runde 2 (F5): whether the one-time repeat `warn!` for the
    /// current [`Self::draft_error_dedup`] message was already emitted. Keeps a
    /// persistent failure from spamming `warn!` once per frame, too.
    draft_error_repeat_warned: bool,
    render_key: Option<RenderKey>,
    tone_analysis: Option<lumina_core::ToneAnalysis>,
    /// 256-bin luminance histogram of the full-frame render (GUI-HISTOGRAM-FULL-1,
    /// F-100): always computed from the un-cropped full frame — never from the
    /// ROI-cropped viewport texture — stored together with
    /// [`Self::tone_analysis`] from the single shared
    /// `analyze_tone_with_histogram` pass. Feeds the filled Painter curve;
    /// `None` until the first render (panel shows `NotCurrent`).
    preview_histogram: Option<LuminanceHistogram>,
    /// Pending slider commit awaiting the debounced full render
    /// (GUI-SLIDER-SAVE-1): `(recipe_key, value)` recorded by
    /// [`Self::set_adjustment`] / [`Self::set_presence`] /
    /// [`Self::reset_single_adjustment`]. Consumed by
    /// [`Self::commit_pending_slider_save`], which renders, saves the sidecar
    /// and logs `<key>=<value> saved`. Zoom/pan state is deliberately never
    /// recorded here — it stays GUI session state, never recipe.
    pending_slider_commit: Option<(String, f64)>,
    /// Pending geometry history step (G-06, LRPAR-G06-GEO): armed by the
    /// geometry setters (`set_geometry_*`, `set_crop_*`, `set_straighten`,
    /// `set_perspective_value`, `set_lens_correction_value`,
    /// `set_lens_profile`) alongside the slider commit. Consumed by
    /// [`Self::save_sidecar`], which appends exactly one history entry
    /// (`geometry-<n>`, final recipe) per saved commit — slider drags
    /// coalesce to one step per debounce, discrete actions to one step
    /// each. `None` outside geometry edits: all other sliders keep the
    /// established no-history-commit behaviour.
    pending_history_step: Option<String>,
    /// MASK-LOCAL-P0: complete mask-layer snapshot from before the pending
    /// local edit. It is consumed when the debounced sidecar save writes the
    /// corresponding history step, so Previous/History cannot lose local
    /// values while the recipe snapshot remains backward-compatible.
    pending_mask_state_before: Option<Vec<MaskLayer>>,
    /// Effective mask layers of the last [`Self::render`] (F-041): the
    /// measurement domain of `Match Total Exposure` is the rendered preview
    /// weighted by these planes. Empty whenever the render produced no layers.
    render_mask_layers: Vec<MaskLayerResult>,
    document: Option<SidecarDocument>,
    virtual_copy_id: String,
    selected_mask_id: Option<String>,
    mask_name_input: String,
    /// R5-BRUSH-24 rename field; kept in sync by `select_mask` and persisted
    /// only when the user presses the real Rename button.
    mask_rename_input: String,
    /// Per-row rename drafts so every mask row owns a real editable control;
    /// entries are session UI state and are synchronized on copy/source load.
    mask_rename_inputs: BTreeMap<String, String>,
    mask_tool: MaskTool,
    /// Normalized brush radius (0..=1 relative to the shorter source axis).
    brush_radius: f32,
    /// R5-BRUSH-24 edge softness, snapshotted into every persisted brush mark.
    brush_softness: f32,
    /// R5-BRUSH-24 per-dab flow, snapshotted into every persisted brush mark.
    brush_flow: f32,
    /// When true, brush marks use the negative (eraser) sign.
    brush_eraser: bool,
    /// Marks accumulated during an in-progress brush drag (cleared on release).
    pending_brush_marks: Vec<BrushMark>,
    /// Drag start/current normalized points for gradient/radial gestures.
    drag_start: Option<Point2>,
    drag_current: Option<Point2>,
    /// True while a mask-tool drag is in progress (drives the live overlay).
    drawing: bool,
    spot_tool: SpotTool,
    spot_mode: SpotMode,
    spot_radius: f32,
    spot_feather: f32,
    spot_opacity: f32,
    /// R5-DUST-23-FOLLOWUP: selected spot removal (display-only session
    /// state like `selected_mask_id`, never recipe/sidecar).
    selected_spot_id: Option<String>,
    /// LRPAR-G04-REMOVE session/panel state (display inputs, never recipe —
    /// except where noted): `spot_detect_threshold` is the input for heuristic
    /// Detect-Objects (`0..=1`); `spot_detect_status` holds the last detection
    /// outcome text (candidates listed, never silently applied);
    /// `spot_gen_prompt`/`spot_gen_seed`/`spot_gen_variant` are the inputs for
    /// generative variant regeneration (persisted per spot on Regenerate,
    /// targeting the selected removal); `spot_gen_status` holds the last
    /// regeneration outcome text. The visualize threshold itself is recipe-backed (see
    /// [`Self::set_spot_visualize`]); the distraction switches are
    /// recipe-backed too (see [`Self::set_spot_distraction`]).
    spot_detect_threshold: f32,
    spot_detect_status: String,
    spot_gen_prompt: String,
    spot_gen_seed: u64,
    spot_gen_variant: u64,
    spot_gen_status: String,
    preset_name: String,
    preset_fields: BTreeMap<String, bool>,
    preset_relative_exposure: bool,
    /// F-009: user-global presets directory; `None` means the platform config
    /// base could not be determined and file presets are shown as unavailable
    /// (no silent fallback directory).
    presets_dir: Option<std::path::PathBuf>,
    /// F-009: current snapshot of the presets directory. Failed files stay in
    /// the list with their error text instead of being skipped silently.
    preset_entries: Vec<presets::PresetEntry>,
    idle_queue: IdleQueue,
    /// PERF-FILMSTRIP: dedicated background thread pool for filmstrip
    /// thumbnails. `thumbnail_tx` enqueues jobs (unbounded mpsc, no capacity
    /// gate); `thumbnail_rx` delivers rendered frames to be textured on the
    /// main thread.
    thumbnail_tx: mpsc::Sender<ThumbnailJob>,
    thumbnail_rx: mpsc::Receiver<ThumbnailResult>,
    /// R2-MODSWITCH-1 F7: metadata-only per-folder preview index the thumbnail
    /// scheduler probes (memoized settings + `read_dir`, no per-cell disk read).
    thumbnail_cache: thumb_cache::PreviewIndexCache,
    /// Active top-level module (Library / Develop / Export).
    active_module: Module,
    /// R2-MODSWITCH-1 F7: module the render scheduler last ran for. The first
    /// run after a change defers a due full render past the switch frame.
    last_scheduled_module: Option<Module>,
    /// R3-OPEN-1: display path of the background decode currently in flight
    /// (`None` when idle). Set by `begin_load_path`, cleared by the decode
    /// drain; lets the Develop-switch open reuse an already-started decode
    /// instead of starting a duplicate one.
    pending_load_path: Option<String>,
    /// R3-WARMUP-1: one-shot cold-start warmup state (scheduling only).
    warmup: warmup::WarmupState,
    /// R3-LOG-1: cross-frame timing anchors (module switch event, in-flight
    /// decode). Measurement only — never read for logic.
    timing: timing::TimingState,
    /// Export module UI state (F-103-N5). The target path is chosen via a
    /// native save dialog; the format/quality drive the shared export path.
    export_path: String,
    export_format: ImageFileFormat,
    export_quality: u8,
    /// Before/After toggle state. Never mutates the recipe.
    before_after: bool,
    /// Welle 2 (LR-09): session-only copy/paste-settings clipboard. Holds the
    /// recipe snapshot taken by `Cmd/Ctrl+Shift+C`; `None` until the first
    /// copy. Never persisted (Lightroom behaviour) — paste applies it to the
    /// active copy through the normal save/render path. Native-only: clipboard
    /// and sidecar persistence are file-system capabilities.
    settings_clipboard: Option<EditRecipe>,
    /// LRPAR-G08-PREVIOUS: cross-image Previous reference (the image edited
    /// immediately before the current one). Captured on every successful
    /// image switch in [`Self::finish_decode`]; applied by
    /// [`Self::apply_previous_to_selection`]. Session-only, never persisted
    /// (like `settings_clipboard` above).
    previous_reference: Option<PreviousReference>,
    /// UX-LOOK-HISTORY-18: session-only clock override for deterministic
    /// history timestamps in headless/kittest tests. Never persisted; `None`
    /// uses the real UTC clock.
    history_timestamp_override: Option<String>,
    /// Welle 2 display-only view flags (`J` clipping overlay, `L` lights-out,
    /// `Tab` panel hide, `R` crop mode). None of them mutates the recipe; the
    /// B&W `V` treatment is recipe-backed instead (see `toggle_black_white`).
    clipping_overlay: bool,
    lights_out: bool,
    panels_hidden: bool,
    crop_mode: bool,
    /// G-11 (LRPAR-G11-OVERLAYS) session-only display state. Never persisted
    /// to the sidecar and never part of the recipe (like `Tab`/`L`/`F` above):
    /// * `all_panels_hidden`: `Shift+Tab` hides side panels, navigator rail
    ///   AND filmstrip (header/module bar + preview stay).
    /// * `overlay_mode`: global tool-overlay mode (mask matte tint).
    /// * `pin_visibility`: global edit-pin visibility (mask anchors + spots).
    /// * `solo_mode` + `section_open`: solo collapses the 8 Develop sections
    ///   to a single open one; `section_open` is the explicit open state so
    ///   solo stays headless-testable (no egui-implicit collapsing memory).
    all_panels_hidden: bool,
    overlay_mode: OverlayMode,
    pin_visibility: PinVisibility,
    /// R5-MASKVIS-25: pins-only versus the full selected-mask matte. This is
    /// session display state and is intentionally separate from the legacy
    /// global G-11 `OverlayMode` gate.
    mask_overlay_mode: MaskOverlayMode,
    solo_mode: bool,
    section_open: [bool; SECTION_COUNT],
    /// G-03 (LRPAR-G03-MASK) mask-overlay + panel session state. Display-only
    /// (never recipe/sidecar, like G-11 above) except `MaskLayer.visible`,
    /// which is persisted per virtual copy through
    /// [`Self::set_mask_visible`]:
    /// * `show_mask_overlay`: master Show switch ANDed with `overlay_mode`.
    /// * `overlay_color`: matte tint RGB (default Lightroom-red).
    /// * the remaining fields are panel inputs for AI-select / range adds and
    ///   combine/duplicate (kind, part detail, range parameters, other-mask
    ///   reference, duplicate name).
    show_mask_overlay: bool,
    overlay_color: [u8; 3],
    ai_select_kind: AiSelectKind,
    ai_detail_input: String,
    ai_name_input: String,
    lum_name_input: String,
    lum_min: f32,
    lum_max: f32,
    lum_feather: f32,
    col_name_input: String,
    col_hue_center: f32,
    col_hue_width: f32,
    col_sat_min: f32,
    col_sat_max: f32,
    col_lum_min: f32,
    col_lum_max: f32,
    col_feather: f32,
    combine_other_id: String,
    combine_name_input: String,
    duplicate_name_input: String,
    /// LRPAR-G03-MASKGROUP-03 session state: the group marked selected in the
    /// panel, the pending group name, the masks marked for late grouping and the
    /// shared feather/density offsets applied to the selected group's members.
    /// Display-only; groups themselves persist in the sidecar.
    selected_group_id: Option<String>,
    group_name_input: String,
    group_member_selection: BTreeSet<String>,
    group_feather_offset: f32,
    group_density_offset: f32,
    /// G-02 (LRPAR-G02-COLOR) tone-curve panel session state. Display-only
    /// (never recipe/sidecar): the selected curve channel (`0` master,
    /// `1..=3` red/green/blue). UX-LOOK-TONECURVE-18 edits the curve through
    /// the interactive graph, so no add-point input buffers are kept.
    tone_curve_channel: usize,
    /// Welle 3 (LR-13/LR-20/LR-09/LR-12/LR-17 light) display/session state.
    /// All of these are display-only or `extras`/history-backed, so no
    /// sidecar schema change was needed:
    /// * `filter_bar_visible` + `library_filter`: `\` Library drawer (text
    ///   filter over the scanned entry metadata + Quick Develop sliders).
    /// * `compare_mode`: `C` compare / `N` survey proxy reusing
    ///   `before_after` (compare) and the Library grid (survey).
    /// * `before_after_split`: `Shift+Y` split-view marker (full-frame
    ///   Before proxy; side-by-side render is follow-up work).
    /// * `fullscreen`: `F` fullscreen preview (hides the same chrome as
    ///   lights-out and settles the zoom on Fit).
    filter_bar_visible: bool,
    library_filter: String,
    compare_mode: Option<CompareMode>,
    /// G-09 (LRPAR-G09-LIB) Library view: Grid/Loupe/Compare/Survey over the
    /// same selection and filter. Display-only (never recipe/sidecar); a
    /// reload restores the default (`Grid`).
    library_view: LibraryView,
    /// LRPAR-G09-SORT-09: active Library sort mode (Name/CaptureDate/Custom).
    /// Display-only, but persisted per folder together with the custom order in
    /// `lumina-sort.json` in `.lumina/` (see `library_sort`). Default `Name`.
    library_sort: LibrarySort,
    /// LRPAR-G09-SORT-09: custom order as relative keys of the listed folder
    /// (stable names, never array positions). Empty when unused.
    library_sort_order: Vec<String>,
    before_after_split: bool,
    fullscreen: bool,
    /// G-16 (LRPAR-G16-POWER) session-only display state. Never persisted to
    /// the sidecar and never part of the recipe (like `Tab`/`L`/`F`):
    /// * `softproof_preview`: plain `S` softproof-preview badge (the G-10
    ///   binding reservation; full print/gamut simulation is G-10 follow-up).
    /// * `show_original_histogram`: G-10 "Original Photo" histogram compare —
    ///   shows the unedited Original-Decode measurement instead of the edited
    ///   render (same `analyze_tone`/`LuminanceHistogram` path as Before/After,
    ///   no second analysis path).
    /// * `masking_preview`: `Alt`-held tone-slider edit transiently shows the
    ///   clipping badge through the `J` path; holds the slider key while armed.
    softproof_preview: bool,
    show_original_histogram: bool,
    masking_preview: Option<String>,
    /// LRPAR-G01-BASIC: "Reset Sliders Automatically" (Develop footer
    /// checkbox, default off). Folder-inherited via `.lumina/settings.json`
    /// (`FolderCacheSettings::reset_sliders_automatically`): when armed,
    /// switching images discards an armed-but-uncommitted slider edit;
    /// otherwise it is flushed to the previous image's sidecar. Never part
    /// of a recipe or sidecar (edit behaviour, not image state).
    reset_sliders_automatically: bool,
    /// LRPAR-G01-BASIC: Previous baseline (recipe snapshot captured at image
    /// load and after every successful save). `Previous` restores one
    /// section's fields from this snapshot; `Reset` sets the section's
    /// documented defaults. Masking layers travel separately in
    /// `mask_baseline` (they live on the virtual copy, not the recipe).
    recipe_baseline: Option<EditRecipe>,
    /// LRPAR-G01-BASIC: mask-layer baseline of the active virtual copy for
    /// the Masking section Previous/Reset (same capture points as above).
    mask_baseline: Vec<MaskLayer>,
    /// White-balance eyedropper armed state (global absolute picker).
    wb_pick_mode: bool,
    /// Local relative-WB picker armed state. It is intentionally separate from
    /// `wb_pick_mode` so a local sample can never mutate global WB fields.
    local_wb_pick_mode: bool,
    /// MASK-LOCAL-P1.2a: selected channel of the mask-local tone-curve editor
    /// (0 = master, 1..=3 = red/green/blue). Display-only session state, like
    /// `tone_curve_channel`; the curve itself lives on the mask layer.
    mask_local_curve_channel: usize,
    /// MASK-LOCAL-P1.2b: selected HSL band (0..=7) of the mask-local colour
    /// editor. Display-only session state; the band lives on the mask layer.
    mask_local_hsl_band: usize,
    /// MASK-LOCAL-P1.2b: selected grading range (0 = shadows, 1 = midtones,
    /// 2 = highlights) of the mask-local colour editor. Display-only session
    /// state; the range lives on the mask layer.
    mask_local_grading_range: usize,
    /// LRPAR-G14-REDEYE-15: red-eye region picker armed state (click the
    /// preview to mark a pupil; regions are persisted explicitly).
    red_eye_pick_mode: bool,
    /// LRPAR-G14-REDEYE-AUTO-15: last detection outcome text (candidates
    /// listed, never silently applied).
    red_eye_detect_status: String,
    /// Generated filmstrip thumbnail textures.
    thumbnails: ThumbnailManager,
    /// GUI-FILMSTRIP-SYNC-1: multi-selection of filmstrip entries
    /// (Lightroom-like). Paths are stored as display strings (the same key
    /// [`Self::open_file`] takes), never indices — entries re-sort on rescan.
    filmstrip_selection: BTreeSet<String>,
    /// Anchor for Shift-Click range selection (last plain/toggle click).
    filmstrip_anchor: Option<String>,
    // ---- PERF-GUI-* (CPU interactivity quick-wins, no GPU) ----
    /// True while the preview shows a low-resolution draft (rendered from
    /// `draft_original` during a slider drag); cleared on the full render.
    preview_is_draft: bool,
    /// Source downscaled to draft resolution, cached on load so draft renders
    /// never re-allocate during a slider drag (PERF-GUI-3 "zero alloc").
    draft_original: Option<ImageFrame>,
    /// R2-JANK-1 F1/F4: frame-budget throttle for the interactive draft render
    /// (F1) and cadence throttle for the draft analysis pass (F4). Time-state
    /// only; the visible pending marker is derived from it.
    draft_throttle: DraftThrottle,
    /// PERF-GUI-1: RAM cache of prepared base stages (`prepare_source_base`
    /// output: post decode/source-actions/ROI-crop, pre-adjustment), keyed by
    /// `CacheStage::Base` digests. Recipe-blind keys mean every slider change
    /// reuses the cached demosaiced base and re-renders only the adjustment
    /// stage downstream; a new source clears it in [`Self::apply_decoded_frame`].
    base_stage_cache: StageFrameCache,
    /// PERF-GUI-1: memoized blake3 content hash of `source_bytes`. Hashing the
    /// whole RAW file per render tick was part of the old hot path; it is now
    /// computed once per loaded source and invalidated with it.
    source_hash_memo: Option<String>,
    /// PERF-GUI-1: stage work counters of the last completed render
    /// (diagnostics + cache-hit tests). `None` until the first staged render.
    last_stage_work: Option<StageWork>,
    /// R2-GUIMOD-04a: milliseconds of the `analyze_tone_with_histogram` pass
    /// inside the last `render_from` (measurement only, never read for logic).
    last_analysis_ms: f64,
    /// R2-GUIMOD-04a: per-tick drag-render timings of the last coalesced
    /// pointer-drag tick (measurement only, feeds F-103-N6).
    last_drag_tick: Option<DragTickTimings>,
    /// R3-RENDER-SIZE-1: preview viewport-cap state (dpr, draft edge, cache).
    preview_cap_state: preview_size::PreviewCapState,
    /// Timestamp (egui `ctx.input(|i| i.time)`) of the last frame that still
    /// had pending edits; drives the 150 ms idle debounce (PERF-GUI-3/4).
    last_edit_time: f64,
    /// Preview zoom factor (1.0 = fit). >1 zooms into the centre; the render
    /// path crops to the visible source bounding box (ROI, PERF-GUI-5).
    preview_zoom: f32,
    /// Active zoom mode driving `preview_zoom` (see [`ZoomMode`]). Non-`Custom`
    /// modes re-derive `preview_zoom` from the pane each frame (so they survive
    /// resizes); `Custom` pins an explicit relative-to-fit multiplier.
    zoom_mode: ZoomMode,
    /// Screen-space pan offset (px) from the centred position, applied when the
    /// preview is zoomed beyond fit so the image can be dragged (hand tool).
    preview_pan: egui::Vec2,
    /// Source ROI `(x, y, w, h)` of the currently displayed texture. `None`
    /// means the whole frame. Pointer→source mapping accounts for this crop so
    /// the WB eyedropper and mask tools stay accurate while zoomed/panned.
    /// The rect is expressed in *render-source* pixels (see
    /// [`Self::preview_render_src`]): identical to full-source pixels for a
    /// full render, downscaled for a draft render.
    preview_roi: Option<[u32; 4]>,
    /// Render-source dimensions `(w, h)` backing the currently displayed
    /// texture (GUI-DRAFT-JUMP-1): the `source.width/height` that
    /// [`Self::render_from`] consumed for the last render — the full original
    /// on the non-draft path, the downscaled `draft_original` on the draft
    /// path. `draw_preview` scales the texture back into full-source geometry
    /// with it (`draw = tex_dims · (full/render_src) · scale`), so a draft
    /// and its full render share the exact on-screen placement instead of the
    /// draft drawing too small with a pan offset error. `None` until the first
    /// render (legacy/empty state: no rescaling).
    preview_render_src: Option<(u32, u32)>,
    /// Cached geometry from the last [`Self::draw_preview`] so the next frame's
    /// [`Self::sync_zoom`] can derive absolute zoom modes correctly.
    ///
    /// `preview_base_fit_scale` is the object-contain fit of the pane against
    /// the **un-cropped source dimensions**, never against the currently
    /// displayed texture: at zoom > 1 that texture is an ROI crop whose fit
    /// scale depends on the zoom itself, so deriving absolute modes (100% /
    /// 200% / Fit Width) from it oscillates frame-by-frame
    /// (REVIEW-GUI-ZOOMLOOP-1).
    preview_base_fit_scale: f32,
    preview_pane_w: f32,
    preview_pane_h: f32,
    /// Un-cropped source dimensions backing the displayed texture (cached so
    /// `sync_zoom` can compute Fit-Width and the base fit without borrowing
    /// `self.original`).
    preview_src_w: f32,
    preview_src_h: f32,
    /// Effective on-screen scale (screen px per source px) for the zoom readout.
    preview_effective_scale: f32,
    /// KITTEST-PARITY-PATHS-1: on-screen rect of the preview image quad as
    /// painted by the last [`Self::draw_preview`] — the same `rect` both present
    /// paths (CPU texture / GPU VRAM user texture) blit into. Pure geometry
    /// readout for the path-parity framework; never painted as text/badge.
    /// `None` before the first painted frame and while the empty state shows.
    preview_screen_rect: Option<egui::Rect>,
    /// KITTEST-PARITY-PATHS-1: the pane rect the preview was fitted into for the
    /// last painted frame (companion of [`Self::preview_screen_rect`]).
    preview_pane_rect: Option<egui::Rect>,
    /// KITTEST-PARITY-PATHS-1: the full-source overlay rect (`full_rect`) the
    /// last [`Self::draw_preview`] mapped every overlay (mask matte, pins,
    /// lens-blur focus, crop) onto. At Fit with no ROI this equals the painted
    /// preview rect; storing it lets the framework assert absolutely that
    /// overlays land on the photo instead of only comparing CPU↔GPU parity.
    overlay_full_rect: Option<egui::Rect>,
    /// Whether the left thumbnail navigator rail is open.
    navigator_open: bool,
    /// Library module: expanded folder-tree nodes, keyed by absolute path.
    open_folders: BTreeSet<String>,
    /// Library module: lazy per-folder children cache, filled via `read_dir`
    /// the first time a folder node is expanded.
    folder_children: BTreeMap<String, Vec<String>>,
    /// Library module: depth-limited RAW count + "has supported image"
    /// per folder node for R4-LIB-1 pruning (computed once per folder).
    folder_raw_counts: BTreeMap<String, library_tree::FolderTreeInfo>,
    /// UX-SLICE-2 (F2): injectable native folder-picker seam. `None`
    /// (production) opens the real `rfd` dialog; headless tests install a
    /// closure (no display server) so the empty-state CTA wiring can be
    /// asserted end to end. Session state, never persisted.
    folder_picker: Option<Box<dyn FnMut() -> Option<PathBuf>>>,
    /// Library module: current thumbnail cell size (px) for the center grid,
    /// driven by a toolbar slider (Lightroom-like resizable library thumbs).
    library_thumb_size: f32,
    /// G-09 (LRPAR-G09-LIB) Library grid columns of the last laid-out grid
    /// (drives ArrowUp/ArrowDown navigation by one row; headless default 4
    /// until the first draw measures the real width).
    library_cols: usize,
    /// G-15 META-MVP (Slice 3) Library metadata session state. The persisted
    /// truth stays Sidecar-first (`SidecarDocument.keywords` /
    /// `.collections` via `apply_batch_op` + `save_sidecar`); these fields
    /// are panel inputs + catalog session state only:
    /// * `keyword_input`: text field for adding a keyword to the loaded image.
    /// * `collection_id_input`/`collection_name_input`: `id` + display `name`
    ///   for adding the loaded image to a static collection (`id=name` also
    ///   accepted in the id field — split at the first `=`).
    /// * `active_collection`: grid filter selection (`None` = all images).
    /// * `smart_catalog`/`smart_catalog_path`: portable smart-collection
    ///   catalog in the CLI-identical `lumina-smart-catalog` v1 format;
    ///   persisted only via explicit Load/Save (never implicitly).
    /// * `smart_id_input`/`smart_name_input`/`smart_rule_kind`/
    ///   `smart_rule_value`: smart-definition editor inputs.
    /// * `smart_rule_stack`: pushed rules awaiting `And`/`Or`/`Not`
    ///   composition before Create.
    /// * `batch_kind`/`batch_value`: batch-bar inputs selecting exactly one
    ///   `BatchOp` for [`Self::apply_metadata_batch`].
    keyword_input: String,
    collection_id_input: String,
    collection_name_input: String,
    active_collection: Option<CollectionFilter>,
    smart_catalog: Vec<SmartCollectionDef>,
    smart_catalog_path: String,
    smart_id_input: String,
    smart_name_input: String,
    smart_rule_kind: String,
    smart_rule_value: String,
    smart_rule_stack: Vec<SmartRule>,
    batch_kind: String,
    batch_value: String,
    /// LRPAR-G15-IPTC-S8: Library Metadata panel (right column) session
    /// state. The persisted truth stays Sidecar-first
    /// (`SidecarDocument.metadata` draft + history, `keywords`); these
    /// fields are panel inputs + dialog state only:
    /// * `meta_buffers`/`meta_buffers_key`: per-field draft text inputs,
    ///   synced from the loaded document (`(path, latest_rev)` key); an
    ///   empty buffer on commit removes the field (S1 draft semantics).
    /// * `meta_presets_dir_override`: explicit presets directory for
    ///   headless tests (production uses the user-global directory).
    /// * `meta_preset_entries`: last scanned meta-presets (failed files
    ///   stay visible, never skipped silently).
    /// * `selected_meta_preset`: preset spec for Apply (display name or
    ///   path, same resolution as the CLI).
    /// * `meta_preset_dialog`: open prompt dialog for dynamic presets
    ///   (one required input per placeholder; Cancel discards it).
    /// * `meta_sync_fields`: field checkbox selection for
    ///   [`Self::sync_metadata_to_selection`] (default: all on).
    meta_buffers: BTreeMap<String, String>,
    meta_buffers_key: Option<(String, u64)>,
    /// LRPAR-G15-IPTC-S8: true once the user edited a draft buffer since
    /// the last sync — [`Self::ensure_meta_buffers`] then keeps the
    /// keystrokes and only fills missing fields, instead of resyncing from
    /// the document (which would wipe the edit, e.g. when the document is
    /// first created by the commit itself).
    meta_buffers_dirty: bool,
    meta_presets_dir_override: Option<PathBuf>,
    meta_preset_entries: Vec<MetaPresetEntry>,
    selected_meta_preset: String,
    meta_preset_dialog: Option<MetaPresetDialog>,
    meta_sync_fields: BTreeMap<String, bool>,
    /// KITTEST-COVERAGE-STATES-1: session clipboard for the metadata panel's
    /// own copy/paste system (separate from the Develop settings clipboard).
    /// Holds the non-empty draft field values copied from an image; `None`
    /// until the first copy. Session-only — never persisted (Sidecar-first).
    meta_clipboard: Option<BTreeMap<String, String>>,
    /// LRPAR-G15-IPTC-S8: cached embedded IPTC of the loaded image
    /// (read-only display). Keyed by `(path, length, mtime)` so the panel
    /// never re-reads the file per frame; `Err` text is the loud
    /// unreadable-JPEG error (never a silent skip).
    meta_embedded_cache: Option<EmbeddedCache>,
    /// Develop history section: currently selected (last restored) history
    /// entry id of the active virtual copy.
    history_selected: Option<String>,
    /// PERF-GUI-7: receiver for a background RAW/raster decode. `Some` while a
    /// decode is in flight on a worker thread. The result carries the request
    /// generation so a superseded worker can never adopt deferred navigation.
    decode_rx: Option<std::sync::mpsc::Receiver<DecodeRequestResult>>,
    /// Monotonic latest-wins tag for asynchronous decodes. A result is applied
    /// only when it still belongs to the most recently started request.
    decode_generation: u64,
    /// R2-MODSWITCH-1 F8: receiver for a background folder scan. `Some` while a
    /// scan is in flight on its worker thread; drained by `poll_scan`.
    scan_rx: Option<std::sync::mpsc::Receiver<library_scan::ScanResult>>,
    /// R2-MODSWITCH-1 F8: latest-wins generation tag for the async folder scan;
    /// a result tagged with an older generation is dropped (never merged).
    scan_generation: u64,
    /// R2-MODSWITCH-1 F8: a scan is in flight — drives the visible loading
    /// status so the grid never stalls silently.
    scan_pending: bool,
    /// A successful cross-directory decode keeps its active path here until
    /// the matching scan applies the target listing and schedules neighbors.
    pending_directory_open: Option<PendingDirectoryOpen>,
    /// REVIEW-GUI-N1: revision (BLAKE3 over the JSON) of the on-disk sidecar
    /// that the in-memory `document` lineage is based on. `None` means no
    /// sidecar file existed when this lineage started (fresh document). Passed
    /// to the compare-and-swap write in [`Self::save_sidecar`] so an
    /// externally modified sidecar surfaces as a visible conflict instead of
    /// being silently overwritten; refreshed after every successful save.
    sidecar_revision: Option<String>,
    /// True while an edit (slider drag, presence change, etc.) needs a
    /// full-quality render. Drives the debounced full render after a pointer
    /// drag settles (PERF-GUI-3/4). Cleared once the full render runs.
    pending_full_render: bool,
    /// Set once the directory-auto-load has begun a background decode, so
    /// `list_directory` never re-triggers it every time the directory is
    /// rescanned (the decode is async and `original` stays `None` until it
    /// finishes). Left unset while no RAW entry exists, so a later scan of a
    /// now-populated directory can still auto-load. Native-only (directory
    /// auto-load is a native file-system capability).
    auto_load_attempted: bool,
    /// GUI-60FPS-1: optional GPU context for the desktop. `None` when no adapter
    /// is bound (CPU fallback remains fully functional).
    #[cfg(feature = "gpu")]
    gpu: Option<lumina_gpu::GpuContext>,
    /// GUI-60FPS-1 H1: persistent R16 mask plane (Vec<u16> u16-LE, row-major,
    /// `width × height`) backing the interactive brush. Kept CPU-side so each
    /// dirty 512² tile can be (re-)stamped incrementally via
    /// `lumina_core::mask_tiles::stamp_brush_mark` and then uploaded with
    /// `queue.write_texture` (`bytemuck::cast_slice` → `&[u8]`). Only dirty tiles
    /// are uploaded per stroke (no whole-plane rewrite, no dummy zeros).
    #[cfg(feature = "gpu")]
    brush_mask_plane: Option<Vec<u16>>,
    #[cfg(feature = "gpu")]
    brush_mask_plane_dims: Option<(u32, u32)>,
    /// Identity of the retained live plane: source hash, copy id, mask id and
    /// dimensions. A mismatch forces a full selected-prompt rebuild/upload.
    #[cfg(feature = "gpu")]
    brush_mask_plane_scope: Option<(String, String, String, u32, u32)>,
    /// GPU-MASKPLANE-BRUSH-50: the live brush upload wrote nothing, so whatever
    /// plane is still resident must not be presented as current. Set by
    /// `note_live_brush_upload_outcome(false)` and read by
    /// `vram_mask_plane_intent`, which demotes the intent to `Clear`.
    #[cfg(feature = "gpu")]
    live_brush_plane_stale: bool,
    /// GUI-WGPU-PRESENT-1: the eframe wgpu renderer's shared state. When
    /// present, `lumina-gpu` was constructed on the *same* Device/Queue
    /// (see `attach_wgpu_render_state`), so the VRAM overlay composite can be
    /// registered as an egui user texture and presented without any CPU
    /// readback.
    #[cfg(feature = "gpu")]
    wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    /// Offscreen target the VRAM overlay pass composites into; registered once
    /// as an egui user texture and re-created only when dimensions change.
    #[cfg(feature = "gpu")]
    present_target: Option<PresentTarget>,
    /// True while the VRAM output corresponds to the current recipe/source:
    /// set right after a successful `render_to_vram`, cleared by every edit
    /// ([`Self::mark_dirty`]) so a stale tone result can never be presented.
    /// R2-GUIMOD-01: also cleared by every completed **full-quality** CPU
    /// render (`render_from` on the non-draft path) — otherwise the debounced
    /// full render after a drag would compute sharp pixels that are then never
    /// shown because the gate kept presenting the superseded VRAM draft.
    #[cfg(feature = "gpu")]
    vram_fresh: bool,
    /// R2-GUIMOD-05: memoized `unsupported_gpu_stages(&self.recipe)` verdict,
    /// keyed by the [`RenderKey`] of the render it was computed for. The gate
    /// used to rebuild this `Vec<String>` (with `format!` allocations) every
    /// frame although recipe/render identity rarely changes. `None` while no
    /// key-backed verdict is stored; queried without a render key (dirty
    /// preview) deliberately bypasses the memo because the recipe may have
    /// drifted since the last render.
    #[cfg(feature = "gpu")]
    gpu_stage_gate: Option<GpuStageGate>,
    /// True while the VRAM mask texture carries the pipeline-*evaluated* layer
    /// planes (pushed after a full render) rather than only live brush stamps —
    /// then the shader overlay already shows what the CPU overlay would paint.
    #[cfg(feature = "gpu")]
    vram_mask_is_evaluated: bool,
    /// GPU-MASKPLANE-CLEAR-PERF-51 (`GPU-CLEAR-TRANSITION`), see
    /// `present_mask_plane.rs`.
    #[cfg(feature = "gpu")]
    vram_mask_plane_clear: crate::present_mask_plane::VramMaskPlaneClear,
    /// The egui user-texture id + size of the GPU-presented preview for THIS
    /// frame (recomputed in `update_texture`, consumed in `draw_preview`).
    #[cfg(feature = "gpu")]
    gpu_present_frame: Option<(egui::TextureId, [usize; 2])>,
    /// R2-GUIMOD-06: visible (non-stderr) feedback for the GPU→CPU routing
    /// fallback. `Some(reason)` when a GPU context is available and usable but
    /// the recipe references stages the VRAM tone path cannot evaluate, so the
    /// preview is computed on the CPU — a silent fallback before this fix.
    /// `None` while the GPU present path is usable (or when no GPU context
    /// exists at all, in which case there is no "fallback" to report). Surfaced
    /// as a status badge in the preview HUD; it never affects rendered pixels.
    #[cfg(feature = "gpu")]
    gpu_route_fallback: Option<String>,
    /// GUI-LENSFUN-GATE-3 (F1): the precise present-refusal reason of the last
    /// failed [`lumina_gpu::GpuContext::render_to_vram`] attempt. The recipe
    /// gate ([`Self::gpu_unsupported_stage_reasons`]) does not cover
    /// dimension-changing geometry (`render_to_vram` refuses it after the gate
    /// passed) and was therefore the only CPU route without a visible badge.
    /// When set, [`Self::routing_fallback_reason`] surfaces it as the badge
    /// **only if** the gate itself is empty. Diagnostic only: never consulted
    /// for routing/presentation; cleared by any edit (`mark_dirty`/
    /// `set_adjustment`), a new source, adopting a neighbor frame, or a
    /// successful VRAM render.
    #[cfg(feature = "gpu")]
    vram_render_refusal: Option<String>,
    /// PARITY-PATHS-2: diagnostic override for the adapter-availability probe
    /// ([`Self::gpu_adapter_available`]). `Some(false)` lets the parity matrix
    /// exercise its adapterless SKIP branch on a machine that does have a
    /// Metal adapter; `None` (default) reports the real GPU context.
    /// Presentation/routing logic does **not** consult this — it only changes
    /// the probe the SKIP contract keys on.
    #[cfg(feature = "gpu")]
    gpu_adapter_override: Option<bool>,
    /// GUI-SCROLL-200-1: per-frame diagnostic counters for `LUMINA_PERF_LOG=1`.
    /// `frame_thumb_enqueued` counts worker jobs enqueued (or cached previews
    /// loaded) this frame, `frame_thumbs_ready` counts worker results applied.
    /// Both are reset at the start of [`Self::update`]; a scroll spike while
    /// thumbnail jobs run shows up as large values in the same frame that
    /// exceeds the 16.7 ms budget.
    frame_thumb_enqueued: usize,
    frame_thumbs_ready: usize,
    /// PREVIEW-CACHE-FEATURE: neighbor-preview controller (worker pool + RAM/disk
    /// LRU + prefetch window). Lazy-created on first navigation so unit tests
    /// that never schedule neighbors stay thread-free.
    preview_ctrl: Option<preview_ctrl::PreviewController>,
    /// PREVIEW-CACHE-FEATURE: per-frame counters for the neighbor-preview work
    /// (LUMINA_PERF_LOG diagnostics).
    frame_previews_enqueued: usize,
    frame_previews_ready: usize,
    /// GUI-TOAST-OVERLAP-1: transient overlay toast (message + egui-time
    /// deadline). Shown in its own [`egui::Area`] so it never takes layout
    /// width or covers thumbnails persistently; auto-dismissed after
    /// [`TOAST_TIMEOUT_SECONDS`], manually dismissible via its button.
    toast_message: Option<String>,
    toast_until: f64,
    /// LRPAR-G14-DENOISE-IMPL-20 (GUI slice): resolved status/policy of the
    /// active `denoise_ai` stage. Session state (never persisted — the recipe
    /// is); refreshed from recipe + `.lumina.zdata` by
    /// [`denoise_gui::LuminaApp::refresh_denoise_gui`].
    denoise_gui: denoise_gui::DenoiseGuiState,
    /// Explicit integration/test seam for the live denoiser model context.
    /// Production leaves it `None`: no licence-cleared weights are bundled
    /// (F-078 gate), so the resolved status is the honest `unavailable`.
    denoise_live_model: Option<DenoiseModelIdentity>,
    /// Set by every denoise-relevant edit (recipe/source/copy); the panel and
    /// the render then refresh the resolved state once instead of reading the
    /// `.lumina.zdata` bundle every frame.
    denoise_gui_dirty: bool,
    /// LRPAR-G09-CULL-25 (GUI slice): session-only People-view filter for the
    /// Library module (`person:<name>` token). Display state, never persisted.
    people_filter: String,
    /// LRPAR-G12-FACE-20 (S5): selected cluster id of the People view (empty =
    /// none selected). Session-only, never persisted.
    people_selected_cluster: String,
    /// LRPAR-G12-FACE-20 (S5): cached face-crop textures of the People view,
    /// keyed by `"<path>|<detection_id>"` (session-only, rebuilt on demand).
    face_crop_textures: BTreeMap<String, egui::TextureHandle>,
    /// LRPAR-G09-CULL-25 (GUI slice): selected cluster/person inputs for the
    /// People view actions (confirm name, split subset, merge target).
    /// Session-only panel inputs.
    people_name_input: String,
    people_split_subset: String,
    people_merge_target: String,
    /// LRPAR-G13-MERGE-15 (GUI slice): running merge job receiver.
    merge_job: Option<merge_gui::MergeJob>,
    /// LRPAR-G13-MERGE-15 (GUI slice): last merge outcome text (status line).
    merge_status: String,
}
