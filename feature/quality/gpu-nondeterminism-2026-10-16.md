# GPU-Render-Nichtdeterminismus: Grenze und Beweisplan (2026-10-16)

**Feature:** F-074 GPU-Bench / `GPU-RENDER-NONDETERMINISM-69`

**Anker-Stand:** die Zeilenangaben dieses Berichts sind **Teil des
Befunddatums** vom 2026-10-16 und beziehen sich auf den Code-Stand `ece0ae8`;
sie sind ueber `git show ece0ae8:<pfad> | sed -n '<zeile>p'` rekonstruierbar.

## Ergänzender Adapterbefund (2026-10-25, vor Ausführungsreport)

Dieser Zusatz aktualisiert den älteren, read-only Befund mit der späteren
Adapter-Reproduktion. Er ersetzt keine Ursache durch eine Vermutung.

### Gemessen

- Der unveränderte CLI-Test
  `tests::auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move`
  lief mit
  `PATH="$HOME/.cargo/bin:$PATH" LUMINA_GPU_BACKENDS=metal cargo test -p lumina-cli --features gpu --bin lumina-cli tests::auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move -- --exact --test-threads=1 --nocapture`
  seriell dreimal: **pass / fail / pass** (Exitcodes **0 / 101 / 0**).
  Rohlogs und Umgebung: `target/gpu-render-nondeterminism-69/2026-10-25T173334Z/`.
- Im roten Lauf erreichten alle 40 synthetischen Fixtures den Test. Die
  Persisted-Text-Assertion war zuvor erfolgreich; die abschließende
  PNG-Byte-Assertion meldete `39/40 members` statt `40/40`. Sie identifiziert
  weder das abweichende Mitglied noch, ob Pixel oder PNG-Metadaten abweichen.
- Der rote Lauf enthält **keinen** harten Route-, Backend- oder Device-Loss-
  Beleg. Die separate Adapterprobe belegt einen verfügbaren Metal-Adapter, aber
  nicht die Route des CLI-Tests.
- Eine getrennte, ignorierte Scratchprobe lief **grün** und meldete dort
  120/120 GPU-Tone-Pass/Queue-Submit/Readback-Abschlüsse sowie 0 CPU-Fallbacks.
  Sie lief nicht im roten Originalprozess; sie beweist dessen Route nicht.
- Der Host der Messung war macOS 27.0.1, Apple M5 Pro / Metal 4, wgpu 30.0.1.

  Der ursprüngliche Befundbericht führte
  `target/gpu-render-nondeterminism-69/2026-10-25T173334Z/` als Rohlog-Pfad
  auf. Der Verifier dieser Fixrunde fand die dort genannten CLI-Rohlogs an
  diesem Pfad nicht. Daher wird hier weder ihre aktuelle Verfügbarkeit noch
  ihre Provenienz behauptet; die Pass/Fail-Folge bleibt eine überlieferte
  Messangabe und keine erneut anhand der CLI-Rohlogs verifizierte Messung.

### Schlussfolgerung und Grenzen

Die Ausgabe ist reproduziert, aber ihr Ausführungspfad im roten Prozess und ihre
Ursache sind **unbekannt**. Insbesondere beweist der Scratchlauf keinen
GPU-Pfad für den roten Lauf. Die ältere Zeile „Verbleibende Kandidaten liegen
in der Treiber-/wgpu-Schicht“ unten war und bleibt eine **Vermutung**, kein
Messbefund und keine übernommene Root-Cause; ein Treiber-allein-Claim ist nicht
belegt. Auch der dokumentierte Pointer-Identity-Nebenbefund ist kein Beleg für
diesen Fehler.

Der nächste kleinste Schritt wurde als per Aufruf vom GPU-Renderer gelieferter
Ausführungsreport umgesetzt: Er meldet GPU-Abschluss erst nach erfolgreichem
finalem Readback und weist interne CPU-Fallbacks mit ihren Gründen aus. Der
Report belegt nur den Pfad dieses Frames (GPU-Submit plus abgeschlossenen
Readback oder CPU-Fallback); er beweist weder GPU-Parität sämtlicher Stufen noch
die Ursache eines Pixel-/PNG-Unterschieds. Fehler, einschließlich eines
fehlgeschlagenen Readbacks, bleiben Fehler und erzeugen keinen Erfolgsreport.
CLI-Verdrahtung, Route des roten Originaltests und Ursache/Fix bleiben
nachfolgende offene Arbeit.

### Vertrag der Welle 1

`GpuContext::render_with_gpu_report(&ImageFrame, &EditRecipe)` und
`GpuContext::render_with_gpu_and_generative_report(&ImageFrame, &EditRecipe,
&GenerativeCanvasInput)` liefern `Result<(Frame, RenderExecutionReport),
GpuError>`. `GpuPipelineCompleted` bedeutet, dass dieser Renderer den GPU-
Tone-/Pipeline-Pfad samt Queue-Submit durchlaufen und den finalen Readback
vollständig abgeschlossen hat. `CpuFallback { reason }` benennt die tatsächlich
getroffene interne Route (`GpuFeatureDisabled`, `AdapterOrDeviceUnavailable`,
`DeviceLost`, `UnsupportedStages` oder `PipelineUnavailable`). Jeder Render-
oder Readbackfehler bleibt `Err(GpuError)`; dafür gibt es keinen Report.
Bestehende `render_with_gpu`-Signaturen liefern weiterhin nur `Frame` und
delegieren auf denselben Pfad. Die Report-Aussage gilt ausschließlich für den
einzelnen Aufruf in `lumina-gpu`, ist kein GPU-Paritätsbeweis und ersetzt keine
äußere CLI-/GUI-Routenklassifikation. Der Report hat keinen globalen oder
statischen „letzter Pfad“-Zustand.

### Implementierungs- und Verifikationsstand der Welle 1 (2026-10-25)

Der Reportvertrag ist im `lumina-gpu`-Produktionspfad implementiert. Die
adaptergebundene Suite
`cargo test -p lumina-gpu --features gpu-adapter-tests --no-fail-fast`
bestand mit **114 passed / 0 failed / 0 ignored**. Darin prüfen insbesondere
`adapter_render_reports_gpu_completion_only_after_readback` den GPU-Abschluss
nach Readback und `unsupported_recipe_report_names_the_real_internal_cpu_route`
den gemeldeten Fallback. Die CPU-/Fehleranker
`cpu_only_report_names_the_compile_time_fallback`,
`device_loss_reports_cpu_fallback_for_that_call` und
`render_error_is_not_returned_as_an_execution_report` liefen ebenfalls grün.
Der Lauf ohne Default-Features ergab **33 passed / 0 failed / 4 ignored**; der
Cargo-Prozess endete mit Exit 0. Der aufzeichnende zsh-Wrapper endete separat
mit Exit 1, weil er `status` als schreibgeschützte Variable setzen wollte
(`target/gpu-execution-evidence/cargo-test-no-default-features.log`).

Der Produktionspfad wurde wirksam mutiert: der erfolgreiche GPU-Readback wurde
fälschlich als `CpuFallback { reason: DeviceLost }` ausgegeben. Der
Adaptertest scheiterte erwartungsgemäß mit Exit 101 an genau diesem Report
(`target/gpu-execution-evidence/adapter-report-mutation.log`); der unmutierte
Baseline-Test bestand mit **1/1**. Die Mutation wurde zurückgenommen und
`cargo clean -p lumina-gpu` ausgeführt. Zusätzlich bestanden `cargo fmt --all
-- --check`, `cargo clippy -p lumina-gpu --all-targets --all-features --
-D warnings`, `sh scripts/check_file_sizes.sh`, `sh
scripts/check_plan_format.sh`, `git diff --check` sowie alle fünf gelisteten
Konfigurationen des GPU-Crates (`target/gpu-execution-evidence/`). Diese
Ergebnisse sind lokale Gate-Messungen, keine unabhängige Verifikation und kein
CI-Nachweis.

Dieser Stand schließt nur die Report-/Beweis-Schnittstelle im GPU-Crate ab.
Der rote CLI-Test wurde in dieser Welle nicht erneut mit dem Report instrumentiert;
seine Route, der abweichende Fixture-Eintrag und die Ursache bleiben unbekannt.
Es wurde keine GUI-GPU-Suite ausgeführt. Damit ist weder der vollständige
Hardware-Task `GPU-RENDER-NONDETERMINISM-69-HW` abgeschlossen noch ein Fix oder
eine GPU-Parität aller Stufen belegt.

### Fixrunde Welle 1: Ressourcenloser GPU-Feature-Fallback (2026-10-25)

`resource_less_context_reports_adapter_fallback_for_both_render_entries` ist
ein zusätzlicher adapterunabhängiger Unit-Test in
`crates/lumina-gpu/src/tests/render_execution.rs` (**1 Test**, beide
Report-Entry-Points). Er konstruiert einen gültigen `GpuContext` mit
`resources: None`, ohne Adapterinitialisierung, Umgebungsvariable oder Skip.
Für `render_with_gpu_report` und
`render_with_gpu_and_generative_report` prüft er jeweils den exakten Grund
`AdapterOrDeviceUnavailable` und den Frame der Identitäts-CPU-Referenz. Der
Test belegt nur diesen ressourcenlosen internen Fallback; er belegt weder die
Adapter-/GPU-Route noch CLI- oder GUI-Routing.

Gemessene lokale Gates dieser Fixrunde (Rohlogs unter
`target/gpu-execution-evidence/`; die Cargo-Aufrufe wurden mit
`PATH="$HOME/.cargo/bin:$PATH"` ausgeführt):

- `cargo test -p lumina-gpu --features gpu --lib tests::render_execution::resource_less_context_reports_adapter_fallback_for_both_render_entries -- --exact`:
  **1 passed / 0 failed**, 63 gefiltert, Exit 0
  (`resource-less-render-baseline-gpu-path.log`).
  Der erste Aufruf ohne expliziten Cargo-Pfad endete vor Teststart mit Exit 127
  (`resource-less-render-baseline-gpu.log`, `zsh:1: command not found: cargo`); der
  erneute Aufruf mit dem oben genannten `PATH`-Präfix ist der grüne Lauf.
- Derselbe gezielte Test mit `--features gpu-adapter-tests`:
  **1 passed / 0 failed**, 64 gefiltert, Exit 0
  (`resource-less-render-baseline-adapter-tests.log`). Nach der Mutation und
  dem Restore wurde er erneut mit **1 passed / 0 failed**, Exit 0 ausgeführt
  (`resource-less-render-restored.log`).
- `cargo test -p lumina-gpu --no-default-features --lib`:
  **27 passed / 0 failed / 0 ignored**, Exit 0
  (`resource-less-render-no-default-regression.log`). Der neue Test ist in
  dieser Konfiguration wegen `#[cfg(feature = "gpu")]` nicht enthalten.
- Mutationsnachweis am Produktionszweig `render_dispatch.rs`: dessen
  ressourcenloser Reportgrund wurde gezielt von
  `AdapterOrDeviceUnavailable` auf `DeviceLost` geändert. Der gezielte Test
  wurde **0 passed / 1 failed**, Exit 101; die Assertion meldete tatsächlich
  `left: CpuFallback { reason: DeviceLost }` gegen
  `right: CpuFallback { reason: AdapterOrDeviceUnavailable }`. Exakter Patch:
  `resource-less-render-mutation.patch`, Rohlog:
  `resource-less-render-mutation.log`. Die Mutation wurde zurückgenommen,
  `cargo clean -p lumina-gpu` endete mit Exit 0
  (`resource-less-render-restore-clean.log`), und der wiederhergestellte Test
  bestand wie oben protokolliert.
- `cargo fmt --all -- --check`, `cargo clippy -p lumina-gpu --all-targets
  --all-features -- -D warnings`, `sh scripts/check_file_sizes.sh`,
  `sh scripts/check_plan_format.sh` und `git diff --check` beendeten alle mit
  Exit 0 (`resource-less-render-fmt-check.log`,
  `resource-less-render-clippy.log`, `resource-less-render-file-sizes.log`,
  `resource-less-render-plan-format.log`,
  `resource-less-render-diff-check.log`).

Die unabhängige Nachprüfung bestätigte diese Fixrunde am 2026-10-25 mit
**APPROVED**, ohne neue Befunde; ein CI-Lauf steht noch aus. Die bereits genannten 114
adapterabhängigen Tests wurden in dieser ausschließlich unit-testbezogenen
Fixrunde nicht erneut ausgeführt.

## Ziel und Geltungsbereich

Dieses Dokument hält die **Grenze** der headless Untersuchung des
GPU-Render-Nichtdeterminismus fest. Es trennt strikt **gemessen**,
**abgeleitet** und **Vermutung** und listet die **Nicht-Behauptungen**
explizit auf. Es ist die dokumentarische Grundlage für die Aufteilung des
Tasks `GPU-RENDER-NONDETERMINISM-69` nach der Regel
`HEADLESS-GPU-TASK-SPLIT` (`Agents.headless.md` §1 Punkt 6).

## Gemessen (read-only, 2026-10-16)

### Adapterlage

- **Kein wgpu-Adapter auf dieser Maschine.** Beleg:
  `cargo test -p lumina-gpu --features gpu-adapter-tests --test stages upload_mask_plane_roundtrip_is_byte_exact -- --exact`
  → Panik in `crates/lumina-gpu/tests/support/mod.rs:45`
  („this test requires a real GPU adapter, but none was bound").
- Zusätzlich: kein `/dev/dri`, kein `/dev/kfd`, kein `vulkaninfo`.

### Roter Test ist headless nicht reproduzierbar

- `cargo test -p lumina-cli --features gpu --bin lumina-cli tests::auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move -- --exact`
  → **3/3 grün** (`1 passed; 158 filtered out`).
- Grund: ohne Adapter nimmt `render_best_effort`
  (`crates/lumina-cli/src/main.rs:350`) im `_ =>`-Arm
  (`crates/lumina-cli/src/main.rs` → `render_best_effort`)
  die CPU-Route: `render_frame_with_generative(...)` →
  `Ok((output, RenderRoute::Cpu { reasons }))`.
- Die grünen Läufe belegen die **CPU-Route**, **nicht** die GPU-Route.

### Keine lokalisierbare Nichtdeterminismus-Quelle im Projekt-Code

Der Auto-Tone-GPU-Pfad ist **synchron/blocking**:
`map_async` + `poll(wgpu::PollType::wait_indefinitely)` + `rx.recv()`
(`crates/lumina-gpu/src/lib.rs:3615-3628`, `crates/lumina-gpu/src/gpu_util.rs` → `readback_texture`).

Der Pass nutzt `LoadOp::Clear(0,0,0,1)` + `StoreOp::Store` +
Fullscreen-Triangle (`crates/lumina-gpu/src/lib.rs` → `render_with_gpu_inner`).

Das Readback liest exakt `width*4` Bytes je Zeile und überspringt Padding
(`crates/lumina-gpu/src/lib.rs` → `render_with_gpu_inner`).

Das GPU-Ressourcen-Pooling ist ein `Mutex<HashMap<…>>` (`crates/lumina-gpu/src/lib.rs` →
`GpuContext`, `render_with_gpu_inner`).

Der Tone-Shader verwendet **keine** Atome (die einzige `atomicMax`-Verwendung
steht in `crates/lumina-gpu/src/stages.rs` → `SHARPEN_GRADIENT_SRC`, im Fixture
inaktiv).

**Historische Vermutung, nicht als Ursache belegt:** verbleibende Kandidaten
könnten in der Treiber-/wgpu-Schicht liegen. Der Adapterbefund vom 2026-10-25
hat diese Hypothese nicht bestätigt; siehe den Zusatz oben.

### Nebenbefund (ehrlich benannt, NICHT als Ursache behauptet)

Der Input-Upload überspringt auf **Pointer-Identität** statt Content-Hash
(`crates/lumina-gpu/src/lib.rs:3263-3285`) — echtes latentes Risiko, im
konkreten Test nicht auslösbar (gleiche Quelldatei pro Lauf). **Kein Fix.**

## Abgeleitet

- Der rote Lauf des Tests `the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move`
  ist auf dieser Maschine **nicht** reproduzierbar, weil die GPU-Route ohne
  Adapter nicht aktiviert wird. Die CPU-Route ist grün.
- Die ursprünglichen Rot-Läufe (Befund 2026-10-15) waren auf einer Maschine
  mit Adapter aufgetreten; die CPU-Route war dort ebenfalls grün.

## Vermutung (nicht als Befund)

- Readback, uninitialisierter VRAM-Speicher oder ein Race sind **denkbar**.
  Eine Vermutung wird hier **nicht** als Befund ausgegeben.

## Nicht-Behauptungen

- **Keine Aussage**, ob der GPU-Pfad auf Adapter-Hardware wirklich
  nichtdeterministisch ist.
- **Keine Aussage**, der Test sei auf Adapter-Hardware grün.
- **Kein Fix** vorgeschlagen.

## Benannte Grenze

Reproduktion und Behebung des Nichtdeterminismus sind **nur auf einer
Maschine mit gebundenem Adapter** (Metal/Vulkan) möglich. Auf dieser
headless Maschine ist die GPU-Route nicht aktivierbar; die CPU-Route ist
grün und belegt nichts über die GPU-Route.

## Beweisplan (auf Adapter-Hardware)

1. **Reproduktion:** Auf einer Maschine mit gebundem Adapter (Metal/Vulkan)
   den Test `cargo test -p lumina-cli --features gpu --bin lumina-cli tests::auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move -- --exact`
   fahren. Erwartet: der Test ist laut Ausgangsbefund rot.
2. **Adapter-Nachweis:** `cargo test -p lumina-gpu --features gpu-adapter-tests`
   plus die betroffenen GUI-GPU-Tests (Entscheidung `GPU-E2E`).
3. **Mechanismus eingrenzen:** Den Nichtdeterminismus bis zu einer belegten
   Aussage eingrenzen (z. B. zweimaliger GPU-Render im selben Prozess vs.
   zwei Prozesse, betroffene Stufen, Einfluss der Testparallelität).
4. **Beheben oder Grenze bestätigen:** Den Mechanismus beheben oder als
   dauerhafte Grenze mit Messwerten dokumentieren.

## Abnahme

- Ein **vorher rotes, reproduzierbares** Kommando auf Adapter-Hardware,
  danach grün **oder** eine belegte, benannte Grenze inkl. Adapter-Referenz.
- Verbindliches Kommando: `cargo test -p lumina-gpu --features gpu-adapter-tests`
  plus die betroffenen GUI-GPU-Tests (Regel `GPU-E2E`).
