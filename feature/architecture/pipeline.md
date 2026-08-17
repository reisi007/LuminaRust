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
`contrast` endliche Werte im Bereich `-1..=1`. Ungültige Werte und unbekannte
Adjustment-Keys werden mit einem Fehler abgelehnt; sie werden nicht still
geclippt oder ignoriert.

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

Die Messdomäne für Histogramm, Median, 1%- und 99%-Perzentil sowie gewichtete
Luminanz muss festgelegt werden. Auto-Tone benötigt begrenzte Ausgabewerte,
definiertes Clipping-Verhalten und reproduzierbare Fallbacks für leere,
überbelichtete oder nahezu schwarze Bilder.

## Exposure Matching

`Match Total Exposure` misst nach dem Auto-Schritt die definierte gewichtete
Luminanz und berechnet die Zielkorrektur. Die Implementierung muss Schutz gegen
Division durch null, extreme Zielwerte, Clipping und Maskeneinflüsse enthalten.
Das Matching misst die finale sichtbare Fläche nach Crop/Geometrie und aktiven
Masken, aber vor Outputprofil und Export-Transferfunktion. Es bleibt optional.

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
