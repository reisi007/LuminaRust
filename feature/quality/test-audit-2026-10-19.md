# Test-Audit — Runde 2026-10-19 (TEST-AUDIT-36)

Befundbericht der wöchentlichen Redundanz-Runde. **Nichts wurde gelöscht, nichts
geändert** — dieser Bericht ist die Abnahmeleistung; jede Löschung läuft über
SOLL → Implementierungs-Agent → unabhängige Verifikation (Aufgabenstellung in
`Agents.todo.md`, Eintrag `TEST-AUDIT-36`).

**Baum:** HEAD `c2c2ce1` („refactor(core): unify the four downscale paths on one
kernel"). Sessionsbeginn und -ende auf demselben Commit; der einzige Vorbestand
im Arbeitsbaum war ` M agent-harness/Cargo.lock` (nicht berührt). Für die
Mutationsmessungen lief die Runde auf einem **exklusiven** Baum (Regel
`PARALLELITAeTS-BEFUND`/F-P1): alle drei Mutationsziele wurden byte-identisch
restauriert (SHA-256 vor/nach unten), `git status --porcelain` zeigt am Ende
keine Test- oder Logikänderung.

Der neue Bericht wird **nicht** in `feature/README.md` indexiert: der
Vorrundenbericht `test-audit-2026-10-12.md` ist dort ebenfalls nicht gelistet
(`grep -n "test-audit" feature/README.md` → kein Treffer), deshalb bleibt die
Indexierung aus Konsistenz unverändert.

**Anker-Form (Stand 2026-10-20, Welle H zu `PROSE-ANCHOR-POLICY-88`):** die
Fundstellen dieses Berichts nennen den vollständigen repo-relativen **Pfad +
Symbol- bzw. Testnamen** — **keine** Zeilennummern mehr. Die Zeilennummern
dieses Berichts galten gegenüber dem **Audit-Stand `c2c2ce1`** (2026-10-19);
sie wurden über die Historie rekonstruiert (`git show c2c2ce1:<pfad> | sed -n
'<zeile>p'` — der umschließende Symbolträger) und in Pfad + Symbol
umgestellt. Wo ein Symbol heute umgezogen ist, nennt der Anker den
**heutigen** Pfad; der damalige Stand bleibt als datierter Beleg
(Commit + Fundstelle) erhalten. Die inhaltlichen Befunde dieses Berichts
sind **unverändert** — umgestellt wurde nur die Anker-Form, nicht die
Aussage.

**Rotationshinweis — neu seit 2026-10-12:** `cb2da59` legte das SOLL
`feature/quality/downscale-ein-pfad-soll.md` an, `c2c2ce1` konsolidierte die
**vier** Downscale-Pfade (core `downscale_bilinear`, `ImageFrame::downscale`,
gui `filmstrip::downscale_rgba`, gui `preview_jobs::downscale_to_target`) auf
**einen** f64-Kernel `lumina_core::downscale` mit zwei Zielformen. Damit ist
`crates/lumina-core/tests/downscale_anchor.rs` (10 Tests +
`downscale_anchor_support/mod.rs`) neu im Baum. Die Vorrunden-Befunde **B1** und
**G3** sind behoben, **B2** und **G1** sind geschlossen, **G2** ist als
dokumentierte Grenze geschlossen. Die neue Datei trägt die Lücken F1–F3.

## Gemessene Basis

| Messung | Ergebnis |
|---|---|
| `cargo test --workspace --all-targets -- --list` | **139** Testbinaries (`Running`-Zeilen = `Executable`-Zeilen aus `--no-run`), davon **130** libtest-Binaries mit **3117** listbaren Tests; die übrigen **9** sind Criterion-Benches (`bench/*.rs` ×7, `benches/*.rs` ×2) ohne libtest-Ausgabe |
| `cargo test -p lumina-gui --all-targets` (single-threaded) | **22** Testbinaries, **989 passed / 0 failed / 82 ignored** (1071 total); Summe der per-Ziel `finished in`-Zeiten **91,55 s** |
| GUI-lib-Ziel | **950 passed / 0 failed / 5 ignored**, **56,61 s** |
| `kittest_snapshots` | **56** Tests, auf diesem Host **alle 56** `#[ignore]`, **0,00 s** |
| `kittest_parity` | **7** Tests = **1 passed + 6 ignored** (die GPU-/Matrix-Tests bleiben `#[ignore]`; der eine laufende ist der adapterlose SKIP-Zweig-Test, `PARITY-PATHS-2`) |
| `cargo test -p lumina-core --test downscale_anchor` | **10 passed / 0 failed** |
| `cargo test -p lumina-mcp --test mcp downscale` | **1 passed** |
| `cargo test -p lumina-cull` | **45 passed / 0 failed** |
| `cargo test -p lumina-gui --lib histogram` | **15 passed** (lib total 950) |

**Vormerkung zur Workspace-Zahl:** Die Vorrunde nannte **119** Testbinaries /
**64** libtest-Binaries / **1609** listbare Tests. Mit demselben Kommando
(`--no-run` + `--list`) ist diese Zahl am heutigen Baum **nicht reproduzierbar**;
gemessen ist `139 / 130 / 3117` (aus `--no-run`-`Executable`-Zeilen und
`--list`-`Running`-/Summenzeilen). Die heutige Zahl trägt ihr Kommando; die alte
Zahl wird nicht übernommen.

## (a) Konstante auf sich selbst / tautologische Assertions

Weiterhin **kein wörtlicher Selbstvergleich** im Baum (regex über alle
Testrümpfe: `assert_eq!(X, X)`, `assert_eq!(X, X.clone())`,
`assert_eq!(Default::default(), Default::default())` → 0 Treffer). Drei
Teilbefunde:

1. **`person_ids_are_stable_and_name_derived`** —
   `crates/lumina-gui/src/face_gui.rs` → `person_ids_are_stable_and_name_derived`
   (im Audit-Stand `c2c2ce1` war das die erste `assert_eq!`-Zeile des Tests):
   `assert_eq!(face_person_id_for("Alex"), face_person_id_for("Alex"))` —
   unverändert vorhanden, für eine reine Funktion nicht failbar. Die Aussage
   bleibt in den **übrigen Assertions desselben Tests** getragen (Stabilität,
   Präfix, Whitespace, Ordnungsfreiheit); nur diese eine Zeile ist tot.
   Priorität: niedrig.
2. **`fixture_helpers_are_deterministic`** —
   `crates/lumina-cull/tests/common/mod.rs` → `fixture_helpers_are_deterministic`:
   sechsmal `f(x) == f(x)` mit
   identischen Literalen; `splitmix64(&mut 1)` reseedet pro Aufruf. Wieder
   **gemessen** (Mutation M3): deterministisch falsch gekippt bleibt die gesamte
   `lumina-cull`-Suite (45 Tests) grün. Priorität: niedrig; Löschung erspart
   nichts (0,06 s).
3. **NEU: `fit_preview_histogram_matches_thumbnail` / `fit_preview_portrait_histogram_matches_thumbnail`** —
   `crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` →
   `fit_preview_histogram_matches_thumbnail` /
   `fit_preview_portrait_histogram_matches_thumbnail` (im Audit-Stand `c2c2ce1`
   lagen beide Tests in `crates/lumina-gui/src/tests/histogram.rs`; heute sind sie
   in das eigene Modul `preview_thumb_parity` gewandert). Beide vergleichen das
   Voll-Preview-Histogramm mit dem Thumbnail-Histogramm über
   `histogram_l1(...) < 0.35` (jeweils **im selben** Test) — die Vergleichszeile
   ist aber **tautologisch erfüllt** (Befund F1, siehe (e)).

## (b) Doppelte Abdeckung derselben Eigenschaft

Status der Vorrunden-Befunde (B1–B7), jeder gegen HEAD nachgeprüft:

- **B1 — ERLEDIGT/ÜBERHOLT.** `downscale_never_upsizes_and_is_deterministic`
  (`crates/lumina-mcp/tests/mcp.rs` → `downscale_never_upsizes_and_is_deterministic`)
  assertet jetzt die No-Op-Byte-Identität (**im selben** Test, „no-op downscale
  must preserve pixels byte-for-byte"). Fix
  `8d613d2` („test(mcp): repair own fail signal on downscale
  (TEST-REDUNDANCY-61, B1)"), Task verifiziert geschlossen `bb37c15`.
  **Nachgemessen (M1):** die No-Op-Perturbation schlägt jetzt auch im
  mcp-Exemplar an (rot) → das Exemplar hat sein eigenes Fehlsignal. Die Dublette
  ist damit **keine** mehr.
- **B2 — ERLEDIGT.** Die fünf Exemplare der Abweisungs-Aussage sind auf **einen**
  kanonischen Pin kollabiert:
  `crates/lumina-sidecar/src/tests/local_adjustments_color_digest.rs`
  (`disabled_local_presence_detail_denoise_and_optics_stay_rejected`; der
  Doc-Kommentar **über** diesem Test nennt die Konsolidierung aus den drei
  Vertretern).
  Task `TEST-REDUNDANCY-61`/`-62` geschlossen. Die Rest-Aussage
  (Legacy-`adjustment_*`-Pfad) ist erhalten.
- **B3 — OFFEN (unverändert).** Fünf `previous_refuses_*`-Tests, ein Pad, nur im
  gesetzten Stufen-Key unterschiedlich:
  `crates/lumina-cli/tests/mask_local_e2e.rs` →
  `previous_refuses_recipe_only_local_mask_transfer_on_source_or_target`,
  `crates/lumina-cli/tests/mask_local_detail_previous_e2e.rs` →
  `previous_refuses_a_detail_only_local_state_on_source_and_target`,
  `crates/lumina-cli/tests/mask_local_presence_e2e.rs` →
  `previous_refuses_a_presence_only_local_state_on_source_and_target`,
  `crates/lumina-cli/tests/mask_local_curve_e2e.rs` →
  `previous_refuses_a_curve_only_local_state_on_source_and_target`,
  `crates/lumina-cli/tests/mask_local_color_history_e2e.rs` →
  `previous_refuses_a_color_only_local_state_on_source_and_target`.
  **Nicht gemessen** (CLI-e2e-Lauf bleibt
  benanntes Gate).
- **B4 — OFFEN, erweitert.** `legacy_versions_migrate_losslessly_and_refuse_a_smuggled_*`
  liegt jetzt **viermal** vor:
  `crates/lumina-sidecar/src/tests/local_adjustments_detail.rs` →
  `legacy_versions_migrate_losslessly_and_refuse_a_smuggled_detail_block`,
  `crates/lumina-sidecar/src/tests/local_adjustments_presence.rs` →
  `legacy_versions_migrate_losslessly_and_refuse_a_smuggled_presence_block`,
  `crates/lumina-sidecar/src/tests/local_adjustments_color.rs` →
  `legacy_versions_migrate_losslessly_and_refuse_a_smuggled_color_block`,
  `crates/lumina-sidecar/src/tests/local_adjustments_curves.rs` →
  `legacy_versions_migrate_losslessly_and_refuse_a_smuggled_curve` — identische
  Migrationsschleife + Refusal, unterschiedlich nur der eingeschleuste Block.
- **B5 — OFFEN (Nur-Abgrenzung).**
  `brush_zero_width_/brush_zero_height_resolution_rejects_invalid_sidecar`
  (`crates/lumina-sidecar/src/tests/mask_prompts.rs` →
  `brush_zero_width_resolution_rejects_invalid_sidecar` /
  `brush_zero_height_resolution_rejects_invalid_sidecar`) teilen den
  Produktionszweig (`resolution.0 == 0 || resolution.1 == 0`) —
  Condition-Coverage, **kein** Streichkandidat.
- **B6/B7 — unverändert** (kein neuer Befund; Abgrenzung wie Vorrunde:
  `effects_validation_rejects_invalid_values` zwei Oberflächen, `initialize_*`
  Überlapp nur protocolVersion+tools).

## (c) Implementierungsentscheidung statt Verhalten

**Keine Befunde.** Geprüft und leer: Debug-Format-Pins, Zeiger-/Adressidentität,
`size_of`-Pins (nur `crates/lumina-raw/tests/abi_layout.rs` →
`abi_constants_match_rust_struct_layout`, der ABI-Vertrag),
Call-Order-Vergleiche. Die neuen Anker-Tests pinnen **Bytes und Geometrie**,
nicht die Implementierungswahl.

## (d) Tests, die bei Wegfall der Logik nichts verlieren

**Keine Befunde.** Der Naht-Scan der Vorrunde bleibt gültig: die zwei Rohtreffer
(`run_merge`, `classify_required`) sind weiterhin falsch positiv (Produktions-
bzw. Testinfrastruktur-Aufrufer existieren).

## (e) Tests ohne Fehlsignal — gemessen

Drei Mutationen, je einzeln, Mutationsziel byte-identisch restauriert (SHA-256
vor/nach):

| Messung | Mutation | Ergebnis |
|---|---|---|
| **M1** | `lumina_core::downscale` No-Op-Pfad (Early-Return-Klon in `crates/lumina-core/src/lib.rs` → `downscale`; im Audit-Stand `c2c2ce1` die Klon-Zeile) perturbiert jedes Pixel um +1 | core `downscale_max_width_never_upsizes_and_is_deterministic` **rot**, mcp `downscale_never_upsizes_and_is_deterministic` **rot** (B1 jetzt mit eigenem Fehlsignal), anchor `downscale_max_width_byte_anchor_matches_pre_consolidation` + `max_width_geometry_honours_the_width_cap` **rot** |
| **M2** | Kernel-Interpolation in `crates/lumina-core/src/lib.rs` → `downscale` (im Audit-Stand `c2c2ce1` die `.round()`-Zeile) `.round()` → `.floor()` | anchor **3 rot** (`downscale_max_width_byte_anchor…`, `downscale_long_edge_byte_anchor_frozen`, `bilinear_thumbnail_path_is_closer_to_the_area_reference_than_nearest`); **GUI-Histogramm-Parität grün → F1 bewiesen** |
| **M3** | `checkerboard` dark↔light vertauscht (`crates/lumina-cull/tests/common/mod.rs` → `checkerboard`; im Audit-Stand `c2c2ce1` die `dark`/`light`-Argumente im Funktionskörper) | `lumina-cull` **45/45 grün** → G2 reproduziert (Polarität unverankert, jetzt dokumentierte Grenze) |

**Restore-Beleg (SHA-256 vor und nach jeder Messung):**

- `crates/lumina-core/src/lib.rs` =
  `717294d0d9e166468275cd53df3e471a31c6d5f01435a326892668ccd49ff311`
- `crates/lumina-cull/tests/common/mod.rs` =
  `f693958ba38b792a27646b715167bbfcb741121753ad0ad9845b7a966cdad527`

**M-A und M-C2 wurden in dieser Runde nicht erneut gefahren;** ihr Status stammt
aus den geschlossenen Tasks 61/62 respektive dem Vorrundenlauf (M-C2). Dieser
Bericht behauptet sie nirgends als neu gemessen.

### F1 — NEU, GEMESSEN: die Preview↔Thumbnail-Histogramm-Parität ist vakuum

`fit_preview_histogram_matches_thumbnail`
(`crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` →
`fit_preview_histogram_matches_thumbnail`) und die
Portrait-Variante (`crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` →
`fit_preview_portrait_histogram_matches_thumbnail`) bauen die „Thumbnail-Pipeline" mit
`downscale(&original, DownscaleTarget::LongEdge(THUMBNAIL_MAX_DIM))`
(jeweils **im selben** Test), wobei `THUMBNAIL_MAX_DIM = 200`
(`crates/lumina-gui/src/filmstrip.rs` → `THUMBNAIL_MAX_DIM`).

Die Quellframes sind `synthetic_gradient_png()` **64×40**
(`crates/lumina-gui/src/lib.rs` → `synthetic_gradient_png`; im Audit-Stand `c2c2ce1`
die `let (w, h) = (64u32, 40u32)`-Zeile) bzw.
**40×64** (`crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` →
`fit_preview_portrait_histogram_matches_thumbnail`). Die lange Kante ist in beiden Fällen
**64 ≤ 200** → `downscale` nimmt den Early-Return-Klon
(`crates/lumina-core/src/lib.rs` → `downscale`). Das
„Thumbnail" ist also das **unveränderte Original**, und `render_frame(original)`
== der Voll-Preview-Render → `histogram_l1(preview, thumb)` ist **exakt 0** und
die Assertion `distance < 0.35` kann nicht scheitern. **Beweis:** M2
(Kernel-Rundung mutiert) macht die drei Anker-Tests rot, lässt aber genau diese
beiden GUI-Paritätstests grün — sie prüfen den Downscale-Pfad des Thumbnails
**nicht** (der läuft in ihnen nie). Die Aussage „Voll-Preview-Histogramm gleicht
dem Thumbnail" ist im Test damit nicht abgedeckt. Lücke → F1.

## (f) Laufzeit-Ballast

1. **`kittest_snapshots` = 56 Tests, auf diesem Host 56× `#[ignore]`, 0,00 s** —
   unverändert. Der CI-GUI-Shard **kompiliert** das Ziel, **führt 0** seiner
   Tests aus. Der Vorrunden-Befund G1 ist inzwischen adressiert (siehe G1-Status
   unten), die Kompilat-/Listungskosten bleiben.
2. Langsamste kleine GUI-Ziele (gemessen, single-threaded):
   `mask_local_editors_wiring` 3/7,47 s; `mask_local_editors` 3/6,94 s;
   `mask_local_color_controls` 1/6,77 s; `slider_sidecar_save` 4/5,61 s;
   `mask_local_hsl_bands` 2/3,77 s. GUI-lib: 950 Tests/56,61 s (≈60 ms/Test).
3. **Methoden-Richtigstellung (selbst gemessen):** Die Vorrunden-Falle „`--list`
   liefert für die Custom-Harness-Binaries (egui_kittest) **0 Tests**" gilt am
   heutigen Baum **nicht mehr**: `cargo test --workspace --all-targets -- --list`
   listet `kittest_snapshots.rs` mit **56** Tests und `kittest_parity.rs` mit
   **7** (Zeilen `Running tests/kittest_*.rs` gefolgt von den Testnamen). Die
   `--list`-Zählung sieht die kittest-Schicht jetzt mit; das behebt die
   Vorrunden-Unterzählung.

## (g) Assertionen, die nur noch den Refactor beschreiben

**Keine Befunde im Alt-Bestand.** Zwei Grenzfälle im **neuen** Anker sind
benannte Grenzen, kein Streichgrund — beide als Lücke geführt (F2/F3):

- **F2 — Pixel-Bytes des f32→f64-Wechsels nur an glatten Fixtures gepinnt.**
  `downscale_long_edge_byte_anchor_frozen` (`crates/lumina-core/tests/downscale_anchor.rs` →
  `downscale_long_edge_byte_anchor_frozen`) pinnt die
  Bytes an acht **Gradienten**-Fixtures;
  `long_edge_f32_to_f64_geometry_difference_is_the_measured_one`
  (`crates/lumina-core/tests/downscale_anchor.rs` →
  `long_edge_f32_to_f64_geometry_difference_is_the_measured_one`) pinnt
  nur den **Geometrie**-Zähler 82/1.120.249. Die in der Modul-Doku
  (`crates/lumina-core/tests/downscale_anchor.rs` → `//!`-Kopf)
  und im Commit behauptete Aussage „an allen acht Fixtures sind alte f32- und
  neue f64-**Bytes** identisch" ist damit nur für glatte Bilder belegt; ein
  **Pixel-Byte-Delta** f32↔f64 auf **nicht-glatten** Bildern (≤1-LSB-Behauptung)
  ist ungepinnt.
- **F3 — `exactly_one_resampler_exists` ist eine 3-Datei-String-Blacklist.**
  `crates/lumina-core/tests/downscale_anchor.rs` → `exactly_one_resampler_exists`
  (im Audit-Stand `c2c2ce1`; heute in
  `crates/lumina-core/tests/downscale_anchor/resampler_gate.rs` →
  `exactly_one_resampler_exists`) prüft per `include_str!`/`contains` nur
  `core/src/lib.rs`, `gui/src/filmstrip.rs`, `gui/src/preview_jobs.rs` auf die
  Abwesenheit der drei Altnamen. Weitere Downscale-/Resampler-Implementierungen
  außerhalb dieser drei Dateien — z. B. `crates/lumina-core/src/render.rs` →
  `resample_plane_bilinear`, `crates/lumina-core/src/mask_alignment.rs` →
  `resample_plane_bilinear` (Masken-**Plane**, nicht Frame) — liegen
  außerhalb des Gates, ohne dass die Frame-vs-Masken-Plane-Grenze im Test benannt
  ist.

## Lücken (neue offene Aufgaben — kein Nachbar wurde gestrichen)

- **F1 — Task `PREVIEW-THUMB-PARITY-VACUOUS-76` (PRIO mittel):** siehe (e) — die
  beiden `fit_preview*_histogram_matches_thumbnail`-Tests sind wegen
  `Quelle ≤ THUMBNAIL_MAX_DIM` vakuum. Aufgabe: ein Quellframe **über** der Kappe
  wählen (lange Kante > 200), so dass das Thumbnail wirklich resampelt, damit die
  Parität eine failbare Aussage trägt.
- **F2 — Task `DOWSCALE-ANCHOR-PIXEL-DELTA-77` (PRIO niedrig):** siehe (g) —
  Pixel-Byte-Delta des f32→f64-Wechsels auf nicht-glatten Bildern pinnen (oder die
  ≤1-LSB-Behauptung als unbelegt streichen).
- **F3 — Task `RESAMPLER-GATE-SCOPE-78` (PRIO niedrig):** siehe (g) — Gate
  `exactly_one_resampler_exists` auf workspace-weite Frame-Downscale-Suche
  erweitern oder die Frame-vs-Masken-Plane-Grenze im Test dokumentieren.

**Status der Vorrunden-Lücken (G1–G3):**

- **G1 — ERLEDIGT, ersetzt.** Nicht gestrichen, sondern durch ein Gate abgelöst:
  `.github/workflows/golden-gate.yml` (schedule/dispatch) fährt
  `cargo test -p lumina-gui --test kittest_snapshots -- --ignored` und
  protokolliert ein nicht-stilles Rot (Schritt „Record a non-silent result“ in
  `.github/workflows/golden-gate.yml`); dokumentiert in
  `feature/quality/golden-references.md` §10. Task `CI-GOLDEN-GATE-75`
  (Vorrunden-`GOLDEN-CIGATE-62`), nach dem Toolchain-Fix `7cec73c`/`2f4300c` aus
  dem Plan ausgetragen.
- **G2 — ERLEDIGT.** `CULL-POLARITY-63` geschlossen (`911a635`) via
  **dokumentierter Grenze**: Kommentar in `crates/lumina-cull/tests/fixtures.rs` →
  `fixtures` (im Audit-Stand `c2c2ce1` der Kommentarblock über dem Fixture)
  („Polaritätsgrenze … polaritätsinvariant … kein Defekt"). M3 reproduziert die
  Invarianz.
- **G3 — ERLEDIGT.** `PARITY-MATRIX-ASSERT-64` geschlossen (`9a22617`):
  `cpu_gpu_path_parity_matrix` (`crates/lumina-gui/tests/kittest_parity.rs` →
  `cpu_gpu_path_parity_matrix`) assertet jetzt das
  `MatrixOutcome` (**im selben** Test) statt es zu verwerfen.

**Prozessbefund „die Runde 2026-10-12 habe keine Tasks angelegt" — WIDERLEGT.**
Commit `ea82f05` legt zusammen mit dem Bericht `TEST-REDUNDANCY-61`,
`GOLDEN-CIGATE-62`, `CULL-POLARITY-63`, `PARITY-MATRIX-ASSERT-64` an
(Releaseplan-Zeilen + Task-Zeilen). Nur weil alle vier nach ihrer Erledigung
repo-konform wieder aus `Agents.todo.md` ausgetragen wurden, findet eine Suche
nach ihren Symbolen sie heute nicht mehr — das ist die normale Lebensdauer
abgeschlossener Tasks, **kein** Abnahmeverstoß.

## Methodik — welche Zahl gilt für welche Menge

| Zahl | Menge | Quelle |
|---|---|---|
| 139 / 130 / 3117 | Testbinaries / davon libtest / listbare Tests | `cargo test --workspace --all-targets --no-run` (139 `Executable`-Zeilen) + `cargo test --workspace --all-targets -- --list` (139 `Running`, 130 libtest-Summen, Summe 3117), 2026-10-19 |
| 22 / 989 / 82 | GUI-Testbinaries / ausgeführt / ignoriert | echter Lauf `cargo test -p lumina-gui --all-targets`, 2026-10-19 |
| 950 (950+5) | GUI-lib-Unit | `cargo test -p lumina-gui --lib` (56,61 s) |
| 10 / 45 / 15 | downscale_anchor / lumina-cull / GUI-lib histogram | Einzelläufe, s. o. |

**Trennung gemessen / nicht ausgeführt / benanntes Gate:** (e) M1, M2, M3
**gemessen** (je eine Einzellauf-Messung, kein Streuungsniveau — `DoD.md` §9);
F1 **gemessen** (M2) und statisch belegt (Quell-/Zielgeometrie); F2/F3
**statisch**; B3 **statisch** mit benanntem Gate (CLI-e2e-Lauf); G1–G3
**erledigt** (Code/Commit belegt), die Golden-Ausführung selbst bleibt benanntes
Hardware-Gate (`GOLDEN-REF-30`/`GOLDEN-BASELINE-32`).
