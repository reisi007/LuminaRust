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

| Oberfläche | Pfad | Art der Eingabe/Ausgabe | Was hier noch offen ist |
|---|---|---|---|
| **MCP-Server (Transport)** | `crates/lumina-mcp/src/lib.rs` (`Server`, JSON-RPC über stdio), `crates/lumina-mcp/src/error.rs` | Eingabe: JSON-RPC-Requests auf stdin; Ausgabe: JSON-RPC-Responses auf stdout | **noch nie enumeriert** — Population wäre die Protokoll-/Methodenliste des Servers |
| **MCP-Werkzeug-Oberfläche** | `crates/lumina-mcp/src/tools/mod.rs` (`list_tool_definitions`, `dispatch_tool`, `is_known_tool`), Werkzeug-Implementierungen in `crates/lumina-mcp/src/tools/*.rs` | Eingabe: Tool-Argumente als JSON; Ausgabe: Tool-Ergebnis als JSON | **noch nie enumeriert** — Population wäre die Tool-Liste (29 registrierte Tools, je ein eigenes Modul; daneben 4 gemeinsame Helper-Module im selben Verzeichnis); **SOLL-Quelle**: `feature/platform/mcp-server.md` |
| **MCP-Pfad-/Zielbehandlung** | `crates/lumina-mcp/src/output_guard.rs` (`reject_protected_target`), `crates/lumina-mcp/src/util.rs` (`build_source_identity`, `write_output_guarded`, `parse_bounded_uint`, `validate_output_extension`) | Eingabe: Datei- und Zielpfade, Zahlen- und Formatparameter; Ausgabe: abgewiesene Schreibziele, Source-Identität | **noch nie enumeriert** — jede der Wurzelfunktionen wäre einzeln zu betrachten |
| **MCP-Session-Zustand** | `crates/lumina-mcp/src/session.rs` | Eingabe: Tool-Aufrufe mit `image_id`; Ausgabe: Sitzungszustand, Vorschau-/Temp-Dateien | **noch nie enumeriert** — Sitzung-Lebenszyklus und Aufräumen |
| **CLI-Kommando-Oberfläche** | `crates/lumina-cli/src/main.rs` (`enum Command`, clap-`Parser`), Argument-Strukturen ab `struct FileArgs` ff., Kommando-Implementierungen ab `fn run(cli: Cli)` | Eingabe: Kommandozeile (Argumente, Pfade, Zahlen); Ausgabe: Dateien, stdout/stderr, Exit-Code | **noch nie enumeriert** — Population wäre das `Command`-Enum (32 Varianten mit Argument-Strukt, zusätzlich die Feature-gate-Variante `Mcp`); **SOLL-Quelle**: `feature/platform/cli-gui-wasm.md` |
| **CLI-Submodule** | `crates/lumina-cli/src/{auto_tone_cli,cull_cli,lensfun_cli,mask_local,mask_local_color,mask_local_curves,mask_local_detail,mask_local_presence,merge,matrix,stages,library}.rs` | Eingabe: je Subcommand eigene Argumente; Ausgabe: Rezept-/Sidecar-/Status-Änderungen | **noch nie enumeriert** — jede Kommando-Gruppe wäre getrennt zu zählen |
| **Sidecar-JSON (Schreibpfad, CAS/Atomic-Write)** | `crates/lumina-sidecar/src/lib.rs` (`save_sidecar`, `save_sidecar_if_unchanged`, `write_atomically`, `migrate_sidecar_file`, `recover_sidecar`, `acquire_write_lock`) | Eingabe: `SidecarDocument`/`EditRecipe` (serde); Ausgabe: `.lumina.json` auf Platte, atomar ersetzt, mit Compare-and-Swap und Write-Lock | **noch nie enumeriert** — der CAS-/Atomic-Write-Pfad wäre je Kanal (Lesen, Schreiben, Migration, Lock-Reclaim) zu enumerieren |
| **Sidecar-JSON (Lesepfad) und Pfadableitung** | `crates/lumina-sidecar/src/lib.rs` (`load_sidecar`), `crates/lumina-sidecar/src/sidecar_paths.rs` (`sidecar_path_for`) | Eingabe: JSON-Datei von Platte; Ausgabe: `SidecarDocument` | **noch nie enumeriert** — Grenzwerte, Parser-Verhalten und Pfadableitung je eigene Klasse |
| **Sidecar-Begleitdatei `.lumina.zdata`** | `crates/lumina-sidecar/src/zdata.rs` (`zdata_path_for`, Validierung/Prüfsummen der Kachel-Blöcke) | Eingabe: Binärdatei mit Tile-Blöcken und Prüfsumme; Ausgabe: Masken-Kacheln | **noch nie enumeriert** |
| **Cache- und Vorschau-Pfade** | `crates/lumina-core/src/cache.rs` (`FolderCache`, `CacheStore`, `preview_cache_key`), `crates/lumina-core/src/preview_cache.rs` (`LruPreviewCache`, `encode_webp_lossless`, `decode_webp`), `crates/lumina-core/src/stage_cache.rs` (`StageFrameCache`) | Eingabe: Cache-Schlüssel aus Source/Virtual-Copy/Rezept-Digest; Ausgabe: Bildframes im Speicher | **noch nie enumeriert** — Schlüsselbildung und Lösch-/Prune-Verhalten wären je Klasse zu zählen |
| **Cache auf Platte** | `crates/lumina-cache/src/disk.rs` (`store_preview`, `load_preview`, `save_settings`, `atomic_write`), `crates/lumina-cache/src/preview_disk.rs` (`store`, `load`, `prune_orphans`) | Eingabe/Ausgabe: Vorschau-Dateien und Cache-Einstellungen im Ordner-Cache, atomar geschrieben | **noch nie enumeriert** |
| **RAW-/Bild-Decoding** | `crates/lumina-raw/src/lib.rs` (`decode_file`, `decode_bytes`, `decode_bytes_with_options`, `RAW_EXTENSIONS`, `is_raw_extension`), `crates/lumina-core/src/memory.rs` (`MemoryBudget::from_env`, `check_decode`, `check_mask`) | Eingabe: Bild-/RAW-Dateien fremder Formate; Ausgabe: dekodierte Frames, Budgetentscheidungen | **noch nie enumeriert** — Formatliste und Decoder je Klasse |
| **Merge-/DNG-Ausgabe** | `crates/lumina-merge/src/dng.rs` (`encode_linear_dng`, `write_merge_dng`, `merge_dng_filename`, `validate_dng_file_name`), `crates/lumina-merge/src/bundle.rs` (`run_merge`, `merge_checksum`), `crates/lumina-merge/src/digest.rs` | Eingabe: Frames + Rezept; Ausgabe: DNG-Datei mit EXIF und Prüfsumme | **noch nie enumeriert** |
| **Metadaten (IPTC/XMP/IIM/JPEG)** | `crates/lumina-iptc/src/xmp.rs` (`build_xmp_packet`, `parse_xmp_packet`, `build_app1`, `parse_app1`), `crates/lumina-iptc/src/iim.rs` (`encode_iim`, `decode_iim`, `build_app13`, `parse_app13`), `crates/lumina-iptc/src/jpeg.rs`, `crates/lumina-iptc/src/registry.rs` | Eingabe: Metadatenblöcke in Bilddateien; Ausgabe: Sidecar-Metadatenentwurf bzw. zurückgeschriebene APP1/APP13-Segmente | **noch nie enumeriert** — Parser- und Encoder-Paare je Klasse |
| **Rezept-/JSON-Eingaben (allgemein)** | `crates/lumina-sidecar/src/lib.rs` (`EditRecipe`, `SidecarDocument::from_json`, `to_json`), `crates/lumina-sidecar/src/local_adjustments/` (Unterblöcke und Migration) | Eingabe: JSON-Dokumente (Sidecar, Rezept, Local-Adjustments-Blöcke); Ausgabe: validierte Rezeptstruktur | **noch nie enumeriert** — Feld- und Versionsklassen je Block |
| **ONNX-Modell- und Artefakt-Eingaben** | `crates/lumina-onnx/src/manifest.rs` (`ModelManifest::from_json`, `validate`, `to_model_identity`), `crates/lumina-onnx/src/hash.rs`, `crates/lumina-onnx/src/resolve.rs` | Eingabe: Manifest- und Modell-Dateien von Platte; Ausgabe: Modellidentität, aufgelöste Artefaktpfade | **noch nie enumeriert** |
| **Lensfun-Datenbank (Systempfade)** | `crates/lumina-lensfun/src/db_path.rs` (`resolve`, `xml_files_in`, `from_process`), `crates/lumina-lensfun/src/system_load.rs`, `crates/lumina-lensfun/src/db_user_dir.rs` | Eingabe: Lesezugriff auf System-Verzeichnisse der Lensfun-Datenbank; Ausgabe: aufgelöste Profilpfade | **noch nie enumeriert** |
| **GUI-Dateieingabe und -ausgabe** | `crates/lumina-gui/src/dropped_files.rs` (Drop-Handling), `crates/lumina-gui/src/file_browser_entry.rs`, `crates/lumina-gui/src/export.rs` (`export_to`) | Eingabe: vom Nutzer gewählte Pfade (Dialog, Drag-and-Drop); Ausgabe: exportierte Bilddateien | **noch nie enumeriert** — die GUI ist eine lokale Desktop-Oberfläche, aber mit derselben Datei-Eingabe-/Ausgabeklasse wie CLI und MCP |
| **Prozess-Umgebung** | `crates/lumina-core/src/memory.rs` (`LUMINA_MAX_RAW_PIXELS`, `LUMINA_MAX_MASK_PIXELS`, `LUMINA_MAX_ALLOC_BYTES`), `crates/lumina-mcp/src/lib.rs` (`LUMINA_MCP_KEEP_PREVIEWS`) | Eingabe: Umgebungsvariablen der Prozessumgebung; Ausgabe: Budget- und Cleanup-Entscheidungen | **noch nie enumeriert** |

**Hinweise zur Reichweite dieser Basis:**

- Der MCP-Server ist **nicht** netzwerkexponiert (Transport stdio, kein
  HTTP/WebSocket im MVP — `feature/platform/mcp-server.md`, Abschnitt
  „MCP-Protokoll und Transport"). Das ändert nichts an der
  Enumerationspflicht: Werkzeug-Argumente, Pfade und Datei-I/O bleiben
  Eingabe-Außengrenzen.
- WASM/Browser ist **gestrichen** (Eigentümer-Entscheid 2026-09-04, siehe
  `feature/platform/wasm-limits.md`); eine WASM-Oberfläche existiert daher
  nicht und steht hier nicht.
- Ob die GUI (Datei-Eingabe/-Ausgabe) in den Scope der Security-Review-Wellen
  gehört, ist eine **offene Scope-Frage des Eigentümers**; die Tabelle führt
  sie vorläufig, damit die spätere Enumeration sie nicht übersieht.
- Diese Liste ist eine **Bestandsaufnahme der Flächen**, keine vollständige
  Bedrohungsanalyse: sie sagt, wo Eingaben an der Außengrenze entstehen, und
  nichts über deren Behandlung.

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
  bedeutet: hier hat bisher niemand systematisch gesucht. Es bedeutet weder,
  dass etwas gefunden wurde, noch dass nichts vorhanden ist.
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
  Enumerationsbasis (§5) erstmals strukturiert festgehalten.
- **Pflege:** Der Build-Agent trägt nach jeder bestandenen Verifikation einer
  Welle die zugehörige Zeile in §3 nach; die Implementierungs- und
  Verifizierungs-Agenten liefern Beleg und Testpfad. Der Index-Eintrag in
  `feature/README.md` wird vom Build-Agent gesetzt.
- **Fortschreiben:** §3 wächst mit den Wellen, §5 mit neu erkannten
  Oberflächen. Beide Änderungen sind Dokumentationsänderungen; sie brauchen
  keinen Build, aber dieselbe Review wie jeder andere Doku-Stand.

**Offen und bewusst nicht in diesem Dokument:**

- Die **erste Security-Review-Welle** ist ein **eigener, noch nicht geplanter
  Task** — mit Auftrag an einen Implementierungs-Agenten und einen
  Verifizierungs-Agenten.
- Die **Ablage im Feature-Index** (`feature/README.md`) macht der Build-Agent.
- Ob und wo eine **Security-Review-Pflicht** in `Agents.md`/`DoD.md` verankert
  wird, ist eine **eigene** Entscheidung des Eigentümers; dieses Dokument
  nimmt sie nicht vorweg.
- Die **Scope-Frage GUI** (Datei-Eingabe/-Ausgabe über `lumina-gui`) ist eine
  **eigene, offene Entscheidung des Eigentümers**; dieses Dokument nimmt sie
  nicht vorweg.
