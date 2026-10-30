# LuminaRust Agentenregeln

Dieses Dokument enthält die verbindlichen Arbeitsregeln für alle Agenten im
Projekt. `Agents.todo.md` ist der ausführbare Arbeitsplan, `feature/README.md`
der Einstiegspunkt zum normativen Feature-SOLL, `DoD.md` die verbindliche
Definition of Done mit den normativen Verifizierungsregeln. Die verlinkten
Dokumente unter `feature/` müssen vor der Implementierung eines Features gelesen
und bei Änderungen zuerst aktualisiert werden.

## Nur Regeln, keine Entscheidungen

**Nur Regeln, keine Entscheidungen:** Diese Datei enthält ausschließlich Regeln
— Anweisungen, die für jede Arbeit in diesem Repo gelten. Dauerhafte
Entscheidungen, Begründungen, Messwerte und Historie gehören nicht hierher,
sondern in `feature/`/`docs/` bzw. die Projekt-Doku; offene Punkte nach
`Agents.todo.md`; Erledigtes in die Git-Historie.

## Inhaltsverzeichnis

- [Nur Regeln, keine Entscheidungen](#nur-regeln-keine-entscheidungen)
- [Produktprinzipien](#produktprinzipien)
- [Persistenz und Sidecars](#persistenz-und-sidecars)
- [Architekturgrenzen](#architekturgrenzen)
- [Rollen und Delegation](#rollen-und-delegation)
- [Branch- und Merge-Konvention](#branch--merge-konvention)
- [Entscheidungen an den Projekteigentümer](#entscheidungen-an-den-projekteigentümer)
- [Verbindlicher Arbeitsablauf](#verbindlicher-arbeitsablauf)
- [Verifizierung und Tests](#verifizierung-und-tests)
- [Definition of Done](#definition-of-done) (normativ: [`DoD.md`](DoD.md))
- [Dokumentations- und Todo-Regeln](#dokumentations--todo-regeln)
- [Änderungsregeln](#änderungsregeln)

## Produktprinzipien

Die Produktprinzipien und Invarianten sind **nicht vollständig** in dieser
Datei festgelegt. Ihre normative Quelle ist [`feature/README.md`](feature/README.md):
§„Leitbild" beschreibt das Zielbild, §„Invarianten" die verbindlichen
Zusicherungen; die Detaildokumente stehen in der Feature-Matrix derselben
Datei. Diese Datei hält daraus nur die Regeln, die für jede Arbeit gelten:

- Das Original wird nie verändert, verschoben, überschrieben oder durch einen
  Export ersetzt — ein Export ist immer ein zusätzliches Ziel.
- Eine gültige persistierte AI-Maske wird wiederverwendet; eine erneute
  Inferenz darf nicht ungefragt erfolgen (Gültigkeit, Veraltung und
  Neuberechnung siehe „Persistenz und Sidecars" → „AI-Masken").

## Persistenz und Sidecars

### Quelle der Wahrheit

- Das Sidecar ist die primäre und vollständige Quelle für Lumina-Bearbeitungen.
- Die empfohlene Datei liegt direkt neben dem Original und heißt
  `<original-dateiname>.lumina.json`, zum Beispiel
  `IMG_0001.ARW.lumina.json`.
- Das Original bleibt unverändert und kann ohne Sidecar weiterhin als RAW-Datei
  gelesen werden.
- Eine zentrale Datenbank ist nicht erforderlich, damit Bearbeitungen korrekt
  gespeichert oder wiederhergestellt werden können.
- Eine optionale SQLite-Datenbank darf nur einen wiederaufbaubaren Index,
  Suchdaten, Jobstatus und Cache-Metadaten enthalten. Sie ist niemals die
  einzige Quelle für Rezept, virtuelle Kopien oder Maskendefinitionen.
- XMP ist in v1 weder Lese- noch Sidecar-Format, und ein späterer XMP-Adapter
  darf Lumina-spezifische Daten niemals stillschweigend zur autoritativen
  Quelle machen. Das Opt-in-Schreiben von IPTC IIM + XMP in **neu erzeugte
  Exportdateien** (JPEG) ist erlaubt (siehe `feature/product/iptc-metadata.md`).

### Sidecar-Struktur

- Das JSON-Sidecar enthält Schema-Version, Quellidentität, virtuelle Kopien,
  Rezeptdaten, Maskenmetadaten und Verweise auf binäre Maskenartefakte.
- Große Masken werden nicht als unkomprimierte Float-Arrays in das JSON
  geschrieben. Sie werden im binären Sidecar
  `<original-dateiname>.lumina.zdata` gespeichert.
- Jeder binäre Artefaktverweis enthält mindestens relativen Pfad, Format,
  Prüfsumme, Auflösung, Kanaltyp und Datenversion.
- Relative Verweise müssen beim Verschieben eines gesamten Sidecar-Bundles
  gültig bleiben. Absolute Pfade sind in persistenten Rezeptdaten verboten.
- Sidecar- und Maskenänderungen werden atomar geschrieben. Unvollständige
  temporäre Dateien dürfen nach einem Abbruch nicht als gültig erkannt werden.

### Virtuelle Kopien

- Eine virtuelle Kopie ist eine eigenständige Bearbeitungsvariante desselben
  Originals und benötigt keine Kopie der RAW-Pixel.
- Jede virtuelle Kopie besitzt eine stabile, innerhalb des Sidecars eindeutige
  ID, einen Anzeigenamen, ein eigenes Rezept und einen eigenen Aktivierungs-
  beziehungsweise Exportstatus.
- Ein Sidecar enthält mindestens eine Standardkopie. Die Standardkopie darf
  nicht stillschweigend gelöscht werden.
- Maskeninferenz und gespeicherte Matte können auf Quellbild-Ebene geteilt
  werden. Masken-Layer, Invertierung und lokale Anpassungen gehören zur
  jeweiligen virtuellen Kopie.
- Virtuelle Kopien dürfen nicht über ihre Position im JSON-Array identifiziert
  werden. IDs bleiben bei Umbenennung und Neuordnung stabil.
- Eine virtuelle Kopie darf nicht durch ein zweites Original-Sidecar dupliziert
  werden, ohne dass die Beziehung ausdrücklich dokumentiert ist.
- Vererbungen zwischen virtuellen Kopien sind bis zu einer ausdrücklichen
  Schemaentscheidung nicht implizit. Eine Kopie besitzt im ersten Zielumfang
  ein vollständiges, selbständig auswertbares Rezept.

### AI-Masken

Eine persistierte AI-Maske muss mindestens folgende Identität tragen:

- Quell-Content-Hash und relevante Decode-/Geometrieparameter
- Modellname, Modellversion und Modell-Hash
- Inferenzauflösung, Vorverarbeitung und Nachskalierungsverfahren
- Koordinatensystem und Ausrichtung des Quellbildes
- Matte-Format, Auflösung, Kanäle und Prüfsumme
- Erstellungszeitpunkt, Status und optionaler Fehlertext
- Bearbeitungsparameter wie Invertierung, Feathering und Blur

Eine Maske ist gültig, wenn Quelle, Decode-Kontext, Modellkontext und Artefakt-
Prüfsumme übereinstimmen. Bei einer Abweichung wird sie als veraltet markiert;
eine automatische Neuberechnung darf nicht die einzige Option sein.

## Architekturgrenzen

- `lumina-core` enthält plattformneutrale Domänen- und Renderlogik. Im
  Produktionscode ist es dateisystemfrei und darf keine GUI-, native ONNX- oder
  native RAW-Abhängigkeiten voraussetzen.
- Der native Ordner-Cache-/Preview-Disk-Tier liegt im Crate `lumina-cache`,
  dem Adapter zwischen Core und den Frontends. `lumina-cache` darf `std::fs`
  verwenden; Test-only-Vorkommen bleiben in Core. Geprüft über
  `scripts/check_core_prod_fs_free.sh` (Grenze: textueller Scan, kein
  `std as x`-Alias).
- Ein verpflichtendes `lumina-sidecar`-Modul soll Schema, Validierung,
  Roundtrip-Serialisierung, Migrationen und atomare Sidecar-Operationen bündeln.
- `lumina-raw` kapselt RAW-Decoder, Demosaicing, EXIF und Farbprofile.
- `lumina-onnx` kapselt native Inferenz, Modellverwaltung und Maskenartefakte.
- `lumina-cli` orchestriert Import, Analyse, Render, Export und Batch-Jobs,
  enthält aber keine eigene Bildverarbeitungslogik.
- `lumina-gui` enthält Darstellung, Interaktion und Jobsteuerung, aber keine
  zweite Implementierung der Renderpipeline.
- Ein optionaler Index-/Katalogadapter darf später als eigenes Modul ergänzt
  werden. Er muss aus Sidecars vollständig neu aufgebaut werden können.
- Native Abhängigkeiten werden über eine dokumentierte Capability-Matrix
  getrennt behandelt (WASM/Browser sind kein Ziel).
- Jede Pipeline besitzt eine explizite Version. Rezeptversion und
  Pipelineversion werden getrennt migriert und validiert.

## Rollen und Delegation

### Build-Agent

Der Build-Agent ist der verantwortliche Orchestrator. Der Build-/Verify-Flow
steht im Skill `build-verify` (agents-skills); der Skill wird über seinen
Trigger in die Session geladen. Den verbindlichen Rahmen setzt das Projekt-
`AGENTS.md` (dieses Dokument), das der Build-Agent vor jeder Aufgabe liest;
der repo-spezifische Teil steht nur hier — was in **diesem** Repo wann grün
sein muss (siehe „Verifizierung und Tests" und [`DoD.md`](DoD.md)). Die
Minimalregeln des Flows stehen in `.agents/rules/build-verify.md`
(agents-skills) und werden von dort gelesen.

Er:

- liest vor jeder Aufgabe `AGENTS.md`, `Agents.todo.md`, `feature/README.md` und
  das betroffene Feature-Dokument;
- zerlegt Arbeit in kleine, unabhängig prüfbare Aufgaben und delegiert sie an
  passende Subagenten;
- verhindert parallele Änderungen an gemeinsam genutzten APIs und Schemas (siehe
  „Parallele Delegation und Konfliktvermeidung");
- übernimmt eine Aufgabe erst nach bestandener Verifizierung und aktualisiert
  danach Plan und Feature-Dokument.

### Parallele Delegation und Konfliktvermeidung

- **Ein Crate = ein schreibender Agent.** Jeder Implementierungs-Agent erhält
  exklusiven Schreibzugriff auf genau ein Crate (bzw. ein klar abgegrenztes
  Docs-/Modulverzeichnis). Dateien desselben Crates werden nicht auf mehrere
  parallele Agenten verteilt.
- **Gemeinsame APIs/Schemas bleiben seriell.** Ändert eine Aufgabe eine
  öffentliche Schnittstelle, die ein anderer Agent nutzt, wird sie in einer
  eigenen Welle vor den abhängenden Agenten umgesetzt; abhängige Agenten
  verwenden nur bereits existierende, stabile Signaturen.
- **Build-Parallelität ist durch den Cargo-Lock begrenzt.** Mehrere Agenten
  dürfen `cargo` zeitgleich aufrufen — Cargo serialisiert den Zugriff auf
  `target/` per Lock.
- **Frische Session pro Runde — Implementierer und Verifizierer.** Jeder
  Dispatch ist eine neue Session mit dem vollen Auftrag im Prompt; eine Session
  wird nie über Runden oder Aufgaben hinweg fortgesetzt. Einzige Ausnahme: eine
  **Fix-Runde in derselben Aufgabe**, deren zu korrigierende Runde die
  Session selbst implementiert hat — und auch dann entscheidet der
  Orchestrator bewusst. Der Grund: ein
  fortgesetzter Kontext trägt die Defekte der Runde in die nächste, wächst mit
  jeder Runde und macht aus der unabhängigen Prüfung eine Mitprüfung.
- **Kein Commit durch Subagenten.** Implementierungs-Agenten committen nicht;
  der Build-Agent committet (wie und wann: Skill `build-verify`).

### Direkte Änderungen des Build-Agenten

Bis 5 Dateien / ~80 Diff-Zeilen implementiert der Build-Agent selbst; darüber
delegiert er an einen Implementierungs-Agenten (frische Session, voller Auftrag).
Jede Änderung — auch die eigene — durchläuft Review und CI; was Verhalten,
Datenformat, Migration, Persistenz, Pipeline oder Sicherheit betrifft, wird
zusätzlich fachlich geprüft (Skill `build-verify`).

### Implementierungs-Agent

Der Implementierungs-Agent:

- arbeitet nur innerhalb des delegierten Umfangs;
- prüft zuerst bestehende APIs, Persistenz und Abhängigkeiten;
- ändert das SOLL-Dokument vor Code, wenn die Zielsemantik noch nicht passt;
- ergänzt Tests zusammen mit der Implementierung;
- meldet geänderte Dateien, ausgeführte Tests, Migrationen und offene Risiken.

### Verifizierungs-Agent

Der Verifikations-Lauf folgt dem Skill `build-verify`; hier steht nur der
repo-spezifische Teil. Er:

- prüft die **Substanz**: die tatsächliche Änderung gegen `feature/README.md`
  und das betroffene Feature-Dokument (Diff gegen den Auftrag), fachliche
  Korrektheit, Invarianten, Blast Radius, Fehlerfälle,
  Rückwärtskompatibilität und Persistenzfolgen — und übernimmt gemeldete
  Gate-Ergebnisse **mit zitierter Ausgabe**; die mechanischen Gates
  (u. a. `cargo fmt`, Clippy, Tests, Builds) führt der Implementierer bzw.
  Orchestrator aus. Vier Ausnahmen, in denen der Verifizierer selbst läuft
  (Skill `build-verify`, `references/verifier-prompt.md`): kein CI für dieses
  Repo oder Artefakt (bzw. der Stand wird nie gepusht); die Änderung berührt
  die Gates selbst (Tests, Konfiguration von Lint/Format — ein „grün" wäre
  zirkulär); der Bericht deckt die geänderte Fläche nicht ab (dann wenigstens
  die relevante Teilmenge); es ist **keine Ausgabe zitiert** — dann fordert er
  die Ausgabe nach, statt sie zu ersetzen (eine Behauptung ist kein Beleg);
- prüft ausdrücklich, ob die Tests die neue Funktion tatsächlich abdecken;
- prüft die Dateigrößen-Regel (Anti-Gaming) **am Diff**: neue Logik in
  neuen/kohärenten Dateien, kein Wachstum in >500-Zeilen-Dateien, keine
  Kommentar-/Doku-/Test-Löschung zur Kompensation — und übernimmt das
  Gate-Ergebnis `sh scripts/check_file_sizes.sh` aus dem Implementierer-Bericht,
  statt es selbst zu fahren (Ausnahmen wie oben);
- darf eine Aufgabe als **nicht bestanden** zurückweisen;
- liefert einen kurzen Prüfbericht mit bestanden/nicht bestanden, Befunden,
  Test-/Gate-Kommandos mit zitierter Ausgabe und verbleibenden Risiken
  (Verdict-Format und Schweregrade `critical`/`high`/`medium`/`low`: Skill
  `build-verify`).

## Branch- und Merge-Konvention

- **Es werden keine Pull Requests verwendet.** Der Build-Agent merged direkt
  nach `main` (`git checkout main && git merge <branch> && git push origin
  main`). Ein PR wird **nicht** eröffnet — auch nicht, um CI anzustoßen.
- **`main` ist nicht branch-protected**, es gibt also keine required checks.
  Die lokalen Gates des Build-Agents sind damit das **einzige** Gate vor dem
  Merge und werden vor jedem Merge vollständig gefahren: betroffene Suites,
  `fmt`, Clippy, Ratchet und `golden_ref.sh check` grün.
- **Nach dem Merge wird die CI beobachtet, nicht vorher.** `main` löst per
  `push` die CI aus; der Build-Agent prüft das Ergebnis und behebt einen roten
  Lauf **sofort**. Ein rotes `main` gilt als offener Befund, nicht als Flake.
- **Maßgeblich ist der Stand auf `main`.** Arbeit auf einem Feature-Branch ohne
  Merge zählt nicht als geliefert; der Abschlussbericht nennt den Merge-Commit
  und das CI-Ergebnis. Ein Task gilt erst mit dem Merge als erfüllt, die
  Verifikation bleibt davon unberührt.
- Vor dem Merge ist `origin/main` zu holen und bei vorhandenen Upstream-Commits
  zu mergen. Merge-Konflikte werden **nachgemessen** entschieden
  (Zeilenzahlen, Testzahlen), nicht nach Erinnerung.

## Entscheidungen an den Projekteigentümer

- **Entscheidungen werden nie nur im Chat getroffen.** Eine getroffene
  Entscheidung wird vor dem nächsten Arbeitsschritt schriftlich festgehalten:
  dauerhaft in `feature/`/`docs/` (Quelle der Wahrheit für Datenmodell,
  Event-Regeln, GUI-Verhalten, Persistenz und SOLL-Zielsemantik) und, solange
  offen, in `Agents.todo.md`. Entscheidungen, Begründungen und Messwerte stehen
  **nicht** in `AGENTS.md`.
- **Jede Entscheidung mit echten Wahlmöglichkeiten wird als interaktive Frage
  gestellt** (Werkzeug `question`), **niemals** als Frage im Fließtext. Echte
  Wahlmöglichkeiten sind z. B.: Alternative ja/nein, Datenmodell, Event-Form,
  Abbruch-vs-weiter, Task-Reihenfolge. Die **Empfehlung steht als erste
  Option**, die Varianten sind je **in einem Satz** begründet.
- **Der Build-Agent entscheidet nicht allein.** Er sammelt die Optionen, misst
  die Fakten und legt sie dem Eigentümer vor. Er entscheidet **nur** dann ohne
  Frage, wenn es keine Wahl gibt — Messungen, Formprüfungen, Pflichtschritte
  aus `DoD.md` und das Ausführen einer bereits im Repo festgeschriebenen
  Entscheidung. Ein Auftrag, der sich auf eine Zusage stützt, wird
  **ausgeführt, nicht neu verhandelt**.
- **Zurückgezogene Ansätze werden nicht nachträglich implementiert.** Wird ein
  Ansatz verworfen, wird die **geltende Regel** klar benannt und der alte Stand
  als zurückgezogen gekennzeichnet; der Negativrecord gehört nach
  `Agents.todo.md` (Abschnitt „Verworfen"). Eine Regel, die sich als falsch
  erweist, wird **ersetzt**, nicht daneben stehen gelassen.
- **Aufgabenreihenfolge: alle offenen Tasks werden abgearbeitet; der
  Build-Agent wählt die Reihenfolge.** Kein Punkt wird fallen gelassen. Die
  Wahl wird begründet und folgt drei Kriterien in dieser Rangfolge:
  1. **Gate-Gefährdung** — was einen Merge blockieren oder ein `main`-Rot
     verursachen kann (ein nicht-deterministisch rotes CI trifft **jeden**
     künftigen Merge);
  2. **Abhängigkeit** — was andere Tasks freigibt;
  3. **Hardware-Gate** — was ohne den manuellen Fahrplan (R5-LOG-1) nicht
     bearbeitbar ist, wird vorbereitet, nicht vorgezogen.
  Ein Task, der nur **Hardware oder einen Menschen** braucht, ist kein Grund
  anzuhalten: die übrigen Tasks werden abgearbeitet und der Fahrplan bleibt
  offen im Plan.

## Verbindlicher Arbeitsablauf

**Auf headless Maschinen (kein Display, kein GPU-Adapter, geteilter Build-Host)
gilt zusätzlich [`Agents.headless.md`](Agents.headless.md).** Dort stehen die
Regeln zum Umgang mit einem fehlenden Adapter, die zwei Fehlerklassen dieser
Umgebung und der reproduzierbare Verifikationslauf
(`target/verify_cpu_tasks.sh`).

- **Entscheidungen mit Wahlmöglichkeiten vorher als Frage stellen** (siehe
  Abschnitt „Entscheidungen an den Projekteigentümer") und die getroffene
  Entscheidung vor dem nächsten Arbeitsschritt schriftlich festhalten.

0. **Vor jedem Pull/Fetch/Merge/Rebase die lokale Arbeit committen.** Niemals
   in einen schmutzigen Arbeitsbaum hinein aktualisieren. Der Commit dient der
   Sicherung — er wird auch dann ausgeführt, wenn die Arbeit noch nicht
   verifiziert ist (der Verifikationsstand steht dann im Commit-Text und in
   `Agents.todo.md`). Reihenfolge: `git status` prüfen →
   `scripts/check_file_sizes.sh` + die betroffenen Gates → committen → holen →
   mergen. Nach dem Merge: die gelieferten Tests laufen lassen, das Ratchet
   erneut prüfen und den Merge-Stand im Abschlussbericht nennen.
1. `AGENTS.md`, `Agents.todo.md`, `feature/README.md`, das betroffene
   Feature-Dokument und relevante ADRs lesen.
2. Die betroffene Funktion anhand einer stabilen Feature-ID identifizieren.
3. Prüfen, ob die geplante Änderung dem SOLL widerspricht oder einen neuen
   Konflikt erzeugt.
4. Bei einem Konflikt zuerst `feature/README.md`, das betroffene
   Feature-Dokument und gegebenenfalls ein ADR aktualisieren. Ohne klare
   Zielentscheidung beginnt keine Implementierung.
5. Implementieren — bis 5 Dateien / ~80 Diff-Zeilen selbst, darüber ein
   Implementierungs-Agent mit frischer Session und vollem Auftrag (Skill
   `build-verify`, Schritt 2). Die Schwelle ist Kontext, nicht Kompetenz.
6. Genau einen Commit pro Aufgabe schreiben (Conventional, explizite Pfade,
   `Verify:`-Footer nach dem Review). Implementierungs-Agenten committen
   nicht.
7. Einen anderen Subagenten mit der Prüfung des **gesamten unpushed Ranges**
   beauftragen (`git diff origin/main...HEAD`), nicht nur des letzten Commits.
8. Bei Befunden korrigieren, `git commit --amend`, den Range erneut prüfen
   lassen. Erst nach `APPROVED` nach `main` mergen (kein PR, siehe
   „Branch- und Merge-Konvention") und die CI beobachten.
9. Erst nach bestandener Verifizierung die offene Aufgabe aus
   `Agents.todo.md` entfernen.
10. Das betroffene Feature-Dokument um den tatsächlich erreichten Status, neue
    bekannte Grenzen oder Folgekonflikte ergänzen.
11. Im Abschlussbericht Änderung, Tests, Verifizierungsbericht und offene
    Risiken nennen.

## Verifizierung und Tests

Der generische Build-/Verify-Flow steht im Skill `build-verify`; alles Folgende
ist der **repo-spezifische** Teil. Je nach Änderung sind mindestens diese
Prüfungen zu verwenden:

- Formatierung und Clippy für Rust-Code
- Feature-Matrix-Gate: `scripts/check_feature_matrix.sh` kompiliert je
  Workspace-Crate jede deklarierte Feature-Menge (Union `--all-features` +
  Einzelachse `--features <F>`), Feature-Listen aus den Manifesten; CI-Schritt
  in `rust-fast`. Compile-only, kein Adapter, kein Laufzeit-Test. Benannte
  Grenzen: `--all-features` beweist nicht, dass ein Feature *allein*
  funktioniert; `lumina-gui-harness` ist macOS-only und wird auf Linux als Stub
  kompiliert; der onnx-rt-Link (glibc 2.36 < 2.38) ist ein Host-Gate, das
  `cargo check` nicht trifft.
- **Lokale Gate-Stufen:** `sh scripts/gates.sh fast` (Sekunden, ohne Codegen:
  Formatter plus neun Textgates — Quelle: `sh scripts/gates.sh list`) und
  `sh scripts/gates.sh slow` (Minuten, CI-Mirror, Fail fast). Die schnelle Stufe
  meldet grün nur, wenn mindestens 10 Checks gestartet wurden (Floor:
  `scripts/gates.sh`, `MIN_CHECKS=10`); eine leere Auswahl ist ein Fehler, kein
  Pass. `sh scripts/gates.sh list` ist die Quelle der Wahrheit für die
  Tier-Tabelle; die Lücken zwischen den Stufen (GPU-Adapter, onnx-rt-Link,
  macOS-Golden-Re-Pin, manuelle GUI-Läufe) stehen mit Grund, Owner und
  Task-ID in `feature/quality/local-gates.md`.
- Unit-Tests für mathematische und serielle Logik
- Property-Tests für Wertebereiche, Monotonie und Clipping
- JSON-Roundtrip- und Schema-Migrationstests
- Sidecar-Recovery-, Atomic-Write- und Konflikttests
- Virtuelle-Kopien-Tests inklusive stabiler IDs und unabhängiger Rezepte
- Masken-Cache-Hit-, Miss- und Invalidierungstests
- Golden-Image-Tests mit dokumentierten Toleranzen
- CLI-End-to-End-Tests inklusive Exit-Codes
- GUI-Features: Jede sichtbare GUI-Funktion (Regler, Module, Shortcuts, Preview,
  Sidecar-Persistenz) wird mit automatischem headless GUI-Test (egui Context +
  LuminaApp, tempdir) abgedeckt; visuelle Änderungen zusätzlich via kittest
  Golden/PSNR/Histogram (byte-identisch/PSNR, kein stiller Fallback), kein
  manueller Test als einzige Absicherung. `cargo test -p lumina-gui` läuft ohne
  GPU grün.
- **Manuelle Akzeptanz-Runs (R5-LOG-1):** Jeder manuelle GUI-Run startet mit
  `RUST_LOG=trace` (Trace-Timings für Switch/Drag/Warmup sind sonst unsichtbar),
  genau EINER App-Instanz, Log-Redirect in
  `/tmp/lumina_manual_<datum>.log`. Der User steuert die GUI nach Fahrplan, der
  Build-Agent verwaltet und analysiert den Log.
- native Build-/Smoke-Tests (kein Browser-Build)
- Tests für fehlende Modelle, fehlende Maskenartefakte und veränderte Quellen
- Performance-Methodik gemäß `feature/quality/performance-benchmarks.md`
  (F-074): Benchmarks laufen gegen die committeten Baseline-/Budget-Stores
  (`scripts/perf/compare.mjs`, Modi report/warn/gate); eine Budget-Anpassung
  wegen bewusstem Feature-Wachstum wird im selben Commit wie das Feature
  begründet
- **GPU-Tests (lumina-gpu):** GitHub-Actions-Runner haben keinen GPU-Zugriff;
  die CI kann den GPU-Pfad nur kompilieren (`cargo check -p lumina-gpu
  --features gpu`). Ein GPU-Thema gilt ohne lokalen Lauf der **relevanten
  E2E-Tests** als **nicht getestet**, nicht als grün: verbindlich ist
  `cargo test -p lumina-gpu --features gpu-adapter-tests` (`tests/support::
  require_adapter` macht einen fehlenden Adapter zum harten Fehler) **plus** die
  betroffenen GUI-GPU-Tests. `--features gpu` allein genügt **nicht**. Das
  Testergebnis ist im Abschlussbericht mit Kommando, Zahlen und Fehlernamen zu
  dokumentieren; das erste lokale Ergebnis dient als Regressions-Baseline. Die
  CI-Lücke ist in `feature/platform/capability-matrix.md` dokumentiert.

AI-Modelle, RAW-Fixtures und Referenzbilder müssen reproduzierbar versioniert
und lizenzrechtlich dokumentiert sein. Tests dürfen nicht von einem spontanen
Modell-Download oder externen Netzwerkzugriff abhängen.

### Integration headless gelieferter Features

**Arbeitsteilung:** Ein anderer Agent entwickelt Features **headless** — also
Schema, `lumina-core`, `lumina-cli`, `lumina-sidecar` — **ohne** GUI und
**ohne** kittest. Der Build-Agent **integriert** sie danach in `lumina-gui` und
in die kittest-Abdeckung.

**Grenze der GUI-Testbarkeit:** Der GUI-Pfad ist **ausschließlich headless**
prüfbar — `egui Context` + `LuminaApp` im tempdir, ergänzt um `egui_kittest`
mit wgpu-Adapter. Es gibt **keinen** echten Fenster-/Interaktions-Test.
**Keine Abnahmekriterien formulieren, die ein real gerendertes Fenster, echte
Maus-/Zeigerereignisse, DPI- oder Multi-Monitor-Verhalten oder einen sichtbaren
GUI-Lauf durch einen Menschen voraussetzen.** Ist ein Feature nur so prüfbar,
ist das eine **laut benannte Lücke** mit Hardware-Gate — kein Anlass, die
Abnahme zu beschönigen oder den Test als „manuell geprüft" zu verbuchen. Die
kittest-Goldens laufen headless über den wgpu-Adapter und sind ein **lokales**
macOS-Gate; in CI werden sie **nie** verifiziert. Der **einzige** Weg zu einem
real gestarteten Prozess ist der manuelle Akzeptanz-Run nach R5-LOG-1 — eine
User-Aktion, die **nicht** durch einen headless Test ersetzt wird.

Pro geliefertem Headless-Feature prüft und ergänzt der Build-Agent:

1. **GUI-Verdrahtung.** Für jede neue Fähigkeit muss ein `draw_*`-Pfad
   existieren, der vom tatsächlichen UI-Panel aufgerufen wird; ein Modul mit
   Tests ohne `draw_`-Aufruf ist **nicht integriert**.
2. **Interaktive Bedienung.** Regler, Kurvengraphen, Picker und Buttons müssen
   **anklickbar** sein und ihren Wert tatsächlich in das Rezept schreiben —
   nicht nur paintbar. Für jede sichtbare Funktion ein klickbarer headless Test.
3. **kittest-Goldens.** Jede neue sichtbare Editor-Fläche braucht einen Golden
   im passenden Ziel (`kittest_snapshots` oder ein neues, feature-spezifisches
   `tests/kittest_*.rs`).
4. **Fixture-Klasse.** Neue Goldens brauchen eine echte Quelle. `R1` = echtes
   RAW/echte Bilddaten, `S1` = Layout-Sentinel, `S2` = Smoke-Raster
   (`feature/quality/golden-fixtures.md`). Ein Golden, das einen Decode-Fehler
   als Soll-Zustand festschreibt, ist **kein** Render-Nachweis — die
   Klassenabgrenzung ist in `golden-fixtures.md` Regel 5 verbindlich.
5. **Aktions-Audit.** Jede neue `GuiAction`-Variante oder Enum-Variante erzwingt
   den `ALL_GUI_ACTIONS`-Audit.
6. **SOLL-Zuordnung.** Das headless gelieferte SOLL-Dokument ist zu lesen und um
   den GUI-/kittest-Stand zu ergänzen; fehlt der GUI-Teil im SOLL, ist er **vor**
   dem Code zu schreiben.
7. **Ratchet.** Neue GUI-Module bleiben ≤ 500 Zeilen; wächst
   `lumina-gui/src/lib.rs`, ist echte Extraktion Pflicht und die Baseline danach
   zu senken.

**Abnahme einer Integration:** `cargo test -p lumina-gui --all-targets` grün,
jede neue Fläche hat einen klickbaren headless Test **und** einen Golden, der
`ALL_GUI_ACTIONS`-Audit ist grün, `sh scripts/check_file_sizes.sh` grün ohne neue
Baseline-Einträge, fmt/Clippy clean, und das Feature-Dokument nennt den GUI- und
kittest-Stand ausdrücklich.

### Golden-SOLL

- Ein Golden friert den **abgewarteten** Render-Zustand ein (`settle_render`,
  Prädikat `render_key.is_some()`), nicht einen Zwischenzustand. Normativ:
  `feature/quality/golden-references.md` §7.2.
- **Kein fester Neuaufnahme-Rhythmus.** Jedes `record` ist ein einzeln
  begründeter Schritt. Normativ: `feature/quality/golden-references.md`
  §9 Punkt 7.

### Verifikations-Niederlagen und Modellwahl

- **Nach drei aufeinanderfolgenden `NICHT BESTANDEN` derselben Findings-Gruppe
  gibt es KEINEN Modellwechsel.** Es werden ausschließlich freie Subagenten
  eingesetzt — kein deepseek, kein Bezahlmodell, keine Ausnahme.
- **Ausweg statt Modell:** Aufgabe weiter zerlegen (kleinere, unabhängig
  prüfbare Einheiten); jede Runde eine frische Session mit vollem
  Auftragskontext (Ausnahme: Fix-Runde derselben Aufgabe, siehe §„Rollen und
  Delegation"). Hält die Serie an, gehen die Befunde als interaktive Frage an
  den Eigentümer — drei gleiche Fehlschläge sind ein Signal zum
  Perspektivwechsel, nicht zur vierten Wiederholung.
- **Gilt für die Verifikation und alle weiteren Fix-Runden derselben Aufgabe.**
  Eine frische Session — oder die Ausnahme-Fortsetzung einer Fix-Runde — ändert
  nichts daran.
- **Zählung:** aufeinanderfolgende Runden **derselben** Aufgabe. Ein
  zwischenzeitlich bestandener Task setzt die Zählung zurück.
- **Dokumentation:** die dreifache Niederlage und der auslösende Befund stehen
  im Commit-Text und im Task-Eintrag.

### Testabdeckungs-Politik

- **Grundregel: alles bekommt Tests.** Jede Änderung an Produktcode, Schema,
  Migration, Persistenz, Pipeline oder Agentenregeln wird mit Tests geliefert,
  die die **neue** Aussage tatsächlich abdecken. Ein Test, der die Aussage nicht
  belegt, zählt nicht (DoD §7.5). Diese Regel wird **nicht** aus Bequemlichkeit
  oder Laufzeitgründen relativiert.
- **Gegenregel: Tests sind kein Selbstzweck.** Abdeckung ohne Informationswert
  ist ein Defekt. Ein Test, der eine Konstante auf sich selbst prüft, eine
  bereits abgedeckte Eigenschaft ein zweites Mal durchläuft, eine
  Implementierungsentscheidung statt des Verhaltens pinnt oder bei Wegfall der
  Logik nichts verlieren würde, ist ein **unnötiger Test**.
- **Rhythmus: höchstens einmal pro Woche ein Test-Redundanz-Audit**
  (`TEST-AUDIT-36`, fortlaufend). Der Build-Agent prüft die Suite auf unnötige
  Tests und **berichtet**; er entfernt nichts eigenmächtig. Für jede Löschung
  gilt der normale Weg: SOLL zuerst, Delegation an einen
  Implementierungs-Agent, danach unabhängige Verifikation. Beim Audit wird
  ausdrücklich geprüft, ob der Test eine **Fehlalarm-resistente** Aussage trägt.
- **Der Audit darf keine Löschung als Abkürzung für fehlende Abdeckung
  missbrauchen.** Wird eine Lücke sichtbar (Feature, Klausel oder Fehlerpfad
  ohne Anker), ist das eine **neue offene Aufgabe**.
- **Nachweispflicht:** Jeder umgesetzte Audit-Befund nennt Test-Name, Datei und
  die konkrete Begründung. Eine Löschung ohne Begründung gilt als
  Regel-Umgehung.
- **Eine extrahierte Testnaht ist noch kein Test der Produktion.** Wird eine
  Funktion extrahiert, um sie testbar zu machen, muss der **Produktionspfad sie
  aufrufen**. **Pflichtnachweis:** eine Mutation am Produktionspfad muss den
  Test rot machen. Ein Test, der die falsche Funktion prüft, ist schlimmer als
  keiner.
- **Eine Behauptung über „gemessen" ist ein Prüfauftrag, keine Aussage.** Werte,
  die als gemessen bezeichnet werden, müssen aus einer Messung stammen und
  gegen die Quelle geprüft sein — durch Nachrechnen oder eine unabhängige
  Gegenmessung, nicht durch Behauptung.
- **Vor dem Vergleich zweier Mengen wird die Definition beider geprüft.** Für
  jede Menge ist vorab zu belegen: was ihre Elemente sind (Klasse, Semantik,
  Quelle der Klassifikation), wer sie erzeugt und was sie **nicht** enthält.
  Details und das Fehlmuster stehen in [`DoD.md`](DoD.md) §9.

## Definition of Done

Normativ und vollständig: [`DoD.md`](DoD.md). Zusammengefasst ist eine Änderung
erst fertig, wenn:

- die SOLL-Anforderung eindeutig erfüllt oder bewusst angepasst ist;
- die betroffenen Datenformate versioniert und migriert werden können;
- die Implementierung mit passenden Tests ausgeliefert ist;
- die relevanten Prüfungen erfolgreich ausgeführt wurden;
- ein anderer Subagent die Implementierung und Testabdeckung bestätigt hat;
- keine unaufgelösten kritischen oder hohen Befunde bestehen;
- `Agents.todo.md` die erledigte Aufgabe nicht mehr enthält;
- `feature/README.md` auf das richtige Dokument verweist und das betroffene
  Feature-Dokument den erreichten Zustand sowie verbleibende Grenzen kennt;
- der Abschlussbericht reproduzierbare Prüfkommandos nennt;
- ein GUI-Feature nur als fertig gilt, wenn ein automatischer headless GUI-Test
  (egui Context + LuminaApp, tempdir; visuelle Änderungen zusätzlich via kittest
  Golden/PSNR/Histogram, byte-identisch/PSNR, kein stiller Fallback) vorliegt und
  `cargo test -p lumina-gui` ohne GPU grün ist;
- der Verifizierungsbericht die BESTANDEN-Checkliste aus [`DoD.md`](DoD.md) §7
  vollständig mit Belegen beantwortet (End-to-End-Kette, zeitbasierte Pfade,
  Klassen-Vollständigkeit, Log-Level, Spez→Test-Mapping, Gates);
- jede **Abdeckungs- oder Ursachenaussage** entweder mutationsbewiesen am
  Produktionspfad oder ausdrücklich als ungeprüft gekennzeichnet ist
  ([`DoD.md`](DoD.md) §9); eine Aussage, die eine Messung widerlegt hat, wurde an
  ihrer Fundstelle zurückgenommen (§9);
- kein Test eine Aussage stützt, die er nicht falsch machen kann — keine
  Adress-Identität, keine selbstbezügliche Erwartung, keine Umgebungs- oder
  Timing-Annahme ([`DoD.md`](DoD.md) §10);
- ein behobener Defekt **vorher rot reproduziert** war und ein nicht
  reproduzierbarer Befund als zurückgenommen gekennzeichnet ist
  ([`DoD.md`](DoD.md) §11);
- die CI auf dem gelieferten Stand **nachweislich gelaufen** ist, oder das
  verbleibende Risiko ausdrücklich benannt wurde ([`DoD.md`](DoD.md) §12).

## Dokumentations- und Todo-Regeln

- `Agents.todo.md` enthält ausschließlich offene, umsetzbare Aufgaben und die
  noch offenen Entscheidungen; Entscheidungen tragen dort ihre Begründung und
  einen `Stand`.
- Abgeschlossene Punkte werden nach bestätigter Verifizierung **komplett aus
  der Datei entfernt** — niemals nur abhaken und niemals in einen neuen
  Bereich/Block (z. B. „Erledigt", „Verifiziert", „Done") verschieben. Erledigter
  Zustand lebt in der Git-Historie und den Feature-Dokumenten weiter. Das gilt
  für Aufgaben und Entscheidungen gleichermaßen; der `Stand`-Wert
  `umgesetzt`/`erledigt` ist ein Formverstoß, ebenso jede `- [x]`-Zeile
  (`scripts/check_plan_format.sh`).
- Eine Aufgabe oder Entscheidung darf nur entfernt werden, wenn der
  Verifizierungs-Agent die Implementierung und Testabdeckung bestätigt hat.
- Neue Erkenntnisse oder Folgearbeiten werden als neue offene Aufgaben ergänzt.
- Änderungen an fachlichen Zielzuständen werden zuerst im betroffenen Dokument
  unter `feature/` beschrieben und im `feature/README.md` verlinkt.
- Ein Anker auf Code in der Prosa nennt den **vollständigen repo-relativen Pfad**
  *und* den **Symbol- bzw. Testnamen**; Zeilennummern sind in Prosa nur in
  **datierten Audit-Belegen** erlaubt.
- Wichtige Architekturentscheidungen werden als ADR oder klar markierter
  Entscheidungsabschnitt dokumentiert.
- Der Plan darf niemals stillschweigend durch Codeänderungen veralten.

## Änderungsregeln

- Keine Originaldatei und kein Benutzerfoto wird für Tests oder Migrationen
  verändert.
- Keine stillen Fallbacks bei veralteten Rezepten, Farbprofilen, Masken oder
  fehlenden Artefakten.
- Keine vorbestehenden Build-/Clippy-/Test-Fehler oder -Warnungen werden als
  „bekannt", „vorbestehend" oder „nicht Teil des Gates" abgehakt. Jeder solche
  Befund wird vor dem Abschluss eines Batches sauber behoben — in der Regel
  durch korrektes `#[cfg]`-Gating oder echte Fehlerbehandlung, nicht durch
  `allow`-Attribute oder das Ausklammern aus den Prüfschritten.
- Keine Pipeline-Stufe ohne definierte Reihenfolge, Versionierung und Tests.
- Keine Rezeptfeldänderung ohne Schema- und Migrationsentscheidung.
- Keine absolute Pfadangabe in einem portablen Sidecar.
- Keine zentrale DB-Funktion, die ohne Sidecar-Datenverlust verursachen würde.
- Keine GUI-spezifische Bildlogik außerhalb der gemeinsamen Pipeline.
- Keine native Dependency ohne dokumentierte Capability-Entscheidung.
- **Dateigröße (Ratchet):** Eine `.rs`-Datei mit mehr als 500 Zeilen darf nicht
  wachsen (CI: `scripts/check_file_sizes.sh` gegen
  `scripts/file_size_baseline.txt`). Verkleinern und Extrahieren jederzeit;
  danach den Baseline-Eintrag senken (bzw. entfernen, sobald die Datei wieder
  ≤ 500 Zeilen hat). Neue Dateien bleiben strikt bei ≤ 500 Zeilen — keine neuen
  Baseline-Einträge; größere Ziele werden feiner gesplittet, bestehende Einträge
  bleiben bestehen. Kein willkürliches Splitten ohne Kohäsion — Extraktion folgt
  Modulgrenzen. Für `crates/lumina-gui/src/lib.rs` ist die kohäsionsgetriebene
  Extraktion ein eigener, fortlaufender Planpunkt (`GUI-LIB-SPLIT-65`): Slice für
  Slice, jeder byte-identisch und mit **gesenktem** Baseline-Eintrag. **Anti-
  Gaming (Pflicht):** Neue Logik gehört in neue oder passende kleine Dateien —
  nicht in die übergroße Bestandsdatei; Kleinstverdrahtung an bestehenden
  Aufrufstellen darf bleiben. Das Löschen von Kommentaren, Doku oder Tests zur
  Kompensation von Wachstum gilt als Regel-Umgehung und wird in der Verifizierung
  abgelehnt (Diff-Prüfung).
- **Volle GPU-Parität:** Jede Renderstufe muss auch GPU-tauglich sein — nichts
  darf CPU-only bleiben. Ein Rezept, das wegen einer nicht implementierten
  GPU-Stufe auf CPU zurückfällt („Render routed to CPU"), ist ein Fail mit
  Fix-Pflicht, kein akzeptierter Zustand. Umgekehrt bleibt die CPU die
  vollständige Referenz: Jede Renderung ist auf CPU erreichbar; GPU ist reine
  Beschleunigung, nie Voraussetzung (kein GPU-only-Weg).
- Lizenzbedingungen von RAW-Backends, ONNX-Runtime und Modellen werden vor
  Integration geprüft und dokumentiert.

## Gates

Regeln dieser Ebene. Was nur ein Verzeichnis betrifft, gehört in die
`AGENTS.md` **dieses** Verzeichnisses — sie wird geladen, wenn eine Datei
daraus gelesen wird.

Ein Plan in `docs/plans/` beginnt mit `**Status:** Plan | In Arbeit | Done |
Empfehlung, nicht implementiert`.

Die kanonischen Agent-Regeln laufen über `ratchet` (Preset `agent-rules`):
`ratchet check` vor jedem Commit und in der CI.
