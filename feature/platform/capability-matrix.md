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
  `onnx-wasm`, off by default) und werden vor Freigabe einzeln als Capability
  bewertet und dokumentiert — auch Lizenz und Hash-Pin der Modelle (F-078).
- Browser-Dateiimport, temporärer Speicher (OPFS), Exportmodell und
  quantitative Limits sind als SOLL in `feature/platform/wasm-limits.md`
  (F-069…F-071) festgelegt.
- Die Capability-Anzeige im Browser muss RAW und die genannten Punkte klar als
  nicht verfügbar beziehungsweise (bei F-069/F-070) als erst nach expliziter
  Aktivierung verfügbar ausweisen, solange sie nicht implementiert sind.

## Quantitative Limits (F-071)

Die detaillierten quantitativen Grenzen für Bildgröße, Speicher, Threads und
GPU stehen in `feature/platform/wasm-limits.md` (F-071). Kurzfassung:

| Limit | native CLI | Desktop (eframe/wgpu) | Browser (WASM) |
| --- | --- | --- | --- |
| Bildgröße (interaktiv vollauflösend) | nur RAM-begrenzt | ≤ 45 MP empfohlen | Soft-Limit 24 MP, darüber Preview-only |
| RAM-Budget (`StageFrameCache`) | 512 MiB (implementiert) | 512 MiB (implementiert) | 48 MiB (implementiert), Arbeitsbudget ≤ 2 GiB |
| VRAM-Pool | n. a. (Headless) | 1024 MiB, 4 Einträge (implementiert) | n. a. (kein `lumina-gpu`) |
| Threads | Rayon (`available_parallelism`) | Worker-Threads | 1 Haupt-Thread; Worker nur mit SharedArrayBuffer (COOP/COEP) |

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
