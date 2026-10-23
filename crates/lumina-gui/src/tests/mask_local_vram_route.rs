#[cfg(feature = "gpu")]
#[test]
fn failed_cpu_render_prevents_both_live_gpu_callbacks() {
    let mut fresh = true;
    let global_called = std::cell::Cell::new(false);
    let mask_called = std::cell::Cell::new(false);
    let route = crate::LuminaApp::run_live_gpu_present(
        &mut fresh,
        false,
        None,
        &[],
        || {
            global_called.set(true);
            Ok::<_, &'static str>(())
        },
        |_| {
            mask_called.set(true);
            Ok(())
        },
    );
    assert!(matches!(
        route,
        crate::gpu_routing::local_adjustments::LiveVramPresent::Refused(_)
    ));
    assert!(!global_called.get());
    assert!(!mask_called.get());
    assert!(!fresh);
}

#[cfg(feature = "gpu")]
#[test]
fn live_present_freshness_requires_global_and_mask_success() {
    let (_directory, mut app, _source) = super::local_app();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.render().unwrap();
    let (width, height) = app
        .original
        .as_ref()
        .map(|source| (source.width(), source.height()))
        .unwrap();
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

    let mut fresh = true;
    let route = crate::LuminaApp::run_live_gpu_present(
        &mut fresh,
        true,
        None,
        &layers,
        || Ok::<_, &'static str>(()),
        |_| Err("mask composition failed"),
    );
    assert!(matches!(
        route,
        crate::gpu_routing::local_adjustments::LiveVramPresent::Failed("mask composition failed")
    ));
    assert!(!fresh, "failed local composition must not mark VRAM fresh");

    let global_called = std::cell::Cell::new(false);
    let mut fresh = false;
    let route = crate::LuminaApp::run_live_gpu_present(
        &mut fresh,
        true,
        None,
        &layers,
        || {
            global_called.set(true);
            Ok::<_, &'static str>(())
        },
        |current| {
            assert_eq!(current, layers);
            Ok(())
        },
    );
    assert!(matches!(
        route,
        crate::gpu_routing::local_adjustments::LiveVramPresent::Presented
    ));
    assert!(global_called.get());
    assert!(fresh, "global + mask success publishes freshness");
}

#[cfg(feature = "gpu")]
#[test]
fn local_mask_gpu_route_refuses_preview_geometry_mismatch() {
    let (_directory, mut app, _source) = super::local_app();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.render().unwrap();
    assert!(app.preview.is_some());
    let (width, height) = app
        .original
        .as_ref()
        .map(|source| (source.width(), source.height()))
        .unwrap();

    app.preview_roi = Some([0, 0, width.saturating_sub(1), height]);
    let reason = app
        .local_mask_vram_geometry_refusal(width, height)
        .expect("ROI CPU previews cannot be paired with full-frame VRAM");
    assert!(reason.contains("does not match full source"), "{reason}");
    let called = std::cell::Cell::new(false);
    let mut fresh = true;
    let route = crate::LuminaApp::run_live_gpu_present(
        &mut fresh,
        true,
        Some(reason),
        &[],
        || {
            called.set(true);
            Ok::<_, &'static str>(())
        },
        |_| {
            called.set(true);
            Ok(())
        },
    );
    assert!(matches!(
        route,
        crate::gpu_routing::local_adjustments::LiveVramPresent::Refused(_)
    ));
    assert!(!called.get());
    assert!(!fresh);

    app.preview_roi = None;
    app.preview_is_draft = true;
    assert!(app
        .local_mask_vram_geometry_refusal(width, height)
        .is_some());
}

#[cfg(feature = "gpu")]
#[test]
fn malformed_mask_plane_is_refused_and_cpu_preview_remains_available() {
    let (_directory, mut app, _source) = super::local_app();
    app.set_mask_local_presence("texture", 0.5).unwrap();
    app.render().unwrap();
    let (width, height) = app
        .original
        .as_ref()
        .map(|source| (source.width(), source.height()))
        .unwrap();
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
    app.render_mask_layers[0].plane.values.pop();
    let refusal = app.gpu_local_mask_layers(width, height).unwrap_err();
    assert!(refusal.contains("malformed plane data"), "{refusal}");
    assert!(!refusal.contains("P1.2"), "wrong refusal reason: {refusal}");
    app.vram_fresh = true;
    app.note_vram_refusal(&refusal);
    assert!(!app.vram_fresh);
    assert!(app.preview.is_some());
}
