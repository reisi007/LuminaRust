# Golden-Referenzplattform (GUI-Snapshots)

**Feature-IDs:** F-103-N9 (UI-Snapshot-Regressionen) · F-043
(Korrektheits-Golden-Tests) · GOLDEN-REF-30
**Status:** SOLL und Erzwingung umgesetzt (Dokument + `scripts/golden_ref.sh`
+ `scripts/golden_ref.lock` + Pre-Commit-Gate `.githooks/pre-commit` +
committeter Regressions-Suite `scripts/golden_ref_test.sh`, §4.2, in der
`docs`-CI-Stufe auf `ubuntu-latest`); Baseline-Erneuerung selbst ist
`GOLDEN-BASELINE-32` und **nicht** Teil dieses Dokuments.
**User-Entscheidung 2026-09-25:** Die Golden-Referenz ist **macOS/Metal**
(diese Maschinenklasse). Die Goldens sind ein **lokales** Gate und werden in
CI **nie** verifiziert.
**Verwandt:** `feature/platform/cli-gui-wasm.md` (F-103-N9),
`feature/platform/capability-matrix.md` (LibRaw-Pin, native Fähigkeiten),
`feature/quality/fixtures-licensing.md` (F-073/F-078 Fixture- und
Lizenzregeln), `feature/quality/performance-benchmarks.md` (F-074 Baseline- und
Budget-Stores als Vorbild für gepinnte Stores), `Agents.md` (Dateigrößen- und
Verifikationsregeln).

---

## Inhaltsverzeichnis

- [1. Problem und Ziel](#1-problem-und-ziel)
- [2. Verbindliche Referenzplattform](#2-verbindliche-referenzplattform)
- [3. Fingerabdruck und Lock-Format](#3-fingerabdruck-und-lock-format)
- [4. `scripts/golden_ref.sh`](#4-scriptsgolden_refsh)
  - [4.1 Pre-Commit-Gate: die tatsächlich lasttragende Schranke](#41-pre-commit-gate-die-tatsächlich-lasttragende-schranke)
  - [4.2 Regressions-Suite des Wächters: `scripts/golden_ref_test.sh`](#42-regressions-suite-des-wächters-scriptsgolden_ref_testsh)
    - [4.2.1 Der eine Bruch in der Selbstkonsistenz — und warum er nötig war](#421-der-eine-bruch-in-der-selbstkonsistenz--und-warum-er-nötig-war)
    - [4.2.2 Der zweite Bruch: die Golden-Inventar-Ebene war nicht verankert](#422-der-zweite-bruch-die-golden-inventar-ebene-war-nicht-verankert)
- [5. `UPDATE_SNAPSHOTS`-Sperre (Geltung: `check` und `gate`)](#5-update_snapshots-sperre-geltung-check-und-gate)
- [6. Fixture-Set der Goldens](#6-fixture-set-der-goldens)
- [7. Golden-Inventar](#7-golden-inventar)
- [8. Exakte Regenerations-Kommandozeile](#8-exakte-regenerations-kommandozeile)
- [9. Mismatch-Politik](#9-mismatch-politik)
- [10. CI-Politik: lokal, nie CI](#10-ci-politik-lokal-nie-ci)
- [11. Bekannte Grenzen](#11-bekannte-grenzen)
  - [11.1 Der Guard ist nicht automatisch — die Umgehung im Klartext](#111-der-guard-ist-nicht-automatisch--die-umgehung-im-klartext)
  - [11.2 Weitere Grenzen](#112-weitere-grenzen)

---

## 1. Problem und Ziel

Die `egui_kittest`-Goldens unter `crates/lumina-gui/tests/snapshots/` waren
bis 2026-09-25 an **keiner** Maschine dokumentiert reproduzierbar: die
Fingerprints (OS, Treiber, Adapter, Fonts, Skalierung, LibRaw) waren nirgends
festgeschrieben, und ein erheblicher Teil der Vergleiche schlug auf der
Referenzmaschine fehl (Zählung offen, siehe §7). Jeder „einfach die Goldens neu
schreiben"-Schritt hat denselben Blocker damit wiederhergestellt, ohne ihn zu
benennen.

Ziel dieses Dokuments ist deshalb **nicht** eine Baseline, sondern eine
**benannte, prüfbare Referenzumgebung**:

1. Die Referenzplattform ist normativ beschrieben und liegt als
   Fingerabdruck im Repo (`scripts/golden_ref.lock`).
2. `scripts/golden_ref.sh` erkennt die aktuelle Umgebung zur Laufzeit und
   vergleicht sie mit dem Pin. Es **verweigert** jede Golden-Aktualisierung
   auf einer nicht gepinnten Maschine — aber nur für Aufrufe, die durch das
   Skript selbst gehen (`check` / `gate`). Es kann einen Aufruf, der an ihm
   **vorbeiläuft** (etwa ein nacktes `UPDATE_SNAPSHOTS=1 cargo test …`), nicht
   abfangen; dafür ist das Pre-Commit-Gate in §4.1 zuständig. Diese Grenze ist
   in §11.1 im Klartext benannt.
3. Damit wird „irgendeine Referenzmaschine" durch „diese benannte,
   reproduzierbare Referenz" ersetzt.

Ausdrücklich **nicht** Gegenstand dieses Dokuments:

- das Neu-Schreiben der Goldens selbst (`GOLDEN-BASELINE-32`),
- die Bereinigung der Fixture-Dateien (`GOLDEN-FIXT-31`),
- die 12 heute abweichenden Goldens einzeln zu begründen (ebenfalls
  `GOLDEN-BASELINE-32`).

---

## 2. Verbindliche Referenzplattform

> **Verbindlich (SOLL).** Ein Golden darf nur auf genau dieser Plattform
> erzeugt werden. Jede andere Plattform — auch ein anderes macOS, auch ein
> anderes Apple-Silicon-Chip — ist **kein** gültiger Erzeuger. Ein auf einer
> anderen Plattform erzeugtes Golden ist ungültig und wird nicht committet.

### 2.1 Gemessene Referenz

| Merkmal | Wert der Referenz |
| --- | --- |
| OS | macOS **27.0** (Produktversion via `sw_vers -productVersion`) |
| Architektur | `arm64` (Apple Silicon) |
| CPU / SoC | **Apple M5 Pro** |
| GPU / wgpu-Adapter | **Apple M5 Pro** (Metal-Gerät, via `system_profiler SPDisplaysDataType` erfasst) |
| wgpu-Backend | **Metal** (`wgpu::Backends::all()` löst sich unter macOS auf Metal auf; wgpu 30 bringt dort kein Vulkan/GL/DX12-Backend mit) |
| Unterstützte Metal-Version | **Metal 4** |
| wgpu-Crate | **30.0.1** (aus `Cargo.lock`) |
| egui/eframe/egui_kittest | **0.36.2** (aus `Cargo.lock`) |
| Toolchain | `channel = "stable"` aus `rust-toolchain.toml`, `rustc 1.98.0` |
| Font-Stack | **egui-gebundener Default** (in den Binary kompiliert, Feature `default_fonts`; **keine** Systemfonten, **keine** `FontDefinitions`-Umleitung in `crates/lumina-gui/src`) |
| UI-Skalierung | **1.0** (`pixels_per_point`, egui_kittest-Default; wird aus den Goldens selbst abgeleitet) |
| Viewport | **1024 × 720** logisch = 1024 × 720 physisch (`.with_size([1024.0, 720.0])` in `crates/lumina-gui/tests/kittest_snapshots_support/mod.rs`) |
| LibRaw | **0.22.2** (`pkg-config --modversion libraw_r`; der vendorte `libraw-sys`-Build löst genau dieses Modul mit `.atleast_version("0.22.0")` auf) |

### 2.2 Regeln

1. **Adapter ist erfasst, nicht geraten.** `wgpu` protokolliert den gewählten
   Adapter nicht (`wgpu`/`wgpu-core` 30.0.1 enthalten keine Adapter-Log-Zeile),
   und das Harness legt keinen Adapter-String ab. Der Fingerabdruck erfasst
   deshalb zur Laufzeit die Metal-Geräteidentität und die unterstützte
   Metal-Version aus dem laufenden OS — das ist genau das Gerät, an das der
   Metal-Backend von wgpu bindet, plus die wgpu-Crate-Version aus
   `Cargo.lock`, die den Rasterizer bestimmt. Es wird nichts geraten und nichts
   hartkodiert.
2. **Font-Auflösung ist nachweisbar.** `font.resolution` wird ermittelt, indem
   `crates/lumina-gui/src` nach `FontDefinitions` / `set_fonts` / `font_data(`
   durchsucht wird. Heute: `egui-bundled-default`. Sobald jemand Systemfonten
   einbindet, schlägt `check` fehl und verlangt ein bewusstes Re-Pin — genau
   dann, wenn sich das Font-Rendering ändern kann.
3. **Skalierung ist aus den Artefakten abgeleitet.** `ui.scale_factor` wird aus
   den IHDR-Abmessungen der committeten Goldens geteilt durch den logischen
   Viewport berechnet (`1024x720 / 1024x720 = 1.0`). Eine Baseline, die auf
   einem anderen Faktor gerendert wurde, kann sich also nicht hinter einem
   hartkodierten `1.0` verstecken; unterschiedliche Golden-Größen ergeben
   `mixed` und sind ein harter Mismatch.
4. **Kein stiller Ersatz.** Fehlt ein Wert (kein `sw_vers`, kein
   `system_profiler`, kein `pkg-config`, kein Goldens-Verzeichnis), steht
   `unavailable` bzw. `none` im Fingerabdruck. Das ist **kein** Treffer auf den
   Pin, `check` schlägt fehl. Es gibt keinen Fallback-Wert.

---

## 3. Fingerabdruck und Lock-Format

Der Fingerabdruck ist eine geordnete Liste `key=value`-Zeilen. Die
**Schlüsselreihenfolge ist Teil des Formats und wird erzwungen**: `check` und
`gate` validieren vor dem Wertvergleich die kanonische Form des Locks
(`lock_is_canonical`) und lehnen ihn ab, wenn die Schlüsselliste von der
kanonischen Reihenfolge abweicht — kein fehlender, kein zusätzlicher, kein
doppelter und kein umgestellter Schlüssel. Grund: `lock_value` löst nach **Namen**
auf und würde sonst still den ersten Treffer eines doppelten oder
angehängten Eintrags nehmen.

| Schlüssel | Bedeutung | Quelle |
| --- | --- | --- |
| `schema.golden_ref` | Formatversion des Lock-Files (aktuell `1`) | Konstante |
| `os.name` | `macOS` (aus `uname -s`) | Laufzeit |
| `os.macos` | macOS-Produktversion | `sw_vers -productVersion` |
| `os.arch` | CPU-Architektur | `uname -m` |
| `os.cpu` | SoC-Modell | `sysctl -n machdep.cpu.brand_string` |
| `toolchain.channel` | Channel aus `rust-toolchain.toml` | `rust-toolchain.toml` |
| `toolchain.rustc` | installierte `rustc`-Version | `rustc --version` |
| `wgpu.version` | wgpu-Crate-Version | `Cargo.lock` |
| `wgpu.backend` | effektives Backend-Set (`all->metal` unter macOS) | Laufzeit + `uname` |
| `wgpu.backend_env` | `LUMINA_GPU_BACKENDS` wörtlich oder `unset` | Laufzeit |
| `gpu.adapter` | Metal-Gerät / Adapter-Name | `system_profiler SPDisplaysDataType` |
| `gpu.metal` | unterstützte Metal-Version | `system_profiler SPDisplaysDataType` |
| `font.resolution` | `egui-bundled-default` oder `override-in:<dateien>` | Quellscan `crates/lumina-gui/src` |
| `font.egui` | egui-Version (trägt den Font-Datensatz) | `Cargo.lock` |
| `ui.golden_px` | gemeinsame Pixelgröße der Goldens oder `mixed`/`none` | PNG-IHDR der committeten Goldens |
| `ui.scale_factor` | daraus abgeleiteter `pixels_per_point` | Rechnung aus `ui.golden_px` |
| `libraw.version` | verlinkte LibRaw-Version | `pkg-config --modversion libraw_r` |
| `fixtures.count` | Anzahl committeter Fixture-Dateien | `crates/lumina-gui/tests/fixtures` |
| `fixtures.digest` | `<modus>:<sha256>` über `pfad sha256`-Zeilen des Fixture-Baums | Laufzeit |
| `goldens.count` | Anzahl committeter Golden-PNGs | `crates/lumina-gui/tests/snapshots/*.png` |
| `goldens.digest` | `<modus>:<sha256>` über `pfad sha256`-Zeilen der Golden-Baselines | Laufzeit |

**Die beiden abgeleiteten Werte sind gegen eine committete Fixture gepinnt.**
`ui.golden_px` und `ui.scale_factor` sind **Ableitungen**, und die Suite
gleicht sie sonst nur gegen sich selbst ab (§4.2). Der Anker dafür ist
`scripts/fixtures/png_ihdr_probe.png` (33 Byte, Herkunft und
Reproduktionskommando in `scripts/fixtures/README.md`): die Suite lässt die
echte `emit_fingerprint` des echten `scripts/golden_ref.sh` über diese Datei
laufen und prüft den **literalen** Wert `16909060x84281096` — damit ist die
Byte-Reihenfolge des IHDR-Lesens (`4x u32 big endian`, siehe die Zeile
`ui.golden_px` in dieser Tabelle) an etwas festgemacht, das die Ableitung
nicht selbst erzeugt hat, und `ui.scale_factor` ist mit derselben Fixture über
einen **synthetischen** Viewport (`1.0` bei Passung,
`non-unit:<w>/<viewport>,<h>/<viewport>` bei Abweichung) erreichbar.

`scripts/golden_ref.lock` ist eine normale Textdatei:

```text
# Kommentarzeilen beginnen mit '#'.
# Danach genau eine Zeile je Schlüssel: <key>=<value>
# Werte sind einzeilig; Leerzeichen sind erlaubt, CR/LF sind entfernt.
# Die Schlüsselliste muss exakt der kanonischen Reihenfolge entsprechen -
# von `check`/`gate` erzwungen (siehe oben).
schema.golden_ref=1
os.name=macOS
...
```

**CRLF.** `lock_value` normalisiert ein abschließendes `\r` beim **Lesen**,
genau wie `lock_observed_keys` es auf der Strukturseite schon tat. Dadurch
verhält sich ein CRLF-Lock exakt wie sein LF-Zwilling, statt alle 21 Schlüssel
als abweichend zu melden, obwohl gepinnt und aktuell identisch aussehen.
`record` schreibt nie ein CR (Werte werden mit `tr -d '\r\n'` bereinigt), also
kann die Normalisierung nur ein handbearbeitetes Lock dem committeten
angleichen, nie umgekehrt.

### 3.1 Digest-Modus und Inventar

Der Modus-Präfix der beiden Digest-Werte ist **Teil** des Wertes:

- `git:` — Inventar über
  `git ls-files --cached --others --exclude-standard -- <pfad>`: committete
  Dateien plus nicht-ignorierte neue Dateien, unter Achtung der Ignore-Regeln
  des Repos.
- `walk:` — Fallback ohne `git`: Dateisystem-Walk mit expliziter Ausschlussliste
  (`.lumina/`, `*.lumina.json`, `*.lumina.zdata`, `*.lumina-preset.json`,
  `*.cr3`, `.DS_Store`).

Ein Moduswechsel erscheint damit als **Mismatch** und nicht als unerklärlicher
Hashwert.

`git:` ist der Normalfall und notwendig, weil die Laufzeit-Artefakte **keine
feste Suffixliste** haben: `GOLDEN-FIXT-31` staged lizenzierte CR3-Kopien
(gleiche Bytes wie `sample-data/raw/`) zur Test-Setup-Zeit in den Fixture-Baum
und gitignoriert sie. Ein reiner Disk-Walk würde den Digest also zwischen „vor
dem Testlauf" (Datei fehlt) und „nach dem Testlauf" (Datei da) umspringen
lassen. Die Ignore-Regeln des Repos sind die eine Quelle der Wahrheit dafür,
was Laufzeit-Artefakt ist.

Eine gelistete, aber nicht vorhandene Datei geht als `absent` in den Digest
ein (statt als Fehler): der Digest bleibt so eine wohldefinierte Funktion aus
(Pfad, Inhalt oder Abwesenheit) und ein zwischenzeitlich gelöschtes Fixture ist
sichtbar, ohne dass das Skript abbricht.

`goldens.digest` und `fixtures.digest` sind absichtlich **harter** Bestandteil
des Pins:

- Jede Änderung an einem committeten Golden oder an einer committeten Fixture
  erzeugt einen Mismatch. Damit kann kein Golden — und keine Fixture — je
  unbemerkt in die Baseline wandern.
- Nach einer **beabsichtigten** Neuregistrierung (`GOLDEN-BASELINE-32`) ist
  deshalb genau ein anschließender, expliziter `record`-Lauf nötig. Das ist der
  gewollte Zusatzschritt, keine Lastendrift.

Nicht Teil des Fingerabdrucks (bewusst): der `.lumina/`-Preview-Cache, die
`*.lumina.json`-Sidecars, `*.lumina.zdata`- und `*.lumina-preset.json`-Artefakte,
die zur Test-Setup-Zeit gestagten CR3-Kopien sowie egui_kittests
Vergleichsartefakte `*.new.png` / `*.diff.png` / `*.old.png`. Sie sind
gitignoriert und pro Lauf flüchtig; ihre Aufnahme würde den Pin bei jedem Lauf
auffächern (siehe §3.1).

---

## 4. `scripts/golden_ref.sh`

POSIX-`sh` (macOS liefert bash 3.2 ohne assoziative Arrays), kein Netzwerk,
keine neuen Abhängigkeiten. Benutzte Systembefehle: `sh`, `uname`, `sw_vers`,
`sysctl`, `system_profiler`, `pkg-config`, `rustc`, `shasum` (Fallback
`sha256sum`), `dd`, `od`, `awk`, `grep`, `sed`, `tr`, `head`, `wc`, `find`,
`sort`, `diff`, `mv`, `git` (nur lokal, ohne Netz; fehlt `git`, schaltet das
Skript sichtbar auf den `walk:`-Modus, siehe §3.1). Jedes dieser Werkzeuge
wird mit `command -v` geprüft und degradiert auf einen **lauten**
`unavailable`-Wert, nie auf einen stillen Ersatz. (`cut` wird **nicht**
benutzt; die Liste hier und der Skriptkopf sind deckungsgleich.)

| Subkommando | Verhalten | Exit-Code |
| --- | --- | --- |
| `sh scripts/golden_ref.sh print` | zeigt erkannten und gepinnten Fingerabdruck plus Verdikt. **Bleibt auch bei Mismatch und auch bei nicht-kanonischem Lock bei Exit 0** — das ist die Diagnose-Ansicht. | 0 |
| `sh scripts/golden_ref.sh check` | Gate. Prüft **zuerst** die kanonische Form des Locks, dann den Wertvergleich. Exit 0 bei Übereinstimmung, 1 bei Mismatch oder nicht-kanonischem Lock (mit Diagnose bzw. Schlüssel-Diff), 2 bei Nutzungs-/IO-Fehler. | 0 / 1 / 2 |
| `sh scripts/golden_ref.sh record --confirm "<reason>"` | pinnt neu. **Verweigert** (Exit 2, ohne eine Zeile zu schreiben) bei: fehlendem `--confirm`; leerem Grund; **LF**, **CR** oder **CRLF** im Grund (sonst Injection einer `key=value`-Zeile); `--` im Grund; einem unerwarteten Zusatzargument; einem Grund **kürzer als 20 Zeichen**. **Leerzeichen im Grund sind erlaubt** (und der Normalfall). Druckt **vor** dem Pin den `old -> new`-Diff des Locks, den es pinnen will. Schreibt atomar über temporäre Datei + `mv`. | 0 / 2 |
| `sh scripts/golden_ref.sh gate -- <cmd> [args…]` | führt `check` aus und führt `<cmd>` **erst danach** per `exec` aus. Das ist der Hook für Golden-Neuregistrierungen, die man freiwillig durch `gate` führt. | wie `cmd` |

**Namen der Re-Pin-Operation.** Der Arbeitsplan (`Agents.todo.md`, `GOLDEN-REF-30`)
nennt sie `--force-record`; das Werkzeug implementiert sie als
`record --confirm "<reason>"`. Es gibt **kein** zusätzliches `--force-record`-Flag:
`record` **ist** immer die bewusste, begründete Re-Pinierung und verweigert ohne
Begründung.

`GOLDEN_REF_LOCK` überschreibt den Pfad der Lock-Datei (nur für Testläufe
gedacht). **Alle vier** Subkommandos — auch `record`, also auch der einzige
schreibende Pfad — geben dafür eine laute `WARNING:`-Zeile auf stderr aus,
damit ein Guard, der auf eine beliebige Datei zeigen könnte, sichtbar bleibt.

### 4.1 Pre-Commit-Gate: die tatsächlich lasttragende Schranke

> **Warum es das braucht.** `scripts/golden_ref.sh` kann einen Aufruf, der an
> ihm **vorbeiläuft**, nicht abfangen. Der natürlichste Befehl eines Entwicklers
> — `UPDATE_SNAPSHOTS=1 cargo test -p lumina-gui -- --ignored` — berührt das
> Skript nicht und setzt die Umgebungsvariable ohne Beteiligung des Skripts.
> Solange nicht §11.1 im Hinterkopf bleibt, ist der Mechanismus damit umgangen.
> Das Pre-Commit-Gate schließt genau diese Lücke, indem es **das, was in die
> Historie geht**, prüft statt **das, was ausgeführt wird**.

`.githooks/pre-commit` enthält zusätzlich zum bestehenden CodeGraph-Sync (der
weiterhin *fail-open* ist) ein **fail-closed** Gate:

1. Ist **irgendein** `crates/lumina-gui/tests/snapshots/*.png` gestaged
   (Hinzufügen, Ändern, Löschen, Umbenennen), dann:
2. muss `scripts/golden_ref.lock` im **selben** Commit gestaged sein, **und**
3. muss dessen `# Grund:`-Zeile sich **geändert** haben (Vergleich `HEAD:` gegen
   den **Index**, nicht gegen die Arbeitskopie).

Damit erreicht **jeder Commit, der über `git commit` entsteht**, Folgendes: ein
gestagtes Golden kommt nicht ohne begründete Re-Pinierung im selben Commit in
die Historie. Kosten: kein GPU, kein Cargo, kein Build — reines `git diff` und
Textvergleich.

**Was das ausdrücklich _nicht_ bedeutet** (gemessen, nicht vermutet — siehe
§11.1): Commits, die **keinen** Pre-Commit-Hook ausführen, umgehen das Gate
ohne jede Absicht. Nachgemessen und bestätigt sind `git cherry-pick` (rc=0, keine
Hook-Ausgabe, `# Grund:` unverändert), `git revert` (rc=0), `git rebase` (keine
Hook-Ausgabe beim Replay) und `git commit-tree` (rc=0, reine Plumbing). **Nicht**
gemessen bzw. widerlegt ist `git merge --squash`: dort läuft der Hook beim
abschließenden `git commit` very wohl und lehnt ab (rc=1) — der Squash selbst
merkt nichts, der Commit danach schon. „Nur was einen Pre-Commit-Hook
ausführt, ist geprüft" ist deshalb die Formulierung; „keine Baseline ohne
Re-Pin in der Historie" wäre zu stark und wurde in Runde 1 der Verifikation zu
Recht zurückgewiesen. Was als Restschutz **bleibt**: `goldens.digest` und
`goldens.count` machen jedes spätere `check` rot, sobald ein Golden ohne
passenden Pin committet wurde (§3.1, §9.3) — ein einsames, über
`cherry-pick`/`revert`/`rebase` committetes Golden ist also nicht *stumm*, nur
nicht im Moment seines Entstehens blockiert.

> **Beide Seiten der Index-Klausel sind getestet.** „Vergleich `HEAD:` gegen
> den **Index**, nicht gegen die Arbeitskopie" (Punkt 3) hat zwei Seiten, und
> beide sind Matrixzeilen in §4.2: `index-differs-from-worktree` (Goldenseite:
> die Änderung steckt nur im Index) und
> `lock-index-differs-from-worktree` (Lockseite: die `# Grund:`-Zeile steckt
> nur im Index, während die Arbeitskopie eine andere behauptet). Gemessen: der
> Hook las vorher nur die Goldenseite — ein Hook, der `git show ":$LOCK_PATH"`
> durch `cat "$repo_root/$LOCK_PATH"` ersetzt, blieb bei 180/180 grün.

**Installation — nach jedem frischen Clone einmal auszuführen.**

```sh
git config core.hooksPath .githooks
```

`core.hooksPath` ist **lokale** Git-Konfiguration und damit **nicht** Teil des
Repositories: `git clone` setzt sie **nicht**, auch der CI-Runner nicht. Ein
neuer Checkout ist also per Default **ungeschützt**, bis die Zeile oben gelaufen
ist. Das ist eine bewusst in Kauf genommene Lücke (ein Hook lässt sich nicht per
`clone` mitliefern), aber sie muss benannt und nicht versteckt werden. Als
Mindestmaßnahme prüft `scripts/golden_ref_test.sh`, dass `.githooks/pre-commit`
**ausführbar committet** ist — geht dieses Bit verloren, ist das Gate für jeden
installierten Clone lautlos weg.

**Ist der Hook nicht installiert**, gilt: das Pre-Commit-Gate läuft nicht. Es
gibt dann **keinen** mechanischen Schutz — die verbleibenden Schranken sind
`check` vor dem Lauf (§8.3) und die menschliche Prüfung des Golden-Diffs im
Review. Das ist eine bewusst in Kauf genommene Lücke, keine Formalie.

> **Kein `--diff-filter` in der Erkennung.** Das Gate ruft
> `git diff --cached --name-only -- <glob>` **ohne** `--diff-filter` auf. Grund:
> mit git 2.54 lieferte `--diff-filter=ACDR` bei einer **geänderten** Datei im
> `--name-only`-Output **nichts** — ein frisch regeneriertes Golden wäre
> spurlos durch das Gate gelaufen. Der Glob-Pfadespec allein erfasst A/M/D/R
> (und überschreitet dabei `/`, deckt also auch Unterordner ab).

> **Fail-closed auch bei kaputtem Index.** Der Hook wertet einen **fehlgeschlagenen**
> `git diff --cached` ausdrücklich als **Verweigerung**, nicht als „nichts
> gestaged": ein leeres Diff bei gleichzeitigem git-Fehler ist ein Fehler, kein
> Freipass. Sonst wäre die Überschrift „FAILS CLOSED" nicht gedeckt.

### 4.2 Regressions-Suite des Wächters: `scripts/golden_ref_test.sh`

Der Wächter ist Shell-Code; sein gesamter Vertrag ist „Exit-Code plus
Verweigerungstext". Eine Rust-Testfunktion könnte nur die Shell-Semantik
nachbauen, die sie prüfen soll. Der Vertrag hat darum eine eigene,
**committete** Shell-Suite:

```sh
sh scripts/golden_ref_test.sh
```

Sie läuft in der `docs`-CI-Stufe auf einem nackten `ubuntu-latest` (kein GPU,
kein macOS) und ist plattformunabhängig: sie behauptet **nie**, dass die
ausführende Maschine die Referenzplattform ist, sondern pinnt zuerst einen
synthetischen Lock aus der laufenden Umgebung und prüft gegen diesen.

Abgedeckt (201 Prüfungen, Exit 0 = alles grün):

| Bereich | Inhalt |
| --- | --- |
| Schlüssel-Sweep | jede der **21** Schlüssel einzeln perturbiert → `check` Exit 1, und der Diff nennt **genau** diesen einen Schlüssel (kein Schlüssel ist unlasttragend) |
| Wertableiter | `png_size` und `ui.scale_factor` gegen **committete, handgeprüfte** Eingaben statt gegen sich selbst: IHDR-Probe `scripts/fixtures/png_ihdr_probe.png` mit literal erwartetem `16909060x84281096`, `ui.scale_factor` mit `1.0` bei passendem und `non-unit:<w>/<Viewport>,<h>/<Viewport>` bei abweichendem synthetischem Viewport — Details in §4.2.1 |
| Golden-Inventar-Ebene | die Schranke aus §3.1/§11.1 selbst, gegen eine **unabhängige** Ableitung: `goldens.count` = Zahl der committeten Golden-PNGs aus `git ls-files` (**66**, nicht 25), das vom Wächter enumerierte Set **pfadweise** gleich diesem, `goldens.digest` = der über alle committeten Goldens unabhängig berechnete Wert, `git:` als Modus bei vorhandenem git und `walk:` unter einem `git`-Shim, und die Aussage, dass der Modus **Teil des Wertes** ist — Details in §4.2.2 |
| Lock-Kanonik | 8 Varianten → Exit 1: angehängter Doppelschlüssel, umgestellte Reihenfolge, unbekannter Schlüssel, fehlender Schlüssel, Zeile ohne `=`, abschließendes Leerzeichen (dann über den **Wert**vergleich), ungültiges Schlüsselzeichen, leerer Schlüssel |
| Legitime Formen | 7 Formen → Exit 0: wie aufgezeichnet, nur Schlüsselzeilen, handgeschriebener Kopf, Kommentar mitten im Block, Leerzeilen, **CRLF** (siehe §3), Wert mit Leerzeichen **und** `=` |
| `record`-Verweigerungen | 9 Fälle → Exit 2 **und** keine Lock-Datei entstanden: kein `--confirm`, `--confirm` ohne Wert, leerer Grund, 19 Zeichen, LF, CR, CRLF, `--` im Grund, unerwartetes Argument |
| `record`-Annahmen | 20 Zeichen, Grund mit Leerzeichen, erneutes Aufzeichnen mit anderem Grund, `old -> new`-Diff **vor** dem Pin |
| `UPDATE_SNAPSHOTS` | 8 falsche Werte (`''`, `0`, `false`, `no`, `off` + Großschreibung) bleiben still, 6 wahre (`1`, `true`, `yes`, `on`, `force`, `garbage`) lösen die Verweigerung aus — jeweils über `gate` und mit der Zusicherung, dass das gated Kommando **nicht** gelaufen ist; auf passendem Pin läuft es |
| Pre-Commit-Matrix | 16 Fälle in einem Wegwerf-`git init`-Repo mit dem **echten** Hook (14 `mc_commit`-Zeilen + "nichts gestaged" + kaputter Index als Positivkontrolle): geändert/angelegt/gelöscht/umbenannt/in Unterordner, mit und ohne Lock, mit unverändertem, geändertem und fehlendem `# Grund:`, Index ≠ Arbeitskopie auf **beiden** Seiten (Goldenseite und Lockseite, §4.1), Lock ohne Golden, PNG außerhalb des Snapshot-Baums, nichts gestaged, kaputter Index |
| Sandbox-Disziplin | `scripts/golden_ref.lock` ist am Ende byte-identisch, der echte Git-Index unverändert, keine `golden_ref.lock.tmp.*` übrig — und **jeweils mit Vorbedingung**: der Before-/After-Wert muss ein `sha256` bzw. ein Tree-OID sein, sonst ist der Vergleich nicht aussagekräftig und die Prüfung wird rot statt grün (§4.2.2) |

### 4.2.1 Der eine Bruch in der Selbstkonsistenz — und warum er nötig war

Die Suite ist absichtlich **selbstkonsistent**: sie behauptet nie, dass die
ausführende Maschine die Referenz ist, sondern pinnt zuerst einen synthetischen
Lock aus der laufenden Umgebung und prüft dagegen. Der Preis ist messbar —
der Basiswert und der Erwartungswert entstehen beide aus **demselben** Code,
also verschiebt eine selbstkonsistente Änderung einer *Ableitung* beide Seiten
gleichzeitig. Gemessen wurde genau das an zwei Stellen, bevor die zugehörigen
Zeilen existierten:

| Mutation (nur die Ableitung, Produktionstest unverändert) | vorher | jetzt |
| --- | --- | --- |
| `png_size`: die beiden hohen Breitenbytes vertauscht | **180/180 grün** | 184/188, 4 rot |
| `ui.scale_factor`-Fall auf immer `1.0` festgenagelt | **180/180 grün** | 186/188, 2 rot (nur die `non-unit`-Zeilen; die `1.0`-Zeile bleibt grün — genau deshalb sind es zwei) |

**Suite-Stand beim Messen: 188 Prüfungen.** Die Zahlen in dieser Tabelle sind
also *historisch* und beziehen sich auf den Stand **vor** der Ergänzung in
§4.2.2; die Suite hat heute 201. Sie sind hier behalten, weil der Nachweis genau
darin liegt: zu diesem Zeitpunkt war die Mutation **grün**, und genau das war die
Lücke. (Die Tabelle in §4.2.2 zeigt dieselben Klassen auf dem aktuellen Stand
und ist dort mit dem Stand gekennzeichnet.)

Geschlossen wird das nicht durch eine Kopie der Ableitung im Test (eine Kopie
prüft sich selbst), sondern indem die Suite die **echte** `emit_fingerprint`
mit zwei ersetzten Eingaben fährt: dem committierten Golden-Inventar
ersetzt durch `scripts/fixtures/png_ihdr_probe.png` und dem logischen Viewport
(`VIEWPORT_W`/`VIEWPORT_H`). `scripts/golden_ref.sh` wird dafür **gesourct**,
nicht nachgebaut; der abgeleitete Repository-Root des Skripts wird dabei
geprüft statt vorausgesetzt, weil er aus `$0` kommt.

Die Fixture ist eine Datei aus einem **literalen Byte-List**-Kommando, nicht aus
`png_size` erzeugt; IHDR-Bytes `01 02 03 04 05 06 07 08` stehen im Kommentar
und in `scripts/fixtures/README.md`, unabhängig bestätigt durch `file`
(„PNG image data, 16909060 x 84281096"). Die Dimensionen sind bewusst
unrealistisch groß, damit **jedes** der vier Bytes je Dimension lasttragend ist:
ein vertauschtes Bytepaar, eine vertauschte Hälfte, ein Little-Endian-Lesen
und ein auf zwei Bytes gekürztes Lesen liefern alle einen anderen String.

Die Suite schreibt **nie** in das echte Repository: alle Locks liegen in einem
`mktemp -d`-Sandbox-Verzeichnis **außerhalb** des Repos (über die
dokumentierte Test-Override `GOLDEN_REF_LOCK`), das Pre-Commit-Gate läuft in
einem Wegwerf-Repo, und ein `trap` räumt den Sandbox bei Erfolg, Fehler und
Abbruch auf. Laufzeit auf einem echten Checkout: grob **eineinhalb bis zwei
Minuten** (jeder `check`/`record`-Aufruf erfasst den Fingerabdruck neu und
läuft `dd|od|awk` über 66 Goldens; die drei zusätzlichen Läufe aus §4.2.1
kosten zusammen wenige Sekunden, weil dort nur das Golden-Inventar
ausgetauscht wird).

### 4.2.2 Der zweite Bruch: die Golden-Inventar-Ebene war nicht verankert

§4.2.1 hat den ersten selbstkonsistenten Blindfleck geschlossen (die beiden
*Ableitungen* `png_size` und `ui.scale_factor`). Der zweite war die Schicht, die
§3.1 und §11.1 als **dauerhaften Restschutz** benennen: `goldens.count` und
`goldens.digest`. Sie hatte im Wächter **keinen** Anker, und
`feature/quality/golden-fixtures.md` §4 sagt dasselbe von der anderen Seite —
"`golden_ref.sh check` vergleicht Digests, nicht diese Tabelle — eine fehlende
Zeile fällt dort nicht auf".

Gemessen wurde der Blindfleck an drei Mutationen, jede einzeln auf
`scripts/golden_ref.sh` angewandt und die Suite gefahren. Alle drei ließen
**alle 188 bis dahin grünen Prüfungen** grün, jede wird jetzt gefangen:

| Mutation (nur die Inventar-Ebene, Produktionstest unverändert) | vorher | jetzt (Suite-Stand 201) |
| --- | --- | --- |
| `list_goldens` meldet nur die `develop_*`-Teilmenge (25 von 66) | **188/188 grün** | 198 grün, 3 rot: `goldens.count`, enumeriertes Set, `goldens.digest` |
| `goldens.digest` hasht nur das **erste** Golden | **188/188 grün** | 200 grün, 1 rot: `goldens.digest` |
| Digest-Modus auf `walk:` festgenagelt, obwohl git vorhanden ist | **188/188 grün** | 199 grün, 2 rot: Moduswahl `git:` und „Modus ist Teil des Wertes" |

Geschlossen wird das wie in §4.2.1 **nicht** mit einer Kopie der Ableitung im
Test, sondern indem die Suite die **echte** `emit_fingerprint` **ohne jeden
Ersatz** fährt — diesmal ist `list_goldens`/`digest_goldens` selbst der
Gegenstand, also wird nichts überschrieben. Die Erwartungen kommen von außerhalb
des Wächters und je für sich:

- `goldens.count` gleich der Zahl der committeten Golden-PNGs aus
  `git ls-files --cached -- 'crates/lumina-gui/tests/snapshots/*.png'`
  (gemessen: **66**);
- das vom Wächter **enumerierte** Set **pfadweise** gleich dieser Liste — nicht
  nur gleicher Anzahl: eine Liste kann richtig lang sein und trotzdem ein Golden
  gegen eine Datei eingetauscht haben, und genau das ist der „eine Zeile fehlt
  in der Klassifikationstabelle"-Fall, den `check` nicht sieht;
- `goldens.digest` gleich dem über **alle** committeten Goldens unabhängig
  berechneten Wert im dokumentierten Format `<modus>:<sha256>` über
  `<pfad> <sha256>`-Zeilen. Das ist der einzige Anker, der einen Digest über
  *ein* Golden von einem über *alle* unterscheidbar macht — der Rows 1 der
  Tabelle.
- der Modus: `git:` bei vorhandenem git, `walk:` unter einem `git`-Shim, der
  **nur** die Enumeration unbrauchbar macht (der dokumentierte Auslöser aus
  §3.1), und die Aussage, dass der Modus **Teil des Wertes** ist. Beide Richtungen
  werden geprüft, damit kein Modus festgenagelt sein kann.

**Was diese Prüfungen nicht behaupten.** In diesem Checkout enumerieren beide
Modi dieselben 66 Dateien, `git:` und `walk:` unterscheiden sich also allein im
Präfix — die Aussage ist damit „der Modus steckt im Wert", nicht „die Modi sehen
verschiedene Dateien". Und die `walk:`-Enumerierung wird gegen eine
`find`-Ableitung geprüft, die die dokumentierten Artefakt-Ausschlüsse
(`*.new.png` / `*.diff.png` / `*.old.png`) anwendet; dieser Checkout trägt solche
Artefakte tatsächlich auf der Platte, die also lasttragend sind.

**Zwei Vorbedingungen, die vorher fehlten (DoD §10).** Die beiden
„nichts wurde angefasst"-Wächter am Suite-Ende verglichen
`git write-tree … || echo "no-index"` und einen `sha_of`-Wert, der den Fehler
schluckt und **nichts** ausgibt. Gemessen: außerhalb eines Git-Arbeitsbaums war
beides konstant, also verglichen beide Wächter eine Konstante mit sich selbst
und meldeten Erfolg, ohne je etwas gelesen zu haben. Beide Wächter bleiben
unverändert (sie verhindern, dass jemand diese Suite für den Nachweis hält, sie
habe selbst nichts angefasst); **ergänzt** ist je eine Vorbedingung — der
Before-Wert muss ein `sha256`, der Index-OID muss ein Tree-OID sein. Ein kaputtes
git oder ein fehlender Lock macht die Suite damit **rot** statt still grün;
nachgemessen über einen `git`-Shim, der ausschließlich `write-tree` scheitern
lässt (2 rot, beide Vorbedingungen), während der unveränderte Wächter daneben
weiterhin „ok" meldet.

---

## 5. `UPDATE_SNAPSHOTS`-Sperre (Geltung: `check` und `gate`)

> **Verbindlich, aber nur für Aufrufe durch dieses Skript.** Ist
> `UPDATE_SNAPSHOTS` in der Umgebung gesetzt (alles außer leer, `0`, `false`,
> `no`, `off`), dann führen **`check` und `gate`** bei Abweichung vom Pin
> **kein** Kommando aus, **schreiben kein** Golden und beenden mit Exit-Code 1
> plus klarer Meldung. Der Aufruf endet, **bevor** Cargo startet.
>
> **Was das ausdrücklich _nicht_ ist:** Das Skript setzt, löscht oder
> kontrolliert `UPDATE_SNAPSHOTS` nicht. Es verweigert nur, **sein eigenes**
> `check`/`gate` unter diesen Bedingungen fortzusetzen. Ein Aufruf, der das
> Skript umgeht, wird nicht erfasst — siehe §4.1 (Pre-Commit-Gate) und §11.1
> (die Umgehung im Klartext).

Grund: `UPDATE_SNAPSHOTS=1` überschreibt committete Referenzbilder. Auf einer
ungepinnten Maschine erzeugt das kein gültiges Golden, sondern eine neue,
unbenannte Baseline — genau der Mechanismus, der den fehlschlagenden Golden-Lauf
immer wieder neu erzeugt hat. Der Besitzer entscheidet stattdessen ausdrücklich
zwischen:

- **beabsichtigt auf der Referenzplattform:** `check` besteht, `UPDATE_SNAPSHOTS`
  darf laufen, anschließend ist `goldens.digest` veraltet und ein `record
  --confirm "…"` folgt;
- **auf einer Fremdplattform / absichtlich geänderte Referenz:** `record
  --confirm "<begründung>"` zuerst, dann der Lauf. Der Grund landet als
  Kommentarzeile im Lock und damit in der Git-Historie.

`print` bleibt bewusst ausgenommen: die Diagnose muss auch dann funktionieren,
wenn jemand den Zustand untersuchen will.

---

## 6. Fixture-Set der Goldens

Die Goldens hängen an genau diesen Eingaben. Der Fingerabdruck pinnt die
committeten Dateien per SHA-256 (`fixtures.digest`), die zur Laufzeit erzeugten
Satzungen sind deterministisch und werden im Folgenden festgeschrieben.

### 6.1 Inventar

Die **exakte, maschinenprüfbare** Liste ist der Wert `fixtures.digest` in
`scripts/golden_ref.lock` (Modus `git:`) — er ist eine Funktion aus
(Pfad, Inhalt) über genau die Dateien, die `git ls-files --cached --others
--exclude-standard -- crates/lumina-gui/tests/fixtures` liefert. Diese
Definition ist stabil, obwohl sich der konkrete Dateibestand während der
Fixture-Arbeit (GOLDEN-FIXT-31) mehrfach verschiebt: jede Verschiebung ist ein
sichtbarer Digest-Wechsel und damit genau eine bewusste `record`-Entscheidung.

Zum Zeitpunkt der Aufnahme (2026-09-25, `fixtures.count=16`):

```text
fixtures/.gitignore                       (von GOLDEN-FIXT-31 ergänzt)
fixtures/generative/auto_fill.png          (64x64 RGBA, generatives Fill-Fixture)
fixtures/generative/photo.png              (4x3 RGBA, Quellbild der generativen Panels)
fixtures/library/.gitkeep                  (leerer Library-Raster, Pfadrelativität)
fixtures/library_badges/{top.arw, sub/mid.arw, sub/nested/deep.arw}
fixtures/library_rated/{rated.arw, rejected.arw, labeled.arw}
fixtures/library_stack/images/{a_stack1.arw, b_stack2.arw, z_solo.arw}
fixtures/library_views/{a01.arw, a02.arw, b01.arw}
```

Die `*.arw`-Dateien sind (bisher) RAW-Sentinel-Bytes (`lumina-raw-fixture`); ein
Verzeichnis-Scan dekodiert sie nicht. Grid-/Filmstrip-Zellen zeigen für sie den
deterministischen LibRaw-Fehllplatzhalter — deshalb pinnen mehrere Goldens
bewusst eine LibRaw-Fehlermeldung mit. GOLDEN-FIXT-31 ersetzt sie durch zur
Setup-Zeit gestagte, lizenzierte Canon-EOS-R1-CR3-Kopien aus `sample-data/raw/`
(same bytes, keine Doppelablage im Testbaum, gitignoriert — deshalb nicht im
Digest, siehe §3.1).

> **Stand des Fixtures-Digests 2026-09-26 (gemessen, nicht behauptet):** Die
> 12 `*.arw`-Sentinels **sind** committet — `git ls-files
> crates/lumina-gui/tests/fixtures/*.arw` liefert 0 Treffer, `git ls-files` über das
> fixtures-Verzeichnis zeigt 4 echte Formate (`.gitignore`, `library/.gitkeep`,
> zwei generative PNGs), und `fixtures.count=4` im Pin entspricht dem.
> Die Übergangsphase, in der die 12 Pfade als `absent` im Digest standen, wurde
> in `2e9827f` durch einen `record` **legal verabschiedet** — der Lock-Grund nennt
> `fixtures.count 16->4 aus GOLDEN-FIXT-31 (11 synthetische .arw entfernt)`.
>
> **Achtung, drei Zahlen für denselben Vorgang überleben im Repo:** 12 (dieser
> Abschnitt, korrekt), 11 (der Commit-Grund oben — die automatisch gemessene
> Anzahl, nicht die manuell gezählte), 9 (`Agents.todo.md`, Block F-3 der
> `GOLDEN-REF-30`-Task). Die Diskrepanz war nie aufgelöst; sie ist hier benannt.
> `GOLDEN-BASELINE-32` führt die endgültige Neumessung.
>
> Was damals als „einmal legitim, dann schlägt `check` fehl" formuliert wurde, ist
> damit **erledigt**; die Aussage war als *Plan* korrekt, als Beschreibung des
> Zustands am 2026-09-26 ist sie falsch und wird hier zurückgenommen.

### 6.2 Zur Laufzeit deterministisch erzeugte Fixtures

| Fixture | Erzeugung | Zweck |
| --- | --- | --- |
| `library_badges/**` | bei jedem Lauf neu aufgebaut, Preview-Cache vorher entfernt | Unterordner-Badges |
| `library_rated/*` + Sidecars | neu aufgebaut, Cache vorher entfernt, **keine** Standard-Previews (s. Korrektur 2026-09-26) | Rating/Flag/Color-Label-Badges |
| `library_views/*` + `.lumina/`-Cache | neu aufgebaut, `vc-original`-Standardpreviews **nicht** gesät (s. Korrektur 2026-09-26) | Loupe/Compare/Survey mit echten Thumbnail-Pixeln |
| lizenzierte CR3-Kopien aus `sample-data/raw/` (GOLDEN-FIXT-31) | zur Setup-Zeit in die Fixture-Unterordner kopiert, gitignoriert, pro Lauf identisch | echte RAW-Dekodierung statt Platzhalter |
| `photo.png` / `sample.png` in einem `tempfile::tempdir` | `LuminaApp::sample_image_png()` (4 × 3 px RGBA, in `lib.rs` als Konstante) | Develop/Export/Overlay-Previews |
| `photo.jpg` + IPTC-JPEG bzw. Sidecar mit 10 Historienzeilen | echte JPEG-Bytes über den Projekt-Encoder, feste RFC-3339-Zeitstempel | Metadata-Subpanels |
| 20 Dummy-`IMG_0000..0019.ARW` | `tempfile::tempdir` | Filmstrip-Geometrie |

Wichtig: alle diese Pfade sind **relativ** bzw. tragen den Zufalls-Präfix
nie in gerenderte Pixel (Konvention aus den Tests: der Verzeichnisbaum zeigt
den relativen Fixture-Namen, ein `tempdir`-Pfad würde in Pfadfeld und
Ordnerbaum leaken).

### 6.3 Nicht Fixture, aber wirksam

- `build_harness()`: `.with_size([1024.0, 720.0]).wgpu()` — legt Viewport und
  Backend fest.
- `set_history_timestamp_override("2026-09-19T12:00:00Z")` — pinnt die
  Zeitstempelzeile im History-Golden.
- `set_meta_presets_dir(<leeres tempdir>)` — verhindert, dass maschinenglobale
  Preset-Namen in Goldens leaken (Ausnahme: `develop_section_presets` zeigt
  bewusst den maschinenglobalen Preset-Ordner — siehe Grenzen).

---

## 7. Golden-Inventar

66 committete Golden-PNGs in `crates/lumina-gui/tests/snapshots/`:

| Test-Target | Goldens |
| --- | --- |
| `crates/lumina-gui/tests/kittest_snapshots.rs` (56 Tests, davon 46 Golden-Vergleiche) | 46 |
| `crates/lumina-gui/tests/kittest_spot_tool.rs` | 3 |
| `crates/lumina-gui/tests/kittest_crop_overlay.rs` | 2 |
| `crates/lumina-gui/tests/kittest_mask_visibility.rs` | 1 |
| `crates/lumina-gui/tests/kittest_library_stack.rs` | 1 |
| `crates/lumina-gui/src/tests/brush_management.rs` (Lib-Target, `mask_management_controls_have_a_representative_kittest_golden`) | 1 |
| `crates/lumina-gui/tests/kittest_parity.rs` (`parity_paths_{scene}_{cpu,gpu}`, nur mit `--features gpu`) | 8 |
| `crates/lumina-gui/tests/kittest_mask_local.rs` | 4 |

Alle sind `#[ignore]`. Es gibt dabei **zwei** dokumentierte Gründe, nicht einen:
`"headless GPU required; …"` in den Integrationstest-Targets
(`kittest_snapshots`, `kittest_spot_tool`, `kittest_crop_overlay`,
`kittest_parity`, `kittest_library_stack`) und
`"native wgpu adapter required; …"` in `kittest_mask_visibility.rs` und im
Lib-Target (`brush_management.rs`). Der Dateiname je Golden ist der
`harness.snapshot(<name>)`-Name; die konkrete Liste steht in
`crates/lumina-gui/tests/snapshots/`.

> **Merke zur Zahlenangabe:** „56 Goldens" bezeichnet die **Tests** in
> `kittest_snapshots.rs`, nicht die Vergleichsanzahl. Vergleichsanzahl in
> diesem Target: 46. Über alle Targets: **66** (gezählt mit
> `git ls-files "crates/lumina-gui/tests/snapshots/*.png"`, identisch mit dem
> `goldens.count` aus `golden_ref.sh check`). Die in der Task-Notiz genannten
> „12 von 56" sind damit weder 56 noch 66 und müssen neu gezählt werden.
> Ein früherer Zwischenstand dieses Dokuments stützte sich auf 11 Paare
> `<name>.diff.png` / `<name>.new.png`, die zu einem GPU-Lauf im Arbeitsbaum
> lagen. Das ist **kein** belastbarer Beleg: diese Artefakte sind gitignoriert,
> werden bei jedem Lauf neu erzeugt und verschwinden wieder, ihre Anzahl
> schwankt mit jedem GPU-Lauf (im Prüfzeitraum zwischen 0 und 14). Verbindlich
> ist erst der **einmal gezählte** Lauf in `GOLDEN-BASELINE-32` auf der
> Referenzplattform; bis dahin wird „12 fehlschlagende Goldens" hier bewusst
> nicht als feststehende Zahl geführt.

---

## 8. Exakte Regenerations-Kommandozeile

`UPDATE_SNAPSHOTS=1` steht in allen Formen **außerhalb** des `gate --`, weil
sonst die Sperre aus §5 gar nicht erst greift: das Skript müsste die Variable
prüfen, während sie in der aufrufenden Shell noch gar nicht gesetzt ist.

### 8.1 Vollständiger Lauf aller GPU-Golden-Targets (inkl. Lib-Target)

```sh
UPDATE_SNAPSHOTS=1 sh scripts/golden_ref.sh gate -- sh -c '
  cargo test -p lumina-gui --test kittest_snapshots      -- --ignored &&
  cargo test -p lumina-gui --test kittest_spot_tool     -- --ignored &&
  cargo test -p lumina-gui --test kittest_crop_overlay  -- --ignored &&
  cargo test -p lumina-gui --test kittest_mask_visibility -- --ignored &&
  cargo test -p lumina-gui --test kittest_library_stack -- --ignored &&
  cargo test -p lumina-gui --lib mask_management_controls_have_a_representative_kittest_golden -- --ignored &&
  cargo test -p lumina-gui --features gpu --test kittest_parity -- --ignored
'
```

### 8.2 Einzelnes Target

```sh
UPDATE_SNAPSHOTS=1 sh scripts/golden_ref.sh gate -- \
  cargo test -p lumina-gui --test kittest_snapshots -- --ignored
```

Beide Formen setzen die Variable in der aufrufenden Shell, `gate` sieht sie
also, prüft sie gegen den Pin und führt erst danach `cargo` aus. Und selbst
wenn jemand auf die Variable verzichtet, greift die zweite Schranke: der
Pre-Commit-Gate aus §4.1 verlangt die begründete Re-Pinierung im selben
Commit.

### 8.3 Vergleichslauf ohne Neuschreiben

```sh
sh scripts/golden_ref.sh check &&
cargo test -p lumina-gui --test kittest_snapshots -- --ignored
```

Ein rotes Snapshot erzeugt `<name>.diff.png` neben dem Golden (gitignoriert).
Diese Datei ist **die** Begründungsgrundlage für jeden `record`-Lauf.

> **Zwei verschiedene Belege, nicht einer.** Der von `record` gedruckte
> `old -> new`-Diff ist der Beleg für die **Re-Pin** (welche Fingerabdruck-
> Schlüssel sich bewegt haben). Er ist **kein** Beleg für den **Inhalt**: der
> Wert `goldens.digest` ist ein einzelner Hash, aus dem nicht hervorgeht, *welches*
> Golden sich geändert hat. Der Inhaltsbeleg ist ausschließlich der visuelle
> Vergleich je Golden, also die `.diff.png` (bzw. der `git diff` der PNGs im
> Review).

### 8.4 Re-Pin nach beabsichtigter Neuregistrierung

```sh
# Der Grund nennt die Anzahl **nicht**, damit das Beispiel nicht beim nächsten
# Golden-Zuwachs veraltet; der Lock nennt `goldens.count` ohnehin selbst.
sh scripts/golden_ref.sh record --confirm "GOLDEN-BASELINE-32: <Plattform> / <Toolchain> / <Fixture-Set> neu erzeugt, <N> Abweichungen einzeln begruendet"
```

---

## 9. Mismatch-Politik

1. **Ein Mismatch bedeutet „bewusst neu aufnehmen", nicht „CI ist kaputt".**
   Es gibt kein CI, das diese Goldens prüft (§10). Ein Mismatch auf einer
   anderen Maschine ist eine korrekte, erwartete Aussage: „diese Maschine ist
   nicht die Referenz".
2. **Reihenfolge ist verbindlich:** erst `print`/`check` lesen, Diff
   verstehen, `*.diff.png` prüfen, dann entscheiden zwischen
   (a) Re-Plattform-Pin (`record --confirm`) und (b) Produktänderung
   zurücknehmen. Solange die UI-Änderung nicht beabsichtigt ist, darf der
   Mismatch **nicht** durch einen neuen Pin weggeräumt werden.
3. **Was das Hand-editieren des Locks tatsächlich verhindert — und was nicht.**
   `scripts/golden_ref.lock` ist unverschlüsselter Text. Der Schutz ist
   dreistufig, und keine der drei Stufen ist kryptografisch:

   | Stufe | Was sie verhindert | Was sie **nicht** verhindert |
   | --- | --- | --- |
   | kanonische Form (`lock_is_canonical`, §3) | angehängte, doppelte, umgestellte, unbekannte oder unlesbare Schlüsselzeilen — der angehängte `os.cpu=`-Zeilentrick aus der Verifikation fällt hier durch | eine vollständige, in kanonischer Form gehaltene Handbearbeitung aller Werte |
   | Pre-Commit-Gate (§4.1) | ein Golden im selben Commit **ohne** `# Grund:`-Änderung | ein Golden mit einer selbst geschriebenen, aber überzeugend klingenden Begründung |
   | `goldens.digest` / `fixtures.digest` | eine Baseline, die zu den committeten Bytes passt | eine gleichzeitige Neubaseline, die danach wieder passt |

   Wer den Lock vollständig von Hand in kanonischer Form umschreibt, ist
   gegenüber `check` nicht von einem `record` zu unterscheiden. Genau das ist
   der Grund, weshalb §4.1 (Begründung im selben Commit) und das Review des
   Golden-Diffs die eigentlichen Schranken sind — nicht der Zahlenvergleich.
4. **Jedes `record` ist ein eigener, begründeter Commit** mit der Begründung
   als Kommentarzeile im Lock. Der Build-Agent committet; ein Implementierungs-
   Agent committet nie (`Agents.md`).
5. **Keine stillen Anpassungen.** Es gibt keine Toleranz, keinen
   „close enough"-Modus und kein Feld, das bei Abweichung stillschweigend
   übersprungen wird.
6. **`wgpu.backend_env` und `font.resolution` sind Absichtsschalter.** Wer
   `LUMINA_GPU_BACKENDS` setzt oder eine Font-Umleitung einbaut, verändert die
   Golden-Bedingungen; das Skript macht das sichtbar, statt es zu übergehen.

---

## 10. CI-Politik: lokal, nie CI

> **Verbindlich (User-Entscheidung 2026-09-25).** Die Goldens sind ein
> **lokales macOS-Gate**. Sie werden in CI **nie** verifiziert.

Begründung: GitHub-Actions-Runner haben keinen GPU-Zugriff (kein
Metal-Compute), alle `egui_kittest`-Tests sind deshalb `#[ignore]`, und der
`lumina-gpu`-Pfad kann in CI nur kompiliert, nicht ausgeführt werden
(`Agents.md`, `feature/platform/capability-matrix.md`). Ein CI-Job, der die
Goldens „prüft", würde entweder dauerhaft grün sein, ohne etwas zu prüfen, oder
bei jedem Runner-Drift rot werden und als Produktfehler fehlgedeutet.

Daraus folgt:

1. **Kein CI-Gate auf den Golden-Vergleich.** `cargo test -p lumina-gui` ohne
   `--ignored` bleibt in CI das einzige GUI-Gate; es ist grün ohne GPU.
2. **Kein CI-Job ruft `scripts/golden_ref.sh check` als Produkt-Gate.** Der
   Guard ist eine lokale Hilfsschicht für die Entwickler-Maschine.
3. **CI darf `scripts/check_file_sizes.sh` weiterhin erzwingen** — der
   Fingerabdruck selbst ist eine reine lokale Sache.
4. Jeder Merge eines geänderten Goldens muss im Abschlussbericht die
   `record`-Begründung und die Diff-Belege nennen. Zusammen mit §4.1 ist das die
   einzige Review-Schranke, die es für Goldens gibt.

---

## 11. Bekannte Grenzen

### 11.1 Der Guard ist nicht automatisch — die Umgehung im Klartext

> **Das hier ist die wichtigste Grenze dieses Dokuments.**
>
> `scripts/golden_ref.sh` ist **kein** transparenter Guard. Es ist ein Skript,
> das man **aufruft**. Der natürlichste Befehl eines Entwicklers,
>
> ```sh
> UPDATE_SNAPSHOTS=1 cargo test -p lumina-gui -- --ignored
> ```
>
> **umgeht den gesamten Mechanismus vollständig**: Das Skript läuft nicht, wird
> nicht aufgerufen, und die Umgebungsvariable wird ohne Beteiligung des
> Skripts gesetzt. Es gibt keine `.cargo/config`-Alias, kein `Makefile`, kein
> `justfile` und keinen Wrapper, der den Aufruf abfangen würde.
>
> Was danach dennoch greift:
>
> 1. das **Pre-Commit-Gate** (§4.1) — es prüft nicht den Aufruf, sondern das
>    Ergebnis: ein gestagtes Golden ohne `# Grund:`-Änderung im selben Commit
>    wird abgewiesen. Voraussetzung: `core.hooksPath` zeigt auf `.githooks`
>    (in diesem Repo gesetzt). Fehlt der Hook, entfällt diese Schranke
>    vollständig.
> 2. die **menschliche Prüfung** des `<name>.diff.png` im Review bzw. im
>    Abschlussbericht.
>
> Und das ist **nicht** alles. Es gibt drei Klassen, die das Gate umgehen.
>
> **Klasse 1 — beabsichtigt.** `git commit --no-verify`; das vollständige
> Hand-editieren des Locks in kanonischer Form (§9.3); und der Blick auf den
> Golden-Diff wird unterlassen.
>
> **Klasse 2 — unbeabsichtigt, weil git gar keinen Pre-Commit-Hook ausführt.**
> Das ist die Klasse, die man nicht auf dem Schirm hat, weil sie aussieht wie
> ganz normale Git-Arbeit. Nachgemessen in einem Wegwerf-Repo mit dem echten
> Hook:
>
> | Befehl | gemessen |
> | --- | --- |
> | `git cherry-pick <Golden-only-Commit>` | `rc=0`, **keine** Hook-Ausgabe, `# Grund:` unverändert |
> | `git revert <Golden+Re-Pin-Commit>` | `rc=0`, **keine** Hook-Ausgabe |
> | `git rebase <Replay>` | `rc=0`, beim Replay keine Hook-Ausgabe |
> | `git commit-tree <tree> -p HEAD -m …` | `rc=0`, keine Hook-Ausgabe (reine Plumbing) |
> | `git am <Golden-Patch>` | `rc=0`, **keine** Hook-Ausgabe (läuft `pre-applypatch`/`post-applypatch`, nicht `pre-commit`) |
> | `git merge <Branch>` (Fast-Forward **und** Merge-Commit) | `rc=0`, **keine** Hook-Ausgabe — git führt `pre-merge-commit` aus, und `.githooks/` hat **keinen** `pre-merge-commit`. Gemessen: das HEAD-Golden wird dabei still neu geschrieben, `# Grund:` bleibt unverändert. |
> | `git merge --squash <Branch>` | **kein** Bypass: der abschließende `git commit` läuft durch den Hook und wird abgewiesen (`rc=1`) |
>
> Ein Revert, ein Cherry-Pick, ein Rebase, ein `git am` oder ein **`git merge`**
> sind die normalsten Fehlerbehebungen bzw. Arbeitsschritte der Welt — und ein
> Merge ist noch normaler als ein Cherry-Pick. Niemand denkt dabei an einen
> Golden-Pin, und genau deshalb ist das eine **unbeabsichtigte** Lücke und keine
> Formalie.
>
> **Zwei dieser Lücken sind ohne Zusatzaufwand schließbar**, wenn das gewollt
> wird: `.githooks/pre-merge-commit` würde `git merge` fangen, und eine
> `post-applypatch`-Prüfung würde `git am` fangen. Beides ist **bewusst nicht
> umgesetzt** — der Wächter soll kein zweiter Mechanismus mit eigener
> Fehlermode-Logik werden, und der dauerhafte Restschutz ist `goldens.digest`.
> Der Preis dieser Entscheidung ist oben benannt, nicht weggeredet.
>
> **Was in Klasse 2 als Restschutz bleibt:** `goldens.digest` und
> `goldens.count` (Fingerabdruck, §3) machen jedes **spätere** `check` rot, weil
> die committeten Bytes nicht mehr zum Pin passen. Ein so committetes Golden ist
> also nicht still — es fällt spätestens beim nächsten `check` auf. Was fehlt,
> ist die Zuordnung *zu diesem einen Commit*: die Schranke sieht „die Baseline
> ist veraltet", nicht „dieser Commit hat sie verändert".
>
> **Klasse 3 — der Hook ist gar nicht installiert.** `core.hooksPath` ist lokale
> Git-Konfiguration und überlebt kein `git clone` (§4.1). Jeder frische Checkout
> — auch der CI-Runner — ist bis zur einmaligen
> `git config core.hooksPath .githooks`-Zeile ungeschützt. Auch hier bleibt
> `goldens.digest` die Schranke, aber erst beim nächsten `check`.
>
> Ein Entwickler, der den Hook deaktiviert, den Lock vollständig von Hand in
> kanonischer Form umschreibt (§9.3) **und** den Diff nicht ansieht, hinterlässt
> keine Spur in der Historie, die dieses Werkzeug aufdecken könnte. **Der Wert
> dieses Dokuments ist die Benennung und Prüfbarkeit der Referenzplattform, nicht
> die Unmöglichkeit einer Umgehung.**

### 11.2 Weitere Grenzen

- **Kein automatischer CI-Ersatz.** Solange es keinen GPU-Runner gibt, bleibt
  das lokale Gate unersetzt. Das ist bewusst in Kauf genommen und hier als
  Lücke benannt.
- **Die Recoverability des Locks hängt daran, dass er committet ist.** Solange
  `scripts/golden_ref.lock` untracked ist, existiert der Pin nur im Arbeitsbaum
  und geht bei einem Branch-Wechsel verloren. Der Pin muss **committet**
  werden, damit die Referenzplattform überhaupt aus dem Repo lesbar ist.
- **`gpu.adapter` ist eine OS-erfasste Metal-Geräteidentität, nicht der
  wgpu-`AdapterInfo`-Debug-String.** wgpu 30.0.1 gibt diesen String nicht aus
  (`wgpu`/`wgpu-core` loggen den Adapter nicht, und das Harness legt ihn nicht
  ab). Erfasst werden Gerät, Metal-Version und wgpu-Crate-Version — das sind
  die Größen, die das Rasterergebnis bestimmen. Ein tiefer Adapter-Probe ist
  bewusst nicht ergänzt: Er würde ein zusätzliches Rust-Target unter
  `crates/lumina-gui/**` brauchen, das außerhalb des Auftragsumfangs von
  `GOLDEN-REF-30` liegt.
- **`develop_section_presets` ist maschinenhalbspezifisch.** Der Preset-Abschnitt
  zeigt den maschinenglobalen Preset-Ordner; ein Ordnerinhalt verändert die
  Pixels. Die Pfadzeile selbst ist maschinenunabhängig. Eine spätere
  Festlegung auf ein committetes Preset-Verzeichnis ist als Folgeaufgabe zu
  behandeln, nicht als Teil von `GOLDEN-REF-30`.
- **Fixture-Digest umfasst nur committete Dateien.** Ein zur Laufzeit erzeugtes
  und danach manuell verändertes Fixture (z. B. `.lumina/`-Cache) ist nicht
  abgedeckt; die Erzeugung ist in §6.2 beschrieben und deterministisch.
- **Ein macOS-Punktupdate erzwingt ein Re-Pin.** `os.macos` ist die volle
  Produktversion. Das ist gewollt: ein OS-Update kann Treiber- und
  Text-Rendering ändern, und diese Änderung soll sichtbar eine Entscheidung
  erzwingen, nicht still durchrutschen.
- **`record` überschreibt den Lock ohne Rückfrage.** Deshalb der Pflichtgrund
  und die Warnung bei abweichendem Lock-Pfad. Die Git-Historie ist die
  eigentliche Revisionssicherheit.
- **`system_profiler` ist lokalisiert.** `gpu.adapter` und `gpu.metal` werden
  über die englischen Schlüssel `Chipset Model:` / `Metal Support:` gelesen. Auf
  einem lokalisierten System ergeben beide Felder `unavailable` — das ist ein
  **lauter** Mismatch, kein stiller Treffer. Für eine deutsche macOS-Referenz
  müssten die Schlüssel ergänzt (nicht geraten) werden; das ist eine bewusst
  offene Grenze, keine stillschweigende Lücke.
