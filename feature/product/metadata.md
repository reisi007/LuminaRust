# Metadaten: Keywords, Filter, Sammlungen, Stapel (G-15 META-MVP)

Normativer SOLL-Stand 2026-09-19. Goal-Referenz: `.goal/Goal.md` G-15
(~10 %). Slice 1 (Sidecar-Schema: `keywords`, `collections`,
`SmartCollectionDef`/`SmartRule`, `BatchOp`/`apply_batch_op`,
`schema_version` 2, additiv-optional) und Slice 2 (CLI: `keywords`,
`collections`, `batch-meta`, `smart-collections`, Katalogformat
`lumina-smart-catalog` v1) sind BESTANDEN und werden wiederverwendet —
**keine Schema-Brüche, keine zweite Persistenz**. §6/§7 ergänzen die
Bilderstapel-Vollfunktion (LRPAR-G15-STACK-15) rein additiv: dieselbe
Sidecar-Persistenz, dasselbe `schema_version` (kein Bump), keine zweite
Quelle.

## 1. Keywords (GUI, Library-Modul)

- Das Library-Panel zeigt die Quellbild-Keywords des geladenen Dokuments
  (`SidecarDocument.keywords`) und erlaubt Hinzufügen/Entfernen.
- Jede Mutation läuft über `apply_batch_op` (`AddKeyword`/`RemoveKeyword`)
  und den normalen `save_sidecar`-Pfad (CAS, atomar); Fehler sind laut
  (sichtbarer Status, `error!`), ungültige Keywords werden abgelehnt, nie
  still normalisiert. Validierungsregeln = Slice 1 (getrimmt-nicht-leer,
  keine führenden/folgenden Whitespaces, keine Steuerzeichen, ≤128 Zeichen,
  ≤512 Einträge, keine exakten Duplikate).
- User-Aktionen loggen `info!` (vergeben/entfernt/unverändert-idempotent).
- Kein absoluter Pfad, keine Original-Mutation.

## 2. Filter (über die `\`-Leiste hinaus)

- Die `\`-Leiste (Text + `rating:`/`flag:`/`label:`) bleibt; zusätzlich
  versteht der Library-Filter die Präfixe `keyword:` (exakt,
  case-sensitiv wie Slice 1; nur Einwort-Keywords — Mehrwort-Keywords sind
  per `keyword:` nicht filterbar, s. u.), `collection:` (exakte `id` oder
  exakter `name`, jeweils case-insensitiv), `camera:`
  (case-insensitiver Teilvergleich auf
  `make + model`), `iso:` (exakt, z. B. `iso:400`) und
  `focal:`/`focal_length:` (exakt in mm, z. B. `focal:50`).
- Mehrere Token (Leerzeichen-getrennt) werden mit UND verknüpft; ein
  erkanntes Präfix mit unparsebarem Wert matcht nichts (sichtbar leeres
  Grid, niemals stilles Pass-Through). `collection:`- und `camera:`-Werte
  dürfen Leerzeichen enthalten: Folgetoken ohne `:` gehören zum Wert
  (`collection:best of` matcht die Sammlung `Best Of`). Unverändert: leere
  Query matcht alles; reiner Text matcht den Dateinamen (case-insensitiv).
- EXIF-Quellen (`camera`/`iso`/`focal`) stammen aus
  `lumina_raw::read_metadata` (best effort pro Scan-Eintrag); fehlt die
  Metadatei, matcht das entsprechende Prädikat nichts. Filter lesen nur
  gecachte Scan-Einträge — kein zusätzliches IO pro Frame.
- Zusätzlich filtert die aktive Sammlungsauswahl (statisch: Mitglieds-`id`;
  smart: Regel-Auswertung über Eintrags-Keywords + Default-Copy-
  Rating/Flag) das Grid.

## 3. Sammlungen + Smart-Sammlungen (GUI, Sidecar-first)

- Statische Mitgliedschaften werden pro Sidecar als
  `SidecarDocument.collections` (`{ id, name }`, Quellbild-Ebene)
  persistiert — exakt wie Slice 1/CLI (`AddToCollection`/
  `RemoveFromCollection` via `apply_batch_op` + `save_sidecar`).
  Umbenennen = Stapel-Operation über alle betroffenen Sidecars.
- Die GUI aggregiert die Sammlungsliste durch Scannen der Sidecars im
  Verzeichnis (Rebuild aus Sidecars, keine DB-Autorität); Auswahl filtert
  das Grid (s. §2).
- Smart-Sammlungen sind versionierte Daten (`SmartCollectionDef`,
  `version = 1`): Die GUI lädt/speichert einen portablen Katalog im
  CLI-identischen Format (`{"format":"lumina-smart-catalog","version":1,
  "collections":[...]}`), validiert mit `validate_smart_collection_def`
  (laut, kein stiller Fallback) und wertet mit `matches_any_copy`
  (`keywords` + Default-Copy-`rating`/`flag`) aus. Der Katalog enthält nie
  Pfade.
- Regel-Editor (Mindestumfang): `All`/`None`/`Keyword`/`RatingAtLeast`/
  `RatingEquals`/`Flag` plus `Not`/`And`/`Or`-Komposition; Keyword-Teil-
  regeln unterliegen denselben Grenzen wie Sidecar-Keywords,
  Rating-Teilregeln `0..=5`.

## 4. Batch-Vollfunktion (GUI, Auswahl)

- `Anwenden auf Auswahl`: genau ein `BatchOp` (alle sechs Varianten:
  `add_keyword`, `remove_keyword`, `add_to_collection`,
  `remove_from_collection`, `set_rating`, `set_flag`) wird über die
  Filmstreifen-Auswahl iteriert (leere Auswahl = aktives Bild); je Datei
  atomar (`load_sidecar` → `apply_batch_op` → `validate` →
  `save_sidecar`); Fehler sind pro Bild laut (`error!` + Reporteintrag)
  und brechen den Rest nie ab (vgl. Sync/Match-Pattern).
- Idempotente No-Ops (`Ok(false)`) zählen als unverändert, nicht als
  Fehler; sie werden in `applied` mitgezählt (der Report kennt nur
  `applied`/`failed`, anders als die CLI, die changed/unchanged trennt).
  Ergebnis als `SelectionSyncReport` (`applied`/`failed`) plus
  Statuszeile und `info!`-Logs je Datei.
- Ein Bild ohne Sidecar ist ein lauter Pro-Bild-Fehler (kein stilles
  Erzeugen von Quell-Identitäten): Nie editierte Bilder erst einmal
  öffnen/bewerten, bevor der Stapel sie erreicht.
- Stapel berührt niemals Rezepte, Masken oder History; das Original bleibt
  byte-identisch.

## 5. Abnahme (Slice 3)

- CLI-Seite (Slice 2) ungebrochen; `cargo test -p lumina-gui` grün ohne
  GPU; `cargo fmt --check`, `cargo clippy -p lumina-gui -- -D warnings`
  grün.
- Je Ansicht/Aktion ein Headless-Test (egui Context + LuminaApp + tempdir)
  mit Kette Edit→Commit→Datei→Reload: Keyword-Roundtrip (+ Ablehnung
  ungültig), Sammlungs-Roundtrip (+ batch-Umbennenung über 2 Dateien),
  erweiterte Filterprädikate (alle Präfixe + UND + unparsebar→leer +
  Keyword-Case-Sensitivität), Smart-Katalog Save→Load→Match (+ Ablehnung
  ungültig), Stapel über Auswahl (angewendet + Reload beider Dateien,
  Fehlerisolation).
- DoD §7-BESTANDEN-Checkliste gilt (End-to-End-Kette, zeitbasierte Pfade —
  hier: keine Timer, alles synchroner Commit —, Klassen-Vollständigkeit
  aller Präfixe/Op-Varianten, `info!`-Level, Spez→Test-Mapping, Gates).

## 6. Bilderstapel (LRPAR-G15-STACK-15, GUI, Sidecar-first)

- Ein **Stapel** gruppiert mindestens zwei Quellbilder **desselben Ordners**
  zu einer zuklappbaren Einheit (Lightroom-„Stack"). Stapel sind
  Quellbild-Ebene (wie Keywords/Masken-Artefakte), nicht Rezept- oder
  Kopien-Ebene; Rezepte, Masken und History werden nie berührt.
- **Mitgliedschaft** wird Sidecar-first in **jedem** Mitglieds-Sidecar als
  additives, optionales Top-Level-`stack`-Feld persistiert
  (`SidecarDocument.stack: Option<StackMembership>`, `schema_version` bleibt
  unverändert — kein Bump, analog `keywords`/`collections`/`face`/`culling`).
  Fehlt das Feld, ist das Bild nicht gestapelt; es wird beim Serialisieren
  wieder ausgelassen (legacy-byte-stabil).
- `StackMembership` trägt: `version` (= `STACK_SCHEMA_VERSION` = 1),
  `stack_id` (innerhalb des Ordners stabile ID; bei Anlage deterministisch
  aus den sortierten Mitgliedsnamen abgeleitet und danach nie neu berechnet),
  `cover` (relativer Dateiname des Deckbilds, gleicher Ordner), `members`
  (sortierte, eindeutige relative Dateinamen inklusive `cover`, mindestens
  zwei) und `collapsed` (Zuklappstatus, `false` wenn abwesend).
- **Gleicher Ordner, portabel:** `cover`/`members` sind reine Dateinamen ohne
  Pfadtrenner; absolute Pfade und `..`/`.` werden laut abgelehnt. Ein
  verschobenes Sidecar-Bundle bleibt gültig.
- **Zuklappstatus** ist persistent (`collapsed`). Jeder Toggle schreibt alle
  Mitglieds-Sidecars; bei gleichzeitigen Schreibern gilt **last-writer-wins**
  (kein Merge, kein Dialog). Lesend bevorzugt die GUI den Wert des
  `cover`-Sidecars und fällt nur bei fehlendem `cover`-Eintrag auf das
  betrachtete Sidecar zurück (nie still auf „aufgeklappt").
- **Schreibpfad:** je Datei atomar (CAS/Rebase wie Sync/Batch); Fehler sind
  pro Bild laut (`error!` + Status) und brechen die übrigen nie ab. Fehlt
  einem gewählten Bild das Sidecar, ist das ein lauter Fehler — es wird kein
  Sidecar still erzeugt. Anlage verlangt mindestens zwei Sidecar-tragende
  Bilder im selben Ordner.
- **Validierung** ist laut und ohne stille Normalisierung: `version`-Pin,
  `stack_id` nicht leer/≤128 Zeichen/keine Steuerzeichen, jedes Mitglied
  gültiger Dateiname/≤512 Zeichen, keine Duplikate, `cover ∈ members`,
  2 ≤ `members` ≤ 256. Unbekannte/höhere `schema_version`-Sidecars lehnt der
  Loader weiterhin laut ab (`from_json`).
- **GUI (Grid + Filmstrip):** Ein zugeklappter Stapel zeigt nur das
  Deckbild mit einem Stapel-Badge (`count`); ein aufgeklappter Stapel zeigt
  alle Mitglieder, jedes mit Stapel-Kennzeichnung. Klick auf irgendein
  Mitglied selektiert den **gesamten Stapel**; die Tastatur-Navigation
  (`move_library_selection`) überspringt bei zugeklapptem Stapel die
  verdeckten Mitglieder. Dadurch wirken Sync/Batch/Previous automatisch auf
  den Stapel als Einheit.
- **Sichtbare Funktionen (klickbare Buttons im Library-Metadaten-Panel):**
  `Stack` (aus der Auswahl ≥2 Bilder desselben Ordners anlegen),
  `Unstack` (Stapel der Auswahl auflösen) und ein Zuklapp-/Aufklapp-Toggle.
  Kein Shortcut ist erforderlich; ein späterer Shortcut wäre nur ein Alias
  auf denselben Button-Pfad.
- Kein absoluter Pfad, keine Original-Mutation.

## 7. Abnahme (STACK-15)

- `cargo test -p lumina-sidecar` grün (Schema-Roundtrip, Validierung aller
  Ablehnungsfälle, `schema_version` unverändert, absolute Pfade laut).
- `cargo test -p lumina-gui` grün ohne GPU; `cargo fmt --check`,
  `cargo clippy -p lumina-gui --all-targets -- -D warnings` grün.
- Je Ansicht/Aktion ein Headless-Test (egui Context + LuminaApp + tempdir)
  mit Kette Edit→Commit→Datei→Reload: Stapel anlegen (≥2, gleicher Ordner),
  Anlage ablehnen (ungültige Auswahl/Fremdordner/fehlendes Sidecar),
  Zuklappen/Aufklappen persistiert und nach Reload wiederhergestellt, Grid +
  Filmstrip zeigen zugeklappt nur den `cover`, aufgeklappt alle Mitglieder,
  Klick selektiert den ganzen Stapel, `Unstack` entfernt die Mitgliedschaft
  in allen Sidecars.
- DoD §7-BESTANDEN-Checkliste gilt (End-to-End-Kette, zeitbasierte Pfade —
  hier: keine Timer, alles synchroner Commit —, Klassen-Vollständigkeit,
  `info!`-Level, Spez→Test-Mapping, Gates).
