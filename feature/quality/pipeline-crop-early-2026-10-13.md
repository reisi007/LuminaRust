# Pipeline-Crop-Early — Messung 2026-10-13 (E1, KEINE Umordnung)

Status: **E1 umgesetzt** — Messung und Probe sind Referenz; die **Umordnung selbst
ist nicht umgesetzt** (Owner-Entscheid offen). Task `PIPELINE-CROP-EARLY-9`
bleibt offen — aber die drei Vorbedingungen (a)(b)(c) aus
`PIPELINE-ANNASSUNGEN-10` sind mit eigenen Messwerten auf derselben Maschine
belegt, und die Gewichtung unten ist Ergebnis der Umordnung-Probe.

## 1. Methodik

- Toolchain: Rust 1.99.0 (stable, neu installiert — `~/.cargo` war leer; in den
  Repo-Baum fließt NICHTS ein).
- Apparatur: bestehende Produktions-Primitive im gleichen Prozess, Two-Order-
  Vergleich per `assert_eq!` auf RGBA-Kanälen. Release-Mode; Debug-Mode
  cross-checked und kongruent.
- Frame: adversarischer Synthetik-Frame (Ranges + 16-px-Checkerboard + per-pixel
  Rauschen, 512×512 — Worst-Case für Resampler-Divergenz).
- Geometrie in der Probe: Perspective `vertical: 0.6` ("wedge_perspective"),
  Lensfun/Manuelle Lens auf Default, d.h. die Geometrie-Kette **läuft**.
- Crop als normalisierter `Crop::Free` rect, 2500 / 250 px (10 % Restfläche) und
  vollframes.
- Timed run: 12 Iterationen nach Warmup, `std::time::Instant`, 3 unabhängige
  Durchläufe.
- Strikte Ausschlussregel: **keine** Mutation an Produktionsdateien — nach der
  Probe `git status` unberührt, HEAD `b7c0799` byte-identisch an Source.
- Dateien nicht komittiert: keine. Probe-Testmodul wurde vollständig entfernt.

## 2. Gemessen — Per-pixel Tonalität kommutiert NUR mit Crop, nicht mit Warp

`apply_recipe_with_scale_white_balance_and_denoise` als Ganzes (als „Adjustments-
Bündel" vor Geometry vs. nach Crop) auf dem adversarischen Fixture:

| Rezept | Pixel | abweichende RGB-Kanäle | max Δ | Mittel Δ | byte-identisch |
|---|---|---|---|---|---|
| Geometrie frei, Tonalität nur | 98 304 | 0 (0,00 %) | 0 | 0,000 | **ja** |
| vollflächiges Rect, Tonalität nur | 98 304 | 0 (0,00 %) | 0 | 0,000 | **ja** |
| 10 % Restrect, Tonalität nur | 9 801 | 0 (0,00 %) | 0 | 0,000 | **ja** |
| **Perspective + 10 % Crop**, nur Tone | 13 760 | 6 018 (14,58 %) | 12 | 1,550 | **nein** |

Der Crop selbst kommutiert kommutativ mit Tonalität (uint8-Funktion des Werts).
Eine **Resampling-Reststage (Perspective/Lens) bricht es**: u8-Rundung vor dem
Warp ändert, welcher Pixel durch Bilinear-`sample()` nach `floor()` anderweitig
gelesen wird; die sechs Tonalitäts-Slider addieren sich auf (Clamp/Round +
resample → 66,54 % der Kanäle, max Δ 20, Mittel 17,3 LSB). Zuzüglich Curves/HSL/
PointColor/Vibrance sitzt man bei 73,28 % der Kanäle, max Δ 77, Mittel 32,6.

## 3. Sieben räumlich gekoppelte Sub-Stufen in `apply_recipe`

Jede nicht null (Probe erzeugt Abweichung) — die Abweichung ist also nicht
vacuos:

| Sub-Stufe | abw. Kanäle |
|---|---|
| presence/texture (clarity) — DoG | 71,44 % |
| presence (dehaze — Airlight 95. Percentil) | 44,73 % |
| Noise Reduction — bilateral | 27,28 % |
| Sharpening — Gradient/Effektiv-Scale | 48,11 % |
| Vignette — Mitte + `r_min/r_max`-Normieren | 43,62 % |
| Grain — Seed/Geometrie/DIM | 91,47 % |
| Red-eye — Radius zur Mitte | 38,58 % |

→ Eine Umordnung kann **nicht** das ganze `apply_recipe` nach hinten ziehen;
nur der per-pixel Tonal-Halbe (WB/Exposition/Kontrast/Shadows/Highlights/
Whites/Blacks, Curves, HSL, PointColor, Vibrance/Saturation, Color Grading,
Color-Harmony) kann ohne byte-Wirkung auf Crop-only-Rezepten re-positioniert
werden.

## 4. Gemessener Gewinn (Zugrunde liegend bei 10 % Restfläche, Ton-only)

| Reihenfolge | ms/Render (3 Durchläufe) |
|---|---|
| HEAD (Tonalität vor Geometrie) | 477,5 / 521,9 / 503,4 |
| Umgestellt (per-Pixel-Hälfte nach Crop) | 170,3 / 141,5 / 146,6 |

→ **≈2,8–3,4 ×** schneller, **64–71 %** weniger Wandzeit auf einem Ton-R render
mit 10 % Restfläche. Lauffähigkeit gemessen, nicht gewertet auf Photo-Content:
99 % dieser Numbers (±1 LSB) kommen aus Rundungs-Verkettung und würden auf
einem weichen Gradientenbild um ein Vielfaches kleiner werden.

## 5. Abhängigkeit, die die Umordnung blockiert (harte/weiche Grenzen)

1. **Hart (Koordinaten):** Crop läuft auf der Leinwand, Perspective/Expand im
   Kettenbefund nach `crop_rect(width, height, crop)`. Ein normalisierter Crop-Rect
   liefert 81×61 pre-warp vs 165×82 post-warp auf demselben Bild. Crop kann nicht
   vor Perspektive/Expand; die Geometrie-Kette ist damit volle Bildkosten.
2. **Weich (Identität):** Der eine **sichere** Stufe (per-pixel Tonality) steht
   heute **vor** Geometry; hinter Crop geschoben ändert es Bytes, sobald ein Warp
   aktiv ist. Crop-only-Rezepte bleiben byte-identisch.
3. **Sind exempt von der Umordnung (vermutbar):** `GEOMETRY_STAGE_ORDER`
   (Pipeline, `pipeline.rs:50`) — nur der Koordinaten-Ausdruck in den coarse-
   Stadien Counts. `mask_input_width/height` stays pre-Crop (Zeilen 611/612) —
   die Masken-Komposition geschieht in beiden Ordnungsrichtungen nach Crop.
4. **Auto-Tone-Domäne:** unverändert — `AUTO-TONE-ANALYSIS-INPUT-8` ist
   Analyse-Domäne; diese Task berührt Rechenreihenfolge, nicht die Analyse.

## 6. Entscheidung (Owner-Freigabe per `question` 2026-10-13)

**E1 gewählt (empfohlen): Messung referenziert, Umordnung ausgesetzt.**

Umordnung nur möglich, wenn der Eigentümer die Bytes-Änderung (14,58 % der
Kanäle bei 10 % Crop, max Δ12, Mittel 1,55 auf adversarial; 66,54 % mit
Curves/HSL/Vibrance) bewusst als Gewichtung auf die gemessene Speedup akzeptiert.
Ohne diese Freigabe ist E2b nicht wertbar ("nein-Bytes sind Abnahmevertrag").

## 7. Migrationspfad E2b (falls später ausgelöst)

1. Extrahieren des per-Pixel-Halbes `apply_recipe_with_scale_white_balance_and_
   denoise` in privaten Helper (`apply_per_pixel_tonal`), räumlicher Halbe und
   Validierung bleiben an der Spitze.
2. Re-Positionierung des Helper-Aufrufs **hinter** `apply_crop_stage`, vor
   `apply_lens_blur` und Masken, in `render_frame_from_base_impl`, im GPU-/
   Dev-Mirror und allen UI/CLI-Partial-Pfaden.
3. `mask_input_*` und `effective_scale` unverändert.
4. Auto-Tone-Attribution unverändert.

## 8. Akzeptanz-Schriften für einen künftigen E2b-Commit

- Byte-Identität render/export png+jpeg/batch für **warp-freie** Rezepte,
  inkl. vollflächiger Crop.
- Masken-Positionstest mit und ohne Crop (Produktionspfad).
- Generative-Expand-Byte-Test (Artifact-Abmessungen unverändert — Expand bleibt
  vor Crop).
- Perspective + Crop bewusst dokumentiert als geänderte Bytes, mit der messbasierten
  Klasse (14,58 % @ max Δ12 @ 10 % crop) als Verweiswert in `pipeline.md` und
  `Agents.md`.
- Crop 50 / 25 / 10 % vor/nach-Bench auf der Referenzmaschine neu.
- `check_file_sizes.sh` grün; keine Baseline-Anhebung; `pipeline.rs`-Docs-als-
  Test passen (Coarse-Liste unverändert).

## 9. Nicht-Bezogenes / Lücken

- 15–73 % sind **adversarial**, nicht photorealistisch; Mutmaßung aus
  Einzel-Slider-Evidenz ~8 % ±1 LSB auf echtem Photo.
- nur manuelles Lens-Modell gemessen; Lensfun-Pfad nicht.
- keine Sidecar-Schema-Änderung, keine neuen Cache-Keys, keine Auto-Tone-
  attribution nötig — das fühlt sich an wie ein No-Op, ist aber ein eigenes
  Gateset (`GOLDEN-CIGATE-62`, `PARITY-MATRIX-ASSERT-64`) und sollte vor
  E2b-Einzug fehlen.

## Bezirk-Referenzen

- Plantask: `Agents.todo.md` § `[PRIO: mittel] PIPELINE-CROP-EARLY-9`
- Vorbedingung (belegt): `PIPELINE-ANNASSUNGEN-10` + `crates/lumina-bench/bench/crop_pipeline.rs`
- DoD §9