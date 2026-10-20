#!/bin/sh
# DOC-ZAHLEN-DRIFT-82 (Welle 1): GENERATOR der Fakten-Bloecke in der Prosa.
#
# Zweck: „Eine Zahl in Prosa ist eine Kopie einer fremd gepflegten Tatsache
# und altert still." (Skill `generated-docs`.) Die Loesung ist nicht besseres
# Pflegen, sondern **erzeugen + diffen**: dieser Generator liest die Quellen
# (Textdateien, Dateisystem, committete Lockfiles) und liefert den fertigen
# Block-Text; `scripts/check_doc_facts_drift.sh` vergleicht ihn mit dem Block
# aus der Zieldatei und wird bei jeder Abweichung rot. Muster wie bei
# `scripts/gen_ci_tiers.sh` + `scripts/check_ci_tier_drift.sh`.
#
# SICHERHEITSREGEL: der Generator liest NUR TEXT und fuehrt NICHTS aus, was er
# aus einem gelesenen Dokument gelesen hat — kein `eval`, kein `sh -c` mit
# extrahierten Werten, kein `source` auf einer gelesenen Datei. Die gelesenen
# Dateien sind Eingabedokumente; wer ein daraus gelesenes Kommando ausfuehrt,
# baut aus einer Pruefung ein Ausfuehrungs-Freigabegate. Ausgefuehrt werden
# ausschlich Festplatzwerkzeuge (`wc`, `find`, `sort`, `awk`, `sed`) mit festen
# Argumenten.
#
# KEIN `git`: der Generator braucht **kein** Git-Binary. Grund: die Stufe
# `fast` laeuft in der CI im Container `lumina-ci:latest`, in dem `git` NICHT
# installiert ist (die `checkout`-Action bringt ihr eigenes Node mit, aber kein
# `git`). Ein Generator, der `git ls-files` braucht, ist im Container rot und
# lokal gruen — genau die container-spezifische Luecke, die eine lokale
# Verifikation nicht sieht (CI-Lauf 39459843221, Job „Rust fast checks",
# 2026-10-20). Der Block-Text MUSS in beiden Umgebungen byteweise identisch
# sein; deshalb ist die Quelle **immer** dieselbe committete Datei — nicht
# „git, wenn verfuegbar, sonst Datei".
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
#   * `goldens`     — dem committeten Lockfile `scripts/golden_ref.lock`
#                     (Feld `goldens.count`). Das Lockfile ist die committete
#                     Baseline der Golden-Zahl — genau das Muster der anderen
#                     Baselines im Repo: eine committete Datei mit der Zahl, ein
#                     Gate, das die Doku dagegen prueft. Die Kette zum Repo
#                     bleibt geschlossen: `scripts/golden_ref_test.sh`
#                     re-derived die Goldens aus dem Git-Index und prueft das
#                     Lockfile dagegen (im Job „Documentation checks" OHNE
#                     Container, also mit `git`). Ein Verzeichnis-Scan (`ls`)
#                     waere ausgeschlossen: im Snapshot-Verzeichnis liegen
#                     gitignorierte `*.new.png`-Artefakte, die ein Scan
#                     mitzaehlen wuerde (mehr als 66 — die Zahl der liegengebliebenen Artefakte ist hostabhaengig).
#
# DETERMINISTISCH: `LC_ALL=C`, feste Sortierung, keine Zeitstempel, keine
# absoluten Pfade, kein Hostname. Zweimal laufen lassen -> byteweise identische
# Ausgabe. Ein Gate, das bei zwei Laeufen unterschiedliche Ausgaben liefert,
# ist tot.
#
# HYGIENE: das Arbeitsverzeichnis (`mktemp -d`) wird per `trap ... EXIT`
# geloescht (SIGINT/SIGTERM ebenso). Bei SIGPIPE (die Ausgabe laeuft in eine
# geschlossene Pipe, z. B. `... | head`) greift der EXIT-Trap nicht und das
# Verzeichnis bleibt unter `${TMPDIR:-/tmp}` liegen — das ist der Normalfall
# einer abgebrochenen Pipe, kein Fehlerpfad; im Regelbetrieb raeumt der Trap
# auf. Kein Umbau noetig.
#
# Aufruf:
#   sh scripts/gen_doc_facts.sh                 # alle Bloecke, mit Markern
#   sh scripts/gen_doc_facts.sh <block-id>      # der Block-Text (ohne Marker)
#   sh scripts/gen_doc_facts.sh --manifest      # "block-id|zieldatei" je Zeile
#   sh scripts/gen_doc_facts.sh --list          # die bekannten Block-Ids
#
# Testhaken: `LUMINA_GOLDENS_LOCK` ueberschreibt den Pfad des Lockfiles, aus
# dem der `goldens`-Block liest (Default: IMMER `scripts/golden_ref.lock` des
# Repos). Er existiert nur fuer den Rotbeweis „Quelle wandert"; er aendert
# nur, welche TEXTdatei gelesen wird.
#
# Exit: 0 = Ausgabe auf stdout. 2 = Fehler (Quelle nicht lesbar, Lockfile
# fehlt oder unlesbar, Konstante nicht gefunden). Der Drift-Gate behandelt
# Exit 2 als Fehler (rot) — eine Quelle, die nicht gelesen werden kann, ist
# kein gruener Zustand.
set -eu

export LC_ALL=C

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Quellen
CEILING_LIST="$ROOT/scripts/file_size_ceiling.txt"
SIZE_BASELINE="$ROOT/scripts/file_size_baseline.txt"
SIZE_GATE="$ROOT/scripts/check_file_sizes.sh"
CEILING_GATE="$ROOT/scripts/check_file_ceiling.sh"
GOLDEN_DIR_REL="crates/lumina-gui/tests/snapshots"
GOLDEN_LOCK="${LUMINA_GOLDENS_LOCK:-$ROOT/scripts/golden_ref.lock}"

# MANIFEST: Block-Id | Zieldatei (relativ zur Repo-Wurzel).
# Eine Quelle der Wahrheit fuer Generator UND Drift-Gate: der Gate liest das
# Manifest hier, damit ein fehlender erwarteter Block ein Fehler ist und kein
# stilles Uebersehen.
MANIFEST='file-sizes|feature/quality/file-size-policy.md
population|feature/quality/file-size-policy.md
goldens|feature/README.md
goldens|feature/quality/golden-references.md
goldens|feature/quality/golden-fixtures.md'

# Die Block-Ids werden AUS DEM MANIFEST abgeleitet (keine zweite Liste): eine
# Block-Id, die nur in einer eigenen Liste stuende, wuerde nie erzeugt und nie
# geprueft — eine stille Luecke (L-1 aus dem Verifikationsbericht).
manifest_ids() {
    printf '%s\n' "$MANIFEST" |
        awk -F'|' 'NF == 2 && $1 != "" && !seen[$1]++ { print $1 }'
}

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
# Quelle: das COMMITTEDTE Lockfile `scripts/golden_ref.lock`, Feld
# `goldens.count` — die committete Baseline der Golden-Zahl (Muster der
# Baselines im Repo: committete Datei mit der Zahl + Gate, das die Doku
# dagegen prueft).
#
# Warum NICHT `git ls-files`: (a) der CI-Container `lumina-ci:latest` hat kein
# `git`-Binary — der Generator waere dort rot und lokal gruen; (b) die Quelle
# muss in JEDER Umgebung dieselbe sein, sonst waere der Block-Text nicht
# byteweise gleich und der Drift-Gate in einer der Umgebungen rot.
# Warum NICHT `ls`: im Snapshot-Verzeichnis liegen gitignorierte
# `*.new.png`-Artefakte, die ein Verzeichnis-Scan mitzaehlen wuerde
# (mehr als 66 — die Zahl der liegengebliebenen Artefakte ist hostabhaengig).
# Die Kette zum Repo bleibt geschlossen: `scripts/golden_ref_test.sh`
# re-derived die Goldens aus dem Git-Index und prueft das Lockfile dagegen
# (Zeile 725: „inventory/goldens.count covers every committed golden"), und
# `scripts/golden_ref.sh check` vergleicht es zur Laufzeit.
# Fehlt das Lockfile oder steht kein gueltiger Wert drin, MELDET der Generator
# das (Exit 2) — er liefert NICHT still 0.
gen_goldens() {
    [ -f "$GOLDEN_LOCK" ] ||
        die "das Lockfile fehlt: $GOLDEN_LOCK - die Zahl der committeten Goldens ist nicht ermittelbar"
    [ -r "$GOLDEN_LOCK" ] ||
        die "das Lockfile ist nicht lesbar: $GOLDEN_LOCK - die Zahl der committeten Goldens ist nicht ermittelbar"

    n=$(awk -F= '$1 == "goldens.count" { print $2; exit }' "$GOLDEN_LOCK")
    case "$n" in
        '') die "im Lockfile steht kein Feld goldens.count: $GOLDEN_LOCK" ;;
        *[!0-9]*) die "im Lockfile steht kein gueltiger Wert fuer goldens.count: $GOLDEN_LOCK" ;;
    esac
    [ "$n" -gt 0 ] || die "im Lockfile steht goldens.count=0 - das ist kein gueltiger Stand"

    printf '**%s** committete Golden-Dateien in `%s/` (Quelle:\n' "$n" "$GOLDEN_DIR_REL"
    printf '`scripts/golden_ref.lock`, Feld `goldens.count`; `scripts/golden_ref_test.sh`\n'
    printf 'prueft dieses Lockfile gegen den Git-Index, `golden_ref.sh check` gegen die Laufzeit).\n'
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

Exit 0 = Ausgabe auf stdout. Exit 2 = Fehler (Quelle nicht lesbar, Lockfile
fehlt, Konstante nicht gefunden).
EOF
}

case "${1:-}" in
    --manifest)
        printf '%s\n' "$MANIFEST"
        ;;
    --list)
        manifest_ids
        ;;
    --help|-h)
        usage
        ;;
    '')
        # Alle Bloecke, je Block-Id EINMAL, in der Reihenfolge des Manifests
        # (die Zuordnung zu den Zieldateien steht im Manifest; der Block-Text
        # ist davon unabhaengig).
        manifest_ids | while IFS= read -r id; do
            [ -n "$id" ] || continue
            emit_block "$id"
            printf '\n'
        done
        ;;
    *)
        manifest_ids | grep -qx -e "$1" ||
            die "unbekannter Block: $1 (siehe --list)"
        gen_block "$1"
        ;;
esac
