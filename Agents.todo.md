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
`R5-MASKVIS-25`, `GPU-MASKPLANE-CLEAR-PERF-51` und die kittest-Goldens.

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
| 1.0 | FIXTURE-ENV-1 | alle G | Env-gegateter Fixture-Test panickt unter `--ignored` |
| 1.0 | GPU-RENDER-MASK-19 | alle G | Masken-Pixelpass |
| 1.0 | R5-DUST-23-FOLLOWUP | G-04 | Spot-Heal-Followup |
| 1.0 | R5-BRUSH-24 | G-03 | Pinsel-Masken |
| 1.0 | R5-MASKVIS-25 | G-03 | Masken-Overlay |
| 1.0 | GPU-MASKPLANE-CLEAR-PERF-51 | alle G | GPU-Kosten des Plane-Clears gemessen |
| 1.0 | KITT-WAIT-PIN-57 | alle G | geteilter Wait maschinell gepinnt |
| 1.0 | GOLDEN-STALE-55 | alle G | 127px-Stale-Indikator in develop_section_rating geklaert |
| fortlaufend | CI-WATCH-1 | alle G | CI-Beobachtung |
| fortlaufend | TEST-AUDIT-36 | alle G | Test-Redundanz-Audit (1×/Woche) |
| 2.0 | LRPAR-G14-DENOISE-IMPL-20 | G-14 | KI-Denoise-Impl |
| 2.5 | LRPAR-G09-CULL-IMPL-25 | G-09 | KI-Culling-Impl |
| 1.5 | JSON-FLOAT-ROUNDTRIP | offen — bei der Task | f64-Roundtrip |
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



- [ ] **[PRIO: mittel] GPU-RENDER-MASK-19 (Release 1.0; aus dem Releaseplan-Audit als EINZIGE echte Lücke belegt, 2026-10-02)** Der **GPU-Masken-Pixelpass** fehlt: lokale Masken modulieren auf der GPU keine Pixel. **Gemessen (Audit 2026-10-02, eigene Nachmessung):** `crates/lumina-gpu/src` hat `combine_mask_planes` (`lib.rs:615`) und `upload_mask_plane` (`lib.rs:2782`) — Upload/Clear/Combine, **nicht** den Pass; `grep -rniE 'local_adjust|mask_layer|apply_mask' crates/lumina-gpu/src` → **0 Treffer**; `upload_mask_plane_roundtrip_is_byte_exact` (`tests/stages.rs:261`) belegt nur den Upload-Roundtrip. `GPU-PARITY-MASKGATE-1` ist ein Present-Gate, nicht der Pass; `GPU-MASKPLANE-BRUSH-50`/`-CLEAR-PERF-51` sind Brush-Upload bzw. Clear-Performance; `MASK-LOCAL-P0`…`P1.2d` bauen das Rezept-Datenmodell. Die CPU-Referenz moduliert bereits (`apply_local_adjustments`), die GPU-Stufe fehlt (dokumentierte F-042-Grenze, `pipeline.md:1885-1890`). **Abnahme:** (1) eine WGSL-Stufe, die die lokal ausgewertete Maske (Alpha) pixelgenau auf den Composite anwendet — keine CPU-Route, keine stille Ausnahme (volle GPU-Parität, User-Entscheid 2026-09-12); (2) CPU-Oracle-Parität byte-identisch wo 0, sonst maxAbsDiff ≤ 1 (Muster `parity.rs`); (3) E2E am Adapter: `cargo test -p lumina-gpu --features gpu-adapter-tests` **plus** die betroffenen GUI-GPU-Tests (Regel `GPU-E2E`); (4) SOLL in `feature/architecture/pipeline.md` §GPU und `feature/product/ai-masks.md` von „dokumentierte F-042-Grenze" auf „GPU-Stufe vorhanden" umstellen; (5) der Masken-Pass wird im Render-Key/der Route sichtbar (kein stiller CPU-Fallback). **Abgrenzung:** der **Pass** selbst; Upload/Clear/Brush bleiben bei ihren Tasks. Parent-Gate-Referenzen: `pipeline.md:2261`, `ai-masks.md:349`.
- [ ] **[PRIO: niedrig] GPU-MASKPLANE-CLEAR-REMEASURE (Release 1.0; User-Weisung 2026-10-02)** Die F-074-N10-Messung des Masken-Plane-Clears (`37,4 µs / 722 µs / 15,21 ms` @160×120/1280×853/6000×4000, `crates/lumina-gpu/benches/mask_plane_clear.rs`) ist **unter Fremdlast** entstanden — ein Spielprozess (`TransportFever3`, ~140–270 % CPU, Load 5–18) **und** parallel laufende andere GPU-lastige Vorgänge. Sie ist als **nach oben verzerrt** benannt, trägt aber die Entscheidung `GPU-CLEAR-TRANSITION` mit. **Aufgabe für eine ruhige Session:** denselben Bench auf einer **quieszenten** Maschine (kein Spielprozess, keine parallelen GPU-Jobs) neu fahren, Median/p95 für die drei Auflösungen plus Ein-Aufruf-Kontrolle dokumentieren, Lastangabe beilegen, und `feature/quality/performance-benchmarks.md` F-074-N10 auf die sauberen Zahlen aktualisieren (oder die Abweichung als Messunsicherheit festhalten). **Abnahme:** neue Zahl mit Kommando + Lastangabe; die Entscheidung `GPU-CLEAR-TRANSITION` wird gegen die saubere Zahl **erneut geprüft** — bleibt sie in ms-Größe, gilt sie unverändert, sonst wird sie revidiert. **Abgrenzung:** reine Nachmessung, keine Codeänderung.
- [ ] **[PRIO: mittel] GPU-MASKPLANE-CLEAR-PERF-51 (Release 1.0; offene Messung aus der Verifikation zu `GPU-PARITY-MASKGATE-1`, 2026-09-27)** **Die GPU-Seite des Plane-Clears ist ungemessen: der Sync schreibt bei jedem Render ohne Maskenebene eine Null-Plane, und die Kosten des `write_texture` auf der Queue sind nicht gemessen.** **Gemessen ist nur die Host-Seite** (Release, 100 Iterationen, ohne GPU): 0,004 ms pro Sync, auflösungsunabhängig, weil `vec![0u16; …]` über `alloc_zeroed` geht und die Seiten erst beim `write_texture` faultiert. **Gerade das macht die Lücke relevant:** die hostseitige Zahl ist klein und täuscht über die Queue-Kosten hinweg. **Bandrechnung nachgerechnet:** 1280×853 → 409 Zeilen/Band, 3 Bänder, 2 183 680 B; 160×120 → 1 Band; 6000×4000 → 87 Zeilen/Band, 46 Bänder, 48 000 000 B. **Abnahme:** (1) Benchmark **am Adapter** nach der Methodik von `feature/quality/performance-benchmarks.md` (F-074), gegen den committeten Baseline-Store, Modus `report`; (2) die Zahl für 160×120, 1280×853 und 6000×4000; (3) falls der Clear spürbar wiegt, eine Entscheidung **mit** Messung — z. B. Clear nur beim Übergang „ Ebene resident → keine Ebene" statt bei jedem Render — und die dann mutationsbewiesen wirkt. **Abgrenzung:** Messung, keine Leistungsbehauptung; ein Budget wird nicht angepasst, bevor die Zahl existiert. **STAND 2026-10-02 — die Messung ist erbracht (F-074-N10, Metal M5 Pro):** Bench `crates/lumina-gpu/benches/mask_plane_clear.rs`; banded Clear **37 µs / 722 µs / 15,21 ms** (160×120 / 1280×853 / 6000×4000, 100 Samples, Median), volumengetrieben (Ein-Aufruf-Kontrolle ≈ banded), ~13× ein Vollrender @24 MP; E2E `cargo test -p lumina-gpu --features gpu-adapter-tests` **alle grün** (`parity.rs` 27/27). **Der Clear wiegt spürbar → Entscheidung (Eigentümer, 2026-10-02): nur beim Übergang clearen (`GPU-CLEAR-TRANSITION`).** **Umsetzung (offen, in `lumina-gui/src/present_mask_plane.rs`):** den Null-Plane-Write nur ausführen, wenn zuvor eine Nicht-Null-Plane resident war — minimale, testbare Markierung (ein `bool`/Generation), konsistent mit `live_brush_plane_stale`/`KeepLiveBrush`/Pool-LRU; `clear_texture` ist **kein** Drop-in (Gerät mit `Features::empty()`, würde paniken). **Abnahme der Umsetzung:** (a) Mutation beweist, dass ohne die Übergangsbedingung wieder bei jedem Render gecleart wird (Test rot); (b) ein Render **ohne** Maske nach einem Render **mit** Maske cleart genau einmal; (c) ein Render **ohne** Maske ohne vorherige Nicht-Null-Plane cleart **nicht**; (d) keine Regression in den `present_mask_plane`-Tests; GPU-E2E-Regel (`--features gpu-adapter-tests`) + betroffene GUI-GPU-Tests. **Benannte Grenze:** die Absolutzahlen sind unter Fremdlast gemessen (Spielprozess, Load 5–18) und damit nach oben verzerrt. **Nachmessung in ruhiger Session:** `GPU-MASKPLANE-CLEAR-REMEASURE` (eigener Task).
- [ ] **[PRIO: mittel] KITT-WAIT-PIN-57 (Release 1.0; aus der Verifikations-Runde 2 zu `KITT-SETTLE-UNIFY-53`, 2026-09-28)** **Die zentrale Invariante des Tasks ist ungepinnt: kein Test stellt fest, dass eine Golden-Suite den geteilten Wait benutzt. Ein Zurueckbau von `kittest_snapshots.rs:342` auf den stillen 400er-Loop macht keinen Test rot.** **Gemessen in Runde 2:** der alte Loop wortgleich wiederhergestellt, `kittest_crop_overlay` **2 passed / 0 failed**, Golden byte-identisch auf dem gesunden Pfad — und `git status --porcelain -- crates/lumina-gui/tests/snapshots` leer. **Gepinnt sind bisher nur die beiden Exits der Schleife** (`kittest_decode_bound`) **und der Scan-Wait** (`folder_scan_settle`). **Warum das der importanteste offene Punkt des Tasks ist:** `KITT-SETTLE-UNIFY-53` hat die stille Schleife beseitigt und die Diagnose eingebaut — aber nichts haelt fest, dass die **naechste** handgeschriebene Warte-Schleife nicht wieder in derselben Datei entsteht. Der Defekt kehrt dann lautlos zurueck, mit derselben Fehlerklasse. **Erfahrung aus `KITT-SCAN-PREMISSE-58` (2026-09-29; Bauform **zwei Wartephasen** per Eigentümer-Entscheid):** der gemeinsame Decode-Wait **lässt sich nicht um eine Listing-Bedingung erweitern**. `pump_until_ready` armiert intern `Some(is_settled)`, und sobald `entries().len() == 1` in sein `Ready` aufgenommen wird, feuert dieser Exit **genau im Zielfenster** des Tests und **panickt** statt zu warten (gemessen: „settle of …/photo.png never finished … cause: the decode settled and reported its outcome"). **Konsequenz für diesen Task:** ein Test, der auf ein **Listing** wartet, braucht einen eigenen `pump(..., None)` — **nicht** ein erweitertes `pump_until_ready`. `is_settled` darf laut eigenem Modul-Doc **keine** Scan-Bedingung entscheiden und blieb deshalb unverändert. **Und der Grund für zwei Wartephasen statt einer:** ein **fehlschlagender Decode** soll laut und schnell gemeldet werden — genau das war der Zweck von `KITT-SETTLE-UNIFY-53`. Bei einem einzigen Wartevorgang käme ein Decode-Fehler als **Deadline-Timeout** statt als Fehlerursache, und ein klarer Befund würde zum Verwirrungsfall. **Abnahme:** (1) ein Test **liest die Quellen** der Golden-Suiten und wird **rot**, sobald dort eine handgeschriebene Warteschleife entsteht (`for _ in 0..N { … step … }`, `while …_pending() { … }`), also genau die Formen, die dieser Task beseitigt hat; (2) der Test darf **keine** blosse Dateiliste hartkodieren, die immer gruen bleibt — er muss die **Muster** suchen, und seine eigene Erkennung ist **mutationsbewiesen**: eine wieder eingefuehrte Schleife macht ihn rot, und **ohne** Schleife bleibt er gruen; (3) der Test benennt die **erlaubten** Stellen (die geteilten Module `kittest_decode_support` und `scan_settle_support`) explizit, damit er seine eigene Regel kennt; (4) er prueft **alle** Suiten, die Snapshots oder `Harness` pumpen — `kittest_snapshots`, `kittest_crop_overlay`, `kittest_library_stack`, `kittest_spot_tool`, `kittest_sidecar_identity`, `kittest_mask_local`, `kittest_mask_visibility`, `kittest_decode_bound`, `folder_scan_settle` — **nicht** eine Stichprobe; (5) **drei** bekannte Stellen werden als **bewusste Ausnahmen** mit begruendeter Begründung benannt, statt sie stillschweigend zu ueberspringen: `kittest_snapshots.rs` `filmstrip_twenty_dummies` (Prädikat ist eine accesskit-Labelabfrage, kein App-Zustand, vom geteilten Loop nicht ausdrueckbar) und `kittest_spot_tool.rs:94-103` (600-Frame-Budget, aber mit `assert!` unmittelbar danach, also **laut**). Beide sind in Runde 2 am Code als *laut* bestaetigt. **Wichtig:** die Ausnahmen sind **laut** — sie verletzen die Regel „laut sein", nicht die Regel „der geteilte Wait". Eine stumme Schleife darf **nicht** als Ausnahme aufgenommen werden. **Abnahme der Gates:** `cargo fmt -p lumina-gui -- --check`, `cargo clippy -p lumina-gui --tests` 0 Warnungen, `sh scripts/check_file_sizes.sh`, **keine neuen Baseline-Eintraege**, `cargo test -p lumina-gui --lib` gruen (Sollwert **917**), `cargo test -p lumina-gui --all-targets` gruen, `sh scripts/golden_ref.sh check` MATCH, keine Golden-PNG. **Abgrenzung:** keine Produktlogik, keine Aenderung an der Warte-Logik selbst — der Test beobachtet nur ihre Form.


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

- [ ] **[PRIO: mittel] PIPELINE-CROP-EARLY-9 (vom Eigentümer am 2026-10-02 angestoßen, Release 1.0, GUI+CLI, nach AUTO-TONE-ANALYSIS-INPUT-8)** **Anlass:** Der Eigentümer fragt, ob sich Crop im Render **weiter nach vorn** ziehen lässt, um Arbeit zu sparen. **Gemessener Ist-Zustand (Reihenfolge aus `crates/lumina-core/src/render.rs`, nicht aus der Doku):** `apply_source_actions` (307) → `apply_spot_heals_from_recipe` (619) → **Adjustments/WB/Tonwerte (620, auf dem vollen Bild)** → Lens (645) → AutoFill (656) → Perspective (664) → GenerativeExpand (674) → **Crop (678)** → LensBlur (690) → Masks (712). **KORREKTUR AN DER GEWICHTUNG, gemessen in `PIPELINE-ANNASSUNGEN-10` (A2):** die Annahme, der Hebel sei die Kette zwischen 620 und 678, war **falsch gewichtet**. `apply_recipe_with_white_balance` ist **85 %** von `render_frame` (3,39/14,02/56,39 ms gegen 3,98/16,36/65,76 ms bei 512/1024/2048, aus `perf/baseline.json` ausgerechnet). **Der Hebel ist die Tonal-Stufe, nicht die Geometrie-Kette** — und genau die kommt man los, indem Crop **vor** Adjustments wandert. Ein vorgezogener Crop beschraenkt damit die **85 %** auf den Ausschnitt. Die Geometrie-Stufen bleiben der Grund, warum Crop nicht beliebig weit vorn stehen darf, sind aber **nicht** der Performance-Gewinn. Der Hebel ist damit die Kette zwischen 620 und 678: vier Stufen (Lens, AutoFill, Perspective, GenerativeExpand) arbeiten heute auf dem **vollen** Bild, davon Perspective und Lens als **Neuabtastung**. Ein vor Adjustments (620) gesetzter Crop würde die **sechs** teueren Stufen (Tonal/WB/Rauschen + die vier Geometrie-Stufen) nur noch auf den beschnittenen Pixeln laufen lassen. **HARTE UNTERGRENZE — Crop muss nach GenerativeExpand bleiben (674):** der Kommentar in `render.rs` sagt ausdrücklich, der generative Expand müsse **vor** Crop laufen, weil „crop coordinates reference the expanded canvas". Das ist keine Optimierung, sondern eine Koordinaten-Abhängigkeit. **WEICHE UNTERGRENZE — Crop muss nach Perspective (664):** Perspective verzerrt die Bildgeometrie; ein davor gesetzter Ausschnitt säße danach an der falschen Stelle. **Die beiden Ziele ziehen also in verschiedene Richtungen und der Konflikt ist der Expand/Crop-Konflikt**, nicht der Crop selbst. **Zwei Vorannahmen des Eigentümers, beide AM LAUF geprüft und eine davon KORRIGIERT — der Punkt ist hier, weil er die Abnahme geprägt hat:** (1) „dann berechnen wir Masken nicht auf einem Bereich, wo es nicht zählt" — **Prämisse falsch, die Masken rechnen schon heute nur auf dem beschnittenen Bereich**: Crop (678) liegt **vor** `apply_local_adjustments` (712). Es gibt hier nichts zu holen, und jede Formulierung, die das als Gewinn ausweist, wäre erfunden. Was **wirklich** mitwandert, ist die Maskengeometrie: `mask_input_width`/`mask_input_height` werden in Zeile 611/612 **vor** dem Crop eingefangen und in 695/696 weitergereicht, damit Masken in **Quellkoordinaten** definiert bleiben. Wird Crop vorgezogen, muss diese Angabe mitwandern, sonst verschieben sich Masken relativ zum Bild. (2) „während des Crop-Vorgangs wird ungecropt berechnet, das ist kein Problem" — **bestätigt und sauberer belegt, als angenommen**: `develop_geometry/crop_overlay.rs` dokumentiert, das gezogene Rechteck sei ein **Session-Entwurf in egui-Temp-Speicher**, „never the recipe and never the sidecar", committet erst mit `Enter` über `set_crop_free`, `Esc` verwirft; `effective_crop_draft` (Zeile 84) wählt **Draft → Rezept-Crop → voller Frame**, und der Modul-Kommentar sagt „crop-mode preview renders the full frame". **Ein Crop-Dragger erreicht das Rezept also gar nicht** und kann von einer vorgezogenen Crop-Stufe nicht gestört werden — die Randbedingung ist damit **erledigt und nicht in die Abnahme aufgenommen**. **Ziel:** die Reihenfolge so umstellen, dass die teuren Stufen nur auf dem beschnittenen Bereich laufen, **ohne** die Expand/Koordinaten-Bindung zu brechen. **Abnahme:** (1) **Byte-Nachweis per `cmp` gegen HEAD** für `render`, `export` png/jpeg und `batch` **ohne** Crop — nach `DoD.md` §11 mit echtem Vergleich, nicht mit Toleranz; (2) **Byte-Nachweis mit Crop**: die Umordnung darf gerenderte Bytes **nur** ändern, wenn der Crop das Ergebnis überhaupt verändert; bei einem auf das volle Bild beschnittenen Rect müssen die Bytes **identisch** bleiben (reines „rechnet weniger" ändert nichts); (3) **Messung des Gewinns**, mit Zahlen: `perf/baseline.json` misst `render_frame__512/1024/2048` als **Gesamt**; dieser Task braucht eine **Vorher/Nachher**-Messung auf einer Fixture **mit** Crop, über mehrere Beschnittgrößen (z. B. 50 %, 25 %, 10 % Restfläche) — eine Behauptung „spart Arbeit" ohne gemessene Vorher/Nachher ist nach `DoD.md` §9 keine Aussage; (4) **Masken-Test**: eine Maske in Quellkoordinaten sitzt nach der Umordnung **an derselben Bildstelle** wie vorher, mit und ohne Crop — der Test fährt den Produktionspfad und vergleicht gerenderte Bytes, nicht nur Normalisierungszahlen; (5) **Generative-Expand-Test**: mit aktivem Expand und Crop-Koordinaten, die sich auf die erweiterte Leinwand beziehen, muss das Ergebnis byte-identisch zum heutigen sein — das ist der direkte Nachweis, dass die harte Untergrenze eingehalten ist; (6) **Perspective-Test**: mit Perspektivkorrektur **und** Crop, byte-identisch; (7) Auto-Tone bleibt von dieser Umordnung **unberührt** — die Messdomäne ist `AUTO-TONE-ANALYSIS-INPUT-8`, und diese beiden Tasks dürfen sich nicht gegenseitig die Zuschreibung der Byte-Änderung streitig machen; (8) `check_file_sizes.sh` grün; keine neue Baseline-Anhebung, denn die Umordnung ändert keine Zeilenzahl. **VORBEDINGUNG, eigener Task:** `PIPELINE-ANNASSUNGEN-10` — der erwartete Gewinn muss **gemessen** sein, sonst ist dieser Task eine Umordnung auf Verdacht; dort steht auch die **korrigierte Gewichtung** (die Tonal-Stufe ist der Hebel, 85 % des Renders, nicht die Geometrie-Kette). **Abhängigkeit:** **nach** `AUTO-TONE-ANALYSIS-INPUT-8`, weil beide dieselbe Stufe (`apply_crop_stage`) und dieselbe Frage „auf welcher Domäne rechnet Auto" berühren; **nicht** parallel zu einem Slice, der Sidecar-Bytes ändert (`JSON-FLOAT-ROUNDTRIP`). **Vor dem Bau zu klären:** Ist der erwartete Gewinn überhaupt groß genug, um den Expand/Koordinaten-Konflikt anzufassen? Punkt (3) ist die Vorbedingung, nicht die Abnahme — ohne gemessenen Gewinn ist der ganze Task eine Umordnung auf Verdacht. **NACHTRAG DER MESSUNG (`PIPELINE-ANNASSUNGEN-10`, auf der Referenzmaschine gemessen, 2026-10-02) — die Vorbedingung ist erfuellt, und ein Teil der Gewichtung dieses Tasks ist damit zu korrigieren.** Bench `crates/lumina-bench/bench/crop_pipeline.rs`, Kommando `cargo bench -p lumina-bench --bench crop_pipeline -- --sample-size 50 --warm-up-time 2 --measurement-time 5`; Median/p95 in ms. **(b) Crop heute:** die Ersparnis bei 10 % Restflaeche ist **9,0 / 9,9 / 9,7 %** (512/1024/2048: volles Rect 4,62 → 4,21; 19,51 → 17,58; 78,21 → 70,58) — also **klein**, nicht die 17–27 %, die eine fruehere Fremdmaschinen-Messung meldete. **(a) Geometrie-Einzelkosten (jetzt direkt gemessen, nicht subtrahiert):** `lens_stage` **11,4 / 44,4 / 177,4 ms** (manuelle Linsenkorrektur: 8-stufige Newton-Iteration pro Pixel, `lib.rs:1697-1701`), `perspective_stage` **3,5 / 13,4 / 53,8 ms** (inverse Neuabtastung), `autofill_stage` **0,012 / 0,052 / 0,247 ms** und `expand_stage` **0,019 / 0,072 / 0,282 ms** (beide seit GEN-ONNX-1 nur noch Artifact-`clone`, `generative.rs:92/124` — die teure BFS ist nicht mehr im Renderpfad). **Das korrigiert die Gewichtung dieses Tasks, und zwar in beide Richtungen:** die Geometrie-Kette ist bei **eingeschalteter** Lens-/Perspective-Korrektur **nicht** billig — `lens_stage` @2048 (177 ms) kostet mehr als das ganze Baseline-`render_frame__2048` (65,8 ms) und mehr als die Tonal-Stufe (56,4 ms) — aber sie ist **nur dann** teuer, wenn der Nutzer sie einschaltet (ohne Eintrag ist sie die Identitaet). **(c) Cache:** `prepare_source_base` (Miss) **0,012 / 0,051 / 0,220 ms** gegen `StageFrameCache::get` (Hit) **0,012 / 0,050 / 0,227 ms** — beide praktisch identisch und vernachlaessigbar; der Cache sitzt **vor** einem vorgezogenen Crop und wird von ihm weder behindert noch beschaedigt. **Was weiterhin offen ist und NICHT behauptet wird:** der Vorher/Nachher-Gewinn der **umgestellten** Pipeline ist ungemessen (sie existiert nicht), und ob der Expand/Perspective-Konflikt die Umordnung zulaesst, ist ungeprueft. **Die drei Vorbedingungen (a)(b)(c) sind damit mit Vorher/Nachher-Zahlen auf derselben Maschine belegt; die Zahlen stehen vollstaendig in `PIPELINE-ANNASSUNGEN-10` — dieser Nachtrag fuehrt nur das Ergebnis.**

- [ ] **[PRIO: mittel] JSON-FLOAT-ROUNDTRIP (User-Order 2026-09-26; Abspaltung aus AUTO-TONE-CLI-6, Release 1.5)** Dieser Workspace baut `serde_json` **ohne** das Feature `float_roundtrip`. **Gemessen 2026-09-26:** von 200.000 `f64`-Werten im Reglerbereich `[-10, 10]` ueberleben 14.900 (7,45 %) den Round-Trip nicht (1 ULP), von 171 real erzeugten Auto-Tone-Werten 31 (18,1 %). Persistierte `f64`-Felder in `EditRecipe`/`AutoFeatures` (Auto-Tone-Spiegel, `adjustments`, die mask-lokalen Skalare in `LocalAdjustments` — `exposure`, `contrast`, `highlights`, `shadows`, `temperature_delta_k`, `tint_delta`, `vibrance`, `saturation`) koennen daher beim Laden **einen** ULP vom geschriebenen Wert abweichen. **Praezisierung:** die Mask-Local-**Digests** selbst sind blake3-Hashes und damit nicht betroffen; betroffen sind die `f64`-Werte, ueber die sie gebildet werden. **Der Mechanismus ist einseitig** (gemessen 2026-09-26): das Feature-Flag steuert nur den **Parser**, waehrend der Serialisierer (ryu) immer die kuerzeste exakt wiederlesbare Dezimalzahl schreibt — der Sidecar-Text ist also **immer korrekt**, und der Verlust passiert beim **Laden**. Folge fuer Digests: Lauf 2 schreibt die kuerzere Textform des um 1 ULP verschobenen Werts, d. h. die Sidecar-Bytes aendern sich messbar (bei der 40-Fixture-Familie 27 von 40), und jeder ueber die serialisierten Bytes gebildete Digest ist davon betroffen. Ein Digest-Wechsel auf einem echten Mask-Layer wurde **nicht** gemessen — ohne persistierte f64-Mask-Local-Skalare im Auto-Tone-Fixture — und genau diese Luecke ist der Abnahmepunkt dieses Tasks. ****ENTSCHIEDEN 2026-10-02 (Eigentümer, `JSON-ROUNDTRIP`, festgeschrieben in `Agents.md` — Variante (a) dieser Task-Formulierung):** `serde_json` wird mit `float_roundtrip` gebaut; jeder `f64`-Round-Trip ist damit exakt, und die gemessenen **7,45 %** 1-ULP-Verluste beim Laden entfallen. Variante (b) (bewusst lassen, Workspace-Grenze dokumentieren) ist damit **verworfen** und wird hier **nicht** erneut zur Wahl gestellt. **Bekannter Preis (entschieden mitgetragen):** jedes bestehende Sidecar-Byte kann sich aendern; alle Goldens, Rezept-Digests und Render-Identitaeten sind neu zu pruefen. **Urspruengliche Entscheidungsoptionen, wortgetreu fuer den Aktenvermerk:** (a) `float_roundtrip` aktivieren — dann ist jeder Round-Trip exakt, aber **jedes** bestehende Sidecar-Byte kann sich aendern, und alle Goldens, Rezept-Digests und Render-Identitaeten sind neu zu pruefen; (b) bewusst lassen und die Eigenschaft als dokumentierte Workspace-Grenze behandeln — dann braucht es einen expliziten Vertrag, welche Felder als **exakt reproduzierbar** gelten duerfen (die `f32`-Werte der Mask-Local-Pipeline sind es, weil 0..=255 in `f32` exakt ist) und welche als best-eff-only zu gelten haben. **Abnahme (Variante (a), die entschiedene ist):** das Feature im Workspace aktivieren; **vor** der Aktivierung ein Migrations-/Golden-Plan fuer die betroffenen Sidecar-Bytes; danach ein Nachweis, dass **kein** Rezept-Digest sich unbeabsichtigt aendert; die betroffenen Goldens neu aufnehmen bzw. die Abweichung als gemessen begruenden. (Die Abnahme der **nicht** gewaehlten Variante (b) — Liste der betroffenen Felder plus ein Test, der die Restunschaerfe dokumentiert statt sie zu verstecken — entfaellt mit der Entscheidung; die f32-Betreiber der Mask-Local-Pipeline sind davon ohnehin nicht betroffen.) **Abhaengigkeit:** darf nicht parallel zu einem Slice laufen, der Sidecar-Bytes aendert; erst nach MASK-LOCAL-P1.2a-d; die zweite genannte Abhaengigkeit (`AUTO-TONE-CLI-6`) ist verifiziert abgeschlossen und aus dieser Datei entfernt.

- [ ] **[PRIO: niedrig] MCP-PARITY-C (User-Order 2026-09-26; SOLL zuerst, Release 2.0, GUI-frei, nach A und B)** Rest der Lücke: die **Mehrbild-Pipelines** `lumina merge-hdr`, `lumina merge-pano`, `lumina matrix`. **Warum zuletzt und warum ein eigener Task:** diese drei sind die teuersten und speicherlastigsten, sie arbeiten ueber viele Eingaben und schreiben eigene Artefakte; sie sind hier headless **nur** mit echten Fixtures sinnvoll prüfbar. **Festgeschriebene Semantik:** (1) **Kein Blindschreiben von `expected_route`** — `matrix` hat ein `expected_route`-Feld, das laut `testdata/matrix/recipe-set.v1.json` und `feature/quality/conflicts-and-acceptance.md` gepflegt wird; dieses Slice darf daran **nichts** ändern, und ein Tool darf `expected_route` weder setzen noch auswerten, um sich selbst zu validieren. (2) `merge-hdr`/`merge-pano` muessen die Eingabemenge **vor** der Verarbeitung validieren und bei unvollstaendiger Menge **laut** abbrechen; keine stillen Teil-Ergebnisse. (3) Paritaet wie A: MCP- und CLI-Aufruf erzeugen byte-identische Ausgaben fuer dieselbe Eingabemenge. (4) Die Modellpfade (`merge-hdr` mit Tone-Mapping, `matrix` mit Denoise) muessen ohne Modell **laut** scheitern. **Abnahme:** Byte-Paritaet MCP-gegen-CLI je Pipeline, Abbruch bei unvollstaendiger Eingabemenge, kein `expected_route`-Anfassen (per Diff nachweisbar), Registry in beiden Pfaden. **Abnahme-Gate:** mindestens eine echte Kamera-RAW-Fixture-Reihe, sonst bleibt der Task offen — eine reine Mock-Reihe wuerde die I/O-Pfade nicht pruefen.

### PRIO: mittel


### PRIO: niedrig

- [ ] **[PRIO: hoch] TEST-ORPHAN-97 (Release 1.0; **korrigiert `TEST-DEDUP-100`** nach unabhaengiger Verifikation, 2026-09-29)** **97 `#[test]`-Attribute liegen in 14 Dateien, die `cargo` nie liest — und ein Commit hat sie wiederbelebt, ohne eine `mod`-Deklaration.** **Die urspruengliche Fassung dieses Befunds war FALSCH und ist hiermit zurueckgenommen.** Sie meldete „100 Tests laufen doppelt"; **gemessen sind es 3**, davon 2 in einem Default-Build. **Der Denkfehler war methodisch und offen sichtbar:** der Befund entstand aus einem Dateisystem-Walk, und ein Walk hat kein Konzept von einem Kompilierziel. **Verifiziert an rustcs eigenem dep-info** (`target/debug/deps/lumina_gui-183f191d296a0278.d`): `instrdbg_*`, `f100_*`, `r3_*`, `g01_*` sind **alle** nicht enthalten; die thematischen Nachfolger sind kompiliert. **Herkunft, gemessen:** `b88b14e` hat diese Dateien per `git mv` in thematische Namen umbenannt; `4a8bad5` — dessen eigene Nachricht „Kein Produktcode in diesem Commit" sagt — hat sie mit **+4133 Zeilen und ohne eine einzige `mod`-Deklaration** wieder hinzugefuegt. **Das ist toter Code, keine Testredundanz, und der Unterschied ist der ganze Befund:** ein Fehler in einer nicht kompilierten Datei faellt bei keinem Lauf auf, und die Datei liest sich beim Review wie gelebte Abdeckung. **23 der 55 toten Dateien referenzieren einander** (`instrdbg.rs`→`instrdbg_prepare`, `f100_audit.rs`→`f100_surface`) — die Insel referenziert nur sich selbst und waere auch nicht reaktivierbar. **Abnahme:** (1) die 16 von `4a8bad5` zurueckgefuehrten Dateien werden entfernt — nach SOLL → Implementierungs-Agent → unabhaengige Verifikation, der Build-Agent entfernt **selbst nichts**; (2) **vor** dem Entfernen wird fuer jede Datei belegt, dass ihr Inhalt in der thematischen Nachfolgerdatei **enthalten** ist (die Bodies sind heute byte-gleich, das ist je Datei zu zeigen, nicht global zu behaupten); (3) `cargo test -p lumina-gui --lib` bleibt bei **exakt 929** — das Entfernen eines Orphans aendert die Zahl um **0**, und eine Abweichung ist ein Befund; (4) **neues Gate:** ein Check, der eine Datei unter `src/tests/` findet, die im dep-info des Test-Targets **nicht** vorkommt. Genau das haette `4a8bad5` gefangen; ein Check auf „zwei gleiche Bodies" haette **97 Mal auf Dateien gefeuert, die cargo nicht liest**, und den echten Defekt nicht gesehen (die alte Abnahme (5) ist deshalb **zurueckgenommen**); (5) nach dem Lauf bleibt `cargo check -p lumina-gui --all-targets` gruen und `sh scripts/check_file_sizes.sh` gruen. **Korrigierte Teilmessungen fuer den Aktenvermerk:** `#[test]`-Attribute im Baum **3170**, nicht 2986 (nur `//!`-Doc-Kommentar und eine exotische Crate-Auswahl liefern 2986); `#[ignore]` im Baum **112**, in keiner Zaehlung ausgeschlossen; davon **503** in 55 Dateien ohne Modulbaum. **Zurueckgenommene Behauptungen:** die Erklaerung des GUI-Shard-Laufzeitballasts durch diese Dateien (die 100 sind im 929er-GUI-Shard nicht enthalten, Ballast ist real, die Ursache eine andere); das Muster „in jedem Paar" (in 14 von 17; die drei onnx-Paare haben kein Praefix auf einer Seite und sind genau die, in denen **beide** Dateien kompilieren). **Bleibende echte Redundanz, getrennt gefuehrt:** 3 byte-gleiche Paare, davon 2 im Default-Build — und beide sind **Unit-gegen-Integrationstests**, die **nicht austauschbar** sind (`manifest.rs:902` `capabilities_validate_directly` beweist als Unit-Test private Sichtbarkeit, die Integrationskopie beweist die oeffentliche Exportierbarkeit; `preprocess.rs:263`/`tests/preprocess.rs:39` haben sogar **verschiedene Namen**, sind also zwei Behauptungen mit einem Body). **Von diesen 3 Paaren ist also keine echt redundant** — sie ist damit **0 Duplikate**, nicht 3.

- [ ] **[PRIO: niedrig] TEST-AUDIT-36 (fortlaufend, User-Regel 2026-09-26)** Redundanz-Audit der Testsuite: **höchstens einmal pro Woche** einen Build-Agenten starten, der prüft, ob es unnötige Tests gibt. Der Build-Agent **berichtet nur** und entfernt nichts eigenmächtig; jede Löschung läuft über SOLL → Implementierungs-Agent → unabhängige Verifikation. **Prüfkatalog:** (a) Tests, die eine Konstante auf sich selbst prüfen; (b) doppelte Abdeckung derselben Aussage in mehreren Dateien; (c) Tests, die eine Implementierungsentscheidung statt des Verhaltens pinnen; (d) Tests, die bei Wegfall der Logik nichts verlieren würden; (e) Tests ohne Fehlsignal — die grün bleiben, egal ob die Aussage stimmt; (f) **Laufzeit-Ballast**, der die Suite verlangsamt, ohne Aussage zu tragen (der 3:11-GUI-Shard ist das aktuelle Beispiel); (g) Assertions, die nach einem Refactor nur noch den Refactor beschreiben. **Abnahme:** ein priorisierter Befundbericht mit Test-Name, Datei und der konkret entfallenden Aussage; keine Löschung ohne Begründung; keine Reduzierung der Netto-Aussagenabdeckung — eine beim Audit entdeckte **Lücke** (Feature, Klausel, Fehlerpfad ohne Anker) wird als **neue offene Aufgabe** angelegt, nicht durch Streichen eines Nachbarn ersetzt. Grundlage: Policy in `Agents.md` §„Testabdeckungs-Politik". **Abgrenzung:** keine Produkt- oder Schemaänderung; der Audit verändert keine Abnahmegate und ersetzt keine DoD-Prüfung. **Teilergebnis 2026-09-28 (Build-Agent, mechanische Vorpruefung — Punkt (b) teilweise, Punkt (f) nicht behandelt):** Von **781** `#[test]`-Funktionen in `crates/lumina-gui/src/tests/` haben **null** einen byte-gleichen Rumpf (Methode: `#[test]`-Funktionen klammerbasiert extrahieren, Zeilen- und Blockkommentare plus Whitespace entfernen, SHA-1 ueber den Rest, Gruppen >1 melden). Die **einfachste** Form doppelter Abdeckung existiert damit nicht. Das beweist **nicht**, dass keine **Eigenschaft** doppelt abgedeckt wird — dieselbe Aussage kann in verschiedenem Code stehen; die semantische Ebene ist der teure Teil des Audits und **nicht** erledigt. **Zweiter Befund, andere Klasse:** **22 Gruppen / 56 Funktionen** mit byte-gleichem Rumpf sind **Helfer**, keine Tests (`pointer_button` 7x, `click_action_lines` 4x, `assert_single_action_line` 4x, `solid_gray` 3x, `stub_identity` 2x). Das ist **duplizierte Testinfrastruktur**, keine doppelte Abdeckung — Loeschung waere falsch, sie wuerde den Build brechen. Die groesste Einzelposten: `src/tests/g15_stacks.rs` und `src/tests/library_sort.rs` teilen **vier** Helfer mit zusammen **1 202 Zeilen** (`stub_identity` 674, `stub_raw` 288, `put_selection` 123, `scan` 117). Das ist **Dekonsolidierung** und gehoert zu `GUITEST-STRUCT-34`, **nicht** in diesen Audit: es sinkt die Zeilenzahl, ohne eine Aussage zu verlieren. **Fuer die naechste Runde offen:** (b) auf der Eigenschaftsebene, (a) (c) (d) (e) (g) unberuehrt, (f) **Laufzeit-Ballast unberuehrt** — die gemessene Streuung des `kittest_snapshots`-Wanduhrlaufs (46,41 / 51,92 / 245,66 s auf identischem Baum, `DoD.md` §9) macht (f) erst nach der Ursachenanalyse sinnvoll messbar.
- [ ] **[PRIO: niedrig] CI-WATCH-1 (fortlaufend)** Nach jedem Push (morgen als erstes): CI-Runs prüfen (`gh run watch` / `gh run list --branch main`), Ergebnis im Tagesstand vermerken. Bei Rot: als Next-Task in `Agents.todo.md` dokumentieren, NICHT still umsetzen (User-Vorgabe). Abnahme: jeder Push hat ein geprüftes CI-Verdict.
- [ ] **[PRIO: niedrig] LRPAR-G14-DENOISE-IMPL-20 (Release: 2.0)** KI-Denoise-Implementierung nach Entscheid `feature/decisions/LRPAR-G14-DENOISE-20.md` (F-078-Fixture-Entscheid ✓, Schema ✓, Pipeline-Stufe + Persistenz + GPU-Refusal ✓, ONNX-Backend ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only, F1-Input-Spec-v2 inkl. Core-Blend-/Assembly-Identität ✓, F3-zdata-Feldvalidierung ✓; offen: echte Gewichte). Der Parent-Checkbox bleibt bis zum F-078-Gate (Gewichts-Lizenz, Provenienz und Hash-Pin) offen; keine stillen Stub-/Produktions-Fallbacks. Abnahme: CLI + GUI-headless + Golden/PSNR, kein stiller Fallback.
- [ ] **[PRIO: niedrig] LRPAR-G09-CULL-IMPL-25 (Release: 2.5)** KI-Culling-Implementierung nach Entscheid `feature/decisions/LRPAR-G09-CULL-25.md` (Schema ✓, Heuristik `lumina-cull` ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only; **Follow-up 2026-09-24:** CLI/GUI erzwingen Live-Quellenidentität und verweigern Sidecar-Writes bei Hash-/Längenkonflikt, inklusive `--force`/Batch-Isolation; offen: optional ONNX Stufe 2 und kittest). Abnahme: CLI-Exit-Codes + GUI-headless + kittest, Vorschlag schreibt nie Rating/Flag/Label. **Parent bleibt wegen Stage 2/kittest offen.**

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
