//! The mutually exclusive arming state machine of the preview's competing
//! input modes.
//!
//! White-balance eyedropper, local relative-WB picker, red-eye region picker,
//! spot tool and mask tool all consume the same preview click, so exactly one
//! of them may be armed. That invariant is a state machine, not five
//! independent setters, and it used to live inline in the app root — extracted
//! here so the exclusivity rule is readable in one place and the root file
//! keeps shrinking.
//!
//! Every arming path also ends a live mask gesture ([`LuminaApp::
//! clear_mask_gesture`]), so a stale in-progress brush can never intercept the
//! first click of the newly armed tool.
//!
//! Extracted verbatim from `lib.rs` (ratchet: the app root stays <= its
//! baseline). Only the visibility changed, from private to `pub(crate)`, because
//! the callers now live in other modules.

use super::*;
use log::info;

impl LuminaApp {
    /// G-14 (H1): arm the WB eyedropper and disarm the red-eye region picker.
    /// Both pickers consume the same preview click, so they are mutually
    /// exclusive — a single click must never sample a white balance *and* mark
    /// a pupil.
    pub(crate) fn arm_wb_picker(&mut self) {
        instrument_gui_action!(self, GuiAction::ArmWbEyedropper);
        self.wb_pick_mode = true;
        self.local_wb_pick_mode = false;
        self.red_eye_pick_mode = false;
        self.mask_tool = MaskTool::None;
        self.spot_tool = SpotTool::None;
        self.clear_mask_gesture();
        info!("GUI interaction: white-balance pick mode armed");
    }

    /// G-14 (H1): arm/disarm the red-eye region picker. Arming disarms the WB
    /// eyedropper (see [`Self::arm_wb_picker`]).
    pub(crate) fn set_red_eye_pick_mode(&mut self, armed: bool) {
        instrument_gui_action!(self, GuiAction::SetRedEyePickMode);
        self.red_eye_pick_mode = armed;
        if armed {
            self.wb_pick_mode = false;
            self.local_wb_pick_mode = false;
            self.mask_tool = MaskTool::None;
            self.spot_tool = SpotTool::None;
            self.clear_mask_gesture();
        }
        info!("GUI interaction: red-eye pick mode -> {armed}");
    }

    /// Disarm both preview pickers (image switch and `Esc`). The recipe and the
    /// persisted state are never touched.
    pub(crate) fn disarm_preview_pickers(&mut self) {
        self.wb_pick_mode = false;
        self.local_wb_pick_mode = false;
        self.red_eye_pick_mode = false;
    }

    /// `Esc` cancels an armed WB eyedropper / red-eye region picker and an
    /// armed spot tool (F-103-N3 / G-14 / SPOT). The recipe stays untouched.
    pub(crate) fn cancel_armed_preview_tools(&mut self) {
        self.disarm_preview_pickers();
        self.spot_tool = SpotTool::None;
    }

    /// `Esc` key wiring: read the frame's input and cancel the armed preview
    /// tools. Extracted so a headless test can drive the real key event.
    pub(crate) fn handle_escape_shortcut(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_armed_preview_tools();
        }
    }
}
