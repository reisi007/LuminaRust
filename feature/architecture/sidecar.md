# Sidecar-Architektur

**Feature:** F-001 Sidecar-first

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Dateimodell](#dateimodell)
- [Manifest](#manifest)
- [Binärdaten](#binärdaten)
- [Persistenzregeln](#persistenzregeln)
- [Metadaten: Keywords, Sammlungen, Stapel-Ops (G-15 META-MVP, Slice 1)](#metadaten-keywords-sammlungen-stapel-ops-g-15-meta-mvp-slice-1)
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

### Bewertung und Flaggen (LR-01)

Jede virtuelle Kopie trägt Lightroom-ähnliche Organisationsmetadaten (keine
Bilddaten, keine Rezeptwerte):

- `rating: u8` — Sternebewertung `0..=5`, wobei `0` „unbewertet“ bedeutet.
  Werte `> 5` werden beim Laden laut abgelehnt (kein Clamping, kein stiller
  Fallback).
- `flag` — Pick-Status: `unflagged` (Default), `pick` oder `reject`.

Beide Felder sind je virtueller Kopie gespeichert (nicht je Quelle): Zwei
Kopien desselben Originals können unterschiedliche Bewertungen tragen.
`duplicate_virtual_copy` übernimmt Bewertung und Flag der Quellkopie als
Startwerte; danach sind sie unabhängig. Fehlende Felder in älteren Sidecars
werden als `rating = 0` / `flag = unflagged` gelesen (additive Erweiterung,
`schema_version` bleibt in der Pre-Alpha-Phase 1).

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
- **zdata generativ (GEN-ZDATA-PERSIST, 2026-09-03, 1e0ccbd, verifiziert
  BESTANDEN):** Zwei weitere RGBA8-Artefakttypen teilen dasselbe Bundle mit
  eigenem `kind`-Diskriminator bei unverändertem Container-`VERSION = 1`:
  `GenerativeCanvasArtifact` (`kind = 2`, GEN-EXPAND-1 `generative_canvas`,
  volles kompositiertes Canvas) und `SpotHealGenerativeArtifact` (`kind = 3`,
  SPOT-REMOVE-1 `spot_heal_generative`, ersetzte Spot-Kachel ohne
  Canvas-Expansion). Rohformat je Record: `encoding_version u32 (= 1)` +
  `width`/`height u32` + `width*height*4` RGBA8-Bytes; BLAKE3 über den
  unkomprimierten Rohstrom (`checksum()`), Lese-/Schreibvalidierung von ID,
  Dimensionen und Payload-Split, strikte Kind-Trennung (ein `generative_canvas`-
  Record wird nie als Spot-Heal gelesen und umgekehrt), Duplikat-IDs über alle
  Kinds abgelehnt. `append_generative_canvas`/`append_spot_heal_generative`
  laufen unter derselben `.zdata.lock` und schreiben atomar (Temp + Rename);
  WASM-Stub ersatzlos gestrichen (2026-09-04, kein stiller
  Fallback nötig — kein WASM-Build mehr). Tests: `88p` ohne / `124p` mit `zdata`-Feature (Stand 1e0ccbd),
  Clippy/Format (`--features zdata`) gruen.
- **Rezept-Verlinkung generativ (GEN-ZDATA-LINK-1, 2026-09-03, 69dad91,
  verifiziert BESTANDEN):** Typisierte, additive Schema-v2-Rezeptfelder
  verknuepfen `GenerativeEdit` (`generative_canvas`, kind=2) und
  `spot_removals` (`spot_heal_generative`, kind=3) per Record-ID +
  `ArtifactReference` (relativer Pfad, Format, BLAKE3-Pruefsumme, Aufloesung,
  Kanaltyp, Datenversion) mit den `.lumina.zdata`-Records; Validierung lehnt
  unbekannte Versionen laut ab (gegenseitige Ausschlussregeln je Modus),
  JSON-Roundtrip + `artifact_status`-Abdeckung
  (`Available`/`Missing`/`Corrupt` eager), relative Pfade bleiben nach
  Bundle-Verschiebung gueltig, Writes atomar. Tests: sidecar `96p` ohne /
  `134p` mit `zdata`-Feature (Stand 69dad91; Folge-Fixes c000c6f/1bbb564:
  `101p`/`139p`), Clippy/Format/wasm gruen. Offene Folgearbeit: Link-Felder
  in `recipe_hash`/RenderKey (GEN-RENDERKEY-LINK-1, siehe `Agents.todo.md`
  Block A).
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

## Metadaten: Keywords, Sammlungen, Stapel-Ops (G-15 META-MVP, Slice 1)

Normativer SOLL-Stand 2026-09-03 (Slice 1 = nur Sidecar-Schema + Persistenz in
`lumina-sidecar`; CLI/GUI-Anbindung sind Folge-Slices):

- **Keywords liegen auf Quellbild-Ebene** (`SidecarDocument.keywords`,
  `Vec<String>`), nicht je virtueller Kopie. Begründung: Keywords beschreiben
  den Bildinhalt (wie Masken-Artefakte auf Quellenebene geteilt); pro-Kopie-
  Keywords würden Filter, Sync und Smart-Sammlungen fragmentieren und die
  Stapelvergabe verkomplizieren. Bewertung (`rating`) und Flag bleiben bewusst
  je Kopie (LR-01). Validierung (laut, kein stiller Fallback): Eintrag muss
  getrimmt-nicht-leer sein, darf keine führenden/folgenden Whitespaces und
  keine Steuerzeichen enthalten, ist auf 128 Zeichen begrenzt, maximal 512
  Einträge je Sidecar; exakte Duplikate werden abgelehnt (kein stilles
  Deduplizieren). Keyword-Vergleiche (Smart-Regeln) sind exakter,
  case-sensitiver Vergleich (deterministisch, keine Locale-Abhängigkeit).
- **Statische Sammlungen sind Sidecar-first:** Die Mitgliedschaft wird pro
  Sidecar als `SidecarDocument.collections: Vec<CollectionMembership>`
  (`{ id, name }`, Quellbild-Ebene) persistiert. Es gibt keine
  DB-autoritative Sammlungsliste: Der optionale Index baut Sammlungen durch
  Scannen der Sidecars wieder auf (Rebuild-Pflicht wie F-064–F-067).
  Umbenennen einer Sammlung ist eine Stapel-Operation über alle betroffenen
  Sidecars (kein stilles Auseinanderlaufen von `id`→`name`). Validierung:
  `id`/`name` nicht-leer, ohne führende/folgende Whitespaces, `id` ohne
  `/`, `\`, `:` (portabel, nie pfadartig); maximal 512 Mitgliedschaften je
  Sidecar. Keine absoluten Pfade.
- **Smart-Sammlungen sind versionierte Daten, keine DB-Logik:** Der Regel-AST
  (`SmartCollectionDef { version = 1, id, name, rule: SmartRule }`) und der
  reine, deterministische Evaluator (`SmartRule::matches`,
  `SmartCollectionDef::matches_copy/matches_any_copy`) leben in
  `lumina-sidecar`. Auswertungs-Inputs sind ausschließlich Sidecar-Felder
  (`keywords` der Quelle + `rating`/`flag` der jeweiligen Kopie). Die
  Regel-Bibliothek (alle Smart-Definitionen eines Katalogs) wird **nicht** pro
  Bild-Sidecar dupliziert (Umbenennungen/Edits würden sonst divergieren); ihre
  portable Datei-Persistenz ist Folge-Slice (CLI/GUI) und nutzt exakt diese
  versionierten Typen als Format. Regel-Validierung (laut): `version` muss 1
  sein, `And`/`Or` brauchen mindestens eine Unterregel, maximale
  Verschachtelungstiefe 32, Keyword-/Rating-Teilregeln unterliegen denselben
  Grenzen wie ihre Sidecar-Gegenstücke (`rating <= 5`).
- **Stapel-Operationen als Datenmodell:** `BatchOp` (`add_keyword`,
  `remove_keyword`, `add_to_collection`, `remove_from_collection`,
  `set_rating`, `set_flag`) plus `apply_batch_op(document, op) -> bool
  (changed)` ist die einzige Metadaten-Mutationssprache dieses Slices. Sie ist
  idempotent (vorhandenes Keyword erneut hinzufügen → `Ok(false)`, kein
  Duplikat), validiert laut (unbekannte `copy_id`, `rating > 5`, ungültige
  Keywords/Sammlungen) und mutiert nie Rezepte, Masken oder History. CLI/GUI-
  Slices iterieren damit über Sidecar-Dateien (je Datei atomar via
  `save_sidecar`/`save_sidecar_if_unchanged`); Datei-Iteration selbst gehört
  nicht zu diesem Slice.
- **Schema-Version-Entscheid: kein Bump, `schema_version` bleibt 2.**
  Alle Felder sind additiv-optional (`#[serde(default,
  skip_serializing_if = "Vec::is_empty")]`): abwesend = leer = altes
  Verhalten, alte Sidecars laden ohne Datenverlust und serialisieren ohne
  neue Schlüssel zurück. Die Migrations-Maschinerie (`migrate_json`,
  `migrate_sidecar_file`, `.bak`-Backup, atomarer Replace) bleibt
  unverändert; der v1→v2-Musterpfad deckt alte Dateien ab. Unbekannte Felder
  bleiben via `serde(flatten)`-Extras roundtrip-fähig; inkompatible
  Hauptversionen werden laut abgelehnt.

Ankerpunkte für Folge-Slices: `SidecarDocument::{keywords, collections}`,
`CollectionMembership`, `SmartCollectionDef`/`SmartRule`
(+ `matches_copy`/`matches_any_copy`), `BatchOp`/`apply_batch_op`.

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
