#!/bin/sh
# DOC-ZAHLEN-DRIFT-82 (Welle 1): DRIFT-GATE fuer die Fakten-Bloecke in der
# Prosa.
#
# Zweck: eine Zahl in Prosa ist eine Kopie einer fremd gepflegten Tatsache und
# altert still. Dieser Gate macht die Kopien pruefbar — Muster "generate +
# diff", nicht "wirkt plausibel":
#
#   1) GENERIEREN: `scripts/gen_doc_facts.sh <block-id>` liest die Quelle
#      (Textdatei, Dateisystem oder committetes Lockfile) und liefert den Block-Text.
#   2) EXTRAHIEREN: der committete Block wird aus der Zieldatei gelesen —
#      zwischen `<!-- BEGIN GENERATED: <id> -->` und
#      `<!-- END GENERATED: <id> -->`.
#   3) DIFFEN: beide Seiten werden verglichen; jede Abweichung wird als
#      unified diff ausgegeben (mit Block-Id und Dateiname) und der Gate
#      endet mit Exit != 0.
#
# Der Gate MELDET, er SCHREIBT NICHT: die Zieldatei wird nicht automatisch
# umgeschrieben. Die Aenderung bleibt ein sichtbares, reviewbares Ereignis
# (der Aufrufer — ein Mensch oder ein Agent — regeneriert den Block und
# committet ihn im selben Commit wie die Quellen-Aenderung).
#
# FEHLERFAELLE (alle Exit != 0, alle lauter):
#   * die Zieldatei fehlt              -> FAIL
#   * der Block fehlt in der Datei     -> FAIL (kein BEGIN-Marker)
#   * die Marker sind unbalanciert     -> FAIL (kein END-Marker, zwei
#     BEGIN-Marker, END vor BEGIN)
#   * der Generator kann nicht laufen  -> FAIL (Quelle nicht lesbar, Lockfile
#     fehlt oder unlesbar)
# Ein stilles Gruen waere eine Behauptung ohne Aussage: der Block, der nicht
# gelesen werden kann, ist kein gepruefter Block.
#
# SICHERHEITSREGEL: der Gate startet NICHTS, was er aus einer gelesenen Datei
# gelesen hat. Der Generator liest nur Text; es gibt kein `eval`, kein
# `sh -c` mit extrahierten Werten, kein `source` auf einer Zieldatei. Der
# einzige gestartete Prozess ist `scripts/gen_doc_facts.sh` (eigenes
# Repo-Skript) und der Vergleich `diff`.
#
# ANMERKUNG zum Testhaken `LUMINA_GOLDENS_LOCK`: damit kann der Generator
# gegen ein anderes Lockfile laufen (Rotbeweis "Quelle wandert"). Der Default
# ist IMMER `scripts/golden_ref.lock` des Repos; der Haken aendert nur,
# welche TEXTdatei gelesen wird.
#
# KEIN `git`: weder der Gate noch der Generator brauchen ein Git-Binary. Grund:
# die Stufe `fast` laeuft in der CI im Container `lumina-ci:latest`, in dem
# `git` NICHT installiert ist (CI-Lauf 39459843221, Job „Rust fast checks",
# 2026-10-20). Ein Gate, das `git` voraussetzt, ist dort rot und lokal gruen.
# Die Quelle des `goldens`-Blocks ist deshalb das committete Lockfile
# `scripts/golden_ref.lock`; die Kette zum Repo haelt
# `scripts/golden_ref_test.sh` (im Job „Documentation checks" OHNE Container,
# also mit `git`): es re-derived die Goldens aus dem Git-Index und prueft das
# Lockfile dagegen.
#
# HYGIENE: das Arbeitsverzeichnis (`mktemp -d`) wird per `trap ... EXIT`
# geloescht (SIGINT/SIGTERM ebenso). Bei SIGPIPE (die Ausgabe laeuft in eine
# geschlossene Pipe, z. B. `... | head`) greift der EXIT-Trap nicht und das
# Verzeichnis bleibt unter `${TMPDIR:-/tmp}` liegen — das ist der Normalfall
# einer abgebrochenen Pipe, kein Fehlerpfad des Gates; im Regelbetrieb
# (Ausgabe in eine Datei oder ins Log) raeumt der Trap auf. Kein Umbau
# noetig.
#
# Gate-Header (Skill `ratchet-gates`, Invariante 8):
#   Defektklasse: stille Drift — die committete Prosa nennt Zahlen, die von
#                  den Quellen abgewichen sind (oder die Quellen sind nicht
#                  mehr lesbar und der Block wurde nicht erneuert).
#   Anlass:        DOC-ZAHLEN-DRIFT-82; gemessen am 2026-10-20: `feature/README.md`
#                  nannte 62 committete Goldens, real sind es 66.
#   Ratchet-Richtung: Boolean-Gate (keine Baseline-Datei) — die Abweichung
#                  muss 0 bleiben; es gibt keinen Wert, der steigen darf.
#   Nicht gedeckt:  (a) die Zahlen, die NICHT generiert werden (der R/C-Split
#                  der Goldens, die Oberflaechenzahl im Security-Register,
#                  Testlaufzahlen) — sie sind handgepflegt und altern weiter;
#                  (b) die Auswahl der Zeilen einer generierten Tabelle, wenn
#                  sie regelbasiert nicht ableitbar ist; (c) die Tatsache, ob
#                  die generierten Zahlen inhaltlich richtig sind — der Gate
#                  vergleicht Text, er misst nichts nach.
set -eu

export LC_ALL=C

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GEN="$ROOT/scripts/gen_doc_facts.sh"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/check_doc_facts_drift.XXXXXX") || exit 2
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

[ -f "$GEN" ] || {
    echo "FEHLER: der Generator fehlt: $GEN" >&2
    exit 2
}

# Den Block aus der Zieldatei lesen (ARBEITSBAUM, nicht `HEAD`). Der Block-Text steht zwischen
# den beiden Markern; die Marker muessen zeichengleich sein (kein Whitespace
# drumherum) — ein Marker mit angehaengtem Leerzeichen ist kein Marker.
# Exit: 0 = Block gelesen, 2 = unbalanciert/mehrfach, 3 = kein BEGIN,
# 4 = kein END.
extract_block() {
    file="$1"
    id="$2"
    awk -v id="$id" '
        BEGIN { code = 0; state = 0; seenb = 0; seene = 0 }
        {
            if ($0 == "<!-- BEGIN GENERATED: " id " -->") {
                if (state != 0) {
                    printf "FEHLER: mehrfacher BEGIN-Marker fuer den Block \"%s\"\n", id > "/dev/stderr"
                    code = 2; exit 2
                }
                if (seene) {
                    printf "FEHLER: BEGIN-Marker nach END-Marker fuer den Block \"%s\"\n", id > "/dev/stderr"
                    code = 2; exit 2
                }
                seenb = 1; state = 1; next
            }
            if ($0 == "<!-- END GENERATED: " id " -->") {
                if (state != 1) {
                    printf "FEHLER: END-Marker ohne BEGIN-Marker fuer den Block \"%s\"\n", id > "/dev/stderr"
                    code = 2; exit 2
                }
                seene = 1; state = 2; next
            }
            if (state == 1) print
        }
        END {
            if (code) exit code
            if (!seenb) {
                printf "FEHLER: der Block \"%s\" hat keinen BEGIN-Marker (<!-- BEGIN GENERATED: %s -->)\n", id, id > "/dev/stderr"
                exit 3
            }
            if (!seene) {
                printf "FEHLER: der Block \"%s\" hat keinen END-Marker (<!-- END GENERATED: %s -->)\n", id, id > "/dev/stderr"
                exit 4
            }
        }
    ' "$file"
}

# ------------------------------------------------------------------ Manifest
if ! sh "$GEN" --manifest > "$WORK/manifest" 2> "$WORK/manifest.err"; then
    echo "FEHLER: das Manifest des Generators ist nicht lesbar:" >&2
    sed 's/^/  /' "$WORK/manifest.err" >&2
    exit 2
fi
if [ ! -s "$WORK/manifest" ]; then
    echo "FEHLER: das Manifest des Generators ist leer - es gibt nichts zu pruefen." >&2
    exit 2
fi

findings=0
checked=0

while IFS='|' read -r id file; do
    [ -n "$id" ] || continue
    [ -n "$file" ] || continue
    checked=$((checked + 1))

    # 1) Zieldatei
    if [ ! -f "$ROOT/$file" ]; then
        echo "FEHLER: Zieldatei fehlt: $file (Block \"$id\")" >&2
        findings=$((findings + 1))
        continue
    fi

    # 2) generieren
    if ! sh "$GEN" "$id" > "$WORK/generated" 2> "$WORK/generated.err"; then
        echo "FEHLER: der Generator konnte den Block \"$id\" nicht bilden ($file):" >&2
        sed 's/^/  /' "$WORK/generated.err" >&2
        findings=$((findings + 1))
        continue
    fi

    # 3) Block aus der Zieldatei extrahieren (Arbeitsbaum)
    if ! extract_block "$ROOT/$file" "$id" > "$WORK/committed" 2> "$WORK/committed.err"; then
        echo "FEHLER: der Block \"$id\" in $file ist nicht lesbar:" >&2
        sed 's/^/  /' "$WORK/committed.err" >&2
        findings=$((findings + 1))
        continue
    fi

    # 4) diffen
    rc=0
    diff -u -L "committed: $file (Block $id)" -L "generated: $file (Block $id)" \
        "$WORK/committed" "$WORK/generated" > "$WORK/diff" 2>&1 || rc=$?
    if [ "$rc" -eq 1 ]; then
        echo "FEHLER: Prosa-Drift im Block \"$id\" ($file) — der committete Block weicht vom Generator ab:" >&2
        sed 's/^/  /' "$WORK/diff" >&2
        echo "" >&2
        echo "  Der Block wird NICHT automatisch umgeschrieben (der Gate meldet nur)." >&2
        echo "  Neu erzeugen: sh scripts/gen_doc_facts.sh $id" >&2
        findings=$((findings + 1))
        continue
    fi
    if [ "$rc" -gt 1 ]; then
        echo "FEHLER: der Vergleich des Blocks \"$id\" ($file) ist fehlgeschlagen:" >&2
        sed 's/^/  /' "$WORK/diff" >&2
        findings=$((findings + 1))
        continue
    fi

    echo "ok: Block \"$id\" in $file stimmt mit dem Generator ueberein."
done < "$WORK/manifest"

if [ "$findings" -ne 0 ]; then
    echo "" >&2
    echo "FEHLER: $findings von $checked Block(en) weichen vom Generator ab." >&2
    exit 1
fi

files=$(cut -d'|' -f2 "$WORK/manifest" | sort -u | grep -c . || true)
echo "doc_facts OK: $checked Block(e) in $files Datei(en) stimmen mit dem Generator ueberein."
exit 0
