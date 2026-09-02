# Gap-Analyse Generatives Füllen: Transparent nach Lens, Expand, Staub, visuelle Auto-Verifikation

**Datum:** 2026-09-02  
**Status:** Analyse (kein Code, Doku-first)  
**Auftrag:** 3 Wünsche vor manuellem Test — Gap zwischen SOLL/Ist + priorisierte Lücken  
**Bezug:** `Agents.md`, `Agents.todo.md` (Phase 10b GEN-EXPAND-1 46f6baf), `feature/README.md`, `feature/product/generative-expand.md`, `feature/architecture/pipeline.md`, `feature/architecture/sidecar.md`, `feature/product/ai-masks.md`, `feature/product/export.md`, `feature/platform/capability-matrix.md`, `feature/platform/wasm-limits.md`, `feature/quality/fixtures-licensing.md`, `docs/plans/gui-tests-2026-09-02.md` (T01-T10), `crates/lumina-core/src/**`, `crates/lumina-gui/src/lib.rs`, `crates/lumina-mcp/src/tools/dust_removal.rs`, `crates/lumina-raw`, `crates/lumina-onnx` (inpaint)

---

## 1 Was SOLL schon gehen — und was ist bereits getestet

### 1.1 SOLL (normativ, verifiziert)

| Feature | SOLL-Dokument | Kurzbeschreibung |
|---|---|---|
| F-098 Objektivkorrektur | `feature/architecture/pipeline.md` §F-098 | Manuelles Modell `distortion_k1..k3`, `vignette_c0..c2`, `ca_red/blue` + optionales Lensfun-Profil (Kamera/Objektiv, dynamisch LGPL-3.0, DB CC-BY-SA). Geometrie-Reihenfolge `Lens → Perspective → Crop → Rotation → Mirror` in `apply_geometry`. Native-only Capability. |
| F-099 Perspektive | `pipeline.md` §F-099 | `vertical/horizontal/rotation/scale/aspect/shift_x/y`, Homographie, nach Lens, vor Crop. |
| F-093 Crop/Geometry | `pipeline.md` §F-093 | `recipe.geometry` mit `Crop::Aspect` (Presets) / `Free {x,y,width,height 0..=1}`, `rotation_degrees -180..=180`, Mirror. Normierte Koordinaten im post-Geometrie-Frame. Finaler Messbereich für F-041 nach Crop. |
| F-042 Dust/ SourceActions | `pipeline.md` §Source-Actions | `SourceActionArtifact { region: MaskPlane u16, replacement: ImageFrame RGBA8 }` mit Schwellwert `>=32768` (50 %), identische Dimensionen Quelle==Region==Replacement, vor Auto-Analyse. Persistenz via `EditRecipe.source_actions` + `.lumina.zdata` Record `kind=RepairRegion` (atomar, `.zdata.lock`, BLAKE3). |
| GEN-EXPAND-1 Doku-first | `feature/product/generative-expand.md` (46f6baf) | **Nur Doku, kein Code** (unabhängig verifiziert BESTANDEN). Definiert `GenerativeEdit` Rezeptstufe v1: `model {name,version,sha256}`, `prompt/negative_prompt`, `seed u64`, `inference_resolution`, `canvas {output_width/height, source_offset_x/y}` (>100 % Regel), `region` / `mask_reference`, `artifact ArtifactReference`, `status valid/stale/missing/corrupt`, Pipeline `Decode→SourceActions→GenerativeEdit→Lens→Perspective→Crop→Output`, post-GenerativeEdit-Canvas als Referenz für Crop/Masken, `.lumina.zdata kind=generative_canvas` atomar, Identität analog AI-Masken, kein stiller Fallback, Capability lokal vs Cloud getrennt, Lizenz F-078. |
| Sidecar/Arbeitsfarbraum | `feature/architecture/sidecar.md`, `feature/architecture/pipeline.md` | Sidecar `.lumina.json` + `.lumina.zdata` (BLAKE3, zstd, relative Pfade, atomar). Arbeitsfarbraum MVP `Rgba8Srgb` (LinearProPhotoRgb reserviert, nicht aktiv). `RenderKey`/`recipe_hash`/`stage_digest` mit `source_content_hash`, `decode_version`, `pipeline_version`, `output_dimensions`. |
| Platform/WASM | `feature/platform/capability-matrix.md`, `feature/platform/wasm-limits.md` | `zdata`/`zstd` native-only target-gegatet, `lumina-onnx` native-only (`ort` target-gegatet, wasm_stub `RuntimeDisabled`), `lumina-raw` wasm `UnsupportedPlatform`, Limits 45 MP/24 MP, 8 GB/512 MiB/48 MiB, VRAM 1024/4, Rayon/1 Thread, LibRaw 0.22.2. |
| Export/Bildexport | `feature/product/export.md` | `export_image` gemeinsame Logik GUI/CLI, PNG/JPEG/WebP byte-identisch, GUI WASM `cargo check` grün. |

### 1.2 Ist — verifizierte Tests (Stand 2026-09-02)

| Crate/Suite | Kommando | Ist | Hinweis |
|---|---|---|---|
| `lumina-gui` | `cargo test -p lumina-gui` | **147 passed** verifiziert (43b1b73, F-103-INTEGRATION-PREVIEW-SIDECAR) — vorher 133 in `gui-tests` Plan | Unit 95+7+5+8+7+14, Integration kittest 5 Snapshots + 3 Interaktion `#[ignore]` (headless wgpu), Preview-Cache 6 Benches |
| `lumina-core` | `cargo test -p lumina-core` | **277 + 7** passed | Pipeline, SourceActions (Schwellwert 32768/32767), Masks, MaskLoader, tone/histogram, RenderKey stage_digest |
| `lumina-sidecar` | `cargo test -p lumina-sidecar` | **86 passed** | JSON-Roundtrip v1→v2, atomic write, `.zdata` container, `artifact_status` eager BLAKE3, Lock/CAS |
| `lumina-onnx` | `cargo test -p lumina-onnx --features onnx-rt` | **107 passed** (49f4f76) | `sam2_1_manifests` 4 Varianten 1024², `StubSam2Backend`, Hash-Pin Fixture `2a2ede66…` 139 B `lumina-crafted-reducemax.onnx`, `resolve::try_load_onnx_engine` |
| `lumina-raw` | `cargo test -p lumina-raw` | Fixtures `aircraft-landscape/portrait.cr3` via `LUMINA_RAW_FIXTURE` |  |
| kittest/Preview-Cache | `cargo test -- --ignored` / `cargo bench` | 5 Goldens (library/develop/export), `scripts/perf/compare.mjs` 6/6 OK | `gui-tests-2026-09-02.md` T01-T10 noch offen |

**Preview-Cache** (`feature/quality/preview-cache.md`): 7 Slots, LRU 1.5 GiB, Hybrid GPU-Textur + WebP-Disk, asymmetrisch +4/-2, 2026-09-02 A1-A6/B7 verifiziert — Outcome-Gaps (Stale-Badge, korrupt `.tmp` nie Hit, OneToOne-Invalidierung, Prio) in `gui-tests` Plan T01-T10 als Doku-first offen.

---

## 2 Was für die 3 Wünsche fehlt — SOLL vs Ist vs Tests

### 2.1 Wunsch 1: Automatisch transparente Teile nach Lens Correction generativ füllen

**Soll laut GEN-EXPAND-1 + F-098:**
- Lens/Perspective/Crop erzeugen transparente Randbereiche (nach `apply_geometry` → bilineares Resampling, schwarze Randfüllung im MVP als Platzhalter `transparent→schwarz`, `pipeline.md` §F-099).
- Wunsch: **Auto-Fill** dieser transparenten Pixel generativ (inpaint/outpaint) unmittelbar nach Lens/Perspective, vor Crop.

**Ist:**
- Pipeline heute: `Decode → SourceActions → AutoAnalysis → Adjustments → Masks → Crop(→apply_geometry inkl. Lens/Perspective) → Output` (`crates/lumina-core/src/pipeline.rs:29-68`, `lib.rs:443 apply_geometry`).
- `GenerativeEdit` existiert **nicht** als Stufe; kein Code (`grep generative` 0 Treffer außer Doku). `ImageFrame` hat `u8` RGBA, aber kein `alpha`-transparent Detection nach Lens (Alpha bleibt, schwarze Füllung dokumentiert).
- Kein Inpaint/Outpaint-Modell im Workspace (`lumina-onnx` nur `subject_segmentation` BiRefNet + `box/point/mask_prompt` SAM 2.1, kein `inpaint/outpaint` Capability, `capability-matrix.md` keine Zeile dafür — GEN-EXPAND-1 schlägt `inpaint`/`outpaint` als neue Capability vor, nicht umgesetzt).
- Kein transparent-Pixel Detektor, keine auto-trigger Logik, kein Masken-Pipeline für Rand.

**Gap konkret:**
- Pipeline-Platzierung: Wunsch "nach Lens" widerspricht GEN-EXPAND-1 Reihenfolge `GenerativeEdit → Lens → Perspective → Crop`. Nach Lens füllen = Canvas wird **nach** Lens vergrößert — Canvas-Geometrie gehört aber laut GEN-EXPAND-1 **vor** Lens (Lens arbeitet auf Canvas). Entscheidung nötig: Auto-Fill als **post-Geometry Inpaint** (Lens-Ränder füllen im gleichen Canvas) vs **Canvas-Expand Outpaint** (GEN-EXPAND-1 Outpaint). Beides braucht eigene Rezept-Felder.
- `ImageFrame`/`RenderContext` kennt keinen transparent-Rand-Input für Inpaint.
- `lumina-onnx` braucht neues Modell (z. B. LaMa, SD-Inpaint) + `ModelManifest` Capability `inpaint`/`outpaint`, `input_spec_digest`.

**Tests fehlen:** Kein transparent-Detection-Test, kein Auto-Fill Golden, kein Cache-Invalidierungstest für post-Lens-Fill.

### 2.2 Wunsch 2: Manuell größer ziehen (Checkbox default "auf Bild beschneiden", Crop-Entscheidung behalten vs zuschneiden)

**Soll laut GEN-EXPAND-1 + F-093:**
- Canvas `>100 %` Expand (`output_width/height > source`, `source_offset_x/y` deterministisch, negative Offsets erlaubt, Bounds geprüft).
- Normierte Crop-Koordinaten referenzieren **post-GenerativeEdit-Canvas** (`generative-expand.md` §Koordinatenreferenzrahmen).
- Wunsch 2 detailliert: Checkbox default **"auf Bild beschneiden"** (Crop auf sichtbaren Bereich clippen), optional **"generative Inhalte behalten"** vs **"auf aktuelle Ansicht zuschneiden"**.

**Ist:**
- `EditRecipe.geometry` + `lens_correction` + `perspective` vorhanden, aber **kein** `GenerativeEdit.canvas` Feld. `Geometry.crop` `Free {x,y,width,height 0..=1}` bezieht sich auf die **Quellgeometrie**, nicht auf ein erweitertes Canvas. Kein `canvas` in `EditRecipe` (`lumina-sidecar/src/lib.rs:457-486` nur geometry/lens/perspective/effects/source_actions).
- Keine Checkbox in `lumina-gui/src/lib.rs` (Crop UI: `Aspect`/`Free` Presets, Rotation, Mirror — kein `keep_generative` Flag, kein "auf Bild beschneiden" Default).
- Kein `keep_generative` / `crop_mode` Flag im Rezept, kein RenderKey-Eintrag dafür.
- `RenderKey` kennt `output.width/height`, `geometry` via `recipe_hash`, aber keine `canvas` Identität.

**Gap konkret:**
- Rezept-Felder fehlen: `GenerativeEdit.canvas` (`output_width/height`, `source_offset_x/y`) oder alternativ `Geometry.canvas` + `crop.keep_generative: bool` (Entscheidung: wo lebt Checkbox — in `GenerativeEdit` oder in `Geometry.crop`? GEN-EXPAND-1 sieht Canvas als Teil von `GenerativeEdit`, Wunsch 2 braucht zusätzlich Crop-Flag).
- Koordinatensystem: `crop_rect` (`lib.rs:508`) rechnet normiert auf `self.width/height` (Quell-Frame nach Lens). Bei Canvas>Quelle muss `crop_rect` auf Canvas-Dimensionen rechnen — Breaking Change.
- GUI: Checkbox-Steuerelement + Persistenz + History + Preview-Cache Invalidierung.
- Export: `export_image` (`lib.rs:102`) rendert `render_frame` direkt — kein Canvas-Expand Pfad, kein `clip_to_image` vs `keep`.

**Tests fehlen:** Kein Canvas-Bounds-Test (negativer Offset), kein Crop-auf-Canvas-Roundtrip, kein `keep_generative` Golden, kein Sidecar-Migrationstest.

### 2.3 Wunsch 3: Staub entfernen schnell (heuristisch/Clone) wie Lightroom, aber auch lokale generative KI

**Soll laut F-042 + GEN-EXPAND-1 Abgrenzung:**
- F-042: Source-Actions als kontext-übergebene Artefakte (u16 Region + RGBA8 Replacement) — heute persistiert als `SourceActionSpec {kind, artifact {id, relative_path, checksum}}` ( `lumina-sidecar/src/lib.rs:234-258`, `OPTION kind DustRemoval | AiReplacement`).
-GEN-EXPAND-1 Abgrenzung: `SourceActions` = kontext-Regionen **ohne** Canvas >100 % und ohne Modell/Prompt; `GenerativeEdit` = **mit** Modell/Prompt/Seed.
- Wunsch: **schnell heuristisch** (Clone/Spot-Heal, keine KI) als Lightroom-ähnlich + **lokal generativ** (ONNX Inpaint, Maske malen/Prompt).

**Ist — schnell/heuri:**
- `SourceActionArtifact` exige **full-size** `region` + `replacement` identischer Frame-Dimension (`render.rs:435 apply_source_actions` prüft `region.width==frame.width` hart). Kein Clone-Stempel, kein billiger Single-Tap Heal.
- MCP `lumina_dust_removal` (`crates/lumina-mcp/src/tools/dust_removal.rs:1`) validiert region==source, replacement==region — kein heuristischer Fill, erfordert externes replacement Bild.
- CLI `dust-removal` existiert (pipeline.md F-042-N1), aber GUI hat **kein** Staub-UI Panel (`grep Dust` in `lumina-gui/src/lib.rs` 0 Treffer). Kein Pinsel/Spot-Tool für Dust im GUI.
- Tests: `source_action_*` in `render.rs:614-738` nur threshold und dimensions-mismatch.

**Ist — generativ lokal:**
- `SourceActionKind::AiReplacement` existiert im Schema, aber kein ONNX Inpaint-Modell dahinter. `lumina-onnx` Capabilities: `subject_segmentation`, `box_prompt`, `point_prompt`, `mask_prompt` — kein `inpaint` (`crates/lumina-onnx/src/manifest.rs`).
- Kein `MaskPrompt`→Inpaint Mapping, kein Prompt/Seed im `SourceActionSpec` (GEN-EXPAND-1 hätte `prompt/seed/inference_resolution` in `GenerativeEdit`, nicht in `SourceAction`).
- Kein Modell-Lizenz/Hash-Pin für Inpaint (F-078 `fixtures-licensing.md` nur BiRefNet MIT, SAM 2.1 Apache-2.0).

**Gap konkret:**
- Schnell-Pfad braucht neuen Core-Algorithmus (Clone/Healing Brush, radius-basiert, nicht full-frame replacement) — entweder erweitertes `SourceActionKind::HeuristicClone` oder eigener `HealingAction` Typ. Entscheidung: F-042 erweitern oder neues Rezept-Feld.
- Generativ-Pfad braucht `lumina-onnx` Inpaint Backend + GUI Maske-malen (Brush/Box → Inpaint-Maske) + `render_frame` Inpaint-Stufe.
- GUI Staub-Panel fehlt komplett (Tool-Auswahl, Radius, Feather, Quelle setzen, generativ Toggle).

**Tests fehlen:** Kein Clone-Heal Unit-Test, kein generativ Inpaint Stub-Test, kein Performance-Vergleich schnell vs generativ.

### 2.4 Querschnitt: Pipeline-Reihenfolge + Sidecar-Artefakt

| Aspekt | GEN-EXPAND-1 SOLL | Ist Pipeline (`pipeline.rs` / `render.rs:152`) | Gap |
|---|---|---|---|
| Stufen | `Decode→SourceActions→GenerativeEdit→Lens→Perspective→Crop` | `Decode→SourceActions→AutoAnalysis→Adjustments→Masks→Crop(apply_geometry)` | `GenerativeEdit` fehlt; Lens/Perspective sind **innerhalb** von `Crop` versteckt (`apply_geometry` 5-in-1), nicht eigene Stage |
| Canvas | `canvas {output_width,height, source_offset}` Teil der Identität | Kein Canvas, kein offset — `OutputSpec width/height` nur Output, nicht Canvas | Neues Top-Level-Feld oder `GenerativeEdit` Struct |
| Artefakt | `.lumina.zdata kind=generative_canvas` RGBA8 kompositiertes Canvas, BLAKE3 | `.lumina.zdata` nur `MaskTile` + `RepairRegion` (`RecordKind 0/1`) | Neuer `RecordKind=2` + `kind=generative_canvas` Container, Version bump |
| RenderKey | `canvas` + `prompt/seed/model` in `recipe_hash`, `artifact.checksum` in downstream digests | Nur `source_actions` checksums + `geometry` im hash | `GenerativeEdit` muss in `RenderKey::new` und `stage_digest` (mask vs render vs histogram) |

---

## 3 Visuelle Ergebnis-Analyse automatisch in UI (keine manuelle Prüfung)

**SOLL (F-074 `performance-benchmarks.md`, `preview-cache.md`, `conflicts-and-acceptance.md`):** Golden-Image Toleranz, Histogram-Delta Ziel, kittest Snapshots, Bench `compare.mjs` report/warn/gate, Preview-Cache Budgets.

**Ist:** `lumina-gui` hat 5 kittest Goldens (Library/Develop/Export), 6 Benches `preview_cache/*`, `LuminanceHistogram` (256 Bins, BLAKE3 Digest), `analyze_tone` vorhanden. Keine Generative-Goldens, kein Lens-transparent Diff, kein Dust-Heal Golden, kein Histogram-Blende-Vergleich im GUI.

**Was für "alles getestet, so viel wie möglich automatisch" nötig ist:**
- **Golden-Image** (toleranzbehaftet, nicht byte-identisch wegen Inpaint Nichtdeterminismus — Seed pinnen oder `seed` in Golden aufnehmen; Toleranz per PSNR/SSIM dokumentieren, z. B. PSNR > 35 dB wie Fixtures-Policy).
- **Histogram-Delta** (vor/nach Dust/Inpaint/Canvas-Expand: `LuminanceHistogram::digest` Vergleich, `analyze_tone` Median/p01/p99 Delta schwellwert).
- **Blende-Vergleich** (Before/After Toggle `Y` existiert (`lib.rs:916 before_after`), aber kein A/B Split-View für generative Artefakte).
- **kittest Snapshot** für neuen Checkbox-Zustand (Crop-Panel mit "auf Bild beschneiden" default, "generative behalten" vs "zuschneiden") — 2 neue Snapshots.
- **Bench** für Inpaint Latenz (Ziel < 200 ms für 8 MP Clone, < 2 s für 1024² ONNX Inpaint, F-071 Budget-Erweiterung).
- **Keine manuelle Prüfung** = jede Pipeline-Stufe braucht deterministischen Seed + hash-gepinnte Fixture (F-073), sonst `#[ignore]`.

---

## 4 Priorisierte Lücken-Liste (PRIO hoch/mittel/niedrig)

**Legende:** Testbarkeit `A`=voll automatisch (unit/property), `I`=Integration (headless + Fixture), `V`=visuell Golden/kittest; Aufwand `S<1d, M1-3d, L>1w`; Risiko `Lizenz/Modell/Performance`.

| # | PRIO | Lücke | SOLL-Anker | Ist | Crate | Testbarkeit | Aufwand | Risiko |
|---|---|---|---|---|---|---|---|---|
| G1 | **hoch** | **GenerativeEdit Rezept + Sidecar** (`GenerativeEdit` v1 Felder, `canvas` >100 %, `prompt/seed/model`, `artifact` ref) fehlt — Doku 46f6baf, kein Code | `generative-expand.md` §Rezeptmodell, `sidecar.md` zdata, `pipeline.md` Reihenfolge | `EditRecipe` hat kein canvas/generative Feld, zdata nur 2 Kinds | `lumina-sidecar` | A (Roundtrip, Migration v1→v2, `serde(flatten)` extras) | M | Lizenz F-078 (Modell-Hash/Pin vor Commit), Schema-Bump Entscheidung |
| G2 | **hoch** | **Pipeline Stufe `GenerativeEdit`** fehlt — Decode→GenerativeEdit→Lens→Perspective→Crop nicht existent; `apply_geometry` 5-in-1 muss entkoppelt werden | `generative-expand.md` §Pipeline-Platzierung, `pipeline.rs` Default | Stufe nicht in `Pipeline::default()`, kein `render_frame` Pfad | `lumina-core` | A (stage_digest Trennung, `Pipeline::validate`, `render_frame_from_base` byte-identisch) | M | Performance (zusätzlicher Full-Canvas Pass, 45 MP×RGBA8 ~180 MiB) |
| G3 | **hoch** | **Transparent-Detection nach Lens/Perspective** (Alpha/Randfüllung) + Auto-Fill Trigger fehlt | F-098/F-099 `transparent→schwarz`, Wunsch 1 | Kein Detektor, kein Job, schwarze Ränder sichtbar | `lumina-core` + `lumina-gui` | I (synthetische Lens-Distortion 15 % auf Checker-Fixture → transparente Ecken detektieren) | S | Modellwahl (welches Inpaint füllt Ränder ohne Artefakt) |
| G4 | **hoch** | **zdata `kind=generative_canvas` Artefakt** (kompositiertes RGBA8 Canvas, BLAKE3, atomar) fehlt | `generative-expand.md` §Binärdaten | Nur `MaskTile`/`RepairRegion`, kein Canvas Record | `lumina-sidecar` + `lumina-core` | A (checksum mismatch→Corrupt, Bundle-Shift relativer Pfad, atomic `.zdata.lock`) | S | Native-only zstd, WASM `not available` Parity |
| G5 | **hoch** | **Staub schnell (heuristisch Clone/Heal)** fehlt — GUI Tool + Core Algorithmus (Radius, Feather) | `pipeline.md` §Source-Actions (Schwellwert 50 % full-frame), Wunsch 3 Lightroom | Full-frame replacement Pflicht, kein Clone-Brush, kein GUI Panel | `lumina-core` (neuer `HealingPass`) + `lumina-gui` | A (heal 3×3 Clone deterministisch, `source_action` Heal vs replacement) + V (Golden Spot-Heal) | M | Performance (Heal muss <200 ms auf 24 MP, sonst Preview-Cache Budget gerissen) |
| G6 | **hoch** | **Generative Staub lokal (ONNX Inpaint)** fehlt — `lumina-onnx` Capability `inpaint`, ModelManifest `inpaint/outpaint`, `lumina-mcp`/`lumina-gui` Verdrahtung | GEN-EXPAND-1 §Modell/Capability, Wunsch 3 generativ | `SourceActionKind::AiReplacement` Schema existiert, aber kein Backend, keine Maske-malen UI | `lumina-onnx` + `lumina-core` + `lumina-gui` | I (StubInpaint Backend deterministisch, hash-gepinnte Fixture `pending-integration` bis Modell committet) | L | **Lizenz** (Inpaint Modelle oft CC BY-NC, SD-Inpaint Apache-2.0 nur mit Pflicht-Check), Modellgewichte ~1-4 GB, `pending-integration` bis LIZ-Entscheidung |
| G7 | **hoch** | **Crop Checkbox default "auf Bild beschneiden" + Entscheidung "generative behalten vs zuschneiden"** (`keep_generative` Flag) fehlt — normierte Crop-Koordinaten auf Canvas vs Quelle unentschieden | GEN-EXPAND-1 §Interaktion Crop/Geometry, F-093, Wunsch 2 | Kein Flag, Crop immer auf Quell-Dimensionen, keine Canvas-Koordinaten | `lumina-sidecar` (`Geometry.crop` / `GenerativeEdit.canvas`) + `lumina-gui` (Checkbox Panel) + `lumina-core` (`crop_rect` Umstellung) | A (Roundtrip flag, `keep=false` clippt auf source_offset, `keep=true` zeigt Canvas) + V (kittest 2 Snapshots) | M | UX (Default darf bestehenden Export nicht breaken — Pre-MVP ok, aber `feature/README.md` Invariante "reproduzierbar") |
| G8 | **mittel** | **Manueller Canvas Expand per Drag (Expand-Rahmen ziehen)** fehlt — GUI Interaktion, Zielformat-Aspekt als Orientierung | GEN-EXPAND-1 §UI-Flow 3, Wunsch 2 "größer ziehen" | Kein Drag-Handle, kein Canvas-Resize UI | `lumina-gui` (preview drag) + `lumina-sidecar` | I (headless drag → `canvas.source_offset` deterministisch) | M | Performance (Vorschau mit transparent/skizziertem Rand während Drag) |
| G9 | **mittel** | **Dust GUI Panel** (Brush/Box/Pinsel für Staub, Radius/Feather, Toggle schnell vs generativ, Maske malen/Prompt) fehlt komplett | F-042, `gui-tests` nicht vorhanden | `lumina-gui/src/lib.rs` MaskTool nur Brush/Gradient/Radial für Masken, kein Dust Tool | `lumina-gui` | I (headless `rasterize_prompt` → Dust-Maske deterministisch, F-079 vorhanden) | M | Pre-MVP Schema-Entscheidung: Dust-Pinsel = `MaskPrompt::Brush` reuse oder neues `DustMark` |
| G10 | **mittel** | **Visuelle Auto-Analyse in UI** (Golden Toleranz, Histogram-Delta, Blende Before/After, kittest diff, Bench Budget) unvollständig für generative Pfade | Wunsch 4, `gui-tests-2026-09-02.md` §3/5, F-074 | Unit/kittest vorhanden, aber keine generative Goldens/Hist-Delta/PSNR Gate | `lumina-core` (Histogram) + `lumina-gui` (kittest) + `scripts/perf` | V (PSNR>35 dB Gate, histogram p01/median/p99 1/256 Toleranz dokumentiert) + Bench (criterion Inpaint 1024²) | M | Toleranz-Entscheidung (Inpaint nicht byte-identisch ohne Seed-Pin — Seed muss Pflicht werden, sonst Gate flakey) |
| G11 | **mittel** | **RenderKey/Cache Invalidierung für generative Artefakte** fehlt (canvas/prompt/seed/model in digest, Canvas-Wechsel invalidiert Geometrie sichtbar) | `generative-expand.md` §Identität/Veraltung, `pipeline.md` Reproduzierbarkeit | `RenderKey` kennt nur `source_actions` checksums + geometry, kein generative canvas | `lumina-core` (`pipeline.rs:98 RenderKey`) | A (stale/missing/corrupt bei jeder Identitätsabweichung, `recipe_hash` Änderung invalidiert ab GenerativeEdit, nicht Decode) | S | — |
| G12 | **mittel** | **CLI/MCP Vervollständigung** (`dust-removal` → generativ Pfad, `generative-expand` CLI Command) fehlt | `mcp/src/tools/dust_removal.rs` nur Full-frame replacement, kein Heal/Inpaint | MCP/CLI nur Dummy/Passthrough | `lumina-mcp` + `lumina-cli` | I (CLI E2E `render_out` darf nicht auf Quelle schreiben, `reject_protected_target`) | S | — |
| G13 | **niedrig** | **Capability-Matrix Erweiterung** (inpaint/outpaint lokal vs Cloud getrennt, WASM `onnx-wasm` off) nicht dokumentiert | GEN-EXPAND-1 §Modell/Capability | Matrix hat keine inpaint Zeile | `feature/platform/capability-matrix.md` | A (wasm32 `cargo check --features inpaint` grün, `RuntimeDisabled` sichtbar) | S | Cloud-Falle (kein stiller Fallback, explizite Entscheidung nötig) |
| G14 | **niedrig** | **Lizenz/Fixtures für Inpaint/Outpaint Modelle** vor Integration dokumentieren (F-078) | `fixtures-licensing.md` §5/8, THIRD-PARTY-NOTICES | Nur BiRefNet MIT + SAM 2.1 Apache-2.0 geprüft, kein Inpaint Modell gepinnt | `docs` + `THIRD-PARTY-NOTICES.md` | — (Audit, kein Test) | S | **Hoch**: viele SOTA Inpaint Modelle non-commercial, AGPL-Falle via `ultralytics` analog dokumentieren |
| G15 | **niedrig** | **Quantitative Limits erweitern** (Inpaint 24 MP→Preview-only, 45 MP Desktop Voll-Canvas, RAM+VRAM Budget für generative Canvas) | `wasm-limits.md` F-071 | Limits nur für Preview-Cache, kein generative Canvas Budget | `feature/platform/wasm-limits.md` + `preview_cache.rs` | Bench | M | 45 MP Canvas Expand → 180 MiB RGBA8 + Inpaint Tensor 1024² transient → Budget Riss |

---

## 5 Abhängigkeiten & Reihenfolge (Doku-first beachtet)

```
G1 (Sidecar Rezept) ─┐
G4 (zdata Canvas)    ─┼─► G2 (Pipeline Stufe) ─► G3 (Auto-Fill transparent) ─┐
G11 (RenderKey)      ─┘                         G7 (Crop keep_generative) ─┼─► G8 (Expand Drag)
                                                                             └─► G10 (visuelle Gates)
G5 (Heal schnell) ────┐
                      ├─► G9 (Dust GUI Panel) ─► G12 (CLI/MCP)
G6 (Inpaint lokal) ───┘         (G6 blockiert auf G13/G14 Lizenz)
```

- **Welle 1 seriell:** G1 + G4 + G11 (ein schreibender Agent auf `lumina-sidecar`, ein auf `lumina-core` Pipeline — keine Parallelschreiber auf gleichem Crate).
- **Welle 2:** G2 (pipeline) nach G1, G5 (Heal) parallel zu G6-Vorbereitung (Lizenz G14).
- **Welle 3:** G3/G7/G8 GUI (ein Agent `lumina-gui`), G9 Dust Panel.

---

## 6 Offene Risiken — vor manuellem Test zu entscheiden

1. **Pipeline-Platzierung "nach Lens" vs GEN-EXPAND-1 "vor Lens":** Wunsch 1 sagt nach Lens füllen, GEN-EXPAND-1 sagt Lens arbeitet auf Canvas. Ohne Entscheidung entsteht stiller Koordinaten-Fallback. **Empfehlung:** Auto-Fill als post-Geometry Inpaint (im gleichen Canvas, keine Canvas-Vergrößerung) von Outpaint (Canvas >100 %) trennen — ersteres ist Wunsch 1, letzteres Wunsch 2.
2. **Canvas Koordinatensystem Breaking Change:** `crop_rect` heute Quell-bezogen. Umstellung auf Canvas-bezogen invalidiert bestehende Geometrie-Hashes. Pre-MVP erlaubt Break ohne Migration (`Agents.todo.md` Gepinnte Entscheidung), aber `schema_version` bleibt 1 — Dokumentation muss Bump-Entscheidung treffen.
3. **Modell-Lizenz Inpaint:** SOTA LaMa/MAT/CC BY-NC nicht shippable; SD-Inpaint Apache-2.0 nur nach Verifikation. F-078 Gate blockiert bis Hash/Pin dokumentiert — kein Code vor LIZ-Entscheidung (analog BiRefNet/SAM 2.1 `pending-integration`).
4. **Performance:** 24 MP Canvas Expand (96 MiB) + Inpaint 1024² transient + Preview-Cache 7 Slots 1.5 GiB sprengt 8 GB GUI Budget nur knapp nicht — F-074 `compare.mjs` gate:true erst nach Kalibrierung (`gui-tests` G15).
5. **Visuelle Gate Flakiness:** Inpaint ohne Seed-Pin ist nicht deterministisch — Golden-Toleranz braucht PSNR Gate, nicht byte-identisch. Seed muss Pflichtfeld werden (GEN-EXPAND-1 bereits `seed u64` Pflicht).
6. **WASM Parity:** Generative Canvas auf WASM bleibt `not available` (zdata native-only) — Capability-Anzeige muss erweitertes `inpaint` als nicht verfügbar ausweisen, sonst stiller Fallback.

---

**Pfad:** `docs/plans/gap-generative-fill-transparent-2026-09-02.md`  
**Summary:** SOLL erfüllt für Lens/Perspective/Crop/SourceActions (inkl. Lensfun, 277+7/86/107/147 Tests, WASM grün), GEN-EXPAND-1 nur Doku-first. Für 3 Wünsche fehlen 7 hoch-prio Lücken: Rezept+Pipeline Canvas (`lumina-sidecar`/`lumina-core`), transparent-Detection/Auto-Fill, zdata generative_canvas, schneller Heal-Algorithmus + GUI, lokale generative Inpaint Backend (Modell/Lizenz), Crop Checkbox keep_generative + visuelle Auto-Gates. Priorisierte Tabelle 15 Lücken mit Crate, Testbarkeit, Aufwand, Lizenz/Performance-Risiko; Welle 1 Sidecar/Pipeline seriell, dann GUI. Kein Todo-Edit, kein Code.  
**Offene Risiken:** Entscheidung post-Lens vs pre-Lens Canvas, Crop-Koordinaten Break, Inpaint Lizenz/Pin vor Integration, 45 MP Canvas RAM-Budget, Seed-Pflicht für deterministische Goldens, WASM not-available Capability.
