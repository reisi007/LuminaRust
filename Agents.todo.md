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
| `GPU-E2E` | GPU-Themen gelten ohne lokalen Lauf der **relevanten E2E-Tests** als **nicht getestet**; Pflichtkommando `cargo test -p lumina-gpu --features gpu-adapter-tests` plus die betroffenen GUI-GPU-Tests. `--features gpu` allein genügt **nicht**. | User-Weisung 2026-10-02; gemessen: die 28 adapterabhängigen Integrationstests (`crates/lumina-gpu/tests/`, u. a. `parity.rs`) sind ohne `gpu-adapter-tests` `#[ignore]`t, ein `cargo test -p lumina-gpu --features gpu` überspringt sie und liefert einen grünen Lauf ohne Aussage. CI hat keinen GPU-Runner und kompiliert nur | gilt |
| `AUTO-DOMAIN` | Auto-Tone rechnet auf dem Frame **nach** SourceActions **und nach** Crop; der Zuschnitt beeinflusst die Auto-Werte | Eigentümerentscheid 2026-10-02 (wörtliche Anforderung „nach wegretuschieren UND nach Zuschnitt"); die Domäne war ausdrücklich als Eigentümer-Entscheidung markiert, nicht als Codefrage | entschieden 2026-10-02; Umsetzung in `AUTO-TONE-ANALYSIS-INPUT-8` |
| `JSON-ROUNDTRIP` | `serde_json` wird mit `float_roundtrip` gebaut; jeder f64-Round-Trip ist exakt | Eigentümerentscheid 2026-10-02: „es soll nichts verloren gehen" (gemessen: 7,45 % der f64-Werte verloren beim Laden 1 ULP). Kostet eine Neubewertung aller Sidecar-Bytes/Digests/Goldens | entschieden 2026-10-02; Umsetzung in `JSON-FLOAT-ROUNDTRIP` |
| `NAMING-F1 (2)` | Der **Anzeigename** bleibt „Lumina" (`crates/lumina-gui/src/main.rs:30` Fenstertitel, `lib.rs:11965` UI-Überschrift); der Formatbezeichner `.lumina.*` bleibt ohnehin | Eigentümerentscheid 2026-10-02 (Option „Lumina beibehalten"); keine Migration, null Zeilen | gilt |
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
| Defekt-Claim „ein Regler-Drag verliert den entprellten Sidecar-Save" (`PLAN-ORPHAN-1`, als Waise aus dem Plan entfernt, 2026-10-02) | Widerlegt, und **mit zwei falschen Quellenangaben** überliefert. (1) `full_render_debounce_remaining` steht in `crates/lumina-gui/src/render_tick.rs:35–41` — **nicht** `29–35`, das ist der Doc-Kommentar (6 Zeilen daneben). (2) `slider.rs:203–206` **bewaffnet `pending_full_render` überhaupt nicht**: `grep -n "pending_full_render" crates/lumina-gui/src/slider.rs` liefert **null Treffer**; der Bewaffner sitzt in `dirty.rs:51` (`mark_dirty`), erreicht aus einem Slider-Drag über `set_adjustment` → `mark_recipe_dirty` → `mark_dirty`. (3) Die Messbehauptung „am 4-Frame-Exposure-Drag gemessen" hat **keinen** Test im Baum, der `dragged()` fährt. Die Wirkungsrichtung der Aussage stimmt, die angegebene Datei nicht | Der Defekt existiert nicht und ist **nicht** zu fixen; `pending_full_render = true` steht an vier Stellen (`dirty.rs:51`, `lib.rs:7981`, `render_schedule.rs:69`, `warmup.rs:173`), ein veralteter `last_edit_time` macht den Commit **eifriger**, nicht strandend. Ein Reproduktionsfall wurde nie belegt |
| Vorziehen des **Crop**-Schritts vor die Geometrie-Kette/die Tonalstufe (`PIPELINE-CROP-EARLY-9`, geschlossen 2026-10-16) | Belegte **Totalordnung** `SpotHeal ≺ Adjustments ≺ Lens ≺ AutoFill ≺ Perspective ≺ Expand ≺ Crop ≺ LensBlur ≺ Masks` — jede Nachbarbeziehung ist durch eine Koordinaten-/Semantik-Invariante erzwungen: H1 expand→crop (`render.rs:634`, `lib.rs:593–595`, `generative.rs:709–738`), H2 Lens/Perspective→Crop über den Default-Content-Crop (`lib.rs:583–590`, `crop_max_rect.rs:41–83`), H4 Tonal-vor-Geometrie (`feature/architecture/pipeline.md:41–42`). Ein Crop vor Tonal **ändert Bytes** bei aktivem Warp (14,58 % Kanäle, `feature/quality/pipeline-crop-early-2026-10-13.md:38`). Es existiert kein tauschbares Nachbarpaar | Crop bleibt **direkt nach `composite_expand`** (`render.rs:678`). Die 85 %-Gewichtung der Tonalstufe ist am `perf/baseline.json` **nachgerechnet** (85,2/85,7/85,8 %), gilt aber nur im **geometriefreien** Bench-Rezept; die heutigen 9–10 % Crop-Ersparnis (`perf/crop_pipeline_measurement.json`) stammen aus einem **Frame-Klon**, nicht aus der Tonalstufe. Analyse durch einen deepseek-v4.1-Flash-Subagenten, abgenommen durch den Eigentümer am 2026-10-16 |

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
| `GPU-RENDER-MASK-19` (361) | **teilweise** | CLI-Teil geliefert (Runde 2, BESTANDEN); offen bleibt nur der GUI-Present-Pfad: `cargo test -p lumina-gui --lib` | der GPU-Paritaetsnachweis braucht `--features gpu-adapter-tests` **mit** Adapter |
| `GPU-RENDER-NONDETERMINISM-69-HW` | **nein** | — | Adapter-Nachweis (Metal/Vulkan): `cargo test -p lumina-gpu --features gpu-adapter-tests` plus GUI-GPU-Tests (Regel `GPU-E2E`) |
| `GPU-MASKPLANE-CLEAR-REMEASURE` (362) | **nein** | — | ruhige Session **mit** Adapter (Fremdlast verfaelscht die Messung) |
| `MCP-PARITY-C` (408) | **nein** | — | echte RAW-Fixture-Reihe (Referenzdaten) |
| `LRPAR-G09-CULL-IMPL-25` (418) | **teilweise** | CLI-/Heuristik-Teil headless | kittest-Goldens + ONNX-Stufe 2 brauchen Adapter/Gewichte |
| `LRPAR-G14-DENOISE-IMPL-20` (417) | **nein** | — | echte Modell-Gewichte (F-078-Gate) |
| `R5-DUST-23-FOLLOWUP` (375), `R5-BRUSH-24` (376), `R5-MASKVIS-25` (377) | **nein** | — | Metal/Vulkan + echte Maus-/DPI-/Pointer-Ereignisse |
| `F-103-N6` (513), `F-103-N6-GUI-COVERAGE-27` (482) | **nein** | — | sichtbarer `RUST_LOG=trace`-GUI-Lauf durch einen Menschen (R5-LOG-1) |
| `R2-GUIMOD-04b` (510), `R2-GUIMOD-04c` (511) | **nein** | — | Messwerte aus `F-103-N6` (manueller Lauf) |
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
   (`denoise_ai (not GPU-wired)`, `lumina-gpu/src/lib.rs:323-324`); für
   `PREVIEW`/`EXPORT` ist sie **nicht** ausgeschlossen, aber unbelegt.
   **Zweiter Durchgang 2026-09-29 (Lese-Audit + eigene Nachmessung):** Von den
   vier `GPU-RENDER-*-19`-Zeilen ist eine **Dublette** und drei sind ungeklärt.
   `GPU-RENDER-DENOISE-19` ist **(D)**: die Arbeit läuft unter
   `LRPAR-G14-DENOISE-IMPL-20` (eigene Task-Zeile, Release 2.0), und genau diese
   ID nennt `lumina-gpu/src/lib.rs:324` als Beleg der Lücke — der Code führt
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
| 2.5 | LRPAR-G09-CULL-IMPL-25 | G-09 | KI-Culling-Impl |
| 1.5 | MCP-MASK-APPLY | offen — bei der Task | Masken anwenden (MCP) |
| 1.5 | MCP-MASKPOLICY | offen — bei der Task | MaskPolicy-MCP |
| 1.5 | MCP-PARITY-A | verifiziert geschlossen (2a88d3b) | MCP-Stage-Editoren |
| 1.5 | MCP-PARITY-B | offen — bei der Task | MCP-Bibliothek+Stufe |
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



- [ ] **[PRIO: mittel] GPU-RENDER-MASK-19 (Release 1.0; aus dem Releaseplan-Audit als EINZIGE echte Lücke belegt, 2026-10-02)** Der **GPU-Masken-Pixelpass** fehlt: lokale Masken modulieren auf der GPU keine Pixel. **Gemessen (Audit 2026-10-02, eigene Nachmessung):** `crates/lumina-gpu/src` hat `combine_mask_planes` (`lib.rs:615`) und `upload_mask_plane` (`lib.rs:2782`) — Upload/Clear/Combine, **nicht** den Pass; `grep -rniE 'local_adjust|mask_layer|apply_mask' crates/lumina-gpu/src` → **0 Treffer**; `upload_mask_plane_roundtrip_is_byte_exact` (`tests/stages.rs:261`) belegt nur den Upload-Roundtrip. `GPU-PARITY-MASKGATE-1` ist ein Present-Gate, nicht der Pass; `GPU-MASKPLANE-BRUSH-50`/`-CLEAR-PERF-51` sind Brush-Upload bzw. Clear-Performance; `MASK-LOCAL-P0`…`P1.2d` bauen das Rezept-Datenmodell. Die CPU-Referenz moduliert bereits (`apply_local_adjustments`), die GPU-Stufe fehlt (dokumentierte F-042-Grenze, `pipeline.md:1885-1890`). **Abnahme:** (1) eine WGSL-Stufe, die die lokal ausgewertete Maske (Alpha) pixelgenau auf den Composite anwendet — keine CPU-Route, keine stille Ausnahme (volle GPU-Parität, User-Entscheid 2026-09-12); (2) CPU-Oracle-Parität byte-identisch wo 0, sonst maxAbsDiff ≤ 1 (Muster `parity.rs`); (3) E2E am Adapter: `cargo test -p lumina-gpu --features gpu-adapter-tests` **plus** die betroffenen GUI-GPU-Tests (Regel `GPU-E2E`); (4) SOLL in `feature/architecture/pipeline.md` §GPU und `feature/product/ai-masks.md` von „dokumentierte F-042-Grenze" auf „GPU-Stufe vorhanden" umstellen; (5) der Masken-Pass wird im Render-Key/der Route sichtbar (kein stiller CPU-Fallback). **Abgrenzung:** der **Pass** selbst; Upload/Clear/Brush bleiben bei ihren Tasks. Parent-Gate-Referenzen: `pipeline.md:2261`, `ai-masks.md:349`. **STAND 2026-10-15 (Slice A geliefert und verifiziert):** der Pass **existiert und ist fachlich korrekt** — Commit `1981330` (`crates/lumina-gpu/src/local_mask_pass.rs` 372 Z., `src/local_mask_gpu.rs` 295 Z. nur `feature = gpu`, `tests/local_mask_parity.rs` 439 Z.; `lib.rs` +4 Verdrahtungszeilen). Am Metal-Adapter gemessen: `cargo test -p lumina-gpu --features gpu-adapter-tests` → lib 56/0, golden 7/0, init_warning 1/0, lensfun_map 3/0, lensfun_tca_skip 0/0, **local_mask_parity 5/0 (neu)**, parity 27/0, stages 4/0 — keine Regression. CPU-Oracle-Parität: P0-only, relative WB und der Mehr-Layer-/Global-Fall **byte-identisch** (`maxAbsDiff = 0`), WB-Zelle deklariert `Bounded(1)` und **misst 0** (Probe `Bounded`→`ByteIdentical` passed). Unabhängige Verifikation (`deepseek-v4.1-flash`) mit **5 eigenen Mutationen** (Rundungskonstante → 3/2 failed; `rev()` auf die Layer-Reihenfolge → 4/1, `maxAbsDiff 63`; `65535u - alpha` → `alpha` → 2/3, alle Hardware-Zellen rot; `adj.rgb` → `adj.bgr` → 2/3 rot; Endpunkt-`alpha == 65535u` → `>` → 5/0 grün, weil die Bruchformel denselben Wert liefert) — jede Mutation trug eine Aussage, jede exakt per SHA-256 restauriert. Ratchet/Anti-Gaming/fmt/Clippy clean. **VERDICT: NICHT BESTANDEN** — *nicht* wegen des Passes, sondern wegen Abnahme-Punkt 5. **Offener Befund `GPU-RENDER-MASK-19-INTEGRATION` (HIGH):** der Pass wird von **keinem** Produktionspfad aufgerufen. Repo-weiter Grep über `render_with_gpu_local_masks`/`unsupported_local_mask_stages`/`validate_local_mask_layers`: 19 Treffer, **alle** Definition/Interna in `local_mask_pass.rs` oder im Test — **null** in `crates/*/src`, `crates/*/tests`, benches. GUI (`crates/lumina-gui/src/gpu_routing/local_adjustments.rs:15`, `mask_local_controls.rs:42`) routet jeden sichtbaren lokalen Maskenzustand über `local_adjustment_route_reason()` weiter auf die CPU; CLI (`crates/lumina-cli/src/main.rs:459`) hängt `masks (active copy has layers)` an die Routing-Gründe → CPU. Nach `Agents.md` ist ein Modul, das kein `draw_*`-/Render-Pfad aufruft, **nicht integriert**. Der CPU-Fallback ist **laut** (Routing-Grund + Log), nicht still — das mildert den Befund, schließt ihn aber nicht. **Was Slice A NICHT leistet:** (a) P1.2-Blöcke (lokale Tonkurve/Farbe/Presence/Detail) sind nicht GPU-verdrahtet — `unsupported_local_mask_stages` nennt sie als CPU-Routing-Grund, `render_with_gpu_local_masks` verweigert sie laut; (b) kein Pipeline-Cache (`build_local_mask_pipeline` baut die zwei Pipelines pro Aufruf); (c) Render-Key-Bezug ungeprüft (`lumina-gpu` hat keinen RenderKey); (d) die WB-`Bounded(1)`-Schranke ist nur auf **einem** Fixture belegt (`maxAbsDiff = 0`), ein `f32`-vs-`f64`-Divergenzfall wird nicht provoziert; (e) `cargo doc -D warnings` ungeprüft (6 vorbestehende private-item-Links, keine aus den neuen Dateien). **STAND 2026-10-15 (Slice B1 geliefert und unabhängig verifiziert — BESTANDEN):** die zwei fehlenden **Nahtstellen** sind da, **ohne** Verhaltensänderung. (1) `lumina_core::evaluate_local_mask_layers(context, input_w, input_h, frame_w, frame_h)` in `crates/lumina-core/src/render/mask_evaluation.rs` (150 → 335 Z.; Re-Export `lib.rs` **+1 Zeile = exakt Baseline 2781**) — ein **dünner** Wrapper, der genau `prevalidate_local_adjustments` + **die eine** `super::evaluate_mask_stage(...)` ruft; `render.rs` bleibt **exakt 2536** (nur `mod` → `pub mod`). (2) `GpuContext::apply_gpu_local_masks(&self, global, layers)` und `unsupported_local_mask_adjustments(layer_id, adjustments)` in `crates/lumina-gpu/src/local_mask_pass.rs` (372 → **483** Z., ≤ 500) — `render_with_gpu_local_masks` ist jetzt exakt `render_with_gpu` + diesem zweiten Schritt; `run_local_mask_chain` hat **eine** Definition und **einen** Aufrufer. Unabhängige Verifikation (`deepseek-v4.1-flash`) zählte „genau ein Auswertungspfad" selbst nach: `MaskGraph::new(` **1×** im Produktionscode (`mask_evaluation.rs:48`), `modulate_mask_plane` 1 Definition/1 Produktions-Caller, `evaluate_mask_stage` **1** Definition bei **2** Aufrufern (CPU-Render `render.rs:691` + neue Naht `mask_evaluation.rs:173`) — also geteilt, keine Kopie; **kein** Sichtbarkeitsleck (`evaluate_layer`/`LayerFailure` bleiben `pub(super)`). Testzahlen selbst gemessen: `lumina-core --lib` **551/0**, `lumina-gpu --lib` **58/0**, GPU-Pflichtgate `--features gpu-adapter-tests` exit 0 mit `local_mask_parity` **6/0 (0 ignored)**, `parity` 27, `golden` 7, `stages` 4. Drei eigene Mutationen am Produktionspfad: `take(0)` auf der Maskenschleife → **3** Orakel-Tests rot; P1.2-Prädikat neutralisiert → `--lib`-Unit-Test **und** die P1.2-Zelle rot; `frame_width + 1` im Plumbing → Naht-Test rot; jede per SHA-256 byte-identisch restauriert. `check_file_sizes.sh` exit 0, `fmt`/`clippy -D warnings` clean, **kein** CLI-/GUI-Byte geändert. Zwei **low**-Beobachtungen daraus: (i) der neue Adapter-Test pinnt nur das **Delegationsverhältnis**, nicht die Kettenwirksamkeit — der Doc-Kommentar wurde entsprechend ehrlich gestellt, die Kette selbst decken die drei Orakel-Tests (mutationsbewiesen); (ii) der Neutral-Frühausstieg (`Ok(global.clone())`) und die `AdapterUnavailable`-Verweigerung von `apply_gpu_local_masks` sind code-korrekt, aber **nicht** direkt getestet (vorbestehend aus Slice A): der **Neutral-Fall** wird in Slice B2 testbar mitgenommen, der **No-Adapter-Zweig** ist auf dieser Maschine (Metal vorhanden) nicht auslösbar und bleibt als benannte, ungeprüfte Lücke stehen. **Der Integrationsbefund `GPU-RENDER-MASK-19-INTEGRATION` bleibt offen (HIGH):** B1 liefert nur die Naht, **keinen** Caller. **Nächster Slice (B2 — CLI-Verdrahtung):** neues gpu-only `crates/lumina-cli/src/local_mask_gpu.rs` (Muster `lensfun_gpu.rs`), Ersetzung des pauschalen `masks (active copy has layers)` (`main.rs:459`) durch das P1.2-Prädikat (netto ≤ 0 Zeilen, `main.rs` bleibt **≤ 7874**), im GPU-Zweig von `render_best_effort` `evaluate_local_mask_layers` → `apply_gpu_local_masks` binden und `RenderOutput.mask_layers`/`mask_warnings` füllen; ein Verweigern des Passes wird **laut** behandelt (Log + exakter CPU-Pfad, damit `--require-gpu` laut scheitert) — **kein** stiller un-modulierter Frame. Die **GUI**-Verdrahtung (Present-Pfad, `local_adjustment_route_reason`) bleibt ein eigener, späterer Slice mit klickbarem headless-Test/Golden und wird **nicht** mit-erledigt. **STAND 2026-10-15 (Slice B2 geliefert, in Runde 2 unabhaengig verifiziert — BESTANDEN):** die **CLI** ruft den Pass jetzt produktiv auf; der HIGH-Befund `GPU-RENDER-MASK-19-INTEGRATION` ist damit **fuer die CLI geschlossen** (offen bleibt allein der GUI-Present-Pfad). Neu: gpu-only `crates/lumina-cli/src/local_mask_gpu.rs` (229 Z.) mit `mask_layer_routing_reasons` (aktive Kopie; unsichtbare und neutrale Ebenen uebersprungen, sonst `unsupported_local_mask_adjustments`), `composite` (Maskenstufe **immer** auswerten → id-basierte Paarung → `apply_gpu_local_masks`), `pair_layers`, `active_copy`, `gpu_output`; `main.rs` **7874 → 7869** (netto −5: der pauschale Grund `masks (active copy has layers)` ersetzt, der GPU-Zweig ruft `composite`, ein `Err` wird zur **lauten** CPU-Route mit `log_cpu_routing_once` und dem exakten CPU-Pfad — kein stiller un-modulierter Frame). Argument-Abbildung wie im CPU-Render: `input_* = frame.width/height` (Decode-Quelle vor Geometrie), `frame_* = global.width/height` (Post-Geometrie), gegen `render.rs:611-612`/`697-698` geprueft. **Bewusste Abweichung von der Vorgabe:** `composite` wertet die Maskenstufe auch ohne sichtbare nicht-neutrale Ebenen aus, weil der CLI aus `RenderOutput.mask_layers` die `match_total_exposure`-Maskendomaene ableitet; nur der **GPU-Pass** laeuft bedingt. **Runde 1 (Commit `2e3b56a`) war NICHT BESTANDEN** — HIGH-Befund: `pair_layers` lief unbedingt, dadurch fiel **jedes maskenfreie Rezept** mit dem falschen Grund „the active copy vanished before layer pairing" auf die CPU (A/B selbst gemessen: pristine `bc9e090` exit 0 / route `gpu` / reasons `[]` gegen `2e3b56a` exit 1 / route `cpu` / reasons `["masks (GPU local-mask pass: the active copy vanished before layer pairing)"]`). **Runde 2 behebt das** (`evaluated.is_empty()` → No-Op vor `pair_layers`; ein nicht-leeres `evaluated` setzt die aktive Kopie voraus, der Fehler bleibt fuer eine echte Divergenz reserviert) und pinnt drei weitere Befunde: der Sichtbarkeits-Skip ist jetzt **tragend** (`invisible_p12_layer_is_not_a_cpu_route_reason` mit P1.2-Block; ein neutraler P1.2-Block ist per Definition unmoeglich, weil `is_neutral()` alle vier P1.2-Praedikate ausschliesst), der F2-Vertrag „kein stilles `Ok(Gpu)` mit un-moduliertem Frame" (`unavailable_mask_fails_loudly_not_with_a_silent_gpu_frame`), und `cargo check -p lumina-cli --features gpu,lensfun --tests` kompiliert (fehlender `RawMetadata`-Import, dieselbe Klasse wie der `Geometry`-Fix). Testzahlen selbst gemessen: `--bin lumina-cli` **146/0** ohne gpu; mit gpu **158/1** — der eine rote Test ist **genau** der benannte, vorbestehende Nicht-Determinismus `auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move` (eigener Task `GPU-RENDER-NONDETERMINISM-69`), **kein** neuer Test; `tests::local_mask_gpu` **10/0**, `tests::local_mask_gpu_route` **3/0**; vier Mutationen am Produktionspfad (F1-Fix rueckgaengig; Sichtbarkeits-Skip entfernt; `apply_gpu_local_masks(global, &[])`; `main.rs`-`Err`-Zweig → un-moduliertes `global` als `Gpu`), jede per SHA-256 byte-identisch restauriert. **Benannte, ungepruefte Luecke:** der `Err`-Zweig-Ausgang „erfolgreicher CPU-Pfad als `RenderRoute::Cpu`" ist mit den heutigen Routing-Regeln **unerreichbar** (jeder `composite`-Fehler laesst unter `Strict` auch das CPU-Orakel scheitern) und daher nicht end-to-end gepinnt — defensive Doppelabsicherung, kein stummer Fallback; ebenso bleiben die zwei adapter-gated Tests in CI ohne Aussage (bestehende CLI-GPU-Policy, kein neuer Bruch). **Offen:** der **GUI-Present-Pfad** und **P1.2** (laut verweigert). **Neu erkannt beim B2-Abschluss:** der CLI-`gpu`-Testbuild wird von **keiner** CI gebaut (`CI-CLI-GPU-TESTBUILD-68`).
- [ ] **[PRIO: hoch] GPU-RENDER-NONDETERMINISM-69-HW (Adapter-Nachweis; Regel `HEADLESS-GPU-TASK-SPLIT`)** Der headless Teil von `GPU-RENDER-NONDETERMINISM-69` ist abgeschlossen: die Analyse ist dokumentiert in `feature/quality/gpu-nondeterminism-2026-10-16.md`. **Dieser Task fuehrt nur den Adapter-Nachweis:** Reproduktion des roten Laufs auf echter Hardware (Metal/Vulkan), Mechanismus jenseits der Projektgrenze eingrenzen, beheben oder endgueltig als Grenze bestaetigen. **Abnahme:** ein **vorher rotes, reproduzierbares** Kommando auf Adapter-Hardware, danach gruen **oder** eine belegte, benannte Grenze inkl. Adapter-Referenz; verbindliches Kommando `cargo test -p lumina-gpu --features gpu-adapter-tests` plus die betroffenen GUI-GPU-Tests (Regel `GPU-E2E`). **Abgrenzung:** nur der Adapter-Nachweis; die headless Analyse ist abgeschlossen.


### PRIO: hoch — LR-Parität (Welle 1/2; Gate-Abhängigkeit siehe oben)

**LR-Parität aus `.goal/Goal.md` (Batch 1 + User-Featureliste, Stand 2026-09-03, 16 Goals, Aggregat ~39,1 %)**

Ziel: UI zum Verwechseln ähnlich zu Lightroom Classic; jedes Feature auf
CLI- **und** GUI-Ebene getestet. Quelle: `.goal/Goal.md` G-01…G-16, Beleg:
`.goal/lighroom screenshots batch 1/Content.md`. Umsetzung je Task via
`general`-Implementierungs-Agent + unabhängiger `general`-Verifizierungs-Agent
(Regel oben). LR-Parität-Batch aus 2026-09-04 (teils erledigt s. Git-Historie; Rest s. Releaseplan/Blöcke).

- [ ] **[PRIO: hoch] R5-DUST-23-FOLLOWUP (User-Order 2026-09-20; Remediierung 2026-09-23)** Dust-Erweiterung: Anzeige nur bei Auswahl (Ausgewähltes wie Maske), Entfernungen bearbeitbar inkl. Neu-Generierung, Typ Generate/KI-generiert (Clone nie verwendet). **Stand nach R5-DUST-23-FOLLOWUP-Remediierung:** CLI-Kontradiktionsmatrix mit atomarem No-Write-Failure, eindeutige Spot-IDs/compat-ID-Materialisierung für typed-only generative Einträge, Referenz-basierter missing/stale/corrupt-Status in GUI+CLI und 1024×720-Selected-Detail-Anordnung sind implementiert und headless abgedeckt; die unabhängige Funktionsverifizierung ist bestanden. Der Task bleibt wegen des separaten Hardware-Gates offen. **Konkreter manueller GPU-Nachweis bleibt erforderlich:** auf echter Hardware (Metal/Vulkan, nicht llvmpipe/Software) `RUST_LOG=trace cargo test -p lumina-gpu --features gpu-adapter-tests --test parity` mit genau einer App-Instanz und Log-Redirect ausführen; GUI-R5 darf bis dahin nicht als vollständig abgeschlossen gelten. **Kommando korrigiert 2026-10-02 (`GPU-E2E`):** `--features gpu` allein lässt 22 der 27 Parity-Tests `#[ignore]`t (gemessen: 5 passed / 22 ignored) und belegt **keine** Hardware-Parität; verbindlich ist `--features gpu-adapter-tests` (gemessen: 27 passed / 0 ignored, Metal 4).
- [ ] **[PRIO: hoch] R5-BRUSH-24 (User-Order 2026-09-20, User-Urteil „großer Fail", nach Sort-Mini-Welle)** Pinsel-Masken auf Lightroom-Niveau: Größe/Weichheit/Fluss einstellbar (Slider + `[`/`]`-Shortcuts als Alias), Kreis-Cursor mit live Größe am Zeiger, mehrere Masken pro Bild anlegbar + einzeln wählbar (Pin-Liste klickbar), dazu vollständige Maskenverwaltung (Liste aller Masken mit Sichtbarkeits-Toggle, Umbenennen, Löschen, Reihenfolge, Duplizieren — alles klickbare Buttons). **Stand 2026-09-24:** Code-Level-Funktionsverifizierung bestanden: CPU/no-GPU, Persistenz-Atomizität, kumulative Live-Plane/Overlay, 110-Action-Audit und 1024×720-Struktur/Golden sind grün. Der Task bleibt bis zum echten Metal/Vulkan-/DPI-/Pointer-Nachweis offen; der Native-Kittest-Golden wird separat mit `--ignored` geprüft.
- [ ] **[PRIO: hoch] R5-MASKVIS-25 (User-Order 2026-09-20, präzisiert, nach Sort-Mini-Welle)** Masken-Overlay nur sichtbar, wenn die Masken-Ansicht geöffnet ist; togglebar: nur Pins für alle Masken vs. volles Overlay für die aktuell ausgewählten Maske. Dazu Bildbereich vergrößern (Seiten-Panels ausblendbar für maximale Preview). **Stand 2026-09-24:** State-Machine, CPU/GPU-Gates, Overlay-Farbparität, 110-Action-Audit und R5-Goldens unabhängig verifiziert; der Task bleibt bis zum echten Metal/Vulkan-/DPI-/Pointer-Nachweis offen.


### PRIO: hoch — Produktbefund aus GOLDEN-FIXT-31






**Stand 2026-09-26 (Build-Agent + Reparaturlauf; unabhängige Verifikation PASS nach einem FAIL, Details unten):** Schema v6 mit `detail: Option<Detail>` (`crates/lumina-sidecar/src/detail.rs`: die **globalen** `Sharpening`-/`NoiseReduction`-Structs plus der lokale Container, alle drei aus `lib.rs` extrahiert); die Bereichsvalidatoren liegen in `crates/lumina-sidecar/src/detail_block.rs` (`validate_sharpening`/`validate_noise_reduction`/`sharpening_is_neutral`/`noise_reduction_is_neutral`/`SHARPENING_FIELDS`/`NOISE_REDUCTION_FIELDS`/`sharpening_radius_range`) und werden von globalem (`validate_adjustments`) und lokalem Rezept **wörtlich geteilt** — die Fehlerstrings sind unverändert. v1..v5 migrieren verlustfrei mit `detail: None`, ein v1..v5-Payload mit `detail` (auch `null`/Nicht-Objekt/`{"__unset__":true}`) ist laut; die Versions-Gates bleiben versionsrichtig verankert (`CURVE_…=3`, `COLOR_…=4`, `PRESENCE_…=5`, `DETAIL_…=6`), ein v5-Dokument behält seinen Presence-Block (eigener Test). `adjustment_detail`/`adjustment_sharpening`/`adjustment_noise_reduction` bleiben „unknown local adjustment". CPU-Kern `crates/lumina-core/src/detail_stages.rs` (5x5-Bilateral-Gewichte und -Mittelung, Chroma-Offsets, separierbares Gauß inkl. der globalen Radiusformel `sigma = max(r·scale, 0.5)`, Rec.709-Luminanz, `gx`/`gy` + Frame-Maximum, Detail-Mixing, Flat-Area-Faktor) plus `crates/lumina-core/src/render/local_detail.rs` (**eine** Quantisierungsgrenze; die lokale Kernel-Funktion nimmt **weder Maske noch ROI** entgegen — genau das ist der tragende Beweis, und der Boundary-Test ruft sie ohne Maske auf und vergleicht mit dem voll maskierten Render). `local_color.rs` wurde in `local_tone_and_color_normalized` (un-quantisiert) + `local_tone_and_color_stages` (eine Rundung) geteilt, damit die Detail-Stufen auf dem un-quantisierten Farbresultat laufen, ohne dass ein P1.2b/P1.2c-Layer ein Byte verliert. Render-Scale: `render_frame_from_base_impl` berechnet **eine** `effective_scale` und gibt sie sowohl an `apply_recipe_with_scale_white_balance_and_denoise` als auch an `apply_local_adjustments` → `apply_mask_local_recipe_with_scale`; es gibt kein persistiertes lokales Scale-Feld, und die Radius-Formel ist per Doppel-Skalen-/Doppel-Radius-Identität **für den lokalen und den globalen Pfad** gepinnt. Inhaltliche Pfadwahl: die Neutralität eines Sharpening-Blocks ist `amount == 0` (der eigene Early-Return des globalen F-095-Stages), also ist `masking = 0` die **stärkste** Einstellung und nie ein stiller No-op, und ein Radius kann einen Block nie neutral machen (`sigma`-Floor `0.5`, erster Nachbartap `exp(−2) ≈ 0.135`). GUI `crates/lumina-gui/src/mask_local_detail.rs` (Editor + Setter/Reset pro Bereich und gesamt, keine globale Rezeptmutation, kein Scale-Widget; `is_neutral` trägt den Block, also verweigern alle Stand-in-/GPU-Routen sichtbar, und der Verweigerungstext nennt „detail"). CLI `crates/lumina-cli/src/mask_local_detail.rs` im generischen Kanal (`sharpening.<field>=`/`noise_reduction.<field>=` plus die Resets `sharpening`/`noise_reduction`/`detail`); `main.rs` unverändert bei 11398. Neue/gezielte Anker: Core `local_sharpening_amount_has_an_exact_golden_and_preserves_alpha`, `local_sharpening_radius_has_exact_goldens_at_both_legal_boundaries`, `local_sharpening_detail_has_exact_goldens_for_both_ends_of_the_mix`, `local_sharpening_masking_has_an_exact_golden_and_suppresses_the_flat_area`, `a_masking_zero_sharpening_block_is_not_a_silent_no_op`, `local_noise_reduction_luminance_has_an_exact_golden_and_preserves_alpha`, `local_noise_reduction_color_has_an_exact_golden`, `local_noise_reduction_runs_before_local_sharpening` (Golden **plus** unabhängige f64-Nachrechnung beider Reihenfolgen), `the_full_local_detail_stack_has_an_exact_golden`, `local_detail_json_round_trip_is_byte_stable`, `detail_resets_work_per_sub_block_and_for_the_whole_block`, `the_detail_block_is_part_of_the_local_render_identity`, `two_overlapping_local_detail_layers_use_persisted_order`, `local_detail_sits_after_the_colour_block`, `local_detail_alpha_endpoints_and_partial_alpha_are_exact`, `no_local_detail_keeps_the_p0_p11_p12a_p12b_p12c_bytes`, `invalid_local_detail_values_are_a_loud_preflight_error`, `a_local_detail_edit_never_touches_the_global_recipe`, `the_local_detail_layer_owns_exactly_one_quantization_boundary` (unabhängige Nachrechnung in `local_detail_reference.rs` **plus** der Nachweis, dass früheres Runden ein anderes Byte liefert), `the_detail_neighbourhood_is_full_frame_and_the_mask_only_gates_the_blend`, `the_local_detail_follows_the_global_render_scale`; Sidecar `local_detail_round_trips_through_the_sidecar_file`, `legacy_versions_migrate_losslessly_and_refuse_a_smuggled_detail_block`, `local_detail_uses_the_existing_global_ranges`, `neutral_absent_and_neutral_detail_blocks_read_the_same_and_reset_clears`, `canonical_digests_cover_the_local_detail_block`, `disabled_local_denoise_and_optics_stay_rejected_and_the_legacy_keys_stay_p0`; CLI `local_detail_round_trips_through_the_sidecar_file_and_resets`, `per_area_detail_resets_remove_exactly_one_sub_block`, `local_detail_edit_reaches_pixels_through_the_cpu_compositor`, `invalid_local_detail_specs_are_loud_and_change_no_bytes`, `local_detail_survives_history_and_reload_verbatim`, `previous_refuses_a_detail_only_local_state_on_source_and_target`; GUI `a_local_detail_edit_changes_the_render_identity_and_never_the_global_recipe`, `local_detail_setters_and_resets_cover_every_field`, `invalid_local_detail_edits_are_refused_without_mutating`, `a_detail_only_local_layer_refuses_the_stand_in_routes`, `local_detail_state_digest_invalidates_mask_and_render`, `gui_and_cli_local_detail_share_the_same_persisted_typed_block`, `local_detail_survives_history_and_previous`, `the_local_detail_editor_paints_and_writes_only_the_mask_layer`.

**Verifikations-Rundlauf 2026-09-26 des **ersten** Build-Agent-Laufs — **UEBERHOLT durch die Reparatur unten; dieser Lauf beschreibt den defekten Stand, nicht den gepushten.** Der damalige Lauf (eigenes Target `target/p12d`, gepinntes LibRaw 0.22.2

**Offen:** (1) die **unabhängige Verifikation ist PASS** (zweiter Agent, eigene Byte-Harness; Details im Verifikationsabsatz weiter oben) — inhaltlich erledigt, offen bleibt der Punkt nur wegen der unten genannten Gates; (2) die drei kittest-Goldens `develop_basic`/`develop_sections_expanded`/`develop_section_masking` brauchen weiterhin einen `UPDATE_SNAPSHOTS=true`-Refresh auf der Referenz-Maschine (headless wgpu-Adapter nötig) — das verbleibende native Golden-Gate aus P0/P1.1/P1.2a/P1.2b/P1.2c; die UI-Änderung fügt Zeilen in der Masking-Sektion hinzu, `develop_section_masking`/`develop_sections_expanded` sind also betroffen, und **keine** `.png` wurde angefasst; (3) die 10 vorbestehenden Lensfun-Datenbank-Tests bleiben P1.2d-fremd rot (fehlendes `/usr/share/lensfun`, auf HEAD 8101c6d reproduziert); (4) **AI-Denoise und Optics bleiben bewusst deaktiviert** — der lokale Renderer darf sie weder aktivieren noch als Stub vortäuschen, ein entsprechender Key ist ein lauter „unknown local adjustment"-Fehler, und Optics bleibt **dauerhaft** deaktiviert, weil Linsenkorrektur/Perspektive geometrische Stufen vor den Masken sind, AI-Denoise bis zum F-078-Modellgate; (5) die Parent-Gates `GPU-RENDER-MASK-19`, `GPU-PARITY-HW-28`, `R5-BRUSH-24` und `R5-MASKVIS-25` bleiben offen, und es wird **keine** GPU-Parität behauptet.


**Stand 2026-10-02 (zwei unabhaengige Verifikationslaeufe, beide NICHT BESTANDEN; Commits 9f3a893, d77db28, 53e1ac0; der Task bleibt OFFEN):** die Verifikation hat nicht am Fix gelegen, sondern an **Behauptungen ueber eigenen Code** — und die hat sie zu Recht zweimal geschlagen. **Lektion dieser Runde, in der Reihenfolge, in der sie mich getroffen hat:** (a) `6f46e0d` hatte seine eigene Hauptaussage zurueckgenommen ("GEMESSEN FALSCH. Kein GUI-Rezept war je stale") — aber nur im Commit-Text und hier, **nicht an der Fundstelle**: der Code-Kommentar an `LuminaApp::auto_tone` behauptete weiter "two consequences, both measured: ... permanently stale", mit gefaelschtem Beleg. DoD §9 verlangt die Ruecknahme AN DER FUNDSTELLE. Vier Fundstellen korrigiert (Code-Kommentar, Modul-Doku in `lumina-stages/src/auto_tone.rs` mit dem jetzt ueberholten GUI-Ausschluss, `pipeline.md` mit zwei Tode-Verweisen auf `auto_tone_cli.rs` als Writer-Ort, `cli-gui-wasm.md`). (b) Der G-16-**Endpunkt** verletzte Klausel (2) seit HEAD: er schrieb 1 von 6 Reglern, 1 von 6 Spiegeln, `enable_auto_tone` und den Fingerprint und persistierte das — ein gemischter Zustand. Die Begruendung in `6f46e0d` verwechselte Klausel (1) mit Klausel (2), und `g16_apply_auto_endpoint_sets_only_its_field` **pinnte den Defekt als erwartetes Verhalten** (`assert_eq!(auto_blacks, None)`). Behoben (d77db28): der Endpunkt nimmt seinem Regler den Spiegel, schreibt sonst keinen Auto-Tone-Zustand. **Warum gerade das — gemessen, zwei Auswege ausgeschlossen:** ohne den Spiegel-Verzicht loescht `clear_stale_auto_tone` den ausdruecklichen Nutzerwert (Sonde rot, `None` statt `0.2396...`); ein zweiter voller Sechser-Lauf ueberschreibt fuenf unberuehrte Regler. **Der Preis ist benannt statt weggeredet: nach einem vollen Auto-Tone-Lauf bleiben 5 von 6 Spiegel — ein echter gemischter Zustand**, und der Reparaturlauf **ueberschreibt einen danach von Hand gesetzten Wert** (gemessen: `-0.5` -> `0.0287...`). Das ist damit **keine Erfuellung von Klausel (2)**, sondern die guenstigste der drei Lesarten; die vollstaendige Loesung braucht ein Override-Konzept je Regler, das der Workspace nicht hat, und wird **nicht** behauptet. Gepinnt als Wächter auf das gemessene Fehlverhalten: `g16_the_state_repair_overwrites_a_later_manual_value_known_defect`. **Drei meiner eigenen Behauptungen waren falsch und sind an ihren Fundstellen zurueckgenommen:** (1) "NUR der Datenverlust-Test rot" — M-1 macht **vier** Tests rot, nicht einen; (2) "NUR der Self-healing-Test rot" — M-2 macht zwei; (3) `lib.rs 12553 -> 12571, +18` — die 12553 waren der **uncommittete** Zwischenstand eines abgebrochenen Agentenlaufs, eine Zahl, die es im Git nie gab; nachgezaehlt 12561 -> 12572, +11, und das Ledger nennt jetzt ausdruecklich seine zwei falschen Fassungen. Dazu der Testname `g16_apply_auto_endpoint_never_creates_mixed_auto_tone_state` (behauptete "never mixed", laesst 5 von 6 zu) umbenannt nach dem, was er tut. **Zwei Extraktionen/Aktionen mit echtem Nutzen:** `src/tests/g16_auto_endpoint.rs` nimmt die Auto-Tone-Zustandstests aus `g16_shortcuts.rs` (**314 -> 194 Z.** nachgezaehlt; `d77db28` nannte 314 als 509, das war der Arbeitsstand des abgebrochenen Laufs — **dieselbe Fehlerform wie bei der lib.rs-Zahl, an einer zweiten Stelle**), also unter die 500er-Schwelle, und `compute_auto_tone` gibt den Fingerprint nicht mehr zurueck (der einzige Aufrufer brauchte ihn nicht) — das ist -1 Zeile und zugleich eine Korrektur, denn der Doc-Kommentar dort behauptete, `auto_tone` rufe diese Funktion auf, was seit Klausel (1) falsch ist. **NOCH OFFEN, ausdruecklich nicht behoben:** (1) der Test `g16_auto_endpoint_after_auto_tone_keeps_its_value_across_a_stale_clear` ruft `clear_stale_auto_tone` **direkt** auf und faehrt den Load-Pfad nicht — eine extrahierte Testnaht ist noch kein Test der Produktion (Agents.md); (2) Klausel (2) ist fuer den Endpunkt weiterhin nicht vollstaendig erfuellt, siehe oben — **ABGESPALTEN als eigener Task `AUTO-TONE-ENDPOINT-MIXED-7` (Entscheidung des Eigentuemers 2026-10-02)**: die vollstaendige Loesung braucht ein Override-Konzept je Regler und damit Schema und Migration, was weit ausserhalb dieses Contract-Slices liegt; (3) die unabhaengige Verifikation des Korrekturlaufs 53e1ac0 steht aus. **STAND 2026-10-02, DRITTE Verifikationsrunde: BESTANDEN, ein Befund NIEDRIG, beide Zahlen von mir selbst nachgezaehlt.** Die dritte Runde hat bestaetigt, dass die Behauptungen der drei Korrekturlauf-Commits diesmal stimmen — lib.rs 12561 -> 12572, Suite 933/0/5, Testfunktionen 9 -> 14, Mutationszuordnungen M-1 = 4 Tests rot und M-2 = 2 (beide selbst nachgefahren, Namen exakt), **kein Test verschwunden**, Baseline-Anhebung ist Ladungsinhalt und keine Kompensation. **Der NIEDRIG-Befund ist dieselbe Fehlerform wie bei lib.rs, an einer zweiten Stelle:** `d77db28` behauptete fuer `g16_shortcuts.rs` "509 -> 194", im Git stand dort **314 -> 194** — die 509 war wieder der Arbeitsstand des abgebrochenen Agentenlaufs. Zwei unabhaengige Stellen, an denen eine Zahl aus einem **nicht existierenden Zwischenstand** in einen Commit-Text geriet; beide sind jetzt korrigiert und als korrigiert benannt. **Die Runde hat ausserdem die von mir als Luecke benannte Testnaht bestaetigt, statt sie zu uebernehmen:** sie hat den Aufruf von `clear_stale_auto_tone` im Load-Pfad entfernt und der Test blieb **gruen** — die Luecke ist real, der Test ist fuer die Produktion vakuos, und genau das ist jetzt in `AUTO-TONE-ENDPOINT-MIXED-7` Abnahmepunkt (3). **Ebenfalls bestaetigt:** der Waechter `g16_the_state_repair_overwrites_a_later_manual_value_known_defect` ist ehrlich (rot, sobald das Defekt behoben ist), und die Abspaltung von `AUTO-TONE-ENDPOINT-MIXED-7` **versteckt keinen Befund**, sondern benennt ihn ausdruecklich — sie wurde als vertretbar beurteilt. **Was daraus folgt und NICHT behauptet wird:** Klausel (2) ist fuer den G-16-Endpunkt weiterhin **nicht** erfuellt (5 von 6 nach einem Endpunkt, 4 von 6 nach zweien — auch diese Reihenfolge ist in keinem Test gepinnt), die vollstaendige Loesung ist der neue Task, und der 5-von-6-Zustand ist laut, selbstheilend **und kostet** einen Handwert. Die drei Gates des Korrekturlaufs wurden von der Runde selbst gefahren: fmt 0, clippy -D warnings 0, GUI-lib 933/0/5, lumina-stages 8, sizes/plan 0, 0 Snapshots. **Der Task bleibt dennoch OFFEN** — nicht wegen eines Befunds, sondern weil die Verifikation ausdruecklich unter dem Vorbehalt abgibt, dass der Build-Agent die Abnahme erst nach eigener Pruefung spricht (DoD.md §7 Checkliste punkt 1-7 ist Bewertungssache, nicht Testzahl). Gates des Korrekturlaufs: fmt 0, clippy -D warnings 0, GUI-lib **933/0/5** (932 + 1 neuer Canary, nachgezaehlt, nicht angenommen), sizes/plan 0, 0 Snapshots geaendert, Mutationsdiff zurueckgenommen.
    **ABNAHME durch den Build-Agent (DoD.md §7, 2026-10-02, nach der dritten Verifikationsrunde BESTANDEN).** Die Verifikation gibt ausdrücklich ab, dass §7 Bewertungssache des Build-Agenten ist — hiermit beantwortet, jeder Punkt mit Beleg statt mit Ableitung:

    1. **End-to-End-Kette (Edit→Commit→Datei→Reload).** Über die **echte Binary**, nicht über einen Funktionsaufruf: `cargo test -p lumina-cli --test auto_tone_e2e` → **7 passed / 0 failed**, darin `process_auto_tone_persists_the_full_six_set_and_renders_the_golden` (Sechsersatz **und** gerendertes Byte-Golden, nicht nur Sidecar-JSON), `regenerate_is_skipped_as_fresh_after_process_auto_tone` (Reload leg: Freshness überlebt den kollektiven Lauf), `render_and_export_are_byte_identical_without_auto_tone` (Klausel 7) und `the_exit_codes_are_unchanged`. Die GUI-Kette separat: `auto_tone_commits_and_reloads` (`tests/basic_commit.rs`) fährt Datei + Reload in einem frischen `LuminaApp`.
    2. **Zeitbasierter Pfad.** `auto_tone` committet **synchron** (`commit_pending_slider_save([0, 0])` direkt nach `mark_recipe_dirty`), der 150-ms-Idle-Debounce bleibt also unbewaffnet: `auto_tone_commits_and_reloads` prüft `app.pending_slider_commit == None` **und** dass die Sidecar-Datei die Werte schon ohne manuellen Debounce-Drive trägt. Für den Endpunkt gilt dieselbe Commit-Disziplin (`mark_recipe_dirty` + `commit_pending_slider_save`), getrieben von `g16_apply_auto_endpoint_sets_only_its_field` über den Persistenz-Assert auf `lumina_sidecar::load_sidecar`.
    3. **Geprüfte Klassenmitglieder, vollständig.** Die sechs Regler `exposure/contrast/whites/blacks/highlights/shadows` (`AUTO_TONE_ADJUSTMENT_KEYS`, `auto_tone.rs`), die sechs Spiegel `auto_exposure…auto_shadows` (`mirrored_values`), `analysis_fingerprint` (`algorithm`, `version`, `input_fingerprint`), `enable_auto_tone`, `target_luminance`, die beiden Endpunkte `AutoEndpoint::{White,Black}` (`g16_apply_auto_endpoint_sets_only_its_field` + `g16_apply_auto_black_endpoint_sets_only_blacks`), der Reuse-Entscheid (`PersistedAutoTone::{ReuseIfComplete,AlwaysRecompute}`) und die Freshness (`auto_tone_is_fresh`). **Nachgezählt**, nicht behauptet: 3 Vertragstests in `auto_tone_contract.rs`, 7 in `g16_auto_endpoint.rs`, 7 in `auto_tone_reuse.rs`, 38 in `auto_tone_process.rs`, 7 e2e.
    4. **Log-Level je neuer User-Aktion — hier war eine LÜCKE, die ich selbst gefunden und geschlossen habe.** `grep` über den Testbaum fand **15** Referenzen auf `apply_auto_endpoint` und **keine einzige** in einer Log-Assertion: das Level war nirgends belegt, obwohl `info!` (lib.rs:3735) die Zeile schreibt und `auto_tone` über `instrument_gui_action!` läuft. `DoD.md` §7.4 verlangt den Beleg, also ist er jetzt **neu**: `g16_auto_tone_actions_log_their_level_and_the_key` pinnt genau **eine** `INFO`-Zeile, die den **Schlüsselnamen** nennt (das ist der Support-Fall: welcher Endpunkt wurde benutzt). **Nicht-Vakuinität gemessen:** `info!` → `debug!` mutiert macht genau diesen Test rot (`got DEBUG: GUI interaction: apply_auto_endpoint whites=0.2396…`), Kontrolllauf 15/15 grün. Das ist die **fünfte** Mutation dieser Runde; die vier vorherigen (M-1, M-2 und die beiden Zahlenbefunde) sind getrennt dokumentiert.
    5. **Spez→Test-Mapping.** Klausel (1) eine Quelle der Wahrheit → `the_gui_writes_exactly_what_the_shared_writer_writes` (Differenz, 6 Regler + 6 Spiegel + Fingerprint-**Wert**, nicht nur -Algorithmus), `a_gui_auto_tone_recipe_is_fresh_for_the_shared_regenerate_predicate`, `the_tone_algorithm_literal_is_defined_once_and_never_hand_copied` (Struktur, vier contract-schreibende Crates). Klausel (2) alles-oder-nichts → `a_complete_state_with_a_matching_fingerprint_is_reused_verbatim`, `one_missing_mirror_forces_a_full_recompute_of_all_six`, `the_historic_two_slider_artifact_is_completed_to_the_full_set`, `reuse_reads_mirrors_only_so_a_user_value_cannot_become_an_auto_value`. Klausel (3) explizite CLI-Angaben zuletzt → `explicit_cli_values_win_last_for_all_six_and_keep_the_auto_mirror`, `each_explicit_flag_overrides_only_its_own_slider`, `the_six_auto_values_stay_reconstructible_after_an_override`. Klausel (4)/(5) Spiegel=auto, `adjustments`=effektiv, präsenzbasierte Freshness → `the_preset_wins_where_it_speaks_and_the_auto_value_survives_elsewhere`, `regenerate_is_skipped_as_fresh_after_process_auto_tone`. Klausel (6) Matching → `match_total_exposure_adds_onto_the_effective_exposure_only`, `match_total_exposure_uses_the_explicit_exposure_as_its_base`. Klausel (7) ohne `--auto-tone` alles unverändert → `without_auto_tone_no_auto_feature_is_written_and_the_render_is_unchanged`, `render_and_export_are_byte_identical_without_auto_tone`. Klausel (9) JSON-Round-Trip-Grenze → `auto_tone_float.rs`. **Für Klausel (2) am G-16-**Endpunkt** gilt die Karte nicht als erfüllt**, siehe den abgespaltenen Task.
    6. **Gates, Kommandos + Ergebnis.** `cargo fmt --all -- --check` → exit 0. `cargo clippy -p lumina-gui --all-targets -- -D warnings` → exit 0. `cargo test -p lumina-gui --lib -- --test-threads=1` → **934 passed / 0 failed / 5 ignored** (933 + 1 neuer Log-Test, nachgezaehlt). `cargo test -p lumina-stages` → **8 passed**. `cargo test -p lumina-cli --test auto_tone_e2e` → **7 passed**. `sh scripts/check_file_sizes.sh` → exit 0. `sh scripts/check_plan_format.sh` → exit 0. `git status --porcelain -- crates/lumina-gui/tests/snapshots` → leer. **Nicht geprüft und nicht behauptet:** GPU-/wgpu-Parität, Paint-Schritt, VRAM-Pixel — benanntes Hardware-Gate, auf dieser headless Maschine nicht messbar (`Agents.headless.md` §1).
    7. **Dateigrößen-Regel.** `check_file_sizes.sh` exit 0. `crates/lumina-gui/src/lib.rs` 12561 → **12572** (+11), **bewusst** angehoben, Ladungsinhalt im Ledger aufgeschlüsselt — **mit einer echten Extraktion daneben**: `src/tests/g16_auto_endpoint.rs` (439 Z.) nimmt die Auto-Tone-Zustandstests aus `g16_shortcuts.rs` (509 → 194 Z. im Arbeitsstand, **314 → 194** im Git, nachgezaehlt), beide unter der 500er-Schwelle. **Keine Kompensations-Löschung:** 9 → 15 Testfunktionen in den G-16-Dateien über die Commits hinweg, **kein Test verschwunden**, keine gelöschte Testdatei, kein `#[ignore]`, keine `should_panic`; der +11-Zuwachs in lib.rs ist aufgeschlüsselt und **zweimal** versucht worden ohne Verlust einer Aussage zu kürzen. **Clippy fand übrigens zwei echte Fehler, die ich übersehen hatte:** zwei beim Extraktions-Skript abgeschnittene Doc-Fragmente ("empty line after doc comment") — nicht mein Testdesign, sondern meine Werkzeugkette.




### PRIO: mittel

- [ ] **[PRIO: mittel] DOWNScaler-UNIFY-71 (Eigentümerentscheid 2026-10-16; Entscheid `DOWNSCALE-EIN-PFAD`)** Die drei Downscale-Implementierungen (`lumina-core::downscale_bilinear` f64/breitenbasiert, `downscale` f32/lange Kante, `lumina-gui::filmstrip::downscale_rgba` Nearest-Neighbour) auf **einen** Pfad konsolidieren, GPU-fähig gedacht, **Qualität vor Geschwindigkeit** (Entscheid `DOWNSCALE-EIN-PFAD`). **Abnahme:** genau eine Implementierung; Aufrufer umgestellt; für Aufrufer, deren Ergebnis sich nicht ändern darf, ein **Byte-Beweis** (vorher/nachher gleich bzw. bewusst neu eingefroren mit Begründung); headless-Tests grün. **Abgrenzung:** kein Bildqualitäts-Regress ohne benannte Entscheidung; Geschwindigkeitsoptimierung ist **nicht** Teil dieses Tasks.

- [ ] **[PRIO: mittel] GPU-TASK-HEADLESS-SPLIT-74 (Eigentümerweisung 2026-10-16; Regel `HEADLESS-GPU-TASK-SPLIT`)** Rückwirkende Anwendung der Regel `HEADLESS-GPU-TASK-SPLIT` (`Agents.headless.md` §1 Punkt 6) auf die **bestehenden** GPU-blockierten Tasks (`GPU-RENDER-MASK-19`, `GPU-RENDER-NONDETERMINISM-69-HW`, `GPU-MASKPLANE-CLEAR-REMEASURE`, `MCP-PARITY-C`, `LRPAR-G09-CULL-IMPL-25`, `LRPAR-G14-DENOISE-IMPL-20`, `R5-DUST-23-FOLLOWUP`, `R5-BRUSH-24`, `R5-MASKVIS-25`, `F-103-N6`, `F-103-N6-GUI-COVERAGE-27`, `R2-GUIMOD-04b/04c`; vollständig abgeleitet aus der Headless-Tabelle oben in Tabellenreihenfolge): je Task den headless baubaren Teil umsetzen und den adapter-/GUI-E2E-Rest als **eigenen** Eintrag führen. **Abnahme:** für jeden betroffenen Task liegt ein belegter headless Teil und ein eigener Rest-Eintrag mit verbindlichem Kommando vor. **Abgrenzung:** kein Task wird fallengelassen; nur die Rest-Grenze wird sichtbar gemacht; `GPU-PARITY-HW-28` ist bewusst nicht enthalten — der Task hat weder eine offene `- [ ]`-Zeile noch einen headless-baubaren Teil (reiner Adapter-Nachweis).

### PRIO: niedrig






- [ ] **[PRIO: niedrig] MCP-LENSFUN-72 (Architektur-Review 2026-10-16)** Der MCP-Renderpfad baut nie einen Lensfun-Corrector (`crates/lumina-mcp/src/util.rs`, `lensfun: None`) und liefert damit bewusst andere Ergebnisse als CLI/GUI. Verdrahten oder als dauerhafte Grenze härten. **Abnahme:** MCP- und CLI-Ergebnis mit aktivem Profil byte-identisch **oder** eine benannte, dokumentierte Grenze mit Beleg. **Abgrenzung:** keine neue Lensfun-Funktionalität, nur der Anschluss.

- [ ] **[PRIO: niedrig] FRAME-INVARIANT-SEAL-77 (Befund 2026-10-17, aus der Q5-Abnahme zu `ImageFrame::pixels`)** Q5 hat die **Pufferseite** der Frame-Invariante versiegelt (`pixels` privat, `pixels()`/`pixels_mut()`/`into_pixels()`), aber **nicht** die Dimensionsseite: `width`/`height` bleiben `pub` und setzbar, ein Aufrufer kann `len == width*height*4` nach der Konstruktion weiterhin brechen. Der Ist-Zustand ist dokumentiert (`crates/lumina-core/src/lib.rs`, Typ-Doc) und grün — dies ist **keine** Defektmeldung, sondern die offene Frage der **vollständigen** Versiegelung. **Belegte Nebenfolge:** das Löschen der Guards `linear_from_rgba`/`linear_from_frame` lässt die Tests grün (Mutationsexperiment in der Q5-Verifikation, byte-identisch restauriert, SHA-256 gleich) — die Guards pinnen „ungültiger Frame wird abgelehnt", nicht „der Guard feuert"; ihr echter Zusatznutzen ist der Short-Circuit vor der `width*height*3`-Allokation (steht jetzt am Guard). **Abnahme:** entweder `width`/`height` kapseln (Accessoren, Konsumenten außerhalb `lumina-core` migriert, die Relation danach nur über `new` herstellbar) — **oder** eine bewusste, benannte Entscheidung, sie öffentlich zu lassen, mit dann *nachgewiesener* Mutation (ein Guard-Löschen macht einen Test rot). **Abgrenzung:** keine Render-/Rezept-/Schema-Änderung, kein zweiter Konstruktor. **Offene Wahl (Eigentümer):** vollständige Kapselung (Folgearbeit: alle `frame.width`/`frame.height`-Lesestellen außerhalb `lumina-core` migrieren) oder öffentlich lassen und die Guards als benannte Doppelabsicherung führen (heutiger, dokumentierter Stand).

- [ ] **[PRIO: niedrig] CORE-FS-GATE-IMPORT-FORM-78 (Befund 2026-10-17, aus der Abnahme von `CORE-PROD-FS-FREE-GATE-76`)** Das neue Gate `scripts/check_core_prod_fs_free.sh` findet `std::fs` und die einzeilige Klammer-Form `use std::{fs, io}`. Zwei Formen entgehen ihm, beide von der Verifikation belegt: **(F4, low)** ein ueber mehrere Zeilen umbrochener Import-Baum (`use std::{` … `fs,` … `};` — rustfmt bricht erst bei langen Listen; die Form wird im Repo heute nirgends verwendet) und **(F5, low)** eine Produktionszeile, die das Muster nur **erwaehnt** (Kommentar), schlaegt als Produktions-Treffer an. Beides ist dem textuellen Scan inhärent. **Abnahme:** entweder die Erkennung nachziehen (Import-Baum ueber Zeilengrenzen, Kommentar-Zeilen ausschliessen) **oder** die beiden Formen im Skriptkopf benennen und F5 als bewusst lautes Falsch-Positiv fuehren. **Abgrenzung:** kein Parsing-Umbau, keine neue Abhaengigkeit, kein cargo-Gate. **Offene Wahl (Eigentuemer):** die zwei Formen benennen (kleinster Schritt, heutiger Stand) oder die Erkennung erweitern, dann mit Mutationsbeweis in beide Richtungen.

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

  **11. Vollständigkeitsnachweis:** alle `ALL_GUI_ACTIONS` (aktuell 110) gegen eine gezeichnete, klickbare Fläche und einen PASS/FAIL-Test mappen; jede neue `GuiAction`/Enum-Variante erzwingt den Audit. Für visuelle Änderungen `egui_kittest`-Golden/PSNR/Histogram plus Vision-Review; für unveränderte Modelle genügt der jeweilige headless Test nicht als manueller GUI-Erfolg.

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

- [ ] **[PRIO: niedrig] MCP-PARITY-C (User-Order 2026-09-26; SOLL zuerst, Release 2.0, GUI-frei, nach A und B)** **Stand 2026-10-14 — Merge-Teil geliefert und verifiziert:** `lumina merge-hdr` → `lumina_merge_hdr`, `lumina merge-pano` → `lumina_merge_pano` (gemeinsamer Decode-/Exposure-Adapter `189b7d9`, Tools `d2dc045`, Byte-Paritäts-Suite `7255c85`). MCP und CLI erzeugen für dieselbe Eingabemenge byte-identische Digest-/Checksum-, DNG- und Sidecar-Bytes und identische Fehlerklassen (T11–T20, `crates/lumina-cli/tests/merge_mcp_parity*.rs`). **Stand 2026-10-14 — RAW-Pfad (Einzelpaar) bewiesen:** der `#[ignore]`-Gate `crates/lumina-cli/tests/merge_mcp_parity_gate.rs` (T21) fährt ohne Serien-Fixture die ganze RAW→DNG-Strecke auf beiden Transporten — LibRaw-Dekodierung der committeten CR3-Fixtures, EXIF-Exposure-Auflösung, `decode_context` = libraw/`libraw_decode_version()`/Orientation aus den RAW-Metadaten, DNG-Writer, byte-identische Digest-/Checksum-/DNG-/Sidecar-Ausgaben CLI≡MCP, Cache-Hit, DNG-Re-Import via `lumina_raw::decode_bytes`; bei untauglicher Paarung (gemischte Geometrie) scheitern beide Frontends laut-identisch (`merge unsupported: invalid merge input: hdr alignment needs equal dimensions, got 6032x4024 vs 4024x6032`, MCP `MergeUnsupported`/-32012). Aufruf: `cargo test -p lumina-cli --test merge_mcp_parity_gate -- --ignored` (~5 min); gepinnte Regressionsanker (HDR/Pano digest+checksum) sowie alle offenen Restpunkte in `feature/quality/hdr-pano-raw-dng-status-2026-10-14.md`. Bewiesen ist der Pfad, nicht die Serie — eine echte Belichtungsreihe fehlt weiterhin (Punkt (a)). **Rest der Lücke: nur noch die Mehrbild-Pipeline `lumina matrix`** als eigener Sub-Slice (§12 des SOLL) — `matrix` bringt `expected_route` (`testdata/matrix/recipe-set.v1.json`) und ein eigenes Gate. **Stand 2026-10-15 — Matrix-Teil geliefert und verifiziert (T15–T25; T26 geschrieben, nicht ausgeführt):** `lumina matrix` → `lumina_matrix` (geteiler Kern `lumina-stages::matrix_run::run`/... Der Parent-Checkbox bleibt bis zur vollständigen Serien-Füllung (echte Belichtungsreihe) offen.

- [ ] **[PRIO: niedrig] TEST-AUDIT-36 (fortlaufend, User-Regel 2026-09-26)** Redundanz-Audit der Testsuite: **höchstens einmal pro Woche** einen Build-Agenten starten, der prüft, ob es unnötige Tests gibt. Der Build-Agent **berichtet nur** und entfernt nichts eigenmächtig; jede Löschung läuft über SOLL → Implementierungs-Agent → unabhängige Verifikation. **Prüfkatalog:** (a) Tests, die eine Konstante auf sich selbst prüfen; (b) doppelte Abdeckung derselben Aussage in mehreren Dateien; (c) Tests, die eine Implementierungsentscheidung statt des Verhaltens pinnen; (d) Tests, die bei Wegfall der Logik nichts verlieren würden; (e) Tests ohne Fehlsignal — die grün bleiben, egal ob die Aussage stimmt; (f) **Laufzeit-Ballast**, der die Suite verlangsamt, ohne Aussage zu tragen (der 3:11-GUI-Shard ist das aktuelle Beispiel); (g) Assertions, die nach einem Refactor nur noch den Refactor beschreiben. **Abnahme:** ein priorisierter Befundbericht mit Test-Name, Datei und der konkret entfallenden Aussage; keine Löschung ohne Begründung; keine Reduzierung der Netto-Aussagenabdeckung — eine beim Audit entdeckte **Lücke** (Feature, Klausel, Fehlerpfad ohne Anker) wird als **neue offene Aufgabe** angelegt, nicht durch Streichen eines Nachbarn ersetzt. Grundlage: Policy in `Agents.md` §„Testabdeckungs-Politik". **Abgrenzung:** keine Produkt- oder Schemaänderung; der Audit verändert keine Abnahmegate und ersetzt keine DoD-Prüfung. **PARALLELITAeTS-BEFUND 2026-10-12 (F-P1, aus der Runde-3-Verifikation von `JSON-FLOAT-ROUNDTRIP`):** Audit-Runden, die **Mutationen** fahren, duerfen nicht parallel zu einem anderen Baum-veraendernden Lauf laufen — gemessen stand waehrend einer Verifikation fuer ~1 Minute eine uncommittete Logik-Mutation in `crates/lumina-core/src/lib.rs` im Tree. Kein Dauerschaden (Restore byte-identisch), aber die Messung des Parallel-Laufs ist fuer dieses Fenster nicht belastbar. Konkret fuer diesen Task: der Audit bekommt einen **exklusiven** Baum, wenn er mutiert; reine Lese-Runden duerfen parallel laufen.

- [ ] **[PRIO: niedrig] CI-WATCH-1 (fortlaufend)** Nach jedem Push: CI-Runs prüfen (`gh run watch` / `gh run list --branch main`), Ergebnis im Tagesstand vermerken. Bei Rot: als Next-Task in `Agents.todo.md` dokumentieren, NICHT still umsetzen (User-Vorgabe). Abnahme: jeder Push hat ein geprüftes CI-Verdict. **TAGESSTAND 2026-10-15:** Matrix-Sub-Slice abgeschlossen — alle Pushes grün; kein Rot, keine Task angelegt. **TAGESSTAND 2026-10-16:** GUI-LIB-SPLIT-65 Slice 6–10 (lib.rs 11935 → 11459; neue Module `library_smart`, `generative_canvas`, `histogram_clip`, `file_browser_entry`, `tone_curve_regions`), der Abschluss von `PIPELINE-CROP-EARLY-9` (als nicht machbar in `Verworfen` eingetragen) und eine Kommentar-Korrektur gepusht — alle Runs grün (je Commit zwei Läufe: check/lint + test), kein Rot, keine Task angelegt.

- [ ] **[PRIO: niedrig] LRPAR-G14-DENOISE-IMPL-20 (Release: 2.0)** KI-Denoise-Implementierung nach Entscheid `feature/decisions/LRPAR-G14-DENOISE-20.md` (F-078-Fixture-Entscheid ✓, Schema ✓, Pipeline-Stufe + Persistenz + GPU-Refusal ✓, ONNX-Backend ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only, F1-Input-Spec-v2 inkl. Core-Blend-/Assembly-Identität ✓, F3-zdata-Feldvalidierung ✓; offen: echte Gewichte). Der Parent-Checkbox bleibt bis zum F-078-Gate (Gewichts-Lizenz, Provenienz und Hash-Pin) offen; keine stillen Stub-/Produktions-Fallbacks. Abnahme: CLI + GUI-headless + Golden/PSNR, kein stiller Fallback.

- [ ] **[PRIO: niedrig] LRPAR-G09-CULL-IMPL-25 (Release: 2.5)** KI-Culling-Implementierung nach Entscheid `feature/decisions/LRPAR-G09-CULL-25.md` (Schema ✓, Heuristik `lumina-cull` ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only; **Follow-up 2026-09-24:** CLI/GUI erzwingen Live-Quellenidentität und verweigern Sidecar-Writes bei Hash-/Längenkonflikt, inklusive `--force`/Batch-Isolation; offen: optional ONNX Stufe 2 und kittest). Abnahme: CLI-Exit-Codes + GUI-headless + kittest, Vorschlag schreibt nie Rating/Flag/Label. **Parent bleibt wegen Stage 2/kittest offen.**

- [ ] **[PRIO: niedrig] GUI-LIB-SPLIT-65 (Eigentümerentscheid 2026-10-15; reine Struktur, kein Release-Blocker)** `crates/lumina-gui/src/lib.rs` ist mit **12 571 Zeilen** die größte Datei des Repos (1,6× die nächstgrößte, `lumina-cli/src/main.rs` 7 874) und über den Ratchet-Eintrag **12 573 eingefroren** — null Spielraum. Die Folge ist gemessen: Änderungen an dieser Datei wurden überwiegend mit einer **bewussten Anhebung** bezahlt (Eintrag zuletzt am 2026-10-15 von 12 572 auf 12 573), nicht mit Extraktion. **Ziel:** kohäsionsgetriebene Extraktion, Slice für Slice, bis der Eintrag entfällt. **Abnahme je Slice:** (1) eigener Commit, `cargo test -p lumina-gui --lib` **vor und nach dem Slice mit identischer Testzahl** — kein Test entfernt, kein `#[ignore]` ergänzt; (2) `sh scripts/check_file_sizes.sh` grün und der Baseline-Eintrag **gesenkt**, nicht bloß unverändert — `git diff scripts/file_size_baseline.txt` nennt die neue Zahl; (3) keine neue Anhebung irgendwo; (4) die Extraktion folgt **Modulgrenzen**, kein willkürliches Splitten (`Agents.md` §„Änderungsregeln"); (5) keine kompensierende Löschung von Kommentar, Doku oder Tests (Diff-Prüfung); (6) `cargo fmt --all -- --check` und `cargo clippy -p lumina-gui --all-targets -- -D warnings` clean. **Abgrenzung:** **kein** Verhaltenswechsel, **keine** Schema-, Rezept- oder Pipelineänderung, **kein** GUI-Verhalten — der Diff darf außer Modul- und `use`-Zeilen nichts Semantisches enthalten. **Einordnung:** PRIO niedrig; keine Gate-Gefährdung, gibt keinen anderen Task frei — er läuft nach den drei üblichen Kriterien nur, wenn kein Gate-Task ansteht, und darf `GPU-RENDER-MASK-19` und den GPU-E2E-Pfad **nicht** verdrängen. **STAND 2026-10-15 — Slice 1 geliefert und unabhängig verifiziert (BESTANDEN):** das F-103-N5 Export-Modul (`resolve_export_target`, `paths_resolve_equal_symmetric`, `export_to`, `suggested_export_name`, `draw_export_panel`) ist aus `lib.rs` nach `crates/lumina-gui/src/export.rs` (306 Z.) extrahiert; `lib.rs` von 12 572 auf 12 573 Zeichen gesenkt.
