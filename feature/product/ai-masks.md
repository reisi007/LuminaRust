# Persistente AI-Masken

**Feature:** F-004 Persistente AI-Masken

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Maskenidentität](#maskenidentität)
- [Artefakte](#artefakte)
- [Masken-DAG](#masken-dag)
- [Benutzergeführte Segmentierung](#benutzergeführte-segmentierung)
- [Status und Wiederverwendung](#status-und-wiederverwendung)
- [Lokale Anpassungen](#lokale-anpassungen)
- [Abnahme](#abnahme)

## Ziel

Lokale ONNX-Inferenz erzeugt eine Alpha-Matte einmalig und persistiert sie als
Sidecar-Artefakt. Beim erneuten Öffnen wird die Matte geladen. Ein Modell muss
nicht dauerhaft installiert sein, um eine bereits gültige Maske zu verwenden.

## Maskenidentität

Jede Maske referenziert mindestens:

- `source_content_hash`;
- RAW-Decode- und Orientierungsparameter;
- Modellname, Modellversion und Modell-Hash;
- Vorverarbeitung und Inferenzauflösung;
- Nachskalierung und Koordinatensystem;
- Datenformat, Auflösung, Kanalzahl und Artefakt-Prüfsumme;
- Erstellungszeitpunkt und Generatorversion.

## Artefakte

Die JSON-Datei speichert Definition und Referenz. Die Matte selbst liegt als
komprimiertes, sidecarbezogenes Binärartefakt in `.lumina.zdata` vor. Das
Format soll Kachelung oder Multi-Resolution und mindestens 16-Bit-
Graustufengenauigkeit ermöglichen.
Unkomprimierte Vollauflösungs-`f32`-Arrays im JSON sind nicht zulässig.

## Masken-DAG

Jede virtuelle Kopie besitzt eine eigene Maskenbibliothek. Knoten können jedoch
auf Knoten anderer virtueller Kopien referenzieren. Die Auswertung bildet einen
gerichteten azyklischen Graphen; Zyklen werden bei der Validierung abgelehnt.

Unterstützte v1-Operationen sind `union`, `intersect`, `subtract` und
`invert`. `duplicate & invert` erzeugt keinen zweiten Matte-Payload, sondern
einen neuen Referenz-/Operationsknoten. Werden Cross-Copy-Referenzen durch
Löschen der Quellkopie ungültig, werden Graphdefinitionen in die Zielbibliothek
materialisiert. Identische binäre Payloads dürfen im `.zdata`-Container über
ihren Content-Hash dedupliziert bleiben.

### Auswertung

Maskendefinitionen tragen in Schema 1 das optionale Feld `operation`; fehlt es,
ist der serde-Default `source`. Eine Source-Maske hat keine Referenzen und wird
mit einer bereitgestellten `uint16`-Fläche aus `.zdata` gespeist. `invert`
benötigt genau eine Referenz, `union` und `intersect` mindestens zwei, und
`subtract` genau zwei Referenzen (`a` zuerst, `b` danach). Alle Flächen müssen
dieselbe Breite und Höhe besitzen. Pro Pixel gilt: Union ist `max`, Intersect
ist `min`, Invert ist `65535 - value`, und Subtract ist
`round(a * (1 - b / 65535))`, integer-sicher als
`(a * (65535 - b) + 32767) / 65535` berechnet. Fehlende Payloads, Ziele und
Zyklen sind Fehler; es gibt keine stillen Resizes oder leeren Fallbacks.

## Benutzergeführte Segmentierung

Neben automatischer Subject-Segmentierung soll LuminaRust ein Objekt anhand
einer Benutzerführung isolieren können. Das ist eine eigene Maskenquelle und
keine zerstörende Änderung am Original.

### Prompt-Typen

- Rechteck beziehungsweise Box als grobe Objektbegrenzung
- Pinselmaske als positive/negative Markierung oder als Masken-Prompt
- Polygon, Ellipse und weitere Grundformen als kombinierbare Promptquellen

Eine Box wird in das Koordinatensystem des Modells transformiert. Eine
Pinselmaske kann abhängig von den Fähigkeiten des konkreten ONNX-Modells als
Masken-Prompt verwendet oder in positive und negative Promptpunkte umgewandelt
werden. Diese Umwandlung muss als Teil der Maskenidentität gespeichert werden.

### Modelladapter

Der ONNX-Adapter muss Modellfähigkeiten deklarieren, mindestens:

- `box_prompt`
- `point_prompt`
- `mask_prompt`
- `class_detection`
- `instance_segmentation`

Ein interaktives Modell wie SAM 2 kann Box- und Pinsel-Prompts in eine
Objektmaske umwandeln. Ein Modell wie YOLO-Segmentation kann später zusätzlich
eine erkannte Objektklasse und Instanzmaske liefern. Beide Modellarten werden
über dieselbe versionierte Masken- und Artefaktidentität eingebunden.

BiRefNet ist das erste automatische Subject-Modell. SAM 2 ist das erste
interaktive Box-/Pinsel-Modell. Der Adapter bleibt modellagnostisch, damit
später mehrere ONNX-Modelle gleichzeitig verfügbar sein können. Automatische
Kategorien wie „Haare von Person 1“ oder „Haare aller Personen“ gehören zu einer
späteren Instanz- und Teilsegmentierung.

### Persistenz

Promptdaten bleiben neben dem erzeugten Maskenknoten erhalten. Dazu gehören
Prompttyp, Koordinaten, Pinselauflösung, positive/negative Markierungen,
Modellfähigkeiten, Modellhash und die verwendete Transformation. Die erzeugte
Matte kann dadurch später explizit neu berechnet werden, ohne die Benutzer-
auswahl zu verlieren.

## Status und Wiederverwendung

- `valid`: Quelle, Modellkontext und Prüfsumme stimmen; Matte wird direkt
  verwendet.
- `stale`: Quelle oder Modellkontext weicht ab; alte Matte bleibt
  nachvollziehbar und kann explizit verwendet oder ersetzt werden.
- `missing`: Referenziertes Artefakt fehlt; es wird nicht stillschweigend
  inferiert.
- `corrupt`: Prüfsumme oder Format ist ungültig; Wiederherstellung oder
  explizite Neuberechnung ist erforderlich.

Eine neue Inferenz findet nur nach ausdrücklicher Aktion oder nach der
festgelegten Ungültigkeitsentscheidung statt.

Eine fehlende oder noch nicht berechnete Maske wird bei der Auswertung wie eine
leere Maske behandelt und erhält zusätzlich den sichtbaren Status `missing`
beziehungsweise `pending`. Die GUI bietet Berechnung vor dem Export oder eine
Hintergrundberechnung für nicht aktive Bilder an. Die Idle-Queue ist per
Ordner-/GUI-Einstellung deaktivierbar. Die CLI warnt standardmäßig und kann
mit `--update-masks` explizit neu berechnen.

Aktive, aber veraltete oder fehlende Masken dürfen exportiert werden; GUI und
CLI warnen. Die GUI bietet vor dem Export die Aktualisierung an. Eine Warnung
darf nicht stillschweigend in eine Neuberechnung umgewandelt werden.

## Lokale Anpassungen

Invertierung, Feathering, Blur, Dichte und lokale Regler werden als Rezept- oder
Masken-Layer-Daten gespeichert. Sie werden nicht in die Quellmatte gebrannt.
So kann dieselbe Matte in mehreren virtuellen Kopien unterschiedlich genutzt
werden.

## Implementierungsstatus (F-047 / F-080)

**Stand 2026-08-19 (F-047 Adapter-Crate `lumina-onnx` implementiert):**

- Der austauschbare ONNX-Adapter existiert als native-only-Crate `lumina-onnx`
  (spiegelt `lumina-raw`, nie im WASM-Build). Er entlastet `lumina-core` und
  kapselt native Inferenz, Modellverwaltung und Maskenartefakte.
- `ModelManifest` (serde) trägt Modellname, -version, -hash, Lizenz,
  Eingabespezifikation (Auflösung, Kanal-Layout, Tensorname/-format) und
  `ModelCapabilities`.
- `ModelCapabilities` (F-080) bildet `box_prompt`, `point_prompt`,
  `mask_prompt`, `class_detection` und `instance_segmentation` ab;
  `subject_segmentation` ist die Basisfähigkeit. Mindestens eine Fähigkeit muss
  gesetzt sein; unbekannte Felder werden abgelehnt (`deny_unknown_fields`).
- BiRefNet-Deskriptor (`birefnet_manifest`): automatische Subject-Segmentierung,
  ein RGB-Eingang → Alpha-Matte, keine Prompts (nur `subject_segmentation`,
  übrige Fähigkeiten `false`), dokumentierte Inferenzauflösung 1024×1024,
  Lizenz `Apache-2.0` (verifiziert, kein Download).
- Austauschbare Oberfläche über das Trait `SubjectInference`
  (`infer(&ImageFrame) -> Result<MaskPlane, OnnxError>`). Ein deterministischer
  `StubBackend` (zentrierte radiale Matte, rein aus Eingabedimensionen, keine
  Gewichte/Netz) ist die vollständige, getestete Standardoberfläche.
- `OnnxError` (thiserror) kennt `UnsupportedModel`, `InferenceFailed`,
  `InvalidDimensions`, `MissingModel` (keine stillen Fallbacks).
- Reales ONNX-Runtime-Backend ist hinter dem nicht-default Feature `onnx-rt`
  (`ort` v2.0.0-rc.13, in dieser Umgebung baubar) vorbereitet; die
  numerische Validierung gegen echte Modellgewichte folgt in F-048/F-082.

**Folgearbeit (F-048+):** Die Anbindung an Sidecar (Maskenidentität
`ModelIdentity` ↔ `ModelManifest`), CLI (`mask`-Command, `--update-masks`) und
GUI (Capability-Anzeige, Hintergrundberechnung) sowie die Persistenz/
Wiederverwendung/Stale-Erkennung erfolgt in den Folge-Tasks. `lumina-onnx`
hängt bewusst noch nicht von `lumina-sidecar` ab; die Modellidentität wird in
F-048 auf das Sidecar-Modell abgebildet.

## Abnahme

- Eine gültige Matte wird nach Neustart ohne Modell-Download verwendet.
- Ein verändertes Original markiert abhängige Masken als veraltet.
- Ein fehlendes oder beschädigtes Artefakt wird sichtbar gemeldet.
- Modellwechsel führen nicht zu stiller Neuberechnung.
- Eine Box- oder Pinsel-Prompt kann eine eigene Objektmaske erzeugen und
  zusammen mit ihrer Promptdefinition wieder geöffnet werden.
- Ein Modell ohne `mask_prompt` darf eine Pinselmaske nicht stillschweigend als
  gleichwertige Eingabe behandeln; die GUI zeigt die nicht unterstützte
  Fähigkeit an.
- Masken-Roundtrip, Prüfsumme, fehlendes Modell und Quelländerung sind getestet.
