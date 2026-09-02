# Capability-Matrix: native CLI / Desktop / Browser (WASM)

**Features:** F-006 Capability-Matrix (native CLI, Desktop, Browser),
F-069 Browser-Dateiimport/Speicher/Export, F-070 ONNX im Browser, F-071
quantitative Limits

Diese Matrix dokumentiert plattformabhängige Fähigkeiten getrennt nach nativem
CLI, nativer Desktop-GUI und Browser/WASM. Sie ist die verbindliche
Ergänzung zu `cli-gui-wasm.md`, zum SOLL für Browser-Import/Speicher/Export und
ONNX (`feature/platform/wasm-limits.md`, F-069…F-071) und zur
`docs/adr/`-Entscheidung zum RAW-Backend.

| Fähigkeit | native CLI | Desktop (eframe) | Browser (WASM) |
| --- | --- | --- | --- |
| Raster (PNG/JPEG/WebP) laden | ja | ja | ja (F-069: File-Picker; Drag-and-drop erst nach Async-Brücke) |
| Raster entwickeln (Exposure/Kontrast/Highlights/Shadows) | ja | ja | ja (portabler Core) |
| Vorschau / Histogramm | nein (Headless) | ja | geplant (portabler Core) |
| RAW dekodieren (LibRaw, nativ) | **ja (MVP)** | **ja (MVP)** | **nein (post-MVP)** |
| RAW-Datei per Pfad/Drag-and-Drop öffnen | ja | ja | nein (Upload, RAW offen) |
| Auto-Tone / Match Total Exposure | ja | ja | ja (portabler Core) |
| Virtuelle Kopien / Presets | ja | ja | gleiches Rezeptmodell (post-MVP UI) |
| Sidecar schreiben (nativ, neben Original) | ja | ja | nein — temporärer Speicher OPFS (F-069) |
| ONNX-Inferenz (BiRefNet/SAM2) | ja (MVP) | ja (MVP) | optional, off by default (F-070, Feature `onnx-wasm`) |
| Persistente AI-Masken | post-MVP | post-MVP | post-MVP (OPFS-Artefakte, F-069/F-070) |
| Export (PNG/JPEG/WebP) | ja | ja | ja — Download-/OPFS-Modell, byte-identisch (F-069) |
| Optionale zentrale Indizierung (`lumina-index`) | post-MVP (optional) | post-MVP (optional) | offen |

## RAW im Browser (post-MVP, vorbereitet)

- Der native LibRaw-Adapter (`lumina-raw`) ist über
  `cfg(target_arch = "wasm32")` gekapselt und liefert im Browser
  `RawError::UnsupportedPlatform` (MVP: RAW-frei).
- Für die spätere Browser-Anbindung ist `libraw-wasm` (Emscripten/npm)
  vorgesehen. Die Rust-Seite würde die JS-`LibRaw`-Klasse als
  `wasm-bindgen`-Extern deklarieren und `open`/`metadata`/`imageData` in
  `lumina-raw::decode_bytes` bzw. `RawMetadata` übersetzen.
- Das Backend ist hinter dem Feature `wasm-js` gekapselt und nur für
  `cfg(target_arch = "wasm32")` aktiv; der native Pfad bleibt Default für
  CLI/Desktop.
- Ein unabhängiger Verifizierungs-Agent prüft später, dass derselbe
  `decode_bytes`-Vertrag (Orientierung, Metadaten, 8/16-bit) in beiden
  Backends gilt.

## Binäre Sidecar-Artefakte (`zdata`) und zstd (native-only)

- Das binäre Sidecar-Artefakt `<original>.lumina.zdata` (große Masken-/
  Source-Action-Daten) wird mit `zstd` komprimiert. `zstd` bindet das native
  C-Backend `zstd-sys` und ist **nicht WASM-kompilierbar** (F-006 /
  R2-SIDECAR-ZDATA-WASM).
- Die `zdata`-Funktion ist in `lumina-sidecar` als optionales, **nicht
  default**-Feature hinterlegt (`[features] default = []; zdata = ["dep:zstd"]`).
- **Workspace-weites `wasm32`-Gate (gelöst, R2-SIDECAR-ZDATA-WASM):** Die
  `zstd`-Dependency ist in `crates/lumina-sidecar/Cargo.toml` **per
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` target-gegatet**.
  Damit existiert `zstd`/`zstd-sys` nur in nativen Dependency-Graphen; selbst
  wenn ein Konsument (CLI, MCP, GUI) das `zdata`-Feature einschaltet, zieht es
  bei einem `cargo check --target wasm32-unknown-unknown` **kein** `zstd-sys`
  mehr in den wasm32-Graphen. `cargo check --target wasm32-unknown-unknown
  -p lumina-sidecar --features zdata` ist grün.
- **Code-Gating:** Der Codec (`src/zdata.rs`) wird nur unter
  `#[cfg(all(feature = "zdata", not(target_arch = "wasm32")))]` kompiliert; im
  WASM-Build existiert auch mit aktiviertem `zdata`-Feature kein zstd-Code.
  `artifact_status` hat dafür in jeder Konfiguration eine Definition: mit
  Codec (einzig auf nativ+zdata) führt es die tiefe BLAKE3-/Container-Prüfung
  aus, auf WASM/zdata-frei greift die strukturelle Variante ohne Codec
  (dokumentierte Grenze, kein stiller Fallback — Verhalten identisch bis zur
  eager-Checksummen-Pass).
- **Browser (WASM):** persistierte Masken/Source-Actions aus `.lumina.zdata`
  werden im Browser als **nicht verfügbar beziehungsweise unverifizierbar**
  gemeldet (Artefaktstatus-Prüfung ohne Codec); eine Neuberechnung ist für
  WASM ein separater, ausdrücklich aktivierter Schritt. Das entspricht dem
  Prinzip „kein stiller Fallback bei fehlenden Artefakten“.
- **Capability-Entscheidung:** `zdata`/`zstd` bleibt **native-only**; die
  Target-Gating-Lösung macht das workspace-weite wasm32-Gate wieder grün, ohne
  dass ein Konsument sein Feature-Verhalten für WASM umbauen muss. Die
  WASM-Pfade in `lumina-core`/`lumina-sidecar` sind bereits
  `cfg(target_arch = "wasm32")`-gekapselt; `zdata`/`zstd` ist damit keine
  native Bremswirkung mehr fürs workspace-weite wasm32-Gate.
- **Consumer-Gating (erledigt, FOLLOWUP-WASM-ZDATA-CONSUMER e60a9ad):** Die Konsumenten
  `lumina-cli`/`lumina-mcp`/`lumina-gui` aktivieren `zdata` nur per
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` (Cargo-Gate) und
  gaten ihre zdata-Importe/Aufrufe per `cfg(not(target_arch = "wasm32"))`
  (Code-Gate). Auf `wasm32` meldet der Sidecar-Stub (`zdata_wasm_stub`,
  `cfg(all(feature = "zdata", wasm32))`) „zdata not available" statt `E0425`;
  `lumina-onnx` gatet `ort` ebenso per Target und liefert auf `wasm32` den
  `wasm_stub` (`RuntimeDisabled`/`DummyManifest`/`StubBackend`). Damit bleibt
  `cargo check --workspace --target wasm32-unknown-unknown` auch mit
  `--features zdata`/`onnx-rt` grün; `reject_protected_target`/`reject_protected_output`
  nutzen auf `wasm32` ein `Vec` statt Array (Cfg-Push). Keine absolute Pfade,
  kein stiller Fallback.

## Offene Browser-Punkte

- ONNX/Masken im Browser sind **optionale** Fähigkeiten (F-070, Feature
  `onnx-wasm`, off by default, aktuell `RuntimeDisabled` auf WASM, e60a9ad) und
  werden vor Freigabe einzeln als Capability bewertet und dokumentiert — auch
  Lizenz und Hash-Pin der Modelle (F-078). `cargo check --target wasm32 -p lumina-onnx --features onnx-rt` ist grün (`RuntimeDisabled`).
- Browser-Dateiimport, temporärer Speicher (OPFS), Exportmodell und
  quantitative Limits sind als SOLL in `feature/platform/wasm-limits.md`
  (F-069…F-071) normativ festgelegt (Akzeptanzkriterien/Testanforderungen/
  Capability-Anzeige je Feature, konsistent zu dieser Matrix).
- Die Capability-Anzeige im Browser muss RAW (`UnsupportedPlatform` bis `wasm-js`),
  ONNX (`RuntimeDisabled` bis `onnx-wasm` + Modell), `zdata` („not available"),
  Sidecar-nachbar-Original (OPFS) und die quantitativen Limits (45 MP/24 MP,
  8 GB/512 MiB/48 MiB, Threads, VRAM, LibRaw 0.22.2) klar als nicht verfügbar
  beziehungsweise (bei F-069/F-070) als erst nach expliziter Aktivierung
  verfügbar ausweisen, solange sie nicht implementiert sind.

## Ist-Stand 2026-09-02

**Verifiziert (FOLLOWUP-WASM-ZDATA-CONSUMER e60a9ad):** `cargo check --workspace --target wasm32-unknown-unknown`
(auch mit `--features zdata`/`onnx-rt`) grün; `cargo check -p lumina-core --target wasm32-unknown-unknown`
grün, `cargo check -p lumina-gui --target wasm32-unknown-unknown --no-default-features` grün;
`lumina-sidecar` `zdata`/`zstd` target-gegatet (`[target.'cfg(not(wasm32))'.dependencies]` + `#[cfg(all(feature="zdata", not(wasm32)))]`),
Konsumenten `lumina-cli`/`lumina-mcp`/`lumina-gui` consumer-gegatet; `lumina-onnx` native-only
(`#![cfg(not(wasm32))]` + `ort` target-gegatet, `wasm_stub` `RuntimeDisabled`). Matrix und
`wasm-limits.md` (F-069…F-071) sind konsistent: Browser-Import Upload/File-Picker + OPFS + Download-Export,
ONNX optional `onnx-wasm` off, quantitative Budgets implementiert (GUI 8 GB/1,5 GiB LRU, CLI 512 MiB, WASM 48 MiB,
VRAM 1024 MiB/4, 45 MP/24 MP, Rayon/1-Thread, LibRaw 0.22.2). `feature/README.md` verlinkt
`platform/wasm-limits.md` (Zeile 90) konsistent; `feature/platform/cli-gui-wasm.md` WASM-Abschnitt
führt dieselbe Capability-Grenze und Ist-Stand.

## Quantitative Limits (F-071)

Die detaillierten quantitativen Grenzen für Bildgröße, Speicher, Threads und
GPU stehen in `feature/platform/wasm-limits.md` (F-071). Kurzfassung
(Stand 2026-09-02, Budgets aus `preview_cache.rs` / `feature/quality/preview-cache.md`
implementiert):

| Limit | native CLI | Desktop (eframe/wgpu) | Browser (WASM) |
| --- | --- | --- | --- |
| Bildgröße (interaktiv vollauflösend) | nur RAM-begrenzt | ≤ 45 MP empfohlen (M2, Volltextur-Pooling darüber) | Soft-Limit 24 MP (≈ 96 MiB RGBA8/Frame), darüber Preview-only |
| RAW-Backend / Decode | LibRaw 0.22.2 (gepinnt, `lumina-raw`, `lumina-ci:latest`) | LibRaw 0.22.2 (gepinnt) | nein — `UnsupportedPlatform` (Post-MVP `libraw-wasm`/`wasm-js`) |
| RAM/Heap | `StageFrameCache` 512 MiB (implementiert) | **8 GB gesamt (RAM+VRAM, LRU)** — `LruPreviewCache` 7 Slots, **LRU-Cap 1,5 GiB** (implementiert; 7×24 MP ≈ 672 MiB) | 48 MiB `StageFrameCache` (implementiert), Arbeitsbudget ≤ 2 GiB (≤ 4 GiB Adressraum), Disk-Tier nativ-only |
| VRAM-Pool | n. a. (Headless; GPU optional `--features gpu`) | 1024 MiB, 4 Einträge (implementiert, `LUMINA_GPU_VRAM_BUDGET_MB`/`POOL_ENTRIES`) | n. a. (kein `lumina-gpu`) |
| Threads | Rayon (`available_parallelism`) + `batch --jobs` | Worker-Threads (Decode/Prefetch/Preview) | 1 Haupt-Thread; Worker nur mit SharedArrayBuffer (COOP/COEP) |
| `zdata`/zstd | ja (Feature `zdata`, target-gegatet) | ja (Feature `zdata`) | nein (native-only, „not available" auf WASM) |
| ONNX | ja (MVP `onnx-rt`) | ja (MVP `onnx-rt`) | optional (F-070 `onnx-wasm` off, aktuell `RuntimeDisabled` e60a9ad) |

Siehe `wasm-limits.md` für Startwert-Hinweis (F-074-Kalibrierung, `compare.mjs`
report/warn/gate) und detaillierte Bemerkungen zu 24 MP/45 MP/8 GB/LibRaw-Pin.

## ONNX-Adapter native-only (Capability-Entscheidung F-082-FOLLOWUP)

- `lumina-onnx` ist **native-only** und über
  `#![cfg(not(target_arch = "wasm32"))]` gekapselt — analog zu `lumina-raw`
  liefert es im WASM-Build einen leeren Stub (`cargo check --target
  wasm32-unknown-unknown -p lumina-onnx` bleibt grün).
- Das `onnx-rt`-Feature (echte `ort`-Runtime, Prebuilt-Binaries) darf für
  WASM-Ziele **nicht** aktiviert werden; `ort` ist eine native Abhängigkeit
  und würde das workspace-weite wasm32-Gate brechen. Die Browser-Fähigkeit
  existiert nativ nicht im MVP; als Post-MVP-Option (F-070) ist ein eigener
  WASM-Backend-Pfad (`onnx-wasm`, off by default) vorgesehen.
- Backend-Auswahl ohne stillen Fallback: `lumina_onnx::try_load_onnx_engine`
  liefert `RuntimeDisabled` (Feature aus), `OnnxRuntime` (Feature an,
  Artefakt verifiziert) oder einen harten Fehler (fehlendes/stale/fehlbenanntes
  Artefakt) — nie einen stillen Stub-Ersatz.
- **CLI-Konsum (F-082-FOLLOWUP-Rest):** `lumina-cli` fragt die echte Engine
  über `lumina_onnx::resolve::try_load_onnx_engine` an, sobald ein Lauf
  Re-Inferenz brauchen kann; das `.onnx`-Artefakt kommt aus
  `LUMINA_MODEL_PATH`. Ohne das CLI-Feature `onnx-rt` bleibt der
  `StubBackend` der Default-Draht; mit `onnx-rt` ist ein fehlendes/stale/
  unkonfiguriertes Artefakt ein harter CLI-Fehler — nie ein stiller
  Stub-Ersatz (Details in `feature/product/ai-masks.md`, F-082-FOLLOWUP-Rest).
- **ONNX im Browser (F-070):** optionaler WASM-Backend-Pfad hinter eigenem
  Feature (`onnx-wasm`), off by default, mit klarer Capability-Anzeige und
  denselben Identitäts-/Veraltungsregeln (Modellname/-version/-hash); siehe
  `feature/platform/wasm-limits.md`. `lumina-onnx` bleibt native-only und
  wird im WASM-Build nicht aktiviert.

## Geplante generative Capabilities (Doku-first, 2026-09-02, GEN-EXPAND-1 / SPOT-REMOVE-1)

Noch nicht implementiert — nur dokumentiert (kein Code, kein Gate-Bruch):

| Fähigkeit | native CLI | Desktop (eframe) | Browser (WASM) |
| --- | --- | --- | --- |
| Generatives Entfernen (`inpaint`, lokal ONNX, `GenerativeEdit`) | geplant, `lumina-onnx` | geplant, `lumina-onnx` | nein (kein lokales ONNX ohne `onnx-wasm`) |
| Generatives Erweitern (`outpaint`/`canvas expansion >100 %`, lokal ONNX) | geplant | geplant | nein |
| Generatives Entfernen/Erweitern (Cloud-API) | nicht geplant — nur mit expliziter Capability-Entscheidung | nicht geplant | nicht geplant |
| Staub schnell (heuristisch/Clone, kein ONNX) | geplant, `lumina-core` | geplant, `lumina-core` | geplant (portabler Core) |
| Staub generativ (`inpaint_heal`, lokal ONNX, `kind = "spot_heal_generative"`) | geplant, `lumina-onnx` | geplant, `lumina-onnx` | nein (`onnx-wasm` off, `missing`/`RuntimeDisabled`) |

Lokal ONNX vs. Cloud sind **getrennte** Capabilities (kein stiller Fallback, siehe `feature/product/generative-expand.md` und `feature/product/spot-removal.md`). `zdata`/`zstd` bleibt native-only (WASM `missing`/`not available`).
