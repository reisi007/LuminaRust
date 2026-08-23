# Logging-Plan für LuminaRust

Status: **Plan (SOLL), Version 0.1** — noch nicht implementiert.
Umfang: strukturiertes Logging über alle Crates; Umsetzung in kleinen,
unabhängig prüfbaren Schritten gemäß `Agents.md`.

## 1. Ziel und Umfang

### 1.1 Motivation

Beim ersten manuellen GUI-Test stürzte die GUI ab (White-Balance-Panic).
Der Zustand war danach nicht rekonstruierbar, weil:

- kein Logging-Framework im Projekt existiert (verifiziert: keine
  `log::`/`tracing`-Aufrufe in `crates/*/src`);
- die GUI ihren Zustand nur in `self.status`/`self.error` (Strings,
  `crates/lumina-gui/src/lib.rs`) hält, die bei einem Crash verloren gehen;
- die CLI nur unstrukturiertes `eprintln!`/`println!`
  (`crates/lumina-cli/src/main.rs:248`, `:429`, `:718–732`, `:853`,
  `:1130`, `:1134`) ausgibt.

Ziele des Loggings:

- **Traceability vor Crashes**: Der letzte Log-Stream soll Dekontext,
  Metadaten (u. a. Kamera-Weißabgleich `cam_mul`), Rezeptzustand und die
  letzte fehlgeschlagene Operation zeigen.
- **Testbarkeit**: Log-Aufrufe dokumentieren Entscheidungen (Cache-Hit/Miss,
  Masken-Fallbacks) und machen sie in Integrationstests prüfbar.
- **Keine stillen Fallbacks**: Logging ergänzt die sichtbare Meldung
  (Agents.md: „Reproduzierbarkeit ist wichtiger als ein stiller Fallback"),
  ersetzt sie aber nie.

### 1.2 Severities und Einsatzregeln

| Severity | Bedeutung | Beispiele (dieses Projekt) |
| --- | --- | --- |
| `trace` | Hot-Path-Diagnostik, standardmäßig deaktiviert | pro Render-Stufe (WB-LUT, Kurven, Resample-Kantenmaße) |
| `debug` | detaillierte Zustands-/Entscheidungsdaten | `cam_mul`-Werte, Cache-Hit/Miss je Maske, Render-Key-Bestandteile |
| `info` | Lebenszyklus-Meilensteine, Standardlevel | App-Start, Workdir gesetzt, Decode erfolgreich, Sidecar gespeichert, Subcommand gestartet |
| `warn` | degradierter, aber fortgesetzter Betrieb | veraltete Auto-Tone-Analyse, Masken-Skip unter `MaskPolicy::Warn`, Cached-Fallback bei fehlendem Modell |
| `error` | Operation fehlgeschlagen mit Nutzerwirkung | Decode-/Render-/Sidecar-Schreibfehler, fehlendes Modell |

Regeln:

- Jede `warn`/`error`-Stelle muss einer bestehenden sichtbaren Meldung
  (`self.error`, CLI-`eprintln!`, `Result::Err`) zugeordnet sein — Logging ist
  zusätzlich, nicht ersetzend.
- `info!` niemals im Pixel-Hot-Path (render_frame-Inneres); dort nur
  `debug`/`trace`.
- Log-Zeilen enthalten Kontext als Schlüssel-Wert-artige Anhänge, z. B.
  `"raw decode ok name=IMG_0001.ARW w=6032 h=4024 orientation=1"`. Pfade sind
  erlaubt (siehe Abschnitt 4), Bildpixel/Rohdaten niemals.

## 2. Empfohlene Lösung

### 2.1 Facade: `log`-Crate (0.4)

**Empfehlung:** ausschließlich die `log`-Crate als Facade in allen
Bibliotheks-Crates (`lumina-core`, `lumina-sidecar`, `lumina-raw`,
`lumina-onnx`, `lumina-gui`). Keine konkrete Logger-Implementierung in
Bibliotheken.

Begründung gegen die Architekturgrenzen (Agents.md):

- `lumina-core` darf keine GUI-, Dateisystem- oder nativen Abhängigkeiten
  erzwingen. `log` hat keine Abhängigkeiten und kompiliert auf `wasm32`.
- Die Initialisierung erfolgt **nur in Binärziele-Startpfaden**
  (`main.rs` bzw. WASM-Einstieg), sodass jedes Ziel seine eigene
  Implementierung wählt.
- `tracing` wird bewusst NICHT eingeführt: schwereres Abhängigkeitsprofil,
  für den aktuellen Bedarf (Crash-Nachvollzug, keine verteilte Tracing-
  Infrastruktur) nicht erforderlich. Ein späterer Wechsel bleibt offen.

### 2.2 Initialisierung je Binärziel

| Ziel | Ort | Logger | Filter-Standard |
| --- | --- | --- | --- |
| lumina-gui native | `crates/lumina-gui/src/main.rs:6` (`fn main`), vor `eframe::run_native` | `env_logger` mit `Builder::new().filter_level(Info).parse_env("LUMINA_LOG")`, Ausgabe nach stderr | `info`; Override via `LUMINA_LOG=debug/lumina_gui=trace` |
| lumina-gui wasm32 | `crates/lumina-gui/src/lib.rs:3650` (`pub fn start()`, `#[wasm_bindgen(start)]`), zuerst im Funktionskörper | minimaler eigener `log::Log`-Adapter über `web_sys::console` (~25 Zeilen, siehe unten) ODER Crate `console_log` | `info`, optional via `localStorage["LUMINA_LOG"]` |
| lumina-cli | `crates/lumina-cli/src/main.rs:246` (`fn main`), vor `Cli::parse()` | `env_logger` (stderr), Env `LUMINA_LOG` + Fallback `RUST_LOG` | `warn` für JSON-Modus-kompatible Ausgaben; `--verbose`-Flag setzt `info`/`debug` |

**WASM-Adapter (bevorzugt):** `web-sys` ist für `wasm32` bereits Dependency
mit dem Feature `console`
(`crates/lumina-gui/Cargo.toml`, Block `[target.'cfg(target_arch = "wasm32")'.dependencies]`;
genutzt in `lib.rs:3668` `web_sys::console::error_1`). Ein eigener
`log::Log`-Mapper (`error_1/warn_1/info_1/log_1`) vermeidet jede neue
Dependency im WASM-Pfad und erfüllt die Capability-Regel aus Agents.md
(„Keine native Dependency im WASM-kompatiblen Pfad ohne dokumentierte
Capability-Entscheidung"). `console_log` ist als fertige Alternative
zulässig, zieht aber zusätzliche Crates nach sich.

**Panic-Hook (beide GUI-Ziele + CLI):** `std::panic::set_hook`, der die
Panic-Meldung inklusive Location als `error!` schreibt. Damit landen auch
Panics (wie der White-Balance-Crash) im Log-Stream. Bekannte Panic-Kandidaten
aus Code-Lektüre: `expect("loaded frame")` in
`crates/lumina-gui/src/lib.rs:1543`, Indexierung `document.virtual_copies[0]`
in `lib.rs:1536`.


### 2.3 App-Ringbuffer („Zustand VOR dem Crash")

Ergänzend zum Stream-Log erhält `LuminaApp` einen kleinen In-Memory-
Ringbuffer (z. B. 100 Einträge) der letzten Log-Ereignisse mit Severity.
Bei Panic schreibt der Panic-Hook den Ringbuffer als letzten Block in den
Log (und optional in ein Crash-Dump-Feld, das beim nächsten Start im
Header-Panel angezeigt werden kann). Damit ist auch ohne Dateisystem-Zugriff
(WASM) der Zustand vor dem Crash rekonstruierbar.

## 3. Event→Ort→Severity-Tabelle (das Herzstück)

Alle Zeilenangaben sind durch Code-Lektüre verifiziert (Stand: Plan-Erstellung).
Meldungsvorschläge sind format-Beispiele; Schlüssel-Wert-Anhänge sind bindend.

### 3.1 lumina-raw (`crates/lumina-raw/src/lib.rs`)

| Ort | Ereignis | Severity | Meldungsinhalt |
| --- | --- | --- | --- |
| `decode_file` :102–121 | Decode-Auftrag aus Datei | debug | `path`, Größe in Bytes |
| :111–114 | Datei nicht lesbar | error | `path`, IO-Fehlertext (`RawError::Io`) |
| `decode_bytes` :123–133 / `decode_bytes_with_options` :135–149 | Decode-Start | debug | `name`, `bytes.len()`, `demosaicing`, `output_bits` |
| :124–128 | WASM-Aufruf | warn | „RAW decoding unavailable on wasm32" (`UnsupportedPlatform`) — einmalig pro Aufruf |
| native `decode_bytes_with_options` :252–404 | Decode-Start (native) | debug | `name`, `len`, LibRaw-Version (`libraw_version()` :160–169) |
| :257–259 | Leere Eingabe | error | „empty input" |
| :260–262 | Ungültige Bit-Tiefe | error | angeforderte `output_bits` |
| :263–266 | `libraw_init` fehlgeschlagen | error | „LibRaw handle" |
| :273–278 | `open_buffer` fehlgeschlagen | error | Operation „opening input", LibRaw-Code+StrError |
| :280–283 | Orientierung extrahiert | debug | `orientation` (aus `sizes.flip`, Fallback 1); **info** wenn ≠1 (Rotation relevant für Nutzer) |
| :285 | **Kamera-Weißabgleich (`cam_mul`) extrahiert** | debug | alle vier Werte `[r,g,b,g2]`; **warn** wenn ein Wert nicht endlich oder ≤0 ist (Rohwert wird unverändert übernommen!) |
| :286 | `pre_mul` | debug | vier Werte |
| :298–316 | Metadaten-Satz | debug | make/model/iso/shutter/aperture/focal_length (nur die durch `positive()` :245–250 gefilterten Werte) |
| :322–323 | Decoder-Flags | trace | `use_camera_wb=1`, `use_camera_matrix=1` |
| :328–331 | `unpack` fehlgeschlagen | error | Operation „unpacking input", Code+StrError |
| :341–343 | Memory-Budget überschritten | error | Budget, angeforderte Dimensionen (`MemoryBudgetExceeded`) |
| :344–347 | `dcraw_process` fehlgeschlagen | error | Code+StrError |
| :348–353 | `make_mem_image` fehlgeschlagen | error | Code+StrError |
| :355–357 | Ungültiges Format (bits/colors) | error | tatsächliche bits/colors |
| :358–360 | Null-Dimensionen | error | width/height |
| :361–404 | **Decode erfolgreich** | info | `name`, `w`, `h`, `orientation`, `bytes_out` |

Kernpunkt WB: `camera_white_balance = data.color.cam_mul` (:285) wird
**ungefiltert** übernommen. Die Validierung (finit, >0) passiert erst spät in
lumina-core (`crates/lumina-core/src/lib.rs:478–489`). Genau deshalb müssen
die Rohwerte hier auf `debug` (und Abnormitäten auf `warn`) — sie sind die
primären Verdächtigen für den White-Balance-Crash-Pfad.

### 3.2 lumina-core

#### 3.2.1 Render-Einstieg (`crates/lumina-core/src/render.rs`)

| Ort | Ereignis | Severity | Meldungsinhalt |
| --- | --- | --- | --- |
| `render_frame` :107–181 | Einstieg | trace | Frame-Dimensionen, Anzahl Source-Actions, Masken-Kontext an/ab, Policy (Strict/Warn) |
| :112 | Source-Actions angewendet | trace | Anzahl; bei Verstoß bereits `Err(InvalidSourceAction)` → error unten |
| :113 | Rezept+W B angewendet (Ergebnis) | debug bei Erfolg mit WB-Schlüsseln: `wb_temperature`, `wb_tint`, abgeleitete Gains; **error** bei `CoreError::InvalidAdjustment` inkl. Feldname+Wert |
| :119–127 | Lensfun-Korrektur aktiv | trace | Corrector vorhanden (Feature `lensfun`) |
| :131–174 | Maskenstufe Ergebnis | debug | Layer-Anzahl, Warning-Anzahl |
| :146–155/:160–170 | Layer unter `MaskPolicy::Warn` übersprungen | **warn** | exakter Message-Text aus `evaluate_layer` (copy_id/mask_id/status bzw. Grund) |
| :147–153/:162–168 | Strict-Fehler | error | `MaskUnavailable`/`MaskEvaluation` mit copy_id/mask_id |
| :176–180 | **Render erfolgreich** | debug (nicht info! Hot Path) | w/h, Layer-Anzahl, Dauer in ms |

#### 3.2.2 Masken-Auswertung und Resample (`render.rs`)

| Ort | Ereignis | Severity | Meldungsinhalt |
| --- | --- | --- | --- |
| `evaluate_layer` :200–261 | Definition nicht verfügbar/nicht `Valid` | warn (unter Warn-Policy; sonst via render_frame error) | layer.id, copy_id/mask_id, Status (`:214–217`) |
| :225–240 | Graph-Auswertung fehlgeschlagen | warn/error analog oben | copy_id/mask_id + Fehlergrund (`:229–232`) |
| :241–255 | Zero-Dimension-Plane | warn/error | layer.id, „invalid zero-dimension plane" |
| :256 | Resample durchgeführt | trace | Quell-/Zielmaße (`resample_plane_bilinear` :280–306; deterministisch, kein Warnfall) |
| :259 | Modulation (invert/feather/blur/density) | trace | Parameterwerte |

#### 3.2.3 Rezept-Validierung und WB-Anwendung (`crates/lumina-core/src/lib.rs`)

| Ort | Ereignis | Severity | Meldungsinhalt |
| --- | --- | --- | --- |
| `apply_recipe_with_white_balance` :445–451 | As-Shot-Kontext übergeben | debug | `Some([r,g,b,g2])`-Werte oder `None` |
| `apply_recipe_with_scale_and_white_balance` :470–477 | `effective_scale` ungültig | error | Wert |
| :478–489 | **`camera_white_balance`-Validierung fehlgeschlagen** (nicht finit oder ≤0) — **Kernstelle des WB-Crash-Pfads** | **error** | alle vier Gain-Werte plus Index des Verstoßes; Hinweis auf Herkunft (`RawMetadata.camera_white_balance`) |
| :490–506 | Adjustment außerhalb Range/unbekannt | error | key, value, min/max (`wb_temperature`: 1500..=12000 :495) |
| :507 | Verschachtelte Validierung fehlgeschlagen | error | Fehler von `validate_nested_adjustments` |
| :514–527 | WB-Gain-Ableitung | debug | temperature/tint → warmth/Gains (Formel `:523–524`) |
| :559–574 | Kurvenanwendung, Luminanz-Division | trace | Division nur bei `luminance > 1e-9` abgesichert (`:570`) — kein Log im Normalfall; `trace` genügt |

#### 3.2.4 Masken-Cache-Entscheidung (`crates/lumina-core/src/mask_loader.rs`)

| Ort | Ereignis | Severity | Meldungsinhalt |
| --- | --- | --- | --- |
| `resolve_mask_planes` :116–216 | Start | debug | aktive Kopie, erreichbare Definitionen, refresh-Flag |
| :139–146 | Güteprüfung persistierter Maske | trace | `source_ok`, `decode_ok`, `identity_ok`, `artifact_present`, Status |
| :148–158 | **Cache-Hit**: gültige persistierte Maske verwendet | debug | copy_id/mask_id, `from=LoadedPersisted` |
| :166–187 | **Re-Inferenz** | info | copy_id/mask_id, `from=ReInferred`, Grund (refresh/stale/source-changed/model-changed/missing) |
| :173–178 | Re-Inferenz fehlgeschlagen | error | copy_id/mask_id, Backend-Fehler |
| :189–206 | **Cached-Fallback**: Modell nicht verfügbar, veraltete Maske verwendet | **warn** | exakter Bestätigungstext `:201–205` (F-051) |
| :209–215 | Weder Modell noch Cache | **error** | `MaskUnavailable` mit copy_id/mask_id |
