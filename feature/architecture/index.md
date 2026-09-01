# Optionale zentrale Indizierung (`lumina-index`)

**Features:** F-064 Minimaler Indexumfang, F-065 SQLite-Adapter, F-066
Rebuild/Aktualisierung/Locking, F-067 Löschsicherheit

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Ist-Stand](#ist-stand)
- [Minimaler Indexumfang (F-064)](#minimaler-indexumfang-f-064)
- [SQLite als optionaler Adapter (F-065)](#sqlite-als-optionaler-adapter-f-065)
- [Rebuild, Aktualisierung und Locking (F-066)](#rebuild-aktualisierung-und-locking-f-066)
- [Beschädigte Datenbank (F-066)](#beschädigte-datenbank-f-066)
- [Löschsicherheit (F-067)](#löschsicherheit-f-067)
- [Konsistenzregeln](#konsistenzregeln)
- [Abnahme](#abnahme)

## Ziel

Eine zentrale Datenbank ist **optional** und beschleunigt ausschließlich Suche,
Index und Jobsteuerung. Sie ist weder Voraussetzung noch Quelle der Wahrheit
(Agents.md, „Persistenz und Sidecars"; `feature/README.md`, Invarianten). Das
Sidecar-Bundle bleibt die primäre und vollständige Quelle für Rezepte,
virtuelle Kopien und Masken. Der Index muss jederzeit vollständig und
byte-identisch aus den Sidecars wiederaufgebaut werden können.

Dieses Dokument ist das normative SOLL für F-064…F-067 und präzisiert den
Abschnitt „Optionale zentrale Indizierung" in
`feature/platform/cli-gui-wasm.md` sowie die F-006-Zeile der
Capability-Matrix (`feature/platform/capability-matrix.md`). Es enthält
keinen Code; es legt Datenumfang, Adaptervertrag, Rebuild-Semantik, Locking,
Korruptionsbehandlung und Löschsicherheit fest.

## Ist-Stand

**Stand 2026-09-01:** Es gibt **kein** Index-Modul im Workspace. Der CLI-Befehl
`reindex` (`crates/lumina-cli/src/main.rs`, `fn reindex`) ist ausschließlich
ein **Sidecar-Scan**: Er sammelt Sidecars unter der Eingabe-Root, zählt valide
und invalide Sidecars und **persistiert nichts**. Korrupte Sidecars werden
sichtbar gemeldet und beenden den Befehl mit Exit ≠ 0 (REVIEW-CLI-N4), sodass
auch ein späterer Index-Adapter den defekten Zustand bemerkt. Eine Index-
Datenbank, Jobtabelle oder Cache-Metadaten-Ablage existiert nicht; CLI und GUI
laufen vollständig ohne zentrale DB.

Die langfristige Struktur sieht `lumina-sidecar` als verpflichtendes Modul und
`lumina-index` als **optionalen, wiederaufbaubaren Adapter** vor
(`feature/platform/cli-gui-wasm.md`, „Gemeinsame Grenze").

## Minimaler Indexumfang (F-064)

Der Index speichert **ausschließlich** wiederaufbaubare Such-, Status- und
Cache-Metadaten. Der Umfang ist bewusst minimal und geschlossen:

| Bereich | Gespeichert | Herkunft |
| --- | --- | --- |
| Pfad | relativer Pfad des Originals zur Index-Root, Dateiname, RAW-/Raster-Extension-Klassifikation | Scan/Walk |
| Quellhash | BLAKE3-`content_hash`, `byte_length`, `modified_at` | Import/Scan, aus Sidecar `source` oder File-Hash |
| Metadaten | Suchbare Metadaten: Pixel-Dimensionen, EXIF-Orientierung, Aufnahmezeit, Kamera-/Objektiv-Felder | `lumina_raw::read_metadata`/EXIF; reine Suchdaten |
| Sidecarstatus | `missing` / `valid` / `invalid` / `stale`, `schema_version`, Anzahl und IDs der virtuellen Kopien | Sidecar-Validierung |
| Jobstatus | letzter Jobtyp, Status (`queued` / `running` / `done` / `failed`), Retry-Zähler, Fehlertext, Abschlusszeit | Job-/Batch-Verwaltung |
| Cacheverweise | Verweise auf den löschbaren `.lumina/`-Cache: Preview-Keys, Stage-Cache-Digests, Render-Key-Teile | Cache-Schicht |

Jeder Eintrag trägt eine Index-Schemaversion (DB-Schema über
`PRAGMA user_version`), eine Quellidentität (Pfad + Quellhash) und einen
Zeitstempel der letzten Aktualisierung. Der Pfad ist relativ zur Index-Root;
**absolute Pfade sind in persistenten Indexdaten verboten** (Agents.md,
Änderungsregeln) und werden beim Anlegen/Umhängen des Indexes neu aufgelöst.

### Bewusst NICHT im Index

- Rezepte, `recipe_hash`, virtuelle-Kopien-Inhalte (nur Anzahl/IDs als
  Sidecarstatus-Hinweis);
- Maskenmetadaten, Masken-DAG, Maskenartefakte;
- Presets;
- Vorschauen, Pixel- oder Renderdaten.

Alle diese Daten bleiben ausschließlich im Sidecar-Bundle
(`<original>.lumina.json` / `<original>.lumina.zdata`). Ein Index-Eintrag, der
ohne Sidecar nicht wiederherstellbar wäre, ist verboten (Agents.md, „Keine
zentrale DB-Funktion, die ohne Sidecar-Datenverlust verursachen würde").

## SQLite als optionaler Adapter (F-065)

SQLite ist der vorgesehene optionale Adapter, gekapselt als eigenes Crate
`lumina-index` hinter einem **nicht-default** Cargo-Feature `index`
(analog `zdata`/`lensfun`/`onnx-rt`). Ohne das Feature bleibt der
Workspace-Default-Build **DB-frei**; CLI, GUI und `reindex` funktionieren
unverändert ohne Datenbank.

- **Tabellen:** `assets` (Pfad + Quellhash + Metadaten + Sidecarstatus),
  `jobs` (Jobstatus), `cache_refs` (Cacheverweise) — oder eine äquivalente,
  in der Implementierung dokumentierte Normalisierung. Rezepte und
  Maskenartefakte haben in keiner Tabelle Spalten oder Payloads.
- **Schemaversion:** `PRAGMA user_version` plus dokumentierte Migrationstabelle;
  ein Versionswechsel wird explizit migriert, inkompatible Hauptversionen werden
  laut abgelehnt (kein stiller Rebuild-Verstecken hinter einer Warnung).
- **Modus:** WAL für gleichzeitige Lese- und Ein-Schreiber-Nutzung,
  `busy_timeout` für begrenzte Schreibkonflikte, ein **Single-Writer**
  (Job-/Index-Writer serialisiert). Die DB liegt unter `.lumina/index/` bzw.
  einem dokumentierten, löschbaren Ort — niemals neben dem Original.
- **Schreibpfade:** Jede Index-Schreiboperation aktualisiert auch den
  Sidecarstatus; widerspricht der Index dem Sidecar, **gewinnt das Sidecar**
  (`feature/architecture/sidecar.md`, „Migration und Konflikte").

## Rebuild, Aktualisierung und Locking (F-066)

### Vollständiger Rebuild

`reindex` (in einer Index-fähigen Variante) baut den Index vollständig aus den
Sidecars auf: Walk der Root, pro Original: Quellhash bestimmen, Sidecar laden
und validieren, Metadaten sammeln, Sidecarstatus ablegen. Die Reihenfolge ist
deterministisch; identische Eingaben ergeben byte-identische Indexinhalte
(Abnahme: Rebuild-Idempotenz).

### Inkrementelle Aktualisierung

Zwei Wege, beide optional und kombinierbar:

1. **Scan-basiert:** Vergleich von `modified_at`/Größe/Hash gegen den
   Indexstand; nur geänderte Einträge werden aktualisiert.
2. **Event-basiert (später, optional):** Dateisystem-Watcher melden
   Änderungen; es gibt keine stille Annahme über nicht beobachtete
   Änderungen — ein Scan bleibt immer möglich und vollständig.

### Locking

- Ein **exklusives Lock** (`.lumina/index/index.lock` bzw. SQLite-`BEGIN
  EXCLUSIVE` mit dokumentierter Dauer) schützt den Rebuild und
  Massen-Aktualisierungen vor gleichzeitigen Schreibversuchen.
- Parallele Lesevorgänge bleiben möglich (WAL). Ein anstehender Rebuild
  wartet begrenzt (`busy_timeout`); bei Überschreitung meldet er einen
  sichtbaren `Conflict`-Fehler — **kein** stilles Last-Write-Wins.
- Das Lock ist eine Implementierungsdetails der DB-Schicht und wird beim
  Prozessende freigegeben (inkl. Crash-Sweep eines verwaisten Lockfiles nach
  dokumentierter mtime-Schwelle, analog `recover_sidecar`).

## Beschädigte Datenbank (F-066)

- Beim Öffnen läuft ein `PRAGMA integrity_check` bzw. eine äquivalente
  Header-/Strukturprüfung. Ist die DB beschädigt oder inkompatibel, gilt sie
  als `corrupt` und wird **sichtbar** gemeldet — sie wird nicht stillschweigend
  als leere DB behandelt, aus der die GUI „ohne Bilder" startet.
- Wiederherstellung erfolgt **ausschließlich durch expliziten Rebuild aus den
  Sidecars**. Ein korrupter Index berührt weder Originale noch Sidecars; CLI
  und GUI fallen auf den DB-freien Pfad zurück (Sidecar-only), ohne
  Benutzerdaten zu verlieren oder stillschweigend „halb korrekt" zu arbeiten.
- Der Rebuild ersetzt eine korrupte DB vollständig (frisches Schema +
  Repopulate), nie durch inkrementelles Flicken.

## Löschsicherheit (F-067)

Das Löschen der Datenbank (oder der gesamten `.lumina/index/`-Ablage) **zerstört
keine Bearbeitungsdaten, virtuellen Kopien oder Masken**:

- Rezepte, virtuelle Kopien, Masken-DAGs und Maskenartefakte liegen
  ausschließlich im Sidecar-Bundle; der Index referenziert sie höchstens über
  wiederauflösbare Verweise.
- Nach dem Löschen stellt ein Rebuild aus den Sidecars einen **identischen**
  Index wieder her (Idempotenz als Abnahmekriterium, siehe unten).
- Dieser Nachweis ist Teil der Tests: Delete → Rebuild → identischer Index
  (Pfad, Quellhash, Metadaten, Sidecarstatus, Jobstatus, Cacheverweise);
  zusätzlich ein End-to-End-Szenario, bei dem nach dem Löschen Bearbeitung,
  virtuelle Kopien und Masken unverändert aus dem Sidecar geladen werden.

## Konsistenzregeln

- Widerspricht ein optionaler Index dem Sidecar, **gewinnt das Sidecar**
  (`feature/architecture/sidecar.md`); der Konflikt wird sichtbar gemeldet,
  nicht still aufgelöst.
- Kein Index-Eintrag enthält Daten, die nicht aus Sidecars rekonstruiert
  werden können.
- Cacheverweise (`.lumina/`) sind löschbare Metadaten; ihre Abwesenheit
  invalidiert nur den Cache, nie Bearbeitungen (Veraltung sichtbar, kein
  stiller Fallback).
- Ein Reindex-/Rebuild-Lauf mit korrupten Sidecars endet mit Exit ≠ 0 und
  listet die betroffenen Pfade (Verhalten von `reindex` bleibt erhalten).
- `schema_version` des Sidecars und Schemaversion des Indexes werden getrennt
  migriert und validiert (Agents.md, Architekturgrenzen).

## Abnahme

- `reindex` funktioniert ohne Datenbank und bleibt ein korrekter Sidecar-Scan.
- Mit aktiviertem `index`-Feature wird der Index aufgebaut, aktualisiert und
  wiederaufgebaut; identische Eingaben ergeben byte-identische Indexinhalte.
- Ein Rebuild ist unter parallelen Lesevorgängen safe (Locking), und ein
  Schreibkonflikt wird sichtbar gemeldet.
- Eine korrupte Datenbank wird sichtbar gemeldet und ausschließlich per
  Rebuild aus Sidecars wiederhergestellt.
- Das Löschen der Datenbank zerstört nachweislich keine Bearbeitungsdaten,
  virtuellen Kopien oder Masken (Delete→Rebuild→identischer Index, getestet).
- Rezepte, virtuelle-Kopien-Inhalte und Masken sind im Index nachweislich
  nicht enthalten.
