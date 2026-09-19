//! library scheduling, thumbnails and neighbour prefetch tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn idle_queue_is_bounded_prioritized_and_cancellable() {
    let mut queue = IdleQueue::new(2);
    let low = queue
        .enqueue(
            IdleTask::MaskInference {
                mask_id: "low".into(),
            },
            1,
        )
        .unwrap();
    queue
        .enqueue(
            IdleTask::MaskInference {
                mask_id: "high".into(),
            },
            9,
        )
        .unwrap();
    assert!(queue
        .enqueue(
            IdleTask::MaskInference {
                mask_id: "full".into()
            },
            9
        )
        .is_none());
    assert!(queue.cancel(low));
    assert_eq!(
        queue.pop_next().unwrap().1,
        IdleTask::MaskInference {
            mask_id: "high".into()
        }
    );
    assert!(queue.is_empty());
}

// ---- Display-only crop overlay thumbnail (pure helper) ----

#[test]
fn crop_overlay_rect_maps_normalized_free_crop_into_image_rect() {
    let img = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
    assert!(crop_overlay_rect(None, img).is_none());
    let geometry_none = Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    };
    assert!(crop_overlay_rect(geometry_none.crop.as_ref(), img).is_none());

    // Aspect presets have no normalized rect without the source aspect.
    let aspect = Crop::Aspect {
        preset: lumina_sidecar::AspectPreset::OneToOne,
    };
    assert!(crop_overlay_rect(Some(&aspect), img).is_none());

    let free = Crop::Free {
        x: 0.1,
        y: 0.2,
        width: 0.5,
        height: 0.4,
    };
    let rect = crop_overlay_rect(Some(&free), img).unwrap();
    assert!((rect.min.x - 20.0).abs() < 1e-4);
    assert!((rect.min.y - 30.0).abs() < 1e-4);
    assert!((rect.max.x - 70.0).abs() < 1e-4);
    assert!((rect.max.y - 50.0).abs() < 1e-4);

    // Degenerate crop sizes resolve to no overlay.
    let degenerate = Crop::Free {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.5,
    };
    assert!(crop_overlay_rect(Some(&degenerate), img).is_none());
}

// ---- History restore (non-destructive session state change) ----

#[test]
fn history_restore_swaps_session_recipe_without_touching_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", -2.0);
    app.save_sidecar();

    // Seed one explicit history step holding a different recipe state.
    let mut stored = EditRecipe::default();
    stored.adjustments.insert("exposure".into(), 1.5);
    {
        let document = app.document.as_mut().unwrap();
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == app.virtual_copy_id)
            .unwrap();
        copy.history.push(HistoryEntry {
            id: "history-test-1".into(),
            recipe: stored,
            recorded_at: Some("2026-08-23T10:00:00Z".into()),
            extras: BTreeMap::new(),
        });
    }

    // Restore swaps the session recipe and marks the selection.
    app.restore_history("history-test-1").unwrap();
    assert_eq!(app.recipe().adjustments["exposure"], 1.5);
    assert_eq!(app.history_selected.as_deref(), Some("history-test-1"));

    // Non-destructive: the sidecar still holds the pre-restore recipe.
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == "vc-original")
        .unwrap();
    assert_eq!(copy.recipe.adjustments["exposure"], -2.0);

    // Unknown entry ids fail visibly instead of silently resetting.
    assert!(app.restore_history("nope").is_err());
}

/// Visible cells (+ buffer ring) are enqueued first and in full; off-screen
/// cells receive at most `PREFETCH_BUDGET_PER_FRAME` nearest-first jobs.
#[test]
fn thumbnail_scheduling_prioritizes_visible_over_off_screen() {
    let (mut app, _dir, indices) = app_with_entries(40);
    let keys: Vec<String> = indices
        .iter()
        .map(|&i| app.entries()[i].thumb_key().to_owned())
        .collect();
    let ctx = egui::Context::default();

    // Viewport shows cells 0..10 → buffered 0..18 immediate + 4 prefetch.
    let window = 0..10;
    let buffered_len = viewport::buffered_range(
        window.clone(),
        indices.len(),
        viewport::VISIBLE_BUFFER_CELLS,
    )
    .len();
    let enqueued = app.ensure_thumbnail_priority(&ctx, &indices, window);
    assert_eq!(
        enqueued,
        buffered_len + viewport::PREFETCH_BUDGET_PER_FRAME,
        "visible work must be unbounded, off-screen work capped per frame"
    );
    for (i, key) in keys.iter().enumerate().take(buffered_len) {
        assert!(
            !app.thumbnails.needs_job(key),
            "cell {i} (visible/buffered) must be scheduled"
        );
    }
    for key in keys
        .iter()
        .skip(buffered_len + viewport::PREFETCH_BUDGET_PER_FRAME)
    {
        assert!(
            app.thumbnails.needs_job(key),
            "distant cell must NOT be scheduled yet"
        );
    }

    // Re-scheduling the same window must never redo visible work: every
    // visible cell is already in flight/done, so only the bounded
    // off-screen prefetch progresses (≤ budget per frame).
    let again = app.ensure_thumbnail_priority(&ctx, &indices, 0..10);
    assert!(
        again <= viewport::PREFETCH_BUDGET_PER_FRAME,
        "identical schedule call must stay within the prefetch budget, got {again}"
    );
    let visible_again =
        viewport::buffered_range(0..10, indices.len(), viewport::VISIBLE_BUFFER_CELLS);
    for key in keys.iter().take(visible_again.len()) {
        assert!(
            !app.thumbnails.needs_job(key),
            "visible cell must not be rescheduled"
        );
    }
}

/// Scrolling to the end schedules the end first and prefetches backwards
/// from there (nearest-first), never the far-away start.
#[test]
fn thumbnail_scheduling_follows_the_viewport_nearest_first() {
    let (mut app, _dir, indices) = app_with_entries(40);
    let keys: Vec<String> = indices
        .iter()
        .map(|&i| app.entries()[i].thumb_key().to_owned())
        .collect();
    let ctx = egui::Context::default();
    // First frame: user is at the top.
    app.ensure_thumbnail_priority(&ctx, &indices, 0..10);
    // Then scrolls to the very end without idle time to prefetch.
    let count = indices.len();
    let enqueued = app.ensure_thumbnail_priority(&ctx, &indices, count - 2..count);
    assert_eq!(enqueued, 10 + viewport::PREFETCH_BUDGET_PER_FRAME);
    for key in keys.iter().skip(count - 12) {
        assert!(
            !app.thumbnails.needs_job(key),
            "end-of-list cell must be scheduled"
        );
    }
    // Prefetch walked backwards from index 30: 29, 28, 27, 26.
    for key in &keys[26..30] {
        assert!(
            !app.thumbnails.needs_job(key),
            "nearest prefetch cell must be scheduled"
        );
    }
    // The middle of the list was never touched by this short session.
    for key in &keys[23..26] {
        assert!(
            app.thumbnails.needs_job(key),
            "middle cell must remain untouched"
        );
    }
    for key in keys.iter().take(18.min(count)) {
        assert!(
            !app.thumbnails.needs_job(key),
            "first-frame cells stay scheduled"
        );
    }
}

/// A failed worker frees its in-flight slot and stays inside the bounded
/// retry budget — scheduling never re-enqueues beyond
/// [`filmstrip::THUMBNAIL_MAX_ATTEMPTS`] (REVIEW-GUI-THUMB-2 regression).
#[test]
fn thumbnail_scheduling_respects_retry_budget() {
    let (mut app, _dir, indices) = app_with_entries(1);
    let key = app.entries()[indices[0]].thumb_key().to_owned();
    let ctx = egui::Context::default();
    // First schedule enqueues exactly one job for the single visible cell.
    assert_eq!(app.ensure_thumbnail_priority(&ctx, &indices, 0..1), 1);
    // While that job is in flight (or has already finished) nothing is
    // rescheduled — no duplicate jobs regardless of worker speed.
    assert_eq!(
        app.ensure_thumbnail_priority(&ctx, &indices, 0..1),
        0,
        "in-flight/done cells must not be rescheduled"
    );
    // Each reported worker failure consumes one bounded retry unit
    // (REVIEW-GUI-THUMB-2); the schedule path must honour that budget.
    for _ in 0..filmstrip::THUMBNAIL_MAX_ATTEMPTS {
        app.thumbnails.mark_failed(&key, "boom");
    }
    assert_eq!(
        app.ensure_thumbnail_priority(&ctx, &indices, 0..1),
        0,
        "exhausted retry budget must not be rescheduled"
    );
    assert_eq!(
        app.thumbnails.failure(&key),
        Some("boom"),
        "exhausted retries must stay a visible error, never a silent fallback"
    );
}

// ---- PREVIEW-CACHE-FEATURE: neighbor prefetch scheduling (native) ----

/// Scheduling the neighbor window around the active image must lazily spawn
/// the controller and enqueue exactly the available +4/−2 neighbors in the
/// mandated priority order (no wrap at the edges).
#[test]
fn neighbor_prefetch_schedules_asymmetric_window_around_active() {
    let (mut app, _dir, indices) = app_with_entries(8);
    let active_path = app.entries()[indices[3]].path.display().to_string();
    app.schedule_neighbor_previews(&active_path);
    let ctrl = app.preview_ctrl.as_ref().expect("lazily spawned");
    let mut probes = ctrl.in_flight_probes();
    probes.sort();
    // Active pic3 of 8: window +1..+4, −1..−2 → indices 4,5,2,6,1,7.
    let expected: Vec<String> = [1usize, 2, 4, 5, 6, 7]
        .into_iter()
        .map(|i| app.entries()[indices[i]].thumb_key().to_owned())
        .collect();
    assert_eq!(
        probes.len(),
        6,
        "+4/−2 window on a mid folder = 6 neighbors"
    );
    for want in &expected {
        assert!(probes.contains(want), "scheduled {want}");
    }
    // The active image itself is never a prefetch target (GPU texture only).
    assert!(!probes
        .iter()
        .any(|p| *p == app.entries()[indices[3]].thumb_key()));

    // A second schedule for the same active must not enqueue duplicates
    // (one job per key: in-flight/done probes are skipped).
    let again_enqueued = app.schedule_neighbor_previews(&active_path);
    assert_eq!(again_enqueued, 0, "identical window is fully deduplicated");
}

/// At the folder start the window shrinks (no wrap-around).
#[test]
fn neighbor_prefetch_window_shrinks_at_folder_edge() {
    let (mut app, _dir, indices) = app_with_entries(8);
    let start_path = app.entries()[indices[0]].path.display().to_string();
    let enqueued = app.schedule_neighbor_previews(&start_path);
    assert_eq!(enqueued, 4, "+1..+4 only, no backward wrap");
    let ctrl = app.preview_ctrl.as_ref().unwrap();
    let probes = ctrl.in_flight_probes();
    assert_eq!(probes.len(), 4);
    let expected: Vec<String> = [1usize, 2, 3, 4]
        .into_iter()
        .map(|i| app.entries()[indices[i]].thumb_key().to_owned())
        .collect();
    for want in &expected {
        assert!(probes.contains(want), "scheduled {want}");
    }
}

/// A directory change discards the neighbor-cache state for the previous
/// folder (RAM LRU, jobs, failures) so stale entries never resurface.
#[test]
fn directory_change_resets_neighbor_cache() {
    let (mut app, _dir, indices) = app_with_entries(4);
    let active_path = app.entries()[indices[0]].path.display().to_string();
    app.schedule_neighbor_previews(&active_path);
    let ctrl = app.preview_ctrl.as_ref().unwrap();
    assert!(!ctrl.in_flight_probes().is_empty(), "jobs enqueued");
    let second = tempfile::tempdir().unwrap();
    app.set_directory(second.path().display().to_string());
    let ctrl = app.preview_ctrl.as_ref().unwrap();
    assert!(ctrl.lru().is_empty(), "RAM LRU cleared on directory change");
    assert!(
        ctrl.in_flight_probes().is_empty(),
        "in-flight jobs cleared on directory change"
    );
}

// ---- REVIEW-GUI-N4: IdleQueue FIFO tie-break ----

#[test]
fn idle_queue_pops_fifo_within_same_priority() {
    let mut queue = IdleQueue::new(4);
    queue
        .enqueue(
            IdleTask::MaskInference {
                mask_id: "first".into(),
            },
            5,
        )
        .unwrap();
    queue
        .enqueue(
            IdleTask::MaskInference {
                mask_id: "second".into(),
            },
            5,
        )
        .unwrap();
    queue
        .enqueue(
            IdleTask::MaskInference {
                mask_id: "high".into(),
            },
            9,
        )
        .unwrap();
    // Higher priority first…
    assert_eq!(
        queue.pop_next().unwrap().1,
        IdleTask::MaskInference {
            mask_id: "high".into()
        }
    );
    // …then the *earliest*-enqueued task of the equal-priority class
    // (the old `max_by_key` implementation returned the last maximum →
    // LIFO and popped "second" first).
    assert_eq!(
        queue.pop_next().unwrap().1,
        IdleTask::MaskInference {
            mask_id: "first".into()
        }
    );
    assert_eq!(
        queue.pop_next().unwrap().1,
        IdleTask::MaskInference {
            mask_id: "second".into()
        }
    );
    assert!(queue.pop_next().is_none());
}
