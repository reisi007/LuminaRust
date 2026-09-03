# WASM/Browser: Import, Speicher, Export und Limits (F-069…F-071)

> **ENTFERNT (2026-09-04, Eigentümer-Entscheidung):** WASM/Browser ist ersatzlos
> gestrichen. Dieses Dokument ist historisch und nicht normativ; F-069…F-071
> entfallen. Der Ausbau aus dem Code läuft unter Task WASM-REMOVE-01.

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

**Stand 2026-09-02 (F-069…F-071 BESTANDEN verifiziert 2026-09-02, 287fe75/e60a9ad):** Capability-Matrix qualitativ
erweitert um quantitative Kurzfassung (8 GB GUI / 512 MiB CLI / 48 MiB WASM, VRAM-Pool 1024 MiB/4,
45 MP/24 MP, LibRaw 0.22.2, Threads),
`lumina-onnx` native-only mit `wasm_stub` (`RuntimeDisabled`/`DummyManifest`/`StubBackend`,
`ort` per `[target.'cfg(not(wasm32))'.dependencies]` gegatet — `cargo check --target wasm32 -p lumina-onnx --features onnx-rt` grün liefert `RuntimeDisabled`),
`zdata`/`zstd` native-only target-gegatet (sidecar + cli/mcp/gui consumer gating);
`cargo check --workspace --target wasm32-unknown-unknown` (auch mit `zdata`/`onnx-rt`) ist grün.
`cargo check -p lumina-core --target wasm32-unknown-unknown` und
`cargo check -p lumina-gui --target wasm32-unknown-unknown --no-default-features` sind grün.
`StageFrameCache`-Budgets implementiert (GUI 8 GB gesamt, LRU-Cap 1,5 GiB, CLI 512 MiB, WASM 48 MiB;
`crates/lumina-core/src/preview_cache.rs`), RAW-Backend LibRaw 0.22.2 (Docker `lumina-ci:latest`,
Homebrew lokal).
Browser-Dateiimport/ONNX-Inferenz bleiben Post-MVP. F-069 (File-Picker vs `bytes_async`, OPFS 2-stufig löschbar `zdata not available`, Export byte-identisch), F-070 (onnx-wasm off-by-default `RuntimeDisabled` Capability-Anzeige) und F-071 (quantitative Limits je Plattform) sind normativ vervollständigt und unabhängig verifiziert BESTANDEN (Doku-first, kein Code, alle Gates grün). Bekannte Browser-Eckpunkte aus dem MVP:

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
  sichtbar deaktiviert (MVP: `status`/`error` sichtbar, `warn!`-Log, kein
  stilles Ignorieren — `crates/lumina-gui/src/lib.rs` `#[cfg(target_arch="wasm32")]`).
- **Formate:** Raster (PNG/JPEG/WebP) immer; RAW im Browser bleibt eine
  getrennte, separat ausgewiesene Capability (Post-MVP `libraw-wasm`/Feature
  `wasm-js`, siehe Capability-Matrix „RAW im Browser"; im WASM-Build liefert
  `lumina-raw` `RawError::UnsupportedPlatform`).
- **Mehrfach-Import:** mehrere Dateien in einem Durchlauf; jede Datei wird
  über ihren BLAKE3-Content-Hash identifiziert (Kollisionen/Duplikate werden
  gemeldet, nicht still zusammengeführt).
- **Orientierung/Metadaten:** es gilt derselbe `decode_bytes`/`RawMetadata`-
  Vertrag wie nativ (Orientierung, Metadaten, 8/16-Bit, EXIF-Grenze: `camera_white_balance`
  erst mit F-036-N1 im Core-Pfad; LibRaw-Version fließt in `decode_version`/
  Render-Key); ein unabhängiger Verifizierungs-Agent prüft die Äquivalenz der
  Backends.

### F-069 Akzeptanzkriterien (Import)

- File-Picker lädt Raster (PNG/JPEG/WebP) im Browser; ohne async-Brücke ist
  Drag-and-drop sichtbar deaktiviert und loggt `warn!`.
- Mehrfach-Import über BLAKE3-Hash dedupliziert/meldet Duplikate, kein stilles
  Zusammenführen.
- RAW-Import im Browser wird als nicht verfügbare Capability ausgewiesen
  (`UnsupportedPlatform`), bis `libraw-wasm`/`wasm-js` implementiert ist.

### F-069 Testanforderungen (Import)

- WASM-Build-Gate: `cargo check -p lumina-core --target wasm32-unknown-unknown`
  und `cargo check -p lumina-gui --target wasm32-unknown-unknown --no-default-features`
  grün (RAW-Code `cfg(target_arch="wasm32")`-gekapselt).
- Verifizierungs-Agent prüft `decode_bytes`/`RawMetadata`-Vertrag (Orientierung,
  Metadaten, 8/16-Bit) zwischen nativem LibRaw-0.22.2-Pfad und künftigem
  `libraw-wasm`-Pfad bei Einführung.

### F-069 Capability-Anzeige (Import)

- Browser-Capability-Anzeige: „Raster laden ja (File-Picker)", „RAW dekodieren
  nein (Post-MVP `wasm-js`)", „Drag-and-drop nein / nur mit async-Brücke"
  — nie als verfügbar dargestellt, nie still ersetzt.

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
  veraltet und wird sichtbar gemeldet (kein stiller Fallback, Agents.md).
- Der Browser kann ein importiertes Original nicht dauerhaft neben seinem
  Sidecar halten; bei erneutem Import derselben Quelle wird das OPFS-Sidecar
  über den Content-Hash wieder zugeordnet (kein stilles Duplizieren).
- Der `.lumina/`-Cache (Previews, Stage-Cache, Index-Metadaten) lebt im OPFS
  unter einer eigenen Ablage und ist vollständig löschbar ohne
  Bearbeitungsverlust (F-067-Semantik: DB/Cache löschbar, Sidecar gewinnt).
- `zdata`/`zstd` bleibt native-only; ein Browser-Sidecar mit binären
  Artefakten wird als **nicht verfügbar / unverifizierbar** gemeldet
  (Artefaktstatus ohne Codec) und erst mit einem WASM-kompatiblen
  Kompressionspfad unterstützt (Capability-Entscheidung `zdata` native-only,
  siehe Capability-Matrix; WASM-`zdata_wasm_stub` meldet „not available").

### F-069 Akzeptanzkriterien (Speicher)

- In-Memory-Session geht bei Reload sichtbar verloren (kein stilles „schon
  gespeichert"); OPFS hält Sidecar-Bundle, Preview-Disk und Index-Cache
  origin-gebunden.
- OPFS-Sidecars sind portable Lumina-Sidecars (Roundtrip `load_sidecar` nativ
  wiederverwendbar, relative Pfade, keine absoluten Pfade).
- OPFS ist löschbarer Cache: Diverenz/Fehlen → sichtbar veraltet, Re-Import
  über Content-Hash zugeordnet.

### F-069 Testanforderungen (Speicher)

- OPFS-Sidecar-Roundtrip: importierte Quelle + Rezept überleben Reload,
  exportierbares `.lumina.json`/`.lumina.zdata` ist nativ byte-identisch
  wiederverwendbar (JSON-Roundtrip + `artifact_status` ohne Codec auf WASM).
- Löschbarkeitstest: Leeren des OPFS-Caches (`.lumina/`-Äquivalent) zerstört
  kein Rezept/keine virtuelle Kopie — Rebuild aus Sidecar bleibt identisch
  (F-067-Semantik, analog `index.md`).

### F-069 Capability-Anzeige (Speicher)

- „Sidecar neben Original: nein — OPFS" und „binäre Artefakte (`zdata`): nein
  (native-only)" werden als nicht verfügbare Capability sichtbar ausgewiesen
  (nie still ersetzt).

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

### F-069 Akzeptanzkriterien (Export)

- Browser-Export erzeugt byte-identische Artefakte zur nativen Pipeline
  (PNG/JPEG/WebP) über die gemeinsame `export_image`-Logik (deterministische
  Encoder, gleiche Eingabe → gleiche Bytes; PNG als Byte-Vergleichsanker).
- Re-Export aus OPFS ist möglich; Original wird nie überschrieben.

### F-069 Testanforderungen (Export)

- Byte-Vergleichstest GUI-Export ↔ CLI-Export (nativ vs. WASM-Pfad gleiche
  Eingabe, gleiche Bytes; `export_image` ist die einzige Logik).
- Veraltete/fehlende Masken-/Generative-Artefakte: sichtbare Warnung/harte
  Ablehnung, nie stiller Export eines anderen Standes (F-082-Semantik).

### F-069 Capability-Anzeige (Export)

- „Export: ja — Download-/OPFS-Modell, byte-identisch" wird als verfügbare
  Capability ausgewiesen; Zielpfad-Dialog als nicht verfügbare native
  Capability abgrenzt.

## ONNX im Browser (F-070)

ONNX-Inferenz im Browser ist eine **optionale Fähigkeit** mit klarer
Capability-Anzeige (Post-MVP, off by default):

- **Ist-Stand 2026-09-02 (e60a9ad, verifiziert):** `lumina-onnx` ist native-only
  (`#![cfg(not(target_arch = "wasm32"))]` + `[target.'cfg(not(wasm32))'.dependencies]` für `ort`);
  auf `wasm32` liefert das Crate einen leeren `wasm_stub` mit
  `RuntimeDisabled` / `DummyManifest` / `StubBackend` — `cargo check --target wasm32-unknown-unknown -p lumina-onnx --features onnx-rt`
  ist grün und liefert `RuntimeDisabled` (kein `ort` im WASM-Graphen). `try_load_onnx_engine`
  kennt nur `RuntimeDisabled` (Feature aus) / `OnnxRuntime` (Feature an, Artefakt
  verifiziert) / harten Fehler — nie stillen Stub-Ersatz (F-082-Semantik).
- **Backend (SOLL Post-MVP):** ein eigener WASM-Backend-Pfad (onnxruntime-web
  bzw. ein WASM-kompatibler ORT-Support) hinter einem separaten Cargo-Feature
  (z. B. `onnx-wasm`, off by default); `lumina-onnx` bleibt native-only und
  wird im WASM-Build nicht aktiviert. Der WASM-Pfad implementiert dieselben
  Traits und dieselbe Identität (Modellmanifest, `ModelHashStatus`, Fähigkeiten)
  wie das native Backend.
- **Capability-Anzeige:** Im Browser wird „ONNX-Inferenz" nur dann als
  verfügbar ausgewiesen, wenn das Feature aktiv **und** das erwartete,
  hash-gepinnte Modell vorhanden ist (Modell-Download/Netz wird explizit als
  Capability geführt — kein stiller Download, keine stillen Fallbacks).
  Aktuell: „ONNX im Browser: optional, off by default (Feature `onnx-wasm`) —
  im MVP nicht verfügbar" (Capability-Matrix F-070).
- **Modelle:** Modellartefakte werden wie nativ über `model_name`/
  `model_version`/`model_hash` identifiziert und vor Integration lizenz- und
  hash-gepinnt (F-078, `feature/quality/fixtures-licensing.md`); Browser-
  Ladepfade (fetchen aus Bundle/URL) werden als Teil der Modellidentität
  dokumentiert.
- **Kein stiller Fallback:** fehlendes/nicht verfügbares Modell im Browser
  erzeugt denselben sichtbaren Status wie nativ (`MissingModel`/
  `ModelArtifactStale`/`RuntimeDisabled` — `wasm_stub` liefert aktuell
  `RuntimeDisabled`); der Stub wird nie als echte Inferenz ausgegeben.
- **Persistente AI-Masken:** Maskenartefakte bleiben in einem
  WASM-kompatiblen Artefaktformat (OPFS, Post-MVP); gültige Matten werden
  wiederverwendet und nicht ungefragt neu berechnet (Agents.md, Produktprinzipien);
  im MVP werden `zdata`-Artefakte im Browser als nicht verfügbar/unverifizierbar
  gemeldet.

### F-070 Akzeptanzkriterien

- `cargo check --workspace --target wasm32-unknown-unknown` (auch mit
  `--features zdata`/`onnx-rt`) bleibt grün; `lumina-onnx` auf WASM liefert
  `RuntimeDisabled`, nie stillen Stub-Cache.
- Capability-Anzeige im Browser: „ONNX-Inferenz: nicht verfügbar (Post-MVP
  `onnx-wasm` off)" solange Feature nicht aktiv + Modell nicht vorhanden;
  bei aktiviertem Feature + hash-gepinntem Modell: verfügbar (kein stiller
  Download, Lizenz/hash gepinnt per F-078).
- Fehlendes/stales Modell → sichtbarer Fehler (`MissingModel`/`ModelArtifactStale`/
  `RuntimeDisabled`), identisch zu nativ.

### F-070 Testanforderungen

- WASM-Gate: `cargo check -p lumina-onnx --target wasm32-unknown-unknown --features onnx-rt`
  grün und liefert `RuntimeDisabled`; `grep` sichert Target-Gating
  (`[target.'cfg(not(wasm32))'.dependencies]`).
- Capability-Tests: Feature aus → `RuntimeDisabled`; Feature an + fehlendes
  Artefakt → harter Fehler (kein stiller Fallback) — analog
  `lumina_onnx::resolve::try_load_onnx_engine`.
- Bei Einführung `onnx-wasm`: gleicher Identitäts-/Veraltungsvertrag
  (Modellname/-version/-hash, Inferenzauflösung, `source_fingerprint`,
  Artefakt-Prüfsumme wie Agents.md AI-Masken) und Lizenz/hash-Pin (F-078).

### F-070 Capability-Anzeige

- Browser führt ONNX als optionale, separat bewertete Capability (off by default,
  Feature `onnx-wasm`); der Status wird deterministisch aus Feature-Flag +
  Modell-Präsenz abgeleitet und nie als verfügbar dargestellt, solange nicht
  implementiert.

## Quantitative Limits (F-071)

Die Limits sind **Startwerte**, soweit nicht als implementiert markiert; sie
werden nach Umsetzung durch Messungen/Benchmarks kalibriert und bei bewusster
Änderung im selben Commit begründet (F-074-Methodik,
`feature/quality/performance-benchmarks.md`, `scripts/perf/compare.mjs`
report/warn/gate). „n. a." = Fähigkeit nicht vorhanden. Quantitative Werte
stammen aus `crates/lumina-core/src/preview_cache.rs` (implementiert),
`feature/quality/preview-cache.md` (8 GB GUI gesamt, LRU-Cap 1,5 GiB, 512 MiB
CLI, 48 MiB WASM) und `docker/Dockerfile` / `Cargo.toml` (LibRaw 0.22.2, `zstd`
native-only, `ort` native-only).

| Limit | native CLI | Desktop (eframe/wgpu) | Browser (WASM) |
| --- | --- | --- | --- |
| **Bildgröße (Quellpixel)** | kein harter Scan-/Decode-Cap; begrenzt durch verfügbaren RAM (8-Bit-RGBA ≈ 4 Byte/Pixel, transiente Puffer) | kein hartes Decode-Limit; **interaktive Vollauflösung ≤ 45 MP** empfohlen (darüber Volltextur-Pooling/ROI-Zoom, dokumentierte Grenze GPU-Pfad, M2) | **Soft-Limit 24 MP** (≈ 96 MiB RGBA8 je Frame, transiente Decode-/Render-Puffer 2–3×); darüber Preview-/Teilregion-only; hartes Limit durch Heap |
| **RAW-Backend / Decode** | **LibRaw 0.22.2** (gepinnt, `lumina-raw`, `lumina-ci:latest`, `RawError::UnsupportedPlatform` nur auf WASM) — CR2/CR3/NEF/ARW/DNG/ORF/RAF/RW2/CRW/PEF/SRW/3FR/IIQ/RWL/MOS/ERF/KDC/X3F, 8/16-Bit (`RawDecodeOptions::output_bits` 8\|16 → RGBA8), EXIF-Orientierung, `camera_white_balance`/`camera_matrix`/`icc_profile`, `decode_version` = LibRaw-Version + `+luminaabiN` | wie CLI (nativ) | **nein** — `UnsupportedPlatform` (Post-MVP `libraw-wasm`/`wasm-js`); `decode_bytes`/`RawMetadata`-Vertrag gilt nach Einführung identisch (Orientierung, Metadaten, 8/16-Bit) |
| **RAM/Heap** | workstationspezifisch; `StageFrameCache` **512 MiB** (implementiert), kein GUI-LRU-Pool | **8 GB gesamt (RAM+VRAM kombiniert, dynamisch/LRU, aktiv nie evictet)** — `LruPreviewCache` 7 Slots, **LRU-Cap 1,5 GiB** (`max_bytes` 1 500 000 000, implementiert: 7×24 MP ≈ 672 MiB < 1,5 GiB < 8 GB; 7×45 MP ≈ 1,3 GiB < 1,5 GiB); `StageFrameCache` 512 MiB | wasm32-Adressraum **≤ 4 GiB**, empfohlenes Arbeitsbudget **≤ 2 GiB**; `StageFrameCache`-Budget **48 MiB** (implementiert), Disk-Tier ausschließlich nativ (WASM RAM-only LRU) |
| **VRAM (GPU)** | n. a. (Headless; GPU nur mit `--features gpu` optional, `lumina-gpu`) | **Pool-Budget 1024 MiB** (`LUMINA_GPU_VRAM_BUDGET_MB`), **4 Pool-Einträge** (`LUMINA_GPU_VRAM_POOL_ENTRIES`, implementiert), `VramState` LRU-Pool dimensionsschlüsselt, 512² `TiledCache`/`DraftPyramid` M2 | n. a. — kein `lumina-gpu`/wgpu-Pfad im Browser; WebGPU wäre eine separat dokumentierte Option (Post-MVP, eigener Adapter) |
| **Threads** | **Rayon**-Pool über `std::thread::available_parallelism` (Kernanzahl); `batch --jobs` steuert Batch-Parallelität; `lumina-raw`/`lumina-onnx` blockieren nicht den UI-Thread | Hintergrund-**Worker-Threads** (Decode, Prefetch, Preview-Cache, Thumbnail-Pool; `LuminaApp` IdleQueue → Worker-Pool, Prio-Queue, kein Wrap) | **1 Haupt-Thread**; Worker-Pool nur mit **SharedArrayBuffer** (COOP/COEP-Header) — ohne diese Header single-threaded (dokumentierte Capability, kein stiller Parallel-Check; `wasm32` Rayon-Pool auf 1 Thread begrenzt) |
| **Sidecar neben Original** | ja (`.lumina.json`/`.lumina.zdata`, atomar, relative Pfade) | ja | nein — OPFS (siehe oben) |
| **Binäre Artefakte (`zdata`/zstd)** | ja (Feature `zdata`, `[target.'cfg(not(wasm32))'.dependencies]`) | ja (Feature `zdata`) | nein (Feature `zdata` native-only; WASM `zdata_wasm_stub` „not available") |
| **ONNX-Inferenz** | ja (MVP, `onnx-rt`, `RuntimeDisabled` wenn Feature aus) | ja (MVP, `onnx-rt`) | optional (F-070, Feature `onnx-wasm`, off by default; aktuell `RuntimeDisabled` auf WASM, e60a9ad) |

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
- **8 GB GUI-Budget:** `preview-cache.md` „GUI (Desktop) 8 GB gesamt
  (RAM+VRAM kombiniert, dynamisch/LRU, aktiv nie evictet)" — `LruPreviewCache`
  (`preview_cache.rs`) hält 7 Slots (aktiv + 6 Nachbarn +4/-2) mit Cap
  1,5 GiB; CLI bleibt minimal (~512 MiB, kein Preload), WASM 48 MiB. Das
  8-GB-Budget deckt 7 Vollauflösungs-Frames (672 MiB @24 MP, ~1,3 GiB @45 MP)
  plus VRAM-Pool (1 GiB) locker ab; die LRU-Cap verhindert pathologisches
  Wachstum.
- **RAW LibRaw 0.22.2:** `docker/Dockerfile` `ARG LIBRAW_VERSION=0.22.2`
  (gepinnt, `lumina-ci:latest` OCI-Label `lumina.libraw_version`), lokal
  Homebrew `libraw` 0.22.2; `lumina-raw` unterstützt 8/16-Bit, 18 Formate
  (`RAW_EXTENSIONS`), EXIF-Orientierung, `camera_white_balance` (F-036-N1),
  `decode_version` trägt LibRaw-Version + `+luminaabiN` (F-102).
- **Threads (Browser):** SharedArrayBuffer erfordert Cross-Origin-Isolation
  (COOP/COEP). Ohne diese Header ist der WASM-Build single-threaded; die
  Capability-Anzeige zeigt den tatsächlich aktiven Modus (kein Rat aus der
  Anzahl verfügbarer Kerne). Nativ: Rayon `available_parallelism`; WASM:
  single-threaded.
- **CI/Build:** `.github/workflows/ci.yml` `wasm` job prüft
  `cargo check -p lumina-core --target wasm32-unknown-unknown` und
  `cargo check -p lumina-gui --target wasm32-unknown-unknown --no-default-features`
  (dokumentierte WASM-Grenze; `lensfun`/`gpu` native-only).

### F-071 Akzeptanzkriterien

- Quantitative Limits (Bildgröße/Speicher/Threads/GPU/RAW) sind je Plattform
  (CLI/Desktop/WASM) dokumentiert und gegen `preview_cache.rs` (8 GB/512 MiB/
  48 MiB, 45 MP/24 MP), `lumina-raw` (LibRaw 0.22.2), `lumina-gpu` (VRAM 1024 MiB/4)
  konsistent; Werte sind Startwerte mit F-074-Kalibrierungspfad.
- RAW-Limits: LibRaw 0.22.2 gepinnt (Docker + Homebrew), WASM liefert
  `UnsupportedPlatform`; späteres `libraw-wasm` nutzt identischen
  `decode_bytes`-Vertrag.
- Threads: nativ Rayon/Worker-Pool, WASM single-threaded ohne COOP/COEP —
  Capability-Anzeige weist den tatsächlich aktiven Modus aus.

### F-071 Testanforderungen

- Budget-Tests: `LruPreviewCache` 7 Slots / 1,5 GiB Cap / 8 GB GUI-Budget
  (`preview_cache.rs` Tests `lru_*`, `prefetch_window_*`), `StageFrameCache`
  512 MiB (nativ) / 48 MiB (WASM) kompilierbar auf `wasm32`.
- WASM-Gate: `cargo check --workspace --target wasm32-unknown-unknown` grün,
  `cargo check -p lumina-gui --target wasm32-unknown-unknown --no-default-features`
  grün, `cargo check -p lumina-onnx --target wasm32-unknown-unknown --features onnx-rt`
  liefert `RuntimeDisabled`.
- F-074-Benchmarks: `scripts/perf/preview-cache-baseline.json` / `preview-cache-budgets.json`
  + `compare.mjs` (6/6 OK, report/warn/gate), Budgets im selben Commit wie
  bewusstes Wachstum.
- LibRaw-Pin: `docker/Dockerfile` `LIBRAW_VERSION=0.22.2` + CI-Image
  `lumina-ci:latest` OCI-Label; `lumina_raw::libraw_version()` trägt in
  `decode_version`/`RenderKey` (F-102).

### F-071 Capability-Anzeige

- Quantitative Limits werden in der Capability-Matrix und im Browser sichtbar
  als „Budget/Limit" geführt (Bildgröße 45 MP/24 MP, RAM 8 GB/512 MiB/48 MiB,
  VRAM 1024 MiB/4, Threads Rayon/1, RAW LibRaw 0.22.2 vs. `UnsupportedPlatform`,
  `zdata` native-only, ONNX `onnx-wasm` optional) — kein stiller Fallback.

## Capability-Anzeige

Im Browser müssen alle nicht verfügbaren oder nicht aktivierten Fähigkeiten
sichtbar ausgewiesen werden (unverändert aus der Capability-Matrix): RAW-
Decode (bis `wasm-js`, aktuell `UnsupportedPlatform`), ONNX-Inferenz (bis
`onnx-wasm` + Modell, aktuell `RuntimeDisabled` auf WASM), Sidecar-
schreiben neben dem Original (OPFS), `zdata`-Artefakte (native-only,
„not available" auf WASM), GPU/`lumina-gpu`, sowie die quantitativen Limits
(Bildgröße 45 MP/24 MP, RAM 8 GB/512 MiB/48 MiB, VRAM, Threads, LibRaw 0.22.2).
Eine nicht aktivierte Fähigkeit wird nie als verfügbar dargestellt und nie
stillschweigend ersetzt.

## Abnahme

- **F-069** Browser-Import (File-Picker) lädt Rasterdateien; Drag-and-drop ist
  nach Async-Brücke nutzbar, vorher sichtbar deaktiviert (`warn!`).
- **F-069** OPFS-Sidecar-Roundtrip: importierte Quelle + Rezept überleben einen
  Reload und lassen sich als portables Lumina-Sidecar exportieren und nativ
  wieder verwenden (relative Pfade, `load_sidecar`-Vertrag); Verlust/Diverenz
  wird sichtbar gemeldet; `.lumina/`-Cache (OPFS) ist vollständig löschbar
  ohne Bearbeitungsverlust.
- **F-069** Browser-Export erzeugt byte-identische Artefakte zur nativen
  Pipeline (PNG/JPEG/WebP) über die gemeinsame `export_image`-Logik; Re-Export
  aus OPFS möglich, Original nie überschrieben.
- **F-070** ONNX im Browser: Capability-Anzeige korrekt bei Feature aus/an und
  bei fehlendem/stalem Modell (`RuntimeDisabled`/`MissingModel`/
  `ModelArtifactStale`); kein stiller Fallback auf Stub (e60a9ad).
- **F-071** Die quantitativen Limits (Bildgröße 45 MP/24 MP, Speicher 8 GB
  gesamt / 512 MiB CLI / 48 MiB WASM + LRU-Cap 1,5 GiB, VRAM-Pool 1024 MiB/4,
  Threads Rayon/1, RAW LibRaw 0.22.2, `zdata` native-only) sind je Plattform
  dokumentiert, gegen `preview_cache.rs`/`lumina-gpu`/`lumina-raw`/
  `capability-matrix.md` konsistent und durch WASM-Gates + F-074-Benchmarks
  (`compare.mjs` 6/6 OK) bestätigt.
- **Gates:** `cargo check --workspace` grün,
  `cargo check --target wasm32-unknown-unknown -p lumina-core` grün,
  `cargo check -p lumina-gui --target wasm32-unknown-unknown --no-default-features` grün,
  `cargo check --workspace --target wasm32-unknown-unknown` (auch mit `zdata`/`onnx-rt`) grün
  (kein Code-Change, nur Docs — kein Widerspruch zu Matrix/`cli-gui-wasm.md`).
