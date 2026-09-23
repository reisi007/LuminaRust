# Konflikte, Tests und Abnahme

**Feature:** F-011 Konflikt- und Releasequalität

## Inhaltsverzeichnis

- [Konfliktmatrix](#konfliktmatrix)
- [Abnahmeszenarien](#abnahmeszenarien)
- [Rezept-Matrix (LRPAR-MATRIX-RECIPE)](#rezept-matrix-lrpar-matrix-recipe)
- [Testanforderungen](#testanforderungen)
- [Bewusste Nichtziele](#bewusste-nichtziele)
- [Änderungsregeln](#änderungsregeln)

## Konfliktmatrix

| Konflikt | Erkennung | SOLL-Auflösung |
| --- | --- | --- |
| Original-Hash weicht ab | Sidecar-Hash gegen Quelle prüfen | Status `source_changed`, keine stille Überschreibung |
| Sidecar-Hauptversion unbekannt | Schema-Validator | Lesen verweigern oder explizite Migration |
| Sidecar beschädigt | JSON-/Prüfsummenfehler | Original offen lassen, Backup/Recovery anbieten |
| Maskenartefakt fehlt | relativen Pfad und Prüfsumme prüfen | Status `missing`, Neuberechnung optional |
| Modell-Hash abweichend | Maskenidentität prüfen | Alte Matte als `stale` anzeigen |
| Aktive Maske fehlt/veraltet | Maskenstatus vor Export prüfen | Warnen, Aktualisierung anbieten, Export trotzdem erlauben |
| Sidecar und DB widersprechen | Revision/Hash vergleichen | Sidecar gewinnt, Index aktualisieren |
| Zwei Prozesse schreiben | Revision/Lock/Atomic Write | Konflikt melden, kein stilles Last-Write-Wins |
| Virtuelle-Kopie-ID doppelt | Schema-Validierung | Sidecar ablehnen, korrigierbare Meldung |
| XMP widerspricht Lumina | Import-/Export-Prüfung | Lumina-Sidecar bleibt autoritativ |
| Entwurfswert überschreitet IIM-Oktettlimit | Bake-In-Validierung gegen Feld-Registry | Lauter Fehler pro Exportdatei (Feld + Limit genannt), kein Still-Kürzen |
| IPTC-Entwurf leer, `--write-metadata` gesetzt | Bake-In-Status vor Schreiben | Export folgt mit lauter Warnung + `metadata_written: "empty"`, kein stiller No-Op |
| Pipeline nicht verfügbar | Versionsregistry | Render blockieren oder migrieren |
| RAW-Backend fehlt | Capability-Prüfung | Sidecar lesbar, Render nicht verfügbar |

## Abnahmeszenarien

### RAW-MVP

1. Eine CR2-, CR3-, NEF-, ARW- oder DNG-Datei wird über den nativen LibRaw-Adapter
   geöffnet.
2. EXIF-Orientierung und die vollständige Bildgeometrie werden übernommen.
3. Das Bild läuft durch denselben Core-/CLI-Renderpfad wie ein Rasterbild.
4. Ein optionaler RAW-Fixture-Test prüft Decode, Orientierung und Dimensionen;
   er wird ohne `LUMINA_RAW_FIXTURE` übersprungen und nicht als Golden bestanden
   gezählt. Lizenzgeeignete CR2/CR3/NEF/ARW/DNG-Dateien liegen unter
   `sample-data/raw/` oder werden extern über diese Variable referenziert.
   Die lokalen Testläufe lauten beispielsweise:
   `LUMINA_RAW_FIXTURE="$PWD/sample-data/raw/aircraft-landscape.cr3" rustup run stable cargo test -p lumina-raw -- --ignored`
   und
   `LUMINA_RAW_FIXTURE="$PWD/sample-data/raw/aircraft-portrait.cr3" rustup run stable cargo test -p lumina-raw -- --ignored`.
   Im Browser wird RAW als nicht verfügbare Fähigkeit ausgewiesen. Lens,
   Kamera-Farbmatrix und Profile bleiben bis zur Prüfung der konkreten
   LibRaw-Felder als F-034 offen.

### Sidecar ohne DB

1. Eine RAW-Datei erhält zwei virtuelle Kopien.
2. Die zentrale DB wird gelöscht oder war nie vorhanden.
3. Das Sidecar wird neben dem Original geöffnet.
4. Beide Kopien, Rezepte und Maskenreferenzen werden rekonstruiert.

### Persistierte Maske

1. Eine AI-Maske wird erzeugt und gespeichert.
2. Das ONNX-Modell wird entfernt.
3. Das Sidecar wird erneut geladen.
4. Die gespeicherte Maske wird ohne Inferenz verwendet.

### Ungültige Maske

1. Die Quelle wird verändert oder ausgetauscht.
2. Das Sidecar wird geladen.
3. Die Maske wird als `stale` markiert.
4. Neuberechnung bleibt eine explizite Aktion.

### Virtuelle Kopien

1. Kopie A erhält einen hellen Look und Subjektmaske.
2. Kopie B erhält einen dunklen Look ohne Maskenebene.
3. Beide werden exportiert.
4. Rezepte und Exporte bleiben unabhängig.

### DB-Wiederaufbau

1. Sidecars und Artefakte liegen vollständig auf dem Dateisystem.
2. Der optionale Index wird gelöscht.
3. Reindex liest alle Sidecars.
4. Fotos, Kopien, Status und Artefaktverweise sind wieder vorhanden.

### Auswahlbasierte Mehrbildbearbeitung

1. Mehrere Bilder werden in der GUI ausgewählt.
2. Eine globale Einstellung, Auto-Regel oder Maskenabsicht wird angewendet.
3. Jedes Bild erhält ein eigenes Sidecar und eigenes Rezept.
4. Zielmasken stehen je nach Berechnungsstand als `missing` oder `pending` zur
   Verfügung und werden nicht als gemeinsame Quellmatte ausgegeben.

### Preset-Anwendung

1. Ein Preset enthält nur explizit ausgewählte Felder.
2. Absolute Werte und gültige relative Exposure-Werte werden auf mehrere Bilder
   angewendet.
3. Relative Exposure ohne aktiviertes Auto-Tone wird abgelehnt.
4. Jedes Zielbild erhält genau einen neuen History-Schritt; die Quellhistorie
   wird nicht kopiert.

### IPTC-Draft und JPEG-Bake-In

1. Ein IPTC-Entwurf wird gesetzt (CLI, MCP oder GUI) — nur das Sidecar ändert
   sich, das Original bleibt byte-identisch.
2. Ausgewählte Felder werden per Sync auf zwei Ziel-Sidecars übertragen;
   ein Ziel ohne Sidecar schlägt laut und isoliert fehl.
3. Der JPEG-Export mit `--write-metadata` erzeugt eine Exportdatei, deren
   IPTC-Tags (IIM + XMP) re-lesbar sind und deren Pixelbytes dem Export ohne
   Metadaten entsprechen; ein PNG/WebP-Export mit derselben Option scheitert
   laut.
4. Ohne Opt-in bleiben Exporte exakt wie heute (keine Metadaten, keine
   stillen Annahmen); dynamische Presets ohne alle Platzhalter-Variablen
   scheitern laut, ohne etwas zu schreiben.

### Preview-Cache

1. Beim Verlassen eines Bildes wird standardmäßig nur die aktuelle Standard-
   Vorschau gespeichert.
2. Eine geerbte Ordneroption kann zusätzlich eine 1:1-Vorschau aktivieren.
3. Der Cache liegt unter `.lumina/`, ist nicht autoritativ und darf vollständig
   gelöscht oder bei nicht gefundenen Quellen automatisch entfernt werden.

## Rezept-Matrix (LRPAR-MATRIX-RECIPE)

**Feature:** F-011 Konflikt- und Releasequalität (Aufgabe LRPAR-MATRIX-RECIPE,
Dach-Test über die Goals G-01…G-16). **Status:** umgesetzt (Slice 1: CLI-Runner,
Slice 2: GUI-headless-Modus, `.github`-Nightly-Wiring, gemeinsamer Ablageort
`testdata/matrix/`, `--require-gpu`-Routengate, Masken-Readback-Fix).

**Zweck.** Die Rezept-Matrix ist der End-to-End-Dach-Test über alle Goals: Sie
wendet ein versioniertes Rezept-Set auf die beiden committeten RAW-Samples an,
exportiert über den gemeinsamen Standard-Renderpfad und vergleicht das Ergebnis
gegen committete Goldens mit dokumentierten PSNR-Toleranzen. Sie ergänzt die
punktuellen Golden-/PSNR-Tests der Einzel-Features um einen Lauf, der
Regressionen über Rezeptkombinationen, Pipeline-Stufen und beide
Orientierungen sichtbar macht.

**Sample-Bilder (committet, lizenzdokumentiert, F-073).**

- `sample-data/raw/aircraft-landscape.cr3` (6032×4024, Orientierung 1)
- `sample-data/raw/aircraft-portrait.cr3` (4024×6032, Orientierung 8)

Das Rezept-Set referenziert die Samples **relativ zu seiner eigenen Datei**
(keine absoluten Pfade); Tests und Runner greifen nicht auf das Netzwerk zu.

**Rezept-Set (versioniert, sidecar-kompatibel, relativ portabel).** Datei:
`testdata/matrix/recipe-set.v1.json` (`schema_version: 1`) am Workspace-Root,
gemeinsamer Ablageort von CLI-Runner und GUI-headless-Matrix. Jeder Eintrag
trägt `id`, `goals`, `stages`, `tolerance` (Toleranzklasse
`exact`/`strict`/`standard`/`robust`, siehe unten), `expected_route` (erwartete
Render-Route, siehe `--require-gpu`) und ein `recipe`-Objekt in exakt der
JSON-Form, die eine virtuelle Kopie
im Sidecar unter `recipe` verwendet (`EditRecipe`-Roundtrip). Rezept-IDs und
Sample-IDs sind innerhalb des Sets eindeutig. Konkrete Rezeptliste:

| Rezept-ID | Abgedeckte Stufen | Goals | Toleranz | `expected_route` |
| --- | --- | --- | --- | --- |
| `g01-identity` | Decode → Output (Identität) | G-01 | standard | gpu |
| `g01-tone-wb` | globaler Tonwert, WB (`wb_temperature`/`wb_tint`) | G-01 | strict | gpu |
| `g01-presence-color` | Presence (Texture/Clarity/Dehaze), Vibrance/Saturation | G-01 | standard | gpu |
| `g02-curves-hsl` | Gradationskurve (Master + R/G/B), HSL | G-02 | strict | gpu |
| `g02-point-color-grading` | Point Color, Color Grading (inkl. Luminanz/Blending) | G-02 | standard | gpu |
| `g06-geometry` | Crop (Aspect), Rotation, Spiegelung | G-06 | standard | gpu |
| `g06-lens-perspective` | manuelle Objektivkorrektur (Verzeichnung/Vignette/CA), Perspektive | G-06 | standard | cpu (`geometry (default content crop)`, siehe unten) |
| `g14-detail` | Rauschreduzierung, Schärfen, Rote Augen | G-14 | standard | gpu |
| `g01-effects` | Vignettierung, Körnung (Seed) | G-01 | robust | gpu |
| `g05-lens-blur` | Lens Blur (heuristischer Fokus-Rahmen, Bokeh) | G-05 | robust | gpu |
| `g04-spot-heal` | Spot-Heal (heuristisch, Extras-Geometrie) | G-04 | robust | gpu |

Jede implementierte Renderstufe der Pipeline ist damit mindestens einmal
abgedeckt. **Bewusst nicht im Rezept-Set (laut begründet, kein stiller
Fallback):**

- **Masken (G-03):** benötigen ein persistiertes Maskenartefakt (`.lumina.zdata`)
  und/oder ein Inferenzmodell; sie laufen über die bestehenden Masken-Tests und
  einen eigenen Fixture-Slice.
- **`source_actions`/`generative_edit`:** benötigen Artefakte/Modelle, die der
  Runner nicht synthetisieren darf. Der Runner **lehnt solche Rezepte laut ab**
  (Rezept-Set-Validierung), statt sie still zu überspringen.
- **Unbekannte Top-Level-Keys:** `EditRecipe` leitet unbekannte Schlüssel nach
  `extras` um, wo sie ein stiller No-Op wären (Tippfehler wie `geometri` würde
  als Identität „grün"). Die Rezept-Set-Validierung **lehnt jeden
  `extras`-Schlüssel außer dem dokumentierten `spot_removals`-Geometrie-Spiegel
  laut ab**.
- **`lens_correction` mit Lensfun-Auto-Profil:** Der Runner übergibt bewusst
  keinen Lensfun-Corrector, damit die Goldens feature-unabhängig und
  reproduzierbar bleiben; der Lensfun-Pfad ist über
  `lumina geometry --lensfun-status` und die fokussierten Lensfun-Tests
  abgedeckt (manuelles Modell bleibt im Rezept-Set wirksam).
- **HDR-/Panorama-Merge (G-13):** eigener Merge-Pfad (`lumina-merge`) mit
  eigenen Golden-Tests.
- **IPTC/Metadaten (G-15):** export-seitig, durch die Meta-/IPTC-E2E-Tests
  abgedeckt.

**Golden/PSNR-Toleranzen (pro Typ, dokumentiert).** Vergleichsdomäne ist
RGBA8 sRGB. Der Runner rendert in voller Sample-Auflösung — im Verify-Modus
über den gemeinsamen Standardpfad (`render_best_effort`: GPU-Default wo
verfügbar, sonst CPU-Referenz — keine zweite Renderlogik), im
Baseline-Modus (`--update-goldens`) bewusst auf der CPU-Referenz
(plattformneutral/reproduzierbar, kein GPU-Baseline) — und vergleicht ein
deterministisch bilinear herunterskaliertes Bild (`comparison_width = 384`,
`lumina-core::downscale_bilinear`, rein mathematisch, kein Zufall) gegen das
Golden. Das Downscaling ist eine Vergleichs-Normalisierung, kein Renderpfad.
Toleranzklassen:

| Typ | Schwellwert | Einsatz | Begründung |
| --- | --- | --- | --- |
| `exact` | (fehlt) → PSNR = ∞ | deterministische CPU-geroutete Rezepte und hermetische Synthetik-Fixtures | byte-identische Wiederholung, wie in den bestehenden Golden-/PSNR-Tests (`lens_blur`) belegt |
| `strict` | 45 dB | per-Pixel-Operationen ohne Resampling (Tone/WB, Kurven, HSL, Vignette) | hohe Empfindlichkeit, kein Resampling-Rauschen |
| `standard` | 40 dB | Stufen mit Resampling/spatialen Filtern (Geometrie, Objektiv/Perspektive, Presence, Schärfen/Denoise) | absorbiert ±1 LSB GPU↔CPU/Architektur-Differenz, oberhalb der „sichtbaren" 40-dB-Grenze |
| `robust` | 30 dB | varianzstarke Spatial-Stufen (Lens-Blur-Bokeh, Grain, Spot-Heal) | entspricht dem bestehenden `agent-harness`-Wert für sRGB-Previewtreue |

Die Klassen sind an bestehenden Tests orientiert (PSNR ≥ 30 dB im
`agent-harness`, byte-identisch = ∞ in `lens_blur`/`spot_heal`), nicht geraten.
Ein **fehlendes Golden ist ein lauter Fehler** (kein stiller Fallback);
`--update-goldens` schreibt Goldens explizit neu.

**Modi (User-Entscheid 2026-09-03).**

1. **Nightly-Schedule (1×/Tag) und vor Releases:** volle Matrix.
2. **Opt-in pro Commit:** `[matrix]` im Commit-Titel/Body **oder** `!`- bzw.
   `BREAKING CHANGE`-Commits triggern die Matrix mit.
3. **Manuell:** `workflow_dispatch`.

Die PR-CI bleibt bewusst schlank (kein Matrix-Lauf im PR). Das
`.github`-Wiring ist umgesetzt: `.github/workflows/matrix-nightly.yml` läuft per
Schedule (1×/Tag), `workflow_dispatch` und — per `guard`-Job entschieden — beim
Push mit `[matrix]`-Marker oder Breaking-Change (`type(scope)!:` /
`BREAKING CHANGE`). Der Runner läuft als `lumina matrix` (verify) bzw.
`lumina matrix --update-goldens` (baseline).

**Route-Gate `--require-gpu` (Slice 2).** Jedes Rezept deklariert im Rezept-Set
seine erwartete Route: `"gpu"` oder die einzige dokumentierte CPU-Ausnahme
`{ "cpu": "geometry (default content crop)" }` (`g06-lens-perspective`).
`lumina matrix --require-gpu` vergleicht die deklarierte Route mit der Route,
die der Standard-Renderpfad **tatsächlich** genommen hat (der Rückgabewert von
`render_standard`/`render_best_effort`, keine Log-Text-Auswertung): Jede
nicht-exempte CPU-Route — inklusive „kein GPU-Adapter verfügbar" — ist ein
lauter Fail (Exit ≠ 0); eine CPU-Ausnahme muss ihren deklarierten Grund
enthalten, sonst ist auch sie ein Fail. Ein CPU-Build ohne `gpu`-Feature lehnt
`--require-gpu` am Einstieg laut ab, `--require-gpu --update-goldens` ebenso
(Baseline ist bewusst CPU-gepinnt). Ehrliche Trennung: Auf Metal beweist
`--require-gpu` die GPU-Parität lokal; ohne Metal (CI) läuft die Matrix als
CPU-Referenz-Regression und behauptet **keinen** GPU-Beweis.

**Pfadauflösung (Slice 2).** Das Rezept-Set liegt neutral unter
`testdata/matrix/` (gemeinsamer Ablageort von CLI und GUI-headless). Der
CLI-Runner backt keinen Build-Host-Pfad ein: `--recipe-set` gewinnt, sonst
`LUMINA_MATRIX_RECIPE_SET`, sonst `LUMINA_MATRIX_DIR/recipe-set.v1.json`, sonst
`testdata/matrix/recipe-set.v1.json` relativ zum nächsten Vorfahren des
Arbeitsverzeichnisses (Workspace-Root). Alle Sample-/Golden-Pfade bleiben
relativ zur Rezept-Set-Datei.
**DoD-§5-Anker (Spez-Aussage → Test):** die Kette ist hermetisch gepinnt in
`crates/lumina-cli/tests/matrix_paths.rs` —
`recipe_set_env_wins_over_matrix_dir` (Priorität),
`matrix_dir_env_uses_default_recipe_set_name` (Default-Dateiname) und
`ancestor_walk_finds_workspace_testdata_matrix` (Workspace-Root-Walk); die
Tests setzen die Kind-Env gezielt (`Command::env`/`env_remove`), ohne den
Prozess-Env-Status zu mutieren.

**Exit-Codes.** `0` = grün; `1` = Matrix-Fehler (fehlendes Golden,
PSNR-Verletzung, `--require-gpu`-Routenverletzung, Render-/Exportfehler,
ungültiges Rezept-Set); `2` = clap-Nutzungsfehler. Jeder Fehler nennt Grund und
betroffenes (Sample, Rezept).

**GPU-Parität und CPU-Routen (Slice 1).** Es gilt die No-Fallback-Doktrin aus
`Agents.md`: GPU-Fehler werden **hart propagiert** (CLI und MCP identisch); ein
GPU→CPU-Fallback bei Laufzeitfehlern existiert nicht — ein GPU-Fehler ist ein
Fail mit Fix-Pflicht. Die zuvor beobachtete 24-MP-Dehaze-Grenze war ein
**4×-Überallokations-Bug** im Dark-Channel-Staging
(`aligned_bytes_per_row(width * 4)`; die Funktion rechnet die 4 Byte/RGBA8-Texel
bereits ein) und ist behoben (`aligned_bytes_per_row(width)`, ≈93 MiB bei
24 MP, < 256 MiB `max_buffer_size`); dieselbe Fehlerklasse ist in
`readback_output_frame` mitkorrigiert. Beweis: `g01-presence-color` rendert
jetzt **auf der GPU** ohne `routed to CPU`-Log (PSNR 102,8 dB
(landscape) / 91,7 dB (portrait), Δ ≤ 1 LSB, ~0,6 s statt ~36 s CPU).

**GPU-Parität Slice 2 — Masken-Readback.** Dieselbe 4×-Klasse war noch in
`readback_mask_plane` offen: Die Maske ist `R16Uint` (2 Byte/Texel), und
`aligned_bytes_per_row` rechnet die 4 Byte/RGBA8-Texel bereits ein. Der alte
Aufruf `aligned_bytes_per_row(width * 2)` überallokierte daher 4×
(≈186 MiB statt ≈47 MiB Staging bei 24 MP). Korrigiert auf
`aligned_bytes_per_row(width.div_ceil(2))` (die R16-Texelzeile entspricht
`width * 2 / 4` RGBA8-Texeln). Lokal auf Metal verifiziert:
`cargo test -p lumina-gpu --features gpu` grün, inklusive
`upload_mask_plane_roundtrip_is_byte_exact` (byte-exakte u16-Domäne).

Darüber hinaus gibt es **genau eine dokumentierte, laute CPU-Route** im
Rezept-Set: `g06-lens-perspective` (Objektiv-/Perspektivkorrektur ohne
expliziten Crop) nutzt die in `feature/architecture/pipeline.md` § F-093/GPU
festgelegte Route `geometry (default content crop)` — das datenabhängige
Maximum-Rectangle ist rezept-planbar nicht auf der GPU reproduzierbar. Sie ist
der SOLL-Zustand dieses Rezepts, **keine** pauschale GPU-Paritätszusage: Jede
weitere CPU-Route (oder ein GPU-Laufzeitfehler) ist ein Fail mit Fix-Pflicht,
nicht akzeptiert. Das `--require-gpu`-Gate (siehe oben) erzwingt diese Zusage
nun maschinell über das `expected_route`-Feld des Rezept-Sets.

**Kosten-/Dauer-Pflicht (F-074).** Der Runner berichtet pro (Sample, Rezept)
die Dauer und die Gesamtdauer (Text und `--json`). Kosten/Dauer der Matrix
werden im betroffenen Feature-Dokument bzw. im Commit dokumentiert. Die Matrix
ergänzt F-074 (Ausführungs-/Gesamtkosten pro Rezept-Set), ersetzt aber keine
Benchmarks und hat kein hartes Laufzeitbudget. Volle Auflösung ist
kostenpflichtig; Benchmarks bleiben der Ort für Mikro-Messungen.

**GUI-headless-Modus (Slice 2, umgesetzt).** Dasselbe Rezept-Set wird
zusätzlich headless über `LuminaApp` (egui Context, tempdir) geladen, gerendert
und gegen **dieselben** Goldens geprüft. Der Runner
(`crates/lumina-gui/src/matrix.rs`, test-only) fährt die App-Pipeline
(`load_bytes` → Rezept setzen → expliziter Full-Resolution-Matrix-Eintrag →
`preview()`). Dieser test-only Eintrag setzt die Preview-Geometrie auf die
Source-Größe, sodass der R3-Viewport-Cap die 24-MP-Samples **nicht** vor dem
Golden-Vergleich verkleinert, ruft aber weiterhin `LuminaApp::render()` und
dessen gemeinsamen `render_from`-CPU-Pfad auf. Nach jedem Render muss
`preview_render_src` exakt der Source-Geometrie entsprechen; eine Abweichung ist
ein lauter Matrix-Fehler. Die normale interaktive Preview bleibt durch den
R3-Cap unverändert. Wie beim CLI-CPU-Referenzpfad verwendet der Render denselben
`RenderContext`: Rezept und dekodierte Kamera-Weißabgleich, keine
Source-Actions, Masken, Lensfun-Auto-Korrektor oder Depth-Plane. Damit gibt es
keine zweite Pipeline und keinen GPU-only-Weg: Die App-Vorschau **ist** der
gemeinsame CPU-Core-Render (ein hermetischer Test mit einer Quelle oberhalb des
Default-Viewport-Caps pinnt Full-Resolution und Byte-Identität). Wie beim
CLI-Runner wird bewusst kein Lensfun-Auto-Korrektor gebunden, damit die
Goldens feature-unabhängig bleiben. Der volle Lauf über beide CR3-Samples ist
RAW-abhängig und deshalb `#[ignore]`d + env-gated (`LUMINA_MATRIX=1`),
analog zum CLI-`matrix_e2e`; hermetisch (synthetisches PNG, tempdir) laufen ein
Byte-Identitäts- und ein Toleranz-Gate-Test im Standard-`cargo test -p
lumina-gui` ohne GPU. Das `.github`-Nightly fährt den vollen GUI-Lauf als
CPU-Referenz.

**Messwerte (Slice 2, Ist; macOS, Apple M5 Pro, Metal-Adapter).** Gemessen mit
dem optimierten Release-Build (`cargo run --release …` bzw.
`cargo test --release -p lumina-gui`), also demselben Profil wie das
CI-Nightly; „Dauer" ist die Matrix-Gesamtdauer inkl. Decode und
Export/Einlesen, kein Benchmark und ohne hartes Budget (siehe
Kosten-/Dauer-Pflicht):

| Modus | Paare | Gesamtdauer | Ergebnis |
| --- | --- | --- | --- |
| CLI verify, GPU (`--require-gpu`) | 22 | 7,7 s | 22/22 grün; 20 GPU + 2 exempte CPU-Routen (`g06-lens-perspective` je Sample, Grund `geometry (default content crop)`); 10× PSNR ∞, sonst ≥ 81,5 dB, Δ ≤ 1 LSB |
| CLI verify, CPU-Referenz (Nightly-Modus, ohne `gpu`-Feature) | 22 | 107,8 s | 22/22 grün, alle `route=cpu`, 22× PSNR ∞, Δ = 0 (byte-identisch zu den Goldens) |
| GUI headless (App-Pipeline, CPU-Core) | 22 | 104,5 s | 22/22 grün, 22× PSNR ∞, Δ = 0 (byte-identisch zu denselben Goldens) |

Der Debug-Lauf des GPU-Gates (`--require-gpu`, unoptimiert) dauerte konservativ
247,7 s (pro Paar ~4,5–23 s; das ist die eingangs dokumentierte
Slice-1-Referenzklasse). Der GUI-Lauf über die volle Matrix läuft im Nightly in
Release; lokal ist zusätzlich `LUMINA_MATRIX_RECIPES=<id,…>` als zeitlich
begrenzter Beweis möglich (Default bleibt der volle Satz).

**Hermetik.** Tests benötigen keinen spontanen Netzwerkzugriff und keine
absoluten Pfade. Der CR3-Matrix-Lauf ist RAW-/Fixture-abhängig und läuft
deshalb **nicht** im schnellen Standard-`cargo test`, sondern env-gated
(analog `LUMINA_RAW_FIXTURE`). Der Runner selbst und seine Exit-Codes werden
hermetisch mit synthetischen PNG-Samples in einem tempdir getestet.

## Testanforderungen

- Unit-Tests für Schema, Migration, IDs und atomare Writes
- JSON-Roundtrip- und Property-Tests für Wertebereiche und Rezeptdaten
- Sidecar-Recovery-, Konflikt- und parallele-Schreibtests
- Virtuelle-Kopien-Tests für Duplikation, Umbenennung und geteilte Artefakte
- Masken-Hit-, Miss-, Prüfsummen-, Modellwechsel- und Quelländerungstests
- Masken-DAG-, Zyklus-, Cross-Copy-Referenz- und Materialisierungstests
- Box-/Pinsel-Prompt-, SAM-Adapter- und nicht unterstützte Capability-Tests
- Source-Action-Tests vor Auto-Analyse und vor Maskenanwendung
- Golden-Image-Tests mit dokumentierten Toleranzen
- IPTC-Draft-Roundtrip-, Preset-Platzhalter-, Sync- und Meta-Historie-Tests
- JPEG-IPTC-Bake-In-Tests: Tag-Re-Parse, Pixelbyte-Identität, laute
  PNG/WebP- und IIM-Limit-Fehler, Ziel-Guard gegen Quelle/Bundle
- CLI-End-to-End-Tests mit Exit-Codes
- Rezept-Matrix: Runner-Exit-Codes, PSNR-Toleranz-Gates
  (`exact`/`strict`/`standard`/`robust`), fehlendes Golden laut, ungültiges
  bzw. artefaktgebundenes Rezept-Set laut abgelehnt, `--require-gpu`-Routengate
  (exempte CPU-Route passiert, unerwartete CPU-Route laut, Kombination mit
  `--update-goldens` und CPU-Build laut abgelehnt), env-gated CR3-End-to-End-Lauf
  (siehe [Rezept-Matrix](#rezept-matrix-lrpar-matrix-recipe))
- GUI-headless-Matrix: `LuminaApp` (egui Context + tempdir) fährt dasselbe
  Rezept-Set gegen dieselben Goldens; hermetischer Regressionstest mit einer
  Quelle oberhalb des Default-Viewport-Caps beweist Full-Resolution-Render und
  Byte-Identität zum Core-Render mit demselben `RenderContext`; das
  Toleranz-Gate bleibt ohne GPU aktiv, env-gated CR3-Voll-Lauf
- native Build-/Smoke-Tests
- Performance- und Speicherbenchmarks für RAW, Vorschau, Masken und Batch

RAW-Fixtures, Referenzbilder und AI-Modelle werden reproduzierbar versioniert
und mit Lizenzinformationen dokumentiert. Tests dürfen keinen spontanen
Netzwerk-Download benötigen.

## Bewusste Nichtziele

- Bearbeitung oder Überschreibung des Originals
- zentrale DB als Pflichtvoraussetzung
- unbemerkte AI-Neuberechnung beim Öffnen
- implizite Rezeptvererbung in v1
- zweite GUI-spezifische Renderpipeline
- automatische Modell-Downloads

## Änderungsregeln

- Das betroffene Feature-Dokument wird vor Implementierungsbeginn aktualisiert.
- Ein ungelöster Widerspruch zwischen SOLL und Code erzeugt eine offene
  Feature-ID in `Agents.todo.md`.
- Nach Implementierung und unabhängiger Verifizierung wird der erreichte
  Zustand im Feature-Dokument ergänzt.
- `Agents.todo.md` enthält nur offene Arbeit; bestätigte Aufgaben werden daraus
  entfernt.
