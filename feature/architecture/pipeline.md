# Nicht-destruktive Renderpipeline

**Features:** F-003 Nicht-destruktive Entwicklung, F-005 Cache und Render-Key,
F-008 Auto-Tone und Exposure Matching

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Pipeline-Reihenfolge](#pipeline-reihenfolge)
- [Optionale Stufen](#optionale-stufen)
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

```text
Quelle identifizieren
  -> RAW dekodieren und kalibrieren
  -> linearen Arbeitsfarbraum herstellen
  -> optionale Source-Actions
  -> Weißabgleich und globale Tonwerte
  -> Auto-Tone und optional Match Total Exposure
  -> Farb- und lokale Anpassungen
  -> Masken anwenden
  -> Crop und Geometrie
  -> Ausgabeprofil, Schärfung und Export
```

Arbeitsfarbraum, Transferfunktionen, Clipping, Bit-Tiefe, Farbprofile und
Reihenfolge müssen vor mathematischer Implementierung normativ festgelegt
werden. ProPhoto RGB und Rec.2020 dürfen nicht gleichzeitig als unbestimmte
Arbeitsraumalternative verwendet werden.

## Optionale Stufen

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

Ein Dateipfad oder Zeitstempel allein ist kein gültiger Render-Key.

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
- Render-Keys unterscheiden alle relevanten Eingaben.
- Cache-Hit, Cache-Miss und gezielte Invalidierung sind getestet.
- Auto-Tone und Exposure Matching besitzen Unit-, Property- und
  Referenzbildtests.
