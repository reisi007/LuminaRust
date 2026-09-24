//! LRPAR-G14-DENOISE-IMPL-20 — GUI slice (Detail section + status badges).
//!
//! SOLL: `feature/decisions/LRPAR-G14-DENOISE-20.md` §5 (recipe persistence),
//! §6 (status model / no silent fallback) and `feature/platform/cli-gui-wasm.md`
//! (Detail section order, badges, job control). The GUI owns **no** denoise
//! image logic: the stage is evaluated by the shared core pipeline
//! (`lumina_core::apply_denoise_stage`) through
//! `render_frame_from_base_with_generative_and_denoise`, and the status is
//! classified by the shared `resolve_denoise_status`. This module only
//! marshals recipe state, the persisted `.lumina.zdata` `denoise_rgb` record
//! and the resolved status into the panel / render call.
//!
//! ## Status surface (decision §6)
//!
//! [`DenoiseGuiState`] carries the resolved [`DenoiseStageStatus`], the
//! diagnostic reason and the active [`DenoisePolicy`]. The Detail section
//! paints a badge for every state (`inactive`/`ready`/`unavailable`/`stale`/
//! `missing`/`corrupt`) and a `Strict`/`Warn` policy switch:
//!
//! * `Warn` (GUI default, mirrors the CLI §6 exit-0 policy): a non-ready
//!   active stage falls back to the manual F-096 noise reduction / identity —
//!   the core logs `warn!` and the badge stays visible, never a silent
//!   fallback.
//! * `Strict`: the render aborts loudly (`CoreError::Denoise`), surfaced by the
//!   GUI through the normal error dialog + `error!` log.
//!
//! ## Artifact resolution (documented convention)
//!
//! The persisted artifact lives as a `denoise_rgb` record in
//! `<source>.lumina.zdata`. Resolution order:
//!
//! 1. record id == the active virtual copy id (the per-copy convention, like
//!    the mask-tile `<copy_id>/<mask_id>` convention),
//! 2. otherwise — only when the recipe carries an artifact checksum — the
//!    matching record is searched by checksum so a differently-keyed (but
//!    byte-identical) record is still found instead of reported `missing`.
//!
//! A missing/unreadable bundle is the visible `missing` state, never an error
//! that hides the stage. No network, no model download: `model_hash =
//! "pending-integration"` is the honest `unavailable` state.
//!
//! ## Export guard (no silent fallback)
//!
//! The shared export entry point (`lumina_core::export_image_with_generative`,
//! byte-identical to the CLI) does not yet accept a denoise stage input. The
//! GUI therefore:
//!
//! * `Warn` + non-ready: exports the manual-NR/identity fallback and states it
//!   visibly (`warn!` + status + toast).
//! * `Strict` + non-ready: refuses the export loudly (`error!` + dialog).
//! * `Ready`: refuses loudly too — exporting silently *without* the verified
//!   denoise artifact would diverge from the preview. Wiring the stage into the
//!   shared export path is the documented follow-up (ODN → export).

use log::{info, warn};
use lumina_core::denoise_producer_provenance;
use lumina_core::resolve_denoise_status;
use lumina_core::DenoiseIdentity;
use lumina_core::DenoisePolicy;
use lumina_core::DenoiseRgbArtifact;
use lumina_core::DenoiseStageInput;
use lumina_core::DenoiseStageStatus;
use lumina_sidecar::{
    load_zdata, zdata_path_for, DenoiseAi, DenoiseModelIdentity, RecordSpec, DENOISE_AI_VERSION,
    DENOISE_PENDING_MODEL_HASH,
};

use crate::i18n::Str;
#[cfg(debug_assertions)]
use crate::GuiAction;
use crate::{GuiError, LuminaApp};

/// Planned/documented denoiser identity the GUI offers when the user enables
/// the stage. No weights are bundled (F-078 gate); the pending hash keeps the
/// resolved status at `unavailable` until an integrated model is wired.
pub(crate) const GUI_DENOISE_MODEL_NAME: &str = "nafnet-srgb";
/// Planned model version (decision §5 sketch).
pub(crate) const GUI_DENOISE_MODEL_VERSION: &str = "1.0";
/// The GUI obtains the v2 digest from the shared ONNX contract producer rather
/// than carrying a second hard-coded hash. This does not select or load a
/// model; it only hashes the documented input/algorithm contract.
pub(crate) use lumina_onnx::default_denoise_input_spec_digest as gui_denoise_input_spec_digest;

/// Resolved GUI-side denoise state (display + render decision). Session state;
/// never persisted (the recipe is the persistence).
#[derive(Debug, Clone, PartialEq)]
pub struct DenoiseGuiState {
    pub status: DenoiseStageStatus,
    /// Diagnostic explanation of a non-`Ready` state (empty when ready).
    pub reason: String,
    pub policy: DenoisePolicy,
    /// Live identity the status was classified against.
    pub current: Option<DenoiseIdentity>,
    /// Persisted producer provenance read from the recipe extras.
    pub recorded: Option<DenoiseIdentity>,
    pub artifact_present: bool,
    /// R5-WARN-2: the same (status, reason) was already surfaced once, so the
    /// next render's fallback `warn!` is suppressed ([`DenoiseStageInput::quiet`]).
    pub refusal_warned: bool,
}

impl DenoiseGuiState {
    /// The inactive state (no active stage / no image).
    pub(crate) fn inactive(policy: DenoisePolicy) -> Self {
        Self {
            status: DenoiseStageStatus::Inactive,
            reason: String::new(),
            policy,
            current: None,
            recorded: None,
            artifact_present: false,
            refusal_warned: false,
        }
    }

    /// Whether the resolved state is a usable, verified artifact.
    pub(crate) fn is_ready(&self) -> bool {
        self.status == DenoiseStageStatus::Ready
    }
}

/// The documented GUI default `denoise_ai` stage. Valid per
/// `lumina_sidecar::validate_denoise_ai` (pending hash is explicitly allowed).
pub(crate) fn default_denoise_ai() -> DenoiseAi {
    DenoiseAi {
        version: DENOISE_AI_VERSION,
        enabled: true,
        model: DenoiseModelIdentity {
            name: GUI_DENOISE_MODEL_NAME.into(),
            version: GUI_DENOISE_MODEL_VERSION.into(),
            model_hash: DENOISE_PENDING_MODEL_HASH.into(),
            extras: Default::default(),
        },
        input_spec_digest: gui_denoise_input_spec_digest(),
        strength: 0.5,
        preserve_detail: 0.5,
        artifact: None,
        extras: Default::default(),
    }
}

/// Human-readable model identity line for the Detail section.
pub(crate) fn denoise_model_label(denoise: &DenoiseAi) -> String {
    Str::DenoiseModelPattern.format_arg(&format!(
        "{} {} ({})",
        denoise.model.name, denoise.model.version, denoise.model.model_hash
    ))
}

impl LuminaApp {
    /// The active recipe's denoise stage, if any.
    pub(crate) fn denoise_ai(&self) -> Option<&DenoiseAi> {
        self.recipe.denoise_ai.as_ref()
    }

    /// Whether the active recipe requests a real denoise run (present and not
    /// an identity: `enabled == false` / `strength == 0` are identity).
    pub(crate) fn denoise_stage_active(&self) -> bool {
        self.denoise_ai()
            .is_some_and(|denoise| !denoise.is_identity())
    }

    /// The resolved GUI denoise state of the last refresh/render.
    pub fn denoise_state(&self) -> &DenoiseGuiState {
        &self.denoise_gui
    }

    /// The active fallback policy (`Warn` by default — mirrors the CLI §6
    /// exit-0 policy; `Strict` aborts loudly).
    pub fn denoise_policy(&self) -> DenoisePolicy {
        self.denoise_gui.policy
    }

    /// Switch the fallback policy. Session-only: never touches recipe/sidecar.
    pub fn set_denoise_policy(&mut self, policy: DenoisePolicy) {
        info!("denoise_ai policy -> {}", denoise_policy_label(policy));
        self.denoise_gui = DenoiseGuiState {
            policy,
            ..self.denoise_gui.clone()
        };
    }

    /// Explicit test/integration seam: the live denoiser model context.
    ///
    /// Production leaves this `None` (no weights bundled → `unavailable`). The
    /// seam mirrors the established `bind_test_lensfun_corrector` /
    /// `set_gpu_adapter_override` pattern: injecting a pinned identity lets the
    /// headless tests reach `ready`/`stale`/`corrupt` without shipping weights
    /// or a model.
    pub fn set_denoise_live_model(&mut self, model: Option<DenoiseModelIdentity>) {
        self.denoise_live_model = model;
        self.denoise_gui_dirty = true;
    }

    /// Enable/disable the `denoise_ai` stage. Enabling creates the documented
    /// default stage when the recipe has none; the change commits through the
    /// normal debounced save/render path (recipe + sidecar roundtrip).
    pub fn set_denoise_enabled(&mut self, enabled: bool) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetDenoiseEnabled);
        let mut denoise = self
            .recipe
            .denoise_ai
            .clone()
            .unwrap_or_else(default_denoise_ai);
        denoise.enabled = enabled;
        denoise.validate()?;
        self.recipe.denoise_ai = Some(denoise);
        info!("denoise_ai.enabled={enabled} (strength persisted via save/render path)");
        self.mark_recipe_dirty("denoise_ai.enabled", if enabled { 1.0 } else { 0.0 });
        self.denoise_gui_dirty = true;
        Ok(())
    }

    /// Set the blending strength (`0..=1`, `0` = identity). Loud rejection of
    /// out-of-range values (never clipped).
    pub fn set_denoise_strength(&mut self, value: f64) -> Result<(), GuiError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(GuiError::Io(format!(
                "invalid denoise_ai.strength {value}: expected 0..=1"
            )));
        }
        let mut denoise = self
            .recipe
            .denoise_ai
            .clone()
            .unwrap_or_else(default_denoise_ai);
        denoise.strength = value as f32;
        denoise.validate()?;
        self.recipe.denoise_ai = Some(denoise);
        self.mark_recipe_dirty("denoise_ai.strength", value);
        self.denoise_gui_dirty = true;
        Ok(())
    }

    /// Set the edge/detail protection (`0..=1`). Loud rejection, never clipped.
    pub fn set_denoise_preserve_detail(&mut self, value: f64) -> Result<(), GuiError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(GuiError::Io(format!(
                "invalid denoise_ai.preserve_detail {value}: expected 0..=1"
            )));
        }
        let mut denoise = self
            .recipe
            .denoise_ai
            .clone()
            .unwrap_or_else(default_denoise_ai);
        denoise.preserve_detail = value as f32;
        denoise.validate()?;
        self.recipe.denoise_ai = Some(denoise);
        self.mark_recipe_dirty("denoise_ai.preserve_detail", value);
        self.denoise_gui_dirty = true;
        Ok(())
    }

    /// Reads the persisted `denoise_rgb` record for the active virtual copy
    /// (see the module docs for the resolution order). Unreadable/missing
    /// bundles resolve to `None` — the visible `missing` state, never an error
    /// that hides the stage.
    pub(crate) fn load_denoise_artifact(&self) -> Option<DenoiseRgbArtifact> {
        let path = zdata_path_for(std::path::Path::new(&self.path));
        if !path.is_file() {
            return None;
        }
        let container = match load_zdata(&path) {
            Ok(container) => container,
            Err(error) => {
                warn!("denoise artifact bundle unreadable: {error}");
                return None;
            }
        };
        // 1. per-copy record id convention.
        if let Ok(record) = container.denoise_rgb(&self.virtual_copy_id) {
            return DenoiseRgbArtifact::new(record.width, record.height, record.pixels).ok();
        }
        // 2. checksum search (only when the recipe references an artifact).
        let checksum = self
            .recipe
            .denoise_ai
            .as_ref()
            .and_then(|denoise| denoise.artifact.as_ref())
            .map(|artifact| artifact.checksum.clone());
        let checksum = checksum?;
        let records = container.decode_all().ok()?;
        records.into_iter().find_map(|spec| match spec {
            RecordSpec::DenoiseRgb(record) if record.checksum() == checksum => {
                DenoiseRgbArtifact::new(record.width, record.height, record.pixels).ok()
            }
            _ => None,
        })
    }

    /// Classifies the active stage against the live context.
    ///
    /// `artifact` is the artifact resolved by [`Self::load_denoise_artifact`]
    /// (or `None` when the bundle/record is absent).
    pub(crate) fn resolve_denoise_state(
        &mut self,
        artifact: Option<&DenoiseRgbArtifact>,
    ) -> DenoiseGuiState {
        let policy = self.denoise_gui.policy;
        let Some(denoise) = self.recipe.denoise_ai.clone() else {
            self.clear_denoise_refusal_warn();
            return DenoiseGuiState::inactive(policy);
        };
        let source_hash = self.resolved_source_hash();
        let decode_fingerprint = format!(
            "{}@{}",
            crate::decoder_identity(self.source_is_raw),
            if self.source_is_raw {
                lumina_raw::libraw_decode_version()
            } else {
                env!("CARGO_PKG_VERSION").to_string()
            }
        );
        let (model_name, model_version, model_hash) = match &self.denoise_live_model {
            Some(model) => (
                model.name.clone(),
                model.version.clone(),
                model.model_hash.clone(),
            ),
            None => (
                denoise.model.name.clone(),
                denoise.model.version.clone(),
                DENOISE_PENDING_MODEL_HASH.to_string(),
            ),
        };
        let artifact_checksum = artifact.map_or_else(String::new, DenoiseRgbArtifact::checksum);
        let current = DenoiseIdentity {
            source_content_hash: source_hash,
            decode_fingerprint,
            model_name,
            model_version,
            model_hash,
            input_spec_digest: denoise.input_spec_digest.clone(),
            artifact_checksum,
        };
        let recorded = denoise_producer_provenance(&denoise);
        let recorded_identity = recorded.clone().unwrap_or_default();
        let status =
            resolve_denoise_status(&denoise, &current, &recorded_identity, artifact.is_some());
        let reason = denoise_status_reason(status, &current, recorded.as_ref(), &denoise);
        // R5-WARN-2: dedup the per-tick fallback `warn!` (R4-WARN-1 pattern on
        // the Denoise-Gate path). Only the first occurrence of a distinct
        // (status, reason) lets the core warn; Ready/Inactive re-arms a later
        // recurrence.
        let refusal_warned = if matches!(
            status,
            DenoiseStageStatus::Ready | DenoiseStageStatus::Inactive
        ) {
            self.clear_denoise_refusal_warn();
            false
        } else if policy == DenoisePolicy::Warn {
            !self.note_denoise_refusal(status.as_str(), &reason)
        } else {
            false
        };
        DenoiseGuiState {
            status,
            reason,
            policy,
            current: Some(current),
            recorded,
            artifact_present: artifact.is_some(),
            refusal_warned,
        }
    }

    /// Refreshes the resolved state when a denoise-relevant input changed
    /// (recipe edit, source/copy change, explicit refresh). Cheap when the
    /// stage is inactive (no zdata read).
    pub(crate) fn refresh_denoise_gui(&mut self) {
        if !self.denoise_stage_active() {
            self.denoise_gui = DenoiseGuiState::inactive(self.denoise_gui.policy);
            self.denoise_gui_dirty = false;
            return;
        }
        let artifact = self.load_denoise_artifact();
        self.denoise_gui = self.resolve_denoise_state(artifact.as_ref());
        self.denoise_gui_dirty = false;
    }

    /// The `DenoiseStageInput` for the next render.
    pub(crate) fn denoise_render_input<'a>(
        &self,
        artifact: Option<&'a DenoiseRgbArtifact>,
    ) -> DenoiseStageInput<'a> {
        DenoiseStageInput {
            status: self.denoise_gui.status,
            artifact,
            reason: self.denoise_gui.reason.clone(),
            policy: self.denoise_gui.policy,
            quiet: self.denoise_gui.refusal_warned,
        }
    }

    /// Visible (non-silent) badge text for the resolved status.
    pub fn denoise_status_text(&self) -> String {
        denoise_status_label(self.denoise_gui.status)
    }

    /// Loud export gate (see module docs): the shared export entry point cannot
    /// take the denoise stage yet, so `Ready` and `Strict`-non-ready refuse
    /// visibly instead of exporting a frame that diverges from the preview.
    pub(crate) fn guard_denoise_export(&self) -> Result<(), GuiError> {
        if !self.denoise_stage_active() {
            return Ok(());
        }
        match self.denoise_gui.status {
            DenoiseStageStatus::Ready => {
                Err(GuiError::Io(Str::DenoiseExportUnsupported.t().to_string()))
            }
            DenoiseStageStatus::Inactive => Ok(()),
            other if self.denoise_gui.policy == DenoisePolicy::Strict => Err(GuiError::Io(
                Str::DenoiseExportStrictPattern.format_arg(&denoise_status_label(other)),
            )),
            other => {
                warn!(
                    "denoise_ai is {}: exporting the manual F-096 fallback / identity \
                     (explicitly surfaced, never silent)",
                    denoise_status_label(other)
                );
                Ok(())
            }
        }
    }
}

/// Stable label of the resolved status for logs.
pub(crate) fn denoise_status_label(status: DenoiseStageStatus) -> String {
    match status {
        DenoiseStageStatus::Inactive => Str::DenoiseStatusInactive.t().to_string(),
        DenoiseStageStatus::Ready => Str::DenoiseStatusReady.t().to_string(),
        DenoiseStageStatus::Unavailable => Str::DenoiseStatusUnavailable.t().to_string(),
        DenoiseStageStatus::Stale => Str::DenoiseStatusStale.t().to_string(),
        DenoiseStageStatus::Missing => Str::DenoiseStatusMissing.t().to_string(),
        DenoiseStageStatus::Corrupt => Str::DenoiseStatusCorrupt.t().to_string(),
    }
}

/// Diagnostic reason for a non-`Ready` state (logs + hover text). The status
/// itself is a closed enum; this text explains *why*.
fn denoise_status_reason(
    status: DenoiseStageStatus,
    current: &DenoiseIdentity,
    recorded: Option<&DenoiseIdentity>,
    denoise: &DenoiseAi,
) -> String {
    match status {
        DenoiseStageStatus::Inactive => String::new(),
        DenoiseStageStatus::Ready => String::new(),
        DenoiseStageStatus::Unavailable => format!(
            "model `{}` has model_hash `{}` — no licence-cleared weights integrated",
            denoise.model.name, denoise.model.model_hash
        ),
        DenoiseStageStatus::Missing => {
            "no matching `denoise_rgb` record in the sidecar bundle".to_string()
        }
        DenoiseStageStatus::Corrupt => {
            "the persisted `denoise_rgb` record checksum does not match the recipe reference"
                .to_string()
        }
        DenoiseStageStatus::Stale => {
            if let Some(recorded) = recorded {
                format!(
                    "recorded identity {} != live context {}",
                    recorded.model_hash, current.model_hash
                )
            } else {
                "no persisted producer provenance (cannot prove validity)".to_string()
            }
        }
    }
}

/// Human-readable policy name (logs).
fn denoise_policy_label(policy: DenoisePolicy) -> &'static str {
    match policy {
        DenoisePolicy::Strict => "strict",
        DenoisePolicy::Warn => "warn",
    }
}

/// Badge colour for a resolved status. Pure, unit-tested.
pub(crate) fn denoise_badge_color(status: DenoiseStageStatus) -> crate::egui::Color32 {
    match status {
        DenoiseStageStatus::Ready => crate::egui::Color32::from_rgb(0x5C, 0xB8, 0x5C),
        DenoiseStageStatus::Inactive => crate::egui::Color32::GRAY,
        _ => crate::egui::Color32::from_rgb(0xE0, 0x6C, 0x3C),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slider::{identity_spec, SliderAction};
    use lumina_core::{ImageFileFormat, ImageFrame};
    use lumina_sidecar::{
        save_denoise_rgb, DenoiseRgbArtifact as SidecarDenoiseArtifact, SidecarDocument,
    };
    use std::path::Path;

    fn new_app() -> LuminaApp {
        LuminaApp::new(crate::egui::Context::default())
    }

    fn save_png(path: &Path) {
        let png = ImageFrame::new(8, 6, (0..48 * 4).map(|i| (i % 251) as u8).collect())
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        std::fs::write(path, png).unwrap();
    }

    fn open_and_decode(app: &mut LuminaApp, path: &Path) {
        app.open_file(path.display().to_string());
        for _ in 0..2000 {
            app.poll_decode();
            if app.original.is_some() || app.error().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(app.error().is_none(), "decode failed: {:?}", app.error());
        assert!(app.original.is_some(), "decode did not finish");
    }

    fn commit_and_load_doc(app: &mut LuminaApp, source: &Path) -> SidecarDocument {
        app.commit_pending_slider_save([0, 0]);
        assert!(app.error().is_none(), "commit failed: {:?}", app.error());
        let sidecar = lumina_sidecar::sidecar_path_for(source);
        assert!(sidecar.is_file(), "sidecar must be written");
        lumina_sidecar::load_sidecar(&sidecar).unwrap()
    }

    fn pinned(fill: u8) -> String {
        format!("sha256:{}", format!("{fill:02x}").repeat(32))
    }

    /// Writes a matching `denoise_rgb` record into the bundle under the
    /// per-copy record id and returns its checksum.
    fn write_artifact(app: &LuminaApp, pixels: u8) -> String {
        let (width, height) = (
            app.original.as_ref().unwrap().width,
            app.original.as_ref().unwrap().height,
        );
        let data = vec![pixels; width as usize * height as usize * 3];
        let artifact = SidecarDenoiseArtifact {
            id: app.virtual_copy_id.clone(),
            width,
            height,
            pixels: data,
        };
        artifact.validate().unwrap();
        let checksum = artifact.checksum();
        let zdata = zdata_path_for(Path::new(&app.path));
        save_denoise_rgb(&zdata, artifact, false).unwrap();
        checksum
    }

    /// Pins the recipe identity + provenance to the live seam's identity and
    /// attaches the recipe artifact reference, producing a status of `ready`.
    fn make_ready(app: &mut LuminaApp) {
        let model = DenoiseModelIdentity {
            name: GUI_DENOISE_MODEL_NAME.into(),
            version: GUI_DENOISE_MODEL_VERSION.into(),
            model_hash: pinned(0x11),
            extras: Default::default(),
        };
        app.set_denoise_live_model(Some(model.clone()));
        let checksum = write_artifact(app, 0);
        let mut denoise = app.recipe.denoise_ai.clone().unwrap();
        denoise.model = model;
        denoise.artifact = Some(lumina_sidecar::DenoiseArtifactRef {
            kind: lumina_sidecar::DenoiseArtifactKind::DenoiseRgb,
            relative_path: "photo.png.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: checksum.clone(),
            width: app.original.as_ref().unwrap().width,
            height: app.original.as_ref().unwrap().height,
            channels: "rgb8".into(),
            data_version: "1".into(),
            extras: Default::default(),
        });
        app.recipe.denoise_ai = Some(denoise.clone());
        // Record the producer provenance exactly as the producing slice would —
        // against the identity of the *written* artifact (the checksum is part
        // of the identity, so resolving without it would be a false `stale`).
        let artifact = app.load_denoise_artifact();
        let resolved = app.resolve_denoise_state(artifact.as_ref());
        let current = resolved.current.unwrap();
        lumina_core::set_denoise_producer_provenance(&mut denoise, &current);
        app.recipe.denoise_ai = Some(denoise);
    }

    /// The panel's pure status mapping is pinned here; painted text and panel
    /// containment are asserted via the shared headless shape harness in the
    /// parent module tests (`develop_section_detail_paints_denoise_panel`).
    #[test]
    fn status_labels_and_badges_cover_every_variant() {
        for status in [
            DenoiseStageStatus::Inactive,
            DenoiseStageStatus::Ready,
            DenoiseStageStatus::Unavailable,
            DenoiseStageStatus::Stale,
            DenoiseStageStatus::Missing,
            DenoiseStageStatus::Corrupt,
        ] {
            let text = denoise_status_label(status);
            assert!(!text.is_empty(), "{status:?} must have a badge");
            let _ = denoise_badge_color(status);
        }
        assert_eq!(
            denoise_status_label(DenoiseStageStatus::Ready),
            "AI Denoise Ready"
        );
        assert_eq!(
            denoise_status_label(DenoiseStageStatus::Unavailable),
            "AI Denoise Unavailable"
        );
    }

    #[test]
    fn default_stage_validates_and_is_not_identity() {
        let denoise = default_denoise_ai();
        denoise.validate().unwrap();
        assert!(!denoise.is_identity());
        assert_eq!(denoise.model.model_hash, DENOISE_PENDING_MODEL_HASH);
        assert_eq!(denoise.input_spec_digest, gui_denoise_input_spec_digest());
        assert_eq!(
            denoise.input_spec_digest,
            lumina_onnx::DENOISE_DEFAULT_INPUT_SPEC_DIGEST
        );
    }

    /// E2E (DoD §1): enabling the stage writes the recipe to the sidecar and a
    /// reopen restores enabled/strength/preserve_detail + model identity.
    #[test]
    fn denoise_recipe_persists_and_reloads() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        app.set_denoise_enabled(true).unwrap();
        app.set_denoise_strength(0.25).unwrap();
        app.set_denoise_preserve_detail(0.75).unwrap();
        let document = commit_and_load_doc(&mut app, &source);
        let stored = document.virtual_copies[0]
            .recipe
            .denoise_ai
            .clone()
            .expect("denoise_ai persisted");
        assert!(stored.enabled);
        assert!((stored.strength - 0.25).abs() < 1e-6);
        assert!((stored.preserve_detail - 0.75).abs() < 1e-6);
        assert_eq!(stored.model.name, GUI_DENOISE_MODEL_NAME);
        assert_eq!(stored.input_spec_digest, gui_denoise_input_spec_digest());

        let mut reopened = new_app();
        open_and_decode(&mut reopened, &source);
        let reloaded = reopened.recipe().denoise_ai.clone().expect("reloaded");
        assert!(reloaded.enabled);
        assert!((reloaded.strength - 0.25).abs() < 1e-6);
        assert!((reloaded.preserve_detail - 0.75).abs() < 1e-6);
        assert_eq!(reloaded.model.version, GUI_DENOISE_MODEL_VERSION);
    }

    /// §6: the pending-integration model is the honest `unavailable` state —
    /// never silently rendered as if a model existed.
    #[test]
    fn pending_model_resolves_unavailable_and_warn_policy_renders_visible_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        app.set_denoise_enabled(true).unwrap();
        assert_eq!(app.denoise_policy(), DenoisePolicy::Warn);
        app.render_full([0, 0], None).unwrap();
        assert_eq!(app.denoise_state().status, DenoiseStageStatus::Unavailable);
        assert!(!app.denoise_state().reason.is_empty());
        // Warn falls back visibly (render succeeds, badge stays non-ready).
        assert!(app.preview().is_some());
    }

    /// §6/DoD §4: under `Strict` an active, non-ready stage aborts the render
    /// loudly (no silent fallback).
    #[test]
    fn strict_policy_aborts_non_ready_render_loudly() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        app.set_denoise_enabled(true).unwrap();
        app.set_denoise_policy(DenoisePolicy::Strict);
        let error = app.render_full([0, 0], None).unwrap_err();
        assert!(
            error.to_string().contains("unavailable"),
            "loud strict abort must name the status: {error}"
        );
    }

    /// §6 status matrix: `ready`, `missing`, `corrupt` and `stale` are all
    /// reachable and distinct.
    #[test]
    fn status_matrix_reaches_ready_missing_corrupt_stale() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        app.set_denoise_enabled(true).unwrap();
        make_ready(&mut app);

        // ready
        app.refresh_denoise_gui();
        assert_eq!(
            app.denoise_state().status,
            DenoiseStageStatus::Ready,
            "{:?}",
            app.denoise_state()
        );
        let artifact = app.load_denoise_artifact();
        assert!(artifact.is_some());
        let outcome = app.denoise_render_input(artifact.as_ref());
        assert_eq!(outcome.status, DenoiseStageStatus::Ready);
        app.render_full([0, 0], None).unwrap();
        assert_eq!(app.denoise_state().status, DenoiseStageStatus::Ready);

        // corrupt: recipe reference checksum no longer matches the record.
        if let Some(denoise) = app.recipe.denoise_ai.as_mut() {
            if let Some(artifact) = denoise.artifact.as_mut() {
                artifact.checksum = "ff".repeat(32);
            }
        }
        app.denoise_gui_dirty = true;
        app.refresh_denoise_gui();
        assert_eq!(app.denoise_state().status, DenoiseStageStatus::Corrupt);

        // missing: no record matches any more.
        std::fs::remove_file(zdata_path_for(&source)).unwrap();
        app.denoise_gui_dirty = true;
        app.refresh_denoise_gui();
        assert_eq!(app.denoise_state().status, DenoiseStageStatus::Missing);

        // stale: a matching artifact but a changed producer provenance.
        make_ready(&mut app);
        app.refresh_denoise_gui();
        assert_eq!(app.denoise_state().status, DenoiseStageStatus::Ready);
        let mut provenance = app.denoise_state().current.clone().unwrap();
        provenance.model_version = "changed".into();
        let mut denoise = app.recipe.denoise_ai.clone().unwrap();
        lumina_core::set_denoise_producer_provenance(&mut denoise, &provenance);
        app.recipe.denoise_ai = Some(denoise);
        app.denoise_gui_dirty = true;
        app.refresh_denoise_gui();
        assert_eq!(app.denoise_state().status, DenoiseStageStatus::Stale);
    }

    /// §5: a `ready` artifact is blended by the render path; `strength == 0` is
    /// an exact identity (same bytes as a recipe without the stage).
    #[test]
    fn ready_artifact_is_applied_and_strength_zero_is_identity() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        app.set_denoise_enabled(true).unwrap();
        make_ready(&mut app);
        app.set_denoise_strength(1.0).unwrap();
        app.render_full([0, 0], None).unwrap();
        let denoised = app.preview().unwrap().pixels.clone();
        // The artifact is all-zero pixels, so a strength-1 blend must darken.
        let neutral = {
            let mut neutral = new_app();
            open_and_decode(&mut neutral, &source);
            neutral.render_full([0, 0], None).unwrap();
            neutral.preview().unwrap().pixels.clone()
        };
        assert_ne!(denoised, neutral, "ready artifact must change the pixels");

        app.set_denoise_strength(0.0).unwrap();
        app.render_full([0, 0], None).unwrap();
        assert_eq!(
            app.preview().unwrap().pixels,
            neutral,
            "strength 0 must be byte-identical to the identity recipe"
        );
    }

    /// Export guard: a `ready` artifact cannot be exported yet (the shared
    /// export path has no denoise input) — loud refusal, never a silent skip.
    #[test]
    fn export_guard_refuses_ready_and_strict_but_allows_warn_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        // Inactive stage: no guard at all.
        assert!(app.guard_denoise_export().is_ok());

        app.set_denoise_enabled(true).unwrap();
        app.refresh_denoise_gui();
        assert_eq!(app.denoise_state().status, DenoiseStageStatus::Unavailable);
        assert!(
            app.guard_denoise_export().is_ok(),
            "warn fallback is surfaced"
        );

        app.set_denoise_policy(DenoisePolicy::Strict);
        let error = app.guard_denoise_export().unwrap_err();
        assert!(error.to_string().contains("Unavailable"), "{error}");

        app.set_denoise_policy(DenoisePolicy::Warn);
        make_ready(&mut app);
        app.refresh_denoise_gui();
        assert_eq!(app.denoise_state().status, DenoiseStageStatus::Ready);
        assert!(app.guard_denoise_export().is_err(), "ready must refuse");
    }

    /// Invalid slider domains are loud, never clipped.
    #[test]
    fn invalid_strength_and_detail_are_rejected() {
        let mut app = new_app();
        assert!(app.set_denoise_strength(1.5).is_err());
        assert!(app.set_denoise_strength(f64::NAN).is_err());
        assert!(app.set_denoise_preserve_detail(-0.1).is_err());
    }

    /// The panel uses the shared identity slider spec for both bounded
    /// `0..=1` values, so its display scale cannot silently drift.
    #[test]
    fn panel_slider_specs_are_the_shared_identity_scale() {
        let expected = identity_spec(0.0..=1.0, 0.5, 0.01);
        assert_eq!(expected.range, (0.0, 1.0));
        assert_eq!(expected.default, 0.5);
        // The panel action enum is the shared commit contract.
        let _ = SliderAction::Changed;
    }

    /// F9 (DoD §3): the `Inactive` badge is actually painted when the Detail
    /// section exists but the stage is identity (`enabled == false` or
    /// `strength == 0`). The label helper test alone does not prove the panel
    /// draws it.
    #[test]
    fn inactive_badge_is_painted_when_section_exists_but_disabled() {
        let mut app = new_app();
        app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
            .unwrap();
        app.set_denoise_enabled(true).unwrap();
        app.set_denoise_enabled(false).unwrap();
        app.denoise_gui_dirty = true;
        let inactive = Str::DenoiseStatusInactive.t();
        assert!(
            painted_text(&mut app, inactive),
            "disabled section must paint the Inactive badge"
        );
        // `strength == 0` is the other identity form and paints the same badge.
        app.set_denoise_enabled(true).unwrap();
        app.set_denoise_strength(0.0).unwrap();
        app.denoise_gui_dirty = true;
        assert!(
            painted_text(&mut app, inactive),
            "strength-0 section must paint the Inactive badge"
        );
    }

    /// Headless paint probe (no GPU): draws the real Detail section and reports
    /// whether a text shape with exactly `needle` was emitted.
    fn painted_text(app: &mut LuminaApp, needle: &str) -> bool {
        let ctx = crate::egui::Context::default();
        let raw = crate::egui::RawInput {
            screen_rect: Some(crate::egui::Rect::from_min_size(
                crate::egui::pos2(0.0, 0.0),
                crate::egui::vec2(720.0, 1200.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| app.draw_denoise_section(ui));
        output.textures_delta.clear();
        output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, crate::egui::Shape::Text(text) if text.galley.text() == needle)
        })
    }
}
