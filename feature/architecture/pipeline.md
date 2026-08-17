# Nicht-destruktive Renderpipeline

**Features:** F-003 Nicht-destruktive Entwicklung, F-005 Arbeitsfarbraum,
Pipeline-Reihenfolge, Bit-Tiefen, Clipping, Transferfunktionen und
Farbprofilstrategie, F-008 Auto-Tone und Exposure Matching

Dieses Dokument ist die **normative** Spezifikation der Renderpipeline. Die
hier beschriebenen Stufen, Formate, Bit-Tiefen und Clipping-Regeln sind an die
tatsächlich implementierte Pipeline in `crates/lumina-core/src/pipeline.rs`
angeglichen (Abweichungen wurden zugunsten des Codes korrigiert).

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Pipeline-Reihenfolge](#pipeline-reihenfolge)
- [Arbeitsfarbraum (normativ)](#arbeitsfarbraum-normativ)
- [Bit-Tiefen (normativ)](#bit-tiefen-normativ)
- [Transferfunktionen (normativ)](#transferfunktionen-normativ)
- [Clipping (normativ)](#clipping-normativ)
- [Farbprofilstrategie (normativ)](#farbprofilstrategie-normativ)
- [Optionale Stufen und Adjustment-Semantik](#optionale-stufen-und-adjustment-semantik)
- [Bearbeitungsregler](#bearbeitungsregler)
- [Reproduzierbarkeit](#reproduzierbarkeit)
- [Auto-Tone](#auto-tone)
- [Exposure Matching](#exposure-matching)
- [Cache und Invalidierung](#cache-und-invalidierung)
- [Abnahme](#abnahme)

## Ziel

Vorschauen und Exporte werden immer aus dem unveränderten Original, der
gewählten virtuellen Kopie, dem Rezept, den Artefakten und expliziten
Versionen erzeugt. GUI und CLI verwenden dieselbe Renderpipeline.

## Pipeline-Reihenfolge

Die implementierte `Pipeline::default()` definiert die verbindliche
Stufenreihenfolge. Jede Stufe besitzt ein `(Stufe, Eingabeformat,
Ausgabeformat)`-Tupel; `Pipeline::validate()` stellt sicher, dass das
Ausgabeformat einer Stufe dem Eingabeformat der folgenden entspricht:

```text
Decode          EncodedSource  -> Rgba8Srgb
SourceActions   Rgba8Srgb      -> Rgba8Srgb
AutoAnalysis    Rgba8Srgb      -> Rgba8Srgb
Adjustments     Rgba8Srgb      -> Rgba8Srgb
Masks           Rgba8Srgb      -> Rgba8Srgb
Crop            Rgba8Srgb      -> Rgba8Srgb
Output          Rgba8Srgb      -> Output
```

Die vollständige Pipeline verbleibt im Format `Rgba8Srgb` (sRGB-codiertes
8-Bit-RGBA). Eine explizite Linearisierung in einen linearen Arbeitsraum
erfolgt im aktuellen Raster-MVP **nicht**.

## Arbeitsfarbraum (normativ)

- Der interne Arbeitsfarbraum des Raster-MVP ist **sRGB-codiertes RGBA8**
  (`PipelineFormat::Rgba8Srgb`, je Kanal ein `u8` in `0..=255`, Alpha erhalten).
- Das Enum `PipelineFormat` kennt zusätzlich `LinearProPhotoRgb`. Dieses ist
  als Reservat für eine spätere, echte lineare ProPhoto-RGB-Verarbeitung
  vorgesehen, wird aber von `Pipeline::default()` **nicht** verwendet — es
  durchläuft aktuell keine Stufe. Eine Pipelinestufe ohne definierte
  Reihenfolge, Versionierung und Tests ist unzulässig.
- ProPhoto RGB und Rec.2020 dürfen nicht gleichzeitig als unbestimmte
  Arbeitsraumalternative verwendet werden. Im aktuellen MVP ist der einzige
  aktive Arbeitsraum sRGB.
- HINWEIS zur SOLL-Abweichung: `feature/README.md` listet unter
  „Festgelegte Entscheidungen“ weiterhin „lineares ProPhoto RGB“ als
  Zielarbeitsraum. Die Implementierung liefert diesen Pfad noch nicht; die
  normative Pipeline arbeitet in sRGB. Die ProPhoto-Linearisierung bleibt ein
  dokumentiertes Ziel (reserviert über `LinearProPhotoRgb`), nicht der
  implementierte Zustand.

## Bit-Tiefen (normativ)

- Die durchgehende Arbeits-Bit-Tiefe der Pipeline ist **8 Bit pro Kanal**
  (RGBA8).
- Der native RAW-Decoder (`lumina-raw`) unterstützt als Ausgabe **8 oder 16
  Bit** je Kanal (`RawDecodeOptions::output_bits`, geprüft auf `8 | 16`).
  Die dekodierten Rohdaten werden vor Eintritt in die Pipeline auf RGBA8
  reduziert (16-Bit-Werte werden um 8 Bit nach rechts geschoben), sodass alle
  nachfolgenden Stufen in RGBA8 arbeiten.
- Exportformate (PNG, JPEG, WebP) sind 8-Bit-Container; tiefere Bit-Tiefen im
  Export sind im Modell vorbereitet, aber im MVP nicht implementiert.

## Transferfunktionen (normativ)

- Der RAW-Decoder liefert bereits **sRGB-codierte** Pixeldaten
  (`libraw_set_output_color(..., 1)`), ohne automatische Helligkeitsanpassung
  (`no_auto_bright = 0` überlässt LibRaw die Entscheidung; Kamera-White-Balance
  und -Matrix sind aktiv).
- Eine zusätzliche Gamma-Dekodierung oder Linearisierung findet in der Pipeline
  nicht statt: Die Werte in `Rgba8Srgb` sind sRGB-kodiert und werden als solche
  additiv und multiplikativ bearbeitet.
- Auto-Tone und Exposure Matching messen die **sRGB-kodierte** Helligkeit: Die
  RGB-Kanäle werden auf `0..=1` normalisiert (Wert `/ 255`) und mit Rec.709
  (`0.2126 / 0.7152 / 0.0722`) gewichtet. Alpha wird ignoriert.

## Clipping (normativ)

- Jeder Kanal wird nach multiplikativen/multiplikativen Operationen auf `0..=255`
  begrenzt (`clamp(0.0, 255.0)` als `u8`). Alpha bleibt unverändert.
- Die globalen Raster-Adjustments arbeiten auf dem normalisierten `x` in
  `0..=1`; das Ergebnis wird zurück auf `0..=255` skaliert und begrenzt.
- Ungültige Adjustment-Werte werden **nicht** still geclippt, sondern mit einem
  Fehler abgelehnt:
  - `exposure` endlich und in `-10..=10` EV
  - `contrast`, `highlights`, `shadows` endlich und in `-1..=1`
  - unbekannte Adjustment-Keys werden mit `UnsupportedAdjustment` abgelehnt
- Auto-Tone und Exposure Matching sind explizit gegen Division durch null,
  extreme Zielwerte und Clipping abgesichert (Epsilon-Schutz, Begrenzung auf
  `-10..=10` EV).

## Farbprofilstrategie (normativ)

- **Ausgabeprofil:** Standard ist **sRGB**. Der `RenderKey` führt ein
  `output_profile`-Feld (Zeichenkette); weitere Profile sind im Modell
  vorbereitet, im MVP aber nicht implementiert.
- **Quellprofile:** Der RAW-Decoder extrahiert Kamera-Matrix
  (`camera_matrix`), Kamera-White-Balance (`camera_white_balance`),
  Vor-Multiplikatoren (`pre_multipliers`) und ein optionales eingebettetes
  ICC-Profil (`icc_profile`). Im MVP wird die sRGB-Ausgabe von LibRaw ohne
  eigene zusätzliche Farbtransformation verwendet; eine proprietäre
  Matrix-/Profilanwendung ist noch nicht in die Pipeline integriert.
- **Persistenz von Farbkontext:** Decode-Version, Pipeline-Version und
  `output_profile` gehören zum Render-Key, sodass ein Wechsel des
  Farbkontexts gezielt invalidiert.

## Optionale Stufen und Adjustment-Semantik

Alle Stufen sind optional, sofern das Rezept sie nicht aktiviert. Die
verbindliche Reihenfolge für aktivierte Entwicklungsfunktionen lautet:

```text
Source-Actions
  -> Auto-WB / Auto-Tone
  -> Preset-Werte
  -> lokale Masken und Masken-Adjustments
  -> Match Total Exposure
```

Im Raster-MVP sind `exposure` endliche Werte im Bereich `-10..=10` EV und
`contrast`, `highlights` sowie `shadows` endliche Werte im Bereich `-1..=1`.
Ungültige Werte und unbekannte Adjustment-Keys werden mit einem Fehler
abgelehnt; sie werden nicht still geclippt oder ignoriert.

Die globalen Raster-Adjustments arbeiten auf jedem RGB-Kanal als `x` in
`0..=1`; Alpha bleibt unverändert. Nach Exposure und Contrast werden die
Stufen in Rezeptreihenfolge angewendet. Sind beide Stufen vorhanden, wird
`shadows` zuerst und `highlights` danach ausgeführt:

```text
shadow_weight = ((0.5 - x) / 0.5).max(0)^2
x' = clamp(x + shadows * shadow_weight * 0.25)

highlight_weight = ((x - 0.5) / 0.5).max(0)^2
x' = clamp(x + highlights * highlight_weight * 0.25)
```

Dabei bezeichnet `x` bei der zweiten Formel den Wert nach der Shadows-Stufe,
falls diese aktiv ist. Dies ist bewusst eine einfache deterministische
Raster-MVP-Heuristik und keine finale RAW-/Farbmanagement-Semantik.

Source-Actions wie nicht-destruktive Staubentfernung und spätere KI-
Teil-Ersetzung werden als Rezeptoperationen gespeichert. Sie wirken nach
Decode/Demosaic und vor Auto-Analyse. Das Original bleibt unverändert.

Auto-WB, Auto-Tone und Auto-Exposure speichern ihr Ergebnis zusammen mit einem
Analysefingerprint. Sie werden nur manuell, durch ein Preset oder bei
ungültigem Fingerprint neu berechnet.

## Bearbeitungsregler

Dieser Abschnitt erweitert die Rezeptsemantik normativ. Alle Felder liegen in
der jeweiligen virtuellen Kopie unter `recipe` und werden mit
`recipe_schema_version` sowie der unabhängigen `pipeline_version` validiert.
Die MVP-Version dieser Regler ist `1`; unbekannte oder außerhalb des
angegebenen Bereichs liegende Werte werden abgelehnt, nicht still geclippt.
Alle Berechnungen liefern weiterhin `Rgba8Srgb`; Zwischenwerte dürfen intern
höhere Genauigkeit verwenden und werden erst am Ende der jeweiligen Operation
auf `0..=1` begrenzt.

Die top-level-Reihenfolge bleibt `Decode → SourceActions → AutoAnalysis →
Adjustments → Masks → Crop → Output`. Die in diesem Abschnitt genannten
Unterstufen innerhalb von `Adjustments` beziehungsweise `Crop` sind verbindlich
und ändern dieses Format-Tupel nicht.

**Schema-Migration:** Das Upgrade von `recipe_schema_version` 1 auf 2 ist
erforderlich, sobald verschachtelte Adjustment-Felder (`curves`, `hsl`,
`color_grading`, `presence`, `sharpening`, `noise_reduction`) oder neue
Top-Level-Keys (`geometry`, `lens_correction`, `perspective`, `effects`)
verwendet werden. Altdateien mit flacher `adjustments`-Map bleiben als
`schema_version: 1` gültig; nicht gesetzte verschachtelte Felder werden als
Identität interpretiert. Eine automatische Migration von `schema_version: 1`
nach `2` findet nicht statt; sie erfolgt verzögert mit Bestätigung gemäß der
festgelegten Migrationsstrategie.

### F-089 Gradationskurve

**Ziel und Definition:** `recipe.adjustments.curves` enthält
`version`, `master` und `channels`. Jede Kurve ist eine Liste
`points: [{input, output}]` mit mindestens zwei und höchstens 32 Punkten.
`input` und `output` sind endliche `f32`-Werte in `0..=1`; Punkte müssen nach
`input` streng aufsteigend sein, Endpunkte `(0,0)` und `(1,1)` werden bei
fehlenden Endpunkten nicht implizit ergänzt (die Validierung verlangt sie).
`master` wirkt auf die Luminanz-/RGB-Kurve, `channels` enthält die getrennten
Kurven `red`, `green` und `blue`; nicht gesetzte Kanäle sind Identität.

Zwischen Stützpunkten wird eine monotone kubische Hermite-Interpolation mit
begrenzt berechneten Tangenten verwendet. Sie ist gegenüber Catmull-Rom
bevorzugt, weil sie bei monotonen Stützpunkten kein Überschwingen erzeugt.
Ausgaben werden nach der Interpolation auf `0..=1` geclippt. Auto-Tone wird
vor der Kurve berechnet und angewandt; die Kurve ist danach die letzte globale
Tonwertoperation (vor HSL/Color-Grading nur gemäß den nachfolgenden
Unterstufen). Die Kurve wird auf sRGB-codierten Werten angewandt, nicht auf
linearisierten ProPhoto-Werten.

**Platzierung, Cache und Abnahme:** In `Adjustments`, nach Auto-WB,
Auto-Tone/Preset und globalem Tonwert, vor lokalen Masken. Die Kurvenstruktur,
ihre Version und der wirksame Inhalt müssen im `recipe_hash` enthalten sein;
eine Änderung invalidiert ab `adjustments` alle abhängigen Preview-/Export-
Einträge, nicht Decode oder AI-Masken. Abnahme: JSON-Roundtrip, monotone
Interpolation ohne Overshoot, Kanaltrennung, Endpunktvalidierung, Clipping
und Cache-Miss werden getestet. MVP-Grenze: keine parametrische Kurve und keine
lineare-Farbraum-Variante; spätere Pipelineversionen dürfen diese nur migriert
einführen. Abhängigkeiten: F-029, F-031, F-036, F-039 und F-041.

### F-090 HSL/Farbmischer

**Ziel:** Selektive Farbkorrektur ohne Änderung der übrigen Farbbereiche.
`recipe.adjustments.hsl` enthält `version` und acht Kanalobjekte mit den
Feldern `hue`, `saturation`, `luminance`. Die Kanäle heißen normativ `red`,
`orange`, `yellow`, `green`, `cyan`, `blue`, `violet`, `magenta` (Zentren
0°, 30°, 60°, 120°, 180°, 240°, 270°, 300°); `violet` und `magenta` sind
also zwei getrennte, benachbarte Kanäle, keine doppelte Magenta-Kategorie.
`hue` und `saturation` liegen in `-1..=1` (`hue` entspricht einer relativen
Drehung von höchstens 30°), `luminance` in `-1..=1`.

Die MVP-Transformation erfolgt im **sRGB-codierten** RGB-Raum nach
Normalisierung, mit HSL-Konvertierung pro Pixel. Jeder Kanal verwendet eine
zyklische, stückweise lineare Dreiecksgewichtung über die Hue-Abstände zum
eigenen Zentrum und den beiden Nachbarzentren; die Beiträge werden normiert.
Sättigung und Luminanz werden gewichtet addiert, Hue wird gewichtet gedreht,
danach wird auf `0..=1` geclippt. Das ist bewusst keine lineare- oder
ProPhoto-Farbverarbeitung.

Die Stufe liegt in `Adjustments` nach globalem Tonwert und vor Color Grading,
Masken und Crop. `recipe_hash` enthält alle acht Objekte und `version`;
Änderungen invalidieren ab dieser Unterstufe. Abnahme: Kanalzentren und
Nachbarübergänge, zyklische Hue-Grenzen, Wertevalidierung, neutrale Identität
und Cache-Invalidierung. MVP-Grenzen: keine selektive Masken-HSL-Matrix und
keine lineare HSL-Alternative. Abhängigkeiten: F-031, F-036, F-039.

### F-091 Color Grading

**Ziel:** Unabhängige Tönung von Schatten, Mitteltönen und Lichtern mit einem
kontrollierbaren Übergang zwischen den Bereichen.
`recipe.adjustments.color_grading` enthält `version`, die Bereiche `shadows`,
`midtones`, `highlights` mit `hue_degrees` in `0..360` und `saturation` in
`0..=1`, sowie `balance` in `-1..=1`. Der Farbton ist ein Winkel (zyklisch,
0° = Rot) im sRGB-HSL-Farbmodell; die Tönungsfarbe wird in RGB umgerechnet
und mit der jeweiligen Sättigung gemischt.

Die Bereichsgewichte sind weiche, überlappende Funktionen der sRGB-
Luminanz: Schatten `smoothstep(0.65,0.0, L)`, Lichter
`smoothstep(0.35,1.0,L)`, Mitteltöne `1 - shadow - highlight`, jeweils auf
Summe 1 normiert. `balance` verschiebt die beiden Übergänge symmetrisch
zwischen Schatten und Lichtern. Color Grading folgt HSL und globaler Kurve,
liegt vor Masken und wird vor Crop ausgeführt. Damit ist die dokumentierte
Zusammenwirkung: Auto-Tone → Kurve → HSL → Color Grading.

`recipe_hash` berücksichtigt alle Werte und `version`; Änderungen invalidieren
Preview/Export ab Color Grading, nicht Decode/Maskenartefakte. Abnahme:
zyklische Hue-Werte, weiche Übergänge, Balance-Richtung, Identität bei
Sättigung 0 und reproduzierbarer Cache-Miss. MVP-Grenzen: sRGB-HSL statt
linearem oder perceptuellem Farbmodell, keine getrennte Blending-Methodik.
Abhängigkeiten: F-031, F-036, F-039.

### F-092 Dynamik und Sättigung

**Ziel:** Schwache Farben gezielt beleben und eine einfache globale Sättigung
anbieten, ohne bereits gesättigte Farben unnötig zu übersteuern.
`recipe.adjustments.vibrance` und `saturation` sind endliche Werte in
`-1..=1`. `saturation` skaliert die HSL-Sättigung linear um den Faktor
`1 + saturation` (sRGB-codiertes RGB, Clipping danach). `vibrance` erhöht oder
senkt sie gewichtet: Schutzgewicht = Produkt aus geringer vorhandener
Sättigung und einem Hautfarbenschutz (Hue-Bereich ungefähr 15°..55° mit
weichen Rändern); bereits gesättigte Farben und geschützte Hauttöne werden
höchstens proportional schwach verändert. Negative Vibrance wirkt ebenfalls
gewichtet, nicht als zweiter linearer Sättigungsregler.

Die Unterstufenreihenfolge ist `vibrance → saturation`, nach HSL und vor Color
Grading, Masken und Crop. Werte, Schutzfunktion und `version` gehen in den
`recipe_hash`; Änderungen invalidieren ab Adjustments. Abnahme: Identität bei
0, Begrenzung, Schutz gesättigter Farben/Hauttöne und Cache-Invalidierung.
MVP-Grenze: heuristischer HSL-Hautschutz; kein kameramodell- oder
hauttonadaptives Lernen. Abhängigkeiten: F-031, F-036, F-090.

### F-093 Zuschneiden und Drehen

**Ziel:** Einen nicht-destruktiven sichtbaren Bildausschnitt samt Drehung und
Spiegelung reproduzierbar festlegen.
`recipe.geometry` enthält `version`, `crop` (`mode: "aspect"|"free"`, bei
`aspect` `preset` aus `original`, `1:1`, `4:5`, `5:4`, `3:2`, `2:3`, `4:3`,
`3:4`, `16:9`, `9:16`, bei `free` `x`, `y`, `width`, `height` in normierten
Quellkoordinaten `0..=1`), `rotation_degrees` in `-180..=180` und
`mirror_horizontal`/`mirror_vertical` als bool. Freie Rechtecke müssen eine
positive Fläche besitzen; Presets werden auf die Bildgrenzen eingepasst.

In der Crop-Stufe gilt intern: Objektivkorrektur → Perspektive → Crop →
Rotation → Spiegelung. Rotation erfolgt um das Crop-Zentrum; Ausgabemaße und
Seitenverhältnis werden deterministisch aus Transform und Outputvorgabe
berechnet. Die finale F-041-Messdomäne liegt nach Crop und Geometrie, vor
Outputprofil/Export. Geometrieparameter, `version` und resultierende
`output_dimensions` gehören zum RenderKey: Decode, Auto-Analyse und Masken
bleiben bei reiner Geometrieänderung cachebar, Preview/Export werden
invalidiert. Abnahme: Presets, normiertes Rechteck, Drehung/Spiegelung,
RenderKey-Trennung und F-041-Messbereich. MVP-Grenze: kein Auto-Crop; abhängig
von F-029, F-039, F-041, F-098 und F-099.

### F-094 Präsenz

**Ziel:** Feine und mittlere lokale Bildstrukturen sowie atmosphärischen Dunst
mit getrennten, kontrollierbaren Reglern beeinflussen.
`recipe.adjustments.presence` enthält `version`, `texture`, `clarity` und
`dehaze`, jeweils endlich in `-1..=1` (UI kann -100..+100 anzeigen). Texture
ist lokaler Kontrast eines kleinen, skalenabhängigen Radius (feine Strukturen,
MVP Radius 1..3 px); Clarity ist lokaler Mittenkontrast mit großem Radius
(MVP 8..32 px). Beide verwenden eine deterministische Difference-of-Gaussians-
Heuristik. Dehaze verwendet ein dunkelkanal-basiertes Atmosphärenmodell mit
lokaler Transmission, begrenzt auf `0.05..=1`; negative Werte werden als
umgekehrter, abgeschwächter Effekt definiert.

Reihenfolge in `Adjustments`: Exposure → Contrast → Highlights/Shadows →
Presence (Texture → Clarity → Dehaze) → Kurve/HSL gemäß den dort definierten
Unterstufen; Alpha bleibt unverändert. `version` und alle Werte stehen im
`recipe_hash`, Änderungen invalidieren ab Adjustments. Abnahme: Wertefehler,
Radiusverhalten, deterministisches Clipping, Kontrast-Reihenfolge und Cache.
MVP-Grenzen: Heuristiken statt Lightroom-identischer RAW-/Dehaze-Semantik,
kein GPU-spezifischer Fallback. Abhängigkeiten: F-031, F-036, F-039, F-041.

### F-095 Schärfen

**Ziel:** Details reproduzierbar schärfen und homogene Flächen über eine
Luminanz-Kantenmaske vor Halos schützen.
`recipe.adjustments.sharpening` enthält `version`, `amount` `0..=3`, `radius`
`0.1..=10` (in Quellpixeln), `detail` `0..=1` und `masking` `0..=1`.
MVP ist eine Unsharp-Mask: Luminanz, Gauß-Blur mit Radius, Differenzsignal
und `amount`; `detail` mischt feinere und gröbere Differenzen. `masking` ist
eine aus Luminanzgradienten erzeugte Kantenmaske: hohe Maskierung unterdrückt
Flächen, Kanten bleiben wirksam. Radius wird bei Vorschau/Export proportional
zur effektiven Bildskalierung umgerechnet.

Schärfen liegt am Ende von `Adjustments`, nach Rauschreduzierung und vor
Masks/Crop/Output. Werte, Version und effektive Skalierung sind im RenderKey/
`recipe_hash`; Änderungen invalidieren ab Schärfen. Abnahme: Unsharp-Verhalten,
Kantenmaske, Skalierung, Clipping und Cache. MVP-Grenzen: keine
Ausgabe-Schärfung mit eigenem Profil und kein lokales Masken-Schärfen.
Abhängigkeiten: F-031, F-036, F-096.

### F-096 Rauschreduzierung

**Ziel:** Manuelles Luminanz- und Farbrauschen vor dem Schärfen vermindern,
ohne ein nicht reproduzierbares KI-Modell vorauszusetzen.
`recipe.adjustments.noise_reduction` enthält `version`, `luminance` und
`color`, jeweils `0..=1`; 0 ist Identität. Das MVP verwendet einen
deterministischen, kantenbewussten lokalen Mittelwert (5x5-Fenster): Luminanz
wird nach Ähnlichkeit der Luminanz gewichtet geglättet, Farbrauschen wird im
Chromakanal stärker geglättet. Das ist bewusst ein einfaches, CPU/WASM-
kompatibles Modell statt eines nicht reproduzierbaren KI-Verfahrens.

Rauschreduzierung liegt in `Adjustments` vor Schärfen, Masken und Crop.
Felder/Version gehen in den `recipe_hash`; Änderungen invalidieren ab dieser
Unterstufe. Abnahme: 0-Identität, Kantenbewahrung, Kanaltrennung,
Determinismus und Schärfen-Reihenfolge. KI-Denoise ist nur eine optionale
spätere Erweiterung und wird hier nicht spezifiziert. Abhängigkeiten: F-031,
F-036.

### F-097 Vignettierung und Körnung (niedrige Priorität)

**Ziel:** Eine reproduzierbare Randabdunklung und eine deterministische
prozedurale Körnung als abschließende Stilmittel ermöglichen.
`recipe.effects.vignette` enthält `version`, `amount` `-1..=1`, `midpoint`
`0..=1`, `roundness` `-1..=1` und `feather` `0..=1`; der Effekt ist radial und
wird vor Output angewandt. `recipe.effects.grain` enthält `version`, `amount`,
`size` und `roughness`, jeweils `0..=1`, und `seed` als `u64`. Körnung ist
prozedurales, kanalgekoppeltes Rauschen; der effektive Seed wird aus
`seed` und dem RenderKey deterministisch abgeleitet. Beide Effektobjekte und
Versionen stehen im RenderKey/`recipe_hash` und invalidieren Preview/Export.
Die Effekte laufen innerhalb der `Adjustments`-Stufe als letzte Unterstufe
(nach Schärfen, vor Masks und Crop); das Format-Tupel bleibt unverändert.
Abnahme: radiale Parameter, deterministische Wiederholung und Seed-Wechsel.
MVP-Grenze: niedrige Priorität, kein filmspezifisches Kornmodell. Abhängigkeit:
F-031, F-093.

### F-098 Objektivkorrekturen

**Ziel:** Typische Objektivfehler im MVP manuell und ohne externe Profildaten
geometrisch, tonal und kanalbezogen korrigieren.
`recipe.lens_correction` enthält `version`, `profile` (optional benannter,
eingebauter Presetname), `distortion_k1`, `distortion_k2`, `distortion_k3` in
`-1..=1`, `vignette_c0`, `vignette_c1`, `vignette_c2` in `-1..=1`, sowie
`ca_red` und `ca_blue` in `-0.05..=0.05`; Grün ist Referenz. Die Verzeichnung
ist ein normiertes radiales Polynom `r' = r*(1+k1*r²+k2*r⁴+k3*r⁶)`, die
Vignette ein radiales Polynom, CA eine relative R-/B-Kanal-Skalierung.

Die geometrische Korrektur liegt vor Perspektive und Crop; Vignette folgt der
Geometriekorrektur, CA wird kanalweise unmittelbar vor dem Crop-Resampling
angewandt, damit Farbsaumkorrektur nicht durch spätere Geometrie verstärkt
wird. Alle Felder und `profile` gehen in den RenderKey. Abnahme: Polynom,
Kanalreferenz, Preset-Roundtrip und gezielte Cache-Invalidierung. MVP-Grenze:
manuelle Koeffizienten und einfache benannte Presets; Lensfun ist ausdrücklich
Post-MVP. Für Lensfun sind LGPL-3.0/CC-BY-SA und F-078 zu prüfen; die native
Dependency benötigt einen Capability-Matrix-Eintrag. Abhängigkeiten: F-031,
F-037, F-078, F-099.

### F-099 Upright und Perspektive

**Ziel:** Perspektivische Verzerrungen manuell korrigieren und die resultierende
Geometrie mit Objektivkorrektur und Crop kombinierbar machen.
`recipe.perspective` enthält `version`, `vertical`, `horizontal`, `rotation`,
`scale`, `aspect_ratio`, `shift_x` und `shift_y`; alle sind endliche Werte,
die Achsenkorrekturen und Rotation in `-1..=1`, `scale` in `0.1..=10`,
`aspect_ratio` in `0.1..=10` und Verschiebungen in `-1..=1`. Die vier
Eckpunkte der normierten Bildebene werden mit einer 3x3-Homographie auf die
Ausgabeebene projiziert; bilineares Resampling und definierte Randfüllung
(`transparent` wird im MVP zu Schwarz) sind verbindlich.

Die manuelle Perspektive liegt nach F-098-Verzeichnung und vor F-093-Crop;
die automatische Analyse stürzender Linien ist ausdrücklich Post-MVP. Alle
Parameter, Homographie-/Pipelineversion und resultierenden Dimensionen
gehören zum RenderKey; Geometrieänderungen invalidieren Preview/Export, nicht
Decode oder AI-Artefakte. Abnahme: Identität, Eckpunktprojektion,
Parametergrenzen, Zusammenspiel mit Objektivkorrektur/Crop und Cache.
Abhängigkeiten: F-029, F-031, F-041, F-098.

## Reproduzierbarkeit

Jeder Render-Key enthält mindestens:

```text
source_content_hash
decode_parameters
pipeline_version
virtual_copy_id
recipe_hash
mask_artifact_hashes
output_profile
output_dimensions
output_format
```

Ein Dateipfad oder Zeitstempel allein ist kein gültiger Render-Key. Der
`RenderKey` wird deterministisch gehasht; `stage_digest` ermöglicht
stufenspezifische Digests (decode / mask / histogram / render), sodass
beispielsweise eine reine Ausgabegrößenänderung den Decode-Cache nicht
invalidiert, wohl aber Preview und Export.

## Auto-Tone

Im Raster-MVP werden die RGBA8-RGB-Kanäle als sRGB-codierte Werte auf 0..=1
normalisiert und mit Rec.709 (0.2126/0.7152/0.0722) gewichtet. Alpha wird
ignoriert, auch bei transparenten Pixeln. Perzentile verwenden lineare
Interpolation zwischen sortierten Samples. Auto-Tone richtet den Median auf das
Ziel aus und bestimmt Kontrast aus der p01/p99-Spanne; Exposure ist auf
-10..=10 EV und Contrast auf -1..=1 begrenzt. Leere Bilder liefern 0, Schwarz
liefert den oberen Exposure-Fallback und Weiß den unteren.

## Exposure Matching

`Match Total Exposure` misst nach dem Auto-Schritt die definierte gewichtete
Luminanz und berechnet `log2(target/current)` mit Epsilon, finite-Schutz und
-10..=10-Begrenzung. Die Implementierung muss Schutz gegen
Division durch null, extreme Zielwerte, Clipping und Maskeneinflüsse enthalten.
Im aktuellen Raster-MVP messen Auto-Tone und Matching ausschließlich den
dekodierten aktuellen Raster-Messbereich (alle RGBA-Pixel, Alpha ignoriert).
Der spätere finale Messbereich ist davon getrennt: Er entsteht erst nach Crop,
Geometrie und aktiven Masken und liegt vor Outputprofil und Export-
Transferfunktion. Dieser finale Messbereich ist noch nicht implementiert;
F-041 bleibt deshalb offen, bis Crop und Masken in dieser Pipeline tatsächlich
verfügbar sind.
Die Raster-MVP-Reihenfolge lautet Source-Actions (noch nicht im CLI),
Auto-Tone, Preset, CLI-Overrides, Masken später, danach Matching. Berechnete
Auto-Werte und ein RGBA8-Analysefingerprint werden im Rezept persistiert und
bei gültigem Fingerprint wiederverwendet.

## Cache und Invalidierung

Cache-Stufen dürfen Decode, Demosaicing, Histogramm, Preview, Masken und Export
enthalten. Jeder Eintrag kennt Eingabeschlüssel, Versionen und Prüfsumme und
kann vollständig gelöscht werden.

Der Ordner-Cache liegt unter `.lumina/` und ist nicht autoritativ. Pro Quelle
und virtueller Kopie wird standardmäßig nur die aktuelle Standardvorschau beim
Verlassen des Bildes gespeichert. Eine 1:1-Vorschau ist eine geerbte
Ordneroption und standardmäßig deaktiviert. Verwaiste Cacheeinträge dürfen
gelöscht werden, sobald ein Bild über Lumina verschoben oder umbenannt wurde
beziehungsweise beim Scan nicht mehr gefunden wird.

> **Implementierungsstatus (F-086, 2026-08-17):** Umgesetzt und unabhängig
> verifiziert. `lumina-core` besitzt eine native, per
> `#[cfg(not(target_arch = "wasm32"))]` gekapselte Disk-Schicht
> (`DiskFolderCache`, crates/lumina-core/src/cache/disk.rs): atomare Writes,
> `settings.json` mit feldweiser Eltern-Vererbung, Vorschauen pro Quelle +
> virtueller Kopie unter `.lumina/previews/` (Standard- vs. 1:1-Vorschau) und
> sofortiger Prune verwaister Einträge. Offene Folgeaufgabe: Test für partielle
> Settings-Vererbung (Kind-JSON mit nur einem gesetzten Feld).

Eine reine Crop- oder Ausgabeänderung soll keine unnötige AI-Inferenz auslösen.
Ändert sich dagegen Quelle, Decode-Kontext, Pipelineversion, Maskenartefakt oder
Rezeptabschnitt einer abhängigen Stufe, wird die betroffene Stufe invalidiert.
Parallele Preview-Ergebnisse werden verworfen, wenn sie nicht mehr zum aktuellen
Rezeptstand gehören.

## Abnahme

- CPU und CLI liefern für identische Eingaben reproduzierbare Ergebnisse mit
  dokumentierten Toleranzen.
- Render-Keys unterscheiden alle relevanten Eingaben (inklusive
  Arbeitsfarbraum und `output_profile`).
- Cache-Hit, Cache-Miss und gezielte Invalidierung sind getestet.
- Auto-Tone und Exposure Matching besitzen Unit-, Property- und
  Referenzbildtests.
- Die implementierte Stufenreihenfolge und die Formatverträglichkeit sind über
  `Pipeline::validate()` abgesichert.
