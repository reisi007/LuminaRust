# Konflikte, Tests und Abnahme

**Feature:** F-011 Konflikt- und Releasequalität

## Inhaltsverzeichnis

- [Konfliktmatrix](#konfliktmatrix)
- [Abnahmeszenarien](#abnahmeszenarien)
- [Testanforderungen](#testanforderungen)
- [Bewusste Nichtziele](#bewusste-nichtziele)
- [Änderungsregeln](#änderungsregeln)

## Konfliktmatrix

| Konflikt | Erkennung | SOLL-Auflösung |
| --- | --- | --- |
| Original-Hash weicht ab | Sidecar-Hash gegen Quelle prüfen | Status `source_changed`, keine stille Überschreibung |
| Sidecar-Hauptversion unbekannt | Schema-Validator | Lesen verweigern oder explizite Migration |
| Sidecar beschädigt | JSON-/Prüfsummenfehler | Original offen lassen, Backup/Recovery anbieten |
| Maskenartefakt fehlt | relativen Pfad und Prüfsumme prüfen | Status `missing`, Neuberechnung optional |
| Modell-Hash abweichend | Maskenidentität prüfen | Alte Matte als `stale` anzeigen |
| Aktive Maske fehlt/veraltet | Maskenstatus vor Export prüfen | Warnen, Aktualisierung anbieten, Export trotzdem erlauben |
| Sidecar und DB widersprechen | Revision/Hash vergleichen | Sidecar gewinnt, Index aktualisieren |
| Zwei Prozesse schreiben | Revision/Lock/Atomic Write | Konflikt melden, kein stilles Last-Write-Wins |
| Virtuelle-Kopie-ID doppelt | Schema-Validierung | Sidecar ablehnen, korrigierbare Meldung |
| XMP widerspricht Lumina | Import-/Export-Prüfung | Lumina-Sidecar bleibt autoritativ |
| Pipeline nicht verfügbar | Versionsregistry | Render blockieren oder migrieren |
| RAW-Backend fehlt | Capability-Prüfung | Sidecar lesbar, Render nicht verfügbar |

## Abnahmeszenarien

### RAW-MVP

1. Eine CR2-, CR3-, NEF-, ARW- oder DNG-Datei wird über den nativen LibRaw-Adapter
   geöffnet.
2. EXIF-Orientierung und die vollständige Bildgeometrie werden übernommen.
3. Das Bild läuft durch denselben Core-/CLI-Renderpfad wie ein Rasterbild.
4. Ein optionaler RAW-Fixture-Test prüft Decode, Orientierung und Dimensionen;
   er wird ohne `LUMINA_RAW_FIXTURE` übersprungen und nicht als Golden bestanden
   gezählt. Lizenzgeeignete CR2/CR3/NEF/ARW/DNG-Dateien liegen unter
   `sample-data/raw/` oder werden extern über diese Variable referenziert.
   Die lokalen Testläufe lauten beispielsweise:
   `LUMINA_RAW_FIXTURE="$PWD/sample-data/raw/aircraft-landscape.cr3" rustup run stable cargo test -p lumina-raw -- --ignored`
   und
   `LUMINA_RAW_FIXTURE="$PWD/sample-data/raw/aircraft-portrait.cr3" rustup run stable cargo test -p lumina-raw -- --ignored`.
   Im Browser wird RAW als nicht verfügbare Fähigkeit ausgewiesen. Lens,
   Kamera-Farbmatrix und Profile bleiben bis zur Prüfung der konkreten
   LibRaw-Felder als F-034 offen.

### Sidecar ohne DB

1. Eine RAW-Datei erhält zwei virtuelle Kopien.
2. Die zentrale DB wird gelöscht oder war nie vorhanden.
3. Das Sidecar wird neben dem Original geöffnet.
4. Beide Kopien, Rezepte und Maskenreferenzen werden rekonstruiert.

### Persistierte Maske

1. Eine AI-Maske wird erzeugt und gespeichert.
2. Das ONNX-Modell wird entfernt.
3. Das Sidecar wird erneut geladen.
4. Die gespeicherte Maske wird ohne Inferenz verwendet.

### Ungültige Maske

1. Die Quelle wird verändert oder ausgetauscht.
2. Das Sidecar wird geladen.
3. Die Maske wird als `stale` markiert.
4. Neuberechnung bleibt eine explizite Aktion.

### Virtuelle Kopien

1. Kopie A erhält einen hellen Look und Subjektmaske.
2. Kopie B erhält einen dunklen Look ohne Maskenebene.
3. Beide werden exportiert.
4. Rezepte und Exporte bleiben unabhängig.

### DB-Wiederaufbau

1. Sidecars und Artefakte liegen vollständig auf dem Dateisystem.
2. Der optionale Index wird gelöscht.
3. Reindex liest alle Sidecars.
4. Fotos, Kopien, Status und Artefaktverweise sind wieder vorhanden.

### Auswahlbasierte Mehrbildbearbeitung

1. Mehrere Bilder werden in der GUI ausgewählt.
2. Eine globale Einstellung, Auto-Regel oder Maskenabsicht wird angewendet.
3. Jedes Bild erhält ein eigenes Sidecar und eigenes Rezept.
4. Zielmasken stehen je nach Berechnungsstand als `missing` oder `pending` zur
   Verfügung und werden nicht als gemeinsame Quellmatte ausgegeben.

### Preset-Anwendung

1. Ein Preset enthält nur explizit ausgewählte Felder.
2. Absolute Werte und gültige relative Exposure-Werte werden auf mehrere Bilder
   angewendet.
3. Relative Exposure ohne aktiviertes Auto-Tone wird abgelehnt.
4. Jedes Zielbild erhält genau einen neuen History-Schritt; die Quellhistorie
   wird nicht kopiert.

### Preview-Cache

1. Beim Verlassen eines Bildes wird standardmäßig nur die aktuelle Standard-
   Vorschau gespeichert.
2. Eine geerbte Ordneroption kann zusätzlich eine 1:1-Vorschau aktivieren.
3. Der Cache liegt unter `.lumina/`, ist nicht autoritativ und darf vollständig
   gelöscht oder bei nicht gefundenen Quellen automatisch entfernt werden.

## Testanforderungen

- Unit-Tests für Schema, Migration, IDs und atomare Writes
- JSON-Roundtrip- und Property-Tests für Wertebereiche und Rezeptdaten
- Sidecar-Recovery-, Konflikt- und parallele-Schreibtests
- Virtuelle-Kopien-Tests für Duplikation, Umbenennung und geteilte Artefakte
- Masken-Hit-, Miss-, Prüfsummen-, Modellwechsel- und Quelländerungstests
- Masken-DAG-, Zyklus-, Cross-Copy-Referenz- und Materialisierungstests
- Box-/Pinsel-Prompt-, SAM-Adapter- und nicht unterstützte Capability-Tests
- Source-Action-Tests vor Auto-Analyse und vor Maskenanwendung
- Golden-Image-Tests mit dokumentierten Toleranzen
- CLI-End-to-End-Tests mit Exit-Codes
- native Build-/Smoke-Tests
- Performance- und Speicherbenchmarks für RAW, Vorschau, Masken und Batch

RAW-Fixtures, Referenzbilder und AI-Modelle werden reproduzierbar versioniert
und mit Lizenzinformationen dokumentiert. Tests dürfen keinen spontanen
Netzwerk-Download benötigen.

## Bewusste Nichtziele

- Bearbeitung oder Überschreibung des Originals
- zentrale DB als Pflichtvoraussetzung
- unbemerkte AI-Neuberechnung beim Öffnen
- implizite Rezeptvererbung in v1
- zweite GUI-spezifische Renderpipeline
- automatische Modell-Downloads

## Änderungsregeln

- Das betroffene Feature-Dokument wird vor Implementierungsbeginn aktualisiert.
- Ein ungelöster Widerspruch zwischen SOLL und Code erzeugt eine offene
  Feature-ID in `Agents.todo.md`.
- Nach Implementierung und unabhängiger Verifizierung wird der erreichte
  Zustand im Feature-Dokument ergänzt.
- `Agents.todo.md` enthält nur offene Arbeit; bestätigte Aufgaben werden daraus
  entfernt.
