# SOLL — DOWNScaler-UNIFY-71 (Ein Downscale-Pfad)

**Scope:** Konsolidierung der drei Downscale-Implementierungen (plus einem
redundanten Wrapper) auf **einen** Pfad in `lumina-core`. Heimat ist
`lumina-core` (keine GUI-/Plattform-Abhängigkeit). Entscheid
`DOWNSCALE-EIN-PFAD` (`Agents.todo.md:43`): Qualität vor Geschwindigkeit,
GPU-fähig gedacht, aber GPU-Implementierung ist **nicht** Teil dieses Tasks.

**Belegbasis (alles unten ist gelesen, nichts spekulativ):**
`crates/lumina-core/src/lib.rs` (`downscale_bilinear` :181-230,
`ImageFrame::new` :395-414, `ImageFrame::downscale` :480-538),
`crates/lumina-gui/src/filmstrip.rs` (`downscale_rgba` :266-291,
`THUMBNAIL_MAX_DIM` :27), `crates/lumina-gui/src/preview_jobs.rs`
(`downscale_to_target` :241-250), `crates/lumina-gui/src/thumb_worker.rs`
(:166-173), `crates/lumina-gui/src/preview_size.rs` (:158-209),
`crates/lumina-gui/src/navigator.rs` (:118-131), `crates/lumina-gui/src/lib.rs`
(:7087-7090), `crates/lumina-gui/src/preview_masks.rs` (:215-253),
`crates/lumina-gui/src/matrix.rs` (:323-332), `crates/lumina-gui/src/matrix/render.rs`
(:146-151), `crates/lumina-gui/src/cull_gui.rs` (:173-175),
`crates/lumina-gui/src/face_gui.rs` (:351-373), `crates/lumina-gui/src/tests/histogram.rs`
(:333-383, :415-469), `crates/lumina-gui/src/tests/sliders_filmstrip.rs`
(:198-209), `crates/lumina-cli/src/main.rs` (:7389-7403),
`crates/lumina-mcp/src/tools/preview.rs` (:36-55),
`crates/lumina-mcp/tests/mcp.rs` (:163-180), `crates/lumina-cull/src/analyze.rs`
(:178-195), `crates/lumina-stages/src/matrix_run.rs` (:298-307),
`crates/lumina-core/src/tests/image_io.rs` (:3-48),
`feature/quality/golden-fixtures.md` (Inventar Version 2, 66 Goldens),
`feature/quality/golden-references.md` (GOLDEN-REF-30),
`Agents.todo.md` (:43, :478).

## 1. Ziel

- Ein Downscale-Pfad für den gesamten Workspace: **eine** Implementierung in
  `lumina-core`, keine GUI-Implementierung bleibt. Die drei aktuellen
  Resampler (`downscale_bilinear`, `ImageFrame::downscale`,
  `filmstrip::downscale_rgba`) und der Wrapper `downscale_to_target` werden
  auf diesen einen Pfad konsolidiert.
- **Genau ein Resampling-Kernel**: die f64-bilineare Schleife aus
  `downscale_bilinear` (`crates/lumina-core/src/lib.rs` → `downscale`) bleibt **arithmetisch unverändert**
  (f64-Interpolation, `.round().clamp(0.0,255.0)`). Die beiden Zielformen
  (`MaxWidth`, `LongEdge`) unterscheiden sich **nur** in der
  Geometrie-Funktion davor. Kein zweiter Schleifendurchlauf, keine
  Nearest-Nachbildung.
- Das Ergebnis wird über **`ImageFrame::new`** gebaut
  (`crates/lumina-core/src/lib.rs` → `ImageFrame::new`); der (beweisbar unmögliche) Fehler wird **weitergegeben,
  nicht geschluckt** — Begründung und Formulierung aus `crates/lumina-core/src/lib.rs` → `downscale`
  übernehmen.

## 2. SOLL-API (Vertrag)

```rust
/// Der eine Downscale-Pfad des Workspace.
pub fn downscale(frame: &ImageFrame, target: DownscaleTarget) -> Result<ImageFrame, CoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownscaleTarget {
    /// Laengste Kante hoechstens `max_dim` (Seitenverhaeltnis erhalten).
    LongEdge(u32),
    /// Breite hoechstens `max_width` (Seitenverhaeltnis erhalten).
    MaxWidth(u32),
}
```

**Begruendung aus `crates/lumina-core/src/lib.rs` → `downscale`:** Die Funktion gibt `Result` zurück
(statt zu panikieren), weil der fallible `ImageFrame::new`-Vertrag
**weitergegeben, nicht geschluckt** wird. Der Fehler ist für die exakte
Allokation `new_width * new_height * 4` beweisbar unmöglich, aber das
Contract-Design von `ImageFrame::new` wird nicht umgangen.

**`ImageFrame::new` bleibt der einzige oeffentliche Konstruktor**
(`crates/lumina-core/src/lib.rs` → `ImageFrame::new`). Das Struktliteral in `ImageFrame::downscale`
(`crates/lumina-core/src/lib.rs` → `ImageFrame::downscale`) entfaellt; das Ergebnis laeuft ab sofort durch dieselbe
gepruefte Fabrik wie jeder andere Frame.

## 3. Der eine Resampling-Kernel

Die bilinearen f64-Ausgabe-Schleife (`crates/lumina-core/src/lib.rs` → `downscale`) ist der einzige Kernel.
Sie wird aus `downscale_bilinear` uebernommen und bleibt **Byte-identisch**
in der Arithmetik:

- `source_y = (y + 0.5) * height / new_height - 0.5`
- `y0 = source_y.floor().max(0.0)`, `y1 = (y0+1).min(height-1)`,
  `ty = (source_y - y0).clamp(0.0, 1.0)`
- Interpolation `top + (bottom - top) * ty` pro Kanal, dann
  `.round().clamp(0.0, 255.0) as u8`

**Kein zweiter Schleifendurchlauf.** `LongEdge` fuehrt dieselbe Schleife mit
einer anderen Geometrie-Funktion aus; `MaxWidth` fuehrt dieselbe Schleife
mit der Geometrie-Funktion aus, die `downscale_bilinear` heute schon hat.

## 4. Geometrie-Funktionen (nur der Unterschied)

| Zielform | Geometrie | Herkunft |
| --- | --- | --- |
| `MaxWidth(w)` | `new_width = width.min(w).max(1)`; `new_height = ((new_width as f64 / width as f64) * height as f64).round().max(1.0) as u32` | `crates/lumina-core/src/lib.rs` → `downscale` (unveraendert) |
| `LongEdge(d)` | `long = max(width, height)`; `scale = d as f64 / long as f64`; `new_width = (width as f64 * scale).round().max(1.0) as u32`; `new_height = (height as f64 * scale).round().max(1.0) as u32` | f64-Fassung der bisherigen f32-Rechnung (`crates/lumina-core/src/lib.rs` → `ImageFrame::downscale`) |

**Ganzzahl-Fallstrick — die `as f64`-Casts sind Teil des Byte-Ankers, nicht
Kosmetik:** In Rust ist `new_width / width` mit `u32`-Operanden
**Ganzzahldivision** (abrundend, *bevor* `* height` greift). Ohne die Casts
liefe die `MaxWidth`-Formel auf `new_height = 0` und damit auf eine falsche
Geometrie; `scale = d / long` waere ebenfalls `0`. Die Casts sind also genau
der Unterschied zwischen „gleiche Bytes wie `downscale_bilinear`" und
„falsch", und dasselbe gilt fuer `scale = d as f64 / long as f64`.

**`MaxWidth` reproduziert `downscale_bilinear` byte-identisch** (gleiche
Geometrie inkl. der Early-Returns bei `width==0||height==0` und bei
`new == old`, `crates/lumina-core/src/lib.rs` → `downscale`). Das ist der **Byte-Anker** des Tasks.

**`LongEdge` ist die f64-Fassung** der bisherigen f32-Rechnung
(`crates/lumina-core/src/lib.rs` → `ImageFrame::downscale`). Die Umstellung f32->f64 ist eine **bewusste, benannte
Aenderung** (besseres Runden, Qualitaet zuerst). Der SOLL verlangt eine
**Messung**, an wie vielen von N repraesentativen Groessen sich die Geometrie
dadurch unterscheidet (Abschnitt 7).

## 5. Nearest-Neighbour entfaellt

`filmstrip::downscale_rgba` (`filmstrip.rs:266-291`) wird **geloescht**.
Die Aufrufer (Filmstreifen-Thumbnails `thumb_worker.rs:168`, Masken-Vorschau
`preview_masks.rs:237`, Tests `crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` → `fit_preview_histogram_matches_thumbnail`/`fit_preview_portrait_histogram_matches_thumbnail` und
`sliders_filmstrip.rs:201,206`) gehen auf den kanonischen Pfad.

**Die Aenderung ist eine Qualitaetsverbesserung** und wird als solche benannt
und belegt (Byte-Unterschied vorher/nachher je Aufrufer +
Histogramm-Abstand vorher/nachher), **nicht** als stiller Neu-Einfrier-Vorgang.

**Reihenfolge `.round()`/`.max(1.0)` — `ImageFrame::downscale` ist massgeblich:**
Die Ex-Nearest-Geometrie in `filmstrip.rs:278-279` rechnet
`(width as f64 * scale).max(1.0).round()`, waehrend `ImageFrame::downscale`
(`crates/lumina-core/src/lib.rs` → `ImageFrame::downscale`) `(self.width as f32 * scale).round().max(1.0)` rechnet —
also `.round()` **vor** `.max(1.0)`. Fuer die Konsolidierung ist die
Reihenfolge von `ImageFrame::downscale` **massgeblich**: der kanonische Pfad
bildet `(… as f64 * scale).round().max(1.0)`. Die Ex-Nearest-Form darf
**nicht** als Referenz kopiert werden. (Fuer nicht-negative Faktoren sind
`.round().max(1.0)` und `.max(1.0).round()` numerisch identisch, weil
`round` monoton und `max(1.0, ·) >= 1.0` ist — der Punkt ist die Form als
Referenz, nicht ein Wertunterschied.)

### 5.1 Histogramm-Paritaet (`crates/lumina-gui/src/tests/histogram.rs`)

`crates/lumina-gui/src/tests/histogram.rs:337,447` vergleicht das
Preview-Histogramm (bilinear, `ImageFrame::downscale`) gegen das
Thumbnail-Histogramm (**Nearest**, `downscale_rgba`) ueber
`histogram_l1(&preview_hist.bins, &thumb_hist.bins)` mit Toleranz
`< 0.35` (`crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` → `fit_preview_histogram_matches_thumbnail`).

**Richtung der Aenderung durch den Wechsel auf bilinear:** Das Thumbnail
bewegt sich bilinear-aekquivalent zur Preview; der L1-Abstand zwischen
Preview- und Thumbnail-Histogramm **verschraenkt sich** (beide Seiten
dieselbe Kernel-Arithmetik). Der Abstand wird **nicht** null (unterschiedliche
Aufloesung, unterschiedliche Groessen), aber die Histogramme werden
aehnlicher, weil beide Seiten dieselbe Interpolation nutzen.

**Messung vorher/nachher:** Der Test `fit_preview_histogram_matches_thumbnail`
bzw. `fit_preview_portrait_histogram_matches_thumbnail` (`crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` → `fit_preview_portrait_histogram_matches_thumbnail`)
muss **vorher** (HEAD) und **nachher** (nach Umstellung) laufen. Der gemessene
`distance`-Wert wird dokumentiert. Erwartung: `distance` sinkt oder bleibt
gleich (niemals groesser), weil beide Seiten dann dieselbe Arithmetik nutzen.

### 5.2 Pin-Tests (sliders_filmstrip.rs)

`crates/lumina-gui/src/tests/sliders_filmstrip.rs:198-209` pinnt:

- `downscale_rgba(&small, 2, 1, 160)` -> `(2, 1)` und **Byte-Gleichheit**
  (`out == small`, :201-203) — der Early-Return bei `longest <= max_dim`
  (`filmstrip.rs:274-276`) bleibt im kanonischen Pfad erhalten
  (`long <= max_dim` -> unveraenderter Klon).
- `downscale_rgba(&big, 320, 200, 160)` -> `(160, 100)` und
  `out2.len() == 4 * 160 * 100` (:205-208) — die Geometrie bleibt unter
  bilinear identisch (`320*0.5=160`, `200*0.5.100`), die **Bytes** werden sich
  jedoch **aendern** (Nearest -> Bilinear).

**Folge fuer den Test:** Der Test muss auf den kanonischen Pfad umgestellt
werden (`downscale(frame, LongEdge(160))`). Die Geometrie-Authoren bleiben
bestehen; die Byte-Pruefung (`out == small`) bleibt fuer den Early-Return-Fall
bestehen. Der Test pinnt danach **nicht** mehr die Nearest-Bytes.

## 6. `downscale_to_target` entfaellt

`preview_jobs.rs:243-250` ist ein duenner, redundanter Wrapper — er nimmt
**keinen `u32`, sondern ein `(u32, u32)`-Tupel**, bildet
`max_edge = target.0.max(target.1)` und hat einen **eigenen Early-Return**
vor der Delegation:

```rust
fn downscale_to_target(frame: &ImageFrame, target: (u32, u32)) -> ImageFrame {
    let long = frame.width().max(frame.height());
    let max_edge = target.0.max(target.1);
    if max_edge == 0 || long <= max_edge {
        return frame.clone();
    }
    frame.downscale(max_edge)
}
```

Er verdoppelt damit die Geometrie- und Guard-Logik der Zielmethode. Er wird
**geloescht**; der Aufruf in `preview_jobs.rs:167` geht direkt auf
`downscale(frame, LongEdge(max_edge))`. **Folge fuer den Implementer:** Beim
Ersetzen bleibt die Tupel-Extraktion `max_edge = target.0.max(target.1)`
erhalten; der eigene Guard (`max_edge == 0 || long <= max_edge`) entfaellt,
weil der kanonische Pfad denselben Guard traegt (`LongEdge`:
`d == 0 || long <= d` -> Klon, `crates/lumina-core/src/lib.rs` → `downscale` / Abschnitt 4).

**Die 5 infalliblen GUI-Aurufer von `ImageFrame::downscale`** (alle nutzen
LongEdge-Semantik):

| Aufrufer | Datei:Zeile |
| --- | --- |
| `capped_preview_source` | `preview_size.rs:177` |
| `refresh_preview_cap` | `preview_size.rs:206` |
| `navigator_overview` (ohne Actions) | `crates/lumina-gui/src/navigator.rs` → `navigator_zoomed_overview` |
| `navigator_overview` (mit Actions) | `crates/lumina-gui/src/navigator.rs` → `navigator_zoomed_overview` |
| `draft_original` | `crates/lumina-gui/src/lib.rs` → `apply_decoded_frame` |

**Die Methode `ImageFrame::downscale` bleibt** als duenne Delegation auf
`downscale(self, LongEdge(max_dim))` mit dokumentiertem, beweisbar
unmoeglichem `expect`. Begruendung:

- `downscale` ist heute **infallibel** (`crates/lumina-core/src/lib.rs` → `ImageFrame::downscale`); sie kann nicht
  scheitern. Der `expect` ist damit heute schon impliziert.
- Die Delegation auf `downscale(self, LongEdge(max_dim))` nutzt denselben
  Kernel, denselben Guard (`long <= max_dim` -> Klon), dieselbe Geometrie.
- Ein `expect` mit dem Text, dass `ImageFrame::new` fuer die exakte
  `width*height*4`-Allokation nicht scheitern kann, ist die ehrliche
  Dokumentation einer Tatsache, die `crates/lumina-core/src/lib.rs` → `downscale` bereits nennt.

**Alternative (Ergebnis-Propagierung an 5 Stellen):** Die Aufrufer selbst
`downscale(self, LongDim(max_dim))?` aufrufen. **Vorteil:** kein `expect`
nimment die Fehlerbehandlung weg. **Nachteil:** die GUI ist heute
infallibel und muesste an 5 Stellen `Result`-Propagation einfuehren; das
wuerde die API-Oberflaeche verbreitern, ohne dass ein realer Fehlerfall
existiert, den je ein Error-Handler tragen wuerde. Die Delegation mit
dokumentiertem `expect` ist die kleinere, ehrlichere Aenderung.

## 7. Messung f32->f64 (LongEdge)

Die Umstellung der Geometrie von f32 (`crates/lumina-core/src/lib.rs` → `ImageFrame::downscale`) auf f64 ist eine
bewusste Aenderung. Der SOLL verlangt eine **Messung**, an wie vielen von N
repraesentativen Groessen sich die Geometrie dadurch unterscheidet.

**Verfahren:** Ueber N repraesentative `(width, height)`-Paare (z. B. alle
Kombinationen aus `{1..=4096}` in Schritten plus die beiden CR3-Dimensionen
`6032x4024` und `4024x6032`) wird die Geometrie einmal mit f32 (`scale =
max_dim as f32 / long as f32`, `round`) und einmal mit f64 (`scale =
max_dim as f64 / long as f64`, `round`) berechnet. Gezaehlt wird, an wie
vielen Paaren `(new_width, new_height)` uebereinstimmen.

**Erwartung:** Die meisten Paare stimmen ueberein (f32 und f64 rundeten
identisch, solange die Werte in f32 repraesentierbar sind). An paarigen
Paaren (z. B. wenn `max_dim / long` nahe an `.5` liegt und f36-Umlauf den
Rundungsmodus beeinflusst) kann es zu 1px-Abweichungen kommen. Diese Zahl
wird dokumentiert und ist die **benannte Aenderung** (Qualitaet zuerst).

## 8. Invarianten

1. **Determinismus:** Gleiche Eingabe -> gleiche Bytes, keine
   parallelitaetsabhaengige Rundung. `downscale_bilinear` ist heute bereits
   deterministisch (`crates/lumina-core/src/lib.rs` → `downscale`: "fully deterministic (pure math, no
   randomness)"); der kanonische Pfad erbt diese Eigenschaft.
2. **Originale unveraendert:** `downscale` nimmt `&ImageFrame` und gibt ein
   neues `ImageFrame` zurueck; das Original wird nie geschrieben.
3. **Keine Rendering-, Rezept- oder Schema-Aenderung:** Der Task betrifft nur
   die Downscale-Geometrie und -Ke rnel. Rezept-Schema, Sidecar-Schema,
   Render-Pipeline und Goldens (ausser dem benannten Gate in Abschnitt 9)
   bleiben unangetastet.
4. **`ImageFrame::new` bleibt der einzige oeffentliche Konstruktor**
   (`crates/lumina-core/src/lib.rs` → `ImageFrame::new`). Das Struktliteral in `crates/lumina-core/src/lib.rs` → `ImageFrame::downscale` entfaellt.
5. **Keine neue Abhaengigkeit:** `lumina-core` bleibt ohne GUI- oder
   Plattform-Importe (Entscheid `DOWNSCALE-EIN-PFAD`, `Agents.todo.md:43`).
6. **Kein stiller Fallback:** `ImageFrame::downscale` delegiert auf den
   kanonischen Pfad und dokumentiert den `expect`; es gibt keinen
   stillen Nearest-Fallback, keinen zweiten Resampler, keine
   `unwrap`-Kruecke in Produktion.
7. **Kein Bildqualitaets-Regress ohne benannte Entscheidung:** Jede
   Byte-Aenderung ist in Abschnitt 10 (Aufrufer-Tabelle) und Abschnitt 11
   (Goldens-Gate) benannt und belegt.

## 9. Goldens-Gate

**Inventar:** `feature/quality/golden-fixtures.md` (Inventar-Version 2)
fuehrt **66** committete Goldens (gezaehlt mit `git ls-files`, normativ
dort :177-179). **Achtung:** Die Aufgabenstellung nennt **62** Goldens; die
normative Quelle (`golden-fixtures.md`) nennt **66**. Dieser SOLL verwendet
die belegte Zahl **66** aus dem Inventar.

**Klassifikation (golden-fixtures.md:154-169):**

- **R — Render-Invariante:** echter Bildinhalt aus echter Pipeline.
- **C — Chrome-/Layout-Invariante:** nur UI-Chrome und Layout.
- **R-F:** verboten (Render-Invariante mit Decode-Fehler als Soll-Inhalt).

**Betroffene Goldens (durch Nearest->Bilinear bytesveraendert):**

| Golden | Inventar-Zeile | Klasse | Betroffen weil |
| --- | --- | --- | --- |
| `library_compare.png` | 30 | R | "echte CR3-Thumbnails in Before/After + Filmstreifen" |
| `library_loupe.png` | 32 | R | "echte CR3-Thumbnails im Loupe" |
| `library_rated_badges.png` | 40 | R | "echte CR3-Thumbnails + Rating-/Flag-/Label-Badges" |
| `library_subfolder_badges.png` | 41 | R | "echte CR3-Thumbnails + relative Pfad-Badges" |
| `library_survey.png` | 42 | R | "drei echte CR3-Thumbnails (Multi-Selection)" |
| `library_stack_membership.png` | 49 | R | "echte CR3-Thumbnails + amber Stapel-Zeichen" |

**Grund:** Die Filmstreifen-Thumbnails werden vom Produktions-Worker
`thumb_worker.rs:168` ueber `downscale_rgba` (**Nearest**) erzeugt. Der
Wechsel auf bilinear aendert diese Thumbnails und damit alle Goldens, die
diese Thumbnails zeigen.

**Nicht betroffene Goldens:**

- Alle **C**-Goldens (52 Stueck): kein echter Bildinhalt, kein
  Downscale-bezogener Inhalt.
- Die **Parity**-Goldens (`parity_paths_*`, Zeilen 55-62): synthetische
  S2-Szenarien, kein Filmstreifen-/Thumbnail-Inhalt.
- Die **Develop**-Goldens (Zeilen 1-20): nutzen `sample_image_png()` (4x3),
  die **hoch**skaliert wird — der Downscale-Pfad skaliert nie hoch
  (`crates/lumina-core/src/lib.rs` → `downscale`: `width.min(max_width)`).

**Regenerations-Hardware:** Diese Goldens lassen sich **nur** auf der
gepinnten macOS/Metal-Maschine neu erzeugen (Referenz: macOS 27.0, Apple
M5 Pro, Metal 4, wgpu 30.0.1, egui 0.36.2, LibRaw 0.22.2;
`golden-references.md:10-12`, `golden_ref.lock`). Auf diesem Host (Linux)
koennen sie **nicht** neu aufgenommen werden.

**Das Gate ist ein benanntes, eigentuemer-sichtbares Gate:** Der SOLL
behauptet **nicht**, dass die Goldens unveraendert bleiben. Stattdessen:
die 6 R-Goldens sind **bewusst betroffen** und duerfen nur auf der
Referenzplattform neu aufgenommen werden. Der Impl-Schritt dokumentiert
diese Aenderung als **Qualitaetsverbesserung** (Nearest -> Bilinear) und
verweist auf die Golden-Neuaufnahme als Folge auf der Referenzplattform.

## 10. Aufrufer-Tabelle

### 10.1 MaxWidth-Aufrufer (downscale_bilinear)

| Datei:Zeile | Zielsemantik | Byte-Gleich? | Fehlerbehandlung heute |
| --- | --- | --- | --- |
| `lumina-cli/src/main.rs:7393` | MaxWidth | **ja** (Byte-Anker) | `?` -> `CliError` |
| `lumina-mcp/src/tools/preview.rs:51` | MaxWidth | **ja** (Byte-Anker) | `map_err(map_core_error)` -> `McpError` |
| `lumina-cull/src/analyze.rs:189` | MaxWidth | **ja** (Byte-Anker) | `?` -> `CullError` |
| `lumina-stages/src/matrix_run.rs:298` | MaxWidth | **ja** (Golden-Vergleich) | `match` -> Outcome error |
| `lumina-gui/src/matrix.rs:323` | MaxWidth | **ja** (Golden-Vergleich) | `match` -> report error |
| `lumina-gui/src/matrix/render.rs:146` | MaxWidth | **ja** (Test, Golden) | `unwrap` |
| `lumina-gui/src/cull_gui.rs:174` | MaxWidth | **ja** (Byte-Anker) | `.ok()?` -> `None` |
| `lumina-gui/src/face_gui.rs:372` | MaxWidth | **ja** (Byte-Anker) | `.ok()` -> `None` |
| `lumina-core/src/tests/image_io.rs:11,15,19,23,38` | MaxWidth | **ja** (Byte-Anker, Test) | `unwrap` |
| `lumina-mcp/tests/mcp.rs:166,174,178` | MaxWidth | **ja** (Byte-Anker, Test) | `unwrap` |

**Byte-Verdikt:** Alle MaxWidth-Aufrufer **muessen byte-gleich bleiben**.
Sie sind der Byte-Anker des Tasks: gleiche Geometrie, gleiche Arithmetik,
gleiche Early-Returns.

### 10.2 LongEdge-Aufrufer (ImageFrame::downscale, f32)

| Datei:Zeile | Zielsemantik | Byte-Gleich? | Fehlerbehandlung heute |
| --- | --- | --- | --- |
| `preview_size.rs:177` | LongEdge | **nein** (f32->f64) | infallibel |
| `preview_size.rs:206` | LongEdge | **nein** (f32->f64) | infallibel |
| `crates/lumina-gui/src/navigator.rs` → `navigator_zoomed_overview` | LongEdge | **nein** (f32->f64) | infallibel |
| `crates/lumina-gui/src/navigator.rs` → `navigator_zoomed_overview` | LongEdge | **nein** (f32->f64) | infallibel |
| `crates/lumina-gui/src/lib.rs` → `apply_decoded_frame` | LongEdge | **nein** (f32->f64) | infallibel |
| `preview_jobs.rs:167` (via `downscale_to_target`) | LongEdge | **nein** (f32->f64) | infallibel |

**Byte-Verdikt:** Diese Aufrufer **duerfen sich aendern** (f32->f64). Die
Aenderung ist benannt und belegt (Abschnitt 7). Die Fehlerbehandlung bleibt
infallibel (Delegation mit `expect`).

### 10.3 LongEdge-Nearest-Aufrufer (downscale_rgba)

| Datei:Zeile | ZielSemantik | Byte-Gleich? | Fehlerbehandlung heute |
| --- | --- | --- | --- |
| `thumb_worker.rs:168` | LongEdge + Nearest | **nein** (Nearest->Bilinear) | `map_err` -> Worker error |
| `preview_masks.rs:237` | LongEdge + Nearest | **nein** (Nearest->Bilinear) | `unwrap` (:243) |
| `crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` → `fit_preview_histogram_matches_thumbnail` | LongEdge + Nearest (Test) | **nein** (Nearest->Bilinear) | `unwrap` |
| `crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` → `fit_preview_portrait_histogram_matches_thumbnail` | LongEdge + Nearest (Test) | **nein** (Nearest->Bilinear) | `unwrap` |
| `sliders_filmstrip.rs:201` | LongEdge + Nearest (Test) | **ja** fuer 2,1 (Early-Return) | `unwrap` |
| `sliders_filmstrip.rs:206` | LongEdge + Nearest (Test) | **nein** (Nearest->Bilinear) | `unwrap` |

**Byte-Verdikt:** Alle Nearest-Aufrufer **duerfen sich aendern**
(Nearest->Bilinear). Die Aenderung ist eine Qualitaetsverbesserung und wird
als solche benannt und belegt. `sliders_filmstrip.rs:201` ist die Ausnahme:
der Early-Return-Fall (2,1 -> 2,1) bleibt byte-gleich, weil der kanonische
Pfad denselben Early-Return hat (`long <= max_dim` -> Klon).

## 11. Byte-Beweis-Plan

**Methode (nicht nur Absicht):**

1. **Byte-gleich bleiben (alle MaxWidth-Aufrufer):** Ein deterministischer
   synthetischer Frame + Fixture, der durch `downscale(frame, MaxWidth(w))`
   laeuft, muss **byte-identisch** mit dem aktuellen
   `downscale_bilinear(frame, w)` sein. SHA-256 der Ergebnis-Pixels vorher
   (aus HEAD) und nachher (nach Umstellung) sind identisch. Beleg: Test
   in `lumina-core` (der `image_io.rs`-Test ist bereits der Anker).
2. **Bewusst aendern (alle LongEdge-Aufrufer durch f32->f64):** Ein
   Test misst die Geometrie-Aenderung ueber N repraesentative Groessen und
   dokumentiert die Zahl. Die Pixel-Aenderung ist benannt (Qualitaet
   zuerst).
3. **Bewusst aendern (alle Ex-Nearest-Aufrufer durch bilinear):** Ein
   Test dokumentiert den Byte-Unterschied vorher/nachher je Aufrufer.
   Der Histogramm-Abstand (Abschnitt 5.1) wird vorher/nachher gemessen.
4. **Eingefrorene Golden-freie Testfixture:** Ein Test, der den
   `MaxWidth`-Byte-Anker dauerhaft pint (synthetischer Frame mit bekannten
   SHA-256), damit keine zukuenftige "Optimierung" den Kernel veraendert,
   ohne dass ein Test rot wird.

**Verfahren:** Deterministischer synthetischer Frame (z. B. 64x48 mit
bekanntem Gradienten) + Fixture, SHA-256 der Ergebnis-Pixels vorher aus HEAD
und nachher, plus eingefrorene Testfixture, die den `MaxWidth`-Byte-Anker
dauerhaft pint.

## 12. Testmatrix (Impl-Schritt, headless)

| ID | Art | Ort (SOLL) | Prueft | Fixture |
| --- | --- | --- | --- | --- |
| T1 | U | `lumina-core` (neu) | `downscale(frame, MaxWidth(w))` ist **byte-identisch** mit `downscale_bilinear(frame, w)` (SHA-256 vorher/nachher) | synthetischer 64x48-Gradient |
| T2 | U | `lumina-core` (neu) | Genau **ein** Resampler existiert: `downscale_bilinear`, `ImageFrame::downscale` (nur als Delegation), `downscale_rgba`, `downscale_to_target` sind **nicht** mehr als Resampler vorhanden (Quelltext-Gate oder Test, dass die drei Altnamen nicht mehr existieren) | keine |
| T3 | U | `lumina-core` (neu) | Geometrie-Test `LongEdge`: `downscale(frame, LongEdge(d))` liefert `(new_width, new_height)` mit `max(new_width, new_height) == d` (Seitenverhaeltnis erhalten) | synthetische Paare |
| T4 | U | `lumina-core` (neu) | Geometrie-Test `MaxWidth`: `downscale(frame, MaxWidth(w))` liefert `(new_width, new_height)` mit `new_width == min(width, w)` | synthetische Paare |
| T5 | U | `lumina-core` (neu) | Messung f32->f64: an wie vielen von N repraesentativen Groessen sich die Geometrie unterscheidet (Abschnitt 7) | N Paare |
| T6 | P | `lumina-gui` (neu) | Histogramm-Paritaet: `fit_preview_histogram_matches_thumbnail` und `fit_preview_portrait_histogram_matches_thumbnail` laufen vorher (HEAD) und nachher; `distance`-Wert wird dokumentiert (erwartet: sinkt oder bleibt gleich) | synthetisches 64x40- bzw. 40x64-PNG |
| T7 | U | `lumina-gui` (neu) | `filmstrip_downscale_keeps_small_images_and_shrinks_large` (aus `sliders_filmstrip.rs:198`) umgestellt auf kanonischen Pfad; Geometrie (160,100) und Early-Return-Byte-Gleichheit bleiben bestehen | synthetische Buffer |
| T8 | U | `lumina-gui` (neu) | Byte-Beweis vorher/nachher je Aufrufer (Abschnitt 11) dokumentiert | synthetische Frames |
| T9 | U | `lumina-core` (neu) | Determinismus: gleiche Eingabe -> gleiche Bytes (bereits in `image_io.rs:19-20` gepinnt, bleibt bestehen) | synthetischer Frame |

**Kein `#[ignore]`, keine `should_panic`-Kruecke.** Alle Tests laufen
headless.

## 13. Akzeptanz

- Genau **eine** Implementierung: `downscale(frame, DownscaleTarget)` in
  `lumina-core`. `downscale_bilinear` ist entfernt (oder nur noch als
  `pub use`-Alias, falls noetig); `ImageFrame::downscale` ist eine duenne
  Delegation; `downscale_rgba` und `downscale_to_target` sind geloescht.
- Alle MaxWidth-Aufrufer sind byte-gleich (T1 gruen).
- Alle LongEdge-Aufrufer sind umgestellt; die f32->f64-Aenderung ist
  benannt und gemessen (T5 gruen).
- Alle Nearest-Aufrufer sind umgestellt; die Qualitaetsverbesserung ist
  benannt (T7, T8 gruen).
- Histogramm-Paritaet: `distance` sinkt oder bleibt gleich (T6 dokumentiert).
- Headless-Tests gruen: `cargo test -p lumina-core`, `cargo test -p lumina-mcp`,
  `cargo test -p lumina-cull`, `cargo test -p lumina-gui --test histogram`.
- `cargo fmt --all -- --check` gruen.

## 14. Roadmap (SOLL -> Impl -> Verify)

1. **SOLL (dieses Doc):** Review/Sign-off durch den Eigentuemer.
2. **Impl-A — Kernel-Extraktion:** Die f64-bilineare Schleife aus
   `downscale_bilinear` (`crates/lumina-core/src/lib.rs` → `downscale`) wird als gemeinsamer Kern
   in `downscale(frame, DownscaleTarget)` extrahiert. Die Geometrie-Funktionen
   (Abschnitt 4) werden davor geschaltet.
3. **Impl-B — API-Einfuehrung:** `downscale` + `DownscaleTarget` werden in
   `lumina-core` eingefuehrt. `downscale_bilinear` wird entfernt (oder als
   `pub use downscale(frame, MaxWidth(w))`-Alias gefuehrt, falls noetig).
4. **Impl-C — Aufrufer-Umstellung:** Alle Aufrufer in Abschnitt 10 werden
   umgestellt. MaxWidth-Aufrufer: `downscale(frame, MaxWidth(w))`.
   LongEdge-Aufrufer: `downscale(frame, LongEdge(d))` oder Delegation
   `ImageFrame::downscale`. Nearest-Aufrufer: `downscale(frame, LongEdge(d))`.
5. **Impl-D — `ImageFrame::downscale`-Delegation:** Die Methode wird als
   duenne Delegation auf `downscale(self, LongEdge(max_dim))` mit
   dokumentiertem `expect` gefuehrt (Abschnitt 6).
6. **Impl-E — Tests:** Testmatrix (Abschnitt 12) wird umgesetzt. Byte-Beweis
   vorher/nachher wird dokumentiert.
7. **Verify:** `cargo test -p lumina-core`, `cargo test -p lumina-mcp`,
   `cargo test -p lumina-cull`, `cargo test -p lumina-gui --test histogram`
   und `cargo test -p lumina-gui --test sliders_filmstrip` laufen headless.
   Verifikationsbericht (Spez->Test-Mapping) gem. `DoD.md` 7.
8. **Golden-Folge (separat):** Die 6 betroffenen R-Goldens
   (`library_compare`, `library_loupe`, `library_rated_badges`,
   `library_subfolder_badges`, `library_survey`, `library_stack_membership`)
   werden auf der gepinnten macOS/Metal-Referenzplattform neu aufgenommen.
   Dies ist ein **benannter, eigentuemer-sichtbarer Schritt** und Teil des
   Golden-Rhythmus (`GOLDEN-RHYTHMUS`, `Agents.todo.md:36`).

### 14.1 Folge-Dokumente (der Impl-Schritt muss nachpruefen)

Die Umstellung beruehrt nicht nur Code, sondern drei committete Dokumente,
die eine Downscale-/Thumbnail-/Preview-Cap-Semantik pinnen. Der Impl-Schritt
prueft sie gegen den neuen Pfad und aktualisiert sie, wenn sie Dimensionen,
den Funktionsnamen oder den Cap-Vertrag festhalten.

1. **`feature/quality/preview-cache.md:375`** (A6, OneToOne verdrahtet) —
   pinnt woertlich: „Worker downscaled nicht bei `PreviewKind::OneToOne`
   (volles Frame bewahrt); `LuminaApp::schedule_neighbor_previews` waehlt
   Kind/Target aus `zoom_mode` (`OneToOne→(0,0)` sonst `Screen→draft_max_dim`)".
   Die f32→f64-Aenderung von `ImageFrame::downscale` kann die exakten
   `(new_width, new_height)` des gecappten Drafts aendern
   (`preview_size.rs:177,206`). **Nachzupruefen:** ob das Dokument
   Dimensionen oder einen Cap-Vertrag festhaelt, der sich aendert; ggf.
   aktualisieren.
2. **`feature/platform/cli-gui-wasm.md:2839-2900`** (R3-RENDER-SIZE-1) —
   beschreibt das Preview-Cap-System; `PreviewCapState`/`refresh_preview_cap`/
   `capped_source_max_dim` stehen in der Umsetzung bei :2884-2889, die
   Cap-Formel bei :2843-2850. **Nachzupruefen:** ob die Cap-Formel bzw. die
   Draft-Dimensionen den f32→f64-Umstand beruecksichtigen.
   *(Hinweis zur Zitation: Der Verifikator nannte `cli-gui-wasm.md:2878`;
   dort steht die Masken-Notiz (`resample_plane_bilinear`), die
   Cap-Systembeschreibung liegt bei :2839-2900.)*
3. **`feature/quality/conflicts-and-acceptance.md:203-204`** — pinnt die
   Golden-Vergleichs-Normalisierung mit dem exakten Funktionsnamen:
   „ein deterministisch bilinear herunterskaliertes Bild (`comparison_width =
   384`, `lumina-core::downscale_bilinear`, rein mathematisch, kein Zufall)".
   **Aenderung noetig:** wenn `downscale_bilinear` durch
   `downscale(frame, MaxWidth(384))` ersetzt wird, muss dieser Funktionsname
   dort aktualisiert werden — sonst verweist das Dokument auf einen Namen, den
   es nicht mehr gibt.

**Viertens, als Audit-Referenz:** `feature/quality/test-audit-2026-10-12.md:51-56,158`
fuehrt `lumina_core::downscale_bilinear` und den doppelten mcp-Test
(`mcp.rs:163`) als Befund (B1, M-B). Nach der Konsolidierung ist zu pruefen,
ob der Audit-Befund (Doppelabdeckung durch denselben Kernel) noch zutrifft
und der Funktionsname dort angepasst werden muss.

## 15. Abgrenzung

- **Keine GPU-Implementierung:** Der Task dokumentiert die Naht
  (`lumina-core` ist GPU-faehig; die Schleife ist reine CPU-Arithmetik), die
  GPU-Implementierung ist **nicht** Teil dieses Tasks (Entscheid
  `DOWNSCALE-EIN-PFAD`, `Agents.todo.md:43`).
- **Keine Geschwindigkeitsoptimierung:** Der Task konsolidiert und
  verbessert die Qualitaet (Nearest->Bilinear, f32->f64). Jede
  Geschwindigkeitsmessung ist **nicht** Teil des Tasks.
- **Keine neuen Zielformen:** Nur `MaxWidth` und `LongEdge`. Keine
  `MaxHeight`, keine `Area`, keine `Exact`.
- **Keine Rezept-/Sidecar-/Schema-Aenderung:** Die Aenderung betrifft nur
  die Downscale-Geometrie und -Kernel.
- **Kein stiller Fallback:** Jede Byte-Aenderung ist benannt und belegt.
  Keine `unwrap`-Kruecke in Produktion, kein zweiter Resampler.
- **Goldens:** Die 6 betroffenen R-Goldens sind **bewusst betroffen** und
  werden auf der Referenzplattform neu aufgenommen. Der SOLL behauptet
  **nicht**, dass die Goldens unveraendert bleiben.

## 16. Offene Fragen an den Eigentuemer

**Keine.** Der Eigentuemer hat fuer diese Welle "ohne Zwischenabnahme"
entschieden. Der SOLL ist vollstaendig aus der Belegbasis abgeleitet; keine
Design-Entscheidung ist offen. Die einzige Zahl, die nicht vollstaendig
belegt ist, ist die Anzahl der Goldens (62 in der Aufgabenstellung vs. 66
im Inventar); dieser SOLL verwendet die belegte Zahl **66** aus
`golden-fixtures.md:177-179`.

## 17. Nachtrag 2026-10-19 — PREVIEW-THUMB-PARITY-VACUOUS-76

**Kein Eingriff in die normativen Abschnitte** — dieser Nachtrag haelt nur fest,
wann und wie die Histogramm-Paritaet aus § 5.1 **ueberhaupt faehig** wurde.
Die beiden Tests liegen heute in
`crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs` (die Datei
`crates/lumina-gui/src/tests/histogram.rs` fuehrt fuer diese beiden Tests nur
noch die Modul-Verdrahtung und behaelt ihre sieben eigenen Tests); die Fixture
ist **256x160** bzw. **160x256** (lange Kante
256 > `THUMBNAIL_MAX_DIM` = 200, also ein **echter** Resample), die Schranken
sind **L1 < 0.05** und **Mean-Luecke < 0.15** Luminanzstufen. Der alte
Messplan aus § 5.1 und § 12 (T6: synthetisches 64x40- bzw. 40x64-PNG,
`distance` < 0.35) hat das **Vakuum** geliefert: die lange Kante 64 lag unter
der Kappe, `downscale` nahm den Early-Return-Klon, der Abstand war **exakt 0**
und die Kernel-Mutation `.round()` -> `.floor()` blieb gruen. Die
**normativen** Ziele dieses SOLL bleiben unveraendert gueltig und sind mit der
neuen Fixture erstmals **tatsaechlich erfuellt** (§ 5.1: „Der Abstand wird
**nicht** null").
