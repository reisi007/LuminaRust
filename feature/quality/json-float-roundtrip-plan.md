# JSON-FLOAT-ROUNDTRIP — Migrations- und Golden-Plan

Normative Grundlage: `Agents.md` Entscheidung `JSON-ROUNDTRIP` (2026-10-02,
Variante (a)) und Task `JSON-FLOAT-ROUNDTRIP` in `Agents.todo.md`. Abnahme:
Feature im Workspace aktivieren, **vor** der Aktivierung diesen Plan aufstellen,
**danach** nachweisen, dass kein Rezept-Digest sich unbeabsichtigt ändert.

Alle Zahlen unten sind in diesem Arbeitsstrom **selbst gemessen** (Werkzeug,
Kommando und Ergebnis stehen daneben). Wo eine Zahl aus der Vergangenheit
übernommen wird, ist sie als solche markiert.

---

## 1. Mechanismus — geprüft, nicht übernommen

Das Task-Formular behauptet: „das Feature-Flag steuert nur den **Parser**,
während der Serialisierer (ryu) immer die kürzeste exakt wiederlesbare
Dezimalzahl schreibt". Diese Behauptung wurde gegen die tatsächlich verlinkte
`serde_json`-Version geprüft, weil davon abhängt, welche Bytes sich ändern
können.

Verlinkte Version: `serde_json` 1.0.151 (`Cargo.lock:3720`).

| Behauptung | Prüfung | Ergebnis |
|---|---|---|
| Feature nur im Parser | `grep -rn float_roundtrip src/` im Registry-Quelltext | **22** Treffer: 19 in `src/de.rs`, 2 in `src/value/de.rs`, 1 in `src/lib.rs:435` (die `mod lexical;`-Deklaration des Feature-eigenen Parsers). `src/ser.rs` enthält **0** Treffer; ein Verzeichnis `src/ser/` existiert nicht. Berührt ist also nur die Parser-Seite |
| Feature ist abhängigkeitsfrei | `[features]` in `serde_json/Cargo.toml` | `float_roundtrip = []` — keine Kraten, also keine `Cargo.lock`-Änderung |
| Float-Parser im Feature ist korrekt gerundet | `src/de.rs:674` `parse_long_integer` speist die Ganzzahl- und Bruchstellen an `crate::lexical` | eigenes, im Krat gepacktes `lexical` (`src/lexical/`, 20 Dateien) — kein `ryu` mehr (0 Treffer für `ryu` im ganzen Krat) |
| Serialisierer schreibt kürzeste exakte Form | `src/ser.rs:1716` `write_f64` → `zmij::Buffer::format_finite` | unabhängig vom Feature; `zmij` wird vom Serialisierer in **beiden** Builds benutzt |

**Zwei Korrekturen an der übernommenen Formulierung** (belegt, nicht vermutet):

1. Der Serialisierer ist seit `serde_json` 1.0.151 **nicht mehr `ryu`**, sondern
   das krat-eigene `zmij`. `grep -rn ryu src/ Cargo.toml` im verlinkten
   `serde_json`-Quelltext → **0 Treffer**. Für die Eigenschaft ist das
   unerheblich (beide schreiben die kürzeste exakt-wiederlesbare Form), für die
   Diagnose im Text schon: wer im Fehlerfall den Serialisierer verdächtigt, sucht
   heute in der falschen Datei.
2. Die Feature-Deklaration `float_roundtrip = []` hat **keine** Abhängigkeiten.
   Der Task ging von einer erwarteten `Cargo.lock`-Änderung aus; das ist
   **nicht** eingetreten (`git diff Cargo.lock` nach dem Flip: leer). Der Lock
   ist von der Umstellung nicht berührt.

---

## 2. Vor der Aktivierung gemessen — welche Bytes heute betroffen sind

Werkzeug: throwaway-Krat außerhalb des Workspace
(`/tmp/lum/measure`, `serde_json` 1.0.151 aus dem Registry-Cache des Hosts,
`blake3` 1.8.7). Es scannt den Roh-**Text** (nicht das geparste `Value`, das
die ULP schon verloren hätte), liest jedes Float-Literal zweimal — mit der
aktiven `serde_json`-Parser-Instanz und mit `f64::from_str` (korrekt gerundet,
dieselbe Algorithmusklasse wie `float_roundtrip`) — und druckt die kanonische
Form des Dokuments plus deren blake3-Digest.

### 2.1 Alle committeten JSON-Dateien des Workspace

Eingabe: `git ls-files | grep -E '\.json$'` → **43** Dateien (`agent-harness/
artifacts/**`, `perf/*.json`, `scripts/perf/*.json`,
`testdata/matrix/recipe-set.v1.json`).

| Messung | Feature AUS (vorher) | Feature AN (nachher) |
|---|---|---|
| Float-Literale gesamt | 5236 | 5236 |
| davon exakt rückgelesen | **5236** | **5236** |
| davon 1 ULP verloren | **0** | **0** |
| Dokumente, deren kanonische Form sich ändert | **0** | **0** |
| Dokumente, deren blake3-Digest sich ändert | **0** | **0** |
| Dokumente, die re-serialisierbar stabil sind | 43 / 43 | 43 / 43 |

Kommando (vorher): `./target/release/measure <43 Dateien>` →
`FLOAT_SUMMARY total=5236 exact=5236 lost=0 loss_pct=0.00`.
Dasselbe Kommando mit `serde_json = { version = "1", features =
["float_roundtrip"] }` → **identische Ausgabe** (`diff` leer).

**Ergebnis 1: kein committetes Byte ändert sich.** Der Golden-Satz dieses
Slices ist damit trivial erfüllt, aber nicht aus dem Bauch heraus, sondern
deshalb, weil — geprüft an der Parser-Quelle (`src/de.rs:638`
`#[cfg(not(feature = "float_roundtrip"))] f64_from_parts`) — der Verlust nur
eintritt, wenn

* die Dezimalstelle als `u64`-Signifikant **nicht mehr exakt** in `f64` passt
  (Signifikant > 2^53, also ≥ 17 Dezimalstellen, bzw. 16 Stellen oberhalb
  `9007199254740992`), **oder**
* der Skalierfaktor `10^|exp|` in `f64` nicht exakt ist (`|exp| > 22`).

Kurze Dezimalliterale mit ≤ 16 signifikanten Stellen — und genau solche schreibt
der Serialisierer für einen **von Hand gesetzten Reglerwert** — überstehen den
Round-Trip auch ohne das Feature. Verloren gehen nur **berechnete** `f64`, deren
kürzeste Darstellung ≥ 17 signifikante Stellen hat. Das ist auch der Grund,
warum die Auto-Tone-Spiegel (berechnet) mit 18,3 % betroffen sind, ein
Slider-Domain-Zug aber nur mit ~7,9 %.

### 2.2 Reproduktion des Verlusts (rot vor dem Fix)

Domäne exakt wie gemessen: 200.000 pseudozufällige `f64` in `[-10, 10]`
(splitmix64, Seed `0x243f_6a88_85a3_08d3`, `unit = (z >> 11) / 2^53`), mit
serde_json' eigenem Serialisierer geschrieben und zurückgelesen.

| Build | verloren | exakt | Quote |
|---|---|---|---|
| Feature **AUS** (vorher) | 15.801 | 184.199 | **7,90 %** |
| Feature **AN** (nachher) | **0** | 200.000 | **0,00 %** |

Maximale Abweichung im Vorher-Lauf: **1 ULP** in allen 15.801 Fällen. Zwei
verifizierte Einzelfälle aus dem Lauf:

```
literal=-1.5769134665636955 written=(bits bff93b099d783580) read_back=(bits bff93b099d783581) ulp=1
literal=-9.936999806814127  written=(bits c023dfbe704d3fff) read_back=(bits c023dfbe704d4000) ulp=1
```

Anmerkung zur übernommenen Zahl: die Task-Dokumentation nennt 7,45 %
(lineares Raster, Schrittweite 1e-4, 2026-09-26). Dieser Arbeitsstrom hat
7,90 % mit einem deterministischen splitmix64-Zug reproduziert. Beide liegen in
derselben Größenordnung; die Quote ist, wie auch die Vergangenkeitsmessung
selbst notiert, stichprobenabhängig. Für die Umsetzung ist sie nicht
entscheidend — entscheidend ist, dass der Verlust **vorher vorhanden** und
**nachher null** ist.

### 2.3 Was physisch betroffen ist

| Klasse | Umfang | Änderung durch das Feature |
|---|---|---|
| committete JSON-Fixtures (`testdata/matrix/recipe-set.v1.json`, `agent-harness/artifacts/**`, `perf/**`) | 43 Dateien, 5236 Float-Literale | **keine** (§2.1) |
| Goldens PNG (`testdata/matrix/golden/`, 22 Dateien) | Render-Ergebnis | **keine** — `git status --porcelain` nach dem Flip zeigt das Verzeichnis unverändert; die Matrix-Suite ist grün. Nachgezählt in der Verifikationsrunde (2026-10-12): das Verzeichnis hält **22** PNGs, der ganze Workspace **114** getrackte PNGs (`git ls-files '*.png'`) — keines davon ist nach dem Flip verändert |
| kittest-Goldens / `scripts/golden_ref.lock` | 66 Goldens, lokales macOS-Gate | **nicht ausführbar hier** — `golden_ref.sh check` exit **1**, gemeldete Abweichung „this machine is not the pinned golden reference platform" (`os.name=Linux` vs. `macOS`, `gpu.adapter=unavailable` vs. `Apple M5 Pro`). Benanntes Gate, siehe §5 |
| Digest-Literale im Code | `grep -rn "blake3:[0-9a-f]\{16,\}" --include=*.rs crates/` → **0** | **keine** einzutragende Neuerwartung |
| zur Laufzeit geschriebene Sidecars (`.lumina.json`, gitignored) | nicht committet | betroffen sind ausschließlich Auto-Tone-Spiegel und berechnete Mask-Lokal-Skalare, deren kürzeste Form ≥ 17 signifikante Stellen hat. Das ist eine **Eigenschaft der Datei auf der Anwender-Platte**, nicht des Repositorys |

**Ergebnis 2: das Repository hat keinen einzigen neuen Golden aufzunehmen.**
Die einzigen Dateien, die sich ändern, sind die Test-Module, die den **Verlust
selbst** als Erwartung gepinnt haben — siehe §4.

### 2.4 Blast Radius des Flips auf die Test-Suites (vorher → nachher)

| Suite | vorher (Feature aus) | nachher (Feature an) | Delta |
|---|---|---|---|
| `lumina-sidecar` | 282 / 0 | 284 / 0 | +2 (die beiden neuen Wächter) |
| `lumina-cli` | 369 / 0 | 369 / 0 | 5 Tests umgestellt, gleiche Summe |
| `lumina-core` | 562 / 0 | 562 / 0 | 0 |
| `lumina-gui` | 989 / 0 (82 ign.) | 989 / 0 (82 ign.) | 0 |
| `lumina-stages` | 13 / 0 | 13 / 0 | 0 |
| `lumina-merge` | 58 / 0 | 58 / 0 | 0 |
| `lumina-raw` | 35 / 0 | 35 / 0 | 0 |
| `lumina-onnx` | 210 / 0 | 210 / 0 | 0 |
| `lumina-gpu` | 70 / 0 (28 ign.) | 70 / 0 (28 ign.) | 0 |
| `lumina-mcp` | 83 / 0 | 83 / 0 | 0 |
| `lumina-cull` | 45 / 0 | 45 / 0 | 0 |
| `lumina-iptc` | 31 / 0 | 31 / 0 | 0 |
| `lumina-testskip` | 6 / 0 | 6 / 0 | 0 |

Format: passed / failed. Vorher/Nachher-Zahlen der sechs betroffenen Suites
wurden im **selben** Arbeitsstrom gemessen; die übrigen sieben Kraten wurden
zusätzlich mit zurückgenommenem Flip wiederholt, damit ein „vorher rot, nachher
grün" nicht als Verbesserung unterging. Gefunden: keine.

---

## 3. Nach der Aktivierung — Digest-Nachweis

### 3.1 Rezept-Digest: welche Digests es überhaupt gibt

| Digest | Quelle | Vom Feature berührt? |
|---|---|---|
| `RenderKey::recipe_hash` | blake3 über `serde_json::to_vec(recipe)` (`crates/lumina-core/src/pipeline.rs:194`) | nein — der Serialisierer ist identisch |
| `RenderKey::mask_recipe_hash` | blake3 über `serde_json::to_vec(mask_recipe)` (`pipeline.rs:217`) | nein |
| `LocalAdjustments::digest` | blake3 über `serde_json::to_vec(self)` (`local_adjustments.rs:430`) | nein |
| `mask_layers_digest`, `MaskStateSnapshot::digest` | über serialisierte Bytes (`local_adjustments/mask_state.rs:23/88`) | nein |
| `PreviewKey::digest`, `source_fingerprint` | blake3/sha256 über Bytes/Felder | nein |

Alle diese Digests gehen über **serialisierte** Bytes. Der Serialisierer ist in
beiden Builds derselbe (`write_f64` → `zmij`, `src/ser.rs:1716`, ohne
`#[cfg]`). Ein Digest kann sich also nur ändern, wenn das `f64` **vor** dem
Serialisieren anders ist — und das heißt: nur, wenn es vorher aus einer Datei
**gelesen** wurde.

Daraus folgt die einzige Stelle, an der ein Digestwechsel auftreten kann, und
sie ist beabsichtigt: ein Dokument, das **geladen** wird, liefert für eine
Minderheit der Float-Literale vorher einen um 1 ULP verschobenen Wert, nachher
den exakten. Wird es danach neu serialisiert, schreibt der Serialisierer die
kürzeste Form **dieses** Werts. Der Digest über diese Bytes ist nachher der
**richtige** (er bezeichnet den Wert, den der Text ausdrückt), vorher der
**falsche**. Genau das ist der Zweck des Features.

### 3.2 Bewiesene Aussagen (Nachweis, nicht Behauptung)

| # | Aussage | Beleg | Kategorie |
|---|---|---|---|
| D1 | Kein committeter Rezept-/Sidecar-Digest ändert sich | `diff` der 43-Datei-Messung vor/nach dem Flip: **leer**; `FLOAT_SUMMARY lost=0` in beiden Läufen | gemessen |
| D2 | Kein committeter Golden (PNG) ändert sich | `git status --porcelain` nach dem Flip: `testdata/matrix/golden/` unverändert (22 PNGs); nachgezählt 2026-10-12: **alle 114 getrackten PNGs** des Workspace unverändert; Matrix- und GUI-Suiten grün (989 / 0) | gemessen |
| D3 | Der Serialisierer ist byte-identisch in beiden Builds | Serde-Quelle: 22 Treffer auf `float_roundtrip`, keiner davon in `src/ser.rs` (0 Treffer); `write_f64` (`ser.rs:1716`) hat kein `#[cfg]` | gemessen am Quelltext |
| D4 | Ein geladen-→-gespeichert-Zyklus ist nun bitstabil | neuer Test `every_persisted_f64_survives_a_save_load_save_cycle_bit_identically`, grün mit 284 / 0 in `lumina-sidecar`; rot unter Mutation M1 | gemessen (Test gelaufen) |
| D5 | Ein im Speicher gebautes Rezept hat vorher und nachher denselben Digest | neuer Test `a_recipe_built_in_memory_keeps_its_digest_across_the_boundary`; außerdem die `RenderKey`-Digest-Tests der `lumina-core`-Suite (562 / 0, unverändert) | gemessen |
| D6 | Die 44-von-240-Auto-Tone-Drift ist auf 0 gefallen | umgestellter Test `three_runs_write_the_same_literals` über dieselbe 40-Mitglieder-Familie, 240 Werte, grün | gemessen |
| D7 | Der frühere 1-ULP-Verlust war echt und ist nicht etwa verschwunden, weil die Probe unempfindlich wurde | Mutation M1: Feature aus allen 12 Manifests entfernt → **7 von 7** umgestellten/neuen Tests rot (§6) | gemessen |
| D8 | Kein Digest-Literal im Code braucht eine Neuerwartung | `grep -rn "blake3:[0-9a-f]\{16,\}" --include=*.rs crates/` → **0** Treffer | gemessen |

### 3.3 Was M1 konkret zeigte (der Beleg, nicht die Zusammenfassung)

Unter der Mutation M1 (Feature aus) fiel
`every_persisted_f64_survives_a_save_load_save_cycle_bit_identically` mit
bitgenau ausweisbaren Werten — das ist die Mechanik an einem echten Dokument,
nicht an einem Spielwert:

```
auto_exposure: Some(-0.20253906249999998)  ->  Some(-0.2025390625)          (1 ULP)
auto_contrast: Some(0.013476562499999997)  ->  Some(0.013476562499999995)   (1 ULP)
local.exposure:  -0.20253906249999998      ->  -0.2025390625
local.contrast:   0.013476562499999997     ->   0.013476562499999995
Digest  cc665d46f5d072f34e6d92ad52a9356e9d4e8f2745934341f5a85baad6c6d7da
     -> 7e0ea2c334296774fb04e234780f6e18f05c27bea76ea59e2afbbffa3fb3b332
```

Auffällig und ehrlich festgehalten: die **übrigen** 17-stelligen Literale des
Dokuments verloren keinen Bit. 17 signifikante Stellen sind die **notwendige**
Bedingung für den Verlust, nicht die hinreichende. Die beiden Werte mit
Provenienz (gemessen verlustbehaftet am 2026-09-26) sind deshalb Korpus, nicht
Statistik.

---

## 4. Was sich ändern musste — Golden-Plan im engeren Sinn

### 4.1 `crates/lumina-cli/src/tests/auto_tone_float.rs` — Erwartung invertiert

Dieses Modul pinnt den **Verlust**. Fünf Erwartungen werden durch das Feature
**beabsichtigt** aufgehoben; jede ist einzeln aufgeführt, damit keine als
„nebenbei grün" durchgeht.

| Test (vorher) | Vorher gepinnt | Nachher gepinnt | Begründung |
|---|---|---|---|
| `the_emitted_text_is_exact_and_the_parser_loses_exactly_one_ulp` → `the_emitted_text_and_the_parser_agree_bit_for_bit` | `serde_json_round_trip(v) == v ± 1 ULP` | `serde_json_round_trip(v).to_bits() == v.to_bits()` über dieselbe Wertemenge | die Werte `-0.20253906249999998` und `0.013476562499999997` wurden am 2026-09-26 als 1-ULP-verlustbehaftet gemessen; sie bleiben als Regressionskorpus erhalten, nur die Erwartung kehrt sich |
| `the_lossy_rate_over_the_slider_domain_is_measured_not_hidden` → `no_value_in_the_slider_domain_loses_a_bit` | Quote im Band 4–12 %, `lossy > 0` | `lossy == 0` auf demselben splitmix64-Zug desselben Umfangs (50.000) | direkte Umkehrung der gemessenen 7,90 % |
| `a_lossy_mirror_is_reused_one_ulp_away_and_the_rest_is_untouched` → `a_seventeen_digit_mirror_is_reused_bit_identically` | `mirrors[3] == KNOWN_LOSSY[0] ± 1 ULP` | `mirrors[3] == KNOWN_LOSSY[0]` bitidentisch | der Wiederverwendungspfad ist der Produktionspfad des Verlusts |
| `the_reuse_path_converges_after_one_run_and_does_not_accumulate` → `three_runs_write_the_same_literals` | Lauf1→Lauf2 driftet > 0, Lauf2→Lauf3 = 0 | alle drei Läufe schreiben **identische** Literale (0 Drift in beiden Übergängen) | „Fixpunkt ab Lauf 2" wird zu „Fixpunkt ab Lauf 1" |
| `the_rendered_output_stays_byte_stable_while_the_persisted_value_drifts` → `the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move` | PNG bytegleich **bei** driftendem Text | PNG bytegleich **und** Text identisch | die Byte-Stabilität des Renders ist die einzige Hälfte, die unverändert bleibt |

**Kein Golden wird „nachgezogen", um grün zu werden**: jede invertierte
Erwartung ist dieselbe Messung mit umgekehrter Polarität. Die Datei behält ihre
Struktur (dieselbe 40-Mitglieder-Familie, derselbe Seed, derselbe Umfang), damit
die Zahlen von vorher und nachher direkt vergleichbar bleiben. Zusätzlich
behält das Modul die Historie im Doc-Kopf — wer die Umstellung rückgängig macht,
muss die Zahlen von 2026-09-26 widerlegen, nicht nur den Test löschen.

Zwei neue Wächter gegen Vakuosität, weil „alles drei Läufe gleich" sonst auch an
einem Fixture bestehen könnte, das die Grenze verlassen hat:

* `three_runs_write_the_same_literals` zählt, wie viele der 240 Literale
  **mindestens 17 signifikante Dezimalstellen** brauchen, und besteht auf
  `> 0`. Ohne diesen Zweig könnte die Familie degenerieren und der Test würde
  weiterhin grün bleiben.
* `the_rendered_output_stays_byte_stable_...` besteht ebenfalls darauf, dass
  mindestens ein Familienmitglied ein Literal aus der Verlustklasse trägt.

### 4.2 `crates/lumina-cli/src/tests/support/auto_tone.rs` — Doku

Die Modul-Doku sagte, das Fixture sei bewusst so gewählt, dass alle sechs Werte
den Round-Trip exakt überleben („a value like `0.44062500000000004` would come
back one ULP lower"). Diese Einschränkung ist ab jetzt **nicht mehr nötig**; das
Fixture bleibt, weil es ein committeter, lizenzfreier Byte-Golden ist. Der Text
wurde an der Fundstelle korrigiert und der Widerlegungsgrund genannt — nicht
still ersetzt (`DoD.md` §9).

### 4.3 `crates/lumina-sidecar/src/tests/roundtrip.rs` — neue Wächter

Zwei neue Tests, die die **Klasse** sichern statt eines einzelnen Werts
(`DoD.md` §10: ein Wächter, der einen *Weg* abschließt, ist eine
Absichtserklärung):

1. `every_persisted_f64_survives_a_save_load_save_cycle_bit_identically` —
   belegt mit 17-stelligen Werten in **allen** persistierten `f64`-Feldern der
   Task-Formulierung: den sechs `AutoFeatures`-Spiegeln plus
   `matched_exposure`/`target_luminance`, `EditRecipe::adjustments` und den acht
   `LocalAdjustments`-Skalaren (`exposure`, `contrast`, `highlights`,
   `shadows`, `temperature_delta_k`, `tint_delta`, `vibrance`, `saturation`).
   Prüft über `SidecarDocument::to_json` → `from_json` → `to_json`, dass der
   zweite Text gleich dem ersten ist, und dass das geladene Dokument
   bit-identisch (`PartialEq` auf `f64`) zum geschriebenen ist.
2. `a_recipe_built_in_memory_keeps_its_digest_across_the_boundary` — blake3 über
   `serde_json::to_vec` vor und nach dem Schleifendurchlauf; plus
   `LocalAdjustments::digest`, `mask_layers_digest` und
   `MaskStateSnapshot::digest` gegen ein frisch gebautes Vergleichsobjekt.
   Beleg für D5. Die Form ist bewusst **nicht** selbstbezüglich (siehe §6, M3).

**Herkunft der Werte, gemessen statt erfunden — in der Verifikationsrunde
2026-10-12 nachgezählt und an der Fundstelle korrigiert (DoD §9):** zwei
Literale sind die am 2026-09-26 als verlustbehaftet gemessenen, aus der
CLI-Seite übernommen (im Korpus je zweimal belegt). 15 der 20 Einträge sind
Mitglieder desselben deterministischen splitmix64-Zugs (Seed
`0x243f6a8885a308d3`, `unit = (z >> 11) / 2^53`) — 13 davon als `unit * 2 - 1`,
`exposure` (`1.5695229505050552`) als `unit * 20 - 10` und `temperature_delta_k`
(`-2328.6878550005117`) als `unit * 10000 - 5000`: derselbe Zug, je Feld eine
andere lineare Abbildung. Ausgenommen ist `wb_temperature = 10415.777975832698`,
das unter keiner linearen Abbildung ein Mitglied des Zugs ist — dieses eine
Literal wurde von Hand gesetzt. Die frühere Fassung behauptete, **kein** Literal
sei von Hand geraten; diese Aussage ist zurückgenommen, nicht stillschweigend
ersetzt. Was das Korpus trägt, ist die geprüfte Klasseneigenschaft: alle 20
Literale sind der Text, den serde_json eigener Serialisierer schreibt, und
brauchen **genau** 17 signifikante Stellen.

**Eine Korrektur an meiner eigenen ersten Fassung:** die erste Version dieses
Tests enthielt handgesetzte Literale. Clippy wies zwei davon zurück
(`excessive_precision`, `inconsistent_digit_grouping`) — sie waren nicht die
kürzeste Rundreise-Darstellung und damit nicht im Verlustklassen-Format. Die
Literale wurden daraufhin vollständig durch gemessene ersetzt.

### 4.4 Dokumentation

* `feature/architecture/pipeline.md` § „Die JSON-Round-Trip-Grenze der
  `f64`-Felder" → „Der `f64`-JSON-Round-Trip ist exakt". Die Messungen von
  2026-09-26 bleiben als **historisierte** Zahlen erhalten (mit Datum und
  Build), sonst verliert der nächste Leser, woher die Regressionskorpus-Werte
  kommen. Die Absätze „Warum das hier nicht behoben wird" und „Gemessene
  Nebenwirkung auf Byte-Ebene" werden **zurückgenommen**, weil das Feature sie
  erledigt — nicht gelöscht ohne Spur.
* `feature/platform/cli-gui-wasm.md`, Bullet „Byte-Identität (bekannte Grenze,
  2026-09-17)" → „Stand 2026-10-12". Der Zusatz „der serde_json-Load/Save-
  Roundtrip kann fremde Float-Felder um 1 ULP verschieben (ohne
  `float_roundtrip`, vorbestehend)" ist mit der Entscheidung **hinFällig** und
  wird mit Widerlegungsgrund zurückgenommen. Die zugehörige Zeile „Offen: …
  Byte-Roundtrip (s. o.)" wandert von Offen nach Erledigt.
* `crates/lumina-cli/src/auto_tone_cli.rs` Modul-Doku — derselbe Abschnitt,
  gleiche Behandlung inklusive der beiden Korrekturen aus §1.

### 4.5 `Cargo.lock`

**Keine Änderung.** `float_roundtrip = []` ist abhängigkeitsfrei. Das ist eine
Korrektur zur Erwartung im Task-Formular („Cargo.lock-Änderung durch
Feature-Flip ist erwartet") — belegt in §1.

### 4.6 Warum alle 12 Manifests, nicht eins

`serde_json` ist in **jedem** der 12 Kraten einzeln deklariert
(`crates/*/Cargo.toml`, `agent-harness/Cargo.toml`); es gibt im Workspace
**kein** `[workspace.dependencies]`. Ein einzelner Eintrag würde über die
Feature-Unifikation des Cargo-Resolver-V2 nur in den Builds greifen, die diesen
Kraten im Graphen haben — `cargo test -p lumina-sidecar` allein bekäme ein
`serde_json` **ohne** das Feature. Genau diese Fehlerklasse „ein Feature
funktioniert nur in Anwesenheit eines anderen" hat der Workspace in
`scripts/check_cli_features.sh` (CLI-GPU-BUILD-1) schon einmal teuer bezahlt.
Deshalb: in jedem Manifest, mit derselben Zeile und demselben Kommentar.

---

## 5. Benannte Gates (hier nicht ausführbar)

| Gate | Gemessen | Folge für den nächsten Lauf |
|---|---|---|
| `sh scripts/golden_ref.sh check` | **exit 1**, Eintrag in der Verifikationsrunde 2026-10-12 vollständig korrigiert: es sind **9** Abweichungen, nicht 4 (Aufzählung unten). **Verwertbares Teilergebnis:** `goldens.count=66` ist gleich dem Pin, `fixtures.count=4` ebenfalls, und `goldens.digest` wie `fixtures.digest` sind gleich den Zeilen in `scripts/golden_ref.lock` — die Inventare sind unverändert, abweichend ist ausschließlich der Host | auf dem Mac mit Adapter laufen lassen. Erwartung: **grün ohne Neuaufnahme**, weil kein Render-Pfad und kein committeter Byte geändert wird (D2/D3). Sollte es rot sein, ist das ein **Befund**, kein Flake |
| `LUMINA_MATRIX=1 cargo test -p lumina-gui --lib -- --ignored real_matrix_headless` | nicht gelaufen — `expected_route: gpu` ist in `testdata/matrix/recipe-set.v1.json` fest verdrahtet, es gibt keinen Adapter | CPU-Parität des GPU-Pfads ist hier nicht beweisbar (`Agents.headless.md` §3) |
| `LUMINA_MATRIX=1 cargo test -p lumina-cli ... real_matrix_against_committed_goldens` | nicht gelaufen, dito | dito |
| CPU↔GPU-Byte-Parität (`--features gpu-adapter-tests`) | **kompiliert** (`cargo check -p lumina-gpu --features gpu-adapter-tests`, exit 0), **nicht ausgeführt** | Läufe mit Adapter |
| `#[cfg(feature = "gpu")]`-Pfade | **kompiliert** (`cargo check -p lumina-gui --features gpu`, exit 0) | „Kompiliert" gilt als ungeprüft (`Agents.headless.md` §2.1) |
| „kein Anwender-Sidecar auf der Platte ändert sein Digest bei bloßem Laden" | Anwenderdateien sind gitignored und hier nicht vorhanden | Beweislastumkehr: das Feature macht den Round-Trip exakt, ein Load-Save-Zyklus kann kein Digest mehr bewegen. Das ist Test 4.3(1) |
| CI auf diesem Stand | **nicht gelaufen** — der Auftrag verbietet `git add/commit/push`, also existiert kein CI-Ergebnis | benanntes Restrisiko (`DoD.md` §12). Der nächste Arbeitsstrom muss nach dem Push das CI-Ergebnis prüfen |

**Die 9 Abweichungen des `golden_ref.sh check`-Laufes, nachgezählt am
2026-10-12** (der Eintrag oben hatte vier benannt; die restlichen fünf fehlten).
Alle neun sind Host-Fakten, keine Golden-Fakten:

| # | Schlüssel | gepinnt | hier gemessen |
|---|---|---|---|
| 1 | `os.name` | `macOS` | `Linux` |
| 2 | `os.macos` | `27.0.1` | `unavailable` (`sw_vers` existiert nicht) |
| 3 | `os.arch` | `arm64` | `x86_64` |
| 4 | `os.cpu` | `Apple M5 Pro` | `unavailable` (`sysctl` existiert nicht) |
| 5 | `toolchain.rustc` | `1.98.0` | `unavailable` (kein `rustc` auf dem `PATH` der aufrufenden Shell — der Werkzeugkasten liegt hinter `target/headless_env.sh`) |
| 6 | `wgpu.backend` | `all->metal` | `all` (nicht Darwin) |
| 7 | `gpu.adapter` | `Apple M5 Pro` | `unavailable` (`system_profiler` existiert nicht) |
| 8 | `gpu.metal` | `Metal 4` | `unavailable` |
| 9 | `libraw.version` | `0.22.2` | `unavailable` — einzige Abweichung, die **keine** Plattform-Abweichung ist: mit der `PKG_CONFIG_PATH` aus `target/headless_env.sh` meldet derselbe Host `0.22.2`, also genau den Pin. Das Skript wurde ohne diese Umgebung aufgerufen |

Abweichungen 1–8 sind echte Host-Fakten (Linux/x86_64, kein macOS-Werkzeugkasten,
kein Metal). Abweichung 9 ist ein Umgebungsartefakt des Aufrufs — festgehalten,
damit der Mac-Lauf nicht nach einer LibRaw-Differenz sucht, die es nicht gibt.

Gleich dem Pin und damit das verwertbare Teilergebnis: `fixtures.count=4`,
`fixtures.digest=git:e949f6835ed1e4de980934547a995d96e122dba495f97e6a36e9a285226fea4a`,
`goldens.count=66`,
`goldens.digest=git:9cb231fe5c1457e49f882643aa4fc4d79b49f6ecdf6b55c88eaeb7f7ac3a7dcb`
— identisch zu den Zeilen in `scripts/golden_ref.lock`. Das Gate scheitert also
an der Plattform, nicht am Inhalt — genau die Erwartung aus D2/D3, nur steht der
Nachweis dafür jetzt im Plan statt in der Annahme.

---

## 6. Mutationsnachweis (DoD §10)

Mutation M1 ist „Feature aus den Manifests entfernen" — genau die Rücknahme, die
die Umstellung später wiederholt sehen wird. Ausgeführt wurde sie chirurgisch
(`git stash push -- <nur die 12 Cargo.toml-Pfade>`), damit die neuen Tests
selbst **an Ort und Stelle blieben**; ein erstes, gröberes Stash über `crates`
hätte nämlich auch die neuen `.rs`-Dateien zurückgenommen und den Lauf
aussagelos gemacht.

| Mutation | Erwartetes Resultat | Gemessen |
|---|---|---|
| **M1** Feature aus allen 12 Manifests entfernt; dann `cargo test -p lumina-cli --bins tests::auto_tone_float` und `cargo test -p lumina-sidecar --lib tests::roundtrip` | alle umgestellten/neuen Tests rot | **7 von 7 rot**: `a_seventeen_dit...`, `no_value_in_the_slider_domain_loses_a_bit`, `the_emitted_text_and_the_parser_agree_bit_for_bit`, `the_rendered_output_..._does_not_move`, `three_runs_write_the_same_literals`, `every_persisted_f64_...`, `a_recipe_built_in_memory_...`. Zahlen und 1-ULP-Werte in §3.3 |
| **M2** ein Feld aus dem Korpus streichen (`("saturation", …)` entfernt) | Test muss rot werden, weil das Feld unbewacht ist | **Farb-Beweis nicht möglich, stattdessen Strukturbeweis gemessen:** `const LOCAL_LOSS_VALUES: [(&str, f64); 8]` lässt sich nicht stillschweigend auf 7 kürzen — Compile-Fehler `E0308: expected an array with a size of 8, found one with a size of 7`. Das Korpus kann nicht schrumpfen, ohne dass es auffällt. **Ehrlich benannt:** der Verlust eines Feldes aus der *Schleife* wäre farblich nicht nachweisbar, weil der Test seine eigene Kürzung nicht sehen kann. Der belastbare Farb-Beweis ist M1 |
| **M3** Digest-Erwartung selbstbezüglich machen (`assert_eq!(h(reloaded), h(reloaded))`) | muss rot werden | **grün, und das ist der Befund:** unter der Mutation M1 blieb der Test **grün**, während sein Geschwister im selben Lauf rot wurde. Selbstbezügliche Erwartungen können nicht scheitern — genau die Form, die `DoD.md` §10 verbietet. Die ausgelieferte Fassung vergleicht zwei **unabhängig erzeugte** Digests (einmal direkt, einmal über einen vollständigen `save`/`load`-Zyklus) und war unter M1 rot |

Nach jeder Mutation wurden Manifests und Datei aus dem Arbeitsbaum
zurückgesetzt und der Endstand gegen die Sicherung **byteweise** verglichen
(`diff` → identisch), nicht nur per `git status`. Der Workspace steht nach dem
letzten Verify-Lauf wieder auf 284 / 562 / 13 / 58 / 989 / 369, überall 0
Fehler.

---

## 7. Abweichungen von der Abnahme — offen benannt

1. **`Cargo.lock`-**Erwartung aus dem Auftrag ist nicht eingetreten** und zwar
   nach Messung, nicht als Weglassung: `float_roundtrip = []` hat keine
   Abhängigkeiten, `git diff Cargo.lock` ist leer. Kein Golden, kein Digest und
   keine Datei im Tree hängt daran.
2. **Der Auftrag erwartete „betroffene Goldens neu aufnehmen"**. Gemessen gibt
   es **keine** betroffenen Goldens im Repository (§2.3): 0 von 5236
   Float-Literalen der committeten JSON-Dateien, 0 von 22 PNG-Goldens im
   Verzeichnis `testdata/matrix/golden/` (nachgezählt: 0 von allen 114
   getrackten PNGs des Workspace), 0 Digest-Literale im Code. Neu aufgenommen
   wurde deshalb nichts. Die einzigen Umstellungen sind die fünf
   Test-Erwartungen, die den **Verlust** gepinnt hatten (§4.1) — jede davon
   einzeln begründet und unter M1 rot geworden.
3. **Die Auto-Tone-Spiegel der Anwender sind die tatsächlich betroffene
   Klasse**, und sie liegen nicht im Repository (`.lumina.json` ist gitignored).
   Ihr Schutz ist Test 4.3(1), der genau diese Felder über den
   `save`/`load`-Zyklus treibt. Ein Nachweis an einer echten Anwenderdatei ist
   hier strukturell nicht möglich und wird nicht behauptet.
4. **M2 ist kein Farb-Beweis.** Benannt in §6 statt als Deckung geführt.
5. **CI fehlt** (Auftragsverbot: kein `git add/commit/push`). Benanntes
   Restrisiko gemäß `DoD.md` §12.
6. **`golden_ref.sh check` exit 1** hier, aus Plattformgründen, nicht wegen
   dieser Änderung. Benanntes Gate für den Mac-Lauf (§5).
