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
| ONNX-Inferenz (BiRefNet/SAM2) | post-MVP | post-MVP | offen |
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

## Offene Browser-Punkte

- ONNX/Masken/Export im Browser sind explizit **offen** und werden vor Freigabe
  einzeln als Capability bewertet und dokumentiert.
- Die Capability-Anzeige im Browser muss RAW und die genannten Punkte klar als
  nicht verfügbar ausweisen, solange sie nicht implementiert sind.
