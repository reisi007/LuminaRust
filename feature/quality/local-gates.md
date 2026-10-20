# Lokale Gate-Stufen (fast / slow)

**Feature-IDs:** LOCAL-GATE-TIERS-81
**Status:** umgesetzt (Runner `scripts/gates.sh`, Tier-Tabelle in §3, Floor in
§4, Lückenliste in §5); die CI fährt die schnelle Stufe über denselben
Runner (§6).
**Herkunft:** Skill `ratchet-gates` (agents-skills `70d49e0`), Range
`787670a..00dbd22`.
**Verwandt:** `Agents.md` §„Verifizierung und Tests", `DoD.md`,
`.github/workflows/ci.yml`, `feature/quality/golden-references.md` (lokales
macOS-Gate), `feature/quality/performance-benchmarks.md` (Bench-Budgets).

---

## Inhaltsverzeichnis

- [1. Zweck](#1-zweck)
- [2. Die zwei Stufen](#2-die-zwei-stufen)
  - [2.1 `fast` — Sekunden, ohne Codegen](#21-fast--sekunden-ohne-codegen)
  - [2.2 `slow` — Minuten, CI-Mirror](#22-slow--minuten-ci-mirror)
  - [2.3 `list` und `all`](#23-list-und-all)
- [3. Tier-Tabelle](#3-tier-tabelle)
- [4. Floor-Regel der schnellen Stufe](#4-floor-regel-der-schnellen-stufe)
- [5. Lückenliste](#5-lückenliste)
  - [5.1 Was `slow` zusätzlich zu `fast` abdeckt](#51-was-slow-zusaetzlich-zu-fast-abdeckt)
  - [5.2 Was keine Stufe abdeckt](#52-was-keine-stufe-abdeckt)
  - [5.3 Was der CI-Mirror nicht abbildet](#53-was-der-ci-mirror-nicht-abbildet)
- [6. CI-Spiegelung der schnellen Stufe](#6-ci-spiegelung-der-schnellen-stufe)
- [7. Bekannte Grenzen](#7-bekannte-grenzen)

---

## 1. Zweck

Ein Gate, das niemand benennen, starten und reproduzieren kann, ist keine
Schranke, sondern eine Behauptung. LuminaRust hatte seine Gates nur als
CI-Schritte in `.github/workflows/ci.yml`: lokal gab es keinen benannten
Runner, keine Stufen-Definition, keinen Floor und keine Lückenliste. Wer
lokal arbeiten wollte, musste die Schritte aus der Workflow-Datei
herauslesen — und die Frage „was deckt die CI ab und was nicht?" hatte keine
Antwort im Repo.

Dieses Dokument und der Runner `scripts/gates.sh` liefern diese Antwort:

1. **zwei benannte Stufen** — `fast` (Sekunden) und `slow` (Minuten), beide
   lokal startbar, beide mit fester Kommando-Menge;
2. **ein Floor für die schnelle Stufe** — sie meldet nur „grün", wenn
   tatsächlich mindestens fünf Checks gelaufen sind (§4);
3. **eine Lückenliste** — jede Defektklasse, die keine Stufe abdeckt, steht
   mit Grund, Owner und Task-ID hier (§5).

Die Tier-Tabelle in §3 ist **wortgleich** mit der Ausgabe von
`sh scripts/gates.sh list`. Der Runner ist die Quelle der Wahrheit; diese
Tabelle ist eine Kopie, kein zweiter Bestand.

## 2. Die zwei Stufen

### 2.1 `fast` — Sekunden, ohne Codegen

```sh
sh scripts/gates.sh fast
```

- **Budget:** Sekunden (gemessen: unter zehn Sekunden auf diesem Host).
- **Bedarf:** keine Infrastruktur — kein Server, keine Datenbank, kein
  Netz, kein Build-Artefakt, kein Adapter. Nur `cargo fmt` (rustfmt, **kein**
  Compile) und vier POSIX-Shell-Gates.
- **Deckt ab:** Formatierung des gesamten Workspace; die vier Textschranken
  (Dateigrößen-Ratchet, Plan-Format, Env-Gate-Begründungen,
  Produktions-Dateisystemfreiheit von `lumina-core`).
- **Deckt nicht ab:** alles, was kompiliert, linkt, läuft oder einen Adapter
  braucht. `fast` ruft **kein** `cargo build/check/clippy/test/bench` auf; der
  Runner prüft das vor dem Lauf und bricht ab, falls die Definition
  kippt.
- **Läuft:** überall, wo `cargo fmt` und `/bin/sh` vorhanden sind — im
  Arbeitsverzeichnis, in der CI, auf einer frischen Maschine.

### 2.2 `slow` — Minuten, CI-Mirror

```sh
sh scripts/gates.sh slow
```

- **Budget:** Minuten.
- **Bedarf:** Build-Artefakte (der Workspace wird gebaut) und die
  Systemabhängigkeiten des CI-Containers (LibRaw 0.22.2, `liblensfun`,
  `libssl-dev`/`clang` für `onnx-rt`, Node.js für den Perf-Vergleich). Kein
  GPU-Adapter: die CI hat keinen, also spiegelt die Stufe keinen.
- **Deckt ab:** genau die Kommandos der Rust-Jobs der CI (§3, Spalte
  „Kommando") — Compile-Gates, Clippy, Feature-Gates, alle Test-Shards und
  die zweite Workspace `agent-harness/`. Der Bench-Aufruf (`slow-21`,
  `slow-22`) wird **gemessen, aber nicht erzwungen** (siehe §5.2).
- **Deckt nicht ab:** die GPU-Pfade mit echten Adapter, den onnx-rt-**Link**-
  Gate und die manuellen GUI-Läufe. Diese Klassen stehen in §5.
- **Fail fast:** der erste Fehlschlag bricht die Stufe ab (das dauert
  Minuten). Ausnahme: `slow-21` und `slow-22` sind wie im CI
  `continue-on-error` (nicht blockierend) und werden als solcher gemeldet.
- **Gleichheit mit der Merge-Schranke:** die Kommandos sind eins zu eins
  aus `.github/workflows/ci.yml` übernommen — mit Job-Name,
  `working-directory` und `env:`-Notiz pro Eintrag. Der Job „Rust agent-
  harness" läuft im Unterverzeichnis `agent-harness/` (zweites Cargo-
  Workspace, eigener Lockfile); alle anderen Jobs laufen in der
  Repo-Wurzel. Kein Schritt der CI setzt `RUSTFLAGS="-D warnings"` — die
  strenge Lint-Schranke ist der Clippy-Schritt selbst (`-- -D warnings`).

### 2.3 `list` und `all`

```sh
sh scripts/gates.sh list   # Tier-Tabelle (§3), eine Quelle der Wahrheit
sh scripts/gates.sh all    # fast, danach slow
sh scripts/gates.sh help   # Usage
```

`list` ist deterministisch (feste Reihenfolge, keine Zeitstempel, keine
absoluten Pfade) und damit als Quelle für die Tier-Tabelle in diesem
Dokument geeignet. `all` bricht ab, wenn `fast` rot ist — nach einem roten
`fast` ist die Aussage „slow ist grün" wertlos.

## 3. Tier-Tabelle

Wortgleich mit `sh scripts/gates.sh list`:

| ID | Stufe | Kommando | Arbeitsverzeichnis | Budget | Bedarf |
| --- | --- | --- | --- | --- | --- |
| fast-1 | fast | `cargo fmt --all -- --check` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-2 | fast | `sh scripts/check_file_sizes.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-3 | fast | `sh scripts/check_plan_format.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-4 | fast | `sh scripts/check_env_gate_reasons.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-5 | fast | `sh scripts/check_core_prod_fs_free.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| slow-1 | slow | `cargo check --workspace --all-targets` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-2 | slow | `cargo clippy --workspace --all-targets --features "lumina-sidecar/zdata,lumina-bench/raw-bench" -- -D warnings` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-3 | slow | `cargo clippy -p lumina-core --features lensfun -- -D warnings` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-4 | slow | `cargo clippy -p lumina-onnx --features onnx-rt -- -D warnings` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-5 | slow | `cargo check -p lumina-onnx --features onnx-rt` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-6 | slow | `cargo check -p lumina-gpu --features gpu` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-7 | slow | `sh scripts/check_cli_features.sh` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-8 | slow | `sh scripts/check_feature_matrix.sh` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-9 | slow | `cargo check -p lumina-bench --all-features` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-10 | slow | `cargo test -p lumina-gui --all-targets` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-11 | slow | `cargo test -p lumina-gui --lib --features janklog` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-12 | slow | `cargo test --workspace --all-targets --exclude lumina-gui --exclude lumina-sidecar --exclude lumina-bench` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-13 | slow | `cargo test -p libraw-sys` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-14 | slow | `cargo test -p lumina-sidecar --features zdata` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-15 | slow | `cargo test -p lumina-lensfun --features native -p lumina-core --features lensfun -p lumina-cli --features lensfun` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-16 | slow | `cargo test -p lumina-onnx --features onnx-rt` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-17 | slow | `cargo test -p lumina-cli --bin lumina-cli --features gpu,lensfun` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-18 | slow | `cargo fmt --check` | agent-harness | Minuten | keine Infrastruktur |
| slow-19 | slow | `cargo clippy --all-targets -- -D warnings` | agent-harness | Minuten | Build-Artefakt |
| slow-20 | slow | `cargo test` | agent-harness | Minuten | Build-Artefakt |
| slow-21 | slow | `cargo bench -p lumina-bench -- --sample-size 50 --warm-up-time 2` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-22 | slow | `node scripts/perf/compare.mjs --mode warn --report-dir perf/results` | Repo-Wurzel | Minuten | Build-Artefakt |

**Kein Eintrag braucht einen Adapter.** Der Wert „Adapter" kommt in dieser
Tabelle nicht vor, weil die CI keinen GPU-Runner hat — die Adapter-Pfade
sind die Lücke in §5.2, nicht ein Eintrag hier.

## 4. Floor-Regel der schnellen Stufe

Der Skill `ratchet-gates` benennt die klassische tote Schranke: eine
auswahlbasierte Stufe läuft in einem sauberen Checkout auf **null** Checks
und meldet grün. Das Signal „null Checks" ist von „alles in Ordnung" nicht
zu unterscheiden — wer das grüne Gate liest, hat nichts gelernt.

Der Runner zählt deshalb die **gestarteten** Checks der Stufe `fast` und
erzwingt einen Mindeststand:

- `MIN_CHECKS=5` — die Zahl der gestarteten Checks muss mindestens 5 sein.
- Darunter ist das Ergebnis ein **Fehler** (Exit 1), kein Pass. Meldung:
  `floor nicht erreicht: nur N von 5 Checks ausgefuehrt`.
- Eine **leere Tier-Liste** ist ebenfalls ein Fehler (Exit 1), kein Pass.
- Ein **fehlendes Prüfskript** zählt als **Fehlschlag** (Exit 127), nicht als
  Überspringen: ein Check, der nicht startet, ist ein toter Check.
- Der Floor ist selbst eine Schranke mit nur einer Richtung: er kann sinken,
  aber nicht ohne Kommentar steigen. Wer die Stufe erweitert, hebt `MIN_CHECKS`
  im selben Commit an.

Der Floor ist die einzige Zahl, die der Runner selbst erzwingt; alle
anderen Werte in diesem Dokument sind gemessen oder zitiert.

## 5. Lückenliste

Die Gates sagen, was erzwungen wird; diese Liste sagt, was **nicht**
erzwungen wird. Eine Gate-Liste ohne Lückenliste liest sich wie
Vollabdeckung. Jede Lücke nennt Klasse, Grund, Owner und Task-ID — eine
Lücke ohne Owner ist ein Friedhofseintrag.

### 5.1 Was `slow` zusätzlich zu `fast` abdeckt

Diese Klassen sind **nur** von `slow` (bzw. von der CI, die dieselben
Kommandos fährt) gedeckt. Sie sind keine Lücken, sondern der Grund, warum
die Stufe `slow` existiert:

| Defektklasse | Deckt ab | Kommando |
| --- | --- | --- |
| Compile-Bruch nach einer Änderung | Workspace-Kompilierbarkeit | `slow-1` |
| Lint-Bruch (`-D warnings`) | Clippy, inkl. Feature-Varianten | `slow-2` bis `slow-4` |
| Feature-Kombination, die nicht baut | onnx-rt-, GPU- und CLI-Feature-Gates | `slow-5` bis `slow-8` |
| Feature-Isolation (Feature baut nur mit anderem) | CLI- und Feature-Matrix-Gate | `slow-7`, `slow-8` |
| Laufzeit-Regression im Workspace | Test-Shards | `slow-10` bis `slow-17` |
| Bit-Rot in der zweiten Workspace `agent-harness/` | fmt/clippy/test dort | `slow-18` bis `slow-20` |

### 5.2 Was keine Stufe abdeckt

„Deckt ab" heißt hier **erzwungen**: eine Klasse, die nur *gemessen*, aber
nicht mit einem Fehlschlag belegt wird, hat einen Bericht, aber keine
Schranke — sie steht deshalb ebenfalls hier.

| Defektklasse | Grund (warum nicht gedeckt) | Owner | Task-ID |
| --- | --- | --- | --- |
| **GPU-/wgpu-Adapter-Pfade (Metal/Vulkan)**: CPU↔GPU-Byte-Parität, VRAM-Pixel, Adapter-Fehlerkanäle, kittest-Goldens gegen echten Adapter | Braucht einen **echten wgpu-Adapter**. Weder `fast` noch `slow` haben einen: die CI-Runner haben keinen GPU-Zugriff, der lokale Host ist headless. `slow-6` kompiliert den GPU-Pfad nur (`cargo check -p lumina-gpu --features gpu`); die 28 adapter-abhängigen Tests bleiben `#[ignore]`d. Ein GPU-Thema gilt ohne lokalen Lauf der relevanten E2E-Tests als **nicht getestet**, nicht als grün. | Eigentümer (Hardware-Schritt) | `GPU-PARITY-HW-28`, `GPU-RENDER-NONDETERMINISM-69-HW`, `UI-VISION-REVIEW-70` |
| **onnx-rt-**Link**-Gate** (glibc 2.36 < 2.38) | Ein **Host-Gate**: `cargo test -p lumina-onnx --features onnx-rt` linkt die ONNX-Runtime-Prebuilt-Binaries und braucht glibc ≥ 2.38; dieser Host hat glibc 2.36 (Debian 12), der Testlauf scheitert am Link. `cargo check` linkt nicht, deshalb treffen die Compile-Gates (`slow-4`, `slow-5`, `slow-8`) diesen Gate nicht — er wird nur beim Testlauf (`slow-16`) sichtbar. | Build-Agent (Container-/Host-Pflege) | `LOCAL-GATE-TIERS-81` (Host-Gate, im CI-Container grün) |
| **macOS-Re-Pin der sechs R-Goldens** (`library_compare`, `library_loupe`, `library_rated_badges`, `library_subfolder_badges`, `library_survey`, `library_stack_membership`) | Die Goldens sind ein **lokales macOS/Metal-Gate** auf der gepinnten Referenzplattform (`scripts/golden_ref.lock`); in der CI werden sie nie verifiziert (kein GPU-Runner). Nach dem Downscale-Umbau müssen die sechs Library-Render-Invarianten auf dem gepinnten Host neu aufgenommen werden — das geht nur auf der Referenzmaschine. | Eigentümer (Referenzmaschine) | `GOLDEN-REPIN-R-85` (Ticket; Kontext: Feature-ID `DOWNSCALER-UNIFY-71` (Golden-Folge), `GOLDEN-REF-30` (Referenzplattform)) |
| **Manuelle GUI-Läufe** (Sichtprüfung, DnD, Persistenz im echten Fenster) | Braucht einen **Menschen** an einem Fenster mit genau einer App-Instanz, `RUST_LOG=trace` und Log-Redirect (Verfahren R5-LOG-1). Der headless Weg (`egui_kittest`-Goldens, PSNR/Histogram) ersetzt den manuellen Lauf nicht; die Kopfliste in `Agents.md` nennt die Fälle. | Eigentümer (menschlicher Lauf) | `F-103-N6`, `F-103-N6-GUI-COVERAGE-27` |
| **Performance-Regression gegen das committete Baseline-/Budget-Store** | **In keiner Stufe und in keiner CI erzwungen.** `scripts/perf/compare.mjs --mode warn` gibt immer Exit 0 (der Modus `warn` bricht nie ab), die beiden Bench-Schritte der CI sind `continue-on-error: true`, und `cargo bench` selbst läuft ohne Schwellenwert. Der erzwungene Modus (`--mode gate`, Exit 1 bei Verletzung) wird von **keiner** Stufe gefahren; die Stufe `slow` führt `slow-21`/`slow-22` aus, meldet einen Fehlschlag, macht die Stufe aber nicht rot. **Ein manueller lokaler Hard-Gate existiert** für den Preview-Cache-Store: `scripts/perf/bench-preview-cache.sh --mode gate` („hard gate (local only)" im Skriptkopf) ruft `node scripts/perf/compare.mjs --mode gate --baseline scripts/perf/preview-cache-baseline.json --budgets scripts/perf/preview-cache-budgets.json` auf und liefert bei Verletzung **Exit 1** — er ist aber an **keine** Stufe und **keinen** CI-Job gebunden. | Build-Agent | `GATE-DRIFT-UND-LUECKEN-84` (Ticket; Kontext: Entscheidung `CI-Runner-Rauschen-Regel` — die CI erzwingt die Budgets bewusst nicht, belegt in `.github/workflows/ci.yml` (Job „Benchmarks (optional)") und `feature/quality/performance-benchmarks.md`; `F-074` ist die Feature-ID der Methodik) |
| **Testdatei, die kein Compilation-Target liest** (toter Test) | `scripts/check_test_orphans.sh` (Gate-ID `TEST-ORPHAN-97`) braucht **Build-Artefakte** (die dep-info eines Default-Feature-Builds mit Testtarget) und laeuft in **keinem** CI-Job und in **keiner** Stufe — das Skript ist lokal nur manuell zu fahren (`sh scripts/check_test_orphans.sh [crate]`); ohne passende Artefakte endet es mit Exit 2 („cannot conclude"), also ohne Ergebnis. Die Klasse wird derzeit von keiner Stufe erzwungen. | Build-Agent | `GATE-DRIFT-UND-LUECKEN-84` (Ticket; GATE-ID des Skripts: `TEST-ORPHAN-97`, `scripts/check_test_orphans.sh:2` — die Gate-ID, keine offene Task) |

### 5.3 Was der CI-Mirror nicht abbildet

| Was | Grund | Owner | Task-ID |
| --- | --- | --- | --- |
| **`slow` läuft in diesem Repo lokal nicht** (RAM/Platte dieses Hosts) | Die Stufe braucht den gebauten Workspace inklusive LibRaw, Lensfun und ONNX-Runtime; dieser Host hat dafür weder RAM noch die Systemabhängigkeiten (der `libraw-sys`-Build scheitert an fehlendem `libraw_r`). Der Beleg für die Übereinstimmung mit der CI ist deshalb **statisch**: der Kommando-Diff in §6 bzw. die Herkunft jedes Eintrags aus `ci.yml` — nicht ein Lauf. | Build-Agent | `LOCAL-GATE-TIERS-81` |
| **CI-Job „Documentation checks"** (Präsenz der 13 Planungsdokumente, `actionlint`, ImageMagik-Installation, Golden-Wächter-Shell-Suite `sh scripts/golden_ref_test.sh`, Executable-Prüfung des Pre-Commit-Hooks) | Der Job ist **kein** Rust-Gate: er braucht ImageMagik und `actionlint` und prüft Doku, nicht Code. Er ist im CI grün, aber in keiner lokalen Stufe abgebildet — die Wächter-Suite läuft lokal separat (`sh scripts/golden_ref_test.sh`, braucht ImageMagik). | Build-Agent | `LOCAL-GATE-TIERS-81` |
| **Feature-Matrix-Baseline** (`scripts/file_size_baseline.txt`) | Die Datei wird **handgepflegt**, nicht generiert: eine Zahl in einer Datei, die das Repo selbst pflegt, altert still (dieselbe Klasse wie die Prosa-Zahlen in `Agents.todo.md`). Ein Generator + Drift-Gate ist offen. | Build-Agent | `DOC-ZAHLEN-DRIFT-82` |

## 6. CI-Spiegelung der schnellen Stufe

Die CI führt die fünf schnellen Checks **über denselben Runner** wie lokal:
der Job „Rust fast checks" in `.github/workflows/ci.yml` hat einen Schritt
„Run fast tier (scripts/gates.sh fast)". Definition und Ausführung sind
damit dieselbe — ein Check, der lokal existiert und in der CI fehlt (oder
umgekehrt), kann nicht mehr unbemerkt auseinanderlaufen.

**Schritt vorher (fünf Einzelschritte im Job „Rust fast checks"):**
„Check formatting", „Check file-size ratchet", „Check plan format",
„Check env-gate reasons", „Check core production fs-free".

**Schritt nachher (ein Schritt):**
„Run fast tier (scripts/gates.sh fast)" → `sh scripts/gates.sh fast`.

Der abgedeckte Kommando-Satz ist damit **identisch**: der Runner führt genau
`fast-1`…`fast-5` aus, also die Kommandos der Tier-Tabelle in §3 — dieselbe
Quelle, aus der der Runner seine Liste liest (`sh scripts/gates.sh list`).
Die Kommando-Liste steht damit **nur** an einer Stelle; §6 führt sie nicht
noch einmal auf. Die fünf Checks laufen dabei **gebündelt am Job-Anfang**
(vorher lief „Check formatting" zuerst, die vier Textgates zuletzt); alle
fünf sind **artefaktfrei** (kein Build, kein Adapter, kein Netz), und ihre
Reihenfolge **innerhalb** der Stufe ist unverändert.

Die übrigen Jobs der CI („Rust GUI tests", „Rust tests (excluding GUI)",
„Rust agent-harness", „Benchmarks (optional)", „Documentation checks") sind
unberührt; ihre Kommandos sind die Stufe `slow` (§3).

Die Herkunft jedes `slow`-Eintrags — **Job-Name** und `working-directory` —
steht in der Tier-Definition von `scripts/gates.sh`. Bewusst **ohne
Zeilennummer**: eine handgepflegte Kopie der `ci.yml`-Struktur wäre die
zweite Quelle der Wahrheit, die der Skill `ratchet-gates` vermeiden will; der
Job-Name driftet nicht, die Zeilennummer driftet (in einem einzigen
Kommentar-Change schon um drei Zeilen). Das Folgeticket
`GATE-DRIFT-UND-LUECKEN-84` erzeugt die Tier-Liste später aus `ci.yml`
(generate + diff); dann ist die Herkunft automatisch exakt. Die Tabelle in §3
zeigt nur die Spalten, die die Doku braucht; die Zuordnung steht damit an
genau einer Stelle.

## 7. Bekannte Grenzen

- Der Runner prüft **nicht**, ob `slow` dem CI-Zustand entspricht: die
  Übereinstimmung ist statisch belegt (Kommando-Herkunft aus `ci.yml`), nicht
  durch einen Lauf. Eine Änderung in `ci.yml` ohne Änderung in
  `scripts/gates.sh` macht beide Listen still auseinanderlaufen — die
  Drift-Prüfung dafür ist offen (`DOC-ZAHLEN-DRIFT-82` bearbeitet die
  Zahlen-Drift, nicht diese Kommando-Drift).
- Der Floor zählt **gestartete** Checks, nicht bestandene: ein Check, der
  fehlschlägt, zählt mit — und macht die Stufe rot, **sofern er blockierend
  ist**. Nicht-blockierende Schritte (die beiden Bench-Einträge `slow-21`,
  `slow-22`, in der CI `continue-on-error`) zählen mit, werden mit
  Exit-Code und Hinweis gemeldet, machen die Stufe aber **nicht** rot. Der
  Floor zählt auch nicht *wie viele* Prüfungen ein Check intern durchführt —
  die Zahl 5 ist die Zahl der Checks, nicht die Zahl der Einzelprüfungen.
- Die Budgets in der Tier-Tabelle („Sekunden", „Minuten") sind Größenordnungen
  der Stufen, keine Messwerte aus einer Zeitmessung; der Runner misst die
  Sekunden je Check selbst und zeigt sie in der Zusammenfassung.
- Der Runner kennt nur zwei Stufen. Checks, die weder in `fast` noch in
  `slow` passen (etwa weil sie einen Adapter brauchen), gehören in §5.2 —
  nicht in eine dritte, unsichtbare Stufe.
