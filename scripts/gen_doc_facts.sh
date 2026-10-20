#!/bin/sh
# DOC-ZAHLEN-DRIFT-82 (Welle 1): GENERATOR der Fakten-Bloecke in der Prosa.
#
# Zweck: „Eine Zahl in Prosa ist eine Kopie einer fremd gepflegten Tatsache
# und altert still." (Skill `generated-docs`.) Die Loesung ist nicht besseres
# Pflegen, sondern **erzeugen + diffen**: dieser Generator liest die Quellen
# (Textdateien, Dateisystem, Git-Index) und liefert den fertigen Block-Text;
# `scripts/check_doc_facts_drift.sh` vergleicht ihn mit dem Block aus der Zieldatei
# in der Zieldatei und wird bei jeder Abweichung rot. Muster wie bei
# `scripts/gen_ci_tiers.sh` + `scripts/check_ci_tier_drift.sh`.
#
# SICHERHEITSREGEL: der Generator liest NUR TEXT und fuehrt NICHTS aus, was er
# aus einem gelesenen Dokument gelesen hat — kein `eval`, kein `sh -c` mit
# extrahierten Werten, kein `source` auf einer gelesenen Datei. Die gelesenen
# Dateien sind Eingabedokumente; wer ein daraus gelesenes Kommando ausfuehrt,
# baut aus einer Pruefung ein Ausfuehrungs-Freigabegate. Ausgefuehrt werden
# ausschlich Festplatzwerkzeuge (`wc`, `find`, `sort`, `git ls-files`, `awk`,
# `sed`) mit festen Argumenten.
#
# KEINE handgepflegte Zahlen- oder Faktenliste: der Generator enthaelt keine
# Datei-, Zeilen- oder Mengenangabe. Er enthaelt nur
#   * das MANIFEST (Block-Id -> Zieldatei) — eine Zuordnung, keine Tatsache,
#   * die Pfade der Quellen,
#   * die beiden Schwellen, die er aus den GATE-Skripten liest (500 aus
#     `scripts/check_file_sizes.sh`, 3000 aus `scripts/check_file_ceiling.sh`),
#     damit die Werte nicht an zwei Stellen stehen.
# Die Zeilenauswahl (welche Dateien in einen Block gehoeren) ist regelbasiert:
#   * `file-sizes`  — jede Zeile der Ausnahmeliste `scripts/file_size_ceiling.txt`,
#                     deren erstes Feld eine Zahl ist (die Kommentarzeilen der
#                     Liste fallen damit automatisch weg).
#   * `population`  — der Population des Gates `scripts/check_file_ceiling.sh`
#                     (alle `.rs` unter `crates/` und `agent-harness/`, ohne
#                     `target/`), gzaehlt und gemessen.
#   * `goldens`     — `git ls-files` ueber das Snapshot-Verzeichnis; NICHT `ls`
#                     (im Verzeichnis liegen gitignorierte `*.new.png`-
#                     Artefakte, die ein Verzeichnis-Scan mitzaehlen wuerde).
#
# DETERMINISTISCH: `LC_ALL=C`, feste Sortierung, keine Zeitstempel, keine
# absoluten Pfade, kein Hostname. Zweimal laufen lassen -> byteweise identische
# Ausgabe. Ein Gate, das bei zwei Laeufen unterschiedliche Ausgaben liefert,
# ist tot.
#
# HYGIENE: das Arbeitsverzeichnis (`mktemp -d`) wird per `trap ... EXIT`
# geloescht (SIGINT/SIGTERM ebenso).
#
# Aufruf:
#   sh scripts/gen_doc_facts.sh                 # alle Bloecke, mit Markern
#   sh scripts/gen_doc_facts.sh <block-id>      # der Block-Text (ohne Marker)
#   sh scripts/gen_doc_facts.sh --manifest      # "block-id|zieldatei" je Zeile
#   sh scripts/gen_doc_facts.sh --list          # die bekannten Block-Ids
#
# Exit: 0 = Ausgabe auf stdout. 2 = Fehler (Quelle nicht lesbar, Git nicht
# da, Konstante nicht gefunden). Der Drift-Gate behandelt Exit 2 als Fehler
# (rot) — eine Quelle, die nicht gelesen werden kann, ist kein gruener Zustand.
set -eu

export LC_ALL=C

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Quellen
CEILING_LIST="$ROOT/scripts/file_size_ceiling.txt"
SIZE_BASELINE="$ROOT/scripts/file_size_baseline.txt"
SIZE_GATE="$ROOT/scripts/check_file_sizes.sh"
CEILING_GATE="$ROOT/scripts/check_file_ceiling.sh"
GOLDEN_DIR_REL="crates/lumina-gui/tests/snapshots"
GOLDEN_PATTERN="${LUMINA_GOLDENS_PATTERN:-crates/lumina-gui/tests/snapshots/*.png}"

# MANIFEST: Block-Id | Zieldatei (relativ zur Repo-Wurzel).
# Eine Quelle der Wahrheit fuer Generator UND Drift-Gate: der Gate liest das
# Manifest hier, damit ein fehlender erwarteter Block ein Fehler ist und kein
# stilles Uebersehen.
MANIFEST='file-sizes|feature/quality/file-size-policy.md
population|feature/quality/file-size-policy.md
goldens|feature/README.md
goldens|feature/quality/golden-references.md
goldens|feature/quality/golden-fixtures.md'

BLOCK_IDS='file-sizes
population
goldens'

WORK=$(mktemp -d "${TMPDIR:-/tmp}/gen_doc_facts.XXXXXX") || exit 2
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

TAB=$(printf '\t')

die() {
    echo "gen_doc_facts: FEHLER: $*" >&2
    exit 2
}

# read_const <datei> <name> — Wert der Konstante <name>=<zahl> aus <datei>.
# Liest die Schwellen aus den GATE-Skripten, damit sie nicht an zwei Stellen
# im Repo stehen. Findet er die Konstante nicht, ist das ein Fehler (Exit 2),
# kein stiller Ersatzwert.
read_const() {
    file="$1"
    name="$2"
    [ -f "$file" ] || die "Datei nicht lesbar: $file"
    val=$(sed -n "s/^${name}=\([0-9][0-9]*\)\$/\1/p" "$file" | head -n 1)
    [ -n "$val" ] || die "die Konstante $name wurde in ${file##*/} nicht gefunden"
    printf '%s\n' "$val"
}

# ---------------------------------------------------------------- file-sizes
# Quelle: `scripts/file_size_ceiling.txt` (die Ausnahmeliste) fuer die
# Auswahl und den Eintrag, `wc -l` fuer den aktuellen Stand, und
# `scripts/file_size_baseline.txt` (die 500er-Liste) fuer den Vergleichs-
# wert. Die Differenz zwischen den Spalten ist der Drift-Fall: die Datei
# ist kleiner als der Eintrag (Eintrag zu hoch) oder groesser (Eintrag
# gerissen).
gen_file_sizes() {
    [ -f "$CEILING_LIST" ] || die "Ausnahmeliste nicht lesbar: $CEILING_LIST"
    [ -f "$SIZE_BASELINE" ] || die "500er-Baseline nicht lesbar: $SIZE_BASELINE"

    printf '| Datei | Ist-Stand (Zeilen) | Eintrag `scripts/file_size_ceiling.txt` | Eintrag `scripts/file_size_baseline.txt` |\n'
    printf '| --- | --- | --- | --- |\n'

    # Regelbasierte Auswahl: jede Zeile der Ausnahmeliste, deren erstes
    # Feld eine Zahl ist (die Kommentarzeilen fallen damit weg).
    awk -v tab="$TAB" '$1 ~ /^[0-9]+$/ && NF >= 2 { print $2 tab $1 }' "$CEILING_LIST" > "$WORK/entries"
    [ -s "$WORK/entries" ] || die "die Ausnahmeliste enthaelt keinen verwertbaren Eintrag"

    while IFS="$TAB" read -r path value; do
        [ -n "$path" ] || continue
        [ -f "$ROOT/$path" ] || die "Datei aus der Ausnahmeliste existiert nicht: $path"
        now=$(wc -l < "$ROOT/$path" | tr -d ' ')
        base=$(awk -v p="$path" '$1 ~ /^[0-9]+$/ && $2 == p { print $1; exit }' "$SIZE_BASELINE")
        [ -n "$base" ] || base='-'
        printf '| `%s` | %s | %s | %s |\n' "$path" "$now" "$value" "$base"
    done < "$WORK/entries"
}

# ---------------------------------------------------------------- population
# Quelle: das Dateisystem — dieselbe Population, die `scripts/check_file_ceiling.sh`
# enumeriert (alle `.rs` unter `crates/` und `agent-harness/`, ohne `target/`).
# Die Schwellen (500 und 3000) werden aus den beiden GATE-Skripten gelesen.
gen_population() {
    threshold=$(read_const "$SIZE_GATE" THRESHOLD)
    ceiling=$(read_const "$CEILING_GATE" CEILING)

    : > "$WORK/pop.raw"
    for dir in crates agent-harness; do
        if [ -d "$ROOT/$dir" ]; then
            find "$ROOT/$dir" -name '*.rs' -not -path '*/target/*' -exec wc -l {} + >> "$WORK/pop.raw"
        fi
    done

    # Die `total`-Zeilen von `wc` (je Batch) fallen weg; der Rest wird nach
    # Pfad sortiert (feste Reihenfolge, unabhaengig vom Dateisystem). Der Pfad
    # wird als REST der Zeile gelesen (nicht als Feld), damit ein Pfad mit
    # Leerzeichen die Population nicht verkleinert.
    awk -v tab="$TAB" '{ n = $1; $1 = ""; sub(/^[ \t]+/, "");
        if (n ~ /^[0-9]+$/ && $0 != "total") print n tab $0 }' "$WORK/pop.raw" |
        LC_ALL=C sort > "$WORK/pop"

    total=$(wc -l < "$WORK/pop" | tr -d ' ')
    over_threshold=0
    over_ceiling=0
    while IFS="$TAB" read -r n path; do
        [ -n "$path" ] || continue
        if [ "$n" -gt "$threshold" ]; then
            over_threshold=$((over_threshold + 1))
        fi
        if [ "$n" -gt "$ceiling" ]; then
            over_ceiling=$((over_ceiling + 1))
        fi
    done < "$WORK/pop"

    printf 'Population (`crates/` + `agent-harness/`, alle `.rs`-Dateien): **%s** Dateien, davon\n' "$total"
    printf '**%s** ueber %s Zeilen und **%s** ueber %s Zeilen.\n' \
        "$over_threshold" "$threshold" "$over_ceiling" "$ceiling"
}

# ------------------------------------------------------------------- goldens
# Quelle: der Git-Index (`git ls-files`) — die committeten Golden-PNGs.
# KEIN `ls`: im Snapshot-Verzeichnis liegen gitignorierte `*.new.png`-
# Artefakte (Vergleichsartefakte des Testlaufs); ein Verzeichnis-Scan
# zaehlt sie mit und liefert eine falsche Zahl. Fehlt `git` oder der
# Arbeitsbaum, MELDET der Generator das (Exit 2) — er liefert NICHT still 0.
gen_goldens() {
    command -v git >/dev/null 2>&1 || die "git fehlt - die Zahl der committeten Goldens ist nicht ermittelbar"
    git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1 ||
        die "kein Git-Arbeitsbaum unter $ROOT - die Zahl der committeten Goldens ist nicht ermittelbar"

    git -C "$ROOT" ls-files "$GOLDEN_PATTERN" > "$WORK/goldens.raw" 2> "$WORK/goldens.err" ||
        die "git ls-files '$GOLDEN_PATTERN' ist fehlgeschlagen: $(cat "$WORK/goldens.err")"

    # Vergleichsartefakte (`*.new.png`, `*.diff.png`, `*.old.png`) sind nie
    # committet; die Regel ist trotzdem explizit, damit ein committetes
    # Artefakt nicht mitzaehlt (dieselbe Regel wie in `golden_ref_test.sh`).
    awk '!/\.new\.png$/ && !/\.diff\.png$/ && !/\.old\.png$/' "$WORK/goldens.raw" > "$WORK/goldens"

    n=$(wc -l < "$WORK/goldens" | tr -d ' ')
    [ "$n" -gt 0 ] || die "die Liste der committeten Goldens ('$GOLDEN_PATTERN') ist leer - das ist kein gueltiger Stand"

    printf '**%s** committete Golden-Dateien in `%s/` (gezählt mit\n' "$n" "$GOLDEN_DIR_REL"
    cmd="git ls-files '$GOLDEN_PATTERN'"
    printf '`%s`; dieselbe Zahl meldet\n' "$cmd"
    printf '`golden_ref.sh check` als `goldens.count`).\n'
}

# --------------------------------------------------------------------- Ausgabe
gen_block() {
    case "$1" in
        file-sizes) gen_file_sizes ;;
        population) gen_population ;;
        goldens)    gen_goldens ;;
        *)          die "unbekannter Block: $1" ;;
    esac
}

emit_block() {
    id="$1"
    printf '<!-- BEGIN GENERATED: %s -->\n' "$id"
    gen_block "$id"
    printf '<!-- END GENERATED: %s -->\n' "$id"
}

usage() {
    cat <<'EOF'
Usage: sh scripts/gen_doc_facts.sh [<block-id> | --manifest | --list]

  <block-id>  Der Block-Text OHNE Marker auf stdout (das ist die Form, die
              der Drift-Gate gegen den Block aus der Zieldatei difft).
  --manifest  "block-id|zieldatei" je Zeile (die Zuordnung Block -> Datei).
  --list      Die bekannten Block-Ids, je einer je Zeile.
  (ohne Arg)  Alle Bloecke, jeweils mit BEGIN-/END-Marker, in der Reihenfolge
              des Manifests — die Form zum Einfuegen in eine Zieldatei.

Exit 0 = Ausgabe auf stdout. Exit 2 = Fehler (Quelle nicht lesbar, kein Git,
Konstante nicht gefunden).
EOF
}

case "${1:-}" in
    --manifest)
        printf '%s\n' "$MANIFEST"
        ;;
    --list)
        printf '%s\n' "$BLOCK_IDS"
        ;;
    --help|-h)
        usage
        ;;
    '')
        # Alle Bloecke, je Block-Id EINMAL (die Zuordnung zu den Zieldateien
        # steht im Manifest; der Block-Text ist davon unabhaengig).
        printf '%s\n' "$BLOCK_IDS" | while IFS= read -r id; do
            [ -n "$id" ] || continue
            emit_block "$id"
            printf '\n'
        done
        ;;
    *)
        printf '%s\n' "$BLOCK_IDS" | grep -qx -e "$1" ||
            die "unbekannter Block: $1 (siehe --list)"
        gen_block "$1"
        ;;
esac
