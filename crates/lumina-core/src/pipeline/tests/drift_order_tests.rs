//! PIPELINE-ORDER-DRIFT: quelltextbasierter Drift-Schutz zwischen der
//! nominalen Pipeline-Reihenfolge (`Pipeline::default()`) und der
//! tatsächlichen Stufenfolge im Renderer. `render.rs` ist zur Laufzeit
//! nicht introspektierbar (die Stufen sind einzelne Funktionsaufrufe in
//! `render_frame_from_base_impl`), daher liest dieser Test den Quelltext
//! zur Compile-Zeit und prüft die **relative Zeilenordnung** der Anker.
//!
//! Anker (erstes Aufruf-Vorkommen im Funktionskörper, d. h. `Anker(` —
//! Kommentare und Aufrufe in anderen Funktionen finden sich so nicht):
//!   - `apply_spot_heals_from_recipe(`              → SpotHeal
//!   - `apply_recipe_with_scale_white_balance_and_denoise(` → Adjustments
//!   - `composite_expand(`                         → Crop (GenerativeExpand)
//!   - `apply_crop_stage(`                          → Crop
//!   - `evaluate_mask_stage(`                       → Masks
//!   - `apply_local_adjustments(`                   → Masks (lokale Anwendung)
//!
//! Erwartete Ordnung: exakt die nominale Reihenfolge der modellierten
//! Stufen `SpotHeal → Adjustments → Crop → Masks`. Die Geometrie-
//! Unterschritte laufen innerhalb der `Crop`-Stufe und sind über
//! `GEOMETRY_STAGE_ORDER` abgesichert; hier zählt nur, dass die Crop-Stufe
//! (`apply_crop_stage`) **vor** der Masks-Stufe liegt.
//!
//! Stabilität: Suche innerhalb des Funktionskörpers, nur an Aufruf-
//! Stellen; Kommentare/Zeilenumbrüche verschieben die Anker, ohne die
//! Ordnung zu ändern. Eine Umstellung der Stufen macht den Test rot.
//! `include_str!` ist relativ zu dieser Datei: `../../render.rs` ist
//! `crates/lumina-core/src/render.rs`.

#[test]
fn render_stage_order_matches_nominal_pipeline_order() {
    let source = include_str!("../../render.rs");
    let body = render_frame_from_base_impl_body(source);
    let anchors = [
        ("apply_spot_heals_from_recipe(", "SpotHeal"),
        (
            "apply_recipe_with_scale_white_balance_and_denoise(",
            "Adjustments",
        ),
        ("composite_expand(", "Crop (GenerativeExpand)"),
        ("apply_crop_stage(", "Crop"),
        ("evaluate_mask_stage(", "Masks"),
        ("apply_local_adjustments(", "Masks (lokale Adjustments)"),
    ];
    let mut previous: Option<(usize, &str)> = None;
    for (needle, stage) in anchors {
        let offset = body.find(needle).unwrap_or_else(|| {
            panic!(
                "Anker `{needle}` ({stage}) nicht in `render_frame_from_base_impl` \
                 gefunden — die Stufenstruktur hat sich geändert"
            )
        });
        let line = body[..offset].matches('\n').count() + 1;
        if let Some((prev_line, prev_stage)) = previous {
            assert!(
                prev_line < line,
                "Stufen-Drift: `{prev_stage}` (Zeile {prev_line}) muss in `render.rs` \
                 vor `{stage}` (Zeile {line}) liegen — Nominalliste: \
                 SpotHeal → Adjustments → Crop → Masks"
            );
        }
        previous = Some((line, stage));
    }
}

/// Isoliert den Funktionskörper von `render_frame_from_base_impl` aus dem
/// Quelltext: ab der Signatur bis zur schließenden Klammer auf Spalte 0.
/// Damit werden Aufrufe in anderen Funktionen (z. B. die SpotHeal-Anwendung
/// in der Auto-Analyse-Domäne) von der Anker-Suche ausgeschlossen.
fn render_frame_from_base_impl_body(source: &str) -> &str {
    let signature = "fn render_frame_from_base_impl(";
    let start = source
        .find(signature)
        .expect("Signatur `render_frame_from_base_impl` nicht in render.rs gefunden");
    let rest = &source[start..];
    let end = rest
        .find("\n}")
        .expect("Funktionsende von `render_frame_from_base_impl` nicht gefunden");
    &rest[..end]
}
