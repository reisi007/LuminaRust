# LuminaRust Umsetzungsplan

Dieser Plan ist eine lebende Arbeitsliste. Er wird während der Implementierung
fortgeschrieben. Erledigte Aufgaben werden nach bestandener unabhängiger
Verifizierung und bestätigter Testabdeckung aus dieser Datei entfernt — es gibt
keine dauerhafte Liste abgehakter Aufgaben. Details zu Erledigtem liegen in den
Feature-Dokumenten und der Git-Historie.

## Gepinnte Entscheidungen und Absprachen

**Formregel (User-Regel 2026-09-28, bestaetigt):** Jede hier gefuehrte
Entscheidung traegt **ihre Begruendung** und einen **Stand**. Eine Entscheidung,
die **verifiziert abgeschlossen** ist, wird **aus dieser Datei entfernt** — genau
wie eine Aufgabe, und aus demselben Grund: der erledigte Zustand gehoert in die
Git-Historie und in die `feature/`-Dokumente, nicht in eine Liste, die als
Arbeitsvorrat gelesen wird. Es gibt hier **kein** `- [x]`; `scripts/check_plan_format.sh`
verbietet es in dieser Datei, und der Spaltenwert `umgesetzt`/`erledigt` ist
dort ebenfalls ein Verstoss. **Zurueckgezogene** Ansätze stehen in
[Verworfen](#verworfen) — sie sind keine offenen Entscheidungen, sondern
Negativrecords, damit sie nicht erneut implementiert werden.

### Geltende Entscheidungen

| Kürzel | Entscheidung | Begründung | Stand |
| --- | --- | --- | --- |
| `LIZ` | interim proprietär/kommerziell: **kein** `license`-Feld, **keine** `LICENSE`-Datei | Eigentümerentscheid 2026-08-20; eine erfundene Lizenz wäre eine Rechtsbehauptung ohne Grundlage. Fixtures-R1 ist davon **unberührt** geschlossen (`fixtures-licensing.md` §4/§8); Lensfun (LGPL-3.0 dynamisch, DB CC-BY-SA-3.0) ist in `THIRD-PARTY-NOTICES.md` dokumentiert und gilt unabhängig von der Wahl | **offen** (Sobald entschieden: `license`-Felder + Root-`LICENSE` ergänzen) |
| `MVP-Grenze` | MVP = CLI + native Desktop-GUI inkl. nativem RAW. WASM/Browser **ersatzlos gestrichen** (2026-09-04), Cache- und Mehrbild-Synchronisierung bewusst Post-MVP | Architektur bleibt nativ, einheitlicher `decode_bytes`/`RawMetadata`-Vertrag; ein zweiter Backend-Pfad hätte zwei Render-Mengen erzwungen | gilt |
| `Release-Staffel` | IPTC/Metadaten-Presets von 1.5 nach **1.0** vorgezogen; 1.5 = HDR/Panorama-Merge + Rote Augen + Auto-Upright; 2.0 = Gesichtserkennung + KI-Denoise; 2.5 = KI-Culling; **nie** Ziel: Karten/GPS, Veröffentlichungsdienste | User-Entscheid 2026-09-04; Entscheid dokumentiert in `feature/decisions/LRPAR-G15-META-15.md` (Sidecar-Draft, pure-Rust-Bake-In, JPEG-only, GUI-Panel mit) | gilt |
| `Sidecar-Pre-MVP` | Schemaänderungen sind bis zum MVP Breaking Changes; die Migrations-Maschinerie bleibt **dauerhaft** im Code; `schema_version` bleibt 1, inkompatibles wird laut abgelehnt | Vor-MVP gibt es keine Abwärtskompatibilitätspflicht, aber die Maschinerie wird ab MVP für Release-Migrationen gebraucht — der v1→v2-Pfad ist als Muster mit Tests umgesetzt | gilt |
| `PINS` | libraw-sys vendored (`[patch.crates-io]`, macOS-C++-Fix), `ort =2.0.0-rc.13`, LibRaw 0.22.2 + Ubuntu-24.04-lensfun-Distro-Pin | Determinismus; ein Upgrade-Pfad ist skizziert (neuer Image-Tag parallel → Golden-Rebaseline wegen CR3-Dimensionen → alter Tag erst dann entfernt) | gilt |
| `CI-GATE` | `onnx-rt` wird im CI **geprüft** (Image liefert `libssl-dev` + `clang`); **GPU bleibt hartes CI-Nein** | Runner haben kein Metal, dort ist nur `cargo check -p lumina-gpu --features gpu` möglich; ein GPU-Test in CI wäre ein grüner Schein | gilt |
| `Toolchain` | CI fährt `@stable`, damit neue Clippy-Lints automatisch anschlagen; lokal vor jedem Push `rustup update` + Workspace-Clippy | Ein Lint, der erst beim nächsten Toolchain-Wechsel sichtbar wird, ist kein Gate (Beispiel: `chunks_exact_to_as_chunks`) | gilt |
| `REIHENFOLGE` | **alle** offenen Tasks werden abgearbeitet; der Build-Agent wählt die Reihenfolge nach Gate-Gefährdung → Abhängigkeit → Hardware-Gate | Eigentümerentscheid 2026-09-28 („alle müssen abgearbeitet werden"). Kein Fallenlassen, aber eine **begründete** Wahl — ein hardwaregegateter Punkt ist kein Grund anzuhalten | gilt |
| `MODELL` | ausschliesslich freie Subagenten, kein deepseek, keine Ausnahme | User-Weisung 2026-09-29; ersetzt deepseek-Sparsamkeit (2026-09-28) und deepseek-Eskalation (2026-09-26); die drei laufenden Auftraege enden noch auf Standardmodell | gilt |
| `GOLDEN-STALE-SOLL` | `develop_section_rating` friert den abgewarteten Render-Zustand ein (`settle_render`-Helfer, Prädikat `render_key.is_some()`); Golden dafür neu aufgezeichnet | Mechanismus belegt 2026-09-28 (drei Sonden); die Mask-Warnung ist der ehrliche Endzustand zur Fixture; Seed-Alternative verworfen | gilt |
| `GOLDEN-RHYTHMUS` | kein fester Neuaufnahme-Rhythmus; jedes `record` bleibt einzeln begründet (Modell `GOLDEN-BASELINE-32`) | Eigentümerentscheid 2026-09-28; ein Rhythmus hätte den unbelegten Vorzustand nur neu eingefroren | gilt |
| `GPU-E2E` | GPU-Themen gelten ohne lokalen Lauf der **relevanten E2E-Tests** als **nicht getestet**; Pflichtkommando `cargo test -p lumina-gpu --features gpu-adapter-tests` plus die betroffenen GUI-GPU-Tests. `--features gpu` allein genügt **nicht**. | User-Weisung 2026-10-02; **gemessen am 2026-10-22 (CI-Lauf `39733087689`): die 32 adapterabhängigen Integrationstests** (u. a. `crates/lumina-gpu/tests/parity.rs`) sind ohne `gpu-adapter-tests` `#[ignore]`t, ein `cargo test -p lumina-gpu --features gpu` überspringt sie und liefert einen grünen Lauf ohne Aussage. CI hat keinen GPU-Runner und kompiliert nur | gilt |
| `AUTO-DOMAIN` | Auto-Tone rechnet auf dem Frame **nach** SourceActions **und nach** Crop; der Zuschnitt beeinflusst die Auto-Werte | Eigentümerentscheid 2026-10-02 (wörtliche Anforderung „nach wegretuschieren UND nach Zuschnitt"); die Domäne war ausdrücklich als Eigentümer-Entscheidung markiert, nicht als Codefrage | entschieden 2026-10-02; Umsetzung in `AUTO-TONE-ANALYSIS-INPUT-8` |
| `JSON-ROUNDTRIP` | `serde_json` wird mit `float_roundtrip` gebaut; jeder f64-Round-Trip ist exakt | Eigentümerentscheid 2026-10-02: „es soll nichts verloren gehen" (gemessen: 7,45 % der f64-Werte verloren beim Laden 1 ULP). Kostet eine Neubewertung aller Sidecar-Bytes/Digests/Goldens | entschieden 2026-10-02; Umsetzung in `JSON-FLOAT-ROUNDTRIP` |
| `NAMING-F1 (2)` | Der **Anzeigename** bleibt „Lumina" (`crates/lumina-gui/src/main.rs` -> `main` — der `eframe::run_native`-Fenstertitel; `crates/lumina-gui/src/lib.rs` -> `LuminaApp::ui` — der `ui.heading("Lumina")`-Kopf); der Formatbezeichner `.lumina.*` bleibt ohnehin | Eigentümerentscheid 2026-10-02 (Option „Lumina beibehalten"); keine Migration, null Zeilen | gilt |
| `GPU-CLEAR-TRANSITION` | Der Masken-Plane-Clear läuft **nur beim Übergang** „Nicht-Null-Plane resident → keine Ebene", nicht bei jedem Render | Eigentümerentscheid 2026-10-02; gemessen (F-074-N10, Metal M5 Pro): der Clear kostet **37 µs / 722 µs / 15,21 ms** (160×120 / 1280×853 / 6000×4000), volumengetrieben, ~13× ein Vollrender @24 MP — die Host-Zahl 0,004 ms täuschte | entschieden 2026-10-02; Umsetzung in `GPU-MASKPLANE-CLEAR-PERF-51` |
| `CACHE-SCHICHT` | die native Cache-/Preview-Disk-Schicht wird aus `lumina-core` in ein **eigenes Crate `lumina-cache`** gezogen; Core kompiliert danach ohne Dateisystemzugriff | Architektur-Review 2026-10-16 (Befund R4) — Core brach die in `Agents.md` und `feature/platform/cli-gui-wasm.md` gesetzte Grenze, während `feature/architecture/pipeline.md` den Bruch als umgesetzt dokumentierte | **entschieden 2026-10-16; Umsetzung in `LUMINA-CACHE-EXTRACT-70`** |
| `DOWNSCALE-EIN-PFAD` | es bleibt **ein** Downscale-Pfad, GPU-fähig gedacht; wo Geschwindigkeit und Qualität auseinandergehen, gilt **Qualität**, die Geschwindigkeitsoptimierung bleibt Zukunft | Eigentümerentscheid 2026-10-16 auf den Review-Befund „zwei bilineare Downscaler im Core plus ein Nearest-Neighbour-Thumbnailer in der GUI" | **entschieden 2026-10-16; Umsetzung in `DOWNScaler-UNIFY-71`** |
| `HEADLESS-GPU-TASK-SPLIT` | GPU-gegatete Tasks werden headless gebaut, der adapter-/GUI-E2E-Rest wandert als eigener Eintrag in `Agents.todo.md` — **rückwirkend** für alle bestehenden | Eigentümerweisung 2026-10-16; die Regel steht in `Agents.headless.md` §1 Punkt 6 | **gilt** |

### Offene Entscheidungen (warten auf den Eigentümer)

_(keine — `NAMING-F1 (1)` wurde am 2026-10-02 vom Eigentümer entschieden: der Anzeigename bleibt „Lumina"; siehe `Geltende Entscheidungen` / `NAMING-F1 (2)`.)_

### Verworfen

Zurückgezogene Ansätze. Sie sind **keine** offenen Entscheidungen und dürfen
**nicht** erneut implementiert werden; der geltende Stand steht jeweils daneben.

| Verworfener Ansatz | Warum verworfen | Geltender Stand |
| --- | --- | --- |
| Umfangs-Zusicherung „der Nachweis decodiert **jede** committete Fixture" (`fixtures-licensing.md` §3.2.1 **Regel 4 und Regel 6**) | Sechs Verifikationsrunden, sechs Orte **derselben** Fehlerklasse. Jeder Fix verlagerte den einzigen Vertrauenspunkt, und die nächste Mutation zielte dorthin. Ein **Modellwechsel** änderte nichts — der Befund war ein **Entwurfsproblem** | Als **Zusage** zurückgenommen; die Abdeckung ist eine **im Code lesbare Eigenschaft**, keine durchgesetzte Invariante. Die Produktaussage („ein verworfenes CR3 behält Orientierung, Metadaten, Identität") hat **nie** daran gehangen. Preis: **neun** mutationsbewiesene Widerstandsfälle (Menge in §3.2.1 **definiert**; eine frühere Fassung nannte zehn und zählte die fehlende Datei mit — die ist durch Regel 3 weiterhin gedeckt) |
| Fassung von **Regel 4** mit dem Schlusssatz „der Zustand ist nicht mehr ausdrückbar" | Durch Messung widerlegt: eine Regel, die einen **Weg** abschließt statt einer **Klasse**, ist eine Absichtserklärung (`DoD.md` §9). Zurückgenommen **an der Fundstelle**, nicht fortgeschrieben | Regel 4 als **Zusage** zurückgenommen, siehe oben |
| `check_plan_format.sh`-Ausnahme für abgehakte Entscheidungen | Der Eigentümer hat am 2026-09-28 bestätigt: **verifiziert abgeschlossene Sachen werden aus `Agents.todo.md` entfernt** — auch Entscheidungen. Eine Ausnahme hätte das Gate genau dort geschwächt, wo es am ehesten sichtbar bleiben soll | Kein Checkbox-Syntax; der `Stand`-Wert `umgesetzt`/`erledigt` ist im Skript ein Formverstoß |
| Defekt-Claim „ein Regler-Drag verliert den entprellten Sidecar-Save" (`PLAN-ORPHAN-1`, als Waise aus dem Plan entfernt, 2026-10-02) | Widerlegt, und **mit zwei falschen Quellenangaben** überliefert. (1) `full_render_debounce_remaining` steht in `crates/lumina-gui/src/render_tick.rs` -> `full_render_debounce_remaining` — **nicht** `29–35`, das ist der Doc-Kommentar (6 Zeilen daneben). (2) `crates/lumina-gui/src/slider.rs` **bewaffnet `pending_full_render` überhaupt nicht**: `grep -n "pending_full_render" crates/lumina-gui/src/slider.rs` liefert **null Treffer**; der Bewaffner sitzt in `crates/lumina-gui/src/dirty.rs` (`mark_dirty`), erreicht aus einem Slider-Drag über `set_adjustment` → `mark_recipe_dirty` → `mark_dirty`. (3) Die Messbehauptung „am 4-Frame-Exposure-Drag gemessen" hat **keinen** Test im Baum, der `dragged()` fährt. Die Wirkungsrichtung der Aussage stimmt, die angegebene Datei nicht | Der Defekt existiert nicht und ist **nicht** zu fixen; `pending_full_render = true` steht an vier Stellen (`crates/lumina-gui/src/dirty.rs` → `mark_dirty`, `crates/lumina-gui/src/lib.rs` → `set_adjustment`, `crates/lumina-gui/src/render_schedule.rs` → `schedule_render`, `crates/lumina-gui/src/warmup.rs` → `maybe_run_startup_warmup`), ein veralteter `last_edit_time` macht den Commit **eifriger**, nicht strandend. Ein Reproduktionsfall wurde nie belegt |
| Vorziehen des **Crop**-Schritts vor die Geometrie-Kette/die Tonalstufe (`PIPELINE-CROP-EARLY-9`, geschlossen 2026-10-16) | Belegte **Totalordnung** `SpotHeal ≺ Adjustments ≺ Lens ≺ AutoFill ≺ Perspective ≺ Expand ≺ Crop ≺ LensBlur ≺ Masks` — jede Nachbarbeziehung ist durch eine Koordinaten-/Semantik-Invariante erzwungen: H1 expand→crop (`crates/lumina-core/src/render.rs` → `render_frame_from_base_with_generative_and_denoise`, `crates/lumina-core/src/lib.rs` → `apply_geometry`, `crates/lumina-core/src/generative.rs` → `materialize_canvas_for_crop`), H2 Lens/Perspective→Crop über den Default-Content-Crop (`crates/lumina-core/src/lib.rs` → `apply_geometry`, `crates/lumina-core/src/crop_max_rect.rs` → `maximum_content_rect`), H4 Tonal-vor-Geometrie (`feature/architecture/pipeline.md:41–42`). Ein Crop vor Tonal **ändert Bytes** bei aktivem Warp (14,58 % Kanäle, `feature/quality/pipeline-crop-early-2026-10-13.md:38`). Es existiert kein tauschbares Nachbarpaar | Crop bleibt **direkt nach `composite_expand`** (`crates/lumina-core/src/render.rs` → `composite_expand`). Die 85 %-Gewichtung der Tonalstufe ist am `perf/baseline.json` **nachgerechnet** (85,2/85,7/85,8 %), gilt aber nur im **geometriefreien** Bench-Rezept; die heutigen 9–10 % Crop-Ersparnis (`perf/crop_pipeline_measurement.json`) stammen aus einem **Frame-Klon**, nicht aus der Tonalstufe. Analyse durch einen deepseek-v4.1-Flash-Subagenten, abgenommen durch den Eigentümer am 2026-10-16 |

## Arbeitsregeln

- Vor jeder Umsetzung `Agents.md`, `feature/README.md` und das betroffene
  Feature-Dokument lesen.
- Wenn Code und SOLL-Zustand widersprechen, zuerst den Zielzustand klären und
  dokumentieren.
- Jede Aufgabe erhält bei Delegation eine Feature-ID, einen klaren Umfang und
  Abnahmekriterien.
- Der Build-Agent delegiert die Implementierung und anschließend die Prüfung an
  unterschiedliche Subagenten.
- Implementierungs-Agenten werden als `general`-Agenten delegiert (nicht als
  `build`-Agenten); Verifikation läuft immer über einen davon unabhängigen
  `general`-Agenten.
- Der unabhängige Verifizierungs-Agent muss Korrektheit und Testabdeckung
  bestätigen, bevor die Aufgabe aus dieser Datei entfernt wird.
- Eine fehlgeschlagene Verifizierung lässt die Aufgabe offen und erzeugt eine
  konkrete Folgeaufgabe.

## Offene Tasks — Legende der drei Blöcke und Releaseplan

Alle offenen Aufgaben sind in drei Blöcke gegliedert. Innerhalb jedes Blocks
gilt die Sortierung `[PRIO: hoch]` → `[PRIO: mittel]` → `[PRIO: niedrig]`;
die Priorisierung bewertet technische Tragweite/Risiko (kritische
Korrektheits-Bugs = hoch, Kosmetik/Doku = niedrig). Stand 2026-09-26
(nach Merge von `origin/main` @ `d392a51`, nach BESTANDEN von
`THUMB-HASH-PERF-35`, `LENSFUN-DB-33` und `LENSFUN-CALLER-37`).

**Die Anzahl offener Tasks wird hier bewusst nicht hingesrieben.** Eine
festgeteilte Zahl wird bei *jeder* Task-Ergaenzung falsch, und eine falsche
Zahl im Plan ist schlimmer als keine — sie bindet Disposition auf einen
Zustand, den es nicht gibt. Stattdessen bei Bedarf selbst zaehlen:

```sh
grep -c '^- \[ \]' Agents.todo.md          # offene Tasks
grep -c '^- \[ \].*PRIO: hoch' Agents.todo.md   # davon Prioritaet hoch
```

Stand der Zaehlung bei der letzten Pflege: siehe Git-Historie dieses
Abschnitts.

**User-Entscheid 2026-10-16 — Wiedereingliederung der Headless-Tasks.**
Die offenen Tasks (GPU/Hardware-geblockte und headless-arbeitbare) stehen
wieder gemeinsam in dieser Datei. Die separate `Agents.headless-todo.md` ist
entfernt; die headless Maschine arbeitet aus dieser Datei. Zaehlung:

```sh
grep -c '^- \[ \]' Agents.todo.md                 # offene Tasks (gesamt)
```

Der Abschnitt `Releaseplan` ordnet jede Task-ID genau einer Version zu
(1.0 = MVP, 1.5, 2.0, 2.5, nie) — für Mensch und Maschine lesbar.

**User-Anweisung 2026-09-24:** Nach dem aktuellen Release-Block sollen
Future-Release-Tasks proaktiv vorgezogen werden, sobald keine aktuelle
Abhängigkeit und kein unüberwindbares Hardware-/User-Gate blockiert. Die
Releaseplan-Zuordnung und die bestehenden Abnahme-Gates bleiben dabei unverändert.

## Umgebungsgrenze: headless Maschine ohne GPU (Build-Agent, 2026-09-29)

**Was eine headless Maschine ohne Adapter prüfen kann: den CPU-Pfad vollständig.**
`cargo test -p lumina-gui --lib` laeuft dort ohne wgpu-Adapter grün; die
gesamte Rezept-/Render-/Sidecar-Logik, alle klickbaren headless GUI-Tests und
alle CLI-/MCP-Pfade sind headless abgedeckt. Ein erster Lauf auf einer solchen
Maschine (LibRaw 0.22.2 wie im CI-Image, 5 `#[ignore]`-Tests) ergab
`919 passed / 2 failed / 5 ignored` — die zwei roten Tests lagen in neuem
Testcode, nicht im Produktionspfad, und sind im selben Lauf behoben worden.

**Was dort ausdruecklich NICHT behauptet wird:** GPU-/wgpu-Paritaet, der
Paint-Schritt (Shader-Kompilierung, MSAA, Textur-Sampling, Treiberfehler) und
jede Aussage ueber VRAM-Pixel. Das ist eine **benannte Luecke mit
Hardware-Gate**, kein gruener Pass und kein stiller Ersatz (`DoD.md` §10).
Betroffen sind `GPU-PARITY-HW-28`, `R5-DUST-23-FOLLOWUP`, `R5-BRUSH-24`,
`R5-MASKVIS-25` und die kittest-Goldens.

**Verifikations-Rundlauf 2026-09-27 (unabhängig, PASS) — `LUMINA-GPU-TESTS-38`:
`lumina-gpu`-Tests in CI, zweiteilig.** **Zuordnung:** die Position war eine
**CI-Lücke** und ist seit der Verifikation **erledigt und verifiziert** — sie
steht hier als **Nachweis**, nicht als offene Aufgabe (das Board führt
geschlossene Positionen als Prosa, nie als `- [x]`; `grep -c '^- \[x\]'
Agents.todo.md` ist 0). Sie stand bis `3d02895` („LUMINA-GPU-TESTS-38:
erledigt, am CI-Log verifiziert") als offene Position im Board und wurde dort
**entfernt**, nicht abgehakt; der Nachweis aus dieser Zeit lebt in der
Git-Historie. Die Implementierung lag bereits vor (`add5d5f`, 2 Commits); offen
war allein der Nachweis. Gemessen wurde auf einem **GPU-losen Host**, beide
Teile der Aufgabe.

**(a) Die 58 adapterfreien Unit-Tests laufen in CI.** `ci.yml:239` lautet jetzt
`cargo test --workspace --all-targets --exclude lumina-gui --exclude
lumina-sidecar --exclude lumina-bench` — `lumina-gpu` ist **nicht** mehr
ausgeschlossen. **Gemessen am 2026-09-27:** `cargo test -p lumina-gpu` meldete
**70 passed / 0 failed / 28 ignored**; die 70 enthielten die 56 lib-Unit-Tests
(`geometry`, `validate`, `stages`, `tiling`, `lens_blur`, `lensfun` plus
`src/lib.rs`-Blöcke). **Stand 2026-10-22 (CI-Lauf `39733087689`, Job „Rust tests
(excluding GUI)"):** `cargo test -p lumina-gpu` meldet **74 passed / 0 failed /
32 ignored**; die 74 enthalten die **58** lib-Unit-Tests. 74 ≥ 58 ✓.

**(b) Die 28 `ignored` sind genau die adapterpflichtigen Tests.** Der Briefkasten
ist direkt gemessen, nicht aus der Testliste abgeleitet: `cargo test -p
lumina-gpu --features gpu-adapter-tests --no-fail-fast` auf diesem Host **ohne**
Adapter ergibt `passed=70  FAILED=28  ignored=0` mit **28 Härte-Panics**.
Dieselben 28, die default `ignored` gemeldet werden, sind es, die im Opt-in hart
scheitern — die Mengen sind komplementär, nicht nur ähnlich. Damit ist nichts als
gedeckt gemeldet, was nichts prüft ✓.

**(c) Die `ci.yml`-Begründung ist durch eine gemessene Aussage ersetzt** (Zeilen
138–154): benennt **74/32**, die **58** `src/`-Unit-Tests, die **32**
adapterpflichtigen Integrationstests, den Opt-in-Befehl und den Umstand, dass ein
GPU-loser Runner `#[ignore]`d niemals als bestanden zählt ✓.

**(d) Nicht-Vakuumität** — hier ist die Abnahme sprachlich von „ein Test"
formuliert; was tatsächlich existiert, ist der **Mechanismus** (`gated_test!` in
`tests/support/mod.rs:39` setzt `#[cfg_attr(not(feature = "gpu-adapter-tests"),
ignore)]` an **eine** Stelle statt an 28 Tests, `require_adapter` `:60` ersetzt
das frühe `return` durch ein `assert!`). Die Nicht-Vakuumität ist deshalb
**gemessen, nicht asserted-by-a-test**: auf einem adapterlosen Host sind es 0
bestandene statt 28 (default) bzw. 28 Fehlschläge statt 28 stille Pässe
(Opt-in), Exit-Code 101. Das ist der stärkere Beleg, ersetzt aber keinen
committeten Meta-Test über das Gate selbst; die Formulierung wird hier nicht
als wörtlich erfüllt ausgegeben.

**(e) Keine Assertion entfernt oder geschwächt.** Die alten Skip-Zweige stehen
wörtlich erhalten (`tests/stages.rs:123-126`, `:229-232`, `:266` —
`eprintln!("{SKIP_MESSAGE}"); return;`), sie sind nur nach dem vorangestellten
`assert!` statisch unerreichbar, damit die Skip-Meldungen dokumentiert bleiben.
Kein Test wurde gestrichen; `init_warning.rs` (der „no silent fallback"-Vertrag)
besteht unverändert.

**Zahlen im Aufgabentext sind überholt und bleiben als Vorher-Stand stehen:**
der Text nennt 98 Tests / 42 adapterpflichtige, gemessen sind am 2026-09-27
70 / 28. (31 `gated_test!`-Aufrufe, davon 3 in `lensfun_tca_skip.rs` und
`parity.rs` hinter dem `lensfun`-Feature, das unter Default-Features nicht
kompiliert.) Die 42/98 sind der Zustand **vor** der Umsetzung, die 70/28 der
danach; die Doku `ci.yml:138-152` führte die **damaligen**; **seit 2026-10-22
nennt sie die heutigen (74/32)**.

**Bleibende Hardware-Grenze (unverändert, durch diese Aufgabe nicht schließbar):**
der wgpu-**Paint**-Schritt (Shader-Kompilierung, MSAA, Textur-Sampling,
Treiberfehler) braucht einen echten Adapter und bleibt lokales macOS-Gate.
`GPU-PARITY-HW-28`, `R5-DUST-23-FOLLOWUP`, `R5-BRUSH-24` und `R5-MASKVIS-25`
tragen denselben Hardware-Gate unabhängig davon und bleiben offen. Mit dieser
Messung ist zusätzlich belegt, dass der Default-Lauf auf einem GPU-losen Runner
**nicht** vakuum grün ist — die 28 fehlen als `ignored` sichtbar, statt als
bestanden zu zählen.

**Stand der Belege am 2026-10-22 (nachgetragen, HEAD `923a0c0`; der Lauf selbst
wurde **nicht** wiederholt — auf diesem Host fehlt die Rust-Toolchain,
`cargo fmt` endet mit Exit 127):** die **Behauptungsstruktur der Messung vom
2026-09-27 gilt unverändert**, die **Zahlen und Zeilenangaben sind teilweise
überholt.**

- `add5d5f` existiert unverändert (`git log -1 --format='%s %ad'` →
  „LUMINA-GPU-TESTS-38: 56 echte GPU-Tests in CI, Skip ehrlich", 2026-09-26);
  8 Dateien, davon **0 Produktcode** unter `crates/lumina-gpu/src/`. Der zweite
  Commit `6af70dd` („GPU-GATING-FMT-41: gated_test! entfernt, fmt-Abdeckung
  zurück") hat den Mechanismus aus (d) später verändert — das ist der Grund,
  warum (d) heute anders aussieht.
- **`ci.yml:239`** — die zitierte `cargo test`-Zeile steht **heute in Zeile 376**
  (`cargo test --workspace --all-targets --exclude lumina-gui --exclude
  lumina-sidecar --exclude lumina-bench`; `lumina-gpu` weiterhin **nicht**
  ausgeschlossen ✓). Zeile 239 trägt heute einen Kommentar zum
  CLI-Feature-Build-Gate.
- **`ci.yml:138-154`** — der Begründungsblock steht heute in **Zeilen 217-231**.
  Er nannte **70/28**, die **56** `src/`-Unit-Tests und die **28**
  adapterpflichtigen Tests; **die Zahlen darin waren überholt** — **seit
  2026-10-22 korrigiert** (siehe unten): der Block nennt heute **74/32**, die
  **58** `src/`-Unit-Tests und die **32** adapterpflichtigen Tests; der
  Mechanismus und das Opt-in-Kommando stehen unverändert.
- **Zahlen heute** (aus dem CI-Log des grünen Laufs `39733087689` zu `923a0c0`,
  2026-10-22, Job „Rust tests (excluding GUI)" — **kein** lokaler Lauf; **identisch**
  zur Messung vom 2026-10-21, Lauf `39670086731` zu `8c9df57`): `lumina_gpu (lib)`
  **58 passed / 0 failed / 0 ignored** (statt 56); Summe über alle `lumina-gpu`-Binaries
  **74 passed / 0 failed / 32 ignored** (statt 70/28). Gegengezählt: `golden`
  5/2, `init_warning` 1/0, `lensfun_map` 2/1, `lensfun_tca_skip` 0/0,
  `local_mask_parity` 2/4, `parity` 5/22, `stages` 1/3. **Aussage (b) gilt damit
  unverändert**: die `ignored` sind genau die adapterpflichtigen Tests — in der
  Quelle stehen heute **35** `#[cfg_attr(not(feature = "gpu-adapter-tests"),
  ignore)]`-Attribute, davon **3** hinter `#[cfg(feature = "lensfun")]`
  (der Modul-Deklaration `mod parity_lensfun` in `crates/lumina-gpu/tests/parity.rs`,
  dem Test `f2_vignetting_only_corrector_matches_cpu_oracle` in
  `crates/lumina-gpu/tests/lensfun_map.rs` und der ganzen Datei
  `crates/lumina-gpu/tests/lensfun_tca_skip.rs` — dort
  `#![cfg(all(feature = "gpu", feature = "lensfun"))]`) → **32** unter
  Default-Features. Die 35 statt 31 sind neue Adaptertests aus
  `GPU-RENDER-MASK-19` (`local_mask_parity.rs`, `parity_generative/`).
- **(d) ist heute anders formuliert, weil der Mechanismus heute ein anderer
  ist:** `gated_test!` gibt es **nicht mehr** (entfernt in `6af70dd`); das
  `ignore`-Attribut steht heute an **35** Stellen einzeln statt an einer;
  `require_adapter` steht heute in
  `crates/lumina-gpu/tests/support/mod.rs:44` (damals `:60`). Die **Wirkung** ist
  dieselbe — `require_adapter` ist ein `assert!` statt eines `return false`, ein
  adapterloser Runner zählt also keinen stillen Frühaufstieg als bestanden.
- **(e) gilt inhaltlich, mit verschobenen Zeilen:** die Skip-Zweige in
  `crates/lumina-gpu/tests/stages.rs` stehen wörtlich erhalten, heute in
  **`:124-127`**, **`:231-234`** und **`:269`** (damals `:123-126`, `:229-232`,
  `:266`); der vorangestellte `assert!` steht weiterhin bei `:229`.
- **Bleibende Hardware-Grenze unverändert** — der wgpu-Paint-Schritt braucht
  weiterhin einen echten Adapter; die oben genannten Hardware-Gates bleiben
  offen.

**Die Grenze ist nicht die einzige offene Sache — das wird hier ausdruecklich
nicht behauptet.** Eine headless Maschine ohne GPU loest keinen Task aus
Block B und ersetzt keinen manuellen Lauf:

| Art der Luecke | Beispiel | Was sie braucht |
| --- | --- | --- |
| **Adapter/Hardware** | `GPU-PARITY-HW-28`, die kittest-Goldens | echter wgpu-Adapter (Metal/Vulkan) |
| **Eigentuemer-Entscheid** | derzeit keine offen — `NAMING-F1` und `JSON-FLOAT-ROUNDTRIP` wurden am 2026-10-02 entschieden | Antwort des Eigentuemers; ein Build-Agent erfindet keinen Produktnamen |
| **Referenz-Fixture** | `GOLDEN-BASELINE-32`, `MCP-PARITY-C` | gepinnte Referenzmaschine bzw. echte RAW-Fixture-Reihe |
| **Menschlicher Lauf** | `F-103-N6` | `RUST_LOG=trace` GUI-Lauf nach R5-LOG-1 durch einen Menschen |

**Konsequenz fuer die Abnahme:** ein Task gilt erst als abgeschlossen, wenn die
zugehoerige Abnahme auf der **jeweils zutreffenden** Umgebung gemessen wurde. Ein
CPU-gate, das gruen ist, deckt die GPU-Paritaet desselben Tasks **nicht** ab —
die beiden Aussagen werden getrennt gefuehrt und nie zusammengefasst. Umgekehrt
gilt: eine fehlende GPU-Paritaet invalidiert ein gruenes CPU-Ergebnis nicht.

**Handwerkliche Folge fuer Agenten auf so einer Maschine:** `#[ignore]`-Tests
mit `#[cfg(feature = "gpu")]` sind dort **echt lauffaehig**, wenn sie keine
Adapter-routen betreten (die Testsuite selbst benennt das so). Ein Test, der an
`CustomNativeAdapterSelectionError("No adapter found")` scheitert, ist **kein
Befund am Produkt** — das ist die Hardware-Grenze in ihrer eigenen Form. Ebenso
ist ein OOM/SIGKILL beim Codegen **kein** Defekt: er ist eine RAM-Erschoepfung
der Bauumgebung und wird durch Wiederholen mit freiem Speicher neu gemessen, nicht
durch Aendern am Code.

- **Block A — „Vor dem nächsten manuellen GUI/User-Test umsetzbar“:** alles,
  was ohne Rückfrage direkt umgesetzt werden kann und nicht von einem
  manuellen Test abhängt (Reihenfolge: PRIO hoch → mittel → niedrig).
  **Korrektur 2026-09-25:** Block A war zum Einführen nicht mehr
  vollständig ohne User-Interaktion abarbeitbar. Die fünf
  PRIO-hoch-GUI-Tasks (`R5-BRUSH-24`, `R5-MASKVIS-25`,
  `R5-DUST-23-FOLLOWUP`, `MASK-LOCAL-P0`, `MASK-LOCAL-P1.1`) waren
  implementiert und code-seitig verifiziert, aber alle durch
  **ein** undefiniertes Gate blockiert: einen Lauf auf echter Hardware
  gegen eine definierte Referenz-Umgebung. Dieses Gate ist inzwischen
  als `GOLDEN-REF-30`/`GOLDEN-FIXT-31`/`GOLDEN-BASELINE-32` explizit
  zerlegt; siehe [GUI-Test-Arbeit: Gate-Struktur](#gui-test-arbeit-gate-struktur).
  Die erneut Block-A-fähigen, reinen Code-/Doku-Aufgaben stehen
  unter **PRIO: mittel**/**niedrig**.
- **Block B — „Offene Rückfragen“:** Tasks, bei denen eine User-Entscheidung
  oder Klärung fehlt (Produkt-, Naming-, Lizenz-/Schema- oder Übernahme-
  Fragen). Dieser Block blockiert Block A nicht.
- **Block C — „Nach dem nächsten manuellen GUI-Test“:** Tasks, die erst nach
  dem nächsten manuellen GUI-Test sinnvoll/erforderlich sind (Verifikations-
  und Abschluss-Tasks, die auf Testergebnissen aufbauen).

### Headless-TODO — welche offenen Tasks ohne GPU/Display/Mensch abarbeitbar sind (Stand 2026-10-15)

**Rolle dieser Liste: abgeleitete Sicht, kein zweiter Task-Eigentuemer.** Der
verbindliche Aufgabentext steht **ausschliesslich** in den Blöcken A/B/C oben
(dort mit Nummer, Abnahme und Abgrenzung). Diese Tabelle sagt nur, **ob** ein
offener Task auf einer Maschine ohne wgpu-Adapter, ohne Display und ohne
menschlichen GUI-Lauf abarbeitbar ist, und mit welchem Gate. Sie darf deshalb
auch **keine Checkboxen** tragen — sonst entstuenden Doppel-Tasks, die das
Plan-Format-Gate mitzaehlt. Wird ein Task nach bestandener Verifikation aus dem
Plan entfernt, wird **seine Zeile hier mit entfernt**; die Liste wird bei jeder
Task-Aufnahme/-Entfernung mitgefuehrt (sonst driftet sie und wird durch die
naechste Messung widerlegt).

**Maßstab:** „headless abarbeitbar“ heisst, dass **Implementierung und Gate**
ohne Adapter, ohne Display und ohne Menschen auskommen. Ein Task, dessen
Verifikation einen echten Metal-/Vulkan-Adapter oder einen sichtbaren GUI-Lauf
braucht, ist **nicht** headless — auch wenn sein Code headless geschrieben
werden kann. Reine Struktur-/Audit-/Doku-Tasks sind der Regelfall.

| Task-ID | headless? | Headless-Gate (Kommando) | Was sonst noetig ist |
| --- | --- | --- | --- |
| `MERGE-DEGHOST-RATCHET-67` (427) | **ja** | `cargo test -p lumina-merge`, `sh scripts/check_file_sizes.sh` | — |
| `GUI-LIB-SPLIT-65` (428) | **ja** | `cargo test -p lumina-gui --lib`, `sh scripts/check_file_sizes.sh` | — |
| `SIDE-LOCK-STALE-FLAKE-66` (423) | **ja** | `cargo test -p lumina-sidecar` (deterministischer Rotbeweis zuerst) | — |
| `TEST-AUDIT-36` (415) | **ja** | gezielte `cargo test`-Laeufe + Mutationen (exklusiver Baum) | — |
| `CI-WATCH-1` (416) | **ja** | `gh run list --branch main` / `gh run watch` | — |
| `CI-CLI-GPU-TESTBUILD-68` | **ja** | `sh scripts/check_cli_features.sh`, `cargo check -p lumina-cli --features gpu,lensfun --all-targets` | — (kein Adapter: die neuen CLI-GPU-Tests sind nicht adapterpflichtig) |
| `GPU-RENDER-MASK-19` (361) | **Headless-Seams unabhängig verifiziert (2026-10-23)** | CPU-Preview-/Layer-Pairing-Helper und live-Route/Freshness-Helper-Tests im GUI-Crate; sie rufen `render_draft_tick` nicht auf und belegen keine Closure-Verdrahtung | Adapter-Present für P0-Exposure unabhängig bestätigt (2026-10-25), Liefer-/CI-Beleg und Grenzen in `feature/architecture/pipeline.md` §GPU; kein Nachweis sämtlicher Adjustment-Klassen oder manueller Fensterabnahme |
| `GPU-RENDER-NONDETERMINISM-69-HW` | **nein** | — | Adapter-Nachweis (Metal/Vulkan): `cargo test -p lumina-gpu --features gpu-adapter-tests` plus GUI-GPU-Tests (Regel `GPU-E2E`) |
| `GPU-MASKPLANE-CLEAR-REMEASURE` (362) | **nein** | — | ruhige Session **mit** Adapter (Fremdlast verfaelscht die Messung) |
| `MCP-PARITY-C` (408) | **Headless-Slices verifiziert; RAW-Serienfixture fehlt** | Historisch T11–T20 und T19–T25; aktueller Lauf `cargo test -p lumina-cli --test matrix_mcp_parity` → **4 passed**. T26-Gate wurde nicht ausgeführt; die echte Serien-Fixture fehlt. | echte RAW-Mehrbild-Belichtungsreihe; siehe `MCP-PARITY-C-RAW-SERIES` |
| `LRPAR-G09-CULL-IMPL-25` (418) | **Headless-Slice verifiziert; kein aktueller GUI-Goldenlauf** | `cargo test -p lumina-cull` → **45 passed**; `cargo test -p lumina-cli --test cull_e2e` → **10 passed**; GUI/headless-Verifikation ist im SOLL/Task historisch belegt. | optionale ONNX-Stufe-2-Gewichte und Native-kittest-Golden; siehe `LRPAR-G09-CULL-REST` |
| `LRPAR-G14-DENOISE-IMPL-20` (417) | **Headless-Slice verifiziert; echte Gewichte fehlen** | `lumina-onnx --test denoise` **8**, Core `denoise` **21**, Sidecar `denoise` **10**, CLI `denoise_e2e` **13**, GUI `denoise_gui::tests` **11** passed; reale Gewichte nicht vorhanden/integriert. | F-078 Lizenz/Provenienz/Hash-Pin und ONNX-Modelllauf; siehe `LRPAR-G14-DENOISE-WEIGHTS` |
| `R5-DUST-23-FOLLOWUP` (375) | **Headless-Funktion belegt; historischer Lauf** | Tasktext nennt bestandene unabhängige Funktionsverifikation; kein erneuter Lauf an aktuellem HEAD behauptet | eigener GUI/Hardware-Rest `R5-DUST-23-GUI-REST` |
| `R5-BRUSH-24` (376) | **Headless-Funktion belegt; historischer Lauf** | Tasktext nennt CPU/no-GPU, Atomizität, Overlay, Action-Audit und Struktur/Golden grün; kein aktueller Lauf behauptet | eigener GUI/Hardware-Rest `R5-BRUSH-24-GUI-REST` |
| `R5-MASKVIS-25` (377) | **Headless-Funktion belegt; historischer Lauf** | Tasktext nennt unabhängige State-/CPU-/Overlay-/Audit-Verifikation; kein aktueller Lauf behauptet | eigener GUI/Hardware-Rest `R5-MASKVIS-25-GUI-REST` |
| `F-103-N6` (513) | **nicht headless** | sichtbarer GUI-Lauf durch Menschen ist der eigentliche Produktabnahmescope; headless Tests ersetzen ihn nicht | `RUST_LOG=trace cargo run -p lumina-gui` nach R5-LOG-1; eigener Lauf bleibt erforderlich |
| `F-103-N6-GUI-COVERAGE-27` (482) | **nicht headless** | vollständige manuelle GUI-/Persistenz-Checks sind Menschen-/Display-Gate | `RUST_LOG=trace cargo run -p lumina-gui`; Testfälle/Logs gemäß Task |
| `R2-GUIMOD-04b` (510) | **nicht headless (abhängig)** | Entscheidung benötigt 04a-Messwerte aus `F-103-N6`, die hier nicht vorliegen | nach GUI-Lauf/Messwerten eigene Akzeptanz in bestehendem Task |
| `R2-GUIMOD-04c` (511) | **nicht headless (abhängig)** | Histogramm-Implementierungsentscheidung benötigt 04a-Messwerte aus `F-103-N6` | nach GUI-Lauf/Messwerten eigene Akzeptanz in bestehendem Task |
| `UI-VISION-REVIEW-70` | **nein** | — | sichtbarer GUI-Lauf mit Metal-Adapter; Screenshot- und Klick-Recht der Terminal-App |

**Klammerzahlen:** Die Zahl in Klammern stammt aus der Erstfassung dieser Tabelle
(`0795465`), ist im Dokument sonst nirgends belegt und wird **nicht**
fortgeschrieben. Neu aufgenommene Zeilen tragen deshalb **keine** Klammerzahl —
eine erfundene Nummer waere laut Legende schlimmer als keine.

**Reihenfolge unter headless:** zuerst, was ein Gate gefaehrdet oder ein `main`-Rot
verursachen kann (`CI-CLI-GPU-TESTBUILD-68`, `MERGE-DEGHOST-RATCHET-67`,
`SIDE-LOCK-STALE-FLAKE-66`), dann was andere Tasks freigibt
(`GPU-RENDER-MASK-19`-Verdrahtung), dann der GPU-Nichtdeterminismus
(`GPU-RENDER-NONDETERMINISM-69-HW` — fuer den Rotbeweis ist ein Adapter noetig), dann
die fortlaufenden Audits/Doku (`TEST-AUDIT-36`, `CI-WATCH-1`). `GUI-LIB-SPLIT-65`
ist PRIO niedrig und darf den GPU-E2E-Pfad **nicht** verdraengen.

## Releaseplan (User-Entscheide 2026-09-03; MVP = 1.0)

Maschinenlesbar: Die Tabelle `Version | Task-ID | Goal | Stichwort` ist die
verbindliche Zuordnung. Jede Task-ID kommt genau einmal vor; Ausführungsort
bleiben Block A/B/C. Tasks ohne expliziten User-Versionsentscheid gelten als
MVP-Annahme (1.0) und können per User-Entscheid umgebucht werden.
`fortlaufend` = ab 1.0 aktiv, gilt für alle Releases (CI-Strategie im Task).

**Zwei Regeln, die `PLAN-TABLE-COVERAGE-44` am 2026-09-27 praezisiert hat.** Beide
sind aus einem gemessenen Widerspruch entstanden, nicht aus einer Absicht:

1. **`Goal` wird nicht in der Planung geraten.** Fuer Tasks, die zum Zeitpunkt des
   Releaseplans noch nicht existierten, steht in der Goal-Spalte
   `offen — bei der Task`. Die Zuordnung wird **im Zuge der Bearbeitung** der
   jeweiligen Task getroffen und mit Begruendung nachgetragen. Die Version ist
   demgegenueber **belegbar** — mit einer Einschränkung, die dieser Absatz
   zuvor falsch darstellte. **Korrektur 2026-09-29 (Build-Agent, nachgemessen):**
   die alte Fassung behauptete pauschal, die Version stehe im Task-Text. Das trifft
   auf **29** der 41 Zeilen zu und auf **12** nicht. Die 41 Zeilen zerfallen so:
   **23** mit eigener Task und Versionsangabe im Task-Text; **7** mit eigener
   Task, aber **ohne** jede Versionsangabe im Text (`F-103-N6-GUI-COVERAGE-27`,
   `GPU-PARITY-HW-28`, `R5-DUST-23-FOLLOWUP`, `R5-BRUSH-24`, `R5-MASKVIS-25`,
   `CI-WATCH-1`, `TEST-AUDIT-36` — nachgemessen, in keinem dieser Texte kommt
   `Release` oder eine nackte `1.0`/`1.5`/`2.0` vor); **6**, deren Task nur unter
   einer Combined-ID existiert (`R2-GUIMOD-04b`/`04c`, `MASK-LOCAL-P1.2a`…`d` —
   die Version steht im **Elterntask**); **3**, die in der offenen Frage
   `GPU-RENDER-ID-BRIDGE-52` gebündelt sind; **2** Platzhalter (`—`, nie-Ziel,
   ohne Version). **Bei den 7** stammt der Spaltenwert aus einer **anderen**
   Quelle: der **Vorgabe** dieses Abschnitts — `1.0` als MVP-Annahme mangels
   User-Versionsentscheid, `fortlaufend` für ab 1.0 dauerhaft aktive Aufgaben.
   Beide Quellen sind belegbar, aber sie sind **nicht dieselbe**; eine Regel, die
   eine Quelle für alle behauptet, ist an zwölf Zeilen falsch.
2. **Ein Eintrag ohne Task ist erlaubt** und bedeutet *geplant, noch nicht
   begonnen*. Die Tabelle ist der Release-**plan**, die Task-Liste ist der
   Arbeitsplan; ein geplanter Releasepunkt muss nicht schon als umsetzbare
   Task existieren. Eine leere Task-Zeile anzulegen waere regelwidrig, weil
   `Agents.todo.md` ausschliesslich **offene, umsetzbare** Aufgaben enthaelt.
   **Korrektur 2026-09-29 — der alte Satz war falsch.** Die frühere Fassung
   führte **neun** Zeilen als Beleg an (`GPU-RENDER-*-19`, `LRPAR-G09-SORT-09`,
   `LRPAR-G03-MASKGROUP-03`, `LRPAR-G12-FACE-*-2x`, `LRPAR-G15-STACK-15`) und
   schrieb, `git log -S` habe belegt, dass sie **nie** als offene Task
   existiert hätten. Geprüft wurde aber nur **eine** ID
   (`git log -S 'PRIO: hoch] GPU-RENDER-MASK-19'`), und die Aussage wurde auf
   alle neun verallgemeinert. Nachgemessen per Lese-Audit: für die **fünf**
   LRPAR-IDs ist sie **widerlegt** — `a3f797e`, `dab4aa8`, `7328da7` und
   `e707ed4` legten die Tasks an, `23c9595`, `c76e65c`, `318a8dc` und `a5968a3`
   schlossen sie nach **BESTANDEN** ab und entfernten die Task-Zeile. Diese fünf
   Zeilen waren keine geplanten Punkte, sondern **Reste erledigter Tasks**; sie
   sind ersatzlos aus der Tabelle entfernt. Was bleibt, ist die Begründung für
   `GPU-RENDER-*-19` — dort trifft „geplant, noch nicht begonnen" zu: für
   `DENOISE` belegt der Code die Lücke ausdrücklich
   (`denoise_ai (not GPU-wired)`, `crates/lumina-gpu/src/lib.rs` -> `unsupported_gpu_stages`); für
   `PREVIEW`/`EXPORT` ist sie **nicht** ausgeschlossen, aber unbelegt.
   **Zweiter Durchgang 2026-09-29 (Lese-Audit + eigene Nachmessung):** Von den
   vier `GPU-RENDER-*-19`-Zeilen ist eine **Dublette** und drei sind ungeklärt.
   `GPU-RENDER-DENOISE-19` ist **(D)**: die Arbeit läuft unter
   `LRPAR-G14-DENOISE-IMPL-20` (eigene Task-Zeile, Release 2.0), und genau diese
   ID nennt `crates/lumina-gpu/src/lib.rs` -> `unsupported_gpu_stages` als Beleg der Lücke — der Code führt
   `denoise_ai (not GPU-wired)` mit dieser Task, nicht mit `GPU-RENDER-DENOISE-19`.
   Die Zeile ist deshalb entfernt; **eine eigene Task dafür anzulegen hätte
   Arbeit doppelt erfasst.** Das Lese-Audit hatte sie als (C) „offen" eingestuft —
   das war **falsch**, und die Korrektur folgt dem Codeverweis, nicht der
   Vermutung. `GPU-RENDER-MASK-19` ist **nicht** durch `MASK-LOCAL-P0`…`P1.2d`
   abgedeckt: diese Tasks bauen das **Rezept-Datenmodell** (Tone Curves, HSL,
   Presence, Detail), während die Zeile den **GPU-Masken-Pixelpass** meint; die
   vorhandenen `GPU-MASKPLANE-BRUSH-50`/`-CLEAR-PERF-51` betreffen Brush-Upload
   und Plane-Clear-Performance. `GPU-RENDER-PREVIEW-19` und `-EXPORT-19` waren
   ungeklärt (die Fähigkeit existiert — `gpu_present_if_ready`, GPU-first
   CLI-Render — aber ohne ID-Brücke). **AUFLÖSUNG 2026-10-02
   (`GPU-RENDER-ID-BRIDGE-52`, Audit + eigene Nachmessung):** `PREVIEW` und
   `EXPORT` sind **Dubletten** verifizierter Arbeit — `PREVIEW` unter
   `GUI-WGPU-PRESENT-1`/`GPU-RENDER-PARITY-1` (`b10fcbc`: „alle Wellen
   BESTANDEN/entschieden"; `a2d0a90`), `EXPORT` unter `GPU-RENDER-PARITY-1`;
   beide Releaseplan-Zeilen sind **gestrichen**. `MASK` ist die **einzige echte
   Lücke** (kein WGSL-Pass in `crates/lumina-gpu/src`; `0` Treffer für
   `local_adjust|mask_layer|apply_mask`) und hat jetzt eine **eigene Task-Zeile**
   `GPU-RENDER-MASK-19`. Die ID-Brücke für `PREVIEW`/`EXPORT` bleibt
   **dokumentarisch unbelegt** — die Zuordnung ist sachlich eindeutig, aber keine
   Quelle verbindet die IDs; das ist als benannte Grenze festgehalten.

| Version | Task-ID | Goal | Stichwort |
| --- | --- | --- | --- |
| 1.0 | R2-GUIMOD-04b | G-10 | GPU-Drossel-Entscheid |
| 1.0 | R2-GUIMOD-04c | G-10 | GPU-Histogramm |
| 1.0 | F-103-N6 | alle G | visueller User-Test |
| 1.0 | F-103-N6-GUI-COVERAGE-27 | alle G | vollständige manuelle GUI-/Persistenz-Checks |
| 1.0 | UI-VISION-REVIEW-70 | alle G | visueller Review der gezeichneten Fläche |
| 1.0 | FIXTURE-ENV-1 | alle G | Env-gegateter Fixture-Test panickt unter `--ignored` |
| 1.0 | GPU-RENDER-MASK-19 | alle G | Masken-Pixelpass |
| 1.0 | CI-CLI-GPU-TESTBUILD-68 | offen — bei der Task | CLI-`gpu`-Testbuild in CI |
| 1.0 | GPU-RENDER-NONDETERMINISM-69-HW | offen — bei der Task | GPU-Renderpfad nicht-deterministisch (Adapter-Nachweis) |
| 1.0 | R5-DUST-23-FOLLOWUP | G-04 | Spot-Heal-Followup |
| 1.0 | R5-BRUSH-24 | G-03 | Pinsel-Masken |
| 1.0 | R5-MASKVIS-25 | G-03 | Masken-Overlay |
| 1.0 | GPU-MASKPLANE-CLEAR-PERF-51 | verifiziert geschlossen (2026-10-15, GPU-Gate gruen) | GPU-Kosten des Plane-Clears gemessen |
| 1.0 | GOLDEN-STALE-55 | alle G | 127px-Stale-Indikator in develop_section_rating geklaert |
| fortlaufend | CI-WATCH-1 | alle G | CI-Beobachtung |
| fortlaufend | TEST-AUDIT-36 | alle G | Test-Redundanz-Audit (1×/Woche) |
| 2.0 | LRPAR-G14-DENOISE-IMPL-20 | G-14 | KI-Denoise-Impl |
| 2.0 | MODEL-MGMT-1 | offen — bei der Task | generische Modellartefakt-Installation |
| 2.5 | LRPAR-G09-CULL-IMPL-25 | G-09 | KI-Culling-Impl |
| 1.5 | MCP-MASK-APPLY | verifiziert geschlossen (2026-10-23; `crates/lumina-mcp/tests/mask_policy.rs`, 2/2) | Masken anwenden (MCP) |
| 1.5 | MCP-MASKPOLICY | verifiziert geschlossen (2026-10-23; `crates/lumina-mcp/tests/mask_policy.rs`, 2/2) | MaskPolicy-MCP |
| 1.5 | MCP-PARITY-A | verifiziert geschlossen (2a88d3b) | MCP-Stage-Editoren |
| 1.5 | MCP-PARITY-B | verifiziert geschlossen (2026-10-23; Library-/Stage-Paritätstests, 35/35) | MCP-Bibliothek+Stufe |
| 2.0 | MCP-PARITY-C | offen — bei der Task | MCP-Merge-Pipelines |
| nie | — | G-12 | Karten-Modul/GPS (Nicht-Ziel) |
| nie | — | G-15 | Veröffentlichungsdienste (Nicht-Ziel) |

## GUI-Test-Arbeit: Gate-Struktur

**Dieser Abschnitt hält die Abhängigkeitsordnung der offenen Waves — nicht die
Analyse, die sie begründet hat.** Die Analyse (`### Befund`, ~210 Zeilen:
Zahlenbasis, vier gemessene Defekte, Fixture-Vertrag, Testgruppen-Themen) ist am
2026-09-25 abgeschlossen worden, steht im **Git-Verlauf** und ist in den
Feature-Dokumenten festgehalten: `feature/quality/golden-references.md`
(Referenzplattform, Fingerabdruck, Wächtervertrag),
`feature/quality/golden-fixtures.md` (Fixture-Vertrag, Klassen R1/S1/S2/C) und
`feature/platform/capability-matrix.md` (Welle 2 ist **kein** Hardwareproblem).
Sie wird hier bewusst **nicht** wiederholt: der Plan führt offene Aufgaben, und
eine abgeschlossene Analyse ist keine.

**Zahlenbasis, gemessen 2026-09-25 auf `ci-shard-gui-tests` @ `83c0a2f`** (macOS,
Apple M5 Pro, Metal 4, LibRaw 0.22.2) — **vor** der Neumessung durch
`GOLDEN-BASELINE-32`, also als **historischer Ausgangspunkt** zu lesen, nicht als
aktueller Sollwert: committet waren **62** Golden-PNGs über 6 Golden-Targets plus
Lib-Target; auf dem Target `kittest_snapshots` schlugen **12 von 56** Tests fehl;
die Fehlerzahl für den **vollständigen** 62er-Korpus war offen und sollte
einmalig gemessen werden. Die Corpus-Zählung war gegen die Call-Sites geprüft:
**keine verwaisten Goldens** (acht `parity_paths_*` werden per `format!` erzeugt,
`mask_management_controls` stammt aus `src/tests/brush_management.rs`, also aus dem
Lib-Target). **Vier Defekte** waren gemessen und sind in den genannten
Feature-Dokumenten benannt.

### Abhängigkeitsordnung der offenen Waves

### Auflösung

```
Welle 0  Gate überhaupt erst herstellbar   (Block A, rein, kein Hardwarebedarf)
  GOLDEN-REF-30      → Referenzplattform + Fingerabdruck + Pin im Repo
  GOLDEN-FIXT-31     → Fixture-Vertrag, Fehler-Zustand ≠ Render-Gate
  LENSFUN-DB-33      → 10 rote Tests ehrlich auflösen
  GUITEST-STRUCT-34  → 91 flache Testdateien thematisch gruppieren
        │
Welle 1  genau eine geprüfte Baseline       (Block A nach Welle 0)
  GOLDEN-BASELINE-32 → EIN Refresh, jede Änderung visuell begründet
        │
        ├──► MASK-LOCAL-P0         (offener Posten 1 entfällt)
        └──► MASK-LOCAL-P1.1       (offener Posten 1 entfällt)

Welle 2  Hardware-Lauf                   (bereits auf M5/Metal möglich!)
  GPU-PARITY-HW-28   → Primär-Gate der Welle
        ├──► R5-BRUSH-24            (offen: Metal/Vulkan/DPI/Pointer)
        ├──► R5-MASKVIS-25          (dito)
        └──► R5-DUST-23-FOLLOWUP    (offen: echter GPU-Nachweis)

Welle 3  manueller GUI-Lauf             (Block C, unverändert)
  F-103-N6
        ├──► F-103-N6-GUI-COVERAGE-27
        ├──► R2-GUIMOD-04b / 04c    (brauchen 04a-Zahlen)
        └──► DPI-/Pointer-Anteile aus R5-BRUSH-24/-MASKVIS-25
```

**Der Hebel ist Welle 0, nicht Welle 2.** Welle 2 ist nur *ein*
`cargo test`-Kommando auf dieser Maschine (Metal 4 vorhanden,
Headless-wgpu-Adapter nachweislich funktionsfähig). Dass sie als
offen geführt wird, ist ein Dokumentations-, kein Hardwareproblem.
Welle 0 verwandelt außerdem jede künftige Baseline-Erzeugung aus
einem ungeprüften `UPDATE_SNAPSHOTS=1` in einen nachvollziehbaren,
auf eine gepinnte Umgebung beschränkten Schritt.

**Verifikations-Rundlauf 2026-09-27 (unabhängig, PASS) — `HSL-BAND-CLICK-43`:
die HSL-Bänder `Hue` und `Luminance` werden angeklickt.** **Zuordnung:** dieser
Abschnitt ist die Sektion der GUI-Test-Arbeit; die Position war eine echte
Testlücke der Klick-Abdeckung und ist seit der Verifikation **erledigt und
verifiziert** — sie steht hier als **Nachweis**, nicht als offene Aufgabe (das
Board führt geschlossene Positionen als Prosa, nie als `- [x]`; `grep -c '^- \[x\]'
Agents.todo.md` ist 0). Sie stand **nie** im committeten Board (`git log -S
"HSL-BAND-CLICK-43" -- Agents.todo.md` war leer) — sie war eine uncommittete
Position im Arbeitsbaum, die beim Pull gestasht wurde. Die Implementierung lag
bereits als `fccbea8` vor (3 Dateien, **0 Produktcode** — `crates/lumina-gui/src/`
unberührt; die Abgrenzung ist damit strukturell belegt und nicht nur behauptet),
offen war allein der Nachweis. Gefahren auf einer GPU-losen Maschine mit frisch
gebautem Toolchain (LibRaw 0.22.2 gepinnt, `cargo` 1.98.1): Baseline plus alle
drei in `feature/product/ai-masks.md` dokumentierten Mutationen, jeweils **beide**
betroffenen Targets, Mutation nach jeder Messung sofort reverted, Working Tree
am Ende sauber.

| Mutation | Doku (Stand 2026-09-27) | Gemessen (Stand 2026-09-27) | |
|---|---|---|---|
| `HSL_FIELDS` 0<->2 (Feldseite) | `hsl_bands:195`, `:255` | `mask_local_hsl_bands.rs:195` (Feld-Pin Hue, *"got 0"*) und `:255` (Zeugen-Pin Luminance) | **rot** |
| Label-`match` 0<->2, `HSL_FIELDS` unberührt (Beschriftungsseite) | `hsl_bands:129` | `mask_local_hsl_bands.rs:129` (Stack-Assertion, meldet die drei Rail-Rechtecke) | **rot** |
| `HSL_FIELDS` 0<->1 (Kontrolle) | `color_controls:225` + `hsl_bands:165`, `:255` | `mask_local_color_controls.rs:225`, `mask_local_hsl_bands.rs:165` und `:255` | **rot** |
| Baseline (unmutiert) | grün | 3 passed / 0 failed | **grün** |

Die Zeilennummern der Doku stimmen **exakt**. Die zweite Zeile ist der
eigentliche Nachweis: weil `draw_mask_local_color` das Label (`match index`) und
das Feld (`HSL_FIELDS[index]`) aus **zwei unabhängigen Quellen** ableitet, sind
Feld- und Beschriftungs-Transposition zwei verschiedene Defektklassen — bei der
Label-Mutation ist der Schienen-Vektor korrekt und nur die Beschriftung
vertauscht, was ein ausschließlich auf M1 geprüfter Test durchwinken würde. Beide
sind unabhängig nachgewiesen.

**Abnahme erfüllt:** je ein Klick-Test auf `Hue` und `Luminance` vorhanden, der
0<->2-Tausch macht beide rot, keine Änderung an `mask_local_color.rs` oder am
Rezept. Das Gate dieses Tasks war ausdrücklich GPU-frei, er ist hiermit
geschlossen. Vollständiges CI-Gate am selben Tag grün: 8/8 Stufen, 2738 passed /
0 failed / 106 ignored (alle 106 sind die ehrlich als `headless GPU required`
markierten Tests).

**Bewusst nicht neu gemessen:** die Voller-Suite-Zahl zum Zeitpunkt *vor* der
Zustellung (im Tasktext 926/926, in `fccbea8` und `feature/product/ai-masks.md:1443`
dagegen 931/931). Eine Neumessung wäre ein kompletter Suite-Lauf unter Mutation
und ist für die Abnahme nicht erforderlich; die Doku-Zahl 931/931 bleibt als
die spätere, konsistente geführt. Der Widerspruch wird hier nur benannt, nicht
aufgelöst.

**Stand der Belege am 2026-10-21 (nachgetragen, HEAD `ee2c4d4`; die Mutationen
wurden heute nicht neu gefahren — auf diesem Host fehlt die Rust-Toolchain,
`cargo fmt` endet mit Exit 127):** die **Quelltext**-Anker stimmen
**unverändert** — `crates/lumina-gui/tests/mask_local_hsl_bands.rs` ist
byte-identisch zu `fccbea8` (`git diff fccbea8 HEAD -- <pfad>` ist leer), und
`:195` (Feld-Pin Hue), `:255` (Zeugen-Pin Luminance), `:129` (Stack-Assertion),
`:165` (Kontroll-Zeugen) sowie
`crates/lumina-gui/tests/mask_local_color_controls.rs:225` (Kontrolle Saturation)
tragen heute dieselben Assertions; `crates/lumina-gui/src/mask_local_color.rs`
(`HSL_FIELDS`) ist seit `fccbea8` unverändert. **Verschoben sind die
Zeilennummern der Doku selbst:** die zitierten `feature/product/ai-masks.md:1442-1444`
halten heute die Zeilen 21/22 („Reset pro Bereich"/„all local color reset") der
Grading-Tabelle; derselbe Inhalt steht jetzt in
`feature/product/ai-masks.md:1354-1356` (Test-Mutationstabelle) und
`:1493-1495` (Klassifikationstabelle), die „931/931"-Notiz von damals
`ai-masks.md:1443` heute `:1494`. Die Klassifikation selbst ist unverändert:
Zeile 6 (HSL **Hue**) und Zeile 8 (HSL **Luminance**) stehen in
`feature/product/ai-masks.md:1425` und `:1427` auf **ja** mit den Testnamen
`the_hsl_hue_rail_writes_only_the_hue_field_of_the_selected_band` und
`the_hsl_luminance_rail_writes_only_the_luminance_field_of_the_selected_band`.
**Die CI-Zahlen des 2026-09-27-Laufs sind ebenfalls überholt:** die Fast-Stufe
zählt heute **9** Checks (`fast-1`…`fast-9`, `MIN_CHECKS=9`), nicht 8; der grüne
CI-Lauf `39670086731` zu `8c9df57` hatte **7 Jobs** alle grün, davon „Rust tests
(excluding GUI)" **3416 passed / 0 failed / 41 ignored** und „Rust GUI tests"
**1946 passed / 0 failed / 87 ignored**. Die Mutationen selbst sind heute
**nicht** neu gemessen — das ist eine offene Messung, keine Behauptung.

## Phase 3–5: Renderpipeline, RAW, Auto-Tone

Keine offenen Punkte. SOLL: `feature/architecture/pipeline.md` und
`feature/quality/performance-benchmarks.md`.

## Block A – „Vor dem nächsten manuellen GUI/User-Test umsetzbar“

**Welle 0 (dieser Block) ist vollständig ohne User-Interaktion und ohne
Hardware-Gate abarbeitbar.** Sie stellt die Gates her, an denen die
PRIO-hoch-Tasks dieses Blocks hängen; die Aufgaben selbst werden erst
in Welle 1/2 (Abschnitt [GUI-Test-Arbeit](#gui-test-arbeit-gate-struktur))
abgeschlossen. Reihenfolge: PRIO hoch → mittel → niedrig.

### PRIO: hoch — Welle 0: Gates herstellen

Ziel dieser Welle: die Voraussetzungen schaffen, unter denen die bereits
implementierten GUI-Tasks dieses Blocks überhaupt abgeschlossen werden können.
Reihenfolge und Abhängigkeiten: Abschnitt
[GUI-Test-Arbeit: Gate-Struktur](#gui-test-arbeit-gate-struktur).
SOLL **vor** Code, je Task mit eigener Feature-ID.




- [ ] **[PRIO: mittel] GPU-RENDER-MASK-19 (Release 1.0; aus dem Releaseplan-Audit als EINZIGE echte Lücke belegt, 2026-10-02)** Der **GPU-Masken-Pixelpass** fehlt: lokale Masken modulieren auf der GPU keine Pixel. **Gemessen (Audit 2026-10-02, eigene Nachmessung):** `crates/lumina-gpu/src` hat `combine_mask_planes` (`crates/lumina-gpu/src/lib.rs`) und `upload_mask_plane` (`crates/lumina-gpu/src/lib.rs`) — Upload/Clear/Combine, **nicht** den Pass; `grep -rniE 'local_adjust|mask_layer|apply_mask' crates/lumina-gpu/src` → **0 Treffer**; `upload_mask_plane_roundtrip_is_byte_exact` (`crates/lumina-gpu/tests/stages.rs`) belegt nur den Upload-Roundtrip. `GPU-PARITY-MASKGATE-1` ist ein Present-Gate, nicht der Pass; `GPU-MASKPLANE-BRUSH-50`/`-CLEAR-PERF-51` sind Brush-Upload bzw. Clear-Performance; `MASK-LOCAL-P0`…`P1.2d` bauen das Rezept-Datenmodell. Die CPU-Referenz moduliert bereits (`apply_local_adjustments`), die GPU-Stufe fehlt (dokumentierte F-042-Grenze, `pipeline.md:1885-1890`). **Abnahme:** (1) eine WGSL-Stufe, die die lokal ausgewertete Maske (Alpha) pixelgenau auf den Composite anwendet — keine CPU-Route, keine stille Ausnahme (volle GPU-Parität, User-Entscheid 2026-09-12); (2) CPU-Oracle-Parität byte-identisch wo 0, sonst maxAbsDiff ≤ 1 (Muster `crates/lumina-gpu/tests/parity.rs`); (3) E2E am Adapter: `cargo test -p lumina-gpu --features gpu-adapter-tests` **plus** die betroffenen GUI-GPU-Tests (Regel `GPU-E2E`); (4) SOLL in `feature/architecture/pipeline.md` §GPU und `feature/product/ai-masks.md` von „dokumentierte F-042-Grenze" auf „GPU-Stufe vorhanden" umstellen; (5) der Masken-Pass wird im Render-Key/der Route sichtbar (kein stiller CPU-Fallback). **Abgrenzung:** der **Pass** selbst; Upload/Clear/Brush bleiben bei ihren Tasks. Parent-Gate-Referenzen: `pipeline.md:2261`, `ai-masks.md:349`. **STAND 2026-10-15 (Slice A geliefert und verifiziert):** der Pass **existiert und ist fachlich korrekt** — Commit `1981330` (`crates/lumina-gpu/src/local_mask_pass.rs` 372 Z., `src/local_mask_gpu.rs` 295 Z. nur `feature = gpu`, `tests/local_mask_parity.rs` 439 Z.; `crates/lumina-gpu/src/lib.rs` +4 Verdrahtungszeilen). Am Metal-Adapter gemessen: `cargo test -p lumina-gpu --features gpu-adapter-tests` → lib 56/0, golden 7/0, init_warning 1/0, lensfun_map 3/0, lensfun_tca_skip 0/0, **local_mask_parity 5/0 (neu)**, parity 27/0, stages 4/0 — keine Regression. CPU-Oracle-Parität: P0-only, relative WB und der Mehr-Layer-/Global-Fall **byte-identisch** (`maxAbsDiff = 0`), WB-Zelle deklariert `Bounded(1)` und **misst 0** (Probe `Bounded`→`ByteIdentical` passed). Unabhängige Verifikation (`deepseek-v4.1-flash`) mit **5 eigenen Mutationen** (Rundungskonstante → 3/2 failed; `rev()` auf die Layer-Reihenfolge → 4/1, `maxAbsDiff 63`; `65535u - alpha` → `alpha` → 2/3, alle Hardware-Zellen rot; `adj.rgb` → `adj.bgr` → 2/3 rot; Endpunkt-`alpha == 65535u` → `>` → 5/0 grün, weil die Bruchformel denselben Wert liefert) — jede Mutation trug eine Aussage, jede exakt per SHA-256 restauriert. Ratchet/Anti-Gaming/fmt/Clippy clean. **VERDICT: NICHT BESTANDEN** — *nicht* wegen des Passes, sondern wegen Abnahme-Punkt 5. **Offener Befund `GPU-RENDER-MASK-19-INTEGRATION` (HIGH):** der Pass wird von **keinem** Produktionspfad aufgerufen. Repo-weiter Grep über `render_with_gpu_local_masks`/`unsupported_local_mask_stages`/`validate_local_mask_layers`: 19 Treffer, **alle** Definition/Interna in `crates/lumina-gpu/src/local_mask_pass.rs` oder im Test — **null** in `crates/*/src`, `crates/*/tests`, benches. GUI (`crates/lumina-gui/src/gpu_routing/local_adjustments.rs` → `local_adjustment_route_reason`, `crates/lumina-gui/src/mask_local_controls.rs` → `local_adjustment_route_reason`) routet jeden sichtbaren lokalen Maskenzustand über `local_adjustment_route_reason()` weiter auf die CPU; CLI (`crates/lumina-cli/src/local_mask_gpu.rs` -> `mask_layer_routing_reasons`) hängt `masks (active copy has layers)` an die Routing-Gründe → CPU. Nach `Agents.md` ist ein Modul, das kein `draw_*`-/Render-Pfad aufruft, **nicht integriert**. Der CPU-Fallback ist **laut** (Routing-Grund + Log), nicht still — das mildert den Befund, schließt ihn aber nicht. **Was Slice A NICHT leistet:** (a) P1.2-Blöcke (lokale Tonkurve/Farbe/Presence/Detail) sind nicht GPU-verdrahtet — `unsupported_local_mask_stages` nennt sie als CPU-Routing-Grund, `render_with_gpu_local_masks` verweigert sie laut; (b) kein Pipeline-Cache (`build_local_mask_pipeline` baut die zwei Pipelines pro Aufruf); (c) Render-Key-Bezug ungeprüft (`lumina-gpu` hat keinen RenderKey); (d) die WB-`Bounded(1)`-Schranke ist nur auf **einem** Fixture belegt (`maxAbsDiff = 0`), ein `f32`-vs-`f64`-Divergenzfall wird nicht provoziert; (e) `cargo doc -D warnings` ungeprüft (6 vorbestehende private-item-Links, keine aus den neuen Dateien). **STAND 2026-10-15 (Slice B1 geliefert und unabhängig verifiziert — BESTANDEN):** die zwei fehlenden **Nahtstellen** sind da, **ohne** Verhaltensänderung. (1) `lumina_core::evaluate_local_mask_layers(context, input_w, input_h, frame_w, frame_h)` in `crates/lumina-core/src/render/mask_evaluation.rs` (150 → 335 Z.; Re-Export `crates/lumina-core/src/lib.rs` **+1 Zeile = exakt Baseline 2781**) — ein **dünner** Wrapper, der genau `prevalidate_local_adjustments` + **die eine** `super::evaluate_mask_stage(...)` ruft; `crates/lumina-core/src/render.rs` bleibt **exakt 2536** (nur `mod` → `pub mod`). (2) `GpuContext::apply_gpu_local_masks(&self, global, layers)` und `unsupported_local_mask_adjustments(layer_id, adjustments)` in `crates/lumina-gpu/src/local_mask_pass.rs` (372 → **483** Z., ≤ 500) — `render_with_gpu_local_masks` ist jetzt exakt `render_with_gpu` + diesem zweiten Schritt; `run_local_mask_chain` hat **eine** Definition und **einen** Aufrufer. Unabhängige Verifikation (`deepseek-v4.1-flash`) zählte „genau ein Auswertungspfad" selbst nach: `MaskGraph::new(` **1×** im Produktionscode (`crates/lumina-core/src/render/mask_evaluation.rs` → `MaskGraph::new`), `modulate_mask_plane` 1 Definition/1 Produktions-Caller, `evaluate_mask_stage` **1** Definition bei **2** Aufrufern (CPU-Render `crates/lumina-core/src/render.rs` → `evaluate_mask_stage` + neue Naht `crates/lumina-core/src/render/mask_evaluation.rs` → `evaluate_local_mask_layers`) — also geteilt, keine Kopie; **kein** Sichtbarkeitsleck (`evaluate_layer`/`LayerFailure` bleiben `pub(super)`). Testzahlen selbst gemessen: `lumina-core --lib` **551/0**, `lumina-gpu --lib` **58/0**, GPU-Pflichtgate `--features gpu-adapter-tests` exit 0 mit `local_mask_parity` **6/0 (0 ignored)**, `parity` 27, `golden` 7, `stages` 4. Drei eigene Mutationen am Produktionspfad: `take(0)` auf der Maskenschleife → **3** Orakel-Tests rot; P1.2-Prädikat neutralisiert → `--lib`-Unit-Test **und** die P1.2-Zelle rot; `frame_width + 1` im Plumbing → Naht-Test rot; jede per SHA-256 byte-identisch restauriert. `check_file_sizes.sh` exit 0, `fmt`/`clippy -D warnings` clean, **kein** CLI-/GUI-Byte geändert. Zwei **low**-Beobachtungen daraus: (i) der neue Adapter-Test pinnt nur das **Delegationsverhältnis**, nicht die Kettenwirksamkeit — der Doc-Kommentar wurde entsprechend ehrlich gestellt, die Kette selbst decken die drei Orakel-Tests (mutationsbewiesen); (ii) der Neutral-Frühausstieg (`Ok(global.clone())`) und die `AdapterUnavailable`-Verweigerung von `apply_gpu_local_masks` sind code-korrekt, aber **nicht** direkt getestet (vorbestehend aus Slice A): der **Neutral-Fall** wird in Slice B2 testbar mitgenommen, der **No-Adapter-Zweig** ist auf dieser Maschine (Metal vorhanden) nicht auslösbar und bleibt als benannte, ungeprüfte Lücke stehen. **Der Integrationsbefund `GPU-RENDER-MASK-19-INTEGRATION` bleibt offen (HIGH):** B1 liefert nur die Naht, **keinen** Caller. **Nächster Slice (B2 — CLI-Verdrahtung):** neues gpu-only `crates/lumina-cli/src/local_mask_gpu.rs` (Muster `crates/lumina-cli/src/lensfun_gpu.rs`), Ersetzung des pauschalen `masks (active copy has layers)` (`crates/lumina-cli/src/main.rs` → `mask_layer_routing_reasons`) durch das P1.2-Prädikat (netto ≤ 0 Zeilen, `crates/lumina-cli/src/main.rs` bleibt **≤ 7874**), im GPU-Zweig von `render_best_effort` `evaluate_local_mask_layers` → `apply_gpu_local_masks` binden und `RenderOutput.mask_layers`/`mask_warnings` füllen; ein Verweigern des Passes wird **laut** behandelt (Log + exakter CPU-Pfad, damit `--require-gpu` laut scheitert) — **kein** stiller un-modulierter Frame. Die **GUI**-Verdrahtung (Present-Pfad) ist headless umgesetzt: `render_draft_tick` paart ausgewertete Full-Frame-Maskenebenen mit persistierten Layer-Anpassungen und komponiert sie nach dem globalen VRAM-Render; Draft/ROI/Stage- und Fehlerfälle verweigern Freshness laut, die CPU-Preview bleibt autoritativ. Die GUI-Route-Tests decken die Produktionsnaht ab. Die historische Formulierung, sie sei noch ein späterer Slice, ist damit überholt. Offen ist ausschließlich `GPU-RENDER-MASK-19-ADAPTER` (GPU-Pixelparität und GUI-Present auf echtem Adapter); kein Adapterergebnis wird behauptet. **STAND 2026-10-15 (Slice B2 geliefert, in Runde 2 unabhaengig verifiziert — BESTANDEN):** die **CLI** ruft den Pass jetzt produktiv auf; der HIGH-Befund `GPU-RENDER-MASK-19-INTEGRATION` ist damit **fuer die CLI geschlossen** (offen bleibt allein der GUI-Present-Pfad). Neu: gpu-only `crates/lumina-cli/src/local_mask_gpu.rs` (229 Z.) mit `mask_layer_routing_reasons` (aktive Kopie; unsichtbare und neutrale Ebenen uebersprungen, sonst `unsupported_local_mask_adjustments`), `composite` (Maskenstufe **immer** auswerten → id-basierte Paarung → `apply_gpu_local_masks`), `pair_layers`, `active_copy`, `gpu_output`; `crates/lumina-cli/src/main.rs` **7874 → 7869** (netto −5: der pauschale Grund `masks (active copy has layers)` ersetzt, der GPU-Zweig ruft `composite`, ein `Err` wird zur **lauten** CPU-Route mit `log_cpu_routing_once` und dem exakten CPU-Pfad — kein stiller un-modulierter Frame). Argument-Abbildung wie im CPU-Render: `input_* = frame.width/height` (Decode-Quelle vor Geometrie), `frame_* = global.width/height` (Post-Geometrie), gegen `crates/lumina-core/src/render.rs` → `mask_input_width`/`mask_input_height` und `frame_width`/`frame_height` geprueft. **Bewusste Abweichung von der Vorgabe:** `composite` wertet die Maskenstufe auch ohne sichtbare nicht-neutrale Ebenen aus, weil der CLI aus `RenderOutput.mask_layers` die `match_total_exposure`-Maskendomaene ableitet; nur der **GPU-Pass** laeuft bedingt. **Runde 1 (Commit `2e3b56a`) war NICHT BESTANDEN** — HIGH-Befund: `pair_layers` lief unbedingt, dadurch fiel **jedes maskenfreie Rezept** mit dem falschen Grund „the active copy vanished before layer pairing" auf die CPU (A/B selbst gemessen: pristine `bc9e090` exit 0 / route `gpu` / reasons `[]` gegen `2e3b56a` exit 1 / route `cpu` / reasons `["masks (GPU local-mask pass: the active copy vanished before layer pairing)"]`). **Runde 2 behebt das** (`evaluated.is_empty()` → No-Op vor `pair_layers`; ein nicht-leeres `evaluated` setzt die aktive Kopie voraus, der Fehler bleibt fuer eine echte Divergenz reserviert) und pinnt drei weitere Befunde: der Sichtbarkeits-Skip ist jetzt **tragend** (`invisible_p12_layer_is_not_a_cpu_route_reason` mit P1.2-Block; ein neutraler P1.2-Block ist per Definition unmoeglich, weil `is_neutral()` alle vier P1.2-Praedikate ausschliesst), der F2-Vertrag „kein stilles `Ok(Gpu)` mit un-moduliertem Frame" (`unavailable_mask_fails_loudly_not_with_a_silent_gpu_frame`), und `cargo check -p lumina-cli --features gpu,lensfun --tests` kompiliert (fehlender `RawMetadata`-Import, dieselbe Klasse wie der `Geometry`-Fix). Testzahlen selbst gemessen: `--bin lumina-cli` **146/0** ohne gpu; mit gpu **158/1** — der eine rote Test ist **genau** der benannte, vorbestehende Nicht-Determinismus `auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move` (eigener Task `GPU-RENDER-NONDETERMINISM-69`), **kein** neuer Test; `tests::local_mask_gpu` **10/0**, `tests::local_mask_gpu_route` **3/0**; vier Mutationen am Produktionspfad (F1-Fix rueckgaengig; Sichtbarkeits-Skip entfernt; `apply_gpu_local_masks(global, &[])`; `crates/lumina-cli/src/main.rs`-`Err`-Zweig → un-moduliertes `global` als `Gpu`), jede per SHA-256 byte-identisch restauriert. **Benannte, ungepruefte Luecke:** der `Err`-Zweig-Ausgang „erfolgreicher CPU-Pfad als `RenderRoute::Cpu`" ist mit den heutigen Routing-Regeln **unerreichbar** (jeder `composite`-Fehler laesst unter `Strict` auch das CPU-Orakel scheitern) und daher nicht end-to-end gepinnt — defensive Doppelabsicherung, kein stummer Fallback; ebenso bleiben die zwei adapter-gated Tests in CI ohne Aussage (bestehende CLI-GPU-Policy, kein neuer Bruch). **Headless-Stand 2026-10-23:** GUI-Present ist abgeschlossen; Geometrie-/ROI-/Draft- und nicht unterstützte Stage-Fälle verweigern laut und bleiben auf CPU. **Offen:** ausschließlich Adapter-/GUI-E2E-Prüfung gemäß `GPU-RENDER-MASK-19-ADAPTER`; kein Pixelparitätsanspruch. **Neu erkannt beim B2-Abschluss:** der CLI-`gpu`-Testbuild wird von **keiner** CI gebaut (`CI-CLI-GPU-TESTBUILD-68`).
- [ ] **[PRIO: hoch] GPU-RENDER-NONDETERMINISM-69-HW (Adapter-Nachweis; Regel `HEADLESS-GPU-TASK-SPLIT`)** Der headless Teil von `GPU-RENDER-NONDETERMINISM-69` ist abgeschlossen: die Analyse ist dokumentiert in `feature/quality/gpu-nondeterminism-2026-10-16.md`. **Dieser Task fuehrt nur den Adapter-Nachweis:** Reproduktion des roten Laufs auf echter Hardware (Metal/Vulkan), Mechanismus jenseits der Projektgrenze eingrenzen, beheben oder endgueltig als Grenze bestaetigen. **Abnahme:** ein **vorher rotes, reproduzierbares** Kommando auf Adapter-Hardware, danach gruen **oder** eine belegte, benannte Grenze inkl. Adapter-Referenz; verbindliches Kommando `cargo test -p lumina-gpu --features gpu-adapter-tests` plus die betroffenen GUI-GPU-Tests (Regel `GPU-E2E`). **Abgrenzung:** nur der Adapter-Nachweis; die headless Analyse ist abgeschlossen. **STAND 2026-10-25 (`b8dcee5`): Reproduktion und Route belegt, Ursache offen.** Drei serielle Adapterlaeufe der neuen Diagnosevariante endeten dreimal mit Exit 101 und jeweils **120/120** echten `GpuPipelineCompleted`-Reports plus CLI-GPU-Route; der unveraenderte Original-Reproducer ebenfalls Exit 101 (38/40). Damit ist die frueher offene Routenfrage beantwortet — der rote Lauf ist **kein** CPU-Fallback und **kein** fehlender Adapter. Die Abweichung betrifft 254–256 von 256 Pixeln je betroffenem Mitglied, die Slider-Literale sind gleich; je Mitglied weicht genau ein Vergleichspaar ab, Mitglied 01 in allen drei Laeufen. Belege: `target/gpu-render-nondeterminism-69/2026-10-25T195812Z/`. Offen bleiben Mechanismus und Fix; die staerkste pruefbare Hypothese (Upload-Identitaet ueber Zeigeradresse statt Inhalt) steht mit Kriterium im Feature-Dokument und ist **nicht** gemessen.


### PRIO: hoch — LR-Parität (Welle 1/2; Gate-Abhängigkeit siehe oben)

**LR-Parität aus `.goal/Goal.md` (Batch 1 + User-Featureliste, Stand 2026-09-03, 16 Goals, Aggregat ~39,1 %)**

Ziel: UI zum Verwechseln ähnlich zu Lightroom Classic; jedes Feature auf
CLI- **und** GUI-Ebene getestet. Quelle: `.goal/Goal.md` G-01…G-16, Beleg:
`.goal/lighroom screenshots batch 1/Content.md`. Umsetzung je Task via
`general`-Implementierungs-Agent + unabhängiger `general`-Verifizierungs-Agent
(Regel oben). LR-Parität-Batch aus 2026-09-04 (teils erledigt s. Git-Historie; Rest s. Releaseplan/Blöcke).



### PRIO: hoch — Produktbefund aus GOLDEN-FIXT-31






**Stand 2026-09-26 (Build-Agent + Reparaturlauf; unabhängige Verifikation PASS nach einem FAIL, Details unten):** Schema v6 mit `detail: Option<Detail>` (`crates/lumina-sidecar/src/detail.rs`: die **globalen** `Sharpening`-/`NoiseReduction`-Structs plus der lokale Container, alle drei aus `crates/lumina-sidecar/src/lib.rs` extrahiert); die Bereichsvalidatoren liegen in `crates/lumina-sidecar/src/detail_block.rs` (`validate_sharpening`/`validate_noise_reduction`/`sharpening_is_neutral`/`noise_reduction_is_neutral`/`SHARPENING_FIELDS`/`NOISE_REDUCTION_FIELDS`/`sharpening_radius_range`) und werden von globalem (`validate_adjustments`) und lokalem Rezept **wörtlich geteilt** — die Fehlerstrings sind unverändert. v1..v5 migrieren verlustfrei mit `detail: None`, ein v1..v5-Payload mit `detail` (auch `null`/Nicht-Objekt/`{"__unset__":true}`) ist laut; die Versions-Gates bleiben versionsrichtig verankert (`CURVE_…=3`, `COLOR_…=4`, `PRESENCE_…=5`, `DETAIL_…=6`), ein v5-Dokument behält seinen Presence-Block (eigener Test). `adjustment_detail`/`adjustment_sharpening`/`adjustment_noise_reduction` bleiben „unknown local adjustment". CPU-Kern `crates/lumina-core/src/detail_stages.rs` (5x5-Bilateral-Gewichte und -Mittelung, Chroma-Offsets, separierbares Gauß inkl. der globalen Radiusformel `sigma = max(r·scale, 0.5)`, Rec.709-Luminanz, `gx`/`gy` + Frame-Maximum, Detail-Mixing, Flat-Area-Faktor) plus `crates/lumina-core/src/render/local_detail.rs` (**eine** Quantisierungsgrenze; die lokale Kernel-Funktion nimmt **weder Maske noch ROI** entgegen — genau das ist der tragende Beweis, und der Boundary-Test ruft sie ohne Maske auf und vergleicht mit dem voll maskierten Render). `crates/lumina-core/src/render/local_color.rs` wurde in `local_tone_and_color_normalized` (un-quantisiert) + `local_tone_and_color_stages` (eine Rundung) geteilt, damit die Detail-Stufen auf dem un-quantisierten Farbresultat laufen, ohne dass ein P1.2b/P1.2c-Layer ein Byte verliert. Render-Scale: `render_frame_from_base_impl` berechnet **eine** `effective_scale` und gibt sie sowohl an `apply_recipe_with_scale_white_balance_and_denoise` als auch an `apply_local_adjustments` → `apply_mask_local_recipe_with_scale`; es gibt kein persistiertes lokales Scale-Feld, und die Radius-Formel ist per Doppel-Skalen-/Doppel-Radius-Identität **für den lokalen und den globalen Pfad** gepinnt. Inhaltliche Pfadwahl: die Neutralität eines Sharpening-Blocks ist `amount == 0` (der eigene Early-Return des globalen F-095-Stages), also ist `masking = 0` die **stärkste** Einstellung und nie ein stiller No-op, und ein Radius kann einen Block nie neutral machen (`sigma`-Floor `0.5`, erster Nachbartap `exp(−2) ≈ 0.135`). GUI `crates/lumina-gui/src/mask_local_detail.rs` (Editor + Setter/Reset pro Bereich und gesamt, keine globale Rezeptmutation, kein Scale-Widget; `is_neutral` trägt den Block, also verweigern alle Stand-in-/GPU-Routen sichtbar, und der Verweigerungstext nennt „detail"). CLI `crates/lumina-cli/src/mask_local_detail.rs` im generischen Kanal (`sharpening.<field>=`/`noise_reduction.<field>=` plus die Resets `sharpening`/`noise_reduction`/`detail`); `crates/lumina-cli/src/main.rs` unverändert bei 11398. Neue/gezielte Anker: Core `local_sharpening_amount_has_an_exact_golden_and_preserves_alpha`, `local_sharpening_radius_has_exact_goldens_at_both_legal_boundaries`, `local_sharpening_detail_has_exact_goldens_for_both_ends_of_the_mix`, `local_sharpening_masking_has_an_exact_golden_and_suppresses_the_flat_area`, `a_masking_zero_sharpening_block_is_not_a_silent_no_op`, `local_noise_reduction_luminance_has_an_exact_golden_and_preserves_alpha`, `local_noise_reduction_color_has_an_exact_golden`, `local_noise_reduction_runs_before_local_sharpening` (Golden **plus** unabhängige f64-Nachrechnung beider Reihenfolgen), `the_full_local_detail_stack_has_an_exact_golden`, `local_detail_json_round_trip_is_byte_stable`, `detail_resets_work_per_sub_block_and_for_the_whole_block`, `the_detail_block_is_part_of_the_local_render_identity`, `two_overlapping_local_detail_layers_use_persisted_order`, `local_detail_sits_after_the_colour_block`, `local_detail_alpha_endpoints_and_partial_alpha_are_exact`, `no_local_detail_keeps_the_p0_p11_p12a_p12b_p12c_bytes`, `invalid_local_detail_values_are_a_loud_preflight_error`, `a_local_detail_edit_never_touches_the_global_recipe`, `the_local_detail_layer_owns_exactly_one_quantization_boundary` (unabhängige Nachrechnung in `crates/lumina-core/src/render/tests/local_detail_reference.rs` **plus** der Nachweis, dass früheres Runden ein anderes Byte liefert), `the_detail_neighbourhood_is_full_frame_and_the_mask_only_gates_the_blend`, `the_local_detail_follows_the_global_render_scale`; Sidecar `local_detail_round_trips_through_the_sidecar_file`, `legacy_versions_migrate_losslessly_and_refuse_a_smuggled_detail_block`, `local_detail_uses_the_existing_global_ranges`, `neutral_absent_and_neutral_detail_blocks_read_the_same_and_reset_clears`, `canonical_digests_cover_the_local_detail_block`, `disabled_local_denoise_and_optics_stay_rejected_and_the_legacy_keys_stay_p0`; CLI `local_detail_round_trips_through_the_sidecar_file_and_resets`, `per_area_detail_resets_remove_exactly_one_sub_block`, `local_detail_edit_reaches_pixels_through_the_cpu_compositor`, `invalid_local_detail_specs_are_loud_and_change_no_bytes`, `local_detail_survives_history_and_reload_verbatim`, `previous_refuses_a_detail_only_local_state_on_source_and_target`; GUI `a_local_detail_edit_changes_the_render_identity_and_never_the_global_recipe`, `local_detail_setters_and_resets_cover_every_field`, `invalid_local_detail_edits_are_refused_without_mutating`, `a_detail_only_local_layer_refuses_the_stand_in_routes`, `local_detail_state_digest_invalidates_mask_and_render`, `gui_and_cli_local_detail_share_the_same_persisted_typed_block`, `local_detail_survives_history_and_previous`, `the_local_detail_editor_paints_and_writes_only_the_mask_layer`.

**Verifikations-Rundlauf 2026-09-26 des **ersten** Build-Agent-Laufs — **UEBERHOLT durch die Reparatur unten; dieser Lauf beschreibt den defekten Stand, nicht den gepushten.** Der damalige Lauf (eigenes Target `target/p12d`, gepinntes LibRaw 0.22.2

**Offen:** (1) die **unabhängige Verifikation ist PASS** (zweiter Agent, eigene Byte-Harness; Details im Verifikationsabsatz weiter oben) — inhaltlich erledigt, offen bleibt der Punkt nur wegen der unten genannten Gates; (2) die drei kittest-Goldens `develop_basic`/`develop_sections_expanded`/`develop_section_masking` brauchen weiterhin einen `UPDATE_SNAPSHOTS=true`-Refresh auf der Referenz-Maschine (headless wgpu-Adapter nötig) — das verbleibende native Golden-Gate aus P0/P1.1/P1.2a/P1.2b/P1.2c; die UI-Änderung fügt Zeilen in der Masking-Sektion hinzu, `develop_section_masking`/`develop_sections_expanded` sind also betroffen, und **keine** `.png` wurde angefasst; (3) die 10 vorbestehenden Lensfun-Datenbank-Tests bleiben P1.2d-fremd rot (fehlendes `/usr/share/lensfun`, auf HEAD 8101c6d reproduziert); (4) **AI-Denoise und Optics bleiben bewusst deaktiviert** — der lokale Renderer darf sie weder aktivieren noch als Stub vortäuschen, ein entsprechender Key ist ein lauter „unknown local adjustment"-Fehler, und Optics bleibt **dauerhaft** deaktiviert, weil Linsenkorrektur/Perspektive geometrische Stufen vor den Masken sind, AI-Denoise bis zum F-078-Modellgate; (5) die Parent-Gates `GPU-RENDER-MASK-19`, `GPU-PARITY-HW-28`, `R5-BRUSH-24` und `R5-MASKVIS-25` bleiben offen, und es wird **keine** GPU-Parität behauptet.


**Stand 2026-10-02 (zwei unabhaengige Verifikationslaeufe, beide NICHT BESTANDEN; Commits 9f3a893, d77db28, 53e1ac0; der Task bleibt OFFEN):** die Verifikation hat nicht am Fix gelegen, sondern an **Behauptungen ueber eigenen Code** — und die hat sie zu Recht zweimal geschlagen. **Lektion dieser Runde, in der Reihenfolge, in der sie mich getroffen hat:** (a) `6f46e0d` hatte seine eigene Hauptaussage zurueckgenommen ("GEMESSEN FALSCH. Kein GUI-Rezept war je stale") — aber nur im Commit-Text und hier, **nicht an der Fundstelle**: der Code-Kommentar an `LuminaApp::auto_tone` behauptete weiter "two consequences, both measured: ... permanently stale", mit gefaelschtem Beleg. DoD §9 verlangt die Ruecknahme AN DER FUNDSTELLE. Vier Fundstellen korrigiert (Code-Kommentar, Modul-Doku in `lumina-stages/src/auto_tone.rs` mit dem jetzt ueberholten GUI-Ausschluss, `pipeline.md` mit zwei Tode-Verweisen auf `crates/lumina-cli/src/auto_tone_cli.rs` als Writer-Ort, `cli-gui-wasm.md`). (b) Der G-16-**Endpunkt** verletzte Klausel (2) seit HEAD: er schrieb 1 von 6 Reglern, 1 von 6 Spiegeln, `enable_auto_tone` und den Fingerprint und persistierte das — ein gemischter Zustand. Die Begruendung in `6f46e0d` verwechselte Klausel (1) mit Klausel (2), und `g16_apply_auto_endpoint_sets_only_its_field` **pinnte den Defekt als erwartetes Verhalten** (`assert_eq!(auto_blacks, None)`). Behoben (d77db28): der Endpunkt nimmt seinem Regler den Spiegel, schreibt sonst keinen Auto-Tone-Zustand. **Warum gerade das — gemessen, zwei Auswege ausgeschlossen:** ohne den Spiegel-Verzicht loescht `clear_stale_auto_tone` den ausdruecklichen Nutzerwert (Sonde rot, `None` statt `0.2396...`); ein zweiter voller Sechser-Lauf ueberschreibt fuenf unberuehrte Regler. **Der Preis ist benannt statt weggeredet: nach einem vollen Auto-Tone-Lauf bleiben 5 von 6 Spiegel — ein echter gemischter Zustand**, und der Reparaturlauf **ueberschreibt einen danach von Hand gesetzten Wert** (gemessen: `-0.5` -> `0.0287...`). Das ist damit **keine Erfuellung von Klausel (2)**, sondern die guenstigste der drei Lesarten; die vollstaendige Loesung braucht ein Override-Konzept je Regler, das der Workspace nicht hat, und wird **nicht** behauptet. Gepinnt als Wächter auf das gemessene Fehlverhalten: `g16_the_state_repair_overwrites_a_later_manual_value_known_defect`. **Drei meiner eigenen Behauptungen waren falsch und sind an ihren Fundstellen zurueckgenommen:** (1) "NUR der Datenverlust-Test rot" — M-1 macht **vier** Tests rot, nicht einen; (2) "NUR der Self-healing-Test rot" — M-2 macht zwei; (3) `crates/lumina-gui/src/lib.rs` 12553 -> 12571, +18 — die 12553 waren der **uncommittete** Zwischenstand eines abgebrochenen Agentenlaufs, eine Zahl, die es im Git nie gab; nachgezaehlt 12561 -> 12572, +11, und das Ledger nennt jetzt ausdruecklich seine zwei falschen Fassungen. Dazu der Testname `g16_apply_auto_endpoint_never_creates_mixed_auto_tone_state` (behauptete "never mixed", laesst 5 von 6 zu) umbenannt nach dem, was er tut. **Zwei Extraktionen/Aktionen mit echtem Nutzen:** `src/tests/g16_auto_endpoint.rs` nimmt die Auto-Tone-Zustandstests aus `crates/lumina-gui/src/tests/g16_shortcuts.rs` (**314 -> 194 Z.** nachgezaehlt; `d77db28` nannte 314 als 509, das war der Arbeitsstand des abgebrochenen Laufs — **dieselbe Fehlerform wie bei der lib.rs-Zahl, an einer zweiten Stelle**), also unter die 500er-Schwelle, und `compute_auto_tone` gibt den Fingerprint nicht mehr zurueck (der einzige Aufrufer brauchte ihn nicht) — das ist -1 Zeile und zugleich eine Korrektur, denn der Doc-Kommentar dort behauptete, `auto_tone` rufe diese Funktion auf, was seit Klausel (1) falsch ist. **NOCH OFFEN, ausdruecklich nicht behoben:** (1) der Test `g16_auto_endpoint_after_auto_tone_keeps_its_value_across_a_stale_clear` ruft `clear_stale_auto_tone` **direkt** auf und faehrt den Load-Pfad nicht — eine extrahierte Testnaht ist noch kein Test der Produktion (Agents.md); (2) Klausel (2) ist fuer den Endpunkt weiterhin nicht vollstaendig erfuellt, siehe oben — **ABGESPALTEN als eigener Task `AUTO-TONE-ENDPOINT-MIXED-7` (Entscheidung des Eigentuemers 2026-10-02)**: die vollstaendige Loesung braucht ein Override-Konzept je Regler und damit Schema und Migration, was weit ausserhalb dieses Contract-Slices liegt; (3) die unabhaengige Verifikation des Korrekturlaufs 53e1ac0 steht aus. **STAND 2026-10-02, DRITTE Verifikationsrunde: BESTANDEN, ein Befund NIEDRIG, beide Zahlen von mir selbst nachgezaehlt.** Die dritte Runde hat bestaetigt, dass die Behauptungen der drei Korrekturlauf-Commits diesmal stimmen — lib.rs 12561 -> 12572, Suite 933/0/5, Testfunktionen 9 -> 14, Mutationszuordnungen M-1 = 4 Tests rot und M-2 = 2 (beide selbst nachgefahren, Namen exakt), **kein Test verschwunden**, Baseline-Anhebung ist Ladungsinhalt und keine Kompensation. **Der NIEDRIG-Befund ist dieselbe Fehlerform wie bei lib.rs, an einer zweiten Stelle:** `d77db28` behauptete fuer `crates/lumina-gui/src/tests/g16_shortcuts.rs` "509 -> 194", im Git stand dort **314 -> 194** — die 509 war wieder der Arbeitsstand des abgebrochenen Agentenlaufs. Zwei unabhaengige Stellen, an denen eine Zahl aus einem **nicht existierenden Zwischenstand** in einen Commit-Text geriet; beide sind jetzt korrigiert und als korrigiert benannt. **Die Runde hat ausserdem die von mir als Luecke benannte Testnaht bestaetigt, statt sie zu uebernehmen:** sie hat den Aufruf von `clear_stale_auto_tone` im Load-Pfad entfernt und der Test blieb **gruen** — die Luecke ist real, der Test ist fuer die Produktion vakuos, und genau das ist jetzt in `AUTO-TONE-ENDPOINT-MIXED-7` Abnahmepunkt (3). **Ebenfalls bestaetigt:** der Waechter `g16_the_state_repair_overwrites_a_later_manual_value_known_defect` ist ehrlich (rot, sobald das Defekt behoben ist), und die Abspaltung von `AUTO-TONE-ENDPOINT-MIXED-7` **versteckt keinen Befund**, sondern benennt ihn ausdruecklich — sie wurde als vertretbar beurteilt. **Was daraus folgt und NICHT behauptet wird:** Klausel (2) ist fuer den G-16-Endpunkt weiterhin **nicht** erfuellt (5 von 6 nach einem Endpunkt, 4 von 6 nach zweien — auch diese Reihenfolge ist in keinem Test gepinnt), die vollstaendige Loesung ist der neue Task, und der 5-von-6-Zustand ist laut, selbstheilend **und kostet** einen Handwert. Die drei Gates des Korrekturlaufs wurden von der Runde selbst gefahren: fmt 0, clippy -D warnings 0, GUI-lib 933/0/5, lumina-stages 8, sizes/plan 0, 0 Snapshots. **Der Task bleibt dennoch OFFEN** — nicht wegen eines Befunds, sondern weil die Verifikation ausdruecklich unter dem Vorbehalt abgibt, dass der Build-Agent die Abnahme erst nach eigener Pruefung spricht (DoD.md §7 Checkliste punkt 1-7 ist Bewertungssache, nicht Testzahl). Gates des Korrekturlaufs: fmt 0, clippy -D warnings 0, GUI-lib **933/0/5** (932 + 1 neuer Canary, nachgezaehlt, nicht angenommen), sizes/plan 0, 0 Snapshots geaendert, Mutationsdiff zurueckgenommen.
    **ABNAHME durch den Build-Agent (DoD.md §7, 2026-10-02, nach der dritten Verifikationsrunde BESTANDEN).** Die Verifikation gibt ausdrücklich ab, dass §7 Bewertungssache des Build-Agenten ist — hiermit beantwortet, jeder Punkt mit Beleg statt mit Ableitung:

    1. **End-to-End-Kette (Edit→Commit→Datei→Reload).** Über die **echte Binary**, nicht über einen Funktionsaufruf: `cargo test -p lumina-cli --test auto_tone_e2e` → **7 passed / 0 failed**, darin `process_auto_tone_persists_the_full_six_set_and_renders_the_golden` (Sechsersatz **und** gerendertes Byte-Golden, nicht nur Sidecar-JSON), `regenerate_is_skipped_as_fresh_after_process_auto_tone` (Reload leg: Freshness überlebt den kollektiven Lauf), `render_and_export_are_byte_identical_without_auto_tone` (Klausel 7) und `the_exit_codes_are_unchanged`. Die GUI-Kette separat: `auto_tone_commits_and_reloads` (`tests/basic_commit.rs`) fährt Datei + Reload in einem frischen `LuminaApp`.
    2. **Zeitbasierter Pfad.** `auto_tone` committet **synchron** (`commit_pending_slider_save([0, 0])` direkt nach `mark_recipe_dirty`), der 150-ms-Idle-Debounce bleibt also unbewaffnet: `auto_tone_commits_and_reloads` prüft `app.pending_slider_commit == None` **und** dass die Sidecar-Datei die Werte schon ohne manuellen Debounce-Drive trägt. Für den Endpunkt gilt dieselbe Commit-Disziplin (`mark_recipe_dirty` + `commit_pending_slider_save`), getrieben von `g16_apply_auto_endpoint_sets_only_its_field` über den Persistenz-Assert auf `lumina_sidecar::load_sidecar`.
    3. **Geprüfte Klassenmitglieder, vollständig.** Die sechs Regler `exposure/contrast/whites/blacks/highlights/shadows` (`AUTO_TONE_ADJUSTMENT_KEYS`, `crates/lumina-stages/src/auto_tone.rs`), die sechs Spiegel `auto_exposure…auto_shadows` (`mirrored_values`), `analysis_fingerprint` (`algorithm`, `version`, `input_fingerprint`), `enable_auto_tone`, `target_luminance`, die beiden Endpunkte `AutoEndpoint::{White,Black}` (`g16_apply_auto_endpoint_sets_only_its_field` + `g16_apply_auto_black_endpoint_sets_only_blacks`), der Reuse-Entscheid (`PersistedAutoTone::{ReuseIfComplete,AlwaysRecompute}`) und die Freshness (`auto_tone_is_fresh`). **Nachgezählt**, nicht behauptet: 3 Vertragstests in `crates/lumina-gui/src/tests/auto_tone_contract.rs`, 7 in `crates/lumina-gui/src/tests/g16_auto_endpoint.rs`, 7 in `crates/lumina-cli/src/tests/auto_tone_reuse.rs`, 38 in `crates/lumina-cli/src/tests/auto_tone_process.rs`, 7 e2e.
    4. **Log-Level je neuer User-Aktion — hier war eine LÜCKE, die ich selbst gefunden und geschlossen habe.** `grep` über den Testbaum fand **15** Referenzen auf `apply_auto_endpoint` und **keine einzige** in einer Log-Assertion: das Level war nirgends belegt, obwohl `info!` (`crates/lumina-gui/src/auto_endpoint.rs` -> `LuminaApp::apply_auto_endpoint`) die Zeile schreibt und `auto_tone` über `instrument_gui_action!` läuft. `DoD.md` §7.4 verlangt den Beleg, also ist er jetzt **neu**: `g16_auto_tone_actions_log_their_level_and_the_key` pinnt genau **eine** `INFO`-Zeile, die den **Schlüsselnamen** nennt (das ist der Support-Fall: welcher Endpunkt wurde benutzt). **Nicht-Vakuinität gemessen:** `info!` → `debug!` mutiert macht genau diesen Test rot (`got DEBUG: GUI interaction: apply_auto_endpoint whites=0.2396…`), Kontrolllauf 15/15 grün. Das ist die **fünfte** Mutation dieser Runde; die vier vorherigen (M-1, M-2 und die beiden Zahlenbefunde) sind getrennt dokumentiert.
    5. **Spez→Test-Mapping.** Klausel (1) eine Quelle der Wahrheit → `the_gui_writes_exactly_what_the_shared_writer_writes` (Differenz, 6 Regler + 6 Spiegel + Fingerprint-**Wert**, nicht nur -Algorithmus), `a_gui_auto_tone_recipe_is_fresh_for_the_shared_regenerate_predicate`, `the_tone_algorithm_literal_is_defined_once_and_never_hand_copied` (Struktur, vier contract-schreibende Crates). Klausel (2) alles-oder-nichts → `a_complete_state_with_a_matching_fingerprint_is_reused_verbatim`, `one_missing_mirror_forces_a_full_recompute_of_all_six`, `the_historic_two_slider_artifact_is_completed_to_the_full_set`, `reuse_reads_mirrors_only_so_a_user_value_cannot_become_an_auto_value`. Klausel (3) explizite CLI-Angaben zuletzt → `explicit_cli_values_win_last_for_all_six_and_keep_the_auto_mirror`, `each_explicit_flag_overrides_only_its_own_slider`, `the_six_auto_values_stay_reconstructible_after_an_override`. Klausel (4)/(5) Spiegel=auto, `adjustments`=effektiv, präsenzbasierte Freshness → `the_preset_wins_where_it_speaks_and_the_auto_value_survives_elsewhere`, `regenerate_is_skipped_as_fresh_after_process_auto_tone`. Klausel (6) Matching → `match_total_exposure_adds_onto_the_effective_exposure_only`, `match_total_exposure_uses_the_explicit_exposure_as_its_base`. Klausel (7) ohne `--auto-tone` alles unverändert → `without_auto_tone_no_auto_feature_is_written_and_the_render_is_unchanged`, `render_and_export_are_byte_identical_without_auto_tone`. Klausel (9) JSON-Round-Trip-Grenze → `crates/lumina-cli/src/tests/auto_tone_float.rs`. **Für Klausel (2) am G-16-**Endpunkt** gilt die Karte nicht als erfüllt**, siehe den abgespaltenen Task.
    6. **Gates, Kommandos + Ergebnis.** `cargo fmt --all -- --check` → exit 0. `cargo clippy -p lumina-gui --all-targets -- -D warnings` → exit 0. `cargo test -p lumina-gui --lib -- --test-threads=1` → **934 passed / 0 failed / 5 ignored** (933 + 1 neuer Log-Test, nachgezaehlt). `cargo test -p lumina-stages` → **8 passed**. `cargo test -p lumina-cli --test auto_tone_e2e` → **7 passed**. `sh scripts/check_file_sizes.sh` → exit 0. `sh scripts/check_plan_format.sh` → exit 0. `git status --porcelain -- crates/lumina-gui/tests/snapshots` → leer. **Nicht geprüft und nicht behauptet:** GPU-/wgpu-Parität, Paint-Schritt, VRAM-Pixel — benanntes Hardware-Gate, auf dieser headless Maschine nicht messbar (`Agents.headless.md` §1).
    7. **Dateigrößen-Regel.** `check_file_sizes.sh` exit 0. `crates/lumina-gui/src/lib.rs` 12561 → **12572** (+11), **bewusst** angehoben, Ladungsinhalt im Ledger aufgeschlüsselt — **mit einer echten Extraktion daneben**: `src/tests/g16_auto_endpoint.rs` (439 Z.) nimmt die Auto-Tone-Zustandstests aus `crates/lumina-gui/src/tests/g16_shortcuts.rs` (509 → 194 Z. im Arbeitsstand, **314 → 194** im Git, nachgezaehlt), beide unter der 500er-Schwelle. **Keine Kompensations-Löschung:** 9 → 15 Testfunktionen in den G-16-Dateien über die Commits hinweg, **kein Test verschwunden**, keine gelöschte Testdatei, kein `#[ignore]`, keine `should_panic`; der +11-Zuwachs in lib.rs ist aufgeschlüsselt und **zweimal** versucht worden ohne Verlust einer Aussage zu kürzen. **Clippy fand übrigens zwei echte Fehler, die ich übersehen hatte:** zwei beim Extraktions-Skript abgeschnittene Doc-Fragmente ("empty line after doc comment") — nicht mein Testdesign, sondern meine Werkzeugkette.




### PRIO: mittel

- [ ] **[PRIO: niedrig] MCP-PARITY-C-RAW-SERIES (Rest zu `MCP-PARITY-C`)** Einzige offene Fixture-Aussage ist die echte Mehrbild-RAW-Belichtungsreihe; Einzelpaar-RAW und Matrix T15–T25 sind historisch belegt. T26 wurde nicht ausgeführt, da die geeignete Serien-Fixture fehlt. Abnahme nach Aufnahme einer committeten geeigneten RAW-Serie: `LUMINA_MATRIX=1 cargo test -p lumina-cli --test matrix_mcp_parity_gate -- --ignored`; identische CLI/MCP-Ausgaben protokollieren.
- [ ] **[PRIO: niedrig] LRPAR-G09-CULL-REST (Rest zu `LRPAR-G09-CULL-IMPL-25`)** Die verpflichtende Stufe 1, Schema, CLI und GUI-headless-Logik sind geliefert und headless verifiziert; offen sind die optionale ONNX-Stufe 2 (F-078-Gewichtsgate) sowie Native-kittest-Golden. Abnahme der GUI-Golden: `cargo test -p lumina-gui --test kittest_snapshots -- --ignored` auf der gepinnten Referenzplattform; ONNX erst nach Lizenz-/Hash-Pin mit passender Fixture testen. Die bereits gelaufenen headless Gates stehen in der Headless-Tabelle.
- [ ] **[PRIO: niedrig] LRPAR-G14-DENOISE-WEIGHTS (Rest zu `LRPAR-G14-DENOISE-IMPL-20`)** Qualcomm NAFNet-DeNoise (`NAFNet-SIDD-width64`) ist seit 2026-10-23 als bedingter Kandidat dokumentiert: AI Hub deklariert MIT und veröffentlicht ONNX-Assets; ein konkreter hashgebundener Gewichtsgrant ist nicht belegt, Upstream-Klärung zu width32 ist offen. F-078 bleibt offen. Nächste Schritte: schriftliche Rechteklärung für genau das Qualcomm-Artefakt, Quelle/Hash-Pin, danach echter Modelllauf; keine Gewichte durch Stub ersetzen. Abnahme: `cargo test -p lumina-onnx --features onnx-rt --test denoise_ort`; bis zur Klärung keine Verteilung/kein Runtime-Download.
- [ ] **[PRIO: hoch] R5-DUST-23-GUI-REST (Rest zu `R5-DUST-23-FOLLOWUP`)** Eigenständiger Adapter-/GUI-Akzeptanzrest: echter GPU-Pfad plus Sicht-/Pointerverhalten, keine Wiederholung der abgeschlossenen headless Funktionsverifikation. Abnahme: `cargo test -p lumina-gui --test kittest_snapshots -- --ignored` auf gepinnter macOS/Metal-Referenz, danach R5-LOG-1-Lauf mit `RUST_LOG=trace`.
- [ ] **[PRIO: hoch] R5-BRUSH-24-GUI-REST (Rest zu `R5-BRUSH-24`)** Eigenständiger Adapter-/GUI-Rest für Native-Kittest-Golden und Hardware/Pointer/DPI; CPU/state/interaction-Abdeckung ist historisch belegt. Abnahme: `cargo test -p lumina-gui --test kittest_snapshots -- --ignored` auf gepinnter macOS/Metal-Referenz, danach R5-LOG-1-Lauf mit `RUST_LOG=trace`.
- [ ] **[PRIO: hoch] R5-MASKVIS-25-GUI-REST (Rest zu `R5-MASKVIS-25`)** Eigenständiger Adapter-/GUI-Rest für Overlay-Paint, Native-Kittest-Golden und Hardware/Pointer/DPI; State-/CPU-/Overlay-Logik ist historisch belegt. Abnahme: `cargo test -p lumina-gui --test kittest_snapshots -- --ignored` auf gepinnter macOS/Metal-Referenz, danach R5-LOG-1-Lauf mit `RUST_LOG=trace`.

### PRIO: niedrig


### PRIO: niedrig (Block A, nicht-LRPAR)


**Phase 6: Persistente AI-Masken**

_(keine offenen Tasks — F-082-FOLLOWUP BESTANDEN verifiziert 2026-09-02, 107p `onnx-rt`, wasm32 `onnx-rt` grün, Commit 49f4f76)_

**Phase 9: Optionale zentrale Indizierung (Post-MVP)**

_(keine offenen Tasks — F-064…F-067 BESTANDEN verifiziert 2026-09-02, Commit 1520ac5, Doc-only: minimaler Umfang/Cacheverweise, SQLite non-default `index` `assets`/`jobs`/`cache_refs` WAL/`user_version`/`.lumina/index/`, Rebuild/Locking/`integrity_check`/corrupt sichtbar/Sidecar-only, Löschsicherheit Delete→Rebuild identisch; `cargo check --workspace` grün)_

Ist-Stand 2026-09-02: kein Index-Modul im Workspace; CLI-`reindex` ist nur ein
Sidecar-Scan (zählt valide Sidecars, persistiert nichts); `feature/architecture/index.md` ist normativ vervollständigt und verifiziert.

**Phase 10: WASM und Plattformen — ENTFERNT 2026-09-04**

_(WASM/Browser ersatzlos gestrichen; F-069…F-071 entfallen, WASM-CI-Job gelöscht.
Historisch: F-069…F-071 Doku-first 2026-09-02, Commits 287fe75/e60a9ad)_

**Phase 10b: Generatives Entfernen + Erweitern (Post-MVP)**

_(keine offenen Tasks — GEN-EXPAND-1 BESTANDEN verifiziert 2026-09-02, Commit 46f6baf: Doku-first GenerativeEdit Felder sha256 Prompt/Seed inference_resolution canvas>100% region/mask ref artifact .lumina.zdata kind=generative_canvas atomar, Pipeline Decode→SourceActions→GenerativeEdit→Lens→Perspective→Crop, Identität/Veraltung analog AI-Masken, kein stiller Fallback, Capability lokal vs Cloud getrennt, Lizenz F-078; kein Code)_

### PRIO: mittel



## Block B – „Offene Rückfragen“

Tasks, bei denen eine User-Entscheidung/Klärung fehlt (Produkt-, Naming-,
Lizenz-/Schema- oder Übernahme-Fragen). Blockiert Block A nicht; sollte aber,
wo möglich, vor dem nächsten manuellen GUI-Test geklärt werden.

### PRIO: mittel


## Block C – „Nach dem nächsten manuellen GUI-Test“

Tasks, die erst nach dem nächsten manuellen GUI-Test sinnvoll/erforderlich
sind (Verifikations- und Abschluss-Tasks, die auf Testergebnissen aufbauen).

### PRIO: hoch

**Phase 8: Desktop-GUI (F-103, MVP)**

UI-Konventionen F-100 sind spezifiziert, verifiziert und für jede GUI-Arbeit
verbindlich — normativ in `feature/platform/cli-gui-wasm.md` (Abschnitt
F-100). SOLL für den MVP-Scope: ebenda „Desktop-GUI" und „Erster visueller
User-Test". Die implementierten Slices (Module, Develop-Sektionen, interaktive
Maskenwerkzeuge, Exportmodul, i18n, Presence/Vibrance, kittest-Snapshots) sind
unabhängig verifiziert; Details in Git-Historie und Feature-Dokument.

Vor F-103-N6 bleibt die nachstehende vollständige GUI-Checkliste offen; die
Review-Befunde (REVIEW-CORE-CROP-1, REVIEW-GUI-DEBOUNCE-1,
REVIEW-GUI-MASKRENDER-1) sind mit Marker-Kommentaren im Code implementiert;
die F-103-N6-Runde 1 hat eigene Befunde erzeugt (GUI-CLICK-ALL-17,
GUI-ROUTING-N6, GUI-INSTRDBG-17 — alle BESTANDEN verifiziert).

- [ ] **[PRIO: hoch] F-103-N6-GUI-COVERAGE-27 (User-Order 2026-09-23; manueller GUI-Gate offen)** Vollständiger manueller/visueller Abnahmelauf für alle noch nicht durch einen normalen `cargo test -p lumina-gui` (ohne `--ignored`) belegten Desktop-GUI-Fälle. Diese Task bleibt offen, bis ein Lauf auf einer Display-/GPU-Maschine mit **genau einer** App-Instanz, `RUST_LOG=trace` und Log-Redirect nach `/tmp/lumina_manual_2026-09-23.log` erfolgt; die headless CI ersetzt diesen Lauf nicht. Jeder Fall wird mit Testanker, Logauszug und Ergebnis dokumentiert.

  **P0 — DnD-/Persistenz-Risiko (`F-103-N6-DND-PERSIST-01`):** nativer Drop einer PNG/JPEG/WebP und einer CR3 in eine leere App; Drop von Quelle B nach geladener Quelle A; Exposure/Contrast-, Spot- und Maskenedit nach Drop; Sidecar adjacent zu B, frischer Neustart und byte-identisches Original; unlesbarer/unsupported Drop mit sichtbarem Fehler und unveränderter vorheriger Quelle. **Stand 2026-09-24:** Path-first-/Deferred-Decode-Produktionsfix, generation-gebundene Async-Scan-Nachbarplanung und headless Regressionen sind eingebaut; der echte OS-DnD-Nachweis bleibt offen.

  **1. Start, Eingabe und Shell:** leere App, Ordner-CLI, `--module library|develop|export`, `--fullscreen`, Open/Refresh/Up/Breadcrumb, nativer Datei-/Ordnerdialog, Drag-and-drop, Auto-Load des ersten Bildes, RAW-Orientierung/Metadaten, Status/Banner/Modal-Dialog, Toasts, Neustart und Rendern des tatsächlich geladenen Pfades.

  **2. Responsive Layout und View-Toggles:** 1024×720, 1280×800, schmale/verbreiterte Panels, linke Rail/Navigator, rechte Histogramm-/Develop-Panels, Filmstrip, Toolbar, Lights-out/Fullscreen sowie `Tab`, `Shift+Tab`, `L`, `F`; die Preview-Rechtecke müssen tatsächlich wachsen/schrumpfen, nicht nur State-Flags toggeln. Vorher/Nachher-Goldens und Clip/Overlap-Prüfung sind Pflicht.

  **3. Library-Navigation und Auswahl:** Grid/Loupe/Compare/Survey/People, Ordner-/Unterordnerbaum, Empty-State/CTA, Thumbnail-Größe, Filter, Name/Aufnahmedatum/Custom-Sortierung, Custom-Drag-Insertion, Click/Cmd/Shift-Auswahl, Doppelklick, Home/End/Enter/Esc, Rating/Flag/Color/Badges, Stacks (collapse/expand/select), R6-SCAN-1 erster Paint mit großem echten Verzeichnis.

  **4. Develop-Grundlagen:** alle sichtbaren Basic-Slider (Exposure, Contrast, Highlights, Shadows, Whites, Blacks), Doppelklick-/Alt-Scroll-/Section-Reset, Preset/Profil/Treatment, Auto-Tone, WB-Pipette, Before/After und Split, Original/Softproof/Clipping, Histogramm, Export/Apply/Save/Reset/Match/Regenerate; jede Änderung muss `Edit → Debounce/Commit → Sidecar → Fresh Reopen → Wert/Preview` belegen.

  **5. Farbe, Kurven und Detail:** Tone-Curve-Grafik (Punkt setzen/ziehen/löschen, Kanal Master/R/G/B), HSL/Color-Mixer, Point Color, Color Grading, Presence Texture/Clarity/Dehaze, Vibrance/Saturation, Detail (Schärfen, Rauschreduzierung, Rote-Augen-Picker/Detect/Apply/Remove/Clear), Denoise-Status, Effects (Vignette/Grain), Optics (Profil, Bokeh/Enable), Lens Blur und Generative-Controls inklusive Fehlermodi.

  **6. Geometrie und Preview:** Crop-Handles/Drag/Aspect/Enter/Esc, Straighten, Rotate/Mirror, Perspective/Auto-Upright, Zoom Fit/100–200 %/Fit Width, Pan/Navigator-Drag, Loupe-Vergleich, Draft/Stale/Ready/Failed-Badges, GPU-Present versus CPU-Fallback und sichtbare Route-Gründe.

  **7. Masken (`R5-BRUSH-24`/`R5-MASKVIS-25`):** Mask-Liste mit Anlegen/Mehrfachmasken, Auswahl, Pin-Liste, Sichtbarkeits-Auge, Umbenennen, Löschen, Reihenfolge, Duplizieren, Copy-vs-Duplicate-Gruppen, lokale Layerwerte; Brush-Größe/Weichheit/Fluss, `[`/`]`-Alias, live Kreis-Cursor, Pinsel/Gradient/Radial, Invert/Feather/Blur/Density, Show/Overlay-Farbe, Overlay nur bei geöffneter Masken-Ansicht, Modus „nur Pins aller Masken“ versus „volles Overlay der ausgewählten Maske“ und Panel-Hide mit vergrößerter Preview. Headless-Anker (`brush_marks_roundtrip_through_sidecar`, `g03_*`, `g11_*`, `brush_interaction`, `brush_management_controls_have_headless_structural_action_coverage`) decken Modell, Widget/Layout und echten Preview-Pointer ab; der kombinierte 1024×720-Snapshot bleibt ein explizit ignorierter Native-Adapter-Gate, der unabhängige Metal/Vulkan/DPI-Nachweis bleibt offen.

  **8. Spot-Heal (`R5-DUST-23-FOLLOWUP`):** Toolbar-Heal/Q, Quick/Generativ-Modus, Size/Feather/Opacity, `[`/`]`, Cursor, Klick-Dab, Spot-Auswahl, Liste nur mit ausgewählter Entfernung, Typ/Status, Parameter editieren, Löschen einzelner Spots, Detect/Apply, Distraction-Schalter, Visualize, Regenerate-Variante ohne Clone-Fallback und Reload; fehlende/veraltete/fehlerhafte generative Artefakte müssen sichtbar bleiben.

  **9. Metadaten, Sync und Export:** Keywords, Collections, Smart Collections, IPTC-Draft/Historie/Presets/Sync, Stack-/Batch-Aktionen, Sync Settings/Match Total Exposure/Previous, Merge, Exportziel/Format/Qualität, native Save-Dialog, PNG/JPEG/WebP/RAW-Export, Same-Path-Guard, Metadaten-Bake-In und Exportfehler.

  **10. Persistenz, Fehler und Nebenläufer:** Sidecar-Roundtrip über mindestens zwei virtuelle Kopien, History/Undo/Reset, Source-Changed/Missing/Corrupt/Stale, CAS-Konflikt, echter Zwei-Prozess-Schreibkonflikt, Cache löschen/rebuilden, fehlende Modelle, Panic/Recovery, Abbruch während Debounce sowie Original-Bytes byteweise unverändert.

  **11. Vollständigkeitsnachweis:** alle `ALL_GUI_ACTIONS` (aktuell 110) gegen eine gezeichnete, klickbare Fläche und einen PASS/FAIL-Test mappen; jede neue `GuiAction`/Enum-Variante erzwingt den Audit. Für visuelle Änderungen `egui_kittest`-Golden/PSNR/Histogram plus Vision-Review: alle neuen/geänderten Snapshots legt der Build-Agent **dem konfigurierten Modell selbst** vor (max. 10 Bilder je Durchgang); für unveränderte Modelle genügt der jeweilige headless Test nicht als manueller GUI-Erfolg.

  **Abnahme:** native PNG/JPEG/WebP/CR3- und DnD-Kette, Sidecar-Datei inspiziert, App neu gestartet, `cargo test -p lumina-gui`, gezielte Regressionstests, `cargo fmt --check`, Clippy mit `-D warnings`, `sh scripts/check_file_sizes.sh` und unabhängiger Verifizierungsbericht mit DoD-§7-Mapping; kein Befund darf als „manuell geprüft“ ohne Log-/Testbeleg gelten.

- [ ] **[PRIO: mittel] R2-GUIMOD-04b (→ G-10, Release: 1.0)** (nach manuellem Test + 04a-Zahlen): CPU-Draft-Drossel auf GPU-Pfaden entscheiden (throttlen vs. GPU-Histogramm 04c vs. lassen). Eingang: 04a-Messwerte aus F-103-N6.
- [ ] **[PRIO: mittel] R2-GUIMOD-04c (→ G-10, Release: 1.0)** (nach manuellem Test, Alternative zu 04b): Histogramm per GPU-Compute aus VRAM (1-KB-Readback statt Full-Frame-Analyse). Nur wenn 04a-Zahlen den Aufwand rechtfertigen; CPU-Pfad bleibt für Non-GPU (als Fallback, nicht WASM — WASM ist gestrichen).

- [ ] **[PRIO: hoch] F-103-N6 (→ Querschnitt, alle G, Release: 1.0)** Erster visueller User-Test: `RUST_LOG=trace cargo run -p lumina-gui` (Trace-Pflicht nach DoD §6) mit
  PNG/JPEG/WebP + nativen RAW per Pfad und Drag&drop; Preview + Exposure/
  Contrast ändern den Renderstand; Sidecar wird geschrieben und beim Neustart
  wiederhergestellt. Abnahme:
  reproduzierbare Befehle aus cli-gui-wasm.md + Log-Ausschnitt; unabhängiger Verifizierungs-
  Agent bestätigt F-100-Checkliste + Tests (BESTANDEN). Letzter Schritt vor
  Abschluss von Phase 8.
  **Bereit für Runde 2 (Release-Build, Stand 2026-09-18):** Geladet: GUI-GPU-AUDIT-17
  (verifiziert + committet — Audit-Test + Timing-Baseline), GPU-LENSFUN-Kern +
  -Wiring (gemeinsam verifiziert BESTANDEN; Commit nach SEC-Abgrenzung der
  Baseline — die Lensfun-Ausnahme steht nicht mehr im Runde-2-Log).
  GPU-RENDER-DENOISE/PREVIEW/EXPORT/MASK-19 sind bewusst dokumentiert-only
  (keine Implementierung vor Runde 2 — User-Entscheid 2026-09-18); Runde 2
  protokolliert deren CPU-Routen als bekannte Gaps, nicht als neue Befunde.
  GUI-PARITY-GOLDENS-18 empfohlen, nicht Pflicht (nur CI-ignorierte Goldens).
  R2-GUIMOD-04b/04c brauchen 04a-Zahlen aus Runde 2 (danach). UX-LOOK-18 sind
  unabhängig (Look beeinflusst Routing-/Perf-Messung nicht).

- [ ] **[PRIO: mittel] UI-VISION-REVIEW-70 (User-Auftrag 2026-10-17; Verfahren aus dem prime-photos-UI-Review derselben Session)** Der **visuelle Review am wirklich laufenden Fenster** wird hier als Verfahren festgeschrieben: geprüft wird die **gezeichnete Fläche** — Abstände, Klemmungen, angeschnittene Elemente, Rand-Symmetrie —, nicht nur State und Routing. **Verfahren, in der Reihenfolge des Laufs:** (1) **Fensterbild, kein Bildschirmbild:** `screencapture -x -o -l <windowID>` nimmt das ganze Fenster auf, auch verdeckte Teile; Fenster-ID und Position kommen aus `System Events` (`position`/`size`) oder einer kurzen Swift-Auflistung. Die Position wird **unmittelbar vor jedem Klick neu gelesen**, weil das Fenster zwischen zwei Aufrufen wandert. (2) **Messen statt schauen:** ein Bandscanner zählt je Bildzeile/-spalte die Nicht-Hintergrund-Pixel und liefert die Kanten von Textbändern, Trennlinien und Rändern als Zahlen; eine Trennlinie über die volle Breite erkennt man am Peak über alle Spalten. Für lesbare Detailbilder `sips -c <h> <w> --cropOffset <y> <x>` (plus `-Z` zum Vergrössern; `--cropOffset` nur mit X ≥ 1). (3) **Klicken:** egui-Fenster haben keine AX-Elemente, geklickt wird also per Koordinate über ein kleines Swift-Hilfsprogramm mit `CGEvent`; Voraussetzung ist das **Bedienungshilfen-Recht der verantwortlichen Terminal-App** (`CGPreflightPostEventAccess()` ist der schnelle Nachweis — ein `osascript`-„click at" scheitert an derselben Stelle trotzdem). Vor dem Klick muss das Zielfenster vorn sein, und **nach jedem Klick wird mit einem frischen Bild geprüft, dass er gewirkt hat**; ein wirkungsloser Klick gilt nicht als erledigt. (4) **Maßstab:** der Abstands-Rhythmus der App (Theme-Konstante, in prime-photos 8 px); jede gemessene Lücke wird dagegen gehalten, jede Abweichung ist ein Befund **mit Zahl**. (5) **Umfang je Lauf:** alle Seiten/Panels **und ihre Zustände** (leer/gefüllt/gescrollt) — der 0-px-Befund aus prime-photos war nur im gescrollten Zustand sichtbar, den kein State-Test erzeugt. (6) **Befunde bündeln:** erst die Liste (Seite, Element, gemessene Zahl, Bild), **dann eine** Fix-Welle — keine stückweise Optimierung, und nur die Befunde, kein „allgemein was optimieren". Die Hilfsprogramme sind Wegwerfwerkzeuge ausserhalb des Repos und werden **nicht** committet. **Beleg für den Nutzen (gemessen im prime-photos-Lauf 2026-10-17):** eine Überschrift klebte mit 0 px an der gescrollten Liste; eine Karte stand rechts 25 px statt links 9 px innen; ein Randabstand war 17 px statt 8 px. **Abnahme:** Befundliste mit **gemessenen Koordinaten** je Befund plus ein Vorher/Nachher-Bild je Befund; die Fix-Welle ist unabhängig verifiziert. **Commit und Push dieses Ablaufs laufen in einem Subagenten, mit vorherigem Pull** (Eigentümer-Vorgabe 2026-10-17; es ist die einzige Stelle, an der ein Subagent committet — die Regel „Kein Commit durch Subagenten" für Implementierungs- und Verifizierungs-Agenten bleibt unberührt). **Abgrenzung:** **kein** Abnahmekriterium für GUI-Features — Gate bleiben der headless Test und das kittest-Golden (Regel „Grenze der GUI-Testbarkeit"); der manuelle Lauf nach R5-LOG-1 (genau eine Instanz, `RUST_LOG=trace`, Log-Redirect) wird **nicht** ersetzt, und es wird **keine** GPU-Parität behauptet. Reine Sichtprüfung, keine neue Renderlogik.

## Abnahmekriterien

Die erste produktiv nutzbare Version muss mindestens Folgendes erfüllen:

- Ein RAW kann ohne zentrale Datenbank importiert, bearbeitet und exportiert
  werden.
- Nach dem Neustart werden Bearbeitungsrezept und virtuelle Kopien ausschließlich
  aus dem Sidecar wiederhergestellt.
- Zwei virtuelle Kopien desselben Originals können unterschiedliche Rezepte,
  Masken-Layer und Exporte besitzen.
- Eine gültige persistierte AI-Maske wird wiederverwendet und nicht ungefragt
  neu berechnet.
- Änderungen an Quelle, Modell, Decode-Kontext oder Maskenartefakt werden als
  veraltet erkannt.
- Vorschauen und Exporte sind über einen reproduzierbaren Render-Key cachebar.
- Das Löschen eines optionalen zentralen Indexes zerstört keine Bearbeitung.
- Originaldateien bleiben byteweise unverändert.
- Sidecar-, Migration-, Cache-, Masken- und virtuelle-Kopien-Tests sind durch
  einen unabhängigen Verifizierungs-Agenten bestätigt.

## Festgelegte Produktentscheidungen

Die fachlichen Entscheidungen stehen in [`feature/README.md`](feature/README.md)
und den verlinkten SOLL-Dokumenten; die **kurze Checkliste mit Begründung je
Entscheidung** steht oben in [Gepinnte Entscheidungen und
Absprachen](#gepinnte-entscheidungen-und-absprachen), die **offenen** Fragen in
der Tabelle darunter und die **zurückgezogenen** Ansätze im Abschnitt
`Verworfen`.

**Wohin eine neue Feststellung gehört** (User-Regel 2026-09-28, und es ist eine
Korrektur der früheren Fassung dieses Abschnitts): eine Entscheidung mit echten
Wahlmöglichkeiten wird **zuerst interaktiv gestellt** (Werkzeug `question`), dann
**hier** mit Begründung und Stand eingetragen, und die betroffene
SOLL-Zielsemantik in `feature/` **vor** dem Code geschrieben. Sie wird **nicht**
als unpriorisierte Entscheidungsliste gesammelt **und nicht** erst nachträglich
als Task beigelegt — der Task entsteht aus der Entscheidung, nicht umgekehrt.

## Headless-Arbeitsliste (wiedereingegliedert aus `Agents.headless-todo.md`, 2026-10-16)

Diese Liste sammelt die zuletzt headless arbeitbaren Tasks; die GPU/Hardware-geblockten
Tasks stehen in den Bloecken oben. `PIPELINE-CROP-EARLY-9` wurde am 2026-10-16 als nicht
machbar geschlossen (siehe `Verworfen`).

**Stand: 2026-10-16**

---

## PRIO: niedrig



## PRIO: niedrig

- [ ] **[PRIO: niedrig] GPU-MASKPLANE-CLEAR-REMEASURE (Release 1.0; User-Weisung 2026-10-02)** Die F-074-N10-Messung des Masken-Plane-Clears (`37,4 µs / 722 µs / 15,21 ms` @160×120/1280×853/6000×4000, `crates/lumina-gpu/benches/mask_plane_clear.rs`) ist **unter Fremdlast** entstanden — ein Spielprozess (`TransportFever3`, ~140–270 % CPU, Load 5–18) **und** paralle laufende andere GPU-lastige Vorgänge. Sie ist als **nach oben verzerrt** benannt, trägt aber die Entscheidung `GPU-CLEAR-TRANSITION` mit. **Aufgabe für eine ruhige Session:** denselben Bench auf einer **quieszenten** Maschine (kein Spielprozess, keine parallelen GPU-Jobs) neu fahren, Median/p95 für die drei Auflösungen plus Ein-Aufruf-Kontrolle dokumentieren, Lastangabe beilegen, und `feature/quality/performance-benchmarks.md` F-074-N10 auf die sauberen Zahlen aktualisieren (oder die Abweichung als Messunsicherheit festhalten). **Abnahme:** neue Zahl mit Kommando + Lastangabe; die Entscheidung `GPU-CLEAR-TRANSITION` wird gegen die saubere Zahl **erneut geprüft** — bleibt sie in ms-Größe, gilt sie unverändert, sonst wird sie revidiert. **Abgrenzung:** reine Nachmessung, keine Codeänderung.


- [ ] **[PRIO: niedrig] TEST-AUDIT-36 (fortlaufend, User-Regel 2026-09-26)** Redundanz-Audit der Testsuite: **höchstens einmal pro Woche** einen Build-Agenten starten, der prüft, ob es unnötige Tests gibt. Der Build-Agent **berichtet nur** und entfernt nichts eigenmächtig; jede Löschung läuft über SOLL → Implementierungs-Agent → unabhängige Verifikation. **Prüfkatalog:** (a) Tests, die eine Konstante auf sich selbst prüfen; (b) doppelte Abdeckung derselben Aussage in mehreren Dateien; (c) Tests, die eine Implementierungsentscheidung statt des Verhaltens pinnen; (d) Tests, die bei Wegfall der Logik nichts verlieren würden; (e) Tests ohne Fehlsignal — die grün bleiben, egal ob die Aussage stimmt; (f) **Laufzeit-Ballast**, der die Suite verlangsamt, ohne Aussage zu tragen (der 3:11-GUI-Shard ist das aktuelle Beispiel); (g) Assertions, die nach einem Refactor nur noch den Refactor beschreiben. **Abnahme:** ein priorisierter Befundbericht mit Test-Name, Datei und der konkret entfallenden Aussage; keine Löschung ohne Begründung; keine Reduzierung der Netto-Aussagenabdeckung — eine beim Audit entdeckte **Lücke** (Feature, Klausel, Fehlerpfad ohne Anker) wird als **neue offene Aufgabe** angelegt, nicht durch Streichen eines Nachbarn ersetzt. Grundlage: Policy in `Agents.md` §„Testabdeckungs-Politik". **Abgrenzung:** keine Produkt- oder Schemaänderung; der Audit verändert keine Abnahmegate und ersetzt keine DoD-Prüfung. **PARALLELITAeTS-BEFUND 2026-10-12 (F-P1, aus der Runde-3-Verifikation von `JSON-FLOAT-ROUNDTRIP`):** Audit-Runden, die **Mutationen** fahren, duerfen nicht parallel zu einem anderen Baum-veraendernden Lauf laufen — gemessen stand waehrend einer Verifikation fuer ~1 Minute eine uncommittete Logik-Mutation in `crates/lumina-core/src/lib.rs` im Tree. Kein Dauerschaden (Restore byte-identisch), aber die Messung des Parallel-Laufs ist fuer dieses Fenster nicht belastbar. Konkret fuer diesen Task: der Audit bekommt einen **exklusiven** Baum, wenn er mutiert; reine Lese-Runden duerfen parallel laufen. **RUNDE 2026-10-19 (abgeschlossen, Bericht in `feature/quality/test-audit-2026-10-19.md`):** gemessen gegen HEAD `c2c2ce1` (Downscale-Konsolidierung): (a) 0 wortgleiche Selbstvergleiche, aber die neue Preview↔Thumbnail-Histogramm-Parität ist vakuum (F1, durch Mutation M2 bewiesen); (b) B1+B2 erledigt (mcp-Exemplar jetzt mit eigenem Fehlsignal, Abweisungs-Familie auf einen kanonischen Pin kollabiert), B3/B5 offen, B4 auf vier Exemplare erweitert; (c)/(d) 0 Befunde; (e) drei Mutationen (M1 no-op-Klon, M2 Kernel-Rundung, M3 Fixture-Polaraet), alle byte-identisch restauriert (SHA-256 belegt); (f) `kittest_snapshots` unveraendert 56× `#[ignore]`/0,00 s, Methodenfalle `--list` sieht die kittest-Schicht jetzt mit; (g) 0 Befunde im Alt-Bestand, zwei Grenzfaelle im neuen Anker (F2 f32→f64-Pixel-Delta nur an glatten Fixtures, F3 Resampler-Gate nur 3 Dateien). Vorrunden-Luecken G1/G2/G3 erledigt; Prozessbefund „keine Tasks angelegt“ widerlegt (`ea82f05`). Neue Tasks: `PREVIEW-THUMB-PARITY-VACUOUS-76`, `DOWSCALE-ANCHOR-PIXEL-DELTA-77`, `RESAMPLER-GATE-SCOPE-78`. **WOCHENRUNDE 2026-10-25 (statisch, kein Testlauf):** 3 262 `#[test]` in 508 Dateien (gui 1092, core 578, cli 431, sidecar 345, onnx 269, mcp 119, gpu 114, …); exakt gleiche Testkoerper ueber Dateigrenzen: **2** Paare, beide aelter als die Modul-Splits. Befunde: **F1/F2 hoch** (falsch-gruen, siehe `AUTOFILL-LENS-TRANSPARENCY-101`), **F3 hoch** (Konstante gegen sich selbst: `crates/lumina-onnx/src/face/tests.rs` -> `shipped_descriptors_use_the_pinned_hashes`), F4-F9 mittel/niedrig (u. a. drei zeilengleiche Tests in `crates/lumina-onnx/tests/capabilities.rs`, doppelter `tensor_name_error`-Test, 49 kopierte CLI-/GUI-Fixture-Helfer), Luecken L1/L2 als neue Aufgabe angelegt, L3 (`<none>`-Zweig von `tensor_name_error`) nur berichtet. Keine Datei geaendert; keine Loeschung ohne eigenen Auftrag.

- [ ] **[PRIO: niedrig] CI-WATCH-1 (fortlaufend)** Nach jedem Push: CI-Runs prüfen (`gh run watch` / `gh run list --branch main`), Ergebnis im Tagesstand vermerken. **PRÜFREGEL (2026-10-19, gemessen am Lauf `39287220736`):** `gh run watch` kann einen Lauf **vorzeitig** als grün melden — der Lauf-Record stand auf `completed/success`, während der Job-Record „Rust fast checks" noch `in_progress` war (und `gh run watch` mit Exit 0 endete). Ein grünes `gh run watch` ist deshalb **kein** Beleg: das Verdict gilt erst, wenn **alle** Job-Records `completed` sind. Jobweise prüfen mit `gh api repos/reisi007/LuminaRust/actions/runs/<id>/jobs` — nicht nur `gh run view --json conclusion`. Bei Rot: als Next-Task in `Agents.todo.md` dokumentieren, NICHT still umsetzen (User-Vorgabe). Abnahme: jeder Push hat ein geprüftes CI-Verdict. **TAGESSTAND 2026-10-15:** Matrix-Sub-Slice abgeschlossen — alle Pushes grün; kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-16:** GUI-LIB-SPLIT-65 Slice 6–10 (lib.rs 11935 → 11459; neue Module `library_smart`, `generative_canvas`, `histogram_clip`, `file_browser_entry`, `tone_curve_regions`), der Abschluss von `PIPELINE-CROP-EARLY-9` (als nicht machbar in `Verworfen` eingetragen) und eine Kommentar-Korrektur gepusht — alle Runs grün (je Commit zwei Läufe: check/lint + test), kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-19:** Push `c2c2ce1` (Downscale-Konsolidierung) — CI-Run 7/7 grün (Detect Rust workspace, Documentation checks, Rust agent-harness, Rust tests excluding GUI, Benchmarks optional, Rust GUI tests, Rust fast checks; Matrix-Nightly ebenfalls grün). Das Golden-Gate-Nightly-Rot ist der dokumentierte Sollzustand (kein GPU-Adapter, `golden-references.md` §10). Kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-19 (Folge):** Die Pushes `c2c2ce1` (Downscale-Konsolidierung), `7d17392` (Test-Audit 2026-10-19) und `2df66a5` (GUI-LIB-SPLIT-65 Slice 16) sind je **CI 7/7 grün** (alle sieben Jobs `success`, Matrix-Nightly ebenfalls grün; per `gh run view` gegengeprüft) — kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-19 (Folge 2):** Der Push `c222746` (PREVIEW-THUMB-PARITY-VACUOUS-76) ist **CI 7/7 grün** (alle sieben Jobs `success` — Detect Rust workspace, Documentation checks, Rust fast checks, Rust agent-harness, Benchmarks (optional), Rust tests (excluding GUI), Rust GUI tests; Matrix-Nightly ebenfalls grün; per `gh run view` gegengeprüft) — kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-19 (Folge 3):** Der Push `e18d024` (DOWSCALE-ANCHOR-PIXEL-DELTA-77) ist **CI 7/7 grün** (alle sieben Jobs `success` — Detect Rust workspace, Documentation checks, Rust fast checks, Rust agent-harness, Benchmarks (optional), Rust tests (excluding GUI), Rust GUI tests; Matrix-Nightly ebenfalls grün; per `gh run view` gegengeprüft). Zusammen mit dem bereits vermerkten, ebenfalls per `gh run view` geprüften `c222746` (PREVIEW-THUMB-PARITY-VACUOUS-76, CI 7/7 grün) sind damit **beide** Pushes des Tages grün — kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-19 (Folge 4):** Der Push `a9201ee` (Resampler-Gate auf workspace-weite Frame-Suche) ist **CI 7/7 grün** (alle sieben Jobs `success` — Detect Rust workspace, Documentation checks, Rust fast checks, Rust agent-harness, Benchmarks (optional), Rust tests (excluding GUI), Rust GUI tests; Matrix-Nightly ebenfalls grün; per `gh run view` gegengeprüft). Zusammen mit den bereits vermerkten, ebenfalls per `gh run view` geprüften Pushes `e18d024` (DOWSCALE-ANCHOR-PIXEL-DELTA-77) und `c222746` (PREVIEW-THUMB-PARITY-VACUOUS-76) — beide **CI 7/7 grün** — sind damit **alle drei** Pushes des Tages grün — kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-19 (Folge 5):** Der Push `f1ec2f8` (GUI-LIB-SPLIT-65 Slice 17) ist **CI 7/7 grün** (alle sieben Jobs `success` — Detect Rust workspace, Documentation checks, Rust fast checks, Rust agent-harness, Benchmarks (optional), Rust tests (excluding GUI), Rust GUI tests; Matrix-Nightly ebenfalls grün; per `gh run view` gegengeprüft). Zusammen mit den ebenfalls per `gh run view` geprüften und grünen Pushes `c222746` (PREVIEW-THUMB-PARITY-VACUOUS-76), `e18d024` (DOWSCALE-ANCHOR-PIXEL-DELTA-77) und `a9201ee` (Resampler-Gate auf workspace-weite Frame-Suche) sind damit **alle vier oben genannten** Pushes grün. Ebenso ist der Push `ffd65e3` (Root-Agents.md auf Regeln reduzieren) **CI 7/7 grün** (per `gh run view` geprüft) — damit sind **alle acht** Pushes des Tages grün, kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-19 (Folge 6):** Der Push `3d7ee9c` (GUI-LIB-SPLIT-65 Slice 18) ist **CI 7/7 grün** (alle sieben Jobs `success` — Detect Rust workspace, Documentation checks, Rust fast checks, Rust agent-harness, Benchmarks (optional), Rust tests (excluding GUI), Rust GUI tests; Matrix-Nightly ebenfalls grün; per `gh run view` gegengeprüft). Zusammen mit den ebenfalls per `gh run view` geprüften und grünen Pushes `c2c2ce1` (Downscale-Konsolidierung), `7d17392` (Test-Audit 2026-10-19), `2df66a5` (GUI-LIB-SPLIT-65 Slice 16), `c222746` (PREVIEW-THUMB-PARITY-VACUOUS-76), `e18d024` (DOWSCALE-ANCHOR-PIXEL-DELTA-77), `a9201ee` (Resampler-Gate auf workspace-weite Frame-Suche), `f1ec2f8` (GUI-LIB-SPLIT-65 Slice 17) und `ffd65e3` (Root-Agents.md auf Regeln reduzieren) sind damit **alle neun** Pushes des Tages grün — kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-20 (Folge 7):** Der Push `d73f313` (vier Commits: `2334d00` Gate-Stufen, `cfc8ebd` Vision-Pflicht, `e27156c` Plan-Hygiene + 3k-Vorgabe, `d73f313` Lieferweg-Wortlaut) ist **CI 7/7 gruen** (alle sieben Job-Records `completed/success`, Lauf `39407006678`; die Job-Liste per `gh api .../jobs` geprueft, nicht per `gh run watch`). **TAGESSTAND 2026-10-20 (Folge 8):** Der Push `b706335` (Drift-Gate + Orphan-Gate in der CI) ist **CI 7/7 gruen** (Lauf `39411017898`) — damit sind die beiden Gates geschlossen, die lokal **nicht** lauffaehrig waren: der neue Schritt `sh scripts/check_test_orphans.sh lumina-gui` im Job „Rust GUI tests“ laeuft mit Exit 0 (kein „cannot conclude“), und `fast-6` (der Drift-Gate) laeuft im CI-Container ohne Python/Node gruen. Kein Rot, keine Task angelegt.
 **SPLICE-GPU-LIB-91 abgeschlossen und aus der Liste entfernt:** alle sechs geplanten Modulgrenzen sind geliefert (Basis-Shader, Schaerfe-Encoding, Masken-I/O, Post-/Geometrie-Encoding, VRAM-Pool, Routing/Validierung), `crates/lumina-gpu/src/lib.rs` ist von 4 767 auf **3 019** Zeilen gefallen. Der Routing-Schnitt (365 Z. neues `routing.rs`, `GpuError` + `unsupported_gpu_stages*` + Validierer) ging **nicht** in das vorhandene `validate.rs` (888 Z.) — das waere mit dem Block ueber die neue 1000-Zeilen-Grenze gelaufen; so braucht die Zerlegung keine neue Ratchet-Ausnahme. `mod routing;` ist unbedingt deklariert (GpuError existiert auch ohne `gpu`-Feature, per `cargo check` beider Konfigurationen belegt), die fuenf oeffentlichen Namen sind per `pub use` unveraendert. Verifikation: Runde 1 CHANGES REQUIRED (Baselines nicht gesenkt — berechtigt), Runde 2 APPROVED. `lib.rs` bleibt unter dem 1000er-Ratchet eine Ausnahme; eine weitere Zerlegung waere eine neue Task. 27 offene Aufgaben bleiben.
Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 9):** Der Push `531b797` (PERF-BASELINE-TABELLE-87 — die F-074-N4-Tabelle in `feature/quality/performance-benchmarks.md` neu aus `perf/baseline.json` (Stand 2026-09-17, `9f8c632`) gezogen; `analyze_tone` 64,7 % -> **4,6 %**, F-074-A3 zurueckgezogen) ist **CI 7/7 gruen** (alle sieben Job-Records `completed/success`, Lauf `39429079919`; die Job-Liste per `gh api .../jobs` geprueft, nicht per `gh run watch`). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 10):** Der Push `e8670bd` (FILE-SIZE-CEILING-3K-83 — Zeilen-Obergrenze 3000 als Ratchet mit Ausnahmeliste: `feature/quality/file-size-policy.md`, `scripts/check_file_ceiling.sh` (Regeln A–F), `scripts/file_size_ceiling.txt` mit den sieben Ausnahme-Eintraegen) ist **CI 7/7 gruen** (alle sieben Job-Records `completed/success`, Lauf `39449914355`; per `gh api .../jobs` geprueft). Der Job „Rust fast checks“ ist damit der **erste CI-Lauf des neuen 3000er-Gates im Container ohne python3/node** — der Schritt `sh scripts/gates.sh fast` laeuft mit 7 Checks gruen durch. Regel (D) ist nach dem Commit aktiv: der HINWEIS „die committete Baseline ist nicht lesbar“ ist verschwunden. Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 11):** Der Push `ddbbb72` (GUI-LIB-SPLIT-65 **Slice 20** — die sieben Methoden der G-03-Masken-Sichtbarkeits-Policy, 168 Zeilen, zeichengleich aus `crates/lumina-gui/src/lib.rs` in das bestehende Modul `crates/lumina-gui/src/mask_visibility.rs` verschoben; `crates/lumina-gui/src/lib.rs` 10685 -> **10517**, `crates/lumina-gui/src/mask_visibility.rs` 205 -> 374; beide Baselines (500er und 3000er) auf 10517 gesenkt) ist **CI 7/7 gruen** (alle sieben Job-Records `completed/success`, Lauf `39453314212`; per `gh api .../jobs` geprueft). Der Job „Rust GUI tests“ ist der **unabhaengige Nachweis der Testzahl** auf dem gepushten Artefakt (955 Namen, 950 passed, 5 ignored — vor und nach dem Schnitt identisch, per `diff` der Namenslisten belegt). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 12):** Der Push `3bc67d8` (chore(ratchet): acht veraltete 500er-Baseline-Eintraege auf den Ist-Wert senken — 519 Zeilen nicht gebunkerte Schrumpfung gebunkert; alle 67 Eintraege stehen jetzt exakt auf dem Ist-Wert) ist **CI 7/7 gruen** (alle sieben Job-Records `completed/success`, Lauf `39457056348`; per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 13):** Der Push `9b8730f` (fix(gate): 500er-INFO-Hinweis erfasst kommentierte Baseline-Eintraege) + `d17ac25` (feat(gates): Generator + Drift-Gate fuer die Faktenzahlen in der Prosa, `fast-8`, `MIN_CHECKS` 8) ist **CI ROT** (Lauf `39459843221`; der Matrix-Nightly-Lauf `39459843315` ist gruen). **Rot ist genau ein Job:** „Rust fast checks“ — dort schlug `fast-8` (`sh scripts/check_doc_facts_drift.sh`) fehl: `gen_doc_facts: FEHLER: git fehlt - die Zahl der committeten Goldens ist nicht ermittelbar` (3 von 5 Block(en) weichen ab). **Ursache:** der Job laeuft im Container `ghcr.io/reisi007/luminarust/lumina-ci:latest`, in dem das **`git`-Binary fehlt**; der `goldens`-Block zaehlte mit `git ls-files`. Lokal ist `fast` 8/8 gruen — die Verifikation hatte `git` und konnte den Container-Fall nicht sehen. **Als Position festgehalten** (nicht still umgesetzt): die Zahl kommt aus dem committeten `scripts/golden_ref.lock` (`goldens.count`), der Drift-Gate vergleicht weiterhin byteweise, und `scripts/golden_ref_test.sh` (Job „Documentation checks“ **ohne** Container, hat `git`) prueft das Lockfile gegen den echten Git-Index — damit ist die Kette in **beide** Richtungen geschlossen. **Unveraendert:** die Richtung-Pruefung (D) des 3000er-Gates laeuft im Container nicht (dokumentierte Grenze: der Gate schreibt den HINWEIS statt still gruen zu sein) — lokal laeuft sie, und die committete Anhebung faellt ueber den Review des Baseline-Diffs auf. **Der Folge-Push `a7c3d42` (docs(anker): UXG/UXD-Lueckentabelle auf Pfad + Symbol umstellen) ist aus demselben Grund rot** (Lauf `39461851111`, derselbe Job „Rust fast checks“, dieselbe Ursache) — der Fix lag zu diesem Zeitpunkt noch uncommittet im Arbeitsbaum. **Folge:** der rote Lauf ist kein zweiter Fehler, sondern derselbe; er ist mit dem DOC-ZAHLEN-Fix-Commit erledigt. **Pushes der Parallel-Session am selben Tag** (`888dc09`, `f899842`, `a27748d`, `dff6948`): je **CI 7/7 gruen** (alle Job-Records `completed/success`; Laeufe `39438203346`, `39439783056`, `39441034298`, `39443617249`). Fuer `8162323` existiert **kein CI-Lauf-Record** — der Lauf wurde beim naechsten Push **superseded** (GitHub bricht laufende Laeufe eines Branch-Pushes ab); der Matrix-Nightly-Lauf `39446400523` ist gruen, und der Inhalt dieses Commits ist im Baum des spaeteren Laufs enthalten und dort durch die uebrigen Jobs abgedeckt.
 **TAGESSTAND 2026-10-20 (Folge 14):** Der Push `b7c6851` (fix(gates): Goldens-Zahl aus dem committeten Lockfile statt aus dem Git-Index) ist **CI 7/7 gruen** (alle sieben Job-Records `completed/success`, Lauf `39465424303`; per `gh api .../jobs` geprueft) — **damit ist der rote Zustand behoben**: der Job „Rust fast checks“ laeuft im Container `lumina-ci:latest` **ohne `git`** durch, weil der `goldens`-Block jetzt aus dem committeten `scripts/golden_ref.lock` liest. Der Push `92036aa` (docs(security): 24 Oberflaechen enumerieren) war ebenfalls **rot** (Lauf `39464659259`, derselbe Job, **dieselbe** Ursache) — die rote Kette umfasste damit **drei** Commits (`d17ac25`, `a7c3d42`, `92036aa`), alle mit derselben Ursache. Kein Rot, keine Task angelegt. **Welle 1 des `DOC-ZAHLEN-DRIFT-82` ist damit abgeschlossen** (der Plan haelt die Bedingung fest: „erst mit einem gruenen CI-Lauf“).
 **TAGESSTAND 2026-10-20 (Folge 15):** Der Push `e3e4bd2` (docs(anker): Welle H — haengende Anker auf Pfad + Symbol/Testname umstellen) ist **CI 7/7 gruen** (alle sieben Job-Records `completed/success`, Lauf `39468828349`; per `gh api .../jobs` geprueft). **Welle H** hat die vier haengenden Anker plus den Anker ohne Datei aufgeloest und dabei — im Rahmen der Welle — **alle** `.rs`-Zeilenanker der vier Dateien mitgezogen (vorher 30/71/43/10 -> **0/0/0/0**); die Verifikation hat 99 Pfad→Symbol-Paare einzeln auf Existenz **und Relevanz** geprueft und **4 Fehlzuordnungen** gefunden (3 × `high`), die in Runde 2 behoben sind. Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 16):** Die beiden Kleinheits-Commits `4dbd362` (docs(gates): handgeschriebene Dateizahl aus den Gate-Headern entfernen — `scripts/file_size_ceiling.txt` und `scripts/check_file_ceiling.sh` nannten „830 .rs-Dateien“ und damit eine Zahl, die still altert) und `41a4bed` (docs(regeln): `DoD.md` §9 verweist auf die Erzeugungs-Regel fuer Faktenzahlen) sind **je CI 7/7 gruen** (Laeufe `39470203190` und `39470249163`; per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt. **Anmerkung zur Reihenfolge:** die Bereinigung der `830` kam **nach** der Generator-Welle 1 — die Zahl steht jetzt im generierten Block `population`; haette man sie vorher entfernt, waere die Welle auf einer Basis entstanden, die es gleich nicht mehr gibt.
 **TAGESSTAND 2026-10-20 (Folge 17):** Die beiden Commits `e8b6bd1` (feat(gates): vier weitere Fakten-Bloecke — `mcp-tools`, `perf-baseline`, `crates`, `surfaces` — Hygiene-Gate `fast-9` (Vollstaendigkeit, Determinismus, Git-Freiheit), `MIN_CHECKS` 8 -> 9) und `7d773c0` (docs(skills): Marker-Stand auf die Welle-2-Wirkung nachziehen) sind **CI 7/7 gruen** (Lauf `39472677182`; per `gh api .../jobs` geprueft). **Damit ist zum ersten Mal ein Gate im CI-Container **ohne `git`** gelaufen, das genau das prueft** (`fast-9` baut die PATH-Sandbox ohne `git*` und vergleicht byteweise) — und `fast-1` (`cargo fmt`) wurde dort **echt** geprueft (die Luecke, die der Implementierer offengelegt hatte, ist damit geschlossen: lokal war `fast-1` nur ein Stub). Der Folge-Commit `67acb2f` (docs(anker): Welle P — Teilpfade und Nicht-Code-Anker auf Pfad+Symbol umstellen) ist ebenfalls **CI 7/7 gruen** (Lauf `39473068950`; per `gh api .../jobs` geprueft) — Teilpfade **228 -> 199**, nicht aufloesbare Teilpfade **51 -> 28**, davon **mit** Zeilennummer **6 -> 0** ✓. Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 18):** Der Plan-Commit `70c2649` (docs(plan): Tagesstand 14-17, veraltete Zahlen und Welle-3-Restpunkte) ist **CI 7/7 gruen** (Lauf `39476482251`; per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 19):** Der Plan-Commit `070cc75` (docs(plan): erste Schnitte der sechs Slicing-Tasks eintragen — slice-fertig, mit 36 gemessenen Abweichungen von der Plananalyse) ist **CI 7/7 gruen** (Lauf `39479162176`; per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 20):** Der Push `3d17a22` (docs(anker): Welle N — mehrdeutige nackte Anker auf Pfad + Symbol umstellen; 140 umgestellt, mehrdeutig 338 -> 198, **114** neue Pfad→Symbol-Paare, 114/114 belegt) ist **CI 7/7 gruen** (Lauf `39481686896`; per `gh api .../jobs` geprueft). Der Folge-Commit `f28f894` (docs(gates): Budget der fast-Stufe als Schranke plus datierte Messung — die Angabe „unter zehn Sekunden“ war durch den neunten Check **überholt** und wurde still) ist **CI 7/7 gruen** (Lauf `39483013401`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 21):** Der Plan-Commit `cbaf96f` (docs(plan): Plan-Anker aufloesen, UX-Gap-Testzuordnung, Tagesstand 20) ist **CI 7/7 gruen** (Lauf `39484030880`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 22):** Der Push `83c9552` (docs(anker): Welle F — pfadgebundene Zeilenanker auf Pfad + Symbol umstellen; 70 neue Pfad→Symbol-Paare, 36/36 Stichproben mit Satzbezug, EOF-Pruefung ueber alle Ankerformen: 1 Verletzung — im Plan selbst — behoben) ist **CI 7/7 gruen** (Lauf `39490101850`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). **Der Plan traegt jetzt null Zeilenanker** ✓ (die 12 verbliebenen hat der Orchestrator aufgeloest — der Anker auf Zeile 3735 in `crates/lumina-gui/src/lib.rs` zeigte auf ein umgezogenes Modul: `crates/lumina-gui/src/auto_endpoint.rs` -> `LuminaApp::apply_auto_endpoint` ✓). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 23):** Der Push `7f63cde` (docs(anker): Welle A — datierte Audit-Dokumente auf nachpruefbare Bezuege bringen; 142 Anker, 124 rekonstruierbar, 7 umgestellt, 11 nicht rekonstruierbar — der auditierte Arbeitsbaum existiert in keinem Commit) ist **CI 7/7 gruen** (Lauf `39492993396`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Der Folge-Commit `63c4674` (docs(anker): die letzten sechs Zeilenanker in den Generator-Dokumenten — mcp-server.md, local-gates.md, performance-benchmarks.md — aufloesen) hat den Lauf `39494503090`. Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 24):** Der Push `63c4674` (docs(anker): die letzten sechs Zeilenanker in den Generator-Dokumenten — `feature/platform/mcp-server.md`, `feature/quality/local-gates.md`, `feature/quality/performance-benchmarks.md` — auf Pfad + Symbol aufgeloest; die drei Dokumente tragen jetzt **null** Zeilenanker) ist **CI 7/7 gruen** (Lauf `39494503090`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 25):** Der Push `449b49f` (docs(anker): unbalancierten Backtick in `mcp-parity-c-soll.md` §2 Pkt. 3 reparieren — ein einzelner fehlender Backtick aus der Welle F; das Dokument hat jetzt 2152 Backticks und **null** Bloecke mit ungerader Zahl; zugleich der Welle-N-Rest-Commit `a3963e4`, der keinen eigenen CI-Lauf hat, weil beide Commits in **einem** Push landeten) ist **CI 7/7 gruen** (Lauf `39496704470`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-20 (Folge 26):** Der Push `fa6411c` (docs(anker): Anker-Rest — die nicht aufloesbaren Teilpfade auf vollstaendige repo-relative Pfade umgestellt; 116 entfernte Pfadanker-Vorkommen gegen 121 hinzugefuegte, 90 neue Zielpfade geprueft, 0 neu eingefuehrte nicht aufloesbare Pfade) ist **CI 7/7 gruen** (Lauf `39506819692`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-21 (Folge 27):** Der Push `ad63baf` (docs(ux-parity): die 15 UXG/UXD-Zeilen mit Laufbelegen unterlegen — je Zeile ein gelaufener, gruener Test als Beleg, die 7 "teilweise geschlossen"-Zeilen in offene SOLL-Entscheide ueberfuehrt ohne Produktentscheidung, die SOLL-/Task-Anker-Spalten der 8 geschlossenen Zeilen nachgezogen, die 8 Restbefunde mit klar als Vorschlag markierten Testnamen, die falsche CI-Praemisse zu den adapter-pflichtigen Golden-Tests korrigiert; Runde 1 CHANGES REQUIRED mit einem Punkt, Runde 2 APPROVED) ist **CI 7/7 gruen** (Lauf `39685650914`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-21 (Folge 28):** Die beiden Pushes `6945256` (refactor(sidecar): Testblock aus zdata.rs in zdata/tests.rs verschoben — der inline Testblock wanderte als reiner Move in die neue Datei, `zdata.rs` 3333 -> 2015 Zeilen, damit faellt die Datei unter die harte 3000er-Grenze und der 3000er-Eintrag ist nach Regel E entfernt; 48 von 48 Testnamen unveraendert, die Laufzeitliste byteweise identisch, der whitespace-normalisierte Byte-Diff leer auf 1317 vs. 1317 Zeilen) und `dae4674` (fix(cache): settings.json mit einer harten Groessengrenze lesen — SEC-4 der Sicherheits-Welle 1; die Datei wird jetzt ueber einen begrenzten Leser mit 64 KiB plus Kontrollbyte gelesen, ein 4-GiB-Sparse-Datei-Test belegt RSS 2,8 MiB statt 412 MiB, der Fehler propagiert in der Vererbungskette) sind **CI 7/7 gruen** (Lauf `39695286806`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-22 (Folge 29):** Die beiden Pushes `14704f3` (fix(mcp): Zeilenlaengengrenze im stdio-Transport — SEC-1 der Sicherheits-Welle 1; die stdin-Zeile wird jetzt an der Lesekante gekappt, ein 100-MiB-Frame mit Marker endet am echten Binary mit exit 0 und einem JSON-RPC-Fehler -32700 statt SIGABRT, der Peak-RSS bleibt auf Basisniveau; vier Verifikationsrunden, drei Mutationstests) und `a80f081` (docs(security): vier Befunde der Welle 1 ins Register — SEC-1 und SEC-4 belegt und geschlossen, SEC-2 und SEC-3 am Falsch-Positiv-Gate widerlegt und mit Grund zurueckgenommen) sind **CI 7/7 gruen** (Lauf `39708046809`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-22 (Folge 30):** Der Push `51f2770` (refactor(gpu): Basis-WGSL-Color/Tone-Shader in ein eigenes Modul — der Block wanderte als reiner Move nach `crates/lumina-gpu/src/color_tone_shader.rs`, `lib.rs` 4767 -> 4650 Zeilen, `SHADER_SRC` wurde `pub(crate)` ohne oeffentlichen Pfadwechsel; der WGSL-Text ist byteweise identisch, SHA-256 des Textes vor und nach `f82862bc…`) ist **CI 7/7 gruen** (Lauf `39712579206`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-22 (Folge 31):** Der Push `633c933` (refactor(gpu): DOG-Stufe in ein eigenes Modul — der Block wanderte als reiner Move nach `crates/lumina-gpu/src/stages/dog.rs`, `stages.rs` 3264 -> 3102 Zeilen; die Positionsbedingung (\\`pub mod dog;\\\` unmittelbar nach dem textual gescopten \\\`macro_rules! common_src\\\`) ist positiv **und negativ** belegt — mit der Deklaration vor dem Makro scheitert der Build mit \\\"cannot find macro \\\`common_src\\\`\\\"; der expandierte WGSL-Text ist dreifach unabhaengig bestaetigt, SHA \\\`be03da75…\\\` vor und nach identisch) ist **CI 7/7 gruen** (Lauf `39714723959`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-22 (Folge 32):** Der Fix-Commit `ec19af9` (fix(cli): Import der Kurven-Typen im gpu-getesteten CLI-Testmodul — der Schnitt `81c21dd` hatte die neun Importnamen des Farbblocks aus dem Crate-Root mitgenommen, und `crates/lumina-cli/src/tests/local_mask_gpu.rs` holte drei davon unqualifiziert ueber den Glob `use super::*;` in `tests/mod.rs`; das Modul ist `#[cfg(feature = "gpu")]`-gegatet, deshalb blieb der Default-Build lokal gruen und die CI mit `--features gpu` rot — 10x E0422 in zwei Jobs) ist **CI 7/7 gruen**; **die beiden vorigen Laeufe `81c21dd` und `b84f73c` sind rot geblieben** (derselbe Fehler, dort noch ungefixt) und stehen damit in der Historie — sie sind die Vorgeschichte dieses Fixes, nicht ein offenes Problem (Lauf `39720533453`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-22 (Folge 33):** Der Push `481ce4b` (refactor(sidecar): Metadaten, Artefakt-Status und Sidecar-I/O in eigene Module — die drei **Restbloecke** des Sidecar-`lib.rs` wandern als reine Moves nach `crates/lumina-sidecar/src/metadata.rs` (440 Z.), `crates/lumina-sidecar/src/artifact_status.rs` (227 Z.) und `crates/lumina-sidecar/src/sidecar_io.rs` (429 Z.), `lib.rs` **4040 -> 3099** Zeilen; alle drei Bloecke **byteweise identisch**, die oeffentliche Flaeche per Rustdoc **349 = 349** Item-Seiten unveraendert, **287/344** Tests gruen mit **identischen** Namenslisten, **sieben** Fremd-Crate-Builds gruen; **ein** Commit, weil die Entfernungen in `HEAD` nebeneinander liegen (I/O 2844–3286, Artefakt-Status 3288–3482) und die generierten Bloecke baumweit sind; der Verifikator beanstandete drei falsche Ledger-Zahlen, deren Korrektur wiederum seine eigene Doppelzaehlung der Leerzeilen war — uebernommen wurde −383 + 19 − 2 = −366) ist **CI 7/7 gruen** (Lauf `39750049842`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-22 (Folge 34):** Der Plan-Commit `a429310` (docs(plan): Tagesstand 33, Sidecar-STAND und fuenf Wellen-Positionen — die Offen-Liste des ersten Sidecar-Schnitts war nach `481ce4b` falsch und ist auf den Wertetypen-Block geschnitten, die Folgen 27–32 tragen jetzt ihre echten Commit-Tage statt alle 2026-10-20, und die fuenf Wellen-Positionen `SEC-WELLE-2A..2E` sind angelegt; `check_plan_format.sh` 38 offene Aufgaben, 0 abgehakt) ist **CI 7/7 gruen** (Lauf `39756943703`, alle sieben Job-Records `completed/success`) und der Register-Commit `481f445` (docs(security): sechs belegte Befunde der Welle 2 — `SEC-5` bis `SEC-10` stehen als `BELEGT`/`in Welle` in §3 mit Reproduktion und Messwerten, ihre Fundstellen wurden vom Orchestrator unabhaengig gegen **HEAD** nachgeprueft — `std::fs::write` in `matrix_run.rs` gegen den `tempfile`+`persist`-Pfad von `encode_and_stage`, `"force"` in `ALLOWED`, `create_dir_all` im Batch, sechs `display()`-Stellen im Guard, `<output>.lumina.json` per Suffix-Append in `sidecar_path_for`; die Zusammenfassung nennt zehn Eintraege und die Aufteilung nach Mechanismus) ist **CI 7/7 gruen** (Lauf `39757197965`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-22 (Folge 35):** Der Plan-Commit `b3db422` (docs(plan): Tagesstand 34 — beide Doku-Pushes des Vormittags mit ihren Lauf-IDs) ist **CI 7/7 gruen** (Lauf `39757887692`, alle sieben Job-Records `completed/success`) und der Push `ca21cfe` (fix(stages): Matrix-Export atomar veroeffentlichen — **SEC-5 der Sicherheitswelle 2a**; der Export geht jetzt ueber `write_bytes_atomically`, denselben tempfile+`persist`-Schritt, den der Golden-Pfad seit immer nutzt, `encode_and_stage` delegiert darauf; mit dem echten Binary gemessen: Symlink-Opfer byteidentisch und Eintrag wird regulaere Datei mit neuer inode, Hardlink-Quelle byteidentisch mit `nlink` 2 → 1, **0** Zwischenstaende bei **2.092.129** Messpunkten gegen **4.892** gerissene Beobachtungen der Positivkontrolle mit `std::fs::write`; ein Regressionstest, in drei Mutanten rot; die CR3-Gates `matrix_e2e` (106,69 s) und das MCP-Paritaetsgate (206,64 s) sind gelaufen — der Implementierer hatte sie als „RAM-schwer" abgehakt, sie liefen aber in 0,20 s bzw. 3,4 min; bewusst benannte Nebenfolge Modus 0600 statt 0644, Vereinheitlichung ist SEC-8/2d) ist **CI 7/7 gruen** (Lauf `39761430502`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-22 (Folge 36):** Der Push `09adde9` (refactor(sidecar): Wertetypen-Block in `value_types.rs` — der an Symbolen verankerte Block `DecodeFingerprint` bis `impl EditRecipe::effective_perspective` umfasst 1806 Zeilen; `lib.rs` 3099 -> 1322, das neue Modul 1863 Zeilen; genau ein notwendiges Sichtbarkeitstoken (`default_color_grading_blending` -> `pub(crate)`), 75/75 öffentliche Namen erhalten, Testnamenlisten byteidentisch, beide Ratchet-Rotproben und die negativen Sichtbarkeitsproben wie erwartet; der Verifikator hat die Plan-Grenze 288-2094 widerlegt (keine Anker, 1807 Zeilen) und den Baseline-Kommentar off-by-one korrigiert) ist **CI 7/7 gruen** (Lauf `39763794911`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-22 (Folge 37):** Der Plan-Commit `dc85e66` (docs(plan): Schnitt 10 schliessen, Tagesstand 36 — `SPLICE-SIDECAR-LIB-90` entfernt, alle zehn Sidecar-Schnitte abgeschlossen) ist **CI 7/7 gruen** (Lauf `39772780199`, alle sieben Job-Records `completed/success`, einzeln per `gh api .../jobs` geprueft) (Lauf `39772780199`; alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft). Kein Rot, keine Task angelegt.

 **TAGESSTAND 2026-10-23:** Der Push `92bd936` (fix(gpu): die sieben Paritaets-Helfer als Module unter `crates/lumina-gpu/tests/parity_depth/mod.rs` und `crates/lumina-gpu/tests/parity_source_action/mod.rs` einhaengen, damit Cargo sie nicht als eigenstaendige Integrationstest-Targets kompiliert) ist **CI 7/7 gruen** (Lauf `40017637560`; alle sieben Job-Records `completed/success`, einzeln per `gh api repos/reisi007/LuminaRust/actions/runs/40017637560/jobs` geprueft — `gh run watch` meldete trotzdem Exit 101 mit der Annotation „Benchmarks (optional)", obwohl dessen Job-Record `success` ist: das ist die dokumentierte Watch-Ungenauigkeit, kein Rot). Kein Rot, keine Task angelegt. **Nebenbefund (Umgebung, keine Aufgabe):** das Projekt-Dateisystem war zu Beginn des Tages zu 100 % belegt (`/projects` 98 G, 2,1 M frei; `target/debug/deps` 59 G, `target/debug/incremental` 5,7 G); der reine Build-Cache `target/debug/incremental` wurde geloescht (5,7 G frei), ein `cargo test -p lumina-gpu --lib` scheiterte zuvor an der Plattengroesse — dieser Fehlschlag ist kein Testergebnis und wird nicht als solches gefuehrt.
 **TAGESSTAND 2026-10-23 (Folge):** Die Pushes `929daa2` (NOISE), `87242b0` (Vignette), `12fe109` (Grain), `7f41c42` (Red-Eye), `02eb020` (Spot-Heal), `bc1c885` (Sharpening) und `e5fc64c` (Color) samt ihren Doku-Commits `c7a45cc`, `1144cbf`, `7bc9630`, `5ba6dcf`, `0659db4`, `0af6106` sind **gruen** (je **CI 7/7**, alle sieben Job-Records `completed/success`, per `gh api .../jobs` geprueft; die Laeufe `40021580013`, `40022253826`, `40022909556` stehen fuer die gebuendelten Pushes, die spaeteren Commits laufen im jeweils naechsten Lauf mit). **Ein Rot dazwischen:** der Lauf `40019179763` (Push `c7a45cc`) war im Job „Rust fast checks" rot, weil der NOISE-Schnitt `scripts/file_size_baseline.txt`/`file_size_ceiling.txt` aenderte, ohne die generierten Bloecke `file-sizes` und `population` in `feature/quality/file-size-policy.md` neu zu erzeugen (`fast-8`, 2 von 12 Bloecken driften) — kein Code-Defekt; behoben mit `sh scripts/regen_block.sh` in `1144cbf` (Lauf `40020131270` gruen), und seither ist die Regeneration Teil jedes Stage-Schnitts. **`SPLICE-GPU-STAGES-94` ist damit abgeschlossen und aus der Liste entfernt:** alle neun Stufen (DOG, NOISE, Vignette, Grain, Dehaze/Dark-Channel, Red-Eye, Spot-Heal, Sharpening, Color) liegen als eigene Module unter `crates/lumina-gpu/src/stages/`, `crates/lumina-gpu/src/stages.rs` ist von 3 264 auf **492** Zeilen gefallen, der 3000er-Ausnahme-Eintrag ist entfallen; jeder Schnitt hat einen eigenen Commit, eine unabhaengige Verifikation (vier davon mit einer zweiten bzw. dritten Runde, weil die erste nur die Messung schuldig blieb — nie wegen eines Defekts) und den Beleg, dass der expandierte WGSL-Text byteweise identisch ist. 28 offene Aufgaben bleiben.
 **TAGESSTAND 2026-10-24:** Die Folge-Pushes des 23. sind **CI 7/7 gruen**, einzeln per `gh api .../jobs` gegen die Job-Records geprueft: `742c47a` (Post-/Geometrie-Encoding, Lauf `40026387572`), `d9a09b1` (Meta-Kommandos, Lauf `40028302862`) und `71accd4` (Masken-Kommandos, Lauf `40029205087`; alle sieben Jobs `completed/success`). Kein Rot aus diesen Pushes. **Fremdstand im selben Baum:** eine parallele Sitzung hat `151d45c` (1000-Zeilen-Ratchet ersetzt die bisherige Ceiling-Pruefung; `ratchet.baseline.json`/`ratchet.config.json`, `scripts/check_file_ceiling.sh` auf Ratchet v0.5.0) und `83ccea1` (CI-Tier-Split headless/GPU) gepusht; beide Laeufe gruen (`40100659173`). Folge fuer diese Liste: neue Module aus Schnitten sind jetzt gegen **1000** Zeilen zu messen, nicht mehr gegen 3000, und `scripts/check_file_ceiling.sh` braucht `ratchet` auf dem PATH (`RATCHET_BIN`) — lokal fehlt das Binary (nur unter `/tmp/opencode/verify032|rel033` vorhanden), der Gate laeuft in der CI. Kein Rot, keine Task angelegt.
  **TAGESSTAND 2026-10-25:** Die Aufgabe `FILE-SIZE-1000-ALL-EXCEPTIONS-100` ist **abgeschlossen und aus der Liste entfernt**: alle 30 Rust-Dateien ueber 1 000 Zeilen sind umgebaut, der Ausnahme-Block in `ratchet.config.json`/`ratchet.baseline.json` ist **leer** (kein `scopes`-Schluessel mehr), und `scripts/check_file_ceiling.sh` meldet mit Ratchet v0.5.0 `COHERENT — 1 of 1` ueber die reine Population; die groesste unausgenommene Datei ist `crates/lumina-mcp/tests/mcp.rs` mit 976 Zeilen (Population 953 Dateien, 109 ueber 500, **0 ueber 1 000**). Die 500er-Liste fuehrt die neuen Module mit exakten Ist-Werten. Reihenfolge und Belege je Datei stehen in der Commit-Historie; jedes Paket wurde unabhaengig verifiziert, mehrere Nachrunden betrafen nur Doku-Zahlen oder zurueckgebliebene Doc-Kommentare. Zwei begleitende Gate-Anpassungen: der Ratchet-`tier.floor` sank von 10 auf 1, weil er je Ausnahme-Datei einen Check zaehlt und die Liste planmaessig schrumpft (Scope-Abdeckung erzwingt weiterhin `check_ratchet_exceptions.py`), und `fast-1` prueft jetzt zusaetzlich `scripts/check_included_rs_fmt.sh`, weil `cargo fmt` der per `include!` eingebundenen Datei `app_struct.rs` nicht folgt. **Unveraendert offen bleiben die Hardware-/Rechte-Gates** (GPU-Pixelparitaet, GUI-Present-E2E, Native-GUI-Goldens, manuelle GUI-Abnahme, RAW-Mehrbildserie, NAFNet-Rechte/Hash-Pin) sowie `--features onnx-rt`/`--all-features` lokal (OpenSSL-Entwicklungspaket fehlt; CI deckt es ab).
 **TAGESSTAND 2026-10-25 (Folge 2):** Zwei Doku-Commits, beide gepusht. `acac61a` entfernt vier laengst erledigte Eintraege (`GUI-LIB-SPLIT-65`, `SPLICE-CLI-MAIN-89`, `SPLICE-GPU-PARITY-92`, `SPLICE-SIDECAR-ZDATA-93`; lib.rs 927, main.rs 905, parity.rs 80, zdata.rs 580 Zeilen, Ratchet-Ausnahmeblock leer) und den damit leeren Slicing-Abschnitt — CI 7/7 gruen (Lauf `40175216255`, alle sieben Job-Records `completed/success`), Matrix (nightly) gruen. `ad3230f` zieht das Sicherheits-Register auf das kanonische Vokabular: Severity- und Ticket-Spalte (zehn Spalten je Zeile), deutsche Statuswoerter mit genau einer Abbildung auf `open`/`in-progress`/`fixed`/`accepted`/`wontfix`, Offen-Zaehlung und Verteilung in §4, Owner-und-Ticket-Pflicht fuer alles ausser `fixed` (SEC-2/SEC-3 -> `SEC-WELLE-1`, SEC-8 -> `SEC-WELLE-2D`), belegte Ausnahme fuer SEC-6/SEC-9 (nur Ergebnisprotokoll, kein Kommando), SEC-8 ohne Zusatztext im Wellenstatus, SEC-9 wieder mit zehn Zellen, veraltete 7/2/1-Prosa in Kopfblock/§3/§8/`feature/README.md` nachgezogen. Fuenf Verifikationsrunden (4x CHANGES REQUIRED, zuletzt APPROVED), danach `check_doc_facts_drift.sh` 12 Bloecke in 7 Dateien ohne Drift, `check_plan_format.sh` 23 offene Aufgaben / 0 abgehakte, `git diff --check` still. **`SKILLS-VOKABULAR-98` ist damit abgeschlossen und aus der Liste entfernt** (Herkunft `SEC-REVIEW-WELLE-1-85`); Teil (d) bleibt als datierte Entscheidung im Register dokumentiert. Kein Rot, keine Task angelegt.
 **TAGESSTAND 2026-10-25 (Folge 3):** Die Wochenrunde von `TEST-AUDIT-36` ist **statisch** gelaufen (kein Testlauf, keine Datei geaendert): 3 262 `#[test]` in 508 Dateien. Zwei hoch bewertete Falsch-gruen-Befunde (`auto_fill_transparent_trigger_via_lens` in `crates/lumina-core/src/render/tests/pipeline.rs` und die GUI-Kopie in `crates/lumina-gui/src/tests/distortion.rs`: Trigger-Precondition auskommentiert, zwei Tautologien, `psnr` ohne Transparenz unendlich) sind als `AUTOFILL-LENS-TRANSPARENCY-101` angelegt; die uebrigen Befunde F3-F9 und die Luecke L3 stehen als Bericht in der Task, ohne Loeschauftrag. `check_plan_format.sh` 23 offene Aufgaben, 0 abgehakte. Kein Rot, keine stille Aenderung.
 **TAGESSTAND 2026-10-25 (Folge 4):** `DOC-ZAHLEN-DRIFT-82` ist **abgeschlossen und aus der Liste entfernt**: `25b5dae` korrigiert die einzige echte Drift („20 Dateien" -> **19** `.rs`-Dateien in `serde_json` 1.0.151 `src/lexical/`, mit Zaehlkommando in der Zelle) und belegt, dass die fuenf uebrigen „Restbefunde" gegen den datierten Planstand `ddebe7f` exakt richtig sind (266/179/115/507/300; `git show ddebe7f:<pfad> | wc -l`). Die undatierte Testzahl „84 Tests gruen" in `feature/platform/capability-matrix.md` ist nach der Task-Regel **gestrichen** statt datiert; der Befund (Mutation blieb gruen) bleibt. Der Scan auf Testlaufzahlen liefert danach genau einen Treffer (Beispielsatz im Task-Text), das CLI-Inventar traegt „Zaehlstand 2026-10-15", der Golden-Split steht in den zwei generierten Bloecken, die Inventartabelle bleibt handgepflegt und wird vom Generator abgeglichen. Zwei Verifikationsrunden (1x CHANGES REQUIRED, zuletzt APPROVED); `check_plan_format.sh` 23 -> **22** offene Aufgaben, 0 abgehakte. Kein Rot, keine neue Task.
 **TAGESSTAND 2026-10-25 (Folge 5):** `AUTOFILL-LENS-TRANSPARENCY-101` ist **abgeschlossen und aus der Liste entfernt** (`e5a27e0`): die zwei Tests erzeugen jetzt echte Lens-Transparenz (k1 = -0.5: **304/1024** im Core, **16/64** in der GUI, vorher 0 mit k1 = +1.0), verlangen null transparente Pixel nach dem Fill, verschiedene Pixel, endliches `psnr` und ein veraendertes Histogramm; Tautologien und die auskommentierte Precondition sind ersetzt. Mutationsbeleg: Fill aus -> beide rot (304/16 statt 0), Produktionsdatei danach byte-identisch. Testlisten unveraendert (core 545, gui 968 = 963 + 5 Ignores). Der Nachbartest `auto_fill_seed_changes_output` ist ebenfalls vakuum-gruen (k1 = +1.0, `out1`/`out2` werden nie verglichen) — als `AUTOFILL-SEED-VACUOUS-102` angelegt, nicht im selben Commit repariert. Eine Verifikationsrunde (APPROVED); `check_plan_format.sh` 22 offene Aufgaben vor dem Tausch, danach 22 (eine geschlossen, eine neu). Kein Rot.
 **TAGESSTAND 2026-10-25 (Folge 6):** `AUTOFILL-SEED-VACUOUS-102` ist **abgeschlossen und aus der Liste entfernt** (`f7d9f6e`): `auto_fill_seed_changes_output` nutzt jetzt k1 = -0.5 mit Vollbild-Crop (**304/1024** transparente Pixel), verlangt null transparente Pixel nach dem Fill, bei gleichem Seed byte-identische und bei verschiedenem Seed verschiedene Pixel (gemessen: **81** abweichende RGBA-Pixel zwischen Seed 1 und 2). Mutationsbeleg: Seed im BFS-Tie-Break entfernt -> Test rot; `generative.rs` danach byte-identisch. Testname und Testzahl unveraendert (545), Datei 972 -> **971** Zeilen. Grenze des Belegs im Commit benannt: der Anker deckt die Seed-Verarbeitung im Core-Fill, nicht die CLI/GUI-Weitergabe. Eine Verifikationsrunde (APPROVED). `check_plan_format.sh` 22 -> **21** offene Aufgaben. Kein Rot.
 **TAGESSTAND 2026-10-25 (Folge 7):** `BUILD-VERIFY-ABWEICHUNG-96` ist **entschieden und aus der Liste entfernt** (Eigentuemerentscheid: an Upstream angleichen, Commit `76d1b0a`). `Agents.md` folgt jetzt `build-verify` in beiden Punkten: der Verifizierer prueft die **Substanz** und uebernimmt gemeldete Gate-Ergebnisse mit zitierter Ausgabe (vier Ausnahmen nach `references/verifier-prompt.md`, nicht die bisher im Task genannte Liste); jeder Dispatch ist eine **frische Session** fuer Implementierer und Verifizierer, einzige Ausnahme ist eine Fix-Runde derselben Aufgabe, deren Runde die Session selbst implementiert hat. Die alten `continue`-Formulierungen (auch im Abschnitt „Verifikations-Niederlagen") sind ersetzt. Zwei Verifikationsrunden (beide APPROVED; drei Wording-Klarstellungen eingearbeitet). `check_plan_format.sh` 21 -> **20** offene Aufgaben, 0 abgehakte. Kein Rot.
 **TAGESSTAND 2026-10-25 (Folge 8):** Eine Welle aus `PROSE-ANCHOR-POLICY-88` ist umgesetzt (`7cca739`): `feature/architecture/pipeline.md` (3), `feature/product/ai-masks.md` (12) und `feature/platform/cli-gui-wasm.md` (5) tragen **keine** `pfad:zeile`-Anker mehr — 20 Vorkommen auf vollstaendigen Pfad + Symbol/Testname umgestellt, alle 20 Ziele mit `test -f` und `rg 'fn …'` belegt. Drei alte Zeilen zeigten ins Leere (`render_schedule.rs`, `library_grid.rs`, `preview_draws.rs`) und nennen jetzt das heute tragende Symbol; `clamp_preview_pan` traegt zusaetzlich den Definitionspfad. Eine Verifikationsrunde (APPROVED; 1 mittlerer vorbestehender Befund, 2 niedrige). Der mittlere Befund ist eine **falsche Prosa-Aussage** in `ai-masks.md` (`slider.rs` -> `draw_track` bewaffne `pending_full_render` neu; in `slider.rs` gibt es **0** Schreibzugriffe darauf) und ist als `PROSE-CLAIM-SLIDER-REARM-103` angelegt. Offen bleibt in `PROSE-ANCHOR-POLICY-88` nur die Welle `feature/quality/mcp-parity-c-soll.md` (**85** Anker) und der gesperrte Rest; `check_plan_format.sh` 20 -> **21** offene Aufgaben.
 **TAGESSTAND 2026-10-25 (Folge 9):** `PROSE-CLAIM-SLIDER-REARM-103` ist **abgeschlossen und aus der Liste entfernt** (`5e84d53`). Die falsche Aussage („`slider.rs` -> `draw_track` bewaffnet `pending_full_render` bei jedem `dragged()`-Frame") ist ersetzt: `draw_track` setzt nur `TrackOutcome { changed }`, `lr_slider` -> `SliderAction::Changed` -> `adjustment_slider` -> `set_adjustment` setzt das Flag, mask-local ueber `mark_recipe_dirty` -> `mark_dirty`; `schedule_render` (in `app_impl.rs` -> `ui`, vor den Panels) bewaffnet nach einem nicht committenden Vollrender neu, solange kein Zeiger gedrueckt ist, die Drag-Uhr laeuft und ein Commit-Token armiert ist. Die Bindung an `R2-GUIMOD-04b`/`-04c` steht ausdruecklich im Text. Zwei Verifikationsrunden (CHANGES REQUIRED: `update` existiert nicht, Feld statt Associated Item, Re-Arm-Bedingung unvollstaendig; danach APPROVED). Keine Verhaltensaenderung, 0 Zeilenanker in der Datei. `check_plan_format.sh` 21 -> **20** offene Aufgaben.
 **TAGESSTAND 2026-10-25 (Folge 10):** Die letzten beiden Anker-Wellen aus `PROSE-ANCHOR-POLICY-88` sind abgeschlossen. `feature/quality/mcp-parity-c-soll.md` (**85** Inline-Anker, alle als nackte Dateinamen) traegt **0** Zeilenanker mehr (`aad4a48`): jede Stelle nennt vollstaendigen Pfad + Symbol/Testname, dazu sind **140** weitere Ortsangaben (nackte `:NNN`, `,365-370`, `:9-29`) entfallen, die neben dem schon genannten Symbol keine Aussage trugen; der datierte Belegblock (`9bacea8`) ist unveraendert, der Kopf sagt jetzt, dass die historischen Ausschnitte denselben Stand beschreiben. Drei echte Umzuege wurden aufgedeckt (`check_existing_bundle` -> `bundle/security.rs`, `validate_merge_alignment` -> `merge_recipe_alignment.rs`, ein Test -> `merge_envelope_e2e.rs`); 96 Pfad->Symbol-Paare belegt, keines zeigt ins Leere. Zwei Runden (1x APPROVED mit 2 niedrigen Befunden, einer als Kopf-Klarstellung eingearbeitet; danach APPROVED). Die kleine Restwelle (`f830981`) hat `feature/quality/golden-references.md` (`lib.rs:6106` existierte nicht, `dirty.rs:47` zeigte auf `tone_analysis`, `render_pipeline.rs:484` lag eine Zeile daneben — jetzt `push_mask_definition` -> `transact_mask_mutation` -> `mark_dirty` bzw. `render_from`) und `feature/platform/capability-matrix.md` (`app_frame.rs:148-156` -> Pfad + `draw_routing_fallback_badge`) bereinigt; eine Runde APPROVED. **Stand:** kein undatierter lebender Doku-Ort traegt mehr einen Zeilenanker ausser `docs/logging-plan.md` (**13**, alter SOLL-Plan mit Stand-Notiz `87929c7`) und `docs/ci-parallelization-ideas.md` (**1**, datierte Ideensammlung) — beide bleiben bewusst offen (Stand-Notiz vs. Resymbolisierung ist eine Eigentuemerfrage). Ein neuer Prose-Befund aus der Welle ist als `PROSE-CLAIM-MERGE-ORIENTATION-104` angelegt. `check_plan_format.sh` 20 -> **21** offene Aufgaben.
 **TAGESSTAND 2026-10-25 (Folge 11):** `PROSE-CLAIM-MERGE-ORIENTATION-104` ist **abgeschlossen und aus der Liste entfernt** (`9d11d77`): §4.3 und die Invariante 2 in §6 schrieben `orientation` dem `merge_exif_from_raw` und `decode_version` `CARGO_PKG_VERSION` zu. Gemessen baut `decode_source` den `MergeDecodeContext` (`orientation` = `raw.orientation`, Default 1; `decode_version` = `libraw_decode_version()` bzw. `MERGE_IMAGE_DECODER_VERSION`); `MergeExif` hat kein `orientation`-Feld. Beide Stellen sind korrigiert, die Aussage „einzige Stelle" ist auf die Decode-Metadaten eingegrenzt (Pixel driften ueber `alignment.transforms`/`residual_px`). Zwei Verifikationsrunden (CHANGES REQUIRED: §6 widersprach §4.3; danach APPROVED). Ausserdem tragen `docs/logging-plan.md` und `docs/ci-parallelization-ideas.md` auf Eigentuemerentscheid jetzt eine historische Stand-Notiz; `check_plan_format.sh` 21 -> **20** offene Aufgaben.

## Folge aus `LOCAL-GATE-TIERS-81` (Hardware-Gate, 2026-10-20)

## Befunde aus der Zahlen-Inventur zu `DOC-ZAHLEN-DRIFT-82` (2026-10-20)

- [ ] **[PRIO: niedrig] PROSE-ANCHOR-POLICY-88 (Marker-Run 2026-10-20; Befund aus der Zahlen-Inventur zu `DOC-ZAHLEN-DRIFT-82`; **Pilot umgesetzt, Altbestand offen**)** **Pilot umgesetzt (2026-10-20):** die UXG/UXD-Gap-Tabelle in `feature/platform/lightroom-ux-parity.md` (UXG-01..18, UXD-01..08) ist auf **Pfad + Symbol/Testname** umgestellt — **0** Zeilenanker in der Datei; die vier beauftragten haengenden Einzelanker sind aufgeloest (`crates/lumina-mcp/src/main.rs` -> `crates/lumina-mcp/src/lib.rs` -> `Server::cleanup_previews`; `crates/lumina-cli/src/matrix.rs` -> `crates/lumina-stages/src/matrix_report.rs` -> `report_json`; zwei Sidecar-Testdateien -> konsolidiert in `crates/lumina-sidecar/src/tests/local_adjustments_color_digest.rs` -> `disabled_local_presence_detail_denoise_and_optics_stay_rejected`); R2-MCP-05 in `docs/reviews/2026-08-26-full-review.md` traegt wieder den Originalstatus „GAP“ plus einen Beleg-Nachtrag (kein Statuswechsel). Die **normative Regel** steht jetzt **an genau einer Stelle**: `Agents.md` §„Dokumentations- und Todo-Regeln“ (Anker = vollstaendiger repo-relativer Pfad + Symbol-/Testname; Zeilennummern nur in **datierten** Audit-Belegen). **Anker-Rest abgeschlossen (2026-10-20):** die **nicht aufloesbaren Teilpfade** (Formen wie `src/lib.rs`, `benches/preview_cache.rs`) sind auf den **vollstaendigen repo-relativen Pfad** umgestellt — **116** entfernte Pfadanker-Vorkommen (87 distinkt) gegen **121** hinzugefuegte (82 distinkt) ✓; die +5 sind die Aufspaltung der kollabierten Paar-Notation (`preview-cache-baseline/budgets.json` -> zwei Dateien ✓). **90** neue Zielpfade geprueft: **88** existieren ✓, **1** ist der ehrlich als **geplanter Pfad** markierte `crates/lumina-gui/tests/preview_lru.rs` ✓ (im Text so gekennzeichnet ✓, **nicht** erfunden ✓). **Satzbezug** je Stelle geprueft ✓ — der Dateiname entschied, nicht die Vermutung ✓ (Konkurrenzfaelle: `tests/kittest_snapshots.rs` (nur **ein** Treffer im Repo ✓), `benches/preview_cache.rs` gegen `crates/lumina-core/src/preview_cache.rs` ✓, `src/gpu_routing.rs` (Produktionsfunktion ✓) gegen `src/tests/gpu_routing.rs` (Test ✓), `tests/stages.rs` gegen `crates/lumina-cli/src/stages.rs` **und** `crates/lumina-gpu/src/stages.rs` ✓). **Zaehlkonvention (neu, nachrechenbar ✓):** Korpus 66 Dateien ✓, Muster `[A-Za-z0-9_.\/-]*[A-Za-z0-9_-]+\.rs(:[0-9]+(-[0-9]+)?)?` ✓, **Vorkommen = Zeile × Spalte** ✓; „nicht aufloesbar“ = `test -f` schlaegt fehl **und** Suffix-Aufloesung nicht eindeutig ✓. **Bestand:** `.rs`-Vorkommen **2118 -> 2118** (Δ 0 ✓); nicht aufloesbare Pfadverweise (alle Endungen) **279 -> 274** ✓; nur `.rs` **234 -> 146** ✓; **0** neu eingefuehrte nicht aufloesbare Pfade ✓. **Restbestand, nur gemeldet ✓:** **24** gesperrte Vorkommen in vier Dateien (9 + 1 + 3 + 11 ✓) — darunter drei Pfade, die **suffix-eindeutig aufloesbar** sind ✓ (`tests/kittest_mask_local.rs` ✓, `bench/gpu.rs` ✓, `bench/cull.rs` ✓); sie sind gesperrt, nicht unauflösbar ✓. Und **55** Verweise **ausserhalb** des versionierten Baums (`.goal/Goal.md` 24 ✓, `.lumina/…` 7 ✓, die `src/…`-Pfade im **externen** Crate `serde_json` 22 ✓, `identify.cpp`/`auxfun.cpp`/`gutils.c`/`LUMINA_MATRIX_DIR/…` je 1 ✓) — sie bleiben **unveraendert** ✓, weil sie nicht Teil des Repos sind ✓ (eine Umformulierung „nicht im Repo“ waere bei `.goal/Goal.md` **falsch** ✓ — es liegt im Arbeitsbaum, nur nicht im Index ✓). **Hinweis:** die Welle hat auch bereits **auflösbare** Teilpfade mitgezogen (95 von 97 `.rs`-Ankern ✓) — das ist konsistent mit der Policy „vollstaendiger repo-relativer Pfad“ ✓. **Tabellen intakt** ✓ (0 Pipe-Zahl-Aenderungen ueber 867 Tabellenzeilen ✓), **keine** Befund-/Messungs-/Datumsangabe entfernt ✓ (entfernt wurden ausschliesslich drei undatierte Zeilennummern von `Cargo.toml`-Ankern ✓, ersetzt durch den Abschnitt `[dependencies]` ✓). **Welle A abgeschlossen (2026-10-20):** die **datierten Audit-Dokumente** unter `docs/` (10 Dateien) sind auf nachpruefbare Bezuege gebracht. **Bestand:** 142 Zeilen-/Z.-Anker + 41 nackte Dateiverweise → **(a) 124 rekonstruierbar** (der damalige Stand ist benannt und per `git show <stand>:<pfad> | sed -n '<zeile>p'` belegt ✓), **(b) 7 umgestellt** (der Zeiger zeigte auf etwas anderes → Pfad + Symbol ✓, die zitierte Zeile bleibt erhalten ✓), **(c) 11 nicht rekonstruierbar** — alle in `docs/release-migration-strategy.md`: der auditierte Arbeitsbaum vom 2026-08-19 **existiert in keinem Commit** ✓ (in `b56c9f1` liegen die Symbole **61 Zeilen frueher**, in `3b07ed7` drei bzw. sieben Zeilen tiefer als zitiert ✓ — das steht jetzt so im Dokument ✓, statt eine Ziffer zu behaupten, die in keinem Commit stimmt ✓). **Form-Fixes:** nackte/Teilpfade auf den vollstaendigen repo-relativen Pfad umgestellt ✓ (u. a. `vendor/libraw-sys/build.rs` — belegt: nur dieses `build.rs` nutzt `pkg_config`/`libraw_r` ✓; `crates/lumina-raw/build.rs` existiert **nicht** ✓), alle umgestellten Pfade existieren ✓, **0** Anker jenseits EOF ✓. **Stand-Notizen** in 8 der 10 Dateien (die zwei ohne Zeilenanker brauchen keine ✓) — jede nennt Stand **und** Rekonstruktionsbefehl ✓. **Keine Beweisvernichtung ✓:** die ID-Bilanz je Datei ist unveraendert (review 88 → 88, lightroom 155 → 155, implementation-gap 139 → 139, gui-tests 121 → 121, release-migration 9 → 9, generative 38 → 38, fixtures 10 → 10, histogram-zoom 3 → 3 ✓), **keine Zahl** verloren (Multiset-Vergleich aller Ziffernlaeufe ✓ — die Gewinne sind ausschliesslich die Notiztexte ✓), keine Tabelle gesprengt ✓, kein bestehender Satz ersetzt ✓. **Offen:** **18** nicht aufloesbare Teilpfade in den 10 Dateien (u. a. `src/lib.rs`, `src/preview_ctrl.rs`, `benches/preview_cache.rs` in `docs/plans/gui-tests-2026-09-02.md` ✓ — Welle A war ausdruecklich „nur die Form vereinheitlichen, **kein** Zwang“ ✓, sie sind also **kein** Defekt dieser Welle ✓) und die **Zaehlkonventions-Erweiterung** (die nicht-backtickierten Fortsetzungen und die Belegbasis-Formen bleiben vom Plan-Muster blind ✓). **Welle F abgeschlossen (2026-10-20):** die **pfadgebundenen** Zeilenanker der 11 bearbeiteten Dateien sind auf **Pfad + Symbol-/Testname** umgestellt — **70** neue Pfad→Symbol-Paare (63 bei HEAD nicht vorhanden; 60 fn / 5 const / 2 struct / 2 enum + 1 × `pub mod`; **alle** Pfade existieren, **alle** Symbole definiert ✓; Stichprobe 36/36 mit belegtem **Satzbezug** ✓). **Bestand** (Korpus 66 Dateien, Muster `[A-Za-z0-9_.\/-]*[A-Za-z0-9_-]+\.rs(:[0-9]+(-[0-9]+)?)?`, Konvention **Vorkommen** = Zeile × Spalte ✓): pfadgebunden + Zeile **159 -> 88** (in den 11 bearbeiteten Dateien **80 -> 9**; in den gesperrten Dateien unveraendert **79**: full-review 49, `Agents.todo.md` 11, logging-plan 6, gap-generative 4, mcp-server 3, local-gates 2, ci-parallel 1, cli-gui-parity 1, gui-preview-histogram 1, performance-benchmarks 1) · Zeilenanker gesamt **409 -> 325** (je Datei alle Deltas <= 0 ✓) · Fortsetzungsformen (Backtick + `:` + Ziffern) **33 -> 9** (alle 24 Konvertierungen in den bearbeiteten Dateien ✓) · `.rs`-Vorkommen **+14**, ausschliesslich Pfad-Vervollstaendigungen (99 neue Pfad-Erwaehnungen ohne Zeilennummer gegen 85 entfallene Zeilen-/Teilpfad-Anker ✓) · **EOF-Pruefung ueber alle Formen** (pfadgebunden, Teilpfad, nackt, Komma-Form `:337,447`, Slash-Form `:133/138`, Fortsetzungsformen): **326** Zeilenangaben, **1** jenseits EOF, **0** unaufgeloeste Pfade ✓ — die eine EOF-Verletzung lag im **Plan** selbst und ist mit der Umstellung auf `crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` -> `fit_preview_histogram_matches_thumbnail` **behoben** ✓ (der Plan traegt jetzt **null** Zeilenanker ✓). **Offen (nicht Welle F):** (a) die **nackten** Zeilenanker der 11 bearbeiteten Dateien (**172**, davon **27** mehrdeutig) — Welle-N-Domaene ✓; (b) die **79** undatierten pfadgebundenen Anker in den gesperrten Dateien -> **Welle A** (nur die Form vereinheitlichen, kein Zwang) bzw. Orchestrator ✓; (c) die **vom Plan-Muster nicht gesehenen** Formen — nicht-backtickierte Fortsetzungen `(:NNN)` **53** (downscale 20, mcp-parity-c 32, logging-plan 1) und Belegbasis-Formen „Pfad + Symbol + Zeile“ **18** in den beiden Belegbloecken — sind **datiert ueberdeckt** (Stand-Hinweise `cb2da59` / `9bacea8` / `87929c7` ✓) und damit **nach Eigentuemerauslegung regelkonform** ✓ (die Spannung zum Regeltext steht im Absatz „Offen (bewusst, datiert)" am Ende dieses Eintrags); sie brauchen **keine eigene Welle**, wohl aber eine **Erweiterung der Zaehlkonvention** in der Zahlen-Inventur zu `DOC-ZAHLEN-DRIFT-82` ✓ (das Plan-Muster blendet sie aus ✓). (d) Zahlen-Drift **innerhalb** des datierten SOLL-Standes, nicht Welle F: `downscale-ein-pfad-soll.md` § 5.1 `< 0.35` (der Test nutzt heute `< 0.05` ✓) und dieselbe Zeile „**Nearest**, `downscale_rgba`“ (die Nearest-Strecke existiert heute nicht mehr ✓) — **nicht** angefasst ✓ (Zahlen-Drift ✓, Prosa-Aufgabe ✓). **Welle N abgeschlossen (2026-10-20):** die **mehrdeutigen nackten** Anker sind aufgeloest — mehrdeutig **338 -> 198** (140 umgestellt), nackt gesamt **726 -> 582** (Basis `70c2649`), eindeutig 379 -> 377, ohne Datei 9 -> 7. **Neue Pfad→Symbol-Paare: 114** (68 distinct) — **nicht** 98 (die 98 waren zeilenweise gezaehlt ✓); **114/114** belegt (Pfad existiert **und** Symbol/Kennung im File ✓), 0 Probleme ✓. **Fortsetzungsformen 25 -> 24** (17 Dateien, Form Backtick+Doppelpunkt+Ziffern ✓, reproduzierbar ✓). **Konvention (neu, damit die Zahlen nachrechenbar sind):** Korpus 66 Dateien, Muster `[A-Za-z0-9_.\/-]*[A-Za-z0-9_-]+\.rs(:[0-9]+(-[0-9]+)?)?`, gezaehlt werden **Vorkommen** (Zeile × Spalte) ✓; die Zahl `pfad:zeile 331 -> 250` war **ohne** Konvention nicht reproduzierbar — die nachpruefbare Form ist „nackte Zeilenanker im Subkorpus `feature/` + `docs/`: **334 -> 250**“ ✓. **Restbestand 198 = 29 + 169:** 29 in `feature/quality/mcp-parity-c-soll.md` (28 in §6-§13, 1 in §5 war auflösbar und ist behoben ✓) und **169** in 16 gesperrten/nicht bearbeiteten Dateien (nur gemeldet ✓; der groesste Block: `Agents.todo.md` 53 ✓ — der Plan gehoert dem Orchestrator ✓, die Plan-Anker sind ein eigener Auftrag ✓). **Wichtig — zwei Staende:** die Zahlen 582/198 gelten fuer die Basis `70c2649`; durch `070cc75` (dieser Orchestrator) enthaelt der Korpus heute **615 nackt / 223 mehrdeutig** ✓ — beide Staende nennen, sonst ist die Zahl nicht reproduzierbar ✓. **Welle F offen:** `feature/quality/downscale-ein-pfad-soll.md:141` — `crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` -> `fit_preview_histogram_matches_thumbnail` zeigt **jenseits EOF** (Datei 348 Zeilen) und ist veraltet (der Test liegt in `crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` ✓); `:139` — die Abschnittsueberschrift zeigt auf das **Elternmodul** statt auf den Traeger ✓; `:130` — eine datierte f32-Aussage (Prosa-Aufgabe ✓). **Welle H abgeschlossen (2026-10-20):** die **4 haengenden** nackten Anker + **1** Anker ohne Datei sind auf Pfad + Symbol/Testname umgestellt; die Rekonstruktion erfolgte ueber die Historie (`c2c2ce1`, `2aec896`, `59cc0b5`, `78c21c9` bzw. den Registry-Cache fuer das externe Crate) und ist **datiert** belegt. **Umfangsausweitung (offengelegt):** Welle H hat zusaetzlich **alle** `.rs`-Zeilenanker der vier Dateien mitgezogen (vorher 30/71/43/10 `.rs`-Anker + 7/81/4 Backtick-Fortsetzungsformen -> **0/0/0/0**; 99 eindeutige Pfad→Symbol-Paare einzeln auf Existenz **und Relevanz** geprueft) — das ist der **vorzeitige Anteil von Welle F fuer genau diese vier Dateien**; Welle F muss die Bestandszahlen um diese vier Dateien bereinigen. **Restbestand in diesen vier Dateien:** noch **11** undatierte Zeilenanker auf **Nicht-Code-Dateien** (11 in `feature/quality/mcp-matrix-parity-soll-2026-10-14.md`, **0** in `feature/quality/json-float-roundtrip-plan.md` — der `Cargo.lock`-Anker ist mit B6 weg) — die gehoeren in **Welle P** (Teilpfade) bzw. **Welle F** (pfadgebundene Nicht-Code-Dateien). **Welle P abgeschlossen (2026-10-20):** die **11** Nicht-Code-Anker (Konfiguration, Workflow, JSON, Doku) sind auf **Pfad + Abschnittsüberschrift bzw. Pfad + Schlüssel/Wert** umgestellt (z. B. `Agents.todo.md` -> Eintrag `MCP-PARITY-C`, `recipe-set.v1.json` -> `recipes[]`-Eintrag `g06-lens-perspective`, `conflicts-and-acceptance.md` -> § „Rezept-Matrix (LRPAR-MATRIX-RECIPE)“); die **nicht auflösbaren Teilpfade** sind aufgelöst: Teilpfade **228 -> 199**, davon suffix-eindeutig auflösbar **177 -> 171**, **nicht** auflösbar **51 -> 28**, und **nicht** auflösbar **mit** Zeilennummer **6 -> 0**. Der Restbestand der 28 ist **markiert**: **18 extern** (Pfade im Crate `serde_json` 1.0.151 — mit Crate-Präfix, **kein** `crates/…/ser.rs` ✓), **5 geplant** (`crates/lumina-mcp/tests/merge_parity.rs` — „geplanter Pfad, nicht im Repo“ ✓) und **5 gesperrt, nur gemeldet** (`Agents.todo.md` -> `src/local_mask_gpu.rs`; `docs/ci-parallelization-ideas.md`; `docs/plans/gap-generative-fill-transparent-2026-09-02.md`; `docs/plans/gui-tests-2026-09-02.md` zweimal) ✓. **Ausdruecklich offen:** das Kriterium „0 Zeilenanker je bearbeiteter Datei“ ist **nicht** erreicht (Rest **A+B**: pipeline 28, mcp-parity-c-soll 202, test-audit-2026-10-12 31, cli-gui-wasm 16, ai-masks 31) — das sind **andere** Bestandteile (pfadgebundene Zeilenanker -> **Welle F**; mehrdeutige nackte -> **Welle N**), und der Plan mahnt „keine Massenumstellung in einer Welle“ ✓; **kein neuer Anker** wurde eingefuehrt (alle Deltas <= 0 ✓), **0** Anker zeigt jenseits EOF, **0** genannter Pfad existiert nicht ✓. **Bekannte Restbefunde fuer `DOC-ZAHLEN-DRIFT-82` (Welle 2/3):** (erledigt 2026-10-25: die fünf `->`-Pfeile vergleichen den datierten Planstand `ddebe7f` mit einem spaeteren Baum und sind **kein** Drift; nur `20`→`19` war ein echter Fehler und ist belegt korrigiert — siehe `DOC-ZAHLEN-DRIFT-82`, STAND 2026-10-25) — die Zahlen „20 Dateien“ -> **19** (`src/lexical/`), „266 Z.“ -> **307** (`crates/lumina-mcp/src/error.rs`), „179 Z.“ -> **186** (`tools/mod.rs`), „115 Z.“ -> **119** (`tool_names/mod.rs`), „507 Z.“ -> **510** (`crates/lumina-gui/src/matrix.rs`), „300 Z.“ -> **361** (`crates/lumina-mcp/src/tools/merge_common.rs`) ✓. **Offen ist der Altbestand** (gemessen 2026-10-20; **Korpus**: `git ls-files '*.md' | grep -E '^(feature|docs)/|^Agents[^/]*\.md$|^DoD\.md$|^README\.md$'` — 66 Dateien, ohne `review.notes.md` und ohne `agent-harness/README.md`; **Muster**: `[A-Za-z0-9_.\/-]*[A-Za-z0-9_-]+\.rs(:[0-9]+(-[0-9]+)?)?`; **Fortsetzungsform**: Backtick + Doppelpunkt + >=1 Ziffer + optionaler `-`-Bereich + Backtick): **1999** `.rs`-Muster-**Vorkommen** im Korpus (Muster `[A-Za-z0-9_.\/-]*[A-Za-z0-9_-]+\.rs(:[0-9]+(-[0-9]+)?)?`; davon **509** mit Zeilennummer, **1075** voller repo-relativer Pfad (alle gueltig, **0** haengend), **199** Teilpfad (davon **171** suffix-eindeutig aufloesbar, **28** nicht aufloesbar — **0** davon mit Zeilennummer), **725** nackt (**237** eindeutig, **230** mehrdeutig, **1** ohne Datei) — plus **36** Backtick-Fortsetzungsformen. **Die Konvention ist „Vorkommen“, nicht „distinct Pfade“** (distinct: 279 / 119 / 165) — ohne diese Angabe war die Zahl nicht reproduzierbar ✓. **Die Zahlen sind eine datierte Messung (2026-10-20), und der Plan selbst gehoert zum Korpus** — jede hier notierte Zahl veraendert den Bestand. **Massgeblich ist die Definition**, nicht die Zahl: mit ihr ist der Bestand jederzeit nachrechenbar. Die **haengenden** nackten Anker (ohne Zeilennummer zitiert, damit diese Zeile den Bestand nicht selbst veraendert): `feature/quality/test-audit-2026-10-19.md`, `feature/quality/merge-gain-deghost-crop-soll-2026-10-15.md`, `feature/quality/mcp-matrix-parity-soll-2026-10-14.md` — dazu **1** Anker ohne Datei in `feature/quality/json-float-roundtrip-plan.md` (externes Crate). **Wellenweg (je Welle mit eigener Verifikation, keine Massenumstellung in einer Welle):** **H** zuerst = die 4 haengenden + 1 Anker ohne Datei (den gemeinten Ort ueber die Historie rekonstruieren, auf Pfad+Symbol umstellen); **P** = die 23 nicht aufloesbaren Teilpfade (crate-Praefix vervollstaendigen, externe Crates als solche markieren, fuehrenden Slash normalisieren); **N** = die 230 mehrdeutigen nackten Anker (je Crate geschnitten — `lumina-cli`, `lumina-mcp`, `lumina-stages`, `lumina-gui`, `lumina-core`; dort kollidieren die generischen **Dateinamen** `lib.rs`/`main.rs`/`mod.rs` systematisch); **F** = die drift-anfaelligen, aber pfadgebundenen (118 volle Pfade + 63 aufloesbare Teilpfade + 237 eindeutige nackte); **A** = die datierten Audit-Dokumente (`docs/reviews/`, `docs/plans/`) — dort nur die Form vereinheitlichen, **kein** Zwang. **Abnahme je Welle:** 0 Zeilenanker in der bearbeiteten Datei, jeder neue Anker mit `test -f` + `grep` belegt und gegen den Datei-Index geprueft, dass kein Anker jenseits EOF zeigt. **Abgrenzung:** keine Code-Aenderung; die Regel steht an **einer** Stelle, nicht zusaetzlich in `DoD.md` oder `feature/quality/security-review-register.md`. **WELLEN 2026-10-25 (abgeschlossen):** `feature/architecture/pipeline.md` (3), `feature/product/ai-masks.md` (12), `feature/platform/cli-gui-wasm.md` (5) in `7cca739`; `feature/quality/golden-references.md` (3) und `feature/platform/capability-matrix.md` (1) in `f830981`; `feature/quality/mcp-parity-c-soll.md` (85) in `aad4a48` — je Datei **0** Zeilenanker, alle Ziele mit Pfad-/Symbol-Beleg. **Offen (bewusst, datiert):** `docs/logging-plan.md` (**13** pfadgebundene Anker, dazu **59** nackte Fortsetzungen) und `docs/ci-parallelization-ideas.md` (**2** Zeilenangaben: eine `.rs`, ein `ci.yml`-Verweis) tragen auf Eigentuemerentscheid vom 2026-10-25 eine ausdrueckliche Stand-Notiz (Logging-Plan: Commit `87929c7`, Niederschrift 2026-08-23; CI-Papier: `c4abebf`, 2026-08-23) und sind als **historisch** markiert — die Zeilenangaben sind ueber den genannten Stand rekonstruierbar; eine Resymbolisierung auf HEAD wuerde einen ueberholten Plan als aktuellen Stand ausgeben. **Spannung zum Regeltext, offen ausgewiesen:** `Agents.md` erlaubt Zeilennummern nur in datierten Audit-Belegen, ein SOLL-/Ideenplan ist keiner; die Stand-Notiz ist die bewusste Auslegung des Eigentuemers, keine stille Regeländerung. Datierte Audit-/Plan-Belege (`docs/reviews/`, `docs/plans/`, `*-2026-*.md`, `9bacea8`/`cb2da59`-Blaecke) bleiben regelkonform. **Herkunft:** Zahlen-Inventur zu `DOC-ZAHLEN-DRIFT-82`, 2026-10-20.


- [ ] **[PRIO: niedrig] PERF-DECODE-BASELINE-NEUERFASSEN-97 (Befund aus der Verifikation zu `PERF-BASELINE-TABELLE-87`, 2026-10-20; **Hardware-Gate**)** Die `decode/raw__*`-Werte im Store sind **keine** Messung mit dem Projekt-Toolchain-Stand: `perf/baseline.json` fuehrt sie (Median **und** P95) unveraendert aus dem Stand `3b07ed7` (2026-08-19, rustc **1.97.1**) — belegt durch den bit-identischen Vergleich der beiden Objekt-Versionen (`git show 3b07ed7:perf/baseline.json` vs. `git show 9f8c632:perf/baseline.json`); §F-074-N7 nennt die Decode-IDs ausdruecklich als „unangetastet“, weil die Re-Baseline vom 2026-09-17 nur die messbaren Core-/Batch-/GPU-IDs neu erfasste (der `environment`-Block der Datei nennt fuer den 2026-09-17 rustc **1.98.0**). **Konsequenz:** das Verhaeltnis Decode/Render in der F-074-Tabelle (4,9x / 5,5x) mischt **zwei** Staende — der Decode-Wert ist alt, der Render-Wert neu. Das ist in `feature/quality/performance-benchmarks.md` als Limitation benannt, die **Zahl selbst** ist aber ein Mischstand. **Aufgabe:** die beiden `decode/raw__*`-IDs auf der Referenzmaschine mit dem Projekt-Toolchain neu erfassen (gleiche Fixtures, gleiche IDs, gleiche Felder) und in `perf/baseline.json` **ersetzen** (nicht danebenstellen); danach die F-074-Tabelle und die Anteile neu rechnen. **Abnahme:** die Decode-Werte stammen aus demselben Lauf wie die uebrigen Core-/Batch-/GPU-IDs (gleiches `recorded_at`, gleiche `environment`-Werte), das Verhaeltnis Decode/Render ist aus **einem** Stand gerechnet, und die Limitation in der Doku entsprechend aktualisiert. **Abgrenzung:** **kein** Lauf auf diesem Host (Hardware-Gate, zusaetzlich LibRaw + `LUMINA_RAW_FIXTURE`); keine Aenderung an den Benchmark-Definitionen oder Fixture-Groessen; die Erfassung selbst folgt der bestehenden Methodik (F-074-N3, §F-074-N7). **Herkunft:** Verifikationsbefund (Teil D, offene Gates) zu `PERF-BASELINE-TABELLE-87`, 2026-10-20.

**Korrektur 2026-10-23 — Cargo all-target Gate:** Der volle `cargo test -p lumina-gpu` scheiterte nach den zuvor erfolgreichen gezielten `--test parity`-Pruefungen: Cargo auto-entdeckte `tests/parity_source_action.rs` als eigenstaendiges Integrationstest-Ziel, in dem `use super::*` ungueltig ist und Parent-Imports/Helper fehlen. Der gezielte Test hatte dieses separate Ziel nicht gebaut. Beide extrahierten Helper-Dateien liegen deshalb nun in Modulverzeichnissen als `crates/lumina-gpu/tests/parity_source_action/mod.rs` und `crates/lumina-gpu/tests/parity_depth/mod.rs`. Die unveraenderten `mod parity_source_action;`- und `mod parity_depth;`-Deklarationen in `parity.rs` loesen sie als Module im einzelnen `parity`-Test-Crate auf; der verschobene Code bleibt unveraendert. Vorherige gezielte Tests sind kein Beleg fuer den All-Targets-Gate. Verifikation nach Fix: ausstehend.

**Nach-Pfadfix Verifikation 2026-10-23:** `source /projects/LuminaRust/target/headless_env.sh && cargo fmt --all -- --check` bestanden. `cargo test -p lumina-gpu` bestanden: unit tests 62/62; Integrationstests: golden 5 passed/2 ignored, init_warning 1/1, lensfun_map 2 passed/1 ignored, local_mask_parity 2 passed/4 ignored, parity 5 passed/22 ignored, stages 1 passed/3 ignored; Doc-tests 0. Der zuvor auto-entdeckte separate Zielname `parity_source_action.rs` ist nicht mehr vorhanden; beide umbenannten Modultests laufen im einzigen `parity`-Ziel mit Prefix. `cargo test -p lumina-gpu --test parity --features gpu`: 5 passed, 0 failed, 22 ignored. Sidecar: `cargo test -p lumina-sidecar --features zdata` 345 passed; `cargo test -p lumina-sidecar --no-default-features` 288 passed; jeweils 0 failed und Doc-tests 0. All-target Clippy mit `-D warnings` und Check bestanden fuer `lumina-gpu --features gpu` und `lumina-sidecar --features zdata`. `cargo fmt`, `check_plan_format.sh`, `check_file_sizes.sh`, `check_file_ceiling.sh`, `git diff --check` und `check_doc_facts_drift.sh` bestanden. Adapter-erforderliche Pixel-Paritaetstests bleiben ignoriert; keine Behauptung einer GPU-Adapter-Paritaet.
