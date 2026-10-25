# Lokale Gate-Stufen (fast / slow / headless / gpu / full)

**Feature-IDs:** LOCAL-GATE-TIERS-81, GATE-DRIFT-UND-LUECKEN-84,
DOC-ZAHLEN-DRIFT-82
**Status:** umgesetzt (Runner `scripts/gates.sh`, Tier-Tabelle in §3, Floor in
§4, Lückenliste in §5); die CI fährt die schnelle Stufe und die parallel
aufgeteilten headless-Gruppen über denselben Runner (§6) und prüft die
Spiegelung der Tier-Liste gegen `ci.yml` (§7) sowie
die Zahlen der Prosa gegen ihren Generator (§8) und den Generator selbst
gegen Determinismus und Git-Freiheit (§8.5).
**Herkunft:** Skill `ratchet-gates` (agents-skills `70d49e0`), Range
`787670a..00dbd22`.
**Verwandt:** `Agents.md` §„Verifizierung und Tests", `DoD.md`,
`.github/workflows/ci.yml`, `feature/quality/golden-references.md` (lokales
macOS-Gate), `feature/quality/performance-benchmarks.md` (Bench-Budgets).

---

## Inhaltsverzeichnis

- [1. Zweck](#1-zweck)
- [2. Die Stufen](#2-die-stufen)
  - [2.1 `fast` — Sekunden, ohne Codegen](#21-fast--sekunden-ohne-codegen)
  - [2.2 `slow` — Minuten, CI-Mirror](#22-slow--minuten-ci-mirror)
  - [2.3 `list`, `list-ci` und `all`](#23-list-list-ci-und-all)
  - [2.4 `headless`, `gpu` und `full`](#24-headless-gpu-und-full)
- [3. Tier-Tabelle](#3-tier-tabelle)
- [4. Floor-Regel der schnellen Stufe](#4-floor-regel-der-schnellen-stufe)
- [5. Lückenliste](#5-lückenliste)
  - [5.1 Was `slow` zusätzlich zu `fast` abdeckt](#51-was-slow-zusaetzlich-zu-fast-abdeckt)
  - [5.2 Was keine Stufe abdeckt](#52-was-keine-stufe-abdeckt)
  - [5.3 Was der CI-Mirror nicht abbildet](#53-was-der-ci-mirror-nicht-abbildet)
- [6. CI-Spiegelung der schnellen Stufe](#6-ci-spiegelung-der-schnellen-stufe)
- [7. Drift-Gate: die Tier-Liste gegen `ci.yml` (generate + diff)](#7-drift-gate-die-tier-liste-gegen-ciyml-generate--diff)
  - [7.1 Was er abdeckt](#71-was-er-abdeckt)
  - [7.2 Wie er läuft](#72-wie-er-läuft)
  - [7.3 Was er meldet](#73-was-er-meldet)
  - [7.4 Grenzen](#74-grenzen)
- [8. Drift-Gate: die Zahlen in der Prosa (generate + diff)](#8-drift-gate-die-zahlen-in-der-prosa-generate--diff)
  - [8.1 Bausteine](#81-bausteine)
  - [8.2 Die Blöcke](#82-die-bloecke)
  - [8.3 Wie er läuft](#83-wie-er-laeuft)
  - [8.4 Grenzen](#84-grenzen)
  - [8.5 Der Hygiene-Gate des Generators (`fast-9`)](#85-der-hygiene-gate-des-generators-fast-9)
- [9. Bekannte Grenzen](#9-bekannte-grenzen)
- [10. Headless- und Hardware-Stufen](#10-headless-und-hardware-stufen)

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

1. **benannte Stufen** — `fast`, `slow`, `headless`, `gpu` und `full`, mit
   festen Kommando-Mengen und klar getrennter Hardwareabhängigkeit;
2. **ein Floor für die schnelle Stufe** — sie meldet nur „grün", wenn
   tatsächlich mindestens neun Checks gelaufen sind (§4);
3. **eine Lückenliste** — jede Defektklasse, die keine Stufe abdeckt, steht
   mit Grund, Owner und Task-ID hier (§5);
4. **einen Drift-Gate gegen die CI** — die Tier-Liste wird aus `ci.yml`
   **generiert** und die generierte Liste gegen die committete **diffs**
   (§7). Eine Änderung in der CI kann den Spiegel nicht mehr still
   verlassen: der Gate meldet, er schreibt die Liste nicht um.

Die Tier-Tabelle in §3 ist **wortgleich** mit der Ausgabe von
`sh scripts/gates.sh list`. Der Runner ist die Quelle der Wahrheit; diese
Tabelle ist eine Kopie, kein zweiter Bestand.

## 2. Die Stufen

### 2.1 `fast` — Sekunden, ohne Codegen

```sh
sh scripts/gates.sh fast
```

- **Budget:** Sekunden — **unter fünfzehn Sekunden** auf diesem Host (gemessen 2026-10-20: **11 s** für die neun Checks; die Angabe war „unter zehn Sekunden“, bis die Welle 2 den neunten Check und das Hygiene-Gate `fast-9` hinzufügte ✓).
- **Bedarf:** kein Server, keine Datenbank, kein Netz und kein Adapter.
  `ratchet` v0.5.0 muss lokal installiert sein (CI installiert die gepinnte
  Version vor `gates.sh fast`); zusätzlich laufen `cargo fmt` (rustfmt,
  **kein** Compile) und acht POSIX-Shell-Gates.
- **Deckt ab:** Formatierung des gesamten Workspace; die vier Textschranken
  (Dateigrößen-Ratchet, Plan-Format, Env-Gate-Begründungen,
  Produktions-Dateisystemfreiheit von `lumina-core`) und den **Drift-Gate**
  (§7), der die Tier-Liste gegen die aus `ci.yml` generierte Liste prüft; neu
  dazu die **Zeilen-Obergrenze 1000** (`fast-7`,
  `scripts/check_file_ceiling.sh`) — die harte Grenze je Datei plus
  Ratchet-Richtung, deren SOLL-Zielsemantik in
  [`file-size-policy.md`](file-size-policy.md) steht — der **Drift-Gate
  der Prosa-Zahlen** (`fast-8`, `scripts/check_doc_facts_drift.sh`), der die
  generierten Fakten-Blöcke in den Dokumenten gegen ihren Generator prüft
  (§8), und das **Hygiene-Gate des Generators** (`fast-9`,
  `scripts/check_doc_facts_generator.sh`), das prüft, dass der Generator
  vollständig, deterministisch und **git-frei** ist — derselbe Lauf ohne
  `git` im PATH muss byteweise dieselbe Ausgabe liefern (§8.5).
- **Deckt nicht ab:** alles, was kompiliert, linkt, läuft oder einen Adapter
  braucht. `fast` ruft **kein** `cargo build/check/clippy/test/bench` auf; der
  Runner prüft das vor dem Lauf und bricht ab, falls die Definition
  kippt.
- **Läuft:** überall, wo `cargo fmt` und `/bin/sh` vorhanden sind — im
  Arbeitsverzeichnis, in der CI, auf einer frischen Maschine. Der Drift-Gate
  braucht zusätzlich nur die Workflow-Datei (Text, kein YAML-Parser, kein
  `python3`, kein `node`).

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
- **Reihenfolge-Abhängigkeit:** `headless-gui-2` (der Orphan-Test-Gate) liest die
  dep-info, die `headless-gui-1` erzeugt — er steht deshalb **unmittelbar danach**
  in der Liste. Ohne den vorherigen Test-Build endet er mit Exit 2
  („cannot conclude"), also mit einem Ergebnis ohne Aussage.
- **Gleichheit mit der Merge-Schranke:** die Kommandos sind eins zu eins
  aus `.github/workflows/ci.yml` übernommen — mit Job-Name,
  `working-directory` und `env:`-Notiz pro Eintrag. Der Job „Rust agent-
  harness" läuft im Unterverzeichnis `agent-harness/` (zweites Cargo-
  Workspace, eigener Lockfile); alle anderen Jobs laufen in der
  Repo-Wurzel. Kein Schritt der CI setzt `RUSTFLAGS="-D warnings"` — die
  strenge Lint-Schranke ist der Clippy-Schritt selbst (`-- -D warnings`).
  Diese Gleichheit ist **nicht** nur behauptet, sondern geprüft: §7
  beschreibt den Drift-Gate, der sie bei jedem `fast`-Lauf (also auch in
  der CI) gegen die generierte Liste prüft.

### 2.3 `list`, `list-ci` und `all`

```sh
sh scripts/gates.sh list     # Tier-Tabelle (§3), eine Quelle der Wahrheit
sh scripts/gates.sh list-ci  # ableitbare CI-Sicht (blocking|wd|kommando|job)
sh scripts/gates.sh all      # fast, danach slow
sh scripts/gates.sh help     # Usage
```

`list` ist deterministisch (feste Reihenfolge, keine Zeitstempel, keine
absoluten Pfade) und damit als Quelle für die Tier-Tabelle in diesem
Dokument geeignet. `list-ci` liefert dieselben Felder, die der Generator
aus `ci.yml` zieht — sie sind die Vergleichsgrundlage des Drift-Gates (§7).
`all` bricht ab, wenn `fast` rot ist — nach einem roten
`fast` ist die Aussage „slow ist grün" wertlos.

### 2.4 `headless`, `gpu` und `full`

```sh
sh scripts/gates.sh headless compile
sh scripts/gates.sh headless gui
sh scripts/gates.sh headless rest
sh scripts/gates.sh headless all
sh scripts/gates.sh gpu
sh scripts/gates.sh full
```

`headless` enthält die GPU-Feature-Kompilierung und alle adapterfreien Test-
Shards. Die drei benannten Gruppen entsprechen den parallel laufenden
Rust-CI-Jobs; `headless all` führt sie lokal nacheinander aus. `gpu` aktiviert
die adapterabhängigen `lumina-gpu` Integrationstests und muss auf einem Rechner
mit unterstütztem wgpu-Adapter laufen. `full` ist genau `headless all` gefolgt
von `gpu`; die GPU-Stufe ist absichtlich nicht Teil der CI.

## 3. Tier-Tabelle

Wortgleich mit `sh scripts/gates.sh list`:

| ID | Stufe | Kommando | Arbeitsverzeichnis | Budget | Bedarf |
| --- | --- | --- | --- | --- | --- |
| fast-1 | fast | `cargo fmt --all -- --check && sh scripts/check_included_rs_fmt.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-2 | fast | `sh scripts/check_file_sizes.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-3 | fast | `sh scripts/check_plan_format.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-4 | fast | `sh scripts/check_env_gate_reasons.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-5 | fast | `sh scripts/check_core_prod_fs_free.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-6 | fast | `sh scripts/check_ci_tier_drift.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-7 | fast | `sh scripts/check_file_ceiling.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-8 | fast | `sh scripts/check_doc_facts_drift.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| fast-9 | fast | `sh scripts/check_doc_facts_generator.sh` | Repo-Wurzel | Sekunden | keine Infrastruktur |
| slow-23 | slow | `sh scripts/install_ratchet.sh` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-1 | slow | `cargo check --workspace --all-targets` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-2 | slow | `cargo clippy --workspace --all-targets --features "lumina-sidecar/zdata,lumina-bench/raw-bench" -- -D warnings` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-3 | slow | `cargo clippy -p lumina-core --features lensfun -- -D warnings` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-4 | slow | `cargo clippy -p lumina-onnx --features onnx-rt -- -D warnings` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-5 | slow | `cargo check -p lumina-onnx --features onnx-rt` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-headless-compile | slow | `sh scripts/gates.sh headless compile` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-9 | slow | `cargo check -p lumina-bench --all-features` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-headless-gui | slow | `sh scripts/gates.sh headless gui` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-headless-rest | slow | `sh scripts/gates.sh headless rest` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-18 | slow | `cargo fmt --check` | agent-harness | Minuten | keine Infrastruktur |
| slow-19 | slow | `cargo clippy --all-targets -- -D warnings` | agent-harness | Minuten | Build-Artefakt |
| slow-20 | slow | `cargo test` | agent-harness | Minuten | Build-Artefakt |
| slow-21 | slow | `cargo bench -p lumina-bench -- --sample-size 50 --warm-up-time 2` | Repo-Wurzel | Minuten | Build-Artefakt |
| slow-22 | slow | `node scripts/perf/compare.mjs --mode warn --report-dir perf/results` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-compile-1 | headless | `cargo check -p lumina-gpu --features gpu` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-compile-2 | headless | `sh scripts/check_cli_features.sh` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-compile-3 | headless | `sh scripts/check_feature_matrix.sh` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-gui-1 | headless | `cargo test -p lumina-gui --all-targets` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-gui-2 | headless | `sh scripts/check_test_orphans.sh lumina-gui` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-gui-3 | headless | `cargo test -p lumina-gui --lib --features janklog` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-rest-1 | headless | `cargo test --workspace --all-targets --exclude lumina-gui --exclude lumina-sidecar --exclude lumina-bench` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-rest-2 | headless | `cargo test -p libraw-sys` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-rest-3 | headless | `cargo test -p lumina-sidecar --features zdata` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-rest-4 | headless | `cargo test -p lumina-lensfun --features native -p lumina-core --features lensfun -p lumina-cli --features lensfun` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-rest-5 | headless | `cargo test -p lumina-onnx --features onnx-rt` | Repo-Wurzel | Minuten | Build-Artefakt |
| headless-rest-6 | headless | `cargo test -p lumina-cli --bin lumina-cli --features gpu,lensfun` | Repo-Wurzel | Minuten | Build-Artefakt |
| gpu-1 | gpu | `cargo test -p lumina-gpu --features gpu-adapter-tests` | Repo-Wurzel | Minuten | GPU-Adapter |

Nur `gpu-1` braucht einen Adapter; es ist ausdrücklich kein CI-Eintrag. Alle
`headless`-Zeilen sind ohne Hardwareadapter ausführbar.

## 4. Floor-Regel der schnellen Stufe

Der Skill `ratchet-gates` benennt die klassische tote Schranke: eine
auswahlbasierte Stufe läuft in einem sauberen Checkout auf **null** Checks
und meldet grün. Das Signal „null Checks" ist von „alles in Ordnung" nicht
zu unterscheiden — wer das grüne Gate liest, hat nichts gelernt.

Der Runner zählt deshalb die **gestarteten** Checks der Stufe `fast` und
erzwingt einen Mindeststand:

- `MIN_CHECKS=9` — die Zahl der gestarteten Checks muss mindestens 9 sein.
- Darunter ist das Ergebnis ein **Fehler** (Exit 1), kein Pass. Meldung:
  `floor nicht erreicht: nur N von 9 Checks ausgefuehrt`.
- Eine **leere Tier-Liste** ist ebenfalls ein Fehler (Exit 1), kein Pass.
- Ein **fehlendes Prüfskript** zählt als **Fehlschlag** (Exit 127), nicht als
  Überspringen: ein Check, der nicht startet, ist ein toter Check.
- Der Floor ist selbst eine Schranke mit nur einer Richtung: er kann sinken,
  aber nicht ohne Kommentar steigen. Wer die Stufe erweitert, hebt `MIN_CHECKS`
  im selben Commit an. (Erhöht von 5 auf 6, als `fast-6` — der Drift-Gate —
  hinzukam, und von 6 auf 7, als `fast-7` — die Zeilen-Obergrenze 1000 —
  hinzukam, und von 7 auf 8, als `fast-8` — der Drift-Gate der Prosa-Zahlen —
  hinzukam, und von 8 auf 9, als `fast-9` — das Hygiene-Gate des Generators —
  hinzukam; ein Floor, der unter der Zahl der Checks liegt, würde das
  Entfernen eines Checks still werden lassen.)



### 4.1 Ratchet-Floor der Regel `rust-file-line-ceiling`

Unabhaengig von `MIN_CHECKS` hat die Ratchet-Konfiguration einen eigenen
`tiers.fast.floor` (`ratchet.config.json`). Er zaehlt die **gestarteten
Checks der Regel** (ein Aggregat-Check plus je Ausnahme-Datei ein Check) und
verhinderte bisher, dass eine geschrumpfte Ausnahmeliste still weniger
Pruefungen ausfuehrt. Mit dem Umbau `FILE-SIZE-1000-ALL-EXCEPTIONS-100` sinkt
diese Zahl planmaessig bis auf **1** (nur noch der Aggregat-Check, sobald die
Ausnahmeliste leer ist); der Floor steht deshalb seit 2026-10-24 auf 1.
Die Eigenschaft, die der Floor schuetzen sollte, ist damit nicht aufgegeben:
`scripts/check_ratchet_exceptions.py` laeuft unmittelbar davor und erzwingt
binar, dass **jede** Datei ueber 1 000 Zeilen einen exakten Ausnahme-Eintrag
hat, dass kein Eintrag veraltet ist und dass Konfiguration und Baseline
dieselben Scopes tragen — eine fehlende oder ueberzaehlige Pruefung ist dort
ein Fehler, kein stiller Pass.

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
| Feature-Kombination, die nicht baut | onnx-rt-, GPU- und CLI-Feature-Gates | `slow-5`, `headless-compile` |
| Feature-Isolation (Feature baut nur mit anderem) | CLI- und Feature-Matrix-Gate | `headless-compile` |
| Laufzeit-Regression im Workspace | Test-Shards | `headless-gui`, `headless-rest` |
| Bit-Rot in der zweiten Workspace `agent-harness/` | fmt/clippy/test dort | `slow-18` bis `slow-20` |
| **Testdatei, die kein Compilation-Target liest** (toter Test) | dep-info des Test-Builds: jede `*.rs` unter `crates/lumina-gui/src/tests/` muss im dep-info mindestens einer Test-Kompilationseinheit stehen | `headless-gui-2` (CI: Headless-GUI-Gruppe im Job „Rust GUI tests") |

**Vorbedingung für `headless-gui-2`:** der Gate liest die dep-info einer
Kompilationseinheit, die das **Test-Target** (also mit `cfg(test)`)
kompiliert hat. Nicht der *Befehl* ist die tragende Vorbedingung — auch
`cargo check --all-targets` erzeugt Test-Target-dep-info —, sondern die vier
Bedingungen, die das Skript **selbst erzwingt**; verletzt ist eine davon,
endet es mit **Exit 2** („cannot conclude"):

1. unter `target/debug/deps` liegt überhaupt dep-info für `lumina-gui`
   (`scripts/check_test_orphans.sh:76` — der Default-Suchpfad des Skripts,
   genau das Verzeichnis, in das `cargo test` seine dep-info ablegt);
2. die dep-info ist **frisch** — der Frische-Filter verwirft jede dep-info,
   die älter ist als die Quellen, aus denen sie gebaut wurde;
3. die dep-info nennt mindestens **eine** Datei unter
   `crates/lumina-gui/src/tests/` — sonst stammt sie aus einer Einheit **ohne**
   `cfg(test)` (z. B. `cargo check -p lumina-gui --lib`) und würde jede Datei
   als Waise melden;
4. die dep-info nennt den **Sentinel**
   `crates/lumina-gui/src/tests/lensfun_diagnostics.rs` — der ist
   feature-gegatet (`#[cfg(feature = "lensfun")] mod lensfun_diagnostics;`,
   `crates/lumina-gui/src/lib.rs` -> `mod lensfun_diagnostics;`, im Test-Modul-Block ab
   `crates/lumina-gui/src/lib.rs` -> `mod tests`), und nur ein Default-Feature-
   Build enthält ihn (`default = ["lensfun", "gpu"]`,
   `crates/lumina-gui/Cargo.toml:74`).

Deshalb steht `headless-gui-2` unmittelbar nach `headless-gui-1`
(`cargo test -p lumina-gui --all-targets`, CI-Job „Rust GUI tests"): dort
liegt die dep-info eines **frischen** Test-Builds vor, der alle vier
Bedingungen erfüllt. **Scope:** `lumina-gui`. Die fünf weiteren Crates mit
`src/tests/` (`lumina-cli`, `lumina-core`, `lumina-gpu`, `lumina-lensfun`,
`lumina-sidecar`) bleiben lokal manuell
(`sh scripts/check_test_orphans.sh <crate>`), weil der Gate für sie keinen
Default-Feature-Sentinel kennt (genannte Grenze im Skriptkopf).

### 5.2 Was keine Stufe abdeckt

„Deckt ab" heißt hier **erzwungen**: eine Klasse, die nur *gemessen*, aber
nicht mit einem Fehlschlag belegt wird, hat einen Bericht, aber keine
Schranke — sie steht deshalb ebenfalls hier.

Eine **geschlossene** Klasse bleibt als Zeile stehen und ist als solche
markiert (mit dem Beleg, der sie schließt). Sie wird nicht stillschweigend
entfernt: eine Lücke, die verschwindet, ohne dass jemand ihren Abschluss
benennt, ist eine Lücke, die wiederkehrt. Eine Klasse, die **erzwungen**
 wird, wandert nach §5.1 — die Klasse „Testdatei, die kein Compilation-Target
liest" ist seit `GATE-DRIFT-UND-LUECKEN-84` erzwungen (`headless-gui-2`, plus CI-
Schritt „Check for orphaned test files (TEST-ORPHAN-97)" im Job „Rust GUI
tests") und steht deshalb in §5.1.

| Defektklasse | Grund (warum nicht gedeckt) | Owner | Task-ID |
| --- | --- | --- | --- |
| **GPU-/wgpu-Adapter-Pfade (Metal/Vulkan)**: CPU↔GPU-Byte-Parität, VRAM-Pixel, Adapter-Fehlerkanäle, kittest-Goldens gegen echten Adapter | Braucht einen **echten wgpu-Adapter**. `fast` und die CI-Headless-Gruppen benötigen keinen Adapter. `headless-compile-1` kompiliert den GPU-Pfad (`cargo check -p lumina-gpu --features gpu`); `gpu-1` aktiviert die adapterabhängigen Tests, die ohne Opt-in ignoriert bleiben. Ein GPU-Thema gilt ohne Lauf der relevanten Tests auf passender Hardware als **nicht getestet**, nicht als grün. | Eigentümer (Hardware-Schritt) | `GPU-PARITY-HW-28`, `GPU-RENDER-NONDETERMINISM-69-HW`, `UI-VISION-REVIEW-70` |
| **onnx-rt-**Link**-Gate** (glibc 2.36 < 2.38) | Ein **Host-Gate**: `cargo test -p lumina-onnx --features onnx-rt` linkt die ONNX-Runtime-Prebuilt-Binaries und braucht glibc ≥ 2.38; dieser Host hat glibc 2.36 (Debian 12), der Testlauf scheitert am Link. `cargo check` linkt nicht, deshalb treffen die Compile-Gates (`slow-4`, `slow-5`, `headless-compile-3`) diesen Gate nicht — er wird nur beim Testlauf (`headless-rest-5`) sichtbar. | Build-Agent (Container-/Host-Pflege) | `LOCAL-GATE-TIERS-81` (Host-Gate, im CI-Container grün) |
| **macOS-Re-Pin der sechs R-Goldens** (`library_compare`, `library_loupe`, `library_rated_badges`, `library_subfolder_badges`, `library_survey`, `library_stack_membership`) | Die Goldens sind ein **lokales macOS/Metal-Gate** auf der gepinnten Referenzplattform (`scripts/golden_ref.lock`); in der CI werden sie nie verifiziert (kein GPU-Runner). Nach dem Downscale-Umbau müssen die sechs Library-Render-Invarianten auf dem gepinnten Host neu aufgenommen werden — das geht nur auf der Referenzmaschine. | Eigentümer (Referenzmaschine) | `GOLDEN-REPIN-R-85` (Ticket; Kontext: Feature-ID `DOWNSCALER-UNIFY-71` (Golden-Folge), `GOLDEN-REF-30` (Referenzplattform)) |
| **Manuelle GUI-Läufe** (Sichtprüfung, DnD, Persistenz im echten Fenster) | Braucht einen **Menschen** an einem Fenster mit genau einer App-Instanz, `RUST_LOG=trace` und Log-Redirect (Verfahren R5-LOG-1). Der headless Weg (`egui_kittest`-Goldens, PSNR/Histogram) ersetzt den manuellen Lauf nicht; die Kopfliste in `Agents.md` nennt die Fälle. | Eigentümer (menschlicher Lauf) | `F-103-N6`, `F-103-N6-GUI-COVERAGE-27` |
| **Performance-Regression gegen das committete Baseline-/Budget-Store** | **GESCHLOSSEN durch begründete Entscheidung (GATE-DRIFT-UND-LUECKEN-84): in der CI bewusst NICHT erzwogen.** Beleg 1 — `feature/quality/performance-benchmarks.md:421-422`: „Das harte Gate läuft **bewusst nicht in CI** (CI-Runner-Rauschen-Regel); CI nutzt nur report/warn." Beleg 2 — `.github/workflows/ci.yml:498-502` (Job „Benchmarks (optional)"): „CI uses compare.mjs --mode warn ONLY (never the hard gate), so noisy runners can never fail a merge. Budgets carry gate:true for a controlled local gate on the capture machine, but CI deliberately does not enforce it (CI-Runner-Rauschen-Regel). Bench steps are continue-on-error so the job never breaks the main merge checks." **Warum nicht in der CI:** die Budgets werden auf geteilten, fremden Runnern gemessen; ein dort erzwungenes Budget wäre eine Schein-Schranke — rote Builds ohne echten Defekt, getarnt als Regression. **Wo stattdessen erzwungen wird:** lokal und hart — `scripts/perf/bench-preview-cache.sh --mode gate` („hard gate (local only)", Skriptkopf `scripts/perf/bench-preview-cache.sh:13`) ruft `scripts/perf/compare.mjs` im Modus `gate` auf (Aufruf mit `--baseline`, `--budgets`, `--criterion-dir` und `--report-dir`: `scripts/perf/bench-preview-cache.sh:25-30`) und liefert bei Verletzung **Exit 1**. **Was die CI tut:** nur messen — `scripts/perf/compare.mjs --mode warn` gibt immer Exit 0 (Modus `warn` bricht nie ab), die beiden Bench-Schritte sind `continue-on-error: true`, und die Stufe `slow` führt `slow-21`/`slow-22` **nicht blockierend** mit (Fehlschlag wird gemeldet, die Stufe wird nicht rot). **Bleibende, benannte Lücke:** der erzwungene Modus ist an **keine** Stufe gebunden — er bleibt ein manueller lokaler Gate; die Klasse ist in der CI also weiterhin *gemessen, aber nicht erzwungen*. | Build-Agent | `GATE-DRIFT-UND-LUECKEN-84` (geschlossen: begründete Entscheidung, CI-Runner-Rauschen-Regel; Kontext: `F-074` = Feature-ID der Methodik, `feature/quality/performance-benchmarks.md`) |

### 5.3 Was der CI-Mirror nicht abbildet

| Was | Grund | Owner | Task-ID |
| --- | --- | --- | --- |
| **`slow` läuft in diesem Repo lokal nicht** (RAM/Platte dieses Hosts) | Die Stufe braucht den gebauten Workspace inklusive LibRaw, Lensfun und ONNX-Runtime; dieser Host hat dafür weder RAM noch die Systemabhängigkeiten (der `libraw-sys`-Build scheitert an fehlendem `libraw_r`). Der Beleg für die Übereinstimmung mit der CI ist deshalb **statisch**: der Drift-Gate (§7) vergleicht die Tier-Liste gegen die aus `ci.yml` generierte Liste (Textvergleich, kein Lauf der Kommandos), die Herkunft jedes Eintrags steht im Job-Namen. | Build-Agent | `LOCAL-GATE-TIERS-81` |
| **CI-Job „Documentation checks"** (Präsenz der 13 Planungsdokumente, `actionlint`, ImageMagik-Installation, Golden-Wächter-Shell-Suite `sh scripts/golden_ref_test.sh`, Executable-Prüfung des Pre-Commit-Hooks) | Der Job ist **kein** Rust-Gate: er braucht ImageMagik und `actionlint` und prüft Doku, nicht Code. Er ist im CI grün, aber in keiner lokalen Stufe abgebildet — die Wächter-Suite läuft lokal separat (`sh scripts/golden_ref_test.sh`, braucht ImageMagik). | Build-Agent | `LOCAL-GATE-TIERS-81` |
| **Feature-Matrix-Baseline** (`scripts/file_size_baseline.txt`) | Die Datei wird **handgepflegt**, nicht generiert: eine Zahl in einer Datei, die das Repo selbst pflegt, altert still (dieselbe Klasse wie die Prosa-Zahlen in `Agents.todo.md`). Ein Generator + Drift-Gate ist offen. | Build-Agent | `DOC-ZAHLEN-DRIFT-82` |

## 6. CI-Spiegelung der schnellen Stufe

Die CI führt die neun schnellen Checks **über denselben Runner** wie lokal:
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
`fast-1`…`fast-9` aus, also die Kommandos der Tier-Tabelle in §3 — dieselbe
Quelle, aus der der Runner seine Liste liest (`sh scripts/gates.sh list`).
Die Kommando-Liste steht damit **nur** an einer Stelle; §6 führt sie nicht
noch einmal auf. Die neun Checks laufen dabei **gebündelt am Job-Anfang**
(vorher lief „Check formatting" zuerst, die vier Textgates zuletzt); alle
neun sind **artefaktfrei** (kein Build, kein Adapter, kein Netz), und ihre
Reihenfolge **innerhalb** der Stufe ist unverändert.

Die übrigen Jobs der CI („Rust GUI tests", „Rust tests (excluding GUI)",
„Rust agent-harness", „Benchmarks (optional)", „Documentation checks") sind
unberührt; ihre Kommandos sind die Stufe `slow` (§3). **Eine Ausnahme gibt
es seit `GATE-DRIFT-UND-LUECKEN-84`:** der Job „Rust GUI tests" hat einen
zusätzlichen Schritt „Check for orphaned test files (TEST-ORPHAN-97)" direkt
nach „Run GUI tests". Er hängt dort, weil genau dieser Schritt die dep-info
erzeugt, die der Gate liest (Vorbedingung in §5.1) — er steht deshalb
**unmittelbar nach dem Test-Schritt**, damit die dep-info des Test-Builds
**frisch** vorliegt und der Gate zu einem Ergebnis kommt (statt
„cannot conclude“). Er ist
**blockierend** (kein `continue-on-error`): läuft er mit **Exit 2** („cannot
conclude"), wird der Job rot — und das ist **gewollt**, denn ein stilles
„cannot conclude" würde die Schranke wirkungslos machen; die rote CI ist hier
das Signal, dass die dep-info fehlt oder veraltet ist. Er ist als `headless-gui-2`
in der Tier-Liste gespiegelt und wird vom Drift-Gate (§7) mitgeprüft.

Die Herkunft jedes `slow`-Eintrags — **Job-Name** und `working-directory` —
steht in der Tier-Definition von `scripts/gates.sh`. Bewusst **ohne
Zeilennummer**: eine handgepflegte Kopie der `ci.yml`-Struktur wäre die
zweite Quelle der Wahrheit, die der Skill `ratchet-gates` vermeiden will; der
Job-Name driftet nicht, die Zeilennummer driftet (in einem einzigen
Kommentar-Change schon um drei Zeilen). Seit `GATE-DRIFT-UND-LUECKEN-84` ist
das nicht mehr nur behauptet: der Drift-Gate (§7) erzeugt die Liste aus
`ci.yml` und diffst sie gegen die committete Tier-Liste — die Herkunft ist
damit automatisch exakt. Die Tabelle in §3 zeigt nur die Spalten, die die
Doku braucht; die Zuordnung steht damit an genau einer Stelle.

## 7. Drift-Gate: die Tier-Liste gegen `ci.yml` (generate + diff)

Die Stufe `slow` ist ein **Spiegel** der Rust-Jobs in
`.github/workflows/ci.yml`. Bis `LOCAL-GATE-TIERS-81` wurde dieser Spiegel
von Hand gepflegt — eine Änderung in der CI driftete still, und niemand
merkte es, bis ein Kommando doppelt gelaufen war oder fehlte. Der Drift-Gate
macht den Spiegel prüfbar. Er ist **Muster „generate + diff"**, nicht „wirkt
plausibel".

### 7.1 Was er abdeckt

| Baustein | Datei | Was er tut |
| --- | --- | --- |
| Generator | `scripts/gen_ci_tiers.sh` | liest `ci.yml` **als Text** und zieht die ableitbaren Felder je CI-Schritt → **Soll-Seite** |
| Committierte Seite | `sh scripts/gates.sh list-ci` | liefert dieselben Felder aus der Tier-Liste in `scripts/gates.sh` → **Ist-Seite** |
| Drift-Gate | `scripts/check_ci_tier_drift.sh` (Eintrag `fast-6`) | diffst beide Seiten und meldet jede Abweichung |

**Generiert (ableitbar) — vier Felder je CI-Schritt:**

| Feld | Quelle in `ci.yml` | Bedeutung |
| --- | --- | --- |
| `blocking` | `continue-on-error: true` am Schritt | 0 = nicht blockierend, sonst 1 |
| `wd-path` | `working-directory:` am Schritt | `.` ohne Angabe |
| `command` | `run:` am Schritt | zeichengleich |
| `job` | `name:` am Job | Job-Name (der Job-Key, falls der Job keinen Namen trägt) |

**Annotiert (handgepflegt, vom Generator **nicht** erfunden):** die `id`
(`slow-1`, `headless-gui-2`, …), die **Anzeige** des Arbeitsverzeichnisses
(„Repo-Wurzel") und der **Bedarf** („Build-Artefakt" / „keine
Infrastruktur"). Eine Ableitung, die eine Annotation raten müsste, wäre eine
zweite, schlechtere Wahrheit — deshalb bleiben sie in `scripts/gates.sh`.

### 7.2 Wie er läuft

```sh
sh scripts/gates.sh fast                 # fast-6 = der Drift-Gate
sh scripts/check_ci_tier_drift.sh        # derselbe Gate, einzeln
sh scripts/gen_ci_tiers.sh               # die generierte Soll-Seite
sh scripts/gates.sh list-ci              # die committete Seite
```

Der Generator liest **nur Text**: kein `eval`, kein `sh -c` mit extrahierten
Werten, kein `source` auf der YAML-Datei, kein `yq`, kein `python3`, kein
`node` (der CI-Container hat davon keines). **Sicherheitsregel:** der Gate
startet **nie** ein Kommando, das er aus `ci.yml` gelesen hat — ein
Eingabedokument auszuführen statt es zu lesen wäre ein
Ausführungs-Freigabegate, keine Prüfung. Die Kopie der `ci.yml`, die der
Rotbeweis braucht, wird deshalb nur **gelesen**.

**Testhaken:** die Umgebungsvariable `LUMINA_CI_YAML` überschreibt die
Workflow-Datei, die der Gate liest (Default: die Repo-Datei
`.github/workflows/ci.yml`). Sie existiert nur für den Rotbeweis — die
Kopie unter `/tmp` wird gelesen, nie ausgeführt.

Der Generator liest die Jobs **mit `container:`** (das sind die Rust-Jobs im
Container `ghcr.io/reisi007/luminarust/lumina-ci:latest`); die Jobs ohne
Container (`detect`, `docs`) sind kein Rust-Gate (§5.3). Schritte ohne `run:`
(Checkout, Toolchain, Cache, Artifact-Upload) tragen kein Kommando. Der
Schritt `sh scripts/gates.sh fast` — der Aufruf der Stufe `fast` — ist der
einzige **Anker**, der nicht gespiegelt wird; nur dieser exakte Wert wird
übersprungen, ändert er sich, fällt der Schritt in die Spiegelung zurück und
der Gate wird rot.

### 7.3 Was er meldet

Bei Abweichung nennt der Gate **welcher Job**, **welches Kommando** und
**welche Abweichung**:

```text
FEHLER: CI-Tier-Drift — die Tier-Liste in scripts/gates.sh weicht von der CI ab.
    Job "Rust fast checks": Kommando geaendert
      ci.yml     : `cargo check --workspace` (wd='.', blockierend=1)
      Tier-Liste : `cargo check --workspace --all-targets` (wd='.', blockierend=1)
```

Er meldet **nur**: die Tier-Liste wird **nicht** automatisch umgeschrieben.
Die Änderung bleibt ein sichtbares, reviewbares Ereignis — wer `ci.yml`
ändert, muss `scripts/gates.sh` mitziehen, und der Reviewer sieht beides im
selben Diff.

### 7.4 Grenzen

- Geprüft werden die **ableitbaren** Felder. Die Annotationen (`id`,
  wd-Anzeige, „Bedarf") sind handgepflegt und werden nicht verglichen.
- Die Jobs **ohne** `container:` (`detect`, `docs`) sind nicht im Scope
  (§5.3).
- Ein CI-Schritt mit **mehrzeiligem** `run:`-Block kann nicht 1:1 gespiegelt
  werden; der Generator meldet ihn als Fehler (Exit 2), der Gate wird rot.
- Der Gate sagt **nichts** darüber aus, ob ein Kommando erfolgreich wäre —
  er vergleicht Text. Ob die Kommandos der Stufe `slow` durchlaufen, ist eine
  andere Frage (§5.3: lokal auf diesem Host nicht).
- Die Vergleichsbasis ist die **committete** Tier-Liste in `scripts/gates.sh`
  — sie bleibt handgepflegt (mit Annotationen), ist aber gegen die Quelle
  geprüft.

## 8. Drift-Gate: die Zahlen in der Prosa (generate + diff)

Eine Zahl in Prosa ist eine Kopie einer fremd gepflegten Tatsache und altert
still. Die Zahlen der Prosa (Dateigrößen, Population, Zahl der committeten
Goldens, R/C-Split und Verteilung der Goldens auf die Test-Targets,
MCP-/CLI-Population, Baseline-Kostenverteilung, Zahl der Workspace-Crates, Zahl
der enumerierten Oberflächen) standen als Text im
Dokument — eine zweite Quelle der Wahrheit neben der Datei, aus der sie
stammen. `fast-8` macht sie prüfbar: Muster „generate + diff", nicht „wirkt
plausibel". `fast-9` sichert die Eigenschaft des Generators, ohne die diese
Prüfung überhaupt aussagekräftig ist: **git-frei** (§8.5).

### 8.1 Bausteine

| Baustein | Datei | Was er tut |
| --- | --- | --- |
| Generator | `scripts/gen_doc_facts.sh` | liest die Quelle (Textdatei, Dateisystem, committetes Lockfile) und liefert den Block-Text → **Soll-Seite** |
| Block-Marker | `<!-- BEGIN GENERATED: <id> -->` … `<!-- END GENERATED: <id> -->` | fassen den generierten Abschnitt in der Zieldatei ein |
| Drift-Gate | `scripts/check_doc_facts_drift.sh` (Eintrag `fast-8`) | extrahiert den Block aus der Zieldatei (Arbeitsbaum), difft ihn gegen den Generator und meldet jede Abweichung |
| Hygiene-Gate | `scripts/check_doc_facts_generator.sh` (Eintrag `fast-9`) | prüft den Generator selbst: Syntax, Vollständigkeit, Determinismus und **Git-Freiheit** (§8.5) |

Der Generator liest **nur Text** und führt **nie** etwas aus, was er aus einem
gelesenen Dokument gelesen hat (kein `eval`, kein `sh -c` mit extrahierten
Werten, kein `source` auf einer Zieldatei). Kein `python3`, kein `node` — der
CI-Container hat keines von beiden; nur sh/awk/sed/grep/wc/tr/mktemp/diff.

Der Gate **meldet, er schreibt nicht**: die Zieldatei wird nicht automatisch
umgeschrieben. Die Änderung bleibt ein sichtbares, reviewbares Ereignis —
wer eine Quelle ändert, erzeugt den Block neu und committet ihn im selben
Commit.

### 8.2 Die Blöcke

| Block | Zieldatei | Quelle der Zahlen |
| --- | --- | --- |
| `file-sizes` | `feature/quality/file-size-policy.md` | Ratchet-Konfiguration und Baseline (`ratchet.config.json`, `ratchet.baseline.json`) sowie `wc -l` (aktueller Ist-Stand); `scripts/file_size_baseline.txt` bleibt die separate 500er-Baseline |
| `population` | `feature/quality/file-size-policy.md` | das aktuelle Dateisystem (`crates/`, `agent-harness/`), ausgewertet nach dem Geltungsbereich in `ratchet.config.json`; die 1000-Zeilen-Schwelle ist der Ratchet-Vertrag, die 500er-Baseline bleibt separat |
| `goldens` | `feature/README.md`, `feature/quality/golden-references.md`, `feature/quality/golden-fixtures.md` | `scripts/golden_ref.lock`, Feld `goldens.count` (committete Baseline; **kein** `git` nötig — der CI-Container hat keins; **kein** `ls`: im Verzeichnis liegen gitignorierte `*.new.png`-Artefakte). Die Kette zum Repo hält `scripts/golden_ref_test.sh` (Docs-Job **ohne** Container): es re-derived die Goldens aus dem Git-Index und prüft das Lockfile dagegen |
| `golden-classes` | `feature/quality/golden-fixtures.md` | `scripts/golden_inventory.txt` (eine Zeile je Golden: Dateiname, Klasse `R`/`C`, Test-Target) — der **R/C-Split**, gezählt, nicht abgeschrieben. Die Klassifikation selbst ist eine menschliche Entscheidung (GOLDEN-FIXT-31); die Datei ist ihre maschinenlesbare Form |
| `golden-targets` | `feature/quality/golden-references.md` | dieselbe Datei `scripts/golden_inventory.txt` — die **Verteilung auf die Test-Targets**, je Ziel gezählt. Der Generator prüft die Datei laut: Kopfzeile, Feldzahl, keine Doppelnennung, Sortierung (LC_ALL=C), Existenz jeder genannten Datei und Übereinstimmung mit `goldens.count`; **zusätzlich** gleicht er die Klassen der Inventartabelle in `feature/quality/golden-fixtures.md` §4 gegen die Quelldatei ab — die Tabelle bleibt Prosa, ist aber keine zweite, stille Quelle |
| `mcp-tools` | `feature/platform/mcp-server.md`, `feature/quality/security-review-register.md` | die Quelltexte selbst: die `tool_def(`-Aufrufe in `crates/lumina-mcp/src/tools/mod.rs` (ohne den Konstruktor `fn tool_def`) und die Varianten des `enum Command` in `crates/lumina-cli/src/main.rs` (die Feature-Gate-Variante `Mcp` steht ohne Klammer und wird **mitgezählt**) |
| `perf-baseline` | `feature/quality/performance-benchmarks.md` | der committete Store `perf/baseline.json`: die IDs mit Präfix `core/` (Auflösungs-Suffixe und Reihenfolge ergeben sich aus dem Store), die `batch/`-ID je Auflösung und die IDs mit Präfix `decode/raw__`; der Stand kommt aus `environment.recorded_at` |
| `crates` | `feature/README.md` | die `members` im Workspace-Manifest `Cargo.toml` |
| `surfaces` | `feature/quality/security-review-register.md` | die Datenzeilen der §5-Basis-Tabelle desselben Dokuments (die Liste selbst bleibt handgepflegt; nur ihre **Zahl** wird abgeleitet) |

Die Zuordnung Block → Zieldatei steht im Manifest des Generators; der Gate
liest dasselbe Manifest — ein **fehlender erwarteter Block** ist damit ein
Fehler, kein stilles Übersehen. Fehlt die Zieldatei, fehlt ein Marker oder
sind die Marker unbalanciert, meldet der Gate das lauter und endet mit
Exit ≠ 0.

### 8.3 Wie er läuft

```sh
sh scripts/check_doc_facts_drift.sh        # der Gate (Eintrag fast-8)
sh scripts/check_doc_facts_generator.sh    # der Hygiene-Gate (Eintrag fast-9)
sh scripts/gen_doc_facts.sh                # alle Blöcke, zum Einfügen
sh scripts/gen_doc_facts.sh population     # ein Block (ohne Marker)
sh scripts/gen_doc_facts.sh --manifest     # Block → Zieldatei
```

Deterministisch: `LC_ALL=C`, feste Sortierung, keine Zeitstempel, keine
absoluten Pfade. Der Generator liefert bei zwei Läufen byteweise dieselbe
Ausgabe — ein Gate, das bei zwei Läufen unterschiedliche Ausgaben liefert, ist
tot.

### 8.4 Grenzen

- **Kein Hand-Inhalt im Generator.** Der Generator enthält keine Zahlen- oder
  Faktenliste; die Zeilenauswahl ist regelbasiert. Wer eine Auswahl nicht
  regelbasiert treffen kann, legt sie nicht an — eine generierte Datei mit
  eingebetteter Handliste wäre selbst eine zweite Quelle der Wahrheit.
- **Nicht generiert sind** die Zahlen, die keine Ableitung aus einer Quelle
  sind: die **Klassifikation** der Goldens selbst (R oder C ist eine
  menschliche Entscheidung — sie steht als Text in
  `feature/quality/golden-fixtures.md` §4 und in ihrer maschinenlesbaren Form
  in `scripts/golden_inventory.txt`; die **Zahlen** daraus sind generiert),
  die **Liste** der Oberflächen selbst (nur ihre Zahl wird abgeleitet),
  Testlaufzahlen (ohne Build nicht ableitbar — sie sind datiert oder
  gestrichen). Sie altern weiter — das ist eine offene Lücke, keine Behauptung
  dagegen.
- **Der Gate vergleicht Text**, er misst nichts nach: die erzeugten Zahlen
  sind Textableitungen, keine Laufzeitmessungen.
- **Fehlt eine Quelle** (kein Git-Arbeitsbaum, fehlende Datei), meldet der
  Generator das und der Gate wird rot — er liefert **nie** still 0.

### 8.5 Der Hygiene-Gate des Generators (`fast-9`)

`scripts/check_doc_facts_generator.sh` prüft die Eigenschaft, ohne die der
Drift-Gate `fast-8` **nichts** aussagen würde: dass der Generator in **jeder**
Umgebung dasselbe erzeugt.

- **Syntax** — `sh -n` auf dem Generator und auf dem Gate selbst.
- **Vollständigkeit** — jede Block-Id aus dem Manifest ist erzeugbar (eine Id
  ohne Implementierung wäre eine stille Lücke) und jede Zieldatei existiert.
- **Determinismus** — zwei Läufe liefern byteweise dieselbe Ausgabe.
- **Git-Freiheit** — derselbe Lauf in einer **PATH-Sandbox ohne `git*`**
  (Symlinks auf alle übrigen Werkzeuge des PATH) liefert byteweise dieselbe
  Ausgabe. Der Nachweis ist ein **Verhaltens-Nachweis**: er bräuchte kein
  Git-Binary — und genau das ist die Bedingung, weil der CI-Container
  `ghcr.io/reisi007/luminarust/lumina-ci:latest` **kein** `git` installiert
  hat (CI-Lauf 39459843221, Job „Rust fast checks", 2026-10-20). Ein Generator,
  der `git` bräuchte, wäre dort rot und lokal grün.

Der Gate startet ausschließlich `sh scripts/gen_doc_facts.sh` mit festen
Argumenten (`--manifest`, `--list`, `<block-id>`); der Inhalt einer gelesenen
Datei fliesst nie in eine Kommandozeile.

## 9. Bekannte Grenzen

## 10. Headless- und Hardware-Stufen

`fast`, `slow` und das historische `all` bleiben kompatibel. Zusätzlich
definiert `scripts/gates.sh` `headless compile|gui|rest|all`, `gpu` und `full`.
`headless compile` enthält GPU-Feature-, CLI-Feature- und Workspace-Feature-
Kompilierung; es benötigt keinen Adapter. `headless gui` und `headless rest`
führen die bestehenden adapterfreien CI-Test-Shards aus. `headless all` fasst
Compilechecks und beide Test-Shards zusammen. CI bleibt auf ihren parallelen
Jobs und verwendet ausschließlich diese adapterfreien Prüfungen.

`gpu` führt ausschließlich `cargo test -p lumina-gpu --features
gpu-adapter-tests` aus. Dieses Opt-in aktiviert die ignorierten,
adapterpflichtigen Integrationstests; `require_adapter` macht fehlende Hardware
zu einem Fehler statt einem vacuous pass. `full` ist exakt `headless all` plus
`gpu`, nicht das CI-Gate. Es erfordert einen verfügbaren wgpu-Adapter (Metal
oder Vulkan). Ohne diesen lokalen Lauf bleibt die Hardware-Abdeckung
ungeprüft.

Aufrufbeispiele: `sh scripts/gates.sh headless compile`, `... headless gui`,
`... headless rest`, `... headless all`, `... gpu`, `... full`.

- Der Runner prüft **nicht**, ob `slow` dem CI-Zustand entspricht: die
  Übereinstimmung ist statisch belegt (Kommando-Herkunft aus `ci.yml`), nicht
  durch einen Lauf. Der **Drift-Gate** (§7) schließt die Lücke der
  *stillschweigenden* Abweichung: er vergleicht die committete Tier-Liste
  gegen die aus `ci.yml` generierte Liste bei **jedem** `fast`-Lauf (also
  auch in der CI) und wird bei jeder Abweichung rot. Er prüft aber nur die
  ableitbaren Felder — ein **fehlerhaft ausgeführtes** Kommando sieht er
  nicht.
- Der Floor zählt **gestartete** Checks, nicht bestandene: ein Check, der
  fehlschlägt, zählt mit — und macht die Stufe rot, **sofern er blockierend
  ist**. Nicht-blockierende Schritte (die beiden Bench-Einträge `slow-21`,
  `slow-22`, in der CI `continue-on-error`) zählen mit, werden mit
  Exit-Code und Hinweis gemeldet, machen die Stufe aber **nicht** rot. Der
  Floor zählt auch nicht *wie viele* Prüfungen ein Check intern durchführt —
  die Zahl 9 ist die Zahl der Checks, nicht die Zahl der Einzelprüfungen.
- Die Budgets in der Tier-Tabelle („Sekunden", „Minuten") sind Größenordnungen
  der Stufen, keine Messwerte aus einer Zeitmessung; der Runner misst die
  Sekunden je Check selbst und zeigt sie in der Zusammenfassung.
- `fast` und `slow` bleiben die artefaktfreie Kurzstufe und der CI-Mirror.
  Die expliziten Laufzeitstufen `headless`, `gpu` und `full` sind separat
  dokumentiert in §10: CI nutzt die headless Gruppen, während `full` zusätzlich
  die nur auf passender Hardware ausführbaren Adaptertests startet.
- Der **Drift-Gate der Prosa-Zahlen** (§8) braucht **kein** `git`: die Zahl der committeten
  Goldens liest er aus dem committeten `scripts/golden_ref.lock` (`goldens.count`). Das ist
  noetig, weil der CI-Container `ghcr.io/reisi007/luminarust/lumina-ci:latest` **kein
  `git`-Binary** hat — am 2026-10-20 war der `goldens`-Block dort nicht erzeugbar und der
  Job „Rust fast checks“ wurde **rot**. Die Gegenprobe zur anderen Richtung — ein Golden
  **ohne** Lockfile-Update — macht `scripts/golden_ref_test.sh` im Job „Documentation checks“
  (ohne Container, mit `git`); ein Lockfile-Update **ohne** Doku-Update macht der Drift-Gate.
  Damit ist die Kette in beide Richtungen geschlossen.
- **Alle neun Blöcke sind git-frei** (§8.2) — der Generator liest
  ausschliesslich committete Dateien und das Dateisystem. Der Nachweis ist
  **nicht** das Wort im Kommentar, sondern der Hygiene-Gate `fast-9`
  (`scripts/check_doc_facts_generator.sh`): derselbe Lauf in einer
  PATH-Sandbox **ohne** `git*` muss byteweise dieselbe Ausgabe liefern. Ein
  Block, der `git` braucht, ist im CI-Container nicht erzeugbar — am
  2026-10-20 war das der Grund, warum der Job „Rust fast checks“ rot wurde.
- Das 1000er-Gate (`scripts/check_file_ceiling.sh`) prüft mit Ratchet die
  Arbeitsbaum-Messungen gegen den globalen Fallback und die exakten
  Ausnahmewerte. `scripts/check_ratchet_exceptions.py` erzwingt zusätzlich,
  dass jede aktuell über 1000 Zeilen große Datei genau eine Dateiausnahme hat
  und jede Ausnahme entfernt wird, sobald die Datei höchstens 1000 Zeilen hat.
  Die CI installiert dafür die gepinnte Ratchet-Version vor den schnellen
  Checks; eine Baseline-Anhebung ist durch Ratchets Monotonieprüfung blockiert.
