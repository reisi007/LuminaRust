//! Develop action verbs: flat adjustment/presence setters, tone curve, HSL, color
//! grading, point color, effects, detail and the tone endpoints, before/after and
//! the local white-balance picker.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
    pub fn set_adjustment(&mut self, name: &str, value: f64) {
        trace!(
            "GUI interaction: set_adjustment {}={} (before render)",
            name,
            value
        );
        self.recipe.adjustments.insert(name.into(), value);
        // GUI-SLIDER-SAVE-1: remember the commit so the debounced full render
        // can save the sidecar and log `<key>=<value> saved`. Zoom/pan state
        // is deliberately never recorded here — it stays GUI session state.
        self.pending_slider_commit = Some((name.to_string(), value));
        // PERF-GUI-1 stepwise invalidation: an adjustment is downstream of the
        // base stage, so only the render identity and the derived tone panel
        // are invalidated here. The cached demosaiced base
        // (`base_stage_cache`) stays — its `CacheStage::Base` digest is
        // recipe-blind, so the next render hits it and recomputes exactly the
        // Adjustments(+geometry/masks) stages.
        self.render_key = None;
        self.effective_source_stage = None;
        self.effective_source_stage_digest = None;
        self.tone_analysis = None;
        // Coalesce: the slider drag renders a draft live; the full render is
        // deferred to pointer release (PERF-GUI-3/4).
        self.pending_full_render = true;
        self.status = Str::ChangePending.t().into();
        self.error = None;
        // GFX-SLIDER-VRAM-FRESH: an adjustment is an edit, exactly like
        // `mark_dirty` — the VRAM tone result no longer matches the recipe and
        // must never be presented until the drag path re-renders it. `mark_dirty`
        // cleared `vram_fresh` here, but `set_adjustment` (the interactive slider
        // path) did not, so a stale VRAM frame could keep being presented after a
        // slider change that did not immediately re-run `render_to_vram` (e.g.
        // in headless tests / programmatic `set_adjustment` with no pointer drag).
        #[cfg(feature = "gpu")]
        {
            self.vram_fresh = false;
            self.vram_mask_is_evaluated = false;
            // GUI-LENSFUN-GATE-3 (F1): the recipe changed, so a present refusal
            // captured for the previous recipe is no longer known to apply.
            self.vram_render_refusal = None;
        }
    }

    /// Set a single Presence field (`texture`, `clarity` or `dehaze`). The value
    /// is stored in the normative `-1..=1` domain (shown as `-100..+100`); the
    /// sidecar validation is the source of truth for the domain, so out-of-range
    /// values are stored as given and rejected on save rather than silently
    /// clamped (no silent fallback).
    pub fn set_presence(&mut self, field: &str, value: f64) {
        let mut presence = self.recipe.presence.unwrap_or(Presence {
            version: 1,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
        });
        match field {
            "texture" => presence.texture = value as f32,
            "clarity" => presence.clarity = value as f32,
            "dehaze" => presence.dehaze = value as f32,
            _ => return,
        }
        self.recipe.presence = Some(presence);
        trace!("GUI interaction: set_presence {}={}", field, value);
        // GUI-SLIDER-SAVE-1: presence sliders commit like flat adjustments.
        self.pending_slider_commit = Some((format!("presence.{field}"), value));
        self.mark_dirty();
    }

    /// Validate curve points like the core (`2..=32` points, finite values in
    /// `0..=1`, strictly ascending inputs, `(0,0)`/`(1,1)` endpoints). `None`
    /// is valid with the offending description.
    pub(crate) fn validate_curve_points(points: &[CurvePoint]) -> Option<String> {
        if !(2..=32).contains(&points.len()) {
            return Some(format!("need 2..=32 points, got {}", points.len()));
        }
        for (index, point) in points.iter().enumerate() {
            for (field, value) in [("input", point.input), ("output", point.output)] {
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Some(format!("points[{index}].{field} out of range"));
                }
            }
            if index > 0 && point.input <= points[index - 1].input {
                return Some(format!("points[{index}].input not ascending"));
            }
        }
        let (first, last) = (&points[0], &points[points.len() - 1]);
        if first.input != 0.0 || first.output != 0.0 {
            return Some("first point must be (0,0)".into());
        }
        if last.input != 1.0 || last.output != 1.0 {
            return Some("last point must be (1,1)".into());
        }
        None
    }

    /// Set one free point-curve control point (`input`/`output`) of one channel
    /// (G-02) and record the save commit. The interactive graph
    /// (UX-LOOK-TONECURVE-18) is the production editor; this per-field path is
    /// test-owned (`#[cfg(test)]`, loud-refusal coverage). Violations are
    /// refused loudly (status + no save), never clipped silently.
    #[cfg(test)]
    pub(crate) fn set_curve_point(&mut self, channel: &str, index: usize, field: &str, value: f64) {
        if !matches!(field, "input" | "output") {
            warn!("set_curve_point: unknown field {field}");
            return;
        }
        if !matches!(channel, "master" | "red" | "green" | "blue") {
            warn!("set_curve_point: unknown channel {channel}");
            return;
        }
        let mut candidate = self.recipe.curves.clone().unwrap_or_else(|| Curves {
            version: 1,
            master: identity_curve_points(),
            channels: CurveChannels::default(),
        });
        let slot: &mut Vec<CurvePoint> = match channel {
            "master" => &mut candidate.master,
            "red" => candidate
                .channels
                .red
                .get_or_insert_with(identity_curve_points),
            "green" => candidate
                .channels
                .green
                .get_or_insert_with(identity_curve_points),
            "blue" => candidate
                .channels
                .blue
                .get_or_insert_with(identity_curve_points),
            _ => unreachable!(),
        };
        let Some(point) = slot.get_mut(index) else {
            warn!("set_curve_point: {channel}[{index}] out of bounds");
            return;
        };
        match field {
            "input" => point.input = value as f32,
            "output" => point.output = value as f32,
            _ => unreachable!(),
        }
        if let Some(reason) = Self::validate_curve_points(slot) {
            self.status = Str::ToneCurveInvalidPattern.format_arg(&reason);
            warn!("set_curve_point: {channel}[{index}].{field} refused ({reason})");
            return;
        }
        self.recipe.curves = Some(candidate);
        self.mark_recipe_dirty(&format!("curves.{channel}.points[{index}].{field}"), value);
    }

    /// Insert a free point-curve control point into one channel (G-02),
    /// sorted by input. Refuses invalid/duplicate inputs loudly (status +
    /// no save). Replaces that channel's parametric list (Last-Write-Wins).
    pub(crate) fn add_curve_point(&mut self, channel: &str, input: f64, output: f64) {
        instrument_gui_action!(self, GuiAction::AddCurvePoint);
        if !matches!(channel, "master" | "red" | "green" | "blue") {
            warn!("add_curve_point: unknown channel {channel}");
            return;
        }
        let mut candidate = self.recipe.curves.clone().unwrap_or_else(|| Curves {
            version: 1,
            master: identity_curve_points(),
            channels: CurveChannels::default(),
        });
        let slot: &mut Vec<CurvePoint> = match channel {
            "master" => &mut candidate.master,
            "red" => candidate
                .channels
                .red
                .get_or_insert_with(identity_curve_points),
            "green" => candidate
                .channels
                .green
                .get_or_insert_with(identity_curve_points),
            "blue" => candidate
                .channels
                .blue
                .get_or_insert_with(identity_curve_points),
            _ => unreachable!(),
        };
        slot.push(CurvePoint {
            input: input as f32,
            output: output as f32,
        });
        slot.sort_by(|a, b| a.input.total_cmp(&b.input));
        if let Some(reason) = Self::validate_curve_points(slot) {
            self.status = Str::ToneCurveInvalidPattern.format_arg(&reason);
            warn!("add_curve_point: {channel} ({input},{output}) refused ({reason})");
            return;
        }
        self.recipe.curves = Some(candidate);
        info!("GUI interaction: curves.{channel} add point ({input},{output})");
        self.mark_recipe_dirty(&format!("curves.{channel}.add"), input);
    }

    /// Remove a free point-curve control point from one channel (G-02).
    /// Refuses loudly when fewer than 3 points remain or an endpoint
    /// (`(0,0)`/`(1,1)`) is targeted — endpoints are mandatory.
    pub(crate) fn remove_curve_point(&mut self, channel: &str, index: usize) {
        instrument_gui_action!(self, GuiAction::RemoveCurvePoint);
        if !matches!(channel, "master" | "red" | "green" | "blue") {
            warn!("remove_curve_point: unknown channel {channel}");
            return;
        }
        let mut candidate = match self.recipe.curves.clone() {
            Some(curves) => curves,
            None => {
                warn!("remove_curve_point: no curves for {channel}");
                return;
            }
        };
        let len = match channel {
            "master" => candidate.master.len(),
            "red" => candidate.channels.red.as_ref().map_or(0, Vec::len),
            "green" => candidate.channels.green.as_ref().map_or(0, Vec::len),
            "blue" => candidate.channels.blue.as_ref().map_or(0, Vec::len),
            _ => unreachable!(),
        };
        if len <= 2 {
            self.status = Str::ToneCurveInvalidPattern.format_arg("need 2..=32 points");
            warn!("remove_curve_point: {channel} already minimal");
            return;
        }
        if index == 0 || index + 1 >= len {
            self.status =
                Str::ToneCurveInvalidPattern.format_arg("endpoints (0,0)/(1,1) are mandatory");
            warn!("remove_curve_point: {channel}[{index}] is an endpoint");
            return;
        }
        let slot: &mut Vec<CurvePoint> = match channel {
            "master" => &mut candidate.master,
            "red" => candidate.channels.red.as_mut().expect("len>0"),
            "green" => candidate.channels.green.as_mut().expect("len>0"),
            "blue" => candidate.channels.blue.as_mut().expect("len>0"),
            _ => unreachable!(),
        };
        if index >= slot.len() {
            warn!("remove_curve_point: {channel}[{index}] out of bounds");
            return;
        }
        slot.remove(index);
        self.recipe.curves = Some(candidate);
        info!("GUI interaction: curves.{channel} remove point {index}");
        self.mark_recipe_dirty(&format!("curves.{channel}.remove"), index as f64);
    }

    /// Set one HSL mixer channel field (`red`…`magenta` × `hue`/`saturation`/
    /// `luminance`) and record the save commit (GUI-SLIDER-SAVE-1). Unknown
    /// names are ignored loudly — all call sites pass literals.
    pub(crate) fn set_hsl_value(&mut self, channel: &str, field: &str, value: f64) {
        if !matches!(
            channel,
            "red" | "orange" | "yellow" | "green" | "cyan" | "blue" | "violet" | "magenta"
        ) {
            warn!("set_hsl_value: unknown channel {channel}");
            return;
        }
        let mut hsl = self.recipe.hsl.clone().unwrap_or_default();
        // The derived `Default` carries `version: 0`, which the sidecar
        // validation rejects (`unsupported hsl version`) — a fresh HSL block
        // is always version 1 (same class of explicit-version construction as
        // every other struct setter here).
        hsl.version = 1;
        match field {
            "hue" => hsl_channel_mut(&mut hsl, channel).hue = value as f32,
            "saturation" => hsl_channel_mut(&mut hsl, channel).saturation = value as f32,
            "luminance" => hsl_channel_mut(&mut hsl, channel).luminance = value as f32,
            _ => {
                warn!("set_hsl_value: unknown field {field}");
                return;
            }
        }
        self.recipe.hsl = Some(hsl);
        self.mark_recipe_dirty(&format!("hsl.{channel}.{field}"), value);
    }

    /// Set one color-grading range field (`shadows`/`midtones`/`highlights` ×
    /// `hue_degrees`/`saturation`/`luminance`) and record the save commit
    /// (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly.
    pub(crate) fn set_color_grading_value(&mut self, range: &str, field: &str, value: f64) {
        let mut cg = self
            .recipe
            .color_grading
            .clone()
            .unwrap_or_else(ColorGrading::neutral);
        let slot = match range {
            "shadows" => &mut cg.shadows,
            "midtones" => &mut cg.midtones,
            "highlights" => &mut cg.highlights,
            _ => {
                warn!("set_color_grading_value: unknown range {range}");
                return;
            }
        };
        match field {
            "hue_degrees" => slot.hue_degrees = value as f32,
            "saturation" => slot.saturation = value as f32,
            "luminance" => slot.luminance = value as f32,
            _ => {
                warn!("set_color_grading_value: unknown field {field}");
                return;
            }
        }
        self.recipe.color_grading = Some(cg);
        self.mark_recipe_dirty(&format!("color_grading.{range}.{field}"), value);
    }

    /// Set the color-grading balance and record the save commit
    /// (GUI-SLIDER-SAVE-1).
    pub(crate) fn set_color_grading_balance(&mut self, value: f64) {
        let mut cg = self
            .recipe
            .color_grading
            .clone()
            .unwrap_or_else(ColorGrading::neutral);
        cg.balance = value as f32;
        self.recipe.color_grading = Some(cg);
        self.mark_recipe_dirty("color_grading.balance", value);
    }

    /// Set the color-grading blending (G-02 Feinschliff, `0..=1`) and record
    /// the save commit (GUI-SLIDER-SAVE-1).
    pub(crate) fn set_color_grading_blending(&mut self, value: f64) {
        let mut cg = self
            .recipe
            .color_grading
            .clone()
            .unwrap_or_else(ColorGrading::neutral);
        cg.blending = value as f32;
        self.recipe.color_grading = Some(cg);
        self.mark_recipe_dirty("color_grading.blending", value);
    }

    /// Add a Point Color entry (G-02, F-090b) with neutral shifts and record
    /// the save commit. The id is the next stable `pc-<n>` over the current
    /// recipe state (never positional). At most 8 entries; a ninth is
    /// refused loudly via the status line.
    pub(crate) fn add_point_color(&mut self) {
        instrument_gui_action!(self, GuiAction::AddPointColor);
        let mut block = self.recipe.point_color.clone().unwrap_or(PointColor {
            version: 1,
            entries: Vec::new(),
        });
        block.version = 1;
        if block.entries.len() >= 8 {
            self.status = Str::PointColorFull.t().to_string();
            warn!("add_point_color: entry limit (8) reached");
            return;
        }
        let id = PointColorEntry::next_id(&block.entries);
        block.entries.push(PointColorEntry {
            id: id.clone(),
            hue_center: 0.0,
            hue_range: 30.0,
            hue_shift: 0.0,
            saturation_shift: 0.0,
            luminance_shift: 0.0,
        });
        self.recipe.point_color = Some(block);
        info!("GUI interaction: point_color add {id}");
        self.mark_recipe_dirty(
            "point_color.add",
            self.recipe
                .point_color
                .as_ref()
                .map_or(0.0, |block| block.entries.len() as f64),
        );
    }

    /// Remove one Point Color entry by stable id (G-02). Unknown ids are
    /// refused loudly; removing the last entry drops the whole block
    /// (absent = identity).
    pub(crate) fn remove_point_color(&mut self, id: &str) {
        instrument_gui_action!(self, GuiAction::RemovePointColor);
        let Some(block) = self.recipe.point_color.clone() else {
            warn!("remove_point_color: no point_color block for id {id}");
            return;
        };
        let len = block.entries.len();
        let entries: Vec<PointColorEntry> =
            block.entries.into_iter().filter(|e| e.id != id).collect();
        if entries.len() == len {
            warn!("remove_point_color: unknown id {id}");
            return;
        }
        self.recipe.point_color = if entries.is_empty() {
            None
        } else {
            Some(PointColor {
                version: 1,
                entries,
            })
        };
        info!("GUI interaction: point_color remove {id}");
        self.mark_recipe_dirty("point_color.remove", len as f64);
    }

    /// Set one Point Color entry field (`hue_center`/`hue_range`/
    /// `hue_shift`/`saturation_shift`/`luminance_shift`) and record the save
    /// commit (GUI-SLIDER-SAVE-1). Unknown ids/fields are ignored loudly.
    /// Out-of-range values are refused loudly (status + no save) instead of
    /// being clipped silently.
    pub(crate) fn set_point_color_value(&mut self, id: &str, field: &str, value: f64) {
        let Some(mut block) = self.recipe.point_color.clone() else {
            warn!("set_point_color_value: no point_color block for id {id}");
            return;
        };
        let Some(entry) = block.entries.iter_mut().find(|e| e.id == id) else {
            warn!("set_point_color_value: unknown id {id}");
            return;
        };
        let (lo, hi) = match field {
            "hue_center" => (0.0, 360.0),
            "hue_range" => (0.0, 180.0),
            "hue_shift" | "saturation_shift" | "luminance_shift" => (-1.0, 1.0),
            _ => {
                warn!("set_point_color_value: unknown field {field}");
                return;
            }
        };
        if !value.is_finite() || !(lo..=hi).contains(&value) {
            self.status = Str::PointColorRangePattern.format_arg(&format!("{id}.{field}"));
            warn!("set_point_color_value: {id}.{field}={value} out of range");
            return;
        }
        match field {
            "hue_center" => entry.hue_center = value as f32,
            "hue_range" => entry.hue_range = value as f32,
            "hue_shift" => entry.hue_shift = value as f32,
            "saturation_shift" => entry.saturation_shift = value as f32,
            "luminance_shift" => entry.luminance_shift = value as f32,
            _ => unreachable!(),
        }
        self.recipe.point_color = Some(block);
        self.mark_recipe_dirty(&format!("point_color.{id}.{field}"), value);
    }

    /// Set one effects field (`vignette` × `amount`/`midpoint`/`roundness`/
    /// `feather`, `grain` × `amount`/`size`/`roughness`/`seed`) and record the
    /// save commit (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly.
    pub(crate) fn set_effects_value(&mut self, group: &str, field: &str, value: f64) {
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
        match group {
            "vignette" => {
                let slot = effects.vignette.get_or_insert(Vignette {
                    version: 1,
                    amount: 0.0,
                    midpoint: 0.5,
                    roundness: 0.0,
                    feather: 0.0,
                });
                match field {
                    "amount" => slot.amount = value as f32,
                    "midpoint" => slot.midpoint = value as f32,
                    "roundness" => slot.roundness = value as f32,
                    "feather" => slot.feather = value as f32,
                    _ => {
                        warn!("set_effects_value: unknown vignette field {field}");
                        return;
                    }
                }
            }
            "grain" => {
                let slot = effects.grain.get_or_insert(Grain {
                    version: 1,
                    amount: 0.0,
                    size: 0.0,
                    roughness: 0.0,
                    seed: 0,
                });
                match field {
                    "amount" => slot.amount = value as f32,
                    "size" => slot.size = value as f32,
                    "roughness" => slot.roughness = value as f32,
                    "seed" => slot.seed = value as u64,
                    _ => {
                        warn!("set_effects_value: unknown grain field {field}");
                        return;
                    }
                }
            }
            _ => {
                warn!("set_effects_value: unknown group {group}");
                return;
            }
        }
        self.recipe.effects = Some(effects);
        self.mark_recipe_dirty(&format!("effects.{group}.{field}"), value);
    }

    /// Set one sharpening field (`amount`/`radius`/`detail`/`masking`) and
    /// record the save commit (GUI-SLIDER-SAVE-1). Unknown names are ignored
    /// loudly. Public for the headless KITTEST-PARITY-PATHS-1 matrix, which
    /// needs a Detail-scope recipe built through the same setter the panel uses.
    pub fn set_sharpening_value(&mut self, field: &str, value: f64) {
        let mut sh = self.recipe.sharpening.unwrap_or(Sharpening {
            version: 1,
            amount: 0.0,
            radius: 0.5,
            detail: 0.0,
            masking: 0.0,
        });
        match field {
            "amount" => sh.amount = value as f32,
            "radius" => sh.radius = value as f32,
            "detail" => sh.detail = value as f32,
            "masking" => sh.masking = value as f32,
            _ => {
                warn!("set_sharpening_value: unknown field {field}");
                return;
            }
        }
        self.recipe.sharpening = Some(sh);
        self.mark_recipe_dirty(&format!("sharpening.{field}"), value);
    }

    /// Set one noise-reduction field (`luminance`/`color`) and record the save
    /// commit (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly. Public for
    /// the headless KITTEST-PARITY-PATHS-1 matrix, which needs a Detail-scope
    /// recipe built through the same setter the panel uses.
    pub fn set_noise_reduction_value(&mut self, field: &str, value: f64) {
        let mut nr = self.recipe.noise_reduction.unwrap_or(NoiseReduction {
            version: 1,
            luminance: 0.0,
            color: 0.0,
        });
        match field {
            "luminance" => nr.luminance = value as f32,
            "color" => nr.color = value as f32,
            _ => {
                warn!("set_noise_reduction_value: unknown field {field}");
                return;
            }
        }
        self.recipe.noise_reduction = Some(nr);
        self.mark_recipe_dirty(&format!("noise_reduction.{field}"), value);
    }

    pub fn auto_tone(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::AutoTone);
        // Preserve the historic contract of this entry: the Auto button without a
        // loaded image is a silent no-op, not an error (the button is disabled in
        // that state anyway).
        if self.original.is_none() {
            return Ok(());
        }
        // AUTO-TONE-ANALYSIS-INPUT-8: measure the ANALYSIS DOMAIN (post
        // SourceActions + post Crop, without Adjustments), not the raw decode.
        // `auto_analysis_frame` is the shared production seam the CLI and
        // `regenerate` use, so both front ends measure the same pixels.
        let frame = self.auto_analysis_frame()?;
        // AUTO-TONE-CLI-6 clause (1): THIS IS THE SHARED WRITER. The GUI used to
        // carry its own copy of the six sliders, the six mirrors and the fingerprint,
        // with the algorithm as a hand-copied string literal. WITHDRAWN, because this
        // change's own measurement refuted the first version of this comment
        // (DoD.md §9): the copy did NOT make every GUI-written auto-tone permanently
        // stale — both writers agreed on every field, so no GUI recipe was ever stale.
        // The defect was the DUPLICATION and its latent risk: a diverging algorithm
        // string is exactly what `auto_tone_is_fresh` compares.
        //
        // A user pressing Auto Tone is an explicit request, so it recomputes
        // (`AlwaysRecompute`) — never the CLI's `ReuseIfComplete`. The GUI has
        // no preset layer; the override map below is not a preset — it carries
        // the user's own overrides (a hand value that its mirror does not
        // document).
        let target_luminance = self.recipe.auto_features.target_luminance;
        // AUTO-TONE-ENDPOINT-MIXED-7: the ONE place that decides which sliders
        // the user overrode is the shared `auto_tone_overrides`. It keeps every
        // key whose mirror does not document its effective value — an end point
        // value or a hand value set after the Auto-Tone run — so the collective
        // repair (`regenerate_stale` -> here) preserves it instead of
        // overwriting it, idempotently. Gated on an existing auto state so an
        // explicit Auto Tone on a fresh recipe still computes all six values
        // from the frame (a fresh recipe has no state to preserve).
        let overrides = self
            .recipe
            .auto_features
            .enable_auto_tone
            .then(|| lumina_stages::auto_tone::auto_tone_overrides(&self.recipe));
        let outcome = lumina_stages::auto_tone::apply_auto_tone_result(
            &mut self.recipe,
            &frame,
            target_luminance,
            lumina_stages::auto_tone::PersistedAutoTone::AlwaysRecompute,
            overrides.as_ref(),
        )?;
        debug_assert_eq!(
            outcome.source,
            lumina_stages::auto_tone::AutoToneSource::Computed,
            "an explicit Auto Tone press must recompute, never reuse"
        );
        // The save commit (GUI-AUTOTONE-SAVE-1) needs the exposure as the log
        // representative. Read it back through the shared key list rather than
        // keeping a fourth copy of "exposure is the first knob".
        let exposure = self
            .recipe
            .adjustments
            .get(lumina_stages::auto_tone::AUTO_TONE_ADJUSTMENT_KEYS[0])
            .copied()
            .unwrap_or_default();
        // GUI-AUTOTONE-SAVE-1: record the save commit so the debounced path
        // (`commit_pending_slider_save`) persists the sidecar (CAS, loud
        // conflicts) with an INFO log — same as GUI-SLIDER-SAVE-1. The
        // exposure is the log representative (contrast persists alongside).
        // GUI-SIDECAR-READ-1: commit synchronously (render + save + log) — a
        // bare `render()` would clear `pending_full_render` while the commit
        // stays armed, stranding the save until an unrelated later edit
        // (N6: `auto_tone saved` only fired via a later pan).
        self.mark_recipe_dirty("auto_tone", exposure);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    pub fn match_total_exposure(&mut self, target: f64) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::MatchExposure);
        // REVIEW-GUI-N5: never measure a draft. If the preview is currently a
        // low-resolution drag draft, commit the pending full-quality render
        // first so the measurement domain is the final visible render.
        if self.preview_is_draft {
            self.render_full([0, 0], None)?;
        }
        let Some(frame) = &self.preview else {
            return Ok(());
        };
        debug_assert!(
            !self.preview_is_draft,
            "measurement must run on the full render, never a draft"
        );
        // F-041: measure the final visible domain — the rendered preview
        // (post crop/geometry, same frame that is displayed) weighted by the
        // effective mask planes of the last render; the empty slice keeps the
        // raster measurement when no layers exist.
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
        // GUI-AUTOTONE-SAVE-1: record the save commit so the debounced path
        // (`commit_pending_slider_save`) persists the sidecar (CAS, loud
        // conflicts) with an INFO log — same as GUI-SLIDER-SAVE-1. Zoom/pan
        // view state is never recorded here; it stays GUI session state.
        // GUI-SIDECAR-READ-1: commit synchronously (see `auto_tone`) — a bare
        // `render()` would strand the armed commit (N6 lost-edit class).
        self.mark_recipe_dirty("match_total_exposure", value);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    pub fn reset(&mut self) {
        instrument_gui_action!(self, GuiAction::Reset);
        self.recipe = EditRecipe::default();
        // GUI-SIDECAR-READ-1: the recipe was replaced wholesale — a commit
        // armed by a pre-reset edit is stale (its `<key>=<value> saved` log
        // would misattribute the reset state). Drop it; the reset itself
        // re-renders below and persists on the next committed edit.
        self.pending_slider_commit = None;
        self.pending_history_step = None;
        self.pending_mask_state_before = None;
        if self.original.is_some() {
            // F4/F7: surface a render failure instead of discarding it.
            if let Err(error) = self.render() {
                self.show_error(error);
            }
        }
    }

    /// A **4x3-pixel** synthetic sample RGBA PNG for headless smoke / layout
    /// tests (F-103-N9); fixture class S2 in `feature/quality/golden-fixtures.md`
    /// — a Chrome-/Layout-Invariante source, never a render invariant. Pure
    /// helper with no app side effects; the bytes decode via [`Self::load_bytes`].
    pub fn sample_image_png() -> Vec<u8> {
        ImageFrame::new(
            4,
            3,
            vec![
                20, 30, 40, 255, 200, 180, 160, 255, 255, 255, 255, 255, 10, 10, 10, 255, 50, 60,
                70, 255, 90, 100, 110, 255, 120, 130, 140, 255, 200, 20, 20, 255, 1, 2, 3, 255, 80,
                90, 100, 255, 150, 160, 170, 255, 240, 240, 240, 255,
            ],
        )
        .expect("sample image dimensions are valid")
        .encode(ImageFileFormat::Png)
        .expect("sample image encodes to PNG")
    }

    /// Toggle Before/After. Deliberately does not touch the recipe.
    pub fn toggle_before_after(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleBeforeAfter);
        self.before_after = !self.before_after;
        trace!(
            "GUI interaction: toggle_before_after -> {}",
            self.before_after
        );
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
        self.local_wb_pick_mode = false;
        // GUI-SLIDER-SAVE-1: the eyedropper pick commits like a slider (both
        // fields persist; the temperature is the log representative).
        // GUI-SIDECAR-READ-1: commit synchronously — a bare `render()` would
        // clear `pending_full_render` while the commit stays armed, stranding
        // the save (same lost-edit class as `auto_tone` in N6). Render
        // failures stay loud via `show_error` inside the commit path.
        self.mark_recipe_dirty("wb_temperature", temp);
        self.status = "White balance set from picked point".into();
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// The eyedropper reads source pixels from the loaded frame.
    pub(crate) fn pick_white_balance_at(&mut self, nx: f64, ny: f64) {
        trace!(
            "GUI interaction: pick_white_balance_at nx={:.4} ny={:.4}",
            nx,
            ny
        );
        let Some(frame) = &self.original else {
            return;
        };
        let x = ((nx * frame.width() as f64) as u32).min(frame.width().saturating_sub(1));
        let y = ((ny * frame.height() as f64) as u32).min(frame.height().saturating_sub(1));
        let idx = ((y * frame.width() + x) * 4) as usize;
        let px = &frame.pixels()[idx..idx + 4];
        let r = px[0] as f64 / 255.0;
        let g = px[1] as f64 / 255.0;
        let b = px[2] as f64 / 255.0;
        if let Err(e) = self.set_white_balance_from_point(r, g, b) {
            error!("white balance pick failed at ({nx:.3},{ny:.3}): {e}");
            self.show_error(e);
        }
    }
}
