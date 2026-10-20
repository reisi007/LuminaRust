#!/bin/sh
# DOC-ZAHLEN-DRIFT-82 (Welle 2): HYGIENE-GATE fuer den Generator der
# Fakten-Bloecke (`scripts/gen_doc_facts.sh`).
#
# Zweck: Welle 2 hat die HARTE REGEL eingefuehrt: **jeder Block muss git-frei
# sein**. Grund: die Stufe `fast` laeuft in der CI im Container
# `ghcr.io/reisi007/luminarust/lumina-ci:latest`, in dem `git` NICHT
# installiert ist (CI-Lauf 39459843221, Job „Rust fast checks", 2026-10-20).
# Ein Generator, der `git ls-files` braucht, ist dort rot und lokal gruen —
# genau die container-spezifische Luecke, die eine lokale Verifikation nicht
# sieht. Ein Block, der nur in einer Umgebung erzeugbar ist, ist kein Gate,
# sondern eine Behauptung.
#
# Was dieser Gate prueft:
#   1) SYNTAX: `sh -n` auf dem Generator (und auf sich selbst).
#   2) VOLLSTAENDIGKEIT: jede Block-Id aus dem Manifest des Generators ist
#      erzeugbar (eine Id ohne Implementierung waere eine stille Luecke).
#   3) ZIELVORHANDEN: jede Zieldatei aus dem Manifest existiert.
#   4) DETERMINISMUS: zwei Laeufe des Generators liefern byteweise dieselbe
#      Ausgabe (sonst waere der Drift-Gate environmentabhaengig).
#   5) GIT-FREIHEIT: derselbe Lauf in einer PATH-Sandbox **ohne** `git*`
#      liefert byteweise dieselbe Ausgabe. Der Generator darf also kein
#      Git-Binary brauchen — nachgewiesen durch das Verhalten, nicht durch
#      ein Grep auf seinen Quelltext (dort steht `git` in den Kommentaren).
#
# KEIN `git`, kein `python3`, kein `node`: nur sh/awk/sed/grep/wc/tr/sort/
# cut/mktemp/diff/find/ln/rm.
#
# Der Generator wird NIE mit Werten aus einem gelesenen Dokument gefuehrt:
# der Gate startet ausschliesslich `sh scripts/gen_doc_facts.sh` mit
# festen Argumenten (`--manifest`, `--list`, `<block-id>`) — der Inhalt der
# gelesenen Dateien fliesst nie in eine Kommandozeile.
#
# Gate-Header (Skill `ratchet-gates`, Invariante 8):
#   Defektklasse: stille Drift — ein Block, der nur in einer Umgebung
#                  erzeugbar ist (im CI-Container ohne `git` rot, lokal gruen),
#                  oder ein Generator, der bei zwei Laeufen unterschiedliche
#                  Ausgaben liefert (der Drift-Gate waere dann environment-
#                  abhaengig und koennte rot UND gruen werden).
#   Anlass:        DOC-ZAHLEN-DRIFT-82 (Welle 2, 2026-10-20): die Bloecke
#                  `mcp-tools`, `perf-baseline`, `crates` und `surfaces` sind
#                  dazugekommen; jeder von ihnen muss git-frei bleiben.
#   Ratchet-Richtung: Boolean-Gate (keine Baseline-Datei) — die Abweichung
#                  muss 0 bleiben; es gibt keinen Wert, der steigen darf.
#   Nicht gedeckt:  (a) die SACH-RICHTIGKEIT der erzeugten Zahlen (der Gate
#                  vergleicht Text, er misst nichts nach); (b) die Coverage der
#                  Prosa (welche Dokumente ihre Zahlen erwaehnen) — das ist die
#                  Aufgabe des Drift-Gates `fast-8`; (c) ein Generator, der
#                  `git` *nennt*, aber nicht braucht — das ist harmless, weil
#                  der Nachweis hier ueber das Verhalten laeuft.
set -eu

export LC_ALL=C

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GEN="$ROOT/scripts/gen_doc_facts.sh"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/check_doc_facts_generator.XXXXXX") || exit 2
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

findings=0

say() { echo "$*"; }
bad() {
    echo "FEHLER: $*" >&2
    findings=$((findings + 1))
}

[ -f "$GEN" ] || {
    echo "FEHLER: der Generator fehlt: $GEN" >&2
    exit 2
}

# ---------------------------------------------------------------- 1) Syntax
if sh -n "$GEN" 2> "$WORK/syntax.err"; then
    say "ok: der Generator besteht die Syntaxpruefung (sh -n)."
else
    bad "der Generator besteht die Syntaxpruefung (sh -n) nicht:"
    sed 's/^/  /' "$WORK/syntax.err" >&2
fi
if sh -n "$ROOT/scripts/check_doc_facts_generator.sh" 2> "$WORK/syntax2.err"; then
    say "ok: dieser Gate besteht die Syntaxpruefung (sh -n)."
else
    bad "dieser Gate besteht die Syntaxpruefung (sh -n) nicht:"
    sed 's/^/  /' "$WORK/syntax2.err" >&2
fi

# ------------------------------------------------- 2) Manifest + 3) Ziele
if ! sh "$GEN" --manifest > "$WORK/manifest" 2> "$WORK/manifest.err"; then
    bad "das Manifest des Generators ist nicht lesbar:"
    sed 's/^/  /' "$WORK/manifest.err" >&2
else
    if [ ! -s "$WORK/manifest" ]; then
        bad "das Manifest des Generators ist leer — es gibt nichts zu pruefen."
    else
        while IFS='|' read -r id file; do
            [ -n "$id" ] || continue
            [ -n "$file" ] || continue
            if [ ! -f "$ROOT/$file" ]; then
                bad "Zieldatei fehlt: $file (Block \"$id\")"
            fi
        done < "$WORK/manifest"
        say "ok: alle Zieldateien des Manifests existieren."
    fi
fi

# --------------------------------------- 2b) jede Block-Id ist erzeugbar
if sh "$GEN" --list > "$WORK/ids" 2> "$WORK/ids.err"; then
    ids=0
    while IFS= read -r id; do
        [ -n "$id" ] || continue
        ids=$((ids + 1))
        if ! sh "$GEN" "$id" > "$WORK/block" 2> "$WORK/block.err"; then
            bad "der Block \"$id\" ist nicht erzeugbar (der Generator endet mit Exit != 0):"
            sed 's/^/  /' "$WORK/block.err" >&2
        fi
    done < "$WORK/ids"
    if [ "$ids" -gt 0 ]; then
        say "ok: alle $ids Block-Ids des Manifests sind erzeugbar."
    else
        bad "das Manifest nennt keine Block-Id."
    fi
else
    bad "die Block-Ids des Generators sind nicht lesbar:"
    sed 's/^/  /' "$WORK/ids.err" >&2
fi

# ------------------------------------------------------- 4) Determinismus
if sh "$GEN" > "$WORK/run1" 2> "$WORK/run1.err"; then
    if sh "$GEN" > "$WORK/run2" 2> "$WORK/run2.err"; then
        if diff -q "$WORK/run1" "$WORK/run2" >/dev/null 2>&1; then
            say "ok: zwei Laeufe des Generators liefern byteweise dieselbe Ausgabe."
        else
            bad "zwei Laeufe des Generators liefern unterschiedliche Ausgaben (nicht deterministisch):"
            diff -u "$WORK/run1" "$WORK/run2" 2>&1 | sed 's/^/  /' >&2
        fi
    else
        bad "der zweite Lauf des Generators ist fehlgeschlagen:"
        sed 's/^/  /' "$WORK/run2.err" >&2
    fi
else
    bad "der erste Lauf des Generators ist fehlgeschlagen:"
    sed 's/^/  /' "$WORK/run1.err" >&2
fi

# --------------------------------------------- 5) git-freie PATH-Sandbox
# Die Sandbox enthaelt SYMLINKS auf alle ausfuehrbaren Dateien des PATH —
# ausser den `git*`-Werkzeugen. Damit ist der Nachweis ein Verhaltens-Nachweis:
# der Generator erzeugt denselben Block, ohne ein Git-Binary zu haben.
mkdir -p "$WORK/nogit"
# `printf '%s\n'` (mit Newline): `tr` ersetzt nur die Trennzeichen — ohne
# Newline haette die LETZTE PATH-Komponente kein Zeilenende und wuerde von
# `read` nicht gelesen.
printf '%s\n' "$PATH" | tr ':' '\n' > "$WORK/pathdirs"
while IFS= read -r dir; do
    [ -n "$dir" ] || continue
    [ -d "$dir" ] || continue
    for exe in "$dir"/*; do
        [ -x "$exe" ] || continue
        base=${exe##*/}
        case "$base" in
            git|git-*) continue ;;
        esac
        [ -e "$WORK/nogit/$base" ] && continue
        ln -s "$exe" "$WORK/nogit/$base" 2>/dev/null || true
    done
done < "$WORK/pathdirs"

if PATH="$WORK/nogit" command -v git >/dev/null 2>&1; then
    bad "die Sandbox ohne 'git*' enthaelt trotzdem ein git-Binary — der Nachweis waere wertlos."
elif [ ! -x "$WORK/nogit/sh" ]; then
    bad "die Sandbox ohne 'git*' enthaelt kein 'sh' — der Nachweis ist nicht moeglich."
else
    say "ok: die Sandbox ohne 'git*' ist gebaut (kein git-Binary, 'sh' vorhanden)."
    if PATH="$WORK/nogit" sh "$GEN" > "$WORK/nogit.out" 2> "$WORK/nogit.err"; then
        if diff -q "$WORK/run1" "$WORK/nogit.out" >/dev/null 2>&1; then
            say "ok: der Generator liefert ohne 'git' im PATH byteweise dieselbe Ausgabe (git-frei)."
        else
            bad "der Generator liefert ohne 'git' im PATH eine ANDERE Ausgabe — er braucht ein Git-Binary:"
            diff -u "$WORK/run1" "$WORK/nogit.out" 2>&1 | sed 's/^/  /' >&2
        fi
    else
        bad "der Generator endet ohne 'git' im PATH mit Exit != 0:"
        sed 's/^/  /' "$WORK/nogit.err" >&2
    fi
fi

if [ "$findings" -ne 0 ]; then
    echo "" >&2
    echo "FEHLER: $findings Pruefung(en) des Generators fehlgeschlagen." >&2
    exit 1
fi

say "doc_facts_generator OK: der Generator ist syntaktisch gueltig, vollstaendig, deterministisch und git-frei."
exit 0
