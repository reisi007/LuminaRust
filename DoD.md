# Definition of Done (DoD) — normativ

Diese Datei ist die verbindliche Prüfliste für „fertig" in LuminaRust. Sie wird
von `Agents.md` (§ Definition of Done, § Verifizierung) referenziert und ist
für Implementierungs- und Verifizierungs-Agenten gleichermaßen bindend.
Begründung: Manueller GUI-Test 2026-09-04 (Histogramm-Attrappe, Slider ohne
Sidecar-Save) fiel durch alle bestehenden Gates — die Regeln unten schließen
genau diese Lücken.

## 1. Verhalten testen, nicht Zustand

- Ein Test, der nur In-Memory-Zustand assertet (z. B. Rezept-Map nach
  `set_adjustment`), beweist **keine** User-Story.
- Jede User-Edit braucht einen End-to-End-Anker: **Edit → Commit/Debounce →
  Sidecar-Datei → Reload → Wert wiederhergestellt**. Fehlt ein Glied, ist die
  Story ungetestet — unabhängig davon, wie viele Unit-Tests grün sind.
- Pixel-Tests (kittest Golden/PSNR) ersetzen keine Persistenz-Tests: Ein
  fehlendes Sidecar ist auf keinem Screenshot sichtbar.

## 2. Kein zeitbasierter Pfad ohne Test-Hook

- Debounce-/Update-Loop-/Timer-Pfade (`pending_full_render`, 150-ms-Fenster,
  `ctx.input time`) müssen headless treibbar sein (simulierte Zeit oder
  direkter Commit-Aufruf). Was headless nicht auslösbar ist, gilt als
  **ungetestet** — auch bei 100 % Unit-Coverage daneben.
- Der Verifizierer benennt pro Task den zeitbasierten Pfad und den Test, der
  ihn treibt. „Wird im Loop erledigt" ohne Test-Anker = NICHT BESTANDEN.

## 3. Klassen vollständig prüfen, keine Stichproben

- Gehört eine Änderung einer Interaktionsklasse an (alle Slider, alle
  Shortcuts, alle Zoomstufen), werden **alle** Mitglieder klassifiziert
  (z. B. direkte Feldzuweisung vs. `set_*`/`mark_dirty` vs. `save_sidecar`).
  Eine Stichprobe (ein Slider grün ⇒ alle gut) ist kein Nachweis.
- Neue Enum-Varianten/Modi (z. B. Zoomstufen) brauchen je Variante einen
  Mapping-Test (Eingabe → `preview_zoom`/`roi_from_zoom`).

## 4. Log-Level-Regel

- User-sichtbare Aktionen (Edit, Save, Konflikt, Fehler) loggen mindestens
  `info!`; `trace!` nur für Hot-Path-Details. Default-Level ist INFO —
  `trace!`-only bedeutet „unsichtbar".
- Der Verifizierer prüft das Level jeder neuen User-Aktion (Code-Review +
  Log-Ausschnitt im Bericht).

## 5. Spez-Satz → Test-Anker

- Jede normative Doku-Aussage („Regler ändern Rezept und schreiben Sidecar")
  braucht einen benannten Test. Der Verifizierungsbericht mappt
  **Spez-Aussage → Testname**; ungemappte Aussagen = NICHT BESTANDEN.
- „Per Inspektion verifiziert" ist keine Verifizierung.

## 6. Manueller Befund → Regressionstest + Regel

- Jeder manuelle Test-Befund erzeugt (a) einen automatischen Regressionstest
  und (b) falls eine Regel fehlte, einen DoD-Eintrag hier (dieser Abschnitt
  wurde so geboren).
- **Manuelle GUI-Tests starten immer mit Trace-Level:** `RUST_LOG=trace
  cargo run -p lumina-gui` (o. ä.), damit Slider-/Debounce-/Render-Pfade im Log
  sichtbar sind (`trace!` ist unter INFO unsichtbar). Der Befundbericht nennt
  den Log-Ausschnitt.
- **KI-Validierungs-Loop (GUI, verpflichtend nach jedem GUI-Batch):**
  1. `cargo test -p lumina-gui --test kittest_snapshots -- --ignored` erzeugt
     aktuelle Frames (Goldens + `.diff.png`/`.new.png` bei Abweichung).
  2. Der Build-Agent legt alle neuen/geänderten Snapshots einem
     Vision-Agenten (`vision-technical`, max. 10 Bilder) vor mit der Frage nach
     Layout-Bugs (Overlap, abgeschnittene Panels, fehlende/falsche Elemente,
     Platzierung, Zoom/Fit-Stimmigkeit gegen Navigator).
  3. Jeder Vision-Befund wird als Todo-Task (Block A) angelegt oder widerlegt
     begründet verworfen — kein Befund versandet.
  4. Erst danach startet die unabhängige Code-Verifizierung. Vision-Befunde
     laufen wie Test-Failures: Sie blockieren BESTANDEN.
- `F-103-N6` und jeder folgende manuelle Test gelten erst als abgeschlossen,
  wenn alle Befunde einen automatischen Test-Anker haben.

## 7. BESTANDEN-Checkliste (Verifizierungsbericht)

`BESTANDEN` darf nur stehen, wenn alle Punkte mit Beleg (Testname/Kommando)
beantwortet sind:

1. Welche End-to-End-Kette (Edit→Commit→Datei→Reload) deckt die Story ab?
2. Welcher zeitbasierte Pfad existiert, und welcher Test treibt ihn?
3. Welche Klassenmitglieder wurden geprüft (vollständige Liste)?
4. Welches Log-Level hat jede neue User-Aktion (Beleg)?
5. Welche Spez-Aussagen wurden auf welche Tests gemappt?
6. Gates: `cargo test`, `clippy -D warnings`, `fmt --check` — Kommandos +
   Ergebnis im Bericht.
