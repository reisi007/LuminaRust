# Sicherheits-/Risikoregister LuminaRust

**Feature:** Sicherheits-/Risikoregister — die Form, noch ohne Befunde
**Stand:** 2026-10-20
**Ort:** `feature/quality/security-review-register.md`
**Pflege:** Build-Agent (Orchestrator) nach bestandener Verifikation einer
Welle; der Implementierungs- bzw. Verifizierungs-Agent liefert den Beleg
(Reproduktionskommando + Ausgabe) und den Regressionstest-Pfad.
**Herkunft:** Skill `security-review` (agents-skills `82a9abb`), Marker-Range
`86d677e..787670a`; Eigentümerentscheid 2026-10-20 — der Flow wird
übernommen, das Register wird angelegt.

> **Dieses Register enthält bisher keine Befunde.** Es ist die Form, nicht das
> Ergebnis. Die einzige Datenzeile in §3 ist ein als Beispiel markiertes
> Formular, kein Fund. Eine spätere Review-Runde füllt die Tabelle; ohne
> geschlossene Welle bleibt die Tabelle leer.
>
> **§5.1** hält die **Enumeration** der Oberflächen fest (Welle 1,
> Teil *Finden/Enumerieren*; die Basis wurde am 2026-10-20 auf 24 Oberflächen
> erweitert): Eingabe- und Ausgabepfade, Vertrauensgrenze und die zu prüfenden
> Klassen. Sie enthält **keine Bewertung** und **keinen Befund**.

---

## Inhaltsverzeichnis

- [1. Zweck und normative Rolle](#1-zweck-und-normative-rolle)
- [2. Die Zwei-Hälften-Form](#2-die-zwei-hälften-form)
  - [2.1 Finden](#21-finden)
  - [2.2 Schließen](#22-schließen)
  - [2.3 Stage 6 — pinnen](#23-stage-6--pinnen)
- [3. Registertabelle](#3-registertabelle)
- [4. Regeln für die Einträge](#4-regeln-für-die-einträge)
- [5. Enumerationsbasis dieses Projekts](#5-enumerationsbasis-dieses-projekts)
  - [5.1 Enumerierte Außengrenzen (Welle 1)](#51-enumerierte-außengrenzen-security-review-welle-1)
    - [5.1.1 MCP-Server (Transport)](#511-mcp-server-transport)
    - [5.1.2 MCP-Werkzeug-Oberfläche](#512-mcp-werkzeug-oberfläche)
    - [5.1.3 MCP-Pfad-/Zielbehandlung](#513-mcp-pfad-zielbehandlung)
    - [5.1.4 MCP-Session-Zustand](#514-mcp-session-zustand)
    - [5.1.5 CLI-Kommando-Oberfläche](#515-cli-kommando-oberfläche)
    - [5.1.6 CLI-Submodule](#516-cli-submodule)
    - [5.1.7 Gemeinsame Pfad-/Dateischicht (lumina-stages)](#517-gemeinsame-pfad-dateischicht-lumina-stages)
    - [5.1.8 Culling-Sidecar-Bindung (lumina-cull)](#518-culling-sidecar-bindung-lumina-cull)
    - [5.1.9 Verzeichnis-/Ordnerbaum-Ebene](#519-verzeichnis-ordnerbaum-ebene)
    - [5.1.10 Sidecar-JSON (Schreibpfad, CAS/Atomic-Write)](#5110-sidecar-json-schreibpfad-casatomic-write)
    - [5.1.11 Sidecar-JSON (Lesepfad) und Pfadableitung](#5111-sidecar-json-lesepfad-und-pfadableitung)
    - [5.1.12 Sidecar-Begleitdatei .lumina.zdata](#5112-sidecar-begleitdatei-luminazdata)
    - [5.1.13 Cache- und Vorschau-Pfade (im Speicher)](#5113-cache--und-vorschau-pfade-im-speicher)
    - [5.1.14 Cache auf Platte](#5114-cache-auf-platte)
    - [5.1.15 .lumina-Cachewurzel im Bildordner](#5115-lumina-cachewurzel-im-bildordner)
    - [5.1.16 RAW-/Bild-Decoding](#5116-raw-bild-decoding)
    - [5.1.17 Merge-/DNG-Ausgabe](#5117-merge-dng-ausgabe)
    - [5.1.18 Metadaten (IPTC/XMP/IIM/JPEG)](#5118-metadaten-iptcxmpiimjpeg)
    - [5.1.19 Rezept-/JSON-Eingaben (allgemein)](#5119-rezept-json-eingaben-allgemein)
    - [5.1.20 Meta-Präsets (Dateiformat + Benutzerkonfigurationsverzeichnis)](#5120-meta-präsets-dateiformat--benutzerkonfigurationsverzeichnis)
    - [5.1.21 ONNX-Modell- und Artefakt-Eingaben](#5121-onnx-modell--und-artefakt-eingaben)
    - [5.1.22 Lensfun-Datenbank (Systempfade)](#5122-lensfun-datenbank-systempfade)
    - [5.1.23 GUI-Dateieingabe und -ausgabe](#5123-gui-dateieingabe-und--ausgabe--gleichrangig-geprüft-eigentümerentscheid-2026-10-20)
    - [5.1.24 Prozess-Umgebung](#5124-prozess-umgebung)
  - [5.2 Offene Beobachtungen aus der Enumeration (keine Befunde)](#52-offene-beobachtungen-aus-der-enumeration-keine-befunde)
- [6. Was dieses Register nicht ist](#6-was-dieses-register-nicht-ist)
- [7. Abgrenzung zur Nachbarschaft](#7-abgrenzung-zur-nachbarschaft)
- [8. Herkunft, Stand und Fortschreiben](#8-herkunft-stand-und-fortschreiben)

---

## 1. Zweck und normative Rolle

Der Skill `security-review` (agents-skills `82a9abb`) verlangt in **Stage 6**,
dass jeder geschlossene Befund **zwei** Dinge bekommt:

1. einen Test, der bei Rückkehr des Defekts rot wird, **und**
2. einen **Eintrag in einem Sicherheits-/Risikoregister des Repos**.

LuminaRust hatte beides nicht: es gab kein Register und keine
Security-Review-Pflicht in `Agents.md`/`DoD.md`. Die Eigentümerentscheid vom
2026-10-20 nimmt den Flow an und beauftragt dieses Dokument als
**Register-Ort**.

Damit ist dieses Dokument **normativ** für den Teil „Registereintrag" des
Security-Review-Ablaufs: Eine Welle gilt im Sinne des Skills erst dann als
geschlossen, wenn neben dem Regressionstest auch eine Zeile in §3 existiert.
Fehlt die Zeile, ist die Welle nicht abgeschlossen.

Dieses Dokument macht **keine** Aussage über den Zustand einer Oberfläche und
enthält **keinen** Fund. Es hält fest:

- **welche Form** ein späterer Befund hier haben muss (§2–§4), und
- **welche Oberflächen** dieses Projekt überhaupt hat — die Enumerationsbasis,
  die eine spätere Review-Runde zu enumerieren hätte (§5).

**Die Enumerierung ist damit erstmals strukturiert festgehalten.** Vor diesem
Dokument existierte im Repo keine **projektweite, systematische** Liste
erreichbarer Oberflächen; sie wurde weder beim Lesen der Architekturdokumente
noch beim Marker-Lauf (`AGENTS.skills.md`, Zeile „`security-review` (neu)")
systematisch zusammengestellt. §5 ist diese Liste — eine Bestandsaufnahme der
Eingabe-/Ausgabe-Außengrenzen, **keine** Prüfung.

**Die erste Welle ist ein eigener, noch nicht geplanter Task.** Weder der
Befund-Satz noch die Wellenplanung gehören zu diesem Dokument. Eine
Security-Review-Runde ist ein beauftragter Durchlauf mit eigenem
Implementierungs- und Verifizierungs-Agenten.

---

## 2. Die Zwei-Hälften-Form

Der Ablauf, in den spätere Befunde hier eintragen, besteht aus zwei Hälften in
fester Reihenfolge.

### 2.1 Finden

- Die **erreichbaren Oberflächen** werden **programmatisch** aufgelistet, nicht
  aus dem Gedächtnis und nicht aus einem einzelnen `grep`. Für LuminaRust ist
  die Aufzählungsbasis §5; das populationsgebende Kommando (MCP-Tool-Liste,
  CLI-`Command`-Enum, Modul-/Pfadliste) gehört in den Review-Bericht, nicht in
  dieses Register.
- Jeder Befund wird mit einem **reproduzierbaren Kommando und dessen Ausgabe**
  belegt. „Sieht ungeschützt aus" ist kein Befund.
- Der Befund wird klassifiziert: Defektklasse und Schweregrad (Reach ×
  Impact). Klassen-Vokabular: `BELEGT` (gemessen), `HEURISTIK` (Musterbefund,
  Population stimmt, Einzelfall noch zu bestätigen), `AKZEPTIERT` (echter
  Befund, den der Eigentümer bewusst mitträgt), `ZURÜCKGENOMMEN` (am
  Falsch-Positiv-Gate gescheitert, mit Grund im Register festgehalten).
- Zugriffsmodelle, die der Eigentümer entscheiden muss (welche Fläche öffentlich
  bleibt, welche geschlossen wird), werden **vor** den Wellen entschieden und
  als Entscheidung im Entscheidungslog des Projekts festgehalten.

### 2.2 Schließen

- Befunde werden in **Wellen** geschlossen. Eine Welle = eine zusammenhängende
  Gruppe **nach Mechanismus**, nicht nach Datei; derselbe Defekt an vier
  Stellen ist eine Welle.
- Reihenfolge: Schweregrad zuerst (`critical` → `high` → `medium` → `low`),
  danach der kleinere Wirkungsradius.
- **Jede Welle trägt genau einen Regressionstest** — einen Test, der bei
  Rückkehr des Defekts rot wird. Nicht Null, nicht zwei.
- Jede Welle hinterlässt den Baum grün (Tests, Lint, Build) und ist eine
  eigene, unabhängig verifizierte Runde mit **einem** Commit.
- Ein Fix darf keinen Befund verlagern: taucht er an anderer Stelle wieder auf,
  ist das ein **neuer** Befund, kein geschlossener.

### 2.3 Stage 6 — pinnen

Ein geschlossener Befund braucht **beides**:

- den Regressionstest (Pfad + Testname im Feld „Regressionstest" der
  Tabelle), **und**
- den Registereintrag (die Zeile in §3).

Nur eines von beidem ist kein geschlossener Befund — eine Korrektur, die der
nächste Refactor stillschweigend zurücknehmen darf. Die Tabelle in §3 führt
darum beide Spalten nebeneinander und verlangt für `geschlossen` beide
Einträge.

---

## 3. Registertabelle

Die Form, in der spätere Reviews hier eintragen. Die Kopfzeile ist fix; die
Spaltenbedeutungen:

| Spalte | Bedeutung |
|---|---|
| **Befund-ID** | stabil, `SEC-<n>`; einmal vergeben, nie still umbenannt |
| **Defektklasse** | `BELEGT` / `HEURISTIK` / `AKZEPTIERT` / `ZURÜCKGENOMMEN` (§2.1) |
| **betroffene Oberfläche** | eine Zeile aus §5 (MCP-Tool, CLI-Subcommand, Datei-/Sidecar-Pfad, …) |
| **Wellenstatus** | `offen` / `in Welle` / `geschlossen` / `zurückgenommen` / `akzeptiert` |
| **Regressionstest** | Pfad + Testname des Tests, der den Defekt rot werden lässt; bei `geschlossen` **zwingend** |
| **Owner** | Person/Agent, die den Eintrag pflegt bzw. die Welle führt |
| **Fundstellen-Beleg** | reproduzierbares Kommando **und** Ausgabe (bei `geschlossen` und `akzeptiert` zwingend) |
| **Stand** | Datum des letzten Standes dieses Eintrags |

### Register

| Befund-ID | Defektklasse | betroffene Oberfläche | Wellenstatus | Regressionstest | Owner | Fundstellen-Beleg | Stand |
|---|---|---|---|---|---|---|---|
| `SEC-EXAMPLE` | — (Beispielzeile — kein Befund) | — | — | — | — | — | Beispielzeile — kein Befund |

Die Zeile `SEC-EXAMPLE` ist **Formular**, kein Fund: Sie trägt keine
Defektklasse, keine Oberfläche, keinen Status, keinen Test und keinen Beleg.
Sie zeigt nur, wie eine ausgefüllte Zeile aussehen soll. Sie wird durch die
erste echte Zeile ersetzt, sobald eine Review-Runde einen Befund mit Beleg
liefert; bis dahin bleibt sie als Formbeispiel stehen und wird **nicht** als
Befund gezählt.

---

## 4. Regeln für die Einträge

1. **Belegpflicht.** Jede Zeile nennt ein reproduzierbares Kommando **und**
   dessen Ausgabe. Ein Befund ohne Beleg ist kein Befund; er wird nicht
   eingetragen, sondern zurückgestellt.
2. **Geschlossen heißt geschlossen.** Ein Befund gilt erst als geschlossen,
   wenn **beides** vorliegt: ein Regressionstest, der bei Rückkehr des
   Defekts rot wird, **und** der Eintrag in dieser Tabelle. Fehlt eines, ist
   der Status `in Welle`, nicht `geschlossen`.
3. **Eine Welle, ein Test.** Jede geschlossene Welle erzeugt **genau einen**
   Regressionstest. Mehrere Tests für eine Welle werden in der Zeile vermerkt
   (z. B. „2 Tests: …, …“), aber die Wellenzahl bleibt an der Testzahl
   ablesbar.
4. **Widerlegungen bleiben im Register.** Ein Befund, der am Falsch-Positiv-
   Gate scheitert, wird **nicht** weggelassen, sondern mit Klassenwert
   `ZURÜCKGENOMMEN` und dem Grund **im Register** festgehalten. Ein stilles
   Löschen würde die nächste Runde dasselbe Phantom erneut suchen lassen.
5. **Tragende Befunde bleiben sichtbar.** Ein Befund mit Klasse `AKZEPTIERT`
   (vom Eigentümer bewusst mitgetragen) wird mit Begründung eingetragen und
   bleibt im Register stehen — er wandert **nicht** in `Agents.todo.md`.
6. **Keine Vermischung mit dem Aufgaben-Backlog.** Befunde, die keine
   Sicherheitsklasse haben (Bedienfehler, Performance, GUI-Darstellung), gehören
   in `Agents.todo.md`, nicht in diese Tabelle (§7).
7. **Stabile Bezüge.** Zeilen verweisen auf Pfade und Testnamen, nicht auf
   Zeilennummern (die driften). Der Beleg nennt das Kommando, das den Befund
   reproduzierbar zeigt.

---

## 5. Enumerationsbasis dieses Projekts

Die Oberflächen, die eine spätere Security-Review-Runde dieses Projekts zu
enumerieren hätte. **Nur die Oberfläche wird benannt** — Pfad, Art der
Eingabe/Ausgabe und der Stand der Enumeration. Es wird **nicht** geprüft, ob
an diesen Stellen etwas fehlerhaft ist; dieses Dokument enthält keine
Bewertung des Zustands einer Oberfläche.

Alle Pfade dieser Tabelle wurden am 2026-10-20 gegen den Baum geprüft und
existieren.

**Enumerationsstand:** Die **24** Oberflächen dieser Tabelle wurden am 2026-10-20
(**Security-Review-Welle 1**, Teil *Finden/Enumerieren*) **enumeriert** —
Eingabe- und Ausgabepfade je Oberfläche stehen in **§5.1**. Die Enumeration
bewertet den Zustand einer Oberfläche **nicht** (§6) und hat **keinen** Befund
erzeugt; die Spalte „Was hier noch offen ist" nennt weiterhin nur die noch
ausstehende **Prüfung**.

**Basis-Erweiterung (Eigentümerentscheid 2026-10-20):** die Basis wurde
**vollständig erweitert** — fünf neue Zeilen (`lumina-stages`, `lumina-cull`,
Verzeichnis-/Ordnerbaum-Ebene, `.lumina`-Cachewurzel im Bildordner,
Meta-Präsets) und zwei bisher nicht genannte CLI-Module
(`lensfun_gpu.rs`, `local_mask_gpu.rs`) in der Zeile *CLI-Submodule*. Jede neue
Zeile ist in §5.1 **mit derselben Gliederung enumeriert** (Eingabepfade /
Ausgabepfade / Vertrauensgrenze / zu prüfende Klassen / Beleg).

| Oberfläche | Pfad | Art der Eingabe/Ausgabe | Was hier noch offen ist |
|---|---|---|---|
| **MCP-Server (Transport)** | `crates/lumina-mcp/src/lib.rs` (`Server`, JSON-RPC über stdio), `crates/lumina-mcp/src/error.rs` | Eingabe: JSON-RPC-Requests auf stdin; Ausgabe: JSON-RPC-Responses auf stdout | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: Population wäre die Protokoll-/Methodenliste des Servers |
| **MCP-Werkzeug-Oberfläche** | `crates/lumina-mcp/src/tools/mod.rs` (`list_tool_definitions`, `dispatch_tool`, `is_known_tool`), Werkzeug-Implementierungen in `crates/lumina-mcp/src/tools/*.rs` | Eingabe: Tool-Argumente als JSON; Ausgabe: Tool-Ergebnis als JSON | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: Population wäre die Tool-Liste (29 registrierte Tools, je ein eigenes Modul; daneben 4 gemeinsame Helper-Module im selben Verzeichnis); **SOLL-Quelle**: `feature/platform/mcp-server.md` |
| **MCP-Pfad-/Zielbehandlung** | `crates/lumina-mcp/src/output_guard.rs` (`reject_protected_target`), `crates/lumina-mcp/src/util.rs` (`build_source_identity`, `write_output_guarded`, `parse_bounded_uint`, `validate_output_extension`) | Eingabe: Datei- und Zielpfade, Zahlen- und Formatparameter; Ausgabe: abgewiesene Schreibziele, Source-Identität | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: jede der Wurzelfunktionen wäre einzeln zu betrachten |
| **MCP-Session-Zustand** | `crates/lumina-mcp/src/session.rs` | Eingabe: Tool-Aufrufe mit `image_id`; Ausgabe: Sitzungszustand, Vorschau-/Temp-Dateien | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: Sitzung-Lebenszyklus und Aufräumen |
| **CLI-Kommando-Oberfläche** | `crates/lumina-cli/src/main.rs` (`enum Command`, clap-`Parser`), Argument-Strukturen ab `struct FileArgs` ff., Kommando-Implementierungen ab `fn run(cli: Cli)` | Eingabe: Kommandozeile (Argumente, Pfade, Zahlen); Ausgabe: Dateien, stdout/stderr, Exit-Code | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: Population wäre das `Command`-Enum (32 Varianten mit Argument-Strukt, zusätzlich die Feature-gate-Variante `Mcp`); **SOLL-Quelle**: `feature/platform/cli-gui-wasm.md` |
| **CLI-Submodule** | `crates/lumina-cli/src/{auto_tone_cli,cull_cli,lensfun_cli,lensfun_gpu,local_mask_gpu,mask_local,mask_local_color,mask_local_curves,mask_local_detail,mask_local_presence,merge,matrix,stages,library}.rs` (die vier GPU-/Lensfun-Module stehen unter `#[cfg(feature = "gpu")]` bzw. `#[cfg(feature = "lensfun")]` — im Default-Build sind sie **nicht** kompiliert) | Eingabe: je Subcommand eigene Argumente; Ausgabe: Rezept-/Sidecar-/Status-Änderungen | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: jede Kommando-Gruppe wäre getrennt zu zählen. **Ausnahme:** `lensfun_gpu.rs` und `local_mask_gpu.rs` sind **keine** Subcommands mit eigenen Argumenten, sondern GPU-Binder/-Compositing-Module des CLI-Crates, die `fn run(cli: Cli)` über `render_best_effort` erreicht (`lensfun_gpu::classify`, `lensfun_gpu::bind`, `local_mask_gpu::mask_layer_routing_reasons`, `local_mask_gpu::composite`) |
| **Gemeinsame Pfad-/Dateischicht (`lumina-stages`)** | `crates/lumina-stages/src/paths.rs` (`require_sidecar`, `collect_target_sidecars`, `collect_sidecars`, `collect_tree_files`, `move_file_cross_volume`, `load_smart_catalog`), `crates/lumina-stages/src/relocate.rs` (`run`), `crates/lumina-stages/src/collections.rs` (`run`), `crates/lumina-stages/src/merge.rs` (`decode_source`), `crates/lumina-stages/src/matrix_run.rs` (`run`) | Eingabe: Bild-, Sidecar-, Ziel- und Verzeichnispfade; Ausgabe: verschobene Dateien samt Begleitdateien, geschriebene Sidecars, DNG- und Report-Dateien | **enumeriert (Welle 1, §5.1)** — **Basis-Erweiterung** (Eigentümerentscheid 2026-10-20) — enumeriert in §5.1 |
| **Culling-Sidecar-Bindung (`lumina-cull`)** | `crates/lumina-cull/src/sidecar_io.rs` (`load_culling`, `save_culling`, `record_culling`, `clear_culling`, `evaluate_culling`, `evaluate_section`), `crates/lumina-cull/src/identity.rs` (`identity_mismatches`), `crates/lumina-cull/src/analyze.rs` (`analyze_heuristic`, `analyze_selection`) | Eingabe: Sidecar-Datei von Platte, Culling-Identität; Ausgabe: `CullingReadState`, persistierter `culling`-Block in der `.lumina.json` | **enumeriert (Welle 1, §5.1)** — **Basis-Erweiterung** (Eigentümerentscheid 2026-10-20) — enumeriert in §5.1 |
| **Verzeichnis-/Ordnerbaum-Ebene** | `crates/lumina-mcp/src/util.rs` (`collect_tree_files`, `collect_tree_files_inner`), `crates/lumina-stages/src/paths.rs` (`collect_tree_files`, `collect_tree_files_inner`, `collect_sidecars`, `collect_target_sidecars`) | Eingabe: Verzeichnispfad als Werkzeugargument; Ausgabe: die Liste der darunter gefundenen Dateien (Menge, Reihenfolge, Pfade) | **enumeriert (Welle 1, §5.1)** — **Basis-Erweiterung** (Eigentümerentscheid 2026-10-20) — enumeriert in §5.1 |
| **Sidecar-JSON (Schreibpfad, CAS/Atomic-Write)** | `crates/lumina-sidecar/src/lib.rs` (`save_sidecar`, `save_sidecar_if_unchanged`, `write_atomically`, `migrate_sidecar_file`, `recover_sidecar`, `acquire_write_lock`) | Eingabe: `SidecarDocument`/`EditRecipe` (serde); Ausgabe: `.lumina.json` auf Platte, atomar ersetzt, mit Compare-and-Swap und Write-Lock | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: der CAS-/Atomic-Write-Pfad wäre je Kanal (Lesen, Schreiben, Migration, Lock-Reclaim) zu enumerieren |
| **Sidecar-JSON (Lesepfad) und Pfadableitung** | `crates/lumina-sidecar/src/lib.rs` (`load_sidecar`), `crates/lumina-sidecar/src/sidecar_paths.rs` (`sidecar_path_for`) | Eingabe: JSON-Datei von Platte; Ausgabe: `SidecarDocument` | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: Grenzwerte, Parser-Verhalten und Pfadableitung je eigene Klasse |
| **Sidecar-Begleitdatei `.lumina.zdata`** | `crates/lumina-sidecar/src/zdata.rs` (`zdata_path_for`, Validierung/Prüfsummen der Kachel-Blöcke) | Eingabe: Binärdatei mit Tile-Blöcken und Prüfsumme; Ausgabe: Masken-Kacheln | **enumeriert (Welle 1, §5.1)** |
| **Cache- und Vorschau-Pfade** | `crates/lumina-core/src/cache.rs` (`FolderCache`, `CacheStore`, `preview_cache_key`), `crates/lumina-core/src/preview_cache.rs` (`LruPreviewCache`, `encode_webp_lossless`, `decode_webp`), `crates/lumina-core/src/stage_cache.rs` (`StageFrameCache`) | Eingabe: Cache-Schlüssel aus Source/Virtual-Copy/Rezept-Digest; Ausgabe: Bildframes im Speicher | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: Schlüsselbildung und Lösch-/Prune-Verhalten wären je Klasse zu zählen |
| **Cache auf Platte** | `crates/lumina-cache/src/disk.rs` (`store_preview`, `load_preview`, `save_settings`, `atomic_write`), `crates/lumina-cache/src/preview_disk.rs` (`store`, `load`, `prune_orphans`) | Eingabe/Ausgabe: Vorschau-Dateien und Cache-Einstellungen im Ordner-Cache, atomar geschrieben | **enumeriert (Welle 1, §5.1)** |
| **.lumina-Cachewurzel im Bildordner** | `crates/lumina-cache/src/disk.rs` (`DiskFolderCache::for_image`, `in_folder`, `path`, `settings_path`, `PREVIEWS_DIR`), `crates/lumina-cache/src/preview_disk.rs` (`PreviewDiskCache::in_folder`) | Eingabe: Bildordner-Pfad; Ausgabe: das Verzeichnis `.lumina/` (mit `previews/`) **im Bildordner**, nebst `settings.json` | **enumeriert (Welle 1, §5.1)** — **Basis-Erweiterung** (Eigentümerentscheid 2026-10-20) — enumeriert in §5.1 |
| **RAW-/Bild-Decoding** | `crates/lumina-raw/src/lib.rs` (`decode_file`, `decode_bytes`, `decode_bytes_with_options`, `RAW_EXTENSIONS`, `is_raw_extension`), `crates/lumina-core/src/memory.rs` (`MemoryBudget::from_env`, `check_decode`, `check_mask`) | Eingabe: Bild-/RAW-Dateien fremder Formate; Ausgabe: dekodierte Frames, Budgetentscheidungen | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: Formatliste und Decoder je Klasse |
| **Merge-/DNG-Ausgabe** | `crates/lumina-merge/src/dng.rs` (`encode_linear_dng`, `write_merge_dng`, `merge_dng_filename`, `validate_dng_file_name`), `crates/lumina-merge/src/bundle.rs` (`run_merge`, `merge_checksum`), `crates/lumina-merge/src/digest.rs` | Eingabe: Frames + Rezept; Ausgabe: DNG-Datei mit EXIF und Prüfsumme | **enumeriert (Welle 1, §5.1)** |
| **Metadaten (IPTC/XMP/IIM/JPEG)** | `crates/lumina-iptc/src/xmp.rs` (`build_xmp_packet`, `parse_xmp_packet`, `build_app1`, `parse_app1`), `crates/lumina-iptc/src/iim.rs` (`encode_iim`, `decode_iim`, `build_app13`, `parse_app13`), `crates/lumina-iptc/src/jpeg.rs`, `crates/lumina-iptc/src/registry.rs` | Eingabe: Metadatenblöcke in Bilddateien; Ausgabe: Sidecar-Metadatenentwurf bzw. zurückgeschriebene APP1/APP13-Segmente | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: Parser- und Encoder-Paare je Klasse |
| **Rezept-/JSON-Eingaben (allgemein)** | `crates/lumina-sidecar/src/lib.rs` (`EditRecipe`, `SidecarDocument::from_json`, `to_json`), `crates/lumina-sidecar/src/local_adjustments/` (Unterblöcke und Migration) | Eingabe: JSON-Dokumente (Sidecar, Rezept, Local-Adjustments-Blöcke); Ausgabe: validierte Rezeptstruktur | **enumeriert (Welle 1, §5.1)** — offen für die Prüfung: Feld- und Versionsklassen je Block |
| **Meta-Präsets (Dateiformat + Benutzerkonfigurationsverzeichnis)** | `crates/lumina-sidecar/src/meta_preset.rs` (`META_PRESET_FILE_SUFFIX` = `.lumina-meta-preset.json`, `default_meta_presets_dir`, `meta_preset_filename`, `resolve_meta_preset_path`, `load_meta_preset_file`, `scan_meta_presets_dir`, `render_meta_preset`), `crates/lumina-gui/src/presets.rs` (`PRESET_FILE_SUFFIX` = `.lumina-preset.json`, `default_presets_dir`, `preset_filename`, `save_preset_file`, `load_preset_file`, `scan_presets_dir`) | Eingabe: JSON-Präsetsdatei aus dem Benutzerkonfigurationsverzeichnis; Ausgabe: validierte Präsets, gerenderte Platzhalterwerte, geschriebene `.lumina-preset.json` | **enumeriert (Welle 1, §5.1)** — **Basis-Erweiterung** (Eigentümerentscheid 2026-10-20) — enumeriert in §5.1 |
| **ONNX-Modell- und Artefakt-Eingaben** | `crates/lumina-onnx/src/manifest.rs` (`ModelManifest::from_json`, `validate`, `to_model_identity`), `crates/lumina-onnx/src/hash.rs`, `crates/lumina-onnx/src/resolve.rs` | Eingabe: Manifest- und Modell-Dateien von Platte; Ausgabe: Modellidentität, aufgelöste Artefaktpfade | **enumeriert (Welle 1, §5.1)** |
| **Lensfun-Datenbank (Systempfade)** | `crates/lumina-lensfun/src/db_path.rs` (`resolve`, `xml_files_in`, `from_process`), `crates/lumina-lensfun/src/system_load.rs`, `crates/lumina-lensfun/src/db_user_dir.rs` | Eingabe: Lesezugriff auf System-Verzeichnisse der Lensfun-Datenbank; Ausgabe: aufgelöste Profilpfade | **enumeriert (Welle 1, §5.1)** |
| **GUI-Dateieingabe und -ausgabe** | `crates/lumina-gui/src/dropped_files.rs` (Drop-Handling), `crates/lumina-gui/src/file_browser_entry.rs`, `crates/lumina-gui/src/export.rs` (`export_to`) | Eingabe: vom Nutzer gewählte Pfade (Dialog, Drag-and-Drop); Ausgabe: exportierte Bilddateien | **enumeriert (Welle 1, §5.1)** — **gleichrangig geprüft** (Eigentümerentscheid 2026-10-20): GUI-Oberflächen werden in **jeder** Security-Review-Welle mitgeprüft — dieselbe Datei-Eingabe-/Ausgabeklasse wie CLI und MCP |
| **Prozess-Umgebung** | `crates/lumina-core/src/memory.rs` (`LUMINA_MAX_RAW_PIXELS`, `LUMINA_MAX_MASK_PIXELS`, `LUMINA_MAX_ALLOC_BYTES`), `crates/lumina-mcp/src/lib.rs` (`LUMINA_MCP_KEEP_PREVIEWS`) | Eingabe: Umgebungsvariablen der Prozessumgebung; Ausgabe: Budget- und Cleanup-Entscheidungen | **enumeriert (Welle 1, §5.1)** |

**Hinweise zur Reichweite dieser Basis:**

- Der MCP-Server ist **nicht** netzwerkexponiert (Transport stdio, kein
  HTTP/WebSocket im MVP — `feature/platform/mcp-server.md`, Abschnitt
  „MCP-Protokoll und Transport"). Das ändert nichts an der
  Enumerationspflicht: Werkzeug-Argumente, Pfade und Datei-I/O bleiben
  Eingabe-Außengrenzen.
- WASM/Browser ist **gestrichen** (Eigentümer-Entscheid 2026-09-04, siehe
  `feature/platform/wasm-limits.md`); eine WASM-Oberfläche existiert daher
  nicht und steht hier nicht.
- **Scope-Frage GUI — entschieden (Eigentümerentscheid 2026-10-20):** die
  GUI (Datei-Eingabe/-Ausgabe über `lumina-gui`) gehört **gleichrangig** in den
  Scope der Security-Review-Wellen — GUI-Oberflächen werden in **jeder** Welle
  mitgeprüft, nicht erst nachrangig. Die Zeile *GUI-Dateieingabe und -ausgabe*
  und der Abschnitt §5.1 führen die GUI deshalb **ohne** Vorbehalt; die
  Enumeration der GUI-Oberfläche liegt vollständig vor (§5.1) und wird durch
  die Entscheidung **nicht** neu erhoben.
- Diese Liste ist eine **Bestandsaufnahme der Flächen**, keine vollständige
  Bedrohungsanalyse: sie sagt, wo Eingaben an der Außengrenze entstehen, und
  nichts über deren Behandlung.

---

## 5.1 Enumerierte Außengrenzen (Security-Review-Welle 1)

**Was dieser Abschnitt ist.** Die **24** Oberflächen aus §5 sind **enumeriert**
(die ersten 19 in Welle 1, die weiteren 5 nach der Basis-Erweiterung vom
2026-10-20): je Oberfläche sind die **erreichbaren Eingabe- und Ausgabepfade**
aufgeführt, die **Vertrauensgrenze** benannt und die **Defektklassen**
aufgeführt, die in der späteren Prüfung zu untersuchen wären. Jede Zeile nennt
**Pfad und Symbol**, keine Zeilennummer (Regel 7).

**Was dieser Abschnitt nicht ist.** Er enthält **keine Bewertung** des Zustands
einer Oberfläche (§6) und **keinen Befund** — §3 bleibt unverändert bei null
Befunden. Die genannten Klassen sind **kein Befund**: sie benennen nur, *wo*
die spätere Welle schauen müsste. Jede Aussage hier ist ein **Codebefund**
(Existenz von Pfad und Symbol), **kein Laufzeitbefund**: in dieser Welle wurde
**nichts gebaut, nichts gestartet und nichts gemessen**.

**Zählweise (Population).** Die Zahlen sind selbst gezählt, nicht geschätzt.

| Population | Zahl |
|---|---|
| Oberflächen in §5 | **24** (Zeilen der §5-Basis-Tabelle) |
| `tool_def(`-Aufrufe in `crates/lumina-mcp/src/tools/mod.rs` | **29** Werkzeugdefinitionen (plus die eine Konstruktorfunktion `fn tool_def`) |
| Module in `crates/lumina-mcp/src/tools/` | **33** Module (+ `mod.rs` = **34** Dateien): **29** mit `pub const NAME` (je ein registriertes Werkzeug) + **4** Helper (`library_common`, `merge_common`, `meta_common`, `stage_common`) — 29 + 4 = 33 |
| `Command`-Varianten in `crates/lumina-cli/src/main.rs` | **33** insgesamt: **32** mit Argument-Strukt, dazu die feature-gate-Variante `Mcp` (`#[cfg(feature = "mcp")]`), die **ohne** Klammer steht |
| CLI-Module in `crates/lumina-cli/src/` | **15** `.rs`-Dateien: **14** in §5 benannt (12 Subcommand-Module + `lensfun_gpu.rs` + `local_mask_gpu.rs`), `main.rs` als Einstieg |
| Argument-Strukturen in `crates/lumina-cli/src/main.rs` | **33** (`struct FileArgs` ff.) |

**Die Zählkommandos** (im Repo ausführbar; die Ausgabe steht jeweils in
Klammern dahinter):

- `grep -c "tool_def(" crates/lumina-mcp/src/tools/mod.rs` → 30 (29 Aufrufe +
  die Konstruktorfunktion `fn tool_def`)
- `ls crates/lumina-mcp/src/tools/*.rs | wc -l` → 34 (33 Module + `mod.rs`)
- `grep -l "pub const NAME" crates/lumina-mcp/src/tools/*.rs | wc -l` → 29
- `grep -L "pub const NAME" crates/lumina-mcp/src/tools/*.rs` →
  `library_common.rs`, `merge_common.rs`, `meta_common.rs`, `mod.rs`,
  `stage_common.rs` (4 Helper + `mod.rs`)
- `awk '/^enum Command \{/,/^\}/' crates/lumina-cli/src/main.rs | grep -cE "^    [A-Z][A-Za-z]*"` →
  **33** (Gesamtzahl, `Mcp` **mitgezählt**)
- `awk '/^enum Command \{/,/^\}/' crates/lumina-cli/src/main.rs | grep -cE "^    [A-Z][A-Za-z]*\("` →
  32 (nur die Varianten **mit** Argument-Strukt; `Mcp` steht ohne Klammer)
- `ls crates/lumina-cli/src/*.rs | wc -l` → 15
- `grep -cE "^struct [A-Za-z]+Args" crates/lumina-cli/src/main.rs` → 33

> **Zählhinweis (kein Befund):** die §5-Basiszeile *CLI-Kommando-Oberfläche*
> nennt „32 Varianten mit Argument-Strukt, zusätzlich die Feature-gate-Variante
> `Mcp`". Gezählt sind **33 Varianten** — **32 mit Argument-Strukt** plus `Mcp`,
> das ohne Klammer steht und daher von der Regex `^    [A-Z][A-Za-z]*\(` **nicht**
> erfasst wird. Die Formulierung in §5 ist damit **deckungsgleich** mit der
> Messung; die **Zähl-Clause** in §5 bleibt unverändert (die §5-Tabelle selbst wuchs in dieser Welle von 19 auf 24 Zeilen und ihre Status-Spalte wurde umgestellt).

---

### 5.1.1 MCP-Server (Transport)

- **Eingabepfade** — `crates/lumina-mcp/src/lib.rs`: `run_stdio` liest
  zeilenweise von stdin (`BufRead::lines`); `handle_line` parst die Zeile mit
  `serde_json::from_str::<Value>`; `handle_message` deserialisiert sie zu
  `Request` und liest daraus `method`, `params`, `params.uri`,
  `params.name`, `params.arguments` sowie `id`.
- **Ausgabepfade** — `crates/lumina-mcp/src/lib.rs`: `run_stdio` schreibt je
  Antwort **eine JSON-Zeile auf stdout** (`out.write_all`); die Antwortobjekte
  baut `crates/lumina-mcp/src/error.rs` (`ok_response`, `error_response`,
  `tool_result_response`, `tool_error_result`); `StderrLogger::log` schreibt
  **Logzeilen auf stderr**; `Server::cleanup_previews` löscht die
  Preview-Dateien der Sitzung.
- **Vertrauensgrenze** — MCP-Client → Serverprozess: **stdin ist nicht
  vertrauenswürdig** (JSON-Frames fremder Herkunft). **stdout ist der
  exklusive Transportkanal**, stderr der Logkanal — die Trennung der beiden
  Kanäle ist selbst schutzrelevant (eine Logzeile auf stdout korrumpiert den
  JSON-RPC-Stream).
- **Zu prüfende Klassen** — Deserialisierung/Schema (JSON-RPC-Frame,
  `Request`-Form); Grenzwerte/Größen (unbegrenzte Zeilen- und
  Parametgröße auf stdin); Dispatch (unbekannte Methode, unbekanntes
  Werkzeug); Kanaltrennung (stdout/stderr); Fehlertext-Ausgabe (untrusted
  `method`/`name` im Fehlertext).
- **Beleg** — `grep -n "fn run_stdio\|fn handle_line\|fn handle_message\|fn cleanup_previews" crates/lumina-mcp/src/lib.rs`;
  `grep -n "pub fn ok_response\|pub fn error_response\|pub fn tool_result_response\|pub fn tool_error_result" crates/lumina-mcp/src/error.rs`.

### 5.1.2 MCP-Werkzeug-Oberfläche

- **Eingabepfade** — `crates/lumina-mcp/src/tools/mod.rs`: `dispatch_tool`
  nimmt den Werkzeugnamen und `args: &Value` und ruft je Werkzeug `run(server, args)`
  auf (29 registrierte Werkzeuge, je ein Modul in `crates/lumina-mcp/src/tools/`).
  Die Argumentextraktion läuft über `crates/lumina-mcp/src/util.rs`
  (`get_str`, `parse_bounded_uint`, `parse_output_format`,
  `validate_output_extension`). Pfadtragende Argumente stehen in den
  Werkzeugschemata (`schema()` je Modul). Die pfadtragenden Argumentnamen je
  Werkzeug: `path` (`load`, `import`, `collections`, `smart_collections`,
  `meta_get`, `meta_update`, `generative`, `regenerate` —
  `crates/lumina-mcp/src/tools/{load,import,collections,smart_collections,meta_get,meta_update,generative,regenerate}.rs`),
  `input` (`reindex`, `batch`, `dust_removal`), `output` (`batch`),
  `output_path` (`save`, `meta_export`), `from`/`to` (`relocate`),
  `inputs`/`output` (`merge_hdr`, `merge_pano` —
  `crates/lumina-mcp/src/tools/merge_common.rs`), `source`/`targets`
  (`meta_sync`), `render_out` (`dust_removal`).
- **Ausgabepfade** — `crates/lumina-mcp/src/tools/mod.rs`: `dispatch_tool`
  liefert das Werkzeugergebnis als JSON; `list_tool_definitions` liefert die
  Werkzeugliste für `tools/list`. Werkzeuge, die **auf die Platte schreiben**:
  `save` (`write_output_guarded`), `import`/`load`/`edit`/`meta_update`/
  `meta_sync`/`meta_preset` (`save_sidecar_if_unchanged`), `batch`
  (`write_output_guarded`), `dust_removal` (`write_output_guarded`,
  `save_sidecar`, `append_repair_region`), `meta_export` (`write_output_guarded`),
  `collections` (`save_sidecar` über `crates/lumina-mcp/src/tools/library_common.rs`),
  `relocate` (`lumina_stages::relocate` → `move_file_cross_volume`),
  `generative`/`regenerate` (`lumina_stages::generative`/`lumina_stages::regenerate`
  → `save_sidecar`), `merge_hdr`/`merge_pano` (`lumina_stages::merge` →
  `run_merge` → DNG-Datei), `spot`/`lens_blur`/`geometry`/`upright`
  (`stage_common::execute` → `commit` → `save_sidecar_if_unchanged`),
  `matrix` (`lumina_stages::matrix_run::run` → `std::fs::write`);
  `preview` schreibt eine PNG-Datei in
  `server.preview_dir` (`crates/lumina-mcp/src/tools/preview.rs`,
  `write_atomically`); `meta_get::resource_read` liefert eine Ressource über
  `resources/read`. Rein lesend: `inspect`, `analyze`, `copies`, `recipe`,
  `smart_collections`, `meta_get`, `reindex` (liest den Verzeichnisbaum über
  `collect_tree_files` und gibt das Ergebnis als JSON zurück — **kein** Schreibpfad).
- **Vertrauensgrenze** — Werkzeugargument (Modell-Eingabe) → Dateisystem des
  Serverprozesses: die Werkzeugargumente sind der **einzige** Weg vom
  aufrufenden Modell in den Pfadraum des Prozesses.
- **Zu prüfende Klassen** — Schema/Deserialisierung (JSON-Schema-Angabe vs.
  serverseitige Prüfung); Grenzwerte (`parse_bounded_uint`, Zahlenfelder);
  Pfadableitung/Traversal (Zielpfade aus Argumenten); Rechte/Dateimodus der
  geschriebenen Datei; Symlink/TOCTOU (Prüfung vs. Schreibzeitpunkt);
  Werkzeug-Dispatch (Name → Modul).
- **Beleg** — `grep -c "tool_def(" crates/lumina-mcp/src/tools/mod.rs`;
  `grep -h "pub const NAME" crates/lumina-mcp/src/tools/*.rs | sort`;
  `grep -n "pub fn list_tool_definitions\|pub fn dispatch_tool\|pub fn is_known_tool" crates/lumina-mcp/src/tools/mod.rs`.

### 5.1.3 MCP-Pfad-/Zielbehandlung

- **Eingabepfade** — `crates/lumina-mcp/src/output_guard.rs`:
  `reject_protected_target(source, target)` (beide Pfade stammen aus
  Werkzeugargumenten), `resolve_candidate` (Kanonisierung), `paths_are_same_file`
  (Hardlink-Identität). `crates/lumina-mcp/src/util.rs`:
  `write_output_guarded(source, target, bytes)`,
  `validate_output_extension(output, format)`, `parse_bounded_uint(args, key, min, max)`,
  `build_source_identity(path, bytes, frame, raw_metadata)` (liest
  `fs::metadata`), `parse_output_format(format)`.
- **Ausgabepfade** — `reject_protected_target` erzeugt **nur** einen
  `McpError` (kein Schreibvorgang); `write_output_guarded` schreibt über
  `lumina_sidecar::write_atomically` auf die Platte; `build_source_identity`
  liefert die `SourceIdentity` (Hash, Bytelänge, Geometrie) als
  Werkzeugergebnis; `validate_output_extension`/`parse_bounded_uint`
  liefern `McpError::InvalidParams`.
- **Vertrauensgrenze** — Werkzeugargument → Benutzerdateisystem: **beide
  Pfade sind unvertrauenswürdig**, weil sie aus demselben Aufruf stammen wie
  der Schreibwunsch.
- **Zu prüfende Klassen** — Pfadableitung/Traversal; Symlink/TOCTOU
  (Kanonisierung zum Prüfzeitpunkt vs. Schreibzeitpunkt); Hardlink-Alias
  (Dateiidentität vs. Pfadgleichheit); Rechte/Dateimodus; Fehlertext
  (Pfadinhalt im Fehler).
- **Beleg** — `grep -n "pub fn reject_protected_target\|fn paths_are_same_file\|fn resolve_candidate" crates/lumina-mcp/src/output_guard.rs`;
  `grep -n "pub fn write_output_guarded\|pub fn validate_output_extension\|pub fn parse_bounded_uint\|pub fn build_source_identity" crates/lumina-mcp/src/util.rs`.

### 5.1.4 MCP-Session-Zustand

- **Eingabepfade** — `crates/lumina-mcp/src/session.rs`: `require_id`,
  `require_id_mut` (Abgleich des Werkzeugarguments `image_id`),
  `ImageState::find_copy(name)` (Name oder Id einer virtuellen Kopie).
  Die Id selbst entsteht in `crates/lumina-mcp/src/lib.rs`:
  `Server::generate_image_id` (Prozesszähler + Zeitstempel + Pfadname,
  BLAKE3-Auszug).
- **Ausgabepfade** — `crates/lumina-mcp/src/session.rs`: `ImageState`
  (dekodierter Frame, `SidecarDocument`, `sidecar_revision`) im Speicher;
  Werkzeuge, die daraus schreiben, legen den Zustand über
  `save_sidecar_if_unchanged` auf `state.sidecar_path` ab
  (`crates/lumina-mcp/src/session.rs`, Feld `sidecar_path`);
  `Server::cleanup_previews` löscht die Vorschaudateien beim Herunterfahren.
- **Vertrauensgrenze** — Werkzeugargument `image_id` → prozesslokaler
  Sitzungszustand: die Id ist ein **prozesslokaler Bezeichner**, kein
  kryptografisches Token; der Sitzungszustand selbst (Frame + Sidecar) ist
  prozesslokal.
- **Zu prüfende Klassen** — Identifikatorraum/-ableitung (`generate_image_id`);
  Zustands- und Lebenszyklus (Ersetzen des geladenen Bildes);
  Compare-and-Swap-Revision (`sidecar_revision`); Aufräumen/Temp-Dateien
  (Preview-Verzeichnis, `cleanup_previews`).
- **Beleg** — `grep -n "fn require_id\|fn require_id_mut\|fn find_copy\|pub struct ImageState" crates/lumina-mcp/src/session.rs`;
  `grep -n "fn generate_image_id\|fn cleanup_previews" crates/lumina-mcp/src/lib.rs`.

### 5.1.5 CLI-Kommando-Oberfläche

- **Eingabepfade** — `crates/lumina-cli/src/main.rs`: `Cli::parse` (clap) füllt
  `struct Cli` mit `enum Command` (**33** Varianten: 32 mit Argument-Strukt +
  `Mcp`); `fn run(cli: Cli)`
  verzweigt in die Kommando-Implementierungen; die Argument-Strukturen
  (`struct FileArgs`, `struct ExportArgs`, `struct BatchArgs`, `struct MaskArgs`,
  `struct ProcessArgs`, `struct CullArgs`, `struct FaceArgs`,
  `struct DenoiseArgs` u. a., **33** Strukturen in `main.rs`) tragen Pfade,
  Zahlen, Qualität, Format und Masken-Pfade. Zusätzlich liest `main.rs` die
  Umgebungsvariablen `LUMINA_MODEL_PATH`, `LUMINA_FACE_DETECT_MODEL_PATH`,
  `LUMINA_FACE_EMBED_MODEL_PATH`.
- **Ausgabepfade** — `crates/lumina-cli/src/main.rs`: `fn main` schreibt
  Fehlertexte mit `eprintln!` auf **stderr** und beendet mit dem Exit-Code
  aus `CliError::exit_code`; die Kommandos schreiben ihre Resultate auf
  **stdout** (Text- und JSON-Ausgabe, `crates/lumina-cli/src/stages.rs`
  `print`, `crates/lumina-cli/src/library.rs` `print`); geschriebene Dateien:
  Render-/Exportziele, Sidecars, DNG-Dateien, verschobene Dateien
  (`library::relocate` → `crates/lumina-stages/src/relocate.rs` `run`).
- **Vertrauensgrenze** — Kommandozeile → Prozess: **argv ist nicht
  vertrauenswürdig**; alle Pfade, Zahlen und Qualitätswerte stammen daraus.
  Ausgabe: stdout/stderr des Prozesses sind die Schnittstelle zum aufrufenden
  Skript.
- **Zu prüfende Klassen** — Argument- und Zahlenbereich (je Kommando);
  Pfadableitung/Traversal; Symlink/TOCTOU; Rechte/Dateimodus der
  geschriebenen Dateien; Kanaltrennung stdout/stderr; Exit-Code-Semantik
  (welche Fehlerklasse welcher Code).
- **Beleg** — `grep -n "enum Command\|fn run(cli: Cli)\|fn main()\|enum CliError\|fn reject_protected_output\|fn resolve_candidate\|fn output_format" crates/lumina-cli/src/main.rs`;
  `awk '/^enum Command \{/,/^\}/' crates/lumina-cli/src/main.rs | grep -cE "^    [A-Z][A-Za-z]*\("`.

### 5.1.6 CLI-Submodule

- **Eingabepfade** — je Modul eigene Argument- und Aufrufschnittstellen, alle
  aus `fn run(cli: Cli)` in `crates/lumina-cli/src/main.rs` erreichbar:
  `crates/lumina-cli/src/auto_tone_cli.rs` (`AutoToneDomain::resolve`,
  `apply_preset_layer`, `apply_explicit_slider_values`),
  `crates/lumina-cli/src/cull_cli.rs` (`source_fingerprint`,
  `ensure_sidecar_source_matches`, `persist_one`),
  `crates/lumina-cli/src/lensfun_cli.rs` (`with_diagnostics`,
  `build_lensfun_corrector`), `crates/lumina-cli/src/mask_local.rs`
  (`resolve_mask_copy`, `require_mask_name`, `mask_copy_mut`) samt
  `mask_local_color.rs`, `mask_local_curves.rs`, `mask_local_detail.rs`,
  `mask_local_presence.rs`, `crates/lumina-cli/src/merge.rs`
  (`pub struct MergeArgs`), `crates/lumina-cli/src/matrix.rs`
  (`pub struct MatrixArgs`), `crates/lumina-cli/src/stages.rs`
  (`SpotArgs`, `LensBlurArgs`, `UprightArgs`, `GeometryArgs`, `decode_input`,
  `source_identity`), `crates/lumina-cli/src/library.rs`
  (`CollectionsArgs`, `SmartCollectionsArgs`, `RelocateArgs`,
  `GenerativeArgs`, `RegenerateArgs`, `relocate`).
- **Ausgabepfade** — `crates/lumina-cli/src/stages.rs` `print` und
  `crates/lumina-cli/src/library.rs` `print` (Text-/JSON-Ausgabe auf stdout);
  `crates/lumina-cli/src/library.rs` `relocate` verschiebt Dateien samt
  Begleitdateien (`crates/lumina-stages/src/paths.rs`
  `move_file_cross_volume`, `collect_target_sidecars`);
  `crates/lumina-cli/src/merge.rs` schreibt **selbst keine Datei**, sondern
  delegiert an `crates/lumina-merge/src/bundle.rs` `run_merge` (DNG-Datei +
  Merge-Sidecar — siehe §5.1.17); `crates/lumina-cli/src/matrix.rs` schreibt
  **selbst keine Datei**, sondern delegiert an
  `crates/lumina-stages/src/matrix_run.rs` `run` (Report-/Golden-Ausgaben —
  siehe §5.1.7); `crates/lumina-cli/src/cull_cli.rs` `persist_one` schreibt das
  Culling-`extras` in die Sidecar.
- **Vertrauensgrenze** — argv → Modul → Dateisystem: dieselbe
  Unvertrauenswürdigkeit wie §5.1.5, aber **je Modul ein eigener Pfadraum**
  (Bilder, Masken, Korrekturprofile, Rezeptmatrizen, Bibliotheksbäume).
- **Zu prüfende Klassen** — je Modul: Pfadableitung/Traversal, Grenzwerte
  (Masken-/Spot-/Bereichsangaben), Rechte/Dateimodus, Symlink/TOCTOU;
  zusätzlich die Modulgrenze selbst (welche Module schreiben, welche nur
  lesen).
- **Beleg** — `ls crates/lumina-cli/src/*.rs`;
  `grep -n "^mod " crates/lumina-cli/src/main.rs`;
  `grep -c "fs::write\|write_all\|File::create" crates/lumina-cli/src/matrix.rs crates/lumina-cli/src/merge.rs || echo "(0 Treffer in beiden Dateien)"` → `matrix.rs:0`, `merge.rs:0` (Delegation);
  `grep -n "pub fn relocate\|fn print" crates/lumina-cli/src/library.rs`;
  `grep -n "pub fn decode_input\|pub fn source_identity\|fn print" crates/lumina-cli/src/stages.rs`.

### 5.1.7 Gemeinsame Pfad-/Dateischicht (`lumina-stages`)

- **Eingabepfade** — `crates/lumina-stages/src/paths.rs`: `require_sidecar(input)`
  (Sidecar eines Bildes), `collect_target_sidecars(input)` (Datei, Sidecar oder
  Verzeichnis), `collect_sidecars(path, output)`, `load_smart_catalog(path)`
  (Katalogdatei); `crates/lumina-stages/src/relocate.rs`: `RelocateRequest`
  (`from`, `to`); `crates/lumina-stages/src/collections.rs`: `CollectionsRequest`
  (`input`, `add_to`, `remove_from`), `split_collection_assignment(value)`;
  `crates/lumina-stages/src/merge.rs`: `decode_source(input)`,
  `MergeExplicitInputs`; `crates/lumina-stages/src/matrix_run.rs`: `MatrixRequest`.
- **Ausgabepfade** — `crates/lumina-stages/src/relocate.rs` `run` verschiebt das
  Bild **und** seine Begleitdateien (`.lumina.json`, `.lumina.zdata`) über
  `paths::move_file_cross_volume`; `crates/lumina-stages/src/collections.rs` `run`
  schreibt die Sidecar über `lumina_sidecar::save_sidecar`;
  `crates/lumina-stages/src/matrix_run.rs` `run` erzeugt die Report-/
  Golden-Ausgaben; `crates/lumina-stages/src/paths.rs` `load_smart_catalog`
  liefert `Vec<SmartCollectionDef>`.
- **Vertrauensgrenze** — CLI- **und** MCP-Argumente → gemeinsame Dateischicht:
  **beide Frontends teilen sich diese Schicht**, ein über MCP gelieferter Pfad
  trifft denselben Code wie ein argv-Pfad; die Schicht ist daher für **beide**
  gleichermaßen unvertrauenswürdig.
- **Zu prüfende Klassen** — Pfadableitung/Traversal (`collect_target_sidecars`,
  `sidecar_path_for`); Verzeichniswalk (Symlink-/Zyklenverhalten,
  `entry.file_type()` folgt keinen Verzeichnis-Symbollinks); Symlink/TOCTOU
  (Vorhandenseinprüfung vs. Verschiebung); Rechte/Dateimodus (Zielverzeichnis;
  `fs::copy` + `remove_file` beim Cross-Volume-Fallback); Katalogdatei-Schema
  (`format`, `version`, `validate_smart_collection_def`).
- **Beleg** — `grep -n "pub fn require_sidecar\|pub fn collect_target_sidecars\|pub fn collect_tree_files\|pub fn collect_sidecars\|pub fn move_file_cross_volume\|pub fn load_smart_catalog" crates/lumina-stages/src/paths.rs`;
  `grep -n "pub fn run" crates/lumina-stages/src/relocate.rs crates/lumina-stages/src/collections.rs crates/lumina-stages/src/matrix_run.rs`;
  `grep -n "pub fn decode_source" crates/lumina-stages/src/merge.rs`.

### 5.1.8 Culling-Sidecar-Bindung (`lumina-cull`)

- **Eingabepfade** — `crates/lumina-cull/src/sidecar_io.rs`: `load_culling(path,
  current)` (liest die `.lumina.json` von Platte), `evaluate_section(section,
  current)`, `evaluate_culling(document, current)`; die Identität stammt aus
  `crates/lumina-cull/src/identity.rs` (`identity_mismatches`,
  `heuristic_identity`); das Analyseergebnis aus
  `crates/lumina-cull/src/analyze.rs` (`analyze_heuristic`, `analyze_selection`)
  und `crates/lumina-cull/src/config.rs` (`CullConfig`).
- **Ausgabepfade** — `record_culling(document, section)` setzt den
  `culling`-Block im Dokument, `clear_culling(document)` entfernt ihn,
  `save_culling(path, document)` schreibt die `.lumina.json` über
  `lumina_sidecar::save_sidecar`; `CullingReadState`
  (`NoProposal`/`Valid`/`Stale`/`Unusable`) ist der sichtbare Zustand.
- **Vertrauensgrenze** — Benutzerdateisystem → Prozess: die `.lumina.json` und
  der darin persistierte `culling`-Block sind **fremd**; die Identität
  vergleicht den persistierten Vorschlag mit dem aktuellen Kontext.
- **Zu prüfende Klassen** — Deserialisierung/Schema (`CullingSection`);
  Identitäts- und Veraltungslogik (`identity_mismatches`,
  `CullingReadState::Stale`); Validierung vor dem Schreiben
  (`validate_culling_section`); Grenzwerte des `score`- und `reasons`-Feldes;
  Rechte/Dateimodus der geschriebenen `.lumina.json`.
- **Beleg** — `grep -n "pub fn load_culling\|pub fn save_culling\|pub fn record_culling\|pub fn clear_culling\|pub fn evaluate_culling\|pub fn evaluate_section" crates/lumina-cull/src/sidecar_io.rs`;
  `grep -n "pub fn identity_mismatches\|pub fn heuristic_identity" crates/lumina-cull/src/identity.rs`;
  `grep -n "pub fn analyze_heuristic\|pub fn analyze_selection" crates/lumina-cull/src/analyze.rs`.

### 5.1.9 Verzeichnis-/Ordnerbaum-Ebene

- **Eingabepfade** — `crates/lumina-mcp/src/util.rs`: `collect_tree_files(path,
  output, keep)` (Wurzelverzeichnis aus dem Werkzeugargument),
  `collect_tree_files_inner`; `crates/lumina-stages/src/paths.rs`:
  `collect_tree_files`, `collect_tree_files_inner`, `collect_sidecars(path,
  output)`, `collect_target_sidecars(input)`. Die Werkzeuge, die diese Schicht
  betreten: `reindex` (Argument `input` — `crates/lumina-mcp/src/tools/reindex.rs`),
  `batch` (Argument `input` — `crates/lumina-mcp/src/tools/batch.rs`),
  `collections`/`smart_collections` (Argument `path`).
- **Ausgabepfade** — die **Liste der gefundenen Dateien** (Pfade, sortiert):
  einmal als Werkzeugergebnis (`reindex` liefert Zählwerte und je Datei den
  Fehlertext), einmal als Eingang der Folgeschritte (`batch` rendert und
  schreibt je Datei).
- **Vertrauensgrenze** — Verzeichnispfad (Werkzeugargument) → Dateisystem: die
  Wurzel ist **nicht vertrauenswürdig**; der Walk folgt **keinen**
  Verzeichnis-Symbollinks, fasst aber alles ein, was unterhalb liegt,
  einschließlich fremder Dateien.
- **Zu prüfende Klassen** — Verzeichniswalk (Tiefe, Anzahl, Speicher);
  Symlink-/Zyklenverhalten (`fs::canonicalize` als Besuchsidentität,
  `entry.file_type()` ohne Symlink-Following); Grenzwerte (Anzahl Dateien,
  Pfadlänge); TOCTOU zwischen Walk und Folgeschritt; Fehlerkanal (Pfadinhalt in
  der Fehlermeldung).
- **Beleg** — `grep -n "pub fn collect_tree_files\|fn collect_tree_files_inner" crates/lumina-mcp/src/util.rs crates/lumina-stages/src/paths.rs`;
  `grep -n "pub fn collect_sidecars\|pub fn collect_target_sidecars" crates/lumina-stages/src/paths.rs`;
  `grep -n "collect_tree_files" crates/lumina-mcp/src/tools/reindex.rs`.

### 5.1.10 Sidecar-JSON (Schreibpfad, CAS/Atomic-Write)

- **Eingabepfade** — `crates/lumina-sidecar/src/lib.rs`: `save_sidecar(path,
  document)`, `save_sidecar_if_unchanged(path, document, expected_revision)`
  (Compare-and-Swap), `write_atomically(path, bytes)`, `migrate_sidecar_file(path)`,
  `recover_sidecar(path)`, `acquire_write_lock(path)`; Eingang ist der
  `SidecarDocument`/`EditRecipe`-Wert (serde), erzeugt in CLI, MCP und GUI.
- **Ausgabepfade** — die `.lumina.json` **neben dem Original**
  (`crates/lumina-sidecar/src/sidecar_paths.rs` `sidecar_path_for`,
  `SIDECAR_FILE_SUFFIX`); geschrieben wird **atomar**: temporäre Datei
  `.{name}.tmp-*` im Zielverzeichnis, `flush` + `sync_all`, dann `persist`
  (Rename) über das Ziel, danach `sync_all` des Elternverzeichnisses
  (`save_sidecar_locked`). `write_atomically` nutzt dasselbe Muster ohne
  Verzeichnis-Sync. `recover_sidecar` **löscht** verwaiste Temporärdateien
  (Altersschwelle).
- **Vertrauensgrenze** — Prozess → Benutzerdateisystem: der Zielpfad wird
  **aus dem Quellpfad abgeleitet** (`sidecar_path_for`), und der Quellpfad
  stammt aus argv bzw. Werkzeugargument — er ist damit nicht vertrauenswürdig.
- **Zu prüfende Klassen** — Pfadableitung/Traversal (Suffixanhängung an
  einen beliebigen Namen); Rechte/Dateimodus der temporären Datei und des
  Ziels; Symlink/TOCTOU (Tempfile → Rename, Prüfung vs. Schreibzeitpunkt);
  Lock-Verwaltung (`acquire_write_lock`, Lock-Reclaim nach Prozessabbruch);
  Compare-and-Swap (Vergleich der `document_revision`);
  Crash-Konsistenz (`.tmp-*`-Reste).
- **Beleg** — `grep -n "pub fn save_sidecar\|pub fn save_sidecar_if_unchanged\|pub fn write_atomically\|pub fn migrate_sidecar_file\|pub fn recover_sidecar\|fn acquire_write_lock\|fn save_sidecar_locked\|pub fn document_revision" crates/lumina-sidecar/src/lib.rs`;
  `grep -n "pub fn sidecar_path_for\|SIDECAR_FILE_SUFFIX" crates/lumina-sidecar/src/sidecar_paths.rs`.

### 5.1.11 Sidecar-JSON (Lesepfad) und Pfadableitung

- **Eingabepfade** — `crates/lumina-sidecar/src/lib.rs`: `load_sidecar(path)`
  (Metadaten-Größengate `MAX_SIDECAR_BYTES`, dann `take`-begrenztes Lesen,
  UTF-8-Prüfung, `SidecarDocument::from_json`); `crates/lumina-sidecar/src/sidecar_paths.rs`:
  `sidecar_path_for(source)` leitet den Sidecar-Pfad aus dem Quellpfad ab.
- **Ausgabepfade** — der `SidecarDocument` im Speicher; er fließt weiter in
  CLI, MCP und GUI (Rezept, Masken, Metadatenentwurf) und wird über
  `document_revision` (BLAKE3 über die JSON-Serialisierung) zur
  Compare-and-Swap-Erwartung. Nebenwirkung des Lesens: `load_sidecar`
  ruft `recover_sidecar` auf und **löscht** dabei verwaiste Temporärdateien.
- **Vertrauensgrenze** — Benutzerdateisystem → Prozess: die `.lumina.json` ist
  eine **fremde Datei** (von einem anderen Prozess, einem anderen Werkzeug
  oder von Hand geschrieben) und vollständig unvertrauenswürdig.
- **Zu prüfende Klassen** — Deserialisierung/Schema (`from_json`,
  `schema_version`-Gate); Grenzwerte/Größen (`MAX_SIDECAR_BYTES` doppelt
  geprüft); Zeichenkodierung (UTF-8-Fehler); Parser-Tiefe/Verschachtelung
  (Rezept-/Maskenbaum); Pfadableitung (Suffix an beliebigen Namen);
  Nebenwirkung beim Lesen (Recovery löscht Dateien).
- **Beleg** — `grep -n "pub fn load_sidecar\|MAX_SIDECAR_BYTES\|pub fn document_revision\|pub fn recover_sidecar" crates/lumina-sidecar/src/lib.rs`;
  `grep -n "pub fn sidecar_path_for" crates/lumina-sidecar/src/sidecar_paths.rs`.

### 5.1.12 Sidecar-Begleitdatei `.lumina.zdata`

- **Eingabepfade** — `crates/lumina-sidecar/src/zdata.rs`: `load_zdata(path)`
  (Container-Header `MAGIC` = `LUMZDATA`, `VERSION`, `HEADER_LEN`),
  `parse_record`, `decode_payload` (Dekompression der Kachel-Nutzlast);
  Grenzwerte als Konstanten: `MAX_CONTAINER_BYTES`, `MAX_DIMENSION`,
  `MAX_TILE_VALUES`, `MAX_COMPRESSED`, `MAX_UNCOMPRESSED`, `MAX_RECORDS`,
  `MAX_ID_LEN`. Der Pfad wird über `zdata_path_for(source)` abgeleitet.
- **Ausgabepfade** — `ZDataContainer` mit `MaskTile` (Masken-Kacheln) im
  Speicher; schreibende Zugriffe: `append_repair_region`,
  `append_generative_canvas`, `save_generative_canvas`,
  `append_spot_heal_generative`, `append_denoise_rgb`, `save_denoise_rgb`,
  `save_face_embedding`, `save_face_embeddings`, `save_zdata` — jeweils in die
  `.lumina.zdata` neben dem Original.
- **Vertrauensgrenze** — Binärdatei → Prozess: die Nutzlast ist **komprimiert
  und fremd**; dekodierte Kacheln steuern später Masken und Render-Pipeline.
- **Zu prüfende Klassen** — Binärformat-/Längenprüfung (Header, Record-Header,
  Indexeinträge); Dekompressionsgrenze (Dekomprimierung gegen
  `MAX_UNCOMPRESSED`/`MAX_COMPRESSED`); Prüfsummenverifikation (BLAKE3 über
  die Blöcke); Integer-Überlauf in den Längenfeldern; Ressourcenbudget
  (`MAX_CONTAINER_BYTES`, `MAX_RECORDS`); Pfadableitung (`zdata_path_for`).
- **Beleg** — `grep -n "pub fn load_zdata\|pub fn save_zdata\|pub fn zdata_path_for\|fn parse_record\|fn decode_payload\|MAX_CONTAINER_BYTES\|MAX_DIMENSION\|MAX_RECORDS\|MAX_COMPRESSED" crates/lumina-sidecar/src/zdata.rs`.

### 5.1.13 Cache- und Vorschau-Pfade (im Speicher)

- **Eingabepfade** — `crates/lumina-core/src/cache.rs`: `preview_cache_key(source,
  virtual_copy)` (Schlüsselbildung), `CacheStore::get`/`put`
  (Schlüssel `RenderKey`, Stage), `CacheStore::invalidate`,
  `CacheStore::prune(live_keys)`; `crates/lumina-core/src/preview_cache.rs`:
  `LruPreviewCache::insert` (Schlüssel `PreviewKey`, Digest),
  `PreviewKey::digest`, `decode_webp(bytes)` (dekodiert Cachebytes);
  `crates/lumina-core/src/stage_cache.rs`: `StageFrameCache::insert`/`get`
  (Schlüsselstring).
- **Ausgabepfade** — **im Speicher** (kein Dateizugriff in `crates/lumina-core/src`):
  Bildframes (`ImageFrame`) und Byte-Puffer; Löschpfade `prune_orphans`
  (`FolderCache`), `LruPreviewCache::remove_entry`, `StageFrameCache::clear`.
- **Vertrauensgrenze** — Quellname/Virtual-Copy-ID/Rezept-Digest →
  Cache-Schlüssel: die Schlüssel entstehen aus **fremden** Dateinamen und
  Digest-Werten; die Cacheinhalte (WebP-Bytes) stammen aus dem
  On-Disk-Cache und sind nicht autoritativ.
- **Zu prüfende Klassen** — Schlüsselbildung/Kollision (Trennzeichen, Hash
  über Schlüssel); Lösch-/Prune-Verhalten (welche Einträge überleben);
  Bytegrenzen (`max_bytes`, `max_slots`); Kodierer/Dekodierer-Paar
  (`encode_webp_lossless`/`decode_webp`); Veraltungs-/Generationslogik
  (`CacheStore`).
- **Beleg** — `grep -n "pub fn preview_cache_key\|pub struct FolderCache\|pub struct CacheStore" crates/lumina-core/src/cache.rs`;
  `grep -n "pub struct LruPreviewCache\|pub fn encode_webp_lossless\|pub fn decode_webp\|pub struct PreviewKey" crates/lumina-core/src/preview_cache.rs`;
  `grep -n "pub struct StageFrameCache" crates/lumina-core/src/stage_cache.rs`.

### 5.1.14 Cache auf Platte

- **Eingabepfade** — `crates/lumina-cache/src/disk.rs`:
  `DiskFolderCache::effective_settings` liest die **`.lumina/settings.json`**
  der Ordnerkette (vom Dateisystemwurzelverzeichnis bis zum Bildordner),
  `load_preview`/`load_preview_with_source_hash` lesen die Vorschaudateien,
  `entry_stem(source, virtual_copy, kind)` bildet den Dateinamenstamm;
  `crates/lumina-cache/src/preview_disk.rs`: `PreviewDiskCache::load(key_digest)`
  liest und **dekodiert** den WebP-Byteinhalt (`decode_webp`).
- **Ausgabepfade** — der Ordner-Cache **im Bildordner**: `.lumina/settings.json`
  (`save_settings`, `atomic_write`) und `.lumina/previews/<stem>.json` + `.bin`
  (`store_preview`, `store_preview_with_source_hash`); `prune_orphans`
  löscht verwaiste Vorschau-Dateien; `preview_disk::store` schreibt die
  WebP-Bytes.
- **Vertrauensgrenze** — Benutzerdateisystem → Prozess **und zurück**: der
  Cacheordner `.lumina` liegt im Bildordner und ist für den Benutzer (und für
  jedes andere Werkzeug im selben Ordner) beschreibbar; die Cacheinhalte
  werden später **als Bildbytes** zurückgelesen. Der Cache ist damit ein
  **Nicht-autoritativer** Kanal, der den Decoderpfad erneut betritt.
- **Zu prüfende Klassen** — Pfadableitung (Schlüssel → Dateiname, Ordnerkette);
  Deserialisierung/Schema (`settings.json`, `PreviewRecord`); Grenzwerte
  (Eintrags-/Cachegröße); Symlink/TOCTOU (Prune gegen laufende Writes);
  Rechte/Dateimodus; Vertrauensgrenze Cache (Cache ist kein autoritativer
  Zustand); Dateinamen-/Stamm-Kollision.
- **Beleg** — `grep -n "pub fn entry_stem\|pub fn save_settings\|pub fn store_preview\|pub fn load_preview\|fn atomic_write\|pub const PREVIEWS_DIR" crates/lumina-cache/src/disk.rs`;
  `grep -n "pub fn store\|pub fn load\|pub fn prune_orphans" crates/lumina-cache/src/preview_disk.rs`.

### 5.1.15 `.lumina`-Cachewurzel im Bildordner

- **Eingabepfade** — `crates/lumina-cache/src/disk.rs`:
  `DiskFolderCache::for_image(image)` (aus dem Bildpfad), `in_folder(folder)`
  (aus dem Ordnerpfad); `crates/lumina-cache/src/preview_disk.rs`:
  `PreviewDiskCache::in_folder(folder)`.
- **Ausgabepfade** — das **Verzeichnis** `.lumina/` im Bildordner nebst
  `previews/` (`PREVIEWS_DIR`): angelegt durch `fs::create_dir_all` in
  `in_folder`; darin `settings.json` (`settings_path`) und die Vorschau-Dateien
  (`disk.rs` `store_preview`, `preview_disk.rs` `store` — letztere schreibt
  zusätzlich eine `{digest}.{pid}.tmp`-Zwischendatei **ohne** führenden Punkt — im
  Unterschied zur Sidecar-Schreibweise `.{name}.tmp-*`).
- **Vertrauensgrenze** — Bildordner → Prozess: die Cachewurzel liegt **im
  Ordner der Bilder**, also in einer vom Benutzer (und von jedem anderen
  Werkzeug im selben Ordner) beschreibbaren Zone; das Verzeichnis wird
  **angelegt**, wenn es fehlt.
- **Zu prüfende Klassen** — Pfadableitung (`.lumina/` im Bildordner,
  `previews/` darunter); Rechte/Dateimodus des **neu angelegten** Verzeichnisses;
  Symlink/TOCTOU (existiert `.lumina` schon als Datei oder als Symlink?);
  Grenzwerte (Eintragszahl, Dateigröße); Benutzerumgebung (Cacheort relativ
  zum Bildort).
- **Beleg** — `grep -n "pub fn for_image\|pub fn in_folder\|pub fn path\|pub fn settings_path\|pub const PREVIEWS_DIR" crates/lumina-cache/src/disk.rs`;
  `grep -n "pub fn in_folder\|fn file_path\|pub fn store" crates/lumina-cache/src/preview_disk.rs`.

### 5.1.16 RAW-/Bild-Decoding

- **Eingabepfade** — `crates/lumina-raw/src/lib.rs`: `decode_file(path)`,
  `decode_bytes(bytes, name)`, `decode_bytes_with_options(bytes, name, options)`,
  `read_metadata(path)`, `read_metadata_bytes(bytes, name)`; Formatwahl über
  `RAW_EXTENSIONS` und `is_raw_extension(extension)`. Rasterbilder laufen
  parallel über `ImageFrame::decode` (`crates/lumina-core/src/lib.rs`). Der Dateiname
  geht als **Kontext** in die Fehlermeldungen ein.
- **Ausgabepfade** — `RawImage`/`ImageFrame` (dekodierte Pixel) und
  `RawMetadata` (EXIF-Felder) im Speicher; die Freigabeentscheidung trifft
  `crates/lumina-core/src/memory.rs`: `MemoryBudget::check_decode(width, height,
  channels, bytes_per_channel)` und `MemoryBudget::check_mask(width, height)`.
- **Vertrauensgrenze** — fremde Bild-/RAW-Datei → Decoder (LibRaw) und
  Core-Decoder: die Datei ist **vollständig unvertrauenswürdig**, auch ihre
  Metadatenfelder.
- **Zu prüfende Klassen** — Ressourcenbudget/Abbruch (Pixelzahl, Bytegrenzen,
  Überlauf in `checked_mul`); Decoder-Memory (native Allokation);
  Metadaten-Parser (EXIF-Felder fremder Herkunft); Dateiname als
  Kontext/Diagnosekanal; Fehlertext-Ausgabe (fremder Dateiname im Text).
- **Beleg** — `grep -n "pub fn decode_file\|pub fn decode_bytes\|pub fn decode_bytes_with_options\|pub fn read_metadata\|pub const RAW_EXTENSIONS\|pub fn is_raw_extension" crates/lumina-raw/src/lib.rs`;
  `grep -n "pub fn check_decode\|pub fn check_mask" crates/lumina-core/src/memory.rs`.

### 5.1.17 Merge-/DNG-Ausgabe

- **Eingabepfade** — `crates/lumina-merge/src/bundle.rs`: `run_merge(mode,
  sources, output)` (Quellframes + Rezept), `resolve_dng_path(mode, sources, output)`
  (Zielpfad aus `--output` **oder** abgeleitet aus dem Referenz-Quellnamen),
  `stored_merge_recipe(document)` und `stored_merge_artifact(document)` (Rezept
  und Artefaktverweis aus der Sidecar); `crates/lumina-merge/src/digest.rs`:
  `hdr_inputs_digest`, `pano_inputs_digest`, `merge_inputs_digest`;
  `crates/lumina-merge/src/bundle.rs`: `merge_checksum(bytes)`.
- **Ausgabepfade** — die **DNG-Datei**: `encode_linear_dng` (DNG-TIFF-Struktur),
  `write_merge_dng` (Schreiben), `merge_dng_filename(reference_file_name, mode)`
  (Namensableitung, `HDR_DNG_SUFFIX`/`PANO_DNG_SUFFIX`),
  `validate_dng_file_name(file_name)`; EXIF-Inhalt über
  `exif_timestamp_utc(timestamp)` und `validate_exif(exif)`;
  Nebenprodukt `merge_checksum` (Prüfsumme) und der Bundle-Eintrag im Rezept.
- **Vertrauensgrenze** — Rezept-/Quellframes → Binärdatei: der Zielpfad
  stammt aus argv/Werkzeugargument; der **Inhalt** stammt aus einem fremden
  Rezept (Sidecar) und wird in ein Binärformat geschrieben, das später wieder
  gelesen wird.
- **Zu prüfende Klassen** — Pfadableitung (Dateiname aus dem Quellnamen,
  Suffixanhängung); DNG-Struktur/EXIF-Validierung (`validate_exif`,
  `validate_dimensions`, `DNG_MIN_DIMENSION_PX`/`DNG_MAX_DIMENSION_PX`);
  Grenzwerte (Frame-Anzahl, `validate_source_count`); Binärschreiben/Rechte;
  Ressourcenbudget (Lineare Frames in RAM).
- **Beleg** — `grep -n "pub fn encode_linear_dng\|pub fn write_merge_dng\|pub fn merge_dng_filename\|pub fn validate_dng_file_name\|pub fn exif_timestamp_utc\|DNG_MAX_DIMENSION_PX" crates/lumina-merge/src/dng.rs`;
  `grep -n "pub fn run_merge\|pub fn merge_checksum\|fn resolve_dng_path\|pub fn stored_merge_recipe\|pub fn stored_merge_artifact" crates/lumina-merge/src/bundle.rs`;
  `grep -n "pub fn merge_inputs_digest\|pub fn hdr_inputs_digest\|pub fn pano_inputs_digest" crates/lumina-merge/src/digest.rs`.

### 5.1.18 Metadaten (IPTC/XMP/IIM/JPEG)

- **Eingabepfade** — `crates/lumina-iptc/src/iim.rs`: `decode_iim(data)`,
  `parse_app13(payload)`; `crates/lumina-iptc/src/xmp.rs`:
  `parse_xmp_packet(packet)`, `parse_app1(payload)`;
  `crates/lumina-iptc/src/jpeg.rs`: `extract_metadata(jpeg)` (JPEG-Segmentierung,
  `MAX_SEGMENT_PAYLOAD`); das Feldvokabular und seine Grenzen stehen in
  `crates/lumina-iptc/src/registry.rs` (`REGISTRY`, `MAX_KEYWORDS`,
  `MAX_KEYWORD_OCTETS`, `field_by_id`, `field_by_iim`).
- **Ausgabepfade** — `crates/lumina-iptc/src/xmp.rs`: `build_xmp_packet(meta)`,
  `build_app1(packet)`; `crates/lumina-iptc/src/iim.rs`: `encode_iim(meta)`,
  `build_app13(iim)`; `crates/lumina-iptc/src/jpeg.rs`:
  `embed_metadata(jpeg, meta)` — schreibt **APP1/APP13-Segmente zurück** in die
  Bilddatei; außerdem `IptcMetadata` als Entwurf im Sidecar-Metadatenblock
  (`crates/lumina-sidecar/src/lib.rs`).
- **Vertrauensgrenze** — Bilddatei → Parser **und** Parser → Bilddatei: die
  Metadaten sind **fremder Text** (XML/IIM) und werden in die Ausgabedatei
  **zurückgeschrieben**; Encoder und Decoder sind ein Paar, dessen
  Round-Trip-Verhalten die Kette schließen muss.
- **Zu prüfende Klassen** — Parser-Grenzen (Längen, Verschachtelung, XML-Entities
  über `xml_unescape`); Segmentgrößen (`MAX_SEGMENT_PAYLOAD`);
  Feldgrenzen (`MAX_KEYWORDS`, `MAX_KEYWORD_OCTETS`, `check_text`);
  Kodierer/Decoder-Paar-Konsistenz (IIM ↔ XMP ↔ JPEG);
  Zeichensatz (`CHARSET_UTF8`, Datumsformat `iim_date`/`parse_iim_date`).
- **Beleg** — `grep -n "pub fn build_xmp_packet\|pub fn parse_xmp_packet\|pub fn build_app1\|pub fn parse_app1" crates/lumina-iptc/src/xmp.rs`;
  `grep -n "pub fn encode_iim\|pub fn decode_iim\|pub fn build_app13\|pub fn parse_app13" crates/lumina-iptc/src/iim.rs`;
  `grep -n "pub fn embed_metadata\|pub fn extract_metadata\|MAX_SEGMENT_PAYLOAD" crates/lumina-iptc/src/jpeg.rs`;
  `grep -n "pub const REGISTRY\|MAX_KEYWORDS\|MAX_KEYWORD_OCTETS" crates/lumina-iptc/src/registry.rs`.

### 5.1.19 Rezept-/JSON-Eingaben (allgemein)

- **Eingabepfade** — `crates/lumina-sidecar/src/lib.rs`:
  `SidecarDocument::from_json(json)` (Deserialisierung + Versionsgate
  `schema_version`), `SidecarDocument::validate`, `EditRecipe` (serde-`Deserialize`);
  `crates/lumina-sidecar/src/local_adjustments/wire.rs`:
  `parse_wire_block`, `parse_wire_curves`, `parse_wire_number` (Zahlparsing der
  Local-Adjustment-Blöcke); `crates/lumina-sidecar/src/local_adjustments/migration.rs`:
  `validate_mask_layer_local_state`, `normalize_legacy_local_adjustments`.
  Feldvalidatoren in `crates/lumina-sidecar/src/lib.rs`:
  `validate_metadata_field_value`, `validate_metadata_origin`,
  `validate_metadata_timestamp`, `validate_relative_path`, `validate_artifact`,
  `validate_smart_collection_def`, `validate_smart_rule`, `validate_keyword`,
  `validate_collection_id`.
- **Ausgabepfade** — die validierte Rezeptstruktur (`SidecarDocument`,
  `EditRecipe`) im Speicher; `SidecarDocument::to_json`
  (`crates/lumina-sidecar/src/local_adjustments/migration.rs`) serialisiert sie
  **und** validiert sie erneut — die Re-Serialisierung ist der Weg in die
  `.lumina.json` (siehe §5.1.10). Der Rezeptinhalt steuert später die
  Render-Pipeline und die Auflösung von `ArtifactReference`-Pfaden.
- **Vertrauensgrenze** — JSON-Datei bzw. JSON-RPC-Fragment → Rezeptstruktur:
  das Rezept ist **fremd** und beschreibt sowohl Bildbearbeitung als auch
  **Pfade** (`ArtifactReference`) — die Pfadfelder sind der sicherheitsrelevante
  Teil.
- **Zu prüfende Klassen** — Deserialisierung/Schema; Versions- und
  Migrationslogik (`schema_version`, `migrate_json`, `migrate_sidecar_file`);
  Grenzwerte je Feld; Pfadvalidierung (relative Pfade, `..`-Ablehnung in
  `validate_relative_path`, `validate_artifact`); rekursive Struktur- und
  Tiefengrenzen (Maskenbaum, `validate_smart_rule` mit `depth`);
  Zahlenbereich (Nonfinite/NaN in `parse_wire_number`).
- **Beleg** — `grep -n "pub fn from_json\|pub fn validate\b\|fn validate_relative_path\|fn validate_artifact\|fn validate_smart_rule\|pub fn validate_metadata_field_value" crates/lumina-sidecar/src/lib.rs`;
  `grep -n "fn parse_wire_block\|fn parse_wire_number" crates/lumina-sidecar/src/local_adjustments/wire.rs`;
  `grep -n "pub fn validate_mask_layer_local_state\|pub fn to_json" crates/lumina-sidecar/src/local_adjustments/migration.rs`.

### 5.1.20 Meta-Präsets (Dateiformat + Benutzerkonfigurationsverzeichnis)

- **Eingabepfade** — `crates/lumina-sidecar/src/meta_preset.rs`:
  `load_meta_preset_file(path)` (JSON-Datei von Platte), `scan_meta_presets_dir(dir)`
  (Verzeichnislisting), `resolve_meta_preset_path(spec, explicit_dir)`,
  `meta_preset_filename(name)`, `default_meta_presets_dir()` (aus `HOME`/
  `APPDATA`/`XDG_CONFIG_HOME`), `render_meta_preset(preset, vars, path_display)`;
  `crates/lumina-gui/src/presets.rs`: `load_preset_file(path)`,
  `scan_presets_dir(dir)`, `preset_filename(name)`, `default_presets_dir()`.
- **Ausgabepfade** — `crates/lumina-sidecar/src/meta_preset.rs`:
  `MetaPresetFile`/`MetaPresetEntry` und der gerenderte Platzhaltertext
  (`render_meta_preset`); `crates/lumina-gui/src/presets.rs`:
  `save_preset_file(dir, preset, overwrite)` schreibt `<name>.lumina-preset.json`
  **atomar** über `lumina_sidecar::write_atomically` (Tempdatei + Rename),
  `PresetEntry` aus `scan_presets_dir`.
- **Vertrauensgrenze** — Benutzerkonfigurationsverzeichnis → Prozess: die
  Präsets sind **fremde JSON-Dateien**; ihre Platzhalter werden in
  Metadatenwerte eingesetzt (`render_meta_preset`). Umgekehrt schreibt die GUI
  in dasselbe Verzeichnis.
- **Zu prüfende Klassen** — Deserialisierung/Schema (`META_PRESET_FORMAT`,
  `META_PRESET_VERSION`, `PRESET_FORMAT`, `PRESET_SCHEMA_VERSION`);
  Dateinamenableitung (`meta_preset_filename`, `preset_filename`: Trennzeichen,
  `.`/`..`, Steuerzeichen, abschließender Punkt); Pfadableitung
  (`resolve_meta_preset_path` — ein `spec` mit Trennzeichen gilt als Pfad);
  Platzhalter-/Template-Auswertung (Gruppenauswertung, Variablenauflösung);
  Grenzwerte (Feldlängen, Schlüsselanzahl); Rechte/Dateimodus; Kollision beim
  Überschreiben (`overwrite`).
- **Beleg** — `grep -n "pub const META_PRESET_FORMAT\|pub const META_PRESET_FILE_SUFFIX\|pub fn default_meta_presets_dir\|pub fn meta_preset_filename\|pub fn resolve_meta_preset_path\|pub fn load_meta_preset_file\|pub fn scan_meta_presets_dir\|pub fn render_meta_preset" crates/lumina-sidecar/src/meta_preset.rs`;
  `grep -n "pub const PRESET_FORMAT\|pub const PRESET_FILE_SUFFIX\|pub fn default_presets_dir\|pub fn preset_filename\|pub fn save_preset_file\|pub fn load_preset_file\|pub fn scan_presets_dir" crates/lumina-gui/src/presets.rs`.

### 5.1.21 ONNX-Modell- und Artefakt-Eingaben

- **Eingabepfade** — `crates/lumina-onnx/src/manifest.rs`:
  `ModelManifest::from_json(json)`, `ModelManifest::validate`,
  `to_model_identity`; `crates/lumina-onnx/src/hash.rs`:
  `verify_model_file(path, expected)`, `compute_sha256_hex(reader)`,
  `verify_model_hash(expected, actual)`; `crates/lumina-onnx/src/resolve.rs`:
  `try_load_onnx_engine(model_path, manifest)`. Der Modellpfad kommt aus der
  Prozessumgebung (`LUMINA_MODEL_PATH`, `LUMINA_FACE_DETECT_MODEL_PATH`,
  `LUMINA_FACE_EMBED_MODEL_PATH` in `crates/lumina-cli/src/main.rs`).
- **Ausgabepfade** — `ModelIdentity` (Modellidentität, Hash, Eingabenspezifikation)
  wandert in das Rezept/Sidecar (`to_model_identity`); die geladene Engine
  selbst bleibt im Speicher; `OnnxEngine::RuntimeDisabled` bzw.
  `OnnxError::MissingModel`/`ModelArtifactStale`/`InferenceFailed` sind die
  sichtbaren Zustände statt eines stillen Fallbacks.
- **Vertrauensgrenze** — Modellartefakt + Manifest von Platte → Prozess: das
  Binärartefakt ist **fremd** und wird in eine native Runtime geladen; der
  Modellpfad stammt aus der **Umgebung** des Prozesses.
- **Zu prüfende Klassen** — Artefaktidentität/Hash-Prüfung
  (`verify_model_file`); Pfadableitung über die Umgebung; Grenzwerte
  (Artefaktgröße, Embedding-Dimension `MAX_FACE_EMBEDDING_DIMENSION` in
  `crates/lumina-sidecar/src/zdata.rs`); Manifest-Schema (`from_json`,
  `validate`); Laden in eine native Runtime (Eingabedaten des Graphen);
  Fehlertext (Pfadinhalt im Fehler).
- **Beleg** — `grep -n "pub fn from_json\|pub fn validate\|pub fn to_model_identity" crates/lumina-onnx/src/manifest.rs`;
  `grep -n "pub fn verify_model_file\|pub fn verify_model_hash\|pub fn compute_sha256_hex" crates/lumina-onnx/src/hash.rs`;
  `grep -n "pub fn try_load_onnx_engine" crates/lumina-onnx/src/resolve.rs`;
  `grep -n "LUMINA_MODEL_PATH\|LUMINA_FACE_DETECT_MODEL_PATH\|LUMINA_FACE_EMBED_MODEL_PATH" crates/lumina-cli/src/main.rs`.

### 5.1.22 Lensfun-Datenbank (Systempfade)

- **Eingabepfade** — `crates/lumina-lensfun/src/db_path.rs`: `resolve()`
  (Einstieg aus der Prozessumgebung via `EnvValues::from_process`),
  `resolve_with(env, compiled, xdg, home, probe)`,
  `xml_files_in(dir)` (Verzeichnislisting der Datenbank),
  `candidates(env, compiled)`; `crates/lumina-lensfun/src/db_user_dir.rs`:
  `user_db_dir()`, `user_db_dir_from(xdg, home)`;
  `crates/lumina-lensfun/src/system_load.rs`: `report_with(...)` liest die
  gefundenen XML-Dateien (Korrekturprofile) ein.
- **Ausgabepfade** — `Resolved` (Verzeichnis, Schema-Unterverzeichnis,
  XML-Dateiliste, Layer-Plan, `skipped`-Liste) im Speicher; in die
  Systemdatenbank wird **nicht** geschrieben — die Benutzerebene liegt unter
  `~/.local/share/lensfun` (Schreibzugriffe dort sind eine eigene Frage).
- **Vertrauensgrenze** — Prozessumgebung und Dateisystem → Prozess:
  `LUMINA_LENSFUN_DB` (`DB_DIR_ENV`) **überschreibt** den Systempfad; die
  gefundenen XML-Dateien sind **fremde Daten**, die als Korrekturprofile in die
  Bildberechnung einfließen.
- **Zu prüfende Klassen** — Umgebungsvariablen (`LUMINA_LENSFUN_DB`,
  `XDG_DATA_HOME`, `HOME`); Pfadableitung (relativer Override wird in
  `resolve_primary` abgelehnt, da das Arbeitsverzeichnis je nach Start
  variiert); XML-Parsing fremder Dateien; Symlink/TOCTOU im Verzeichnislisting;
  Rechte/Lesbarkeit der Systemverzeichnisse; Schreibpfad der Benutzerebene.
- **Beleg** — `grep -n "pub fn resolve\|pub fn resolve_with\|pub fn xml_files_in\|pub fn candidates\|DB_DIR_ENV\|fn from_process" crates/lumina-lensfun/src/db_path.rs`;
  `grep -n "pub fn user_db_dir\|pub fn user_db_dir_from" crates/lumina-lensfun/src/db_user_dir.rs`;
  `grep -n "pub fn report_with" crates/lumina-lensfun/src/system_load.rs`.

### 5.1.23 GUI-Dateieingabe und -ausgabe — **gleichrangig geprüft (Eigentümerentscheid 2026-10-20)**

- **Eingabepfade** — `crates/lumina-gui/src/dropped_files.rs`:
  `accept_dropped_file(path, read_bytes)` (Drag-and-Drop: entweder ein
  **Pfad** oder **Bytes**); `crates/lumina-gui/src/open_routing.rs`:
  `open_file_deferred(path)`; `crates/lumina-gui/src/lib.rs`: `load_bytes(bytes,
  name)`, `begin_load_path(path)`; Dateidialog über **Aufrufe der Fremdbibliothek
  `rfd`** — das sind **keine** LuminaRust-Symbole, sondern Aufrufstellen von
  `rfd::FileDialog`: `crates/lumina-gui/src/develop_frame.rs`
  (`rfd::FileDialog::new().pick_file()`), `crates/lumina-gui/src/lib.rs`
  (`rfd::FileDialog::new().pick_folder()`), `crates/lumina-gui/src/export.rs`
  (`rfd::FileDialog::new().set_file_name(&suggested).save_file()`);
  `crates/lumina-gui/src/file_browser_entry.rs`: `FileBrowserEntry` und
  `thumbnail_key(path)` (Ordnerlisting, kanonisierter Pfad als Thumbnailschlüssel).
- **Ausgabepfade** — `crates/lumina-gui/src/export.rs`: `export_to(output)`
  rendert und schreibt über `lumina_sidecar::write_atomically`;
  `resolve_export_target(source, output, extension)` setzt die Endung und prüft
  das Ziel gegen Quelle, Sidecar und `.lumina.zdata`;
  `paths_resolve_equal_symmetric(a, b)`; daneben `crates/lumina-gui/src/presets.rs`
  (`save_preset_file`, `default_presets_dir` → Benutzerkonfigurationsverzeichnis).
- **Vertrauensgrenze** — Benutzeraktion (Dialog/Drop) → Prozess: die vom
  Benutzer gewählten Pfade sind die **einzige** Eingabe, der **Inhalt** der
  gewählten Datei bleibt fremd. Die GUI ist eine lokale Desktop-Oberfläche
  mit derselben Datei-Eingabe-/Ausgabeklasse wie CLI und MCP. Die Scope-Frage
  ist **entschieden**: die GUI wird **gleichrangig** in **jeder** Welle
  mitgeprüft (Eigentümerentscheid 2026-10-20).
- **Zu prüfende Klassen** — Pfadableitung/Traversal; Symlink/TOCTOU
  (Exportziel, Zielprüfung vs. Schreibzeitpunkt); Rechte/Dateimodus der
  exportierten Datei; Nebenwirkungen beim Öffnen (Sidecar-Anlage/-Migration);
  Benutzerkonfigurationspfade (`default_presets_dir`).
- **Beleg** — `grep -n "fn accept_dropped_file" crates/lumina-gui/src/dropped_files.rs`;
  `grep -n "fn export_to\|fn resolve_export_target\|fn paths_resolve_equal_symmetric" crates/lumina-gui/src/export.rs`;
  `grep -n "fn open_file_deferred" crates/lumina-gui/src/open_routing.rs`;
  `grep -n "pub struct FileBrowserEntry\|fn thumbnail_key" crates/lumina-gui/src/file_browser_entry.rs`;
  `grep -n "rfd::FileDialog" crates/lumina-gui/src/develop_frame.rs crates/lumina-gui/src/lib.rs crates/lumina-gui/src/export.rs`.

### 5.1.24 Prozess-Umgebung

- **Eingabepfade** — `crates/lumina-core/src/memory.rs`:
  `MemoryBudget::from_env` liest `LUMINA_MAX_RAW_PIXELS`,
  `LUMINA_MAX_MASK_PIXELS`, `LUMINA_MAX_ALLOC_BYTES`;
  `crates/lumina-mcp/src/lib.rs`: `Server::new` liest `LUMINA_MCP_PREVIEW_DIR`,
  `TMPDIR`, `LUMINA_MCP_KEEP_PREVIEWS`, `init_logger` liest `LUMINA_MCP_LOG`;
  `crates/lumina-lensfun/src/db_path.rs`: `EnvValues::from_process` liest
  `LUMINA_LENSFUN_DB`, `LUMINA_LENSFUN_COMPILED_DATADIR`, `XDG_DATA_HOME`,
  `HOME`; `crates/lumina-cli/src/main.rs`: `LUMINA_MODEL_PATH`,
  `LUMINA_FACE_DETECT_MODEL_PATH`, `LUMINA_FACE_EMBED_MODEL_PATH`;
  `crates/lumina-stages/src/matrix_validate.rs`: `MATRIX_RECIPE_SET_ENV`,
  `MATRIX_DIR_ENV`.
- **Ausgabepfade** — die **Entscheidungen**, die aus der Umgebung folgen:
  Budgetgrenzen (Freigabe oder Ablehnung eines Decodes), das Preview-Verzeichnis
  samt der darin geschriebenen PNG-Dateien, die Logstufe auf stderr, die
  aufgelösten Systemdatenbank- und Modellpfade, die Matrix-Rezeptquelle.
- **Vertrauensgrenze** — Prozessumgebung → Prozessverhalten: die Umgebung ist
  **vom Aufrufer gesetzt** und damit Eingabe; sie steuert Pfade **und**
  Ressourcenlimits — dieselbe Quelle darf nicht gleichzeitig als
  Vertrauensanker gelten.
- **Zu prüfende Klassen** — Umgebungsvariablen (Parsing, Fallback bei
  ungültigem Wert, z. B. `from_env` fällt auf den Default zurück);
  Pfadableitung über die Umgebung (`TMPDIR`, `XDG_DATA_HOME`, `HOME`);
  Ressourcengrenzen (Wirkung der Limits auf den Decoderpfad);
  Voreinstellungswirkung (Default-Policy bei fehlender Variable).
- **Beleg** — `grep -n "fn from_env\|LUMINA_MAX_RAW_PIXELS\|LUMINA_MAX_MASK_PIXELS\|LUMINA_MAX_ALLOC_BYTES" crates/lumina-core/src/memory.rs`;
  `grep -n "LUMINA_MCP_KEEP_PREVIEWS\|LUMINA_MCP_PREVIEW_DIR\|LUMINA_MCP_LOG\|fn keep_previews_from_env" crates/lumina-mcp/src/lib.rs`;
  `grep -n "DB_DIR_ENV\|COMPILED_DATADIR_ENV\|XDG_DATA_HOME_ENV\|HOME_ENV" crates/lumina-lensfun/src/db_path.rs`.

---

## 5.2 Offene Beobachtungen aus der Enumeration (keine Befunde)

Die folgenden Punkte sind **Beobachtungen der Enumeration**, ausdrücklich **keine
Befunde** (§3 bleibt bei null Befunden). Sie betreffen die **Vollständigkeit der
§5-Basis**, nicht den Zustand einer Oberfläche.

**(a) Die Basis-Erweiterung vom 2026-10-20 (Eigentümerentscheid) — abgeschlossen:**

1. **CLI-Module ergänzt.** Die Zeile *CLI-Submodule* in §5 nennt jetzt auch
   `crates/lumina-cli/src/lensfun_gpu.rs` und
   `crates/lumina-cli/src/local_mask_gpu.rs`; die Zeile vermerkt ausdrücklich,
   dass es **keine** Subcommands mit eigenen Argumenten sind, sondern
   GPU-Binder/-Compositing-Module, die `fn run(cli: Cli)` über
   `render_best_effort` erreicht (`lensfun_gpu::classify`, `lensfun_gpu::bind`,
   `local_mask_gpu::mask_layer_routing_reasons`, `local_mask_gpu::composite`).
2. **Fünf neue Oberflächen-Zeilen** in §5: *Gemeinsame Pfad-/Dateischicht
   (`lumina-stages`)*, *Culling-Sidecar-Bindung (`lumina-cull`)*,
   *Verzeichnis-/Ordnerbaum-Ebene*, *`.lumina`-Cachewurzel im Bildordner* und
   *Meta-Präsets (Dateiformat + Benutzerkonfigurationsverzeichnis)* — jede mit
   enumeriertem Abschnitt in §5.1 (5.1.7, 5.1.8, 5.1.9, 5.1.15, 5.1.20).
3. **Nicht aufgenommen: `crates/lumina-gpu/src`.** Das Crate hat **kein**
   Datei-I/O — `grep -rln "std::fs\|fs::read\|fs::write\|File::open" crates/lumina-gpu/src/ || echo "(keine Treffer)"`
   liefert **keine** Treffer; es ist damit keine eigene
   Datei-Eingabe-/Ausgabe-Oberfläche.

**(b) Eingabe-/Ausgabepfade, die auch nach der Erweiterung in keiner §5-Zeile
stehen** (Kandidaten für eine **weitere** Basis-Erweiterung):

- **Weitere Umgebungsvariablen.** Die Zeile *Prozess-Umgebung* nennt in der
  Pfadspalte nur `crates/lumina-core/src/memory.rs` und
  `crates/lumina-mcp/src/lib.rs`; gelesen werden außerdem
  `LUMINA_MCP_LOG`, `LUMINA_MCP_PREVIEW_DIR`, `TMPDIR`,
  `LUMINA_LENSFUN_DB`, `LUMINA_LENSFUN_COMPILED_DATADIR`, `XDG_DATA_HOME`,
  `HOME`, `LUMINA_MODEL_PATH`, `LUMINA_FACE_DETECT_MODEL_PATH`,
  `LUMINA_FACE_EMBED_MODEL_PATH` sowie `MATRIX_RECIPE_SET_ENV`/`MATRIX_DIR_ENV`
  (`crates/lumina-stages/src/matrix_validate.rs`) — alle in §5.1.24
  aufgeführt, aber **nicht** in der Pfadspalte der Basiszeile.
- **Der Smart-Collection-Katalog als eigenes Format** —
  `crates/lumina-stages/src/paths.rs` `load_smart_catalog` liest eine
  Katalogdatei (`format` = `lumina-smart-catalog`, `version`), die **nicht** das
  Sidecar-Format ist; sie fällt derzeit unter die Zeile *Gemeinsame
  Pfad-/Dateischicht*.
- **Der Rezept-Export der GUI** (`crates/lumina-gui/src/presets.rs`
  `save_preset_file`) und der **Meta-Präset-Leser** der CLI (`meta preset list|show|apply` — die CLI schreibt nichts ins Präsets-Verzeichnis) teilen sich ein
  Verzeichnis (`<config base>/lumina/presets`), aber **nicht** das Format — die
  Zeile *Meta-Präsets* deckt beide Formate in einer Zeile ab.

**(c) Stellen, an denen die Enumeration nicht entscheidbar war:**

1. **Laufzeitentstehende Pfade.** Der endgültige Speicherort der
   MCP-Vorschau (`crates/lumina-mcp/src/lib.rs` `Server::new` →
   `LUMINA_MCP_PREVIEW_DIR` bzw. `$TMPDIR/lumina-previews`), der Cachewurzel
   (`.lumina/` im Bildordner) und des Benutzerkonfigurationsverzeichnisses
   (`default_presets_dir`, `default_meta_presets_dir`) entsteht erst zur
   Laufzeit; aus dem Code lässt sich nur die **Ableitungsregel** belegen, nicht
   der konkrete Pfad. Die Enumeration nennt deshalb die Regel, nicht den Pfad.
2. **Wirkung der Umgebung.** Welche Variablen in einer konkreten
   Prozessumgebung gesetzt sind, ist statisch nicht feststellbar; die
   Enumeration nennt die gelesenen Variablen und die jeweilige Default-Wirkung,
   nicht den tatsächlichen Wert.
3. **Der tatsächliche Aufrufer der MCP-Oberfläche.** Ob ein Werkzeug
   pfadbasiert oder sitzungsbasiert genutzt wird, entscheidet der aufrufende
   Client; die Enumeration trennt deshalb nach Werkzeugtyp, nicht nach dem
   konkreten Client.
4. **Die GUI-Dateidialoge.** `rfd::FileDialog` ist eine Fremdbibliothek; die
   Auswahl-Semantik (z. B. ob ein Verzeichnis statt einer Datei gewählt werden
   kann) ist aus dem LuminaRust-Code nicht entscheidbar und wurde deshalb
   nicht behauptet (§5.1.23 nennt die Aufrufstellen, nicht ein Symbol).
5. **`Command`-Variantenzahl in §5 — geprüft, die Zähl-Clause unverändert.** Die Zählung
   ergab **33** Varianten (32 mit Argument-Strukt plus `Mcp`); die
   §5-Formulierung („32 Varianten mit Argument-Strukt, zusätzlich die
   Feature-gate-Variante `Mcp`") ist genau das und **bleibt unverändert** — die
   §5-Tabelle selbst hat sich in dieser Welle geändert (19 -> 24 Zeilen, Status-Spalte umgestellt).
   Die Zählregex `^    [A-Z][A-Za-z]*\(` erfasst `Mcp` nicht, weil die
   Variante ohne Klammer steht — der Zählhinweis in §5.1 weist darauf hin.
6. **Die Zuordnung der Klassen.** Die in §5.1 je Oberfläche genannten Klassen
   sind ein **Vorschlag** (welche Prüffelder die Welle zu öffnen hat), **keine**
   Bewertung und **keine** Aussage darüber, ob an der Stelle etwas fehlerhaft
   ist.
---

## 6. Was dieses Register nicht ist

- **Kein Audit-Ergebnis.** Dieses Dokument hat keine Oberfläche geprüft; es
  enthält keine Befunde, keine Suchergebnisse und keine Trefferliste.
- **Keine Freigabe.** Das Bestehen dieses Registers ist **keine** Aussage über
  den Zustand des Codes, der Oberflächen oder einer Release-Freigabe.
- **Kein Ersatz für den Review-Ablauf.** Die Hälften „Finden" und „Schließen"
  (§2) laufen als beauftragter Durchlauf mit eigenen Agenten; das Register
  nimmt das Ergebnis auf, es führt den Durchlauf nicht herbei.
- **Keine Aussage über den Zustand einer Oberfläche.** „Noch nie enumeriert"
  (bis Welle 1) bzw. „enumeriert (Welle 1, §5.1)" bedeutet: hier hat niemand
  **geprüft**, ob an dieser Stelle etwas fehlerhaft ist. „Enumeriert" heißt
  ausdrücklich **nicht** „gefunden" und erst recht nicht „sicher" — die
  Enumeration benennt Eingabe-/Ausgabepfade und Klassen, nicht ein Ergebnis.
- **Kein Aufgaben- und kein Bug-Tracker.** Aufgaben, Entscheidungen und
  Folgearbeiten stehen in `Agents.todo.md` (Regel in `Agents.md`, Abschnitt
  „Dokumentations- und Todo-Regeln").

---

## 7. Abgrenzung zur Nachbarschaft

- **GUI-Workflow-Teil (`UI-VISION-REVIEW-70`)** bleibt unberührt: der visuelle
  Review am laufenden Fenster (Screenshots, Klicks, gemessene Koordinaten) ist
  ein eigener Vorgang und wird durch dieses Register weder ersetzt noch
  verändert.
- **CI-Red, Golden-Tests und der Build-/Verify-Ablauf** bleiben unberührt. Der
  Build-/Verify-Flow (`build-verify`-Skill, `Agents.md`, Rollen
  „Implementierungs-Agent"/„Verifizierungs-Agent") ist der Ablauf, in dem
  spätere Wellen laufen; dieses Register hängt sich als Nachweisliste daran
  (Stage 6), ohne ihn zu verändern.
- **Kein Aufgaben-Backlog.** Befunde ohne Sicherheitsklasse gehören in
  `Agents.todo.md`; dieses Register führt nur Befunde der im Security-Review
  vergebenen Klassen.
- **Keine Dublette zu `feature/quality/local-gates.md`.** Die dortige
  Lückenliste betrifft *Testabdeckung* der lokalen Gate-Stufen, nicht
  erreichbare Oberflächen und nicht geschlossene Sicherheitsbefunde.

---

## 8. Herkunft, Stand und Fortschreiben

- **Herkunft:** Skill `security-review` (agents-skills `82a9abb`), Marker-Range
  `86d677e..787670a`; Eigentümerentscheid 2026-10-20 (Flow wird übernommen,
  Register wird angelegt). Der Skill verlangt in Stage 6 den Registereintrag —
  dieser Ort ist die projektseitige Festlegung darauf.
- **Stand dieses Dokuments:** 2026-10-20 — Register angelegt, **null Befunde**,
  Enumerationsbasis (§5) erstmals strukturiert festgehalten. §5.1 ergänzt die
  **Enumeration** der Oberflächen (Security-Review-Welle 1, Teil
  *Finden/Enumerieren*) — **ohne Bewertung und ohne Befund**; §3 bleibt bei
  null Befunden. **Basis-Erweiterung desselben Tages** (Eigentümerentscheid):
  §5 führt jetzt **24** Oberflächen (19 + 5 neue Zeilen) und die GUI ist
  **gleichrangig** im Scope (§5, „Hinweise zur Reichweite dieser Basis").
- **Pflege:** Der Build-Agent trägt nach jeder bestandenen Verifikation einer
  Welle die zugehörige Zeile in §3 nach; die Implementierungs- und
  Verifizierungs-Agenten liefern Beleg und Testpfad. Der Index-Eintrag in
  `feature/README.md` wird vom Build-Agent gesetzt.
- **Fortschreiben:** §3 wächst mit den Wellen, §5 mit neu erkannten
  Oberflächen. Beide Änderungen sind Dokumentationsänderungen; sie brauchen
  keinen Build, aber dieselbe Review wie jeder andere Doku-Stand.

**Offen und bewusst nicht in diesem Dokument:**

- Die **prüfende Welle** (Teil *Befunde eintragen* und *Befunde schließen*) ist
  **nicht** Teil von §5.1: §5.1 enumeriert nur. Die Welle braucht Läufe
  (Build, Tests) und ist ein **eigener Auftrag** mit Implementierungs- und
  Verifizierungs-Agenten.
- Die **Ablage im Feature-Index** (`feature/README.md`) macht der Build-Agent.
- Ob und wo eine **Security-Review-Pflicht** in `Agents.md`/`DoD.md` verankert
  wird, ist eine **eigene** Entscheidung des Eigentümers; dieses Dokument
  nimmt sie nicht vorweg.
- Die **Scope-Frage GUI** (Datei-Eingabe/-Ausgabe über `lumina-gui`) ist am
  2026-10-20 **entschieden**: die GUI gehört **gleichrangig** in den Scope der
  Security-Review-Wellen (GUI-Oberflächen werden in **jeder** Welle
  mitgeprüft). Die Enumeration der GUI-Oberfläche liegt vollständig vor
  (§5.1.23) und wird dadurch nicht neu erhoben.
