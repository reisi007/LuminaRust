//! GUI-JANKLOG-19 (Release 1.0): Ruckel-Attribution. Debug-only and gated
//! behind the opt-in Cargo feature `janklog` — without the feature (and in
//! every release build) this module does not exist, so the instrumentation is
//! zero-overhead by construction (`Agents.md`: keine sichtbare Funktion ohne
//! Spez, kein stiller Fallback; Freigabe 2026-09-18 U2/U5).
//!
//! Standardbetrieb bleibt im Log still: genau **eine** Zeile pro nachweislich
//! langsamem Vorgang (Default-Schwelle 8,3 ms = 120-Hz-ProMotion-Budget, U1),
//! mit der Kette Aktion → Rezept-Änderung → Renderpfad/Route → Teil-Dauern:
//!
//! ```text
//! LUMINA_JANK kind=<action|render> action=<name|-> recipe_key=<key|-> route=<present|cpu-fallback|n/a> badge_reason="<reason|->" total_ms=<n> gpu_ms=<n> cpu_draft_ms=<n> analyse_ms=<n>
//! ```
//!
//! Feld-Reihenfolge und Werte sind fix (U3/U4): alle Werte whitespace-frei
//! außer dem gequoteten `badge_reason`; Teil-Dauern sind `-`, wo sie nicht
//! anwendbar sind. Die Emission erfolgt über einen RAII-Scope genau einmal am
//! Ende des äußersten Scopes (Aktion **oder** Render-Tick); verschachtelte
//! Messpunkte (Aktion → Render-Tick) füllen denselben Record und erzeugen keine
//! zweite Zeile.
//!
//! Schwellen-Quelle: Default 8,3 ms, überschreibbar über `LUMINA_JANK_MS`
//! (ganzzahlige Millisekunden, einmalig beim ersten Zugriff gelesen). `0`
//! deaktiviert die Zeile ausdrücklich (einmaliger Hinweis), ein unparsbarer
//! Wert erzeugt `warn!` + Default — nie ein stiller Fallback (U6). Das
//! Enablement bleibt das Cargo-Feature; die Umgebungsvariable regelt nur die
//! Schwelle (U5). Reine Diagnose: keine Rezept-/Sidecar-/Pixel-Auswirkung.

use super::*;
use std::cell::{Cell, RefCell};
use std::time::Instant;

/// U1: einheitliche Default-Schwelle, 120-Hz-ProMotion-Frame-Budget.
const DEFAULT_THRESHOLD_MS: f64 = 8.3;

/// An welchem äußersten Scope die Jank-Zeile hängt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JankKind {
    /// Ein instrumentierter `GuiAction`-Scope.
    Action,
    /// Ein Draft-/Full-Render-Tick ohne umgebenden Aktions-Scope.
    Render,
}

impl JankKind {
    fn label(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Render => "render",
        }
    }
}

/// Aufgelöste Schwelle: `Disabled` ist die ausdrückliche `LUMINA_JANK_MS=0`-
/// Abschaltung (kein stiller Default), `Millis` die wirksame Grenze.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum JankThreshold {
    Disabled,
    Millis(f64),
}

/// Reine Schwellen-Auswertung (testbar ohne Umgebung): `None` → Default,
/// `0` → deaktiviert, ganze Millisekunden → Grenze, sonst Default. Zweiter
/// Wert: laute U6-Meldung (`0`→`info!`, unparsbar→`warn!`, sonst still).
pub(crate) fn resolve_threshold(raw: Option<&str>) -> (JankThreshold, Option<log::Level>) {
    let invalid = raw.is_some_and(|t| t.trim().parse::<u64>().is_err());
    let parsed = match raw {
        None => JankThreshold::Millis(DEFAULT_THRESHOLD_MS),
        Some(value) => match value.trim().parse::<u64>() {
            Ok(0) => JankThreshold::Disabled,
            Ok(ms) => JankThreshold::Millis(ms as f64),
            Err(_) => JankThreshold::Millis(DEFAULT_THRESHOLD_MS),
        },
    };
    let notice = match parsed {
        JankThreshold::Disabled => Some(log::Level::Info),
        JankThreshold::Millis(_) if invalid => Some(log::Level::Warn),
        JankThreshold::Millis(_) => None,
    };
    (parsed, notice)
}

// Einmalig aufgeloeste, prozessweite Schwelle (kein Per-Frame-Parsen, U1/U6).
static THRESHOLD: std::sync::OnceLock<JankThreshold> = std::sync::OnceLock::new();

fn global_threshold() -> JankThreshold {
    *THRESHOLD.get_or_init(|| {
        let raw = std::env::var("LUMINA_JANK_MS").ok();
        let (parsed, notice) = resolve_threshold(raw.as_deref());
        match notice {
            Some(log::Level::Info) => log::info!("LUMINA_JANK disabled (LUMINA_JANK_MS=0)"),
            Some(log::Level::Warn) => log::warn!(
                "LUMINA_JANK: invalid LUMINA_JANK_MS={:?}; using default {DEFAULT_THRESHOLD_MS} ms",
                raw.as_deref().unwrap_or_default()
            ),
            _ => {}
        }
        parsed
    })
}

#[cfg(test)]
thread_local! {
    static TEST_THRESHOLD: Cell<Option<JankThreshold>> = const { Cell::new(None) };
}

fn effective_threshold() -> JankThreshold {
    #[cfg(test)]
    {
        if let Some(overridden) = TEST_THRESHOLD.with(Cell::get) {
            return overridden;
        }
    }
    global_threshold()
}

/// Test-only override of the threshold so the slow/silent paths are
/// deterministic regardless of machine load (`Millis(-1.0)` = emit always).
#[cfg(test)]
fn set_test_threshold(value: JankThreshold) {
    TEST_THRESHOLD.with(|cell| cell.set(Some(value)));
}

// The record being built for the current outermost scope.
struct JankRecord {
    kind: JankKind,
    action: Option<&'static str>,
    recipe_key: Option<String>,
    route: &'static str,
    badge_reason: Option<String>,
    start: Instant,
    gpu_ms: Option<f64>,
    cpu_draft_ms: Option<f64>,
    analyse_ms: Option<f64>,
}

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    static RECORD: RefCell<Option<JankRecord>> = const { RefCell::new(None) };
}

/// RAII scope for one outermost action or render tick (analog
/// [`crate::gui_action::GuiActionTimer`], aber ein gemeinsamer Record statt
/// zweier Timer). Beim Drop des äußersten Scopes wird die Zeile genau einmal
/// emittiert, wenn `total_ms` die Schwelle überschreitet.
pub(crate) struct JankScope {
    outermost: bool,
}

impl JankScope {
    /// Opens a scope. Only the outermost call installs a fresh record;
    /// nested calls just keep the depth counter so they cannot emit a second
    /// line (U3/U4). While the threshold is `Disabled`, no record is created
    /// at all (no `Instant::now`, no allocation).
    pub(crate) fn enter(
        kind: JankKind,
        action: Option<&'static str>,
        route: &'static str,
        badge_reason: Option<&str>,
        recipe_key: Option<&str>,
    ) -> Self {
        let depth = DEPTH.with(|cell| {
            let depth = cell.get();
            cell.set(depth + 1);
            depth
        });
        let outermost = depth == 0;
        if outermost && !matches!(effective_threshold(), JankThreshold::Disabled) {
            RECORD.with(|record| {
                *record.borrow_mut() = Some(JankRecord {
                    kind,
                    action,
                    recipe_key: recipe_key.map(str::to_owned),
                    route,
                    badge_reason: badge_reason.map(str::to_owned),
                    start: Instant::now(),
                    gpu_ms: None,
                    cpu_draft_ms: None,
                    analyse_ms: None,
                });
            });
        }
        Self { outermost }
    }

    /// Convenience for a `GuiAction` scope; the recipe key is filled by the
    /// action body through [`note_recipe_key`].
    pub(crate) fn enter_action(
        action: GuiAction,
        route: &'static str,
        badge_reason: Option<&str>,
    ) -> Self {
        Self::enter(
            JankKind::Action,
            Some(action.name()),
            route,
            badge_reason,
            None,
        )
    }
}

impl Drop for JankScope {
    fn drop(&mut self) {
        DEPTH.with(|cell| cell.set(cell.get().saturating_sub(1)));
        if !self.outermost {
            return;
        }
        let Some(record) = RECORD.with(|record| record.borrow_mut().take()) else {
            return;
        };
        let total_ms = record.start.elapsed().as_secs_f64() * 1000.0;
        if let JankThreshold::Millis(limit) = effective_threshold() {
            if total_ms > limit {
                let line = format_jank_line(&record, total_ms);
                // `warn!` is visible at the default `RUST_LOG=info` (Entwurf,
                // Option B) — the point is a greppable line without level tuning.
                log::warn!("{line}");
                #[cfg(test)]
                JANK_LOG.with(|log| log.borrow_mut().push(line));
            }
        }
    }
}

/// Deliver the just-recorded recipe Dirty-Key to the active jank record
/// (verhaltensneutrale Beobachtung; `dirty.rs` wiring). A no-op outside a
/// scope or when the action already carries a key.
pub(crate) fn note_recipe_key(key: &str) {
    RECORD.with(|record| {
        if let Some(record) = record.borrow_mut().as_mut() {
            if record.recipe_key.is_none() {
                record.recipe_key = Some(key.to_owned());
            }
        }
    });
}

/// Deliver the per-tick drag timings (`DragTickTimings`) to the active jank
/// record, so action and render tick share one line (U3/U4).
pub(crate) fn note_render_timings(gpu_ms: f64, cpu_draft_ms: f64, analyse_ms: f64) {
    RECORD.with(|record| {
        if let Some(record) = record.borrow_mut().as_mut() {
            record.gpu_ms = Some(gpu_ms);
            record.cpu_draft_ms = Some(cpu_draft_ms);
            record.analyse_ms = Some(analyse_ms);
        }
    });
}

/// Deliver the analysis-pass duration of a full render to the active record.
pub(crate) fn note_analyse_ms(analyse_ms: f64) {
    RECORD.with(|record| {
        if let Some(record) = record.borrow_mut().as_mut() {
            record.analyse_ms = Some(analyse_ms);
        }
    });
}

fn format_ms(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), |ms| format!("{ms:.2}"))
}

fn quote_badge(reason: Option<&str>) -> String {
    let text = reason.filter(|reason| !reason.is_empty()).unwrap_or("-");
    // The reason contains spaces (hence the quotes) but must stay one
    // greppable token: strip embedded quotes/newlines instead of leaking them.
    let sanitized: String = text
        .chars()
        .map(|c| {
            if c == '"' || c == '\n' || c == '\r' {
                '\''
            } else {
                c
            }
        })
        .collect();
    format!("\"{sanitized}\"")
}

/// Pure one-liner formatter (single source of truth for the format).
fn format_jank_line(record: &JankRecord, total_ms: f64) -> String {
    format!(
        "LUMINA_JANK kind={} action={} recipe_key={} route={} badge_reason={} total_ms={} gpu_ms={} cpu_draft_ms={} analyse_ms={}",
        record.kind.label(),
        record.action.unwrap_or("-"),
        record.recipe_key.as_deref().unwrap_or("-"),
        record.route,
        quote_badge(record.badge_reason.as_deref()),
        format_ms(Some(total_ms)),
        format_ms(record.gpu_ms),
        format_ms(record.cpu_draft_ms),
        format_ms(record.analyse_ms),
    )
}

// Thread-local capture seam for headless tests (same pattern as INSTRDBG).
#[cfg(test)]
thread_local! {
    static JANK_LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Drains and returns the captured `LUMINA_JANK` lines of the current test
/// thread (debug + `janklog` only).
#[cfg(test)]
pub(crate) fn take_jank_log() -> Vec<String> {
    JANK_LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
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
        app.load_bytes(png(), "jank-test.png").unwrap();
        app
    }

    /// App backed by a real tempdir path so slider/action commits can write a
    /// sidecar (DoD §1) while the jank line is observed.
    fn app_with_path(dir: &tempfile::TempDir) -> LuminaApp {
        let path = dir.path().join("photo.png");
        std::fs::write(&path, png()).unwrap();
        let mut app = LuminaApp::new(egui::Context::default());
        app.open_file(path.display().to_string());
        for _ in 0..2000 {
            app.poll_decode();
            if app.original.is_some() || app.error().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        app
    }

    /// U1/U6: Default 8,3 ms; `0` = aus; ganze ms = Override; unparsbar = Default.
    #[test]
    fn threshold_defaults_and_parsing() {
        let def = JankThreshold::Millis(DEFAULT_THRESHOLD_MS);
        assert_eq!(resolve_threshold(None).0, def);
        assert_eq!(resolve_threshold(Some("0")).0, JankThreshold::Disabled);
        assert_eq!(resolve_threshold(Some("12")).0, JankThreshold::Millis(12.0));
        assert_eq!(
            resolve_threshold(Some(" 12 ")).0,
            JankThreshold::Millis(12.0)
        );
        assert_eq!(resolve_threshold(Some("not-a-number")).0, def);
    }

    /// U6-Regressionstest gegen den stillen Fallback (Doku §5): unparsbarer
    /// Env-Wert → Default WIRKSAM + `warn!`; `=0` → deaktiviert + `info!`.
    #[test]
    fn loud_env_diagnostics_never_fall_back_silently() {
        let (invalid, notice) = resolve_threshold(Some("not-a-number"));
        assert_eq!(invalid, JankThreshold::Millis(DEFAULT_THRESHOLD_MS));
        assert_eq!(
            notice,
            Some(log::Level::Warn),
            "invalid value must warn, never disable silently"
        );
        let (disabled, notice) = resolve_threshold(Some("0"));
        assert_eq!(disabled, JankThreshold::Disabled);
        assert_eq!(
            notice,
            Some(log::Level::Info),
            "0 must announce its explicit disable"
        );
        assert_eq!(resolve_threshold(Some("12")).1, None);
        assert_eq!(resolve_threshold(None).1, None);
    }

    /// U3/U4: the exact one-liner with every field populated (incl. quoted
    /// badge reason containing spaces).
    #[test]
    fn format_pins_every_field() {
        let record = JankRecord {
            kind: JankKind::Action,
            action: Some("set_treatment"),
            recipe_key: Some("treatment".into()),
            route: GPU_ROUTE_CPU_FALLBACK,
            badge_reason: Some("Unsupported GPU stages [geometry (default content crop)]".into()),
            start: Instant::now(),
            gpu_ms: Some(1.25),
            cpu_draft_ms: Some(12.5),
            analyse_ms: Some(3.0),
        };
        assert_eq!(
            format_jank_line(&record, 14.75),
            "LUMINA_JANK kind=action action=set_treatment recipe_key=treatment \
             route=cpu-fallback badge_reason=\"Unsupported GPU stages [geometry (default content crop)]\" \
             total_ms=14.75 gpu_ms=1.25 cpu_draft_ms=12.50 analyse_ms=3.00"
        );
    }

    /// Absent fields render as `-` (render scope without route/badge/timings).
    #[test]
    fn format_uses_dash_for_absent_fields() {
        let record = JankRecord {
            kind: JankKind::Render,
            action: None,
            recipe_key: None,
            route: GPU_ROUTE_NA,
            badge_reason: None,
            start: Instant::now(),
            gpu_ms: None,
            cpu_draft_ms: None,
            analyse_ms: None,
        };
        assert_eq!(
            format_jank_line(&record, 2.0),
            "LUMINA_JANK kind=render action=- recipe_key=- route=n/a \
             badge_reason=\"-\" total_ms=2.00 gpu_ms=- cpu_draft_ms=- analyse_ms=-"
        );
    }

    /// Deckt (a) ab: unterhalb der Schwelle und im deaktivierten Zustand bleibt
    /// der Normalbetrieb still (`U6`: `0` = aus).
    #[test]
    fn sub_threshold_and_disabled_operation_is_silent() {
        set_test_threshold(JankThreshold::Millis(1.0e9));
        let mut app = app();
        app.render().unwrap();
        let _ = take_jank_log();
        app.toggle_crop_mode();
        app.render_draft_tick([320, 200]);
        assert!(
            take_jank_log().is_empty(),
            "normal/sub-threshold operation must stay silent"
        );

        set_test_threshold(JankThreshold::Disabled);
        app.render_draft_tick([320, 200]);
        assert!(
            take_jank_log().is_empty(),
            "LUMINA_JANK_MS=0 semantics must emit nothing"
        );
    }

    /// Deckt (b)/(c)/(d) ab: ein simulierter langsamer Render-Tick erzeugt
    /// **genau eine** Zeile mit Dirty-Key, Route/Badge-Feld und allen
    /// Teil-Dauern.
    #[test]
    fn slow_render_emits_exactly_one_attributed_line() {
        set_test_threshold(JankThreshold::Millis(-1.0)); // emit always (test-only)
        let mut app = app();
        app.render().unwrap();
        let _ = take_jank_log();
        app.set_adjustment("exposure", 1.0);
        app.render_draft_tick([320, 200]);
        let lines = take_jank_log();
        assert_eq!(lines.len(), 1, "exactly one line per slow tick: {lines:?}");
        let line = &lines[0];
        assert!(
            line.starts_with("LUMINA_JANK kind=render action=- recipe_key=exposure route="),
            "{line}"
        );
        for field in [
            "badge_reason=",
            "total_ms=",
            "gpu_ms=",
            "cpu_draft_ms=",
            "analyse_ms=",
        ] {
            assert!(line.contains(field), "{line} is missing {field}");
        }
    }

    /// Deckt (b) ab: eine langsame instrumentierte Aktion trägt ihren
    /// Dirty-Key in derselben Zeile; der verschachtelte Full-Render fügt keine
    /// zweite Zeile hinzu.
    #[test]
    fn slow_action_links_recipe_key_without_a_second_line() {
        set_test_threshold(JankThreshold::Millis(-1.0));
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_path(&dir);
        app.render().unwrap();
        let _ = take_jank_log();
        app.set_treatment("bw").unwrap();
        let lines = take_jank_log();
        assert_eq!(
            lines.len(),
            1,
            "action + nested render = one line: {lines:?}"
        );
        assert!(
            lines[0]
                .starts_with("LUMINA_JANK kind=action action=set_treatment recipe_key=treatment "),
            "{}",
            lines[0]
        );
    }
}
