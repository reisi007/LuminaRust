# SOLL — Zeilen-Obergrenze 3000 Zeilen je Datei (FILE-SIZE-CEILING-3K-83)

**Feature-ID:** `FILE-SIZE-CEILING-3K-83` · **Herkunft:** Eigentuemervorgabe
2026-10-20 · **Status:** SOLL-Zielsemantik, umgesetzt in
`scripts/check_file_ceiling.sh` (Gate) und `scripts/file_size_ceiling.txt`
(Ausnahmeliste)

> **Normativer Kern (woertlich):** „Auf Dauer waere unter 3k Zellen pro File
> wahrscheinlich sinnvoll" und „**Wenn einmal eine Datei unter der Grenze ist,
> dann darf sie nie mehr darueber sein.**"

Der Zielwert **3000** ist damit eine **Eigentuemervorgabe** — kein gemessener
Wert, keine aus dem Code abgeleitete Schwellzahl und keine Kennzahl des
Ist-Zustands. Er wird hier als Vorgabe festgeschrieben, damit die
Umsetzung (Gate + Ausnahmeliste) an **einer** Stelle nachzulesen ist. Wer
kleinere Zahlen mit begruendeter Messung belegen will, muss die Vorgabe
ausdruecklich aendern — nicht die Datei.

Dieses Dokument ist die **Zielsemantik**; die Schranke selbst steht in
`scripts/check_file_ceiling.sh`, die Ausnahmeliste in
`scripts/file_size_ceiling.txt`.

---

## Inhaltsverzeichnis

- [1. Zweck und Geltungsbereich](#1-zweck-und-geltungsbereich)
- [2. Population](#2-population)
- [3. Die Zusage: einmal unter der Grenze heisst dauerhaft darunter](#3-die-zusage-einmal-unter-der-grenze-heisst-dauerhaft-darunter)
- [4. Richtung: die eingetragenen Werte duerfen nur sinken](#4-richtung-die-eingetragenen-werte-duerfen-nur-sinken)
- [5. Verhaeltnis zur 500-Zeilen-Wachstumsschranke](#5-verhaeltnis-zur-500-zeilen-wachstumsschranke)
- [6. Bezahlpfad](#6-bezahlpfad)
- [7. Das Gate: die Pruefregeln](#7-das-gate-die-pruefregeln)
- [8. Was der Gate nicht deckt](#8-was-der-gate-nicht-deckt)
- [9. Pflege der Werte](#9-pflege-der-werte)
- [10. Ist-Stand der Ausnahmeliste (generiert)](#10-ist-stand-der-ausnahmeliste-generiert)

---

## 1. Zweck und Geltungsbereich

Zwei Eigentuemervorgaben vom 2026-10-20:

1. **Obergrenze:** langfristig keine Datei ueber **3000 Zeilen**.
2. **Dauerhaftigkeit:** „Wenn einmal eine Datei unter der Grenze ist, dann darf
   sie nie mehr darueber sein."

Die zweite Vorgabe ist die **tragende** — eine Obergrenze ohne
Dauerhaftigkeitszusage waere eine Momentaufnahme: eine gesrumpfte Datei
koennte spaeter wieder wachsen, und der Bestand traegt die Schranke dann
nicht mehr. Deshalb ist die Zusage im Gate **hart** (FAIL), nicht als
Empfehlung formuliert, und sie bekommt einen eigenen, benannten Zahlungsweg
(§6).

**Geltungsbereich:** alle Rust-Dateien des Workspace (Population in §2), nicht
nur die, die heute auffallen. Die Schranke ist eine **Obergrenze**; die
bestehende 500-Zeilen-Schranke ist eine **Wachstumsschranke mit
Registrierungspflicht** — sie verbietet nicht nur Wachstum ueber 500, sondern
auch **unregistrierte Neuzugaenge** ueber 500 (gemessen:
`scripts/check_file_sizes.sh`, Zeilen 35–48). Beide gelten nebeneinander
(§5).

**Nicht** im Geltungsbereich: Dateien, die keine Rust-Dateien sind
(Markdown, Shell, YAML, TOML). Fuer sie gibt es keine Zeilen-Obergrenze; die
harten Zahlen der Doku (z. B. `Agents.md`) sind eine Sache des
Doku-Drift-Gates, nicht dieses.

## 2. Population

Die Population ist **alle `.rs`-Dateien** unter

- `crates/` (alle Crates des Workspace, `src/` und `tests/`) und
- `agent-harness/` (das zweite Cargo-Workspace des Projekts).

Die Zahlen der Population stehen im generierten Block unten. Sie werden von
`scripts/gen_doc_facts.sh population` aus dem Dateisystem abgeleitet (dieselbe
Population, die `scripts/check_file_ceiling.sh` enumeriert) und vom
Drift-Gate `scripts/check_doc_facts_drift.sh` gegen dieses Dokument geprueft —
eine Zahl im Prosa-Text waere eine zweite Quelle der Wahrheit, die still
altert (Muster `DOC-ZAHLEN-DRIFT-82`):

<!-- BEGIN GENERATED: population -->
Population (`crates/` + `agent-harness/`, alle `.rs`-Dateien): **858** Dateien, davon
**70** ueber 500 Zeilen und **5** ueber 3000 Zeilen.
<!-- END GENERATED: population -->

Es gibt bewusst **keine Ausnahmen fuer Verzeichnisse**: eine Datei, die nicht
in der Population ist, faellt nicht unter die Schranke; eine Datei, die es ist,
faellt ohne Ausnahme darunter. `target/`-Verzeichnisse (Build-Artefakte)
gehoeren nicht zur Population.

Warum `agent-harness/` mitgezogen wird: das Harness ist ein eigenes
Cargo-Workspace mit eigenem Lockfile und eigenem CI-Job. Die 500er-Schranke
deckt es **nicht** ab (ihre Population ist nur `crates/`) — die Obergrenze
soll aber fuer den ganzen Workspace gelten, sonst entsteht genau dort eine
Luecke, in der die Datei still wachsen kann.

## 3. Die Zusage: einmal unter der Grenze heisst dauerhaft darunter

Das ist die **zentrale Zusage** dieses Dokuments, und sie ist in zwei
Schritten zu lesen:

1. **Der Zustand einer Datei ohne Ausnahme-Eintrag ist „unter der Grenze".**
   Fuer sie gilt die harte Grenze 3000 — **dauerhaft**, unabhaengig davon, ob
   die Datei heute 12 Zeilen oder 2999 Zeilen hat.
2. **Faellt eine gelistete Datei auf <= 3000 Zeilen, faellt ihr Eintrag weg.**
   Ab diesem Zeitpunkt gilt fuer sie Regel 1: die harte Grenze 3000. Ein
   spaeteres Wachsen ueber 3000 ist ein **Verstoss** — der Gate meldet ihn
   als Regel (A) „Datei ohne Eintrag ueber der Grenze".

Der Weg von (2) zur harten Grenze ist **nicht** automatisch, sondern
**Pflicht**: ein Eintrag, der bei einer Datei mit <= 3000 Zeilen stehen
bleibt, haelt eine Ausnahme offen, unter der die Datei spaeter wieder
wachsen koennte. Der Gate erzwingt deshalb das Entfernen (Regel (E) in §7):
eine Datei mit Eintrag und <= 3000 Zeilen ist ein **Fehler**, nicht ein
Hinweis.

**Merksatz:** „Einmal unter der Grenze = dauerhaft darunter" heisst nicht
„der Wert wird einmal notiert", sondern „der Zustand wird **gesperrt**".

## 4. Richtung: die eingetragenen Werte duerfen nur sinken

Jeder Eintrag in `scripts/file_size_ceiling.txt` ist eine
**Maximalgroesse** fuer genau eine Datei (das Format ist das des bestehenden
`scripts/file_size_baseline.txt`: `<wert> <pfad>  # <Begruendung/Historie>`).

- Ein eingetragener Wert darf **nur sinken**, nie steigen.
- Jede **Aenderung** an `scripts/file_size_ceiling.txt` ist ein
  **sichtbares Ereignis**: sie erscheint als **Diff auf der Baseline-Datei**
  und wird **reviewt**. Der Gate selbst schreibt die Datei **nicht** um — er
  meldet nur.
- Das gilt fuer **jede** Aenderung, auch fuer das Senken: eine Senkung ist
  erlaubt und erwuenscht, aber sie wird **ebenfalls** sichtbar gemacht (als
  Diff), damit der Weg der Groesse im Repo nachvollziehbar bleibt.

**Arbeitsteilung Tripwire vs. Review:** der Tripwire (Regel (D) in §7)
vergleicht den **Arbeitsbaum** mit der **committeten** Baseline und wird rot,
sobald im Arbeitsbaum ein Wert angehoben wird. Eine **committete**
Anhebung sieht der Tripwire nicht (nach dem Commit sind Arbeitsbaum und
`HEAD` identisch) — sie wird ueber den **Diff der Baseline-Datei** im Review
erkannt. Das ist keine Luecke, sondern die Zweiteilung der Verantwortung:
das Gate faengt den Arbeitsbaum, das Review die Historie.

## 5. Verhaeltnis zur 500-Zeilen-Wachstumsschranke

Es gibt **zwei Klassen, zwei Gates, zwei Baseline-Dateien** — beide bestehen
nebeneinander, beide sind blockierend, beide laufen in der schnellen Stufe
`fast`:

| | 500-Zeilen-Wachstumsschranke | 3000-Zeilen-Obergrenze (hier) |
| --- | --- | --- |
| Gate | `scripts/check_file_sizes.sh` | `scripts/check_file_ceiling.sh` |
| Baseline | `scripts/file_size_baseline.txt` | `scripts/file_size_ceiling.txt` |
| Art | **Wachstumsschranke + Registrierungspflicht** — verbietet Wachstum ueber 500 **und** unregistrierte Neuzugaenge ueber 500 | **Obergrenze** — setzt eine harte Obergrenze 3000 |
| Population | `.rs` unter `crates/` (gemessen: `scripts/check_file_sizes.sh`, Zeile 35) | `.rs` unter `crates/` **und** `agent-harness/` |
| Datei mit Eintrag | darf ihren Wert nicht ueberschreiten | darf ihren Wert nicht ueberschreiten |
| Datei ohne Eintrag | muss **<= 500 Zeilen** haben — sonst **FAIL** („no baseline entry in scripts/file_size_baseline.txt", gemessen `scripts/check_file_sizes.sh`, Zeilen 38–42): sie braucht einen Eintrag | muss **<= 3000 Zeilen** haben — sonst **FAIL** |
| Erlaubte Richtung des Eintrags | **nicht erzwungen** — das 500er-Gate prueft nur `n > b` (gemessen `scripts/check_file_sizes.sh`, Zeile 43); eine **Anhebung** des Eintrags ist maschinell nicht gesperrt (die Baseline enthaelt belegte Anhebungen, z. B. „BEWUSSTE Anhebung 12572 -> 12573“) | nur sinken (Ratchet), erzwungen ueber (D) |
| Einstieg in der CI | `fast-2` | `fast-7` |

**Merke (zwei Schranken, zwei Registrierungen):** ein Schnitt, der ein Modul mit
mehr als **500** Zeilen erzeugt, braucht einen **500er-Baseline-Eintrag** in
`scripts/file_size_baseline.txt` (sonst `fast-2`-Rot) — **zusaetzlich** zu
dieser 3000er-Liste. Die beiden Listen sind unabhaengig: eine Datei mit 900
Zeilen steht in der 500er-Liste, eine mit 3500 Zeilen in der 3000er-Liste.

**Warum zwei Gates und nicht eines:** die beiden Schranken beantworten
verschiedene Fragen. Die 500er fragt „wird eine registrierte Datei grosser, und
ist eine neue Datei ueber 500 ueberhaupt registriert?", die 3000er fragt „gibt
es eine Datei, die zu gross geworden ist?". Die 500er erkennt **Wachstum und
unregistrierte Neuzugaenge** frueh (ab 500 Zeilen), die 3000er erkennt
**Ueberschreitung** der eigentlichen Grenze. Eine Datei mit 900 Zeilen braucht
fuer die 500er einen Eintrag, liegt aber unter beiden Grenzen; eine Datei mit
3500 Zeilen ist unter der 500er erlaubt (sie hat einen Eintrag und waechst
nicht) und unter der 3000er ein **FAIL ohne Eintrag** — bzw. mit Eintrag eine
**Ausnahme, die nur sinken darf**.

Die 500er-Schranke bleibt **unveraendert** bestehen; die 3000er-Schranke
aendert sie nicht, und die 3000er ersetzt sie nicht.

## 6. Bezahlpfad

Der Bezahlpfad ist die Frage: **wie kommt eine Datei aus der Ausnahmeliste
heraus, und was passiert danach?**

1. **Die Datei schrumpft** (Extraktion eines Moduls, Weglassen eines Codes):
   der Ist-Wert sinkt, der **eingetragene Wert muss mitgezogen** werden.
   Der Gate meldet das als Regel (C) („Eintrag kann gesenkt werden") — der
   Hinweis ist der Anstoss, die Baseline von Hand zu senken. Der Wert darf
   dabei **beliebig** sinken, solange er nicht steigt.
2. **Die Datei faellt auf <= 3000 Zeilen:** dann **faellt ihr Eintrag weg**
   (Regel (E) — ein stehengebliebener Eintrag ist ein Fehler). Damit gilt
   fuer die Datei die **harte Grenze 3000 dauerhaft** (§3).
3. **Eine neue Datei ueber 3000 Zeilen** (im Population) ist ein **FAIL**
   (Regel (A)), bis sie mit einem Eintrag in der Ausnahmeliste **bewusst**
   aufgenommen wurde. Jede Aufnahme ist ein sichtbares Ereignis (§4) und
   eine **Ausnahme**, die nur sinken darf. Der **neue Eintrag traegt den
   Ist-Wert der Datei** (Regel (F)) — ein hoeherer Einstiegswert waere eine
   Erlaubnis, die die Datei nie hatte.
4. **Ein Schnitt, der ein neues Modul ueber 500 Zeilen erzeugt,** braucht
   zusaetzlich einen **500er-Baseline-Eintrag** in
   `scripts/file_size_baseline.txt` (sonst `fast-2`-Rot) — die beiden
   Ratchets sind unabhaengig (§5).

Der Bezahlpfad ist also: **Eintrag senken → Eintrag entfernen → harte Grenze
fuer immer.** Und der Weg zurueck (Eintrag wieder anlegen) gibt es nicht.

## 7. Das Gate: die Pruefregeln

`sh scripts/check_file_ceiling.sh` — POSIX `sh`, ohne Cargo, ohne Codegen, in
Sekunden; liest nur, **schreibt nichts** ins Repo. Der Zielwert 3000 steht
als **Konstante** an **einer** Stelle im Skript (`CEILING=3000`).

| Regel | Pruefung | Ergebnis |
| --- | --- | --- |
| **(A) Harte Grenze** | eine `.rs`-Datei der Population **ohne** Eintrag muss <= 3000 Zeilen haben | **FAIL** (Datei + Zeilenzahl) |
| **(B) Je-Datei-Obergrenze** | eine Datei **mit** Eintrag darf ihre eingetragene Groesse nicht ueberschreiten | **FAIL** (Datei, Ist-Wert, erlaubter Wert) |
| **(C) Zahlungshinweis** | der eingetragene Wert ist **groesser** als die tatsaechliche Dateigroesse | **INFO** („Eintrag kann gesenkt werden") |
| **(D) Richtung (Tripwire)** | der eingetragene Wert darf nicht groesser sein als der Wert derselben Datei in der **committeten** Baseline | **FAIL** („Baseline-Wert angehoben") |
| **(E) Zahlungspflicht** | eine Datei **mit** Eintrag hat <= 3000 Zeilen | **FAIL** („Eintrag muss entfernt werden — ab hier gilt die harte Grenze 3000 dauerhaft") |
| **(F) Einstiegswert** | ein **neuer** Eintrag (einer, der nicht in der committeten Baseline steht) traegt den **Ist-Wert** der Datei; steht er darueber | **FAIL** („der Eintrag muss auf den Ist-Wert gesetzt werden — die Obergrenze darf nur sinken") |

Zusaetzlich, ohne eigene Nummer: ein Eintrag zu einer **nicht mehr
existierenden** Datei ist ein **INFO** („Eintrag kann entfernt werden") — die
Regeln (A), (B), (C), (E) und (F) pruefen die **Datei** (ihren Ist-Wert);
nur (D) vergleicht mit der **Historie** (der committeten Baseline).

**Zu (D) im Detail.** Der Vergleichswert kommt aus
`git show HEAD:scripts/file_size_ceiling.txt` (ueber den Testhaken
`LUMINA_CEILING_REF` anstelle von `HEAD` reproduzierbar). Ist `git` nicht
verfuegbar oder ist die Baseline noch nicht committet, **darf das nicht
stillschweigend gruen sein**: der Gate schreibt dann einen **ausdruecklichen
HINWEIS**, dass diese Pruefung **nicht gelaufen** ist — auf **stderr**, damit
er in CI-Logs nicht untergeht — **und** die Schlusszeile nennt den Zustand
mit (`file_ceiling OK: … - Richtung-Pruefung (D) hat NICHT gelaufen …`), damit
eine gruene Zeile nicht missverstanden wird. Ein gruener Gate, der eine
Pruefung uebersprungen hat, waere eine Behauptung ohne Aussage.

**Zu (F) im Detail.** (D) vergleicht nur Werte, die **schon** in der
committeten Baseline stehen; fuer einen neuen Eintrag gibt es keinen
Vergleichswert. Ohne (F) waere der Einstiegswert frei waehlbar — ein Eintrag
99999 erlaubte einer 3500-Zeilen-Datei, auf 99999 Zeilen zu wachsen. (F)
bindet den neuen Eintrag an den Ist-Wert; ein Eintrag **ueber** dem Ist-Wert
ist ein **FAIL**, ein Eintrag **auf** dem Ist-Wert bleibt ein **INFO**
(„sichtbares Ereignis“), damit der allererste Commit der Liste nicht
blockiert wird. **Solange keine committete Baseline lesbar ist, gilt
jeder Eintrag als neu** — ein Eintrag ueber dem Ist-Wert FAILt dann, waehrend
derselbe Zustand nach dem ersten Commit nur (C)-INFO waere (gemessen: Eintrag
3400 bei 3333 Zeilen -> FAIL ohne committete Baseline, INFO danach).

## 8. Was der Gate nicht deckt

Ehrliche Grenzen — sie gehoeren benannt, weil ein Gate, das mehr behauptet
als er prueft, schlimmer ist als kein Gate:

1. **Keine committete Anhebung.** Der Tripwire sieht nur den Arbeitsbaum
   gegen `HEAD`. Eine Anhebung, die **committet** ist, faellt erst im
   **Diff-Review** der Baseline-Datei auf (§4). Wer ohne Review committet,
   hebt die Obergrenze still — das ist die bewusst geteilte Verantwortung.
2. **Keine Pruefung der nicht-Rust-Dateien.** Markdown, Shell, YAML und
   TOML sind nicht in der Population.
3. **Keine Aussage ueber die Qualitaet einer Datei.** Der Gate zaehlt
   Zeilen. Eine Datei mit 2999 Zeilen ist erlaubt, eine mit 1200 Zeilen ist
   erlaubt — ob die Verantwortlichkeiten dort sauber getrennt sind, ist eine
   Frage der Slicing-Tasks, nicht dieses Gates.
4. **Keine Pruefung der Population auf Vollstaendigkeit** gegen eine
   Referenzliste: der Gate enumeriert die Population aus dem Dateisystem.
   Eine Datei, die aus dem Repo verschwindet, faellt nicht auf.
5. **Keine Pruefung der *qualitativen* Schrumpfung.** Der Gate verlangt
   einen sinkenden Wert, nicht den Grund. Ob eine Datei durch **Extraktion**
   geschrumpft ist oder durch Loeschen von Kommentaren, ist nicht
   maschinell prüfbar und wird im Review der Baseline-Datei beurteilt.
6. **Keine Pruefung des `agent-harness/`-Ratchets unter 500 Zeilen** — die
   500er-Schranke deckt nur `crates/` ab (ihre Population endet dort,
   `scripts/check_file_sizes.sh`, Zeile 35); fuer das Harness gilt hier
   ausschliesslich die 3000er-Obergrenze. Ein Harness-Modul ueber 500 Zeilen
   braucht deshalb **keinen** 500er-Baseline-Eintrag, aber es braucht einen
   3000er-Eintrag, sobald es 3000 Zeilen ueberschreitet.
7. **Der Einstiegswert (F) gilt nur fuer NEUE Eintraege.** Ein bereits
   committeter Eintrag darf ueber dem Ist-Wert stehen — das meldet Regel (C)
   als **INFO** („Eintrag kann gesenkt werden"), nicht als Fehler. Der
   committete Eintrag ist die **Maximalgroesse** der Datei, nicht ihr
   Ist-Wert; er darf aber nur sinken (Regel (D)).
8. **Kein Ersatz fuer die Slicing-Tasks.** Die Obergrenze erlaubt Ausnahmen,
   sie entfernt sie nicht. Das Zerschneiden der sieben zu grossen Dateien
   bleibt Planungsarbeit — die **Schnittpfad-Analyse je Datei** und die
   **sechs Slicing-Tasks** liegen seit 2026-10-20 in `Agents.todo.md`
   (Abschnitt „Folge aus `FILE-SIZE-CEILING-3K-83“).

## 9. Pflege der Werte

- **Wer pflegt:** der Build-/Implementierungs-Agent, der eine Datei schrumpft
  (Extraktion), senkt den Eintrag **im selben Commit** wie die Extraktion —
  und der Reviewer prueft diesen Diff.
- **Was der Agent nicht darf:** einen Eintrag anheben, einen Eintrag fuer
  eine Datei stehen lassen, die <= 3000 Zeilen hat, oder einen Eintrag
  entfernen, ohne die Datei tatsaechlich unter 3000 Zeilen zu bringen.
- **Wer pflegt nicht:** niemand automatisch. Der Gate schreibt die
  Baseline-Datei **nie** — er meldet nur (`INFO`/`FAIL`). Eine
  automatisch umgeschriebene Baseline waere eine zweite Quelle der Wahrheit
  und wuerde den Diff unsichtbar machen.
- **Reihenfolge der Commits:** die Datei schrumpfen **und** den Eintrag
  senken **im selben Commit** — sonst sieht der Review zwei halbe Schritte
  statt eines.

## 10. Ist-Stand der Ausnahmeliste (generiert)

**Die Zahlen stehen in `scripts/file_size_ceiling.txt` und in den generierten
Bloecken, nicht im Prosa-Text.** Dieser Abschnitt ist die **Zielsemantik**
plus der gemessene Ist-Stand; eine Wertkopie waere eine zweite Quelle der
Wahrheit, die still altert (Muster `DOC-ZAHLEN-DRIFT-82`).

Die Dateien der Population ueber 3000 Zeilen sind im generierten Block in §2
gezaehlt. Ihre Ist-Werte, der Eintrag der 3000er-Ausnahmeliste und der Eintrag
der 500er-Baseline stehen hier — ebenfalls generiert
(`scripts/gen_doc_facts.sh file-sizes`, geprueft vom Drift-Gate
`scripts/check_doc_facts_drift.sh`). Die Differenz zwischen den Spalten ist
der Drift-Fall: die Datei ist kleiner als ihr Eintrag (Eintrag zu hoch, darf
gesenkt werden) oder groesser (Eintrag gerissen, die harte Grenze gilt).

<!-- BEGIN GENERATED: file-sizes -->
| Datei | Ist-Stand (Zeilen) | Eintrag `scripts/file_size_ceiling.txt` | Eintrag `scripts/file_size_baseline.txt` |
| --- | --- | --- | --- |
| `crates/lumina-gui/src/lib.rs` | 10517 | 10517 | 10517 |
| `crates/lumina-cli/src/main.rs` | 7261 | 7261 | 7261 |
| `crates/lumina-gpu/src/lib.rs` | 4650 | 4650 | 4650 |
| `crates/lumina-gpu/tests/parity.rs` | 3386 | 3386 | 3386 |
| `crates/lumina-gpu/src/stages.rs` | 3102 | 3102 | 3102 |
<!-- END GENERATED: file-sizes -->

- `crates/lumina-gui/src/lib.rs` wird bereits ueber `GUI-LIB-SPLIT-65`
  geschnitten (der Vorgang laeuft; sein letzter dort protokollierter Schnitt
  ist in `scripts/file_size_baseline.txt` kommentiert). Fuer die uebrigen
  Dateien der Tabelle ist je ein Slicing-Task **angelegt** (`Agents.todo.md`,
  Abschnitt „Folge aus `FILE-SIZE-CEILING-3K-83“); die Schnittpfad-Analyse
  mit den gemessenen Modulgrenzen und Symbolnamen steht dort mit.

- **Datierte Beobachtung vom 2026-10-20:** die **500er-Baseline**
  `scripts/file_size_baseline.txt` trug fuer
  `crates/lumina-gpu/tests/parity.rs` noch den Wert vom 2026-09-18, waehrend
  die Datei seither geschrumpft war (der aktuelle Wert steht in
  `scripts/file_size_ceiling.txt` und in der Tabelle oben). Das war eine
  **Drift in der anderen Baseline** (zu hoch, also zu locker) und **nicht**
  Gegenstand dieses Dokuments; die 3000er-Ausnahmeliste traegt den aktuellen
  Ist-Wert. Die Korrektur der 500er-Baseline gehoert in deren eigenes
  Ratchet-Verfahren (`check_file_sizes.sh` meldet einen zu hohen Eintrag
  nicht, weil es nur Wachstum verbietet).
