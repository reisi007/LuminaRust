# LuminaRust Umsetzungsplan

Dieser Plan ist eine lebende Arbeitsliste. Er wird während der Implementierung
fortgeschrieben. Erledigte Aufgaben werden nach bestandener unabhängiger
Verifizierung und bestätigter Testabdeckung aus dieser Datei entfernt — es gibt
keine dauerhafte Liste abgehakter Aufgaben. Details zu Erledigtem liegen in den
Feature-Dokumenten und der Git-Historie.

## Gepinnte Entscheidungen und Absprachen

- **LIZ / Projektlizenz (F-073-R2, MVP-Release-Gate):** interim proprietär/
  kommerziell — bewusst kein `license`-Feld, keine `LICENSE`-Datei (Entscheidung
  des Projekteigentümers 2026-08-20). Sobald entschieden (MIT / Apache-2.0 /
  Dual / MPL-2.0): `license`-Felder + Root-`LICENSE` ergänzen. Fixtures-R1 ist
  geschlossen (uneingeschränkte Nutzungs-/Distributionsgewährung für
  LuminaRust, dokumentiert in `sample-data/raw/README.md` §4/§8). Lensfun
  (LGPL-3.0 dynamisch gelinkt, DB CC-BY-SA-3.0) ist in
  `THIRD-PARTY-NOTICES.md` dokumentiert und gilt unabhängig von der Wahl.
- **MVP-Grenze:** MVP = CLI + native Desktop-GUI inkl. nativem RAW. WASM/Browser
  ist ersatzlos gestrichen (2026-09-04, kein Post-MVP). Cache- und
  Mehrbild-Synchronisierung sind bewusst Post-MVP. Architektur bleibt nativ
  (einheitlicher `decode_bytes`/`RawMetadata`-Vertrag).
- **Release-Staffel (User-Entscheid 2026-09-03, MVP = 1.0; Aktualisierung
  2026-09-04):** IPTC-/Metadaten-Presets + -Verwaltung wurden per
  User-Entscheid 2026-09-04 von 1.5 nach **1.0** vorgezogen (LRPAR-G15-IPTC-
  S1…S8; Entscheid: `feature/decisions/LRPAR-G15-META-15.md`, Sidecar-Draft,
  pure-Rust-Bake-In ohne Runtime-Abhängigkeit, JPEG-only, GUI-Panel gleich
  mit). Damit: 1.5 = HDR/Panorama-Merge, Rote Augen, Auto-Upright;
  2.0 = Gesichtserkennung, KI-Denoise; 2.5 = KI-Culling; nie Ziel:
  Karten-Modul/GPS, Veröffentlichungsdienste. Lensfun-Vollausbau,
  Keywords/Filter/Sammlungen/Smart-Sammlungen sind MVP (1.0).
- **Sidecar-Schema Pre-MVP:** Schemaänderungen sind bis zum MVP Breaking Changes
  (keine Abwärtskompatibilitätspflicht, Altdateien müssen nicht lesbar
  bleiben); die Migrations-Maschinerie (`migrate_sidecar_file`, `.bak`-Backup,
  `migrate_json`) bleibt dauerhaft im Code und wird ab dem MVP für
  Release-Migrationen genutzt; „Tests für jede Migration" gilt ab dem MVP. Der
  v1→v2-Migrationspfad mit Tests bleibt als Muster umgesetzt. Pre-Alpha-
  Ergänzung: `schema_version` bleibt 1; der Loader lehnt inkompatible Sidecars
  laut ab (keine stille Normalisierung außer dem historischen v0→v1-Bump).
- **Dependency-Pins (kein Upgrade ohne ADR):** libraw-sys vendored
  `[patch.crates-io]` (macOS-C++-Fix), `ort =2.0.0-rc.13` (= neueste RC),
  LibRaw 0.22.2 + Ubuntu-24.04-lensfun-Distro-Pin (Determinismus;
  Upgrade-Pfad skizziert: neuer Image-Tag parallel → Golden-Rebaseline wegen
  CR3-Dimensionen → alter Tag erst dann entfernen).
- **CI-Gate `onnx-rt` (2026-09-02, CI-ONNX-RT):** `onnx-rt` wird jetzt im CI geprüft — Image liefert `libssl-dev` + `clang` für `openssl-sys`, `ci.yml` führt `cargo clippy -p lumina-onnx --features onnx-rt` und `cargo test -p lumina-onnx --features onnx-rt` aus. Nur **GPU bleibt hartes CI-Nein** (kein Metal auf Runnern, nur `cargo check -p lumina-gpu --features gpu`).
- **Toolchain:** CI fährt `@stable` → neue Clippy-Lints schlagen automatisch an
  (Beispiel `chunks_exact_to_as_chunks`). Lokal vor jedem Push `rustup update`
  + workspace-clippy laufen lassen.
- **Post-MVP Backlog (nicht MVP-blockierend):** F-019 (siehe Phase 2), Phase 9
  Index (F-064…F-067), MCP-Erweiterungen (siehe
  F-101-F1; die Metadaten-Tools/-Resource sind mit LRPAR-G15-IPTC ab 1.0
  normativ und gehören nicht in dieses Backlog),
  Lensfun-Ausbau (CA via Lensfun, automatische Profil-Erkennung per
  EXIF), Produktnamen-Entscheidung (`docs/naming-brainstorm.md`,
  Brainstorm-Phase offen bis MVP-Entscheidung). WASM-Browser (F-069…F-071)
  ist ersatzlos gestrichen, kein Backlog.

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

## Offene Tasks — Legende der drei Blöcke und Releaseplan

Alle offenen Aufgaben sind in drei Blöcke gegliedert. Innerhalb jedes Blocks
gilt die Sortierung `[PRIO: hoch]` → `[PRIO: mittel]` → `[PRIO: niedrig]`;
die Priorisierung bewertet technische Tragweite/Risiko (kritische
Korrektheits-Bugs = hoch, Kosmetik/Doku = niedrig). Stand 2026-09-17:
13 offene Tasks (Checkbox-Zählung dieser Datei) — Block A: 6,
Block B: 1, Block C: 6.
Der Abschnitt `Releaseplan` ordnet jede Task-ID genau einer Version zu
(1.0 = MVP, 1.5, 2.0, 2.5, nie) — für Mensch und Maschine lesbar.

- **Block A — „Vor dem nächsten manuellen GUI/User-Test umsetzbar“:** alles,
  was ohne Rückfrage direkt umgesetzt werden kann und nicht von einem
  manuellen Test abhängt. **Block A ist vollständig ohne User-Interaktion
  abarbeitbar** (Reihenfolge: PRIO hoch → mittel → niedrig).
- **Block B — „Offene Rückfragen“:** Tasks, bei denen eine User-Entscheidung
  oder Klärung fehlt (Produkt-, Naming-, Lizenz-/Schema- oder Übernahme-
  Fragen). Dieser Block blockiert Block A nicht.
- **Block C — „Nach dem nächsten manuellen GUI-Test“:** Tasks, die erst nach
  dem nächsten manuellen GUI-Test sinnvoll/erforderlich sind (Verifikations-
  und Abschluss-Tasks, die auf Testergebnissen aufbauen).

## Releaseplan (User-Entscheide 2026-09-03; MVP = 1.0)

Maschinenlesbar: Die Tabelle `Version | Task-ID | Goal | Stichwort` ist die
verbindliche Zuordnung. Jede Task-ID kommt genau einmal vor; Ausführungsort
bleiben Block A/B/C. Tasks ohne expliziten User-Versionsentscheid gelten als
MVP-Annahme (1.0) und können per User-Entscheid umgebucht werden.
`fortlaufend` = ab 1.0 aktiv, gilt für alle Releases (CI-Strategie im Task).

| Version | Task-ID | Goal | Stichwort |
| --- | --- | --- | --- |
| 1.0 | NAMING-F1 | kein Goal | Produktname |
| 1.0 | R2-GUIMOD-04b | G-10 | GPU-Drossel-Entscheid |
| 1.0 | R2-GUIMOD-04c | G-10 | GPU-Histogramm |
| 1.0 | F-103-N6 | alle G | visueller User-Test |
| 1.0 | GUI-PARITY-GOLDENS-18 | alle G | Parity-Goldens-Rebless |
| 1.0 | UX-LOOK-LAYOUT-18 | alle G | Develop-Layout links |
| 1.0 | UX-LOOK-TOOLBAR-18 | alle G | Icon-Werkzeugleiste |
| 1.0 | UX-LOOK-TONECURVE-18 | alle G | Kurvengrafik |
| 1.0 | UX-LOOK-CROP-18 | alle G | Crop-Handles+Gitter |
| 1.0 | UX-LOOK-HISTORY-18 | alle G | History lesbar+Presets-Baum |
| 1.0 | GPU-RENDER-DENOISE-19 | G-14 | Denoise-WGSL-Pass |
| 1.0 | GPU-RENDER-PREVIEW-19 | alle G | GPU-Preview |
| 1.0 | GPU-RENDER-EXPORT-19 | alle G | GPU-Export |
| 1.0 | GPU-RENDER-MASK-19 | alle G | Masken-Pixelpass |
| 1.0 | GUI-JANKLOG-19 | alle G | Ruckel-Attribution-Log |
| 1.0 | GUI-REFACTOR-W0-20 | alle G | Wave 0 Blocker |
| 1.0 | GUI-REFACTOR-W1-20 | alle G | Jank-Pfade |
| 1.0 | GUI-REFACTOR-W2-20 | alle G | lib.rs-Entlastung |
| 1.0 | GUI-REFACTOR-W3-20 | alle G | Test/Audit-Extraktion |
| 1.0 | XXL-REFACTOR-SEC-20 | alle G | CLI/Sidecar/Core/GPU |
| fortlaufend | CI-WATCH-1 | alle G | CI-Beobachtung |
| 2.0 | LRPAR-G12-FACE-IMPL-20 | G-12 | Gesichtserkennung-Impl |
| 2.0 | LRPAR-G14-DENOISE-IMPL-20 | G-14 | KI-Denoise-Impl |
| 2.5 | LRPAR-G09-CULL-IMPL-25 | G-09 | KI-Culling-Impl |
| nie | — | G-12 | Karten-Modul/GPS (Nicht-Ziel) |
| nie | — | G-15 | Veröffentlichungsdienste (Nicht-Ziel) |

## Phase 3–5: Renderpipeline, RAW, Auto-Tone

Keine offenen Punkte. SOLL: `feature/architecture/pipeline.md` und
`feature/quality/performance-benchmarks.md`.

## Block A – „Vor dem nächsten manuellen GUI/User-Test umsetzbar“

**Dieser Block ist komplett abarbeitbar, ohne dass es einer Rückfrage oder
sonstigen User-Interaktion bedarf, und hängt nicht vom nächsten manuellen
GUI/User-Test ab.**

### PRIO: hoch

**LR-Parität aus `.goal/Goal.md` (Batch 1 + User-Featureliste, Stand 2026-09-03, 16 Goals, Aggregat ~39,1 %)**

Ziel: UI zum Verwechseln ähnlich zu Lightroom Classic; jedes Feature auf
CLI- **und** GUI-Ebene getestet. Quelle: `.goal/Goal.md` G-01…G-16, Beleg:
`.goal/lighroom screenshots batch 1/Content.md`. Umsetzung je Task via
`general`-Implementierungs-Agent + unabhängiger `general`-Verifizierungs-Agent
(Regel oben). LR-Parität-Batch aus 2026-09-04 (teils erledigt s. Git-Historie; Rest s. Releaseplan/Blöcke).

### PRIO: hoch

- [ ] **[PRIO: hoch] UX-LOOK-LAYOUT-18 (Release: 1.0)** Develop-Makrolayout an LR angleichen (Look-Analyse 2026-09-17, UXG-02): linke Rail mit Navigator + Presets-Baum + Snapshots + History + Copy/Paste; Footer-Admin-Aktionen entzerren, „Previous | Reset"-Äquivalent rechts verankern. Zuerst SOLL-Entscheid in `lightroom-ux-parity.md` (Seiten-Layout offen!), dann Implementierung. Abnahme: GUI-headless + kittest-Goldens, kein Rezept-/Sidecar-Verhalten geändert.
- [ ] **[PRIO: hoch] UX-LOOK-TOOLBAR-18 (Release: 1.0)** Icon-Werkzeugleiste (Look-Analyse 2026-09-17, UXG-04): Crop/Heal/Red-Eye/Masken + View-Toggles als Icons am LR-Ort (unter Histogramm/über Bild), Library-View-Tabs ikonisieren. Zielbild beachten (modern, aber vertraut). Abnahme: GUI-headless (jeder Button malt + schaltet), kittest-Goldens.
- [ ] **[PRIO: hoch] UX-LOOK-TONECURVE-18 (Release: 1.0)** Tone Curve als echte Kurvengrafik (Look-Analyse 2026-09-17, UXG-16): Punkte setzen/ziehen pro Kanal (Kanalwahl besteht), statt P0/P1-Slider-Reihen. Abnahme: GUI-headless (Punkt setzen/ziehen/löschen persistiert), Golden mit Kurvengrafik, kein stiller Fallback.
- [ ] **[PRIO: hoch] UX-LOOK-CROP-18 (Release: 1.0)** Crop-Overlay mit Eck-Handles, Drittel-Gitter, Abdunklung (Look-Analyse 2026-09-17, UXG-01): interaktives Aufziehen/Verschieben, Commit-Semantik per SOLL-Entscheid (steht aus — zuerst Doku). Abnahme: GUI-headless + Golden, Rezept-Semantik unverändert außer dokumentiertem Commit.
- [ ] **[PRIO: hoch] UX-LOOK-HISTORY-18 (Release: 1.0)** History menschenlesbar + Presets als Baum (Look-Analyse 2026-09-17, UXG-07/12): History-Einträge mit Reglername + alt→neu + Uhrzeit (Schema-Entscheid zuerst), Presets als Gruppen-Baum ohne absoluten Pfad. Abnahme: GUI-headless (Einträge lesbar + klickbar), Golden, kein Schema-Bruch ohne Migration.
- [ ] **[PRIO: hoch] GUI-JANKLOG-19 (Release: 1.0)** Ruckel-Ursachen aus dem Log ablesbar machen (User-Vorgabe 2026-09-18): Standard-Frames bleiben still (kein Per-Frame-Spam); nur langsame Aktionen/Render loggen (konfigurierbare Schwelle) mit Auslöser-Kette (Aktion → Rezept-Änderung → Renderpfad/Route → Teil-Dauern), damit aus dem Log klar ist, was den Ruckler verursacht hat. Zuerst SOLL-Entscheid in `feature/platform/cli-gui-wasm.md` (Schwelle, Log-Level, Trigger-Format, Debug vs. Release), dann Implementierung — keine Implementierung vor deiner Order. Abnahme: simulierter Slow-Render erzeugt genau eine attribuierte Zeile; Normalbetrieb bleibt still (Test assertet Stille); kein stiller Fallback. **Abdeckungsprüfung 2026-09-18 (Free-Agent, kein Doppelbau):** Bestand deckt NICHT ab — `action=`-Zeile loggt unbedingt jede Aktion (keine Schwelle, `gui_action.rs`), Dirty-Ursache (`mark_recipe_dirty`-Key) erreicht kein Log, Teil-Dauern (`render_draft_tick`-Split) unverknüpft per separater `trace!`-Zeile, Audit-Timing nur Handler-Summe als Test-Artefakt, Release bis auf `LUMINA_PERF` still. Präziser Rest: (a) Langsam-Filter + Schwelle, (b) Dirty-Key in Timing-Zeile, (c) Teil-Dauern mit Aktion verknüpfen, (d) Kette in einer greppbaren Zeile/Sequenz, (e) Stille-Test, (f) Release-Entscheid (Runde 2 fährt Release). **Refactor-Entscheide 2026-09-18 (Kampagne, Plan aus Build-Agent + free-Exploration):** Modulnamen per Vorschlag (`render_tick.rs`, `dirty.rs`, `present.rs`, `gpu_routing.rs`, `jank_log.rs` …), S1.2b `render_from` wird verschoben, neue Dateien strikt ≤500 (keine neuen Baseline-Einträge), Kampagne über alle XXL-Dateien. Umsetzung in Waves W0–W3 + SEC (s. Folgetasks) — kein willkürliches Splitten, Extraktion folgt Modulgrenzen.
- [ ] **[PRIO: hoch] GUI-REFACTOR-W0-20 (Release: 1.0)** Wave 0 (Blocker, S): `gpu_route_label` + `begin_gui_action` aus `lib.rs` nach `gui_action.rs` (`pub(crate)`, `#[cfg(debug_assertions)]` mitnehmen; `gui_action.rs` ≤500 wahren). Pflicht vor jeder instrumentierten Verschiebung. Abnahme: instrdbg-Format-Test + Release-Passthrough grün, Ratchet grün, unabhängige Verifizierung.
- [ ] **[PRIO: hoch] GUI-REFACTOR-W1-20 (Release: 1.0)** Wave 1 (Jank-Pfade, M–L): S1.1 `render_tick.rs` (Draft/Tick/Timings), S1.3 `dirty.rs` (Dirty-Key + Invalidierungsinvariante, `set_adjustment`-Duplikat NICHT anfassen), S1.4a `present.rs` + S1.4b `gpu_routing.rs`, S1.2a `render_source.rs`, S1.2b `render_from` → `render_pipeline.rs` (verschieben per Entscheid), S1.5 `jank_log.rs` NEU (erst nach JANKLOG-SOLL-Entscheid). Je Slice: verhaltensidentisch, Tests + kittest-Byte-Identität, Baseline senken, Exhaustivität ohne `_`-Arm erhalten, unabhängige Verifizierung.
- [ ] **[PRIO: hoch] GUI-REFACTOR-W2-20 (Release: 1.0)** Wave 2 (lib.rs-Entlastung, L): S2.1 Preview-Draws, S2.2 Develop-Sektionen je Modul, S2.3 Library, S2.4–S2.7 Develop-Frame/Filmstrip/Navigator/App-Frame, S2.8 Ops-Module (Backlog). Je Slice wie W1-Abnahme.
- [ ] **[PRIO: hoch] GUI-REFACTOR-W3-20 (Release: 1.0)** Wave 3 (Test/Audit, M–L): S3.1 `f100_action_button`-Guard als Block nach `src/tests/f100_audit.rs` (Guard bleibt im selben exhaustiven Match, kein `_`-Arm), S3.2 `mod tests`-Split thematisch (Testdateien strikt ≤500). Abnahme wie W1.
- [ ] **[PRIO: hoch] XXL-REFACTOR-SEC-20 (Release: 1.0)** Sekundär-Backlog (L–XL, eigene Crates → parallelisierbar): CLI-`main.rs` (Tests-Split 6181 zuerst), Sidecar-`lib.rs` (Tests-Split 5644; Serde-Teile nur byte-identisch + Roundtrip-Tests), Core-`lib.rs` (Tests-Split; erst nach Dedup-Audit), GPU-`lib.rs` + Baseline 5250→5246 senken. Je Crate ein Agent, Abnahme wie W1.

### PRIO: mittel

- [ ] **[PRIO: niedrig] CI-WATCH-1 (fortlaufend)** Nach jedem Push (morgen als erstes): CI-Runs prüfen (`gh run watch` / `gh run list --branch main`), Ergebnis im Tagesstand vermerken. Bei Rot: als Next-Task in `Agents.todo.md` dokumentieren, NICHT still umsetzen (User-Vorgabe). Abnahme: jeder Push hat ein geprüftes CI-Verdict.
- Veröffentlichungsdienste bleiben explizit nie Ziel (kein Task).

### PRIO: niedrig

- [ ] **[PRIO: niedrig] LRPAR-G14-DENOISE-IMPL-20 (Release: 2.0)** KI-Denoise-Implementierung nach Entscheid `feature/decisions/LRPAR-G14-DENOISE-20.md` (F-078-Fixture-Entscheid ✓, Schema ✓, Pipeline-Stufe + Persistenz + GPU-Refusal ✓, ONNX-Backend ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only; offen: Gewichte). Abnahme: CLI + GUI-headless + Golden/PSNR, kein stiller Fallback.
- [ ] **[PRIO: niedrig] LRPAR-G09-CULL-IMPL-25 (Release: 2.5)** KI-Culling-Implementierung nach Entscheid `feature/decisions/LRPAR-G09-CULL-25.md` (Schema ✓, Heuristik `lumina-cull` ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only; offen: optional ONNX Stufe 2). Abnahme: CLI-Exit-Codes + GUI-headless + kittest, Vorschlag schreibt nie Rating/Flag/Label.

- [ ] **[PRIO: niedrig] LRPAR-G12-FACE-IMPL-20 (Release: 2.0)** Gesichtserkennung-Implementierung nach Entscheid `feature/decisions/LRPAR-G12-FACE-20.md` (S1 Schema ✓, S2 ONNX ✓, S3 Clustering ✓, S4 CLI ✓, S5 GUI inkl. Masken-Brücke ✓, Vektor-Record-Kind `face_embedding` ✓ §3.2; offen: S6 Lizenzen/Gewichte). Abnahme: CLI + GUI-headless, Sidecar-first, kein stiller Fallback. Karten-Modul/GPS bleibt nie Ziel (kein Task).

### PRIO: niedrig (Block A, nicht-LRPAR)

**Phase 6: Persistente AI-Masken**

_(keine offenen Tasks — F-082-FOLLOWUP BESTANDEN verifiziert 2026-09-02, 107p `onnx-rt`, wasm32 `onnx-rt` grün, Commit 49f4f76)_

**Phase 9: Optionale zentrale Indizierung (Post-MVP)**

_(keine offenen Tasks — F-064…F-067 BESTANDEN verifiziert 2026-09-02, Commit 1520ac5, Doc-only: minimaler Umfang/Cacheverweise, SQLite non-default `index` `assets`/`jobs`/`cache_refs` WAL/`user_version`/`.lumina/index/`, Rebuild/Locking/`integrity_check`/corrupt sichtbar/Sidecar-only, Löschsicherheit Delete→Rebuild identisch; `cargo check --workspace` grün)_

Ist-Stand 2026-09-02: kein Index-Modul im Workspace; CLI-`reindex` ist nur ein
Sidecar-Scan (zählt valide Sidecars, persistiert nichts); `feature/architecture/index.md` ist normativ vervollständigt und verifiziert.

**Phase 10: WASM und Plattformen — ENTFERNT 2026-09-04**

_(WASM/Browser ersatzlos gestrichen; F-069…F-071 entfallen, WASM-CI-Job gelöscht.
Historisch: F-069…F-071 Doku-first 2026-09-02, Commits 287fe75/e60a9ad)_

**Phase 10b: Generatives Entfernen + Erweitern (Post-MVP)**

_(keine offenen Tasks — GEN-EXPAND-1 BESTANDEN verifiziert 2026-09-02, Commit 46f6baf: Doku-first GenerativeEdit Felder sha256 Prompt/Seed inference_resolution canvas>100% region/mask ref artifact .lumina.zdata kind=generative_canvas atomar, Pipeline Decode→SourceActions→GenerativeEdit→Lens→Perspective→Crop, Identität/Veraltung analog AI-Masken, kein stiller Fallback, Capability lokal vs Cloud getrennt, Lizenz F-078; kein Code)_


## Block B – „Offene Rückfragen“

Tasks, bei denen eine User-Entscheidung/Klärung fehlt (Produkt-, Naming-,
Lizenz-/Schema- oder Übernahme-Fragen). Blockiert Block A nicht; sollte aber,
wo möglich, vor dem nächsten manuellen GUI-Test geklärt werden.

### PRIO: mittel

**Produktname (Rest von F-101-F1)**

- [ ] **[PRIO: mittel] NAMING-F1 (kein Goal, Release: 1.0)** Produktname final entscheiden
  (`docs/naming-brainstorm.md`). **User-Entscheidung 2026-08-25:** Brainstorm
  läuft bewusst weiter, Naming bleibt offen. **User-Entscheidung 2026-09-04:**
  Naming erst kurz vor MVP entscheiden — bis dahin keine Naming-Arbeit, kein
  Vorziehen. Die übrigen F-101-F1-Anteile
  (MCP-Scope) wurden zur Umsetzung freigegeben und stehen in Block A.

## Block C – „Nach dem nächsten manuellen GUI-Test“

Tasks, die erst nach dem nächsten manuellen GUI-Test sinnvoll/erforderlich
sind (Verifikations- und Abschluss-Tasks, die auf Testergebnissen aufbauen).

### PRIO: hoch

**Phase 8: Desktop-GUI (F-103, MVP)**

UI-Konventionen F-100 sind spezifiziert, verifiziert und für jede GUI-Arbeit
verbindlich — normativ in `feature/platform/cli-gui-wasm.md` (Abschnitt
F-100). SOLL für den MVP-Scope: ebenda „Desktop-GUI" und „Erster visueller
User-Test". Die implementierten Slices (Module, Develop-Sektionen, interaktive
Maskenwerkzeuge, Exportmodul, i18n, Presence/Vibrance, kittest-Snapshots) sind
unabhängig verifiziert; Details in Git-Historie und Feature-Dokument.

Vor F-103-N6 empfohlen: nichts mehr offen — die Review-Befunde
(REVIEW-CORE-CROP-1, REVIEW-GUI-DEBOUNCE-1, REVIEW-GUI-MASKRENDER-1) sind mit
Marker-Kommentaren im Code implementiert; die F-103-N6-Runde 1 hat eigene
Befunde erzeugt (GUI-CLICK-ALL-17, GUI-ROUTING-N6, GUI-INSTRDBG-17 — alle
BESTANDEN verifiziert).

- [ ] **[PRIO: mittel] GUI-PARITY-GOLDENS-18 (Release: 1.0)** Veraltete `kittest_parity`-Goldens neu erzeugen (Befund 2026-09-17, User-Entscheid): `parity_paths_*` (`cpu_gpu_path_parity_matrix`, `lensfun_corrector_cell_presents_cpu_and_refuses_vram`) scheitern lokal an alten Snapshots (numerische Parität grün, nur Snapshots stale; CI-ignoriert). Goldens auf Metal-Maschine mit `UPDATE_SNAPSHOTS=true` neu erzeugen, Diff sichten (nur beabsichtigte UI-Änderungen seit 6b4105e), danach 4/4 grün. Abnahme: `cargo test -p lumina-gui --test kittest_parity -- --ignored` 4/4 grün.
- [ ] **[PRIO: mittel] R2-GUIMOD-04b (→ G-10, Release: 1.0)** (nach manuellem Test + 04a-Zahlen): CPU-Draft-Drossel auf GPU-Pfaden entscheiden (throttlen vs. GPU-Histogramm 04c vs. lassen). Eingang: 04a-Messwerte aus F-103-N6.
- [ ] **[PRIO: mittel] R2-GUIMOD-04c (→ G-10, Release: 1.0)** (nach manuellem Test, Alternative zu 04b): Histogramm per GPU-Compute aus VRAM (1-KB-Readback statt Full-Frame-Analyse). Nur wenn 04a-Zahlen den Aufwand rechtfertigen; CPU-Pfad bleibt für Non-GPU (als Fallback, nicht WASM — WASM ist gestrichen).

- [ ] **[PRIO: hoch] F-103-N6 (→ Querschnitt, alle G, Release: 1.0)** Erster visueller User-Test: `RUST_LOG=trace cargo run -p lumina-gui` (Trace-Pflicht nach DoD §6) mit
  PNG/JPEG/WebP + nativen RAW per Pfad und Drag&drop; Preview + Exposure/
  Contrast ändern den Renderstand; Sidecar wird geschrieben und beim Neustart
  wiederhergestellt. Abnahme:
  reproduzierbare Befehle aus cli-gui-wasm.md + Log-Ausschnitt; unabhängiger Verifizierungs-
  Agent bestätigt F-100-Checkliste + Tests (BESTANDEN). Letzter Schritt vor
  Abschluss von Phase 8.
  **Bereit für Runde 2 (Release-Build, Stand 2026-09-18):** Geladet: GUI-GPU-AUDIT-17
  (verifiziert + committet — Audit-Test + Timing-Baseline), GPU-LENSFUN-Kern +
  -Wiring (gemeinsam verifiziert BESTANDEN; Commit nach SEC-Abgrenzung der
  Baseline — die Lensfun-Ausnahme steht nicht mehr im Runde-2-Log).
  GPU-RENDER-DENOISE/PREVIEW/EXPORT/MASK-19 sind bewusst dokumentiert-only
  (keine Implementierung vor Runde 2 — User-Entscheid 2026-09-18); Runde 2
  protokolliert deren CPU-Routen als bekannte Gaps, nicht als neue Befunde.
  GUI-PARITY-GOLDENS-18 empfohlen, nicht Pflicht (nur CI-ignorierte Goldens).
  R2-GUIMOD-04b/04c brauchen 04a-Zahlen aus Runde 2 (danach). UX-LOOK-18 sind
  unabhängig (Look beeinflusst Routing-/Perf-Messung nicht).

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
