# lumina-gui-harness — autonomer GUI-Verifikations-Harness (macOS)

Dieses Crate stellt einen **autonomen GUI-Verifikations-Harness** für die native
Desktop-App bereit. Er startet die echte `lumina-gui` (eframe/wgpu) als
echtes Fenster und fährt sie **ohne manuelles Zutun** an:

- öffnet `sample-data/raw` als Arbeitsverzeichnis,
- wartet, bis das `Lumina`-Fenster erscheint,
- erstellt `screencapture`-Screenshots der Fensterregion zu mehreren
  Prüfzeitpunkten,
- klickt per [`enigo`](https://crates.io/crates/enigo) auf das erste
  Filmstrip-Thumbnail,
- zieht den **Exposure**-Slider im Develop-Panel,
- schaltet mit den Tasten **`G`** (Library) und **`D`** (Develop) zwischen den
  Modulen um,
- **verifiziert über das eigene stderr-Log** der GUI, dass ein Bild geladen und
  gerendert wurde (`loaded image …`) und dass **kein Crash** (kein `PANIC`,
  Prozess noch am Leben) aufgetreten ist,
- schreibt einen JSON-Report und beendet mit Exit-Code `0` (bestanden) bzw.
  `1` (nicht bestanden).

> Der Harness ist bewusst als **separates Binary-Crate ausgegliedert**, damit
> **kein** Eingriff in `crates/lumina-gui` nötig ist (außer dem
> Workspace-Eintrag im Root-`Cargo.toml`). Er ist **macOS-only** und kein Teil
> der Renderpipeline oder der GUI. Auf anderen Plattformen kompiliert er zu
> einem Stub, der das erklärt — CI bleibt also auf Linux grün.

## Warum macOS?

Die native MVP-GUI wird über die macOS-APIs *Accessibility* (Fenster finden,
synthetische Eingaben) und *Screen Recording* (Screenshots via
`/usr/sbin/screencapture`) gesteuert. Der Harness läuft daher nur auf einem Mac
mit echter Anzeige.

## Voraussetzungen / Berechtigungen (macOS)

Bevor der Harness funktioniert, muss der Prozess, der ihn ausführt (Terminal
oder IDE), die folgenden Berechtigungen besitzen. Diese gewährt man unter
**Systemeinstellungen → Datenschutz & Sicherheit**:

| Berechtigung | Wofür |
| --- | --- |
| **Bedienungshilfen / Accessibility** | `osascript` (System Events) darf das Lumina-Fenster auslesen und in den Vordergrund holen; `enigo` darf synthetische Maus-/Tastatureingaben posten. |
| **Bildschirmaufnahme / Screen Recording** | `screencapture -R` darf die Fensterregion erfassen. |

Praxis:

```bash
sudo tccutil reset Accessibility    # ggf. alte Berechtigungen zurücksetzen
sudo tccutil reset ScreenCapture
```

Danach App/Terminal beenden und neu starten, damit TCC die neue gewährte
Berechtigung übernimmt. Betreibt man den Harness aus einem IDE-Terminal,
muss oft die IDE selbst die Berechtigung erhalten.

> **Hinweis:** Der Harness steuert die **echte** Maus/Tastatur und greift auf
> den Bildschirm zu. Während des Laufs keine anderen Fenster über das
> Lumina-Fenster legen — sonst landen die Klicks falsch. Nach dem Lauf wird die
> GUI sauber beendet (`SIGTERM`), damit nie ein App-Fenster gegen echte
> Benutzerdaten offen bleibt.

## Start

Im Repository-Root:

```bash
cargo run -p lumina-gui-harness
```

Optional mit einem anderen Arbeitsverzeichnis:

```bash
cargo run -p lumina-gui-harness -- /abs/pfad/zu/bildern
```

(Standard ist `sample-data/raw`, das die lizenzierten CR3-Fixtures enthält.)

### Was passiert

1. Der Harness startet `cargo run -p lumina-gui -- sample-data/raw` im
   Hintergrund und leitet dessen stdout/stderr nach
   `target/gui-verify/gui.log`.
2. Er pollt per `osascript` das `Lumina`-Fenster (max. 120 s).
3. Sobald das Fenster da ist: Fokus setzen, ~6 s auf Auto-Load + ersten Render
   warten, Screenshot `01_develop_initial.png`.
4. Filmstrip klicken → `exposure_drag` → Screenshot `02_develop_exposure.png`.
5. `G` (Library) → Screenshot `03_library_g.png` → `D` (Develop) → Screenshot
   `04_develop_final.png`.
6. Das `gui.log` wird nach Markern durchsucht (geladenes Bild, `PANIC`,
   `[ERROR]`), der Report geschrieben, die GUI beendet, Exit-Code gesetzt.

### Ausgaben

Alles landet unter `target/gui-verify/`:

- `gui.log` — das Log der gesteuerten GUI,
- `01_develop_initial.png` … `04_develop_final.png` — die Screenshots,
- `report.json` — strukturbildender JSON-Report (Checkpoints, Marker, Dateien).

### Exit-Codes

| Code | Bedeutung |
| --- | --- |
| `0` | Bestanden: Bild geladen + gerendert, kein Panic, Prozess lebt. |
| `1` | Nicht bestanden (Details im Report). |
| `2` | Nicht unterstützte Plattform / falsche Nutzung. |

## Tests

```bash
cargo test -p lumina-gui-harness
```

Die Tests sind reine Logik-/Parsing-Tests (keine Bildschirmsteuerung) und
laufen nur auf macOS.

## Lints & Format

```bash
cargo fmt -p lumina-gui-harness -- --check
cargo clippy -p lumina-gui-harness --all-targets -- -D warnings
```

## Grenzen & bekannte Verhaltensweisen

- **Pixelgenaue Widget-Koordinaten** (Filmstrip-Zelle, Exposure-Slider) sind
  aus dem Fensterrechteck **heuristisch** berechnet und an die
  Standard-Fenster-/Panelgrößen von `lumina-gui` angelehnt (rechtes Develop-
  Panel, Basic-Sektion oben, Filmstrip unten). Bei geänderten Layouts oder
  anderen Fenstergrößen kann ein Klick danebenliegen; die Screenshots machen
  das sichtbar.
- **Retina/DPI:** `screencapture -R` und `enigo::Coordinate::Abs` arbeiten
  beide im Punkt-Koordinatensystem (top-left, y-down), sodass sie zueinander
  passen. Die Punkte werden nicht skaliert.
- **„Visuell via Vision“** ist eine *optionale* Nachbearbeitung der PNGs durch
  einen Vision-Agenten; der Harness selbst bestätigt Preview/kein-Crash über
  das Log und den Exit-Code (wie in der Aufgabenstellung gefordert).
- Der Harness modifiziert **niemals** Originaldateien oder Sidecars; er steuert
  nur die GUI und legt Artefakte nur unter `target/gui-verify/` ab.

## Architektur-Hinweis

Getrieben wird nur die öffentliche GUI-Oberfläche über macOS-Events — es gibt
**keine** API-Abhängigkeit zu `lumina-gui`-internen Funktionen und **keinen**
Eingriff in `crates/lumina-gui/src/`. Dadurch kollidiert dieser Harness nicht
mit parallelen Änderungen am `lumina-gui`-Crate.
