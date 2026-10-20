# SOLL — MCP-PARITY-C (HDR + Panorama, Release 2.0)

**Scope (eingegrenzt seit User-Order 2026-09-26):** Byte- und Verhaltens-Parität
zwischen `lumina merge-hdr` / `lumina merge-pano` (CLI) und zwei neuen
MCP-Tools, über dieselbe Eingabemenge, denselben Merge-Digest und denselben
DNG-Checksum-Anchor. Matrix bleibt eigener Sub-Slice (Abgrenzung, §12).

**Belegbasis (alles unten ist gelesen, nichts spekulativ):**
`crates/lumina-cli/src/merge.rs` (414 Z., vollständig),
`crates/lumina-cli/src/main.rs` (Merge-Varianten :647-651, Dispatch :1560-1561,
`CliError::Message` :1465-1468, `main`/`run` :1526-1534, `emit` :5675-5682),
`crates/lumina-merge/src/bundle.rs` (`run_merge` :357-366, `MergeRunError`
:231-241, `MergeRunOutcome` :278-289, `MergeRunOptions` :213-227,
`MergeExif` :119-196, `validate_source_count` :482-494, `resolve_dng_path`
:598-653, `build_merge_document` :904-966),
`crates/lumina-sidecar/src/merge_recipe.rs` (Grenzen :26-44, `digest` :182-188,
`merge_digest` :247-261, `validate_merge_alignment` :355-379),
`crates/lumina-mcp/src/tools/mod.rs` (vollständig), `error.rs` (vollständig),
`tools/generative.rs`, `tools/dust_removal.rs`, `tools/import.rs`,
`tools/spot.rs`, `tools/stage_common.rs`, `tools/library_common.rs`,
`util.rs`, `output_guard.rs`, `crates/lumina-mcp/Cargo.toml`,
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
  `crates/lumina-mcp/src/tools/mod.rs:87-111`).
- Beide Tools rufen **dieselbe** Orchestrierung wie CLI + GUI auf:
  `lumina_merge::bundle::run_merge` (CLI-Knoten
  `crates/lumina-cli/src/merge.rs:92-106`). Keine zweite Merge-Mathematik, kein
  zweiter DNG-Writer.
- Resultat: byte-identisches DNG + identischer Merge-Digest für dieselbe
  Eingabemenge über CLI, GUI-headless und MCP
  (`crates/lumina-cli/tests/merge_parity.rs:74-148`).

## 2. `MergeArgs` → MCP JSON-Schema (SOLL-Vertrag)

Quelle der Felder, Typen und Defaults: `crates/lumina-cli/src/merge.rs:45-80`
(`pub struct MergeArgs`, clap). Die MCP-Felder sind snake_case; `--json` entfällt
(§5). Der CLI-Flag `--input` ist `required = true` und ohne `num_args`
wiederholbar (`merge.rs:49-50`) — clap erzwingt damit **nur das Vorhandensein**,
die Spanne 2..=256 kommt allein aus `validate_source_count`
(`bundle.rs:482-494`). Die expliziten Listen sind komma-getrennt
(`merge.rs:58-65`), im MCP-Schema sind sie echte JSON-Arrays.

| CLI-Flag (`MergeArgs`, `merge.rs:45-80`) | MCP-JSON-Feld | Typ | Pflicht | Default (MCP) | Grenzen / Schema-Keywords |
| --- | --- | --- | --- | --- | --- |
| `--input` (wiederholbar) :49-50 | `inputs` | string[] | **ja** | — | `minItems: 2`, `maxItems: 256` (**beschreibend, nicht durchgreifend** — Pkt. 1); absolute Pfade, Merge-Reihenfolge; `inputs[0]` = Referenz |
| `--output` :52-54 | `output` | string \| null | nein | `null`/absent → `<ref-stem>-HDR.dng` / `-Pano.dng` neben `inputs[0]` (`bundle.rs:640-653`) | MUSS auf `.dng` enden (`bundle.rs:608-613`), braucht einen Dateinamen (`:614-622`), darf keiner Quelle gleichen (`:623-636`, Hüll-Verweis `:608-637`) — **Form-Checks sind Merge-Domäne** (Pkt. 3) |
| `--exposure-times` :58-59 | `exposure_times` | number[] | nein | `[]` | Arity: leer **oder** genau `len(inputs)` (`merge.rs:142-156`; Handler, §3 letzte Zeile); Range je Wert endlich, `(0, 86_400]` (`merge_recipe.rs:38`) — **beschreibend**, Durchgriff in `resolve_hdr_exposure` → `merge unsupported:` (`merge.rs:247-252`) |
| `--isos` :61-62 | `isos` | integer[] | nein | `[]` | Arity: leer **oder** genau `len(inputs)`; Range je Wert `1..=10_000_000` (`merge_recipe.rs:40`) — **beschreibend**, Durchgriff `merge.rs:264-268`; **JSON-Integer, kein Float-Cast** |
| `--f-numbers` :64-65 | `f_numbers` | number[] | nein | `[]` | Arity: leer **oder** genau `len(inputs)`; Range je Wert endlich, `(0, 256]` (`merge_recipe.rs:42`) — **beschreibend**, Durchgriff `merge.rs:280-284` |
| `--max-shift-px` :67-68 | `max_shift_px` | integer | nein | `16` | `minimum: 0` **beschreibend**; Durchgriff Library-Guard `bundle.rs:370-375` → `merge failed:` (Referenzklasse §2 Pkt. 4). **Kein** oberes Schema-Limit dokumentiert → offene Frage §13 Punkt 4 |
| `--blend-width-px` :71-72 | `blend_width_px` | integer | nein | `64` | `minimum: 0`, `maximum: 65536` (`merge_recipe.rs:36`) **beschreibend**; Durchgriff `recipe.validate()` (`bundle.rs:427-429`) → `merge failed:` |
| `--force` :75-76 | `force` | boolean | nein | `false` | re-mergt ein stale/dangling Bundle (`merge.rs:73-76`) |
| `--json` :78-79 | — (entfällt) | — | — | — | MCP antwortet **immer** `structuredContent` (§5); `--json` ist reines CLI-Transport-Flag (`emit`, `main.rs:5675-5682`) |

Fußnote zur Range-Spalte (Modus-Asymmetrie, §6 Inv. 4/5): die Range-Durchgriffe
der drei Exposure-Listen gelten nur für **HDR** (`resolve_hdr_exposure`,
`merge.rs:239-300` → `merge unsupported:`). Für **Pano** ist ein expliziter
Außer-Range-Wert kein Fehler, sondern fällt auf EXIF bzw. die neutralen
Defaults zurück (`resolve_pano_exposure`, `merge.rs:302-340`); die MCP-Tools
erben das unverändert.

Serverseitige Pflicht-Prüfungen (nicht nur Schema, sondern Handler-Code —
`reject_unknown_fields`/`stage_common.rs:51-66` ist das verbindliche Muster,
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
   (`bundle.rs:369,482-494`) liefert dafür
   `MergeRunError::Failed` → gemappt `MergeFailed` (§3-Tabelle). Grund ist die
   Fehlerklassen-Gleichheit CLI ≡ MCP (T17): die CLI kennt für 0, 1 oder 257
   Quellen keinen anderen Weg, also darf MCP keinen erfinden. Die Schema-
   Keywords `minItems: 2` / `maxItems: 256` bleiben als **Beschreibung** für
   den Client stehen, greifen aber nicht durch — ein Schema-Verstoß endet
   nicht in `InvalidParams`, sondern in derselben `merge failed:`-Klasse wie
   im CLI.
2. Explizite Listen: `len == 0 || len == inputs.len()` — identische Semantik
   wie `check_explicit_list_lengths` (`merge.rs:142-156`). Das ist die eine
   Gestaltprüfung, die **wirklich** Handler-Sache bleibt: die CLI prüft sie
   vor `run_merge` und meldet `CliError::Message` (exit 1), nicht
   `MergeRunError`; MCP bildet sie auf `InvalidParams` ab (§3-Tabelle,
   letzte Zeile). Fehlerbild laut, kein stilles Truncate/Repeat.
3. `output`: ist ein **optionales** Feld (`merge.rs:52-54`,
   `Option<PathBuf>`) — fehlt es, ist es `null` oder absent, gilt der
   Default-Zielname aus `inputs[0]` (`resolve_dng_path`,
   `bundle.rs:640-653`); das ist **kein** Fehler. Nur ein **vorhandenes,
   nicht-String**-Feld ist ein Typfehler → `InvalidParams`. Die **Form**
   (`.dng`-Endung, `bundle.rs:608-613`), ein fehlender Dateiname
   (`:614-622`) und ein Ziel, das auf eine Quelle zeigt (`Ablehnungszweig
   :631-636`, Prüfschleife :623-630) prüft der Handler **nicht**: das tut
   `resolve_dng_path` (Hüll-Verweis `bundle.rs:608-637`) und liefert dafür
   `MergeRunError::Failed` → gemappt `MergeFailed`. Gleiche Doktrin wie der
   Count (Pkt. 1): jede Eingabe, die im CLI `merge failed:` ergibt, darf im
   MCP nicht `InvalidParams` werden (T17). Ein eigener Handler-Check wäre
   außerdem doppelt — `resolve_dng_path` erreichen beide Frontends über
   denselben Aufruf.
4. `max_shift_px`/`blend_width_px`/`force`: Handler parst strikt auf
   **JSON-Ebene** — `force` muss ein Bool sein, `max_shift_px`/
   `blend_width_px` Integer ohne Nachkommastellen (`optional_bool`/
   `stage_common.rs:208`; `parse_bounded_uint` liefert das Typ-Muster,
   `util.rs:63-83`). Die **Range**-Grenzen (`blend_width_px` ≤ 65536,
   `merge_recipe.rs:36,365-370`) sind wie `minItems`/`maxItems`
   **beschreibend**: sie werden im Schema dokumentiert, aber **nicht**
   handler-seitig erzwungen, sondern von `recipe.validate()`
   (`bundle.rs:427-429`) geprüft. Sonst würde MCP für einen
   Domain-Verstoß `InvalidParams` melden, wo die CLI `merge failed:`
   meldet (Verstoß gegen T17). Out-of-Range wird also nie geclippt
   (Prinzip `stage_common.rs:169-173`: „Out-of-range values are
   deliberately **not** clamped").
   **Referenzklasse `max_shift_px` negativ, explizit:** im CLI gibt es
   nirgends `allow_negative_numbers` oder `allow_hyphen_values`
   (verifiziert: kein Treffer in `crates/lumina-cli/src`) — die
   Space-Form `--max-shift-px -1` fängt clap daher als Usage-Fehler ab
   (Exit 2, `main.rs:1517-1523`). Programmatische Aufrufer (GUI, oder die
   `=`-Form) erreichen dagegen die Library-Guard in `run_merge`
   (`bundle.rs:370-375`, `merge failed:`). MCP ist ein programmatischer
   Aufrufer und folgt deshalb der **Library-Referenz**: `-1` wird
   durchgereicht → `MergeFailed`. Handler-seitig wird nur der JSON-Typ
   geprüft (Integer, nicht `1.5`/`"x"`) → `InvalidParams`, wie in der
   Tool-Argumente-Zeile der §3-Tabelle beschrieben.
5. `additionalProperties: false` im Schema **und** `reject_unknown_fields` im
   Handler (`stage_common.rs:51-66`; Muster `generative.rs:55-67,103`). Das
   ist reine JSON-Shape und verschiebt keine Fehlerklasse.

## 3. CLI-Subcommand-Wiring + stabile Fehlerpräfixe

**Wiring (ungeändert, nur als Vertrag dokumentiert):**
- Varianten: `MergeHdr(merge::MergeArgs)` `main.rs:647`,
  `MergePano(merge::MergeArgs)` `main.rs:651`; Modul-Deklaration `mod merge;`
  `main.rs:125`.
- Dispatch: `Command::MergeHdr(args) => merge::merge_hdr(args)` `main.rs:1560`,
  `Command::MergePano(args) => merge::merge_pano(args)` `main.rs:1561`.
- Einstiege: `merge_hdr`/`merge_pano` `merge.rs:83-90` → gemeinsamer
  `run_merge(mode, args)` `merge.rs:92-106`.
- MCP-Spiegel: `dispatch_tool`-Arm `merge_hdr::NAME => merge_hdr::run(..)`
  (`tools/mod.rs:124-154`, Musterarm `regenerate::NAME` :151) +
  `list_tool_definitions`-Eintrag (`tools/mod.rs:107-111`).

**Fehlerpräfixe (kanonisch):** `missing`/`stale`/`unsupported` sind als
Kanon in `crates/lumina-cli/src/merge.rs:14-19` festgehalten; `failed` steht
**nicht** dort, sondern als vierte Variante in
`crates/lumina-merge/src/bundle.rs:229-241` (`MergeRunError::Failed`, Doc-
Kommentar :239-240). Der gerenderte Präfix-Text kommt einheitlich aus
`bundle.rs:267-271`.

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
  (`bundle.rs:267-271`, `prefix()` :246-253). Der MCP-Adapter mappt
  `MergeRunError` → `McpError` und lässt den Originaltext im `message`-Feld
  unverändert, damit `merge missing:`/`merge stale:`/… clientseitig
  substring-prüfbar bleibt (Muster: `result_payload`/`map_error`,
  `library_common.rs:48-78`; Fehlertext-Trägerschaft `error.rs:89-156`).
- **Kein `debug!`- oder stiller No-Op-Pfad** (Prinzip §11 „Kein stiller
  No-Op"): `dispatch_tool`'s
  Fall-through ist `MethodNotFound` (`tools/mod.rs:152`); ein Merge-Fehler ist
  immer `isError: true` mit `structuredContent = error.data()`
  (`error.rs:219-226`).
- **Quellzahl ist Merge-Domäne, nicht Tool-Validierung:** 0, 1 oder 257
  Quellen landen über `validate_source_count` (`bundle.rs:482-494`, Aufruf
  :369) in `MergeRunError::Failed` und damit in der `MergeFailed`-Zeile —
  **nicht** in der `InvalidParams`-Zeile. Sonst wäre ein und dieselbe Eingabe
  im CLI `merge failed:` und im MCP `InvalidParams` (Verstoß gegen T17).
- **`output`-Form ist ebenfalls Merge-Domäne:** `.dng`-Endung
  (`bundle.rs:608-613`), fehlender Dateiname (`:614-622`) und Ziel == Quelle
  (Prüfschleife :623-630, Ablehnungszweig :631-636) laufen über
  `resolve_dng_path` → `MergeRunError::Failed` → `MergeFailed`. Der Handler
  prüft an `output` nur, ob ein vorhandenes Feld ein String ist — ein
  fehlendes/`null`/absentes Feld ist der gültige Default (§2 Pkt. 3).
- **Residual-Warnung** (`merge.rs:108-116`): `warning: {command} aligned with
  residual {:.2}px …` muss als MCP-Feld `warning` (string, nur gesetzt wenn
  `residual_warning`) auftauchen — **nicht** als separates `warnings[]`-Array,
  weil das CLI-JSON (§5) kein solches Feld hat und 1:1-Parität der Felder
  gefordert ist. TODO/Placeholder: Feldname + genaue Textform ist mit dem
  Eigentümer zu bestätigen (§13 Punkt 5); die CLI-Zeile ist wörtlich aus
  `merge.rs:110-113` übernehmbar.

## 4. MCP-Dispatch-/Toolmaking-Muster (aus zwei Referenz-Handlern)

**Der Tool-Vertrag:** `tools/mod.rs:1-6` legt je Submodul drei Exporte fest —
`NAME` (&str), `schema() -> Value` und `run(server, args) -> Result<Value,
McpError>`. `DESCRIPTION` (&str) ist im Modul-Doc nicht genannt, aber in jedem
Handler gesetzt und wird von `tool_def` übernommen (z. B.
`generative.rs:43-53`, `dust_removal.rs:24-29`). Registrierung erfolgt an
**vier** Stellen: `pub mod` (`tools/mod.rs:8-36`), `list_tool_definitions`
(:43-113), `dispatch_tool` (:124-154) und — testseitig — `TOOL_NAMES`
(`tool_names/mod.rs:22-54`) plus `EXPECTED_TOOLS`
(`lumina-cli/src/tests/mcp_onnx.rs:64-96`, zweiter Pin). `is_known_tool`
leitet sich aus `list_tool_definitions` ab (:160-164), ein separater Eintrag
ist nicht nötig.

### 4.1 Referenz-Handler A — `lumina_dust_removal` (path-basiert, mit Fixture-I/O)

Aufbau (`crates/lumina-mcp/src/tools/dust_removal.rs`), in genau dieser Reihenfolge:

1. `run(_server, args)` Signatur (:101); `_server` bleibt ungenutzt — der
   Server ist hier reiner Lebensraum, kein Session-State. **Für
   `merge_hdr`/`merge_pano` identisch übernehmbar.**
2. **Argument-Parsing mit harter Typ-Prüfung, nicht Schema-Vertrauen**
   (:102-125): `get_str(args, "input")` (`util.rs:49-53`), danach
   `Path::exists()`-Vorbedingung (:104-106) und — typisch für Artefakt-Schreiber
   — `output_guard::reject_protected_target` (:114-116, `output_guard.rs:26-73`)
   **vor** jeder Mutation. Der Merge-Adapter hat keinen `render_out` und
   übernimmt die Ordnung „erst prüfen, dann mutieren" auf Feldebene: `output`
   wird auf Typ-Ebene geprüft (§2 Pkt. 3), die **Form**prüfung bleibt bei
   `resolve_dng_path`. **Zeitpunkt korrekt benannt:** `run_merge` dekodiert
   zuerst (`bundle.rs:378-390`), resolved danach die Exposure
   (`:392-395`), merged (`align_and_merge`, `:398-404`) und ruft
   `resolve_dng_path` erst danach (`:411`). Der `output`-Formfehler kommt
   also **nach** Decode und Alignment — und genau dort tritt er in der CLI
   ebenfalls auf, weil die CLI über denselben `run_merge`-Pfad läuft.
   Fehlerklasse **und** Zeitpunkt sind damit CLI-identisch (T17, T10). Ein
   Handler-Vorab-Check auf die `.dng`-Endung würde lediglich Zeit sparen,
   die Fehlerklasse aber künstlich auf `InvalidParams` ziehen — er
   unterbleibt deshalb.
3. **Nested Objects per `serde_json::from_value`** (:120-125) mit
   `McpError::InvalidParams`-Abbildung — Muster für alle Listen-Args des
   Merge-Tools: ein kleines `struct` mit `#[derive(Deserialize)]` statt
   handgeschachteltem `.get()`/`.as_array()`/`.as_f64()`.
4. **Decode über den geteilten Decoder** (:118 `read_and_decode`, `util.rs:187-206`)
   → Fehlerklasse `McpError::Decode` (:133-134).
5. **Fehler-Mapping** durchgängig domänenspezifisch: `McpError::FileNotFound`
   (:105), `InvalidParams` (:138-145), `Sidecar` (:173-176), `Render` (:271-274),
   `UnknownCopy` (:183). Kein `McpError::Internal` für erwartbare Fälle.
6. **Erfolgspayload als flaches `json!`-Objekt** (:252-259), direkt aus den
   Domain-Werten — genau die Form, in der `merge_hdr` die
   `MergeRunOutcome`-Felder zurückgibt (§5).

### 4.2 Referenz-Handler B — `lumina_generative` (op-Vokabular + Report-Envelope)

Aufbau (`crates/lumina-mcp/src/tools/generative.rs`):

1. `ALLOWED`-Liste (:55-67) + `OPS`-Liste (:69), im Schema als `enum`
   abgebildet (:71-100, `"enum": OPS` :79).
2. `reject_unknown_fields(args, NAME, ALLOWED)?` als **erste** Zeile von `run`
   (:103) → `InvalidParams` bei unbekanntem Feld (`stage_common.rs:51-66`).
3. Pflichtfeld scharf gelesen (:105-116): fehlt → `InvalidParams("missing `op`")`,
   leerer String → eigener Fehler. **Übertragung auf Merge:** `inputs` fehlt
   oder ist kein Array → `InvalidParams("missing `inputs`")`. Ein **leeres
   Array** dagegen ist ein gültiges `string[]` und fällt der Parity wegen
   (T17) an `validate_source_count` → `merge failed:` (§2 Pkt. 1).
4. `reject_unknown_op(NAME, &op, OPS)?` (:117, `stage_common.rs:69-77`).
5. Optionale Felder über `optional_bool`/`optional_str`/`optional_u64`
   (:119-121, `stage_common.rs:139-208`); `null` == abwesend.
6. Gemeinsamer Domain-Aufruf + `map_error` (:155-156): aus
   `lumina_stages::generative::run(..., PATH_PERSIST)` (`library_common.rs:38`)
   wird über `map_error` → `map_stage_error` (`library_common.rs:76-78`) ein
   `McpError`. **Merge übernimmt diese Zeile 1:1**: `run_merge(...)` →
   `.map_err(|e| map_merge_run_error(NAME, e))` (neuer Mapper, §3-Tabelle).
7. `result_payload(outcome.report)` (:157, `library_common.rs:48-71`) — fügt
   `status`/`saved`/`action`/`text` zum BulkReport hinzu.
   **Merge nutzt `result_payload` NICHT**: es erwartet einen `BulkReport` und
   würde `saved: false` + `action: ""` erfinden, die das CLI-JSON nicht kennt.
   Stattdessen: `Ok(json!({...MergeRunOutcome-Felder...}))` wie `import.rs:77-83`.

### 4.3 Direkte Konsequenzen für `merge_hdr` / `merge_pano`

- **Ein gemeinsamer Kern, zwei Namen.** Die beiden Tools unterscheiden sich nur
  im `MergeMode` (`Hdr`/`Panorama`) — `merge_hdr`/`merge_pano` sind heute
  Zwei-Zeiler auf `run_merge(mode, &args)` (`merge.rs:83-90`). Das MCP-Paar
  bekommt deshalb gemeinsam: `fn run_mode(mode: MergeMode, args: &Value)`
  (Parsing + Validierung + `run_merge`), und `merge_hdr::run`/`merge_pano::run`
  sind je ein Einzeiler darauf. Damit ist die GUI-Invariante „eine
  Implementierung" (§4.1) strukturell erzwungen, nicht nur konventionell.
- **Shared Decode+Exposure-Adapter** (Roadmap-Punkt 2 „Impl-A", §10): der Adapter aus
  `merge.rs:158-340` (`decode_source`, `merge_exif_from_raw`, `resolve_exposure`,
  `explicit_at`, `resolve_hdr_exposure`, `resolve_pano_exposure`) wandert als
  **reine, clap-freie** Funktionen-Schnittstelle nach `lumina-merge` (oder
  `lumina-stages`); CLI und MCP rufen beide diesen Adapter. TODO/Placeholder:
  Ziel-Crate + Modulname ist offen (§13 Punkt 1); fest steht nur, dass er **keine**
  zweite Kopie wird (F6-Dedup, `merge.rs:1-12`).
- **`decoder`/`decode_version`/`orientation`** müssen aus **denselben** Quellen
  kommen wie im CLI (`merge.rs:172-180`): `"libraw"` wenn `raw.is_some()` sonst
  `"image"`; `libraw_decode_version()` bzw. `CARGO_PKG_VERSION`;
  `orientation` aus RAW-Metadaten (`merge_exif_from_raw` :201-211). Das ist die
  einzige Stelle, an der ein MCP-eigener Decode-Pfad den Digest driften lässt.
- **Kein `output_guard` nötig, `resolve_dng_path`-Parität genügt:** die
  `.dng`-Endung und der Quellen-Treffer werden von `resolve_dng_path` geprüft
  (`bundle.rs:608-637`) — für beide Frontends über denselben Aufruf
  (`bundle.rs:411`), also ohne MCP-eigene Kopie. Das deckt die Schreibziele
  vollständig ab; `reject_protected_target` (`output_guard.rs:26-73`)
  adressiert Sidecar-/Zdata-Bundles, die beim Merge nie Ziel sind.

## 5. JSON-Rückgabe-Felder 1:1 (CLI `--json` ≡ MCP `structuredContent`)

Quelle: `emit(args.json, serde_json::json!({...}), &message)` in
`crates/lumina-cli/src/merge.rs:123-135`. `emit` druckt exakt dieses Objekt
(`main.rs:5675-5682`); `tool_result_response` legt ein MCP-Payload-Objekt
unverändert in `structuredContent` (`error.rs:204-212`).

| Feld | Typ | Herkunft (1:1) | Anmerkung |
| --- | --- | --- | --- |
| `command` | string | `merge_command_name(mode)` (:93) = `"merge-hdr"` / `"merge-pano"` (`bundle.rs:84-89`) | Werkzeug-unabhängig, nicht der MCP-Tool-Name |
| `output` | string | `outcome.dng_path` :127 (als `PathBuf` direkt an `json!` übergeben, serialisiert als String) | absoluter DNG-Pfad |
| `sidecar` | string | `outcome.sidecar_path` :128 (dito) | `<dng>.lumina.json` (`sidecar_path_for`, `bundle.rs:412`) |
| `digest` | string | `outcome.digest` :129 | Merge-Digest, `blake3:<64 hex>` (`merge_recipe.rs:247-261`) |
| `checksum` | string | `outcome.checksum` :130 | BLAKE3 der DNG-Bytes (`merge_checksum`, `bundle.rs:93-95,465`) |
| `cached` | boolean | `outcome.cached` :131 | `true` = Bundle war schon aktuell, **nichts** neu geschrieben (`bundle.rs:275-277`) |
| `status` | string | Konstante `"ok"` :132 | nur im Erfolgsfall |

Verbindliche Zusatzfelder (begründete Abweichung, kein Beliebiges):
- `warning` — string, nur gesetzt wenn `outcome.residual_warning`
  (`merge.rs:108-116`); sonst Feld fehlt.
- **Keine** weiteren Felder: kein `saved`, kein `action`, kein `text`, kein
  `warnings[]`, kein `mode` (ja, `outcome.mode` existiert in
  `bundle.rs:279` — aber das CLI-JSON gibt es NICHT aus, also darf MCP es
  nicht hinzufügen; `command` trägt dieselbe Information clientseitig
  substring-bar). TODO/Placeholder: wenn der Eigentümer `mode` zusätzlich
  wünscht, gilt das für CLI und MCP gleichermaßen (§13 Punkt 2).

Textkanal: `tool_result_response` (:205) serialisiert das Payload zusätzlich als
`content[0].text`. Der CLI-Erfolgstext `merged {path}` / `merge already current:
{path}` (`merge.rs:118-122`) ist **nicht** Teil des JSON und damit kein
Paritätsgegenstand; für Lesbarkeit KANN er als `content[0].text`-Prefix
ergänzt werden — Placeholder, Eigentümer-Entscheid (§13 Punkt 6).

## 6. Invarianten

1. **Eine Implementierung:** MCP nutzt `run_merge` (wie CLI `merge.rs:83-90`),
   kein eigenes Decode/Blend. F6-Dedup bleibt unangetastet.
2. **Decode-Kontext spiegelt die Quelle:** CLI-Adapter setzt `decoder` auf
   `"libraw"` wenn `raw.is_some()` sonst `"image"`, plus `libraw_decode_version()`
   bzw. `CARGO_PKG_VERSION`, `orientation` aus RAW-Metadaten
   (`crates/lumina-cli/src/merge.rs:172-180`). MCP-Adapter muss diese Werte aus
   denselben Routen beziehen, sonst driftet der Digest.
3. **Eingabe-Validierung vor Verarbeitung:** `inputs` ist Pflicht; die
   **Anzahl** prüft allein `validate_source_count` (2..=256,
   `bundle.rs:369,482-494` — MCP-seitig kein eigener Count-Check, §2 Punkt 1);
   explizite Listen (`exposure_times`/`isos`/
   `f_numbers`) sind entweder leer oder genau `len(inputs)`
   (`check_explicit_list_lengths`, `merge.rs:142-156`; Unit
   `merge.rs:374-384`). Abbruch laut, keine Teil-Ergebnisse.
4. **HDR-Exposure nie geraten:** explicit wins, sonst EXIF
   `exposure_time_s`/`iso`/`f_number`, sonst `unsupported`
   (`resolve_hdr_exposure`, `merge.rs:239-300`; Range-Checks
   `merge.rs:247-293`). Units: `merge.rs:386-404`.
5. **Pano-Exposure ist nur Provenienz:** EXIF oder neutrale Defaults
   `PANO_DEFAULT_EXPOSURE_S/_ISO/_F_NUMBER` (0,01 s / 100 / f/8,
   `bundle.rs:78-80`); Pixel-Merge nutzt Exposure nicht
   (`resolve_pano_exposure`, `merge.rs:302-340`; Unit `merge.rs:406-413`).
   Wichtig für MCP: auch ein MCP-Aufruf ohne jede Exposure-Liste darf für Pano
   **nicht** scheitern.
6. **Stale/Missing ist hart:** fehlende Quelle/DNG/Sidecar → `missing`;
   Hash-/Decode-/DNG-Prüfsummen-Abweichung → `stale`; Re-Merge nur mit
   `force` (`merge.rs:14-19`, `bundle.rs:801+`; E2E
   `merge_e2e.rs:350-415`). Persistenz-Invarianten in
   `feature/decisions/LRPAR-G13-MERGE-15.md` §Normative Invarianten.
7. **Originale unverändert; relative Pfade; atomarer Write; kein stiller
   Fallback** (dasselbe SOLL-Dokument §Normative Invarianten, Absolutpfade
   verboten; E2E `merge_e2e.rs:544-578` für Quell-Sidecars, `:580-605` für
   Quellen außerhalb des Bundles).
8. **Rückgabe identisch:** MCP liefert `command/output/sidecar/digest/checksum/
   cached/status` 1:1 wie CLI-JSON (`merge.rs:123-135`, §5).
   Cache-Hit (`cached=true`) schreibt nichts neu, exit-equivalent 0
   (`bundle.rs:275-277`, E2E `merge_e2e.rs:323-348`).
9. **Residual-Warnung erhalten:** `residual_warning` wird als `warning: ...` auf
   stderr gemeldet, nicht unterdrückt (`merge.rs:108-116`, E2E
   `merge_e2e.rs:697-723`).
10. **Merge-seitige Ablehnungen bleiben Merge-seitig:** `blend_width_px >
    65536` und `residual_px`-Grenzen laufen über `recipe.validate()` →
    `MergeRunError::Failed` (`bundle.rs:427-429`, Regelwerk
    `merge_recipe.rs:355-379`) und werden `merge failed:`; ein `DngError`,
    das kein `Unsupported` ist, ebenfalls (`map_dng_error`,
    `bundle.rs:588-593`). `merge unsupported:` bleibt reserviert für die
    Merge-Domäne (Alignment/Geometrie/Belichtung, `map_merge_error`
    `bundle.rs:579-585`, `align.rs:233`) und für Writer-Grenzen
    (`DngError::Unsupported`, `bundle.rs:588-593`). Der MCP-Handler darf
    diese nicht in `InvalidParams` umdeuten — sonst unterscheiden sich CLI
    und MCP in der Fehlerklasse für dieselbe Eingabe (T17).

## 7. Fixtures / Goldens

- **Hash-Parity-Anker (synthetisch):** zwei 64×48 Graustufen-PNGs im Gradienten
  `level + x % 16` (`crates/lumina-cli/tests/merge_parity.rs:34-49`) mit
  expliziten Exposure/ISO/f-number-Werten; byte-stabiler Digest + DNG-Checksum
  (`merge_parity.rs:74-148`).
- **MCP-Parity-Test (neu, SOLL):** derselbe Anker aus MCP-Seite aufrufen
  (MCP-Adapter-Decode + `run_merge`) und `digest`/`checksum` mit CLI-Run
  vergleichen — Spiegel von `merge_parity.rs`, nur Transport differs.
- **Fehlerpfad-Fixtures, getrennt nach Fehlerklasse** (weil die Klasse selbst
  Teil des Vertrags ist, §3):
  - **→ `merge failed:`** — 0, 1 oder 257 Quellen. Code:
    `validate_source_count` (`bundle.rs:482-494`, Aufruf :369). Ein
    merge-eigener **Unit**-Test dafür existiert **nicht** (in
    `merge.rs:374-413` stehen nur die vier unten genannten); der Nachweis
    läuft über die E2E-Spiegel (`merge_e2e.rs:459-475` prüft „at least 2" für
    1 Quelle).
  - **→ `merge unsupported:`** — HDR ohne EXIF/Explicit-Belichtung
    (`resolve_hdr_exposure`, `merge.rs:239-300`; Unit
    `merge.rs:386-392`), Out-of-Range explizite Werte (`merge.rs:247-293`;
    Unit `merge.rs:394-404`), Dimension-Mismatch und zu kleine Frames
    (`merge_e2e.rs:477-527`). Pano ohne Exposure ist dagegen **kein**
    Fehlerfall, sondern die dokumentierten neutralen Defaults (Unit
    `merge.rs:406-413`).
  - **→ CLI-seitiger `InvalidParams`-Äquivalent (MCP: `InvalidParams`)** —
    Listen-Arity: eine explizite Liste mit 0 < len ≠ `inputs.len()`
    (`check_explicit_list_lengths`, `merge.rs:142-156`; Unit
    `merge.rs:374-384`). Die CLI meldet das als `CliError::Message` (exit 1),
    nicht als `MergeRunError`.
- **Real-RAW-Gate (Abnahme-Gate, Agents.todo.md MCP-PARITY-C):** mindestens
  eine echte Kamera-RAW-Belichtungsreihe (≥3 Frames) und eine Panorama-Reihe;
  PNG-Mocks decken I/O/Decoder nicht ab. Goldens als BLAKE3-Digest + DNG-Checksum.
  **Inventar-Lücke (neu, belegt):** die zwei committeten R1-CR3s
  (`sample-data/raw/aircraft-landscape.cr3`, `-portrait.cr3`,
  `golden-fixtures.md:44`, `sample-data/raw/README.md`) sind **keine**
  Belichtungsreihe und **keine** Panorama-Reihe — verschiedene Motive, 1/1000 s
  / ISO 1000 bzw. 1250. Für das Gate braucht es also eine **neue** Fixture-Serie
  (TODO/Placeholder, §13 Punkt 3); bis dahin deckt das synthetische Paar die
  Byte-Parität ab und das Gate bleibt offen.
- **Toleranzen:** keine für synthetische/quantisierte Pfade (deterministisch).
  Float-Toleranzen falls alignment-subpixel variieren — der normative Ort ist
  LRPAR-G13-MERGE-15 §Normative Invarianten (:88-116,
  Deklarativ/versioniert/reproduzierbar :99-103), **nicht** §Offene Risiken
  (:477-499).

## 8. Testmatrix (Phase 1–3)

Notation: **[U]** Unit (in-crate, `#[cfg(test)]`), **[P]** Parity (CLI-Run vs.
MCP-Run, gleiche Eingabemenge), **[N]** Negativ/Fehlerpfad, **[G]** Gate
(hardware/RAW oder manuell). „Fixture" meint immer das kleinste Objekt, das die
Aussage trägt; PNG-Mocks sind erlaubt, wo die Aussage transportseitig ist.

### Phase 1 — Registry & Schema (kein Pixel, kein I/O)

| ID | Art | Ort (SOLL) | Prüft | Fixture |
| --- | --- | --- | --- | --- |
| T1 | [U] | neue Testdatei `crates/lumina-mcp/tests/merge_tools.rs` (Name wie §10.1) | `TOOL_NAMES` + `list_tool_definitions` + `is_known_tool` enthalten `lumina_merge_hdr` **und** `lumina_merge_pano`, keine Duplikate, Zählguard — Muster `library_tools.rs:47-91` und `tool_names/mod.rs:60-110` | keine |
| T2 | [U] | `crates/lumina-cli/src/tests/mcp_onnx.rs:26,64-96` | zweiter Pin (`tools.len()`, `EXPECTED_TOOLS`) um beide Namen gewachsen, 26 → **28** | keine |
| T3 | [U] | `crates/lumina-mcp/tests/merge_tools.rs` | `tools/list` liefert **beide** Schemas: `additionalProperties:false`, `required == ["inputs"]`, `enum`/`minimum`-Angaben gem. §2-Tabelle | keine |
| T4 | [U] | `lumina-mcp` Unit in `merge_common`/`merge_hdr` | Schema-JSON der beiden Tools ist bis auf `description`/`enum` gleich → ein Kern, zwei Namen | keine |

### Phase 2 — Validierung & Fehlerklassen (kein Merge-Durchlauf)

| ID | Art | Ort (SOLL) | Prüft | Fixture |
| --- | --- | --- | --- | --- |
| T5 | [U] | `lumina-mcp` Unit (Spiegel `merge.rs:374-384`) | `inputs` fehlt oder ist kein Array → `InvalidParams`; **0, 1 oder 257 Elemente → `MergeFailed`** (über `validate_source_count`, `bundle.rs:482-494` — **nicht** `InvalidParams`, §2 Pkt. 1); Listen-Arity 0/N-Regel sonst Fehler | keine |
| T6 | [U] | `lumina-mcp` Unit | `output` vorhanden, aber kein String → `InvalidParams`; `output` **ohne** `.dng`-Endung, `output == inputs[i]`, `output` leer → `MergeFailed` (über `resolve_dng_path`, `bundle.rs:608-637` — **nicht** `InvalidParams`, §2 Pkt. 3) | 1 PNG (32×24) |
| T7 | [U] | `lumina-mcp` Unit | **JSON-Typfehler** → `InvalidParams`: `isos: [1.5]` (kein Integer), `exposure_times: ["x"]` (keine Zahl), `force` als Nicht-Bool. **Range**-Verstoß `blend_width_px: 65537` → `MergeFailed` über `recipe.validate()` (`bundle.rs:427-429`, `merge_recipe.rs:36,365-370`) — **nicht** `InvalidParams`, §2 Pkt. 4 | keine |
| T8 | [U] | `lumina-mcp` Unit | unbekanntes Feld → `InvalidParams` via `reject_unknown_fields` | keine |
| T9 | [N] | `lumina-mcp` Test | fehlende Quelldatei → `FileNotFound`, `data.error == "FileNotFound"` | Tempdir-Pfad ohne Datei |
| T10 | [N] | `merge_e2e.rs`-Spiegel | 1 Quelle → `merge failed:` (gemappt `MergeFailed`, §3-Tabelle); **nicht** `InvalidParams` — der Count-Check ist Merge-seitig | 1 PNG |

### Phase 3 — Byte-Parität & Artefakte (der Kern)

| ID | Art | Ort (SOLL) | Prüft | Fixture |
| --- | --- | --- | --- | --- |
| T11 | [P] | neu: `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) (Spiegel von `library_parity.rs:41-80` + `merge_parity.rs:74-148`) | gleiche Eingabemenge über CLI-Binary und MCP-Server → **identischer** `digest` **und** `checksum` (String-Gleichheit, `assert_eq!`), `command`/`status` identisch | 2× 64×48 PNG (`merge_parity.rs:34-49`), Exposition explizit |
| T12 | [P] | neu: `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) | DNG-Bytes auf Platte identisch: `blake3(MCP-DNG) == blake3(CLI-DNG)` und `== checksum`-Feld | wie T11, zwei Bäume (Prinzip: CLI-Prozess auf Baum 1, MCP in-process auf byte-identischem Baum 2 — Modul-Doc `crates/lumina-cli/tests/library_parity_common/mod.rs:1-24`) |
| T13 | [P] | neu: `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) | Sidecar-Bytes identisch (`assert_unchanged`-Muster, `library_parity_common/mod.rs:277-284`): `"type": "merge"`, `merge_recipe.mode`, `merge_version 1`, `status ok`, keine Absolutpfade (`merge_e2e.rs:158-199,232-279`) | wie T11 |
| T14 | [P] | neu: `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) | Pano-Parität: gleiche Eingabemenge, gleicher Digest/Checksum, Canvas wächst um den geschätzten Offset (`merge_e2e.rs:286-321`) | 2 PNG, einer um 4 px verschoben |
| T15 | [P] | neu | **Cache-Hit**: zweiter identischer MCP-Aufruf → `cached: true`, `status: "ok"`, DNG-/Sidecar-Bytes unverändert (`assert_unchanged`), kein `saved`/`action`-Feld | wie T11 |
| T16 | [P] | neu | `force`-Kette: Quelle mutieren → MCP meldet `stale` (mapped, `bundle.rs:801+`); mit `force: true` → Erfolg **und** Digest == CLI-Digest nach Force-Merge | wie T11 + mutiertes PNG (`library_parity_common/mod.rs:105-127`) |
| T17 | [P] | neu | Fehlerklassen-Gleichheit: für dieselbe Eingabe liefern CLI-stderr-Präfix und MCP-`data.error` dieselbe Klasse (Tabelle §3); Fehlertext identisch | HDR ohne EXPLIZIT/EXIF, 8×8 zu klein (`merge_e2e.rs:502-527`), Dimension-Mismatch (`:477-500`) |
| T18 | [N] | `merge_e2e.rs`-Spiegel | jede Ablehnung hinterlässt **nichts**: `snapshot` vorher/nachher identisch (`library_parity_common/mod.rs:286-295`) | wie T17 |
| T19 | [U]/[P] | `merge_e2e.rs`-Spiegel | Quell-Sidecars und Quell-Bytes unverändert (`merge_e2e.rs:544-578`); Quelle außerhalb des Bundles → `unsupported` (`:580-605`) | PNGs mit Sidecar |
| T20 | [P] | neu | Envelope-Ablehnung 1:1: Standard-Sidecar am Ziel → laut (`merge_e2e.rs:727-765`); unbekannter `type` → laut (`:769-799`); MCP-Klasse == CLI-Klasse | wie T11 |
| T21 | [G] | `crates/lumina-cli/tests/merge_parity.rs`-Erweiterung ODER neue Datei, `#[ignore]` | Real-RAW-Gate: echte RAW-Belichtungsreihe (≥3) + Panorama-Reihe → identischer Digest/Checksum über CLI und MCP. **Offen solange keine Fixture existiert** (§7, §13 Punkt 3) | **TODO — neue RAW-Fixture** |

### Gates, die ausdrücklich **nicht** behauptet werden

- Real-RAW-I/O (§7, T21): offen, bis Fixture vorhanden.
- GPU/Adapter: nicht berührt, Merge ist CPU/pure-Rust
  (`lumina-merge/Cargo.toml:7-22`); kein GPU-Gate nötig und keines behauptet.
- `cargo`-Läufe sind in der SOLL-Phase verboten (§9); alle Commands stehen als
  Soll-Kommandos in der Verify-Phase.

## 9. Akzeptanz

- Registry: `tool_names::TOOL_NAMES` + `list_tool_definitions` +
  `is_known_tool` um zwei Namen gewachsen, Zählguard
  (`crates/lumina-mcp/tests/tool_names/mod.rs:22-54`) + `mcp_onnx.rs` second pin
  (26 → 28, `:26,64-96`) grün. Kein doppelter Name.
- `tools/list` liefert beide Schemas (`additionalProperties:false`, `inputs`
  required, default-Werte dokumentiert) — MCP-Vertrag aus §2.
- MCP+CLI Produzieren denselben `digest`+`checksum` auf synthetischem Anker
  (SOLL-Test grün) und auf Real-RAW-Gate.
- Fehlerpfade loud: missing/stale/unsupported **und** failed mit denselben
  Präfixen (Kanon `merge.rs:14-19` + `bundle.rs:229-241`), keine
  silent partial, keine `cached`-Fälschung.
- `cargo test -p lumina-mcp` + `cargo test -p lumina-cli --test merge_parity`
  grün; `cargo fmt --all -- --check` grün. (Kein `cargo` während SOLL-Phase.)

### 9.1 Abnahmekriterien für den ersten Code-Slice

„Stubs nicht ausreichend": der erste Slice muss auf einem HDR-Fixture eine
**vollständige, byte-paritäre Antwort** liefern. Konkret, mit dem Anker aus
`crates/lumina-cli/tests/merge_parity.rs:34-49` (zwei 64×48 PNGs, Stufen 40 und
160, `--exposure-times 0.004,0.016 --isos 100,100 --f-numbers 8,8`):

1. `lumina_merge_hdr` antwortet auf `tools/call` mit `isError: false` und einem
   `structuredContent`, das **exakt** die sieben Felder aus §5 trägt
   (`command`, `output`, `sidecar`, `digest`, `checksum`, `cached`, `status`);
   kein `saved`, kein `action`, kein `text`, kein `mode`.
2. `command == "merge-hdr"`, `status == "ok"`, `cached == false`.
3. `digest` ist ein String `blake3:<64 hex>` und **identisch** mit dem Digest,
   den die CLI-Binary für dieselbe Eingabemenge über
   `--json` ausgibt (`merge_e2e.rs:607-631` prüft genau diese Form).
4. `checksum` ist `blake3:<64 hex>`, identisch mit dem CLI-Wert, **und**
   gleich `blake3` der geschriebenen DNG-Datei (`blake3_file`-Muster
   `merge_e2e.rs:152-154`).
5. Das DNG liegt unter `output`, ist **byte-identisch** mit dem CLI-DNG
   (gleicher Baumtyp, T12) und dekodiert über `lumina_raw` auf **64×48** — die
   Fixture-Abmessung des Ankers (`merge_parity.rs:34-49`); dass das DNG
   decodierbar ist und die Geometrie hält, prüft das Re-Import-Muster aus
   `crates/lumina-cli/tests/merge_e2e.rs:281-283` (`lumina_raw::decode_bytes`
   + `assert_eq!((w, h), (W, H))`) — dort mit `W = 32`/`H = 24`
   (`merge_e2e.rs:20-21`), also der eigenen Fixture-Größe jenes Tests
   anzupassen, nicht 64×48.
6. Der Sidecar unter `sidecar` trägt `"type": "merge"`,
   `merge_recipe.mode == "hdr"`, `merge_version == 1`, `status == "ok"`,
   `merge_artifact.checksum == checksum`, nur relative Quellpfade
   (`merge_e2e.rs:232-279`).
7. Der zweite identische Aufruf liefert `cached == true` und ändert **kein**
   Byte an DNG oder Sidecar (`merge_e2e.rs:323-348`).
8. Ein Pano-Fixture-Paar (gleicher Anker, `merge-pano`) erfüllt 1–7 mit
   `command == "merge-pano"` und einem um den Offset verbreiterten Canvas
   (`merge_e2e.rs:286-321`); HDR ohne Exposure scheitert laut als
   `unsupported` (`merge_e2e.rs:444-457`), Pano ohne Exposure gelingt mit den
   neutralen Defaults (`merge.rs:406-413`).
9. Jede Fehleraussage trägt den kanonischen Präfix-Text im `message` und den
   stabilen Namen in `data.error` (§3-Tabelle); `dispatch_tool` kennt beide
   Namen (kein `MethodNotFound`-Durchreichnen, `tools/mod.rs:152`).
10. Kein Tool darf seine Antwort aus einem statischen String zusammenbauen:
    T11–T16 sind genau die Tests, die einen Stub rot machen.

## 10. Roadmap (SOLL → Impl → Verify)

1. **SOLL (dieses Doc):** Review/Sign-off; ggf. Tool-Namen finalisieren.
2. **Impl-A — geteilten Adapter freilegen:** Merge-Decode-Adapter aus
   `merge.rs:158-340` in `lumina-stages` (oder `lumina-merge`-Crate) ziehen,
   damit MCP exakt dieselbe Decode-Logik wie CLI nutzt (keine Kopie).
   Ziel-Crate offen (§13 Punkt 1).
3. **Impl-B — Tool-Module:** `crates/lumina-mcp/src/tools/merge_hdr.rs`,
   `merge_pano.rs` angelegt, analog `generative.rs`; Register in
   `tools/mod.rs` (list + dispatch) und `tool_names/mod.rs` (2 Namen).
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
| `crates/lumina-mcp/tests/merge_tools.rs` (+ `merge_tools_common/mod.rs`) | T1, T3, T4, T5–T10 | ~250 Z. |
| `crates/lumina-mcp/tests/merge_parity.rs` (geplanter Pfad, **nicht** im Repo) | T11–T20 (mit Fixture-Bäumen wie `library_parity_common/mod.rs`) | ~300 Z., droht >500 → dann aufteilen in `merge_parity.rs` + `merge_parity_errors.rs` (Muster `library_parity*.rs`) |

**Zu modifizierende Dateien (minimal-invasiv):**

| Datei | Änderung |
| --- | --- |
| `crates/lumina-mcp/src/tools/mod.rs` | +2 `pub mod`, +2 `tool_def(...)` in `list_tool_definitions` (:43-113), +2 Arme in `dispatch_tool` (:124-154). Aktuell 164 Z. → bleibt unter 500 |
| `crates/lumina-mcp/src/error.rs` | optional neue Varianten `MergeStale`/`MergeUnsupported`/`MergeFailed` + `code()`/`name()`/`data()` (:46-156). Aktuell 231 Z. |
| `crates/lumina-mcp/Cargo.toml` | +`lumina-merge = { path = "../lumina-merge" }` (Fehlt aktuell: :9-29). Prüfen: kein Zyklus (`lumina-merge` hängt nur an core/sidecar/tiff, `Cargo.toml:7-22`) |
| `crates/lumina-mcp/tests/tool_names/mod.rs` | +2 Namen in `TOOL_NAMES` (:22-54) |
| `crates/lumina-cli/src/tests/mcp_onnx.rs` | +2 Namen in `EXPECTED_TOOLS` (:64-96), `:26` 26 → 28 |

**NICHT anfassen:**

- `crates/lumina-cli/src/merge.rs` — CLI bleibt Referenz; nur der Adapter wird
  in Impl-A herausgezogen (dort dann ein dünner Re-Export).
- `crates/lumina-cli/src/main.rs` — **baseline-gesperrt** (7874 Z.,
  `scripts/file_size_baseline.txt:1`); jede Zeile Wachstum bricht
  `scripts/check_file_sizes.sh`. Registry-Änderungen für MCP gehören nach
  `lumina-mcp`, nicht hierher.
- `crates/lumina-cli/tests/merge_e2e.rs` (799 Z., baseline :45) — die
  MCP-Fehlerpfade gehen in die **neue** `merge_parity.rs`, nicht hierhin.
- `crates/lumina-merge/**` — Merge-Mathematik, DNG-Writer, Orchestrierung
  bleiben byte-identisch (nur Adapter-Aufnahme in Impl-A).
- `crates/lumina-sidecar/**` — Schema/DNG-Tags unverändert.
- `crates/lumina-gui/**` — GUI-frei in diesem Slice (`Agents.todo.md:409`);
  `lumina-gui/src/merge_gui.rs:127-197` ist ein zweiter GUI-Adapter und bleibt
  unberührt (seine Vertragstreue ist der parity test
  `merge_parity.rs:53-72`).
- `crates/lumina-cli/tests/merge_parity.rs` — bestehender CLI↔GUI-Anker
  bleibt; der MCP-Spiegel ist eine eigene Datei.
- `scripts/file_size_baseline.txt` — kein neuer Eintrag ohne Not.

**Rollback-Kriterium (ein Satz, vorab festgelegt):** schlägt einer der Schritte
T11–T16 (Byte-Parität, Cache-Hit, Force-Kette) fehl und lässt sich **nicht**
durch Adapter-Anpassung allein reparieren — d.h. der Fix würde
`lumina-merge`- oder `lumina-cli/src/merge.rs`-Verhalten ändern — wird der
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
  Quellen `merge failed:` (`validate_source_count`, `bundle.rs:482-494`), bei
  Geometrie/Decode/Belichtung `merge unsupported:`; kein
  „Einzelbild als Merge tarnt sich".
- **Kein GUI-Slice:** die GUI-Aktionen sind bereits parity-abgedeckt
  (`merge_parity.rs:1-16`); hier zählt nur CLI ↔ MCP.

## 12. Roadmap-Verweis auf Matrix

`matrix` bleibt ein eigener Sub-Slice von MCP-PARITY-C
(`Agents.todo.md:409` nimmt alle drei Pipelines in einen Task; dieses SOLL
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
   (`lumina-stages/Cargo.toml:26-27`, `lumina-merge/Cargo.toml:7-22`); die
   Entscheidung ändert den Dependency-Graph.
2. **Zusatzfeld `mode` im MCP-Payload** (§5): `MergeRunOutcome` hat es
   (`bundle.rs:279`), das CLI-JSON gibt es nicht aus. Eins-zu-eins oder
   additive Abweichung? (Additiv würde CLI und MCP textuell unterscheiden.)
3. **Real-RAW-Fixture** (§7, T21): die committeten CR3s sind keine Reihe.
   Woher kommt die Belichtungsreihe, und wer dokumentiert Provenienz/Lizenz
   nach dem Muster von `sample-data/raw/README.md` §Provenienz-Block?
4. **Obergrenze `max_shift_px`** (§2): im Code gibt es nur `>= 0`
   (`bundle.rs:370-375`, `align.rs:90,210`), kein dokumentiertes Maximum.
   Schema ohne `maximum` (Vertrag folgt dem Code) oder bewusstes Limit?
5. **Form der Residual-Warnung im MCP-Payload** (§3, §6 Invariante 9): Feldname `warning`
   vs. `warnings[]`, und genau welcher Text (CLI-Zeile
   `merge.rs:110-113` vs. gekürzt)? Ein `warnings[]`-Array wäre die einzige
   Stelle, an der MCP ein Feld mehr hat als die CLI.
6. **Erfolgstext im `content[0].text`** (§5): `tool_result_response`
   serialisiert automatisch das Payload; soll der CLI-Satz `merged <path>` /
   `merge already current: <path>` (`merge.rs:118-122`) ergänzend erscheinen?
7. **Neue `McpError`-Varianten vs. Recycling** (§3): vier eigene Varianten
   (klare Namen, neue -32011..-32013) oder bestehende mappen
   (`Sidecar`/`Decode`/`Internal`) mit Prefix im Text? Das ist die einzige
   Stelle, an der dieser Slice die öffentliche Fehler-API von `lumina-mcp`
   erweitert.
8. **Paralleler Dispatch ohne `--json`**: der CLI erzwingt `--json` nicht
   (Default ist der menschenlesbare Satz). Parität ist damit nur über
   `--json` definiert; bestätigen, dass MCP-seitig `structuredContent` immer
   gefüllt ist (entspricht `--json`) — das ist die Annahme in §5.

