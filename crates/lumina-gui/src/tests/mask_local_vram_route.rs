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

#[cfg(feature = "gpu")]
#[test]
fn local_mask_vram_route_pairs_current_cpu_planes_and_refuses_misalignment() {
    let (_directory, mut app, _source) = super::local_app();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.render().unwrap();
    let (width, height) = app
        .original
        .as_ref()
        .map(|source| (source.width(), source.height()))
        .unwrap();
    // This headless route seam supplies the evaluated result explicitly; GPU
    // execution and mask-raster parity remain covered by their adapter tests.
    let layer_id = app.selected_mask_layer().unwrap().id.clone();
    app.render_mask_layers = vec![lumina_core::MaskLayerResult {
        layer_id,
        plane: lumina_core::MaskPlane::new(
            width,
            height,
            vec![u16::MAX; (width * height) as usize],
        )
        .unwrap(),
    }];
    let layers = app.gpu_local_mask_layers(width, height).unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].layer_id, app.selected_mask_layer().unwrap().id);

    app.render_mask_layers[0].plane.width -= 1;
    assert!(app
        .gpu_local_mask_layers(width, height)
        .unwrap_err()
        .contains("does not align with source VRAM"));
    assert!(!app.prime_gpu_present());
    assert!(!app.vram_fresh);
    assert!(app.preview.is_some());
}
