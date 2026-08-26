# Sidecar-Architektur

**Feature:** F-001 Sidecar-first

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Dateimodell](#dateimodell)
- [Manifest](#manifest)
- [Binärdaten](#binärdaten)
- [Persistenzregeln](#persistenzregeln)
- [Migration und Konflikte](#migration-und-konflikte)
- [Abnahme](#abnahme)

## Ziel

Das Sidecar ist die vollständige, portable und autoritative Persistenz für
Lumina-Bearbeitungen. Die zentrale Datenbank ist optional und darf keine Daten
enthalten, die nicht aus Sidecars rekonstruiert werden können.

## Dateimodell

Für `IMG_0001.ARW` soll folgende Struktur möglich sein:

```text
IMG_0001.ARW
IMG_0001.ARW.lumina.json
IMG_0001.ARW.lumina.zdata
```

Die JSON-Datei ist Manifest und Rezept-Sidecar. Die `.zdata`-Datei ist der
einzige autoritative Binär-Sidecar für Maskenpayloads und weitere persistente
binäre Entwicklungsartefakte. Vorschauen und Cache gehören nicht in diese zwei
Dateien.

## Manifest

Das Zielmodell enthält mindestens:

```json
{
  "format": "lumina-sidecar",
  "schema_version": 1,
  "source": {
    "relative_name": "IMG_0001.ARW",
    "content_hash": "...",
    "byte_length": 0,
    "modified_at": "...",
    "raw_format": "ARW",
    "orientation": 1
  },
  "pipeline_version": "...",
  "virtual_copies": [
    {
      "id": "vc-original",
      "name": "Original",
      "recipe": {},
      "mask_library": [],
      "mask_layers": [],
      "history": [],
      "export_records": []
    }
  ]
}
```

Jede Rezeptinstanz enthält zusätzlich `recipe_version`; diese Version wird
unabhängig von `pipeline_version` migriert. Virtuelle Kopien werden über ihre
stabile ID identifiziert und können als archivierte Kopie gelöscht und später
wiederhergestellt werden. Das JSON-Manifest ist die portable, browserlesbare
Repräsentation; native Binärannahmen sind auf den optionalen `.zdata`-Payload
beschränkt.

Alle Felder benötigen definierte Einheiten, Wertebereiche, optionale Zustände
und eine Migrationsstrategie, bevor das Schema eingefroren wird.

## Binärdaten

`.lumina.zdata` ist ein eigener, versionierter Container. Er enthält einen
Index, Prüfsummen und Zstd-komprimierte Kacheln. Maskenwerte werden als
normierte `uint16`-Werte im Bereich `0..65535` gespeichert. Der maximale
absolute Quantisierungsfehler beträgt `1/65535`.

### ZData-Format v1

Der optionale `lumina-sidecar`-Featurepfad `zdata` implementiert ein Little-
Endian-Format. Der 40-Byte-Header beginnt mit `LUMZDATA`, gefolgt von
Formatversion `u16 = 1`, Headerlänge, Recordanzahl, Indexoffset und Indexlänge.
Der Index steht am Dateiende und enthält pro Record ID-Länge, Tilekoordinaten,
Dimensionen, Recordoffset und Recordlänge. IDs sind UTF-8 und innerhalb eines
Containers eindeutig. Jeder Record enthält dieselben Tilemetadaten, die
unkomprimierte und komprimierte Länge, eine 32-Byte-BLAKE3-Prüfsumme der
unkomprimierten Little-Endian-`uint16`-Werte und die Zstd-Nutzlast. Beim Lesen
werden Magic, Version, Bounds, Dimensions-/Payload-Länge, Duplikate und
Prüfsumme vor der Ausgabe geprüft; feste Größenlimits verhindern übergroße
Allokationen. Random Access dekodiert nur den angeforderten Tile.

Die JSON-Datei speichert Masken-DAG, Maskenidentität und Referenzen; zdata
speichert ausschließlich die binären Maskenpayloads. Vorschauen und Cache
bleiben außerhalb beider Dateien. Atomare JSON- und zdata-Schreibvorgänge sind
jeweils gewährleistet; eine gemeinsame Zwei-Dateien-Transaktion ist ausdrücklich
noch offen.

### Artefaktstatus-Prüfung (`artifact_status`)

`artifact_status(bundle_root, reference)` meldet pro Artefakt `Available`,
`Missing` oder `Corrupt`. Stand 2026-08-25 (REVIEW-SIDECAR-STATUS-1,
REVIEW-SIDECAR-FOLLOWUP-1/-2):

- `Missing`: Pfad fehlt oder ist keine reguläre Datei.
- `Corrupt`: Die Datei existiert, ist aber nicht verwendbar — sie ist leer
  bzw. kleiner als der 8-Byte-Container-Magic, deklariert ein
  `.lumina.zdata`-Format (`"zdata"`, `"zdata-mask"`, `"lumina-zdata"` —
  Substring-Match auf `contains("zdata")`, da reale Producer unterschiedlich
  schreiben), ohne
  mit `LUMZDATA` zu beginnen, übersteigt das Containergrößenlimit oder — bei
  echten Containern — scheitert beim Parsen oder an der BLAKE3-Prüfung.
  Diese Prüfsummen werden beim Statuscheck aktiv verifiziert („eager“), nicht
  erst bei der ersten Tile-Anfrage; ein bitflipped Payload zählt damit nie als
  verfügbar.
- `Available`: alle obigen Prüfungen bestanden.

Diese Regeln gelten in jedem Build; ohne das `zdata`-Feature entfällt nur die
aktive Parse-/Prüfsummenverifizierung magic-tragender Dateien (dokumentierte
Grenze, kein stiller Fallback).

**Dokumentierte SOLL-Lücke (bewusst offen):** `artifact_status` validiert die
in der Reference deklarierte Auflösung (`width`/`height`) **nicht** gegen die
Record-Dimensionen des Bundles. Eine solche Validierung ist ohne den
konsumierenden Pipeline-Decoder nicht sound implementierbar:

1. Eine `ArtifactReference` trägt keine Record-ID(s); in einem gemeinsamen
   Bundle liegen mehrere Masken- und Repair-Region-Records, sodass die
   zugehörigen Records einer Reference innerhalb von `artifact_status` nicht
   identifizierbar sind. Eine Prüfung gegen alle Records würde korrekte
   Bundles fälschlich als `Corrupt` melden.
2. Repair-Region-Records führen regionslokale Dimensionen, die zu keiner
   Maskenebene in Beziehung stehen.
3. Die Zusammensetzung von Kacheln zu einer Maskenebene (Offsets, Kachelraster,
   Nachskalierung beim Lesen) ist Semantik des konsumierenden Loaders.

Die tiefere Auflösungsvalidierung gehört in den ladenden Pfad, der Masken-ID
und Dekontext kennt.

**Umgesetzt im ladenden Pfad (Stand 2026-08-26, REVIEW-SIDECAR-LOADER-RES):**
Der Maskenloader `lumina-core::mask_loader` (`resolve_mask_planes`) führt
diese Validierung aus: Die dekodierten Dimensionen jeder geladenen Maske
müssen der in ihrem eigenen `ArtifactReference`-Record deklarierten Auflösung
entsprechen. Bei Abweichung gilt das Artefakt als `Corrupt`: Es wird nie
stillschweigend resampelt und nie als bestätigt gültig geladen, sondern — je
nach Modellverfügbarkeit — neu inferiert oder gemäß F-051 mit expliziter
Warnung aus dem Cache verwendet; ohne Cache und Modell folgt ein lauter
Fehler. In den Ergebnis-Kopien wird der Maskenstatus auf `Corrupt` gesetzt
(eine erfolgreiche Re-Inferenz löst das wieder auf). Fehlt ein
ArtifactReference-Record, gibt es keine Referenzauflösung zum Vergleich — dann
gilt unverändert das bisherige Verhalten (Artefakt nicht bestätigbar ⇒
F-051-Pfade). Die Lücke bleibt damit bewusst auf `artifact_status` selbst
begrenzt: Der Statuscheck meldet weiterhin nur `Missing`/`Available`/`Corrupt`
anhand der obigen Dateiregeln und validiert keine Auflösungen; die
Verantwortungsgrenze liegt dokumentiert zwischen Statuscheck (Datei-Ebene) und
ladendem Pfad (Ebenen-Ebene).

Der Container muss Random Access auf einzelne Kacheln erlauben und darf später
weitere Artefakttypen aufnehmen. OpenEXR und 7z sind für dieses Arbeitsformat
nicht erforderlich. Ein Preset enthält keine binären Maskenpayloads.

## Persistenzregeln

- Relative Pfade sind portabel; absolute Pfade sind verboten.
- Sidecars werden über temporäre Dateien und atomaren Rename geschrieben.
- Ein beschädigtes Sidecar macht das Original nicht unlesbar.
- Ein fehlendes Sidecar bedeutet unbearbeitetes Bild mit sicherem Default-Rezept.
- Unbekannte Hauptversionen werden nicht stillschweigend verändert.
- JSON-Roundtrip darf keine Daten verlieren.
- In Schema-Version 1 werden unbekannte JSON-Felder auf allen persistierten
  Domain-Strukturen über `serde(flatten)` als Extras erhalten. Dadurch bleiben
  Sidecars bei Erweiterungen roundtrip-fähig, ohne unbekannte Semantik
  auszuwerten; inkompatible Hauptversionen werden weiterhin abgelehnt.
- Crash-Recovery, Backups und parallele Schreibkonflikte werden getestet.
- Das Verschieben eines vollständigen Sidecar-Bundles erhält alle relativen
  Referenzen.
- Ein Preset wird als einzelne `.lumina-preset.json`-Datei aus ausgewählten
  Rezeptfeldern erzeugt und ist kein vollständiges Bild-Sidecar.
- XMP wird in v1 weder gelesen noch geschrieben. Ein späterer XMP-Adapter darf
  keine Lumina-Sidecar-Daten als nichtautoritative Alternative behandeln.

### Umgesetzter Stand (Review-Batch 2026-08-25, verifiziert)

- **Schreibserialisierung:** Alle JSON-Schreibzugriffe laufen über einen
  Crate-Lock (`save_sidecar`, `save_sidecar_if_unchanged`,
  `migrate_sidecar_file`); CAS prüft die Revision innerhalb der kritischen
  Sektion. Plain-`save_sidecar` meldet Lock-Kontention als
  `Conflict`/`Io(lock)` statt stillem Last-Write-Wins. Das bewusst lock-freie
  `write_atomically` bleibt Artefakt-Ausgabepfad (CLI/GUI-Export).
- **zdata:** Read-Modify-Write (`append_repair_region`, `save_zdata`) nimmt die
  gemeinsame `.zdata.lock`; `load_zdata` verifiziert BLAKE3-Prüfsummen **eager**
  beim Laden (nicht mehr lazy beim Tile-Zugriff). Korrekte Container bis 512 MB:
  bewusster Korrektheit-vor-Geschwindigkeit-Trade-off.
- **Artefaktstatus:** `artifact_status` unterscheidet `Missing`, `Available`
  und neu `Corrupt` (leere Datei, <8-Byte-Container ohne Magic,
  zdata-deklariertes Format ohne `LUMZDATA`-Magic, fehlende/falsche Magic,
  Parse- oder Prüfsummenfehler). Reference-width/height wird bewusst nicht
  gegen Bundle-Records validiert — siehe oben „Artefaktstatus-Prüfung
  (`artifact_status`)“ für Begründung und Verantwortungsgrenze.
- **Recovery/Validierung:** `recover_sidecar` entfernt Temp-Dateien erst ab
  mtime-Schwelle (lebende Writer bleiben unberührt); Migrationstempfiles tragen
  das Präfix `.{name}.tmp-` und sind damit sweeponfähig. `delete_virtual_copy`
  & Co. validieren vor der Mutation und rollen bei Fehlern auf den exakten
  Vorzustand zurück. `load_sidecar` begrenzt das Lesen vor dem Einlesen
  (Metadata-Gate + Größenlimit) und lehnt `schema_version` 0 laut ab — der
  historische Bump läuft ausschließlich über den expliziten Migrationspfad.
- **Verbraucherhinweis:** Konsumenten müssen auf `!= Available` prüfen, um
  `Corrupt` zu erfassen (CLI tut dies; GUI seit 2026-08-25 ebenfalls).

## Migration und Konflikte

**Pre-Alpha-Entscheidung (2026-08-23, Produkteigentümer):** Solange LuminaRust
in der **Pre-Alpha-Phase** ist, bleibt `schema_version` bei **1** und darf sich
**inkompatibel ändern** (Feldtypen, Pflichtfelder, Semantik) **ohne
Migrationspfad und ohne Versionsbump**. Migrations-, Backup- und
Bestätigungsflows unten sind MVP-/Beta-Zielzustand, nicht Pre-Alpha-Pflicht.
Ein Loader darf Sidecars, die nicht mehr zum aktuellen Schema passen, **laut
ablehnen** (sichtbarer Fehler, Original unberührt) — kein stiller Fallback,
keine stillschweigende Best-Effort-Interpretation. Alte Test-Sidecars werden
in der Pre-Alpha neu erzeugt oder gelöscht, nicht migriert.

Schema- und Pipelineversion werden getrennt behandelt. Ein altes Sidecar bleibt
lesbar, solange keine inkompatible neue Funktion verwendet wird. Dann wird eine
Migration angeboten; nach Bestätigung wird ein Backup geschrieben und atomar
überschrieben. Im CLI ist dafür ein ausdrückliches Migrationsflag erforderlich.

Widerspricht ein optionaler Index dem Sidecar, gewinnt das Sidecar. Bei
gleichzeitigen Sidecar-Schreibvorgängen wird ein Konflikt gemeldet; eine
stille Last-Write-Wins-Policy ist nicht zulässig.

## Abnahme

- Ein RAW kann ohne DB geöffnet und bearbeitet werden.
- Das Löschen und Wiederaufbauen der DB verändert keine Bearbeitungsdaten.
- Beschädigte Sidecars führen zu einem sichtbaren Fehler und nicht zu einer
  Änderung des Originals.
- Sidecar- und Recovery-Tests sind durch einen unabhängigen Verifizierungs-Agenten
  bestätigt.
