# Metadaten: Keywords, Filter, Sammlungen, Stapel (G-15 META-MVP)

Normativer SOLL-Stand 2026-09-04. Goal-Referenz: `.goal/Goal.md` G-15
(~10 %). Slice 1 (Sidecar-Schema: `keywords`, `collections`,
`SmartCollectionDef`/`SmartRule`, `BatchOp`/`apply_batch_op`,
`schema_version` 2, additiv-optional) und Slice 2 (CLI: `keywords`,
`collections`, `batch-meta`, `smart-collections`, Katalogformat
`lumina-smart-catalog` v1) sind BESTANDEN und werden wiederverwendet —
**keine Schema-Brüche, keine zweite Persistenz**.

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

## 4. Stapel-Vollfunktion (GUI)

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
