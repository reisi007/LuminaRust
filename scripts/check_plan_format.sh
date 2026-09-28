#!/bin/sh
# Plan-Format-Schranke (2026-09-28).
#
# `Agents.todo.md` darf laut `Agents.md` ausschliesslich **offene, umsetzbare**
# Aufgaben enthalten. Abgeschlossene Punkte werden nach bestaetigter Verifikation
# **komplett aus der Datei entfernt** — "niemals nur abhaken und niemals in einen
# neuen Bereich/Block (z. B. 'Erledigt', 'Verifiziert', 'Done') verschieben".
#
# Diese Schranke existiert, weil diese Regel am 2026-09-28 verletzt war: vier
# Eintraege standen als `- [x] ...` in der Datei, davon einer mit einem
# dreitaegigen Verifikationsverdikt im Text. Die Regel war also nicht vergessen,
# sondern nicht erzwungen — dieselbe Fehlerklasse wie beim Dateigroessen-Ratchet
# (siehe `scripts/check_file_sizes.sh`), nur auf einer anderen Ebene.
#
# Was hier geprueft wird, ist ausschliesslich die **Form**, nicht der Inhalt:
#   1. keine `- [x]`-Eintraege (abgeschlossene Tasks gehoeren entfernt)
#   2. kein "Erledigt"/"Done"/"Verifiziert"-Sammelabschnitt, in den
#      abgeschlossene Tasks verschoben werden
set -eu

plan="Agents.todo.md"

if [ ! -f "$plan" ]; then
    echo "plan_format OK ($plan nicht vorhanden)"
    exit 0
fi

status=0

checked=$(grep -c '^- \[x\]' "$plan" || true)
if [ "$checked" -ne 0 ]; then
    echo "plan_format FEHLER: $checked Eintrag/Eintraege als '- [x]' in $plan" >&2
    echo "  Agents.md: abgeschlossene Punkte werden nach bestaetigter Verifikation" >&2
    echo "  vollstaendig entfernt, niemals abgehakt. Der erledigte Zustand lebt" >&2
    echo "  in der Git-Historie und in den Feature-Dokumenten." >&2
    grep -n '^- \[x\]' "$plan" | cut -c1-100 >&2
    status=1
fi

# Ein Sammelabschnitt ist nur dann ein Verstoss, wenn er_checkliste-artige
# Marker traegt. Fliesstext, der das Wort erwaehnt, ist erlaubt — sonst wuerde
# dieser Check die eigene Fehlermeldung in diesem Skript selbst sperren.
blocks=$(grep -nE '^#{1,6} +(Erledigt|Done|Verifiziert|Abgeschlossen)' "$plan" || true)
if [ -n "$blocks" ]; then
    echo "plan_format FEHLER: Sammelabschnitt fuer abgeschlossene Aufgaben in $plan" >&2
    echo "$blocks" | cut -c1-100 >&2
    status=1
fi

if [ "$status" -eq 0 ]; then
    open=$(grep -c '^- \[ \] ' "$plan" || true)
    echo "plan_format OK ($open offene Aufgaben, 0 abgehakt)"
fi

exit "$status"
