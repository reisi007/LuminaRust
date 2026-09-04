# IPTC-Metadaten: Draft, Presets, Sync, JPEG-Bake-In (LRPAR-G15-IPTC)

Normativer SOLL-Stand 2026-09-04 (Doku-first, kein Code). Goal-Referenz:
`.goal/Goal.md` G-15. Entscheid: `feature/decisions/LRPAR-G15-META-15.md`
(User-Entscheid 2026-09-04: Vorziehen auf Release 1.0, Sidecar-Speicher,
pure-Rust-Write ohne Runtime-Abhängigkeit, JPEG-only, GUI gleich mit,
MCP pfadbasiert + Resources). Ergänzt — ohne Bruch — das Metadaten-MVP
[`metadata.md`](metadata.md) (Keywords, Filter, Sammlungen, Stapel):
**keine Schema-Brüche, keine zweite Persistenz**.

## 1. Ziel und Abgrenzung

Änderungen an IPTC-Core-Feldern bleiben bis zum Export als **Entwurf
(Draft)** im Sidecar und werden nur in **neu erzeugte Exportdateien**
eingebrannt. Originale werden strikt read-only behandelt. Der Umfang:

- Draft mit eigener, von der Bearbeitungshistorie getrennter Historie (§3).
- Statische und dynamische Presets (Platzhalter, Prompt-on-Paste) (§5).
- Feld-selektiver Sync zwischen Bildern (§6).
- Export-Bake-In als Opt-in für **JPEG** (§7).
- CLI (`lumina meta …`, §8), MCP-Tools + Resource (§9), GUI-Panel (§10).

Nicht-Ziele: XMP/IPTC-**Lesung** als Bearbeitungsquelle (Post-MVP), EXIF-Write
(Post-MVP gemäß `export.md`), IPTC-Extension-Set, PNG/WebP-/TIFF-Bake-In,
Veröffentlichungsdienste (nie Ziel).

## 2. Normative Invarianten

- Originale bleiben byte-identisch; der Bake-In schreibt ausschließlich in
  neue Exportdateien (Ziel-Guard gegen Quelle und `.lumina.*`-Bundle).
- Das Sidecar bleibt einzige Quelle der Wahrheit für Entwürfe, Presets-
  Anwendungsergebnisse und Historie. Keine SQLite-Pflicht, keine XMP-Sidecars.
- Kein stiller Fallback: unbekannte Felder, ungültige Werte, unaufgelöste
  Platzhalter, fehlendes Sidecar, IIM-Limitüberschreitung und fehlende
  Format-Capability sind laute Fehler; Export ohne Opt-in bleibt exakt wie
  heute (keine Metadaten, keine stillen Annahmen).
- Entwürfe sind Quellbild-Ebene (nicht pro virtueller Kopie); Sync und Bake-In
  berühren nie Rezepte, Masken oder die Bearbeitungshistorie.
- Keine absoluten Pfade in `metadata`.

## 3. Datenmodell: Sidecar-`metadata` (Version 1)

Neues, additiv-optional Quellbild-Feld in `SidecarDocument` (Muster
`keywords`/`collections`: `#[serde(default)]`, absent = leerer Entwurf, kein
Migrationszwang pre-MVP):

```json
"metadata": {
  "version": 1,
  "draft": {
    "title": "Startschuss",
    "description": "…",
    "copyright_notice": "© 2026 …",
    "creator": "…",
    "city": "Berlin"
  },
  "history": [
    { "rev": 2, "timestamp": "2026-09-04T10:00:00Z",
      "origin": "preset:veranstaltung", "changed": ["title", "city"] },
    { "rev": 1, "timestamp": "2026-09-04T09:30:00Z",
      "origin": "manual", "changed": ["description"] }
  ]
}
```

- **Feldwerte** gelten nur für in §4 registrierte Feld-IDs; `keywords` ist
  **kein** Draft-Feld — es bleibt das bestehende Sidecar-Feld und wird von
  Draft-UI/Sync/Export nur mitgeführt (Routing).
- **Historie:** `rev` startet bei 1 und ist streng monoton; je Mutation ein
  Eintrag mit `timestamp` (RFC 3339, UTC), `origin`
  (`manual` | `preset:<name>` | `sync:<quell-dateiname>` | `cli` | `gui` |
  `mcp`) und `changed` (Liste betroffener Feld-IDs). Speicherordnung
  neueste-zuerst (der erste Eintrag trägt die höchste `rev`); neue Einträge
  werden vorangestellt. Cap **100 Einträge**, FIFO (älteste fallen
  deterministisch vom Ende). Die Historie ist Herkunfts-/Diagnose-
  kontext, **kein Undo**; Leeren nur als ausdrückliche Operation.
- **Persistenz:** jede Mutation via `load → mutate → validate → save`
  (CAS/`save_sidecar_if_unchanged`, atomarer Write, pro-Ziel-Schreiblock wie
  heute). Konflikt = lauter Fehler, nie stilles Last-Write-Wins.
- **Validierung:** laut (Werte getrimmt-nicht-leer oder leer = Feld entfernen,
  keine Steuerzeichen, Feld-Limits §4, `date_created` nur `YYYY-MM-DD`),
  keine Still-Normalisierung; unbekannte Feld-IDs werden abgelehnt.

## 4. Feld-Registry (MVP)

Feste, stabile Feld-IDs; IIM- und XMP-Mappings sind Teil des Registry-
Vertrags und werden getestet (Re-Parse-Roundtrip):

| Feld-ID | Bedeutung | IIM | XMP | Sidecar-Limit |
| --- | --- | --- | --- | --- |
| `title` | Titel (ObjectName) | 2:05 | `dc:title` | ≤ 256 Zeichen |
| `headline` | Schlagzeile | 2:105 | `photoshop:Headline` | ≤ 256 |
| `description` | Beschreibung | 2:120 | `dc:description` | ≤ 2000 |
| `copyright_notice` | Urheberrechtsvermerk | 2:116 | `dc:rights` (+ `xmpRights:Marked=true`) | ≤ 256 |
| `creator` | Ersteller (By-line) | 2:80 | `dc:creator` | ≤ 256 |
| `credit` | Credit | 2:110 | `photoshop:Credit` | ≤ 256 |
| `source` | Quelle | 2:115 | `photoshop:Source` | ≤ 256 |
| `city` | Ort (Stadt) | 2:90 | `photoshop:City` | ≤ 128 |
| `state_province` | Bundesland/Kanton | 2:95 | `photoshop:State` | ≤ 128 |
| `country` | Land | 2:101 | `photoshop:Country` | ≤ 128 |
| `date_created` | Aufnahmedatum | 2:55 (`YYYYMMDD`) | `photoshop:DateCreated` | Format `YYYY-MM-DD` |

- `keywords` → IIM 2:25 + `dc:subject` (aus dem bestehenden Sidecar-Feld,
  inkl. dessen Validierung ≤ 128 Zeichen/≤ 512 Einträge).
- IIM `1:90` CodedCharacterSet = UTF-8 wird immer gesetzt (kein Mojibake).
- Post-MVP (dokumentierte Grenzen, keine stillen Erweiterungen):
  IPTC-Extension, `2:85` By-lineTitle, `2:92` Sub-location, `2:100`
  CountryCode, `xmpRights:UsageTerms`, EXIF-Write.

## 5. Meta-Presets (statisch + dynamisch)

Portabel als `<name>.lumina-meta-preset.json` (Format-Envelope
`"lumina-meta-preset"`, `version` 1) im **selben** globalen Presets-
Verzeichnis wie Edit-Presets (`<name>.lumina-preset.json`-Logik):

```json
{ "format": "lumina-meta-preset", "version": 1, "name": "Veranstaltung",
  "fields": { "title": "{event_name} — Beispiel", "city": "{ort}" },
  "placeholders": [ { "name": "event_name", "description": "…" } ] }
```

- **Statisch:** `placeholders` leer; Werte sind feste Registry-Werte.
- **Dynamisch:** `{platzhalter}`-Syntax, Namen `[a-z][a-z0-9_]*`; `{{`/`}}`
  escapen Literal-Klammern. Jede Anwendung verlangt **alle** Platzhalter-
  variablen (CLI `--var name=wert` wiederholbar, GUI-Prompt-Dialog, MCP
  `vars`-Objekt). Unaufgelöste Platzhalter, unbekannte Variablen und
  Werte, die Registry-Limits verletzen, sind **laute Fehler** (nichts
  wird geschrieben, all-or-nothing je Ziel).
- Anwendung schreibt je Ziel per CAS + atomar, `origin = "preset:<name>"`,
  je Ziel ein Historie-Eintrag; Idempotenz: gleiches Ergebnis →
  `unchanged` (kein Fehler, kein History-Eintrag).

## 6. Feld-selektiver Sync

`meta sync` überträgt ausgewählte Felder vom Quellbild-Entwurf auf beliebig
viele Ziele:

- `--fields` ist **Pflicht** (Registry-IDs, kommagetrennt; `keywords`
  erlaubt); ohne Angabe kein Still-All.
- Ziel ohne Sidecar = lauter Pro-Bild-Fehler („zuerst importieren“), kein
  stilles Anlegen von Quellidentitäten (Muster: Stapel in `metadata.md` §4).
- Pro Ziel CAS + atomar + eigener Historie-Eintrag
  (`origin = "sync:<quell-sidecar-id>"`); Fehler brechen die Serie nie ab.
- Report: `updated` / `unchanged` / `failed` (pro Datei mit Grund).
- Es werden **nur** Draft-Felder (+ Keywords) übertragen — nie Rezepte,
  Masken, Bearbeitungshistorie oder Originale.
- **Mirror-Semantik (S5):** Für jedes selektierte Feld gilt der Quellwert —
  ein im Quellentwurf fehlendes Feld wird auf den Zielen **entfernt**,
  `keywords` ersetzt die Zielliste als Ganzes (auch leere Quellliste leert).
  Nicht-selektierte Felder bleiben unberührt. `changed` listet nur tatsächlich
  betroffene IDs (sortiert); ohne Änderung kein Historie-Eintrag
  (`unchanged`). `origin` trägt den Quell-**Dateinamen**
  (`sync:<dateiname>`, nie einen Pfad).

## 7. Export-Bake-In (JPEG, Opt-in)

- `--write-metadata` an `lumina export`, `lumina process`, `lumina batch`
  (und später MCP `lumina_trigger_export`). **Ohne** Opt-in unverändertes
  heutiges Verhalten (keine Metadaten).
- **Nur JPEG:** PNG/WebP mit `--write-metadata` = lauter Fehler pro Datei
  (CLI-Exit ≠ 0; Batch: Item `failed` mit Grund). TIFF = Post-MVP.
- **Inhalt:** zusammengeführte Draft-Felder (+ `keywords`). Leeres Feld →
  Tag wird weggelassen (nie leere Tags). Komplett leerer Entwurf und keine
  Keywords → Export **erfolgt** mit lauter Warnung und
  `metadata_written: "empty"` im Record (kein stiller No-Op).
- **Kein EXIF-Write, keine Übernahme** eingebetteter Quell-Metadaten in den
  Export (Post-MVP); der Export schreibt ausschließlich Lumina-Entwürfe.
- **Ablauf:** Encode → Temp-Datei → JPEG-Segment-Splice (APP13/8BIM-IIM +
  APP1-XMP; Einfügeordnung deterministisch) → atomares Rename →
  `ExportRecord.metadata_written` (additiv-optional:
  `{ iim: bool, xmp: bool, status: "written" | "empty" }`).
- **Limits:** überschreitet ein Sidecar-gültiger Wert das IIM-Oktettlimit
  seines Feldes, ist der Export **laut fehlerhaft** (Feld + Limit genannt);
  kein Still-Kürzen.
- **Determinismus:** gleiches Rezept + gleiche Drafts → byte-identische
  Exportdatei; Render-Cache und Pixel-Goldens unberührt (Splice ist
  Post-Encode). **Nicht-destruktiv:** Ziel-Guard gegen Quelle/Bundle wird
  wiederverwendet (`write_output_guarded`-Muster); Original bleibt
  byte-identisch.

## 8. CLI-Schnittstelle

Alle Draft-Operationen laufen über denselben Sidecar-Pfad (CAS, atomar,
laut); Exit-Codes 0/≠ 0 mit klarer stderr-Meldung.

- `lumina meta inspect <path>` — zeigt eingebettete IPTC der Quelle (JPEG:
  IIM/XMP-Read; RAW/Raster ohne IIM/XMP: laut „nicht verfügbar“), das
  Draft-Overlay je Feld (Draft-Wert vs. Embedded-Wert), Keywords und
  Historie-Länge.
- `lumina meta draft set <path> --field <id>=<wert>…` (wiederholbar;
  `keywords` routet aufs bestehende Feld). Unbekannte ID/ungültiger Wert →
  lauter Fehler, **nichts** geschrieben (all-or-nothing pro Aufruf).
- `lumina meta draft clear <path> (--field <id,id,…> | --all)`;
  `--all` leert den Entwurf (Historie bleibt).
- `lumina meta history show <path> [--limit n]`,
  `lumina meta history clear <path>` (ausdrücklich).
- `lumina meta preset list [dir]` / `show <name|pfad>` /
  `apply <preset> --target <pfade…> [--var name=wert]…`.
- `lumina meta sync --source <datei> --target <dateien…> --fields <id,id,…>`.
- Export-Flags: `--write-metadata` bei `export`, `process`, `batch`.

## 9. MCP-Schnittstelle (F-101-Erweiterung)

Pfadbasierte Tools (Muster der F-101-F1-Bulk-Tools: neben der Single-Image-
Session, keine Session-Mutation). Normative Details zusätzlich in
`platform/mcp-server.md`:

- `lumina_get_metadata_draft { path }` → `{ path, embedded, draft, keywords,
  history_len, status }`.
- `lumina_update_metadata_draft { path, fields?, clear_fields? }` →
  `{ ok, rev }`; Write-Through per CAS, `SidecarConflict` → `-32010`
  (analog `lumina_edit`).
- `lumina_apply_meta_preset { paths, preset, vars? }` → per-Pfad-Report
  (`updated`/`unchanged`/`failed`); dynamische Presets bekommen `vars` als
  JSON-Objekt, Fehlendes/Unbekanntes = lauter Tool-Fehler.
- `lumina_batch_sync_metadata { source, targets, fields }` → Report wie §6.
- `lumina_trigger_export { path, output_path, format, quality?,
  virtual_copy?, write_metadata: true }` → wrappt den `lumina_save`-Choke-
  Point; `write_metadata` + Nicht-JPEG = `InvalidParams` (laut).
- **Resource (Read-only):** `metadata://draft/<urlencoded-pfad>` — stabile
  Adresse über den **Pfad**, nicht `image_id` (prozess-lokal, Neustart →
  IDs neu). `resources/read` liefert dasselbe JSON wie
  `lumina_get_metadata_draft`; der Server deklariert die `resources`-
  Capability (`subscribe`/`listChanged` false). `prompts` bleiben ohne.

## 10. GUI: Metadaten-Panel (Library, egui)

- Rechte Spalte „Metadaten“: Draft-Feld-Editor (alle §4-Felder;
  `description` mehrzeilig; `date_created` mit Format-Validierung),
  Keywords-Chips (bestehende Komponente + Validierung), Embedded-Werte
  nur lesend (wenn JPEG/IIM verfügbar), Historie sichtbar (letzte Einträge).
- **Preset-Auswahl + „Anwenden“**: dynamische Presets öffnen einen
  Prompt-Dialog mit je einem Pflicht-Eingabefeld pro Platzhalter; Abbrechen
  ändert nichts.
- **„Auf Auswahl synchronisieren“**: Feld-Checkboxen (Default: alle Draft-
  Felder + Keywords), Report (applied/failed) + Statuszeile + `info!` je
  Datei (Muster: Stapel in `metadata.md` §4).
- Jede Mutation über denselben Sidecar-Pfad wie CLI (CAS, atomar, laute
  Fehler); keine zweite Metadaten-Logik im GUI.

## 11. Test-Strategie und Abnahme

- **Schema (S1):** additive Lese-/Roundtrip-Tests, Validierungs-Matrix
  (Limits, `date_created`, unbekannte IDs), Historie-Cap/`rev`-Monotonie,
  CAS-Konflikt, atomar. `cargo test -p lumina-sidecar`.
- **IPTC-Writer (S2):** Property-Tests „Splice lässt Pixelbytes byte-identisch“,
  Re-Parse-Roundtrip (alle Registry-Felder, UTF-8, Escapes), malformed JPEG
  = lauter Fehler. `cargo test -p lumina-iptc`.
- **CLI (S3–S5):** set→inspect→clear-Roundtrip, Preset-Anwendung (statisch +
  dynamisch + Fehlerfälle), Sync-Fehlerisolation/Idempotenz, Exit-Codes.
- **Bake-In (S6):** Golden-JPEG (Tags re-lesbar, Pixelbytes identisch zum
  Export ohne `--write-metadata`), IIM-Limit-Fehler, PNG/WebP lauter,
  Guard gegen Quelle/Bundle, `ExportRecord`-Inhalt.
- **MCP (S7):** Tool-Schemas, Fehlerpfade (`-32010` CAS, `InvalidParams`
  Nicht-JPEG), initialize-Capabilities inkl. `resources`.
- **GUI (S8):** Headless-Tests (egui Context + LuminaApp, tempdir) je Aktion
  mit Kette Edit→Commit→Datei→Reload; kittest-Golden des Panels;
  `cargo test -p lumina-gui` grün ohne GPU.
- Übergreifend: `cargo fmt --check`, `cargo clippy --workspace -- -D
  warnings`; keine Netzwerk-/ExifTool-Abhängigkeit in Tests (pure Rust).
- Verifizierungsbericht nach `DoD.md` §7 (End-to-End-Kette Draft→Sync→Export,
  Klassen-Vollständigkeit der Fehlerpfade, Spez→Test-Mapping).

## 12. Bekannte Grenzen (dokumentiert, Post-MVP)

- Bake-In nur JPEG; PNG/WebP lauter Fehler, TIFF später.
- Kein EXIF-Write, keine Metadaten-Weitergabe aus der Quelle.
- Keine XMP-/IPTC-Lesung als Import-/Bearbeitungsquelle (Embedded-Read nur
  für `meta inspect`/GUI-Anzeige aus JPEG IIM/XMP).
- Kein IPTC-Extension-Set; Zusatzfelder (`2:85`, `2:92`, `2:100`,
  `UsageTerms`) folgen als Registry-Erweiterung mit Tests.
- Der XMP-Read-Scanner erkennt die konventionellen Präfixe
  (`dc:`/`photoshop:`/`xmpRights:` eigener Ausgabe + gängiger Tools);
  exotische Präfix-Aliase werden als absent gelesen (Toleranz, kein
  stiller Fehler bei vorhandenem XMP-Root).
- Historie ist kein Undo; löschen nur ausdrücklich.
