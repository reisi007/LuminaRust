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

### Neu aus manuellem Test 2026-08-23 (offen)

- [ ] **GUI-CRASH-1** Release-Panic im Zoom/Pan-Pfad nach dem Debounce-
      Vollrender: `f32 clamp "min > max": min = 444.9877, max = 444.98105`
      (`gui.log` Zeile ~66178, Panic-Hook + Ringbuffer funktionieren). Ursache:
      Rect-/Pos-Berechnung aus veraltetem Zoom-State wird invertiert
      (Rundungsdifferenz). Fix: clamp-Reihenfolgen absichern
      (`Rect::from_min_size` + `.max(0.0)`-Breiten, Epsilon-Guards),
      Division durch 0 in `to_normalized` bei 0-Rect vermeiden.
      In Arbeit (Fix-Batch läuft).
- [ ] **GUI-FIT-1** „Fit" zeigt das Bild weiterhin kleiner als das Panel
      (object-contain nutzt evtl. falsche verfügbare Fläche — Navigator/rechte
      Leiste müssen VOR der Preview allokiert sein; kein `min(1.0)`-Cap im
      Fit-Modus; Textur 5774×3849 muss voll ins zentrale Pane gemappt werden).
      In Arbeit (Fix-Batch läuft).
- [ ] **GUI-AUTOLOAD-1** Auto-Load des ersten RAW greift laut Tester nicht
      (Log zeigt zwar `loaded image … cr3`, aber Nutzer-Erlebnis: kein Bild).
      Verifizieren, ob Decode-Ergebnis im Update-Loop konsumiert wird
      (`poll_decode`), bevor „kein Bild“ angezeigt wird. In Arbeit.
- [ ] **GUI-RAWONLY-1** Nicht-RAWs erscheinen trotz Filter in Navigator/
      Filmstrip → RAW-only-Filter direkt in beiden Draw-Loops erzwingen
      (`is_raw_name(&e.name)`, unabhängig von `is_supported_image`, das für
      Tests breiter ist). In Arbeit.
- [ ] **GUI-60FPS-1** Flüssige 60 FPS bei Slider/Maske fehlen noch:
      Readback (`map_async`) aus dem GPU-Pfad entfernen und direkt in die
      egui-Textur/Swapchain rendern; Masken-Brush als R8/R16-Maskentextur im
      VRAM + Overlay-Shader (nur geänderte 512²-Kachel neu zeichnen).
      Nächster Batch nach Crash-Fix. Ziel: Slider-Drag < 16 ms Frame-Zeit.
- [ ] **GPU-STAGE-1** Masken-/WB-/SourceAction-Stufen auf der GPU ergänzen
      (derzeit rendert der GPU-Pfad diese Stufen noch nicht; CPU-Pfad bleibt
      Referenz). Nach GUI-60FPS-1.
- [ ] **BENCH-BASELINE-1** Nach Stabilisierung: Baseline-Capture für die 6
      GPU-Benchmark-IDs in `perf/baseline.json` und Budgets auf `gate:true`
      stellen (aktuell report-only, F-074-N6 draft).

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

## Festgelegte Produktentscheidungen

Die fachlichen Entscheidungen sind in `feature/README.md` und den verlinkten
SOLL-Dokumenten festgeschrieben. Neue offene Punkte werden als konkrete
Implementierungsaufgaben mit Feature-ID ergänzt, nicht als unpriorisierte
Entscheidungsliste gesammelt.