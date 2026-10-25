//! KITT-WAIT-PIN-57: the shared wait is pinned by **reading the sources**, not
//! by trusting them.
//!
//! `KITT-SETTLE-UNIFY-53` removed the hand-written waits from the golden and
//! scan suites and gave them one loop, one report and one condition each. What
//! it did **not** leave behind is a gate: nothing noticed if the next
//! hand-written wait loop reappeared in the same file the old one had lived in.
//! Measured in verification round 2 — the old 400-frame loop restored word for
//! word, `kittest_crop_overlay` reported **2 passed / 0 failed**, the goldens
//! stayed byte-identical on the healthy path and
//! `git status --porcelain -- crates/lumina-gui/tests/snapshots` was empty. The
//! defect returns silently, so this target exists to make it loud.
//!
//! # The rule this file enforces
//!
//! In this crate's Rust sources there is exactly **one** place that may pump the
//! harness frame loop while its exit depends on state read from the app:
//!
//! * `tests/kittest_decode_support/mod.rs` (`pump` / `pump_until_ready`), and
//! * `tests/scan_settle_support/mod.rs` (`settle_scan`, on the same loop).
//!
//! Every other loop that steps the harness **and** ends on an app predicate is
//! a hand-written wait: either it reports nothing when it expires — the silent
//! class, the one that let a golden be taken from an unsettled state — or it
//! carries its own deadline and its own report, a second copy of a diagnosis
//! this tree now has once. Both make this target red.
//!
//! The rule is **pattern**-based, never list-based: the sources are discovered
//! by walking the crate and the rule is applied to whatever the walk found. A
//! hardcoded file list would stay green the moment a suite is renamed, added or
//! split — exactly the failure this task exists to close.
//!
//! # The named, **loud** exceptions
//!
//! Four sites step the harness on an app predicate outside the shared modules.
//! All four are loud — they fail on their own, with the state they found — and
//! each is named below with its reason instead of being skipped in silence. A
//! **silent** loop is never admissible as an exception: it violates the rule
//! "be loud", which is the rule this task is about.
//!
//! # What is deliberately **not** covered (the boundary, pinned not hidden)
//!
//! A loop whose exit does **not** depend on app state is not a wait, and the
//! first test below does not flag it. Eleven such loops step the harness today,
//! and **all eleven are named** so none is skipped in silence:
//!
//! * **Eight blind frame budgets** (`for … in 0..N`) — four input gestures
//!   (`double_click`, `press_hold_release`, `drag_then_hold`, and
//!   `graph_click_at`, whose frames dispatch the pointer gesture itself), and
//!   four fixed layout pumps: 4 frames to let a mask seed paint in
//!   `kittest_mask_visibility`, 5 after a Develop header click in
//!   `kittest_snapshots_support`, and 2 + 5 in `settle_thumbnails` to enqueue
//!   and drain thumbnail cells.
//! * **Three collection iterations** — `kittest_snapshots`' section and view
//!   loops and `kittest_parity`'s scene loop, which step once per *member*.
//!
//! The class this task eliminated — all six loops `KITT-SETTLE-UNIFY-53`
//! replaced — decided its exit on an app predicate, which is why the rule is
//! drawn there. The eight blind frame budgets are pinned by name in
//! `FRAME_BUDGETS` below, so a *ninth* one is red while the existing eight stay
//! green.
//!
//! One further wait does not pump the harness at all:
//! `tests/kittest_fixtures_support/mod.rs`'s `while !all_present(&probes)` waits
//! on the thumbnail worker's cache, sleeping between probes. The shared wait
//! steps the frame loop, so it cannot express that wait; the loop is loud
//! (deadline assert naming the probe state). It is named here so the boundary is
//! visible rather than discovered by whoever trips over it next.
//!
//! # Non-vacuity
//!
//! A source scanner that stops seeing loops reports a clean tree. Three guards
//! make that impossible here: the walk must find the crate's sources, the
//! scanner must find the shared wait loop it is *allowed* to find, and the
//! inventory must match the count this file pins. A suite mutated back into the
//! historical loop turns this target red (measured, see the task report); the
//! same loop inside the shared module stays green, because that is the one place
//! it belongs.

mod kittest_wait_pin_support;

use kittest_wait_pin_support::{
    pumps, relative, rust_sources, step_calls, strip_noise, PumpForm, WaitLoop,
};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// The **allowed** places: the two shared modules that own the wait. Matched on
/// the path suffix, so a target reaching them through a `#[path]` declaration
/// still resolves to the same file.
const ALLOWED_MODULES: [&str; 2] = [
    "kittest_decode_support/mod.rs",
    "scan_settle_support/mod.rs",
];

/// A named, loud exception: a hand-written wait loop that may stay, and why.
struct Exception {
    /// File **basename** — a suffix match survives the file being reached
    /// through `src/` or through `tests/`, and it is not compiled twice.
    file: &'static str,
    /// The `fn` the loop sits in, so a *second* loop in the same file is not
    /// covered by this exception.
    function: &'static str,
    /// Why it may stay. Loud means: it fails on its own.
    reason: &'static str,
}

/// The named exceptions. Two are named by the task's acceptance; the other two
/// are found and named here, with the same standard applied.
const EXCEPTIONS: [Exception; 4] = [
    Exception {
        file: "kittest_snapshots/library.rs",
        function: "filmstrip_twenty_dummies",
        reason: "the predicate is an accesskit label query (\"LibRaw opening \
                 input failed\"), not app state: the shared wait re-reads a \
                 `&LuminaApp` predicate, and a label that exists only once the \
                 failing cells have painted is not expressible there. Loud — a \
                 300 s deadline assert names the sentinel decode.",
    },
    Exception {
        file: "kittest_spot_tool.rs",
        function: "develop_spot_selected",
        reason: "600-frame budget for the temp fixture's decode, closed by an \
                 `assert!` immediately afterwards: an exhausted budget fails \
                 the test instead of falling through to the golden.",
    },
    Exception {
        file: "kittest_snapshots_support/mod.rs",
        function: "settle_render",
        reason: "GOLDEN-STALE-55 waits for the render **identity** \
                 (`render_key().is_some()`), which the shared decode wait does \
                 not name. Loud and bounded: it aborts with the found state \
                 (`render_key`, `preview_generation`, `status`) instead of \
                 snapshotting an unsettled render. A migration onto the shared \
                 wait is possible and welcome — today it is a deliberate, \
                 reported deviation rather than a silent one.",
    },
    Exception {
        file: "kittest_decode_contract.rs",
        function: "trace_until_settled",
        reason: "KITT-DECODE-CONTRACT-52 measures the settle window frame by \
                 frame: it records every frame up to and including the settle \
                 frame, which the shared wait — a loop that returns on the \
                 first match — structurally cannot produce. Loud: the \
                 production bound with an assert naming the last state.",
    },
];

/// The pinned inventory: one allowed loop (`pump`) plus the four named
/// exceptions. A move in either direction fails this target, so a new exception
/// has to be added **here**, in review, rather than discovered by a silent
/// green run.
const INVENTORY: usize = 5;

/// One named blind frame budget: `for … in 0..N` around a harness step, with no
/// app-state exit. Named, because the silent wait this task removed had exactly
/// this shape.
struct FrameBudget {
    file: &'static str,
    function: &'static str,
    /// The loop head, so two loops in one `fn` stay distinguishable.
    condition: &'static str,
    reason: &'static str,
}

/// The eight blind frame budgets, by name.
///
/// Four are input gestures — their frames dispatch a pointer gesture, and a
/// gesture never sleeps. Four are fixed layout pumps — a click or a scheduled
/// thumbnail job needs a deterministic number of frames, and none of them waits
/// for a background worker (`settle_thumbnails`'s own module doc explains why
/// stepping in a poll loop would be the wrong shape). A ninth entry is red until
/// it is named here.
const FRAME_BUDGETS: [FrameBudget; 8] = [
    FrameBudget {
        file: "kittest_mask_visibility.rs",
        function: "mask_view_visibility",
        condition: "_ in 0..4",
        reason: "4 frames to let the mask seed paint before the label queries",
    },
    FrameBudget {
        file: "slider_sidecar_save_support/mod.rs",
        function: "press_hold_release",
        condition: "_ in 0..hold",
        reason: "the `hold` frames ARE the gesture: they start the 150 ms edit \
                 clock (`schedule_render`'s pointer-down branch)",
    },
    FrameBudget {
        file: "slider_sidecar_save_support/mod.rs",
        function: "drag_then_hold",
        condition: "_ in 0..hold",
        reason: "the `hold` frames ARE the gesture: the pointer must stay down \
                 across frames for the debounce to arm",
    },
    FrameBudget {
        file: "kittest_snapshots_support/mod.rs",
        function: "open_collapsing_and_scroll_to",
        condition: "_ in 0..5",
        reason: "5 frames for the clicked Develop header to lay out, before the \
                 scroll target is queried",
    },
    FrameBudget {
        file: "mask_local_curve_graph_support/mod.rs",
        function: "double_click",
        condition: "_ in 0..2",
        reason: "2 frames: press and release must sit in separate frames for \
                 egui to fold them into one double click",
    },
    FrameBudget {
        file: "kittest_fixtures_support/mod.rs",
        function: "settle_thumbnails",
        condition: "_ in 0..2",
        reason: "2 spaced frames to let the grid/filmstrip enqueue their cells",
    },
    FrameBudget {
        file: "kittest_fixtures_support/mod.rs",
        function: "settle_thumbnails",
        condition: "_ in 0..5",
        reason: "5 spaced frames to drain the results the loud worker wait \
                 (`while !all_present`) already observed",
    },
    FrameBudget {
        file: "tone_curve_graph.rs",
        function: "graph_click_at",
        condition: "_ in 0..clicks",
        reason: "the click count IS the gesture: one press and one release per \
                 click, two for the double click",
    },
];

/// The crate, scanned once.
struct Scan {
    sources: Vec<std::path::PathBuf>,
    pumping: usize,
    waits: Vec<WaitLoop>,
    budgets: Vec<WaitLoop>,
    shared_wait_seen: bool,
}

/// The walk must see the crate, and the scanner must see a loop in it. Without
/// these two guards a broken walk or a broken parser reports a clean tree — the
/// vacuous green this task exists to prevent.
fn the_scan_is_not_vacuous(sources: usize, shared_wait_seen: bool) {
    assert!(
        sources >= 60,
        "the source walk found only {sources} files — it is broken, so this run \
         proves nothing"
    );
    assert!(
        shared_wait_seen,
        "the scanner did not find the loop in kittest_decode_support (the shared \
         wait), so it cannot see loops at all — this run proves nothing"
    );
}

/// The report a red run prints: every offending site, with what it is, next to
/// the rule and the places that satisfy it.
fn report(offenders: &[WaitLoop], sources: usize, pumping: usize) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "KITT-WAIT-PIN-57: {n} hand-written wait loop(s) outside the shared wait \
         ({sources} sources scanned, {pumping} of them pump the harness)",
        n = offenders.len(),
    );
    let _ = writeln!(
        text,
        "\nthe rule: one loop pumps the harness on an app predicate, and it lives in"
    );
    for allowed in ALLOWED_MODULES {
        let _ = writeln!(text, "    {allowed}");
    }
    let _ = writeln!(text, "\nfound instead:");
    for found in offenders {
        let _ = writeln!(
            text,
            "    {}:{}  {} {}  (fn {}, {} frame step(s) in the body)",
            found.file, found.line, found.kind, found.condition, found.function, found.steps
        );
    }
    let _ = writeln!(text, "\nnamed loud exceptions (each fails on its own):");
    for exception in EXCEPTIONS {
        let _ = writeln!(
            text,
            "    {}::{}  -- {}",
            exception.file, exception.function, exception.reason
        );
    }
    let _ = writeln!(
        text,
        "\na loop that cannot fail on its own is NOT admissible as an exception — \
         it is the defect this target exists to catch. Move the loop into the \
         shared wait, or give it a loud, bounded exit and name it above."
    );
    text
}

/// The scanned tree, once, for both tests.
fn scan(root: &Path) -> Scan {
    let sources = rust_sources(root);
    let mut scan = Scan {
        sources,
        pumping: 0,
        waits: Vec::new(),
        budgets: Vec::new(),
        shared_wait_seen: false,
    };
    for path in &scan.sources {
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        let file = relative(root, path);
        let code = strip_noise(&source);
        if step_calls(&code) > 0 {
            scan.pumping += 1;
        }
        for (entry, form) in pumps(&file, &code) {
            match form {
                PumpForm::Wait => scan.waits.push(entry),
                PumpForm::FrameBudget => scan.budgets.push(entry),
            }
        }
        if file.ends_with("kittest_decode_support/mod.rs") {
            // The shared wait is allowed; its own loop must stay visible to the
            // scanner, otherwise the scanner is blind and both tests are
            // vacuous.
            scan.shared_wait_seen = scan
                .waits
                .iter()
                .any(|entry| entry.function == "pump" && entry.steps == 1);
        }
    }
    scan
}

#[test]
fn no_suite_carries_its_own_wait_loop() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scan = scan(root);
    the_scan_is_not_vacuous(scan.sources.len(), scan.shared_wait_seen);

    let total = scan.waits.len();
    println!(
        "KITT-WAIT-PIN-57: {} sources scanned, {} pump the harness, {} wait \
         loops ({} allowed, {} named loud exceptions), {} blind frame budgets",
        scan.sources.len(),
        scan.pumping,
        total,
        ALLOWED_MODULES.len(),
        EXCEPTIONS.len(),
        scan.budgets.len(),
    );

    let offenders: Vec<WaitLoop> = scan
        .waits
        .into_iter()
        .filter(|entry| {
            let allowed = ALLOWED_MODULES
                .iter()
                .any(|module| entry.file.ends_with(module));
            let named = EXCEPTIONS.iter().any(|exception| {
                entry.file.ends_with(exception.file) && entry.function == exception.function
            });
            !allowed && !named
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "{}",
        report(&offenders, scan.sources.len(), scan.pumping)
    );
    // The tripwire: a loop that was named as an exception instead of removed
    // still moved the inventory, so adding an exception is a reviewed act rather
    // than a silent one.
    assert_eq!(
        total, INVENTORY,
        "the wait-loop inventory moved ({total} found, {INVENTORY} pinned). If a \
         suite gained a loop, remove it or name it as a loud exception; if one \
         was migrated onto the shared wait, update the pinned count here."
    );
}

#[test]
fn no_unnamed_blind_frame_budget_pump() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scan = scan(root);
    let unnamed: Vec<&WaitLoop> = scan
        .budgets
        .iter()
        .filter(|entry| {
            !FRAME_BUDGETS.iter().any(|budget| {
                entry.file.ends_with(budget.file)
                    && entry.function == budget.function
                    && entry.condition == budget.condition
            })
        })
        .collect();
    assert!(
        unnamed.is_empty(),
        "KITT-WAIT-PIN-57: {n} blind frame budget(s) outside the named set. A \
         `for … in 0..N` pump is how the silent wait looked before \
         KITT-SETTLE-UNIFY-53 — it exhausted its budget, returned normally and \
         let the golden be taken from an unsettled state with a green test.\n\
         found: {found}\n\
         named: {named}\n\
         Either wait on the app state (the shared wait), or add it to \
         FRAME_BUDGETS with a reason — never in silence.",
        n = unnamed.len(),
        found = describe(&unnamed),
        named = FRAME_BUDGETS
            .iter()
            .map(|budget| format!(
                "{}::{} {} ({})",
                budget.file, budget.function, budget.condition, budget.reason
            ))
            .collect::<Vec<_>>()
            .join("; "),
    );
    assert_eq!(
        scan.budgets.len(),
        FRAME_BUDGETS.len(),
        "the blind frame-budget inventory moved ({found} found, {pinned} pinned). \
         A gesture or layout pump that was removed or renamed changes this \
         count; update FRAME_BUDGETS so the class stays enumerated.",
        found = scan.budgets.len(),
        pinned = FRAME_BUDGETS.len(),
    );
}

/// One line per site, for the failure report.
fn describe(entries: &[&WaitLoop]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "{}:{} {} (fn {})",
                entry.file, entry.line, entry.condition, entry.function
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}
