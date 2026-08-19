# Rezept-, Sidecar- und Pipeline-Migrationsstrategie für Releases

**Feature:** F-076 (Phase 11: Qualität, Performance und Release)
**Status:** SOLL-Dokumentation — synthetisiert aus vorhandenem, verteiltem
Migrationsmaterial; keine Codeänderung.
**Datum:** 2026-08-19

Dieses Dokument ist die **einzige autoritative Strategie** für das Bump von
Sidecar-/Rezept-Schema (`schema_version`, `recipe_schema_version`) und
Renderpipeline (`pipeline_version`) über Releases hinweg. Es fasst die in
`feature/architecture/sidecar.md`, `feature/architecture/pipeline.md`,
`Agents.todo.md`, `feature/README.md` und `docs/adr/0001-sidecar-first.md`
verteilten Regeln zusammen und verknüpft sie mit der **tatsächlich
implementierten** Migrationsmechanik in `crates/lumina-sidecar/src/lib.rs`.

Verwandte, nicht veränderte Quelldokumente (per Link, nicht dupliziert):

- `feature/architecture/sidecar.md` → Abschnitt
  [„Migration und Konflikte“](../feature/architecture/sidecar.md#migration-und-konflikte)
- `feature/architecture/pipeline.md` → Abschnitt
  [„Schema-Migration (Pre-MVP)“](../feature/architecture/pipeline.md#bearbeitungsregler)
  sowie [„Render-Key“](../feature/architecture/pipeline.md#reproduzierbarkeit) /
  [„Cache und Invalidierung“](../feature/architecture/pipeline.md#cache-und-invalidierung)
- `Agents.todo.md` → Phase 2
  [„Rezept, virtuelle Kopien und Migrationen“](../Agents.todo.md#phase-2-rezept-virtuelle-kopien-und-migrationen)
  (Pre-MVP-Entscheidung) und Phase 11
  [F-076](../Agents.todo.md#phase-11-qualität-performance-und-release)
- `feature/README.md` →
  [„Festgelegte Entscheidungen“](../feature/README.md#festgelegte-entscheidungen)
  (Pre-MVP-Schema-Entscheidung; „Migrationen erfolgen verzögert …“)
- `docs/adr/0001-sidecar-first.md` →
  [Konsequenzen](adr/0001-sidecar-first.md) (Migration erfordert Backup,
  Bestätigung, CLI-Flag)
- `Agents.md` → [Persistenz und Sidecars](../Agents.md#persistenz-und-sidecars)
  (Sidecar = Quelle der Wahrheit, atomare Writes, keine stillen Fallbacks)

---

## Inhaltsverzeichnis

1. [Release-Migrationsprinzip](#1-release-migrationsprinzip)
2. [Sidecar-Migrationsmechanik (aus dem Code)](#2-sidecar-migrationsmechanik-aus-dem-code)
3. [Schema-Versions-Split](#3-schema-versions-split)
4. [Pre-MVP-Entscheidung](#4-pre-mvp-entscheidung)
5. [Release-Verfahren bei einem Schema-/Pipeline-Bump](#5-release-verfahren-bei-einem-schema-pipeline-bump)
6. [Kompatibilitätszusagen für Releases](#6-kompatibilitätszusagen-für-releases)
7. [Test-/Verifikationserwartungen](#7-test-verifikationserwartungen)
8. [Bekannte Lücken](#8-bekannte-lücken)

---

## 1. Release-Migrationsprinzip

LuminaRust ist ein nicht-destruktiver RAW-Prozessor. Das Sidecar
(`<original>.lumina.json` + `<original>.lumina.zdata`) ist die **vollständige,
portable und autoritative Persistenz** für Lumina-Bearbeitungen
(`Agents.md` → Persistenz; `feature/README.md` → Invarianten;
`docs/adr/0001-sidecar-first.md`). Daraus folgen die harten Regeln für jede
Schema-/Pipeline-Migration:

- **Keine stillen Schemaänderungen.** Ein höheres `schema_version` als das vom
  lesenden Code unterstützte wird explizit abgelehnt (`SidecarError::Invalid`),
  nie still heruntergestuft oder überlesen (`migrate_json`, siehe §2).
- **Migration ist explizit, bestätigt, gesichert und atomar.** Sie läuft nie
  automatisch im Hintergrund. Sie erfordert eine ausdrückliche Benutzeraktion
  (CLI-Flag / GUI-Bestätigung), schreibt vorher ein `.bak`-Backup des Originals
  und ersetzt das Ziel erst durch einen **atomaren Rename** (`Agents.md`,
  ADR 0001, `feature/README.md` Festgelegte Entscheidungen).
- **Das Originalbild bleibt immer unberührt.** Eine Migration verändert nur das
  Sidecar; die RAW-Datei wird niemals gelesen-geschrieben.
- **Kein zentraler Index als Quelle.** Eine optionale SQLite-DB (Phase 9,
  F-064…F-067) darf nie Rezept-, virtuelle-Kopien- oder Maskendefinitionen
  ausschließlich enthalten; sie ist jederzeit vollständig aus Sidecars neu
  aufbaubar (`Agents.md`, ADR 0001). Dies gilt unverändert für Migrationen:
  Ein Rezept wird immer aus dem Sidecar migriert, nie aus dem Index.
- **Reproduzierbarkeit vor Komfort.** Fehlende oder inkompatible Artefakte
  werden sichtbar als veraltet/nicht verfügbar gemeldet; ein automatischer
  stiller Fallback ist verboten (`Agents.md` → Änderungsregeln).

---

## 2. Sidecar-Migrationsmechanik (aus dem Code)

Alle Angaben in diesem Abschnitt sind **wörtlich aus**
`crates/lumina-sidecar/src/lib.rs` (Stand 2026-08-19). Die Migrations-Maschinerie
liegt vollständig in `lumina-sidecar`; `lumina-cli` und `lumina-gui` rufen sie
nur auf, sie implementieren keine eigene Logik.

### 2.1 Konstanten und Fehlertypen

- `pub const SCHEMA_VERSION: u32 = 2;` (lib.rs, Z. 19) — die **aktuell** vom Code
  geschriebene/gelesene Hauptversion.
- `pub enum SidecarError` (lib.rs, Z. 755) mit den für Migration relevanten
  Varianten:
  - `Missing(String)` — Sidecar nicht vorhanden.
  - `Io { operation, path, message }` — alle Dateisystemfehler, **inklusive
    Backup-Schreibfehler und atomarer Schreib-/Sync-Fehler** (es gibt *keine*
    eigene `BackupFailed`-Variante, siehe §8).
  - `Json(String)` — ungültiges JSON beim Parsen/Reserialisieren.
  - `Invalid(String)` — semantisch ungültig: fehlendes `schema_version` oder
    `schema_version` höher als unterstützt.
  - `Conflict(String)` — **Lock-Konflikt** (Sidecar ist gesperrt).
  - `XmpUnsupported` — XMP wird in v1 nicht unterstützt.

### 2.2 `migrate_json` — reiner, nicht-schreibender Migrationstransform

`pub fn migrate_json(json: &str) -> Result<String, SidecarError>` (lib.rs, Z. 988):

1. Parse zu `serde_json::Value`. Fehlt `schema_version` →
   `Err(Invalid("missing schema_version"))`.
2. Ist `version > SCHEMA_VERSION` (aktuell > 2) →
   `Err(Invalid("unsupported schema_version {version}; explicit migration is required"))`.
   **Kein** stiller Downgrade.
3. `version == 0` → auf `1` hochstellen.
4. `version == 1` → auf `2` hochstellen. Kommentar im Code:
   *„v1's flat map is deliberately retained; only the schema stamp changes.“*
   Die flache `adjustments`-Map bleibt erhalten; nur der Versionsstempel
   ändert sich.
5. Das (potenziell veränderte) Value wird über
   `SidecarDocument::from_json` neu validiert und via `to_json()`
   reserialisiert → **Roundtrip-Validierung** innerhalb des Transforms.

Eigenschaften:

- **Nebeneffektfrei:** keine Dateioperation, nur String→String. Damit ist
  `migrate_json` der **sicher einsehbare Migrations-Preview**.
- **Idempotent für already-current:** Ein bereits auf `SCHEMA_VERSION` stehendes
  Dokument wird unverändert durchgereicht und erneut validiert.
- **Template für künftige Bumps:** Der `v1 → v2`-Zweig ist das Muster, das bei
  jedem weiteren Bump (z. B. `v2 → v3`) kopiert/ergänzt wird (siehe §4).

### 2.3 `migrate_sidecar_file` — expliziter, gesicherter Datei-Migrationsschritt

`pub fn migrate_sidecar_file(path: &Path) -> Result<bool, SidecarError>`
(lib.rs, Z. 1014). **Nur aufrufen, wenn der Aufrufer die Migration ausdrücklich
will.** Ablauf:

1. **Lock:** `acquire_write_lock(path)` (siehe §2.5). Bei Konflikt →
   `Err(Conflict("sidecar is locked: …"))`; das Originalsidecar wird **nicht**
   angerührt.
2. **Lesen:** Originalbytes werden gelesen (müssen gültiges UTF-8 sein).
3. **Transform:** `migrate_json(original_string)`.
   - Schlägt dies fehl (z. B. `Invalid` bei zu neuem `schema_version`), bricht
     die Funktion **vor** jedem Schreibvorgang ab → Original intakt.
4. **No-op-Erkennung:** Sind `migrated`-Bytes identisch mit `original` →
   `Ok(false)`, **ohne** Backup und ohne Schreiben.
5. **Backup:** `<pfad>.bak` wird mit dem **Originalinhalt** via
   `atomic_write_bytes` geschrieben.
6. **Atomarer Ersatz:** `<pfad>` wird mit dem migrierten Inhalt via
   `atomic_write_bytes` überschrieben → `Ok(true)`.

Reihenfolge **Backup vor Ersatz** ist entscheidend für die Wiederherstellbarkeit
(siehe §5.4).

### 2.4 `atomic_write_bytes` — Crash-sicheres Schreiben

`fn atomic_write_bytes(path, bytes)` (lib.rs, Z. 1030):

- `tempfile::NamedTempFile` im Elternverzeichnis.
- `write_all` → `flush` → `sync_all` (Datei) → `persist()` (**atomarer Rename**
  auf das Ziel) → `sync_all` des Elternverzeichnisses.

Ein Abbruch **während** des Schreibens hinterlässt nur die Tempartei; das
Ziel wird erst durch den Rename ersetzt. Verwaiste Temparteien werden über
`recover_sidecar` (lib.rs, Z. 898, Präfix `.{dateiname}.tmp-`) aufgeräumt;
`load_sidecar` ruft `recover_sidecar` automatisch auf. **Hinweis:** JSON- und
zdata-Schreibvorgänge sind je einzeln atomar; eine gemeinsame Zwei-Dateien-
Transaktion ist bewusst **noch offen** (siehe `sidecar.md` Binärdaten / `save_sidecar`-Kommentar).

### 2.5 `acquire_write_lock` — parallele Schreibkonflikte vermeiden

`fn acquire_write_lock(path)` (lib.rs, Z. 921):

- Erzeugt `.{dateiname}.lock` per `create_new` (O_CREATE | O_EXCL).
- Bis zu 100 Versuche mit 10 ms Pause bei `AlreadyExists`.
- Ein **verwaister** Lock (> 30 s alt) wird reklamiert; danach →
  `Err(Conflict("sidecar is locked: …"))`.
- `WriteLock` entfernt die Lockdatei in `Drop` (Aufräumfehler werden bewusst
  ignoriert — der nächste Schreiber meldet einen veralteten Lock).

### 2.6 Fehlerbehandlung und Recovery im Überblick

| Situation | Verhalten | Original intakt? |
| --- | --- | --- |
| `schema_version` höher als unterstützt | `Err(Invalid("unsupported schema_version …"))` (vor jedem Schreiben) | Ja |
| `schema_version` fehlt | `Err(Invalid("missing schema_version"))` | Ja |
| Sidecar von anderem Prozess gelockt | `Err(Conflict("sidecar is locked: …"))` | Ja |
| Inhalt bereits aktuell | `Ok(false)`, kein Backup/Schreiben | Ja (unverändert) |
| Backup-Schreibfehler (Platte voll/RO) | `Err(Io{…})` (kein Ziel-Ersatz) | Ja |
| Abbruch nach Backup, vor Rename | Tempartei verwaist; `.bak` = Original | Ja (Resume durch Neuausführung, §5.4) |
| Abbruch während `persist` des Ziels | Tempartei verwaist; Ziel unverändert (Rename nicht committet) | Ja |
| Erfolg | `Ok(true)`, Ziel ersetzt, `.bak` vorhanden | — (Ziel = migriert) |

---

## 3. Schema-Versions-Split

Es gibt **zwei unabhängige Versionsachsen**, die bewusst getrennt behandelt
werden (`feature/README.md` Festgelegte Entscheidungen; `Agents.md` →
Architekturgrenzen; `sidecar.md` → Manifest; `pipeline.md` → Bearbeitungsregler):

### 3.1 `schema_version` / `recipe_schema_version` (Datenform / Rezept)

- Top-Level-`schema_version` (aktuell `2`, Konstante in lib.rs) adressiert die
  Sidecar-**Hülle** und das Rezept-Envelope.
- Pro Rezeptinstanz existiert zusätzlich `recipe_version`
  (`EditRecipe.recipe_version`), unabhängig von `pipeline_version`.
- Ein Bump `recipe_schema_version` 1 → 2 ist laut `pipeline.md` (Z. 304) nötig,
  sobald verschachtelte Adjustment-Felder (`curves`, `hsl`, `color_grading`,
  `presence`, `sharpening`, `noise_reduction`) oder neue Top-Level-Keys
  (`geometry`, `lens_correction`, `perspective`, `effects`) verwendet werden.
- **Cache-Wirkung:** Änderungen an Rezeptwerten/`-version` fließen in den
  `recipe_hash` (RenderKey, `pipeline.md` Z. 560 ff.). Pro Stage gilt: eine
  Änderung **invalidiert ab der betroffenen Stufe** alle abhängigen
  Preview-/Export-Einträge — nicht jedoch Decode oder persistierte AI-Masken
  (Beispiel F-089: Kurvenänderung invalidiert „ab `adjustments`“, nicht Decode/
  Masken). `stage_digest` erlaubt stufenspezifische Invalidation
  (`pipeline.md` Z. 575 ff.).

### 3.2 `pipeline_version` (Render-Identität, separat validiert)

- `pipeline_version` ist Teil des **RenderKey** (`pipeline.md` Z. 560 ff.) und
  wird **getrennt** von `recipe_schema_version` validiert (`pipeline.md` Z. 292).
- Ein `pipeline_version`-Bump ändert die Render-Identität selbst → er
  invalidiert **alle** davon abgeleiteten Render-Caches (Preview/Export),
  unabhängig vom Rezeptinhalt, da der RenderKey insgesamt neu ist.
- Decode- und Masken-Caches werden **nicht** durch einen reinen
  Pipeline-/Schema-Bump invalidiert; sie hängen von
  `source_content_hash`/`decode_parameters` bzw. `mask_artifact_hashes` ab.

**Warum die Trennung:** Rezeptformänderungen (neue Regler) sind oft rein
additiv und betreffen nur die davon abhängigen Renderstufen; ein
Pipeline-Algorithmuswechsel dagegen verändert die gesamte Render-Identität.
Getrennte Versionen erlauben feingranulare Cache-Invalidierung und unabhängige
Validierung/Migration beider Achsen.

---

## 4. Pre-MVP-Entscheidung

Verbatim aus `Agents.todo.md` Phase 2 (Z. 137–150) und gespiegelt in
`feature/README.md` (Festgelegte Entscheidungen, Z. 147–152) sowie
`pipeline.md` Z. 308–315:

> **Produktentscheidung (2026-08-17, Pre-MVP, präzisiert):** Wir befinden uns in
> der Pre-MVP-Phase — das Sidecar-Schema wird bei Bedarf **bewusst nicht
> abwärtskompatibel** abgeändert. Es gilt:
> (a) Schemaänderungen sind bis zum MVP Breaking Changes, Altdateien müssen
> nicht lesbar bleiben;
> (b) die Migrations-Maschinerie (`migrate_sidecar_file`, `.bak`-Backup,
> `migrate_json`) bleibt dauerhaft im Code und wird ab dem MVP für
> Release-Migrationen genutzt;
> (c) die v1→v2-Migration mit ihren Tests (aus F-089/F-090) bleibt als Muster
> erhalten, aber **pre-MVP gibt es keinen Zwang, für jede Migration einen
> eigenen Test zu schreiben** — die Regel „Tests für jede Migration" gilt ab dem
> MVP.

Konsequenzen für dieses Strategiedokument:

- Die v1→v2-Migration (lib.rs `migrate_json`, Zweig `version == 1 → 2`) ist das
  **dauerhaft im Code verbleibende Template** für jeden künftigen Bump.
- Ab MVP gilt die **volle** Migrationsstrategie: verzögert mit Bestätigung,
  `.bak`-Backup, explizitem Aufruf (`migrate_sidecar_file`) — keine
  auto-stille Migration. Die Pre-MVP-Ausnahme (kein Test-Zwang pro Migration)
  endet mit dem MVP; danach ist die Regel „Tests für jede Migration“ bindend
  (siehe §7).

---

## 5. Release-Verfahren bei einem Schema-/Pipeline-Bump

Dieses Verfahren gilt **ab MVP** für jeden Release, der `schema_version`,
`recipe_schema_version` oder `pipeline_version` erhöht. Es ist die
Operationalisierung von `feature/README.md` („Migrationen erfolgen verzögert mit
Bestätigung, Backup und atomarem Write; CLI benötigt dafür ein ausdrückliches
Flag“) und ADR 0001.

### 5.1 Erkennen (Detect)

- Bestimme die neue Zielversion (`SCHEMA_VERSION` in lib.rs anheben; ggf.
  `recipe_schema_version`/`pipeline_version` in den betroffenen Rezept-/Pipeline-
  Strukturen erhöhen).
- Ergänze in `migrate_json` einen neuen Zweig (z. B. `version == 2 → 3`), der
  **nur** die neuen/defaulted Felder einführt und bestehende Daten erhält (Vorbild
  v1→v2: flache Map bleibt erhalten). Niemals Daten verwerfen oder still
  umdeuten.
- Dokumentiere die Semantik des Bumps **zuerst** im betroffenen Feature-Dokument
  (`feature/architecture/*.md`) und verlinke ihn, bevor Code landet
  (`Agents.md` Arbeitsablauf Schritt 4; `feature/README.md` Arbeitsweise).

### 5.2 Sichern (Backup)

- Der Aufruf erfolgt **ausschließlich** über `migrate_sidecar_file` (nicht über
  `migrate_json` + direktem Schreiben). `migrate_sidecar_file` schreibt das
  `.bak` automatisch **vor** dem atomaren Ersatz (§2.3).
- Der Benutzer/das Tool stellt sicher, dass ausreichend Schreibdatenträgerplatz
  für das `.bak` vorhanden ist; ein Backup-Fehler muss als `SidecarError::Io`
  abbrechen, bevor das Ziel verändert wird.

### 5.3 Migrieren + Roundtrip-Validieren

- `migrate_sidecar_file` führt `migrate_json` aus, das das migrierte Dokument
  intern über `SidecarDocument::from_json` → `to_json` validiert (Roundtrip,
  §2.2). Ein ungültiges Ergebnis führt zu `Err(Json/Invalid)` **vor** dem
  Schreiben.
- Idealerweise wird der Migrationsschritt im CLI/GUI nur auf ausdrücklichen
  Benutzerwunsch ausgeführt (CLI: `--migrate`-Flag; GUI: Bestätigungsdialog).
  Damit ist die **Bestätigung** die explizite Benutzeraktion.

### 5.4 Atomar schreiben + Original intakt halten

- `atomic_write_bytes` ersetzt das Ziel erst per Rename; ein Crash hinterlässt
  nur die Tempartei (§2.4).
- **Wiederaufnahme (Resume) nach fehlgeschlagener/unterbrochener Migration:**
  - Scheitert die Migration *vor* oder *während* des `.bak`-Schreibens: das
    Originalsidecar ist unverändert; einfach erneut ausführen.
  - Scheitert sie *nach* dem `.bak`, aber *vor* dem Rename des Ziels: das
    `.bak` entspricht exakt dem Original, das Ziel ist unverändert. Erneute
    Ausführung von `migrate_sidecar_file` liest das Original, migriert erneut,
    überschreibt das `.bak` mit demselben Original und ersetzt das Ziel — sicher
    und idempotent.
  - Verwaiste Temparteien werden durch `load_sidecar`/`recover_sidecar`
    automatisch aufgeräumt.
  - Ist das Ziel **bereits** migriert (erfolgreicher Lauf), erkennt ein
    erneuter Lauf `migrated == original` nicht mehr (Ziel ist jetzt v2) →
    `migrate_json` gibt es unverändert durch → `Ok(false)`, **kein** weiteres
    `.bak`, kein erneuter Schreibvorgang.

### 5.5 Index/abgeleitete Artefakte aktualisieren

- Eine optionale zentrale Indizierung (Phase 9) wird aus Sidecars neu
  aufgebaut; sie ist **nie** die Quelle der Migration. Nach einer
  Sidecar-Migration wird der Index aus dem (nun migrierten) Sidecar neu
  abgeleitet; ein widersprüchlicher Index verliert gegen das Sidecar
  (`sidecar.md` → Migration und Konflikte).
- Vorschauen/Exporte (Cache unter `.lumina/`, nicht autoritativ) werden über den
  geänderten RenderKey/`recipe_hash` automatisch als Cache-Miss neu gerendert
  (§3).

---

## 6. Kompatibilitätszusagen für Releases

Was Konsumenten (CLI, GUI, zukünftige Releases) **annehmen dürfen**:

- **Forward-Kompatibilität (neuer Code liest alte Datei):** Ein Sidecar mit
  `schema_version <= SCHEMA_VERSION` wird durch `migrate_json` auf den aktuellen
  Stand gebracht (Template v1→v2 kopiert für künftige Bumps). Ein bereits
  aktuelles Sidecar wird ohne Backup/Schreiben durchgereicht (`Ok(false)`).
- **Keine stille Backward-Kompatibilität (alter Code liest neue Datei):**
  Ein `schema_version > SCHEMA_VERSION` wird **explizit abgelehnt**
  (`Invalid("unsupported schema_version …; explicit migration is required")`).
  Der alte Code verschluckt die Datei nicht und verändert sie nicht. Eine
  Aktualisierung der Anwendung ist die vorgesehene Reaktion.
- **Pre-MVP:** Altdateien müssen nicht lesbar bleiben (Breaking Changes erlaubt,
  §4). Diese Härte endet mit dem MVP.
- **Seiteneffektfreiheit der Persistenz:** Das Originalbild bleibt byteweise
  unverändert; ein beschädigtes oder migriertes Sidecar macht das Original nicht
  unlesbar (`sidecar.md` Persistenzregeln).
- **Single Source of Truth:** Rezept, virtuelle Kopien und Maskendefinitionen
  leben ausschließlich im Sidecar. Auch eine spätere zentrale DB (Phase 9) ist
  nie die Quelle dieser Daten und darf Lumina-spezifische Daten nicht
  stillschweigend zur autoritativen Quelle machen. Ein XMP-Adapter (v1 nicht
  unterstützt) darf dasselbe nicht tun.
- **Keine stillen Fallbacks:** Inkompatible/veraltete Artefakte (Rezept,
  Farbprofil, Maske, fehlendes Artefakt) werden sichtbar gemeldet, nicht still
  ersetzt.

---

## 7. Test-/Verifikationserwartungen

Gemäß `Agents.md` (Verifizierung und Tests) und den bestehenden Tests in
`crates/lumina-sidecar/src/lib.rs`:

- **JSON-Roundtrip- und Schema-Migrationstests** (vorhanden):
  - `migration_unknown_fields_and_incompatible_version` (lib.rs, Z. 2411):
    v0→v1→v2, `recipe_version` wird restauriert; v99 →
    `Err(Invalid("unsupported schema_version 99; explicit migration is required"))`.
  - `explicit_v1_to_v2_migration_keeps_flat_adjustments` (lib.rs, Z. 2432):
    v1-Sidecar → v2, flache `adjustments` (`exposure`, `contrast`) bleiben
    erhalten.
- **Sidecar-Recovery-, Atomic-Write- und Konflikttests** (vorhanden):
  - `explicit_file_migration_creates_backup_and_rejects_newer_schema`
    (lib.rs, Z. 2576): Datei-Migration erzeugt `<pfad>.bak`, danach
    `schema_version == SCHEMA_VERSION`; v99 → `Err`.
  - `atomic_compare_and_swap_and_recovery` (lib.rs, Z. 2593): Konflikt bei
    falscher Revision (`save_sidecar_if_unchanged`) sowie Aufräumen verwaister
    Temparteien.
- **Ab MVP bindend:** Für **jeden** neuen Migrationsschritt ist ein eigener Test
  zu schreiben (Roundtrip + Atomic-Write + Recovery/Conflict), der die neue
  Funktion tatsächlich abdeckt (Pre-MVP-Ausnahme entfällt, §4). Der
  unabhängige Verifizierungs-Agent prüft ausdrücklich, ob die Tests die
  Migration abdecken (`Agents.md` → Verifizierungs-Agent).
- **Richtlinie für künftige Bumps:** Jeder neue `migrate_json`-Zweig wird im
  selben Commit mit (a) Roundtrip-Test, (b) Datei-Migrations-Test inkl.
  `.bak`-Nachweis und (c) „neuere Version wird abgelehnt“-Test ausgeliefert.

---

## 8. Bekannte Lücken

Diese Punkte sind im Code/CLI-Stand 2026-08-19 **nicht** durch die oben
beschriebene Mechanik abgedeckt und werden hier transparent geführt (keine stillen
Annahmen):

1. **CLI ist noch nicht an `migrate_sidecar_file` angebunden (F-019, Post-MVP).**
   `crates/lumina-cli/src/main.rs` (`migrate_sidecar`, Z. 608) ruft aktuell
   `lumina_sidecar::migrate_json` und schreibt danach via eigenem
   `write_atomically` — **ohne `.bak`-Backup und ohne `acquire_write_lock`**.
   Die von ADR 0001 / `feature/README.md` geforderte Kombination
   (Backup + Bestätigung + Flag) ist im CLI-Pfad also **noch nicht** vollständig
   umgesetzt. `Agents.todo.md` Phase 2 (Z. 152–155) führt dies als F-019
   (deferriert auf Post-MVP) und vermerkt: „Library-Teil ist verifiziert.“ Die
   Strategie gilt ab MVP für den **Library**-Vertrag; die CLI-Verdrahtung ist
   eine offene Folgeaufgabe.
2. **Keine dedizierte `BackupFailed`-Fehlervariante.** Die Aufgabenbeschreibung
   nennt „locked/backup-failed/invalid“. Im Code existieren nur
   `Conflict` (Lock) und `Invalid` (Schema); ein fehlgeschlagenes Backup oder
   ein atomarer Schreibfehler werden als `SidecarError::Io` berichtet. Das ist
   funktional korrekt (Original bleibt intakt, da vor dem Ziel-Ersatz
   abgebrochen wird), aber nicht als eigenständiger Fehlertyp benannt.
3. **`migrate_json` kennt nur v0→v1→v2.** Ein künftiges `schema_version = 3`
   hat (noch) keinen Migrationszweig; eine mit v3 geschriebene Datei wird von
   v2-Code korrekt als `Invalid` abgelehnt. Der v1→v2-Zweig ist das zu
   kopierende Template (§4/§5.1). Dies ist beabsichtigter Zustand vor dem MVP,
   kein Defekt.
4. **Keine Zwei-Dateien-Transaktion (JSON + zdata).** JSON- und `.lumina.zdata`-
   Schreibvorgänge sind je einzeln atomar; eine gemeinsame Transaktion ist
   bewusst offen (`sidecar.md` Binärdaten; `save_sidecar`-Kommentar). Eine
   Schema-Migration des JSON-Sidecars sollte daher keine gleichzeitige,
   atomaritätsbedürftige `.zdata`-Änderung erfordern.
