# Capability-Matrix: native CLI / Desktop / Browser (WASM)

**Feature:** F-006 Capability-Matrix (native CLI, Desktop, Browser)

Diese Matrix dokumentiert plattformabhängige Fähigkeiten getrennt nach nativem
CLI, nativer Desktop-GUI und Browser/WASM. Sie ist die verbindliche
Ergänzung zu `cli-gui-wasm.md` und zur `docs/adr/`-Entscheidung zum RAW-Backend.

| Fähigkeit | native CLI | Desktop (eframe) | Browser (WASM) |
| --- | --- | --- | --- |
| Raster (PNG/JPEG/WebP) laden | ja | ja | ja (Upload) |
| Raster entwickeln (Exposure/Kontrast/Highlights/Shadows) | ja | ja | ja (portabler Core) |
| Vorschau / Histogramm | nein (Headless) | ja | geplant (portabler Core) |
| RAW dekodieren (LibRaw, nativ) | **ja (MVP)** | **ja (MVP)** | **nein (post-MVP)** |
| RAW-Datei per Pfad/Drag-and-Drop öffnen | ja | ja | nein (Upload, RAW offen) |
| Auto-Tone / Match Total Exposure | ja | ja | ja (portabler Core) |
| Virtuelle Kopien / Presets | ja | ja | gleiches Rezeptmodell (post-MVP UI) |
| Sidecar schreiben (nativ, neben Original) | ja | ja | nein (Browser-Speichern offen) |
| ONNX-Inferenz (BiRefNet/SAM2) | ja (MVP) | ja (MVP) | offen |
| Persistente AI-Masken | post-MVP | post-MVP | offen |
| Export (PNG/JPEG/WebP) | ja | ja | offen |
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
  Ein Standard-`wasm32`-Build des Workspace bleibt grün, solange `zdata` nicht
  aktiviert wird.
- **Workspace-weites `wasm32`-Gate:** Ein `cargo build --target
  wasm32-unknown-unknown` des gesamten Workspace schlägt fehl, sobald irgendein
  Crate das `zdata`-Feature einschaltet, weil dann transitiv `zstd-sys` (native)
  gebaut werden müsste. **Capability-Entscheidung: `zdata` ist native-only** —
  für WASM-Ziele darf das `zdata`-Feature nicht aktiviert werden. `--no-default-
  features` reicht nicht, wenn ein anderes Crate `zdata` explizit einschaltet;
  dann muss dieses Crate das Feature für WASM aus-`cfg`-gaten. Die
  WASM-Pfade in `lumina-core`/`lumina-sidecar` sind bereits
  `cfg(target_arch = "wasm32")`-gekapselt; `zdata`/`zstd` ist die einzige
  bekannte native Bremswirkung fürs workspace-weite wasm32-Gate.

## Offene Browser-Punkte

- ONNX/Masken/Export im Browser sind explizit **offen** und werden vor Freigabe
  einzeln als Capability bewertet und dokumentiert.
- Die Capability-Anzeige im Browser muss RAW und die genannten Punkte klar als
  nicht verfügbar ausweisen, solange sie nicht implementiert sind.
