//! U1/U6: the jank threshold policy — `LUMINA_JANK_MS` parsing, the
//! process-wide memo and the test-only override, moved out of `jank_log.rs`;
//! the only edit to the moved code is the `pub(crate)` visibility the parent
//! needs, no logic changed. Which limit is in force is a closed,
//! side-effect-free unit of its own: the module it came from owns the RAII
//! scope, the record and the emitted line, and only asks this module for the
//! effective threshold.

// Only the test-only override needs a parent item (`Cell`); the production
// paths are fully self-contained (`log::…`, `std::…` are crate/extern paths).
#[cfg(test)]
use std::cell::Cell;

/// U1: einheitliche Default-Schwelle, 120-Hz-ProMotion-Frame-Budget.
pub(crate) const DEFAULT_THRESHOLD_MS: f64 = 8.3;

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

pub(crate) fn effective_threshold() -> JankThreshold {
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
pub(crate) fn set_test_threshold(value: JankThreshold) {
    TEST_THRESHOLD.with(|cell| cell.set(Some(value)));
}
