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
Korrektheits-Bugs = hoch, Kosmetik/Doku = niedrig). Stand 2026-09-23:
12 offene Tasks (Checkbox-Zählung dieser Datei) — Block A: 7,
Block B: 1, Block C: 4.
Der Abschnitt `Releaseplan` ordnet jede Task-ID genau einer Version zu
(1.0 = MVP, 1.5, 2.0, 2.5, nie) — für Mensch und Maschine lesbar.

**User-Anweisung 2026-09-24:** Nach dem aktuellen Release-Block sollen
Future-Release-Tasks proaktiv vorgezogen werden, sobald keine aktuelle
Abhängigkeit und kein unüberwindbares Hardware-/User-Gate blockiert. Die
Releaseplan-Zuordnung und die bestehenden Abnahme-Gates bleiben dabei unverändert.

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
| 1.0 | F-103-N6-GUI-COVERAGE-27 | alle G | vollständige manuelle GUI-/Persistenz-Checks |
| 1.0 | GPU-PARITY-HW-28 | alle G | Hardware-GPU-Parität |
| 1.0 | GPU-RENDER-DENOISE-19 | G-14 | Denoise-WGSL-Pass |
| 1.0 | GPU-RENDER-PREVIEW-19 | alle G | GPU-Preview |
| 1.0 | GPU-RENDER-EXPORT-19 | alle G | GPU-Export |
| 1.0 | GPU-RENDER-MASK-19 | alle G | Masken-Pixelpass |
| 1.0 | LRPAR-G15-STACK-15 | G-15 | Bilderstapel |
| 1.0 | LRPAR-G09-SORT-09 | G-09 | Sortierung + Custom-Sort |
| 1.0 | LRPAR-G03-MASKGROUP-03 | G-03 | Maskengruppen |
| 1.0 | R5-DUST-23-FOLLOWUP | G-04 | Spot-Heal-Followup |
| 1.0 | R5-BRUSH-24 | G-03 | Pinsel-Masken |
| 1.0 | R5-MASKVIS-25 | G-03 | Masken-Overlay |
| fortlaufend | CI-WATCH-1 | alle G | CI-Beobachtung |
| fortlaufend | CI-SHARD-26 | alle G | CI-Shard |
| 2.0 | LRPAR-G12-FACE-IMPL-20 | G-12 | Gesichtserkennung-Impl |
| 2.0 | LRPAR-G12-FACE-ADAPTER-25 | G-12 | Face-I/O-Adapter |
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

- [x] **[PRIO: hoch] CI-SHARD-26 (User-Order 2026-09-20)** CI-Wandzeit ~5:00 → ~3:00: `rust`-Job in `ci.yml` splitten (`rust-fast`: fmt+check+alle Clippy+Ratchet; `rust-test-gui`: nur `lumina-gui --all-targets` = Long Pole 2:35; `rust-test-rest`: Rest + libraw-ABI + zdata + lensfun + onnx-rt), Cargo-Cache über alle Jobs teilen. Analyse in Konversation 2026-09-20 (Run 35531727105). Abnahme: CI grün + Wandzeit-Beleg + unabhängige Verifizierung. **Stand 2026-09-23:** Shard-Implementierung, Cache-/Gate-Prüfung, Actionlint und unabhängige statische Verifizierung eingebaut; doppelte zdata-/Dokument-Gates bereinigt und GPU-Laufzeittests aus dem headless Rest-Shard ausgeschlossen. GitHub-Lauf `35908233116` grün (3:45 Gesamt-Laufzeit; GUI 3:11, Rest 3:25); manueller Matrix-Nightly `35905160512` grün (10:07). Abnahme erfüllt; GPU-Parität bleibt separat als `GPU-PARITY-HW-28` offen.
- [ ] **[PRIO: hoch] R5-DUST-23-FOLLOWUP (User-Order 2026-09-20; Remediierung 2026-09-23)** Dust-Erweiterung: Anzeige nur bei Auswahl (Ausgewähltes wie Maske), Entfernungen bearbeitbar inkl. Neu-Generierung, Typ Generate/KI-generiert (Clone nie verwendet). **Stand nach R5-DUST-23-FOLLOWUP-Remediierung:** CLI-Kontradiktionsmatrix mit atomarem No-Write-Failure, eindeutige Spot-IDs/compat-ID-Materialisierung für typed-only generative Einträge, Referenz-basierter missing/stale/corrupt-Status in GUI+CLI und 1024×720-Selected-Detail-Anordnung sind implementiert und headless abgedeckt; die unabhängige Funktionsverifizierung ist bestanden. Der Task bleibt wegen des separaten Hardware-Gates offen. **Konkreter manueller GPU-Nachweis bleibt erforderlich:** auf echter Hardware (Metal/Vulkan, nicht llvmpipe/Software) `RUST_LOG=trace cargo test -p lumina-gpu --features gpu --test parity` mit genau einer App-Instanz und Log-Redirect ausführen; GUI-R5 darf bis dahin nicht als vollständig abgeschlossen gelten.
- [ ] **[PRIO: hoch] R5-BRUSH-24 (User-Order 2026-09-20, User-Urteil „großer Fail", nach Sort-Mini-Welle)** Pinsel-Masken auf Lightroom-Niveau: Größe/Weichheit/Fluss einstellbar (Slider + `[`/`]`-Shortcuts als Alias), Kreis-Cursor mit live Größe am Zeiger, mehrere Masken pro Bild anlegbar + einzeln wählbar (Pin-Liste klickbar), dazu vollständige Maskenverwaltung (Liste aller Masken mit Sichtbarkeits-Toggle, Umbenennen, Löschen, Reihenfolge, Duplizieren — alles klickbare Buttons). **Stand 2026-09-24:** Code-Level-Funktionsverifizierung bestanden: CPU/no-GPU, Persistenz-Atomizität, kumulative Live-Plane/Overlay, 110-Action-Audit und 1024×720-Struktur/Golden sind grün. Der Task bleibt bis zum echten Metal/Vulkan-/DPI-/Pointer-Nachweis offen; der Native-Kittest-Golden wird separat mit `--ignored` geprüft.
- [ ] **[PRIO: hoch] R5-MASKVIS-25 (User-Order 2026-09-20, präzisiert, nach Sort-Mini-Welle)** Masken-Overlay nur sichtbar, wenn die Masken-Ansicht geöffnet ist; togglebar: nur Pins für alle Masken vs. volles Overlay für die aktuell ausgewählten Maske. Dazu Bildbereich vergrößern (Seiten-Panels ausblendbar für maximale Preview). **Stand 2026-09-24:** State-Machine, CPU/GPU-Gates, Overlay-Farbparität, 110-Action-Audit und R5-Goldens unabhängig verifiziert; der Task bleibt bis zum echten Metal/Vulkan-/DPI-/Pointer-Nachweis offen.

### PRIO: mittel

- [ ] **[PRIO: mittel] GPU-PARITY-HW-28 (User-Order 2026-09-23)** Bestehende GPU-Parity-Laufzeitprüfung auf einer echten Hardware-Lösung (Metal/Vulkan) mit renderbaren `R32Float`-Render-Targets wiederholen. Der lokale `llvmpipe`-/GL-Softwareadapter ist keine zulässige Paritätsreferenz und erzeugte die reproduzierten `detail_stage_stack`-/Sharpening-Fehler; keine Toleranzen, Ignores oder CI-Skips als Ersatz. Abnahme: Hardware-Run mit `cargo test -p lumina-gpu --features gpu --test parity`, Protokoll und unabhängige Verifizierung.

### PRIO: niedrig

- [ ] **[PRIO: niedrig] CI-WATCH-1 (fortlaufend)** Nach jedem Push (morgen als erstes): CI-Runs prüfen (`gh run watch` / `gh run list --branch main`), Ergebnis im Tagesstand vermerken. Bei Rot: als Next-Task in `Agents.todo.md` dokumentieren, NICHT still umsetzen (User-Vorgabe). Abnahme: jeder Push hat ein geprüftes CI-Verdict.
- Veröffentlichungsdienste bleiben explizit nie Ziel (kein Task).
- [ ] **[PRIO: niedrig] LRPAR-G14-DENOISE-IMPL-20 (Release: 2.0)** KI-Denoise-Implementierung nach Entscheid `feature/decisions/LRPAR-G14-DENOISE-20.md` (F-078-Fixture-Entscheid ✓, Schema ✓, Pipeline-Stufe + Persistenz + GPU-Refusal ✓, ONNX-Backend ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only; offen: Gewichte). Abnahme: CLI + GUI-headless + Golden/PSNR, kein stiller Fallback.
- [ ] **[PRIO: niedrig] LRPAR-G09-CULL-IMPL-25 (Release: 2.5)** KI-Culling-Implementierung nach Entscheid `feature/decisions/LRPAR-G09-CULL-25.md` (Schema ✓, Heuristik `lumina-cull` ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only; offen: optional ONNX Stufe 2). Abnahme: CLI-Exit-Codes + GUI-headless + kittest, Vorschlag schreibt nie Rating/Flag/Label.

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

Vor F-103-N6 bleibt die nachstehende vollständige GUI-Checkliste offen; die
Review-Befunde (REVIEW-CORE-CROP-1, REVIEW-GUI-DEBOUNCE-1,
REVIEW-GUI-MASKRENDER-1) sind mit Marker-Kommentaren im Code implementiert;
die F-103-N6-Runde 1 hat eigene Befunde erzeugt (GUI-CLICK-ALL-17,
GUI-ROUTING-N6, GUI-INSTRDBG-17 — alle BESTANDEN verifiziert).

- [ ] **[PRIO: hoch] F-103-N6-GUI-COVERAGE-27 (User-Order 2026-09-23; manueller GUI-Gate offen)** Vollständiger manueller/visueller Abnahmelauf für alle noch nicht durch einen normalen `cargo test -p lumina-gui` (ohne `--ignored`) belegten Desktop-GUI-Fälle. Diese Task bleibt offen, bis ein Lauf auf einer Display-/GPU-Maschine mit **genau einer** App-Instanz, `RUST_LOG=trace` und Log-Redirect nach `/tmp/lumina_manual_2026-09-23.log` erfolgt; die headless CI ersetzt diesen Lauf nicht. Jeder Fall wird mit Testanker, Logauszug und Ergebnis dokumentiert.

  **P0 — DnD-/Persistenz-Risiko (`F-103-N6-DND-PERSIST-01`):** nativer Drop einer PNG/JPEG/WebP und einer CR3 in eine leere App; Drop von Quelle B nach geladener Quelle A; Exposure/Contrast-, Spot- und Maskenedit nach Drop; Sidecar adjacent zu B, frischer Neustart und byte-identisches Original; unlesbarer/unsupported Drop mit sichtbarem Fehler und unveränderter vorheriger Quelle. Regressionen müssen insbesondere `dropped_path_uses_open_file_lineage`, `drop_after_loaded_image_never_writes_previous_sidecar`, PNG-/RAW-Reload und `unreadable_drop_preserves_previous_source` abdecken. **Stand 2026-09-23:** Path-first-/Deferred-Decode-Produktionsfix und headless Regressionen sind eingebaut; der echte OS-DnD-Nachweis bleibt offen.

  **1. Start, Eingabe und Shell:** leere App, Ordner-CLI, `--module library|develop|export`, `--fullscreen`, Open/Refresh/Up/Breadcrumb, nativer Datei-/Ordnerdialog, Drag-and-drop, Auto-Load des ersten Bildes, RAW-Orientierung/Metadaten, Status/Banner/Modal-Dialog, Toasts, Neustart und Rendern des tatsächlich geladenen Pfades.

  **2. Responsive Layout und View-Toggles:** 1024×720, 1280×800, schmale/verbreiterte Panels, linke Rail/Navigator, rechte Histogramm-/Develop-Panels, Filmstrip, Toolbar, Lights-out/Fullscreen sowie `Tab`, `Shift+Tab`, `L`, `F`; die Preview-Rechtecke müssen tatsächlich wachsen/schrumpfen, nicht nur State-Flags toggeln. Vorher/Nachher-Goldens und Clip/Overlap-Prüfung sind Pflicht.

  **3. Library-Navigation und Auswahl:** Grid/Loupe/Compare/Survey/People, Ordner-/Unterordnerbaum, Empty-State/CTA, Thumbnail-Größe, Filter, Name/Aufnahmedatum/Custom-Sortierung, Custom-Drag-Insertion, Click/Cmd/Shift-Auswahl, Doppelklick, Home/End/Enter/Esc, Rating/Flag/Color/Badges, Stacks (collapse/expand/select), R6-SCAN-1 erster Paint mit großem echten Verzeichnis.

  **4. Develop-Grundlagen:** alle sichtbaren Basic-Slider (Exposure, Contrast, Highlights, Shadows, Whites, Blacks), Doppelklick-/Alt-Scroll-/Section-Reset, Preset/Profil/Treatment, Auto-Tone, WB-Pipette, Before/After und Split, Original/Softproof/Clipping, Histogramm, Export/Apply/Save/Reset/Match/Regenerate; jede Änderung muss `Edit → Debounce/Commit → Sidecar → Fresh Reopen → Wert/Preview` belegen.

  **5. Farbe, Kurven und Detail:** Tone-Curve-Grafik (Punkt setzen/ziehen/löschen, Kanal Master/R/G/B), HSL/Color-Mixer, Point Color, Color Grading, Presence Texture/Clarity/Dehaze, Vibrance/Saturation, Detail (Schärfen, Rauschreduzierung, Rote-Augen-Picker/Detect/Apply/Remove/Clear), Denoise-Status, Effects (Vignette/Grain), Optics (Profil, Bokeh/Enable), Lens Blur und Generative-Controls inklusive Fehlermodi.

  **6. Geometrie und Preview:** Crop-Handles/Drag/Aspect/Enter/Esc, Straighten, Rotate/Mirror, Perspective/Auto-Upright, Zoom Fit/100–200 %/Fit Width, Pan/Navigator-Drag, Loupe-Vergleich, Draft/Stale/Ready/Failed-Badges, GPU-Present versus CPU-Fallback und sichtbare Route-Gründe.

  **7. Masken (`R5-BRUSH-24`/`R5-MASKVIS-25`):** Mask-Liste mit Anlegen/Mehrfachmasken, Auswahl, Pin-Liste, Sichtbarkeits-Auge, Umbenennen, Löschen, Reihenfolge, Duplizieren, Copy-vs-Duplicate-Gruppen, lokale Layerwerte; Brush-Größe/Weichheit/Fluss, `[`/`]`-Alias, live Kreis-Cursor, Pinsel/Gradient/Radial, Invert/Feather/Blur/Density, Show/Overlay-Farbe, Overlay nur bei geöffneter Masken-Ansicht, Modus „nur Pins aller Masken“ versus „volles Overlay der ausgewählten Maske“ und Panel-Hide mit vergrößerter Preview. Headless-Anker (`brush_marks_roundtrip_through_sidecar`, `g03_*`, `g11_*`, `brush_interaction`, `brush_management_controls_have_headless_structural_action_coverage`) decken Modell, Widget/Layout und echten Preview-Pointer ab; der kombinierte 1024×720-Snapshot bleibt ein explizit ignorierter Native-Adapter-Gate, der unabhängige Metal/Vulkan/DPI-Nachweis bleibt offen.

  **8. Spot-Heal (`R5-DUST-23-FOLLOWUP`):** Toolbar-Heal/Q, Quick/Generativ-Modus, Size/Feather/Opacity, `[`/`]`, Cursor, Klick-Dab, Spot-Auswahl, Liste nur mit ausgewählter Entfernung, Typ/Status, Parameter editieren, Löschen einzelner Spots, Detect/Apply, Distraction-Schalter, Visualize, Regenerate-Variante ohne Clone-Fallback und Reload; fehlende/veraltete/fehlerhafte generative Artefakte müssen sichtbar bleiben.

  **9. Metadaten, Sync und Export:** Keywords, Collections, Smart Collections, IPTC-Draft/Historie/Presets/Sync, Stack-/Batch-Aktionen, Sync Settings/Match Total Exposure/Previous, Merge, Exportziel/Format/Qualität, native Save-Dialog, PNG/JPEG/WebP/RAW-Export, Same-Path-Guard, Metadaten-Bake-In und Exportfehler.

  **10. Persistenz, Fehler und Nebenläufer:** Sidecar-Roundtrip über mindestens zwei virtuelle Kopien, History/Undo/Reset, Source-Changed/Missing/Corrupt/Stale, CAS-Konflikt, echter Zwei-Prozess-Schreibkonflikt, Cache löschen/rebuilden, fehlende Modelle, Panic/Recovery, Abbruch während Debounce sowie Original-Bytes byteweise unverändert.

  **11. Vollständigkeitsnachweis:** alle `ALL_GUI_ACTIONS` (aktuell 110) gegen eine gezeichnete, klickbare Fläche und einen PASS/FAIL-Test mappen; jede neue `GuiAction`/Enum-Variante erzwingt den Audit. Für visuelle Änderungen `egui_kittest`-Golden/PSNR/Histogram plus Vision-Review; für unveränderte Modelle genügt der jeweilige headless Test nicht als manueller GUI-Erfolg.

  **Abnahme:** native PNG/JPEG/WebP/CR3- und DnD-Kette, Sidecar-Datei inspiziert, App neu gestartet, `cargo test -p lumina-gui`, gezielte Regressionstests, `cargo fmt --check`, Clippy mit `-D warnings`, `sh scripts/check_file_sizes.sh` und unabhängiger Verifizierungsbericht mit DoD-§7-Mapping; kein Befund darf als „manuell geprüft“ ohne Log-/Testbeleg gelten.

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
