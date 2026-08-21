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
`write_atomically`), **F-103-M1** (i18n-Restliterale beseitigt).
Offen: F-103-N6 (visueller User-Test), F-103-N7 (Presence-/Vibrance/Saturation-
Regler), F-103-N8 (CLI-Doppelrender), F-103-N9 (kittest-Screenshot-
Verifikation). Browser-Dateispeichern, ONNX, Masken-Inferenz, Cache-
Synchronisierung und Mehrbild-Bearbeitung bleiben bewusst Post-MVP;
WASM ist dokumentierte Capability-Grenze, keine MVP-GUI.

- [ ] **F-103-N7** Fehlende Presence- und Dynamik/Sättigung-Regler ergänzen
  (Befund Verifizierung 2026-08-21): Texture, Clarity, Dehaze (Presence,
  F-094) sowie Vibrance, Saturation (F-092) als Regler in den Sektionen
  Effects bzw. Color — sofern Rezeptfelder/Pipeline-Stufen existieren, sonst
  dokumentierte „nicht verfügbar"-Kennzeichnung je Stufe. Ziel: normative
  F-100-Reglerreihenfolge vollständig sichtbar.
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