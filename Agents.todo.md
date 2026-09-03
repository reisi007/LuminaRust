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
- **Release-Staffel (User-Entscheid 2026-09-03, MVP = 1.0):** 1.5 = HDR/Panorama-
  Merge, Rote Augen, IPTC-/Metadaten-Presets + -Verwaltung, Auto-Upright;
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
  F-101-F1), Lensfun-Ausbau (CA via Lensfun, automatische Profil-Erkennung per
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
Korrektheits-Bugs = hoch, Kosmetik/Doku = niedrig). Stand 2026-09-03:
28 offene Tasks — Block A: 24, Block B: 1, Block C: 3.
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
| 1.0 | GUI-STARTUP-FOLLOWUP-1 | G-10/G-11 | Startup-Followups |
| 1.0 | AGENT-HARNESS-2 | alle G | AccessKit-Semantik |
| 1.0 | AGENT-HARNESS-3 | G-01/G-07/G-08/G-10 | Green-Path-Matrix |
| 1.0 | AGENT-HARNESS-4 | G-10 | Bildkorrektheit |
| 1.0 | LRPAR-G03-MASK | G-03 | Maskierung |
| 1.0 | LRPAR-G04-REMOVE | G-04 | Remove-Automatik |
| 1.0 | LRPAR-G05-LENSBLUR | G-05 | Lens Blur |
| 1.0 | LRPAR-G01-BASIC | G-01 | Develop-Basis |
| 1.0 | LRPAR-G02-COLOR | G-02 | Kurve/Mixer/Grading |
| 1.0 | LRPAR-G06-GEO | G-06 | Crop/Lensfun |
| 1.0 | LRPAR-G08-PREVIOUS | G-08 | Previous |
| 1.0 | LRPAR-G09-LIB | G-09 | Library-Kern |
| 1.0 | LRPAR-G10-VIEWER | G-10 | Viewer/Softproof |
| 1.0 | LRPAR-G11-OVERLAYS | G-11 | Overlays/Pins |
| 1.0 | LRPAR-G15-META-MVP | G-15 | Keywords/Sammlungen |
| 1.0 | LRPAR-G16-POWER | G-16 | Power-Shortcuts |
| 1.0 | NAMING-F1 | kein Goal | Produktname |
| 1.0 | R2-GUIMOD-04b | G-10 | GPU-Drossel-Entscheid |
| 1.0 | R2-GUIMOD-04c | G-10 | GPU-Histogramm |
| 1.0 | F-103-N6 | alle G | visueller User-Test |
| fortlaufend | LRPAR-MATRIX-RECIPE | alle G | Rezept-Matrix |
| 1.5 | LRPAR-G06-UPRIGHT-15 | G-06 | Auto-Upright |
| 1.5 | LRPAR-G13-MERGE-15 | G-13 | HDR/Panorama-Merge |
| 1.5 | LRPAR-G14-REDEYE-15 | G-14 | Rote Augen |
| 1.5 | LRPAR-G15-META-15 | G-15 | IPTC/Presets |
| 2.0 | LRPAR-G12-FACE-20 | G-12 | Gesichtserkennung |
| 2.0 | LRPAR-G14-DENOISE-20 | G-14 | KI-Denoise |
| 2.5 | LRPAR-G09-CULL-25 | G-09 | KI-Culling |
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

- [ ] **[PRIO: mittel] AGENT-HARNESS-3 (→ Querschnitt, G-01/G-07/G-08/G-10)** Green-Path-Matrix (F-100, alle Module): Library (Open/Select/Toggle/Range), Develop (jede Slider-Klasse Edit→Commit→Sidecar→Reload, Auto-Tone, Match, WB-Pick, Rotate, Reset, Render), Sync/Match-Selection (N Sidecars, Fehler isoliert laut), Navigator/Zoom/Pan (alle Stufen, Custom-Pin), Export (Datei byte-valide), Fehlerpfade laut ohne stillen Fallback. Abnahme: pro Zeile ein headless Test + DoD-§7-Mapping.
- [ ] **[PRIO: mittel] AGENT-HARNESS-4 (→ G-10)** Bildkorrektheit (F-100 Preview): opaque Alpha, Center-Pixel-Delta, Fit-Rahmen=Background, Luminanz-Toleranz (sRGB-Fang), Thumbnail-Hash/PSNR gegen 1–2 Fixtures, Stale-Generation-Guard nach Bildwechsel. Abnahme: Pixel-Asserts in Harness-Tests, kein reiner Layout-Nachweis.

**LR-Parität aus `.goal/Goal.md` (Batch 1 + User-Featureliste, Stand 2026-09-03, 16 Goals, Aggregat ~39,1 %)**

Ziel: UI zum Verwechseln ähnlich zu Lightroom Classic; jedes Feature auf
CLI- **und** GUI-Ebene getestet. Quelle: `.goal/Goal.md` G-01…G-16, Beleg:
`.goal/lighroom screenshots batch 1/Content.md`. Umsetzung je Task via
`general`-Implementierungs-Agent + unabhängiger `general`-Verifizierungs-Agent
(Regel oben). 20 Tasks: 4 hoch, 12 mittel, 4 niedrig (Stand 2026-09-03 inkl. Release-Staffel-Entscheiden; Versionszuordnung s. `Releaseplan`).

### PRIO: hoch

- [ ] **[PRIO: hoch] LRPAR-G03-MASK** Maskierungs-Parität (G-03, ~30 %): AI-Auswahl Subject/Sky/Background/Objects/People (+ Teile bis Pupille/Sclera), Add/Subtract/Invert/Duplicate-Kombinatorik im Panel, Color-/Luminance-Range, Show + Color Overlay, Maskenliste mit Sichtbarkeits-Auge. Abnahme: CLI (Rezeptfelder, Roundtrip, Fehler laut, kein stiller Fallback) + `cargo test -p lumina-gui` headless (Panel, Overlay, Persistenz pro virtueller Kopie).
- [ ] **[PRIO: hoch] LRPAR-G04-REMOVE** Remove-Parität (G-04, ~35 %): Visualize-Spots-Slider, Tool-Overlay-Modi (Always/Auto/Never), Detect-Objects, Distraction Removal (Reflections/People/Dust, Auto), generativ-Varianten neu generieren. Abnahme: CLI + GUI-headless wie G03, Golden/PSNR-Gates für Heal-Pfade.
- [ ] **[PRIO: hoch] LRPAR-G05-LENSBLUR** Lens-Blur-Produktfunktion (G-05, ~10 %): Fokus-Rahmen, Focal Range, Blur Amount, Bokeh-Formen, Rezept + Persistenz. Abnahme: CLI + GUI-headless, kein stiller Fallback bei fehlendem Tiefenartefakt.
- [ ] **[PRIO: hoch] LRPAR-MATRIX-RECIPE** Rezept-Matrix auf Sample-Bildern (Dach-Task aller G): x Rezepte × 2 Sample-Bilder (`sample-data/raw/aircraft-landscape.cr3`, `aircraft-portrait.cr3`) anwenden, exportieren, verifizieren (Golden/PSNR mit dokumentierten Toleranzen). CI-Strategie (User-Entscheid 2026-09-03): PR-CI bleibt schlank; volle Matrix läuft per Nightly-Schedule (1×/Tag) + vor Releases + opt-in per Commit-Marker (`[matrix]` im Titel/Body, `!`- bzw. `BREAKING CHANGE`-Commits triggern mit); manueller `workflow_dispatch`. Der Multi-Rezept-Runner-Support (CLI + GUI-headless) ist Teil der Aufgabe. Abnahme: Matrix läuft in allen drei Modi grün, Kosten/Dauer dokumentiert.

### PRIO: mittel

- [ ] **[PRIO: mittel] LRPAR-G01-BASIC** Develop-Basis-Lücken (G-01, ~75 %): Treatment/Profile-Parität, „Original Photo“-Histogrammvergleich, „Reset Sliders Automatically“-Option, Previous-Button-Verhalten je Panel. Abnahme: CLI + GUI-headless.
- [ ] **[PRIO: mittel] LRPAR-G02-COLOR** Kurve/Mixer/Grading-Lücken (G-02, ~60 %): Point Color, Color-Grading-Feinschliff (Schatten/Mitten/Lichter), parametrische + Punkt-Kurve je Kanal. Abnahme: CLI + GUI-headless, Render-Golden.
- [ ] **[PRIO: mittel] LRPAR-G06-GEO (Release: 1.0)** Geometrie-MVP (G-06): Crop + Straighten + Aspect-Parität (History-sichtbar) + Lensfun-Vollausbau (User-Entscheid 2026-09-03). Abnahme: CLI + GUI-headless.
- [ ] **[PRIO: mittel] LRPAR-G06-UPRIGHT-15 (Release: 1.5)** Auto-Upright (G-06-Abspaltung, User-Entscheid 2026-09-03): automatische Upright-Analyse als Rezept-Stufe. Abnahme: CLI + GUI-headless, Golden-Gates.
- [ ] **[PRIO: mittel] LRPAR-G08-PREVIOUS** Previous-Übernahme (G-08, ~55 %): Ein-Klick-Übernahme vom Vorbild (Previous) zusätzlich zu Sync/Match. Abnahme: CLI + GUI-headless, History-Schritt je Zielbild.
- [ ] **[PRIO: mittel] LRPAR-G09-LIB** Library-Parität Kern (G-09, ~45 %): Grid/Loupe/Compare/Survey-Vollparität inkl. `G`/`E`/`C`/`N`, Katalog-/Ordner-Verwaltung; Assisted Culling ist 2.5, KI-Culling kein MVP (User-Entscheid 2026-09-03). Abnahme: GUI-headless je Ansicht + CLI-Seite wo Rezept-relevant.
- [ ] **[PRIO: mittel] LRPAR-G10-VIEWER** Viewer-Lücken (G-10, ~65 %): „Original Photo“-Histogramm, `S`-Softproof-Shortcut + Druck-/Farbraumsimulation-Scope. Abnahme: GUI-headless, kein reiner Layout-Nachweis.
- [ ] **[PRIO: mittel] LRPAR-G11-OVERLAYS** Overlay-/Panel-Comfort (G-11, ~60 %): Tool-Overlay-Modi, Edit-Pins-Sichtbarkeit (Always/Auto/Never), Solo-Mode, `Shift+Tab`. Abnahme: GUI-headless.
- [ ] **[PRIO: mittel] LRPAR-G14-REDEYE-15** Rote-Augen-Korrektur (G-14-Abspaltung, Ziel 1.5, User-Entscheid 2026-09-03): Erkennung + Korrektur als Rezept-Stufe mit Persistenz. Abnahme: CLI + GUI-headless, Golden-Gates.
- [ ] **[PRIO: niedrig] LRPAR-G14-DENOISE-20** KI-Denoise (G-14-Abspaltung, Ziel 2.0, User-Entscheid 2026-09-03): Modell-/Lizenz-/Capability-Entscheid (F-078) + Doku-first, manuelles NR F-096 bleibt MVP. Abnahme: Entscheid in `feature/` + Folge-Implementierungstask.
- [ ] **[PRIO: mittel] LRPAR-G15-META-MVP** Metadaten-MVP (G-15-Kern, MVP = 1.0, User-Entscheid 2026-09-03): Keywords, Filterungen (u. a. Brennweite/Kamera/ISO über `\`-Leiste hinaus), Sammlungen + Smart-Sammlungen, Stapel-Vollfunktion. Abnahme: CLI (Roundtrip, kein Datenverlust, Sidecar-first) + GUI-headless.
- [ ] **[PRIO: mittel] LRPAR-G15-META-15** Metadaten-Verwaltung 1.5 (User-Entscheid 2026-09-03): IPTC-Vergabe, Metadaten-Presets, Stapelvergabe. Abnahme: CLI + GUI-headless. Veröffentlichungsdienste sind explizit nie Ziel (kein Task).
- [ ] **[PRIO: mittel] LRPAR-G16-POWER** Power-Shortcut-Rest (G-16, ~40 %): `Shift`-Doppelklick Auto-Weiß-/Schwarzpunkt, Alt+Regler-Maskierungsvorschau, `S`-Belegung. Abnahme: GUI-headless je Shortcut, keine Kollision mit Bestand (`w2/w3`-Mapping-Tests erweitern).

### PRIO: niedrig

- [ ] **[PRIO: niedrig] LRPAR-G12-FACE-20** Gesichtserkennung (G-12-Abspaltung, Ziel 2.0, User-Entscheid 2026-09-03): Doku-first (Modell-/Lizenz-/Capability-Entscheid, ONNX-Detektion + Embedding + Clustering + UI + Persistenz-Scope). Abnahme: Entscheid in `feature/` + Folge-Implementierungstask. Karten-Modul/GPS ist explizit nie Ziel (kein Task, in `.goal/Goal.md` als Nicht-Ziel vermerkt).
- [ ] **[PRIO: niedrig] LRPAR-G13-MERGE-15** HDR-/Panorama-Merge (G-13, Ziel 1.5, User-Entscheid 2026-09-03): `Cmd/Ctrl+H` + `Cmd/Ctrl+M` → DNG — Doku-first (Pipeline-Einordnung, DNG-Schreibung, Ausrichtungs-Scope). Abnahme: Entscheid in `feature/` + Folge-Implementierungstask.

_(Block A leer — FILMSTRIP-SYNC-1 BESTANDEN 229p + kittest 10/10 + Vision 7/7, Commit folgt; Details Git-Historie)_

_(Block A leer — R2-GUIMOD-04a BESTANDEN 2026-09-04 575f834, WASM-REMOVE BESTANDEN; Details Git-Historie)_

_(weitere ehemals offene hoch-prio Tasks BESTANDEN s. Git-Historie: GUI-AUTOTONE-SAVE-1 204p c29e609a/5e36133, GUI-KIT-01-REFRESH kittest 10/10 a75b42f, CLI-GUI-PARITY-1 Matrix-Doc a75b42f; F-103-INTEGRATION-PREVIEW-SIDECAR 147p 43b1b73; CI-ONNX-RT 953987e/c5e5e06/67690ec)_

_(keine weiteren offenen hoch-prio Tasks — F-103-INTEGRATION-PREVIEW-SIDECAR verifiziert BESTANDEN am 2026-09-02, 147p (144→147 +3), core 277+7, sidecar 86p, clippy/fmt/wasm grün, Commit 43b1b73; CI-ONNX-RT und FOLLOWUP-R2-NIEDRIG-REST verifiziert BESTANDEN am 2026-09-02, siehe Git-Historie 953987e/c5e5e06/67690ec)_

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

- [ ] **[PRIO: mittel] NAMING-F1 (kein Goal)** Produktname final entscheiden
  (`docs/naming-brainstorm.md`). **User-Entscheidung 2026-08-25:** Brainstorm
  läuft bewusst weiter, Naming bleibt offen. Die übrigen F-101-F1-Anteile
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

Vor F-103-N6 empfohlen: kleine Stabilitäts-Fixes aus den Review-Befunden
(z. B. REVIEW-CORE-CROP-1, REVIEW-GUI-DEBOUNCE-1, REVIEW-GUI-MASKRENDER-1),
damit der manuelle Test aussagekräftig ist.

- [ ] **[PRIO: mittel] R2-GUIMOD-04b (→ G-10)** (nach manuellem Test + 04a-Zahlen): CPU-Draft-Drossel auf GPU-Pfaden entscheiden (throttlen vs. GPU-Histogramm 04c vs. lassen). Eingang: 04a-Messwerte aus F-103-N6.
- [ ] **[PRIO: mittel] R2-GUIMOD-04c (→ G-10)** (nach manuellem Test, Alternative zu 04b): Histogramm per GPU-Compute aus VRAM (1-KB-Readback statt Full-Frame-Analyse). Nur wenn 04a-Zahlen den Aufwand rechtfertigen; CPU-Pfad bleibt für Non-GPU (als Fallback, nicht WASM — WASM ist gestrichen).

- [ ] **[PRIO: hoch] F-103-N6 (→ Querschnitt, alle G)** Erster visueller User-Test: `RUST_LOG=trace cargo run -p lumina-gui` (Trace-Pflicht nach DoD §6) mit
  PNG/JPEG/WebP + nativen RAW per Pfad und Drag&drop; Preview + Exposure/
  Contrast ändern den Renderstand; Sidecar wird geschrieben und beim Neustart
  wiederhergestellt. Abnahme:
  reproduzierbare Befehle aus cli-gui-wasm.md + Log-Ausschnitt; unabhängiger Verifizierungs-
  Agent bestätigt F-100-Checkliste + Tests (BESTANDEN). Letzter Schritt vor
  Abschluss von Phase 8.

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
