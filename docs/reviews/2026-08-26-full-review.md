# Full-Repo-Review R2 (2026-08-26)

Review des gesamten Workspaces (10 Crates, ~45k Zeilen) aus vier Dimensionen
(Performance, Architektur, User-Sicht, Gap-Analyse) plus Robustheit/Testabdeckung/
Doku/Lizenz. Nachfolger des Full-Repo-Reviews vom 2026-08-23 (dessen Befunde als
`REVIEW-*`-Tasks über `Agents.todo.md` verfolgt und zwischenzeitlich committet
wurden; bereits behobene Punkte werden hier nicht erneut gemeldet).

## Review-Kontext und Methodik

- **Basis:** HEAD `59efbcf` **plus uncommittete Arbeitsbaumänderungen** (16
  Dateien, +786/−164; laufende Arbeit einer parallelen Session, siehe
  `Agents.todo.md` Block B „Arbeitsbaumänderungen während des Reviews").
  Alle Befunde wurden gegen den Arbeitsbaum-Stand vom 2026-08-26 verifiziert;
  wo der WIP einen Teilagenten-Befund bereits adressierte, wurde der Befund
  verworfen oder auf den Restbestand eingegrenzt (explizit markiert bei
  R2-MCP-01/-02).
- **Prüfläufe (LESEND):**
  - `cargo clippy --workspace --all-targets` → **0 Warnungen, sauber**.
  - `cargo test --workspace --no-run` → **grün** im finalen WIP-Stand.
    Anmerkung: Während des Reviews wurde transient ein Compile-Fehler in
    `lumina-mcp` (E0282, `tools/batch.rs`) beobachtet, der sich mit dem
    Fortschreiten der parallelen WIP-Session auflöste. **Prozessrisiko:**
    Review/Verifikation und Refactoring auf demselben Baum konkurrenzieren
    sich; Gates sollten nur an ruhenden Bäumen gezogen werden.
- Die drei Teilbereiche CLI/MCP, ONNX/RAW/Lensfun und GPU/GUI-Module wurden
  zusätzlich durch unabhängige Lese-Subagenten tiefengeprüft; deren Fundstellen
  wurden stichprobenartig (alle „hoch“/MVP-blockierenden) gegen den Baum
  re-verifiziert.

## Befunde

Konvention: ID · Schwere · Dimension · Fundstelle · Beschreibung · Lösungsvorschlag · Aufwand (S/M/L) · MVP-blockierend.

---

### A. Performance

**R2-PERF-01 · hoch · PERFORMANCE · `crates/lumina-core/src/tone.rs:111-145`
( Aufrufer: `lumina-gui/src/lib.rs` Ende von `render_from`, ~Zeile 2577)**
`analyze_tone` allokiert pro Aufruf ein `Vec<f64>` mit **einem Eintrag pro
Pixel** (24 MP ≈ 192 MB Heap) und sortiert es vollständig (O(n log n),
`sort_unstable_by(total_cmp)`), um mean/median/p01/p99 zu gewinnen. Die GUI
ruft es am Ende **jedes** `render_from` auf — also auch bei jedem Draft-Tick
während eines Slider-Drag. Die Baseline bestätigt die Größenordnung
(`core/analyze_tone__2048` ≈ 69 ms gegenüber `render_frame__2048` ≈ 107 ms).
Ironie: Mit `LuminanceHistogram` existiert in `histogram.rs` bereits die
bessere O(n)-Struktur mit dokumentierter Quantil-Genauigkeit (±1 Bin) und wird
nur für den Histogramm-Cache genutzt, nicht für das Tone-Panel.
*Lösung:* Tone-Analyse auf `LuminanceHistogram` (oder einen gemeinsamen
Single-Pass über 256 Bins) umstellen; alternativ `tone_analysis` an den
Render-Key koppeln und nur bei neuem Key berechnen. *Aufwand M · MVP-blockierend: nein.*

**R2-GPU-01 · hoch · PERFORMANCE · `crates/lumina-gpu/src/lib.rs:896-920`**
Drag-Hot-Pfad `render_to_vram` erzeugt die Input-Textur **jeden Tick neu** und
lädt die kompletten `frame.pixels` hoch (24 MP ≈ 96 MB CPU→GPU-Upload pro
Slider-Tick). Widerspricht der gelockten Designentscheidung
`docs/gpu-bootstrap.md:92-94` („Base texture cached in VRAM … do not re-create
textures per render call"); der `VramPool` cacht nur Output+Maske.
*Lösung:* Input-Textur pro Pool-Eintrag halten, Neu-Upload nur bei
Quellwechsel (Content-Hash/Dims-Key). *Aufwand M · MVP-blockierend: nein.*

**R2-GPU-02 · hoch · PERFORMANCE · `crates/lumina-gpu/src/lib.rs:1221-1223`,
`shaders.rs:776-817`**
`copy_vram_to_texture` baut Overlay-Pipeline (**inkl. Shader-Modul-Kompilierung**),
`dest_view` und Bindgroup bei jedem Present neu — läuft über
`gpu_present_if_ready` bei jedem Repaint mit frischem VRAM.
*Lösung:* Pipeline je Zielformat einmalig cachen; `dest_view` am stabilen
Present-Target halten. *Aufwand M · MVP-blockierend: nein.*

**R2-GPU-04 · hoch · PERFORMANCE · `crates/lumina-gpu/src/lib.rs:1411-1445,
1626-1656`; Beleg `perf/baseline.json`**
Die eigene, mit `gate:true` aktive Baseline zeigt:
`gpu/cpu_vs_gpu__cpu__2048` = 55,30 ms vs. `gpu/cpu_vs_gpu__gpu__2048` =
**55,57 ms** — der GPU-Pfad ist bei 2048² nicht schneller als die CPU. Ursache:
pro Call neue Input-/Output-Texturen, Staging-Buffer, Sampler, Bindgroups plus
blockierendem `map_async`+`poll(wait)`. Für CLI/Export bringt `render_with_gpu`
aktuell nichts (dokumentiertes TODO(PERF), aber messbar).
*Lösung:* Persistente, gepoolte Ressourcen (Input/Staging/Readback); Export
mittelfristig über den VRAM-Pfad. *Aufwand L · MVP-blockierend: nein.*

**R2-GUIMOD-02 · hoch · PERFORMANCE · `crates/lumina-gui/src/lib.rs:2700-2724`
(Aufrufstelle 5761, je Frame)**
CPU-Present-Pfad: Bei **jedem Repaint** (auch ohne Renderänderung, z. B.
Mousemove über Panels solange kein GPU-Present greift) läuft
`ColorImage::from_rgba_unmultiplied` (Vollbild-Memcpy) + `ctx.load_texture`
(vollständiger Textur-Reupload). Kein Dirty-Gate gegen `self.preview`.
*Lösung:* `TextureHandle` halten und `handle.set(...)` nur bei neuer
Preview-Generation ausführen. *Aufwand S · MVP-blockierend: nein.*

**R2-GUIMOD-03 · mittel · PERFORMANCE · `crates/lumina-gui/src/lib.rs:6356-6359`**
Drag-Hot-Pfad klont die Quelle pro Tick:
`self.draft_original.clone().or_else(|| self.original.clone())`. Im
Fallback-Zweig (erster Tick nach Full-Render, kein Draft gecacht) wird die
vollauflösende Original-Frame geklont (bis ~180 MB Memcpy in einem Tick).
`render_to_vram` nimmt `&ImageFrame` — der Clone ist unnötig.
*Lösung:* Borrows nutzen (`as_ref().or(...)`), Feldzugriffe entzerren.
*Aufwand S · MVP-blockierend: nein.*

**R2-GUIMOD-04 · mittel · PERFORMANCE · `crates/lumina-gui/src/lib.rs:6346-6383`**
Während des Drags laufen parallele Doppel-Renderer: GPU `render_to_vram` **und**
immer zusätzlich der CPU-Draft (`render_draft`). Der CPU-Draft speist
Histogramm/Analyse/Fallback, ist auf GPU-eligible Pfaden aber redundante
Volllast (siehe auch R2-PERF-01: jeder Draft-Tick zahlt `analyze_tone`).
*Lösung:* Auf GPU-Pfaden CPU-Draft drosseln (Analyse seltener oder auf
Debounce-Render verschieben) — bewusst und dokumentiert. *Aufwand M · MVP-blockierend: nein.*

**R2-GPU-03 · mittel · PERFORMANCE · `crates/lumina-gpu/src/lib.rs:957-1006,
1482-1527`**
Bei gebundenen Source-Action-Artefakten werden Region-/Replacement-Texturen bei
jedem Render neu erstellt/hochgeladen (inkl. `format!`-Labels), obwohl sich
Artefakte zwischen Drags nicht ändern; `create_source_action_bind_group`
erzeugt zudem eine 1×1-Filler-Textur pro Call.
*Lösung:* Upload in `set_source_action_artifacts` verlagern und cachen.
*Aufwand S/M · MVP-blockierend: nein.*

**R2-LENS-01 · mittel · PERFORMANCE · `crates/lumina-lensfun/src/lib.rs:499-518,
536-551`; Consumer `crates/lumina-core/src/lib.rs:1112-1127`**
Pro Zielpixel zweimal FFI ins C (`apply_geometry_distortion` mit 1×1 plus
`apply_color_modification` mit 1 Pixel) → bei 24 MP ~48 Mio. Übergänge inkl.
f64↔f32-Konvertierungen pro Pixel. Lensfuns Batch-/Row-API wird nicht genutzt.
*Lösung:* Additive Row-Wrapper (`geometry_row`, `apply_vignetting_row`) und den
Core-Loop zeilenweise füttern. *Aufwand M · MVP-blockierend: nein (Lensfun
Post-MVP, aber F-098-Hotpath).*

**R2-RAW-02 · mittel · PERFORMANCE · `crates/lumina-raw/src/lib.rs:170-178`**
`decode_file` liest die komplette RAW-Datei per `fs::read` in den Heap und
übergibt an `open_buffer` — Peak-Memory ≈ Dateigröße + LibRaw-interne Buffer
(bei 50–150 MB CR3 relevant). `libraw_open_file` könnte direkt streamen; der
SAFETY-Kommentar (394-399) feiert eine Kopievermeidung, die hier genau nicht
greift.
*Lösung:* Intern auf `libraw_open_file` umbauen; `open_buffer` nur für
`decode_bytes`. *Aufwand S/M · MVP-blockierend: nein.*

**R2-MCP-04 · mittel · PERFORMANCE · `crates/lumina-mcp/src/tools/load.rs:51-58`**
`lumina_load` liest die RAW-Datei vollständig für Hash/Identity (`fs::read`)
und dekodiert danach via `decode_file(path)` **dieselbe Datei erneut von
Disk**; beide Kopien bleiben nebeneinander im Speicher. Das CLI zeigt das
richtige Muster (`decode_input`: Bytes einmal lesen → `decode_bytes`).
*Lösung:* `decode_bytes(&bytes, filename)` statt `decode_file`.
*Aufwand S · MVP-blockierend: nein.*

**R2-ONNX-02 · mittel · PERFORMANCE · `crates/lumina-onnx/src/backend.rs:140-149`**
`StubBackend::infer` resisiert vor jeder Inferenz das Vollbild auf 1024×1024
(Nearest-Neighbor über alle Quellpixel, ~3 MB-Allokation) und wirft das
Ergebnis weg (`let _rgb`). Läuft bei jeder Re-Inferenz im Standard-Backend
aller Builds.
*Lösung:* Nur Dimensionsvalidierung behalten; Resize in Test verschieben.
*Aufwand S · MVP-blockierend: nein.*

**R2-ONNX-04 · mittel · PERFORMANCE · `crates/lumina-onnx/src/hash.rs:138-146`**
`verify_model_file` hasht die Modelldatei immer vollständig, obwohl bei
`expected == PENDING_INTEGRATION_HASH` das Ergebnis zwangsläufig `Pending` ist.
Sobald echte Gewichte (mehrere hundert MB) integriert sind, zahlt jeder
Backend-Load einen sinnlosen vollen SHA-256-Durchlauf.
*Lösung:* Short-circuit bei Pending-Hash. *Aufwand S · MVP-blockierend: nein.*

**R2-MCP-08 · niedrig · PERFORMANCE · `crates/lumina-mcp/src/tools/analyze.rs:44-48`**
`lumina_analyze` macht sechs separate Full-Frame-Pässe (`analyze_tone`,
Luminance-Histogramm, Kanal-Histogramme, Kanal-Stats, Stddev, Dominant-Colors)
und rendert immer in Vollauflösung (Schema hat kein `max_width`). Im
Agent-Feedback-Loop der teuerste Pfad, obwohl Statistiken auflösungsunabhängig
sind. *Lösung:* Optionales approximatives `max_width` + Pässe fuschen.
*Aufwand M · MVP-blockierend: nein.*

**R2-GPU-08 · niedrig · PERFORMANCE · `crates/lumina-gpu/src/lib.rs:826-828`
(Aufrufe 891, 1092)** — `perf_log_enabled` liest pro Render zweimal Env-Vars
(`std::env::var` im Hot-Pfad) und dopplert `log::info!`/`eprintln!`.
*Lösung:* `OnceLock<bool>`; eine Ausgabe. *Aufwand S · MVP-blockierend: nein.*

**R2-GUIMOD-05 · niedrig · PERFORMANCE · `crates/lumina-gui/src/lib.rs:2745`**
`gpu_present_if_ready` ruft pro Frame `unsupported_gpu_stages(&self.recipe)` →
`Vec<String>` mit `format!`-Allokationen, obwohl Rezept/Bindung sich selten
ändern. *Lösung:* An Render-Key memoisieren. *Aufwand S · MVP-blockierend: nein.*

**R2-ONNX-07 · niedrig · PERFORMANCE · `crates/lumina-onnx/src/preprocess.rs:119-131`**
`normalize_rgb_to_nchw` liest in drei stride-3-Pässen interleaved; ein einzelner
Pixel-Pass mit drei Plane-Writes ist cache-lokaler und bitidentisch.
*Aufwand S · MVP-blockierend: nein.*

---

### B. Architektur

**R2-MCP-01 · hoch · ARCHITEKTUR (Pixel-Divergenz / stiller Fallback) ·
`crates/lumina-mcp/src/util.rs:334-368` (GPU-Zweig ~355);
Signatur `crates/lumina-gpu/src/lib.rs:1376-1380`; Spiegel
`crates/lumina-cli/src/main.rs:127-175`** — **Re-verifiziert gegen WIP-Stand.**
Der GPU-Zweig von `render_best_effort` ruft `ctx.render_with_gpu(frame, recipe)`
**ohne `camera_white_balance`**; der Routing-Guard
(`unsupported_gpu_stages_for`) prüft Rezept-Keys, Curves/HSL/Presence,
Source-Actions und Lensfun — aber nicht die **Anwesenheit von Kontext-WB**.
Bei `--features gpu` und verfügbarem Adapter fällt die As-Shot-WB still weg,
während derselbe CPU-Build dieselbe Quelle damit rendert: gleiche Quelle +
gleiches Sidecar → unterschiedliche Pixel je nach Build-Feature, ohne Warnung.
Der neue WIP hat WB zwar durchgereicht (`render_recipe(..., camera_white_balance)`),
am GPU-Drop ändert das nichts.
*Lösung:* `Some(camera_white_balance)` als expliziten CPU-Routing-Reason
aufnehmen (analog `lensfun_corrector_active`), bis der GPU-Shader
WB-kontextfähig ist. *Aufwand S · MVP-blockierend: nein (feature-gekoppelt,
aber stumme Pixeldivergenz).*

**R2-GPU-05 · hoch · ARCHITEKTUR/Korrektheit ·
`crates/lumina-gpu/src/lib.rs:177-194` +
`crates/lumina-gui/src/lib.rs:2018, 3948`** — Re-verifiziert.
`unsupported_gpu_stages_for` flaggt flat Adjustments **wertblind** (jeder Key
außerhalb des Supportsatzes), während Curves/HSL/Presence Neutralitätsprüfungen
haben. Die GUI schreibt per Slider/Reset aber den Defaultwert `0.0` **zurück in
die Map**, statt den Key zu entfernen. Einmal Vibrance/Saturation berührt (auch
wieder zurück auf 0) → Rezept bleibt dauerhaft GPU-unerlaubt (CPU-Route +
CPU-Present) bis zum Dateiwechsel, obwohl die Pixel identisch wären.
*Lösung:* Wert-Neutralität prüfen (Key == 0.0 überspringen). Key-Removal wäre
eine Sidecar-Schema-Entscheidung gemäß Agents.md. *Aufwand S · MVP-blockierend: nein.*

**R2-RAW-01 · hoch · ARCHITEKTUR/Robustheit (falsche öffentliche API) ·
`crates/lumina-raw/src/lib.rs:20-33`** — Re-verifiziert.
`DemosaicMethod::libraw_value` mapped `Linear→1, Vng→2, Ppg→3, Ahd→4, Dcb→11,
Dht→12, Aahd→13`. LibRaw setzt direkt `params.user_qual`; dcraw_process branched
aber `0=lin, 1=VNG, 2=PPG, 3=AHD, 4=DCB, 11=DHT, 12=AAHD`, alles andere →
**stiller Fallback auf AHD**. Jede explizite Wahl wählt damit den falschen
Algorithmus (`Aahd`=13 fällt unbemerkt auf AHD zurück) — klar gegen „keine
stillen Fallbacks". Kein Test pinnt das Mapping.
*Lösung:* Mapping auf 0/1/2/3/4/11/12 korrigieren, Tabelle als Unit-Test
pinnen, dcraw_process-Referenz kommentieren. *Aufwand S · MVP-blockierend: nein
(Pflicht vor jeder Demosaic-Exposition in UI/Rezept).*

**R2-CLI-02 · mittel · ARCHITEKTUR · `crates/lumina-cli/src/main.rs:1808-1857`
vs. `crates/lumina-mcp/src/util.rs` (~50 Zeilen identischer Logik);
Extension-Listen dreifach (`main.rs:1750`, `util.rs:BATCH_IMAGE_EXTENSIONS`,
`main.rs:1232`)** — CLI und MCP bauen `SourceIdentity` unabhängig; Drift ist
schon da (MCP fällt bei `fs::metadata`-Fehler auf `bytes.len()` zurück, CLI
bricht hart ab). Gefährdet die Invariante „Sidecar-Identität ist
backend-übergreifend gleich".
*Lösung:* `build_source_identity` + Extension-Konstante nach `lumina-sidecar`
(bzw. `lumina-raw`) ziehen, alle Backends rufen gemeinsam auf.
*Aufwand M · MVP-blockierend: nein.*

**R2-GPU-06 · mittel · ROBUSTHEIT · `crates/lumina-gpu/` gesamt**
Kein `Device::on_uncaptured_error` und kein Device-Lost-Callback (grep: 0
Treffer). Da `from_parts` das geteilte eframe-Renderer-Device übernimmt, führt
jeder Validierungsfehler aus `lumina-gpu` zum wgpu-Default-Handler →
App-Panik; nach Device-Loss gibt es keinen sichtbaren CPU-Übergang.
*Lösung:* Error-Handler → `warn!`+Fehlflag; Device-Lost → VRAM-Pool verwerfen,
sichtbar auf CPU delegieren. *Aufwand M · MVP-blockierend: nein.*

**R2-GPU-07 · mittel · ARCHITEKTUR · `crates/lumina-gpu/src/lib.rs:2093-2099`**
`init_gpu_resources` hardcodet `Backends::METAL`. Die standalone-Initialisierung
(CLI/MCP) bekommt auf Windows/Linux nie GPU-Beschleunigung, während die GUI via
`from_parts` jeden eframe-Backend nutzt — inkonsistente Capability, in der
Matrix nicht ausgewiesen.
*Lösung:* Backend-Liste konfigurierbar machen oder Einschränkung explizit in
die Capability-Matrix schreiben. *Aufwand S · MVP-blockierend: nein.*

**R2-ONNX-03 · mittel · ARCHITEKTUR · `crates/lumina-onnx/src/ort_backend.rs:252-260`
vs. `backend.rs:153-169`**
Nur `StubBackend` implementiert die Brücke zu `lumina_core::MaskInference`
(inkl. `is_available`); das reale `OrtBackend` kann heute nicht eingesteckt
werden, ohne additiven Code — asymmetrischer Contract.
*Lösung:* Feature-gegatete Trait-Impl ergänzen (additiv).
*Aufwand S · MVP-blockierend: nein.*

**R2-GAP-01 · mittel · GAP/DOKU · `feature/README.md:107,143` vs. Code**
F-009 „Presets" ist in der Feature-Matrix auf `product/virtual-copies.md`
gemappt — das Dokument enthält **keine einzige Preset-Erwähnung** (grep: 0
Treffer). Die festgelegte Entscheidung „Presets werden als einzelne
`<name>.lumina-preset.json`-Dateien exportiert" ist nirgends implementiert
(grep `lumina-preset` über alle Crates: 0 Treffer); die GUI hat ausschließlich
In-Memory-Presets (`create_preset`/`apply_preset`,
`crates/lumina-gui/src/lib.rs:1083/1118`), die beim Schließen verschwinden.
Relative-Exposure-Validierung ist korrekt umgesetzt (auto-tone-Kopplung beidseitig geprüft).
*Lösung:* Doku-first: F-009-Soll in virtual-copies.md beschreiben (Dateiformat,
Scope), dann Preset-File-I/O implementieren — oder Matrix-Eintrag bewusst auf
Post-MVP verschieben. *Aufwand M · MVP-blockierend: nein, aber Doku-Code-Divergenz.*

**R2-GPU-11 · niedrig · DOKU/ARCHITEKTUR · `Agents.md` (Architekturgrenzen) vs.
Workspace** — `Agents.md` listet die Crate-Grenzen core/sidecar/raw/onnx/cli/gui;
**`lumina-gpu` fehlt** (ebenso `lumina-bench`, `lumina-lensfun`,
`lumina-mcp`), obwohl das Crate Shader-Bildverarbeitung trägt und
`docs/gpu-bootstrap.md` seine Grenze „per Agents.md" herleitet. Verstoß gegen
„Der Plan darf niemals stillschweigend veralten".
*Lösung:* Grenzen der vier fehlenden Crates in Agents.md ergänzen.
*Aufwand S · MVP-blockierend: nein.*

**R2-GPU-10 · niedrig · ARCHITEKTUR (totes Gerüst) ·
`crates/lumina-gpu/src/lib.rs:1691-1708`; `shaders.rs:70-115, 921-945`**
`render_draft` baut `DraftPyramid`+`TiledCache(64)` nur für einen Debug-Log und
rendert dann doch CPU; `bake_3d_lut` (Identity-Stub, 512-KB-Typ), `LUT_DIM` und
`create_fp16_framebuffer` sind ungenutzt; gpu-shaders.md stimmt teils nicht
mehr mit dem Code. *Lösung:* Schrumpfen oder klar als Scaffold ausweisen.
*Aufwand S · MVP-blockierend: nein.*

---

### C. User-Sicht / UX

**R2-GUIMOD-01 · hoch · UX/PERFORMANCE (Hauptinteraktionspfad) ·
`crates/lumina-gui/src/lib.rs:6366` (Set), `3948` (Clear nur bei Edit), `2740`
(Gate), `2137-2160` (`render_full`)** — **Re-verifiziert; MVP-blockierend.**
Beim letzten Drag-Tick wird `vram_fresh = true` gesetzt (VRAM hält das
Draft-Ergebnis). Danach läuft der debounced Full-Render auf CPU — aber weder
`render_full` noch `render_from` invalidieren `vram_fresh`, und
`gpu_present_if_ready` prüft weder `preview_is_draft` noch Dimensions-
übereinstimmung mit `self.preview`. Folge: Das Vollqualitäts-Ergebnis wird
berechnet, aber **nie angezeigt** — die Preview bleibt nach der ersten
Reglerberührung dauerhaft weich (hochskaliertes Draft), bis der nächste Edit
kommt.
*Lösung:* In `render_full`/`render_from`-Erfolgspfad `vram_fresh = false`
setzen **oder** Present-Gate um `vram_dims == preview_dims &&
!preview_is_draft` erweitern. *Aufwand S · MVP-blockierend: JA.*

**R2-MCP-02 · hoch · UX/ROBUSTHEIT (nicht-destruktive Garantie) ·
`crates/lumina-mcp/src/tools/save.rs:63-88`** — **Re-verifiziert gegen
WIP-Stand; MVP-blockierend.** `lumina_save` guardt nur gegen die Quelle
(`paths_resolve_equal(source, output)`). Der neue WIP hat in
`crates/lumina-mcp/src/util.rs` bereits `reject_protected_target`/
`write_output_guarded` (deckt `<input>.lumina.json`, `.lumina.zdata` und
Hardlinks via `(dev,ino)` ab) und nutzt sie in `tools/batch.rs:185` — **aber
`lumina_save` nicht**. Ein Symlink/Hardlink `out.png` aufs Sidecar überschreibt
das autoritative Bundle mit PNG-Bytes. CLI-Guard (`reject_protected_output`,
`main.rs:1888-1917`) deckt denselben Fall ab — MCP hinkt hinterher.
*Lösung:* In `save.rs` `crate::util::write_output_guarded(source, target, &bytes)`
statt `write_atomically` verwenden (Einzeiler, Guard existiert bereits).
*Aufwand S · MVP-blockierend: JA.*

**R2-CLI-01 · hoch · GAP/UX · `crates/lumina-cli/src/main.rs:1228-1238`
(`has_image_extension`) vs. `main.rs:1743-1754` (`is_raw_path`) vs.
`cli-gui-wasm.md:57-62`** — **Re-verifiziert gegen WIP-Stand.**
`has_image_extension` akzeptiert nur 9 Extensions (png/jpg/jpeg/webp/arw/cr2/
cr3/dng/nef), `is_raw_path`/Decode-Pfad und das SOLL aber 18 RAW-Formate (orf,
raf, rw2, crw, pef, srw, 3fr, iiq, rwl, mos, erf, kdc, x3f). `lumina batch
--input <dir>` überspringt RAF/ORF/etc. **stillschweigend**, obwohl dieselben
Dateien einzeln funktionieren.
*Lösung:* RAW-Extension-Liste einmalig definieren (aus `lumina-raw`
exportieren, siehe R2-CLI-02) und in beiden Prädikaten referenzieren; Test:
Batch über Fixture-Dir findet RAF. *Aufwand S · MVP-blockierend: JA.*

**R2-GUIMOD-06 · mittel · UX (stiller Fallback benutzer-sichtbar) ·
`crates/lumina-gui/src/lib.rs:601-613`; `crates/lumina-gpu/src/lib.rs:328-341`;
`i18n.rs` (grep „GPU": 0 Treffer)**
CPU↔GPU-Routing, Shared-Device-Fail und die VRAM-Divergenz-Warnung
(„Interactive preview may diverge") erscheinen **ausschließlich im stderr-Log**
— kein i18n-String, kein Statusbadge. Agents.md fordert „keine stillen
Fallbacks"; log-seitig erfüllt, benutzer-sichtbar nicht. Besonders relevant, da
nach einer Divergenz-Warnung trotzdem weiter im VRAM gerendert wird.
*Lösung:* Statusbadge „GPU: `<adapter>`" / „CPU-Fallback: `<Grund>`" (i18n).
*Aufwand S/M · MVP-blockierend: nein.*

**R2-GUIMOD-07 · mittel · UX/DIAGNOSE · `crates/lumina-gui/src/main.rs:11`,
`logger.rs:32-60`**
Logger startet per Default auf **`LevelFilter::Trace`** (nur RUST_LOG override
bar). Jede Zeile kostet format!/String-clone/Mutex/eprintln; Trace-Chatter aus
wgpu/naga/egui ist ein realer Frame-Time-Faktor, und der Panic-Ring (512
Einträge) füllt sich mit Noise. Beleg: `gui.log` im Repo-Root ist allein durch
kurze Tests already 6 MB.
*Lösung:* Default `Info`, Diagnose via RUST_LOG=trace; Ring-Eintrag ohne Clone.
*Aufwand S · MVP-blockierend: nein.*

**R2-CLI-05 · mittel · UX/ROBUSTHEIT (stiller Fallback) ·
`crates/lumina-cli/src/main.rs:1297-1299, 1306-1308`**
Korruptes `.lumina.zdata` wird bei Masken still als „fehlend" behandelt
(`let Ok(container) = load_zdata(..) else { return planes; }`, Tile-Fehler
`continue`), statt „bundle corrupt" zu melden. Derselbe Korruptfall failt bei
Source-Actions laut (`resolve_source_actions`, 931-951). Kaschiert Datenverlust
als harmlose Fehlende-Maske-Situation.
*Lösung:* Load-/Tile-Fehler als explizite „zdata unreadable/corrupt"-Warnung
über `mask_warnings_out` melden. *Aufwand S · MVP-blockierend: nein.*

**R2-CLI-06 · mittel · UX · `crates/lumina-cli/src/main.rs:1033-1038, 1161`**
Batch gibt keinen Per-Item-Fortschritt (Terminal bleibt bis zur Schlusszeile
stumm) und verwirft die gesammelten `mask_warnings` komplett (`&mut Vec::new()`),
obwohl gerade Agenten sie pro Bild brauchen — `render`/`export` weisen sie im
JSON aus, `batch` nicht. *Lösung:* Progressline auf stderr + warnings ins
Item-JSON. *Aufwand S · MVP-blockierend: nein.*

**R2-CLI-03 · mittel · GAP/UX · `cli-gui-wasm.md:51-53` vs. `main.rs:442-445,
1689-1741`**
SOLL: „`inspect` zeigt den JSON-Status und die virtuellen Kopien". Implementierung
gibt freien Text; `InspectArgs` hat als einziger Befehl kein `--json`. Auto-Tone/
Matching werden gedruckt, aber nicht maschinenlesbar.
*Lösung:* `--json` ergänzen oder SOLL-Formulierung anpassen (doku-first).
*Aufwand S · MVP-blockierend: nein.*

**R2-CLI-04 · mittel · PERFORMANCE/UX · `main.rs:1690-1698`;
API-Check `lumina-raw` (nur decode_file/decode_bytes*)**
`inspect` dekodiert das komplette RAW (alle Pixel, ~200 MB Frame), um 4 Zeilen
EXIF/Maße zu drucken — keine Metadata-only-API vorhanden.
*Lösung:* `lumina_raw::read_metadata(bytes)` ergänzen und nutzen.
*Aufwand M · MVP-blockierend: nein.*

**R2-MCP-03 · mittel · GAP (Doku-Code-Divergenz) · `mcp-server.md:484-485` vs.
`tools/save.rs`**
Doku behauptet SidecarConflict (-32010) bei CAS-Konflikt in `lumina_edit`/**`lumina_save`**;
`lumina_save` hat gar keinen Sidecar-Write-Pfad (-32010 existiert nur in
`edit.rs:136-140`). *Lösung:* Doku präzisieren oder Save-Write-Through
spezifizieren/implementieren. *Aufwand S · MVP-blockierend: nein.*

**R2-MCP-05 · mittel · GAP · `mcp-server.md:404-408` vs.
`crates/lumina-mcp/src/main.rs:11-35`**
Doku: Shutdown-Cleanup der Vorschauen „optional, Default: ja". Implementiert:
nein — Preview-Dateien alter Sessions akkumulieren unbegrenzt in
`$TMPDIR/lumina-previews/`. *Lösung:* Session-geführte Liste + Löschung am
Loop-Ende. *Aufwand S · MVP-blockierend: nein.*

**R2-GUIMOD-09 · mittel · PERFORMANCE (Startup) · `lumina-gui/src/lib.rs:841`
+ `main.rs:25-29`**
`LuminaApp::new` erzeugt einen standalone `GpuContext::new()` (blockierender
Adapter/Device-Request), der kurz darauf von `attach_wgpu_render_state`/
`from_parts` komplett ersetzt wird — doppelte GPU-Init pro Start.
*Lösung:* Lazy erst in `attach_wgpu_render_state` erzeugen.
*Aufwand S · MVP-blockierend: nein.*

**R2-CLI-09 · niedrig · UX · `main.rs:538-543` (Insert ohne Range-Check);
Validierung erst via save→validate (`lumina-sidecar/src/lib.rs:2111-2133`)**
`lumina develop --exposure 999` wird erst beim Save mit generischem
`invalid adjustment \`exposure\`` (ohne Range) abgewiesen; MCP `lumina_edit`
lehnt vorab strukturiert mit min/max ab. Inkonsistent für denselben fachlichen
Vorgang. *Lösung:* Range-Check vor Insert, Meldung analog MCP.
*Aufwand S · MVP-blockierend: nein.*

**R2-CLI-10 · niedrig · UX · `main.rs:216-250`**
`Command::Import(FileArgs)` erbt `--output/--format/--quality/--force-render/
--virtual-copy/--mask-policy`, die still ignoriert werden — Nutzer können
glauben, der Import habe konvertiert. *Lösung:* Eigenes schlankes `ImportArgs`.
*Aufwand S · MVP-blockierend: nein.*

**R2-CLI-07 · niedrig · GAP/UX · `main.rs:463-469`; SOLL `cli-gui-wasm.md:77`**
Exit-Codes sind reproduzierbar (0/1, clap: 2), aber nirgends dokumentiert; ein
Batch mit 500/1000 Fehlern exit-t identisch zu einem Laufzeitfehler.
*Lösung:* Exit-Code-Tabelle ins SOLL; ggf. dedizierter Code für
„Batch teilfehlerhaft". *Aufwand S · MVP-blockierend: nein.*

**R2-MCP-09 · niedrig · UX/Konsistenz · `tools/edit.rs:28-53` vs.
`mcp-server.md:139-149`**
`lumina_edit` akzeptiert `vibrance`/`saturation`, obwohl Schema und Doku diese
Keys nicht ausweisen. *Lösung:* Synchronisieren (Schema erweitern oder Keys
ablehnen). *Aufwand S · MVP-blockierend: nein.*

**R2-ONNX-05 · niedrig · UX · `backend.rs:127-131`, `sam2.rs:339-343`**
Simulations-Abwesenheit wird als `MissingModel { path: model_name }` gemeldet —
Displaytext suggeriert einen Dateipfad, gemeint ist das Verfügbarkeitsflag; vom
wirklich fehlenden `.onnx`-Artefakt nicht klar unterscheidbar.
*Lösung:* Dediziertes Feld/Präfix („model reported unavailable").
*Aufwand S · MVP-blockierend: nein.*

**R2-MCP-07 · niedrig · UX · `lib.rs:100` vs. `preview.rs:57-62`**
Preview-Dir-Anlage wird beim Start still ignoriert (`let _ = create_dir_all`);
der Nutzer erfährt erst beim ersten `lumina_preview` etwas — dann als
`Encode`-Error, der die Ursache verschleiert. *Lösung:* `log::warn!` beim Start.
*Aufwand S · MVP-blockierend: nein.*

---

### D. Robustheit

**R2-GPU-13 · niedrig · ROBUSTHEIT · `crates/lumina-gpu/src/lib.rs:313, 333`**
`lock().unwrap()` in den Dedup-Loggern → Poisoning-Panik, wenn ein Thread beim
Halten panikt; globaler State ist zudem testübergreifend.
*Lösung:* `unwrap_or_else(|p| p.into_inner())`. *Aufwand S · MVP-blockierend: nein.*

**R2-CLI-08 · niedrig · ROBUSTHEIT · `main.rs:1052, 1174`**
Zwei `unwrap()` auf `serde_json::to_string/to_vec` in Batch-Läufen (praktisch
infallibel, aber ein Panik in einem Rayon-Worker reißt den Pool runter statt
das Item als failed zu melden). Grep-bestätigt die einzigen Nicht-Test-unwraps
der CLI. *Lösung:* `unwrap_or_else` + Item failed werten.
*Aufwand S · MVP-blockierend: nein.*

**R2-LENS-02 · niedrig · ROBUSTHEIT (unsafe-FFI) ·
`crates/lumina-lensfun/src/lib.rs:358-373, 596-600`**
Undokumentierte Lebensdauer-Invariante: `Corrector` ist `'static`, wird aber
aus einem db-eigenen `lfLens` initialisiert. Gegen lensfun 0.3.4 verifiziert
sicher (`lfModifier::Initialize` kopiert die Kalibrierdaten), aber weder im
Safety-Kommentar verankert noch per Test geschützt.
*Lösung:* SAFETY-Kommentar mit Quellenverweis + Corrector-nach-drop(db)-Test.
*Aufwand S · MVP-blockierend: nein.*

**R2-GPU-09 · niedrig · ROBUSTHEIT/API · `crates/lumina-gpu/src/lib.rs:1116-1206`**
Inkonsistente Fehlerverträge ohne Adapter: `upload_mask_*` → `Ok(())`,
`render_to_vram`/`readback_mask_plane` → `Err(AdapterUnavailable)`,
`copy_vram_to_texture` → stiller No-op-Erfolg. Aktuell durch GUI-Gate
abgefedert, aber API-Fußschuss. *Lösung:* Einheitlicher Contract + Doku.
*Aufwand S · MVP-blockierend: nein.*

**R2-GAP-02 · niedrig · HYGIENE · Repo-Root `gui.log` (untracked, 6 MB) +
`.gitignore`** — Das TRACE-Logfile liegt unbeabsichtigt im Repo-Root und ist
nicht gitignoriert (kein `*.log`-Eintrag) → versehentliche Commit-Gefahr.
*Lösung:* `/gui.log` (oder `*.log`) in `.gitignore`; Logfile standardmäßig
nach Cache-Dir schreiben. *Aufwand S · MVP-blockierend: nein.*

---

### E. Tests / Gap-Analyse

**R2-ONNX-01 · hoch · GAP ai-masks.md · `crates/lumina-onnx/src/manifest.rs:225-232`**
— **Re-verifiziert.** `to_model_identity` lässt `extras` immer leer; die
persistierte Maskenidentität besteht nur aus name/version/hash. ai-masks.md
fordert „Vorverarbeitung und Inferenzauflösung" ausdrücklich als
Identitätsbestandteil. Wird `InputNormalization`/`Resolution` im Manifest
geändert, ohne `model_version`/`model_hash` zu bumpen, bleiben persistierte
Masken `valid`, obwohl die Inferenzsemantisch anders ist — die Stale-Erkennung
(F-004-Kernabnahme) wird umgangen.
*Lösung:* Deterministischen Digest über `ModelInputSpec` in `extras`
schreiben; Test „Normänderung ⇒ Identität ändert sich".
*Aufwand S/M · MVP-blockierend: nein, zwingend vor echter Gewichts-Integration (F-048).*

**R2-GPU-12 · niedrig · TESTS · `crates/lumina-gpu/tests/`**
Der Overlay-/Present-Composite-Shader (Tint-Mix, u16-Normalisierung,
Alpha-Passthrough) hat keinen Pixeltest — `stages.rs` deckt SA-Stage und
Mask-Roundtrip ab, die Compositing-Mathematik nur den manuellen GUI-Test.
*Lösung:* Golden-Test gegen CPU-Formel `mix(base, tint, mask·0.45)`.
*Aufwand S/M · MVP-blockierend: nein.*

**R2-RAW-04 · niedrig · TESTS · `crates/lumina-raw/src/lib.rs:531-547` vs.
Tests** — Der 16-bit-Decode-Pfad (`output_bits=16`) läuft nie end-to-end
(nur synthetische u16→u8-Unit-Tests); Budget-Gate und Promotion bleiben CI-
unausgeführt, obwohl beide Fixtures verfügbar sind. *Lösung:* Fixture-Test mit
`output_bits:16` + Geometrie-Asserts. *Aufwand S · MVP-blockierend: nein.*

**R2-LENS-03 · niedrig · TESTS · `crates/lumina-lensfun/src/lib.rs:240-252`**
Null-/Clamp-Pfade von `lf_camera_crop_factor` (NaN, ≤0.1, ≥100 → 1.0)
ungetestet; Null-Fall trivial testbar. *Aufwand S · MVP-blockierend: nein.*

**R2-LENS-04 · niedrig · BUILD/ROBUSTHEIT · `crates/lumina-lensfun/build.rs:93-96`**
ABI-Probe läuft nur bei build.rs-Änderung neu (`rerun-if-changed`); ein
System-Upgrade von liblensfun (gleiches `-L`, neues Layout) wird nicht bemerkt.
*Lösung:* `rerun-if-changed` auf die aufgelöste Library-Datei emittieren.
*Aufwand S · MVP-blockierend: nein.*

**R2-ONNX-06 · niedrig · TESTS · `preprocess.rs:29-31, 76-78`**
Degenerations-Guards `dw<=1`/`dh<=1` des Nearest-Neighbor-Mappings ohne
Testabdeckung. *Aufwand S · MVP-blockierend: nein.*

**R2-RAW-03 · niedrig · API-Hygiene · `crates/lumina-raw/src/lib.rs:174-177, 551`**
Der `name`-Parameter der Decode-API ist vollständig tot (`let _ = name;`) —
`MissingName` wird aus diesen Pfaden nie geworfen, irreführende Signatur.
Pre-MVP darf das breaking bereinigt werden. *Aufwand S · MVP-blockierend: nein.*

**R2-MCP-06 · niedrig · ARCHITEKTUR · `util.rs:328-364`**
`downscale_bilinear` ist handgeschriebene Bildverarbeitung im Interface-Layer
(MCP soll reiner Orchestrierungs-Layer sein); enthält zudem eines von genau
zwei Nicht-Test-`expect`s des Crates (Zeile 363). Gehört nach `lumina-core`
(golden-testbar, wiederverwendbar). *Aufwand S · MVP-blockierend: nein.*

**R2-CLI-11 · niedrig · ROBUSTHEIT · `main.rs:1216-1218, 1029-1038`**
Kein Inode-Dedup der Batch-Inputs: Derselbe Inode unter zwei Namen wird bei
`--jobs > 1` parallel verarbeitet; namensbasierte Kollisionserkennung greift
nicht. Lock serialisiert zwar, aber doppelte History-Einträge/Last-wins bleiben.
*Lösung:* Inputs per `(dev, inode)` deduplizieren (Primitive existiert in
`paths_are_same_file`). *Aufwand S · MVP-blockierend: nein.*

**R2-CLI-12 · niedrig · UX (kosmetisch) · `main.rs:1426`** — Formatstring
`unknown virtual copy \`{id}` ohne schließenden Backtick. *Aufwand S · nein.*

---

## Executive Summary — Top 10 nach Schwere

1. **R2-MCP-02** (hoch, MVP-blockierend): `lumina_save` kann Sidecar/zdata via
   Symlink/Hardlink überschreiben — Fix existiert bereits in `util.rs`, wird in
   `save.rs` nur nicht verwendet (S-Aufwand).
2. **R2-GUIMOD-01** (hoch, MVP-blockierend): Nach Regler-Release bleibt das
   Draft-VRAM-Bild im Present-Pfad hängen; der Vollqualitäts-Render wird nie
   angezeigt (S-Aufwand).
3. **R2-CLI-01** (hoch, MVP-blockierend): `lumina batch` überspringt still 9 von
   18 unterstützten RAW-Formaten (S-Aufwand).
4. **R2-MCP-01** (hoch): GPU-Pfad verwirft `camera_white_balance` still →
   build-abhängige Pixeldifferenz ohne Warnung (S-Aufwand).
5. **R2-GPU-05** (hoch): Wertblinde GPU-Routing-Gates für flat Adjustments —
   einmal Vibrance berührt heißt dauerhaft CPU-Route (S-Aufwand).
6. **R2-RAW-01** (hoch): Demosaic-Mapping um +1 verschoben; jede explizite Wahl
   wählt still den falschen Algorithmus (S-Aufwand).
7. **R2-PERF-01** (hoch): `analyze_tone` allokiert n×f64 + vollständige Sortierung
   pro Render-Tick, obwohl `LuminanceHistogram` die O(n)-Alternative ist (M).
8. **R2-GPU-01** (hoch): ~96 MB Textur-Upload pro Drag-Tick durch fehlendes
   Input-Textur-Caching (M).
9. **R2-GUIMOD-02** (hoch): Vollbild-Copy + Textur-Reupload bei jedem Repaint im
   CPU-Present-Pfad ohne Dirty-Gate (S).
10. **R2-ONNX-01** (hoch): Vorverarbeitung/Auflösung fehlen in der persistierten
    Maskenidentität → Stale-Erkennung umgehbar (S/M).

## Positive Befunde (nicht erneut anfassen)

- **Clippy workspace-weit sauber** (`--all-targets`, 0 Warnungen);
  `cargo test --workspace --no-run` grün im finalen WIP-Stand.
- **Stage-Cache (PERF-GUI-1)** sauber designed: vollständige Identity-Digests,
  Clone-on-read, byte-budgetiertes LRU mit dokumentierter Refuse-Politik,
  eigene Tests (`stage_cache.rs`).
- **Sidecar-Persistenz robust:** atomare Writes mit fsync + Same-Dir-Persist
  (JSON wie zdata), TOCTOU-sicherer Lock-Reclaim via atomarem Rename mit
  Conflict für den Verlierer, umfangreiche Roundtrip-/Validierungs-/Cycle-Tests.
- **GUI meldet Veraltet-Zustände sichtbar:** Mask-Status
  Valid/Stale/Corrupt/Missing wird ausgewiesen (lib.rs:999-1007,
  MaskStaleRecalc), RenderStateStale-Indikator (5770), Auto-Tone-Stale mit
  Fingerprint-Check (2983-2991, eigene Tests).
- **Thumbnail-Pipeline nutzt inzwischen echte Worker-Threads** (Channel +
  poisoned-safe recv, lib.rs:720) — die kritischen Hauptthread-Blockaden aus
  `review.notes.md` (2026-08-22) sind behoben.
- **GPU-Testkultur:** SA-Stage byte-exakt getestet, Mask-Readback-Roundtrip,
  Golden-Harness skipped loudly statt grün zu lügen, VramPool-Eviction-Politik
  unit-getestet; WGSL/CPU-Duplikat ist sanktioniert (CPU = Oracle) und durch
  den Golden-Gate abgesichert.
- **CLI Write-Guards:** `reject_protected_output` deckt Original/Sidecar/zdata/
  Hardlinks mit Tests ab; Walker symlink-/loopsicher; Stage→Sidecar→Commit-
  Ordering korrekt.
- **MCP-Protokoll:** korrekte JSON-RPC-Code-Trennung, CAS-Write in `lumina_edit`
  sauber, strikte Parameterbindung, stdout-Strom unverdreckt.
- **onnx/raw/lensfun unsafe-Disziplin:** data_size-Gates an jedem Foreign-Slice,
  ICC-Cap, RAII-Handles, EXIF-Flip-Permutation bitgenau gegen LibRaw bewiesen;
  Lizenzseite (LibRaw/Lensfun/BiRefNet/SAM-2/ORT) vollständig in `licenses/`.
- **Capability-Gating:** WASM-Pfade konsistent `cfg`-gegate; `lumina-core`
  plattformneutral gehalten (rayon/lensfun optional + gated; WIP target-gated
  zusätzlich die proptest-dev-deps → `REVIEW-CORE-WASM-FOLLOWUP` scheint im
  Arbeitsbaum gelöst, Kandidat für Verifikation + Todo-Entfernung).

## Durch manuellen GUI-/Laufzeit-Test noch zu verifizieren

- Sichtbares Bestätigen von R2-GUIMOD-01 (weiche Preview nach Regler-Release)
  und R2-GPU-05 (dauerhaft CPU nach Dynamik/Vibrance-Berührung) am Bildschirm.
- Tatsächliche Drag-FPS bei 24/45 MP auf Metal (praktische Spürbarkeit von
  R2-GPU-01/02, R2-GUIMOD-02/03/04); Messlauf mit `LUMINA_PERF_LOG=1`.
- Verhalten des registrierten Native-Textures bei Fensterresize/Monitorwechsel
  (Present-Target folgt VRAM-Dims, nicht Pane-Größe — Skalierungsqualität).
- Device-Loss/Adapter-Entzug in der Praxis (R2-GPU-06) sowie GPU-Fallback-
  Anzeige (R2-GUIMOD-06) im echten Workflow.
- Numerisches Verhalten echter BiRefNet/SAM-2-Gewichte und dynamischer ONNX-
  Shapes (Fixtures pending integration).
- Windows/Linux-Builds der nativen Pfade (pkg-config libraw/lensfun,
  `Backends::METAL`-Auswirkung, R2-GPU-07).
- Peak-Memory realer Decodes vs. `MemoryBudget` (Budget modelliert nur den
  Finalbuffer).
- kittest-Snapshots sichern Renderer-Wechsel ab, prüfen aber den GPU-Present-
  Inhalt nicht headless — manueller Block-C-Test steht aus.

## Befundzahlen

- **kritisch:** 0
- **hoch:** 12 (davon MVP-blockierend: R2-MCP-02, R2-GUIMOD-01, R2-CLI-01)
- **mittel:** 22
- **niedrig:** 26
- **Gesamt:** 60
