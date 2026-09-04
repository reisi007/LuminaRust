# KI-Culling (Assisted Culling) — Scope-Entscheid (Release 2.5)

- **Task:** LRPAR-G09-CULL-25 (Release: 2.5, Doku-first, kein Code)
- **Goal:** G-09 Library (`/.goal/Goal.md`, Stand 2026-09-03, ~45 %)
- **Referenz:** Lightroom „Assisted Culling" (Vorschlags-Sichtung); manuelles Sichten (Sterne/Flags/Labels) ist MVP
- **Status:** ENTSCHEID (SOLL); Implementierung folgt als eigene Tasks (s. §7)

## 1. Entscheidung (kurz)

KI-Culling in LuminaRust ist ausschließlich eine **Vorschlags-Sichtung
(Assisted Culling)**: Das System darf pro Quellbild eine begründete
Empfehlung (`keep` / `review` / `reject-kandidat`) plus Scores berechnen und
anzeigen. Es darf **niemals** selbständig bewerten, flaggen, labeln,
löschen, verschieben oder exportieren. Jede Übernahme in Sterne/Flags/Labels
ist eine **explizite Benutzeraktion** pro virtueller Kopie.

## 2. Scope: Assisted-Culling-Automatik (Vorschlags-Sichtung)

- **Eingabe:** ein Quellbild (Original-Decode, EXIF-Orientierung beachtet);
  Serien-/Gruppenkontext (Burst/Duplikate) nur als *vergleichende*
  Empfehlung innerhalb einer vom Benutzer gewählten Auswahl, nie
  ordnerübergreifend automatisch.
- **Ausgabe pro Quellbild:** genau ein Vorschlagsdatensatz:
  `proposal {keep|review|reject-kandidat}`, `score 0..=1` (deterministisch,
  dokumentierte Skala), `reasons[]` (maschinenlesbare Codes, z. B.
  `sharpness_low`, `motion_blur_suspect`, `exposure_clipped`,
  `noise_high_iso`, `duplicate_group`), `created_at`, Modell-/Analyse-
  Identität (s. §4).
- **Nicht-destruktiv:** Der Vorschlag ändert weder Original noch Rezept noch
  virtuelle Kopien. Er ist ein aus Original + Analyse ableitbares Artefakt
  (Produktprinzip: ableitbar, reproduzierbar, sichtbar veraltend).
- **Kein stiller Fallback:** Fehlt das Modell/die Analyse oder ist der
  Datensatz `stale`, zeigt die UI „kein Vorschlag verfügbar / veraltet"
  statt einer erfundenen Empfehlung. Eine automatische Neuberechnung darf
  nicht die einzige Option sein (Analogie: AI-Masken-Invalidierung).

## 3. Abgrenzung: manuelles Sichten bleibt MVP (kein KI-Culling im MVP)

Folgendes ist bereits MVP und wird durch 2.5 **nicht** verändert:

- Sterne `1–5` / `0` (Reset), Pick/Reject/Unflag `P`/`X`/`U`, Farb-Labels
  `6–9` — jeweils **pro aktiver virtueller Kopie** (`feature/platform/cli-gui-wasm.md`, F-100-Tabelle).
- Grid-Badges (Sterne der Standardkopie + Pick-/Reject-Markierung),
  Filterleiste `\` (Name/`rating:`/`flag:`/`label:`), `G`/`E`/`C`/`N`-Ansichten,
  `Cmd/Ctrl+G`-Stack-Proxy.
- Quick Develop (Exposure/Contrast/Highlights/Shadows über Save/Render-Pfad).

Normative Abgrenzungsregeln für 2.5:

1. Der Culling-Vorschlag schreibt **nie** `rating`, `flag`, `color_label`
   oder ein anderes Rezeptfeld — weder direkt noch als Seiteneffekt.
2. Die Übernahme („Vorschlag anwenden") ist ein eigener, expliziter Befehl
   pro Bild/Auswahl über den normalen Save/Render-Pfad (je eigenes Sidecar,
   Fehler isoliert laut, wie Sync/Match). Stapelübernahme nur auf explizite
   Auswahl, nie automatisch auf Ordner/Katalog.
3. Es gibt kein automatisches Löschen, Verschieben, Stapeln oder
   Ausblenden auf Basis des Vorschlags.
4. Tastatur: `1–5`/`0`/`P`/`X`/`U` bleiben rein manuell. Ein etwaiger
   „Vorschlag übernehmen"-Shortcut wird erst im Implementierungstask
   vergeben (Kollisionscheck gegen F-100-Tabelle Pflicht) und hier **nicht**
   vorweggenommen.

## 4. Modell-/Capability-Scope

- **Stufe 1 (2.5-Basis, entschieden): heuristische Bildqualitätsanalyse in
  `lumina-core`, ohne ONNX, ohne Gewichte, deterministisch.** Signale:
  Schärfe/Blur-Heuristik, Clipping-/Belichtungsanalyse (bestehende
  `analyze_tone`-/Histogramm-Pfade wiederverwenden, kein Zweit-Algorithmus),
  Rausch-Heuristik, Duplikat-/Serienähnlichkeit (Hash-/Histogramm-Vergleich
  nur innerhalb expliziter Auswahl). Keine Gesichts-/Augen-Signale in Stufe 1.
- **Stufe 2 (optional, offengehalten): ONNX-Qualitäts-/Ästhetikmodell über
  `lumina-onnx`.** Erst nach F-078-Lizenz-/Modellprüfung
  (`feature/quality/fixtures-licensing.md`), Modell-Hash-Pinning
  (`sha256:<hex>`, Analogie `ModelManifest`/`ModelIdentity` in
  `feature/product/ai-masks.md`), Inferenzauflösung + Vorverarbeitung als
  Teil der Identität. Kein spontaner Modell-Download, keine committeten
  Gewichte ohne Lizenz, keine Tests mit Netzwerk.
- **Explizit ausgenommen:** Gesichts-/Augen-offen-Erkennung hängt von
  LRPAR-G12-FACE-20 (Release 2.0) ab und ist **kein** Bestandteil dieses
  Entscheids; falls G-12 vorliegt, kann ein Folge-Slice ein
  `eyes_closed`-Signal ergänzen — kein implizites Mitliefern.
- **Capability (nativ-only):** Culling-Analyse ist eine native Capability
  (CLI/Desktop), dokumentiert in `feature/platform/capability-matrix.md`.
  Heuristik (Stufe 1) läuft überall nativ; ONNX (Stufe 2) nur bei
  verfügbarem `onnx-rt`-Backend — Fehlen ist harter, sichtbarer Zustand
  (`RuntimeDisabled` → „kein Vorschlag"), nie stiller Heuristik-Ersatz und
  nie stille ONNX-Ersetzung der Heuristik. Lokal vs. Cloud sind getrennte
  Capabilities; **Cloud ist nicht geplant** (nur mit expliziter neuer
  Capability-Entscheidung).
- **Kein zweiter Analysepfad für dieselbe Messung:** Soweit Signale aus
  bestehenden Messungen ableitbar sind (Histogramm/Clipping), werden diese
  wiederverwendet.

## 5. Persistenz-Scope (Sidecar-first)

- Der Culling-Vorschlag wird **auf Quellbild-Ebene** (nicht pro virtueller
  Kopie) persistiert — geteilte Analyse, wie geteilte Matte; Rating/Flag/
  Label bleiben pro Kopie. Neue, versionierte Sidecar-Sektion (z. B.
  `culling` mit `proposal`, `score`, `reasons[]`, `analyzer`-, Modell-,
  Decode-Identität, `created_at`, `status`), Schema-Entscheid + Migration
  im Implementierungstask. Keine absoluten Pfade, atomarer Write.
- **Identität/Veraltung analog AI-Masken:** gültig nur bei Übereinstimmung
  von Quell-Content-Hash, Decode-/Geometrieparametern, Analyzer-/Modellname
  + Version + Hash, Inferenz-/Analyseauflösung und Vorverarbeitung.
  Abweichung → `stale` (sichtbar), keine stille Neuberechnung als einzige
  Option. Artefakt-Prüfsumme, falls Scores in `.lumina.zdata` ausgelagert
  werden (kleine Scores bevorzugt inline im JSON; keine
  Float-Arrays im JSON über kleine Score-Structs hinaus).
- Standardkopie-Regel unberührt; ein Sidecar ohne `culling`-Sektion ist
  gültig („kein Vorschlag") und kein Fehler.
- Optionale zentrale Indizierung darf Culling-Scores nur als
  wiederaufbaubaren Index spiegeln (Sidecar bleibt Quelle der Wahrheit).

## 6. UI-Einordnung (Library, F-100)

- **Ausschließlich Library-Modul.** Keine Develop-/Export-Änderungen, keine
  Pipeline-Stufe, keine Render-Änderung.
- Darstellung: Vorschlags-Badge in der Grid-Zelle (visuell klar getrennt
  vom manuellen Rating-/Flag-Badge; keine Verwechslung mit Sternen),
  Detail im Hover/Info, Filter-/Sortier-Option in der `\`-Leiste
  (z. B. `cull:keep/review/reject/none/stale`) über bereits gescannte
  Sidecar-Daten (kein Index-Zwang). Compare (`C`) / Survey (`N`) zeigen
  den Vorschlag lesend an.
- Session-Display-State (Filter, Ein-/Ausblenden der Vorschläge) wird nicht
  persistiert; der Vorschlag selbst schon (§5). Rezept/Sidecar werden durch
  reines Betrachten nie mutiert (Analogie: Tool-Overlays/Pins in F-100).
- Barrierefreiheit/Testbarkeit: Vorschlag als testbarer Modell-Getter
  (nicht nur Painter-Content), analog `visible_edit_pins()`-Präzedenz.

## 7. Folge-Tasks (Implementierung, nicht Teil dieses Dokuments)

1. **Schema-Slice:** `culling`-Sektion in `lumina-sidecar` (Typen,
   Validierung, Roundtrip, Migration, Atomic-Write, Stale-Regeln) + Tests.
2. **Heuristik-Slice:** deterministische Stufe-1-Analyse in `lumina-core`
   (ggf. neuer nicht-GUI-Modulpfad, keine GUI-Bildlogik) + Unit-/Property-
   Tests (Score-Range `0..=1`, Monotonie, Clipping) + Fixture-Genauigkeit
   (Precision/Recall auf lizenzgeeigneten Fixtures, keine Netzwerk-, keine
   Original-Mutation).
3. **CLI-Slice:** `cull`-/`inspect`-Anbindung (explizite Analyse, Exit-Codes
   nach CLI-Tabelle `0`/`1`/`2`/`3`, `--json`), Batch-isolierte Fehler.
4. **GUI-Slice:** Library-Badges + Filter + explizite Übernahme-Aktion,
   headless GUI-Tests (`cargo test -p lumina-gui`, kein GPU-Zwang);
   visuelle Änderung zusätzlich kittest-Golden/PSNR/Histogramm.
5. **Optional Stufe-2-Slice:** ONNX-Modell-Evaluierung + F-078-Lizenzentscheid
   + Capability-Matrix-Eintrag + Invalidierungstests (Hit/Miss/Stale,
   fehlendes Modell laut).
6. **Perf-Slice:** Methodik nach F-074 (Baseline-/Budget-Stores,
   `report`/`warn`/`gate`); Ordner-Scan mit Vorschlägen bleibt interaktiv.

## 8. Abnahme dieses Entscheids

- **Abnahme = dieses Dokument (Entscheid) + ein angelegter
  Folge-Implementierungstask** in `Agents.todo.md` (Verweis auf dieses
  Dokument, Release 2.5). Kein Code, keine Schemaänderung, keine
  Shortcut-Vergabe in diesem Schritt.
- **Nicht-Ziele (nie):** Karten-Modul/GPS, Veröffentlichungsdienste
  (Releaseplan `nie`); automatisches Löschen/Verschieben/Bewerten durch KI;
  Cloud-Inferenz ohne neuen Capability-Entscheid; XMP als autoritative
  Quelle.

## 9. Risiken / offene Punkte für die Implementierung

- Serien-/Duplikat-Erkennung ohne Index kann bei großen Ordnern teuer
  werden → Scope auf explizite Auswahl begrenzen, Budgets (F-074) beachten.
- Verwechslungsgefahr Vorschlags-Badge vs. manuelles Rating → Design muss
  Trennung belegen (Kontrast-/Badge-Nacharbeit-Präzedenz beachten).
- Augen-/Gesichts-Signale erst nach G-12 (2.0); Stufe-1-Vorschläge sind
  bewusst qualitäts- nicht inhaltsbasiert — Erwartungsmanagement in der UI
  (Begründungscodes statt Black-Box-Score).
