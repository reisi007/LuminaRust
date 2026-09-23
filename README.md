# LuminaRust

LuminaRust soll ein nicht-destruktiver, modularer RAW-Prozessor in Rust werden:
headless-first für CLI und Batchverarbeitung, mit nativer Desktop-GUI und
lokaler ONNX-Inferenz.

Der zentrale Architekturentscheid ist **Sidecar-first**. Originaldateien
bleiben unverändert. Bearbeitungen, virtuelle Kopien und persistierte AI-Masken
werden neben dem Original gespeichert. Eine zentrale Datenbank darf später als
wiederaufbaubarer Index ergänzt werden, ist aber keine Voraussetzung.

Pro Bild sind zwei autoritative Dateien vorgesehen:
`<filename>.lumina.json` für Manifest, Rezepte und Masken-DAG sowie
`<filename>.lumina.zdata` für komprimierte binäre Maskenpayloads. Der Ordner
`.lumina/` enthält ausschließlich löschbaren Cache und geerbte
Preview-Einstellungen.

## Inhaltsverzeichnis

- [Projektstatus](#projektstatus)
- [Dokumentation](#dokumentation)
- [Geplante Architektur](#geplante-architektur)
- [Lokales Setup](#lokales-setup)
- [CI](#ci)
- [Arbeitsweise](#arbeitsweise)
- [Nächster Schritt](#nächster-schritt)

## Projektstatus

Das Repository befindet sich in der frühen Implementierungsphase. Der
Workspace enthält einen portablen Rasterbild-MVP: PNG/JPEG/WebP werden über
`lumina-core` dekodiert, mit Exposure/Contrast bearbeitet und exportiert.
`lumina-sidecar` persistiert Rezepte atomar als JSON; `lumina-cli` bietet dafür
`process` und `inspect`. `lumina-gui` ist als native Desktop-GUI verfügbar;
WASM/Browser ist nicht mehr Teil des SOLL. RAW ist Teil des MVP-Gates und
wird nativ über den installierten LibRaw-Adapter ergänzt; ONNX, Maskenoperatoren,
Migrationen,
Cache und Mehrbild-Synchronisierung sind weiterhin offen. Die Feature-SOLL-
Dokumentation bleibt vor jeder weiteren Implementierung die verbindliche
Zieldefinition.

## Dokumentation

- [`feature/README.md`](feature/README.md): Feature-Index und Einstiegspunkt
- [`feature/architecture/sidecar.md`](feature/architecture/sidecar.md):
  Sidecar-Manifest, Artefakte, Migrationen und Persistenz
- [`feature/architecture/pipeline.md`](feature/architecture/pipeline.md):
  Renderpipeline, Versionierung, Render-Key und Cache
- [`feature/product/virtual-copies.md`](feature/product/virtual-copies.md):
  virtuelle Kopien und unabhängige Rezepte
- [`feature/product/ai-masks.md`](feature/product/ai-masks.md): persistierte
  AI-Masken und Gültigkeitsstatus
- [`feature/platform/cli-gui-wasm.md`](feature/platform/cli-gui-wasm.md):
  Plattformgrenzen, CLI, native GUI und optionaler Index
- [`feature/quality/conflicts-and-acceptance.md`](feature/quality/conflicts-and-acceptance.md):
  Konflikte, Abnahmeszenarien und Testanforderungen
- [`Agents.md`](Agents.md): verbindliche Regeln für Build-, Implementierungs-
  und Verifizierungs-Agenten
- [`Agents.todo.md`](Agents.todo.md): lebender Umsetzungsplan mit offenen
  Aufgaben

## Geplante Architektur

```text
crates/
  lumina-core/       # portable Domäne und Renderpipeline
  lumina-sidecar/    # Sidecar-Schema, Migration, Validierung, Writes
  lumina-raw/        # RAW-Decoder, Demosaicing, EXIF, Farbprofile
  lumina-onnx/       # native ONNX-Inferenz und Maskenartefakte
  lumina-cli/        # headless CLI und Batchjobs
  lumina-gui/        # native Desktop-Oberfläche
  lumina-index/      # optionaler, wiederaufbaubarer Index
```

`lumina-sidecar` und `lumina-core` sind für das Zielprodukt erforderlich.
`lumina-index` bleibt optional.

## Lokales Setup

Für die reine Planungsphase ist keine Rust-Toolchain erforderlich. Sobald der
Cargo-Workspace angelegt wird, wird auf macOS empfohlen:

```bash
xcode-select --install
brew install rustup pkg-config cmake libraw
rustup toolchain install stable
rustup default stable
rustup component add rustfmt clippy
ln -sf "$(rustup which rustfmt)" "$HOME/.cargo/bin/rustfmt"
ln -sf "$(rustup which cargo-fmt)" "$HOME/.cargo/bin/cargo-fmt"
ln -sf "$(rustup which cargo-clippy)" "$HOME/.cargo/bin/cargo-clippy"
exec $SHELL -l
rustc --version
cargo --version
```

Homebrew ist nicht zwingend erforderlich, wenn Rust und die nativen Buildtools
bereits anderweitig installiert sind. Für native RAW-Unterstützung müssen
`libraw` und `pkg-config` verfügbar sein (`brew install libraw pkg-config`).
LibRaw steht unter der LGPL-2.1-or-later; Distributionen müssen die LibRaw-
Lizenz und die dynamische Systemabhängigkeit berücksichtigen. Die
`rustup which`-Symlinks stellen bei der Homebrew-Variante sicher, dass
`cargo fmt` und `cargo clippy` auch nach einer separaten Component-Installation
als lokale Cargo-Subcommands gefunden werden.

### Debian/Ubuntu ohne Display

Für den Headless-Build werden zusätzlich `build-essential`, `pkg-config`,
`clang`, `liblensfun-dev`, `libglib2.0-dev`, `libjpeg-dev`, `liblcms2-dev`,
`zlib1g-dev` und `libssl-dev` benötigt. Die GUI-Kompilierung benötigt
zusätzlich die üblichen X11-/Wayland-/GL-Header; ein Display oder eine GPU
wird für die CPU-/egui-Tests nicht vorausgesetzt.

Debian 12 liefert standardmäßig LibRaw 0.20.2, während der Workspace wegen
seines ABI-Vertrags LibRaw >= 0.22.0 verlangt. Für reproduzierbare
RAW-Tests entweder den gepinnten CI-Container verwenden oder LibRaw 0.22.2
unter einem eigenen Prefix installieren und vor Cargo-Commands setzen:

```bash
export PKG_CONFIG_PATH="$HOME/.local/opt/libraw-0.22.2/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$HOME/.local/opt/libraw-0.22.2/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
pkg-config --modversion libraw_r   # muss 0.22.x sein
```

Ohne diese ABI-kompatible Bibliothek schlägt der Build absichtlich laut fehl;
er wird nicht still auf die inkompatible Distro-Version zurückfallen.

## CI

GitHub Actions liegt unter [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

- Dokumentationsdateien werden bereits geprüft.
- Der Detect-Job aktiviert die Rust-Arbeit automatisch, sobald ein Root-`Cargo.toml` existiert.
- Die Rust-Prüfungen sind in `rust-fast` (fmt/check/Clippy/Feature-Checks/Ratchet),
  `rust-test-gui` (alle `lumina-gui`-Targets) und `rust-test-rest` (Workspace
  ohne GUI plus LibRaw-ABI, zdata, Lensfun und ONNX-Runtime) aufgeteilt; die
  GUI-Ausschluss- und GUI-Test-Schritte sind bewusst paarig, sodass keine
  Abdeckung verloren geht.
- Alle drei Rust-Shards verwenden den gepinnten Container
  `ghcr.io/reisi007/luminarust/lumina-ci:latest` (LibRaw 0.22.2, identisch zur
  lokalen Homebrew-Version; das Image baut
  `.github/workflows/ci-libraw-image.yml` aus `docker/Dockerfile` und wird als
  `<commit-sha>`-Tag immutabel veröffentlicht, die Version steht im
  OCI-Label `lumina.libraw_version`). Dadurch dekodieren CI und lokale
  Entwicklung RAW identisch — CR3-Dimensionen unterscheiden sich zwischen
  LibRaw-Versionen.
- Benchmarks und Dokumentationschecks bleiben eigenständige Jobs; es gibt
  keinen WASM-/Browser-Job mehr (WASM ist im SOLL entfernt).
- `actionlint` ist lokal vorhanden und kann alle Workflows prüfen.

## Arbeitsweise

Der Build-Agent orchestriert größere Aufgaben und delegiert Implementierung
sowie unabhängige Verifizierung an unterschiedliche Subagenten. Kleine,
risikoarme Dokumentations-, Plan- und CI-Korrekturen darf er selbst vornehmen.
Änderungen an Codeverhalten, Datenformaten, Persistenz, Pipeline oder Tests
werden immer unabhängig verifiziert.

Erledigte Einträge werden aus `Agents.todo.md` entfernt, sobald ein anderer
Subagent die Implementierung und Testabdeckung bestätigt hat.

## Nächster Schritt

Der Raster-MVP kann beispielsweise so verwendet werden:

```bash
cargo run -p lumina-cli -- process --input photo.png --output edited.webp \
  --exposure 0.5 --contrast 0.2 --highlights -0.15 --shadows 0.2
cargo run -p lumina-cli -- inspect photo.png
```

Der erste GUI-User-Test ist ein nativer Desktop-Test mit optionalem
Argumentpfad:

```bash
RUST_LOG=trace cargo run -p lumina-gui -- /pfad/zum/bildordner
```

Ohne Display werden die headless GUI-/CPU-Prüfungen verwendet:

```bash
cargo test -p lumina-gui
```

Die `egui_kittest`-Snapshots und der vollständige RAW-Matrixlauf benötigen
jeweils ihre dokumentierten optischen/GPU- bzw. `LUMINA_MATRIX=1`-Gates;
sie ersetzen keinen nativen User-Test. GPU-Parität benötigt außerdem eine
echte Hardware mit renderbaren `R32Float`-Targets; ein `llvmpipe`-/GL-
Softwareadapter ist keine gültige Paritätsreferenz. Die native GUI liest
PNG/JPEG/WebP und RAW über einen lokalen Pfad oder Drag-and-drop und speichert
das Rezept als `<original>.lumina.json`.

RAW ist ein verbindliches MVP-Gate: Native CLI und Desktop dekodieren die
unterstützten RAW-Endungen über LibRaw und führen das Ergebnis durch denselben
`ImageFrame`-/Rezeptpfad wie Rasterbilder. Browser/WASM ist nicht mehr Teil
des SOLL. Lizenzgeeignete Fixtures gehören nach `sample-data/raw/` (nicht ins
Repository, falls ihre Lizenz das verbietet); `LUMINA_RAW_FIXTURE` kann auf
eine einzelne CR2-, NEF-, ARW- oder DNG-Datei zeigen. Ohne Fixture gibt es
keinen bestandenen Kamera-Golden-Test.
Die lokalen Nutzer-Fixtures `sample-data/raw/aircraft-landscape.cr3` und
`sample-data/raw/aircraft-portrait.cr3` sind für den Testlauf geeignet. Der
echte Testlauf lautet zum Beispiel
`LUMINA_RAW_FIXTURE="$PWD/sample-data/raw/aircraft-landscape.cr3" rustup run stable cargo test -p lumina-raw -- --ignored`.
Die Golden-Dimensionen (6032×4024) gelten für LibRaw 0.22.2; die CI läuft
deshalb im gepinnten `lumina-ci`-Container (LibRaw 0.22.2; siehe Abschnitt CI
und `docker/Dockerfile`).
Lens, Kamera-Farbmatrix und Profile bleiben bis zur Prüfung der konkreten
LibRaw-Felder als F-034 offen; es werden keine Dummywerte verwendet.

Der Workspace und der vertikale Rasterbild-MVP sind vorhanden. Der nächste
Schritt ist der dokumentierte manuelle Desktop-User-Test mit anschließender
F-103-N6-Abnahme.
