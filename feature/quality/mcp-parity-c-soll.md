# SOLL — MCP-PARITY-C (HDR + Panorama, Release 2.0)

**Scope (eingegrenzt seit User-Order 2026-09-26):** Byte- und Verhaltens-Parität
zwischen `lumina merge-hdr` / `lumina merge-pano` (CLI) und zwei neuen
MCP-Tools, über dieselbe Eingabemenge, denselben Merge-Digest und denselben
DNG-Checksum-Anchor. Matrix bleibt eigener Sub-Slice (Abgrenzung, §12).

**Belegbasis (alles unten ist gelesen, nichts spekulativ; Code-Stand der Lesung:
2026-10-13, `9bacea8` — die Zeilenangaben dieses Belegblocks beziehen sich auf
diesen Stand und sind ueber `git show 9bacea8:<pfad> | sed -n '<zeile>p'`
rekonstruierbar; die historischen Abschnitte beschreiben denselben Stand, nennen
ihre Belege aber seit der Anker-Welle 2026-10-25 als Pfad + Symbol):**
`crates/lumina-cli/src/merge.rs` (414 Z., vollständig),
`crates/lumina-cli/src/main.rs` (Merge-Varianten :647-651, Dispatch :1560-1561,
`CliError::Message` :1465-1468, `main`/`run` :1526-1534, `emit` :5675-5682),
`crates/lumina-merge/src/bundle.rs` (`run_merge` :357-366, `MergeRunError`
:231-241, `MergeRunOutcome` :278-289, `MergeRunOptions` :213-227,
`MergeExif` :119-196, `validate_source_count` :482-494, `resolve_dng_path`
:598-653, `build_merge_document` :904-966),
`crates/lumina-sidecar/src/merge_recipe.rs` (Grenzen :26-44, `digest` :182-188,
`merge_digest` :247-261, `validate_merge_alignment` :355-379),
`crates/lumina-mcp/src/tools/mod.rs` (vollständig), `crates/lumina-mcp/src/error.rs` → `McpError` (vollständig),
`crates/lumina-mcp/src/tools/generative.rs`, `crates/lumina-mcp/src/tools/dust_removal.rs`, `crates/lumina-mcp/src/tools/import.rs`,
`crates/lumina-mcp/src/tools/spot.rs`, `crates/lumina-mcp/src/tools/stage_common.rs`, `crates/lumina-mcp/src/tools/library_common.rs`,
`crates/lumina-mcp/src/util.rs`, `crates/lumina-mcp/src/output_guard.rs`, `crates/lumina-mcp/Cargo.toml`,
`crates/lumina-mcp/tests/tool_names/mod.rs`,
`crates/lumina-cli/src/tests/mcp_onnx.rs`,
`crates/lumina-cli/tests/merge_parity.rs`,
`crates/lumina-cli/tests/merge_e2e.rs` (799 Z.),
`crates/lumina-cli/tests/library_parity.rs` +
`crates/lumina-cli/tests/library_parity_common/mod.rs`,
`crates/lumina-mcp/tests/library_tools*.rs`,
`feature/platform/cli-gui-wasm.md` §HDR/Panorama :278-325,
`feature/platform/mcp-server.md` :12-15, `feature/quality/golden-fixtures.md`
§2 :40-69, `sample-data/raw/README.md`, `Agents.todo.md` :409,
`Agents.md` :317 (Hardware-Gate), `scripts/file_size_baseline.txt`.

## 1. Ziel

- MCP erhält **zwei** path-basierte Tools (ein Aufruf = ein Merge-Job, **kein**
  `image_id`, kein Session-State): `lumina_merge_hdr` und `lumina_merge_pano`.
  Analogie: `lumina_generative`/`lumina_relocate` (MCP-PARITY-B,
  `crates/lumina-mcp/src/tools/mod.rs` → `list_tool_definitions`).
- Beide Tools rufen **dieselbe** Orchestrierung wie CLI + GUI auf:
  `lumina_merge::bundle::run_merge` (CLI-Knoten
  `crates/lumina-cli/src/merge.rs` → `run_merge`). Keine zweite Merge-Mathematik, kein
  zweiter DNG-Writer.
- Resultat: byte-identisches DNG + identischer Merge-Digest für dieselbe
  Eingabemenge über CLI, GUI-headless und MCP
  (`crates/lumina-cli/tests/merge_parity.rs` → `cli_and_gui_contract_paths_share_the_same_anchor`).

## 2. `MergeArgs` → MCP JSON-Schema (SOLL-Vertrag)

Quelle der Felder, Typen und Defaults: `crates/lumina-cli/src/merge.rs` → `MergeArgs`
(`pub struct MergeArgs`, clap). Die MCP-Felder sind snake_case; `--json` entfällt
(§5). Der CLI-Flag `--input` ist `required = true` und ohne `num_args`
wiederholbar (`crates/lumina-cli/src/merge.rs` → `MergeArgs`) — clap erzwingt damit **nur das Vorhandensein**,
die Spanne 2..=256 kommt allein aus `validate_source_count`
(`crates/lumina-merge/src/bundle.rs` → `validate_source_count`). Die expliziten Listen sind komma-getrennt
(`crates/lumina-cli/src/merge.rs` → `MergeArgs`), im MCP-Schema sind sie echte JSON-Arrays.

| CLI-Flag (`MergeArgs`, `crates/lumina-cli/src/merge.rs` → `MergeArgs`) | MCP-JSON-Feld | Typ | Pflicht | Default (MCP) | Grenzen / Schema-Keywords |
| --- | --- | --- | --- | --- | --- |
| `--input` (wiederholbar) | `inputs` | string[] | **ja** | — | `minItems: 2`, `maxItems: 256` (**beschreibend, nicht durchgreifend** — Pkt. 1); absolute Pfade, Merge-Reihenfolge; `inputs[0]` = Referenz |
| `--output` | `output` | string \| null | nein | `null`/absent → `<ref-stem>-HDR.dng` / `-Pano.dng` neben `inputs[0]` (`crates/lumina-merge/src/bundle.rs` → `resolve_dng_path`) | MUSS auf `.dng` enden (`crates/lumina-merge/src/bundle.rs` → `resolve_dng_path`), braucht einen Dateinamen (`resolve_dng_path`), darf keiner Quelle gleichen (`resolve_dng_path`, Hüll-Verweis `resolve_dng_path`) — **Form-Checks sind Merge-Domäne** (Pkt. 3) |
| `--exposure-times` (`MergeArgs::exposure_times`) | `exposure_times` | number[] | nein | `[]` | Arity: leer **oder** genau `len(inputs)` (`crates/lumina-stages/src/merge.rs` → `check_explicit_list_lengths`; Handler, §3 letzte Zeile); Range je Wert endlich, `(0, 86_400]` (`crates/lumina-sidecar/src/merge_recipe.rs` → `MAX_MERGE_EXPOSURE_TIME_S`) — **beschreibend**, Durchgriff in `resolve_hdr_exposure` → `merge unsupported:` (`crates/lumina-stages/src/merge.rs` → `resolve_hdr_exposure`) |
| `--isos` (`MergeArgs::isos`) | `isos` | integer[] | nein | `[]` | Arity: leer **oder** genau `len(inputs)`; Range je Wert `1..=10_000_000` (`crates/lumina-sidecar/src/merge_recipe.rs` → `MAX_MERGE_ISO`) — **beschreibend**, Durchgriff `crates/lumina-stages/src/merge.rs` → `resolve_hdr_exposure`; **JSON-Integer, kein Float-Cast** |
| `--f-numbers` (`MergeArgs::f_numbers`) | `f_numbers` | number[] | nein | `[]` | Arity: leer **oder** genau `len(inputs)`; Range je Wert endlich, `(0, 256]` (`crates/lumina-sidecar/src/merge_recipe.rs` → `MAX_MERGE_F_NUMBER`) — **beschreibend**, Durchgriff `crates/lumina-stages/src/merge.rs` → `resolve_hdr_exposure` |
| `--max-shift-px` | `max_shift_px` | integer | nein | `16` | `minimum: 0` **beschreibend**; Durchgriff Library-Guard `crates/lumina-merge/src/bundle.rs` → `run_merge` → `merge failed:` (Referenzklasse §2 Pkt. 4). **Kein** oberes Schema-Limit dokumentiert → offene Frage §13 Punkt 4 |
| `--blend-width-px` | `blend_width_px` | integer | nein | `64` | `minimum: 0`, `maximum: 65536` (`crates/lumina-sidecar/src/merge_recipe.rs` → `MAX_MERGE_BLEND_WIDTH_PX`) **beschreibend**; Durchgriff `recipe.validate()` (`crates/lumina-merge/src/bundle.rs` → `run_merge`) → `merge failed:` |
| `--force` (`MergeArgs::force`) | `force` | boolean | nein | `false` | re-mergt ein stale/dangling Bundle (`crates/lumina-cli/src/merge.rs` → `MergeArgs`) |
| `--json` (`MergeArgs::json`) | — (entfällt) | — | — | — | MCP antwortet **immer** `structuredContent` (§5); `--json` ist reines CLI-Transport-Flag (`emit`, `crates/lumina-cli/src/main.rs` → `emit`) |

Fußnote zur Range-Spalte (Modus-Asymmetrie, §6 Inv. 4/5): die Range-Durchgriffe
der drei Exposure-Listen gelten nur für **HDR** (`resolve_hdr_exposure`,
`crates/lumina-stages/src/merge.rs` → `resolve_hdr_exposure` → `merge unsupported:`). Für **Pano** ist ein expliziter
Außer-Range-Wert kein Fehler, sondern fällt auf EXIF bzw. die neutralen
Defaults zurück (`resolve_pano_exposure`, `crates/lumina-stages/src/merge.rs` → `resolve_pano_exposure`); die MCP-Tools
erben das unverändert.

Serverseitige Pflicht-Prüfungen (nicht nur Schema, sondern Handler-Code —
`crates/lumina-mcp/src/tools/stage_common.rs` → `reject_unknown_fields` ist das verbindliche Muster,
weil ein Schema für das aufrufende Modell nur eine *Beschreibung* ist).

**Die Trennregel für alles Folgende (Defektklassen-Sweep):** der Handler
prüft ausschließlich **JSON-Shape und -Typ** (Feld vorhanden, String/Array/
Integer/Bool wie zugesagt, `additionalProperties`) — jede Verletzung davon ist
`InvalidParams`. Jede **Merge-Domänen-Regel** (Anzahl, Arity, Ranges,
`output`-Form) wird **nicht** handler-seitig durchgesetzt, sondern der
Merge-Domäne überlassen; die resultierende Klasse
(`merge missing:`/`merge stale:`/`merge unsupported:`/`merge failed:`) wird
gemappt (§3). Ein Handler-Vorab-Check auf eine Domänen-Grenze würde die
Fehlerklasse künstlich auf `InvalidParams` ziehen und damit T17 verletzen;
Schema-Keywords (`minItems`/`maxItems`/`minimum`/`maximum`) bleiben
Beschreibung für den Client. Die einzige bewusste Ausnahme ist die
Listen-Arity (Pkt. 2): dort prüft die CLI selbst vor `run_merge` und erzeugt
keine `MergeRunError`-Klasse.

1. `inputs` ist Pflicht (`required: ["inputs"]`; Fehlen oder Nicht-Array →
   `InvalidParams` — reine JSON-Shape). Die **Anzahl** prüft der MCP-Handler
   **nicht** selbst: `run_merge` → `validate_source_count`
   (`crates/lumina-merge/src/bundle.rs` → `run_merge`, `validate_source_count`) liefert dafür
   `MergeRunError::Failed` → gemappt `MergeFailed` (§3-Tabelle). Grund ist die
   Fehlerklassen-Gleichheit CLI ≡ MCP (T17): die CLI kennt für 0, 1 oder 257
   Quellen keinen anderen Weg, also darf MCP keinen erfinden. Die Schema-
   Keywords `minItems: 2` / `maxItems: 256` bleiben als **Beschreibung** für
   den Client stehen, greifen aber nicht durch — ein Schema-Verstoß endet
   nicht in `InvalidParams`, sondern in derselben `merge failed:`-Klasse wie
   im CLI.
2. Explizite Listen: `len == 0 || len == inputs.len()` — identische Semantik
   wie `check_explicit_list_lengths` (`crates/lumina-stages/src/merge.rs` → `check_explicit_list_lengths`). Das ist die eine
   Gestaltprüfung, die **wirklich** Handler-Sache bleibt: die CLI prüft sie
   vor `run_merge` und meldet `CliError::Message` (exit 1), nicht
   `MergeRunError`; MCP bildet sie auf `InvalidParams` ab (§3-Tabelle,
   letzte Zeile). Fehlerbild laut, kein stilles Truncate/Repeat.
3. `output`: ist ein **optionales** Feld (`crates/lumina-cli/src/merge.rs` → `MergeArgs`,
   `Option<PathBuf>`) — fehlt es, ist es `null` oder absent, gilt der
   Default-Zielname aus `inputs[0]` (`crates/lumina-merge/src/bundle.rs` →
   `resolve_dng_path`); das ist **kein** Fehler. Nur ein **vorhandenes,
   nicht-String**-Feld ist ein Typfehler → `InvalidParams`. Die **Form**
   (`.dng`-Endung, `resolve_dng_path`), ein fehlender Dateiname
   (`resolve_dng_path`) und ein Ziel, das auf eine Quelle zeigt (`Ablehnungszweig`
   `resolve_dng_path`, Prüfschleife `resolve_dng_path`) prüft der Handler **nicht**: das tut
   `resolve_dng_path` (Hüll-Verweis `crates/lumina-merge/src/bundle.rs` →
   `resolve_dng_path`) und liefert dafür
   `MergeRunError::Failed` → gemappt `MergeFailed`. Gleiche Doktrin wie der
   Count (Pkt. 1): jede Eingabe, die im CLI `merge failed:` ergibt, darf im
   MCP nicht `InvalidParams` werden (T17). Ein eigener Handler-Check wäre
   außerdem doppelt — `resolve_dng_path` erreichen beide Frontends über
   denselben Aufruf.
4. `max_shift_px`/`blend_width_px`/`force`: Handler parst strikt auf
   **JSON-Ebene** — `force` muss ein Bool sein, `max_shift_px`/
   `blend_width_px` Integer ohne Nachkommastellen
   (`crates/lumina-mcp/src/tools/stage_common.rs` → `optional_bool`; `parse_bounded_uint` liefert das Typ-Muster,
   `crates/lumina-mcp/src/util.rs` → `parse_bounded_uint`). Die **Range**-Grenzen (`blend_width_px` ≤ 65536,
   `crates/lumina-sidecar/src/merge_recipe.rs` → `MAX_MERGE_BLEND_WIDTH_PX`; `crates/lumina-sidecar/src/merge_recipe_alignment.rs` → `validate_merge_alignment`) sind wie `minItems`/`maxItems`
   **beschreibend**: sie werden im Schema dokumentiert, aber **nicht**
   handler-seitig erzwungen, sondern von `recipe.validate()`
   (`crates/lumina-merge/src/bundle.rs` → `run_merge`) geprüft. Sonst würde MCP für einen
   Domain-Verstoß `InvalidParams` melden, wo die CLI `merge failed:`
   meldet (Verstoß gegen T17). Out-of-Range wird also nie geclippt
   (Prinzip `crates/lumina-mcp/src/tools/stage_common.rs` → `optional_f32`: „Out-of-range values are
   deliberately **not** clamped").
   **Referenzklasse `max_shift_px` negativ, explizit:** im CLI gibt es
   nirgends `allow_negative_numbers` oder `allow_hyphen_values`
   (verifiziert: kein Treffer in `crates/lumina-cli/src`) — die
   Space-Form `--max-shift-px -1` fängt clap daher als Usage-Fehler ab
   (Exit 2, `crates/lumina-cli/src/main.rs` → `CliError::exit_code`). Programmatische Aufrufer (GUI, oder die
   `=`-Form) erreichen dagegen die Library-Guard in `run_merge`
   (`crates/lumina-merge/src/bundle.rs` → `run_merge`, `merge failed:`). MCP ist ein programmatischer
   Aufrufer und folgt deshalb der **Library-Referenz**: `-1` wird
   durchgereicht → `MergeFailed`. Handler-seitig wird nur der JSON-Typ
   geprüft (Integer, nicht `1.5`/`"x"`) → `InvalidParams`, wie in der
   Tool-Argumente-Zeile der §3-Tabelle beschrieben.
5. `additionalProperties: false` im Schema **und** `reject_unknown_fields` im
   Handler (`crates/lumina-mcp/src/tools/stage_common.rs` → `reject_unknown_fields`; Muster `crates/lumina-mcp/src/tools/generative.rs` → `ALLOWED`/`run`). Das
   ist reine JSON-Shape und verschiebt keine Fehlerklasse.

## 3. CLI-Subcommand-Wiring + stabile Fehlerpräfixe

**Wiring (ungeändert, nur als Vertrag dokumentiert):**
- Varianten: `MergeHdr(merge::MergeArgs)` `crates/lumina-cli/src/main.rs` → `Command::MergeHdr`,
  `MergePano(merge::MergeArgs)` `crates/lumina-cli/src/main.rs` → `Command::MergePano`; Modul-Deklaration `mod merge;`
  `crates/lumina-cli/src/main.rs` → `mod merge`.
- Dispatch: `Command::MergeHdr(args) => merge::merge_hdr(args)` `crates/lumina-cli/src/main.rs` → `Command::MergeHdr`,
  `Command::MergePano(args) => merge::merge_pano(args)` `crates/lumina-cli/src/main.rs` → `Command::MergePano`.
- Einstiege: `merge_hdr`/`merge_pano` `crates/lumina-cli/src/merge.rs` → `merge_hdr`/`merge_pano` → gemeinsamer
  `run_merge(mode, args)` `crates/lumina-cli/src/merge.rs` → `run_merge`.
- MCP-Spiegel: `dispatch_tool`-Arm `merge_hdr::NAME => merge_hdr::run(..)`
  (`crates/lumina-mcp/src/tools/mod.rs` → `dispatch_tool`, Musterarm `regenerate::NAME`) +
  `list_tool_definitions`-Eintrag (`crates/lumina-mcp/src/tools/mod.rs` → `list_tool_definitions`).

**Fehlerpräfixe (kanonisch):** `missing`/`stale`/`unsupported` sind als
Kanon in `crates/lumina-cli/src/merge.rs` → Modul-Doc festgehalten; `failed` steht
**nicht** dort, sondern als vierte Variante in
`crates/lumina-merge/src/bundle.rs` → `MergeRunError` (`MergeRunError::Failed`, Doc-
Kommentar an der Variante `Failed`). Der gerenderte Präfix-Text kommt einheitlich aus
`crates/lumina-merge/src/bundle.rs` → `impl std::fmt::Display for MergeRunError`.

| Klasse | CLI-Präfix (stderr, Exit 1) | MCP-`McpError`-Variante | stabiler `error`-Name (`data.error`) | JSON-RPC-Code |
| --- | --- | --- | --- | --- |
| Quelle/DNG/Sidecar fehlt | `merge missing:` | `McpError::FileNotFound` | `FileNotFound` | `-32001` |
| Bundle matched Quellen nicht | `merge stale:` | **neu: `McpError::MergeStale`** — ODER `McpError::Sidecar` (Name `SidecarError`) | `MergeStale` (bzw. `SidecarError`) | `-32011` (bzw. `-32009`) |
| Dekodierung/Geometrie/Belichtung/Dimension/Writer-Limit | `merge unsupported:` | **neu: `McpError::MergeUnsupported`** — ODER `McpError::Decode`/`InvalidParams` je Ursache | `MergeUnsupported` | `-32012` |
| unerwartetes I/O / Serialisierung / Validierung | `merge failed:` | **neu: `McpError::MergeFailed`** — ODER `McpError::Internal` | `MergeFailed` | `-32013` |
| Tool-Argumente (Listen-Arity, Typfehler, unbekanntes Feld) | — (CLI: `check_explicit_list_lengths` / `output`-Typ) | `McpError::InvalidParams` | `InvalidParams` | `-32602` |

Verbindliche Randbedingungen für das Mapping:
- Die vier `MergeRunError`-Varianten tragen ihren Präfix-Text **im
  `Display`**: `write!(f, "merge {}: {}", self.prefix(), self.message())`
  (`crates/lumina-merge/src/bundle.rs` → `impl std::fmt::Display for MergeRunError`, `MergeRunError::prefix`). Der MCP-Adapter mappt
  `MergeRunError` → `McpError` und lässt den Originaltext im `message`-Feld
  unverändert, damit `merge missing:`/`merge stale:`/… clientseitig
  substring-prüfbar bleibt (Muster: `result_payload`/`map_error`,
  `crates/lumina-mcp/src/tools/library_common.rs` → `result_payload`/`map_error`; Fehlertext-Trägerschaft `crates/lumina-mcp/src/error.rs` → `McpError::message`).
- **Kein `debug!`- oder stiller No-Op-Pfad** (Prinzip §11 „Kein stiller
  No-Op"): `dispatch_tool`'s
  Fall-through ist `MethodNotFound` (`crates/lumina-mcp/src/tools/mod.rs` → `dispatch_tool`); ein Merge-Fehler ist
  immer `isError: true` mit `structuredContent = error.data()`
  (`crates/lumina-mcp/src/error.rs` → `tool_error_result`).
- **Quellzahl ist Merge-Domäne, nicht Tool-Validierung:** 0, 1 oder 257
  Quellen landen über `validate_source_count` (`crates/lumina-merge/src/bundle.rs` → `validate_source_count`, Aufruf
  in `run_merge`) in `MergeRunError::Failed` und damit in der `MergeFailed`-Zeile —
  **nicht** in der `InvalidParams`-Zeile. Sonst wäre ein und dieselbe Eingabe
  im CLI `merge failed:` und im MCP `InvalidParams` (Verstoß gegen T17).
- **`output`-Form ist ebenfalls Merge-Domäne:** `.dng`-Endung
  (`crates/lumina-merge/src/bundle.rs` → `resolve_dng_path`), fehlender Dateiname (`resolve_dng_path`) und Ziel == Quelle
  (Prüfschleife `resolve_dng_path`, Ablehnungszweig `resolve_dng_path`) laufen über
  `resolve_dng_path` → `MergeRunError::Failed` → `MergeFailed`. Der Handler
  prüft an `output` nur, ob ein vorhandenes Feld ein String ist — ein
  fehlendes/`null`/absentes Feld ist der gültige Default (§2 Pkt. 3).
- **Residual-Warnung** (`crates/lumina-cli/src/merge.rs` → `run_merge`): `warning: {command} aligned with
  residual {:.2}px …` muss als MCP-Feld `warning` (string, nur gesetzt wenn
  `residual_warning`) auftauchen — **nicht** als separates `warnings[]`-Array,
  weil das CLI-JSON (§5) kein solches Feld hat und 1:1-Parität der Felder
  gefordert ist. TODO/Placeholder: Feldname + genaue Textform ist mit dem
  Eigentümer zu bestätigen (§13 Punkt 5); die CLI-Zeile ist wörtlich aus
  `crates/lumina-cli/src/merge.rs` → `run_merge` übernehmbar.

## 4. MCP-Dispatch-/Toolmaking-Muster (aus zwei Referenz-Handlern)

**Der Tool-Vertrag:** `crates/lumina-mcp/src/tools/mod.rs` → Modul-Doc legt je Submodul drei Exporte fest —
`NAME` (&str), `schema() -> Value` und `run(server, args) -> Result<Value,
McpError>`. `DESCRIPTION` (&str) ist im Modul-Doc nicht genannt, aber in jedem
Handler gesetzt und wird von `tool_def` übernommen (z. B.
`crates/lumina-mcp/src/tools/generative.rs` → `NAME`/`DESCRIPTION`, `crates/lumina-mcp/src/tools/dust_removal.rs` → `NAME`/`DESCRIPTION`). Registrierung erfolgt an
**vier** Stellen: `pub mod` (`crates/lumina-mcp/src/tools/mod.rs` → `pub mod`-Deklarationen), `list_tool_definitions`,
`dispatch_tool` und — testseitig — `TOOL_NAMES`
(`crates/lumina-mcp/tests/tool_names/mod.rs` → `TOOL_NAMES`) plus `EXPECTED_TOOLS`
(`crates/lumina-cli/src/tests/mcp_onnx.rs` → `EXPECTED_TOOLS`, zweiter Pin). `is_known_tool`
leitet sich aus `list_tool_definitions` ab, ein separater Eintrag
ist nicht nötig.

### 4.1 Referenz-Handler A — `lumina_dust_removal` (path-basiert, mit Fixture-I/O)

Aufbau (`crates/lumina-mcp/src/tools/dust_removal.rs`), in genau dieser Reihenfolge:

1. `run(_server, args)` Signatur (`crates/lumina-mcp/src/tools/dust_removal.rs` → `run`); `_server` bleibt ungenutzt — der
   Server ist hier reiner Lebensraum, kein Session-State. **Für
   `merge_hdr`/`merge_pano` identisch übernehmbar.**
2. **Argument-Parsing mit harter Typ-Prüfung, nicht Schema-Vertrauen**
   (`crates/lumina-mcp/src/tools/dust_removal.rs` → `run`): `get_str(args, "input")` (`crates/lumina-mcp/src/util.rs` → `get_str`), danach
   `Path::exists()`-Vorbedingung und — typisch für Artefakt-Schreiber
   — `crates/lumina-mcp/src/output_guard.rs` → `reject_protected_target`
   **vor** jeder Mutation. Der Merge-Adapter hat keinen `render_out` und
   übernimmt die Ordnung „erst prüfen, dann mutieren" auf Feldebene: `output`
   wird auf Typ-Ebene geprüft (§2 Pkt. 3), die **Form**prüfung bleibt bei
   `resolve_dng_path`. **Zeitpunkt korrekt benannt:** `run_merge` dekodiert
   zuerst (`crates/lumina-merge/src/bundle.rs` → `run_merge`), resolved danach die Exposure
   (`crates/lumina-merge/src/bundle.rs` → `run_merge`), merged (`crates/lumina-merge/src/bundle.rs` →
   `align_and_merge`) und ruft
   `resolve_dng_path` erst danach (`crates/lumina-merge/src/bundle.rs` → `resolve_dng_path`). Der `output`-Formfehler kommt
   also **nach** Decode und Alignment — und genau dort tritt er in der CLI
   ebenfalls auf, weil die CLI über denselben `run_merge`-Pfad läuft.
   Fehlerklasse **und** Zeitpunkt sind damit CLI-identisch (T17, T10). Ein
   Handler-Vorab-Check auf die `.dng`-Endung würde lediglich Zeit sparen,
   die Fehlerklasse aber künstlich auf `InvalidParams` ziehen — er
   unterbleibt deshalb.
3. **Nested Objects per `serde_json::from_value`** mit
   `McpError::InvalidParams`-Abbildung — Muster für alle Listen-Args des
   Merge-Tools: ein kleines `struct` mit `#[derive(Deserialize)]` statt
   handgeschachteltem `.get()`/`.as_array()`/`.as_f64()`.
4. **Decode über den geteilten Decoder** (`crates/lumina-mcp/src/util.rs` → `read_and_decode`)
   → Fehlerklasse `McpError::Decode`.
5. **Fehler-Mapping** durchgängig domänenspezifisch: `McpError::FileNotFound`,
   `InvalidParams`, `Sidecar`, `Render`,
   `UnknownCopy`. Kein `McpError::Internal` für erwartbare Fälle.
6. **Erfolgspayload als flaches `json!`-Objekt**, direkt aus den
   Domain-Werten — genau die Form, in der `merge_hdr` die
   `MergeRunOutcome`-Felder zurückgibt (§5).

### 4.2 Referenz-Handler B — `lumina_generative` (op-Vokabular + Report-Envelope)

Aufbau (`crates/lumina-mcp/src/tools/generative.rs`):

1. `ALLOWED`-Liste + `OPS`-Liste, im Schema als `enum`
   abgebildet (`"enum": OPS`).
2. `reject_unknown_fields(args, NAME, ALLOWED)?` als **erste** Zeile von `run`
   → `InvalidParams` bei unbekanntem Feld (`crates/lumina-mcp/src/tools/stage_common.rs` → `reject_unknown_fields`).
3. Pflichtfeld scharf gelesen: fehlt → `InvalidParams("missing `op`")`,
   leerer String → eigener Fehler. **Übertragung auf Merge:** `inputs` fehlt
   oder ist kein Array → `InvalidParams("missing `inputs`")`. Ein **leeres
   Array** dagegen ist ein gültiges `string[]` und fällt der Parity wegen
   (T17) an `validate_source_count` → `merge failed:` (§2 Pkt. 1).
4. `reject_unknown_op(NAME, &op, OPS)?` (`crates/lumina-mcp/src/tools/stage_common.rs` → `reject_unknown_op`).
5. Optionale Felder über `optional_bool`/`optional_str`/`optional_u64`
   (`crates/lumina-mcp/src/tools/stage_common.rs` → `optional_str`/`optional_bool`); `null` == abwesend.
6. Gemeinsamer Domain-Aufruf + `map_error`: aus
   `lumina_stages::generative::run(..., PATH_PERSIST)` (`crates/lumina-mcp/src/tools/library_common.rs` → `PATH_PERSIST`)
   wird über `map_error` → `map_stage_error` (`crates/lumina-mcp/src/tools/library_common.rs` → `map_error`) ein
   `McpError`. **Merge übernimmt diese Zeile 1:1**: `run_merge(...)` →
   `.map_err(|e| map_merge_run_error(NAME, e))` (neuer Mapper, §3-Tabelle).
7. `result_payload(outcome.report)` (`crates/lumina-mcp/src/tools/library_common.rs` → `result_payload`) — fügt
   `status`/`saved`/`action`/`text` zum BulkReport hinzu.
   **Merge nutzt `result_payload` NICHT**: es erwartet einen `BulkReport` und
   würde `saved: false` + `action: ""` erfinden, die das CLI-JSON nicht kennt.
   Stattdessen: `Ok(json!({...MergeRunOutcome-Felder...}))` wie `crates/lumina-mcp/src/tools/import.rs` → `run`.

### 4.3 Direkte Konsequenzen für `merge_hdr` / `merge_pano`

- **Ein gemeinsamer Kern, zwei Namen.** Die beiden Tools unterscheiden sich nur
  im `MergeMode` (`Hdr`/`Panorama`) — `merge_hdr`/`merge_pano` sind heute
  Zwei-Zeiler auf `run_merge(mode, &args)` (`crates/lumina-cli/src/merge.rs` → `merge_hdr`/`merge_pano`). Das MCP-Paar
  bekommt deshalb gemeinsam: `fn run_mode(mode: MergeMode, args: &Value)`
  (Parsing + Validierung + `run_merge`), und `merge_hdr::run`/`merge_pano::run`
  sind je ein Einzeiler darauf. Damit ist die GUI-Invariante „eine
  Implementierung" (§4.1) strukturell erzwungen, nicht nur konventionell.
- **Shared Decode+Exposure-Adapter** (Roadmap-Punkt 2 „Impl-A", §10): der Adapter aus
  `crates/lumina-stages/src/merge.rs` → `decode_source`/`resolve_exposure` (`decode_source`, `merge_exif_from_raw`, `resolve_exposure`,
  `explicit_at`, `resolve_hdr_exposure`, `resolve_pano_exposure`) wandert als
  **reine, clap-freie** Funktionen-Schnittstelle nach `lumina-merge` (oder
  `lumina-stages`); CLI und MCP rufen beide diesen Adapter. TODO/Placeholder:
  Ziel-Crate + Modulname ist offen (§13 Punkt 1); fest steht nur, dass er **keine**
  zweite Kopie wird (F6-Dedup, `crates/lumina-stages/src/merge.rs` → Modul-Doku).
- **`decoder`/`decode_version`/`orientation`** müssen aus **denselben** Quellen
  kommen wie im CLI (`crates/lumina-stages/src/merge.rs` → `decode_source`,
  das den `MergeDecodeContext` baut): `"libraw"` wenn `raw.is_some()` sonst
  `"image"`; `lumina_raw::libraw_decode_version()` bei RAW, sonst die gepinnte
  Konstante `MERGE_IMAGE_DECODER_VERSION` (`"0.1.0"`, in Lockstep mit der
  CLI-Paketversion); `orientation` aus `raw.orientation` desselben
  Decode-Schritts (Default `1`) — **kein** EXIF-Feld: `merge_exif_from_raw`
  mappt nur make/model/lens/exposure/f_number/iso/timestamp. Das ist die
  einzige Stelle, an der ein MCP-eigener Decode-Pfad die
  **Decode-Metadaten** des Digests driften lässt (Pixel-Abweichungen
  erreichen ihn zusätzlich über `alignment.transforms`/`residual_px`).
- **Kein `output_guard` nötig, `resolve_dng_path`-Parität genügt:** die
  `.dng`-Endung und der Quellen-Treffer werden von `resolve_dng_path` geprüft
  (`crates/lumina-merge/src/bundle.rs` → `resolve_dng_path`) — für beide Frontends über denselben Aufruf
  (`crates/lumina-merge/src/bundle.rs` → `run_merge`), also ohne MCP-eigene Kopie. Das deckt die Schreibziele
  vollständig ab; `reject_protected_target` (`crates/lumina-mcp/src/output_guard.rs` → `reject_protected_target`)
  adressiert Sidecar-/Zdata-Bundles, die beim Merge nie Ziel sind.

## 5. JSON-Rückgabe-Felder 1:1 (CLI `--json` ≡ MCP `structuredContent`)

Quelle: `emit(args.json, serde_json::json!({...}), &message)` in
`crates/lumina-cli/src/merge.rs` → `run_merge`. `emit` druckt exakt dieses Objekt
(`crates/lumina-cli/src/main.rs` → `emit`); `tool_result_response` legt ein MCP-Payload-Objekt
unverändert in `structuredContent` (`crates/lumina-mcp/src/error.rs` → `tool_result_response`).

| Feld | Typ | Herkunft (1:1) | Anmerkung |
| --- | --- | --- | --- |
| `command` | string | `merge_command_name(mode)` = `"merge-hdr"` / `"merge-pano"` (`crates/lumina-merge/src/bundle.rs` → `merge_command_name`) | Werkzeug-unabhängig, nicht der MCP-Tool-Name |
| `output` | string | `outcome.dng_path` (als `PathBuf` direkt an `json!` übergeben, serialisiert als String) | absoluter DNG-Pfad |
| `sidecar` | string | `outcome.sidecar_path` (dito) | `<dng>.lumina.json` (`crates/lumina-sidecar/src/sidecar_paths.rs` → `sidecar_path_for`) |
| `digest` | string | `outcome.digest` | Merge-Digest, `blake3:<64 hex>` (`crates/lumina-sidecar/src/merge_recipe.rs` → `merge_digest`) |
| `checksum` | string | `outcome.checksum` | BLAKE3 der DNG-Bytes (`crates/lumina-merge/src/bundle.rs` → `merge_checksum`) |
| `cached` | boolean | `outcome.cached` | `true` = Bundle war schon aktuell, **nichts** neu geschrieben (`crates/lumina-merge/src/bundle.rs` → `MergeRunOutcome`) |
| `status` | string | Konstante `"ok"` | nur im Erfolgsfall |

Verbindliche Zusatzfelder (begründete Abweichung, kein Beliebiges):
- `warning` — string, nur gesetzt wenn `outcome.residual_warning`
  (`crates/lumina-cli/src/merge.rs` → `run_merge`); sonst Feld fehlt.
- **Keine** weiteren Felder: kein `saved`, kein `action`, kein `text`, kein
  `warnings[]`, kein `mode` (ja, `outcome.mode` existiert in
  `crates/lumina-merge/src/bundle.rs` → `MergeRunOutcome::mode` — aber das CLI-JSON gibt es NICHT aus, also darf MCP es
  nicht hinzufügen; `command` trägt dieselbe Information clientseitig
  substring-bar). TODO/Placeholder: wenn der Eigentümer `mode` zusätzlich
  wünscht, gilt das für CLI und MCP gleichermaßen (§13 Punkt 2).

Textkanal: `tool_result_response` serialisiert das Payload zusätzlich als
`content[0].text`. Der CLI-Erfolgstext `merged {path}` / `merge already current:
{path}` (`crates/lumina-cli/src/merge.rs` -> `run_merge`) ist **nicht** Teil des JSON und damit kein
Paritätsgegenstand; für Lesbarkeit KANN er als `content[0].text`-Prefix
ergänzt werden — Placeholder, Eigentümer-Entscheid (§13 Punkt 6).

## 6. Invarianten

1. **Eine Implementierung:** MCP nutzt `run_merge` (wie CLI `crates/lumina-cli/src/merge.rs` → `merge_hdr`/`merge_pano` → `run_merge`),
   kein eigenes Decode/Blend. F6-Dedup bleibt unangetastet.
2. **Decode-Kontext spiegelt die Quelle:** der Adapter baut den
   `MergeDecodeContext` in `crates/lumina-stages/src/merge.rs` → `decode_source`:
   `decoder` `"libraw"` wenn `raw.is_some()` sonst `"image"`; `decode_version`
   `lumina_raw::libraw_decode_version()` bei RAW, sonst die gepinnte Konstante
   `MERGE_IMAGE_DECODER_VERSION` (`"0.1.0"`, Lockstep mit der
   CLI-Paketversion); `orientation` aus `raw.orientation` desselben Schritts
   (Default `1`, **kein** EXIF-Feld). MCP-Adapter muss diese Werte aus
   denselben Routen beziehen, sonst driftet der Digest.
3. **Eingabe-Validierung vor Verarbeitung:** `inputs` ist Pflicht; die
   **Anzahl** prüft allein `validate_source_count` (2..=256,
   `crates/lumina-merge/src/bundle.rs` → `run_merge`, `validate_source_count` — MCP-seitig kein eigener Count-Check, §2 Punkt 1);
   explizite Listen (`exposure_times`/`isos`/
   `f_numbers`) sind entweder leer oder genau `len(inputs)`
   (`check_explicit_list_lengths`, `crates/lumina-stages/src/merge.rs` → `check_explicit_list_lengths`; Unit
   `crates/lumina-stages/src/merge.rs` → `explicit_lists_accept_empty_or_exactly_per_input`). Abbruch laut, keine Teil-Ergebnisse.
4. **HDR-Exposure nie geraten:** explicit wins, sonst EXIF
   `exposure_time_s`/`iso`/`f_number`, sonst `unsupported`
   (`resolve_hdr_exposure`, `crates/lumina-stages/src/merge.rs` → `resolve_hdr_exposure`; die Range-Checks
   selbst: `crates/lumina-sidecar/src/merge_recipe.rs` → `MAX_MERGE_EXPOSURE_TIME_S`/`MAX_MERGE_ISO`/
   `MAX_MERGE_F_NUMBER`). Units: `crates/lumina-stages/src/merge.rs` → `hdr_without_explicit_or_exif_exposure_is_unsupported`/`hdr_rejects_out_of_range_explicit_values`.
5. **Pano-Exposure ist nur Provenienz:** EXIF oder neutrale Defaults
   `PANO_DEFAULT_EXPOSURE_S/_ISO/_F_NUMBER` (0,01 s / 100 / f/8,
   `crates/lumina-merge/src/bundle.rs`); Pixel-Merge nutzt Exposure nicht
   (`resolve_pano_exposure`, `crates/lumina-stages/src/merge.rs` → `resolve_pano_exposure`; Unit `crates/lumina-stages/src/merge.rs` → `pano_without_exif_uses_the_documented_neutral_default`).
   Wichtig für MCP: auch ein MCP-Aufruf ohne jede Exposure-Liste darf für Pano
   **nicht** scheitern.
6. **Stale/Missing ist hart:** fehlende Quelle/DNG/Sidecar → `missing`;
   Hash-/Decode-/DNG-Prüfsummen-Abweichung → `stale`; Re-Merge nur mit
   `force` (`crates/lumina-cli/src/merge.rs` → Modul-Doc, `crates/lumina-merge/src/bundle/security.rs` → `check_existing_bundle`; E2E
   `crates/lumina-cli/tests/merge_e2e.rs` → `merge_stale_after_source_change_and_force_regenerates`). Persistenz-Invarianten in
   `feature/decisions/LRPAR-G13-MERGE-15.md` §Normative Invarianten.
7. **Originale unverändert; relative Pfade; atomarer Write; kein stiller
   Fallback** (dasselbe SOLL-Dokument §Normative Invarianten, Absolutpfade
   verboten; E2E `crates/lumina-cli/tests/merge_e2e.rs` → `merge_leaves_source_sidecars_untouched` für Quell-Sidecars, `crates/lumina-cli/tests/merge_e2e.rs` →
   `merge_source_outside_bundle_is_unsupported` für
   Quellen außerhalb des Bundles).
8. **Rückgabe identisch:** MCP liefert `command/output/sidecar/digest/checksum/
   cached/status` 1:1 wie CLI-JSON (`crates/lumina-cli/src/merge.rs` → `run_merge`, §5).
   Cache-Hit (`cached=true`) schreibt nichts neu, exit-equivalent 0
   (`crates/lumina-merge/src/bundle.rs` → `MergeRunOutcome`, E2E `crates/lumina-cli/tests/merge_e2e.rs` → `merge_is_idempotent_without_force`).
9. **Residual-Warnung erhalten:** `residual_warning` wird als `warning: ...` auf
   stderr gemeldet, nicht unterdrückt (`crates/lumina-cli/src/merge.rs` → `run_merge`, E2E
   `crates/lumina-cli/tests/merge_e2e.rs` → `merge_hdr_large_shift_reports_residual_warning_without_aborting`).
10. **Merge-seitige Ablehnungen bleiben Merge-seitig:** `blend_width_px >
    65536` und `residual_px`-Grenzen laufen über `recipe.validate()` →
    `MergeRunError::Failed` (`crates/lumina-merge/src/bundle.rs` → `run_merge`, Regelwerk
    `crates/lumina-sidecar/src/merge_recipe_alignment.rs` → `validate_merge_alignment`) und werden `merge failed:`; ein `DngError`,
    das kein `Unsupported` ist, ebenfalls (`map_dng_error`,
    `crates/lumina-merge/src/bundle.rs` → `map_dng_error`). `merge unsupported:` bleibt reserviert für die
    Merge-Domäne (Alignment/Geometrie/Belichtung, `map_merge_error`
    `crates/lumina-merge/src/bundle.rs` → `map_merge_error`, `crates/lumina-merge/src/align.rs` → `estimate_pano_transform`) und für Writer-Grenzen
    (`DngError::Unsupported`, `crates/lumina-merge/src/bundle.rs` → `map_dng_error`). Der MCP-Handler darf
    diese nicht in `InvalidParams` umdeuten — sonst unterscheiden sich CLI
    und MCP in der Fehlerklasse für dieselbe Eingabe (T17).

## 7. Fixtures / Goldens

- **Hash-Parity-Anker (synthetisch):** zwei 64×48 Graustufen-PNGs im Gradienten
  `level + x % 16` (`crates/lumina-cli/tests/merge_parity.rs` → `write_png`) mit
  expliziten Exposure/ISO/f-number-Werten; byte-stabiler Digest + DNG-Checksum
  (`crates/lumina-cli/tests/merge_parity.rs` → `cli_and_gui_contract_paths_share_the_same_anchor`).
- **MCP-Parity-Test (neu, SOLL):** derselbe Anker aus MCP-Seite aufrufen
  (MCP-Adapter-Decode + `run_merge`) und `digest`/`checksum` mit CLI-Run
  vergleichen — Spiegel von `merge_parity.rs`, nur Transport differs.
- **Fehlerpfad-Fixtures, getrennt nach Fehlerklasse** (weil die Klasse selbst
  Teil des Vertrags ist, §3):
  - **→ `merge failed:`** — 0, 1 oder 257 Quellen. Code:
    `validate_source_count` (`crates/lumina-merge/src/bundle.rs` → `validate_source_count`, Aufruf in `run_merge`). Ein
    merge-eigener **Unit**-Test dafür existiert **nicht** (in
    `crates/lumina-stages/src/merge.rs` → `mod tests` stehen nur die vier unten genannten); der Nachweis
    läuft über die E2E-Spiegel (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_rejects_single_source_loudly` prüft „at least 2" für
    1 Quelle).
  - **→ `merge unsupported:`** — HDR ohne EXIF/Explicit-Belichtung
    (`resolve_hdr_exposure`, `crates/lumina-stages/src/merge.rs` → `resolve_hdr_exposure`; Unit
    `crates/lumina-stages/src/merge.rs` → `hdr_without_explicit_or_exif_exposure_is_unsupported`), Out-of-Range explizite Werte (`crates/lumina-stages/src/merge.rs` → `resolve_hdr_exposure`;
    Unit `crates/lumina-stages/src/merge.rs` → `hdr_rejects_out_of_range_explicit_values`), Dimension-Mismatch und zu kleine Frames
    (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_dimension_mismatch_is_unsupported`). Pano ohne Exposure ist dagegen **kein**
    Fehlerfall, sondern die dokumentierten neutralen Defaults (Unit
    `crates/lumina-stages/src/merge.rs` → `pano_without_exif_uses_the_documented_neutral_default`).
  - **→ CLI-seitiger `InvalidParams`-Äquivalent (MCP: `InvalidParams`)** —
    Listen-Arity: eine explizite Liste mit 0 < len ≠ `inputs.len()`
    (`check_explicit_list_lengths`, `crates/lumina-stages/src/merge.rs` → `check_explicit_list_lengths`; Unit
    `crates/lumina-stages/src/merge.rs` → `explicit_lists_accept_empty_or_exactly_per_input`). Die CLI meldet das als `CliError::Message` (exit 1),
    nicht als `MergeRunError`.
- **Real-RAW-Gate (Abnahme-Gate, Agents.todo.md MCP-PARITY-C):** mindestens
  eine echte Kamera-RAW-Belichtungsreihe (≥3 Frames) und eine Panorama-Reihe;
  PNG-Mocks decken I/O/Decoder nicht ab. Goldens als BLAKE3-Digest + DNG-Checksum.
  **Inventar-Lücke (neu, belegt):** die zwei committeten R1-CR3s
  (`sample-data/raw/aircraft-landscape.cr3`, `-portrait.cr3`,
  `feature/quality/golden-fixtures.md` §2 → `R1 — echtes RAW-Fixture`, `sample-data/raw/README.md`) sind **keine**
  Belichtungsreihe und **keine** Panorama-Reihe — verschiedene Motive, 1/1000 s
  / ISO 1000 bzw. 1250. Für das Gate braucht es also eine **neue** Fixture-Serie
  (TODO/Placeholder, §13 Punkt 3); bis dahin deckt das synthetische Paar die
  Byte-Parität ab und das Gate bleibt offen.
- **Toleranzen:** keine für synthetische/quantisierte Pfade (deterministisch).
  Float-Toleranzen falls alignment-subpixel variieren — der normative Ort ist
  `feature/decisions/LRPAR-G13-MERGE-15.md` §Normative Invarianten
  (Invariante „Deklarativ, versioniert, reproduzierbar"), **nicht** §Offene Risiken.

## 8. Testmatrix (Phase 1–3)

Notation: **[U]** Unit (in-crate, `#[cfg(test)]`), **[P]** Parity (CLI-Run vs.
MCP-Run, gleiche Eingabemenge), **[N]** Negativ/Fehlerpfad, **[G]** Gate
(hardware/RAW oder manuell). „Fixture" meint immer das kleinste Objekt, das die
Aussage trägt; PNG-Mocks sind erlaubt, wo die Aussage transportseitig ist.

### Phase 1 — Registry & Schema (kein Pixel, kein I/O)

| ID | Art | Ort (SOLL) | Prüft | Fixture |
| --- | --- | --- | --- | --- |
| T1 | [U] | neue Testdatei `crates/lumina-mcp/tests/merge_tools.rs` (Name wie §10.1) | `TOOL_NAMES` + `list_tool_definitions` + `is_known_tool` enthalten `lumina_merge_hdr` **und** `lumina_merge_pano`, keine Duplikate, Zählguard — Muster `crates/lumina-mcp/tests/library_tools.rs` → `all_five_library_tools_are_registered_in_both_paths` und `crates/lumina-mcp/tests/tool_names/mod.rs` → `tools_list_reports_exactly_the_documented_tool_names` | keine |
| T2 | [U] | `crates/lumina-cli/src/tests/mcp_onnx.rs` → `mcp_subcommand_pipeline_answers_handshake_and_lists_all_tools` | zweiter Pin (`tools.len()`, `EXPECTED_TOOLS`) um beide Namen gewachsen, 26 → **28** | keine |
| T3 | [U] | `crates/lumina-mcp/tests/merge_tools.rs` | `tools/list` liefert **beide** Schemas: `additionalProperties:false`, `required == ["inputs"]`, `enum`/`minimum`-Angaben gem. §2-Tabelle | keine |
| T4 | [U] | `lumina-mcp` Unit in `merge_common`/`merge_hdr` | Schema-JSON der beiden Tools ist bis auf `description`/`enum` gleich → ein Kern, zwei Namen | keine |

### Phase 2 — Validierung & Fehlerklassen (kein Merge-Durchlauf)

| ID | Art | Ort (SOLL) | Prüft | Fixture |
| --- | --- | --- | --- | --- |
| T5 | [U] | `lumina-mcp` Unit (Spiegel `crates/lumina-stages/src/merge.rs` → `explicit_lists_accept_empty_or_exactly_per_input`) | `inputs` fehlt oder ist kein Array → `InvalidParams`; **0, 1 oder 257 Elemente → `MergeFailed`** (über `crates/lumina-merge/src/bundle.rs` → `validate_source_count` — **nicht** `InvalidParams`, §2 Pkt. 1); Listen-Arity 0/N-Regel sonst Fehler | keine |
| T6 | [U] | `lumina-mcp` Unit | `output` vorhanden, aber kein String → `InvalidParams`; `output` **ohne** `.dng`-Endung, `output == inputs[i]`, `output` leer → `MergeFailed` (über `crates/lumina-merge/src/bundle.rs` → `resolve_dng_path` — **nicht** `InvalidParams`, §2 Pkt. 3) | 1 PNG (32×24) |
| T7 | [U] | `lumina-mcp` Unit | **JSON-Typfehler** → `InvalidParams`: `isos: [1.5]` (kein Integer), `exposure_times: ["x"]` (keine Zahl), `force` als Nicht-Bool. **Range**-Verstoß `blend_width_px: 65537` → `MergeFailed` über `recipe.validate()` (`crates/lumina-merge/src/bundle.rs` → `run_merge`, `crates/lumina-sidecar/src/merge_recipe.rs` → `MAX_MERGE_BLEND_WIDTH_PX`, `crates/lumina-sidecar/src/merge_recipe_alignment.rs` → `validate_merge_alignment`) — **nicht** `InvalidParams`, §2 Pkt. 4 | keine |
| T8 | [U] | `lumina-mcp` Unit | unbekanntes Feld → `InvalidParams` via `reject_unknown_fields` | keine |
| T9 | [N] | `lumina-mcp` Test | fehlende Quelldatei → `FileNotFound`, `data.error == "FileNotFound"` | Tempdir-Pfad ohne Datei |
| T10 | [N] | `merge_e2e.rs`-Spiegel | 1 Quelle → `merge failed:` (gemappt `MergeFailed`, §3-Tabelle); **nicht** `InvalidParams` — der Count-Check ist Merge-seitig | 1 PNG |

### Phase 3 — Byte-Parität & Artefakte (der Kern)

| ID | Art | Ort (SOLL) | Prüft | Fixture |
| --- | --- | --- | --- | --- |
| T11 | [P] | neu: `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) (Spiegel von `crates/lumina-cli/tests/library_parity.rs` → `collections_read_is_byte_identical_to_the_cli` + `crates/lumina-cli/tests/merge_parity.rs` → `cli_and_gui_contract_paths_share_the_same_anchor`) | gleiche Eingabemenge über CLI-Binary und MCP-Server → **identischer** `digest` **und** `checksum` (String-Gleichheit, `assert_eq!`), `command`/`status` identisch | 2× 64×48 PNG (`crates/lumina-cli/tests/merge_parity.rs` → `write_png`), Exposition explizit |
| T12 | [P] | neu: `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) | DNG-Bytes auf Platte identisch: `blake3(MCP-DNG) == blake3(CLI-DNG)` und `== checksum`-Feld | wie T11, zwei Bäume (Prinzip: CLI-Prozess auf Baum 1, MCP in-process auf byte-identischem Baum 2 — Modul-Doc `crates/lumina-cli/tests/library_parity_common/mod.rs`) |
| T13 | [P] | neu: `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) | Sidecar-Bytes identisch (`assert_unchanged`-Muster, `crates/lumina-cli/tests/library_parity_common/mod.rs` → `assert_unchanged`): `"type": "merge"`, `merge_recipe.mode`, `merge_version 1`, `status ok`, keine Absolutpfade (`crates/lumina-cli/tests/merge_e2e.rs` → `assert_no_absolute_paths`, `merge_hdr_produces_dng_bundle_and_reimportable_dng`) | wie T11 |
| T14 | [P] | neu: `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) | Pano-Parität: gleiche Eingabemenge, gleicher Digest/Checksum, Canvas wächst um den geschätzten Offset (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_pano_produces_wider_dng`) | 2 PNG, einer um 4 px verschoben |
| T15 | [P] | neu | **Cache-Hit**: zweiter identischer MCP-Aufruf → `cached: true`, `status: "ok"`, DNG-/Sidecar-Bytes unverändert (`assert_unchanged`), kein `saved`/`action`-Feld | wie T11 |
| T16 | [P] | neu | `force`-Kette: Quelle mutieren → MCP meldet `stale` (mapped, `crates/lumina-merge/src/bundle/security.rs` → `check_existing_bundle`); mit `force: true` → Erfolg **und** Digest == CLI-Digest nach Force-Merge | wie T11 + mutiertes PNG (`crates/lumina-cli/tests/library_parity_common/mod.rs` → `mutated_png`) |
| T17 | [P] | neu | Fehlerklassen-Gleichheit: für dieselbe Eingabe liefern CLI-stderr-Präfix und MCP-`data.error` dieselbe Klasse (Tabelle §3); Fehlertext identisch | HDR ohne EXPLIZIT/EXIF, 8×8 zu klein (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_undersized_frames_are_unsupported`), Dimension-Mismatch (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_dimension_mismatch_is_unsupported`) |
| T18 | [N] | `merge_e2e.rs`-Spiegel | jede Ablehnung hinterlässt **nichts**: `snapshot` vorher/nachher identisch (`crates/lumina-cli/tests/library_parity_common/mod.rs` → `snapshot`) | wie T17 |
| T19 | [U]/[P] | `merge_e2e.rs`-Spiegel | Quell-Sidecars und Quell-Bytes unverändert (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_leaves_source_sidecars_untouched`); Quelle außerhalb des Bundles → `unsupported` (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_source_outside_bundle_is_unsupported`) | PNGs mit Sidecar |
| T20 | [P] | neu | Envelope-Ablehnung 1:1: Standard-Sidecar am Ziel → laut (`crates/lumina-cli/tests/merge_envelope_e2e.rs` → `merge_standard_sidecar_is_rejected_loudly`); unbekannter `type` → laut (`crates/lumina-cli/tests/merge_envelope_e2e.rs` → `merge_unknown_envelope_type_is_rejected_loudly`); MCP-Klasse == CLI-Klasse | wie T11 |
| T21 | [G] | `crates/lumina-cli/tests/merge_parity.rs`-Erweiterung ODER neue Datei, `#[ignore]` | Real-RAW-Gate: echte RAW-Belichtungsreihe (≥3) + Panorama-Reihe → identischer Digest/Checksum über CLI und MCP. **Offen solange keine Fixture existiert** (§7, §13 Punkt 3) | **TODO — neue RAW-Fixture** |

### Gates, die ausdrücklich **nicht** behauptet werden

- Real-RAW-I/O (§7, T21): offen, bis Fixture vorhanden.
- GPU/Adapter: nicht berührt, Merge ist CPU/pure-Rust
  (`crates/lumina-merge/Cargo.toml` → `[dependencies]`); kein GPU-Gate nötig und keines behauptet.
- `cargo`-Läufe sind in der SOLL-Phase verboten (§9); alle Commands stehen als
  Soll-Kommandos in der Verify-Phase.

## 9. Akzeptanz

- Registry: `tool_names::TOOL_NAMES` + `list_tool_definitions` +
  `is_known_tool` um zwei Namen gewachsen, Zählguard
  (`crates/lumina-mcp/tests/tool_names/mod.rs` → `TOOL_NAMES`) + `mcp_onnx.rs` second pin
  (26 → 28, `crates/lumina-cli/src/tests/mcp_onnx.rs` → `EXPECTED_TOOLS`) grün. Kein doppelter Name.
- `tools/list` liefert beide Schemas (`additionalProperties:false`, `inputs`
  required, default-Werte dokumentiert) — MCP-Vertrag aus §2.
- MCP+CLI Produzieren denselben `digest`+`checksum` auf synthetischem Anker
  (SOLL-Test grün) und auf Real-RAW-Gate.
- Fehlerpfade loud: missing/stale/unsupported **und** failed mit denselben
  Präfixen (Kanon `crates/lumina-cli/src/merge.rs` → Modul-Doc + `crates/lumina-merge/src/bundle.rs` → `MergeRunError::Failed`), keine
  silent partial, keine `cached`-Fälschung.
- `cargo test -p lumina-mcp` + `cargo test -p lumina-cli --test merge_parity`
  grün; `cargo fmt --all -- --check` grün. (Kein `cargo` während SOLL-Phase.)

### 9.1 Abnahmekriterien für den ersten Code-Slice

„Stubs nicht ausreichend": der erste Slice muss auf einem HDR-Fixture eine
**vollständige, byte-paritäre Antwort** liefern. Konkret, mit dem Anker aus
`crates/lumina-cli/tests/merge_parity.rs` → `write_png` (zwei 64×48 PNGs, Stufen 40 und
160, `--exposure-times 0.004,0.016 --isos 100,100 --f-numbers 8,8`):

1. `lumina_merge_hdr` antwortet auf `tools/call` mit `isError: false` und einem
   `structuredContent`, das **exakt** die sieben Felder aus §5 trägt
   (`command`, `output`, `sidecar`, `digest`, `checksum`, `cached`, `status`);
   kein `saved`, kein `action`, kein `text`, kein `mode`.
2. `command == "merge-hdr"`, `status == "ok"`, `cached == false`.
3. `digest` ist ein String `blake3:<64 hex>` und **identisch** mit dem Digest,
   den die CLI-Binary für dieselbe Eingabemenge über
   `--json` ausgibt (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_hdr_json_report_parses` prüft genau diese Form).
4. `checksum` ist `blake3:<64 hex>`, identisch mit dem CLI-Wert, **und**
   gleich `blake3` der geschriebenen DNG-Datei (`blake3_file`-Muster
   `crates/lumina-cli/tests/merge_e2e.rs` → `blake3_file`).
5. Das DNG liegt unter `output`, ist **byte-identisch** mit dem CLI-DNG
   (gleicher Baumtyp, T12) und dekodiert über `lumina_raw` auf **64×48** — die
   Fixture-Abmessung des Ankers (`crates/lumina-cli/tests/merge_parity.rs` → `write_png`); dass das DNG
   decodierbar ist und die Geometrie hält, prüft das Re-Import-Muster aus
   `crates/lumina-cli/tests/merge_e2e.rs` → `merge_hdr_produces_dng_bundle_and_reimportable_dng` (`lumina_raw::decode_bytes`
   + `assert_eq!((w, h), (W, H))`) — dort mit `W = 32`/`H = 24`
   (`crates/lumina-cli/tests/merge_e2e.rs` → `W`/`H`), also der eigenen Fixture-Größe jenes Tests
   anzupassen, nicht 64×48.
6. Der Sidecar unter `sidecar` trägt `"type": "merge"`,
   `merge_recipe.mode == "hdr"`, `merge_version == 1`, `status == "ok"`,
   `merge_artifact.checksum == checksum`, nur relative Quellpfade
   (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_hdr_produces_dng_bundle_and_reimportable_dng`).
7. Der zweite identische Aufruf liefert `cached == true` und ändert **kein**
   Byte an DNG oder Sidecar (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_is_idempotent_without_force`).
8. Ein Pano-Fixture-Paar (gleicher Anker, `merge-pano`) erfüllt 1–7 mit
   `command == "merge-pano"` und einem um den Offset verbreiterten Canvas
   (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_pano_produces_wider_dng`); HDR ohne Exposure scheitert laut als
   `unsupported` (`crates/lumina-cli/tests/merge_e2e.rs` → `merge_hdr_without_exposure_is_unsupported`), Pano ohne Exposure gelingt mit den
   neutralen Defaults (`crates/lumina-stages/src/merge.rs` → `pano_without_exif_uses_the_documented_neutral_default`).
9. Jede Fehleraussage trägt den kanonischen Präfix-Text im `message` und den
   stabilen Namen in `data.error` (§3-Tabelle); `dispatch_tool` kennt beide
   Namen (kein `MethodNotFound`-Durchreichnen, `crates/lumina-mcp/src/tools/mod.rs` → `dispatch_tool`).
10. Kein Tool darf seine Antwort aus einem statischen String zusammenbauen:
    T11–T16 sind genau die Tests, die einen Stub rot machen.

## 10. Roadmap (SOLL → Impl → Verify)

1. **SOLL (dieses Doc):** Review/Sign-off; ggf. Tool-Namen finalisieren.
2. **Impl-A — geteilten Adapter freilegen:** Merge-Decode-Adapter aus
   `crates/lumina-cli/src/merge.rs` in `lumina-stages` (oder `lumina-merge`-Crate)
   ziehen, damit MCP exakt dieselbe Decode-Logik wie CLI nutzt (keine Kopie).
   Ziel-Crate offen (§13 Punkt 1).
   (**Anker überholt:** der Adapter ist inzwischen in
   `crates/lumina-stages/src/merge.rs` → `decode_source`/`resolve_exposure`
   gelandet — CLI und MCP rufen dieselben Adapter.)
3. **Impl-B — Tool-Module:** `crates/lumina-mcp/src/tools/merge_hdr.rs`,
   `crates/lumina-mcp/src/tools/merge_pano.rs` angelegt, analog `crates/lumina-mcp/src/tools/generative.rs` → `NAME`/`schema()`/`run`; Register in
   `crates/lumina-mcp/src/tools/mod.rs` (list + dispatch) und `crates/lumina-mcp/tests/tool_names/mod.rs` (2 Namen).
   Gemeinsamer Kern `merge_common` (§4.3), damit `MergeMode` die einzige
   Variable ist.
4. **Impl-C — Error-Mapping:** CLI-`MergeRunError` (Missing/Stale/Unsupported/Failed)
   → `McpError` mit stabilen Namen; Prefizes erhalten (§3).
5. **Impl-D — Parity-Tests:** MCP-Seite Spiegel von `merge_parity.rs` +
   Listenlängen-/Exposure-Fehlerpfad-Tests; Real-RAW-Gate-Test (`#[ignore]`
   bis Fixture vorhanden).
6. **Verify:** CI:fmt + clippy + unit/parity + Real-RAW-Gate auf
   Referenzmaschine; Verifikationsbericht (End-to-End-Kette, Spez→Test-Mapping)
   gem. `DoD.md` §7.

### 10.1 Phase-1-Scaffold-Entwurf

**Neue Dateien (neu anzulegen):**

| Datei | Inhalt | Größen-Risiko |
| --- | --- | --- |
| `crates/lumina-mcp/src/tools/merge_common.rs` | `NAME`-loses Kernmodul: `run_mode(mode, args)`, Argument-Parsing (§2), Validierung, `map_merge_run_error`, Payload-Bau (§5) | ~200 Z., unter 500 |
| `crates/lumina-mcp/src/tools/merge_hdr.rs` | `NAME`/`DESCRIPTION`/`schema()` + `run` → `merge_common::run_mode(MergeMode::Hdr, args)` | ~40 Z. |
| `crates/lumina-mcp/src/tools/merge_pano.rs` | dito, `MergeMode::Panorama` | ~40 Z. |
| `crates/lumina-mcp/tests/merge_tools.rs` (+ `crates/lumina-mcp/tests/merge_tools_common/mod.rs`) | T1, T3, T4, T5–T10 | ~250 Z. |
| `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) | T11–T20 (mit Fixture-Bäumen wie `crates/lumina-cli/tests/library_parity_common/mod.rs`) | ~300 Z., droht >500 → dann aufteilen in `merge_parity.rs` + `merge_parity_errors.rs` (Muster `library_parity*.rs`) |

**Zu modifizierende Dateien (minimal-invasiv):**

| Datei | Änderung |
| --- | --- |
| `crates/lumina-mcp/src/tools/mod.rs` | +2 `pub mod`, +2 `tool_def(...)` in `list_tool_definitions`, +2 Arme in `dispatch_tool`. Aktuell 164 Z. → bleibt unter 500 |
| `crates/lumina-mcp/src/error.rs` | optional neue Varianten `MergeStale`/`MergeUnsupported`/`MergeFailed` + `code()`/`name()`/`data()`. Aktuell 231 Z. |
| `crates/lumina-mcp/Cargo.toml` | +`lumina-merge = { path = "../lumina-merge" }` (fehlt aktuell im `[dependencies]`-Block). Prüfen: kein Zyklus (`lumina-merge` hängt nur an core/sidecar/tiff, `crates/lumina-merge/Cargo.toml` → `[dependencies]`) |
| `crates/lumina-mcp/tests/tool_names/mod.rs` | +2 Namen in `TOOL_NAMES` |
| `crates/lumina-cli/src/tests/mcp_onnx.rs` | +2 Namen in `EXPECTED_TOOLS`, 26 → 28 (`crates/lumina-cli/src/tests/mcp_onnx.rs` → `EXPECTED_TOOLS`) |

**NICHT anfassen:**

- `crates/lumina-cli/src/merge.rs` — CLI bleibt Referenz; nur der Adapter wird
  in Impl-A herausgezogen (dort dann ein dünner Re-Export).
- `crates/lumina-cli/src/main.rs` — **baseline-gesperrt** (7874 Z.,
  Eintrag in `scripts/file_size_baseline.txt`); jede Zeile Wachstum bricht
  `scripts/check_file_sizes.sh`. Registry-Änderungen für MCP gehören nach
  `lumina-mcp`, nicht hierher.
- `crates/lumina-cli/tests/merge_e2e.rs` (799 Z., Eintrag in
  `scripts/file_size_baseline.txt`) — die
  MCP-Fehlerpfade gehen in die **neue** `merge_parity.rs`, nicht hierhin.
- `crates/lumina-merge/**` — Merge-Mathematik, DNG-Writer, Orchestrierung
  bleiben byte-identisch (nur Adapter-Aufnahme in Impl-A).
- `crates/lumina-sidecar/**` — Schema/DNG-Tags unverändert.
- `crates/lumina-gui/**` — GUI-frei in diesem Slice (`Agents.todo.md` → Eintrag `MCP-PARITY-C`);
  `crates/lumina-gui/src/merge_gui.rs` → `run_merge_sync` ist ein zweiter GUI-Adapter und bleibt
  unberührt (seine Vertragstreue ist der parity test
  `crates/lumina-cli/tests/merge_parity.rs` → `cli_and_gui_contract_paths_share_the_same_anchor`,
  GUI-Seite `decode_gui_contract`).
- `crates/lumina-cli/tests/merge_parity.rs` — bestehender CLI↔GUI-Anker
  bleibt; der MCP-Spiegel ist eine eigene Datei.
- `scripts/file_size_baseline.txt` — kein neuer Eintrag ohne Not.

**Rollback-Kriterium (ein Satz, vorab festgelegt):** schlägt einer der Schritte
T11–T16 (Byte-Parität, Cache-Hit, Force-Kette) fehl und lässt sich **nicht**
durch Adapter-Anpassung allein reparieren — d.h. der Fix würde
`lumina-merge`- oder `crates/lumina-cli/src/merge.rs`-Verhalten ändern — wird der
gesamte Code-Slice verworfen (`git revert`/Branch-Verwerfen), statt die
CLI-Referenz zu verbiegen. Zusatz: jedes neue Modul, das 500 Z. überschreitet,
wird vor dem Commit aufgeteilt (Ratchet, `check_file_sizes.sh`).

## 11. Abgrenzung

- **Nicht Teil:** `matrix` (eigener Sub-Slice von MCP-PARITY-C), Denoise
  (`LRPAR-G14`), Cull, Smart-Collections, Auto-Tone.
- **Keine Änderung:** Merge-Mathematik, DNG-Writer, Sidecar-Schema, DNG-Tags,
  Alignment-Parameter; GUI nutzt weiter `run_merge` unverändert.
- **Kein Modellpfad:** Merge (HDR/Pano) ist modellfrei; „laut scheitern bei
  fehlendem Modell" aus Agents.todo gilt nicht für diese beiden Commands (nur
  für matrix/denoise/generative).
- **HDR-Tonemapping als Regler ist keine Merge-Stufe** (LRPAR-G13-MERGE-15
  §Ziel und Abgrenzung).
- **Kein `expected_route`-Anfassen** (gilt für matrix, hier irrelevant, aber
  analog: Merge-Sidecar wird nie für self-validation modifiziert).
- **Kein stiller No-Op:** leere/short Eingaben → lauter Fehler — bei < 2
  Quellen `merge failed:` (`crates/lumina-merge/src/bundle.rs` → `validate_source_count`), bei
  Geometrie/Decode/Belichtung `merge unsupported:`; kein
  „Einzelbild als Merge tarnt sich".
- **Kein GUI-Slice:** die GUI-Aktionen sind bereits parity-abgedeckt
  (`crates/lumina-cli/tests/merge_parity.rs` → Modul-Doc); hier zählt nur CLI ↔ MCP.

## 12. Roadmap-Verweis auf Matrix

`matrix` bleibt ein eigener Sub-Slice von MCP-PARITY-C
(`Agents.todo.md` → Eintrag `MCP-PARITY-C` nimmt alle drei Pipelines in einen Task; dieses SOLL
deckt nur `merge-hdr`/`merge-pano`). Matrix bringt `expected_route` mit
(`testdata/matrix/recipe-set.v1.json`) und ein eigenes Gate; eine gemeinsame
Ableitung ist erst nach diesem Slice zu prüfen.

## 13. Offene Fragen an Eigentümer

Eigene Annahmen/Caveats des Verfassers — keine davon ist im Code belegt, alle
sind als TODO/Placeholder markiert:

1. **Ziel-Crate für den geteilten Adapter** (Impl-A, §4.3): `lumina-merge`
   (näher an `run_merge`, braucht dann `clap`-Freiheit nur im Adapter) oder
   `lumina-stages` (näher an den anderen Frontend-Adaptern)? Beide Crates sind
   heute frei von einer Abhängigkeit auf die jeweils andere
   (`crates/lumina-stages/Cargo.toml` → `[dependencies]`, `crates/lumina-merge/Cargo.toml` → `[dependencies]`); die
   Entscheidung ändert den Dependency-Graph.
2. **Zusatzfeld `mode` im MCP-Payload** (§5): `MergeRunOutcome` hat es
   (`crates/lumina-merge/src/bundle.rs` → `MergeRunOutcome::mode`), das CLI-JSON gibt es nicht aus. Eins-zu-eins oder
   additive Abweichung? (Additiv würde CLI und MCP textuell unterscheiden.)
3. **Real-RAW-Fixture** (§7, T21): die committeten CR3s sind keine Reihe.
   Woher kommt die Belichtungsreihe, und wer dokumentiert Provenienz/Lizenz
   nach dem Muster von `sample-data/raw/README.md` §Provenienz-Block?
4. **Obergrenze `max_shift_px`** (§2): im Code gibt es nur `>= 0`
   (`crates/lumina-merge/src/bundle.rs` → `run_merge`, `crates/lumina-merge/src/align.rs` → `estimate_hdr_translation`/`estimate_pano_transform`), kein dokumentiertes Maximum.
   Schema ohne `maximum` (Vertrag folgt dem Code) oder bewusstes Limit?
5. **Form der Residual-Warnung im MCP-Payload** (§3, §6 Invariante 9): Feldname `warning`
   vs. `warnings[]`, und genau welcher Text (CLI-Zeile
   `crates/lumina-cli/src/merge.rs` → `run_merge`, die Residual-Warnzeile vs. gekürzt)? Ein `warnings[]`-Array wäre die einzige
   Stelle, an der MCP ein Feld mehr hat als die CLI.
6. **Erfolgstext im `content[0].text`** (§5): `tool_result_response`
   serialisiert automatisch das Payload; soll der CLI-Satz `merged <path>` /
   `merge already current: <path>` (`crates/lumina-cli/src/merge.rs` → `run_merge`, Erfolgstext) ergänzend erscheinen?
7. **Neue `McpError`-Varianten vs. Recycling** (§3): vier eigene Varianten
   (klare Namen, neue -32011..-32013) oder bestehende mappen
   (`Sidecar`/`Decode`/`Internal`) mit Prefix im Text? Das ist die einzige
   Stelle, an der dieser Slice die öffentliche Fehler-API von `lumina-mcp`
   erweitert.
8. **Paralleler Dispatch ohne `--json`**: der CLI erzwingt `--json` nicht
   (Default ist der menschenlesbare Satz). Parität ist damit nur über
   `--json` definiert; bestätigen, dass MCP-seitig `structuredContent` immer
   gefüllt ist (entspricht `--json`) — das ist die Annahme in §5.

