# SOLL — MCP-PARITY-C: `lumina matrix` → `lumina_matrix` (Matrix-Sub-Slice)

**Status:** SOLL/Plan, 2026-10-14. Kein Code geändert, kein Commit. Branch `main`,
HEAD `ddebe7f`; `git status --porcelain` war beim Schreiben **leer** (außer diesem
Dokument).

**Scope (eingegrenzt seit User-Order 2026-09-26, `Agents.todo.md:409`):** Byte- und
Verhaltens-Parität zwischen `lumina matrix` (CLI, `crates/lumina-cli/src/matrix.rs`)
und einem **einen** neuen MCP-Tool `lumina_matrix`. Der Merge-Teil
(`lumina_merge_hdr`/`lumina_merge_pano`) ist geliefert und verifiziert
(`feature/quality/hdr-pano-raw-dng-status-2026-10-14.md`); der Matrix-Sub-Slice ist
der **Rest** der Lücke (`feature/quality/mcp-parity-c-soll.md` §12). Dieses Doc
spiegelt die Struktur jenes SOLL — es erfindet nichts Neues.

**Belegbasis (alles unten ist gelesen, nichts spekulativ):**
`crates/lumina-cli/src/matrix.rs` (1007 Z., vollständig),
`crates/lumina-cli/src/main.rs` (Modul :126-130, `Matrix`-Variante :655-656,
Dispatch :1562, `RenderRoute` :280-329, `render_standard_routed` :497-538,
`render_best_effort` :428-482, `CliError` :1466-1523, `emit` :5675-5682,
`StagedArtifact` :6647-6700),
`testdata/matrix/recipe-set.v1.json` (235 Z., vollständig) und
`testdata/matrix/golden/` (22 committete PNG-Goldens, 7,0 MB),
`crates/lumina-cli/tests/matrix_e2e.rs` (451 Z.), `crates/lumina-cli/tests/matrix_paths.rs`
(151 Z.), `crates/lumina-gui/src/matrix.rs` (507 Z., GUI-Slice 2),
`feature/quality/conflicts-and-acceptance.md` § „Rezept-Matrix" (:125-361),
`.github/workflows/matrix-nightly.yml` (126 Z.),
`crates/lumina-mcp/src/tools/merge_common.rs` (300 Z.), `merge_hdr.rs` (42 Z.),
`src/error.rs` (266 Z.), `tools/mod.rs` (179 Z.), `tests/tool_names/mod.rs` (115 Z.),
`crates/lumina-mcp/tests/merge_tools.rs` + `merge_tools_validation.rs`,
`crates/lumina-cli/tests/merge_mcp_parity.rs` + `_errors.rs` + `_envelope.rs`
+ `_gate.rs` + `merge_mcp_parity_common/{mod,fixtures,graders,job}.rs`,
`crates/lumina-cli/src/tests/mcp_onnx.rs` (:1-96),
`crates/lumina-cli/src/stages.rs` (:440-479, Decode-/Error-Shims),
`crates/lumina-cli/src/library.rs` (:254-299, Render-Port-Muster),
`crates/lumina-stages/src/regenerate.rs` (:107-137, Render-Port-Trait,
:139-153, Request-Struktur), `crates/lumina-stages/src/decode.rs` (:36-50),
`crates/lumina-stages/src/pipeline.rs` (:29-43),
`crates/lumina-mcp/src/util.rs` (:340-463, MCP-eigener Render-Pfad),
`crates/lumina-mcp/Cargo.toml` (:1-49), `scripts/check_file_sizes.sh`,
`scripts/file_size_baseline.txt`, `Agents.todo.md` :409, `Agents.md` :608-628
(GPU-Regel), `feature/platform/mcp-server.md` (:1-23, :740-795).

## 1. Ziel

- MCP erhält **ein** path-basiertes Tool (ein Aufruf = ein Matrix-Lauf, **kein**
  `image_id`, kein Session-State): `lumina_matrix`. Analogie:
  `lumina_generative`/`lumina_relocate` (MCP-PARITY-B,
  `feature/quality/mcp-parity-c-soll.md:36-39`).
- Das Tool ruft **dieselbe** Runner-Funktion auf wie die CLI
  (`matrix::matrix`, `matrix.rs:237-596`) — über einen gemeinsamen, clap-freien
  Kern in einer gemeinsam linkbaren Crate. Keine zweite Matrix-Mathematik, kein
  zweiter Renderpfad, keine zweite Golden-Schreibung.
- Ergebnis: für dasselbe Rezept-Set (Bytes identisch) sind `--json`-Report, die
  geschriebenen Golden-PNGs, die Full-Resolution-Exporte und die Diff-Bilder
  byte-identisch über CLI und MCP; jede Ablehnung trägt denselben Text.
- **Nicht** Ziel dieses Slices: der GPU-Paritätsbeweis auf echtem Metall
  (`--require-gpu`-Verdikt), ein Real-RAW-Gate-Lauf auf beiden Transporten, und
  jede Änderung an `testdata/matrix/**` (§4, §5, §8).

## 2. `MatrixArgs` → MCP-JSON-Schema (SOLL-Vertrag)

Quelle der Felder, Typen und Defaults: `crates/lumina-cli/src/matrix.rs:65-99`
(`pub struct MatrixArgs`, clap). MCP-Felder sind snake_case; `--json` entfällt
(§3). Der CLI-Flag `--recipe` ist wiederholbar (`matrix.rs:80-81`), im
MCP-Schema ist er ein echtes JSON-Array (`recipes`).

| CLI-Flag (`MatrixArgs`, `matrix.rs:65-99`) | MCP-JSON-Feld | Typ | Pflicht | Default (MCP) | Grenzen / Schema-Keywords |
| --- | --- | --- | --- | --- | --- |
| `--recipe-set` :70-71 | `recipe_set` | string \| null | nein | `null`/absent → env-Kette `LUMINA_MATRIX_RECIPE_SET` → `LUMINA_MATRIX_DIR` → Vorfahren-Walk (`matrix.rs:806-835`, gepinnt in `matrix_paths.rs`) | Beschreibung der Kette im `description`-Text; **kein** Handler-Vorab-Check auf Existenz — die Domain prüft laut |
| `--golden-dir` :73-74 | `golden_dir` | string \| null | nein | `null` → `<recipe-set-dir>/golden` (`matrix.rs:274-277`) | rel. oder abs.; der Runner legt es nur im Baseline-Modus an (:278-280) |
| `--work-dir` :77-78 | `work_dir` | string \| null | nein | `null` → frisches `tempfile::tempdir()`, danach entfernt (`matrix.rs:284-295`) | MCP **muss** dasselbe Tempdir-Verhalten erben (kein Repo-Müll, Kommentar :282-283) |
| `--recipe` (wiederholbar) :80-81 | `recipes` | string[] | nein | `[]` = **alle** Rezepte des Sets, in Set-Reihenfolge | unbekannte Id → lauter Abbruch **vor** jedem Render (`prepare_recipes`, :690-697) |
| `--comparison-width` :84-85 | `comparison_width` | integer \| null | nein | `null` → `set.comparison_width` (committet: 384, `recipe-set.v1.json:4`) | `minimum: 1` **beschreibend**; `0` ist ein lauter Domain-Fehler (`:266-272`), nicht `InvalidParams` |
| `--update-goldens` :87-88 | `update_goldens` | boolean | nein | `false` | Baseline-Modus: CPU-gepinnt, plattformneutral (`:371-375`, F4) |
| `--require-gpu` :95-96 | `require_gpu` | boolean | nein | `false` | GPU-Paritätsgate; in dieser Slice **nur** gespiegelt, nicht neu bewiesen (§5) |
| `--json` :98-99 | — (entfällt) | — | — | — | MCP antwortet **immer** `structuredContent`; `--json` ist reines CLI-Transport-Flag (`emit`, `main.rs:5675-5682`) |

**Serverseitige Pflicht-Prüfungen (nicht nur Schema, sondern Handler-Code —**
`reject_unknown_fields`/`stage_common.rs:51-66` ist das verbindliche Muster, weil
ein Schema für das aufrufende Modell nur eine *Beschreibung* ist).

**Die Trennregel (wie Merge, §2 des Merge-SOLL):** der Handler prüft
ausschließlich **JSON-Shape und -Typ** (Feld vorhanden, String/Array/Integer/Bool
wie zugesagt, `additionalProperties`) — jede Verletzung davon ist
`InvalidParams`. Jede **Matrix-Domänen-Regel** (Set-Validierung, Rezept-Filter,
Sample-Existenz, Toleranz, `comparison_width == 0`, `require_gpu`-Kombinationen)
wird **nicht** handler-seitig durchgesetzt, sondern der Domäne überlassen; der
resultierende Text wird 1:1 übernommen (§6). Ein Handler-Vorab-Check würde die
Fehlerklasse künstlich auf `InvalidParams` ziehen und damit die
Fehlerklassen-Gleichheit CLI ≡ MCP verletzen.

1. `recipes`: Array von Strings. Fehlt das Feld oder ist es `null`, gilt `[]`
   (= alle Rezepte — die Auswahl trifft `prepare_recipes`, :685-723; eine leere
   Treffermenge → **lauter** Fehler „recipe set … selected no recipes",
   :298-303). Ein Nicht-Array oder ein
   Nicht-String-Element ist `InvalidParams`. Die **Existenz** jeder Id prüft allein
   `prepare_recipes` (:690-697).
2. `recipe_set`/`golden_dir`/`work_dir`: optionales String-Feld; fehlt es oder ist
   es `null`, gilt der jeweilige Default. Nur ein **vorhandenes, nicht-String**-Feld
   ist ein Typfehler → `InvalidParams`. Die **Existenz** der Datei prüft
   `load_recipe_set` (:599-603, `io_error`) und bleibt Domain — kein eigener
   `FileNotFound`-Vorab-Check, sonst unterscheidet sich die Klasse vom CLI.
3. `comparison_width`: strikt JSON-Integer (`1.5`/`"384"`/`-1` → `InvalidParams`).
   `0` ist ein gültiges Integer und fällt der Domain-Parität wegen an
   `matrix.rs:266-272` → `Matrix`-Fehlerklasse, **nicht** `InvalidParams`.
   Out-of-range wird nie geclippt (`stage_common.rs:169-173`).
4. `update_goldens`/`require_gpu`: `optional_bool` (`stage_common.rs:208`). Die
   **Kombination** beider Flags prüft die Domain (`matrix.rs:243-248`) — der
   Handler darf sie nicht vorab abfangen, sonst wird im CLI ein
   `CliError::Message` und im MCP ein `InvalidParams`.
5. `additionalProperties: false` im Schema **und** `reject_unknown_fields` im
   Handler. Reine JSON-Shape, verschiebt keine Fehlerklasse.

## 3. Rückgabe-Felder 1:1 (CLI `--json` ≡ MCP `structuredContent`)

Quelle: `report_json` in `crates/lumina-cli/src/matrix.rs:964-1007`, ausgegeben
über `emit(args.json, payload, text)` (:562-576); `emit` druckt exakt dieses
Objekt (`main.rs:5675-5682`). `tool_result_response` legt ein MCP-Payload-Objekt
unverändert in `structuredContent` (`error.rs:239-247`).

| Feld | Typ | Herkunft (1:1) | Anmerkung |
| --- | --- | --- | --- |
| `command` | string | Konstante `"matrix"` :996 | Werkzeug-unabhängig, nicht der MCP-Tool-Name |
| `mode` | string | `"verify"`/`"baseline"` :305-309, :997 | der Modus ist ein **Domänen**-Wert, nicht das Transport-Flag |
| `recipe_set` | string (Pfad) | :998 | absolut, wie der CLI sie auflöste → in der Parität normalisiert |
| `golden_dir` | string (Pfad) | :999 | dito |
| `comparison_width` | integer | :1000 | aufgelöster Wert (Set- oder Override) |
| `pairs_total` | integer | :1001 | `outcomes.len()` (im Vollauf = `samples.len() * prepared.len()`, :311) |
| `passed` | integer | :994, :1002 | Zähler über `PairOutcome::passed` (:228-235) |
| `failed` | integer | :1003 | `pairs_total - passed` |
| `duration_ms` | integer | :557, :1004 | **nicht-deterministisch** — Paritätsmaske (§3.1) |
| `pairs` | array | :972-993 | je ein Objekt pro (Sample × Rezept) |

`pairs[]` je Eintrag (`:975-991`):

| Feld | Typ | Herkunft | Anmerkung |
| --- | --- | --- | --- |
| `sample` / `recipe` | string | :976-977 | die IDs aus dem Set (konsistent zu den Golden-Namen `<sample>__<recipe>.png`, :338-340) |
| `status` | string | :978 | `pass` \| `fail` \| `error` \| `baseline-created` \| `baseline-updated` \| `route-fail` |
| `psnr_db` | string \| null | :979-981 | **String**, nicht Zahl: `"inf"` oder `format!("{value:.4}")` — 1:1 übernehmen, kein Float-Cast |
| `min_psnr_db` | number \| null | :982 | aus `ToleranceClass::min_psnr_db` (:119-126): exact → `null`, strict 45, standard 40, robust 30 |
| `max_abs_diff` | integer \| null | :983 | per-Channel-Maximum ohne Alpha (:904-913) |
| `duration_ms` | integer | :984 | **nicht-deterministisch** — Paritätsmaske |
| `expected_route` | string | :985 | **Austrag des deklarierten Werts** — ausgeliefert, nie geschrieben/ausgewertet (§4) |
| `route` | string \| null | :986 | `"gpu"`/`"cpu"`/`null`; gesetzt sobald ein Render erfolgreich war (:401) — auch im Baseline-Modus (`"cpu"`, :381-384) |
| `route_reasons` | string[] | :987 | laute CPU-Routing-Gründe (:402); leer für GPU und im Baseline-Modus |
| `golden` | string (Pfad) | :988 | → normalisieren |
| `output` | string (Pfad) | :989 | Full-Resolution-Export-PNG → normalisieren |
| `message` | string \| null | :990 | Fehler-/Gate-Text; `null` im grünen Fall |

**Keine** Zusatzfelder im MCP-Payload. Anders als beim Merge (dort `warning`,
Merge-SOLL §5) braucht `matrix` **kein** MCP-eigenes Zusatzfeld: der CLI-Report
trägt bereits Route, Toleranz und Diagnose. Kein `saved`, kein `action`, kein
`text`, kein `warnings[]`.

### 3.1 Die zwei Paritätsmasken (und nur diese)

1. **`duration_ms`** (Top-Level und je Pair). Der CLI-Misst Wanduhrzeiten
   (:310, :337, :551, :556); zwei Läufe können nicht identisch sein. Die Maske
   entfernt genau diesen Schlüssel an beiden Stellen und vergleicht alles andere
   roh.
2. ** absolute Pfade** (`recipe_set`, `golden_dir`, `golden`, `output`). Zwei
   Fixture-Bäume haben verschiedene Adressen; `normalize_paths` ersetzt beide
   Wurzeln durch `<TREE>` (Muster `merge_mcp_parity_common/graders.rs:106-113`).

Alles Übrige — `psnr_db`, `max_abs_diff`, `status`, `message`, `route`,
`route_reasons`, `expected_route` — wird **wortgleich** verglichen. Das ist die
Stelle, an der ein Stub rot wird (§10).

Textkanal: `tool_result_response` serialisiert das Payload zusätzlich als
`content[0].text` (`error.rs:239-247`). Der CLI-Erfolgstext (`report_text`,
:923-962, plus die Summenzeile :571-575) ist **nicht** Teil des JSON und damit
kein Paritätsgegenstand; er kann als `content[0].text` ergänzt werden —
Placeholder, Eigentümer-Entscheid (§13 Punkt 6).

## 4. `expected_route`-Schutz (Semantik-Punkt 1 aus `Agents.todo.md:409`)

Festgeschriebene Semantik, wörtlich: „**Kein Blindschreiben von `expected_route`**
— … dieses Slice darf daran **nichts** ändern, und ein Tool darf `expected_route`
weder setzen noch auswerten, um sich selbst zu validieren." Gepflegt wird das Feld
laut `testdata/matrix/recipe-set.v1.json` und
`feature/quality/conflicts-and-acceptance.md` (:152-156, :240-252).

### 4.1 Die Grenze, code-seitig

Das Feld hat im Runner **genau zwei** Leser und **null** Schreiber:

- **Leser 1 — Austrag (erlaubt):** `outcome.expected_route = entry.spec.expected_route.label()`
  (:352, :187-192) → landet im Report (:985). Ein Echo des Eingabewerts, keine
  Bewertung.
- **Leser 2 — Auswertung (nur unter `--require-gpu`):** `check_expected_route`
  (:841-866) wird **ausschließlich** im `if args.require_gpu`-Zweig aufgerufen
  (:403-411) und vergleicht die Deklaration gegen die Route, die der
  Standard-Renderpfad **tatsächlich** nah (`RenderRoute`, `main.rs:280-329`) —
  nie gegen eine abgeleitete oder geloggte Vermutung (Doc :404-406).
- **Schreiber: keine.** Der Runner schrieb ausschließlich Golden-PNGs
  (`encode_and_stage`, :877-880), Diff-Bilder (:884-901) und Full-Resolution-
  Exporte (:422-433); das Rezept-Set wird nur gelesen (`load_recipe_set`, :600).

Damit ist die Grenze code-seitig scharf:

| Frage | Antwort | Beleg |
| --- | --- | --- |
| Darf das Tool den Wert **ausliefern**? | Ja — er ist Pflichtfeld des CLI-Reports. | :985 |
| Darf das Tool ihn **schreiben**? | Nein. Kein Codepfad schreibt in die Set-Datei; `update_goldens` schreibt nur Goldens in `golden_dir`. | :600, :877-880 |
| Darf das Tool ihn **auswerten**? | Nur im gespiegelten `require_gpu`-Zweig, und nur gegen die tatsächliche Route — nicht zur Selbstvalidierung des Tools. Im Default-Modus (verify) ist das Feld **inert**. | :403-411, :841-866 |

**Der Default-Pfad ist damit der wichtigste Schutz:** `lumina_matrix` läuft
standardmäßig im Verify-Modus, in dem `expected_route` ausschließlich dekodiert,
gelabelt und echo-t wird. Eine Auswertung existiert nur, wenn der Aufrufer
`require_gpu: true` verlangt — und dann entscheidet die **Route des Renderpfads**,
nicht das Tool, über eine Abweichung.

### 4.2 Diff-Nachweis „kein Anfassen"

Drei Beweise, alle testseitig, ohne den committeten Datensatz zu ändern:

1. **Fixture-Byte-Stabilität im Test (T16):** der Test fährt das Tool gegen ein
   Rezept-Set mit **geändertem** `expected_route` (kopiertes Set im Temp, eine
   Route von `gpu` auf `{ "cpu": "geometry (default content crop)" }` gedreht —
   dieselbe Fixture-Idee wie `matrix_e2e.rs:107-112`), und prüft per
   `content_fingerprint`/Byte-Vergleich,
   (a) dass die Set-Datei **vorher == nachher** byte-identisch ist,
   (b) dass das Verhalten **identisch** zum CLI-Lauf auf derselben Mutation ist
   (Report-Feld für Feld), und
   (c) dass im Verify-Modus das Ergebnis **nicht** von der Mutation abhängt —
   denn `expected_route` wird dort nicht ausgewertet.
2. **Snapshot vorher/nachher (T18):** der komplette Fixture-Baum (Set, Samples,
   Goldens) wird vor und nach jeder Ablehnung verglichen; eine Ablehnung lässt
   **nichts** zurück (`merge_mcp_parity_common/graders.rs:393-400`).
3. **Diff-Nachweis auf Repo-Ebene:** `git diff --stat -- testdata/matrix/` muss im
   Verify-Bericht des Slices **leer** sein, und
   `git status --porcelain -- testdata/matrix/` darf höchstens das neue Dok
   enthalten. Das ist ein Abnahmekommando, kein Test (§10 (9)).

## 5. GPU-Gate-Behandlung (`--require-gpu`)

Befund aus den Quellen:

- Das Gate steht **nicht** in `lumina-core`/`lumina-merge`, sondern in der CLI:
  `render_standard_routed` gibt zusätzlich die `RenderRoute` zurück
  (`main.rs:497-538`, Dok :497-499 „the matrix's `--require-gpu` gate uses it to
  prove GPU parity from the **same** decision that produced the pixels").
- Es hat **drei** laute Ablehnungen, alle vor dem ersten Render:
  `--require-gpu` + `--update-goldens` (`matrix.rs:243-248`), `--require-gpu` ohne
  GPU-fähigen Build (:252-258), und pro Pair der Routen-Vergleich (:403-411 →
  `route-fail`, :538-549).
- Die committeten Recipes deklarieren 10× `"gpu"` und 1× die dokumentierte
  CPU-Ausnahme `g06-lens-perspective` = `{ "cpu": "geometry (default content crop)" }`
  (`recipe-set.v1.json:133-137`; Tabelle `conflicts-and-acceptance.md:158-170`).

**Einordnung (SOLL-Entscheid):** der MCP-Pfad braucht den GPU-Paritätsbeweis
**nicht**, und er darf ihn auch nicht behaupten. Der Merge-Teil hat genau dieselbe
Grenze gezogen (Merge-SOLL §8 „Gates, die ausdrücklich **nicht** behauptet
werden": „GPU/Adapter: nicht berührt, Merge ist CPU/pure-Rust"; `Agents.md:374-383:
ein GPU-Thema gilt ohne lokalen Lauf als nicht getestet`). Die Matrix ist
CPU-reproduzierbar gebaut — das nightly beweist die CPU-Referenz-Regression
(`.github/workflows/matrix-nightly.yml:14-18, :100-104`) und hält die
GPU-Aussage explizit aus dem Workflow heraus (:121-125).

Was der MCP-Pfad **leisten** muss: die **Vertragsparität** des Gates, nicht den
Beweis. Konkret:

1. `require_gpu` wird als JSON-Feld geführt und 1:1 auf denselben Runner-
   Parameter abgebildet (§2). Damit ist `lumina_matrix --require-gpu` in einem
   GPU-fähigen Build dasselbe Gate wie CLI — inklusive derselben Texte. Diese
   Behauptung ist **ohne** GPU testbar, weil zwei der drei Ablehnungen
   (:243-248, :252-258) deterministisch sind und die dritte (Routenvergleich)
   gegen eine gemockte Route gepinnt werden kann.
2. Der **Hardware**-Beweis („GPU↔CPU-Pixelgleichheit") bleibt eigener Task und
   eigener Host; dieses Doc behauptet ihn nicht. Er ist in
   `conflicts-and-acceptance.md:240-252` bereits als lokaler, Metal-gebundener
   Gate geführt.

**Risiko, offen zu klären (TODO §13 Punkt 3):** `RenderRoute` lebt heute als
`pub(crate)` in `main.rs:280-329`; der gemeinsame Kern (Impl-A, §11) braucht sie
oder einen gleichwertigen Port. TODO/Placeholder: Ziel-Crate + Name der
Route-Abstraktion ist mit dem Eigentümer zu bestätigen; fest steht nur, dass die
**Entscheidung** (welche Route) an genau einer Stelle fällt und von beiden
Frontends nur gelesen wird.

## 6. Invarianten

1. **Eine Implementierung:** MCP ruft denselben Runner wie die CLI
   (`matrix.rs:237-596`) über einen gemeinsamen, clap-freien Kern. Keine zweite
   Matrix-Logik, keine zweite Golden-Schreibung, kein zweiter Report.
2. **Report 1:1:** MCP liefert genau die Felder aus §3 — inklusive
   `expected_route` (Echo) und `route`/`route_reasons` — mit denselben Werten.
   `duration_ms` ist die einzige inhaltlich nicht vergleichbare Größe und wird
   nur im **Vergleich** maskiert, nicht aus dem Payload entfernt.
3. **Sample-Pfade sind relativ:** das Set referenziert Samples relativ zu seiner
   eigenen Datei (`recipe-set.v1.json:5-8`, `resolve_relative` :791-797); absolute
   Sample-Pfade lehnt die Set-Validierung laut ab (:638-644). Der Runner backt
   keinen Build-Host-Pfad ein (:799-835).
4. **Artefakt-tragende Rezepte werden laut abgelehnt**, nie still übersprungen
   (`reject_artifact_backed_stages`, :729-789: `source_actions`,
   `generative_edit`, externes `lens_blur.depth_artifact`, generative Spots,
   nicht-heuristische Spot-Modes, unbekannte Top-Level-Keys). Das Tool erbt das
   unverändert.
5. **Fehlendes Golden ist ein lauter Fehler** (:470-475); ein Toleranz-Verstoß
   ebenso (:503-526) samt Diff-Bild (:521-525). `update_goldens` schreibt
   explizit (:455-469).
6. **Goldens sind CPU-gepinnt** (`--update-goldens` rendert über
   `lumina_core::render_frame`, :376-386, F4-Kommentar :371-375); der Verify-Modus
   nutzt den Standardpfad (:387-389). Paritäts Tests müssen den Modus explizit
   setzen, sonst vergleichen sie unterschiedliche Renderpfade.
7. **`expected_route` wird nicht angefasst** (§4): kein Schreibpfad, Auswertung nur
   unter `require_gpu`, Inertheit im Default.
8. **Exit-Codes sind CLI-Sache, nicht Tool-Sache:** `0` grün, `1` Matrix-Fehler,
   `2` clap-Nutzungsfehler (`conflicts-and-acceptance.md:269-272`;
   `CliError::exit_code` :1517-1523). MCP bildet jeden Nicht-Grün-Fall auf
   `isError: true` ab; ein „grüner" Report mit `failed > 0` ist im CLI ein Fehler
   (:578-595) und darf im MCP **kein** Erfolg sein (§7).
9. **Kein stiller No-Op:** leeres Sample-Array, leeres Rezept-Array,
   nicht-existenter Filter, unbekanntes Feld → alle laut.
10. **Originale unverändert:** der Runner liest Samples nur (:322) und schreibt
    ausschließlich in `work_dir` (Exporte/Diffs) und `golden_dir` (Baseline).
    Sidecars der Samples werden nicht berührt.

## 7. Fehlerklassen (CLI-Text ≡ MCP-Text)

Der Matrix-Runner hat **keine** eigene Fehler-Enum wie `MergeRunError`; jeder
Domain-Abbruch ist ein `CliError::Message` mit einem satzartigen Text (Beispiele:
:243-248, :316-321, :690-697, :729-789). Der Kanon sind damit die **Texte**, nicht
Präfix-Klassen. SOLL-Vertrag:

| Fall | CLI (`matrix.rs`) | MCP-`McpError`-Variante | stabiler `error`-Name (`data.error`) | JSON-RPC-Code |
| --- | --- | --- | --- | --- |
| `--require-gpu` + `--update-goldens` | `Message` :243-248, Exit 1 | **neu: `McpError::Matrix`** | `MatrixError` | `-32014` |
| `--require-gpu` ohne GPU-Backend | `Message` :252-258, Exit 1 | `McpError::Matrix` | `MatrixError` | `-32014` |
| ungültiges Set (Parse, Version, IDs, Pfade, leere CPU-Begründung) | `Message` :599-681 | `McpError::Matrix` | `MatrixError` | `-32014` |
| unbekannter Rezept-Filter | `Message` :690-697 | `McpError::Matrix` | `MatrixError` | `-32014` |
| Sample-Datei fehlt | `Message` :315-321 | `McpError::Matrix` | `MatrixError` | `-32014` |
| Set wählt keine Rezepte | `Message` :298-303 | `McpError::Matrix` | `MatrixError` | `-32014` |
| Vergleichsbreite 0 | `Message` :266-272 | `McpError::Matrix` | `MatrixError` | `-32014` |
| Lauf endet mit `failed > 0` | `matrix failed: N of M pair(s) not green` :590-595 + Report auf stdout :562-576, Exit 1 | `McpError::Matrix` — **mit dem Report im `data`** (§7.1) | `MatrixError` | `-32014` |
| Tool-Argumente (Typfehler, unbekanntes Feld) | — (CLI: clap, Exit 2) | `McpError::InvalidParams` | `InvalidParams` | `-32602` |

Die Codes `-32001..-32013` sind belegt (`error.rs:62-87`); `-32014` ist der erste
freie Block-Platz. TODO/Placeholder: ob statt einer neuen Variante ein bestehendes
Vehikel (`Render`/-32007) genügt, ist §13 Punkt 4 — das Muster
„eigene Variante pro Klasse" hat der Merge-Teil etabliert (`error.rs:46-57`).

### 7.1 Der Report-im-Fehlerfall (einzige Stelle mit Payload plus Fehler)

Ein Matrix-Lauf mit roten Paaren ist ein **Ergebnis mit Fehler**: der CLI druckt
den vollständigen Report auf stdout und scheitert danach laut (:562-595). Ein
MCP-Tool, das in diesem Fall `isError: true` mit einem leeren `data`antwortet,
verliert genau die Information, wegen der der Aufrufer die Matrix gestartet hat.
SOLL deshalb: `McpError::Matrix` trägt den **gesamten** CLI-Report als
`data.report` und den Summensatz als `message`. TODO/Placeholder: Feldname und
genaue Form (`data.report` vs. verschachtelt) mit dem Eigentümer bestätigen
(§13 Punkt 5); `tool_error_result` (`error.rs:254-261`) setzt `data` bereits
unverändert in `structuredContent`, es braucht dort keine Änderung.

## 8. Fixtures / Goldens

- **Synthetischer Anker (hermetisch, Default-Suite):** ein 64×48-PNG-Gradient wie
  `matrix_e2e.rs:30-45` plus ein minimales Rezept-Set nach `matrix_e2e.rs:49-62`
  (`schema_version 1`, `pipeline_version "raster-mvp-1"`,
  `comparison_width 48`, Sample `sample.png` relativ). Zwei Rezepte mit
 unterschiedlichen Toleranzklassen (`exact`/`strict`) nach `:78-105`.
  Damit sind Baseline-Write, Verify-Grün, fehlendes Golden, Toleranz-Verstoß und
  Diff-Bild **ohne RAW** paritätstestbar.
- **Golden-Parität (neu, SOLL):** Baseline-Lauf auf beiden Transporten über
  **zwei** Bäume (`both_trees`, `merge_mcp_parity_common/graders.rs:381-391`) →
  die geschriebenen Golden-PNGs müssen byte-identisch sein. Weil der
  Baseline-Modus CPU-gepinnt ist (:376-386), ist das auf headless **beweisbar**.
- **Route-Fälle (deterministisch, ohne Adapter):** `matrix_e2e.rs:339-372` zeigt
  das Muster — `lens_correction` ohne expliziten Crop routed über den CPU-Orakel-
  Default (`geometry (default content crop)`), egal ob ein Adapter da ist; die
  Gegenprobe mit `expected_route: "gpu"` schlägt laut fehl. Dasselbe Paar für die
  MCP-Seite.
- **`expected_route`-Mutation (T16):** kopiertes Set, eine Route gedreht, Beweis
  der Inertheit im Verify-Modus + Byte-Stabilität der Set-Datei.
- **Committed-Set-Pinning (T2):** `testdata/matrix/recipe-set.v1.json` wird
  **gelesen** (nicht geschrieben): 2 Samples, 11 Rezepte, 22 kommittete Goldens,
  10× `gpu` + 1× dokumentierte CPU-Ausnahme. Verankert an
  `conflicts-and-acceptance.md:158-170` und `recipe-set.v1.json`.
- **Real-RAW-Gate:** die committeten CR3-Samples
  (`sample-data/raw/aircraft-landscape.cr3`, `-portrait.cr3`; Inventar +
  Lizenz in `sample-data/raw/README.md`, `golden-fixtures.md:44`) sind **keine**
  Belichtungsreihe — derselbe Inventar-Befund wie im Merge-Slice (Merge-SOLL §7).
  Ein MCP-Lauf dagegen ist technisch möglich (derselbe Runner, dieselben Goldens),
  aber teuer und RAW-abhängig → `#[ignore]` + env-gated `LUMINA_MATRIX=1`
  (Muster `matrix_e2e.rs:409-431`).
- **Toleranzen:** keine für synthetische Pfade (deterministisch). Es gibt **keine**
  numerische Toleranz im MCP-Vertrag — der Bericht liefert `psnr_db` als String.

## 9. Testmatrix

Notation: **[U]** Unit/Integration ohne Pixel, **[P]** Parität (CLI-Run vs.
MCP-Run, gleiche Eingabemenge), **[N]** Negativ/Fehlerpfad, **[G]** Gate
(RAW/Fixture oder manuell).

### Phase 1 — Registry & Schema (kein Pixel, kein I/O)

| ID | Art | Ort (SOLL) | Prüft | Fixture |
| --- | --- | --- | --- | --- |
| T1 | [U] | neue Testdatei `crates/lumina-mcp/tests/matrix_tools.rs` | `TOOL_NAMES` + `list_tool_definitions` + `is_known_tool` enthalten `lumina_matrix`, keine Duplikate, Zählguard — Muster `library_tools.rs:47-91`, `tool_names/mod.rs:60-114` | keine |
| T2 | [U] | `crates/lumina-mcp/tests/matrix_tools.rs` | committetes Set liegt vor und ist unberührt: 2 Samples, 11 Rezepte, 10× `gpu` + 1× `geometry (default content crop)`, `comparison_width` 384 — Lesen, kein Schreiben | `recipe-set.v1.json` |
| T3 | [U] | `crates/lumina-mcp/tests/matrix_tools.rs` | `tools/list` liefert das Schema: `additionalProperties:false`, Property-Menge == §2-Felder, `required == []`, Typen, `comparison_width` Integer mit `minimum: 1` | keine |
| T4 | [U] | `lumina-mcp` Unit im Tool-Modul | `reject_unknown_fields`-Vorab-Check: unbekanntes Feld → `InvalidParams` **vor** jedem Datei-Zugriff (Shape-Gate zuerst) | keine |
| T5 | [U] | `crates/lumina-cli/src/tests/mcp_onnx.rs:27,65-96` | zweiter Pin: `tools.len()` 28 → **29**, `EXPECTED_TOOLS` um `lumina_matrix` gewachsen | keine |

### Phase 2 — Validierung & Fehlerklassen (kein Lauf)

| ID | Art | Ort (SOLL) | Prüft | Fixture |
| --- | --- | --- | --- | --- |
| T6 | [U] | `lumina-mcp` Unit | Typfehler → `InvalidParams`: `recipe_set` kein String, `recipes` kein Array / Nicht-String-Element, `comparison_width` = `1.5`/`"384"`/`-1`, `update_goldens`/`require_gpu` kein Bool | keins |
| T7 | [N] | `crates/lumina-mcp/tests/matrix_tools.rs` | fehlende Set-Datei → `Matrix` (nicht `InvalidParams`), Text enthält den Pfad | Tempdir-Pfad ohne Datei |
| T8 | [N] | `crates/lumina-mcp/tests/matrix_tools.rs` | `comparison_width: 0` → `Matrix` mit CLI-Text; `null` → Set-Default 384/48 | minimales Set |
| T9 | [N] | `crates/lumina-mcp/tests/matrix_tools.rs` | `recipe_set` ohne `recipes` → `Matrix` „selected no recipes"; `recipes: ["nope"]` → `Matrix` „unknown recipe id" | minimales Set |
| T10 | [N] | `crates/lumina-mcp/tests/matrix_tools.rs` | Sample-Datei fehlt → `Matrix` „matrix sample … not found at …" | Set ohne PNG |
| T11 | [N] | `crates/lumina-mcp/tests/matrix_tools.rs` | ungültiges Set: falsche `schema_version`, falsche `pipeline_version`, leere Samples/Recipes, absoluter Sample-Pfad, doppelte IDs, leere CPU-Begründung → je `Matrix` mit dem jeweiligen Text (:599-681) | 6 Set-Varianten |
| T12 | [N] | `crates/lumina-mcp/tests/matrix_tools.rs` | artefakt-tragende Rezepte und Typoschlüssel werden **laut** abgelehnt: `source_actions`, `generative_edit`, externer `depth_artifact`, generativer Spot, fremder Spot-Mode, unbekannter Top-Level-Key (:729-789) — Muster `matrix_e2e.rs:249-331` | 6 Rezept-Varianten |
| T13 | [N] | `crates/lumina-mcp/tests/matrix_tools.rs` | `require_gpu` + `update_goldens` → `Matrix` mit Text „cannot be combined with --update-goldens" (:243-248) — **ohne** GPU_BUILD bestimmt | minimales Set |
| T14 | [N] | `crates/lumina-mcp/tests/matrix_tools.rs` | `require_gpu` im CPU-Build → `Matrix` „no GPU backend" (:252-258); `#[cfg(feature = "gpu")]`-Gegenstück | minimales Set |

### Phase 3 — Parität & Artefakte (der Kern)

| ID | Art | Ort (SOLL) | Prüft | Fixture |
| --- | --- | --- | --- | --- |
| T15 | [P] | neu: `crates/lumina-cli/tests/matrix_mcp_parity.rs` (Spiegel `merge_mcp_parity.rs`) | Verify-Run: `structuredContent` == CLI-`--json` **Feld für Feld** nach den zwei Masken (§3.1); `command == "matrix"`, `mode == "verify"`, `pairs_total/passed/failed` identisch | Anker §8, 1 Sample × 2 Rezepte |
| T16 | [P] | `matrix_mcp_parity.rs` | **`expected_route`-Schutz:** Set-Kopie mit gedrehter Route → (a) Set-Datei byte-identisch vorher/nachher, (b) MCP == CLI auf derselben Mutation, (c) grünes Ergebnis unverändert (Inertheit im Verify-Modus) | Anker + mutierte Kopie |
| T17 | [P] | `matrix_mcp_parity.rs` | **Goldens byte-identisch:** Baseline-Run (CPU-gepinnt) auf Baum CLI gegen Baum MCP → `blake3(golden_cli) == blake3(golden_mcp)`, `mode == "baseline"`, `status == "baseline-created"` | Anker, zwei Bäume |
| T18 | [P] | `matrix_mcp_parity.rs` | **Export/Diff byte-identisch:** mit `work_dir` → `blake3(export_cli) == blake3(export_mcp)` und auf dem Toleranz-Verstoß-Pfad auch das Diff-Bild | Anker + überschriebenes Golden |
| T19 | [P] | neu: `crates/lumina-cli/tests/matrix_mcp_parity_errors.rs` | Fehlertexte 1:1: für jede Domänen-Ablehnung aus §7 gilt CLI-`error:`-Zeile == MCP-`message` nach dem stabilen Namen (`cli_cause`/`mcp_cause`, `graders.rs:48-66`) | wie T8–T13 |
| T20 | [N] | `matrix_mcp_parity_errors.rs` | jede Ablehnung hinterlässt **nichts**: Snapshot des Fixture-Baums vorher == nachher (`graders.rs:393-400`) | wie T19 |
| T21 | [P] | `matrix_mcp_parity_errors.rs` | roter Lauf: Report kommt an **und** ist ein Fehler — `isError: true`, `data.report` == CLI-stdout-JSON, `failed > 0`, `message` enthält den CLI-Summensatz | Anker + fehlendes Golden |
| T22 | [P] | `matrix_mcp_parity.rs` | golden-dir-Override: explizites `golden_dir` wird von beiden Transporten benutzt und die Report-Pfade zeigen dorthin (normalisiert) | Anker, eigener Golden-Ordner |
| T23 | [P] | `matrix_mcp_parity.rs` | Rezept-Filter: `recipes: ["g01-identity"]` reduziert auf genau dieses Paar auf beiden Transporten, `pairs_total == 1` | Anker |
| T24 | [P] | `matrix_mcp_parity.rs` | Default-Auflösung: weder `recipe_set` noch `golden_dir`/`work_dir` übergeben → **derselbe** Auflösungsweg (env-Kette bzw. `<set>/golden`) in beiden Berichten sichtbar | Anker über die env-Kette |
| T25 | [P] | `matrix_mcp_parity.rs` | **Sidecar/Sample-Unberührtheit:** Sample-PNGs und (falls vorhanden) ihre Sidecars byte-identisch nach dem Lauf | Anker mit Sidecars |
| T26 | [G] | neu: `crates/lumina-cli/tests/matrix_mcp_parity_gate.rs`, `#[ignore]` + `LUMINA_MATRIX=1` | Real-RAW-Parität: committetes Set, gebundener Rezept-Subset, `--json`-Report MCP == CLI field für field; Goldens **nicht** neu schreiben | `testdata/matrix/**`, 2 CR3s |

### Gates, die ausdrücklich **nicht** behauptet werden

- **GPU-Parität** (`--require-gpu`-Verdikt auf echtem Metall): nicht Gegenstand
  dieses Slices (§5); die Vertrags-Parität des Flags schon.
- **Real-RAW-Parität (T26):** offen, läuft nur mit `LUMINA_MATRIX=1` und braucht
  die LibRaw-Fixtures.
- **`cargo`-Läufe in der SOLL-Phase:** verboten (Merge-SOLL §8); alle Kommandos
  stehen als Soll-Kommandos in der Verify-Phase.

## 10. Abnahmekriterien für den ersten Code-Slice

„Stubs nicht ausreichend": der erste Slice muss auf dem synthetischen Anker
**vollständige, byte-paritäre** Antworten liefern, und auf dem **committeten** Set
das, was ein Stub prinzipiell nicht kann — nämlich echte Pixel gegen echte
Goldens. Konkret:

**Auf dem synthetischen Anker (§8: 64×48-PNG, Set mit `comparison_width 48`,
zwei Rezepte `exact` und `strict`):**

1. `lumina_matrix` antwortet auf `tools/call` mit `isError: false` und einem
   `structuredContent`, das **exakt** die zehn Top-Level-Felder aus §3 trägt
   (`command`, `mode`, `recipe_set`, `golden_dir`, `comparison_width`,
   `pairs_total`, `passed`, `failed`, `duration_ms`, `pairs`) — kein
   `saved`/`action`/`text`, kein `warnings[]`.
2. `command == "matrix"`, `mode == "verify"`, `pairs_total == 2`, `passed == 2`,
   `failed == 0`.
3. Je Pair: `status == "pass"`, `psnr_db == "inf"` (CPU-gepinnte Baseline gegen
   identischen Verify-Lauf), `max_abs_diff == 0`, `expected_route` als **Echo**
   des Inputs, `route == "cpu"` im CPU-Build, `route_reasons == []`,
   `message == null`.
4. `digest`-artige Identität: der T15-Vergleich (MCP == CLI Feld für Feld nach
   den zwei Masken) ist grün — inklusive der `psnr_db`-**Strings**.
5. Baseline-Run: `mode == "baseline"`, `status == "baseline-created"`, und das
   geschriebene Golden ist **byte-identisch** mit dem CLI-Golden (T17) — das ist
   der eigentliche Stub-Tod, denn ein Stub müsste echte PNG-Bytes treffen.
6. Toleranz-Verstoß: `isError` false nicht — der Run **scheitert** als Tool-Fehler
   mit `data.report` (T21), `status == "fail"`, `message` enthält
   „requires PSNR", und das Diff-Bild existiert in `work_dir` (T18).
7. Fehlendes Golden: `message` enthält „missing golden" und der Exit-
   äquivalente Fehler befördert den Report (T21).
8. `require_gpu` + `update_goldens` → `Matrix` mit dem CLI-Text; im CPU-Build
   `require_gpu` allein → `Matrix` „no GPU backend" (T13/T14).
9. `expected_route`-Schutz diff-nachweisbar (T16 + `git status --porcelain --
   testdata/matrix/` leer außer dem neuen Dok).
10. Registry in **beiden** Pfaden: `dispatch_tool` kennt den Namen (kein
    `MethodNotFound`-Durchreichnen, `tools/mod.rs:137`-Muster), `is_known_tool`
    akzeptiert ihn, `TOOL_NAMES` und `EXPECTED_TOOLS` sind gewachsen (T1, T5).

**Auf dem committeten Set (lesend, `LUMINA_MATRIX=1`, T26):**

11. Gebundener Subset-Lauf (z. B. `g01-identity` + `g01-tone-wb`, Muster
    `matrix_e2e.rs:432-443`): MCP-`structuredContent` == CLI-`--json`, 22/22
    bzw. 2/2 Paare grün, `psnr_db` identisch (**Strings**).
12. `expected_route` der committeten Rezepte wird unverändert ausgeliefert
    („gpu" bzw. „cpu (geometry (default content crop))") und die Datei bleibt
    byte-identisch.

**Gates für den Slice selbst:** `cargo test -p lumina-mcp` +
`cargo test -p lumina-cli --test matrix_mcp_parity` (+ `_errors`) grün,
`cargo fmt --all -- --check` grün, `cargo clippy -p lumina-mcp --all-targets --
-D warnings` grün, `sh scripts/check_file_sizes.sh` exit 0,
`sh scripts/check_plan_format.sh` exit 0 (**Soll-Kommandos**, Merge-SOLL §9).

## 11. Roadmap (SOLL → Impl → Verify)

1. **SOLL (dieses Doc):** Review/Sign-off; Tool-Name finalisieren.
2. **Impl-A — gemeinsamen Kern freilegen:** der Matrix-Runner
   (`matrix.rs:237-596`) wird als **clap-freier** Kern in eine gemeinsam
   linkbare Crate gehoben (TODO: `lumina-stages` vs. `lumina-core` — §13 Punkt 1;
   das Merge-Vorbild hat den Adapter nach `lumina-stages` gezogen, Commit
   `189b7d9`). Schnittstelle: ein `MatrixRequest`-Struktur (Feld für Feld =
   §2, Muster `RegenerateRequest`, `regenerate.rs:139-153`) plus ein Render-Port
   (Muster `MatchingRender`, `regenerate.rs:107-137`) und der Report-Rückgabe.
   `main.rs` bleibt baseline-gesperrt, also darf der Runner **nicht** dort
   wachsen — nur ein dünner Dispatch bleibt (`main.rs:1562`).
3. **Impl-B — Tool-Modul:** `crates/lumina-mcp/src/tools/matrix.rs` angelegt,
   analog `merge_hdr.rs`/`dust_removal.rs`; Register in `tools/mod.rs` (list +
   dispatch) und `tool_names/mod.rs` (1 Name).
4. **Impl-C — Error-Mapping:** `McpError::Matrix` (name `MatrixError`, Code
   `-32014`) inklusive `data.report` für den roten Lauf (§7.1).
5. **Impl-D — Paritäts-Tests:** `matrix_mcp_parity.rs` + `_errors.rs` +
   `_gate.rs` mit gemeinsamem Harness (Modul-Doku + `fixtures`/`graders`/`job`,
   Spiegel `merge_mcp_parity_common/`); MCP-Oberflächen-Tests
   `crates/lumina-mcp/tests/matrix_tools.rs`.

### 11.1 Phase-1-Scaffold-Entwurf

**Neue Dateien (neu anzulegen):**

| Datei | Inhalt | Größen-Risiko |
| --- | --- | --- |
| gemeinsamer Kern (Impl-A, Crate offen) | `MatrixRequest`, Runner-Logik, Report-Bau, Route-Port | ~400 Z., droht >500 → dann aufteilen (`matrix_run.rs` + `matrix_report.rs`) |
| `crates/lumina-mcp/src/tools/matrix.rs` | `NAME`/`DESCRIPTION`/`schema()` + `run` → gemeinsamer Kern | ~120 Z. |
| `crates/lumina-mcp/tests/matrix_tools.rs` | T1–T4 | ~150 Z. |
| `crates/lumina-cli/tests/matrix_mcp_parity_common/{mod,fixtures,graders,job}.rs` | Harness (Spiegel `merge_mcp_parity_common/`) | ~250 Z. gesamt |
| `crates/lumina-cli/tests/matrix_mcp_parity.rs` | T15–T18, T22–T25 | ~300 Z., droht >500 → dann aufteilen (Muster `merge_mcp_parity*.rs`) |
| `crates/lumina-cli/tests/matrix_mcp_parity_errors.rs` | T19–T21 | ~200 Z. |
| `crates/lumina-cli/tests/matrix_mcp_parity_gate.rs` | T26, `#[ignore]` | ~120 Z. |

**Zu modifizierende Dateien (minimal-invasiv):**

| Datei | Änderung |
| --- | --- |
| `crates/lumina-mcp/src/tools/mod.rs` | +1 `pub mod`, +1 `tool_def(...)`, +1 Arm in `dispatch_tool`. 179 Z. → bleibt unter 500 |
| `crates/lumina-mcp/src/error.rs` | +1 Variante `Matrix(String)` + `code()`/`name()`/`message()`/`data()` (:46-156). 266 Z. → bleibt unter 500 |
| `crates/lumina-mcp/tests/tool_names/mod.rs` | +1 Name in `TOOL_NAMES` (:22-59) |
| `crates/lumina-cli/src/tests/mcp_onnx.rs` | +1 Name in `EXPECTED_TOOLS` (:65-96), `:27` 28 → 29 |
| `crates/lumina-cli/src/main.rs` | **nur** falls Impl-A es erzwingt: Dispatch-Zeile bleibt `Command::Matrix(args) => matrix::matrix(args)` (:1562); jede Zeile Wachstum bricht `check_file_sizes.sh` (Baseline 7874, `scripts/file_size_baseline.txt:1`) — der Runner zieht daher **raus** aus `main.rs` |
| `crates/lumina-cli/src/matrix.rs` | wird zum dünnen Wrapper (Args → `MatrixRequest` → Kern); 1007 Z. → erst unter 500, wenn der Kern raus ist → Baseline-Eintrag senken (`check_file_sizes.sh:35-44` meldet das) |

**NICHT anfassen:**

- `testdata/matrix/recipe-set.v1.json` — `expected_route` wird **nicht**
  geändert (§4). Das ist die härteste Nicht-Anfassen-Regel dieses Slices.
- `testdata/matrix/golden/**` — die 22 committeten Goldens bleiben unberührt;
  ein `--update-goldens` auf dem committeten Set ist in diesem Slice **nicht**
  vorgesehen (und würde eh CPU-pinnte Goldens neu schreiben).
- `crates/lumina-cli/src/main.rs` — baseline-gesperrt (7874 Z.).
- `crates/lumina-cli/tests/matrix_e2e.rs` (451 Z., **kein** Baseline-Eintrag —
  die Datei liegt unter der 500er-Schwelle, `scripts/file_size_baseline.txt`) —
  die MCP-Fehlerpfade gehen in die **neue** `matrix_mcp_parity_errors.rs`, nicht
  hierhin; der bestehende CLI-Anker bleibt.
- `crates/lumina-gui/src/matrix.rs` (507 Z., baseline :68) — GUI-frei in diesem
  Slice; sein Render-Port bleibt unberührt.
- `crates/lumina-core/**`, `crates/lumina-stages/**` (außer der Impl-A-Aufnahme),
  `crates/lumina-sidecar/**`.
- `scripts/file_size_baseline.txt` — kein neuer Eintrag ohne Not; nach einer
  Extraktion den alten Eintrag senken.

**Rollback-Kriterium (ein Satz, vorab festgelegt):** schlägt einer der Schritte
T15–T18 (Report-Parität, `expected_route`-Schutz, Golden-Identität,
Export/Diff-Identität) fehl und lässt sich **nicht** durch Adapter-Anpassung
allein reparieren — d. h. der Fix würde `lumina-core`-Render-, `lumina-cli`-
Runner- oder Golden-Verhalten ändern — wird der gesamte Code-Slice verworfen,
statt die CLI-Referenz zu verbiegen. Zusatz: jedes neue Modul über 500 Z. wird
vor dem Commit aufgeteilt (Ratchet).

## 12. Abgrenzung

- **Nicht Teil:** GPU-Paritätsbeweis (§5), Real-RAW-Serien-Gate T26 als
  Abnahme-Gate, `denoise`/`cull` (F-078-Modellgate, `mcp-server.md:790-795`),
  Merge-Teil (fertig), Auto-Tone.
- **Keine Änderung:** Recipe-Set-Schema, Goldens, Toleranzklassen, Renderpfad,
  `expected_route`-Pflege.
- **Kein GUI-Slice:** die GUI-Headless-Matrix
  (`crates/lumina-gui/src/matrix.rs`) ist ein eigener Frontend-Pfad; hier zählt
  nur CLI ↔ MCP.
- **Kein `expected_route`-Anfassen** (§4) — analog der Merge-Klausel
  (Merge-SOLL §11), nur dass `matrix` das Feld wirklich hat.
- **Kein stiller No-Op:** leere Mengen, unbekannte Filter, unbekannte Felder →
  laut.

## 13. Offene Fragen an Eigentümer

Eigene Annahmen/Caveats des Verfassers — keine davon ist im Code belegt, alle
sind als TODO/Placeholder markiert:

1. **Ziel-Crate für den gemeinsamen Kern** (Impl-A, §11 Schritt 2):
   `lumina-stages` (näher an den anderen Frontend-Adaptern, hat bereits
   `decode.rs`/`pipeline.rs`/`regenerate.rs` als Vorbilder) oder `lumina-core`
   (näher am Render)? Die Entscheidung ändert den Dependency-Graph; `lumina-mcp`
   hängt bereits an `lumina-core`, `lumina-stages`, `lumina-raw`
   (`crates/lumina-mcp/Cargo.toml:9-28`), ein Zyklus wäre in beiden Fällen neu zu
   prüfen.
2. **`McpError::Matrix` vs. Recycling** (§7): eine eigene Variante (freier Code
   `-32014`, stabiler Name `MatrixError`) oder bestehendes Vehikel
   (`Render`/-32007) mit Präfix im Text? Das ist die einzige Stelle, an der
   dieser Slice die öffentliche Fehler-API von `lumina-mcp` erweitert.
3. **Route-Abstraktion für den gemeinsamen Kern** (§5): `RenderRoute` ist heute
   `pub(crate)` in `main.rs:280-329` und `#[cfg]`-abhängig. Zieht sie als
   `gpu`-optionales Item mit, oder bekommt der Port eine cfg-freie
   Minimalform (`is_gpu` + `reasons`)? TODO, code-seitig noch nicht entschieden.
4. **Form des Reports im Fehlerfall** (§7.1): `data.report` als flaches Objekt
   oder verschachtelt (`data.matrix.report`)? Und: darf der **Summensatz**
   (`matrix failed: N of M pair(s) not green …`, :590-595) 1:1 im `message`
   stehen, obwohl er Pfade/Adressen des aufrufenden Hosts enthält?
5. **Textkanal** (§3): `tool_result_response` serialisiert das Payload automatisch
   als `content[0].text`. Soll zusätzlich der CLI-Erfolgssatz
   (`report_text`, :923-962) erscheinen? Für den Merge-Teil war das offen
   (Merge-SOLL §13 Punkt 6); hier stellt sich dieselbe Frage.
6. **`work_dir`-Default im MCP-Pfad** (§2): der CLI nimmt ein frisches Tempdir und
   löscht es nach dem Lauf (:284-295). Ein MCP-Tool, das den Aufrufer nach dem
   Lauf auf ein Diff-Bild hinweisen will, hätte nichts mehr, worauf es zeigen
   kann. Soll das Tool im `work_dir`-Default-Fall die Export-Pfade trotzdem im
   Report nennen (wie der CLI) — oder bewusst auf einem eigenen Scratch-Pfad
   arbeiten? TODO, Verhalten des CLI ist die Vorgabe, die Frage ist nur, ob der
   Eigentümer eine Abweichung wünscht.
7. **`psnr_db` als String** (§3): der CLI liefert `"inf"`/`"42.5000"` als String
   (:979-981). 1:1 heißt String; ein numerisches Feld wäre bequemer für Clients,
   aber additiv. Bestätigen, dass String bleibt.
8. **T26 als Abnahme-Gate:** das committete Set ist mit den zwei CR3s lauffähig
   (anders als Merges Belichtungsreihe). Soll T26 — `#[ignore]` +
   `LUMINA_MATRIX=1` — als **Abnahme** dieses Slices gelten, oder genügt die
   synthetische Parität und T26 bleibt reine Regression? TODO.
9. **Docs-Nachzug:** welche Doks der Slice fortschreiben muss (SOLL-Phase nur
   benannt, nicht getan): `feature/platform/mcp-server.md` (Tool-Tabelle
   :740-747, Offen-Liste :749-750, Kopfzahl :4-23), `Agents.todo.md` :409,
   `feature/quality/conflicts-and-acceptance.md` §Modi (:220-232) falls der
   MCP-Pfad ins Nightly soll.



