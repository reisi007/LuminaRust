# Optionale zentrale Indizierung (`lumina-index`)

**Features:** F-064 Minimaler Indexumfang, F-065 SQLite-Adapter, F-066
Rebuild/Aktualisierung/Locking/corrupt DB, F-067 Löschsicherheit

Post-MVP, kein Workspace-Modul bis zur Implementierung. Dieses Dokument ist
normativ (Doku-first) und enthält keinen Code — es legt Datenumfang,
Adaptervertrag, Rebuild-Semantik, Locking, Korruptionsbehandlung und
Löschsicherheit fest. Implementierung später als eigenes Crate `lumina-index`
hinter nicht-default Feature `index` (siehe F-065).

## Inhaltsverzeichnis

- [Ziel und Einordnung](#ziel-und-einordnung)
- [Ist-Stand](#ist-stand)
- [Minimaler Indexumfang (F-064)](#minimaler-indexumfang-f-064)
- [SQLite als optionaler Adapter (F-065)](#sqlite-als-optionaler-adapter-f-065)
- [Rebuild, Aktualisierung und Locking (F-066)](#rebuild-aktualisierung-und-locking-f-066)
- [Beschädigte Datenbank (F-066)](#beschädigte-datenbank-f-066)
- [Löschsicherheit (F-067)](#löschsicherheit-f-067)
- [Rebuild-Pseudocode (Spec, kein Code)](#rebuild-pseudocode-spec-kein-code)
- [Konsistenzregeln (Invarianten)](#konsistenzregeln-invarianten)
- [Testanforderungen (F-064…F-067)](#testanforderungen-f-064f-067)
- [Abnahme je Feature-ID](#abnahme-je-feature-id)

## Ziel und Einordnung

Eine zentrale Datenbank ist **optional** und beschleunigt ausschließlich Suche,
Index und Jobsteuerung. Sie ist weder Voraussetzung noch Quelle der Wahrheit
(Agents.md § „Persistenz und Sidecars", § „Architekturgrenzen";
`feature/README.md` Invarianten: „Das Sidecar ist die autoritative Persistenz",
„Der optionale Index ist aus Sidecars vollständig neu aufbaubar"). Das
Sidecar-Bundle (`<original>.lumina.json` + `<original>.lumina.zdata`) bleibt die
primäre und vollständige Quelle für Rezepte, virtuelle Kopien und Masken. Der
Index muss jederzeit **vollständig und byte-identisch aus den Sidecars
wiederaufgebaut** werden können — ohne Datenverlust, ohne stillen Fallback.

Dieses Dokument präzisiert den Abschnitt „Optionale zentrale Indizierung" in
`feature/platform/cli-gui-wasm.md` sowie die F-006-Zeile der Capability-Matrix
(`feature/platform/capability-matrix.md`). Es ist konsistent mit
`feature/architecture/sidecar.md` (Sidecar gewinnt bei Widerspruch, atomare
Writes, relative Pfade) und `feature/architecture/pipeline.md` (Render-Key,
`recipe_hash`, `pipeline_version`/`recipe_version` getrennt; Cache-
Invalidierung — der Index speichert nur **löschbare Verweise**, keine
Renderdaten).

## Ist-Stand

**Stand 2026-09-02, verifiziert 2026-09-02 BESTANDEN (Commit 1520ac5, `cargo check --workspace` grün, Doc-only):** Es gibt **kein** Index-Modul im Workspace (`Cargo.toml`
`[workspace].members` enthält kein `lumina-index`). Der CLI-Befehl `reindex`
(`crates/lumina-cli/src/main.rs`, `fn reindex`) ist ausschließlich ein
**Sidecar-Scan**: Er sammelt Sidecars unter der Eingabe-Root, zählt valide und
invalide Sidecars und **persistiert nichts**. Korrupte Sidecars werden sichtbar
gemeldet und beenden den Befehl mit Exit ≠ 0 (REVIEW-CLI-N4), sodass auch ein
späterer Index-Adapter den defekten Zustand bemerkt. Eine Index-Datenbank,
Jobtabelle oder Cache-Metadaten-Ablage existiert nicht; CLI und GUI laufen
vollständig ohne zentrale DB — das ist der MVP-Zustand.

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
| Cacheverweise | Verweise auf den löschbaren `.lumina/`-Cache: Preview-Keys, Stage-Cache-Digests, Render-Key-Teile | Cache-Schicht (`feature/architecture/pipeline.md`, `feature/quality/preview-cache.md`) |

Jeder Eintrag trägt eine Index-Schemaversion (DB-Schema über
`PRAGMA user_version`), eine Quellidentität (Pfad + Quellhash) und einen
Zeitstempel der letzten Aktualisierung. Der Pfad ist relativ zur Index-Root;
**absolute Pfade sind in persistenten Indexdaten verboten** (Agents.md,
Änderungsregeln) und werden beim Anlegen/Umhängen des Indexes neu aufgelöst.
`schema_version` des Sidecars und Schemaversion des Indexes werden getrennt
migriert und validiert (Agents.md, Architekturgrenzen).

### Bewusst NICHT im Index (F-064, negativer Umfang)

- Rezepte, `recipe_hash`, `recipe_version`, virtuelle-Kopien-Inhalte (nur
  Anzahl/IDs als Sidecarstatus-Hinweis);
- Maskenmetadaten, Masken-DAG, Masken-Layer, Invertierung/Feather/Blur,
  Maskenartefakte und deren `zdata`-Payloads;
- Presets;
- Vorschauen, Pixel- oder Renderdaten, Exportartefakte.

Alle diese Daten bleiben ausschließlich im Sidecar-Bundle
(`<original>.lumina.json` / `<original>.lumina.zdata`). Ein Index-Eintrag, der
ohne Sidecar nicht wiederherstellbar wäre, ist verboten (Agents.md: „Keine
zentrale DB-Funktion, die ohne Sidecar-Datenverlust verursachen würde";
`sidecar.md` § Ziel).

### Akzeptanzkriterien F-064

- Der dokumentierte Umfang ist geschlossen: Indexfelder sind Pfad, Quellhash,
  Metadaten, Sidecarstatus, Jobstatus, Cacheverweise — nichts davon ist
  Rezept/Maske/Preset/Vorschau.
- Jeder Eintrag ist aus dem Sidecar-Bundle plus Scan ableitbar; kein Feld
  benötigt eine exklusive DB-Quelle.
- Relative Pfade, Quellidentität und Zeitstempel sind normiert; absolute Pfade
  werden abgelehnt.

## SQLite als optionaler Adapter (F-065)

SQLite ist der vorgesehene optionale Adapter, gekapselt als eigenes Crate
`lumina-index` hinter einem **nicht-default** Cargo-Feature `index` (analog
`zdata`/`lensfun`/`onnx-rt`). Ohne das Feature bleibt der
Workspace-Default-Build **DB-frei**; CLI, GUI und `reindex` funktionieren
unverändert ohne Datenbank (Abnahme F-065).

- **Tabellen (vorgesehen):** `assets` (Pfad + Quellhash + Metadaten +
  Sidecarstatus), `jobs` (Jobstatus), `cache_refs` (Cacheverweise) — oder eine
  äquivalente, in der Implementierung dokumentierte Normalisierung. Rezepte und
  Maskenartefakte haben in **keiner** Tabelle Spalten oder Payloads.
- **Schemaversion:** `PRAGMA user_version` plus dokumentierte Migrationstabelle;
  ein Versionswechsel wird explizit migriert, inkompatible Hauptversionen werden
  **laut abgelehnt** (kein stiller Rebuild hinter einer Warnung).
- **Modus:** WAL für gleichzeitige Lese- und Ein-Schreiber-Nutzung,
  `busy_timeout` für begrenzte Schreibkonflikte, ein **Single-Writer**
  (Job-/Index-Writer serialisiert). Die DB liegt unter `.lumina/index/` bzw.
  einem dokumentierten, löschbaren Ort — **niemals neben dem Original**.
  `.lumina/` enthält ausschließlich löschbaren Cache (siehe `cli-gui-wasm.md`
  „Desktop-GUI" und `pipeline.md` Cache-Semantik).
- **Schreibpfade:** Jede Index-Schreiboperation aktualisiert auch den
  Sidecarstatus; widerspricht der Index dem Sidecar, **gewinnt das Sidecar**
  (`feature/architecture/sidecar.md`, „Migration und Konflikte"). Kein stiller
  Index-über-Sidecar-Fallback.

### Akzeptanzkriterien F-065

- SQLite ist optional und DB-frei ohne Feature; kein Rezept/Mask liegt nur in
  der DB.
- Tabellen, WAL/`busy_timeout`/Single-Writer und Ablage `.lumina/index/` sind
  dokumentiert; `PRAGMA user_version` steuert Migration/Ablehnung.
- Sidecar-gewinnt-Regel ist im Adaptervertrag verankert.

## Rebuild, Aktualisierung und Locking (F-066)

### Vollständiger Rebuild (F-066)

`reindex` (in einer Index-fähigen Variante) baut den Index vollständig aus den
Sidecars auf: Walk der Root, pro Original: Quellhash bestimmen, Sidecar laden
und validieren, Metadaten sammeln, Sidecarstatus ablegen. Die Reihenfolge ist
deterministisch; identische Eingaben ergeben **byte-identische Indexinhalte**
(Abnahme: Rebuild-Idempotenz — zweimaliger Rebuild ohne Änderung ergibt
identische DB-Inhalte, normiert über Dump/Selektion).

Ein Rebuild mit korrupten Sidecars endet mit Exit ≠ 0 und listet die
betroffenen Pfade — das heutige `reindex`-Verhalten bleibt für die
Index-Variante erhalten.

### Inkrementelle Aktualisierung (F-066)

Zwei Wege, beide optional und kombinierbar:

1. **Scan-basiert:** Vergleich von `modified_at`/Größe/Hash gegen den
   Indexstand; nur geänderte Einträge werden aktualisiert.
2. **Event-basiert (später, optional):** Dateisystem-Watcher melden Änderungen;
   es gibt **keine** stille Annahme über nicht beobachtete Änderungen — ein
   Scan bleibt immer möglich und vollständig.

### Locking (F-066)

- Ein **exklusives Lock** (`.lumina/index/index.lock` bzw. SQLite-`BEGIN
  EXCLUSIVE` mit dokumentierter Dauer) schützt den Rebuild und
  Massen-Aktualisierungen vor gleichzeitigen Schreibversuchen.
- Parallele Lesevorgänge bleiben möglich (WAL). Ein anstehender Rebuild wartet
  begrenzt (`busy_timeout`); bei Überschreitung meldet er einen sichtbaren
  `Conflict`-Fehler — **kein** stilles Last-Write-Wins (analog
  `sidecar.md` Persistenzregeln: Konflikt sichtbar, nicht still).
- Das Lock ist Implementierungsdetail der DB-Schicht und wird beim Prozessende
  freigegeben (inkl. Crash-Sweep eines verwaisten Lockfiles nach dokumentierter
  mtime-Schwelle, analog `recover_sidecar`).

### Akzeptanzkriterien F-066 (Rebuild/Locking)

- Rebuild aus Sidecars vollständig, deterministisch, idempotent.
- Inkrementelle Aktualisierung ändert nur betroffene Einträge; vollständiger Scan
  bleibt möglich.
- Locking erlaubt parallele Reads, serialisiert Writes, meldet Konflikte laut.

## Beschädigte Datenbank (F-066)

- Beim Öffnen läuft ein `PRAGMA integrity_check` bzw. eine äquivalente
  Header-/Strukturprüfung. Ist die DB beschädigt oder inkompatibel, gilt sie als
  `corrupt` und wird **sichtbar** gemeldet — sie wird **nicht** stillschweigend
  als leere DB behandelt, aus der die GUI „ohne Bilder" startet (kein stiller
  Fallback, Agents.md „Reproduzierbarkeit ist wichtiger als ein stiller
  Fallback").
- Wiederherstellung erfolgt **ausschließlich durch expliziten Rebuild aus den
  Sidecars**. Ein korrupter Index berührt weder Originale noch Sidecars; CLI und
  GUI fallen auf den DB-freien Pfad zurück (Sidecar-only), ohne Benutzerdaten zu
  verlieren oder stillschweigend „halb korrekt" zu arbeiten.
- Der Rebuild ersetzt eine korrupte DB vollständig (frisches Schema +
  Repopulate), nie durch inkrementelles Flicken einer korrupten Datei.

### Akzeptanzkriterien F-066 (corrupt DB)

- Korrupte/inkompatible DB wird beim Öffnen erkannt (`integrity_check`) und
  sichtbar als `corrupt` gemeldet.
- Fallback ist Sidecar-only ohne Datenverlust; Erholung nur durch expliziten
  Rebuild (kein Auto-Flicken).
- Rebuild nach Korruption ergibt identischen Index zu einem frischen Rebuild
  (Idempotenz).

## Löschsicherheit (F-067)

Das Löschen der Datenbank (oder der gesamten `.lumina/index/`-Ablage)
**zerstört keine Bearbeitungsdaten, virtuellen Kopien oder Masken**:

- Rezepte, virtuelle Kopien, Masken-DAGs und Maskenartefakte liegen
  ausschließlich im Sidecar-Bundle; der Index referenziert sie höchstens über
  wiederauflösbare Verweise (Umfang F-064).
- Nach dem Löschen stellt ein Rebuild aus den Sidecars einen **identischen**
  Index wieder her (Idempotenz als Abnahmekriterium).
- Dieser Nachweis ist Teil der Tests: Delete → Rebuild → identischer Index
  (Pfad, Quellhash, Metadaten, Sidecarstatus, Jobstatus, Cacheverweise — alle
  Felder aus F-064); zusätzlich ein End-to-End-Szenario, bei dem nach dem
  Löschen Bearbeitung, virtuelle Kopien und Masken unverändert aus dem Sidecar
  geladen werden (z. B. `load_sidecar`/`resolve_mask_planes` ohne DB).
- Die Abnahme „Das Löschen eines optionalen zentralen Indexes zerstört keine
  Bearbeitung" aus `Agents.todo.md` Abnahmekriterien ist damit erfüllt; sie wird
  nicht durch eine DB-Annahme ersetzt.

### Akzeptanzkriterien F-067

- Löschen von `.lumina/index/` verändert kein Original und kein Sidecar.
- Rebuild nach Löschen ist byte-identisch zum Vorzustand (bei unveränderter
  Sidecar-Lage).
- Bearbeitungen/virtuelle Kopien/Masken bleiben nach Löschen + Rebuild über
  Sidecar-only-Pfad verfügbar.

## Rebuild-Pseudocode (Spec, kein Code)

Kein Workspace-Modul wird jetzt angelegt. Der folgende Pseudocode ist
Spezifikation für ein späteres `lumina-index`-Crate (hinter Feature `index`):

```text
fn rebuild_index(root: Root, index_path: Path) -> Result<RebuildReport> {
    // 1. Lock erwerben (index.lock + BEGIN EXCLUSIVE), WAL/busy_timeout
    let _guard = acquire_exclusive(index_path, busy_timeout=5s)
        .map_err(|e| Conflict(e))?; // sichtbar, kein stiller Fallback

    // 2. Falls DB corrupt/inkompatibel: integrity_check, laut melden
    if is_corrupt(index_path) { // PRAGMA integrity_check / header check
        return Err(CorruptDb { path: index_path, hint: "run reindex --rebuild" });
        // Aufrufer fällt auf sidecar-only zurück; Erholung nur explizit:
        // remove index_path; create fresh schema(user_version=N)
    }

    // 3. Deterministischer Walk, relative Pfade
    let mut rows = Vec::new();
    for original in walk_sorted(root) { // deterministisch sortiert
        let rel = relative_to_root(original, root)?; // absolute Pfade verboten
        let source = load_source_identity(original)?; // BLAKE3, byte_length, modified_at
        let status = classify_sidecar(original)?; // missing/valid/invalid/stale + schema_version + vc ids
        let meta = read_searchable_metadata(original)?; // read_metadata/EXIF, nur Suchdaten
        let jobs = read_job_status(rel)?; // queued/running/done/failed, optional
        let cache_refs = read_cache_refs(rel)?; // .lumina/ Verweise, löschbar
        // Invariante: kein Rezept, keine Maske, kein Preset, keine Pixel hier ablegen
        rows.push(IndexRow { rel, source, meta, status, jobs, cache_refs });
    }

    // 4. Atomar ersetzen: frisches Schema + bulk insert in Transaktion
    replace_atomically(index_path, |txn| {
        txn.set_user_version(SCHEMA_VERSION)?;
        for r in &rows { txn.upsert(r)?; }
        Ok(())
    })?;

    // 5. Validierung: Sidecar gewinnt — bei Widerspruch Sidecar neu lesen
    // 6. Report: valide/invalide/corrupt counts, Exit !=0 bei invalid (wie heutiges reindex)
    Ok(RebuildReport { rows, invalid_paths })
}

// Fallback bei corrupt/löschen: Aufrufer ohne DB weiter, expliziter rebuild stellt Idempotenz her.
// delete_index(index_path) -> remove_dir_all(index_path); // kein Sidecar berührt
```

Fehlerfälle sind laut: `Corrupt`, `Conflict`, `InvalidSidecar` — nie still.

## Konsistenzregeln (Invarianten)

- Widerspricht ein optionaler Index dem Sidecar, **gewinnt das Sidecar**
  (`feature/architecture/sidecar.md` § Migration und Konflikte); der Konflikt
  wird sichtbar gemeldet, nicht still aufgelöst.
- Kein Index-Eintrag enthält Daten, die nicht aus Sidecars rekonstruiert werden
  können. Insbesondere: kein Rezept, keine virtuelle Kopie, keine Maske, kein
  Preset, keine Pixel.
- Cacheverweise (`.lumina/`) sind löschbare Metadaten; ihre Abwesenheit
  invalidiert nur den Cache, nie Bearbeitungen (Veraltung sichtbar, kein
  stiller Fallback — `pipeline.md` Cache/Invalidierung).
- Ein Reindex-/Rebuild-Lauf mit korrupten Sidecars endet mit Exit ≠ 0 und
  listet die betroffenen Pfade (Verhalten von `reindex` bleibt erhalten).
- `schema_version` des Sidecars und Schemaversion des Indexes werden getrennt
  migriert und validiert (Agents.md, Architekturgrenzen).
- Relative Bezüge bleiben beim Verschieben eines Sidecar-Bundles gültig;
  absolute Pfade sind in persistenten Index- und Rezeptdaten verboten.
- Reproduzierbarkeit vor stillem Fallback (Agents.md Produktprinzipien): fehlende
  oder korrupte DB/Sidecars/Cacheverweise werden als veraltet/nicht verfügbar
  gemeldet.

## Testanforderungen (F-064…F-067)

Die Tests sind Post-MVP-Pflicht für ein späteres `lumina-index`-Crate (Feature
`index`). Sie dürfen nicht von Netzwerkzugriff abhängen und müssen ohne DB
grün bleiben (DB-freier Default-Pfad). Benötigte Nachweise:

- **F-064 Umfang/Isolation:** Schema-Introspektion/Review-Test weist nach, dass
  kein Rezept-/Mask-/Preset-/Pixel-Feld in der DB existiert; nur die
  geschlossene Feldmenge (Pfad, Quellhash, Metadaten, Sidecarstatus, Jobstatus,
  Cacheverweise) ist vorhanden. Negativtest: Einfügen von Rezept/Maske wird
  abgelehnt bzw. gar nicht erst modelliert. Absolute-Pfad-Test: absoluter Pfad
  wird abgelehnt/normalisiert.
- **F-065 Optionalität/Wiederaufbaubarkeit:** `cargo check --workspace` ohne
  Feature `index` baut ohne SQLite; `reindex` bleibt Sidecar-Scan ohne
  Persistenz. Mit Feature `index`: DB liegt unter `.lumina/index/`, WAL +
  `user_version` vorhanden, Single-Writer dokumentiert.
- **F-066 Rebuild-Idempotenz:** Fixture-Ordner mit N Originalen+Sidecars:
  `rebuild` → Dump A; `rebuild` erneut → Dump B byte-identisch (sortierte
  Selektion). Determinismus über Pfad-Sortierung gesichert.
- **F-066 Inkrementell:** Nach Änderung genau eines Sidecars aktualisiert
  `update`/`reindex` nur dessen Eintrag; unveränderte Einträge bleiben
  bit-identisch.
- **F-066 Locking/Konflikt:** Parallele Reads während Rebuild bleiben möglich
  (WAL); zweiter gleichzeitiger Writer erhält sichtbaren `Conflict` nach
  `busy_timeout` — kein stilles Last-Write-Wins. Verwaistes Lockfile wird nach
  mtime-Schwelle erkannt (Sweep-Test).
- **F-066 corrupt DB:** Korrupte DB (abgeschnittener Header, falsches
  `user_version`-Major, `integrity_check`-Fehler) wird beim Öffnen als
  `corrupt` gemeldet; CLI/GUI fallen auf Sidecar-only zurück; expliziter
  `reindex --rebuild` ersetzt die DB vollständig und repopuliert identisch zu
  frischem Rebuild.
- **F-067 Löschsicherheit:** `delete_index` (remove `.lumina/index/`) berührt
  weder Original noch Sidecar (Hash-Vergleich vor/nachher). Anschließender
  `rebuild` ergibt identischen Index (alle F-064-Felder). E2E: Nach Löschen
  lädt `load_sidecar` Bearbeitung, virtuelle Kopien und Masken (inkl.
  `artifact_status`/`resolve_mask_planes` via `zdata`) unverändert; Render
  bleibt byte-identisch.
- **Rezept-Sidecar-Gewinnt:** Bei absichtlich divergiertem Index-Eintrag gewinnt
  nach Validierung das Sidecar; der Konflikt wird sichtbar gemeldet.
- **Korrupte Sidecars:** Rebuild mit korruptem Sidecar endet Exit ≠ 0 und
  listet Pfad (heutiges `reindex`-Verhalten, REVIEW-CLI-N4).
- **Kein stiller Fallback:** Jede der obigen Korrupt-/Konflikt-Situationen wird
  als sichtbarer Fehler/Status gemeldet, nie still als „leer" behandelt.

## Abnahme je Feature-ID

### F-064 Minimaler Indexumfang — Abnahme

- Umfang geschlossen und dokumentiert (Tabelle oben); kein Rezept/Maske/Preset
  im Index.
- Relative Pfade, Quellidentität, Zeitstempel normiert; absolute Pfade
  verboten.

### F-065 SQLite-Adapter (optional) — Abnahme

- `cargo check --workspace` ohne Feature `index` ist grün und DB-frei;
  `reindex` bleibt korrekter Sidecar-Scan.
- Mit Feature `index` existiert eine SQLite-DB unter `.lumina/index/` mit
  `assets`/`jobs`/`cache_refs`, `PRAGMA user_version`, WAL, `busy_timeout`,
  Single-Writer.
- Kein Rezept/Mask-Payload in der DB; Sidecar-gewinnt-Regel gilt.

### F-066 Rebuild/Aktualisierung/Locking/corrupt DB — Abnahme

- Rebuild aus Sidecars vollständig, deterministisch, idempotent (byte-identisch).
- Inkrementelle Aktualisierung und vollständiger Scan beide möglich.
- Locking: parallele Reads safe, Schreibkonflikt sichtbar als `Conflict`.
- Korrupte/inkompatible DB wird per `integrity_check` erkannt, sichtbar
  gemeldet, fällt auf Sidecar-only zurück und wird ausschließlich per
  explizitem Rebuild wiederhergestellt (frisches Schema + Repopulate).

### F-067 Löschsicherheit — Abnahme

- Löschen von `.lumina/index/` zerstört keine Bearbeitungsdaten, virtuellen
  Kopien oder Masken (Hash-Nachweis Original/Sidecar vor/nachher).
- Delete → Rebuild → identischer Index (alle F-064-Felder, Idempotenz).
- E2E: Nach Löschen bleiben Bearbeitung/virtuelle Kopien/Masken über Sidecar
  verfügbar und rendern byte-identisch.

### Übergreifend

- `reindex` funktioniert ohne Datenbank und bleibt ein korrekter Sidecar-Scan.
- Mit aktiviertem `index`-Feature wird der Index aufgebaut, aktualisiert und
  wiederaufgebaut; identische Eingaben ergeben byte-identische Indexinhalte.
- Rezepte, virtuelle-Kopien-Inhalte und Masken sind im Index nachweislich nicht
  enthalten; der Index ist aus Sidecars vollständig wiederaufbaubar.
- Kein Widerspruch zu `sidecar.md`/`pipeline.md`; kein stiller Fallback;
  Rezept bleibt Sidecar-autoritativ.
