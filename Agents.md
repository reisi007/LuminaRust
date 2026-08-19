# LuminaRust Agentenregeln

Dieses Dokument definiert die verbindlichen Arbeitsregeln für alle Agenten im
Projekt. `Agents.todo.md` ist der ausführbare Arbeitsplan. `feature/README.md`
ist der Einstiegspunkt zum normativen Feature-SOLL. Die verlinkten Dokumente
unter `feature/` müssen vor der Implementierung eines Features gelesen und bei
Änderungen zuerst aktualisiert werden.

## Inhaltsverzeichnis

- [Produktprinzipien](#produktprinzipien)
- [Persistenz und Sidecars](#persistenz-und-sidecars)
- [Architekturgrenzen](#architekturgrenzen)
- [Rollen und Delegation](#rollen-und-delegation)
- [Verbindlicher Arbeitsablauf](#verbindlicher-arbeitsablauf)
- [Verifizierung und Tests](#verifizierung-und-tests)
- [Definition of Done](#definition-of-done)
- [Dokumentations- und Todo-Regeln](#dokumentations--und-todo-regeln)
- [Änderungsregeln](#änderungsregeln)

## Produktprinzipien

- LuminaRust ist ein nicht-destruktiver RAW-Prozessor mit Lightroom-ähnlichem
  Bearbeitungsmodell.
- Das Originalbild wird niemals verändert, verschoben, überschrieben oder
  durch einen Export ersetzt.
- Eine Bearbeitung wird als deklaratives, versioniertes Rezept gespeichert.
- Vorschauen, Histogramme, Renderings, Exporte und AI-Ergebnisse sind aus dem
  Original und dem Rezept ableitbare Artefakte.
- Ein erneutes Öffnen eines Projekts darf keine erneute AI-Inferenz benötigen,
  wenn eine gültige persistierte Maske vorhanden ist.
- Reproduzierbarkeit ist wichtiger als ein stiller Fallback. Fehlende oder
  inkompatible Artefakte werden sichtbar als veraltet oder nicht verfügbar
  gemeldet.
- Jede sichtbare Funktion muss im passenden Feature-Dokument beschrieben sein,
  bevor ihre Implementierung beginnt.

## Persistenz und Sidecars

### Quelle der Wahrheit

- Das Sidecar ist die primäre und vollständige Quelle für Lumina-Bearbeitungen.
- Die empfohlene Datei liegt direkt neben dem Original und heißt
  `<original-dateiname>.lumina.json`, zum Beispiel
  `IMG_0001.ARW.lumina.json`.
- Das Original bleibt unverändert und kann ohne Sidecar weiterhin als RAW-Datei
  gelesen werden.
- Eine zentrale Datenbank ist nicht erforderlich, damit Bearbeitungen korrekt
  gespeichert oder wiederhergestellt werden können.
- Eine optionale SQLite-Datenbank darf nur einen wiederaufbaubaren Index,
  Suchdaten, Jobstatus und Cache-Metadaten enthalten. Sie ist niemals die
  einzige Quelle für Rezept, virtuelle Kopien oder Maskendefinitionen.
- XMP wird in v1 nicht unterstützt. Ein späterer XMP-Adapter darf Lumina-
  spezifische Daten niemals stillschweigend zur autoritativen Quelle machen.

### Sidecar-Struktur

- Das JSON-Sidecar enthält Schema-Version, Quellidentität, virtuelle Kopien,
  Rezeptdaten, Maskenmetadaten und Verweise auf binäre Maskenartefakte.
- Große Masken werden nicht als unkomprimierte Float-Arrays in das JSON
  geschrieben. Sie werden im binären Sidecar
  `<original-dateiname>.lumina.zdata` gespeichert.
- Jeder binäre Artefaktverweis enthält mindestens relativen Pfad, Format,
  Prüfsumme, Auflösung, Kanaltyp und Datenversion.
- Relative Verweise müssen beim Verschieben eines gesamten Sidecar-Bundles
  gültig bleiben. Absolute Pfade sind in persistenten Rezeptdaten verboten.
- Sidecar- und Maskenänderungen werden atomar geschrieben. Unvollständige
  temporäre Dateien dürfen nach einem Abbruch nicht als gültig erkannt werden.

### Virtuelle Kopien

- Eine virtuelle Kopie ist eine eigenständige Bearbeitungsvariante desselben
  Originals und benötigt keine Kopie der RAW-Pixel.
- Jede virtuelle Kopie besitzt eine stabile, innerhalb des Sidecars eindeutige
  ID, einen Anzeigenamen, ein eigenes Rezept und einen eigenen Aktivierungs-
  beziehungsweise Exportstatus.
- Ein Sidecar enthält mindestens eine Standardkopie. Die Standardkopie darf
  nicht stillschweigend gelöscht werden.
- Maskeninferenz und gespeicherte Matte können auf Quellbild-Ebene geteilt
  werden. Masken-Layer, Invertierung und lokale Anpassungen gehören zur
  jeweiligen virtuellen Kopie.
- Virtuelle Kopien dürfen nicht über ihre Position im JSON-Array identifiziert
  werden. IDs bleiben bei Umbenennung und Neuordnung stabil.
- Eine virtuelle Kopie darf nicht durch ein zweites Original-Sidecar dupliziert
  werden, ohne dass die Beziehung ausdrücklich dokumentiert ist.
- Vererbungen zwischen virtuellen Kopien sind bis zu einer ausdrücklichen
  Schemaentscheidung nicht implizit. Eine Kopie besitzt im ersten Zielumfang
  ein vollständiges, selbständig auswertbares Rezept.

### AI-Masken

Eine persistierte AI-Maske muss mindestens folgende Identität tragen:

- Quell-Content-Hash und relevante Decode-/Geometrieparameter
- Modellname, Modellversion und Modell-Hash
- Inferenzauflösung, Vorverarbeitung und Nachskalierungsverfahren
- Koordinatensystem und Ausrichtung des Quellbildes
- Matte-Format, Auflösung, Kanäle und Prüfsumme
- Erstellungszeitpunkt, Status und optionaler Fehlertext
- Bearbeitungsparameter wie Invertierung, Feathering und Blur

Eine Maske ist gültig, wenn Quelle, Decode-Kontext, Modellkontext und Artefakt-
Prüfsumme übereinstimmen. Bei einer Abweichung wird sie als veraltet markiert;
eine automatische Neuberechnung darf nicht die einzige Option sein.

## Architekturgrenzen

- `lumina-core` enthält plattformneutrale Domänen- und Renderlogik und darf
  keine GUI-, Dateisystem-, native ONNX- oder native RAW-Abhängigkeiten
  voraussetzen.
- Ein verpflichtendes `lumina-sidecar`-Modul soll Schema, Validierung,
  Roundtrip-Serialisierung, Migrationen und atomare Sidecar-Operationen bündeln.
- `lumina-raw` kapselt RAW-Decoder, Demosaicing, EXIF und Farbprofile.
- `lumina-onnx` kapselt native Inferenz, Modellverwaltung und Maskenartefakte.
- `lumina-cli` orchestriert Import, Analyse, Render, Export und Batch-Jobs,
  enthält aber keine eigene Bildverarbeitungslogik.
- `lumina-gui` enthält Darstellung, Interaktion und Jobsteuerung, aber keine
  zweite Implementierung der Renderpipeline.
- Ein optionaler Index-/Katalogadapter darf später als eigenes Modul ergänzt
  werden. Er muss aus Sidecars vollständig neu aufgebaut werden können.
- Native Abhängigkeiten und WASM-Fähigkeiten werden über eine dokumentierte
  Capability-Matrix getrennt behandelt.
- Jede Pipeline besitzt eine explizite Version. Rezeptversion und
  Pipelineversion werden getrennt migriert und validiert.

## Rollen und Delegation

### Build-Agent

Der Build-Agent ist der verantwortliche Orchestrator. Er:

- liest vor jeder Aufgabe `Agents.md`, `Agents.todo.md`,
  `feature/README.md` und das betroffene Feature-Dokument;
- zerlegt Arbeit in kleine, unabhängig prüfbare Aufgaben;
- delegiert Implementierung, Recherche und Tests an passende Subagenten;
- verhindert parallele Änderungen an gemeinsam genutzten APIs und Schemas;
- führt die Integrationsschritte in der richtigen Reihenfolge aus;
- beauftragt nach jeder Implementierung einen anderen, unabhängigen
  Verifizierungs-Subagenten;
- übernimmt eine Aufgabe erst nach erfolgreicher Verifizierung und aktualisiert
  danach Plan und Feature-Dokument.

### Direkte Änderungen des Build-Agenten

Der Build-Agent darf kleine, risikoarme Änderungen selbst durchführen, ohne
dafür einen Implementierungs-Agenten zu delegieren. Dazu gehören insbesondere:

- reine Dokumentationskorrekturen ohne Änderung des Zielzustands;
- das Aktualisieren von Inhaltsverzeichnissen und internen Links;
- einfache `.gitignore`-, CI- und Agentenregel-Anpassungen;
- das Anlegen oder Aktualisieren von Planpunkten;
- mechanische Formatkorrekturen ohne Verhaltensänderung.

Auch bei direkten Änderungen prüft der Build-Agent die betroffenen Dateien
selbst. Sobald eine Änderung Codeverhalten, Datenformat, Migration,
Persistenz, Pipeline, Sicherheitsverhalten oder Tests betrifft, muss sie wie
eine normale Implementierungsaufgabe delegiert und durch einen anderen
Subagenten verifiziert werden.

### Implementierungs-Agent

Der Implementierungs-Agent:

- arbeitet nur innerhalb des delegierten Umfangs;
- prüft zuerst bestehende APIs, Persistenz und Abhängigkeiten;
- ändert das SOLL-Dokument vor Code, wenn die Zielsemantik noch nicht passt;
- ergänzt Tests zusammen mit der Implementierung;
- meldet geänderte Dateien, ausgeführte Tests, Migrationen und offene Risiken.

### Verifizierungs-Agent

Der Verifizierungs-Agent muss in einem anderen Subagentenlauf als der
Implementierungs-Agent arbeiten. Er:

- liest die Anforderung unabhängig und prüft die tatsächliche Änderung gegen
  `feature/README.md` und das betroffene Feature-Dokument;
- prüft fachliche Korrektheit, Fehlerfälle, Rückwärtskompatibilität und
  Persistenzfolgen;
- führt relevante Tests, Clippy, Formatprüfung und gegebenenfalls Builds aus;
- prüft ausdrücklich, ob die Tests die neue Funktion tatsächlich abdecken;
- darf eine Aufgabe als nicht bestanden zurückweisen;
- liefert einen kurzen Prüfbericht mit bestanden/nicht bestanden, Befunden,
  Testkommandos und verbleibenden Risiken.

Ein Implementierungs-Agent darf nicht zugleich als alleiniger Verifizierungs-
Agent derselben Änderung gelten.

## Verbindlicher Arbeitsablauf

1. `Agents.md`, `Agents.todo.md`, `feature/README.md`, das betroffene
   Feature-Dokument und relevante ADRs lesen.
2. Die betroffene Funktion anhand einer stabilen Feature-ID identifizieren.
3. Prüfen, ob die geplante Änderung dem SOLL widerspricht oder einen neuen
   Konflikt erzeugt.
4. Bei einem Konflikt zuerst `feature/README.md`, das betroffene
   Feature-Dokument und gegebenenfalls ein ADR aktualisieren. Ohne klare
   Zielentscheidung beginnt keine Implementierung.
5. Die Aufgabe mit Umfang, betroffenen Modulen und Abnahmekriterien an einen
   Implementierungs-Agenten delegieren.
6. Implementierung und Tests ausführen lassen.
7. Einen anderen Subagenten mit unabhängiger Verifizierung beauftragen.
8. Bei Befunden die Aufgabe an den Implementierungs-Agenten zurückgeben und
   danach erneut verifizieren lassen.
9. Erst nach bestandener Verifizierung die offene Aufgabe aus
   `Agents.todo.md` entfernen.
10. Das betroffene Feature-Dokument um den tatsächlich erreichten Status, neue
    bekannte Grenzen oder Folgekonflikte ergänzen.
11. Im Abschlussbericht Änderung, Tests, Verifizierungsbericht und offene
    Risiken nennen.

## Verifizierung und Tests

Je nach Änderung sind mindestens diese Prüfungen zu verwenden:

- Formatierung und Clippy für Rust-Code
- Unit-Tests für mathematische und serielle Logik
- Property-Tests für Wertebereiche, Monotonie und Clipping
- JSON-Roundtrip- und Schema-Migrationstests
- Sidecar-Recovery-, Atomic-Write- und Konflikttests
- Virtuelle-Kopien-Tests inklusive stabiler IDs und unabhängiger Rezepte
- Masken-Cache-Hit-, Miss- und Invalidierungstests
- Golden-Image-Tests mit dokumentierten Toleranzen
- CLI-End-to-End-Tests inklusive Exit-Codes
- native und, sofern betroffen, WASM-Build-/Smoke-Tests
- Tests für fehlende Modelle, fehlende Maskenartefakte und veränderte Quellen
- Performance-Methodik gemäß `feature/quality/performance-benchmarks.md`
  (F-074): Benchmarks laufen gegen die committeten Baseline-/Budget-Stores
  (`scripts/perf/compare.mjs`, Modi report/warn/gate); eine Budget-Anpassung
  wegen bewusstem Feature-Wachstum wird im selben Commit wie das Feature
  begründet

AI-Modelle, RAW-Fixtures und Referenzbilder müssen reproduzierbar versioniert
und lizenzrechtlich dokumentiert sein. Tests dürfen nicht von einem spontanen
Modell-Download oder externen Netzwerkzugriff abhängen.

## Definition of Done

Eine Änderung ist erst fertig, wenn:

- die SOLL-Anforderung eindeutig erfüllt oder bewusst angepasst ist;
- die betroffenen Datenformate versioniert und migriert werden können;
- die Implementierung mit passenden Tests ausgeliefert ist;
- die relevanten Prüfungen erfolgreich ausgeführt wurden;
- ein anderer Subagent die Implementierung und Testabdeckung bestätigt hat;
- keine unaufgelösten kritischen oder hohen Befunde bestehen;
- `Agents.todo.md` die erledigte Aufgabe nicht mehr enthält;
- `feature/README.md` auf das richtige Dokument verweist und das betroffene
  Feature-Dokument den erreichten Zustand sowie verbleibende Grenzen kennt;
- der Abschlussbericht reproduzierbare Prüfkommandos nennt.

## Dokumentations- und Todo-Regeln

- `Agents.todo.md` enthält ausschließlich offene, umsetzbare Aufgaben.
- Abgeschlossene Punkte werden nicht nur abgehakt, sondern nach bestätigter
  Verifizierung aus der Datei entfernt.
- Eine Aufgabe darf nur entfernt werden, wenn der Verifizierungs-Agent die
  Implementierung und Testabdeckung bestätigt hat.
- Neue Erkenntnisse oder Folgearbeiten werden als neue offene Aufgaben ergänzt.
- Änderungen an fachlichen Zielzuständen werden zuerst im betroffenen
  Dokument unter `feature/` beschrieben und im `feature/README.md` verlinkt.
- Wichtige Architekturentscheidungen werden als ADR oder klar markierter
  Entscheidungsabschnitt dokumentiert.
- Der Plan darf niemals stillschweigend durch Codeänderungen veralten.

## Änderungsregeln

- Keine Originaldatei und kein Benutzerfoto wird für Tests oder Migrationen
  verändert.
- Keine stillen Fallbacks bei veralteten Rezepten, Farbprofilen, Masken oder
  fehlenden Artefakten.
- Keine Pipeline-Stufe ohne definierte Reihenfolge, Versionierung und Tests.
- Keine Rezeptfeldänderung ohne Schema- und Migrationsentscheidung.
- Keine absolute Pfadangabe in einem portablen Sidecar.
- Keine zentrale DB-Funktion, die ohne Sidecar-Datenverlust verursachen würde.
- Keine GUI-spezifische Bildlogik außerhalb der gemeinsamen Pipeline.
- Keine native Dependency im WASM-kompatiblen Pfad ohne dokumentierte
  Capability-Entscheidung.
- Lizenzbedingungen von RAW-Backends, ONNX-Runtime und Modellen werden vor
  Integration geprüft und dokumentiert.
