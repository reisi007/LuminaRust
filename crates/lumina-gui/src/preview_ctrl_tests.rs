//! Unit tests split from the over-budget preview controller module.

use super::*;

// ---- T08 GUI-FILM-01: Worker-Prio-Queue sortiert nach priority ----
#[test]
fn preview_queue_respects_priority_not_fifo() {
    let queue = PreviewQueue::default();
    // Push mixed priorities; insertion order deliberately shuffled.
    for prio in [3u8, 0, 5, 2, 1, 4] {
        queue.push(PreviewJob {
            probe_id: format!("p{prio}"),
            source: PathBuf::from(format!("/tmp/a{prio}")),
            name: format!("a{prio}.png"),
            virtual_copy: "vc-original".into(),
            target: (64, 64),
            kind: PreviewKind::Screen,
            priority: prio,
            denoise_policy: DenoisePolicy::Warn,
        });
    }
    let mut popped = Vec::new();
    for _ in 0..6 {
        popped.push(queue.pop().job.priority);
    }
    assert_eq!(
        popped,
        vec![0, 1, 2, 3, 4, 5],
        "GUI-FILM-01: pop must be prio-sorted"
    );

    // Larger batch maintains sorting
    let queue2 = PreviewQueue::default();
    for prio in (0..20).rev() {
        queue2.push(PreviewJob {
            probe_id: format!("q{prio}"),
            source: PathBuf::from(format!("/tmp/b{prio}")),
            name: format!("b{prio}.png"),
            virtual_copy: "vc-original".into(),
            target: (32, 32),
            kind: PreviewKind::Screen,
            priority: (prio % 6) as u8,
            denoise_policy: DenoisePolicy::Warn,
        });
    }
    let mut last = 0u8;
    let mut first = true;
    for _ in 0..20 {
        let queued = queue2.pop();
        if !first {
            assert!(
                queued.job.priority >= last,
                "GUI-FILM-01: prio must be non-decreasing"
            );
        }
        last = queued.job.priority;
        first = false;
    }
}
