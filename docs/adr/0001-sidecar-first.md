# 0001: Sidecar-first

**Status:** akzeptiert  
**Feature-IDs:** F-001  
**Datum:** 2026-08-17  

## Kontext

LuminaRust ist ein nicht-destruktiver RAW-Prozessor. Bearbeitungsdaten,
Rezepte, virtuelle Kopien und Maskenartefakte müssen persistiert werden. Es
gibt mehrere Persistenzmodelle: zentrale Datenbank,(dateinahes) Sidecar
oder eine Kombination.

## Entscheidung

Die autoritative Persistenz für Lumina-Bearbeitungen ist das Sidecar-Bundle
neben dem Original:

- `<filename>.lumina.json` (Rezept, Metadaten, Masken-DAG)
- `<filename>.lumina.zdata` (binäre Maskenpayloads)

Das Sidecar ist die vollständige, portable Quelle der Wahrheit. Ein
optionaler zentraler Index (SQLite) darf nur wiederaufbaubare Suchdaten,
Jobstatus und Cache-Metadaten enthalten, aber niemals Rezepte oder
Maskenartefakte ausschließlich dort speichern.

## Alternativen

1. **Nur zentrale Datenbank:** Verletzt die Portabilitätsanforderung; ein
   Kopieren oder Verschieben von Bildern erfordert Synchronisation.
2. **Hybrid ohne Klare Hierarchie:** Führt zu Konflikten bei
   gleichzeitigen Schreibvorgängen und verletzt die Single-Source-Regel.
3. **Sidecar-first (gewählt):** Portabel, atomar pro Bild, ohne
   Infrastrukturabhängigkeiten.

## Konsequenzen

- Jede Sidecar-Änderung muss über atomare temporäre Dateien und Rename
  erfolgen.
- Sidecar-Migrationen erfordern Backup, Bestätigung und (CLI) ein
  ausdrückliches Flag.
- Der optionale Index muss jederzeit vollständig aus Sidecars neu aufgebaut
  werden können.
- XMP wird in v1 nicht unterstützt; ein späterer Adapter darf Lumina-Daten
  nicht als autoritative Quelle behandeln.

## Verweise

- `feature/architecture/sidecar.md`
- `feature/README.md` (Invarianten, Festgelegte Entscheidungen)
- `Agents.md` (Persistenz und Sidecars)
