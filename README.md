# LuminaRust

LuminaRust soll ein nicht-destruktiver, modularer RAW-Prozessor in Rust werden:
headless-first für CLI und Batchverarbeitung, mit optionaler Desktop-/Web-GUI
und lokaler ONNX-Inferenz.

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
`process` und `inspect`. Der erste gemeinsame Desktop-/WASM-User-Test ist als
`lumina-gui` verfügbar. RAW ist Teil des MVP-Gates und wird nativ über den
installierten LibRaw-Adapter ergänzt; ONNX, Maskenoperatoren, Migrationen,
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
  Plattformgrenzen, CLI, GUI, WASM und optionaler Index
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
  lumina-gui/        # Desktop-/Web-Oberfläche
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

## CI

GitHub Actions liegt unter [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

- Dokumentationsdateien werden bereits geprüft.
- Der Rust-Job startet automatisch, sobald ein Root-`Cargo.toml` existiert.
- Danach laufen `fmt`, `check`, `test` und `clippy` mit stabiler Toolchain.
- Ein separater WASM-Job prüft `lumina-core` und `lumina-gui` für
  `wasm32-unknown-unknown`.
- `actionlint` ist lokal bereits vorhanden und kann den Workflow prüfen.

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
  --exposure 0.5 --contrast 0.2
cargo run -p lumina-cli -- inspect photo.png
```

Der erste GUI-User-Test läuft nativ oder im Browser:

```bash
cargo run -p lumina-gui
cd crates/lumina-gui
trunk serve
trunk build --release
```

Die native GUI liest PNG/JPEG/WebP über einen lokalen Pfad oder Drag-and-drop
und speichert das Rezept als `<original>.lumina.json`. Im Browser werden Bilder
über Drag-and-drop geladen; Browser-Dateispeichern ist im MVP noch nicht
implementiert.

RAW ist ein verbindliches MVP-Gate: Native CLI und Desktop dekodieren die
unterstützten RAW-Endungen über LibRaw und führen das Ergebnis durch denselben
`ImageFrame`-/Rezeptpfad wie Rasterbilder. Browser/WASM bleibt für RAW
ausdrücklich nicht verfügbar und meldet eine Capability-Fehlermeldung.
Lizenzgeeignete Fixtures gehören nach `tests/fixtures/raw/` (nicht ins
Repository, falls ihre Lizenz das verbietet); `LUMINA_RAW_FIXTURE` kann auf
eine einzelne CR2-, NEF-, ARW- oder DNG-Datei zeigen. Ohne Fixture gibt es
keinen bestandenen Kamera-Golden-Test.
Der echte Testlauf lautet
`LUMINA_RAW_FIXTURE=/pfad/zu/datei.cr2 rustup run stable cargo test -p lumina-raw -- --ignored`.
Lens, Kamera-Farbmatrix und Profile bleiben bis zur Prüfung der konkreten
LibRaw-Felder als F-034 offen; es werden keine Dummywerte verwendet.

Der Workspace und der vertikale Rasterbild-MVP sind vorhanden. Der nächste
Schritt ist ein Desktop-/WASM-GUI-Grundgerüst mit einem ersten User-Test.
