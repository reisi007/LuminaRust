# Capability-Matrix: native CLI / Desktop (WASM ENTFERNT 2026-09-04)

> WASM/Browser ist ersatzlos gestrichen (Eigentümer-Entscheidung 2026-09-04,
> F-069…F-071 entfallen). Diese Matrix beschreibt nur noch native CLI und
> Desktop. Der Ausbau aus dem Code läuft unter WASM-REMOVE-GUI/-ONNX/-REST.

**Features:** F-006 Capability-Matrix (native CLI, Desktop)

Diese Matrix dokumentiert plattformabhängige Fähigkeiten getrennt nach nativem
CLI und nativer Desktop-GUI.

| Fähigkeit | native CLI | Desktop (eframe) |
| --- | --- | --- |
| Raster (PNG/JPEG/WebP) laden | ja | ja |
| Raster entwickeln (Exposure/Kontrast/Highlights/Shadows) | ja | ja |
| Vorschau / Histogramm | nein (Headless) | ja |
| RAW dekodieren (LibRaw, nativ) | **ja (MVP)** | **ja (MVP)** |
| RAW-Datei per Pfad/Drag-and-Drop öffnen | ja | ja |
| Automatische Linsen-/Vignettierungskorrektur (liblensfun, `lumina-lensfun`) | ja (`--features lensfun`, benötigt installierte Profil-DB) | ja (gleiche Bibliothek) |
| Auto-Tone / Match Total Exposure | ja | ja |
| Virtuelle Kopien / Presets | ja | ja |
| Sidecar schreiben (nativ, neben Original) | ja | ja |
| ONNX-Inferenz (BiRefNet/SAM2) | ja (MVP) | ja (MVP) |
| KI-Denoise (lokales ONNX, `denoise`) | **ja, nativ; `pending-integration` → `unavailable` (F-078-Gate)** | **ja, nativ; Panel/Status, `pending-integration` → `unavailable` (F-078-Gate)** |
| Persistente AI-Masken | post-MVP | post-MVP |
| Export (PNG/JPEG/WebP) | ja | ja |
| IPTC-/XMP-Metadaten in JPEG-Exporte (`--write-metadata`, Opt-in) | ja (LRPAR-G15-IPTC) | ja (Metadaten-Panel) |
| HDR-Merge → lineares DNG (`merge-hdr`, LRPAR-G13-MERGE-15) | ja (nativ, `lumina-merge` + DNG-Writer, 1.5) | geplant 1.5 (gleicher Einstiegspunkt, Jobsteuerung in GUI) |
| Panorama-Merge → lineares DNG (`merge-pano`, LRPAR-G13-MERGE-15) | ja (nativ, gleicher Scope wie oben) | geplant 1.5 |
| KI-Culling Stufe 1 (Heuristik, `lumina-cull`, LRPAR-G09-CULL-25) | implementiert (2.5, nativ, deterministisch, kein Modell) | implementiert (2.5, Library-Badges, gleiche Logik) |
| KI-Culling Stufe 2 (ONNX-Modell, LRPAR-G09-CULL-25) | nicht geplant — nur mit F-078-Lizenzentscheid | nicht geplant |
| Optionale zentrale Indizierung (`lumina-index`) | post-MVP (optional) | post-MVP (optional) |

### Culling-Quellenidentität (LRPAR-G09-CULL-IMPL-25)

CLI-Status und explizite Culling-Übernahme vergleichen stets Hash und
Byte-Länge der aktuell gelesenen Quellbytes mit `SidecarDocument.source`.
Ein Konflikt ist in beiden Oberflächen sichtbar und verhindert jeden
Sidecar-Write byte-identisch; weder `--force` noch eine Batch-Operation
umgehen diese Schranke pro betroffenem Element. Culling-Writes bleiben auf
den `culling`-Vorschlag begrenzt und mutieren insbesondere weder Rating,
Flag, Label noch Rezept.

## RAW-Backend (nativ)

- Der native LibRaw-Adapter (`lumina-raw`) liefert `decode_bytes` /
  `RawMetadata` für CLI/Desktop.

## Lensfun-Profil-Datenbank (plattformabhängige Auflösung, LENSFUN-DB-33)

**Features:** F-098-N1 (native Linsenkorrektur), LENSFUN-DB-33 (portable
DB-Auflösung)

Die automatische Distortion-/Vignetting-/TCA-Korrektur braucht die
XML-Profil-Datenbank des installierten `liblensfun`. Deren Ort ist
**nicht** plattformunabhängig und lässt sich in lensfun 0.3.4 **nicht** zur
Laufzeit umbiegen: die Bibliothek kennt keine Umgebungsvariable für ihre
Systemverzeichnisse (kein `LF_DATA_PATH`), sondern benutzt den zur Bauzeit
kompilierten `LENSFUN_DATADIR` der C-Bibliothek (`/usr/share/lensfun` auf
Linux/Distro-Paketen, `/opt/homebrew/share/lensfun` auf Homebrew/Apple
Silicon, `/usr/local/share/lensfun` auf Homebrew/Intel macOS). `lumina-lensfun`
löst das deshalb **selbst** auf — portabel, geprüft und auditierbar.

### Verbindliche Auflösungsreihenfolge

`LensfunDb::resolve_system_with()` (Implementierung:
`crates/lumina-lensfun/src/db_path.rs`) prüft genau diese Reihenfolge und nimmt
die **erste** Quelle, die eine Profil-Datenbank enthält:

| # | Quelle | Herkunft | Verhalten, wenn unbrauchbar |
| --- | --- | --- | --- |
| 1 | Explizite Umgebungs-Override `LUMINA_LENSFUN_DB` (**absolutes** Verzeichnis) | Laufzeit, Operator | **Harter, benannter Fehler** — *kein* stilles Weiterfallen auf Quelle 2–4: eine explizit gesetzte, aber defekte Absicht darf nie stillschweigend durch eine andere ersetzt werden. Zusätzlich **Fixierung**: die Override-Ebene *ist* die geladene Systemebene, kein Update-Paket kann sie verdrängen (siehe „Fixierung und Verdrängung") |
| 2 | Kompiliertes `LUMINA_LENSFUN_COMPILED_DATADIR` | `build.rs` über `pkg-config --variable=datadir lensfun` (⇒ `…/share/lensfun`) | wird in der Reihenfolge übersprungen (Quelle 3 kann treffen) |
| 3 | Plattformabhängiger Default | `db_path::platform_default_dirs()` | wird in der Reihenfolge übersprungen |
| 4 | — | — | **Lauter, benannter Fehler** `SystemDbError` |

**Zur Zielabhängigkeit von Quelle 2 — bitte nicht überlesen:** die
pkg-config-Abfrage ist **nicht** zielabhängig, sondern **hostseitig**. Sie meldet
das `datadir` der lensfun-Installation, die diese *Build*-Maschine sieht. Für
einen nativen Build ist das korrekt; für ein **Cross-Compile** ist es falsch und
der Wert zeigt ins Leere. Zielabhängig ist ausschließlich der **Fallback**:
meldet pkg-config nichts (oder scheitert es), bäckt `build.rs` den konventionellen
Ort des **Ziel**-OS ein (`/opt/homebrew/share` bzw. `/usr/share`) und gibt
zusätzlich ein `cargo:warning` aus. Ein Cross-Build gegen eine fremd installierte
lensfun-Version ist damit **nicht** korrekt — eine bekannte Grenze dieser Quelle,
kein stiller Korrekturfall.

„Enthält eine Profil-Datenbank" heißt: das Verzeichnis enthält `version_1/`
mit **mindestens einer** `*.xml`-Datei. Ein vorhandenes, aber leeres
Verzeichnis gilt als „keine Datenbank" und wird übersprungen. Ein **relativer**
Override-Wert ist ein Konfigurationsfehler (`NotADirectory`): er würde gegen
das Arbeitsverzeichnis aufgelöst, und eine aus dem Finder gestartete App hat
cwd `/`. Ein vorhandenes, aber **nicht lesbares** Verzeichnis ist
`Unreadable` — **nicht** `Absent`, weil die Abhilfe eine andere ist
(Berechtigungen statt Installation).

Hinweis zur Rangfolge auf macOS: `pkg-config --variable=datadir lensfun`
liefert dort den **Cellar**-Pfad (`/opt/homebrew/Cellar/lensfun/0.3.4/share`),
also gewinnt **Quelle 2** vor Quelle 3. Quelle 3 (`/opt/homebrew/share/lensfun`,
ein Symlink-Ziel ohne Versionspin) greift nur, wenn pkg-config die Variable
nicht kennt. Auf Linux/Distro liefern Quelle 2 und 3 denselben Pfad
(`/usr/share/lensfun`) und werden dedupliziert.

### `SYSTEM_UPDATES_DIR` — eine hart verdrahtete Fremdquelle

`/var/lib/lensfun-updates` entspricht
`SYSTEM_DB_UPDATE_PATH "/${CMAKE_INSTALL_LOCALSTATEDIR}/lib/lensfun-updates"`
aus `include/lensfun/config.h.in.cmake` (lensfun 0.3.4) für einen
**Default-Prefix-Linux-Build**. Das ist ein **Bauzeit**-Wert der C-Bibliothek;
lensfun 0.3.4 bietet dafür **keine** Umgebungsvariable. Ein NixOS-, MacPorts-
oder Custom-Prefix-Build benutzt folglich einen *anderen* Pfad, den wir zur
Laufzeit nicht ermitteln können. Die Ebene bleibt Teil des Ladealgorithmus
(siehe unten), wird aber defensiv behandelt:

- Sie darf **nur mit einem echten, positiven `timestamp.txt`** *und* einem
  nicht-leeren `*.xml`-Satz gewinnen. Grund: der Tie-Break von upstream
  (`0 == 0` ⇒ System-Updates gewinnen) würde ein dateloses Verzeichnis dort eine
  reale Systemdatenbank verdrängen lassen — an einem Pfad, der zu diesem Build
  womöglich gar nicht gehört.
- Sie kann eine **Operator-Override nie verdrängen** (siehe unten).
- Fehlt sie (Normalfall auf macOS und auf den meisten Desktops), wird sie in
  `Resolved::skipped` als `Absent` festgehalten und **nicht** als Defekt
  gemeldet.

Verankert in `tests::db_layers::the_system_update_layer_needs_a_real_timestamp_txt`
und `…::an_update_package_without_xml_cannot_win`.

### Was geladen wird (Parität zu `lfDatabase::Load()`, lensfun 0.3.4)

`lf_db_load()` liest **mehr als ein** Verzeichnis. `lumina-lensfun` bildet das
ab und lädt den Plan explizit, sortiert, über `lf_db_load_file()`:

1. **Genau eines** der drei `version_1`-Verzeichnisse, und zwar nach dem
   Vergleich der drei `timestamp.txt`-Werte. Die „neuer"-Größe ist
   `_lf_read_database_timestamp` (lensfun 0.3.4,
   `libs/lensfun/auxfun.cpp`) — **nicht** ein Dateisystem-Zeitstempel:

   | Verzeichnis | Wert |
   | --- | --- |
   | existiert nicht / ist kein Verzeichnis / ist **leer** | `-1` |
   | existiert, ist nicht leer, `timestamp.txt` fehlt oder unlesbar | `0` |
   | existiert, ist nicht leer, `timestamp.txt` lesbar | der darin geparste UNIX-Sekunden-Wert |

   Der Vergleich ist wörtlich aus `lfDatabase::Load()` übernommen
   (`M` = `main_dirname`, `S` = system-updates, `U` = user-updates):

   ```text
   if (M > S) { if (U > M) UserUpdates else main }
   else       { if (U > S) UserUpdates else system_updates }
   ```

   Bei Gleichstand gilt also **`system-updates` > `main` > `user-updates`** — nicht
   die Iterationsreihenfolge. Verankert in
   `tests::db_layers::the_tie_break_follows_upstream_not_the_iteration_order`.
   Eine Ebene *ohne* `timestamp.txt` kann folglich nie gewinnen, solange die
   Systemdatenbank ein Datum trägt — verankert in
   `tests::db_layers::a_stale_update_package_without_a_timestamp_never_displaces_the_system_database`
   (der gemessene F1-Fall: alle 55 System-XML-Dateien fielen vorher weg).
   Ausführlich, mit dem Upstream-Quelltext und den gemessenen Zahlen:
   `crates/lumina-lensfun/src/db_timestamp.rs`.
2. **Danach unconditional** die **Benutzer-Datenbank**
   `HomeDataDir` = `$XDG_DATA_HOME/lensfun` bzw. `$HOME/.local/share/lensfun`
   — die `*.xml` liegen dort **direkt**, ohne `version_1`-Unterordner.

`HomeDataDir` folgt den **realen** glib-Regeln (glib 2.88.3,
`glib/gutils.c::g_build_user_data_dir` / `g_build_home_dir`): ein gesetztes,
nicht-leeres `XDG_DATA_HOME` gewinnt **ungeprüft, auch als relativer Wert**; ein
*gesetztes* `HOME` wird ebenfalls unverändert verwendet (leer ⇒ `.local/share`,
relativ ⇒ `<rel>/.local/share`), weil glib nur bei **nicht gesetztem** `HOME`
weitergeht. **Dokumentierte Abweichung:** bei nicht gesetztem `HOME` greift glib
auf die Passwd-Datenbank zurück; diese Crate ist dependency-frei und hat keinen
portablen Weg dorthin (macOS löst Home über Open Directory auf, nicht über
`/etc/passwd`), also bleibt die Benutzer-Ebene **weg** — aber nicht still: beide
Ebenen landen in `Resolved::skipped` mit `NoUserDataDir`, und das ist
meldepflichtig. Verankert in `tests::db_layers::home_data_dir_follows_glib`.

**Bewusst nicht mehr behauptet wird:** eine „Obermenge"- oder „identisch"-Parität
der geladenen Profilmenge. Sie ist nicht verankert und wäre es auch nicht, solange
ein Update-Paket die Systemebene verdrängen darf (dann ist die geladene Menge
eine echte Teilmenge). Belegbar ist ausschließlich: die Benutzer-Ebene wird
**immer** zusätzlich gemergt und geht dadurch nie verloren (F1) — verankert in
`tests::db_layers::the_user_database_is_merged_on_top_of_the_system_database`,
`…::an_empty_user_database_is_recorded_instead_of_being_dropped` und (real, auf
der installierten Datenbank) `tests::system_db::the_user_layer_decision_is_explicit_and_consistent_with_the_filesystem`.

### Fixierung und Verdrängung — eine Quelle der Wahrheit

`Resolved::dir` beschreibt, was die **Auflösung** gewählt hat (die Fixierung des
Operators). `Resolved::primary` beschreibt, was **geladen** wird, und
`Resolved::layers[0].origin` ist per Konstruktion dasselbe. Beide Felder
existieren, damit die Unterscheidung *benennbar* ist, statt zwei
unvereinbare Beschreibungen einer Entscheidung zu führen:

- **Fixiert** (`LUMINA_LENSFUN_DB` gesetzt): `primary == SystemSchema` **immer**.
  Kein Update-Paket kann eine Operator-Absicht verdrängen. Upstream kennt das
  Konzept „Override" gar nicht; das ist eine dokumentierte Abweichung zugunsten
  der Regel „explizite Absicht wird nie still ersetzt". Verankert in
  `tests::db_layers::an_operator_override_is_never_displaced_by_an_update_package`.
- **Quelle 2/3**: upstream-Parität — ein Update-Paket *darf* gewinnen. Die
  Abweichung zwischen „aufgelöst" und „geladen" wird dann aber über
  `Resolved::pin_honored()` bzw. `Diagnostics::pin_displaced` **gemeldet**, nicht
  verschwiegen. Verankert in
  `tests::db_layers::a_non_override_source_may_be_displaced_but_is_never_silent`.

**Dokumentierte weitere Abweichungen** von `lfDatabase::Load()`, jede gegen den
stillen Verlust von Profilen:

- Eine Ebene darf nur gewinnen, wenn sie **XML enthält**. Upstream wählt allein
  nach Zeitstempel; eine leere `version_1` kann dort gewinnen und dann aus
  **keinem** der drei Verzeichnisse etwas laden. Solche Kandidaten fliegen **vor**
  dem Vergleich raus (und werden mit `-1` verglichen) und werden als `NoXmlFiles`
  gemeldet.
- Sind **alle drei** Werte `-1` (Systemdatenbank ohne `timestamp.txt` und kein
  Update-Paket vorhanden), behält die aufgelöste Systemdatenbank die Ebene;
  upstream würde `system_updates_dirname` laden — also nichts. Verankert in
  `tests::db_layers::an_undated_system_database_with_no_update_package_still_loads`.

### Sichtbarkeit: keine stillen Fehler, keine rohen `eprintln!` pro Render

`lumina-lensfun` hat **keine Dependencies** und damit keinen Logger. Jedes
Ereignis geht deshalb über eine **vom Aufrufer gelieferte Senke**
(`system_load::Diagnostics`), die **fünf** benannte Ereignisse kennt:

| Methode | Stufe | Bedeutung |
| --- | --- | --- |
| `resolved` | Info | welche Ebenen mit wie vielen Dateien geladen wurden |
| `file_rejected` | Warn | **eine** Profildatei wurde übersprungen, DB bleibt nutzbar |
| `layer_skipped` | Warn | ein Verzeichnis, das **existiert**, ist leer/unlesbar/undatiert und deshalb nicht geladen |
| `pin_displaced` | Warn | die aufgelöste Datenbank wird **nicht** geladen; `Resolved::primary` sagt, welche stattdessen geladen wird |
| `failed` | Fehler | gar keine Datenbank |

Der Aufrufer entscheidet die **Stufe** (DoD §4) und das Routing. Für den
„einmal melden"-Fall liefert die Crate `system_load::report_once()` eine
langlebige, deduplizierende Senke; eine aus dem Finder gestartete GUI hat kein
`stderr`, und ein Cache-Miss wiederholt den Lookup, daher darf die Meldung
**nicht** pro Render erneut emittiert werden.

**`Resolved::skipped` ist vollständig, `layer_skipped` ist es nicht — und das ist
so gewollt.** Jede geprüfte, aber nicht geladene Ebene landet in
`Resolved::skipped` mit einem Grund (`Absent`, `NoXmlFiles`, `Unreadable`,
`NoTimestampFile`, `NoUserDataDir`); `resolve_system_with()` meldet daraus nur die
**handlungsrelevanten** Gründe an die Senke. `Absent` ist der Normalfall (macOS
hat kein System-Update-Paket, die meisten Nutzer keins) und würde sonst pro
Lookup nur Rauschen erzeugen. Verankert in
`tests::diagnostics::every_skip_reason_is_recorded_and_the_actionable_ones_reach_the_sink`
sowie `tests::db_layers::an_absent_user_database_is_recorded_but_not_actionable`.

#### Pflicht des Aufrufers von `load_system()` (F2 — offen)

`load_system()` (Legacy, von `lumina-gui` und `lumina-cli` genutzt) ist **nicht
mehr still**, auch nicht im Erfolgsfall: Erfolg, jede Abweichung und jeder
Fehlschlag erzeugen eine `stderr`-Zeile mit dem Präfix
`lumina-lensfun: LENSFUN-DB-33 <STUFE>:` (`INFO`/`WARNUNG`/`FEHLER`). Die Stufe
ist dabei ein **Textlabel, kein Routing** — die Crate hat keinen Logger, kann
keinen Datensatz an das Logging-System des Hosts geben und keine Stufe
durchsetzen.

Zwei Grenzen bleiben, und **beide** brauchen eine Änderung in `lumina-gui` /
`lumina-cli` (Dateien anderer Agenten, hier ausdrücklich **nicht** angefasst):

1. `load_system()` baut pro Aufruf eine **frische** `StderrDiagnostics`-Senke —
   es gibt also keinen Deduplizierungszustand. Die Zeilen wiederholen sich pro
   Lookup.
2. Eine aus dem **Finder** gestartete macOS-App hat **kein** `stderr`. Für diese
   Oberfläche bleibt der Datenbank-Ladevorgang damit unsichtbar, bis die
   Aufrufer auf `load_system_with(&mut <langlebige Senke>)` umgestellt sind und
   dort den Host-Logger füttern.

Bis dahin gilt: `None` aus `load_system_with` bedeutet **„Linsenkorrektur nicht
verfügbar"**, nicht „kein Profil passt" — und genau dieser Unterschied wird
gemeldet. Ein `None` ohne zusätzliche Auswertung der Senke bleibt in CLI/GUI
eine stille Degradierung, die mit dieser Änderung **nicht** behoben ist.

### Kein stiller Ersatz, harte Fehler

- Eine fehlende Datenbank ist **nie** ein stiller, byte-identischer No-op und
  **nie** eine geratene Korrektur: `resolve_system_with()` liefert
  `Err(SystemDbError)` **und** meldet an die Senke; der `Display`-Text nennt
  **jede** geprüfte Stelle mit Quelle, Grund-Label und Grund-Text (`Absent`,
  `NotADirectory`, `Unreadable`, `NoXmlFiles`, `AllFilesRejected` — jeweils
  unterschiedlich formuliert) sowie die Abhilfe.
- **Teilweise unlesbare Datei** ist kein stiller Fallback, sondern eine
  sichtbare Warnung mit **Dateinamen** (`Diagnostics::file_rejected`).
  Erst wenn **keine** Datei geladen werden konnte, ist der Fehler hart
  (`AllFilesRejected`, „XML-Dateien vorhanden, aber alle von liblensfun
  abgelehnt"). Verankert in `tests::diagnostics::a_rejected_file_is_reported_by_name_and_the_load_still_succeeds`
  bzw. `…::only_when_every_file_is_rejected_is_the_error_hard`.
- **Nicht lesbares Verzeichnis** ist `Unreadable` und wird als solcher gemeldet,
  nicht als „fehlt". Verankert in
  `tests::db_layers::an_unreadable_directory_is_reported_as_unreadable_not_absent`
  (ein Modus-`000`-Verzeichnis; läuft der Prozess als `root`, erkennt der Test
  das, sagt es auf `stderr` und überspringt — er wird nicht grün, ohne die Aussage
  zu prüfen).
- **Sperrung um `load_layers`.** `LensfunDb::load_layers` nimmt
  `lensfun_global_lock` **selbst** und ist deshalb eine **sichere** Funktion;
  die Sperre als dokumentierte Vorbedingung einer `unsafe fn` zu führen war
  genau der Grund, warum vier Tests sie zuvor nicht hielten — parallel zu
  `concurrent_db_load_and_search_is_safe`, dessen Zweck ein glibc-SIGSEGV ist.
  Ein `unsafe`-Präfix, der eine Sperre einfordert, die der Aufrufer nicht
  sehen kann, ist kein Vertrag, sondern eine Bitte.
- **Tests.** Die DB-gestützten Tests prüfen **echten** Datenbankinhalt
  (dokumentierte Kamera/Objektiv aus der installierten Datenbank, echte
  Verzerrungs- und Vignettierungsaussagen). Es gibt **keinen stillen Skip**:
  fehlt die Datenbank, schlägt der Test mit dem vollen, benannten
  Diagnosetext fehl. Die Reihenfolge und der Ladeplan werden hermetisch über
  `db_path::resolve_with()` mit injizierter Quelle **und injiziertem
  Verzeichnis-Probe** geprüft; die Tests mutieren **keine** Prozess-Umgebung
  (`std::env::set_var` ist unsicher, weil ein paralleles `getenv` in einem
  anderen Thread ein UB ist — dafür gibt es keine testlokale Sperre).
  Die Zeit-Fixtures modellieren **`timestamp.txt`**, nie einen mtime — mit dem
  echten Homebrew-Wert `1645386247` als Anker; ein mtime-basiertes Fixture
  hatte den F1-Fehler zuvor in den Test selbst eingebaut und ihn damit
  „bestätigt" statt gefunden.

## IPTC-Metadaten-Write (Capability-Entscheid LRPAR-G15-IPTC, 2026-09-04)

- **Pure Rust, keine externe Binary, kein Subprozess, kein Runtime-Download.**
  IPTC IIM (JPEG `APP13`/8BIM, `1:90` CodedCharacterSet UTF-8) wird im neuen
  Crate `lumina-iptc` handgeschrieben; XMP (`APP1`) via pure-Rust-Crate
  `xmp-writer` (Compile-time-Dependency). **ExifTool wird bewusst nicht
  gebündelt** — User-Vorgabe „keine Runtime-Dependency“; Bündeln wäre trotz
  eigenem Binary eine Runtime-Abhängigkeit (Perl-Runtime, Lizenz-/Distri-
  butionslast).
- **Format-Scope bewusst JPEG-only (MVP):** PNG/WebP lehnen `--write-metadata`
  pro Datei **laut** ab (kein stiller Fallback, kein stilles Weglassen);
  TIFF/EXIF-Write bleiben Post-MVP (siehe `feature/product/export.md`).
- **Lizenzprüfung vor Integration** gemäß F-078 (`feature/quality/fixtures-
  licensing.md`, `THIRD-PARTY-NOTICES.md`): `xmp-writer`-Lizenz verifizieren
  und dokumentieren; andernfalls XMP-Packet-Eigenschreibweise in
  `lumina-iptc`.
- **Nicht-destruktiv:** Bake-In schreibt ausschließlich in neu erzeugte
  Exportdateien; Ziel-Guard gegen Quelle/Bundle (Muster
  `write_output_guarded`). Entwürfe liegen Sidecar-first
  (`feature/product/iptc-metadata.md`).

## Binäre Sidecar-Artefakte (`zdata`) und zstd (native-only)

- Das binäre Sidecar-Artefakt `<original>.lumina.zdata` (große Masken-/
  Source-Action-Daten) wird mit `zstd` komprimiert (`zstd-sys`, natives
  C-Backend).
- Die `zdata`-Funktion ist in `lumina-sidecar` als optionales, **nicht
  default**-Feature hinterlegt (`[features] default = []; zdata = ["dep:zstd"]`).
- **Code-Gating:** `artifact_status` führt mit Codec die tiefe BLAKE3-/
  Container-Prüfung aus, ohne Codec die strukturelle Variante (kein stiller
  Fallback — Verhalten identisch bis zur eager-Checksummen-Pass).
- **Capability-Entscheidung:** `zdata`/`zstd` bleibt **native-only**.
- **Consumer (FOLLOWUP-WASM-ZDATA-CONSUMER e60a9ad, historisch):** Die Konsumenten
  `lumina-cli`/`lumina-mcp`/`lumina-gui` aktivieren `zdata` direkt als
  Cargo-Dependency; `lumina-onnx` liefert `ort` direkt. Keine Target-Gates mehr.

## Quantitative Limits

Die detaillierten quantitativen Grenzen für Bildgröße, Speicher, Threads und
GPU stehen in den nativen Budget-Stores (F-074-Kalibrierung, `compare.mjs`
report/warn/gate). Kurzfassung (implementiert):

| Limit | native CLI | Desktop (eframe/wgpu) |
| --- | --- | --- |
| Bildgröße (interaktiv vollauflösend) | nur RAM-begrenzt | ≤ 45 MP empfohlen |
| RAW-Backend / Decode | LibRaw 0.22.2 (gepinnt, `lumina-raw`, `lumina-ci:latest`) | LibRaw 0.22.2 (gepinnt) |
| RAM/Heap | `StageFrameCache` 512 MiB (implementiert) | **8 GB gesamt (RAM+VRAM, LRU)** — `LruPreviewCache` 7 Slots, **LRU-Cap 1,5 GiB** (implementiert; 7×24 MP ≈ 672 MiB) |
| VRAM-Pool | n. a. (Headless; GPU optional `--features gpu`) | 1024 MiB, 4 Einträge (implementiert, `LUMINA_GPU_VRAM_BUDGET_MB`/`POOL_ENTRIES`) |
| Threads | Rayon (`available_parallelism`) + `batch --jobs` | Worker-Threads (Decode/Prefetch/Preview) |
| `zdata`/zstd | ja (Feature `zdata`) | ja (Feature `zdata`) |
| ONNX | ja (MVP `onnx-rt`) | ja (MVP `onnx-rt`) |

## ONNX-Adapter native-only (Capability-Entscheidung F-082-FOLLOWUP)

- `lumina-onnx` ist **native-only** (kein Stub mehr — WASM gestrichen).
- Backend-Auswahl ohne stillen Fallback: `lumina_onnx::try_load_onnx_engine`
  liefert `RuntimeDisabled` (Feature aus), `OnnxRuntime` (Feature an,
  Artefakt verifiziert) oder einen harten Fehler (fehlendes/stale/fehlbenanntes
  Artefakt) — nie einen stillen Stub-Ersatz.
- **CLI-Konsum (F-082-FOLLOWUP-Rest):** `lumina-cli` fragt die echte Engine
  über `lumina_onnx::resolve::try_load_onnx_engine` an, sobald ein Lauf
  Re-Inferenz brauchen kann; das `.onnx`-Artefakt kommt aus
  `LUMINA_MODEL_PATH`. Ohne das CLI-Feature `onnx-rt` bleibt der
  `StubBackend` der Default-Draht; mit `onnx-rt` ist ein fehlendes/stale/
  unkonfiguriertes Artefakt ein harter CLI-Fehler — nie ein stiller
  Stub-Ersatz (Details in `feature/product/ai-masks.md`, F-082-FOLLOWUP-Rest).

## KI-Denoise (native-only, F-096a / LRPAR-G14-DENOISE-20)

- `denoise` ist eine getrennte lokale ONNX-Capability; sie wird weder als
  Masken- noch als generative Fähigkeit geraten und hat keinen Cloud-Fallback.
- CLI und Desktop können den Status `unavailable`/`stale`/`missing`/`corrupt`
  sichtbar melden und die manuelle F-096-NR als ausgewiesenen Fallback
  verwenden. `pending-integration` bleibt bis zum F-078-Gate (Gewichts-Lizenz,
  Provenienz und Hash-Pin) **keine** Produktionsfreigabe.
- Die Core-/Sidecar-/Input-Spec-Verträge (u. a. `lumina-denoise-input-spec-v2`,
  Core-Blend v1 und Distance-to-Edge-Assembly v1) sind native Vertragsbestandteile,
  keine Cloud-Fähigkeit. Der tests-only Fixture-/Stub-Pfad wird nicht als
  echtes Denoise-Modell ausgeliefert.

## Geplante generative Capabilities (Doku-first, 2026-09-02, GEN-EXPAND-1 / SPOT-REMOVE-1)

Noch nicht implementiert — nur dokumentiert (kein Code, kein Gate-Bruch):

| Fähigkeit | native CLI | Desktop (eframe) |
| --- | --- | --- |
| Generatives Entfernen (`inpaint`, lokal ONNX, `GenerativeEdit`) | geplant, `lumina-onnx` | geplant, `lumina-onnx` |
| Generatives Erweitern (`outpaint`/`canvas expansion >100 %`, lokal ONNX) | geplant | geplant |
| Generatives Entfernen/Erweitern (Cloud-API) | nicht geplant — nur mit expliziter Capability-Entscheidung | nicht geplant |
| Staub schnell (heuristisch/Clone, kein ONNX) | geplant, `lumina-core` | geplant, `lumina-core` |
| Staub generativ (`inpaint_heal`, lokal ONNX, `kind = "spot_heal_generative"`) | geplant, `lumina-onnx` | geplant, `lumina-onnx` |

Lokal ONNX vs. Cloud sind **getrennte** Capabilities (kein stiller Fallback, siehe `feature/product/generative-expand.md` und `feature/product/spot-removal.md`). `zdata`/`zstd` bleibt native-only.
