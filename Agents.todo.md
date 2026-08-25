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
Korrektheits-Bugs = hoch, Kosmetik/Doku = niedrig). Stand 2026-08-25:
84 offene Tasks — Block A: 80, Block B: 3, Block C: 1.

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




### PRIO: mittel

**Review-Befunde Full-Repo-Review (2026-08-23) — Mittel**




- [ ] **[PRIO: mittel] REVIEW-CLI-MASKFLAG-1** `update_masks`/`force_render` bleiben
  ewig im Rezept → permanente Re-Inferenz trotz gültiger Maske
  (mittel; lumina-cli/src/main.rs:443, 901, 1219, 1348; Verstoß gegen
  Persistenz-Invariante). Fix: Option nach Konsum aus dem persistierten
  Rezept entfernen. Re-Check 2026-08-25: Flags werden weiterhin in
  recipe-extras geschrieben (develop/batch) und nie konsumiert entfernt.
- [ ] **[PRIO: mittel] REVIEW-CLI-EXPORTMASK-1** `export --update-masks` bricht bei
  stale Masks ab, batch/develop laufen weiter; Flag wird für Export gar
  nicht durchgereicht (mittel). Teilstand 2026-08-25: Flag wird für
  Export jetzt über `preflight_masks(..., args.update_masks)` gereicht
  (mit Test); die Inkonsistenz zwischen den Subkommanden bleibt.
- [ ] **[PRIO: mittel] REVIEW-CLI-WRITE-1** Overwrite-Guards decken weder Sidecar-/
  zdata-Ziele noch Hardlinks (Inode-Identität) ab (mittel;
  main.rs:1067, 1544; lumina-mcp/src/tools/save.rs:33–44). Fix:
  Zielpfade gegen `<input>.lumina.json/.zdata` prüfen + (dev,inode)-Vergleich.
  Re-Check 2026-08-25: nur ein `paths_resolve_equal`-Guard (Export vs.
  Quelle); keine Inode-Prüfung; MCP-save prüft nur kanonische Quelle.
- [ ] **[PRIO: mittel] REVIEW-CLI-BATCHCOLLIDE-1** Batch schreibt alle Inputs
  namensbasiert in ein Zielverzeichnis → Kollisionen überschreiben
  still, beide melden „ok" (mittel; main.rs:874–881). Fix: Dubletten
  vorab ablehnen oder Struktur spiegeln. Re-Check 2026-08-25:
  unverändert (`args.output.join(name)` ohne Dubletten-Prüfung).

- [ ] **[PRIO: mittel] REVIEW-GUI-MASKGEO-1** Masken-Pinsel/Verlauf/Radial (und
  WB-Pipette) ignorieren Crop/Rotation/Mirror des Rezepts → Markierungen
  landen transformiert-falsch (mittel; lib.rs:2589–2669). Fix: inverse
  Geometrie in `to_normalized` einrechnen oder Werkzeuge bei aktiver
  Geometrie deaktivieren. Re-Check 2026-08-25: `to_normalized` mappt nur
  ROI→Volllösung, keine inverse Rezept-Geometrie.
- [ ] **[PRIO: mittel] REVIEW-GUI-SAVEMSG-1** Nach fehlgeschlagenem `save_sidecar`
  steht trotzdem „Sidecar saved" im Status (mittel; lib.rs:2222–2233).
  Re-Check 2026-08-25: unverändert — `save_sidecar()` setzt am Funktions-
  ende bedingungslos `Str::SidecarSaved`, auch nach dem Err-Zweig.
- [ ] **[PRIO: mittel] REVIEW-GUI-VCSWITCH-1** Virtual-Copy-Wechsel verwirft ungespeicherte
  Edits ohne Rückfrage; `history_selected`/Drag-State werden nicht
  zurückgesetzt, Fehler wird verschluckt (mittel; lib.rs:909–925, 4240).
  Re-Check 2026-08-25: `select_virtual_copy` überschreibt Rezept ohne
  Rückfrage/Reset.
- [ ] **[PRIO: mittel] REVIEW-GUI-CURVE-1** Tone-Curve-Roundtrip clamppt Shadows-
  Slider auf 0 → Regler springt sichtbar zurück (−50 % → −33 %→0)
  (mittel; lib.rs:2981–3003). Fix: Deltas speichern statt geclampte
  Outputs, oder UI-Hinweis. Re-Check 2026-08-25: unverändert — Outputs
  werden in `build_tone_curve` auf [0,1] geclampt (dokumentierte
  MVP-Vereinfachung), Readback leitet Deltas aus geclampten Outputs ab.
- [ ] **[PRIO: mittel] REVIEW-GUI-DEBOUNCE-1** Debounced Vollrender kann stranden:
  im Wartefenster (< 150 ms) wird weder gerendert noch ein getaktetes
  Repaint angefordert → Draft-Vorschau bleibt bis zur nächsten Eingabe
  (mittel; lib.rs:4869–4889). Fix: `request_repaint_after` im
  Warte-Zweig. Re-Check 2026-08-25: kein `request_repaint_after` in
  lib.rs; Warte-Zweig plant kein Repaint.
- [ ] **[PRIO: mittel] REVIEW-GUI-MASKRENDER-1** Masken-Layer-Edits (Invert/Feather/
  Blur/Density) setzen nur `render_key = None`, planen aber kein
  Render → Vorschau bleibt dauerhaft alt (mittel; lib.rs:1077–1092,
  1193–1211). Fix: über `mark_dirty()` routen. Re-Check 2026-08-25:
  Setter nutzen weiterhin nur `self.render_key = None`.
- [ ] **[PRIO: mittel] REVIEW-RAW-FLIP-1** `sizes.flip` (dcraw-Bitmaske) wird 1:1 als
  EXIF-Orientation persistiert — falsche Codewelt (z. B. flip=5 ist
  EXIF 8, nicht 5); Portrait-Fixture persistiert nachweislich falsch
  (mittel; lumina-raw/src/lib.rs:280–283). Fix: explizit übersetzen
  oder Rohwert unter eigenem Namen führen. Re-Check 2026-08-25:
  unverändert (`1..=8 => data.sizes.flip as u8`).


**Performance-Verfeinerung — Lightroom-artige interaktive Geschwindigkeit**

Ausgangslage (Nutzerbericht): Die GUI decodiert/rendert synchron im Main-
Thread; Dateiwechsel und Regler-Drags müssen flüssig werden. Die verbleibenden
Ticks adressieren die interaktive GUI-Latenz (die Batch-/Kernel-Ebene
F-074-A1…A4 ist abgeschlossen). Verifiziert wird gegen
`feature/architecture/pipeline.md`, `feature/quality/performance-benchmarks.md`,
`feature/platform/cli-gui-wasm.md` sowie `crates/lumina-gui/src/lib.rs`.
Draft/Full-Split, Coalescing, CPU-ROI, Async-Decode und Auto-Load sind
implementiert und aus dieser Liste entfernt; offen:

- [ ] **[PRIO: mittel] PERF-GUI-1** DAG/Schritt-Trennung + demosaizierte Basiskachel cachen,
      nur Color/Tone bei Exposure-Änderung invalidieren.
  - **Teilstand 2026-08-25:** `render_draft`/`render_full`/`render_from` mit
    ROI-Crop vorhanden; `draft_original` gecacht (Zero-Alloc beim Drag).
    **Aber:** kein Stufen-Cache; die GUI nullt bei jeder Regleränderung den
    gesamten `render_key` (`set_adjustment`/`mark_dirty`) statt nur der
    Color/Tone-Stufe; `stage_digest` wird dafür nicht genutzt.
  - **Verfeinerung nötig:** Demosaiced-Basis als `ImageFrame` im RAM halten
    und stufenweise invalidieren, sodass Exposure-/Color-Änderungen nur die
    Adjustments-Stufe neu rendern. GPU/VRAM-Variante langfristig via ADR
    (`lumina-core` darf laut `Agents.md` keine GPU-/GUI-Abhängigkeit erhalten).
  - **Abnahmekriterien:** Bei Exposure-Drag wird nur die Adjustments-Stufe neu
    berechnet (nachweisbar über `render_frame`-Stufen-Timing + Golden-
    Identität zu F-043); Decode/Demosaic wird bei Regleränderung **nicht**
    wiederholt; kein stillschweigender Fallback bei Cache-Miss.

- [ ] **[PRIO: mittel] PERF-GUI-2** GPU-Pfad ausbauen (Uniforms/Stages, kein CPU-Readback im
      Present-Pfad).
  - **Teilstand 2026-08-25:** `crates/lumina-gpu` existiert (wgpu/Metal,
    `GpuContext`, Uniforms, Tone+WB-Stage als inline WGSL gegen CPU-Golden-
    Gates getestet, Mask-Overlay-Shader, `render_to_vram` ohne `map_async`).
    **Aber:** keine dedizierten Masken-/SourceAction-Stufen auf GPU
    (SourceActions steht auf der CPU-Route-Liste); VRAM-Output ist offscreen-
    only — das On-Screen-Present läuft über CPU-Upload (`ColorImage`/
    `load_texture`) im glow-Renderer; Vollbild-Pfad liest per `map_async`
    zurück.
  - **Verfeinerung nötig:** Masken-/SourceAction-Stufen auf GPU; egui_wgpu-
    Migration bzw. Readback-freier Present-Pfad; CPU bleibt Referenz.
  - **Abnahmekriterien:** GPU-Pfad liefert wert-identisches Ergebnis zum
    CPU-Pfad (F-043-Toleranzen); aktivierbar ohne `lumina-core`-API-Bruch;
    WASM-Build bleibt ohne GPU-Capability grün.

**Offene Punkte aus manuellem Test / GPU-Follow-ups**

- [ ] **[PRIO: mittel] GPU-STAGE-1** Masken-/WB-/SourceAction-Stufen auf GPU (derzeit nur
      Tone(+WB)-Stage und Mask-Overlay; CPU bleibt Referenz). Nach GUI-60FPS-1.
      Restrisiko bis dahin dokumentiert: VRAM-Vorschau rendert Unsupported-
      Rezepte tone-only (mit Warnung).
- [ ] **[PRIO: mittel] GUI-WGPU-PRESENT-1** Follow-up aus GUI-60FPS-1-Verifizierung:
      `egui_wgpu`-Migration bzw. Upload-Pfad finalisieren (derzeit Present
      unter glow CPU-seitig via `ColorImage`/`load_texture`; <16 ms gilt für
      Masken-Tile-Upload, nicht Preview-Present; `copy_vram_to_texture` ist
      offscreen-only). `VramState` LRU/Pool (45 MP+) fehlt (single-slot);
      `warn!` bei `GpuContext::new` Adapter-Fehler fehlt (Fehler wird
      verschluckt). Dokumentiert in `docs/gpu-bootstrap.md` (Dual-Backend
      glow vs wgpu).

**Phase 11: Qualität, Performance und Release**



### PRIO: niedrig

**Review-Befunde Full-Repo-Review (2026-08-23) — Niedrig (Backlog, nicht
MVP-blockierend)**


- [ ] **[PRIO: niedrig] REVIEW-CORE-WASM-FOLLOWUP** `cargo check -p lumina-core --target wasm32-unknown-unknown --all-targets` scheitert an Dev-Dependencies (wait-timeout/getrandom), identisch an HEAD — der dokumentierte lib-only-Capability-Gate ist grün; Dev-Deps für wasm32 cfg-gaten oder `--all-targets` offiziell als native-only dokumentieren.
- [ ] **[PRIO: niedrig] REVIEW-CLI-N1** CLI lädt zdata-Tiles nur per `mask.id` statt
  `(copy_id, mask_id)` → Kopien mit gleichen Masken-IDs teilen Matte
  (main.rs:1186). Re-Check 2026-08-25: Speicher-Key weiterhin
  `container.tile(&mask.id, …)`; auch GUI betroffen.
- [ ] **[PRIO: niedrig] REVIEW-CLI-N2** dust_removal hängt Artefakt an, bevor Sidecar/
  Copy validiert sind → orphaned Bundles bei Fehlern (main.rs:674).
  Re-Check 2026-08-25: `append_repair_region` läuft vor `load_sidecar`/
  Copy-Validierung.
- [ ] **[PRIO: niedrig] REVIEW-CLI-N3** Batch-Resume per Substring-Match auf
  Statusdatei (main.rs:885). Fix: JSON parsen. Re-Check 2026-08-25:
  `state.contains("\"status\":\"ok\"")`.
- [ ] **[PRIO: niedrig] REVIEW-CLI-N4** reindex ignoriert korrupte Sidecars still, Exit 0
  (main.rs:811). Re-Check 2026-08-25: unverändert (zählt valide, meldet
  immer „ok").
- [ ] **[PRIO: niedrig] REVIEW-CLI-N5** collect_images folgt Symlink-Loops ohne Schutz
  → Stack Overflow (main.rs:947). Re-Check 2026-08-25: rekursiv ohne
  Visited-Set/Symlink-Filter.
- [ ] **[PRIO: niedrig] REVIEW-CLI-N6** Export geschrieben bevor Sidecar-Update; Fehler
  → Exit 1 trotz existierendem Export (main.rs:1346). Re-Check
  2026-08-25: unverändert (als v1-Umfang in sidecar lib.rs dokumentiert).
- [ ] **[PRIO: niedrig] REVIEW-CLI-N7** import akzeptiert geänderte Quelle gegen
  bestehendes Sidecar ohne Warnung (main.rs:400). Re-Check 2026-08-25:
  import_file prüft keinen Content-Hash (im Gegensatz zu process_selected).


- [ ] **[PRIO: niedrig] REVIEW-GUI-N1** Save berechnet Fingerprint neu und löscht
  Konfliktstatus still; GUI nutzt CAS (`save_sidecar_if_unchanged`)
  nicht (lib.rs:2208–2224). Re-Check 2026-08-25: GUI ruft Plain-
  `save_sidecar`, überschreibt `document.source` neu.
- [ ] **[PRIO: niedrig] REVIEW-GUI-N2** `finish_decode` stellt Rezept aus
  `virtual_copies[0]` (positionell) wieder her, während
  `virtual_copy_id` auf `"vc-original"` fixiert wird — verstoßt gegen
  die ID-stabil-Regel bei umsortierten Sidecars (lib.rs:2128/2145,
  1553). Fix: Copy per id/is_default suchen. Re-Check 2026-08-25:
  unverändert (`virtual_copies[0].recipe.clone()`).
- [ ] **[PRIO: niedrig] REVIEW-GUI-N3** Dateiwechsel resettet Zoom/Pan/BeforeAfter/
  WB-Pipette/History-Auswahl nicht → Bild B öffnet im 8×-Crop von
  Bild A (lib.rs:1536–1567). Re-Check 2026-08-25: `apply_decoded_frame`
  setzt Zoom/Pan/before_after/history_selected nicht zurück.
- [ ] **[PRIO: niedrig] REVIEW-GUI-N4** `IdleQueue::pop_next` ist LIFO statt
  dokumentiertem FIFO bei gleichen Prioritäten (`max_by_key` wählt
  letztes Maximum) (lib.rs:184–194). Re-Check 2026-08-25: unverändert.
- [ ] **[PRIO: niedrig] REVIEW-GUI-N5** `preview_is_draft` ist write-only: Histogramm/
  Exposure-Matching messen still Drafts; Flag konsumieren oder Feld
  entfernen (lib.rs:350, 1559, 1691, 1729). Re-Check 2026-08-25:
  weiterhin nur Writes.
- [ ] **[PRIO: niedrig] REVIEW-GUI-N6** Fehlgeschlagener ROI-Crop fällt still auf
  Vollbild zurück, `preview_roi` wird aber trotzdem gesetzt (latent;
  lib.rs:1815–1822). Re-Check 2026-08-25: `crop_region(...).ok()`-
  Fallback + bedingungsloses `self.preview_roi = roi`.
- [ ] **[PRIO: niedrig] REVIEW-RAW-N1** Returncode von `libraw_adjust_sizes_info_only`
  geschluckt — Budget-Gate könnte auf veralteten Maßen basieren
  (lumina-raw/src/lib.rs:335). Re-Check 2026-08-25: `let _ = unsafe {...}`.
- [ ] **[PRIO: niedrig] REVIEW-RAW-N2** `metadata.lens` ist immer `None`, obwohl Feld
  existiert und Lensfun-Integration ihn braucht (lib.rs:307). Befüllen
  oder Feld entfernen. Re-Check 2026-08-25: unverändert (`lens: None`).
- [ ] **[PRIO: niedrig] F-082-FOLLOWUP-ORT** `OrtBackend` panickt bei
  unbekanntem Output-Namen (`outputs[output_name]` unwrap) statt
  `OnnxError::InferenceFailed`; mit echten Gewichten fixen (F-082).
- [ ] **[PRIO: niedrig] F-082-FOLLOWUP-HASH** ORT-Mismatch-Refuse-Zweig
  (`ModelArtifactStale`) ohne ausführbaren Test (benötigt ladbares
  `.onnx` mit abweichendem Pin); hash-gepinnte ONNX-Fixture erfassen.
- [ ] **[PRIO: niedrig] REVIEW-GUI-STATUS-FOLLOWUP** `lumina-gui/src/lib.rs`
  prüft Artefaktstatus nur auf `== Missing`; auf `!= Available` umstellen,
  damit `Corrupt` (REVIEW-SIDECAR-STATUS-1) korrekt behandelt wird.
- [ ] **[PRIO: niedrig] REVIEW-CORE-DIGEST-WIRING** RenderKey-Digest-Fixes
  (ExportOptions/SourceAction-Hashes) in CLI/GUI/MCP beim Bau der
  RenderKeys via `with_export_options`/`with_source_action_hashes`
  verdrahten, damit Cache-Hits korrekte Qualität/Repair-Pixels liefern
  (Core-Seite erledigt).
- [ ] **[PRIO: niedrig] REVIEW-CLI-FOLLOWUP-1** `collect_sidecars` hat weiterhin
  keinen Symlink-/Loop-Schutz (N5 deckte nur `collect_images` ab).
- [ ] **[PRIO: niedrig] REVIEW-MCP-DOCS** `feature/platform/mcp-server.md` um
  neue Verhalten ergänzen: SidecarConflict/-32010, isError-Result,
  Extension-Gate, strenge Quality-Bounds, Identity-Check bei Load.
- [ ] **[PRIO: niedrig] REVIEW-SIDECAR-FOLLOWUP-1** `artifact_status` erkennt
  <8-Byte-Container nicht als `Corrupt` (nur Magic-Parsing); bei
  `reference.format=="zdata"` ohne gültige Magic → `Corrupt`.
- [ ] **[PRIO: niedrig] REVIEW-SIDECAR-FOLLOWUP-2** `artifact_status` validiert
  Reference width/height nicht gegen Bundle-Records (dokumentierte Lücke).

- [ ] **[PRIO: niedrig] REVIEW-GUI-WASM-FOLLOWUP** wasm32-Check der GUI erzeugt ~20
  Warnungen (u. a. dead_code `prefetch_order`) — ohne `-D warnings`;
  sauber cfg-gaten (Folgabe aus REVIEW-GUI-WASM-1; betrifft auch die
  benigne `load_mask_planes`-Notiz aus F-072).

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

- [ ] **[PRIO: niedrig] BENCH-BASELINE-1** Baseline-Capture 6 GPU-Benchmark-IDs
      `perf/baseline.json` → `gate:true`. Ist-Stand 2026-08-25:
      `perf/baseline.json` existiert (Core/Batch), `perf/budgets.json` hat
      Core/Batch `gate:true`; GPU-Einträge bewusst `gate:false` (report-only,
      „until the GPU path stabilises"). criterion-0.8-Ergebnisformate beim
      Capture beachten.
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

- [ ] **[PRIO: mittel] F-103-N10** Sektionsreihenfolge „Detail" vor „Effects"
  — **USER-ENTSCHEIDUNG 2026-08-25: LR-Classic-konform.** SOLL
  (`feature/cli-gui-wasm.md`, F-100) korrigieren („Detail" = Sharpening/Noise
  Reduction VOR „Effects" = Vignette/Grain) und GUI-Anordnung angleichen;
  eine Abweichungs-Dokumentation entfällt damit.
- [ ] **[PRIO: mittel] F-101-F1** Erweiterter MCP-Scope umsetzen —
  **USER-ENTSCHEIDUNG 2026-08-25: jetzt angehen.** Volle CLI-Abdeckung als
  MCP-Tools (`lumina_import`, `lumina_batch`, `lumina_reindex`,
  `lumina_dust_removal` u. a.), `lumina mcp` als CLI-Subcommand,
  Vision-fähiger Agent (strukturierte `lumina_analyze`-Daten für Agents ohne
  Vision). Konzepte/SOLL: `feature/platform/mcp-server.md` (Abschnitt
  „Erweiterter MVP-Scope"). Produktnaming bleibt bewusst offen (NAMING-F1 in
  Block B).

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
