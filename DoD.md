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
7. Dateigrößen-Regel: `sh scripts/check_file_sizes.sh` grün, neue Logik in
   neuen/kohärenten Dateien, keine Kompensations-Löschung (Diff-Beleg)?

## 8. Dateigröße / Anti-Gaming (User-Vorgabe 2026-09-17)

- Eine `.rs`-Datei mit mehr als 500 Zeilen darf nicht wachsen (CI-Ratchet
  gegen `scripts/file_size_baseline.txt`). Neue Logik gehört in neue oder
  passende kleine Dateien (Umdesign-Pflicht); Kleinstverdrahtung an bestehenden
  Aufrufstellen darf bleiben.
- Das Löschen von Kommentaren, Doku oder Tests zur Kompensation von Wachstum
  gilt als Regel-Umgehung (Goodhart) und wird abgelehnt: Die Verifizierung
  prüft den Diff (Logik-Wachstum bei gleichzeitigem Kommentar-Schwund =
  Befund) und weist das Ergebnis in der BESTANDEN-Checkliste (§7, Punkt 7)
  explizit aus.

## 9. Behauptung braucht Beleg; Vermutung ist als solche zu kennzeichnen

- Jede **normative oder factual** Aussage über Verhalten, Abdeckung, Ursache oder
  Anzahl trägt ihren Beleg **an derselben Stelle**: der Name der Mutation, die
  rot wurde, das Kommando mit Ergebnis, oder die nachgezaehlte Zahl. „Sollte",
  „wird behandelt", „verifiziert" ohne Beleg gelten als **ungeprüft**.
- Eine Aussage, die eine Messung widerlegt hat, wird **an der Fundstelle
  zurückgenommen** — nicht stillschweigend ersetzt. Der Widerlegungsgrund steht
  im Text, damit sie beim nächsten Anfassen nicht wiederholt wird.
- Eine Zahl, die man nicht nachzählen kann, wird **nicht hingesrieben**. Statt
  „27 offene Tasks" gehört der Zählweg daneben (`grep -c '^- \[ \]' Agents.todo.md`).
  Tabellensummen und Klassenbilanzen werden **maschinell ausgezählt** und die
  Herleitung genannt.
- Eine Coverage-Angabe („Control X ist angeklickt") verweist auf den Test **und
  die Geste** — nicht auf den Testnamen allein. Existiert der Test, führt er die
  Geste aber nicht aus, ist die Angabe falsch.
- Ein aus einem Verifikationsbericht übernommener Befund wird **vorher selbst
  gemessen**, bevor ein Task daraus entsteht. Wird er widerlegt, wird er
  **gestrichen**, nicht verfeinert. Ein Phantom-Task bindet Arbeitszeit auf
  etwas, das es nicht gibt, und ist schlimmer als gar keiner.
- Ein Task wird nicht durch die Summe seiner Einzelfixings geschlossen,
  sondern durch ein Urteil über ihn als Ganzes.

## 10. Tests, die nicht scheitern können

- **Eine Abdeckungsaussage ohne Mutation am Produktionspfad ist unbelegt.**
  Mindestens eine Mutation, die die Aussage widerlegt, wird angewandt,
  ausgeführt und mit Fehlermeldung berichtet. Bleibt die Suite grün, ist die
  Aussage falsch — nicht der Test „grün".
- **Keine Umgebungsannahme im Test.** Ein Test darf nicht darauf setzen, dass
  der Prozess ein bestimmter Benutzer ist, dass Dateirechte greifen, dass ein
  Werkzeug existiert oder dass ein Zeitstempel feiner ist als der Abstand
  zweier Operationen. `chmod 000` als „unlesbar" gilt für einen unprivilegierten
  Prozess, **nicht** für einen root in einem Container-Runner. Was nicht
  umgebungsunabhängig ausdrückbar ist, wird über den **Effekt** geprüft (die
  Folge, nicht das Flag).
- **Keine selbstbezügliche Erwartung.** Ein Test, der seine Erwartung aus
  derselben Funktion ableitet, die er prüft, ist keine Prüfung. Die Erwartung
  ist ein **Literal** oder ein committiertes, unabhängig nachgeprüftes
  Artefakt (z. B. ein PNG mit von Hand verifiziertem IHDR).
- **Keine Identität über Adressen.** `as *const _`, Zeigervergleich oder ein
  `Debug`-Rendering beweisen im Debug-Build **keinen** Zustand über Aufrufgrenzen:
  ein frisch erzeugtes Objekt kann denselben Stack-Slot belegen. Geprüft wird
  **beobachtbarer Zustand**, der die Aufrufe überlebt.
- **Keine zeitabhängigen Assertions**, deren Auflösung feiner sein müsste als
  der Abstand zweier Operationen. Lässt sich die Eigenschaft nicht stabil
  prüfen, wird sie als **nicht getestet** dokumentiert statt behauptet.
- Ein **Clausel-Invariant**, die kein Test erzwingen kann, wird als
  „durch Begründung getragen, nicht durch einen Test" ausgewiesen. Erfundene
  Tests, die ihn nur symbolisch berühren, sind schlimmer als die ehrliche
  Kennzeichnung.

## 11. Kein Fix ohne Reproduktion

- Ein als Defekt gemeldeter Pfad wird **zuerst reproduziert** — rot vor dem
  Fix, mit Ausgabe. Ist er nicht reproduzierbar, wird die **Ursachenbehauptung
  zurückgenommen**, nicht die vermutete Stelle geändert.
- Eine Ursache gilt als belegt, wenn sie **mechanisch ausgeschlossen** oder
  **gemessen** ist. „Verdächtig", „wahrscheinlich", „sollte" ist kein Befund und
  geht nicht in eine Fix-Begründung ein. Ein Debounce, der einen Wert nur
  verzögern und nie abbrechen kann, kann keinen Save verlieren.
- Findet die Reproduktion einen **anderen** Defekt, wird dieser als eigener Task
  mit eigener Reproduktion geführt und der ursprüngliche **ausdrücklich
  zurückgenommen**. Zwei Befunde werden nicht zu einer Geschichte vermischt.
- Eine Testregression, die **im Betrieb** auftritt, wird nicht wegoptimiert,
  indem man die Annahme im Test durch eine Formulierung in der Doku ersetzt.

## 12. Ein Gate, das nicht lief, ist kein Gate

- Vor jeder Fertigmeldung wird geprüft, **ob die CI auf dem geänderten Stand
  gelaufen ist** — nicht, ob die lokale Suite grün ist.
- Löst der `push`-Trigger eines Feature-Branches **keine** CI aus (häufig:
  Trigger nur auf `main`), ist das eine **Lücke im Nachweis** und wird
  berichtet, nicht übergangen. Am Ende gilt: es gibt ein CI-Ergebnis, oder ein
  ausdrücklich benanntes Restrisiko mit Begründung.
- Ein Test, der **lokal grün und in CI rot** war, ist ein Befund und kein Flake.
  Die häufigste Ursache ist eine Umgebungsannahme (§10), die nächste ein
  Timing-Anker (§2).

## 13. Messung schlägt Bericht

- Verifikationsberichte sind **Messungen, keine Wahrheit**. Jede Angabe daraus,
  die einen Task, eine Codeänderung oder eine Fertigmeldung auslöst, wird
  **vorher selbst gemessen**. Ein Agent, der zweimal danebenlag, macht den
  dritten Bericht nicht richtiger.
- **`cp -p` nach einem Restore erhält die mtime.** Cargo nutzt daraufhin
  möglicherweise ein veraltetes Artefakt, und ein Test schlägt mit der Signatur
  der Mutation fehl, obwohl der Baum sauber ist. Nach jedem Restore
  `cargo clean -p <crate>` vor der nächsten Messung; Restore per
  `diff <(git show HEAD:<datei>) <datei>` prüfen, **nicht** per `git status`.
- **Shell-Quoting ist eine Fehlerquelle.** `cargo check -p x $f --all-targets`
  mit `f="--features lensfun"` reicht das Flag als **ein** Argument durch
  (`unexpected argument`). Sieht ein Build nach einem Build-Fehler aus, ist es
  das nicht. Konfigurationen einzeln aufrufen.
- Ein Subagent, der mitten in der Arbeit abbricht, kann eine Mutation
  **angewandt im Baum** hinterlassen. Nach jedem Abbruch wird der Arbeitsbaum
  gegen `HEAD` geprüft, nicht nur auf neue Dateien.
