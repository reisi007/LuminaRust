# LuminaRust Umsetzungsplan

Dieser Plan ist eine lebende Arbeitsliste. Er wird während der Implementierung
fortgeschrieben. Erledigte Aufgaben werden nach bestandener unabhängiger
Verifizierung und bestätigter Testabdeckung aus dieser Datei entfernt — es gibt
keine dauerhafte Liste abgehakter Aufgaben. Verifiziert Erledigtes steht unten
als kompakte Kurzliste; die Details liegen in den Feature-Dokumenten und der
Git-Historie.

## Stand (2026-08-20)

Offene Arbeit und Abgrenzung bis MVP:

- **LIZ-ENTSCHEIDUNG (beantwortet 2026-08-20: interim proprietär, spätere
  Entscheidung offen; MVP-Release-Gate):** Projekteigentümer hat entschieden,
  die Wahl vorerst **nicht** zu treffen und den **proprietären/kommerziellen
  Status** beizubehalten (kein `license`-Feld in den 9 Crates, keine
  `LICENSE`-Datei). Sobald entschieden: MIT / Apache-2.0 / Dual
  MIT+Apache-2.0 / MPL-2.0 — dann `license`-Felder + Root-`LICENSE`
  ergänzen (F-073-R2). **Lensfun** (MVP-Teil seit F-098-N1, LGPL-3.0
  dynamisch gelinkt, DB CC-BY-SA-3.0) ist in `THIRD-PARTY-NOTICES.md`
  dokumentiert und gilt unabhängig von der späteren Wahl.
- **F-098-N1** Lensfun-Integration als **Pre-MVP**-Bestandteil (Phase 3):
  **verifiziert erledigt** (2026-08-20) — Crate `lumina-lensfun` + lumina-core
  `lensfun`-Feature + Pipeline-Integration mit graceful fallback + Tests
  (siehe Phase 3). Folgeaufgaben **F-098-N2** (CLI-Verdrahtung, `7922ac1`),
  **F-098-N3** (CI-Container, `e1bfed8`), **F-098-N4** (Lizenz-Doku,
  `a72cb80`) ebenfalls **verifiziert erledigt** (2026-08-20) und entfernt.
- **F-082 / F-083** SAM-2.1-Segmentierungsadapter + Prompt-Tests (Phase 6):
  **verifiziert erledigt** (2026-08-20, `452d8a4`) — SAM-2.1-Modellfamilie
  mit dynamischer Variantenwahl, `PromptMaskInference`-Interface, 17 Tests;
  Lizenz an der Quelle geprüft (Code+Gewichte Apache-2.0; Ultralytics-AGPL-
  Weg bewusst ausgeschlossen). Offene Folgearbeit: ORT-Pfad, MaskGraph/CLI-
  Einbindung, hash-gepinnte ONNX-Fixtures.
- **F-073** Fixtures-Lizenzierung/Audit (Phase 11): Audit-Dokumente
  existieren; **R1 geschlossen (2026-08-20)** — Autor via EXIF + Commit
  belegt, uneingeschränkte Nutzungs-/Distributionsgewährung für LuminaRust
  dokumentiert (`sample-data/raw/README.md`, §4/§8); **R2** = LIZ-
  Entscheidung (oben, offen).
- **F-078** Lizenz-/Modell-/Distributionsaudit (Phase 11): Audit-Dokumentation
  abgeschlossen (`THIRD-PARTY-NOTICES.md`, `fixtures-licensing.md`, inkl.
  **Lensfun-LGPL-3.0-Eintrag** seit 2026-08-20); Beleg
  LibRaw = einzige Pflicht-/Datenbank-Dependency, `ort`
  `=2.0.0-rc.13` optional hinter `onnx-rt`, ONNX Runtime MIT, BiRefNet
  Apache-2.0; **unabhängig verifiziert BESTANDEN (2026-08-20)** — Auflagen
  (NOTICES-Vollständigkeit 480/480, stale EN-R1-Texte, Crates-Zahl) direkt
  behoben. Offen bis Final-Release: R3/R4 (Bundle-Artefakte), R5 (ADR-0002-
  Dreifachlizenz), Lensfun-SPDX/DB-Lizenz-Detail, R2-LIZ.
- **F-072** CI-Gates: unabhängig verifiziert **bestanden** (2026-08-20) —
  verbleibender Aufwand nur Eintrag-Nachpflege (siehe Phase 11).

## Antworten des Projekteigentümers (2026-08-20, interaktiv abgefragt)

Alle vier Fragen wurden am 2026-08-20 beantwortet; die Ergebnisse sind in die
betroffenen Feature-Dokumente übernommen. Noch offene Folgearbeit ist unten
mit Datum markiert.

1. **LIZ-Entscheidung — Projektlizenz:** **vorerst offen / proprietären
   Status beibehalten.** Keine `LICENSE`-Datei, keine `license`-Felder;
   interim ist das Projekt bewusst unlizenziert/kommerziell bis zur
   endgültigen Entscheidung (F-073-R2/F-078 bleiben offen). Lensfun-Pflichten
   (LGPL-3.0 dynamisch gelinkt → nur Sammelwerk-Hinweis; DB CC-BY-SA-3.0 →
   Attribution) sind in `THIRD-PARTY-NOTICES.md` dokumentiert und gelten
   unabhängig von der späteren Wahl.
2. **Fixtures-Lizenzgewährung (F-073-R1):** **GELÖST** — Eigentümer hat eine
   **uneingeschränkte Nutzungs- und Distributionsgewährung für das
   LuminaRust-Projekt** erteilt (2026-08-20); eingetragen im Provenienz-Block
   `sample-data/raw/README.md`, Status in `feature/quality/fixtures-licensing.md`
   §4/§8 aktualisiert. Verifikation der Doku im Rahmen der F-078-Abnahme.
3. **Interaktives Segmentierungsmodell (F-082):** **SAM 2 bestätigt und
   umgesetzt** (2026-08-20). F-082/F-083 sind **verifiziert erledigt**
   (Commit `452d8a4`, unabhängig BESTANDEN): SAM-2.1-Modellfamilie
   `sam2.1_hiera_*` mit dynamischer Variantenwahl (DeviceProfile, Kernzahl-
   Schwellen + Override), PromptMaskInference-Interface + Stub-Backend,
   Prompt→Tensor-Kontrakt, 17 F-083-Tests. Lizenzprüfung an der tatsächlichen
   Quelle abgeschlossen: **Code und Gewichte Apache-2.0**
   (facebookresearch/sam2 `LICENSE`, HF-Model-Cards, Meta-Announcement;
   Ultralytics-AGPL-Paketweg wird bewusst nicht genutzt — ONNX-Export via
   ORT-Tooling aus Meta-Checkpoints). Die Projektlizenz (Frage 1) blockierte
   die Implementierung nicht (Modell ist Apache-2.0, unabhängig von der
   Projektlizenz). BiRefNet-Prüfung (R6) ergab: tatsächlich **MIT** (GitHub
   `LICENSE` + HF-Card) — Manifest-/Doku-Korrektur 2026-08-20.
4. **Produktname (F-101-F1):** **später entscheiden** (kein MVP-Blocker) —
   bleibt als offener Punkt dokumentiert (`docs/naming-brainstorm.md`).

**Post-MVP (nicht MVP-blockierend):** F-019 (CLI `migrate_sidecar`), Phase 9
Index (F-064…F-067), WASM-Browser (F-069…F-071), MCP-Erweiterungen
(CLI-Tools als MCP-Tools, `lumina mcp`-Subcommand, Vision-Agent,
Namensfindung — siehe Phase 7), Lensfun-Ausbau (CA via Lensfun, automatische
Profil-Erkennung per EXIF, WASM-Port).

### Verifiziert erledigt (Kurzliste, neueste zuerst)

Details in Git-Historie und Feature-Dokumenten; „Lensfun bleibt Post-MVP" ist
durch F-098-N1 obsolet und bewusst entfernt.

- **F-098** Objektivkorrekturen (manuelles Modell, `apply_geometry`-Reihenfolge
  distortion → vignette → perspective → CA → crop; Presets; RenderKey) —
  unabhängig verifiziert 2026-08-20. Bekannte Testlücke (nicht blockierend):
  Preset-Koeffizienten und Grün-Referenz nicht pixel-explizit assertet.
- **F-098-N1** Lensfun-Integration (Pre-MVP): Crate `lumina-lensfun`
  (FFI + Safe Wrapper, 6 Native-Tests) + lumina-core `lensfun`-Feature +
  Pipeline-Integration mit byte-identischem Fallback — unabhängig verifiziert
  2026-08-20 (BESTANDEN). Folgeaufgaben F-098-N2…N4 offen (Phase 3).
- **F-075** Speicherbudgets + Abbruch großer RAW/Masken; **F-102** LibRaw-
  Version in Decode-/Render-Identität; **F-097** Vignette/Körnung;
  **F-050** Masken-Entscheidungsschicht — verifiziert 2026-08-20.
- **F-074-N1…N5** Performance-Methodik (N1 SOLL/ADR 0003, N2 Gerüst, N3 erste
  Benchmarks + Baseline, N4 Analyse/Hotspots, N5 compare.mjs report/warn/gate)
  — verifiziert 2026-08-19. Offene Hotspot-Tasks A1…A4, siehe Abschnitt unten.
- **F-101** MCP AI-Agent-Schnittstelle (`lumina-mcp`, 8 Tools inkl.
  `lumina_analyze`, `docs/skills/lumina.md`, SOLL
  `feature/platform/mcp-server.md`) — verifiziert 2026-08-19.
- **F-036-N1** As-Shot-WB-Kontext; **F-042** Render-Einstiegspunkt
  `render_frame`; **F-042-N1** Source-Actions-Persistenz + `dust-removal`;
  **F-041** Matching-Messbereich post-Crop; **F-043** 22 Property- + 7
  Referenzbildtests; **F-085** behaviorale Tests; **F-047/F-080** ONNX-Adapter +
  Modellfähigkeiten; **F-048/F-051/F-079/F-081** Masken-Laden/-Quellen;
  **F-076** Migrationsstrategie; **F-077** Backup-/Recovery-Tests;
  **F-072-N2** wasm32-Check lumina-gui; **F-100** GUI-Konventionen — verifiziert
  2026-08-17…19.

## Arbeitsregeln

- Vor jeder Umsetzung `Agents.md`, `feature/README.md` und das betroffene
  Feature-Dokument lesen.
- Wenn Code und SOLL-Zustand widersprechen, zuerst den Zielzustand klären und
  dokumentieren.
- Jede Aufgabe erhält bei Delegation eine Feature-ID, einen klaren Umfang und
  Abnahmekriterien.
- Der Build-Agent delegiert die Implementierung und anschließend die Prüfung an
  unterschiedliche Subagenten.
- Implementierungs-Agenten werden als `general`-Agenten delegiert (nicht als
  `build`-Agenten); Verifikation läuft immer über einen davon unabhängigen
  `general`-Agenten.
- Der unabhängige Verifizierungs-Agent muss Korrektheit und Testabdeckung
  bestätigen, bevor die Aufgabe aus dieser Datei entfernt wird.
- Eine fehlgeschlagene Verifizierung lässt die Aufgabe offen und erzeugt eine
  konkrete Folgeaufgabe.

## Performance-Methodik (hohe Priorität)

Gegenstand ist F-074: Performance-Tests als messbare, reproduzierbare Methodik
mit Baselines, Budgets und semi-automatischer Regressionserkennung
(„erkennen und verhindern – außer wir nutzen bewusst mehr Features“).
Normativ: `feature/quality/performance-benchmarks.md`, Entscheidung in
`docs/adr/0003-performance-benchmarking.md`. Bewusste Nicht-Ziele in diesem
Block: keine feingranularen WASM-Thresholds in CI (native Criterion-Messung
ist Proxy für alle Archs), keine absoluten Laufzeitziele ohne
Umgebungskontext. Feature-Wachstum wird als bewusste, dokumentierte
Budget-Anpassung im selben Commit wie das Feature behandelt.

Verifiziert erledigt und entfernt: F-074-A4 (PNG-Export-Encode: direkter
`PngEncoder` ohne 16-MB-Clone auf non-mutating Pfaden, byte-identisch,
Commit `7045da7`; Dither ~46 % + DEFLATE ~52 % dominieren den Benchmark —
kein Wandzeit-Gewinn dort, Speicherdruck eliminiert; Kompressions-Default
bewusst unverändert), F-074-N1 (Methodik-SOLL, ADR 0003),
F-074-N2 (Setup-Gerüst), F-074-N3 (erste echte Benchmarks: 32 deterministische
Fixtures, Baseline-Erfassung, `compare.mjs`-Bestandsvalidierung), F-074-N4
(Baseline-Analyse: Hotspot `apply_recipe` ~91 %, abgeleitete IDs A1…A4),
F-074-N5 (`scripts/perf/compare.mjs` report/warn/gate, optionaler
nicht-blockierender CI-`bench`-Job im `warn`-Modus), F-074-A1
(Adjustments-Kernel: per-Kanal-LUT-Fusion, −40 % auf dem Hotspot,
byte-identisch über 128 Trigger-Kombinationen, Commit `8dacd3d`),
F-074-A2 (Decode-Durchsatz: Kopie entfernt + RGBA-Fast-Path, −19,6 %/−18,5 %
vs. Baseline, pixel-identisch, Commit `a03c272`), F-074-A3 (Auto-Tone-Kernel:
Single-Pass-`mean_luminance` statt Voll-Sort, −95 % `match_total_exposure`,
−22 % `analyze_tone`/`suggest_auto_tone`, wert-identisch, Commit `2253de5`) —
jeweils unabhängig verifiziert (BESTANDEN).

## Phase 0: Zielzustand und Architektur

(alle Punkte umgesetzt und verifiziert — 2026-08-17)

## Phase 1: Sidecar-Domain-Modell

(alle Punkte umgesetzt und verifiziert — 2026-08-17)

## Phase 2: Rezept, virtuelle Kopien und Migrationen

**Produktentscheidung (2026-08-17, Pre-MVP, präzisiert):** Wir befinden uns in
der Pre-MVP-Phase — das Sidecar-Schema wird bei Bedarf **bewusst nicht
abwärtskompatibel** abgeändert. Es gilt: (a) Schemaänderungen sind bis zum MVP
Breaking Changes, Altdateien müssen nicht lesbar bleiben; (b) die
Migrations-Maschinerie (`migrate_sidecar_file`, `.bak`-Backup, `migrate_json`)
bleibt dauerhaft im Code und wird ab dem MVP für Release-Migrationen genutzt;
(c) die v1→v2-Migration mit ihren Tests (aus F-089/F-090) bleibt als Muster
erhalten, aber **pre-MVP gibt es keinen Zwang, für jede Migration einen
eigenen Test zu schreiben** — die Regel „Tests für jede Migration" gilt ab dem
MVP. Konsequenz: Die Spec-Vorgabe „Altdateien mit flacher adjustments-Map
bleiben als schema_version: 1 gültig" (pipeline.md, Abschnitt
Bearbeitungsregler) ist als Produktanforderung bis zum MVP ausgesetzt; der
v1→v2-Migrationspfad mit Tests wird trotzdem umgesetzt.

- [ ] **F-019** (deferriert auf Post-MVP) CLI `migrate_sidecar`
  (crates/lumina-cli/src/main.rs) auf `lumina_sidecar::migrate_sidecar_file`
  umstellen (`.bak`-Backup + Lock); erst nach MVP relevant, da bis dahin keine
  Migrationen laufen. Verifikations-Hinweis: Library-Teil ist verifiziert.

## Phase 3: Renderpipeline und Cache

F-098-N1 (Lensfun-Integration als **Pre-MVP**-Bestandteil) ist verifiziert
erledigt und entfernt: neues natives Crate `crates/lumina-lensfun`
(Workspace-Member, nicht in wasm32-Checks; Feature `native` default off,
ohne native leere Lib; build.rs linkt System-liblensfun dynamisch via
pkg-config — LGPL-3.0, Datenbank CC-BY-SA, keine neuen Crates/bindgen/cc;
handgeschriebenes `extern "C"`-FFI-Subset; Safe Wrapper `LensfunDb::load_system`
+ `Corrector::for_camera` (None = kein Profil/Identität → graceful fallback) +
`geometry()`/`color_gain()`/`is_identity()`; 6 Native-Tests grün gegen die
reale Profil-DB). `lumina-core`: Feature `lensfun = [dep:lumina-lensfun,
lumina-lensfun/native]` default off (Default-Build + wasm32 bleiben grün);
cfg-gatedes `RenderContext.lensfun`-Feld; `apply_geometry`/`apply_lens` nutzen
per-Pixel Lensfun-Geometrie/Vignette bei nicht-identischem Corrector
(Priorität Lensfun > manuell > Identität), sonst byte-identisches manuelles
Modell (Test `lensfun_none_is_byte_identical_to_default_pipeline`); CA bleibt
manuell (MVP-Grenze). Alle RenderContext-Konstruktionsstellen cfg-korrekt
(cli/gui/mcp/bench deklarieren `lensfun`-Forwarding-Feature). Gates: fmt,
check --workspace, wasm32 core+gui, Clippy workspace CI-Config + Feature
`-D warnings`, Tests Default (204+7) + Feature (207+7) + lumina-lensfun
(6) + lumina-cli (13+8) — alle grün; **unabhängig verifiziert 2026-08-20**
(BESTANDEN; M1 per-pixel-FFI-Overhead ohne Benchmark, M3
crop-factor-Offset-Hack dokumentiert; nicht blockierend). SOLL:
pipeline.md F-098.

Folgeaufgaben **F-098-N2 (CLI-Verdrahtung, `7922ac1`), F-098-N3
(CI-Container, `e1bfed8`), F-098-N4 (Lizenz-Doku, `a72cb80`)** sind
umgesetzt, unabhängig verifiziert (BESTANDEN, 2026-08-20) und entfernt.

## Phase 4: RAW-Verarbeitung

Diese Phase ist ein verbindliches MVP-Gate. Der erste User-Test gilt erst als
produktseitig vollständig, wenn native RAW-Dekodierung, Orientierung und die
minimalen RAW-Golden-Tests vorhanden sind. **MVP-Grenze (2026-08-17):** Das MVP
umfasst CLI und native Desktop (inkl. RAW). Web/WASM-RAW ist aus dem MVP
geschoben (Post-MVP via `libraw-wasm`, Feature `wasm-js`), die Architektur wird
aber kompatibel gehalten (einheitlicher `decode_bytes`/`RawMetadata`-Vertrag,
`cfg(target_arch = "wasm32")`-Kapselung).

Verifiziert erledigt und entfernt:
- **F-036-N1** As-Shot-WB-Kontext `apply_recipe_with_white_balance`;
  CLI/GUI reichen `RawMetadata.camera_white_balance` durch (pipeline.md F-036).
- **F-102** LibRaw-Version in Decode-/Render-Identität
  (`DecodeFingerprint.version`/`RenderKey.decode_version` tragen bei Decoder
  `"libraw"` die gelinkte LibRaw-Version; `libraw_version()` in lumina-raw;
  verhindert stillschweigendes Cache-/Masken-Reuse bei LibRaw-Upgrade — CR3-
  Dimensionen ändern sich zwischen 0.21.x und 0.22.x). Bekannte Grenze:
  `libraw_version()` liefert das Build-Suffix (`"0.22.2-Release"`); optional
  später auf das numerische Tripel normalisieren.
- **F-098** Objektivkorrekturen (manuelles Modell): `LensCorrection` in
  `lumina-sidecar`, `validate_lens`/`apply_lens`/`apply_ca` in `lumina-core`,
  integriert in `apply_geometry`; Presets wide-light/tele-light/standard-
  neutral; `mask_recipe.lens_correction = None` schließt Geometrie aus dem
  Masken-Hash aus, `recipe_hash` invalidiert den RenderKey. **Lensfun-Erweiterung
  als MVP läuft unter F-098-N1 in Phase 3** (nicht mehr Post-MVP).

## Phase 5: Auto-Tone und Exposure Matching

(abgeschlossen: F-042 Render-Einstiegspunkt, F-041 Matching-Messbereich,
F-043 Property-/Referenzbildtests — jeweils verifiziert erledigt und entfernt)

## Phase 6: Persistente AI-Masken

F-082/F-083 sind verifiziert erledigt und entfernt (Commit `452d8a4`):
SAM-2.1-Modellfamilie `sam2.1_hiera_*` (tiny/small/base_plus/large) mit
dynamischer Variantenwahl über `DeviceProfile` (Kernzahl, override),
`PromptMaskInference`-Interface + Stub-Backend, Prompt→Tensor-Kontrakt,
17 F-083-Tests — unabhängig verifiziert, BESTANDEN (2026-08-20). Offene
Folgearbeit (nicht MVP-blockierend): echter ORT-Pfad hinter `onnx-rt`,
MaskGraph/CLI-Einbindung, hash-gepinnte ONNX-Fixtures.

## Phase 7: CLI und Batch

(CLI/Batch-Punkte umgesetzt und verifiziert — 2026-08-17; F-101 MCP-Schnittstelle
verifiziert erledigt: `lumina-mcp`-Crate, 8 Tools inkl. `lumina_analyze`,
Agent-Skill `docs/skills/lumina.md`, SOLL `feature/platform/mcp-server.md` —
2026-08-19)

- [ ] **F-101-F1** Erweiterter MCP-Scope: volle CLI-Abdeckung als MCP-Tools
  (`lumina_import`, `lumina_batch`, `lumina_reindex`, `lumina_dust_removal`
  u. a.), `lumina mcp` als CLI-Subcommand, Vision-fähiger Agent
  (strukturierte `lumina_analyze`-Daten für Agents ohne Vision), finale
  Produktnamen-Entscheidung (`docs/naming-brainstorm.md`). Konzepte stehen im
  SOLL `feature/platform/mcp-server.md` (Abschnitt „Erweiterter MVP-Scope").

## Phase 8: Desktop-GUI (F-103, MVP)

UI-Konventionen F-100 sind spezifiziert, verifiziert und für jede GUI-Arbeit
verbindlich — normativ in `feature/platform/cli-gui-wasm.md` (Abschnitt
F-100). SOLL für den MVP-Scope: ebenda Abschnitt „Desktop-GUI" und „Erster
visueller User-Test" — `cargo run -p lumina-gui` lädt PNG/JPEG/WebP + native
RAW per Pfad oder Drag-and-drop; Preview, Exposure (`-10..=10`), Contrast
(`-1..=1`) über `ImageFrame` + `EditRecipe`; native Sidecars. Browser-
Dateispeichern, ONNX, Masken-Inferenz, Cache und Mehrbild-Synchronisierung
sind bewusst Post-MVP.

**Ist-Stand (2026-08-21):** `lumina-gui` (egui/eframe) hat einen verifizierten
MVP-Slice. **Umgesetzt und unabhängig verifiziert (BESTANDEN, 2026-08-21):**
F-103-N1/N2/N3 (Modul-Leiste Library/Develop/Export, acht kollabierbare
Develop-Sektionen in F-100-Reihenfolge, Filmstreifen mit Preview-Cache-
Thumbnails, LR-dark-Theme mit zentraler Palette + WCAG-Kontrasttests,
i18n-Gerüst englisch — 0 deutsche UI-Literale, Regler-Semantik Einzel-Reset/
Alt-Scroll/Anzeigeskala, Before/After `Y`, Auto-Button, WB-Pipette),
**F-103-N4** (interaktive Maskenwerkzeuge Pinsel/Verlauf/Radial auf der
Vorschau, Overlay via `rasterize_prompt`, Persistenz als MaskPrompt/MaskLayer,
lokale Regler im Panel), **F-103-N5** (Exportieren-Modul; gemeinsame
`lumina_core::export_image`-Logik für CLI+GUI, byte-identisch getestet;
Same-Path-Schutz; Original unverändert; atomarer Write via
`write_atomically`), **F-103-M1** (i18n-Restliterale beseitigt),
**F-103-N7** (Presence-Regler Texture/Clarity/Dehaze F-094 sowie
Vibrance/Saturation F-092; beide als Regler in der Color-Sektion in F-100-
Reihenfolge — Color Grading → Presence → Vibrance/Saturation — über bestehenden
`set_adjustment`/Rezeptpfad; Pipeline-Stufen `apply_presence` und
`apply_vibrance_and_saturation` vorhanden, daher keine „nicht verfügbar"-
Kennzeichnung; GUI-Tests für Rezeptfelder/Domänen/Reset; Modul-Shortcuts
G/D/E mit Textfeld-Fokus-Schutz).
**F-103-N8** (CLI-Doppelrender im Nicht-Match-Pfad beseitigt, byte-identisch
getestet), **F-103-N9** (kittest-Snapshot-Regressionen: 5 Goldens, `#[ignore]`+
`UPDATE_SNAPSHOTS=true cargo test -p lumina-gui --test kittest_snapshots --
--ignored`; Modul-Shortcuts G/D/E).
Offen: F-103-N6 (visueller User-Test), F-103-N10 (Sektionsreihenfolgen-Entscheid). Browser-Dateispeichern, ONNX, Masken-
Inferenz, Cache-Synchronisierung und Mehrbild-Bearbeitung bleiben bewusst
Post-MVP; WASM ist dokumentierte Capability-Grenze, keine MVP-GUI.

- [ ] **F-103-N10** Sektionsreihenfolgen-Inkonsistenz klären (Befund
  Verifizierung 2026-08-21, niedrig): Das SOLL (cli-gui-wasm.md F-100) listet
  die Sektion „Effects" (Vignette/Grain) VOR „Detail" (Sharpening/Noise
  Reduction), während die normative Reglerreihenfolge im selben Dokument
  Sharpening → Noise Reduction → Vignette/Grain verlangt und Lightroom Classic
  „Detail" vor „Effects" anordnet. Produktentscheidung nötig: entweder
  Sektionsreihenfolge im SOLL korrigieren (Detail vor Effects, LR-Classic-
  konform) oder Abweichung dokumentieren; danach GUI-Anordnung angleichen.
- [ ] **F-103-N8** Export-Render-Doppelarbeit in der CLI (Befund Verifizierung
  2026-08-21, niedrig): `process_selected` rendert aktuell zweimal
  (Warn-Render + `export_image`); ohne `match_total_exposure` ließe sich das
  erste Render-Ergebnis wiederverwenden. Umsetzung gemäß F-074-Methodik
  (byte-identischer Output nachweisen, Budget-Vergleich im selben Commit).
- [ ] **F-103-N9** Automatisierte UI-Screenshot-Verifikation (Vorschlag
  2026-08-21, angepasst 2026-08-21): Headless-Snapshot-Tests der **Desktop**-
  GUI via `egui_kittest` (Golden-Screenshots der drei Module und
  Develop-Sektionen, Pixel-Diff mit Toleranz, läuft in CI ohne Browser) als
  verbindliche Regressionssicherung. Die Desktop-App ist die **einzige
  MVP-GUI**; der WASM/trunk-Pfad wird ausschließlich dokumentiert (Capability-
  Matrix) und ist Post-MVP — kein Playwright-/Browser-Harness im MVP.
  Vision-gestützte Screenshot-Analyse (ui-review-Checkliste) als Ergänzung
  möglich.
- [ ] **F-103-N6** Erster visueller User-Test: `cargo run -p lumina-gui` mit
  PNG/JPEG/WebP + nativen RAW per Pfad und Drag&drop; Preview + Exposure/
  Contrast ändern den Renderstand; Sidecar wird geschrieben und beim Neustart
  wiederhergestellt; WASM (`trunk serve`/`trunk build --release`) bleibt
  buildbar und weist RAW als nicht verfügbare Capability aus. Abnahme:
  reproduzierbare Befehle aus cli-gui-wasm.md; unabhängiger Verifizierungs-
  Agent bestätigt F-100-Checkliste + Tests (BESTANDEN). Letzter Schritt vor
  Abschluss von Phase 8.

## Phase 9: Optionale zentrale Indizierung (Post-MVP)

- [ ] **F-064** Minimalen, vollständig wiederaufbaubaren Indexumfang festlegen:
  Pfad, Quellhash, Metadaten, Sidecarstatus, Jobstatus und Cacheverweise.
- [ ] **F-065** SQLite-Index als optionalen Adapter implementieren, ohne
  Rezeptdaten nur dort zu speichern.
- [ ] **F-066** Rebuild aus Sidecars, Aktualisierung, Locking und beschädigte
  DB behandeln.
- [ ] **F-067** Nachweisen, dass Löschen der DB keine Bearbeitungsdaten,
  virtuellen Kopien oder Masken zerstört.

## Phase 10: WASM und Plattformen (Post-MVP)

- [ ] **F-069** Browser-Dateiimport, temporären Speicher und Exportmodell
  definieren.
- [ ] **F-070** ONNX im Browser als optionale Fähigkeit mit klarer
  Capability-Anzeige behandeln.
- [ ] **F-071** native, Desktop- und Browser-Limits für Bildgröße, Speicher,
  Threads und GPU dokumentieren.

## Phase 11: Qualität, Performance und Release

**F-072** ist verifiziert erledigt und entfernt (CI existiert und läuft grün:
fmt, `cargo check --workspace --all-targets`, `cargo test --workspace
--all-targets` inkl. Golden-/Property-Tests (F-043), native zdata-Tests,
Clippy `-D warnings` (0 Fehler), wasm32-Checks lumina-core + lumina-gui,
Docs-Check — unabhängig verifiziert 2026-08-20). Bewusste Ausnahme: das
`onnx-rt`-Feature ist vom Clippy-Lauf ausgeschlossen (zieht `ort` →
`native-tls`/`openssl-sys`, im gepinnten CI-Container ohne libssl-dev nicht
baubar); der Pfad wird lokal gelintet. Bekannte benigne Warnung: `load_mask_planes`
dead-code unter wasm32 (optional stilllegen).

- [x] **F-073** Fixtures-Lizenzierung/Audit: **R1 geschlossen (2026-08-20)** —
  Autor via EXIF + Commit belegt, uneingeschränkte Nutzungs-/Distributions-
  gewährung für LuminaRust dokumentiert (`sample-data/raw/README.md`); R2
  (LIZ) offen — interim proprietär (Eigentümer-Antwort 2026-08-20); Audit-Doku
  existiert.
- [x] **F-078** Lizenz-/Modell-/Distributionsaudit: Belege in
  `THIRD-PARTY-NOTICES.md`/`fixtures-licensing.md` **unabhängig verifiziert
  (BESTANDEN, 2026-08-20)**; **Lensfun-LGPL-3.0-Eintrag ergänzt** (dynamisches
  Linken, CC-BY-SA-3.0-Datenbank, Quellangebot im Release-Bundle — F-098-N1)
  und **SPDX-Auflösung nachgetragen** (2026-08-20): upstream README v0.3.4
  bestätigt libs = LGPL-3.0, apps = GPL-3.0 (nicht ausgeliefert), DB =
  CC-BY-SA-3.0; Fedora-Ausdruck `LGPL-3.0-only AND CC-BY-SA-3.0 AND
  LGPL-2.1-or-later AND GPL-3.0-only`. **R5 geschlossen:** ADR 0002 nennt
  nun die LibRaw-Dreifachlizenz inkl. permissiver Option. **R6 geschlossen:**
  SAM-2-Code+Gewichte Apache-2.0 an der Quelle verifiziert; BiRefNet-Prüfung
  ergab tatsächlich **MIT** (GitHub-LICENSE + HF-Card) — Manifest- und
  Doku-Angaben korrigiert (2026-08-20). Pre-Release-Artefakte verbleiben
  offen (R3/R4): Lensfun-/LibRaw-Lizenztexte + Quellangebot + Modell-
  Lizenzen (BiRefNet MIT, SAM 2 Apache-2.0, ONNX Runtime MIT) im
  Release-Bundle mitliefern.

F-075 ist verifiziert erledigt und entfernt (`MemoryBudget` + `check_decode`/
`check_mask` in `crates/lumina-core/src/memory.rs`; Verdrahtung in lumina-raw
(nativ, vor `dcraw_process`) und `rasterize_prompt` (vor Matten-Allokation);
SOLL in `feature/quality/performance-benchmarks.md` F-075 — unabhängig
verifiziert 2026-08-20). Bekannte Grenzen: `MemoryBudgetError::Overflow`-Pfad
ungetestet (praktisch unerreichbar). Die vormals latente Flakiness der
`from_env`-Tests durch unsynchronisierte Env-Mutation (`from_env_parses_
valid_vars` setzte `LUMINA_MAX_*` parallel zu `from_env_falls_back_to_
default_on_missing_vars`) ist mit Commit `4459851` behoben: ein prozessglobaler
Mutex im `tests`-Modul serialisiert die beiden env-abhängigen Tests
(unabhängig verifiziert, 60× Stress-Lauf flake-frei, 2026-08-20).

## Performance-Verfeinerung — Lightroom-artige interaktive Geschwindigkeit (Verifikation ausstehend)

Ausgangslage (Nutzerbericht, M5 Pro, Debug-Build): Die GUI decodiert/lädt RAW
über LibRaw-Decode + vollständige CPU-Pipeline **synchron im Main-Thread** bei
jedem Regler-Eingriff; kein GPU-Pfad, kein VRAM-Cache, keine Pyramid-/Draft-
Auflösung, erstes Bild wird nicht automatisch geladen. Dateiwechsel und
Regler-Drags sind nicht mehr flüssig benutzbar. Die folgenden fünf Ticks sind
als **offene, nicht erledigte** Verifikations-/Verfeinerungsaufgaben eingetragen
– bewusst **kein** GPU-/Shader-Umbau in diesem Schritt. Verifiziert wird gegen
`feature/architecture/pipeline.md`, `feature/quality/performance-benchmarks.md`,
`feature/platform/cli-gui-wasm.md` sowie `crates/lumina-gui/src/lib.rs`.

Querverweis: Die bestehende Hotspot-Ableitung **F-074-A1…A4** (Apply-Kernel,
Decode-Durchsatz, Auto-Tone-Kernel, PNG-Encode) bleibt bestehen; diese fünf
Ticks adressieren die **interaktive GUI-Latenz**, nicht die Batch-/Kernel-Ebene.

- [ ] **PERF-GUI-1** DAG/Schritt-Trennung + demosaizierte Basiskachel cachen,
      nur Color/Tone bei Exposure-Änderung invalidieren.
  - **Verifikationsstatus:** Teilweise vorhanden. Die Pipeline ist bereits als
    Stufenliste `Decode → SourceActions → AutoAnalysis → Adjustments → Masks →
    Crop → Output` (`feature/architecture/pipeline.md` §Pipeline-Reihenfolge)
    strukturiert; `RenderKey.stage_digest` kennt stufenspezifische Digests
    (`decode`/`mask`/`histogram`/`render`, pipeline.md ~Z.711), sodass eine
    reine Ausgabegrößenänderung den Decode-Cache nicht invalidiert. **Aber:**
    keine GPU/VRAM, kein Schritt-Cache, und die GUI nullt bei jeder
    Regleränderung den **gesamten** `render_key` (`lib.rs:1234` `set_adjustment`,
    `lib.rs:2055` `mark_dirty`) statt nur der Color/Tone-Stufe.
  - **Verfeinerung nötig:** Schritt-Caching/Invalidierung auf CPU-Ebene
    (Demosaiced-Basis als `ImageFrame`/`Rgba8Srgb` im RAM halten, ggf.
    Staging-Textur in VRAM) und Nutzung von `stage_digest`, sodass Exposure-/
    Color-Änderungen nur die Adjustments-Stufe neu rendern. GPU/VRAM-Variante
    ist langfristig und braucht Architekturentscheidung (ADR): `lumina-core`
    darf laut `Agents.md` keine GPU-/GUI-Abhängigkeit erhalten.
  - **Betroffene Module:** `lumina-core` (Render/Stufen/`RenderKey`),
    `lumina-gui` (Invalidierungssteuerung), optional neu `lumina-gpu`
    (langfristig).
  - **Abnahmekriterien:** Bei Exposure-Drag wird nur die Adjustments-Stufe
    neu berechnet (nachweisbar über `render_frame`-Stufen-Timing + Golden-
    Identität zu F-043); Decode/Demosaic wird bei Regleränderung **nicht**
    wiederholt; kein stillschweigender Fallback bei Cache-Miss.

- [ ] **PERF-GUI-2** GPU-Uniforms für Reglerparameter (Fragment-Shader, UBOs).
  - **Verifikationsstatus:** Nicht vorhanden. `lumina-core` ist plattformneutral
    und enthält keinen GPU-Pfad (kein wgpu/GLSL/Shader/Uniform im Quellbaum,
    per Grep über `crates/` verifiziert); `render_frame` läuft vollständig CPU/
    RGBA8. Dies **kollidiert** mit der Architekturgrenze `Agents.md`
    (`lumina-core` darf keine GPU-/native-Abhängigkeit annehmen) und mit der
    WASM-Capability-Matrix (native Desktop ist die einzige MVP-GUI).
  - **Verfeinerung nötig:** Neue Architekturentscheidung (ADR) +
    optionaler `lumina-gpu`-Adapter hinter Feature-Flag; `lumina-core` bleibt
    CPU-Referenz. **Nicht** im aktuellen Schritt umsetzen.
  - **Betroffene Module:** `lumina-core` (Referenzpfad), neu `lumina-gpu`
    (langfristig), `lumina-gui` (Backend-Auswahl).
  - **Abnahmekriterien:** GPU-Pfad liefert byte-/wert-identisches Ergebnis zum
    CPU-Pfad (F-043-Toleranzen); aktivierbar ohne `lumina-core`-API-Bruch;
    WASM-Build bleibt ohne GPU-Capability grün.

- [ ] **PERF-GUI-3** Draft-Modus & Auflösungspyramiden (Mipmaps,
      Viewport-Auflösung beim Draggen, volle Auflösung erst bei Mouse-Up/
      150 ms Idle).
  - **Verifikationsstatus:** Nicht vorhanden. Keine Mipmaps/Pyramiden; die GUI
    rendert synchron `render_frame` in voller Quellauflösung (`lib.rs:1325`
    `render`, `lib.rs:1491` `update_texture` lädt `self.preview` 1:1 in die
    egui-Textur). **Teil-Infrastruktur vorhanden:** `IdleQueue` existiert
    (`lib.rs:117`), wird im `update`-Loop nur bei `!pointer.any_down()`
    abgearbeitet (`lib.rs:3547`) – also ein Synchronisationspunkt für
    „defer to idle", aber **nicht** an das Rendering gekoppelt.
  - **Verfeinerung nötig:** Während des Drags eine herunterskalierte
    Draft-Vorschau (z. B. halbe/viertel Auflösung) rendern, volle Auflösung
    nach Mouse-Up bzw. 150 ms Idle (über bestehende `IdleQueue`/Repaint-Logik).
    Erste, CPU-seitige Quick-Win-Variante ist mittelfristig machbar, ohne GPU.
  - **Betroffene Module:** `lumina-gui` (Render-Steuerung, IdleQueue-Anbindung),
    `lumina-core` (optionale Downscale-/Pyramiden-Hilfen).
  - **Abnahmekriterien:** Drag zeigt flüssige Draft-Vorschau; nach Mouse-Up/
    Idle wird volle Auflösung gerendert und mit dem Sidecar-Ergebnis
    (F-043-Toleranz) abgeglichen; UI bleibt während des Drags bedienbar.

- [ ] **PERF-GUI-4** Eingabe-Events zum Render-Loop bündeln (onMouseMove von
      Render entkoppeln, display-synchroner Ticker, Frames droppen).
  - **Verifikationsstatus:** Nicht vorhanden / inkonsistent. `set_adjustment`
    (`lib.rs:1231`) mutiert das Rezept direkt aus dem egui-Slider-Callback und
    setzt `render_key = None`, löst aber **keinen** eigenen Render-Ticker aus;
    ein durchgehendes Render-Coalescing (Entkopplung Eingabe→Render,
    Frame-Drop bei Überlast) existiert nicht. egui ist zwar display-synchron,
    aber es gibt keinen expliziten, entkoppelten Render-Loop mit Eingabepuffer.
  - **Verfeinerung nötig:** Eingabe-Events in eine Queue/Dirty-Flag bündeln und
    einen einzigen, display-synchronen Render-Pass pro Frame fahren; bei
    Überlast Frames droppen statt synchron zu blockieren. Passt zur
    IdleQueue-/Repaint-Struktur der GUI.
  - **Betroffene Module:** `lumina-gui` (Eingabe-Entkopplung, Ticker).
  - **Abnahmekriterien:** Mehrere Regler-Events pro Frame erzeugen höchstens
    einen Render-Pass; die UI friert beim Draggen nicht ein; keine
    doppelten/konkurrierenden Renders.

- [ ] **PERF-GUI-5** ROI-Rendering (bei Zoom nur Viewport-Kachel/Scissor
      rendern).
  - **Verifikationsstatus:** Nicht vorhanden. Es gibt kein ROI/Scissor/Tiling:
    `render_frame` rendert das Gesamtbild, `draw_preview` (`lib.rs:1801`)
    skaliert die Textur objektgerecht (`object-contain`) in den Pane; beim
    Hineinzoomen wird die volle Textur weiter skaliert, nicht auf den
    sichtbaren Ausschnitt in nativer Auflösung gerendert.
  - **Verfeinerung nötig:** Bei Zoom/Pan nur den sichtbaren Viewport-Ausschnitt
    (Scissor/ROI-Crop) in nativer Auflösung rendern; CPU-seitig machbar über
    einen ROI-Crop vor `render_frame`, GPU-seitig später über Scissor-Region.
    Keine Architekturkollision, mittel-/langfristig.
  - **Betroffene Module:** `lumina-gui` (Viewport/Zoom-Logik, `draw_preview`),
    `lumina-core` (ROI-Crop-Hilfe).
  - **Abnahmekriterien:** Beim Hineinzoomen wird nur der sichtbare Bereich
    gerendert; Pixelidentität zum vollen Render im Ausschnitt (F-043);
    keine unnötige Ganzbild-Verarbeitung im Zoom.

### Zusätzliche Todos aus manuellem Test (verwandt mit PERF-GUI-3)

- [ ] **PERF-GUI-6** GUI lädt beim Öffnen eines Verzeichnisses automatisch das
      erste RAW/Bild in den Develop-Modus.
  - **Verifikationsstatus:** Offener Befund. `list_directory` (`lib.rs:386`)
    füllt `self.entries`, lädt aber **nicht** automatisch den ersten Eintrag;
    `open_file` lädt einen expliziten Pfad, wählt aber nichts automatisch aus.
    „Erstes Bild nicht auto-geladen" ist damit reproduzierbar.
  - **Verfeinerung nötig:** Nach `set_directory`/`list_directory` das erste
    unterstützte `entries`-Element (oder ein definiertes Standardelement)
    automatisch via `load_path`/`load_bytes` laden und in Develop zeigen;
    Verzeichniswechsel entsprechend.
  - **Betroffene Module:** `lumina-gui` (`list_directory`, `set_directory`,
    `open_file`).
  - **Abnahmekriterien:** Öffnen eines Bildverzeichnisses zeigt sofort eine
    geladene Vorschau (erstes Bild) ohne manuellen Klick; reproduzierbar via
    `cargo run -p lumina-gui`.

- [ ] **PERF-GUI-7** Asynchroner RAW-Decode blockiert die UI beim Dateiwechsel
      nicht.
  - **Verifikationsstatus:** Offener Befund. `load_bytes` (`lib.rs:1186`)
    ruft `lumina_raw::decode_bytes` **synchron im Main-Thread** auf
    (`lib.rs:1190`); `load_path` (`lib.rs:1553`) ruft `load_bytes` synchron.
    Die vorhandene `IdleQueue` (`lib.rs:117`) deckt nur Thumbnails/Masken-
    Inferenz ab, nicht den Decode.
  - **Verfeinerung nötig:** RAW-Decode auf einen Worker-Thread/Background-Job
    auslagern (oder yielden), sodass Dateiwechsel die UI nicht einfrieren;
    Ergebnis nach Decode in den Main-Thread zurückreichen und dann erst
    rendern. `lumina-raw::decode_bytes` bleibt die Vertrags-Schnittstelle.
  - **Betroffene Module:** `lumina-gui` (Decode-Offloading, IdleQueue-/Job-
    Anbindung), `lumina-raw` (Vertrag, ggf. async-Hilfe).
  - **Abnahmekriterien:** Dateiwechsel startet Decode im Hintergrund; die GUI
    bleibt während des Decodes bedienbar (Filmstreifen/Regler reagieren);
    nach Decode erscheint die Vorschau; keine Blockade des Main-Threads.

### Umsetzungsstand 2026-08-22 (Session: GPU-first + Interaktivität)

Teilweise umgesetzt (noch **nicht** abgehakt — Verifizierung durch unabhängigen
Subagenten steht aus, siehe unten):

- **PERF-GUI-1 (Teil):** Draft/Full-Split (`render_draft`/`render_full`/
  `render_from` mit ROI-Crop) in `lumina-gui/src/lib.rs` vorhanden;
  `draft_original` wird gecacht (Zero-Alloc beim Drag).
- **PERF-GUI-2 (Gerüst):** `crates/lumina-gpu` neu (wgpu/Metal,
  `GpuContext`, Uniforms 64 B `#[repr(C)]`, `bake_3d_lut` Identity-Stub,
  FP16-Helfer, `TiledCache` 512² LRU, `DraftPyramid`); echter WGSL-Tone-Stage
  implementiert und gegen CPU-Golden-Gates verifiziert
  (`maxAbsDiff ≤ 1`, `PSNR ≥ 45 dB`; `cargo test -p lumina-gpu` 5 passed).
  **Offen:** Masken-/WB-/SourceAction-Stufen auf GPU; Readback (`map_async`)
  noch im Pfad — „Never read back to CPU" noch nicht erfüllt.
- **PERF-GUI-3 (Implementiert):** Draft bei `pointer.any_down()` aus gecachtem
  `draft_original`; Vollrender debounced 150 ms nach Loslassen.
- **PERF-GUI-4 (Implementiert):** Coalescing via Dirty-Flag + `request_repaint`;
  Zwischenframes werden gedroppt.
- **PERF-GUI-5 (Implementiert, CPU):** ROI-Crop vor `render_frame`
  (`preview_roi` für Pointer-Mapping gespeichert); GPU-Scissor offen.
- **PERF-GUI-6 (Implementiert):** Auto-Load des ersten RAW nach
  `list_directory` (mit Async-Decode-Drain im Headless-Test gehärtet).
  **Tester meldete 2026-08-23 „wird nicht automatisch geladen"** → Re-Check im
  Panic-Fix-Batch.
- **PERF-GUI-7 (Implementiert):** `decode_bytes` via Worker-Thread + mpsc;
  UI pollt non-blocking (Status „Decoding…").
- **Neu PERF-GUI-8 (erledigt, verifiziert):** CPU↔GPU-Golden-Image-Harness
  (`crates/lumina-gpu/tests/golden.rs`), Toleranzen dokumentiert in
  `docs/gpu-bootstrap.md`; GPU-Benchmarks (`bench/gpu.rs`) messen beide Pfade:
  @2048 CPU ≈ 59 ms vs. GPU ≈ 5,4 ms (~11×), Uniform-Upload ≈ 5 µs
  (Budgets report-only in `perf/budgets.json`, F-074-N6 draft).
- **Neu GUI-Features:** Thumbnail eigener Thread-Pool (mpsc +
  `available_parallelism`, IdleQueue nur noch Maske), Zoomstufen
  Fit/100 %/200 %/Fit-Width (+/−/1/F, Scroll-Zoom am Cursor, Pan),
  linke Navigator-Leiste (RAW-only, aktives Bild markiert),
  CLI/MCP GPU-first-Wiring mit sichtbarem Backend-Log
  (`render backend: gpu (Apple M5 Pro …)`), `LUMINA_MCP_LOG` implementiert.

### Neu aus manuellem Test 2026-08-23

**Verifiziert erledigt (2026-08-23, `bbb0cba`, unabhängige Verifizierung):**
- **GUI-CRASH-1** Release-Panic `f32 clamp min>max` (Fix: order-independent
  clamp in `draw_preview` + div-by-zero guards in `to_normalized`/scroll-zoom;
  Regression `to_normalized_is_finite_for_zero_size_rect`).
- **GUI-FIT-1** „Fit“ kleiner war Folgesymptom des Panics; Fit ist uncapped
  object-contain und nutzt korrekt das zentrale Pane nach Navigator.
- **GUI-AUTOLOAD-1** Auto-Load robuster (RAW-only `is_raw_name`,
  `auto_load_attempted` + `decode_rx.is_none()` Gate).
- **GUI-RAWONLY-1** Filmstrip + Navigator RAW-only-Filter (`is_raw_name`).

Offen (nächste Batches):

- [ ] **GPU-STAGE-1** Masken-/WB-/SourceAction-Stufen auf GPU (derzeit nur
      Tone-Stage; CPU bleibt Referenz). Nach GUI-60FPS-1.
- [ ] **BENCH-BASELINE-1** Baseline-Capture 6 GPU-Benchmark-IDs
      `perf/baseline.json` → `gate:true` (aktuell report-only, F-074-N6 draft).

### Generativ: Entfernen & Bildfläche erweitern (Wunsch Tester 2026-08-23, nur Doku)

- [ ] **GEN-EXPAND-1** Optionaler generativer Modus „Entfernen + Erweitern“:
      Objekte entfernen (inpainting) **und** das Bild über die ursprüngliche
      Bildfläche hinaus erweitern (outpainting/canvas expansion > 100 %).
      **Nur dokumentiert, Implementierung noch nicht begonnen.**
  - **Info an Agenten, die daran arbeiten:** Nicht-destruktiv per
    Sidecar-Rezept (neue versionierte Stufe, z. B. `GenerativeEdit`, mit
    Modellname/-version/-hash, Prompt-/Maskenreferenz, Seed, Auflösung,
    Prüfsumme des Ergebnisses als binäres Sidecar-Artefakt analog
    AI-Masken — Identität + Veraltets-Erkennung wie bei Masken, Agents.md
    „AI-Masken“). Original bleibt unverändert; Ergebnis ist ableitbares
    Artefakt. Gültigkeit an Quelle + Modellkontext koppeln; kein stiller
    Fallback — fehlendes Modell/Artefakt sichtbar melden. Capability-
    Matrix beachten (lokales ONNX vs. Cloud-API getrennt dokumentieren;
    Lizenz der Modelle vor Integration prüfen). Interaktion mit
    Crop/Geometry klären (Expandiertes Canvas verschiebt
    Koordinatensystem → Rezept-Koordinaten müssen das referenzieren).
  - **Abhängigkeiten:** F-082/F-083 SAM-Adapter existiert; ONNX-Pfad
    (`lumina-onnx`) als Heimat für lokale Inpainting/Outpainting-Modelle;
    GUI-Flow (Prompt, Maske malen, Expand-Rahmen ziehen) nach
    GUI-STAGE-1/GUI-WGPU-PRESENT-1.
- [ ] **GUI-WGPU-PRESENT-1** Follow-up aus GUI-60FPS-1-Verifizierung:
      `egui_wgpu`-Migration oder Upload-Pfad finalisieren (derzeit Present
      unter glow CPU-seitig via `ColorImage`/`load_texture`; <16 ms gilt für
      Masken-Tile-Upload, nicht Preview-Present). `VramState` LRU/Pool
      (45 MP+) + `warn!` bei `GpuContext::new` Adapter-Fehler.
      Dokumentiert in `docs/gpu-bootstrap.md` (Dual-Backend glow vs wgpu).
- **GUI-SCROLL-200-1 — verifiziert erledigt (2026-08-23, Implementierung
  `ses_fd0cdd9c7`, unabhängige Verifizierung `ses_fd051d31` — BESTANDEN):**
  Grid/Navigator via `ScrollArea::show_rows`, Filmstrip horizontal via
  `show_viewport`+Spacer; O(n)-Thumbnail-Loop entfernt →
  `ensure_thumbnail_priority` (sichtbares Fenster + 8-Zellen-Buffer
  ungecappt, Off-Screen-Prefetch 4/Frame nearest-first, neue Datei
  `crates/lumina-gui/src/viewport.rs`); Scroll-Zoom armiert nur den
  Debounce (Test `scroll_wheel_zoom_arms_debounce_without_synchronous_render`);
  `LUMINA_PERF_LOG=1` jetzt mit `thumb_jobs_enqueued/thumbs_ready/
  slow_frame`. Verifiziert: fmt clean, clippy gui + workspace
  `-D warnings` 0, `cargo test -p lumina-gui` **81 passed** (+14).
  Hinweise: F-3 Buffer zellenbasiert statt zeilenbasiert (kosmetisch,
  ggf. später heben); Live-Messung mit 200 RAWs manuell beim nächsten
  Test (`slow_frame=true` darf bei Scroll-Frames nicht auftreten).

### GUI-60FPS-1 (2026-08-23)

**Verifiziert erledigt (Implementierung `ses_fd202100`/Fix `ses_fd1f88470`,
unabhängige Verifizierung `ses_fd1ee2fe` — BESTANDEN):**
- Hot-Path VRAM-resident ohne `map_async` (`render_to_vram`, Export-Readback
  bewusst erhalten); Masken-Brush als persistente R16-Plane
  (`stamp_brush_mark`) mit Dirty-512²-Kachel-Upload
  (`dirty_tiles_for_brush_mark`, `bytemuck::cast_slice`), Overlay immer
  gezeichnet; `LUMINA_PERF_LOG=1` Frame-/Render-Messung; Dual-Backend-Doku
  (glow present vs wgpu offscreen) in `docs/gpu-bootstrap.md` +
  `feature/platform/cli-gui-wasm.md`; neue Datei
  `crates/lumina-core/src/mask_tiles.rs`.
- Verifiziert: fmt clean, clippy workspace `-D warnings` 0, gui 54, gpu 5,
  sidecar zdata 94, core wgpu-frei, `cargo check -p lumina-gpu
  --no-default-features` grün.

### Lightroom-like UI (2026-08-23 beschlossen)

**Verifiziert erledigt (2026-08-23, Subagent `ses_fd28c399…`, unabhängige
Verifizierung):**
- **GUI-LR-RIGHT-1** Rechtes Panel: Crop-Thumbnail (120 px Head,
  `Geometry.crop` Free-Overlay, `crop_overlay_rect`), Presets-Sektion
  (`document.presets`, Hover/Apply, Create konsolidiert; `Str::PresetsSection`),
  History-Sektion (reverse-chronologisch, `restore_history()` nicht-destruktiv).
- **GUI-LR-LIBRARY-1** Library: linker Ordnerbaum (`SidePanel::left "folders"`,
  Root `$HOME`/grandparent, `BTreeSet`/`BTreeMap` lazy `read_dir`,
  RAW-Counts `FOLDER_SCAN_DEPTH=3`, `set_directory`), zentriertes
  Thumbnail-Grid (`ensure_thumbnail`, Doppelklick → `open_file` + Develop).
- Verifiziert: `cargo build -p lumina-gui` grün,
  `cargo clippy -p lumina-gui` clean,
  `cargo test -p lumina-gui` **54 passed** (+4: `library_root_prefers_home…`,
  `crop_overlay_rect…`, `history_restore…`, `to_normalized…`),
  `cargo fmt -- --check` clean; `Str` `Folders/History/NoHistory/NoPresets/
  HistoryEntryMissing`.

Offen: kittest Snapshots für Library-Layout brauchen GPU-Regenerierung
(`--ignored`); Crop-Overlay ignoriert Rotation/Mirroring (display-only).

### CI-Green 2026-08-23

**Verifiziert erledigt:** `lensfun` Feature-Unification (E0063): `RenderContext.lensfun`
via `LensfunCorrectorRef<'a>` immer vorhanden (cfg-aware newtype
`#[derive(Debug,Clone,Copy)]` in `render.rs`, `lensfun: None,`-Gates entfernt,
`Some`/`as_ref` → `LensfunCorrectorRef` + `None`-Fallback, `LensfunCorrectorRef`
re-export). Betrifft `lumina-core`, `lumina-cli`, `lumina-mcp`, `lumina-bench`,
`lumina-gpu/tests/golden.rs`; `lumina-gui` bleibt `default=["lensfun","gpu"]`.
Verifiziert: `cargo build -p lumina-core`/`--features lensfun`,
`cargo clippy --workspace --all-targets --features "lumina-sidecar/zdata,
lumina-bench/raw-bench" -- -D warnings` grün,
`cargo test -p lumina-core` 207/210 passed (Tone-Message `0e0..=1e0` korrigiert).

## Abnahmekriterien

Die erste produktiv nutzbare Version muss mindestens Folgendes erfüllen:

- Ein RAW kann ohne zentrale Datenbank importiert, bearbeitet und exportiert
  werden.
- Nach dem Neustart werden Bearbeitungsrezept und virtuelle Kopien ausschließlich
  aus dem Sidecar wiederhergestellt.
- Zwei virtuelle Kopien desselben Originals können unterschiedliche Rezepte,
  Masken-Layer und Exporte besitzen.
- Eine gültige persistierte AI-Maske wird wiederverwendet und nicht ungefragt
  neu berechnet.
- Änderungen an Quelle, Modell, Decode-Kontext oder Maskenartefakt werden als
  veraltet erkannt.
- Vorschauen und Exporte sind über einen reproduzierbaren Render-Key cachebar.
- Das Löschen eines optionalen zentralen Indexes zerstört keine Bearbeitung.
- Originaldateien bleiben byteweise unverändert.
- Sidecar-, Migration-, Cache-, Masken- und virtuelle-Kopien-Tests sind durch
  einen unabhängigen Verifizierungs-Agenten bestätigt.

## Review-Befunde Full-Repo-Review (2026-08-23, 8 parallele Subagenten)

Erstes vollständiges Review des gesamten bestehenden Codes (alle 10 Crates,
~35k Zeilen). Konsolidiert und dedupliziert; Verifizierungsbedarf je Befund im
Eintrag genannt. Alle acht Teil-Reviews sind abgeschlossen.

### Hoch (Release-blockierend, vor MVP zu beheben)

- [ ] **REVIEW-CORE-GEO-1** Geometrie-Stufe ist im Default-Build tot
  (hoch; Subagent-Befund, vom Orchestrator verifiziert): `render_frame`
  wendet Crop/Rotation/Mirror/Perspektive ausschließlich unter
  `#[cfg(feature = "lensfun")] if let Some(corrector)` an
  (`crates/lumina-core/src/render.rs:133–143`); `apply_recipe*`
  (lib.rs:579 ff.) enthält keine Geometrie, `export_image` ruft nur
  `render_frame`. Im Default-Build (Feature off) und bei
  `lensfun: None` (GUI übergibt immer `None`) wird `recipe.geometry`,
  `recipe.perspective`, `recipe.lens_correction` stillschweigend
  ignoriert — Verstoß gegen „keine stillen Fallbacks". Fix:
  `apply_geometry` bedingungslos aufrufen (unterstützt bereits den
  None-Corrector-Pfad) oder laut fehlschlagen; Golden-Tests um
  Crop/Rotation erweitern.
- [ ] **REVIEW-CORE-CURVE-1** Panik bei fallenden Kurvensegmenten
  (hoch): `monotone_curve` (lumina-core/src/lib.rs:1706) clampt auf
  `[0, 3d]` — bei absteigendem Segment (`d < 0`) panickt
  `f32::clamp(min>max)` pro Pixel. Schema-valide Sidecar-Kurve mit
  „Dip" = Renderer-Crash beim Öffnen. Fix: Clamp über sortierte
  Grenzen oder nicht-monotone Ausgaben laut ablehnen.
- [ ] **REVIEW-CORE-PERSP-1** Perspektive am Slider-Anschlag erzeugt
  Riesen-Canvas/OOM (hoch): Homographie-Nenner `d ≈ 0` bei
  `horizontal=1.0` → projizierte bbox ~10⁶× Bildgröße, `vec![0; ow*oh*4]`
  ohne MemoryBudget (lib.rs:946–965, 1114–1137). Fix: bbox begrenzen,
  `d≈0` laut ablehnen, Allokation über MemoryBudget.
- [ ] **REVIEW-SIDECAR-LOCK-1** TOCTOU beim Reclaim verwaister
  Write-Locks (hoch): Zwei Prozesse können gleichzeitig „stale"
  entscheiden; der zweite löscht den frischen Lock des ersten
  (`crates/lumina-sidecar/src/lib.rs:1146–1158`) → Lost Update ohne
  Conflict. Fix: atomares Reclaim via rename/flock.

**Verifiziert erledigt (2026-08-23, unabhängige Verifizierung
`ses_fd1c884f` + Re-Verifizierung `ses_fd0d2d25` — BESTANDEN):**
- **REVIEW-GPU-DIVERGENCE-1** `unsupported_gpu_stages(recipe)`-Validator
  (inkl. Vibrance/Saturation, Curves≠Identität, HSL, Presence,
  ColorGrading, NR/Sharpening/Effects, Geometry/Lens/Perspective,
  SourceActions; Neutralformen lösen keine Route aus); `render_with_gpu`
  routet bei Befund explizit CPU (`info!` einmal pro Reason-Set), CLI
  `render_best_effort` zusätzlich bei SourceActions/Masken/Lensfun;
  `render_to_vram` warnt sichtbar (VRAM nicht CPU-routbar). Golden-Korb
  +6 Rezepte, +2 Tests mit maxAbsDiff=0-Assertions. Verifiziert: gpu 7
  passed, cli 15+8, clippy workspace `-D warnings` 0.
  Restrisiko dokumentiert: VRAM-Vorschau rendert Unsupported-Rezepte
  tone-only (mit Warnung) bis GPU-STAGE-1.
- **REVIEW-GUI-ZOOMLOOP-1** `preview_base_fit_scale` aus ungeschnittenen
  Originalmaßen (`preview_src_w/h`); absolute Zoommodi ohne Textur-
  Rückkopplung (`sync_zoom_derives_absolute_modes_from_uncropped_source_fit`).
- **REVIEW-GUI-PANROI-1** `roi_from_zoom(w,h,zoom,pan,…)` mit
  `PREVIEW_ROI_MARGIN=1.3` + Border-Clamping; Pan-Drag setzt
  `mark_dirty()` → Draft im Hot-Path, debounced Full-Render mit finalem
  Pan; Test `pan_drag_schedules_draft_and_final_roi_render`.
- **REVIEW-GUI-EXPORT-1** `resolve_export_target`: erst `with_extension`,
  dann Schutz vor Quelle/`.lumina.json`/`.lumina.zdata`
  (`paths_resolve_equal_symmetric`); sichtbarer Fehler, Original byte-
  identisch.
  Verifiziert: fmt clean, clippy gui `-D warnings` 0,
  `cargo test -p lumina-gui` **67 passed**.

**Verifiziert erledigt (2026-08-23, Implementierung `ses_fd1f42935`,
unabhängige Verifizierung `ses_fd1e0f491` — BESTANDEN):**
- **REVIEW-GUI-THUMB-1** `thumbnail_key()` = canonicalisierter Pfad,
  `ensure_directory()` leert nur bei Verzeichniswechsel, Grid/Filmstrip/
  Navigator/Drain umgestellt.
- **REVIEW-GUI-THUMB-2** `mark_probed` entfernt; probed nur bei Erfolg,
  `Failed(message)` sichtbar (⚠) + Retry-Budget 3; Disk-Cache-Korrupt =
  sichtbarer Fehler.
- **REVIEW-GUI-PATHDESYNC-1** `self.path` wird erst in `finish_decode`
  Ok-Zweig committet (open_file/ChooseFile/Drop nicht mehr); Err behält
  konsistentes Bild A → kein Phantom-Sidecar, Fehler sichtbar.
  Verifiziert: fmt clean, clippy `-p lumina-gui -D warnings` 0,
  `cargo test -p lumina-gui` **62 passed** (+8 neue Tests).
- [ ] **REVIEW-GUI-WASM-1** wasm32-Build von lumina-gui bricht
  (hoch): `to_normalized`/`image_dims` sind cfg(not(wasm32)), werden
  aber in `draw_preview` ungegate aufgerufen; `draw_preview_area`
  ebenfalls (lib.rs:2594, 2547–2627, 5029). Verifiziert:
  `cargo check --target wasm32-unknown-unknown --no-default-features -p
  lumina-gui` schlägt fehl (4× E0599) — CI prüft das offenbar nicht.
  Fix: Call-Sites gaten oder wasm-Stubs ergänzen; wasm32-Check für gui
  ohne Default-Features in CI aufnehmen.
  (Update Nacharbeit PANROI: `export_to`-Gate-Fix behebt eine der 4
  Lücken; aktuell noch 4→3? — beim Fix zählen und hier korrigieren.)
- [ ] **REVIEW-RAW-ABI-1** Vendored `libraw-sys`-Structlayouts passen
  nicht zur gelinkten LibRaw 0.22.2 (hoch; empirisch per Offset-Probes
  verifiziert: `params` 1512 vs. real 5232, `color` 1888 vs. 5592,
  `other` 150088 vs. 192680): `camera_matrix`/`camera_white_balance`/
  EXIF-Felder lesen Garbage und werden ins Sidecar persistiert;
  `color.profile` wird via `from_raw_parts` von potentiellem Wild-
  Pointer kopiert (UB/Crash-Risiko); Schreibzugriffe auf
  `params.user_flip`/`use_camera_wb` landen im Makernotes-Bereich —
  user_flip/camera-WB werden still NICHT angewendet (Probe bestätigt:
  `sizes.flip` bleibt trotz `user_flip=0`) und überschreiben hunderte
  Bytes Live-State. Betrifft jeden nativen Decode
  (lumina-raw/src/lib.rs:284–327). Fix: Bindings gegen 0.22.x
  regenerieren oder nur Accessor-Funktionen nutzen;
  statische Size/Offset-Asserts ins build.rs; `tests/sizes.rs` des
  sys-Crates in CI ausführen (existiert, läuft aber nie).
- [ ] **REVIEW-LENSFUN-VIGN-1** Corrector kollabiert Bild bei
  Vignetting-only-Profilen (hoch): `lf_modifier_apply_geometry_
  distortion` gibt bei fehlender Distortion-Kalibrierung `false`
  zurück **ohne `res` zu schreiben**; `geometry()` ignoriert den
  Returnwert → `(0.0, 0.0)` für jedes Pixel, `is_identity()` erkennt
  es nicht (lumina-lensfun/src/lib.rs:386–436, 461–493). Trigger:
  Kamera+Objektiv mit Vignetting-only-Eintrag (häufig) → korrigiertes
  Bild einfarbig. Fix: Returnwert prüfen, bei `false` Koordinaten
  unverändert durchreichen; Corrector nur mit LF_MODIFY_DISTORTION
  geometrisch verwenden.

### Mittel

- [ ] **REVIEW-SIDECAR-TMP-1** `recover_sidecar` löscht Temp-Dateien
  lebender Writer (mittel; lib.rs:1109–1130). Fix: mtime-Schwelle oder
  Sweep unter Lock.
- [ ] **REVIEW-SIDECAR-CAS-1** CAS (`save_sidecar_if_unchanged`) nicht
  gegen Plain-`save_sidecar` serialisiert (mittel; lib.rs:1075–1100).
  Fix: alle Writes über Lock oder Vertrag dokumentieren.
- [ ] **REVIEW-SIDECAR-ZDATA-1** zdata Read-Modify-Write ohne Lock →
  verlorene Repair-Regionen/Dangling Refs (mittel;
  zdata.rs:688–699). Fix: `.zdata.lock` + Checksum-Verifikation beim
  Laden.
- [ ] **REVIEW-SIDECAR-STATUS-1** `artifact_status` prüft nur
  `is_file()`, keine Checksum/Format/Auflösung (mittel;
  lib.rs:1187–1193) → korrupte Artefakte gelten als Available.
- [ ] **REVIEW-CORE-CROP-1** `crop_rect`: u32-Underflow/Empty-Crop durch
  1e-6-Toleranz (mittel; lib.rs:1227–1246). Fix: x/y ≤ 1 explizit,
  saturating_sub, pw/ph == 0 als Fehler.
- [ ] **REVIEW-CORE-GEOMORDER-1** `apply_geometry` mutiert Frame vor
  Validierung → Half-Transformed-State + Doppel-Anwendung bei Retry
  (mittel; lib.rs:363–413). Fix: Validierung an den Anfang.
- [ ] **REVIEW-CORE-EXPORTKEY-1** ExportOptions (quality/dither/seed/
  bit_depth) fehlen in OutputSpec/RenderKey-Identität (mittel;
  pipeline.rs:88–107) → Cache-Hits liefern falsche Qualität. Fix:
  volle ExportOptions in den Digest.
- [ ] **REVIEW-CORE-SRCACC-1** `source_actions` in keinem Cache-Digest
  (mittel; cache.rs:162–210) → geänderte Repair-Artefakte servieren
  alte Pixels. Fix: Artifact-Checksummen in RenderKey.
- [ ] **REVIEW-CORE-DECODE-1** `ImageFrame::decode` unbegrenzte
  Allokation, kein MemoryBudget (mittel; lib.rs:255–260). Fix:
  Dimensionen vorab prüfen + `check_decode`.
- [ ] **REVIEW-MASK-STRICT-1** MaskPolicy::Strict wird nirgends
  verwendet (CLI setzt immer Warn) (mittel). Fix: Strict-Pfad ehrlich
  verdrahten oder Policy entfernen.
- [ ] **REVIEW-MASK-ZERO-1** `rasterize_prompt` panickt bei Breite 0
  (`chunks_exact_mut(0)`) statt `MaskError` (mittel; masks.rs:269 ff.);
  Sidecar-Validierung prüft width/height ≠ 0 nicht. Fix: Guard +
  Validierung.
- [ ] **REVIEW-MASK-BLUR-1** Feathering/Blur O(w·h·radius) — bei
  feather ≈ 1.0 Minuten pro Render/Export (mittel;
  mask_modulation.rs:61–96). Fix: Sliding-Window-Box-Blur O(w·h),
  byte-identisch.
- [ ] **REVIEW-GPU-TILEVER-1** `TiledCache` ohne Edit-Generation →
  zwingend stale Tiles, sobald gradierte Tiles gecacht werden (mittel;
  tiling.rs:34–99). Fix: Generation Counter in TileKey oder Contract
  dokumentieren.
- [ ] **REVIEW-GPU-LEVELS-1** Level-Berechnung inkonsistent:
  `keys_for_viewport` ohne max_level-Clamp vs. `level_for_zoom`
  (mittel; tiling.rs:108–133 vs 169–172).
- [ ] **REVIEW-CLI-MASKFLAG-1** `update_masks`/`force_render` bleiben
  ewig im Rezept → permanente Re-Inferenz trotz gültiger Maske
  (mittel; lumina-cli/src/main.rs:443, 901, 1219, 1348; Verstoß gegen
  Persistenz-Invariante). Fix: Option nach Konsum aus dem persistierten
  Rezept entfernen.
- [ ] **REVIEW-CLI-EXPORTMASK-1** `export --update-masks` bricht bei
  stale Masks ab, batch/develop laufen weiter; Flag wird für Export gar
  nicht durchgereicht (mittel; main.rs:492–526 vs 555, 1200, 1218).
- [ ] **REVIEW-CLI-WRITE-1** Overwrite-Guards decken weder Sidecar-/
  zdata-Ziele noch Hardlinks (Inode-Identität) ab (mittel;
  main.rs:1067, 1544; lumina-mcp/src/tools/save.rs:33–44). Fix:
  Zielpfade gegen `<input>.lumina.json/.zdata` prüfen + (dev,inode)-Vergleich.
- [ ] **REVIEW-CLI-BATCHCOLLIDE-1** Batch schreibt alle Inputs
  namensbasiert in ein Zielverzeichnis → Kollisionen überschreiben
  still, beide melden „ok" (mittel; main.rs:874–881). Fix: Dubletten
  vorab ablehnen oder Struktur spiegeln.
- [ ] **REVIEW-MCP-QUALITY-1** `quality as u8` trunciert ohne
  Validierung (256→0) (mittel; save.rs:47–52; analog preview.rs:31).
  Fix: 1..=100 serverseitig erzwingen.
- [ ] **REVIEW-MCP-SAVE-1** `lumina_save` nutzt `fs::write` statt
  atomarem Write; Format/Extension ungeprüft (mittel; save.rs:66–68).
- [ ] **REVIEW-MCP-SESSION-1** Ganzes, evtl. veraltetes In-Memory-
  Document wird zurückgeschrieben → Lost Update; Load prüft
  content_hash nicht (mittel; session.rs:36–59, edit.rs, load.rs:37).
  Fix: `save_sidecar_if_unchanged` + Quell-Identitätsprüfung wie CLI.
- [ ] **REVIEW-GUI-MASKGEO-1** Masken-Pinsel/Verlauf/Radial (und
  WB-Pipette) ignorieren Crop/Rotation/Mirror des Rezepts → Markierungen
  landen transformiert-falsch (mittel; lib.rs:2589–2669). Fix: inverse
  Geometrie in `to_normalized` einrechnen oder Werkzeuge bei aktiver
  Geometrie deaktivieren.
- [ ] **REVIEW-GUI-SAVEMSG-1** Nach fehlgeschlagenem `save_sidecar`
  steht trotzdem „Sidecar saved" im Status (mittel; lib.rs:2222–2233).
- [ ] **REVIEW-GUI-VCSWITCH-1** Virtual-Copy-Wechsel verwirft ungespeicherte
  Edits ohne Rückfrage; `history_selected`/Drag-State werden nicht
  zurückgesetzt (überlappt REVIEW-5B3F930-5), Fehler wird verschluckt
  (mittel; lib.rs:909–925, 4240).
- [ ] **REVIEW-GUI-CURVE-1** Tone-Curve-Roundtrip clamppt Shadows-
  Slider auf 0 → Regler springt sichtbar zurück (−50 % → −33 %→0)
  (mittel; lib.rs:2981–3003). Fix: Deltas speichern statt geclampte
  Outputs, oder UI-Hinweis.
- [ ] **REVIEW-GUI-DEBOUNCE-1** Debounced Vollrender kann stranden:
  im Wartefenster (< 150 ms) wird weder gerendert noch ein getaktetes
  Repaint angefordert → Draft-Vorschau bleibt bis zur nächsten Eingabe
  (mittel; lib.rs:4869–4889). Fix: `request_repaint_after` im
  Warte-Zweig.
- [ ] **REVIEW-GUI-MASKRENDER-1** Masken-Layer-Edits (Invert/Feather/
  Blur/Density) setzen nur `render_key = None`, planen aber kein
  Render → Vorschau bleibt dauerhaft alt (mittel; lib.rs:1077–1092,
  1193–1211). Fix: über `mark_dirty()` routen.
- [ ] **REVIEW-GUI-DRAFTROI-1** Draft-Render speichert ROI in
  Draft-Pixel-Space, Pointer-Mapping teilt durch Volllösung →
  WB-Pipette/Masken-Klicks falsch innerhalb des Debounce-Fensters
  (mittel; lib.rs:1704–1735, 2595–2612). Fix: ROI mit Bezugssystem
  speichern bzw. in Volllösungskoordinaten reskalieren.
- [ ] **REVIEW-RAW-FLIP-1** `sizes.flip` (dcraw-Bitmaske) wird 1:1 als
  EXIF-Orientation persistiert — falsche Codewelt (z. B. flip=5 ist
  EXIF 8, nicht 5); Portrait-Fixture persistiert nachweislich falsch
  (mittel; lumina-raw/src/lib.rs:280–283). Fix: explizit übersetzen
  oder Rohwert unter eigenem Namen führen.
- [ ] **REVIEW-ONNX-AVAIL-1** `<StubBackend as SubjectInference>::infer`
  ignoriert `self.available` — „fehlendes" Modell liefert still Matte
  (mittel; lumina-onnx/src/backend.rs:114–143 vs. SAM-Gate sam2.rs:277).
- [ ] **REVIEW-ONNX-HASH-1** Modell-Hash wird nie gegen Manifest
  geprüft (`path.exists()` genügt; Platzhalter „pending-integration")
  → getauschte Gewichte laufen unter alter Identität in Masken-Sidecars
  (mittel; ort_backend.rs:34–53). Fix: Hash beim Laden berechnen und
  auf Mismatch stale-markieren.
- [ ] **REVIEW-ONNX-PREPROC-1** ORT-Preprocessing nur [0,1] statt
  ImageNet mean/std, Tensor-Namen hartcodiert statt aus Manifest,
  Output-Shape ungeprüft (Fehler meldet dann 0/0-Dims) (mittel;
  ort_backend.rs:66–95). Fix vor Integration echter Gewichte.

### Niedrig (Backlog, nicht MVP-blockierend)

- [ ] **REVIEW-SIDECAR-N1** Migration-Tempfile nutzt Crate-Default-Prefix
  statt `.{name}.tmp-` → Recover-Sweep räumt nie auf (lib.rs:1241).
- [ ] **REVIEW-SIDECAR-N2** schema_version 0 wird in `from_json` still
  zu 1 normalisiert, divergiert vom Migrationspfad (lib.rs:1311).
- [ ] **REVIEW-SIDECAR-N3** Unbekannte Adjustment-Keys und
  MaskLayer.feather/blur/density sowie target_luminance ohne
  Finite/Range-Validierung (lib.rs:1724, 320, 827).
- [ ] **REVIEW-SIDECAR-N4** `delete_virtual_copy` mutiert vor
  `validate()` → bei Fehler bleibt Rechenliste inkonsistent hängen
  (lib.rs:1373).
- [ ] **REVIEW-SIDECAR-N5** `load_sidecar` liest Datei komplett vor
  Größenlimit (read_to_string) — `load_zdata` macht es richtig
  (lib.rs:979).
- [ ] **REVIEW-CORE-N1** Histogram-Stufen-Digest ohne OutputSpec
  (pipeline.rs:160; latent, bis Vorschau-Histogramme gecacht werden).
- [ ] **REVIEW-CORE-N2** `cdf_at(NaN)` gibt NaN zurück statt Fehler
  (histogram.rs:155).
- [ ] **REVIEW-CORE-N3** `AutoToneConfig.epsilon` bis 1.0 zulässig →
  +10 EV auf fast jedem Bild; Zweige überlappen > 0.5 (tone.rs:36).
- [ ] **REVIEW-CORE-N4** Messbereich-Domäne weicht bis 1 px vom
  tatsächlichen Crop ab (ungerundet vs. gerundet; lib.rs:456 vs 1242).
- [ ] **REVIEW-MASK-N1** MaskGraph ohne Memoization → handcrafted DAGs
  exponentiell (masks.rs:95–203).
- [ ] **REVIEW-MASK-N2** Density < 0 löscht Maske still, > 1 wirkungslos
  (mask_modulation.rs:40).
- [ ] **REVIEW-MASK-N3** `model_identity_matches(None) => true` segnet
  fremdmodellierte Artefakte als valide ab (mask_loader.rs:330).
- [ ] **REVIEW-CLI-N1** CLI lädt zdata-Tiles nur per `mask.id` statt
  `(copy_id, mask_id)` → Kopien mit gleichen Masken-IDs teilen Matte
  (main.rs:1186).
- [ ] **REVIEW-CLI-N2** dust_removal hängt Artefakt an, bevor Sidecar/
  Copy validiert sind → orphaned Bundles bei Fehlern (main.rs:674).
- [ ] **REVIEW-CLI-N3** Batch-Resume per Substring-Match auf
  Statusdatei (main.rs:885). Fix: JSON parsen.
- [ ] **REVIEW-CLI-N4** reindex ignoriert korrupte Sidecars still, Exit 0
  (main.rs:811).
- [ ] **REVIEW-CLI-N5** collect_images folgt Symlink-Loops ohne Schutz
  → Stack Overflow (main.rs:947).
- [ ] **REVIEW-CLI-N6** Export geschrieben bevor Sidecar-Update; Fehler
  → Exit 1 trotz existierendem Export (main.rs:1346).
- [ ] **REVIEW-CLI-N7** import akzeptiert geänderte Quelle gegen
  bestehendes Sidecar ohne Warnung (main.rs:400).
- [ ] **REVIEW-MCP-N1** JSON-RPC-Codes: Parse-vs-Invalid-Request
  konflatet; Tool-Fehler nicht als isError-Result (lib.rs:139).
- [ ] **REVIEW-GPU-N1** golden.rs WARN-Text behauptet CPU-Fallback,
  stimmt nicht mehr; GpuContext::init schluckt Fehler ohne Log;
  Backends::METAL hardkodiert widerspricht Doc (golden.rs:268,
  gpu/lib.rs:126, 718).
- [ ] **REVIEW-GUI-N1** Save berechnet Fingerprint neu und löscht
  Konfliktstatus still; GUI nutzt CAS (`save_sidecar_if_unchanged`)
  nicht (lib.rs:2208–2224).
- [ ] **REVIEW-GUI-N2** `finish_decode` stellt Rezept aus
  `virtual_copies[0]` (positionell) wieder her, während
  `virtual_copy_id` auf `"vc-original"` fixiert wird — verstoßt gegen
  die ID-stabil-Regel bei umsortierten Sidecars (lib.rs:2128/2145,
  1553). Fix: Copy per id/is_default suchen.
- [ ] **REVIEW-GUI-N3** Dateiwechsel resettet Zoom/Pan/BeforeAfter/
  WB-Pipette/History-Auswahl nicht → Bild B öffnet im 8×-Crop von
  Bild A (lib.rs:1536–1567; überschneidet sich mit REVIEW-GUI-PANROI-1).
- [ ] **REVIEW-GUI-N4** `IdleQueue::pop_next` ist LIFO statt
  dokumentiertem FIFO bei gleichen Prioritäten (`max_by_key` wählt
  letztes Maximum) (lib.rs:184–194).
- [ ] **REVIEW-GUI-N5** `preview_is_draft` ist write-only: Histogramm/
  Exposure-Matching messen still Drafts; Flag konsumieren oder Feld
  entfernen (lib.rs:350, 1559, 1691, 1729).
- [ ] **REVIEW-GUI-N6** Fehlgeschlagener ROI-Crop fällt still auf
  Vollbild zurück, `preview_roi` wird aber trotzdem gesetzt (latent;
  lib.rs:1815–1822).
- [ ] **REVIEW-RAW-N1** Returncode von `libraw_adjust_sizes_info_only`
  geschluckt — Budget-Gate könnte auf veralteten Maßen basieren
  (lumina-raw/src/lib.rs:335).
- [ ] **REVIEW-RAW-N2** `metadata.lens` ist immer `None`, obwohl Feld
  existiert und Lensfun-Integration ihn braucht (lib.rs:307). Befüllen
  oder Feld entfernen.
- [ ] **REVIEW-ONNX-N1** SAM-Prompt-Typosystem kann dokumentierte
  Labels −1/2/3 nicht ausdrücken; Koordinatenraum-Verantwortung
  (Source- vs. 1024²-Modell-Space) implizit — vor ORT-Decoder klären
  (sam2.rs:20–56).
- [ ] **REVIEW-ONNX-N2** `ModelManifest::validate()` prüft nur
  Capability-Invariante; leere Hash-/Lizenzstrings und Null-
  Auflösungen passieren (manifest.rs:141).
- [ ] **REVIEW-LENSFUN-N1** `lf_camera_crop_factor` hartkodiert Struct-
  Offset `4*ptr_size` (aktuell korrekt gegen 0.3.4, aber ABI-Wette)
  (lumina-lensfun/src/lib.rs:216–228). Fix: Shim-Funktion oder
  Build-time-Offset-Asserts.

### Arbeitsbaumänderungen während des Reviews (nicht verworfen, ungeprüft)

Während des Reviews entstanden (laut Anweisung bewusst **behalten**, aber
noch nicht reviewt/verifiziert — vor Übernahme prüfen):
- `crates/lumina-core/src/mask_tiles.rs` (neu, 382 Zeilen) + Modul-Export
  in lib.rs
- `crates/lumina-gpu/src/tiling.rs`: `TILE_SIZE` verkettet auf
  `mask_tiles::MASK_TILE_SIZE`
- `crates/lumina-raw/tests/probe_flip.rs` (neu): diagnostischer Test aus
  REVIEW-RAW-ABI-1/REVIEW-RAW-FLIP-1 (Offset-/flip-Probes) — belegt die
  ABI-Befunde; vor Merge entweder als Regressionstest übernehmen oder
  entfernen (Entscheidung offen).

## Festgelegte Produktentscheidungen

Die fachlichen Entscheidungen sind in `feature/README.md` und den verlinkten
SOLL-Dokumenten festgeschrieben. Neue offene Punkte werden als konkrete
Implementierungsaufgaben mit Feature-ID ergänzt, nicht als unpriorisierte
Entscheidungsliste gesammelt.