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
Dach-Test über die Goals G-01…G-16). **Status:** SOLL; der CLI-Runner (Slice 1)
ist umgesetzt, der GUI-headless-Anteil und das `.github`-Nightly-Wiring folgen
in Slice 2.

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
`crates/lumina-cli/matrix/recipe-set.v1.json` (`schema_version: 1`). Jeder
Eintrag trägt `id`, `goals`, `stages`, `tolerance` (Toleranzklasse
`exact`/`strict`/`standard`/`robust`, siehe unten) und ein `recipe`-Objekt in
exakt der JSON-Form, die eine virtuelle Kopie
im Sidecar unter `recipe` verwendet (`EditRecipe`-Roundtrip). Rezept-IDs und
Sample-IDs sind innerhalb des Sets eindeutig. Konkrete Rezeptliste:

| Rezept-ID | Abgedeckte Stufen | Goals | Toleranz |
| --- | --- | --- | --- |
| `g01-identity` | Decode → Output (Identität) | G-01 | standard |
| `g01-tone-wb` | globaler Tonwert, WB (`wb_temperature`/`wb_tint`) | G-01 | strict |
| `g01-presence-color` | Presence (Texture/Clarity/Dehaze), Vibrance/Saturation | G-01 | standard |
| `g02-curves-hsl` | Gradationskurve (Master + R/G/B), HSL | G-02 | strict |
| `g02-point-color-grading` | Point Color, Color Grading (inkl. Luminanz/Blending) | G-02 | standard |
| `g06-geometry` | Crop (Aspect), Rotation, Spiegelung | G-06 | standard |
| `g06-lens-perspective` | manuelle Objektivkorrektur (Verzeichnung/Vignette/CA), Perspektive | G-06 | standard |
| `g14-detail` | Rauschreduzierung, Schärfen, Rote Augen | G-14 | standard |
| `g01-effects` | Vignettierung, Körnung (Seed) | G-01 | robust |
| `g05-lens-blur` | Lens Blur (heuristischer Fokus-Rahmen, Bokeh) | G-05 | robust |
| `g04-spot-heal` | Spot-Heal (heuristisch, Extras-Geometrie) | G-04 | robust |

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
`.github`-Wiring ist Slice 2. Der Runner läuft als `lumina matrix` (verify) bzw.
`lumina matrix --update-goldens` (baseline).

**Exit-Codes.** `0` = grün; `1` = Matrix-Fehler (fehlendes Golden,
PSNR-Verletzung, Render-/Exportfehler, ungültiges Rezept-Set); `2` =
clap-Nutzungsfehler. Jeder Fehler nennt Grund und betroffenes (Sample, Rezept).

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

Darüber hinaus gibt es **genau eine dokumentierte, laute CPU-Route** im
Rezept-Set: `g06-lens-perspective` (Objektiv-/Perspektivkorrektur ohne
expliziten Crop) nutzt die in `feature/architecture/pipeline.md` § F-093/GPU
festgelegte Route `geometry (default content crop)` — das datenabhängige
Maximum-Rectangle ist rezept-planbar nicht auf der GPU reproduzierbar. Sie ist
der SOLL-Zustand dieses Rezepts, **keine** pauschale GPU-Paritätszusage: Jede
weitere CPU-Route (oder ein GPU-Laufzeitfehler) ist ein Fail mit Fix-Pflicht,
nicht akzeptiert. Messwerte des grünen GPU-Laufs: 22/22 Paare, Gesamtdauer
~7,0 s, 12 Paare mit endlicher PSNR, Minimum 81,5 dB, maximale Abweichung
1 LSB (Goldens auf der CPU-Referenz).

**Kosten-/Dauer-Pflicht (F-074).** Der Runner berichtet pro (Sample, Rezept)
die Dauer und die Gesamtdauer (Text und `--json`). Kosten/Dauer der Matrix
werden im betroffenen Feature-Dokument bzw. im Commit dokumentiert. Die Matrix
ergänzt F-074 (Ausführungs-/Gesamtkosten pro Rezept-Set), ersetzt aber keine
Benchmarks und hat kein hartes Laufzeitbudget. Volle Auflösung ist
kostenpflichtig; Benchmarks bleiben der Ort für Mikro-Messungen.

**GUI-headless-Modus (SOLL, Umsetzung Slice 2).** Dasselbe Rezept-Set wird
zusätzlich headless über `LuminaApp` (egui Context + tempdir) geladen,
gerendert und gegen dieselben Goldens geprüft; die GUI verwendet denselben
Core-Renderpfad (keine zweite Pipeline). Bis dahin deckt der CLI-Runner die
Rezept-Matrix ab.

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
  bzw. artefaktgebundenes Rezept-Set laut abgelehnt, env-gated
  CR3-End-to-End-Lauf (siehe [Rezept-Matrix](#rezept-matrix-lrpar-matrix-recipe))
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
