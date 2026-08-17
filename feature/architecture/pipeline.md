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
