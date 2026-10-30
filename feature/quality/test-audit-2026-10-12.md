# Test-Audit — Runde 2026-10-12 (TEST-AUDIT-36)

Befundbericht der wöchentlichen Redundanz-Runde. **Nichts wurde gelöscht, nichts
geändert** — dieser Bericht ist die Abnahmeleistung; jede Löschung läuft über
SOLL → Implementierungs-Agent → unabhängige Verifikation (Aufgabenstellung in
`Agents.todo.md`, Eintrag `TEST-AUDIT-36`).

**Baum:** Sessionsbeginn `4e7b124`, während der Runde vom Parallel-Agenten auf
`6234f08` weitergeschoben (JSON-FLOAT-ROUNDTRIP r2). Die fünf dabei uncommittet
in Arbeit befindlichen Dateien wurden **nicht** berührt; alle
Mutationsziele der Runde sind restauriert und diff-leer gegen HEAD.

**Anker-Stand:** die verbleibenden Zeilenangaben dieses Berichts sind **Teil des
Befunddatums** vom 2026-10-12 und beziehen sich auf den Baum `6234f08` (siehe
„Baum“ oben); sie sind ueber `git show 6234f08:<pfad> | sed -n '<zeile>p'`
rekonstruierbar. Alle uebrigen Fundstellen stehen als **Pfad + Symbol- bzw.
Testname**.

## Gemessene Basis

| Messung | Ergebnis |
|---|---|
| `cargo test -p lumina-gui --all-targets` (single-threaded) | **1073** listbare Tests in 21 GUI-Zielbinaries, **996** ausgeführt, **77** `#[ignore]` |
| GUI-lib-Ziel | **955** listbar (950 passed / 5 ignored) in **54,59 s** |
| Gesamtlauf GUI | **93,4 s** Wanduhr |
| `cargo test --workspace --all-targets --no-run` + `--list` je Binary | **119** aktuelle Testbinaries, **64** libtest-Binaries mit **1609** listbaren Tests |
| `kittest_snapshots` | **56** Tests, auf diesem Host **alle 56** `#[ignore]`, 0,00 s Laufzeit |

## (a) Konstante auf sich selbst / tautologische Assertions

Kein wörtlicher Selbstvergleich im Baum (regex über alle extrahierten Testrümpfe:
`assert_eq!(X, X)`, `assert_eq!(X, X.clone())`,
`assert_eq!(Default::default(), Default::default())` → 0 Treffer). Zwei
Teilbefunde:

1. **`person_ids_are_stable_and_name_derived`** —
   `crates/lumina-gui/src/face_gui.rs:568`:
   `assert_eq!(face_person_id_for("Alex"), face_person_id_for("Alex"))` — für
   eine reine Funktion nicht failbar. Die **Aussage entfällt nicht** (der Test
   trägt sie in den Zeilen 569–576: Stabilität gegen „Sam", Präfix,
   Whitespace-Normalisierung, Ordnungs-Unabhängigkeit). Nur diese eine Zeile ist
   tot. Priorität: niedrig.
2. **`fixture_helpers_are_deterministic`** —
   `crates/lumina-cull/tests/common/mod.rs:118–133`: sechsmal `f(x) == f(x)` mit
   **identischen Literalen** auf beiden Seiten; `splitmix64(&mut 1)` reseedet pro
   Aufruf. Die Assertion kann nur Nichtdeterminismus sehen — keine
   Logikänderung. **Gemessen** (Mutation „checkerboard dark↔light vertauscht"):
   deterministisch falsch, gesamte `lumina-cull`-Suite (45 Tests, 4 Binaries)
   bleibt grün. Priorität: niedrig; Löschung erspart nichts Laufzeit (0,06 s).

## (b) Doppelte Abdeckung derselben Eigenschaft (Eigenschaftsebene)

Die von 2026-09-28 ausdrücklich offene semantische Ebene.

### B1 — GEMESSEN, Priorität 1

`downscale_never_upsizes_and_is_deterministic`
(`crates/lumina-mcp/tests/mcp.rs` → `downscale_never_upsizes_and_is_deterministic`) dupliziert
`downscale_bilinear_never_upsizes_and_is_deterministic`
(`crates/lumina-core/src/tests/image_io.rs:8`). Beide rufen **dieselbe
Produktionsfunktion** `lumina_core::downscale_bilinear` (re-exportiert in
`crates/lumina-mcp/src/util.rs:520`).

**Messung:** Mutation „No-Op-Downscale perturbiert jedes Pixel um +1" →
core-Test **FAILED**, mcp-Test **ok (grün)**. Die mcp-Assertionsmenge ist echte
Teilmenge (ohne No-Op-Pixelidentität, ohne 1×1-Mindestfall). **Entfallende
Aussage: keine** — Streichen des mcp-Exemplars verliert keine Eigenschaft; es
hat nachweislich kein eigenes Fehlsignal.

### B2 — GEMESSEN (2026-10-12), Priorität 1: fünf Tests pinnen dieselbe Aussage

`LocalAdjustments::set_value` muss die deaktivierten Stufen-Keys abweisen
(`crates/lumina-sidecar/src/local_adjustments.rs` → `LocalAdjustments::set_value`,
Tabelle `LOCAL_ADJUSTMENT_RANGES` in `crates/lumina-sidecar/src/lib.rs`).
Gepinnt von:

- `crates/lumina-sidecar/src/tests/local_adjustments_color_digest.rs` →
  `disabled_local_presence_detail_denoise_and_optics_stay_rejected`
- `crates/lumina-core/src/render/tests/local_color_contract_tests.rs` →
  `disabled_local_stages_stay_unreachable`
- `crates/lumina-core/src/render/tests/local_presence_contract_tests.rs` →
  `disabled_local_stages_stay_unreachable`

**Anker-Update 2026-10-20:** die beiden weiteren sidecar-Exemplare sind
**überholt** — sie wurden in das konsolidierte Exemplar
`crates/lumina-sidecar/src/tests/local_adjustments_color_digest.rs` →
`disabled_local_presence_detail_denoise_and_optics_stay_rejected` zusammengeführt
(siehe dessen Doc-Kommentar). Die beiden alten Anker
(`crates/lumina-sidecar/src/tests/local_adjustments_presence.rs` und
`crates/lumina-sidecar/src/tests/local_adjustments_detail_state.rs`) zeigten
ins Leere: die beiden Dateien sind heute 355 bzw. 81 Zeilen lang; auf dem damaligen
Stand (442 bzw. 159 Zeilen) zeigten die Anker noch auf die dort genannten Tests —
der dort genannte Testname
`disabled_local_denoise_and_optics_stay_rejected_and_the_legacy_keys_stay_p0`
existiert heute nicht mehr. Die Aussage „fünf Tests pinnen dieselbe Aussage"
galt für den Stand 2026-10-12; heute sind es **drei** Exemplare.

**Messung (2026-10-12):** Mutation „`set_value` akzeptiert `detail`" → **alle
fünf rot** (3 sidecar, 2 core). Redundanz damit quantifiziert: vier der fünf
Exemplare der *Abweisungs-Aussage* sind gegeneinander austauschbar;
**Rest-Aussagen pro
Exemplar**: die sidecar-presence-Variante pinned zusätzlich den
Legacy-`adjustment_*`-Pfad (`validate_mask_layer_local_state`) — der hat
**keinen** Ersatz; die Key-Listen (6/7/9/10 Keys) und serialisierten Feldsätze
sind je Datei minimal verschieden. Streichvorschlag darf nur auf **einen**
Vertreter pro Codepad kollabieren, nie auf null.

### B3 — statisch, Priorität 2: fünf `previous_refuses_*`-Tests, ein Pad

- `crates/lumina-cli/tests/mask_local_e2e.rs` → `previous_refuses_recipe_only_local_mask_transfer_on_source_or_target` (Original, P0-Skalare)
- `crates/lumina-cli/tests/mask_local_detail_previous_e2e.rs` → `previous_refuses_a_detail_only_local_state_on_source_and_target`
- `crates/lumina-cli/tests/mask_local_presence_e2e.rs` → `previous_refuses_a_presence_only_local_state_on_source_and_target`
- `crates/lumina-cli/tests/mask_local_curve_e2e.rs` → `previous_refuses_a_curve_only_local_state_on_source_and_target`
- `crates/lumina-cli/tests/mask_local_color_history_e2e.rs` → `previous_refuses_a_color_only_local_state_on_source_and_target`

Alle fünf asserten, dass `previous` einen rezept-lokalen Transfer abweist
(„refused recipe-only transfer", Ziel-Bytes unberührt); sie unterscheiden sich
**nur im einen gesetzten Stufen-Key**. Rest-Aussage der vier Abkömmlinge: das
`set_local`-Parsing eben dieser Stufe — separat abgedeckt in den jeweiligen
`mask_local_*_e2e`-Dateien. **Nicht gemessen** (CLI-e2e-Lauf wäre nötig;
benanntes Gate).

### B4 — statisch, Priorität 2

`legacy_versions_migrate_losslessly_and_refuse_a_smuggled_detail_block`
(`crates/lumina-sidecar/src/tests/local_adjustments_detail.rs` →
`legacy_versions_migrate_losslessly_and_refuse_a_smuggled_detail_block`) vs.
`…_presence_block` (`local_adjustments_presence.rs:92`) — identische
Migrationsschleife + Refusal, unterschiedlich ist nur der eingeschleuste Block.

### B5 — statisch, Priorität 3, Nur-Abgrenzung

`brush_zero_width`/`brush_zero_height_resolution_rejects_invalid_sidecar`
(`crates/lumina-sidecar/src/tests/mask_prompts.rs` →
`brush_zero_width_resolution_rejects_invalid_sidecar` und
`crates/lumina-sidecar/src/tests/mask_prompts.rs` →
`brush_zero_height_resolution_rejects_invalid_sidecar`) teilen den
Produktionszweig `crates/lumina-sidecar/src/lib.rs` → `validate_prompt`
(`resolution.0 == 0 || resolution.1 == 0`) —
jeder pinned ein Disjunkt; das ist Condition-Coverage, **kein**
Streichkandidat.

### B6/B7 — statisch, Priorität 3

`effects_validation_rejects_invalid_values`
(`crates/lumina-core/src/tests/vignette_grain.rs` →
`effects_validation_rejects_invalid_values` via `apply_recipe` vs.
`crates/lumina-sidecar/src/tests/stage_serde_b.rs` →
`effects_validation_rejects_invalid_values` via
`SidecarDocument::validate`) — gleiche Unwertig-Matrix, zwei Oberflächen;
`initialize_*` (`crates/lumina-mcp/tests/mcp.rs` →
`initialize_reports_capabilities_and_protocol` vs.
`crates/lumina-mcp/tests/meta_mcp.rs` →
`initialize_declares_resources_capability_without_prompts`) —
Überlapp nur protocolVersion+tools, je eindeutige Hälften.

**Abgrenzung zum 2026-09-28-Befund:** `g15_stacks.rs`/`crates/lumina-gui/src/tests/library_sort.rs`
(1.202 Zeilen geteilte Helfer) und Testinfrastruktur-Duplikate werden
**nicht** erneut gemeldet — sie gehören zu `GUITEST-STRUCT-34`.

## (c) Implementierungsentscheidung statt Verhalten

**Keine Befunde.** Geprüft und leer: Debug-Format-Pins (`format!("{:?}")` in
Assertions), Zeiger-/Adressidentität, `size_of`-Pins (nur
`crates/lumina-raw/tests/abi_layout.rs` →
`abi_constants_match_rust_struct_layout` — legitim, das ist der ABI-Vertrag),
Call-Order-Vergleiche. Error-Text-Pins (z. B. `crates/lumina-stages/src/tests.rs` →
`resolve_copy_selects_the_default_and_rejects_an_unknown_id_loudly`)
pinnen CLI-sichtbare Meldungen = Vertrag. MCP-Registry-Tests
(`library_tools.rs:48`) pinnen Listen↔Dispatch-Konsistenz mit dokumentierter
Begründung.

## (d) Tests, die bei Wegfall der Logik nichts verlieren

**Naht-Scan über den ganzen Workspace: 0 bestätigte Befunde.** Produktions-
funktionen, die nur aus Testcode referenziert werden: 2 Rohtreffer, beide falsch
positiv — `run_merge` (`crates/lumina-merge/src/bundle.rs` → `run_merge`, Aliase-Import
`run_merge_bundle` in `crates/lumina-cli/src/merge.rs` → `run_merge_bundle`,
Produktionsaufruf ebendort und in `crates/lumina-gui/src/merge_gui.rs` →
`run_merge_sync`) und `classify_required`
(`crates/lumina-testskip/src/lib.rs` → `classify_required`, wird von
`crates/lumina-onnx/tests/face_real_weights.rs` →
`real_weights_match_the_adapter_contract` gerufen — testskip ist per Definition
Testinfrastruktur). Der LENSFUN-DB-33-Defekt (Naht ohne Produktionsaufruf)
existiert im aktuellen Baum nicht.

## (e) Tests ohne Fehlsignal — gemessen

| Messung | Mutation | Ergebnis |
|---|---|---|
| **M-B** | `lumina_core::downscale_bilinear`, No-Op-Pfad +1 pro Pixel | core-Test `image_io.rs:8` **rot**; mcp-Test `mcp.rs:163` **grün** → mcp-Exemplar hat kein eigenes Fehlsignal (siehe B1) |
| **M-A** | `LocalAdjustments::set_value` akzeptiert `detail` | 5 Tests rot (3 sidecar, 2 core) → die fünf **haben** Fehlsignal; ihre Redundanz ist quantifiziert (B2) |
| **M-C** | `checkerboard` dark↔light | `lumina-cull` 45/45 **grün** — Determinismus-Test und alle Consumer können Polaritätsfehler nicht sehen |
| **M-C2** | `checkerboard` → einfarbig | rot: `crates/lumina-cull/tests/fixtures.rs` → `heuristic_precision_and_recall_on_synthetic_fixtures` — die Struktur der Generatoren **ist** verankert |

Zusätzlich benannt (nicht gemessen, `#[ignore]`d, kein Default-Gate):
**`cpu_gpu_path_parity_matrix`** —
`crates/lumina-gui/tests/kittest_parity.rs:288–292`:
`let _ = run_cpu_gpu_path_parity_matrix(&mut harness);` verwirft das Ergebnis;
der Test würde einen roten Matrix-Run als grün melden. Die Vertrags-Hälfte
(lauter SKIP) trägt das Geschwister `parity_matrix_skips_loudly_without_adapter`
(`crates/lumina-gui/tests/kittest_parity.rs` →
`parity_matrix_skips_loudly_without_adapter`).

## (f) Laufzeit-Ballast — diesmal GEMESSEN (2026-09-28 offen)

1. **`kittest_snapshots` = 56 Tests, auf diesem Host 56× `#[ignore]`, 0,00 s.**
   Der CI-GUI-Shard (`cargo test -p lumina-gui --all-targets`) **kompiliert**
   dieses Ziel in jedem Lauf, **führt 0 seiner Tests aus**; **kein Workflow**
   ruft es mit `--ignored` auf (einziger `--ignored`-Treffer im CI:
   `matrix-nightly.yml:111` `real_matrix_headless`, ein lib-Test). Die
   46,41/51,92/245,66 s aus `DoD.md` §9 stammen von GPU-Host-Läufen, nicht vom
   CI-Gate. **Aussage im Gate: null; Kosten im Gate: Kompilat + Listung.**
2. Langsamste kleine GUI-Ziele (gemessen, single-threaded):
   `mask_local_editors_wiring` 3 Tests/7,84 s; `mask_local_color_controls`
   1 Test/7,52 s; `mask_local_editors` 3/6,89 s; `slider_sidecar_save`
   4/5,41 s; `mask_local_hsl_bands` 2/3,78 s. GUI-lib: 955 Tests/54,59 s
   (≈57 ms/Test).
3. **Methodische Falle (selbst gemessen):** `--list` liefert für die
   Custom-Harness-Binaries (egui_kittest) **0 Tests** — ein `--list`-basiertes
   Zählen unterschlägt die komplette kittest-Schicht. Nur ein echter Lauf zählt
   sie.

## (g) Assertionen, die nur noch den Refactor beschreiben

**Keine Befunde.** Die Nahtstellen-Kandidaten asserten Verhalten:
`generative_link_status_delegates_to_artifact_status`
(`crates/lumina-sidecar/src/tests/generative_bundle.rs` →
`generative_link_status_delegates_to_artifact_status`) prüft echte
`ArtifactStatus`-Zustände auf Platte; MCP-„registered in both paths"-Tests
prüfen Listen↔Dispatch-Verhalten.

## Lücken (neue offene Aufgaben — kein Nachbar wurde gestrichen)

- **G1 (prioritär):** Die GUI-Golden-Suite (56 `kittest_snapshots` + ignorierte
  kittest-Ziele, insgesamt 77 ignorierte GUI-Tests) hat **kein
  CI-Ausführgate** — gemessen: komplett `#[ignore]` auf GPU-losen Runnern, kein
  Workflow läuft `--ignored` dafür. Nach `DoD.md` §12 und der GPU-Policy
  (`AGENTS.md`) ist das „nicht getestet", nicht „grün". Aufgabe: explizites Gate
  (GPU-Runner-Job oder dokumentierter manueller Lauf mit Protokoll) — **nicht**
  Streichen der Tests.
- **G2:** Die Polarität der cull-Fixture-Generatoren ist unverankert (M-C:
  invertiert → alles grün). Wenn `checkerboard`/`gray_frame` semantisch kippen,
  merkt es niemand; nur die Struktur ist durch `crates/lumina-cull/tests/fixtures.rs` →
  `heuristic_precision_and_recall_on_synthetic_fixtures` gepinnt.
  Aufgabe: eine Polarity-Anker-Assertion (oder Fixture-Kommentar, der sagt:
  Polarität egal).
- **G3:** `cpu_gpu_path_parity_matrix` (siehe (e)) ist der einzige
  Voll-Matrix-Lauf und verwirft sein Ergebnis; auf GPU-Hosts läuft er nur
  manuell. Aufgabe: Ergebnis asserten oder Lauf als Gate protokollieren.

## Methodik — welche Zahl gilt für welche Menge

| Zahl | Menge | Quelle |
|---|---|---|
| 119 / 64 / 1609 | aktuelle Testbinaries / davon libtest / listbare Tests | `cargo test --workspace --all-targets --no-run` (2026-10-12) + `--list` je Binary |
| 1073 / 996 / 77 | GUI-Ziele: listbar / ausgeführt / ignoriert | echter Lauf `cargo test -p lumina-gui --all-targets` |
| 955 (950+5) | GUI-lib-Unit | `--lib -- --list` + Lauf |
| 2972 / 108 | `#[test]` / test-tragende Ziele, **statische Untergrenze** | eigenes Mod-Baum-Resolver-Skript; verifiziert gegen dep-info (`target/debug/deps/*.d`) |
| 3116 / 445 | globaler Dateisystem-Walk | **nur zur Abgrenzung** — die Methode, an der `TEST-DEDUP-100` scheiterte: zählt feature-ausgeschaltete Module (z. B. 7 Tests in `crates/lumina-gui/src/jank_log.rs`, nur hinter `feature="janklog"`; CI deckt sie über den eigenen Schritt `cargo test -p lumina-gui --lib --features janklog`, Kommentar `ci.yml:287`), Test-Support-Dateien ohne Ziel und `#[cfg]`-tote Attribute, und ordnet keinen Test seiner ausführenden Binary zu |

**Bekannte Untergrenze der statischen Zählung:** GUI-lib statisch 936 vs. 955
gelistet (10 per `#[path]`/verschachtelt eingebundene Dateien fehlen);
`lumina-core`-lib statisch 442 vs. 560 gelistet — letzteres ist
**Feature-Unification** unter `--workspace --all-targets`
(`lensfun`-Tests). Jede Zahl oben trägt ihr Kommando; eine Zahl ohne Kommando
ist eine Behauptung.

**Trennung gemessen / nicht ausgeführt / benanntes Gate:** (e) M-A, M-B, M-C/M-C2
und (f) komplett **gemessen** (je eine Einzellauf-Messung, kein Streuungsniveau —
`DoD.md` §9); B3/B4 **statisch** mit benanntem Gate (CLI-e2e-Lauf); G1–G3
**benannte Gates**, keine Läufe.
