# Capability-Matrix: native CLI / Desktop (WASM ENTFERNT 2026-09-04)

> WASM/Browser ist ersatzlos gestrichen (Eigentümer-Entscheidung 2026-09-04,
> F-069…F-071 entfallen). Diese Matrix beschreibt nur noch native CLI und
> Desktop. Der Ausbau aus dem Code läuft unter WASM-REMOVE-GUI/-ONNX/-REST.

**Features:** F-006 Capability-Matrix (native CLI, Desktop)

Diese Matrix dokumentiert plattformabhängige Fähigkeiten getrennt nach nativem
CLI und nativer Desktop-GUI.

| Fähigkeit | native CLI | Desktop (eframe) |
| --- | --- | --- |
| Raster (PNG/JPEG/WebP) laden | ja | ja |
| Raster entwickeln (Exposure/Kontrast/Highlights/Shadows) | ja | ja |
| Vorschau / Histogramm | nein (Headless) | ja |
| RAW dekodieren (LibRaw, nativ) | **ja (MVP)** | **ja (MVP)** |
| RAW-Datei per Pfad/Drag-and-Drop öffnen | ja | ja |
| Auto-Tone / Match Total Exposure | ja | ja |
| Virtuelle Kopien / Presets | ja | ja |
| Sidecar schreiben (nativ, neben Original) | ja | ja |
| ONNX-Inferenz (BiRefNet/SAM2) | ja (MVP) | ja (MVP) |
| Persistente AI-Masken | post-MVP | post-MVP |
| Export (PNG/JPEG/WebP) | ja | ja |
| Optionale zentrale Indizierung (`lumina-index`) | post-MVP (optional) | post-MVP (optional) |

## RAW-Backend (nativ)

- Der native LibRaw-Adapter (`lumina-raw`) liefert `decode_bytes` /
  `RawMetadata` für CLI/Desktop.

## Binäre Sidecar-Artefakte (`zdata`) und zstd (native-only)

- Das binäre Sidecar-Artefakt `<original>.lumina.zdata` (große Masken-/
  Source-Action-Daten) wird mit `zstd` komprimiert (`zstd-sys`, natives
  C-Backend).
- Die `zdata`-Funktion ist in `lumina-sidecar` als optionales, **nicht
  default**-Feature hinterlegt (`[features] default = []; zdata = ["dep:zstd"]`).
- **Code-Gating:** `artifact_status` führt mit Codec die tiefe BLAKE3-/
  Container-Prüfung aus, ohne Codec die strukturelle Variante (kein stiller
  Fallback — Verhalten identisch bis zur eager-Checksummen-Pass).
- **Capability-Entscheidung:** `zdata`/`zstd` bleibt **native-only**.
- **Consumer (FOLLOWUP-WASM-ZDATA-CONSUMER e60a9ad, historisch):** Die Konsumenten
  `lumina-cli`/`lumina-mcp`/`lumina-gui` aktivieren `zdata` direkt als
  Cargo-Dependency; `lumina-onnx` liefert `ort` direkt. Keine Target-Gates mehr.

## Quantitative Limits

Die detaillierten quantitativen Grenzen für Bildgröße, Speicher, Threads und
GPU stehen in den nativen Budget-Stores (F-074-Kalibrierung, `compare.mjs`
report/warn/gate). Kurzfassung (implementiert):

| Limit | native CLI | Desktop (eframe/wgpu) |
| --- | --- | --- |
| Bildgröße (interaktiv vollauflösend) | nur RAM-begrenzt | ≤ 45 MP empfohlen |
| RAW-Backend / Decode | LibRaw 0.22.2 (gepinnt, `lumina-raw`, `lumina-ci:latest`) | LibRaw 0.22.2 (gepinnt) |
| RAM/Heap | `StageFrameCache` 512 MiB (implementiert) | **8 GB gesamt (RAM+VRAM, LRU)** — `LruPreviewCache` 7 Slots, **LRU-Cap 1,5 GiB** (implementiert; 7×24 MP ≈ 672 MiB) |
| VRAM-Pool | n. a. (Headless; GPU optional `--features gpu`) | 1024 MiB, 4 Einträge (implementiert, `LUMINA_GPU_VRAM_BUDGET_MB`/`POOL_ENTRIES`) |
| Threads | Rayon (`available_parallelism`) + `batch --jobs` | Worker-Threads (Decode/Prefetch/Preview) |
| `zdata`/zstd | ja (Feature `zdata`) | ja (Feature `zdata`) |
| ONNX | ja (MVP `onnx-rt`) | ja (MVP `onnx-rt`) |

## ONNX-Adapter native-only (Capability-Entscheidung F-082-FOLLOWUP)

- `lumina-onnx` ist **native-only** (kein Stub mehr — WASM gestrichen).
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

## Geplante generative Capabilities (Doku-first, 2026-09-02, GEN-EXPAND-1 / SPOT-REMOVE-1)

Noch nicht implementiert — nur dokumentiert (kein Code, kein Gate-Bruch):

| Fähigkeit | native CLI | Desktop (eframe) |
| --- | --- | --- |
| Generatives Entfernen (`inpaint`, lokal ONNX, `GenerativeEdit`) | geplant, `lumina-onnx` | geplant, `lumina-onnx` |
| Generatives Erweitern (`outpaint`/`canvas expansion >100 %`, lokal ONNX) | geplant | geplant |
| Generatives Entfernen/Erweitern (Cloud-API) | nicht geplant — nur mit expliziter Capability-Entscheidung | nicht geplant |
| Staub schnell (heuristisch/Clone, kein ONNX) | geplant, `lumina-core` | geplant, `lumina-core` |
| Staub generativ (`inpaint_heal`, lokal ONNX, `kind = "spot_heal_generative"`) | geplant, `lumina-onnx` | geplant, `lumina-onnx` |

Lokal ONNX vs. Cloud sind **getrennte** Capabilities (kein stiller Fallback, siehe `feature/product/generative-expand.md` und `feature/product/spot-removal.md`). `zdata`/`zstd` bleibt native-only.
