//! Crash-Fix Runde 2 (F5): the draft-render failure report — the per-frame
//! dedup of an identical `error!` message, the one-time repeat `warn!` and the
//! headless capture seam that proves both. Moved out of `render_tick.rs`; the
//! moved code is byte-identical, only the `error!` import follows it here.
//! *Reporting* a failed draft render is a concern of its own next to the
//! render it reports on, so it moves out together with its tests.

use super::*;
use log::error;

impl LuminaApp {
    /// Crash-Fix Runde 2 (F5): surface a draft-render failure without spamming
    /// the log or re-arming the dialog once per frame. The first occurrence of a
    /// message is loud (`error!` + "Error" status + dialog); a repeated
    /// identical message is downgraded to a single one-time `warn!` and leaves
    /// status, message and dialog state untouched — so a dialog the user closed
    /// stays closed and a persistent failure does not flood the log. Nothing is
    /// lost silently: the first message and the status stay visible.
    pub(crate) fn show_draft_error(&mut self, error: impl ToString) {
        let message = error.to_string();
        if self.draft_error_dedup.as_deref() == Some(message.as_str()) {
            if !self.draft_error_repeat_warned {
                warn!("draft render failed again (same message, not repeated): {message}");
                self.draft_error_repeat_warned = true;
                #[cfg(test)]
                DRAFT_ERROR_REPEAT_WARNS.with(|warns| warns.set(warns.get() + 1));
            }
            return;
        }
        error!("{message}");
        #[cfg(test)]
        DRAFT_ERRORS.with(|log| log.borrow_mut().push(message.clone()));
        self.draft_error_dedup = Some(message.clone());
        self.draft_error_repeat_warned = false;
        self.status = Str::Error.t().into();
        self.error = Some(message);
        self.error_dialog = true;
    }

    /// Crash-Fix Runde 2 (F5): a successful draft tick forgets the last
    /// failure, so the same failure recurring after a recovery is surfaced
    /// loudly again (no permanent suppression).
    pub(crate) fn clear_draft_error_dedup(&mut self) {
        self.draft_error_dedup = None;
        self.draft_error_repeat_warned = false;
    }
}

// Crash-Fix Runde 2 (F5): thread-local capture of the `error!` lines emitted by
// [`LuminaApp::show_draft_error`], so headless tests can prove the per-frame
// dedup (same capture seam pattern as `jank_log`).
#[cfg(test)]
thread_local! {
    static DRAFT_ERRORS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static DRAFT_ERROR_REPEAT_WARNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Drains and returns the captured draft-error lines of the current test thread.
#[cfg(test)]
pub(crate) fn take_draft_error_log() -> Vec<String> {
    DRAFT_ERRORS.with(|log| std::mem::take(&mut *log.borrow_mut()))
}

/// Drains and returns the number of one-time repeat `warn!` emissions of the
/// current test thread.
#[cfg(test)]
pub(crate) fn take_draft_error_repeat_warns() -> u32 {
    DRAFT_ERROR_REPEAT_WARNS.with(|warns| warns.replace(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }

    fn app() -> LuminaApp {
        let mut app = LuminaApp::new(egui::Context::default());
        app.load_bytes(png(), "draft-error-test.png").unwrap();
        app
    }

    /// F5: three identical draft errors produce exactly one `error!` line.
    #[test]
    fn repeated_identical_draft_errors_emit_one_error_line() {
        let mut app = app();
        app.show_draft_error("draft failed");
        app.show_draft_error("draft failed");
        app.show_draft_error("draft failed");
        assert_eq!(take_draft_error_log(), vec!["draft failed"]);
        assert_eq!(
            take_draft_error_repeat_warns(),
            1,
            "a persistent failure must warn about the repeat exactly once, not per tick"
        );
        assert_eq!(app.error(), Some("draft failed"));
        assert!(app.error_dialog_open());
    }

    /// F5: a repeat does not re-arm a dialog the user closed; a new message is
    /// loud again.
    #[test]
    fn repeated_draft_error_does_not_rearm_a_closed_dialog() {
        let mut app = app();
        app.show_draft_error("draft failed");
        assert!(app.error_dialog_open());
        let _ = take_draft_error_log();
        app.close_error_dialog();
        app.show_draft_error("draft failed");
        assert!(
            !app.error_dialog_open(),
            "a repeated message must not re-open the dialog"
        );
        assert!(
            take_draft_error_log().is_empty(),
            "a repeated message must not emit a second error! line"
        );
        app.show_draft_error("other failure");
        assert!(app.error_dialog_open(), "a new message must be surfaced");
        assert_eq!(take_draft_error_log(), vec!["other failure"]);
    }

    /// F5 end-to-end: driving the real draft tick with a recipe that every
    /// render rejects (positive shadows on the (0,0) endpoint) logs once.
    #[test]
    fn failing_draft_ticks_dedup_the_error() {
        let mut app = app();
        app.render().unwrap();
        app.recipe.curves = Some(Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.5,
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ],
            channels: CurveChannels::default(),
        });
        for _ in 0..3 {
            app.render_draft_tick([64, 48]);
        }
        assert_eq!(
            take_draft_error_log().len(),
            1,
            "three failing draft ticks must log the error exactly once"
        );
        assert!(app.error().is_some(), "the failure stays loud");
    }

    /// F5: clearing the memo after a recovery re-arms the loud path.
    #[test]
    fn clear_draft_error_dedup_rearms_the_loud_path() {
        let mut app = app();
        app.show_draft_error("boom");
        let _ = take_draft_error_log();
        app.clear_draft_error_dedup();
        app.show_draft_error("boom");
        assert_eq!(take_draft_error_log(), vec!["boom"]);
    }
}
