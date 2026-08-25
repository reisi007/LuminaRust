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

- [ ] **[PRIO: mittel] REVIEW-SIDECAR-TMP-1** `recover_sidecar` löscht Temp-Dateien
  lebender Writer (mittel; lib.rs:1109–1130). Fix: mtime-Schwelle oder
  Sweep unter Lock. Re-Check 2026-08-25: unverändert.
- [ ] **[PRIO: mittel] REVIEW-SIDECAR-CAS-1** CAS (`save_sidecar_if_unchanged`) nicht
  gegen Plain-`save_sidecar` serialisiert (mittel; lib.rs:1075–1100).
  Fix: alle Writes über Lock oder Vertrag dokumentieren. Re-Check
  2026-08-25: Plain-`save_sidecar` nimmt weiterhin keinen Lock.
- [ ] **[PRIO: mittel] REVIEW-SIDECAR-ZDATA-1** zdata Read-Modify-Write ohne Lock →
  verlorene Repair-Regionen/Dangling Refs (mittel;
  zdata.rs:688–699). Fix: `.zdata.lock` + Checksum-Verifikation beim
  Laden. Re-Check 2026-08-25: unverändert — `append_repair_region`
  lädt/speichert ohne Lock; BLAKE3 wird erst lazy beim Tile-Zugriff
  geprüft, nicht beim Laden.
- [ ] **[PRIO: mittel] REVIEW-SIDECAR-STATUS-1** `artifact_status` prüft nur
  `is_file()`, keine Checksum/Format/Auflösung (mittel;
  lib.rs:1187–1193) → korrupte Artefakte gelten als Available.
  Re-Check 2026-08-25: unverändert.
- [ ] **[PRIO: mittel] REVIEW-CORE-CROP-1** `crop_rect`: u32-Underflow/Empty-Crop durch
  1e-6-Toleranz (mittel; lib.rs:1227–1246). Fix: x/y ≤ 1 explizit,
  saturating_sub, pw/ph == 0 als Fehler. Re-Check 2026-08-25: Validierung
  ergänzt (finite/w>0/h>0/x,y≥0/x+w≤1+1e-6), aber weiterhin KEIN explizites
  x≤1, kein saturating_sub (`min(width - px)` kann bei px>width unterlaufen)
  und pw/ph==0 ist kein Fehler → Restrisiko im Toleranzfenster bleibt.
- [ ] **[PRIO: mittel] REVIEW-CORE-EXPORTKEY-1** ExportOptions (quality/dither/seed/
  bit_depth) fehlen in OutputSpec/RenderKey-Identität (mittel;
  pipeline.rs:88–107) → Cache-Hits liefern falsche Qualität. Fix:
  volle ExportOptions in den Digest. Re-Check 2026-08-25: OutputSpec =
  {profile,width,height,format}; ExportOptions weiterhin nicht im Digest.
- [ ] **[PRIO: mittel] REVIEW-CORE-SRCACC-1** `source_actions` in keinem Cache-Digest
  (mittel; cache.rs:162–210) → geänderte Repair-Artefakte servieren
  alte Pixels. Fix: Artifact-Checksummen in RenderKey. Re-Check
  2026-08-25: unverändert.
- [ ] **[PRIO: mittel] REVIEW-CORE-DECODE-1** `ImageFrame::decode` unbegrenzte
  Allokation, kein MemoryBudget (mittel; lib.rs:255–260). Fix:
  Dimensionen vorab prüfen + `check_decode`. Re-Check 2026-08-25:
  unverändert.
- [ ] **[PRIO: mittel] REVIEW-MASK-STRICT-1** MaskPolicy::Strict wird nirgends
  verwendet (CLI setzt immer Warn) (mittel). Fix: Strict-Pfad ehrlich
  verdrahten oder Policy entfernen. Re-Check 2026-08-25: CLI/GUI/MCP
  konstruieren ausschließlich `MaskPolicy::Warn`.
- [ ] **[PRIO: mittel] REVIEW-MASK-ZERO-1** `rasterize_prompt` panickt bei Breite 0
  (`chunks_exact_mut(0)`) statt `MaskError` (mittel; masks.rs:269 ff.);
  Sidecar-Validierung prüft width/height ≠ 0 nicht. Fix: Guard +
  Validierung. Re-Check 2026-08-25: Ellipse/Polygon/Gradient-Zweige
  nutzen weiterhin `chunks_exact_mut(w)`; `check_mask(0,0)` passt und
  `MaskPlane::new` erlaubt 0×0.
- [ ] **[PRIO: mittel] REVIEW-MASK-BLUR-1** Feathering/Blur O(w·h·radius) — bei
  feather ≈ 1.0 Minuten pro Render/Export (mittel;
  mask_modulation.rs:61–96). Fix: Sliding-Window-Box-Blur O(w·h),
  byte-identisch. Re-Check 2026-08-25: separabler Box-Blur, aber innere
  Schleife summiert pro Pixel über den Radius → weiterhin O(w·h·radius).

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
- [ ] **[PRIO: mittel] REVIEW-MCP-QUALITY-1** `quality as u8` trunciert ohne
  Validierung (256→0) (mittel; save.rs:47–52; analog preview.rs:31).
  Fix: 1..=100 serverseitig erzwingen. Re-Check 2026-08-25: unverändert
  (`as_u64().map(|v| v as u8)`).
- [ ] **[PRIO: mittel] REVIEW-MCP-SAVE-1** `lumina_save` nutzt `fs::write` statt
  atomarem Write; Format/Extension ungeprüft (mittel; save.rs:66–68).
  Re-Check 2026-08-25: unverändert.
- [ ] **[PRIO: mittel] REVIEW-MCP-SESSION-1** Ganzes, evtl. veraltetes In-Memory-
  Document wird zurückgeschrieben → Lost Update; Load prüft
  content_hash nicht (mittel; session.rs:36–59, edit.rs, load.rs:37).
  Fix: `save_sidecar_if_unchanged` + Quell-Identitätsprüfung wie CLI.
  Re-Check 2026-08-25: MCP persistiert via Plain-`save_sidecar`
  (edit.rs); die CAS-API ist vorhanden, wird aber von MCP nicht genutzt.
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
- [ ] **[PRIO: mittel] REVIEW-ONNX-AVAIL-1** `<StubBackend as SubjectInference>::infer`
  ignoriert `self.available` — „fehlendes" Modell liefert still Matte
  (mittel; lumina-onnx/src/backend.rs:114–143 vs. SAM-Gate sam2.rs:277).
  Teilstand 2026-08-25: Decision-Layer (mask_loader) gated Re-Inferenz
  auf `backend.is_available()`, SAM2-Stub enforced es; der geprüfte
  StubBackend-Pfad selbst ist unverändert.
- [ ] **[PRIO: mittel] REVIEW-ONNX-HASH-1** Modell-Hash wird nie gegen Manifest
  geprüft (`path.exists()` genügt; Platzhalter „pending-integration")
  → getauschte Gewichte laufen unter alter Identität in Masken-Sidecars
  (mittel; ort_backend.rs:34–53). Fix: Hash beim Laden berechnen und
  auf Mismatch stale-markieren. Re-Check 2026-08-25: unverändert.
- [ ] **[PRIO: mittel] REVIEW-ONNX-PREPROC-1** ORT-Preprocessing nur [0,1] statt
  ImageNet mean/std, Tensor-Namen hartcodiert statt aus Manifest,
  Output-Shape ungeprüft (Fehler meldet dann 0/0-Dims) (mittel;
  ort_backend.rs:66–95). Fix vor Integration echter Gewichte.
  Re-Check 2026-08-25: unverändert.

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

- [ ] **[PRIO: niedrig] REVIEW-SIDECAR-N1** Migration-Tempfile nutzt Crate-Default-Prefix
  statt `.{name}.tmp-` → Recover-Sweep räumt nie auf (lib.rs:1241).
  Re-Check 2026-08-25: `atomic_write_bytes` nutzt
  `NamedTempFile::new_in` (Default-Prefix).
- [ ] **[PRIO: niedrig] REVIEW-SIDECAR-N2** schema_version 0 wird in `from_json` still
  zu 1 normalisiert, divergiert vom Migrationspfad (lib.rs:1311).
  Re-Check 2026-08-25: unverändert (Bewusstsein durch Pre-Alpha-Entscheid
  geschärft, Code gleich).
- [ ] **[PRIO: niedrig] REVIEW-SIDECAR-N3** Unbekannte Adjustment-Keys und
  MaskLayer.feather/blur/density sowie target_luminance ohne
  Finite/Range-Validierung (lib.rs:1724, 320, 827). Re-Check
  2026-08-25: unbekannte Keys werden via `_ => continue` übersprungen;
  keine Range-Checks für feather/blur/density/target_luminance.
- [ ] **[PRIO: niedrig] REVIEW-SIDECAR-N4** `delete_virtual_copy` mutiert vor
  `validate()` → bei Fehler bleibt Rechenliste inkonsistent hängen
  (lib.rs:1373). Re-Check 2026-08-25: unverändert.
- [ ] **[PRIO: niedrig] REVIEW-SIDECAR-N5** `load_sidecar` liest Datei komplett vor
  Größenlimit (read_to_string) — `load_zdata` macht es richtig
  (lib.rs:979). Re-Check 2026-08-25: unverändert.
- [ ] **[PRIO: niedrig] REVIEW-CORE-N1** Histogram-Stufen-Digest ohne OutputSpec
  (pipeline.rs:160; latent, bis Vorschau-Histogramme gecacht werden).
  Re-Check 2026-08-25: Digest nimmt `output` nur unter Scope „render".
- [ ] **[PRIO: niedrig] REVIEW-CORE-N2** `cdf_at(NaN)` gibt NaN zurück statt Fehler
  (histogram.rs:155). Re-Check 2026-08-25: `clamp` propagiert NaN.
- [ ] **[PRIO: niedrig] REVIEW-CORE-N3** `AutoToneConfig.epsilon` bis 1.0 zulässig →
  +10 EV auf fast jedem Bild; Zweige überlappen > 0.5 (tone.rs:36).
  Re-Check 2026-08-25: validate erlaubt explizit „at most one".
- [ ] **[PRIO: niedrig] REVIEW-MASK-N1** MaskGraph ohne Memoization → handcrafted DAGs
  exponentiell (masks.rs:95–203). Re-Check 2026-08-25: unverändert.
- [ ] **[PRIO: niedrig] REVIEW-MASK-N2** Density < 0 löscht Maske still, > 1 wirkungslos
  (mask_modulation.rs:40). Re-Check 2026-08-25: Kern-Seite unvalidiert
  (die GUI validiert 0..=1 lokal, Sidecar/Kern nicht).
- [ ] **[PRIO: niedrig] REVIEW-MASK-N3** `model_identity_matches(None) => true` segnet
  fremdmodellierte Artefakte als valide ab (mask_loader.rs:330).
  Re-Check 2026-08-25: unverändert.
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
- [ ] **[PRIO: niedrig] REVIEW-MCP-N1** JSON-RPC-Codes: Parse-vs-Invalid-Request
  konflatet; Tool-Fehler nicht als isError-Result (lib.rs:139).
  Re-Check 2026-08-25: Struct-Parse-Fehler antworten weiterhin -32700.

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
- [ ] **[PRIO: niedrig] REVIEW-ONNX-N1** SAM-Prompt-Typosystem kann dokumentierte
  Labels −1/2/3 nicht ausdrücken; Koordinatenraum-Verantwortung
  (Source- vs. 1024²-Modell-Space) implizit — vor ORT-Decoder klären
  (sam2.rs:20–56). Teilstand 2026-08-25: Docs präzisiert (Labels,
  1024²-Space); Box-Ecken strukturell via BoxPrompt; Labels −1/2/3 als
  PointLabel-Varianten weiterhin nicht ausdrückbar, kein Mapping-Helper.
- [ ] **[PRIO: niedrig] REVIEW-ONNX-N2** `ModelManifest::validate()` prüft nur
  Capability-Invariante; leere Hash-/Lizenzstrings und Null-
  Auflösungen passieren (manifest.rs:141). Re-Check 2026-08-25:
  unverändert.

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
