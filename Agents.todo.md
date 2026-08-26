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
- **MVP-Grenze:** MVP = CLI + native Desktop-GUI inkl. nativem RAW. Web/WASM-RAW
  (via `libraw-wasm`, Feature `wasm-js`), Browser-Dateispeichern, ONNX im
  Browser, Masken-Inferenz im Browser, Cache- und Mehrbild-Synchronisierung
  sind bewusst Post-MVP; WASM ist dokumentierte Capability-Grenze
  (`feature/platform/capability-matrix.md`), keine MVP-GUI. Architektur bleibt
  kompatibel (einheitlicher `decode_bytes`/`RawMetadata`-Vertrag,
  `cfg(target_arch = "wasm32")`-Kapselung).
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
- **CI-Ausnahme `onnx-rt`:** vom Clippy-Lauf ausgeschlossen (zieht `ort` →
  `native-tls`/`openssl-sys`, im gepinnten CI-Container ohne libssl-dev nicht
  baubar); der Pfad wird lokal gelintet.
- **Toolchain:** CI fährt `@stable` → neue Clippy-Lints schlagen automatisch an
  (Beispiel `chunks_exact_to_as_chunks`). Lokal vor jedem Push `rustup update`
  + workspace-clippy laufen lassen.
- **Post-MVP Backlog (nicht MVP-blockierend):** F-019 (siehe Phase 2), Phase 9
  Index (F-064…F-067), WASM-Browser (F-069…F-071), MCP-Erweiterungen (siehe
  F-101-F1), Lensfun-Ausbau (CA via Lensfun, automatische Profil-Erkennung per
  EXIF, WASM-Port), Produktnamen-Entscheidung (`docs/naming-brainstorm.md`,
  Brainstorm-Phase offen bis MVP-Entscheidung).

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

## Offene Tasks — Legende der drei Blöcke

Alle offenen Aufgaben sind in drei Blöcke gegliedert. Innerhalb jedes Blocks
gilt die Sortierung `[PRIO: hoch]` → `[PRIO: mittel]` → `[PRIO: niedrig]`;
die Priorisierung bewertet technische Tragweite/Risiko (kritische
Korrektheits-Bugs = hoch, Kosmetik/Doku = niedrig). Stand 2026-08-26 (R2):
12 offene Tasks — Block A: 10, Block B: 1, Block C: 2.

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

## Phase 3–5: Renderpipeline, RAW, Auto-Tone

Keine offenen Punkte. SOLL: `feature/architecture/pipeline.md` und
`feature/quality/performance-benchmarks.md`.

## Block A – „Vor dem nächsten manuellen GUI/User-Test umsetzbar“

**Dieser Block ist komplett abarbeitbar, ohne dass es einer Rückfrage oder
sonstigen User-Interaktion bedarf, und hängt nicht vom nächsten manuellen
GUI/User-Test ab.**

### PRIO: hoch

**Review-Befunde Full-Repo-Review (2026-08-23) — Hoch (Release-blockierend,
vor MVP zu beheben)**

Erstes vollständiges Review des gesamten bestehenden Codes (alle 10 Crates,
~35k Zeilen, 8 parallele Teil-Reviews). Verifiziert behobene Befunde sind aus
dieser Datei entfernt; Code-Verifikationslauf 2026-08-25 bestätigt den unten
stehenden Restbestand.

**Phase 11: Qualität, Performance und Release**

### PRIO: niedrig

**Review-Befunde Full-Repo-Review (2026-08-23) — Niedrig (Backlog, nicht
MVP-blockierend)**


**Review R2 (2026-08-26) — Bericht: docs/reviews/2026-08-26-full-review.md**
60 Befunde (0 kritisch / 12 hoch / 22 mittel / 26 niedrig). Behobene IDs
werden nach unabhängiger Verifizierung entfernt.




### PRIO: mittel (R2, gebündelt je Crate — Details im Bericht)

- [ ] **[PRIO: mittel] R2-LENS-01-ADOPTION**: Row-Wrapper (geometry_row/
  apply_vignetting_row) sind additiv in lumina-lensfun verfügbar (~48 Mio. → ~16k
  FFI-Übergänge @24MP). Core-Loop umschalten (lumina-core/src/lib.rs ~1112-1127).
  Nicht byteidentisch: erste Spalte bitidentisch, Drift ≤7.4e-4 px @257 Spalten,
  skaliert mit Breite (≈0.75 px @8192). Erfordert Golden-Rebaseline (F-043).
  **Vor manuellem Test** — dokumentierter sub-pixel-Drift, Performance-Gewinn.

### PRIO: niedrig (R2, gebündelt)

- [ ] **[PRIO: niedrig] R2-NIEDRIG-BUNDLE**: RAW-03 (toter name-Parameter der
  Decode-API), PRESETS-FAIL-CLOSED (recipe_scope_violation fail-open bei
  Serialisierungsfehler — praktisch unerreichbar, optional fail-closed),
  GPU-WB-GUI-GATE (gui gpu_present_if_ready sieht keinen Kontext-WB — recipe-only;
  Restbefund aus R2-MCP-01), SIGTRAP-GPU-TESTS (--features gpu cli/mcp:
  Rayon-Worker droppen thread_local GPU_CTX beim Exit, Signal 5 trotz grüner
  Tests; vorbestehend), MCP-06 (downscale_bilinear gehört nach core),
  SIDECAR-ZDATA-WASM (zstd-sys blockiert workspace-weites wasm32-Gate —
  Capability-Entscheidung nötig).

**Phase 2: Rezept, virtuelle Kopien und Migrationen**

- [ ] **[PRIO: niedrig] F-019** (deferriert auf Post-MVP) CLI `migrate_sidecar`
  (crates/lumina-cli/src/main.rs) auf `lumina_sidecar::migrate_sidecar_file`
  umstellen (`.bak`-Backup + Lock); erst nach MVP relevant, da bis dahin keine
  Migrationen laufen. Verifikations-Hinweis: Library-Teil ist verifiziert.
  Ist-Stand 2026-08-25: CLI nutzt lokal `migrate_json` + `write_atomically`
  ohne `.bak`/Lock (`--migrate`-Flag in import/develop/render/export/validate).

**Phase 6: Persistente AI-Masken**

- [ ] **[PRIO: niedrig] F-082-FOLLOWUP** (nicht MVP-blockierend): echter ORT-Pfad hinter
  `onnx-rt`, MaskGraph/CLI-Einbindung, hash-gepinnte ONNX-Fixtures.

**Phase 9: Optionale zentrale Indizierung (Post-MVP)**

- [ ] **[PRIO: niedrig] F-064** Minimalen, vollständig wiederaufbaubaren Indexumfang festlegen:
  Pfad, Quellhash, Metadaten, Sidecarstatus, Jobstatus und Cacheverweise.
- [ ] **[PRIO: niedrig] F-065** SQLite-Index als optionalen Adapter implementieren, ohne
  Rezeptdaten nur dort zu speichern.
- [ ] **[PRIO: niedrig] F-066** Rebuild aus Sidecars, Aktualisierung, Locking und beschädigte
  DB behandeln.
- [ ] **[PRIO: niedrig] F-067** Nachweisen, dass Löschen der DB keine Bearbeitungsdaten,
  virtuellen Kopien oder Masken zerstört.

Ist-Stand 2026-08-25: kein Index-Modul im Workspace; CLI-`reindex` ist nur ein
Sidecar-Scan (zählt valide Sidecars, persistiert nichts).

**Phase 10: WASM und Plattformen (Post-MVP)**

- [ ] **[PRIO: niedrig] F-069** Browser-Dateiimport, temporären Speicher und Exportmodell
  definieren.
- [ ] **[PRIO: niedrig] F-070** ONNX im Browser als optionale Fähigkeit mit klarer
  Capability-Anzeige behandeln.
- [ ] **[PRIO: niedrig] F-071** native, Desktop- und Browser-Limits für Bildgröße, Speicher,
  Threads und GPU dokumentieren.

Ist-Stand 2026-08-25: Capability-Matrix existiert (`feature/platform/capability-
matrix.md`, qualitativ), aber Browser-Dateiimport/ONNX nicht implementiert;
quantitative Limits (Bildgröße/Speicher/Threads/GPU) nirgends dokumentiert.

**Offene Punkte aus manuellem Test / GPU-Follow-ups (Fortsetzung)**

- [ ] **[PRIO: niedrig] GEN-EXPAND-1** Optionaler generativer Modus „Entfernen + Erweitern“:
      Objekte entfernen (inpainting) **und** das Bild über die ursprüngliche
      Bildfläche hinaus erweitern (outpainting/canvas expansion > 100 %).
      **Nur dokumentiert, Implementierung noch nicht begonnen** (Repo-weit
      verifiziert 2026-08-25: kein Code, keine `GenerativeEdit`-Stufe).
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

**USER-ENTSCHEIDUNGEN 2026-08-25 (aus Block B freigegeben)**

## Block B – „Offene Rückfragen“

Tasks, bei denen eine User-Entscheidung/Klärung fehlt (Produkt-, Naming-,
Lizenz-/Schema- oder Übernahme-Fragen). Blockiert Block A nicht; sollte aber,
wo möglich, vor dem nächsten manuellen GUI-Test geklärt werden.

### PRIO: mittel

**Produktname (Rest von F-101-F1)**

- [ ] **[PRIO: mittel] NAMING-F1** Produktname final entscheiden
  (`docs/naming-brainstorm.md`). **User-Entscheidung 2026-08-25:** Brainstorm
  läuft bewusst weiter, Naming bleibt offen. Die übrigen F-101-F1-Anteile
  (MCP-Scope) wurden zur Umsetzung freigegeben und stehen in Block A.

**Arbeitsbaumänderungen während des Reviews (in Arbeit)**



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

- [ ] **[PRIO: mittel] R2-GUIMOD-04** (nach manuellem Test): CPU-Draft läuft auf
  GPU-Pfaden redundant mit. Drosselung ist Verhaltensentscheidung — erst nach
  Test entscheiden, ob der Draft-Throttle nötig ist.

- [ ] **[PRIO: hoch] F-103-N6** Erster visueller User-Test: `cargo run -p lumina-gui` mit
  PNG/JPEG/WebP + nativen RAW per Pfad und Drag&drop; Preview + Exposure/
  Contrast ändern den Renderstand; Sidecar wird geschrieben und beim Neustart
  wiederhergestellt; WASM (`trunk serve`/`trunk build --release`) bleibt
  buildbar und weist RAW als nicht verfügbare Capability aus. Abnahme:
  reproduzierbare Befehle aus cli-gui-wasm.md; unabhängiger Verifizierungs-
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
