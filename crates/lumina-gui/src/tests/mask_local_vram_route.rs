#[cfg(feature = "gpu")]
#[test]
fn local_mask_vram_present_is_refused_and_cpu_preview_remains_authoritative() {
    let (_directory, mut app, _source) = super::local_app();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.vram_fresh = true;

    // This refusal is checked before adapter availability, so the semantic
    // route is testable on the headless host: no GPU attempt may mark the
    // global-only VRAM result fresh, while the full mask-aware CPU render runs.
    assert!(!app.prime_gpu_present());
    assert!(!app.vram_fresh);
    app.render_draft([800, 600], None).unwrap();
    assert!(!app.preview_is_draft);
    assert!(app.preview.is_some());
    assert!(app.local_adjustment_route_reason().is_some());
}
