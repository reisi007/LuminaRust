#!/bin/sh
# File-size ratchet (User-Vorgabe 2026-09-17): Rust files over 500 lines must
# not grow. Files at or below 500 lines are unchecked. A file over 500 lines
# must have a baseline entry in scripts/file_size_baseline.txt and must not
# exceed it. Shrink or extract at any time; after an extraction, lower the
# baseline entry (or drop it once the file is at or below 500 lines).
# New files over 500 lines fail until a baseline entry is added consciously.
# POSIX sh only (macOS ships bash 3.2 without associative arrays).
#
# Gate-Header (Skill `ratchet-gates`, Invariante 8 — je Defektklasse eine
# Schranke, Klasse und Anlass im Header benannt):
#   Defektklasse: oversized unit — eine Rust-Datei ueber 500 Zeilen.
#   Anlass:        User-Vorgabe 2026-09-17 (500-Zeilen-Schranke). Ohne
#                  Baseline waere die Schranke entweder dauerhaft rot (eine
#                  tote Schranke, die nach dem zweiten Lauf abgeschaltet wird)
#                  oder eine unerzwungene Empfehlung.
#   Ratchet-Richtung: ceiling — der Baseline-Eintrag (Maximalgroesse je
#                  Datei) soll nur sinken, nie steigen; gesenkt wird er
#                  zusammen mit der Extraktion, die die Datei schrumpft.
#                  ACHTUNG: erzwungen wird nur die *Wachstumsgrenze*
#                  (n > b -> FAIL). Eine Anhebung des Eintrags ist
#                  maschinell NICHT gesperrt (die Baseline enthaelt
#                  belegte Anhebungen, z. B. "BEWUSSTE Anhebung
#                  12572 -> 12573") — die Richtung ist hier eine
#                  Konvention, kein Gate. (Die 3000er-Liste in
#                  scripts/file_size_ceiling.txt erzwingt sie ueber (D).)
#   Nicht gedeckt:  Nicht-Rust-Dateien (Markdown, Shell, YAML) und Dateien
#                  mit bis zu 500 Zeilen (bewusst ungeprueft); die Baseline
#                  ist handgepflegt, nicht generiert (siehe Lueckenliste in
#                  `feature/quality/local-gates.md`).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASELINE="$ROOT/scripts/file_size_baseline.txt"
THRESHOLD=500
fail=0

listed() {
  # listed <path> -> prints baseline count or nothing (exact path match)
  awk -v p="$1" '$2 == p {print $1}' "$BASELINE" | head -n 1
}

find "$ROOT/crates" -name '*.rs' -not -path '*/target/*' | while IFS= read -r f; do
  rel="${f#$ROOT/}"
  n=$(wc -l < "$f" | tr -d ' ')
  if [ "$n" -gt "$THRESHOLD" ]; then
    b=$(listed "$rel")
    if [ -z "$b" ]; then
      echo "FAIL: $rel has $n lines (> $THRESHOLD) but no baseline entry in scripts/file_size_baseline.txt"
      exit 1
    elif [ "$n" -gt "$b" ]; then
      echo "FAIL: $rel grew $b -> $n lines (extract code instead; lower the baseline after extraction)"
      exit 1
    fi
  fi
done || fail=1

awk '!/^#/ && NF == 2 {print $2}' "$BASELINE" | while IFS= read -r path; do
  if [ -f "$ROOT/$path" ]; then
    n=$(wc -l < "$ROOT/$path" | tr -d ' ')
    if [ "$n" -le "$THRESHOLD" ]; then
      echo "INFO: $path is back at $n lines (<= $THRESHOLD) - baseline entry can be removed"
    fi
  else
    echo "INFO: $path no longer exists - baseline entry can be removed"
  fi
done

exit "$fail"
