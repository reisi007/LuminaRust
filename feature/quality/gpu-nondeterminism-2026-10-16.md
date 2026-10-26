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

### SOLL-Vertrag Welle 2: CLI-Ausführungsreport und Auto-Tone-Diagnose

Der volle CLI-Produktionseinstieg muss denselben pro-Aufruf-Report von
`lumina-gpu` durch `render_best_effort`, den Standard-Render-Einstieg und
`process_selected` bis zu einer internen Diagnose-Testnaht weitergeben. Die
Testnaht ruft genau denselben Produktionspfad auf; sie darf keine zweite
Renderpipeline, globale „letzte Route"-Probe oder reine Logger-Probe verwenden.
Eine Adapter-/Diagnose-Testvariante ist opt-in und fügt weder ein sichtbares
CLI-Flag noch ein Produktfeature hinzu. Bestehende CLI-Einstiege behalten ihre
Signaturen und ihr Exportverhalten; GPU-freie Builds bleiben ausführbar.

Der an die CLI weitergereichte GPU-Report ist ausschließlich der Report, den
`GpuContext::render_with_gpu_and_generative_report` für genau diesen Aufruf
zurückgibt. Wird vor dem GPU-Aufruf auf CPU geroutet oder ist kein Kontext
vorhanden, ist der GPU-Report abwesend; der äußere CLI-Zweig darf keinen
`GpuPipelineCompleted`- oder `CpuFallback`-Report erfinden. Die tatsächliche
CLI-Route ist nur dann GPU, wenn der Report `GpuPipelineCompleted` meldet und
die nachfolgende lokale Maskenkomposition erfolgreich ist. Ein interner
`CpuFallback` macht die Gesamt-CLI-Route CPU, auch wenn eine nachfolgende
Maskenkomposition gelingt. Sein Grund wird neben bereits vorhandenen
CLI-Routinggründen und Warnungen erhalten und sichtbar protokolliert. Bei einem
Fehler des GPU-Aufrufs bleibt das Ergebnis ein Fehler ohne Erfolgsreport.
`render_draft` ist kein Nachweis des vollen GPU-Pfads. Der Report behauptet
weder, dass alle CLI-Stufen GPU-ausgeführt wurden, noch GPU-Parität oder eine
Fehlerursache. Der separat über `export_image_with_generative` erzeugte
`match_total_exposure`-Endexport wird durch den Report des vorherigen
Standard-Render-Aufrufs nicht klassifiziert.

Die ursprünglichen Auto-Tone-Familien-Assertions und ihre Testnamen bleiben
erhalten. Eine zusätzliche, explizit gestartete Adaptervariante fährt dieselben
40 synthetischen Familienmitglieder je dreimal über `process_selected`; alle
120 Aufrufe müssen je einen echten `GpuPipelineCompleted`-Report und die
entsprechende CLI-GPU-Route liefern. Fehlender Adapter oder CPU-Fallback ist ein
harter Testfehler, kein Skip. Der Test bleibt im normalen CPU-/Headless-Lauf
außerhalb der Ausführung.

Bei einer Abweichung werden das konkrete Familienmitglied und die betroffenen
Vergleichspaare 1-vs-2 bzw. 2-vs-3 ausgewiesen. PNG-Bytes und separat dekodierte
RGBA8-Pixel werden unabhängig verglichen; ein PNG-Byte-Mismatch allein ist kein
Pixel-Mismatch. Für jeden Aufruf des abweichenden Mitglieds werden Report,
Sidecar-Text und Sidecar-Hash zusammen mit den drei Roh-PNGs und den dekodierten
Pixeldateien unter
`target/gpu-render-nondeterminism-69/<dated-run>/` gesichert, bevor dessen
temporäres Verzeichnis endet. Verwendet werden ausschließlich die synthetischen
Testquellen; persistierte Sidecars enthalten keine absoluten Pfade.

Die Stabilitätsassertion betrifft die sechs gespeicherten Auto-Tone-Slider-
Literale, nicht den vollständigen Sidecar-Text: beobachtet wurden angehängte
History-Einträge zwischen erfolgreichen `process`-Aufrufen. Der vollständige
Sidecar-Text und sein Hash bleiben pro Aufruf als Diagnoseartefakte erhalten.

Ein roter Adapterlauf bleibt ein Befund und wird nicht durch eine Toleranz,
einen ignorierten Test oder zufälliges Wiederholen grün gemacht. Drei serielle
Läufe mit Exitcode, Rohlog und gegebenenfalls Diagnoseartefakten sind getrennt
zu berichten. Solange der Mechanismus nicht gemessen ist, bleiben Ursache und
Fix offen.

### Implementierungs- und Adaptermessung Welle 2 (2026-10-25)

`crates/lumina-cli/src/render_route.rs::render_best_effort_with_report` reicht
den Report des GPU-Aufrufs zusammen mit dem Ergebnis weiter;
`crates/lumina-cli/src/render_route.rs::render_standard_with_generative_report`
ist der Report-tragende Standard-Render-Einstieg. Der bestehende
`process_selected`-Einstieg in `crates/lumina-cli/src/process_cmd.rs` behält sein
bisheriges Ergebnis und verwirft den Report für gewöhnliche Aufrufer; die
Diagnose ruft `process_selected_with_report` in
`crates/lumina-cli/src/main.rs` auf demselben Produktionspfad auf. Vorab-CPU-
Routen liefern keinen GPU-Report. Die CLI-Route ist GPU genau dann, wenn der
echte Report `GpuPipelineCompleted` ist und die lokale Maskenkomposition gelingt.
`route_after_mask_stage` in `crates/lumina-cli/src/render_route.rs` trifft die
Routen-Entscheidung an **einer** Stelle aus GPU-Report und Maskenergebnis; der
`Err`-Zweig in `render_best_effort_with_report` wählt keinen eigenen Grund. Die
vier Tests `completed_gpu_report_with_successful_mask_stage_routes_gpu_without_reasons`,
`completed_gpu_report_with_failed_mask_stage_uses_exact_mask_reason`,
`internal_cpu_fallback_with_successful_mask_stage_keeps_exact_internal_reason`
und `internal_cpu_fallback_with_failed_mask_stage_keeps_both_reasons_in_order`
in `crates/lumina-cli/src/render_route_execution_report_tests.rs` prüfen alle
Kombinationen mit exakten Grundlisten. Die Vorab-CPU-Assertion bleibt separat
durch `pre_routed_cpu_render_has_no_fabricated_gpu_report` erhalten. Dass die
Kombination „`composite`
scheitert, CPU-Referenz gelingt“ heute nicht konstruierbar ist, ist eine Aussage
über die Routing-Vorbedingungen (`mask_layer_routing_reasons`), nicht über die
getestete Zuordnung. Die Adaptervariante
`tests::auto_tone_float::adapter_runs_of_the_full_process_path_report_gpu_for_all_120_renders`
bleibt ignoriert und wird nur explizit gestartet.

Lokale Gates (mit dem freigegebenen `TMPDIR` für die CPU-Tests) bestanden:

- `cargo fmt --all -- --check`
- `cargo test -p lumina-cli --no-default-features --bin lumina-cli`:
  **148 passed / 0 failed**
- `cargo test -p lumina-cli --features gpu --bin lumina-cli execution_report_tests -- --test-threads=1 --nocapture`:
  **4 passed / 0 failed**
- `cargo clippy -p lumina-cli --all-targets --features gpu,lensfun -- -D warnings`
- `sh scripts/check_cli_features.sh`: alle deklarierten CLI-Featuremengen
- `sh scripts/check_file_sizes.sh`, `sh scripts/check_plan_format.sh` und
  `git diff --check`

Der Produktionsreport wurde wirksam mutiert, indem der erfolgreiche GPU-Zweig
`gpu_report: None` statt des echten Reports zurückgab. Die Diagnose endete
erwartungsgemäß mit Exit **101**: alle 120 äußeren CLI-Routen meldeten GPU,
aber **0/120** Reports waren `GpuPipelineCompleted` (`left: 0`, `right: 120`).
Beleg: `target/gpu-render-nondeterminism-69/2026-10-25T193954Z/` mit
`production-wiring-mutation.patch`, `production-wiring-mutation.log`,
`production-wiring-mutation.exit` und den SHA-256-verifizierten
`mutation-artifacts/`. Dieser Mutationslauf verwendete noch den später
korrigierten Volltext-Sidecarvergleich; dessen Sidecar-Fehlalarme und die dabei
gemessene Pixelabweichung für Mitglied 26 werden daher nicht in die folgende
unmutierte Adaptermessung eingerechnet. Die Produktionszuweisung wurde exakt
aus der vor der Mutation gespeicherten Datei wiederhergestellt.

Der unmutierte Adapterlauf wurde mit
`LUMINA_GPU_BACKENDS=metal cargo test -p lumina-cli --features gpu --bin lumina-cli tests::auto_tone_float::adapter_runs_of_the_full_process_path_report_gpu_for_all_120_renders -- --ignored --exact --test-threads=1 --nocapture`
dreimal seriell ausgeführt. Alle drei Läufe endeten mit Exit **101**. In jedem
Lauf bestanden die vorgelagerten Assertions für **120/120** echte
`GpuPipelineCompleted`-Reports und CLI-GPU-Routen; der abschließende Fehler war
die PNG-/Pixel-Reproduzierbarkeit, nicht ein fehlender Adapter oder ein
CPU-Fallback. Der neue Vergleich meldete:

| Lauf | Abweichende Mitglieder und Paar | Abweichende RGBA8-Pixel |
|---|---|---:|
| 1 | 01 (1-vs-2), 12 (1-vs-2), 28 (2-vs-3), 31 (1-vs-2) | 254, 256, 256, 256 |
| 2 | 01 (1-vs-2), 12 (1-vs-2), 33 (1-vs-2) | 254, 256, 256 |
| 3 | 01 (1-vs-2), 17 (1-vs-2), 31 (1-vs-2), 33 (1-vs-2), 39 (1-vs-2) | 254, 256, 256, 256, 256 |

Jeder aufgeführte RGBA8-Mismatch hatte auch abweichende PNG-Bytes; nicht
aufgeführte Vergleichspaare waren innerhalb ihres jeweiligen Mitglieds gleich.
Für jedes abweichende Mitglied waren die sechs Slider-Literale in allen drei
Aufrufen gleich, während die vollständigen Sidecar-Hashes voneinander abwichen.
Die Rohlogs, Exitcodes, vollständigen Sidecars/Hashes, drei PNGs, dekodierten
Pixel und Per-Call-Reports liegen unter
`target/gpu-render-nondeterminism-69/2026-10-25T195812Z/`; die Gegenprüfung der
archivierten PNG-/Pixelpaare steht in `adapter-artifact-review.txt`.

**Beobachtung** (Artefakte: `target/gpu-render-nondeterminism-69/2026-10-25T195812Z/adapter-artifact-review.txt` und die dort archivierten Paarvergleiche): Jedes abweichende Mitglied hat genau ein abweichendes Vergleichspaar, nie beide. Stets weicht entweder das erste Paar (1-vs-2) oder das zweite (2-vs-3) ab; das jeweils andere Paar ist byte- und pixelgleich. Mitglied 01 ist in allen drei Prozessläufen betroffen; die übrigen Mitglieder wechseln zwischen den Läufen (Lauf 1: 01/12/28/31, Lauf 2: 01/12/33, Lauf 3: 01/17/31/33/39). Mitglied 01 hat 254 abweichende Pixel, erster Index 1; alle übrigen aufgeführten Paare haben 256 abweichende Pixel, erster Index 0. Diese Regelmäßigkeit benennt weiterhin keine Ursache.

Der unveränderte Original-Reproducer
`tests::auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move`
wurde danach einmal separat mit Metal ausgeführt und endete ebenfalls mit Exit
**101**: **38/40** Mitglieder waren byte-identisch (Erwartung: 40/40). Diese
Assertion lokalisiert die abweichenden Mitglieder nicht. Der Adapter-Report
schließt die frühere Beweislücke zur Route des CLI-Prozesses, nicht zur Ursache
der Pixelabweichungen. Die drei Adapterläufe bleiben rot dokumentiert; es wurde
weder eine Toleranz eingeführt noch ein Ergebnis durch Wiederholung als grün
gewertet. Ursache und Fix sind weiterhin offen, und
`GPU-RENDER-NONDETERMINISM-69-HW` ist nicht abgeschlossen.

Die frühere Aussage, die Quellkopien seien nicht gelöscht worden, wird
zurückgenommen: der Quellpfad
`crates/lumina-cli/target/gpu-render-nondeterminism-69/2026-10-25T195812Z/`
ist heute nicht vorhanden, nachdem die Kopien nach dem Abgleich entfernt wurden.
Der SHA-256-Abgleich ist damit heute nicht erneut nachprüfbar. Erhalten ist das
kanonische Archiv `target/gpu-render-nondeterminism-69/2026-10-25T195812Z/`;
`find target/gpu-render-nondeterminism-69/2026-10-25T195812Z -type f | wc -l`
ergab **242 Dateien**. Grund der Rücknahme ist der heutige Dateibaum, der der
früheren Behauptung widerspricht.

### Fixrunde Welle 2: wirksame Routenmutationen (2026-10-25)

Die Fixrunde ergänzte `route_after_mask_stage` in
`crates/lumina-cli/src/render_route.rs`: die Funktion erhält Report und
Maskenergebnis gemeinsam und ist die **einzige** Stelle, die daraus die Route
und Gründe bildet. Der `Err`-Zweig des Renderpfads wählt keinen eigenen Grund.
Die vier Kombinationen werden mit exakten Grundlisten von
`completed_gpu_report_with_successful_mask_stage_routes_gpu_without_reasons`,
`completed_gpu_report_with_failed_mask_stage_uses_exact_mask_reason`,
`internal_cpu_fallback_with_successful_mask_stage_keeps_exact_internal_reason`
und `internal_cpu_fallback_with_failed_mask_stage_keeps_both_reasons_in_order`
im ausgelagerten Testmodul geprüft. Die Kombination „`composite` scheitert,
CPU-Referenz gelingt“ ist heute nicht konstruierbar; das beschreibt die
Routing-Vorbedingungen (`mask_layer_routing_reasons`), nicht die getestete
Zuordnung.

Die Produktionszuordnung wurde mit drei Mutationen geprüft. M1 und M2 liefen
unter `target/gpu-render-nondeterminism-69/2026-10-25T202856Z/` und richteten
sich gegen die damalige Form (`crates/lumina-cli/src/render_route.rs` ->
`route_from_gpu_report`). M1 zwang den `GpuPipelineCompleted`-Fall mit einem
zusätzlichen Mutationsgrund auf CPU; der Lauf endete mit Exit **101**. Rot
wurden die damaligen Tests
`gpu_cli_route_requires_completed_report_and_successful_mask_stage` und
`completed_gpu_report_with_failed_mask_stage_is_cpu_with_the_mask_reason`;
letzterer wies den erfundenen Zusatzgrund nach. Beide Tests wurden in der dritten
Runde durch die vier Kombinationstests ersetzt (siehe oben). M2 zwang
`CpuFallback` auf die GPU-Route; Exit **101**, wobei
`internal_cpu_fallback_report_forces_cpu_cli_route` rot wurde. Die Belege heißen
`M1-production-wiring-mutation.patch`, `.log` und `.exit` sowie
`M2-production-wiring-mutation.patch`, `.log` und `.exit`.

M3 richtete sich gegen die heutige Form und entfernte in
`crates/lumina-cli/src/render_route.rs` -> `route_after_mask_stage` den Beitrag
des Maskengrunds im `Err`-Fall. Der Lauf endete mit Exit **101** und
**3 passed / 2 failed**; rot wurden
`completed_gpu_report_with_failed_mask_stage_uses_exact_mask_reason` (erwartete
`["local mask rejected"]`, gemessen `[]`) und
`internal_cpu_fallback_with_failed_mask_stage_keeps_both_reasons_in_order`
(erwartete beide Gründe, gemessen nur der interne). Die Belege liegen unter
`target/gpu-nondeterminism-69/2026-10-25T210939Z/` — abweichend vom oben
genannten Muster `target/gpu-render-nondeterminism-69/<dated-run>/`, weil dieser
Lauf einen Verzeichnisnamen ohne das Segment `render-` verwendet hat; das ist
eine Namensabweichung, keine inhaltliche. Dort stehen
`M3-production-wiring-mutation.patch`, `M3-mutated-test.log`, `M3-mutated-test.exit`,
`M3-restored-test.log`, `M3-restored-test.exit`, `M3-restore-cmp.exit` und die
Hashdateien.

Vor den Mutationen wurde `render_route.rs` als
`render_route.rs.unmutated` gesichert (SHA-256
`487d896510cc3f21c95f18d186b9f4ecad8be53366311f84de7dcbe0b4488151` für M1/M2;
M3 sicherte die Fassung der dritten Runde mit
`205f4bbfca69cff1b4ebceb52e7422375ff4d212f63f4c05be1535a5179b1aea`). Nach jeder
Mutation wurde die jeweils gesicherte Kopie byte-identisch zurückgeschrieben
(`cmp` Exit 0); die Hashdateien `M1-mutated-source.sha256` /
`M1-restored-source.sha256`, `M2-mutated-source.sha256` /
`M2-restored-source.sha256` sowie `M3-source-before.sha256` /
`M3-source-mutated.sha256` / `M3-source-restored.sha256` dokumentieren die
jeweiligen Zustände:

- M1-mutiert: `9887e4f2d55c7855c8b87d03273281438eabab56034fd520c63e7862d6beee26`
- M2-mutiert: `4c5bbb408ca382f9a8d30f8db3d32d8cf5cc503cf600ee5799491b9c4860725c`
- M3-mutiert: `96ffea539a8915ab50004040756f615a7667f0026ff79de98985d6d3c7b45cda`
- Restores M1/M2: `487d896510cc3f21c95f18d186b9f4ecad8be53366311f84de7dcbe0b4488151`
- Restore M3: `205f4bbfca69cff1b4ebceb52e7422375ff4d212f63f4c05be1535a5179b1aea`

Nach jedem Restore liefen `cargo clean -p lumina-cli` und derselbe unmutierte
GPU-Testlauf erneut: jeweils **5 passed / 0 failed**, Exit 0.

Die abschließenden Gates dieser Fixrunde bestanden: `cargo fmt --all -- --check`,
`cargo test -p lumina-cli --features gpu --bin lumina-cli execution_report_tests -- --test-threads=1`
(**5 passed / 0 failed**),
`cargo test -p lumina-cli --no-default-features --bin lumina-cli`
(**148 passed / 0 failed**),
`cargo clippy -p lumina-cli --all-targets --features gpu,lensfun -- -D warnings`,
`sh scripts/check_file_sizes.sh`, `sh scripts/check_plan_format.sh` und
`git diff --check`. `render_route.rs` hat **395** Zeilen, das neue Testmodul
**74**; `crates/lumina-cli/src/main.rs` bleibt bei **889**.

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

Der Input-Upload überspringt auf **Pointer-Identität** statt auf einen
Content-Hash: `crates/lumina-gpu/src/render_dispatch.rs`
(`input_source_identity`, Schlüssel `(frame.pixels().as_ptr() as usize,
frame.pixels().len())`) und `crates/lumina-gpu/src/render_vram.rs` (derselbe
Schlüssel für die VRAM-Basis). **Kein Fix** (Stand 2026-10-16; am 2026-10-25
als `GPU-UPLOAD-IDENTITY-92` behoben — siehe „Mechanismus lokalisiert" unten).

**Korrektur 2026-10-25 — die frühere Begründung wird an dieser Stelle
zurückgenommen.** Sie lautete „im konkreten Test nicht auslösbar (gleiche
Quelldatei pro Lauf)". Für den Auto-Tone-Familiensatz trifft diese Begründung
**nicht** zu: der Test rendert 40 **verschiedene** Quellen im **selben** Prozess,
alle 16×16 Pixel und 1024 Byte groß
(`crates/lumina-cli/src/tests/support/auto_tone.rs` → `auto_tone_family_frame`).
Für einen Schlüssel aus Adresse und Länge ist „anderer Inhalt, gleiche Länge"
nicht unterscheidbar; eine wiederverwendete Allocation würde den
Vorgängerinhalt rendern. Damit ist die gefährliche Richtung im Test **erreichbar**
— die frühere Einordnung betrachtete nur die drei Läufe *eines* Mitglieds.

Ob dieser Mechanismus die beobachtete Abweichung erklärt, ist **nicht gemessen**
und wird hier nicht behauptet. Er ist eine prüfbare Hypothese mit einem
benennbaren Kriterium: wird die Upload-Identität inhaltsbasiert (oder wird immer
hochgeladen), muss der Familiensatz byte-stabil werden; bleibt er es nicht, ist
die Hypothese widerlegt.

### Mechanismus lokalisiert: Upload-Skip (2026-10-25, Entscheidung `GPU-UPLOAD-ID`)

Der Nebenbefund ist nicht mehr nur eine Hypothese: ein A/B-Lauf grenzt den
**Mechanismus** ein. Gemessen wurde am 2026-10-25 auf demselben Host wie der
Adapterbefund (macOS, Apple M5 Pro / Metal, `LUMINA_GPU_BACKENDS=metal`):

- **A (Skip abgeschaltet):** der Upload-Skip wurde an **beiden** Stellen
  (`crates/lumina-gpu/src/render_dispatch.rs`,
  `crates/lumina-gpu/src/render_vram.rs`) entfernt, sodass jeder Render die
  Quelle neu hochlädt; `cargo build -p lumina-cli --features gpu` endete mit
  Exit **0**. Danach lief der Auto-Tone-Familiensatz
  (`tests::auto_tone_float::adapter_runs_of_the_full_process_path_report_gpu_for_all_120_renders`,
  `--ignored --exact --test-threads=1`) dreimal seriell mit Exit **0/0/0** und
  **0/40** abweichenden Mitgliedern; der Original-Reproducer
  (`tests::auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move`)
  bestand ebenfalls mit Exit **0**.
- **B (byte-identischer Restore):** beide Dateien wurden aus den vor der
  Mutation gesicherten Kopien zurückgeschrieben; `cmp` endete für beide mit
  Exit **0**, und `source-before.sha256`/`source-after.sha256` nennen dieselben
  SHA-256-Werte. Der anschließende unmutierte Wiederholungslauf des
  Familiensatzes endete mit Exit **101**: **1/40** abweichende Mitglieder,
  Mitglied **31**, Paar **1-vs-2**, **256** abweichende RGBA8-Pixel (erster
  Index `0`), PNG- und Pixelvergleich beide ungleich.
- Belege: `target/gpu-render-nondeterminism-69/20261025T214457Z/`
  (`mutated-build.*`, `mutated-diagnostic-run-1..3.*`,
  `mutated-original-reproducer.*`, `source-before.sha256`,
  `source-after.sha256`, `render_dispatch.rs.orig`, `render_vram.rs.orig`,
  `*.restore-cmp.txt`, `unmutated-confirmation.*`). Ein Mutations-Patchfile
  wurde in diesem Lauf nicht abgelegt; belegt sind die gesicherten
  Originalkopien, die SHA-256-Werte und die `cmp`-Exitcodes.

**Damit gilt der ursprüngliche Rotlauf als mechanismisch lokalisiert, nicht als
vollständig erklärt.** Belegt ist: der Schlüssel `(pixels_ptr, pixels_len)`
kann bei „anderer Frame, gleiche Länge, wiederverwendete Allocation" einen
**veralteten** Upload weiterverwenden, und das Abschalten des Skips macht den
Familiensatz in diesem Lauf byte-stabil. Nicht belegt ist, dass dieser
Mechanismus **jede** beobachtete Abweichung erklärt: das A/B ist je Zustand ein
Lauf (A dreifach, B einfach), die Rotläufe vom 2026-10-15/16 haben keine
Rohartefakte, und ein Beitrag von Treiber/wgpu ist damit nicht ausgeschlossen.

**Entscheidung des Eigentümers (2026-10-25, `GPU-UPLOAD-ID`):** die
Upload-Identität ist die **Frame-Instanz**, nicht die Adresse, nicht ein
Inhalts-Hash und nicht ein Aufrufer-Token. `ImageFrame` trägt eine
prozesslokale, monotone `u64`-ID (`ImageFrame::new` vergibt sie, `Clone` erhält
sie, `ImageFrame::instance_id` liest sie; **nicht** Teil von `PartialEq`/`Eq`,
nie persistiert). `should_upload` in
`crates/lumina-gpu/src/vram_pool/upload_identity.rs` überspringt nur bei
gleicher Instanz **und** gleicher Geometrie; beide Upload-Stellen rufen genau
diese Funktion auf (normativ:
`feature/architecture/pipeline.md` → „Implementierungsstatus GPU-Pfad" →
„Input-Upload-Identität"). Zwei verschiedene Frames mit gleichem Inhalt oder
gleicher Länge teilen sich damit nie einen Upload; dasselbe Frame-Objekt (auch
geklont) schon.

#### Fixstand und Messung (2026-10-25, lokale Gates, keine unabhängige Verifikation)

- **Ort:** die ID liegt in `crates/lumina-core/src/frame_identity.rs` (Typ,
  Allokation, `instance_id()`, manuelles `PartialEq`/`Eq` über
  `width`/`height`/`pixels`); die Entscheidung in
  `crates/lumina-gpu/src/vram_pool/upload_identity.rs` (`should_upload`,
  `UploadIdentity { instance_id, width, height }`); der gemeinsame Transfer in
  `crates/lumina-gpu/src/vram_pool/source_upload.rs`
  (`upload_source_texture`). `crates/lumina-gpu/src/lib.rs` →
  `RenderWithGpuResources::input_source_identity` und
  `crates/lumina-gpu/src/vram_pool.rs` → `VramState::input_source_identity`
  tragen `Option<UploadIdentity>`; kein zweiter Vergleich bleibt.
- **Tests:** `crates/lumina-core/src/frame_identity.rs` →
  `new_assigns_a_fresh_instance_id_per_call`, `clone_keeps_the_instance_id`,
  `equality_ignores_the_instance_id_but_not_the_content`;
  `crates/lumina-gpu/src/vram_pool/upload_identity.rs` →
  `same_instance_and_geometry_skips_the_upload`, `cloned_frame_keeps_the_upload`,
  `another_instance_with_identical_pixels_and_length_uploads` (der Kern-Test
  gegen den Defekt), `another_geometry_uploads`,
  `no_previous_upload_uploads`; adaptergebunden
  `crates/lumina-gpu/tests/upload_identity.rs` →
  `readback_path_renders_the_second_frame_with_equal_length` und
  `vram_path_renders_the_second_frame_with_equal_length` (zwei Frames,
  verschiedene Inhalte, gleiche Länge 16 Byte; das zweite Ergebnis muss dem
  frischen Kontext entsprechen und darf den ersten Render nicht wiederholen).
- **Mutationsnachweis:** `should_upload` wurde auf einen reinen
  Geometrievergleich zurückgebaut (`previous.width != width || previous.height
  != height`, Instanz ignoriert) — also genau der alte Defekt. Der Kern-Test
  wurde rot (**4 passed / 1 failed**, Exit **101**), und **beide**
  adaptergebundenen Tests wurden rot (**0 passed / 2 failed**, Exit **101**):
  beide meldeten den Pixelinhalt des **ersten** Frames als zweites Ergebnis.
  Der Patch liegt als `instance-identity-mutation.patch`, die Läufe als
  `mutated-unit-test.log`/`.exit` und `mutated-adapter-test.log`/`.exit`; der
  Restore ist byte-identisch (SHA-256
  `61ed3d047c31cd8afa77fd9b1dc02d614d16513742466ac5900e83b9262706c1` vor und
  nach der Mutation, `cmp` Exit **0**), danach `cargo clean -p lumina-gpu`
  (Exit **0**). Artefakte:
  `target/gpu-render-nondeterminism-69/20261025T223514Z/`.
- **Adaptermessung nach dem Fix** (Metal, `LUMINA_GPU_BACKENDS=metal`): der
  Auto-Tone-Familiensatz lief dreimal seriell mit Exit **0/0/0** und meldete
  **keine** Abweichung (0/40); der Original-Reproducer bestand danach dreimal
  (Exit **0/0/0**). Belege:
  `target/gpu-render-nondeterminism-69/20261025T225434Z/` (`run-1..3.*`,
  `reproducer-1..3.*`).
- **Grenze dieser Messung:** drei grüne Läufe widerlegen die Resthypothese
  (Treiber/wgpu-Anteil) nicht, und sie sind kein Beweis für beliebige
  Frame-Folgen außerhalb dieses Tests; der A/B-Beleg oben bleibt je Zustand
  eine Stichprobe. `GPU-RENDER-NONDETERMINISM-69-HW` bleibt offen, bis ein
  unabhängiger Lauf und der manuelle Fahrplan vorliegen.
- **Portabilitätsfehler im Test-Target von `lumina-cache` — im selben Batch
  behoben (Nachtrag 2026-10-26):** `cargo check --workspace --all-targets` brach
  auf macOS in `crates/lumina-cache/src/lib.rs` → `current_umask` ab
  (`libc::mode_t` ist dort `u16`, deklariert war `-> u32`, E0308). Der Helfer
  gibt den Wert jetzt als `u64` zurück (`mask as u64`) und die Assertion
  vergleicht `u64::from(mode & 0o777)` dagegen. Die erste Fassung mit
  `u32::from(mask)` war auf macOS grün, wäre aber auf Linux eine
  Identitätskonvertierung und damit unter `-D warnings` ein
  `clippy::useless_conversion`-Fehler gewesen; `mode_t` ist nie `u64`, also ist
  die Verbreiterung auf beiden Plattformen eine echte Konvertierung. Gemessen:
  `cargo clippy -p lumina-cache --all-targets --target
  x86_64-unknown-linux-gnu -- -D warnings` → Exit 0 (Linux-Ziel lokal
  installiert, Test-Target eingeschlossen) und `cargo check --workspace
  --all-targets` → Exit 0. Das Verhalten ist unverändert: der `mode_t`-Wert geht
  unverändert an das zurücksetzende `libc::umask`.

  Beim ersten Lauf des reparierten Test-Targets wurden zwei
  `secure_dir_tests` auf macOS rot: `tempfile` legt unter `TMPDIR` ab, und der
  Standardwert `/var/folders/...` führt über den Symlink `/var` →
  `/private/var`, den `create_dir_all_nofollow` bauartbedingt ablehnt — eine
  Umgebungsannahme im Test (DoD §10), die erst sichtbar wurde, weil das
  Test-Target vorher nicht kompilierte. Beide Tests kanonisieren jetzt ihren
  Wurzelpfad (`tmp.path().canonicalize()`), ohne eine Assertion zu ändern;
  `cargo test -p lumina-cache` läuft damit auch **ohne** gesetztes `TMPDIR`
  grün (11 passed / 0 failed). Die frühere Formulierung, der Befund sei
  „vorbestehend, nicht Teil dieses Fixes", ist damit **an dieser Stelle
  zurückgenommen**: sie hätte einen Gate-Fehler als bekannt abgehakt und einen
  künftigen Leser weiter auf `--exclude lumina-cache` geschickt.

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

**Stand 2026-10-16; am 2026-10-26 an dieser Stelle überholt und ersetzt.** Die
damaligen Sätze („keine Aussage, ob der GPU-Pfad auf Adapter-Hardware wirklich
nichtdeterministisch ist", „keine Aussage, der Test sei auf Adapter-Hardware
grün", „kein Fix vorgeschlagen") beschrieben den headless Stand. Heute gilt:

- Der Test war auf Adapter-Hardware **rot** und ist es reproduzierbar gewesen
  (drei serielle Läufe, `target/gpu-render-nondeterminism-69/2026-10-25T195812Z/`);
  nach der Behebung des Upload-Skips ist er **grün** (Abschnitt „Mechanismus
  lokalisiert").
- Weiterhin **nicht** behauptet wird: eine allgemeine Determinismusgarantie des
  GPU-Renderpfads über alle Stufen, Backends, Treiber und Prozesse. Belegt ist
  der konkrete Reproducer und der Mechanismus, der ihn erklärt.

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

**Erfüllt am 2026-10-26.** Das rote Kommando ist
`cargo test -p lumina-cli --features gpu --bin lumina-cli tests::auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move`
(Metal, Exit 101 mit 38/40 bzw. 39/40 Mitgliedern in mehreren Läufen) und
`…::adapter_runs_of_the_full_process_path_report_gpu_for_all_120_renders
-- --ignored` (dreimal Exit 101). Nach `GPU-UPLOAD-IDENTITY-92` sind beide grün:
drei serielle Läufe des Familiensatzes und je einmal der Original-Reproducer
durch den Implementierer, dazu ein unabhängiger Lauf je Kommando durch den
Verifizierer, alle Exit 0 und ohne abweichende Mitglieder
(`target/gpu-render-nondeterminism-69/20261025T225434Z/`). Die
Adapter-Gesamtsuite meldete **122 passed / 0 failed / 0 ignored**. Die Abnahme
ist damit über den **Reproducer** geführt, nicht über eine allgemeine
Determinismusgarantie.
- Verbindliches Kommando: `cargo test -p lumina-gpu --features gpu-adapter-tests`
  plus die betroffenen GUI-GPU-Tests (Regel `GPU-E2E`).
