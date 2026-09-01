# WASM/Browser: Import, Speicher, Export und Limits (F-069…F-071)

**Features:** F-069 Browser-Dateiimport, temporärer Speicher und Exportmodell,
F-070 ONNX im Browser, F-071 quantitative native-/Desktop-/Browser-Limits

## Inhaltsverzeichnis

- [Ziel und Einordnung](#ziel-und-einordnung)
- [Ist-Stand](#ist-stand)
- [Browser-Dateiimport (F-069)](#browser-dateiimport-f-069)
- [Temporärer Speicher (F-069)](#temporärer-speicher-f-069)
- [Exportmodell im Browser (F-069)](#exportmodell-im-browser-f-069)
- [ONNX im Browser (F-070)](#onnx-im-browser-f-070)
- [Quantitative Limits (F-071)](#quantitative-limits-f-071)
- [Capability-Anzeige](#capability-anzeige)
- [Abnahme](#abnahme)

## Ziel und Einordnung

Dieses Dokument definiert das normative SOLL für die Post-MVP-Browser-
Fähigkeiten F-069…F-071. Es ergänzt die qualitative Capability-Matrix
(`feature/platform/capability-matrix.md`) um das Browser-Dateimodell
(Import/Speicher/Export), die ONNX-Capability im Browser und **quantitative**
Limits für Bildgröße, Speicher, Threads und GPU getrennt nach native CLI,
Desktop-GUI und Browser/WASM.

Grundsatz (unverändert aus `feature/platform/cli-gui-wasm.md`): Der
WASM-/Trunk-Pfad bleibt buildbar und wird als dokumentierte Capability-Grenze
geführt; es findet im MVP keine Funktionsentwicklung für WASM statt. Die hier
beschriebenen Fähigkeiten sind **Post-MVP-SOLL**; bis zu ihrer Umsetzung
werden sie in der UI sichtbar als „nicht verfügbar" ausgewiesen (kein stiller
Fallback, Agents.md).

## Ist-Stand

**Stand 2026-09-01:** Capability-Matrix existiert qualitativ
(`feature/platform/capability-matrix.md`); Browser-Dateiimport/ONNX sind nicht
implementiert; quantitative Limits (Bildgröße/Speicher/Threads/GPU) sind
nirgends dokumentiert. Bekannte Browser-Eckpunkte aus dem MVP:

- File-Picker ist der WASM-Importpfad; Drag-and-drop auf WASM ist bewusst
  **nicht** unterstützt (egui-`DroppedFile`-`bytes()` existiert auf wasm32
  nicht; `bytes_async()` bräuchte eine `wasm_bindgen_futures::spawn_local`-
  Brücke, die im MVP nicht verdrahtet ist — ein Drop setzt sichtbar
  `status`/`error`, kein stilles Ignorieren).
- Browser-Sidecar-Schreiben (neben dem Original) ist im MVP nicht verfügbar;
  die Matrix-Zeile lautet „nein (Browser-Speichern offen)".
- `lumina-onnx` ist native-only (`#![cfg(not(target_arch = "wasm32"))]`),
  `lumina-gpu` hat keinen WASM-Pfad; `zdata`/`zstd` ist native-only und darf
  für WASM nicht aktiviert werden.

## Browser-Dateiimport (F-069)

Der Import in den Browser ist **Upload-basiert** — es gibt keinen
Dateisystem-Pfadzugriff wie nativ.

- **Importpfad:** File-Picker (synchroner Datei-Handle) und optional
  Drag-and-drop **nach** Verdrahtung der async-Brücke
  (`bytes_async().await` → `load_bytes`). Ohne Brücke bleibt Drag-and-drop
  sichtbar deaktiviert.
- **Formate:** Raster (PNG/JPEG/WebP) immer; RAW im Browser bleibt eine
  getrennte, separat ausgewiesene Capability (Post-MVP `libraw-wasm`/Feature
  `wasm-js`, siehe Capability-Matrix „RAW im Browser").
- **Mehrfach-Import:** mehrere Dateien in einem Durchlauf; jede Datei wird
  über ihren BLAKE3-Content-Hash identifiziert (Kollisionen/Duplikate werden
  gemeldet, nicht still zusammengeführt).
- **Orientierung/Metadaten:** es gilt derselbe `decode_bytes`/`RawMetadata`-
  Vertrag wie nativ (Orientierung, Metadaten, 8/16-Bit); ein unabhängiger
  Verifizierungs-Agent prüft die Äquivalenz der Backends.

## Temporärer Speicher (F-069)

Der Browser kennt kein „neben dem Original schreiben". Stattdessen gilt ein
zweistufiges, **ausschließlich löschbares** Speichermodell:

1. **In-Memory (ephemeral):** Die Sitzung hält das aktive Bild und den
   Bearbeitungszustand im WASM-Heap. Ein Seiten-Reload verliert nicht
   persistierte Zustände sichtbar (kein stilles „schon gespeichert").
2. **OPFS (Origin Private File System):** Persistenter, origin-gebundener
   Speicher für Sidecars (`.lumina.json`/`.lumina.zdata`), Vorschau-Cache und
   Index-Cache-Daten. OPFS ist ein löschbarer Browser-Cache: Die Sidecar-
   JSONs bleiben vollständig portable Lumina-Sidecars und können per Export
   gesichert und nativ wiederverwendet werden (Roundtrip über denselben
   `load_sidecar`-Vertrag).

Regeln:

- OPFS ist **niemals** eine zweite Quelle der Wahrheit: Der Inhalt ist ein
  1:1-Abbild des Sidecar-Bundles; fehlt oder divergiert er, gilt er als
  veraltet und wird sichtbar gemeldet.
- Der Browser kann ein importiertes Original nicht dauerhaft neben seinem
  Sidecar halten; bei erneutem Import derselben Quelle wird das OPFS-Sidecar
  über den Content-Hash wieder zugeordnet (kein stilles Duplizieren).
- Der `.lumina/`-Cache (Previews, Stage-Cache, Index-Metadaten) lebt im OPFS
  unter einer eigenen Ablage und ist vollständig löschbar ohne
  Bearbeitungsverlust.
- `zdata`/`zstd` bleibt native-only; ein Browser-Sidecar mit binären
  Artefakten wird erst mit einem WASM-kompatiblen Kompressionspfad unterstützt
  (Capability-Entscheidung `zdata` native-only, siehe Capability-Matrix).

## Exportmodell im Browser (F-069)

- Exporte werden in einem Blob/OPFS erzeugt und über einen
  `<a download>`-Link heruntergeladen; es gibt keinen Zielpfad-Dialog wie
  nativ.
- Der Byte-Strom ist **identisch** zur nativen Pipeline (deterministische
  Encoder, PNG/JPEG/WebP; `lumina_core::export_image` bleibt die gemeinsame
  Logik). Das Export-Artefakt ist ableitbar aus Quelle + Rezept + Artefakten
  und kann aus OPFS erneut exportiert werden (Re-Export).
- Ein Export überschreibt niemals ein Original (nicht-destruktiv); ein
  „Überschreiben" existiert im Browser-Modell nur als erneuter Download mit
  sichtbarer Bestätigung.
- Veraltete/fehlende Masken- oder Generative-Edit-Artefakte führen wie nativ
  zu sichtbarer Warnung oder harter Ablehnung — nie zu stillem Export eines
  anderen Standes.

## ONNX im Browser (F-070)

ONNX-Inferenz im Browser ist eine **optionale Fähigkeit** mit klarer
Capability-Anzeige:

- **Backend:** ein eigener WASM-Backend-Pfad (onnxruntime-web bzw. ein
  WASM-kompatibler ORT-Support) hinter einem separaten Cargo-Feature
  (z. B. `onnx-wasm`); `lumina-onnx` bleibt native-only und wird im
  WASM-Build nicht aktiviert. Der WASM-Pfad implementiert dieselben Traits und
  dieselbe Identität (Modellmanifest, `ModelHashStatus`, Fähigkeiten) wie das
  native Backend.
- **Capability-Anzeige:** Im Browser wird „ONNX-Inferenz" nur dann als
  verfügbar ausgewiesen, wenn das Feature aktiv **und** das erwartete,
  hash-gepinnte Modell vorhanden ist (Modell-Download/Netz wird explizit als
  Capability geführt — kein stiller Download, keine stillen Fallbacks).
- **Modelle:** Modellartefakte werden wie nativ über `model_name`/
  `model_version`/`model_hash` identifiziert und vor Integration lizenz- und
  hash-gepinnt (F-078, `feature/quality/fixtures-licensing.md`); Browser-
  Ladepfade (fetchen aus Bundle/URL) werden als Teil der Modellidentität
  dokumentiert.
- **Kein stiller Fallback:** fehlendes/nicht verfügbares Modell im Browser
  erzeugt denselben sichtbaren Status wie nativ (`MissingModel`/
  `ModelArtifactStale`/`RuntimeDisabled`); der Stub wird nie als echte
  Inferenz ausgegeben.
- **Persistente AI-Masken:** Maskenartefakte bleiben in einem
  WASM-kompatiblen Artefaktformat (OPFS); gültige Matten werden wiederverwendet
  und nicht ungefragt neu berechnet (Agents.md, Produktprinzipien).

## Quantitative Limits (F-071)

Die Limits sind **Startwerte**, soweit nicht als implementiert markiert; sie
werden nach Umsetzung durch Messungen/Benchmarks kalibriert und bei bewusster
Änderung im selben Commit begründet (F-074-Methodik,
`feature/quality/performance-benchmarks.md`). „n. a." = Fähigkeit nicht
vorhanden.

| Limit | native CLI | Desktop (eframe/wgpu) | Browser (WASM) |
| --- | --- | --- | --- |
| **Bildgröße (Quellpixel)** | kein harter Scan-/Decode-Cap; begrenzt durch verfügbaren RAM (8-Bit-RGBA ≈ 4 Byte/Pixel, transiente Puffer) | kein hartes Decode-Limit; **interaktive Vollauflösung ≤ 45 MP** empfohlen (darüber Volltextur-Pooling/ROI-Zoom, dokumentierte Grenze GPU-Pfad) | **Soft-Limit 24 MP** (≈ 96 MiB RGBA8 je Frame, transiente Decode-/Render-Puffer 2–3×); darüber Preview-/Teilregion-only; hartes Limit durch Heap |
| **RAM/Heap** | workstationspezifisch; `StageFrameCache`-Budget nativ **512 MiB** (implementiert) | `StageFrameCache` **512 MiB** (implementiert), LRU | wasm32-Adressraum **≤ 4 GiB**, empfohlenes Arbeitsbudget **≤ 2 GiB**; `StageFrameCache`-Budget **48 MiB** (implementiert) |
| **VRAM (GPU)** | n. a. (Headless; GPU nur mit `--features gpu` optional) | **Pool-Budget 1024 MiB** (`LUMINA_GPU_VRAM_BUDGET_MB`), **4 Pool-Einträge** (`LUMINA_GPU_VRAM_POOL_ENTRIES`, implementiert) | n. a. — kein `lumina-gpu`/wgpu-Pfad im Browser; WebGPU wäre eine separat dokumentierte Option (Post-MVP, eigener Adapter) |
| **Threads** | Rayon-Pool über `available_parallelism` (Kernanzahl); `batch --jobs` | Hintergrund-Worker-Threads (Decode, Prefetch, Idle-Queue) | **1 Haupt-Thread**; Worker-Pool nur mit **SharedArrayBuffer** (COOP/COEP-Header) — ohne diese Header single-threaded (dokumentierte Capability, kein stiller Parallel-Check) |
| **Sidecar neben Original** | ja | ja | nein — OPFS (siehe oben) |
| **Binäre Artefakte (`zdata`/zstd)** | ja (Feature `zdata`) | ja (Feature `zdata`) | nein (Feature `zdata` native-only; WASM-Pfad offen) |
| **ONNX-Inferenz** | ja (MVP, `onnx-rt`) | ja (MVP, `onnx-rt`) | optional (F-070, Feature `onnx-wasm`, off by default) |

### Bemerkungen zu den Zahlen

- **24-MP-Soft-Limit (Browser):** Ein 24-MP-RGBA8-Frame belegt 96 MiB; bei
  einem 2-GiB-Arbeitsbudget bleiben davon mindestens ein Bild plus
  Render-/Cache-Puffer ohne das 48-MiB-Stage-Cache-Budget zu sprengen. Der
  Decode bleibt für kleinere Bilder vollauflösend; größere Bilder werden in
  der Vorschau auf Screen-Auflösung herunterskaliert (sichtbar dokumentierter
  Zustand, kein stilles Bescheidenheiten-Fallback).
- **45-MP-Empfehlung (Desktop):** entspricht der dokumentierten GPU-Grenze
  (>45-MP-Zoom → Volltextur-Pooling statt 512²-Tile-Cache, M2,
  `feature/architecture/pipeline.md`, „Implementierungsstatus GPU-Pfad").
- **Threads (Browser):** SharedArrayBuffer erfordert Cross-Origin-Isolation
  (COOP/COEP). Ohne diese Header ist der WASM-Build single-threaded; die
  Capability-Anzeige zeigt den tatsächlich aktiven Modus (kein Rat aus der
  Anzahl verfügbarer Kerne).

## Capability-Anzeige

Im Browser müssen alle nicht verfügbaren oder nicht aktivierten Fähigkeiten
sichtbar ausgewiesen werden (unverändert aus der Capability-Matrix): RAW-
Decode (bis `wasm-js`), ONNX-Inferenz (bis `onnx-wasm` + Modell), Sidecar-
schreiben neben dem Original, `zdata`-Artefakte, GPU/`lumina-gpu`, sowie die
quantitativen Limits (Bildgröße, Heap, Threads). Eine nicht aktivierte
Fähigkeit wird nie als verfügbar dargestellt und nie stillschweigend ersetzt.

## Abnahme

- Browser-Import (File-Picker) lädt Rasterdateien; Drag-and-drop ist nach
  Async-Brücke nutzbar, vorher sichtbar deaktiviert.
- OPFS-Sidecar-Roundtrip: importierte Quelle + Rezept überleben einen Reload
  und lassen sich als portables Lumina-Sidecar exportieren und nativ wieder
  verwenden; Verlust/Diverenz wird sichtbar gemeldet.
- Browser-Export erzeugt byte-identische Artefakte zur nativen Pipeline
  (PNG/JPEG/WebP) über die gemeinsame Exportlogik.
- ONNX im Browser: Capability-Anzeige korrekt bei Feature aus/an und bei
  fehlendem Modell; kein stiller Fallback auf Stub.
- Die quantitativen Limits (Bildgröße/Speicher/Threads/GPU) sind dokumentiert,
  die implementierten Budgets (StageFrameCache 512 MiB/48 MiB, VRAM-Pool
  1024 MiB/4 Einträge) durch Messungen bestätigt.
