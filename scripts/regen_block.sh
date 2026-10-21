#!/bin/sh
# Ersetzt einen GENERATED-Block in einer Zieldatei durch die Generator-Ausgabe.
#
# Zweck: die Bloecke in `feature/quality/file-size-policy.md` (und die weiteren
# im Manifest) entstehen aus dem **Baum** (Dateigroessen, Dateizahl). Jeder
# Schnitt veraendert beide, also driftet der Block — bis er neu erzeugt wird.
# Vorher war das Handarbeit (Block markieren, Generator-Ausgabe einfuegen);
# dieses Skript macht es mechanisch und **git-frei** (der Generator braucht
# kein `git` — der CI-Container `lumina-ci:latest` hat keins ✓).
#
# Aufruf: sh scripts/regen_block.py <block-id>   (Endung .py ist historisch
#         falsch, das Skript ist POSIX sh — der Name bleibt vorerst, damit
#         keine Referenz auf einen anderen Namen bricht; TODO: umbenennen).
#
# Quelle der Zieldatei: `scripts/gen_doc_facts.sh --manifest` (die Zaehlregel ✓).
# Ausgefuehrt werden ausschliesslich Festplatzwerkzeuge mit festen Argumenten ✓
# (awk, cmp, mktemp, rm) — nie etwas, das aus einem gelesenen Dokument stammt ✓.
set -eu

BLOCK=${1:-}
if [ -z "$BLOCK" ]; then
  echo "Aufruf: sh scripts/regen_block.py <block-id>" >&2
  exit 2
fi

ROOT=$(cd "$(dirname "$0")/.." && pwd)

TARGET=$(sh "$ROOT/scripts/gen_doc_facts.sh" --manifest |
         awk -F'|' -v b="$BLOCK" '$1 == b { print $2; exit }')
if [ -z "$TARGET" ]; then
  echo "FEHLER: kein Ziel fuer Block-Id '$BLOCK' im Manifest" >&2
  exit 2
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/regen_block.XXXXXX") || exit 2
trap 'rm -rf "$WORK"' EXIT INT TERM

sh "$ROOT/scripts/gen_doc_facts.sh" "$BLOCK" > "$WORK/new.txt" || exit 2

awk -v begin="<!-- BEGIN GENERATED: $BLOCK -->" \
    -v end="<!-- END GENERATED: $BLOCK -->" \
    -v newfile="$WORK/new.txt" '
  BEGIN {
    while ((getline line < newfile) > 0) { block[n++] = line }
    close(newfile)
  }
  $0 == begin { print; for (i = 0; i < n; i++) print block[i]; skip = 1; next }
  $0 == end   { skip = 0; print; next }
  skip != 1   { print }
' "$TARGET" > "$WORK/out.txt"

if cmp -s "$WORK/out.txt" "$TARGET"; then
  echo "unveraendert: $TARGET [$BLOCK]"
else
  cat "$WORK/out.txt" > "$TARGET"
  echo "aktualisiert: $TARGET [$BLOCK]"
fi
