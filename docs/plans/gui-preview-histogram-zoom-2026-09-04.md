# Plan: Histogramm-Grafik + Preview-Navigator + Zoomstufen + Slider-Save (2026-09-04)

Befunde aus manuellem GUI-Test F-103-N6 (Screenshot Develop, `aircraft-landscape.cr3`):

**Stand der Zeilenangaben (2026-10-20 nachgeprueft):** die Zeilenangaben dieses
Plans beziehen sich auf den Commit `af63f79` (Plan vom 2026-09-04); nackte
`lib.rs`-Anker wurden auf `crates/lumina-gui/src/lib.rs` umgestellt.
- Histogramm ist kein Plot, sondern ein 18px-Clip-Balken (`p01..p99`-Fenster) und wird als
  „Regler über dem Bild" fehlgelesen (`draw_histogram`,
  `crates/lumina-gui/src/lib.rs:5650-5688`).
- Haupt-Preview per Wheel versehentlich auf `Custom`-Zoom + ROI-Crop gestellt
  (verwaschen = Draft), Navigator zeigt Vollbild — ohne sichtbaren Zusammenhang.
- Slider-Edits (`set_adjustment`, `crates/lumina-gui/src/lib.rs:3412`) loggen nur `trace!`
  und rufen nie `save_sidecar` — kein Sidecar, kein INFO-Log, Edit nach Neustart verloren.
Alle Fundstellen sind ueber `git show `af63f79`:<pfad> | sed -n '<zeile>p'` rekonstruierbar.

## Tasks (alle Block A, Details + Abnahme in `Agents.todo.md`)

- **GUI-HISTOGRAM-1 [hoch]:** echtes, hübsches Histogramm als Grafik. 256-Bin-
  Luminanz aus `analyze_tone_with_histogram` (`crates/lumina-core/src/tone.rs:228`),
  per `egui::Painter` als gefüllte Kurve/Bars (~60–80px hoch, Theme-Akzent),
  P01/P99 als schmale Marker-Linien, Mean/Median-Text bleibt. Raus aus dem
  Top-Panel (eigene einklappbare Sektion, Default offen). Leer-/Draft-Zustände
  (`preview_is_draft`, `NotCurrent`) bleiben sichtbar. Kein stiller Fallback.
- **GUI-PREVIEW-NAV-1 [hoch]:** Navigator zeigt das Gesamtbild mit
  Viewport-Rechteck (= aktuell sichtbarer Develop-Arbeitsbereich aus
  `preview_zoom`/`preview_pan`/`roi_from_zoom`); Draggen des Rechtecks pannt
  (`preview_pan` + `mark_dirty`). Navigator-Panel einklappbar (bestehendes
  `navigator_open` weiterverwenden). Zoomstufen **Fit (Default), 25 %, 50 %,
  75 %, 100 % (1:1), 200 %**, FitWidth bleibt. Wheel zoomt nur mit Modifier
  (sonst Scroll/Pan), damit `Custom` nie versehentlich entsteht. Zoom-%-Badge
  + Draft-Badge bleiben sichtbar.
- **GUI-SLIDER-SAVE-1 [hoch]:** Slider-Commit speichert: Debounce-Ende
  (`pending_full_render`-Auflösung,
  `crates/lumina-gui/src/lib.rs:8788-8817`) ruft nach erfolgreichem
  `render_full` `save_sidecar` + `info!`-Log (`<key>=<value> saved`), Status
  „Sidecar saved". Fehler laut (kein stiller Verlust). Preset-/Copy-Paste-Pfade
  unverändert (speichern bereits).
- **CLI-GUI-PARITY-1 [mittel]:** Analyse, ob alle Features korrekt in CLI und
  GUI verankert sind: Matrix Core-Feature × (CLI-Befehl/Flag, GUI-Sektion,
  Sidecar-Rezept-Key). Lücken werden Folge-Tasks (kein Code in dieser Analyse).

## Testpflicht (pro Implementierungs-Task, sonst NICHT BESTANDEN)

- Headless `egui::Context + LuminaApp + tempdir`: Zoomstufen-Mapping
  (25/50/75/100/Fit → `preview_zoom`/`roi_from_zoom`), Pan-Rechteck ↔
  `preview_pan`-Roundtrip, Histogramm-Bins → Plot-Punkte (nicht leer bei
  geladenem Bild), Slider-Commit → Sidecar-Datei existiert + Rezeptwert drin.
- `cargo test -p lumina-gui` grün ohne GPU/WASM; visuelle Änderung zusätzlich
  via kittest Golden/PSNR (Histogramm-Grafik, Navigator-Rechteck).
- `cargo clippy -- -D warnings`, `cargo fmt --check`, WASM
  (`--no-default-features`) grün — Histogramm/Navigator nutzen nur `egui::Painter`
  (portabel, keine nativen Deps).

## Architekturgrenzen

- Nur `lumina-gui` schreiben (ein schreibender Agent). Keine Core-Pipeline-,
  Schema- oder Rezept-Änderung: Zoom/Pan bleiben GUI-Session-State (nie Rezept),
  Histogramm-Bins kommen aus existierendem Core (`crates/lumina-core/src/tone.rs`),
  Save nutzt
  existierendes `save_sidecar_if_unchanged` (CAS-Konflikt bleibt laut).
- `ZoomMode`-Erweiterung (25/50/75) ist GUI-enum-only; `Custom` bleibt für
  Pinch/`+/-`-Feinwerte.
