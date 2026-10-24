use super::*;

fn probe(len: usize) -> Vec<String> {
    (0..len).map(|i| format!("probe-{i}")).collect()
}

#[test]
fn plan_window_honours_priority_and_edges() {
    let sources: Vec<PathBuf> = (0..8).map(|i| PathBuf::from(format!("p{i}"))).collect();
    let names: Vec<String> = (0..8).map(|i| format!("p{i}.cr3")).collect();
    let jobs = plan_window_jobs(
        &probe(8),
        &sources,
        &names,
        3,
        (800, 600),
        PreviewKind::Screen,
        DenoisePolicy::Warn,
    );
    let priorities: Vec<u8> = jobs.iter().map(|j| j.priority).collect();
    assert_eq!(priorities, vec![0, 1, 2, 3, 4, 5]);
    let probes: Vec<&str> = jobs.iter().map(|j| j.probe_id.as_str()).collect();
    assert_eq!(
        probes,
        vec!["probe-4", "probe-5", "probe-2", "probe-6", "probe-1", "probe-7"]
    );

    // No wrap at the start.
    let jobs = plan_window_jobs(
        &probe(8),
        &sources,
        &names,
        0,
        (800, 600),
        PreviewKind::Screen,
        DenoisePolicy::Warn,
    );
    assert_eq!(jobs.len(), 4);
}

#[test]
fn needs_job_dedups_in_flight_and_retries_are_bounded() {
    // Build a controller without threads to exercise bookkeeping only.
    let ctrl = PreviewController {
        lru: LruPreviewCache::default(),
        queue: Arc::new(PreviewQueue::default()),
        result_rx: mpsc::channel::<PreviewResult>().1,
        attempts: BTreeMap::new(),
        failed: BTreeMap::new(),
        pending_failed: BTreeMap::new(),
        in_flight: BTreeMap::new(),
        probe_digests: BTreeMap::new(),
        planned_kind: None,
        active_probe_id: None,
        probe_identities: BTreeMap::new(),
        directory: None,
    };
    let mut ctrl = ctrl;
    // Fake an in-flight entry.
    ctrl.in_flight.insert(
        "k".into(),
        crate::source_actions::NeighborInputIdentity::capture(
            std::path::Path::new("k"),
            "vc-original",
        ),
    );
    assert!(!ctrl.needs_job("k"));
    ctrl.in_flight.remove("k");
    assert!(ctrl.needs_job("k"));
    // A resident current frame means no job is needed.
    ctrl.probe_digests.insert("k".into(), "d-k".into());
    ctrl.lru.insert("d-k", make_frame(1, 1, 1));
    assert!(!ctrl.needs_job("k"), "resident current frame → no job");
    // Eviction makes the (otherwise "done") probe eligible again (A3).
    ctrl.lru.remove_entry("d-k");
    assert!(
        ctrl.needs_job("k"),
        "evicted probe must be re-planned, not permanently done (A3)"
    );
    // Recipe/source change (invalidate) also re-plans (A3).
    ctrl.lru.insert("d-k", make_frame(1, 1, 2));
    ctrl.invalidate_probe("k");
    assert!(ctrl.needs_job("k"), "invalidated probe re-renders (A3)");
    // Exhaust retries → no more jobs.
    for i in 0..PREVIEW_MAX_ATTEMPTS {
        ctrl.attempts.insert("k".into(), i + 1);
    }
    assert!(!ctrl.needs_job("k"));

    // Drain failures surfaces the pending log message (no silent fallback) while the visible badge stays.
    ctrl.failed.insert("k".into(), "boom".into());
    ctrl.pending_failed.insert("k".into(), "boom".into());
    assert_eq!(
        ctrl.drain_failures(),
        vec![("k".to_owned(), "boom".to_owned())]
    );
    assert!(
        ctrl.drain_failures().is_empty(),
        "pending drain is consuming"
    );
    assert_eq!(
        ctrl.failure("k"),
        Some("boom"),
        "visible badge persists after log drain (A2)"
    );
}

/// A *miss* must be visible as a "needs job / wird vorbereitet" state and
/// never as a silently wrong image: before any work, the controller reports
/// `needs_job` and the RAM LRU holds nothing for the probe.
#[test]
fn miss_is_visible_not_silent() {
    // A miss must be visible as a "needs job / wird vorbereitet" state and
    // never as a silently wrong image. Constructed directly (no threads).
    let mut ctrl = PreviewController {
        lru: LruPreviewCache::default(),
        queue: Arc::new(PreviewQueue::default()),
        result_rx: mpsc::channel::<PreviewResult>().1,
        attempts: BTreeMap::new(),
        failed: BTreeMap::new(),
        pending_failed: BTreeMap::new(),
        in_flight: BTreeMap::new(),
        probe_digests: BTreeMap::new(),
        planned_kind: None,
        active_probe_id: None,
        probe_identities: BTreeMap::new(),
        directory: None,
    };
    assert!(ctrl.lru().is_empty());
    assert!(ctrl.needs_job("probe-0"));
    assert!(!ctrl.lru().contains("probe-0"));
    // Enqueueing marks it in flight → the caller knows to show "wird
    // vorbereitet" instead of a stale/wrong frame.
    assert!(ctrl.enqueue(PreviewJob {
        probe_id: "probe-0".into(),
        source: PathBuf::from("/nonexistent/for-unit-test.png"),
        name: "for-unit-test.png".into(),
        virtual_copy: "vc-original".into(),
        target: (64, 64),
        kind: PreviewKind::Screen,
        priority: 0,
        denoise_policy: DenoisePolicy::Warn,
    }));
    assert!(
        !ctrl.needs_job("probe-0"),
        "in flight: preparing state shown"
    );
    ctrl.set_active("probe-0");
    assert_eq!(ctrl.active_probe_id(), Some("probe-0"));
}

#[test]
fn webp_roundtrip_via_helpers() {
    let f = make_frame(3, 3, 42);
    let webp = encode_webp_pub(&f).unwrap();
    let back = decode_webp_pub(&webp).unwrap();
    assert_eq!(back.pixels(), f.pixels());
}

#[test]
fn lru_holds_seven_and_pins_active() {
    let mut lru = LruPreviewCache::default();
    lru.insert("active", make_frame(1, 1, 1));
    lru.set_active("active");
    for i in 0..8 {
        lru.insert(format!("n{i}"), make_frame(1, 1, i as u8));
    }
    assert!(lru.contains("active"), "pinned active must survive");
    assert_eq!(lru.len(), 7, "max 7 slots (active + 6)");
}

/// End-to-end worker test: a real source on disk is decoded + rendered +
/// WebP-encoded by a background worker thread, delivered back via `poll`
/// into the RAM LRU, and persisted to the source's `.lumina/previews`
/// disk tier — without any UI-thread decode.
#[test]
fn worker_roundtrip_decodes_renders_and_stores_disk_tier() {
    use lumina_core::ImageFileFormat;
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("neighbor.png");
    // A 8x8 RGBA8 frame with a distinct alpha sample → PNG source on disk.
    let png = make_frame(8, 8, 77).encode(ImageFileFormat::Png).unwrap();
    std::fs::write(&source, &png).unwrap();

    let (mut ctrl, _queue) = PreviewController::spawn(1);
    ctrl.enqueue(PreviewJob {
        probe_id: "neighbor".into(),
        source: source.clone(),
        name: "neighbor.png".into(),
        virtual_copy: "vc-original".into(),
        target: (8, 8),
        kind: PreviewKind::Screen,
        priority: 0,
        denoise_policy: DenoisePolicy::Warn,
    });

    // Wait (bounded) for the worker result to land in the RAM LRU.
    let deadline = Instant::now() + Duration::from_secs(10);
    while ctrl.lru().is_empty() && Instant::now() < deadline {
        ctrl.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    ctrl.poll();
    assert!(
        !ctrl.lru().is_empty(),
        "worker must deliver the rendered neighbor into the RAM LRU"
    );
    assert!(
        !ctrl.needs_job("neighbor"),
        "a finished job must not be re-enqueued unconditionally"
    );
    assert!(
        ctrl.current_probes().contains(&"neighbor".to_owned()),
        "a successful prepare leaves a resident current preview (A3)"
    );
    assert_eq!(
        ctrl.probe_state("neighbor"),
        PreviewProbeState::Ready,
        "ready preview is visible per cell (A2)"
    );

    // A1: a navigation to this neighbor now hits the RAM LRU (no decode).
    let hit = ctrl
        .neighbor_preview("neighbor", &source)
        .expect("RAM lookup succeeds")
        .expect("neighbor preview is resident after prep");
    assert_eq!((hit.width(), hit.height()), (8, 8), "RAM cache-hit (A1)");

    // A4: evicting the RAM entry still serves the neighbor from the disk
    // tier (the worker persisted the WebP) — no full source re-render.
    let digest = ctrl.probe_digest("neighbor").unwrap().to_owned();
    ctrl.lru.remove_entry(&digest);
    assert!(
        ctrl.neighbor_preview("neighbor", &source)
            .expect("disk lookup succeeds")
            .is_some(),
        "evicted-but-on-disk neighbor still hits (A4)"
    );
    assert_eq!(
        ctrl.probe_state("neighbor"),
        PreviewProbeState::Stale,
        "evicted (RAM) but on-disk preview is visible as stale/veraltet (A2)"
    );

    // Disk tier: `.lumina/previews/*.preview.webp` next to the source.
    let previews = dir.path().join(".lumina").join("previews");
    assert!(previews.is_dir(), "disk tier must be created by the worker");
    let stored: Vec<_> = std::fs::read_dir(&previews)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(stored.len(), 1, "exactly one preview file expected");
    let webp = std::fs::read(&stored[0]).unwrap();
    let back = decode_webp_pub(&webp).unwrap();
    assert_eq!(
        (back.width(), back.height()),
        (8, 8),
        "stored WebP keeps the downscaled geometry"
    );
}

/// A3: a source/recipe change must make a previously-cached neighbor stale
/// and trigger a lazy re-render — never silently show the old frame and
/// never stay blocked by a permanently-"done" probe.
#[test]
fn invalidate_probe_marks_stale_and_triggers_reread() {
    let mut ctrl = PreviewController {
        lru: LruPreviewCache::default(),
        queue: Arc::new(PreviewQueue::default()),
        result_rx: mpsc::channel::<PreviewResult>().1,
        attempts: BTreeMap::new(),
        failed: BTreeMap::new(),
        pending_failed: BTreeMap::new(),
        in_flight: BTreeMap::new(),
        probe_digests: BTreeMap::new(),
        planned_kind: None,
        active_probe_id: None,
        probe_identities: BTreeMap::new(),
        directory: None,
    };
    // Simulate a successfully prepared neighbor under digest "d1".
    ctrl.lru.insert("d1", make_frame(2, 2, 1));
    ctrl.probe_digests.insert("p".into(), "d1".into());
    assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Ready);
    assert!(!ctrl.needs_job("p"));

    // The recipe/source changed → the UI calls `invalidate_probe`.
    ctrl.invalidate_probe("p");
    assert_eq!(
        ctrl.probe_state("p"),
        PreviewProbeState::Miss,
        "stale frame dropped; not silently available (A2)"
    );
    assert!(
        ctrl.needs_job("p"),
        "a changed recipe/source must re-render, not stay done (A3)"
    );
    assert!(
        !ctrl.lru().contains("d1"),
        "stale frame removed from the RAM LRU (A3)"
    );

    // In-flight → loading; ready → visible as ready.
    ctrl.in_flight.insert(
        "p".into(),
        crate::source_actions::NeighborInputIdentity::capture(
            std::path::Path::new("p"),
            "vc-original",
        ),
    );
    assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Loading);
    ctrl.in_flight.remove("p");
    ctrl.lru.insert("d2", make_frame(2, 2, 2));
    ctrl.probe_digests.insert("p".into(), "d2".into());
    assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Ready);
}

/// A2: an exhausted retry budget is a visible per-cell `Failed` state, not
/// a silent gray cell.
#[test]
fn exhausted_retries_surface_visible_failure_state() {
    let mut ctrl = PreviewController {
        lru: LruPreviewCache::default(),
        queue: Arc::new(PreviewQueue::default()),
        result_rx: mpsc::channel::<PreviewResult>().1,
        attempts: BTreeMap::new(),
        failed: BTreeMap::new(),
        pending_failed: BTreeMap::new(),
        in_flight: BTreeMap::new(),
        probe_digests: BTreeMap::new(),
        planned_kind: None,
        active_probe_id: None,
        probe_identities: BTreeMap::new(),
        directory: None,
    };
    ctrl.failed.insert("p".into(), "decode exploded".into());
    assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Failed);
    assert_eq!(ctrl.failure("p"), Some("decode exploded"));
}

// ---- T01 GUI-PREV-01: LRU hit vor Disk (A1) ----
#[test]
fn lru_hit_serves_without_disk_after_clear() {
    use lumina_core::ImageFileFormat;
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("neighbor2.png");
    let png = make_frame(8, 8, 77).encode(ImageFileFormat::Png).unwrap();
    std::fs::write(&source, &png).unwrap();

    let (mut ctrl, _queue) = PreviewController::spawn(1);
    ctrl.enqueue(PreviewJob {
        probe_id: "neighbor2".into(),
        source: source.clone(),
        name: "neighbor2.png".into(),
        virtual_copy: "vc-original".into(),
        target: (8, 8),
        kind: PreviewKind::Screen,
        priority: 0,
        denoise_policy: DenoisePolicy::Warn,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while ctrl.lru().is_empty() && Instant::now() < deadline {
        ctrl.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    ctrl.poll();
    assert!(!ctrl.lru().is_empty(), "GUI-PREV-01: worker must deliver");
    let hit_before = ctrl
        .neighbor_preview("neighbor2", &source)
        .expect("lookup ok")
        .expect("hit before clear");
    assert_eq!((hit_before.width(), hit_before.height()), (8, 8));

    // Clear disk tier: file under .lumina/previews must be removed.
    let disk = PreviewDiskCache::in_folder(dir.path()).unwrap();
    disk.clear().unwrap();
    assert!(disk
        .load(ctrl.probe_digest("neighbor2").unwrap())
        .unwrap()
        .is_none());

    // LRU must still serve without needing disk.
    let hit_after = ctrl
        .neighbor_preview("neighbor2", &source)
        .expect("lookup ok")
        .expect("GUI-PREV-01: LRU hit must survive disk clear");
    assert_eq!((hit_after.width(), hit_after.height()), (8, 8));
    assert_eq!(ctrl.probe_state("neighbor2"), PreviewProbeState::Ready);
    assert!(!ctrl.needs_job("neighbor2"));
}

// ---- T02 GUI-PREV-02: stale nach Rezept/Source-Change sichtbar ----
#[test]
fn probe_becomes_stale_after_source_modification() {
    use lumina_core::ImageFileFormat;
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("stale.png");
    let png = make_frame(8, 8, 11).encode(ImageFileFormat::Png).unwrap();
    std::fs::write(&source, &png).unwrap();

    let (mut ctrl, _queue) = PreviewController::spawn(1);
    ctrl.enqueue(PreviewJob {
        probe_id: "stale".into(),
        source: source.clone(),
        name: "stale.png".into(),
        virtual_copy: "vc-original".into(),
        target: (8, 8),
        kind: PreviewKind::Screen,
        priority: 0,
        denoise_policy: DenoisePolicy::Warn,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while ctrl.lru().is_empty() && Instant::now() < deadline {
        ctrl.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    ctrl.poll();
    assert_eq!(ctrl.probe_state("stale"), PreviewProbeState::Ready);
    assert!(!ctrl.probe_is_stale("stale", &source));

    // Touch source: change length/mtime so cheap fingerprint differs.
    std::thread::sleep(Duration::from_millis(30));
    let png2 = make_frame(8, 8, 99).encode(ImageFileFormat::Png).unwrap();
    std::fs::write(&source, &png2).unwrap();
    // Ensure mtime moved (sleep already); also verify length changed.
    assert!(
        ctrl.probe_is_stale("stale", &source),
        "GUI-PREV-02: modified source must be stale"
    );
    // neighbor_preview must not silently serve old frame: it invalidates and returns Miss.
    let stale_hit = ctrl.neighbor_preview("stale", &source).unwrap();
    assert!(
        stale_hit.is_none(),
        "GUI-PREV-02: stale preview must not be served"
    );
    assert_eq!(ctrl.probe_state("stale"), PreviewProbeState::Miss);
    assert!(ctrl.needs_job("stale"));

    // Re-enqueue with new content must produce new digest and become Ready again.
    ctrl.enqueue(PreviewJob {
        probe_id: "stale".into(),
        source: source.clone(),
        name: "stale.png".into(),
        virtual_copy: "vc-original".into(),
        target: (8, 8),
        kind: PreviewKind::Screen,
        priority: 0,
        denoise_policy: DenoisePolicy::Warn,
    });
    let deadline2 = Instant::now() + Duration::from_secs(10);
    while ctrl.probe_state("stale") != PreviewProbeState::Ready && Instant::now() < deadline2 {
        ctrl.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    ctrl.poll();
    assert_eq!(ctrl.probe_state("stale"), PreviewProbeState::Ready);
}

// ---- T03 GUI-PREV-03: pending_failed persistiert nach drain ----
#[test]
fn pending_failed_badge_persists_after_drain() {
    let mut ctrl = PreviewController {
        lru: LruPreviewCache::default(),
        queue: Arc::new(PreviewQueue::default()),
        result_rx: mpsc::channel::<PreviewResult>().1,
        attempts: BTreeMap::new(),
        failed: BTreeMap::new(),
        pending_failed: BTreeMap::new(),
        in_flight: BTreeMap::new(),
        probe_digests: BTreeMap::new(),
        planned_kind: None,
        active_probe_id: None,
        probe_identities: BTreeMap::new(),
        directory: None,
    };
    ctrl.failed.insert("p".into(), "boom".into());
    ctrl.pending_failed.insert("p".into(), "boom".into());
    assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Failed);
    assert_eq!(ctrl.failure("p"), Some("boom"));
    let drained = ctrl.drain_failures();
    assert_eq!(drained, vec![("p".to_owned(), "boom".to_owned())]);
    // GUI-PREV-03: visible badge must persist after log drain.
    assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Failed);
    assert_eq!(ctrl.failure("p"), Some("boom"));
    assert!(ctrl.drain_failures().is_empty());
}

// ---- T04 GUI-PREV-04: Prefetch-Fenster +4/-2 kein Wrap, Prio exakt ----
#[test]
fn prefetch_window_no_wrap_and_priority_order() {
    // Active 0 of 40: only forward neighbors, priorities 0,1,3,5
    let jobs = {
        let probe = (0..40).map(|i| format!("probe-{i}")).collect::<Vec<_>>();
        let sources: Vec<PathBuf> = (0..40).map(|i| PathBuf::from(format!("p{i}"))).collect();
        let names: Vec<String> = (0..40).map(|i| format!("p{i}.png")).collect();
        plan_window_jobs(
            &probe,
            &sources,
            &names,
            0,
            (800, 600),
            PreviewKind::Screen,
            DenoisePolicy::Warn,
        )
    };
    assert_eq!(
        jobs.len(),
        4,
        "GUI-PREV-04: active 0 of 40 must have 4 forward"
    );
    let prios: Vec<u8> = jobs.iter().map(|j| j.priority).collect();
    assert_eq!(prios, vec![0, 1, 3, 5]);
    for j in &jobs {
        let idx: usize = j.probe_id.strip_prefix("probe-").unwrap().parse().unwrap();
        assert!(idx < 40 && idx != 0, "no wrap, no active");
    }

    // Active 39 of 40: only backwards
    let jobs_end = {
        let probe = (0..40).map(|i| format!("probe-{i}")).collect::<Vec<_>>();
        let sources: Vec<PathBuf> = (0..40).map(|i| PathBuf::from(format!("p{i}"))).collect();
        let names: Vec<String> = (0..40).map(|i| format!("p{i}.png")).collect();
        plan_window_jobs(
            &probe,
            &sources,
            &names,
            39,
            (800, 600),
            PreviewKind::Screen,
            DenoisePolicy::Warn,
        )
    };
    assert_eq!(jobs_end.len(), 2);
    let offsets_end: Vec<i64> = jobs_end
        .iter()
        .map(|j| {
            let idx: i64 = j.probe_id.strip_prefix("probe-").unwrap().parse().unwrap();
            idx - 39
        })
        .collect();
    assert_eq!(offsets_end, vec![-1, -2]);
    assert_eq!(
        jobs_end.iter().map(|j| j.priority).collect::<Vec<_>>(),
        vec![2, 4]
    );

    // Active 10 of 40: full window +1>+2>-1>+3>-2>+4
    let jobs_mid = {
        let probe = (0..40).map(|i| format!("probe-{i}")).collect::<Vec<_>>();
        let sources: Vec<PathBuf> = (0..40).map(|i| PathBuf::from(format!("p{i}"))).collect();
        let names: Vec<String> = (0..40).map(|i| format!("p{i}.png")).collect();
        plan_window_jobs(
            &probe,
            &sources,
            &names,
            10,
            (800, 600),
            PreviewKind::Screen,
            DenoisePolicy::Warn,
        )
    };
    assert_eq!(jobs_mid.len(), 6);
    let prios_mid: Vec<u8> = jobs_mid.iter().map(|j| j.priority).collect();
    assert_eq!(prios_mid, vec![0, 1, 2, 3, 4, 5]);
    let ids_mid: Vec<&str> = jobs_mid.iter().map(|j| j.probe_id.as_str()).collect();
    assert_eq!(
        ids_mid,
        vec!["probe-11", "probe-12", "probe-9", "probe-13", "probe-8", "probe-14"]
    );

    // Edge counts: 1,2,200, active at ends
    let cases: Vec<(usize, usize, usize)> =
        vec![(0, 1, 0), (0, 2, 1), (1, 2, 1), (0, 200, 4), (199, 200, 2)];
    for (active, count, expected_len) in cases {
        let jobs = {
            let probe = (0..count).map(|i| format!("p{i}")).collect::<Vec<_>>();
            let sources: Vec<PathBuf> =
                (0..count).map(|i| PathBuf::from(format!("s{i}"))).collect();
            let names: Vec<String> = (0..count).map(|i| format!("n{i}.png")).collect();
            plan_window_jobs(
                &probe,
                &sources,
                &names,
                active,
                (64, 64),
                PreviewKind::Screen,
                DenoisePolicy::Warn,
            )
        };
        assert_eq!(jobs.len(), expected_len, "active={active} count={count}");
        for j in &jobs {
            let idx: usize = j.probe_id.strip_prefix("p").unwrap().parse().unwrap();
            assert!(idx < count);
            assert!(idx != active || count == 1);
        }
    }
}

// ---- T05 GUI-PREV-05: Disk atomar + korrupt => Miss, .tmp nie Hit ----
#[test]
fn disk_corrupt_and_tmp_are_miss() {
    let dir = tempfile::tempdir().unwrap();
    let disk = PreviewDiskCache::in_folder(dir.path()).unwrap();
    let frame = make_frame(4, 4, 77);
    let webp = encode_webp_pub(&frame).unwrap();
    let digest = "test-digest-corrupt";

    // Happy store/load
    disk.store(digest, &webp).unwrap();
    let hit = disk
        .load(digest)
        .unwrap()
        .expect("GUI-PREV-05: stored must hit");
    assert_eq!((hit.width(), hit.height()), (4, 4));

    // Corrupt bytes => Miss, not Err
    let path = dir
        .path()
        .join(".lumina")
        .join("previews")
        .join(format!("{digest}.preview.webp"));
    std::fs::write(&path, b"not webp garbage").unwrap();
    assert!(
        disk.load(digest).unwrap().is_none(),
        "GUI-PREV-05: corrupt webp must be Miss"
    );

    // Simulate interrupted write: .tmp file must never be served as hit, and is pruned.
    let tmp_path = dir
        .path()
        .join(".lumina")
        .join("previews")
        .join(format!("{digest}.{}.tmp", std::process::id()));
    std::fs::write(&tmp_path, b"partial").unwrap();
    assert!(tmp_path.exists());
    assert!(disk.load(digest).unwrap().is_none());
    let pruned = disk.prune_orphans(&[]).unwrap();
    // At least the tmp is removed; corrupted preview also pruned when live empty.
    assert!(pruned >= 1);
    assert!(!tmp_path.exists(), "GUI-PREV-05: .tmp must be pruned");
    assert!(disk.load(digest).unwrap().is_none());
    // Store again correctly after prune
    disk.store(digest, &webp).unwrap();
    assert!(disk.load(digest).unwrap().is_some());
}

// ---- T09 GUI-PREV-07: Disk löschbar + prune verwaister Einträge ----
#[test]
fn disk_prune_and_clear() {
    let dir = tempfile::tempdir().unwrap();
    let disk = PreviewDiskCache::in_folder(dir.path()).unwrap();
    let mut digests = Vec::new();
    for i in 0..7u8 {
        let frame = make_frame(4, 4, i);
        let webp = encode_webp_pub(&frame).unwrap();
        let d = format!("d{i}");
        disk.store(&d, &webp).unwrap();
        digests.push(d);
    }
    assert_eq!(
        std::fs::read_dir(dir.path().join(".lumina").join("previews"))
            .unwrap()
            .count(),
        7
    );
    // Prune to first 3 live
    let live = digests[0..3].to_vec();
    let removed = disk.prune_orphans(&live).unwrap();
    assert_eq!(removed, 4, "GUI-PREV-07: 4 orphans must be pruned");
    for d in &live {
        assert!(disk.load(d).unwrap().is_some(), "live {d} still hits");
    }
    for d in &digests[3..] {
        assert!(disk.load(d).unwrap().is_none(), "pruned {d} must miss");
    }
    disk.clear().unwrap();
    for d in &digests {
        assert!(disk.load(d).unwrap().is_none());
    }
    assert_eq!(
        std::fs::read_dir(dir.path().join(".lumina").join("previews"))
            .unwrap()
            .count(),
        0
    );
}

// ---- T10 GUI-PREV-08: OneToOne Kind-Wechsel invalidiert ----
#[test]
fn kind_switch_invalidates_stale_neighbors() {
    use lumina_core::ImageFileFormat;
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("kind.png");
    let png = make_frame(16, 16, 55).encode(ImageFileFormat::Png).unwrap();
    std::fs::write(&source, &png).unwrap();

    let (mut ctrl, _q) = PreviewController::spawn(1);
    // First as Screen
    ctrl.plan_kind(PreviewKind::Screen);
    ctrl.enqueue(PreviewJob {
        probe_id: "kind".into(),
        source: source.clone(),
        name: "kind.png".into(),
        virtual_copy: "vc-original".into(),
        target: (8, 8),
        kind: PreviewKind::Screen,
        priority: 0,
        denoise_policy: DenoisePolicy::Warn,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while ctrl.probe_state("kind") != PreviewProbeState::Ready && Instant::now() < deadline {
        ctrl.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    ctrl.poll();
    assert_eq!(ctrl.probe_state("kind"), PreviewProbeState::Ready);
    let digest_screen = ctrl.probe_digest("kind").unwrap().to_owned();
    assert!(ctrl.lru().contains(&digest_screen));

    // Switch to OneToOne must invalidate
    ctrl.plan_kind(PreviewKind::OneToOne);
    assert_eq!(
        ctrl.probe_state("kind"),
        PreviewProbeState::Miss,
        "GUI-PREV-08: kind switch must invalidate"
    );
    assert!(
        !ctrl.lru().contains(&digest_screen),
        "GUI-PREV-08: old digest removed from LRU"
    );
    assert!(ctrl.needs_job("kind"));
    assert!(
        ctrl.neighbor_preview("kind", &source).unwrap().is_none(),
        "no silent old hit"
    );

    // Enqueue OneToOne and verify new digest differs and is full 16x16
    ctrl.enqueue(PreviewJob {
        probe_id: "kind".into(),
        source: source.clone(),
        name: "kind.png".into(),
        virtual_copy: "vc-original".into(),
        target: (0, 0),
        kind: PreviewKind::OneToOne,
        priority: 0,
        denoise_policy: DenoisePolicy::Warn,
    });
    let deadline2 = Instant::now() + Duration::from_secs(10);
    while ctrl.probe_state("kind") != PreviewProbeState::Ready && Instant::now() < deadline2 {
        ctrl.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    ctrl.poll();
    assert_eq!(ctrl.probe_state("kind"), PreviewProbeState::Ready);
    let digest_one = ctrl.probe_digest("kind").unwrap().to_owned();
    assert_ne!(digest_screen, digest_one);
    let frame = ctrl
        .neighbor_preview("kind", &source)
        .unwrap()
        .expect("one-to-one hit");
    assert_eq!(
        (frame.width(), frame.height()),
        (16, 16),
        "GUI-PREV-08: OneToOne must keep full resolution"
    );
}
