# Virtuelle Kopien

**Feature:** F-002 Virtuelle Kopien, F-014 Standardkopie-Regeln

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Datenmodell](#datenmodell)
- [Geteilte Artefakte und Referenzen](#geteilte-artefakte-und-referenzen)
- [Operationen](#operationen)
- [Standardkopie-Regeln (F-014)](#standardkopie-regeln-f-014)
- [Presets (F-009)](#presets-f-009)
- [Abnahme](#abnahme)

## Ziel

Mehrere Looks desselben Originals werden ohne RAW- oder Pixelduplikation
gespeichert. Jede Kopie ist eine eigenständige Entwicklung und kann separat
angezeigt, bearbeitet und exportiert werden.

## Datenmodell

Jede Kopie besitzt:

- stabile, sidecarweit eindeutige ID;
- Anzeigenamen;
- vollständiges, selbständig auswertbares Rezept;
- eigenen Masken-Layer-Zustand;
- eigenen Crop und eigene Geometrie;
- eigenen Exportstatus und eigene Exporthistorie;
- optional einen Status als aktuelle Standardkopie.

Die Standardkopie wird bei der Sidecar-Erstellung erzeugt und darf nicht
stillschweigend verschwinden. Kopien werden nicht über Arraypositionen
identifiziert.

Implizite Vererbung zwischen Kopien ist in v1 nicht vorgesehen. Eine Kopie kann
aus einer anderen erzeugt werden, erhält dabei aber ein vollständiges Rezept.

## Geteilte Artefakte und Referenzen

Jede virtuelle Kopie besitzt eine eigene Maskenbibliothek. Eine Kopie darf aber
auf stabile Maskenknoten einer anderen Kopie verweisen. Invertierung,
Feathering, Blur, Dichte und lokale Anpassungen liegen in der jeweils
referenzierenden Kopie.

Wird eine Quellkopie gelöscht, werden abhängige Graphdefinitionen automatisch
in die Zielbibliothek materialisiert. Die binären Payloads dürfen über
Content-Hash dedupliziert bleiben. Das Löschen der Quellkopie darf dadurch
keine aktive Zielmaske beschädigen.

Nicht mehr referenzierte Artefakte werden nur über einen expliziten und sicheren
Aufräumvorgang entfernt.

## Operationen

Zu unterstützen sind Erstellen, Duplizieren, Umbenennen, Sortieren, Aktivieren,
Zurücksetzen, Exportieren und Löschen. Jede Operation muss per Sidecar-
Roundtrip über Neustarts hinweg erhalten bleiben.

## Standardkopie-Regeln (F-014)

- In jedem Sidecar existiert jederzeit genau eine als Standardkopie
  gekennzeichnete Kopie. Sie wird beim Erstellen des Sidecars angelegt und darf
  weder durch eine UI-Aktion noch durch einen stillen Bereinigungsvorgang
  gelöscht werden.
- Das Umbenennen ändert ausschließlich den Anzeigenamen. Die stabile,
  sidecarweit eindeutige ID, das Rezept und die History bleiben unverändert.
- Kopien dürfen nach Anzeigename, Erstellzeitpunkt oder einer anderen
  ausdrücklich gewählten Sortierung neu angeordnet werden. Die Arrayposition
  ist niemals Identität; stabile IDs bleiben bei Umbenennung und Sortierung
  unverändert.
- Duplizieren erzeugt eine neue, stabile und sidecarweit eindeutige ID. Die
  duplizierte Kopie erhält ein vollständiges, eigenständig auswertbares Rezept
  sowie eine eigene History und eigene Aktivierungs-/Exportstatuswerte. Eine
  implizite Vererbung vom Duplikat oder von der Quellkopie ist nicht zulässig;
  gemeinsam nutzbare Artefakte müssen ausdrücklich als Referenz persistiert
  werden.
- Die Standardkopie darf nur gelöscht werden, wenn zuvor eine andere Kopie
  ausdrücklich als Standardkopie ausgewählt wurde und diese Übertragung
  erfolgreich persistiert ist. Gibt es keine andere Kopie, ist die Löschung
  verboten. Eine Löschaktion darf die Standardkopie nicht stillschweigend
  zurücksetzen, umbenennen oder durch eine neu erzeugte Kopie ersetzen.
- Das Löschen einer anderen Kopie entfernt sie aus der aktiven Kopienliste und
  schreibt den Vorgang in die persistente Sidecar-History. Abhängige geteilte
  Maskenartefakte werden gemäß den Referenzregeln materialisiert oder erhalten;
  andere Kopien dürfen dadurch nicht beschädigt werden.
- Wiederherstellen ist nur möglich, wenn die persistente History die
  vollständige Kopie einschließlich ID, Rezept, Masken-/Geometriestatus und
  relevanter Exportdaten enthält. Fehlt diese History oder ist sie
  inkompatibel, wird die Kopie nicht still rekonstruiert; die GUI muss die
  Wiederherstellung als nicht verfügbar beziehungsweise als Fehler anzeigen.

## Presets (F-009)

Ein Preset ist eine wiederverwendbare Rezeptvorlage. Es ist an kein Bild, kein
Sidecar und keine virtuelle Kopie gebunden: Ein Preset enthält keinen
Quellhash, keine Quellidentität, keine Geometrie- oder Maskenbezugswerte und
keine binären Maskenpayloads (siehe auch `architecture/sidecar.md`). Es enthält
nur die vom Nutzer beim Erstellen explizit ausgewählten Rezeptfelder.

### Dateiformat

- Ein Preset liegt als einzelne Datei `<name>.lumina-preset.json` neben
  weiteren Presetdateien im Presetverzeichnis (festgelegte Entscheidung in
  `feature/README.md`). Es ist kein Bild-Sidecar.
- Die Datei ist ein versionierter JSON-Umschlag:

  ```json
  {
    "format": "lumina-preset",
    "schema_version": 1,
    "preset": {
      "id": "preset-<blake3-hex>",
      "name": "<Anzeigename>",
      "recipe": { "...": "Teilrezept" }
    }
  }
  ```

- `preset` nutzt exakt die Sidecar-`Preset`-Struktur (`id`, `name`, `recipe`
  mit `extras`). Das Rezeptmodell ist damit identisch zum Sidecar-Rezept;
  ein Preset-Rezept ist ein Teilrezept bestehend aus den ausgewählten
  Adjustment-Werten und der Option `exposure_semantics`
  (`absolute` | `relative`).
- `schema_version` des Presets ist unabhängig von der Sidecar-Schema-Version.
  Fremde Hauptversionen werden abgelehnt, nicht still migriert.
- Relative Exposure ist ohne aktiviertes Auto-Tone am Zielbild ungültig und
  wird bei der Anwendung abgelehnt (festgelegte Entscheidung in
  `feature/README.md`).

### Speicherort

- Presets liegen benutzerglobal unter
  `<Konfigurationsverzeichnis>/lumina/presets/`. Konkret: macOS
  `~/Library/Application Support/lumina/presets`, Linux/XDG
  `$XDG_CONFIG_HOME/lumina/presets` (sonst `~/.config/lumina/presets`),
  Windows `%APPDATA%\lumina\presets`.
- Begründung: Presets sind per Definition bild- und ordnerunabhängig; eine
  Ablage neben einem einzelnen Bild wäre semantisch falsch und würde
  Fotoordner mit Steuerdateien vermüllen. Der benutzerglobale Ordner macht
  Presets über Projekte hinweg nutzbar und als einzelne Dateien teilbar.
- Der Ordnername folgt der `.lumina.*`-Dateifamilie; die abschließende
  Produktbenennung (NAMING-F1, offen) kann ihn später umbenennen und
  migrieren.
- Ist das Konfigurationsverzeichnis nicht ermittelbar, wird das Speichern und
  Laden von Presets als nicht verfügbar angezeigt — kein stiller Fallback in
  ein anderes Verzeichnis.

### Dateinamen- und Kollisionsregeln

- Der Dateiname ergibt sich aus dem Anzeigenamen plus der Endung
  `.lumina-preset.json`. Der Anzeigename selbst bleibt unverändert im JSON.
- Namen, die leer sind oder Pfadtrenner (`/`, `\`), NUL-, Steuerzeichen,
  `.`/`..` als Gesamtnamen enthalten, werden mit einem Fehler abgelehnt — es
  wird nichts still umgeschrieben oder bereinigt.
- Der Anzeigename ist die Identität eines Presets: Speichern unter einem
  bereits existierenden Namen ist eine ausdrückliche Aktualisierung und
  ersetzt die Datei atomar. Die Low-Level-Speicherfunktion verlangt dafür
  einen ausdrücklichen `overwrite`-Parameter; ohne ihn ist eine Kollision ein
  Fehler.

### Validierung und Fehlerverhalten

Beim Laden werden laut abgelehnt (kein stiller Fallback, keine stille
Normalisierung):

- nicht parsebares JSON;
- abweichender `format`-Wert;
- unbekannte/fremde `schema_version`;
- leerer Anzeigename;
- Adjustment-Keys außerhalb des Raster-MVP-Satzes (`exposure`,
  `contrast`, `highlights`, `shadows`) sowie nicht-endliche Werte oder Werte
  außerhalb der Pipeline-Bereiche (`exposure` −10…10, übrige −1…1);
- Rezepte, die außerhalb von `adjustments` und `exposure_semantics` vom
  Default-Rezept abweichen (ein Preset darf zum Beispiel weder Auto-Features
  aktivieren noch Geometrie oder Kurven setzen);
- ein `exposure_semantics`-Wert außerhalb von `absolute` | `relative`
  (fehlender Wert gilt als `absolute`).

Beim Auflisten des Presetverzeichnisses wird jede unlesbare bzw. invalide
Datei einzeln mit Dateiname und Fehlergrund angezeigt; Dateien werden nie
still übersprungen.

### Anwendung

- Die Reihenfolge bleibt Source-Actions → Auto-WB/Auto-Tone → Preset →
  Masken → Matching (festgelegte Entscheidung in `feature/README.md`).
- Absolute Werte überschreiben die entsprechenden Zielwerte. Relative
  Exposure wird auf den aktuellen Zielwert addiert und nur bei aktiviertem
  Auto-Tone akzeptiert.
- Jede Anwendung erzeugt auf der aktiven virtuellen Kopie genau einen neuen
  History-Schritt; die Quellhistorie des Presets wird nicht kopiert
  (Akzeptanzszenario „Preset-Anwendung“ in
  `quality/conflicts-and-acceptance.md`).

### Plattformgrenze und Umsetzungsstand

- Dateibasierte Presets sind eine native GUI-Fähigkeit; im WASM-Build bleiben
  sie Post-MVP (`platform/capability-matrix.md`, „Virtuelle Kopien /
  Presets“). Das In-Memory-Erstellen/Anwenden bleibt plattformübergreifend
  verfügbar.
- Umgesetzt (GUI v1): Speichern des aktuellen Ausschnitts als
  `<name>.lumina-preset.json` über das geteilte atomare Schreibmuster
  (`lumina_sidecar::write_atomically`), Laden/Validieren gemäß obigen Regeln,
  dateibasierte Presetliste im Develop-Panel mit lauter Fehleranzeige pro
  invalider Datei.
- Das Sidecar-Feld `presets` bleibt Schema-Bestandteil, wird von der GUI v1
  jedoch nicht mehr als Quelle der Presetliste verwendet; die Liste ist
  ausschließlich dateibasiert.

## Abnahme

- Zwei Kopien desselben RAW besitzen unterschiedliche Rezepte und Exporte.
- Eine Kopie kann eine Maske verwenden, während eine andere sie deaktiviert.
- Umbenennen und Sortieren verändern keine IDs.
- Das Löschen einer Kopie beschädigt keine andere Kopie und kein geteiltes
  Artefakt.
- Ein gespeichertes Preset lädt als identisches Rezept zurück (Roundtrip ohne
  Quellbezug).
- Eine Presetdatei mit fremder `schema_version`, falschem `format` oder
  korruptem JSON wird mit lauter Fehlermeldung abgelehnt, nicht still
  ignoriert oder normalisiert.
- Relative Exposure wird ohne aktiviertes Auto-Tone am Ziel abgelehnt.
- Die Tests werden unabhängig auf fachliche Korrektheit und Abdeckung geprüft.
