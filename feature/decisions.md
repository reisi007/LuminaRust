# Backend-Entscheidungen und Lizenzbedingungen

**Features:** F-007 RAW-Backend, ONNX-Backend, GUI-Framework, F-006, F-010

Dieses Dokument fasst die getroffenen Backend-Entscheidungen und ihre
Lizenzbedingungen zusammen. Architektonische Begründungen sind zusätzlich als
ADRs unter `docs/adr/` festgehalten (siehe `docs/adr/README.md`).

## RAW-Backend: native LibRaw

- **Entscheidung:** RAW-Decodierung erfolgt im MVP über einen gekapselten
  nativen LibRaw-Adapter (`crates/lumina-raw` über `libraw-sys`).
- **Lizenz (LibRaw):** LibRaw wird **dual-lizenziert** unter
  **GNU Lesser General Public License v2.1 (LGPL-2.1)** *und*
  **Common Development and Distribution License v1.0 (CDDL-1.0)**. Anwender
  können die für ihren Fall passende Lizenz wählen. (Quelle: libraw.org/about.)
- **Lizenz (Rust-Bindings):** Das verwendete `libraw-sys`-Crate (FFI-Bindings,
  im Workspace unter `vendor/libraw-sys` gepatcht) steht unter **MIT**. Die
  MIT-Lizenz des Bindings entbindet nicht von den Bedingungen der darunter
  linkten LibRaw-C++-Bibliothek.
- **Lizenzpflichten:** Bei Distribution von Lumina (nativ) ist die gewählte
  LibRaw-Lizenz (LGPL-2.1 oder CDDL-1.0) samt Hinweisen einzuhalten; bei LGPL
  sind die LibRaw-Quellen bzw. ein schriftliches Angebot bereitzustellen. Dies
  ist vor dem ersten Release (F-078) final zu prüfen.

## GUI-Framework: egui / eframe

- **Entscheidung:** Die Desktop-GUI verwendet **egui/eframe** (v1). Tauri ist
  keine v1-Abhängigkeit und kann in einer späteren Architekturentscheidung
  erneut bewertet werden.
- `lumina-gui` ist bewusst sowohl nativ (eframe) als auch als Trunk-WASM-App
  ausgelegt; native Abhängigkeiten (`rfd`) und WASM-Abhängigkeiten
  (`wasm-bindgen`, `web-sys`) sind über `cfg(target_arch = "wasm32")`
  getrennt.

## ONNX-Backend: native Inferenz (MVP)

- **Entscheidung:** KI-Masken (BiRefNet als erstes automatisches Subject-Modell,
  SAM 2 als erstes interaktives Box-/Pinsel-Modell) werden über einen
  **austauschbaren ONNX-Adapter** (`lumina-onnx`) angebunden.
- Der ONNX-Adapter muss native Inferenz, Modellverwaltung und Maskenartefakte
  kapseln und darf den plattformneutralen `lumina-core` nicht belasten.
- **Lizenz:** Die ONNX-Runtime und die Modelle (BiRefNet, SAM 2) sind vor
  Integration nach ihren jeweiligen Lizenzen zu prüfen und zu dokumentieren
  (F-078). Die Modellfähigkeit wird aus dem Modellmanifest gelesen, nicht aus
  dem Modellnamen erraten.

### MVP-Umfang (2026-08-19)

- **Änderung:** Die native ONNX-Inferenz ist per `Agents.todo.md` (Stand
  2026-08-19, Phase 6) **in den MVP-Umfang aufgenommen** (F-047). Die
  Entscheidung „post-MVP" aus der ursprünglichen Fassung wird hiermit für die
  **native** Inferenz (CLI/Desktop) überschrieben; der Adapter bleibt
  austauschbar (Trait `SubjectInference`, später echtes ORT- und SAM-2-Backend).
- **WASM:** Die Browser-/WASM-Seite bleibt **offen** (post-MVP,
  Feature `wasm-js`); `lumina-onnx` ist als native-only-Crate gekapselt
  (spiegelt `lumina-raw`) und wird im MVP nicht im Browser gebaut.
- **F-080:** Die Modellfähigkeiten `box_prompt`, `point_prompt`, `mask_prompt`,
  `class_detection` und `instance_segmentation` sind im ONNX-Manifest
  (`ModelCapabilities`, `lumina-onnx`) abgebildet; `subject_segmentation` ist
  die Basisfähigkeit (BiRefNet). Mindestens eine Fähigkeit muss gesetzt sein.
- **Real-Backend:** Die ONNX-Runtime (`ort`, v2.0.0-rc.13) ist in dieser Umgebung
  **tatsächlich abruf- und baubar** (inklusive Prebuilt-Binary-Download) und
  hinter dem nicht-default Feature `onnx-rt` eingebunden. Die numerische
  Validierung gegen ein echtes BiRefNet-`.onnx`-Artefakt erfolgt später
  (F-048/F-082), sobald Modellgewichte vorliegen; bis dahin ist der
  deterministische `StubBackend` die vollständige, getestete Oberfläche.

## Performance-Benchmarking

- **Entscheidung (ADR 0003):** Performance wird mit **Criterion** als
  einzigem nativen Timing-Harness im separaten Workspace-Crate
  `crates/lumina-bench` gemessen. Die native Messung ist Proxy für alle Archs
  (identische Core-Codepfade in `lumina-core`); Browser-WASM erhält später nur
  grobe, nicht-gerichtete Smoke-Timings.
- **Stores:** `perf/baseline.json` und `perf/budgets.json` sind committet und
  versioniert. Das Vergleichsskript `scripts/perf/compare.mjs` kennt die Modi
  `report` (immer, Exit 0), `warn` (Warnung bei budgetierter Überschreitung)
  und `gate` (nur `gate: true`-Benchmarks, Exit 1 bei Verletzung).
- **Gates:** Harte Gates gibt es erst nach Kalibrierung stabiler Benchmarks
  (F-074-N5); Report und Warnung laufen immer. Verrauschte CI-Runner sind
  kein alleiniger harter Gate.
- **Feature-Wachstum:** Budget-Anpassungen sind bewusste Entscheidungen im
  selben Commit wie das verursachende Feature und werden begründet.
- Normatives SOLL-Dokument: `feature/quality/performance-benchmarks.md`
  (F-074); Speicherbudgets folgen getrennt in F-075.

## Arbeitsweise

- Lizenzbedingungen von RAW-Backends, ONNX-Runtime und Modellen werden vor
  Integration geprüft und dokumentiert (Agents.md, Änderungsregeln).
- Abweichungen von diesen Entscheidungen werden als neue ADR dokumentiert.
