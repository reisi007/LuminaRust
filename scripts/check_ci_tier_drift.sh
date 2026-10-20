#!/bin/sh
# CI-TIER-DRIFT-84: DRIFT-GATE zwischen `.github/workflows/ci.yml` und der
# committeten Tier-Liste in `scripts/gates.sh` (Stufe `slow`).
#
# Zweck: die Tier-Liste war ein handgepflegter Spiegel der CI. Aenderte sich
# die CI, driftete der Spiegel still — die lokale Stufe fuhr dann Kommandos,
# die die CI nicht mehr fuhr (oder umgekehrt). Dieser Gate macht den Spiegel
# pruefbar: Muster "generate + diff", nicht "wirkt plausibel".
#
#   1) GENERIEREN: `scripts/gen_ci_tiers.sh` liest `ci.yml` und liefert die
#      ableitbaren Felder je CI-Schritt (blocking|wd-pfad|kommando|job).
#   2) VERGLEICHEN: `sh scripts/gates.sh list-ci` liefert dieselben Felder aus
#      der committeten Tier-Liste.
#   3) DIFFEN: der Gate vergleicht beide Listen und meldet jede Abweichung:
#      welcher Job, welches Kommando, welches Feld.
#
# Der Gate MELDET, er SCHREIBT NICHT: die Tier-Liste wird nicht automatisch
# umgeschrieben. Die Aenderung bleibt ein sichtbares, reviewbares Ereignis
# (der Aufrufer — ein Mensch oder ein Agent — aendert `scripts/gates.sh`).
#
# SICHERHEITSREGEL: der Gate startet NICHTS, was er aus `ci.yml` gelesen hat.
# Generator und Gate lesen nur TEXT; es gibt kein `eval`, kein `sh -c` mit
# extrahierten Werten, kein `source` auf der YAML-Datei. Grund: `ci.yml` ist
# ein Eingabedokument; wer ein daraus gelesenes Kommando ausfuehrt, baut aus
# einer Pruefung ein Ausfuehrungs-Freigabegate. Der einzige gestartete
# Prozess ist `scripts/gen_ci_tiers.sh` (eigener Repo-Skript, kein YAML-Wert)
# und `scripts/gates.sh list-ci` (liest nur die Tier-Liste).
#
# ANMERKUNG zum Testhaken `LUMINA_CI_YAML`: damit kann der Gate gegen eine
# Kopie der Workflow-Datei laufen (Rotbeweis). Der Default ist IMMER die
# Repo-Datei; der Haken aendert nur, welche TEXT-Datei gelesen wird.
#
# HYGIENE: das Arbeitsverzeichnis (`mktemp -d`) wird per `trap ... EXIT`
# geloescht. Bei SIGPIPE (die Ausgabe laeuft in eine geschlossene Pipe,
# z. B. `... | head`) greift der EXIT-Trap nicht und das Verzeichnis bleibt
# unter `${TMPDIR:-/tmp}` liegen. Kein Umbau noetig — das ist der Normalfall
# einer abgebrochenen Pipe, kein Fehlerpfad des Gates; im Regelbetrieb
# (Ausgabe in eine Datei oder ins Log) raeumt der Trap auf.
#
# Gate-Header (Skill `ratchet-gates`, Invariante 8):
#   Defektklasse: stille Drift — die committete Tier-Liste bildet die CI
#                  nicht mehr ab (Kommando geaendert, ergaenzt, entfernt;
#                  `working-directory` oder `continue-on-error` gewandelt).
#   Anlass:        LOCAL-GATE-TIERS-81 spiegelte die CI von Hand; der
#                  Verifikationsbefund F2 (2026-10-20) belegte, dass eine
#                  CI-Aenderung den Spiegel ohne Signal passieren laesst.
#   Ratchet-Richtung: Boolean-Gate (keine Baseline-Datei) — die Abweichung
#                  muss 0 bleiben; es gibt keinen Wert, der steigen darf, und
#                  keinen Paydown-Pfad (die Drift wird behoben, nicht
#                  weggerechnet).
#   Nicht gedeckt:  (a) die ANNOTATIONEN der Tier-Liste (`id`,
#                  wd-Anzeige, "Bedarf") — sie sind handgepflegt und werden
#                  nicht verglichen; (b) die Jobs OHNE `container:` (`detect`,
#                  `docs`) — sie sind kein Rust-Gate (§5.3 der Doku);
#                  (c) ein CI-Schritt mit mehrzeiligem `run:`-Block — der
#                  Generator meldet ihn als Fehler (Exit 2), weil die
#                  Tier-Liste ihn nicht 1:1 abbilden kann;
#                  (d) die Ausfuehrung der Kommandos — der Gate vergleicht
#                  Text, er behauptet nichts darueber, ob ein Kommando
#                  erfolgreich waere.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CI_FILE="${LUMINA_CI_YAML:-$ROOT/.github/workflows/ci.yml}"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/check_ci_tier_drift.XXXXXX") || exit 1
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

if [ ! -f "$CI_FILE" ]; then
    echo "FEHLER: Workflow-Datei nicht lesbar: $CI_FILE" >&2
    exit 1
fi

# ---------------------------------------------------------------- 1. generieren
if ! sh "$ROOT/scripts/gen_ci_tiers.sh" "$CI_FILE" > "$WORK/generated" 2> "$WORK/generated.err"; then
    echo "FEHLER: der Generator konnte die CI-Sicht nicht bilden:" >&2
    sed 's/^/  /' "$WORK/generated.err" >&2
    echo "  (Die Tier-Liste bildet diesen Schritt nicht ab — das ist selbst Drift.)" >&2
    exit 1
fi

# ------------------------------------------------- 2. committete Seite lesen
if ! sh "$ROOT/scripts/gates.sh" list-ci > "$WORK/committed" 2> "$WORK/committed.err"; then
    echo "FEHLER: die Tier-Liste in scripts/gates.sh konnte nicht gelesen werden:" >&2
    sed 's/^/  /' "$WORK/committed.err" >&2
    exit 1
fi

# --------------------------------------------------------------- 3. vergleichen
cat > "$WORK/compare.awk" <<'AWK'
# Vergleicht die generierte CI-Sicht mit der committeten Tier-Sicht.
# Beide Seiten haben das Format: blocking|wd-pfad|kommando|job
BEGIN { gn = 0; cn = 0; nj = 0; diffs = 0; badline = 0 }
{
    n = split($0, F, "|")
    if (n != 4) {
        printf "FEHLER: Feldformat (4 Felder erwartet): %s\n", $0
        badline = 1
        next
    }
    if (FILENAME == gfile) {
        gn++
        gblk[gn] = F[1]; gwd[gn] = F[2]; gcmd[gn] = F[3]; gjob[gn] = F[4]
    } else {
        cn++
        cblk[cn] = F[1]; cwd[cn] = F[2]; ccmd[cn] = F[3]; cjob[cn] = F[4]
    }
}
END {
    if (badline) exit 1

    # Job-Reihenfolge: erst die generierte, dann die nur committeten Jobs.
    for (i = 1; i <= gn; i++) {
        j = gjob[i]
        if (!(j in jseen)) { jseen[j] = 1; jorder[++nj] = j }
        gidx[j, ++gcnt[j]] = i
    }
    for (i = 1; i <= cn; i++) {
        j = cjob[i]
        if (!(j in jseen)) { jseen[j] = 1; jorder[++nj] = j }
        cidx[j, ++ccnt[j]] = i
    }

    for (k = 1; k <= nj; k++) {
        j = jorder[k]
        ng = gcnt[j] + 0
        nc = ccnt[j] + 0
        # A) exakte Treffer (Kommando, Arbeitsverzeichnis und blockierend gleich)
        for (p = 1; p <= ng; p++) {
            gi = gidx[j, p]
            for (q = 1; q <= nc; q++) {
                ci = cidx[j, q]
                if (cused[ci]) continue
                if (gcmd[gi] == ccmd[ci] && gwd[gi] == cwd[ci] && gblk[gi] == cblk[ci]) {
                    cused[ci] = 1; gused[gi] = 1; break
                }
            }
        }
        # B) gleiches Kommando, anderes Feld (wd / continue-on-error)
        for (p = 1; p <= ng; p++) {
            gi = gidx[j, p]
            if (gused[gi]) continue
            for (q = 1; q <= nc; q++) {
                ci = cidx[j, q]
                if (cused[ci]) continue
                if (gcmd[gi] == ccmd[ci]) {
                    cused[ci] = 1; gused[gi] = 1
                    diffs++
                    printf "  Job \"%s\": Feld weicht ab bei `%s`\n", j, gcmd[gi]
                    if (gwd[gi] != cwd[ci])
                        printf "    Arbeitsverzeichnis: ci.yml='%s'  Tier-Liste='%s'\n", gwd[gi], cwd[ci]
                    if (gblk[gi] != cblk[ci])
                        printf "    blockierend:        ci.yml='%s'  Tier-Liste='%s'\n", gblk[gi], cblk[ci]
                    break
                }
            }
        }
        # C) Rest: positionspaarig als "Kommando geaendert" melden, was uebrig
        #    bleibt als "nur in ..." (ergaenzt bzw. entfernt).
        nu = 0
        for (p = 1; p <= ng; p++) {
            gi = gidx[j, p]
            if (!gused[gi]) { nu++; uidx[nu] = gi }
        }
        mu = 0
        for (q = 1; q <= nc; q++) {
            ci = cidx[j, q]
            if (!cused[ci]) { mu++; vidx[mu] = ci }
        }
        pairs = (nu < mu ? nu : mu)
        for (p = 1; p <= pairs; p++) {
            gi = uidx[p]; ci = vidx[p]
            diffs++
            printf "  Job \"%s\": Kommando geaendert\n", j
            printf "    ci.yml     : `%s` (wd='%s', blockierend=%s)\n", gcmd[gi], gwd[gi], gblk[gi]
            printf "    Tier-Liste : `%s` (wd='%s', blockierend=%s)\n", ccmd[ci], cwd[ci], cblk[ci]
        }
        for (p = pairs + 1; p <= nu; p++) {
            gi = uidx[p]
            diffs++
            printf "  Job \"%s\": nur in ci.yml (in der Tier-Liste fehlt): `%s` (wd='%s', blockierend=%s)\n", \
                   j, gcmd[gi], gwd[gi], gblk[gi]
        }
        for (p = pairs + 1; p <= mu; p++) {
            ci = vidx[p]
            diffs++
            printf "  Job \"%s\": nur in der Tier-Liste (in ci.yml nicht mehr vorhanden): `%s` (wd='%s', blockierend=%s)\n", \
                   j, ccmd[ci], cwd[ci], cblk[ci]
        }
    }

    # Reihenfolge: nur melden, wenn die Mengen gleich sind (sonst waere es
    # ein Rauschen neben dem inhaltlichen Befund).
    if (diffs == 0 && gn == cn) {
        for (i = 1; i <= gn; i++) gseq[i] = gjob[i] "|" gcmd[i] "|" gwd[i] "|" gblk[i]
        for (i = 1; i <= cn; i++) cseq[i] = cjob[i] "|" ccmd[i] "|" cwd[i] "|" cblk[i]
        for (i = 1; i <= gn; i++) {
            if (gseq[i] != cseq[i]) {
                diffs++
                printf "  Reihenfolge weicht ab (gleiche Menge, andere Position): Position %d\n", i
                printf "    ci.yml     : %s\n", gseq[i]
                printf "    Tier-Liste : %s\n", cseq[i]
                break
            }
        }
    }
    exit (diffs > 0 ? 1 : 0)
}
AWK

gen_count=$(wc -l < "$WORK/generated" | tr -d ' ')
com_count=$(wc -l < "$WORK/committed" | tr -d ' ')
if awk -v gfile="$WORK/generated" -f "$WORK/compare.awk" "$WORK/generated" "$WORK/committed" > "$WORK/report" 2>&1; then
    cat "$WORK/report"
    echo "ci_tier_drift OK: $gen_count CI-Schritt(e) in der Workflow-Datei, $com_count Eintrag(e) in der Tier-Liste — keine Abweichung."
    exit 0
else
    echo "FEHLER: CI-Tier-Drift — die Tier-Liste in scripts/gates.sh weicht von der CI ab." >&2
    sed 's/^/  /' "$WORK/report" >&2
    echo "" >&2
    echo "  Die Tier-Liste wird NICHT automatisch umgeschrieben (der Gate meldet nur)." >&2
    echo "  Der Generator zeigt die Soll-Seite:" >&2
    sed 's/^/    /' "$WORK/generated" >&2
    echo "  Die committete Seite steht in scripts/gates.sh (SLOW_ENTRIES; 'list-ci' zeigt sie)." >&2
    exit 1
fi
