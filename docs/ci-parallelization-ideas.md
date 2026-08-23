# CI Parallelization Ideas — 4 vCPU / 16 GB / 14 GB SSD (2026-08-23)

> **Status:** Ideensammlung — keine Umsetzung. Dokumentiert auf Wunsch, vor manuellem Review F-103-N6.
> CI-Runner: `ubuntu-latest` = 4 vCPUs, 16 GB RAM, 14 GB SSD (GitHub-hosted, Stand 2026). Container-Jobs (`rust`, `bench`) laufen im gepinnten LibRaw-Image `ghcr.io/reisi007/luminarust/lumina-ci:latest`.

## 1) Warnings im „Run tests"-Schritt — Erklärung

**Der aktuelle CI-Run `32663797417` ist grün** (alle 5 Jobs `success`; `Rust checks: Run tests` = 2m11s, `WASM checks` = 26s). `cargo test --workspace --all-targets` selbst wirft **keine** Test-Warnings — Warnungen sind Compile-Warnings, die beim Bauen der Test-Binaries erscheinen und im Log stehen:

| Warn-Quelle | Ort | Schwere | Warum nicht rot |
|---|---|---|---|
| `vendor/libraw-sys` 40× `unexpected_cfgs` (`have_*`, `check-cfg` values) + `gcc` crate deprecated alias | `crates/lumina-raw/vendor/libraw-sys/build.rs` | niedrig — reines Vendor-Build-Script | `RUSTFLAGS="-D warnings"` ist im `rust`-Job bewusst **nicht** gesetzt (ci.yml:68ff Kommentar). Würde das Vendor-Script hart brechen. Strenge Lint-Hürde ist der dedizierte `cargo clippy -- -D warnings` Schritt, der nur Workspace-Crates lintet — Vendor wird dort nicht erfasst. |
| `lumina-gui` wasm 20 `dead_code` (`Str` Varianten, `viewport::prefetch_order` etc.) | `cargo check --target wasm32-unknown-unknown --no-default-features -p lumina-gui` | niedrig | WASM-Check läuft **ohne** `-D warnings` (nur `cargo check`, kein Clippy). Warnings sind erwartbar: `Str`-Enum hat viele Varianten, die auf WASM nicht genutzt werden. Kein Blocker. |
| `block v0.1.6` future-incompat (`non_exhaustive` attribute handling) | transitiv via `criterion 0.8` → `block` → `objc2` | info — externe Crate | `cargo build -p lumina-bench --benches` kompiliert mit einer Info-Warnung. Nicht änderungsbedingt, seit criterion 0.8. |
| `lumina-gpu: warn_unsupported_vram_once` dead_code im `--no-default-features` Build | `crates/lumina-gpu/src/lib.rs:242` | **behoben** in `3bb21cd` | War vorbestehend, jetzt `#[cfg(feature="gpu")]` gegated — `cargo check -p lumina-gpu --no-default-features` ist warnungsfrei. Beispiel für die neue Agents.md-Regel „keine vorbestehenden Warnungen abhaken". |

**Kein** `warning` im Sinne von Test-Flakiness oder `#[warn]`-Test-Output. `cargo test` selbst meldet nur Compile-Warnungen + Test-Ergebnisse (`81+5 gui, 94 sidecar-zdata, 7 gpu, 217+7 core-lensfun`).

## 2) Könnte die CI stärker parallelisiert werden? (4-Kern-Betrachtung)

### Ist-Stand (bereits parallel)
```
detect ─┬─► rust  (fmt → check → test workspace → test zdata → test lensfun → clippy → clippy lensfun → check bench)  [seriell]
        ├─► wasm  (check core wasm + check gui wasm)                                                              [parallel zu rust]
        ├─► bench (cargo bench release + compare.mjs warn)  [parallel zu rust, continue-on-error, non-blocking]
        └─► docs  (unabhängig)
```
Nach `detect` laufen `rust`, `wasm`, `bench`, `docs` **bereits parallel** auf je eigenen Runnern (je 4 vCPUs, je eigener Cache/Container). Das ist die günstige Parallelisierung.

### Option A: `rust`-Job intern aufsplitten (z.B. `rust-fmt-check`, `rust-test`, `rust-clippy`, `rust-lensfun`)
- **Pro:** Kürzere *Wall-Clock* wenn Runner frei sind (aktuell 4m56s Rust checks dominant; WASM 26s, Bench 1m32s sind schon vorbei während Rust noch läuft). Aufteilung könnte Rust-Wall auf ~2m drücken (längster Teil-Job `cargo test --workspace --all-targets` ~2m + Build-Cache).
- **Contra (4-Kern-Realität):**
  - Jeder Teil-Job zahlt **Fixkosten** erneut: `checkout` + `rust-toolchain@stable` (stable neu pinnen + clippy/rustfmt) + `Swatinem/rust-cache` Restore + `ghcr.io/.../lumina-ci:latest` Container-Pull (LibRaw+Lensfun Image ~GB). Auf 4 vCPUs ist cargo bereits gut ausgelastet (`cargo test` baut ~10 Crates parallel mit `-j4`); ein zweiter Job bekommt nicht „mehr Kerne", sondern denselben 4-Kern-Slice auf anderem Runner — Durchsatz steigt nur wenn Runner unbegrenzt & Cache warm.
  - **Cache-Effizienz sinkt:** `rust-cache` ist pro Job isoliert (Cache-Key je Job). Split = 3× Cache-Restore/Save, höhere Miss-Rate. `sccache` (nicht genutzt) könnte das abfedern, ist aber Setup-Aufwand.
  - **SSD 14 GB:** Jeder parallele Job braucht eigenes `target/` (mehrere GB Debug+Release). Bei 3 parallelen Rust-Teil-Jobs × `target/debug` + `cargo bench --release` (Bench braucht Release-Artefakte ~1GB) kann der SSD eng werden; aktuell passt es weil Bench eigenen Runner hat.
  - **Matrix-Effekt gering:** Die teuersten Schritte sind nicht CPU-gebunden sonder I/O (crate-Download, Linken von `lumina-core` + `lumina-gui` + `lumina-raw` LibRaw). Linken ist single-threaded, skaliert nicht mit mehr Jobs.
- **Fazit:** Zusätzliche Parallelisierung *innerhalb* `rust` ist auf 4 vCPUs **wenig effektiv** — geschätzt +15–25% Wall-Gewinn bei +40–60% Runner-Minuten & Cache-Overhead. Lohnt erst wenn Rust-Job >8 Min wird oder `cargo nextest` + `sccache` eingeführt werden.

### Option B: Feinere Parallelisierung mit `cargo nextest` (Test-Runner)
- `cargo nextest` führt Test-Binaries parallel mit besserer Partitionierung als `cargo test` (das pro Crate seriell testet). Auf 4 Kernen könnte `lumina-core` (224 Tests) + `lumina-sidecar` (94) + `lumina-gui` (81) statt seriell überlappend laufen. Gewinn: ~20–30s bei aktuellem Umfang, skaliert mit Testzahl. Kosten: `nextest` installieren, JUnit-Output anpassen.
- **4-Kern-Nutzen:** Mittel — `cargo test` ist bereits mit `-j4` gut, aber `nextest` reduziert Scheduling-Gaps bei vielen kleinen Test-Binaries. Auf 14 GB SSD unkritisch.

### Option C: WASM & Bench noch breiter fächern
- `wasm` heute: 2 `cargo check` seriell (core wasm + gui wasm). Könnten als Matrix `wasm-core` / `wasm-gui` parallel. Gewinn <10s (beide zusammen 26s). Nicht wert — Runner-Overhead > Gewinn.
- `bench` heute schon isoliert, `continue-on-error` non-blocking. Könnte `cargo bench -- --sample-size 50` + `compare.mjs` parallel zu `cargo check -p lumina-bench --all-features` (heute seriell im `rust`-Job letzter Step). Auslagern in `bench`-Job würde `rust`-Job um ~17s kürzen.

### Option D: Caching verbessern statt mehr Jobs
- Höchste Hebelwirkung bei 4 Kernen: `Swatinem/rust-cache@v2` bereits aktiv, aber `sccache` + `--shared` Registry-Cache oder `actions/cache` für `~/.cargo/registry` + `target/` würde mehr bringen als Job-Splitting. Auch `cargo check` vor `cargo test` doppelt kompiliert — `cargo test --no-run` + `cargo test` ohne Rebuild wäre effizienter als parallele Jobs.

## 3) Empfehlung — nichts umsetzen vor F-103-N6

- **Dokumentieren, nicht umsetzen.** Der aktuelle CI ist für Pre-MVP angemessen: 4 Jobs parallel, Rust dominant ~5 Min Wall, total ~6 Min bis alle grün. Für manuellen Review F-103-N6 ist Stabilität wichtiger als 1 Min Wall-Gewinn.
- **4-Kern-Constraint dokumentieren:** `ubuntu-latest` = 4 vCPUs ist *normativ* in `feature/quality/performance-benchmarks.md` und hier. Lokale Dev-Maschine (M5 Pro 12 Kerne) ist schneller — Benchmarks sind deshalb lokal zu capturen (`BENCH-BASELINE-1`), nicht in CI zu gaten (CI `compare.mjs --mode warn` non-blocking, per F-074).
- **Wenn später gewünscht (Post-MVP, CI >8 Min):** Erst `sccache` + `cargo nextest` evaluieren, dann ggf. `rust` in `rust-test` / `rust-lint` aufteilen. Nicht jetzt.

### Referenzen
- Workflow: `.github/workflows/ci.yml` (Jobs `rust`/`wasm`/`bench`/`docs`, Container `lumina-ci:latest` LibRaw 0.22.2)
- Baseline/Budgets: `perf/baseline.json`/`perf/budgets.json` (F-074, `BENCH-BASELINE-1` offen)
- Toolchain: `rustc 1.98.0`, `dtolnay/rust-toolchain@stable` + `Swatinem/rust-cache@v2`
- Runner-Spec: https://docs.github.com/en/actions/using-github-hosted-runners/using-github-hosted-runners/about-github-hosted-runners (ubuntu-latest 4-core, 16 GB, 14 GB SSD)
