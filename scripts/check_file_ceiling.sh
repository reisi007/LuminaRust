#!/bin/sh
# File-size CEILING (Zeilen-Obergrenze 3000) — Ratchet-Gate der
# Eigentuemervorgabe 2026-10-20 (FILE-SIZE-CEILING-3K-83).
#
# Zweck: die Eigentuemervorgabe sagt wörtlich „Auf Dauer wäre unter 3k
# Zellen pro File wahrscheinlich sinnvoll" UND „Wenn einmal eine Datei unter
# der Grenze ist, dann darf sie nie mehr darüber sein." Das Skript macht
# beides maschinell:
#
#   (A) HARTE GRENZE  — jede .rs-Datei der Population OHNE Eintrag muss
#                       <= 3000 Zeilen haben; sonst FAIL. Das ist die
#                       Zusage „einmal unter der Grenze = dauerhaft darunter":
#                       ohne Eintrag heisst „unter der Grenze", und das bleibt
#                       so.
#   (B) JE-DATEI-OBERGRENZE — eine Datei MIT Eintrag darf ihre eingetragene
#                       Groesse nicht ueberschreiten; sonst FAIL (mit Datei,
#                       Ist-Wert und erlaubtem Wert).
#   (C) BEZAHLHINWEIS — der eingetragene Wert ist groesser als die Datei:
#                       INFO „Eintrag kann gesenkt werden" (wie beim
#                       500er-Gate).
#   (D) RICHTUNG (Tripwire) — der eingetragene Wert darf nicht groesser sein
#                       als der Wert derselben Datei in der COMMITTETEN
#                       Baseline (`git show HEAD:scripts/file_size_ceiling.txt`);
#                       sonst FAIL „Baseline-Wert angehoben".
#   (E) BEZAHLPFLICHT — eine Datei MIT Eintrag hat <= 3000 Zeilen: FAIL
#                       „Eintrag muss entfernt werden". Ohne diese Regel
#                       bliebe eine Ausnahme offen, unter der die Datei
#                       spaeter wieder ueber 3000 wachsen koennte — genau das
#                       verhindert (A).
#   (F) EINSTIEGSWERT — ein NEUER Eintrag (einer, der nicht in der committeten
#                       Baseline steht) traegt den IST-Wert der Datei; steht er
#                       darueber: FAIL „der Eintrag muss auf den Ist-Wert
#                       gesetzt werden". Ohne diese Regel waere der
#                       Einstiegswert frei waehlbar — ein Eintrag 99999 erlaubt
#                       einer 3500-Zeilen-Datei, auf 99999 Zeilen zu wachsen.
#
# ZIELWERT: 3000 Zeilen. Er ist eine EIGENTUEMERVORGABE (kein gemessener
# Wert) und steht als KONSTANTE an genau EINER Stelle in diesem Skript
# (CEILING=3000). Die normative Zielsemantik steht in
# `feature/quality/file-size-policy.md`.
#
# Population: alle `.rs`-Dateien unter `crates/` UND `agent-harness/`
# (die aktuelle Dateizahl steht im generierten Block `population` in
# feature/quality/file-size-policy.md). `target/` (Build-Artefakte) gehoeren nicht
# dazu. Die Population ist bewusst breiter als die des 500er-Gates
# (`check_file_sizes.sh` deckt nur `crates/` ab).
#
# Gate-Header (Skill `ratchet-gates`, Invariante 8 — je Defektklasse eine
# Schranke, Klasse, Anlass und Richtung im Header benannt):
#   Defektklasse: oversized unit — eine Rust-Datei ueber 3000 Zeilen; und
#                  die Umkehrung: eine Datei, die einmal unter der Grenze
#                  war und wieder darueber waechst.
#   Anlass:        Eigentuemervorgabe 2026-10-20 (Obergrenze 3000 Zeilen,
#                  „einmal unter der Grenze = dauerhaft darunter"). Ohne
#                  Baseline waere die Schranke dauerhaft rot (eine tote
#                  Schranke, die nach dem zweiten Lauf abgeschaltet wird) oder
#                  eine unerzwungene Empfehlung; ohne Richtung (Tripwire) waere
#                  die Ausnahmeliste eine Liste, deren Werte man erhoehen
#                  kann.
#   Ratchet-Richtung: ceiling — der Baseline-Eintrag (Maximalgroesse je
#                  Datei) darf nur sinken, nie steigen; gesenkt wird er
#                  zusammen mit der Extraktion, die die Datei schrumpft. Die
#                  harte Grenze 3000 hat nur EINE Richtung: sie kann fuer
#                  eine Datei fallen (Eintrag weg) und danach nie wieder
#                  angehoben werden.
#   Nicht gedeckt:  (a) eine COMMITTETE Anhebung — der Tripwire vergleicht
#                  den Arbeitsbaum mit `HEAD`; nach dem Commit sind beide
#                  identisch. Die committete Anhebung wird ueber den DIFF
#                  der Baseline-Datei im Review erkannt (Arbeitsteilung:
#                  Tripwire = Arbeitsbaum, Diff-Review = Historie; die
#                  Aenderung der Baseline ist per Definition ein sichtbares
#                  Ereignis); (b) Nicht-Rust-Dateien (Markdown, Shell, YAML,
#                  TOML) — fuer sie gilt keine Zeilen-Obergrenze; (c) die
#                  500-Zeilen-Wachstumsschranke (`check_file_sizes.sh`) — das
#                  ist eine eigene Klasse mit eigenem Gate; (d) die
#                  QUALITAET einer Datei — der Gate zaehlt Zeilen, er
#                  behauptet nichts ueber sauber getrennte Verantwortlichkeiten
#                  (das ist Slicing-Arbeit, keine Zeilenzahl); (e) die
#                  Population auf Vollstaendigkeit — sie wird aus dem
#                  Dateisystem enumeriert, nicht gegen eine Referenzliste
#                  geprueft; (f) ein fehlendes `git` oder eine noch nicht
#                  committete Baseline — dann LAEUFT (D) nicht, und der Gate
#                  schreibt einen ausdruecklichen HINWEIS statt still
#                  gruen zu sein — und (F) wirkt dann STRENGER: solange keine
#                  committete Baseline lesbar ist, gilt JEDER Eintrag als neu,
#                  also FAILt ein Eintrag ueber dem Ist-Wert (nach dem ersten
#                  Commit waere derselbe Zustand nur (C)-INFO); (g) der Einstiegswert (F)
#                  gilt nur fuer
#                  NEUE Eintraege — ein bereits committeter Eintrag darf ueber
#                  dem Ist-Wert stehen (Regel (C) meldet das als INFO); er ist
#                  die Maximalgroesse, nicht der Ist-Wert.
#
# Arbeitsteilung Tripwire vs. Review (siehe oben (a)): der Tripwire fasst
# eine Anhebung im ARBEITSBAUM (Arbeitsbaum > committeter Wert = FAIL). Die
# COMMITTETE Anhebung sieht er nicht — sie wird ueber den Diff der
# Baseline-Datei im Review erkannt. Deshalb schreibt der Gate die
# Baseline-Datei NIE um: jede Aenderung bleibt ein sichtbares Ereignis.
#
# Der Gate schreibt NICHTS ins Repo (nur lesen), braucht kein Cargo, keinen
# Adapter, kein Netz und kein Build-Artefakt, laeuft in Sekunden und ohne
# Codegen — deshalb gehoert er in die Stufe `fast` (`fast-7`).
#
# ANMERKUNG zum Testhaken `LUMINA_CEILING_REF`: damit kann der Tripwire
# gegen einen anderen Git-Ref als `HEAD` laufen (Rotbeweis gegen eine Kopie
# unter /tmp). Der Default ist IMMER die committete Baseline `HEAD`.
#
# POSIX sh (dash, kein Bash).
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASELINE="$ROOT/scripts/file_size_ceiling.txt"
CEILING=3000
REF="${LUMINA_CEILING_REF:-HEAD}"
REL_BASE="scripts/file_size_ceiling.txt"

fail=0

if [ ! -f "$BASELINE" ]; then
    echo "FEHLER: die Ausnahmeliste fehlt: $BASELINE" >&2
    exit 1
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/check_file_ceiling.XXXXXX") || exit 1
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

# ------------------------------------------- (D) Grundlage: committete Baseline
# Wird VOR dem Population-Scan bestimmt, damit der Hinweis "Pruefung (D) hat
# nicht gelaufen" als erstes im Log steht und nicht untergeht.
committed="$WORK/committed"
have_committed=0
top=""
if command -v git >/dev/null 2>&1; then
    top=$(git -C "$ROOT" rev-parse --show-toplevel 2>/dev/null || true)
fi
if [ -n "${top:-}" ] && [ -d "$top" ]; then
    # Der Pfad in `git show <ref>:<pfad>` ist relativ zur WURZEL des
    # Git-Baums. Im Normalfall (ROOT == Git-Wurzel) ist das der Repo-relative
    # Pfad; liegt das Skript in einem Unterverzeichnis, wird der Rest
    # angehaengt.
    case "$ROOT" in
        "$top") REL_BASE="scripts/file_size_ceiling.txt" ;;
        "$top"/*) REL_BASE="${ROOT#"$top"/}/scripts/file_size_ceiling.txt" ;;
    esac
    if git -C "$ROOT" show "$REF:$REL_BASE" > "$committed" 2>"$WORK/git.err"; then
        have_committed=1
    fi
fi
if [ "$have_committed" -eq 1 ]; then
    awk '$1 ~ /^[0-9]+$/ { print $2, $1 }' "$committed" > "$WORK/committed_map"
else
    : > "$WORK/committed_map"
    # Kein stilles Gruen: der Tripwire (D) hat nicht gelaufen, das wird
    # ausdruecklich gesagt (git fehlt ODER die Baseline ist noch nicht
    # committet ODER der Ref ist unlesbar). Der Hinweis geht nach STDERR, damit
    # er in CI-Logs nicht untergeht; zusaetzlich nennt die Schlusszeile den
    # Zustand, damit eine gruene Zeile nicht missverstanden wird.
    echo "HINWEIS: die committete Baseline ($REF:$REL_BASE) ist nicht lesbar (git nicht verfuegbar, die Datei noch nicht committet oder der Ref unlesbar) -" >&2
    echo "        die Richtung-Pruefung (D) hat NICHT gelaufen; ein angehobener Baseline-Wert waere unentdeckt geblieben. (Die Werte duerfen nur sinken.)" >&2
fi

# ------------------------------------------------------------------ Population
: > "$WORK/population.raw"
for dir in crates agent-harness; do
    if [ -d "$ROOT/$dir" ]; then
        find "$ROOT/$dir" -name '*.rs' -not -path '*/target/*' >> "$WORK/population.raw"
    fi
done
sort "$WORK/population.raw" > "$WORK/population"

# Eintrag je Datei aus der Ausnahmeliste (exakter Pfad-Match, nur Zeilen mit
# numerischem Wertfeld; Kommentarzeilen der Baseline werden ignoriert).
listed() {
    awk -v p="$1" '$1 ~ /^[0-9]+$/ && $2 == p { print $1; exit }' "$BASELINE"
}

# ------------------------------------------------------------------ (A) (B) (C) (E)
while IFS= read -r f; do
    rel="${f#"$ROOT"/}"
    n=$(wc -l < "$f" | tr -d ' ')
    echo "$rel $n" >> "$WORK/sizes"
    b=$(listed "$rel")
    if [ -z "$b" ]; then
        # (A) harte Grenze — die Datei ist "unter der Grenze" und bleibt es.
        if [ "$n" -gt "$CEILING" ]; then
            echo "FAIL: $rel hat $n Zeilen (> $CEILING) und keinen Eintrag in scripts/file_size_ceiling.txt"
            echo "      -> die harte Grenze $CEILING gilt fuer Dateien ohne Eintrag dauerhaft (einmal unter der Grenze = dauerhaft darunter)"
            fail=1
        fi
    else
        # (B) je-Datei-Obergrenze
        if [ "$n" -gt "$b" ]; then
            echo "FAIL: $rel ist von $b auf $n Zeilen gewachsen (erlaubt: $b)"
            echo "      -> extrahiere Code, statt die Obergrenze anzuheben (die Werte duerfen nur sinken)"
            fail=1
        fi
        # (C) Zahlungshinweis: der Eintrag ist zu hoch (Datei geschrumpft)
        if [ "$b" -gt "$n" ]; then
            echo "INFO: $rel ist auf $n Zeilen geschrumpft, der Eintrag steht auf $b - der Eintrag kann auf $n gesenkt werden"
        fi
        # (E) Zahlungspflicht: die Datei ist <= Grenze, ihr Eintrag muss weg
        if [ "$n" -le "$CEILING" ]; then
            echo "FAIL: $rel hat $n Zeilen (<= $CEILING) und steht trotzdem in scripts/file_size_ceiling.txt"
            echo "      -> der Eintrag muss entfernt werden; ab hier gilt die harte Grenze $CEILING dauerhaft"
            fail=1
        fi
    fi
done < "$WORK/population"

# ------------------------------------------------- Eintraege ohne Datei (INFO)
awk '$1 ~ /^[0-9]+$/ { print $1, $2 }' "$BASELINE" > "$WORK/entries"
while read -r v p; do
    [ -n "$p" ] || continue
    if [ ! -f "$ROOT/$p" ]; then
        echo "INFO: $p existiert nicht mehr - der Eintrag ($v) kann entfernt werden"
    fi
done < "$WORK/entries"

# ------------------------------------------- (D) Tripwire + (F) Einstiegswert
# (D) vergleicht den Arbeitsbaum mit der committeten Baseline; (F) bindet
# einen NEUEN Eintrag an den Ist-Wert der Datei (der committeten Baseline fehlt
# der Vergleichswert, sonst waere der Einstiegswert frei waehlbar).
while read -r v p; do
    [ -n "$p" ] || continue
    c=$(awk -v q="$p" '$1 == q { print $2; exit }' "$WORK/committed_map")
    actual=$(awk -v q="$p" '$1 == q { print $2; exit }' "$WORK/sizes")
    if [ -z "$c" ]; then
        # (F) Einstiegswert: ein neuer Eintrag traegt den Ist-Wert der Datei.
        if [ -n "$actual" ] && [ "$v" -gt "$actual" ]; then
            echo "FAIL: $p traegt den Einstiegswert $v, die Datei hat aber $actual Zeilen - der Eintrag muss auf den Ist-Wert gesetzt werden"
            echo "      -> die Obergrenze darf nur sinken; ein zu hoher Einstiegswert waere eine Erlaubnis, die die Datei nie hatte"
            fail=1
        elif [ "$have_committed" -eq 1 ]; then
            echo "INFO: $p ist ein NEUER Ausnahme-Eintrag ($v Zeilen) - er steht nicht in der committeten Baseline;"
            echo "      die Aufnahme ist ein sichtbares Ereignis und wird im Review des Baseline-Diffs bestaetigt"
        fi
    elif [ "$v" -gt "$c" ]; then
        # (D) Anhebung im Arbeitsbaum
        echo "FAIL: $p steht in der committeten Baseline auf $c Zeilen, im Arbeitsbaum auf $v - Baseline-Wert angehoben"
        echo "      -> die Obergrenze darf nur sinken; eine Anhebung gehoert in den ausdruecklichen Review des Baseline-Diffs"
        fail=1
    fi
done < "$WORK/entries"

if [ "$have_committed" -eq 1 ]; then
    # Eintraege, die nur in der committeten Baseline stehen, wurden entfernt:
    # fuer diese Dateien gilt die harte Grenze jetzt dauerhaft.
    while read -r p c; do
        [ -n "$p" ] || continue
        if ! awk -v q="$p" '$2 == q { found = 1 } END { exit (found ? 0 : 1) }' "$WORK/entries"; then
            echo "INFO: der Eintrag fuer $p ($c Zeilen) ist aus der Baseline entfernt - fuer die Datei gilt die harte Grenze $CEILING dauerhaft"
        fi
    done < "$WORK/committed_map"
fi

if [ "$fail" -ne 0 ]; then
    echo ""
    echo "FEHLER: die Zeilen-Obergrenze $CEILING ist verletzt (siehe die FAIL-Zeilen oben)." >&2
    exit 1
fi

d_note=""
if [ "$have_committed" -eq 0 ]; then
    d_note=" - Richtung-Pruefung (D) hat NICHT gelaufen (keine committete Baseline: HINWEIS auf stderr)"
fi
echo "file_ceiling OK: $(wc -l < "$WORK/population" | tr -d ' ') Rust-Datei(en) in der Population, Obergrenze $CEILING Zeilen, $(wc -l < "$WORK/entries" | tr -d ' ') Ausnahme-Eintrag(e).$d_note"
exit 0
