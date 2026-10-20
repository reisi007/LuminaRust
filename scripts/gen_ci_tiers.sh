#!/bin/sh
# CI-TIER-DRIFT-84 (Folgeticket zu LOCAL-GATE-TIERS-81): GENERATOR der
# CI-Tier-Sicht.
#
# Zweck: die Tier-Liste in `scripts/gates.sh` (Stufe `slow`) ist ein Spiegel
# der Rust-Jobs aus `.github/workflows/ci.yml`. Bis hierher wurde der Spiegel
# von Hand gepflegt — eine Aenderung in der CI driftete still. Dieser
# Generator liest `ci.yml` und liefert die ABLEITBAREN Felder je CI-Schritt;
# `scripts/check_ci_tier_drift.sh` difft das Ergebnis gegen die committete
# Tier-Liste und meldet jede Abweichung (Muster "generate + diff").
#
# ABLEITBAR (vom Generator erzeugt):
#   * `blocking` — 0, wenn der Schritt `continue-on-error: true` traegt, sonst 1
#   * `wd-path`  — das `working-directory:` des Schritts (`.` ohne Angabe)
#   * `command`  — der `run:`-Wert des Schritts, zeichengleich
#   * `job`      — der `name:` des Jobs (der Job-Key, wenn der Job keinen
#                  Namen traegt)
#
# NICHT ableitbar (Annotation, bleibt handgepflegt in `scripts/gates.sh`):
#   * die `id` (`slow-1`, `slow-2`, ...), die Anzeige des Arbeitsverzeichnisses
#     ("Repo-Wurzel") und der "Bedarf" (Build-Artefakt / keine Infrastruktur).
#     Der Generator erfindet Annotationen nicht: eine Ableitung, die eine
#     Annotation erraten muesste, waere eine zweite, schlechtere Wahrheit.
#
# SICHERHEITSREGEL: der Generator liest NUR TEXT. Er fuehrt KEIN Kommando aus,
# das er aus der YAML-Datei gelesen hat — kein `eval`, kein `sh -c`, kein
# `.`/`source`. Das gelesene Kommando wird ausschliesslich als TEXT verglichen
# (Zeile fuer Zeile). Grund: `ci.yml` ist ein Eingabedokument; ein Kommando
# daraus auszufuehren wuerde aus einer Pruefung ein Ausfuehrungs-Freigabegate
# machen. Der Drift-Gate startet ebenfalls nichts aus `ci.yml`.
#
# SCOPE (und warum):
#   * Jobs MIT `container:` — das sind die Rust-Jobs (Container
#     `ghcr.io/reisi007/luminarust/lumina-ci:latest`). Die Jobs ohne Container
#     (`detect`, `docs`) sind kein Rust-Gate; sie stehen in §5.3 der Doku.
#   * Schritte OHNE `run:` (Checkout, Toolchain, Cache, Artifact-Upload) tragen
#     kein Kommando und sind kein Tier-Eintrag.
#   * ANKER: der Schritt, der die schnelle Stufe faehrt
#     (`sh scripts/gates.sh fast`), wird NICHT gespiegelt — er ist der Aufruf
#     der Stufe `fast`, kein Eintrag der Stufe `slow`. Der Anker ist selbst
#     ableitbar: nur ein `run:`-Wert, der exakt `sh scripts/gates.sh fast`
#     lautet, wird uebersprungen. Aendert sich dieser Wert, faellt der Schritt
#     in die Spiegelung zurueck und der Drift-Gate wird rot.
#
# Ausgabe: eine Zeile je Schritt, Felder mit `|` getrennt:
#     blocking|wd-path|command|job
# Deterministisch: Reihenfolge wie in `ci.yml`, keine Zeitstempel, keine
# absoluten Pfade. Zweimal laufen lassen -> byteweise identische Ausgabe.
#
# HYGIENE: das Arbeitsverzeichnis (`mktemp -d`) wird per `trap ... EXIT`
# geloescht. Bei SIGPIPE (Ausgabe in eine geschlossene Pipe) greift der
# EXIT-Trap nicht und das Verzeichnis bleibt unter `${TMPDIR:-/tmp}` liegen —
# Hygiene-Hinweis, kein Fehlerpfad; im Regelbetrieb raeumt der Trap auf.
#
# Aufruf:
#   sh scripts/gen_ci_tiers.sh [pfad/zu/ci.yml]
#
# Exit: 0 = Liste auf stdout. 2 = die Datei ist nicht lesbar, oder sie enthaelt
# einen Schritt, den die Tier-Liste nicht 1:1 abbilden kann (z. B. einen
# mehrzeiligen `run:`-Block oder ein `|` im Kommando). Der Drift-Gate meldet
# das als Fehler (rot) — ein Schritt, den der Spiegel nicht abbilden kann, ist
# selbst eine Drift.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CI_FILE="${1:-$ROOT/.github/workflows/ci.yml}"

if [ ! -f "$CI_FILE" ]; then
    echo "gen_ci_tiers: Datei nicht lesbar: $CI_FILE" >&2
    exit 2
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/gen_ci_tiers.XXXXXX") || exit 2
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

cat > "$WORK/parse.awk" <<'AWK'
# Parser fuer .github/workflows/ci.yml — liest ausschliesslich Text.
# Erzeugt je Schritt der Container-Jobs eine Zeile
#     blocking|wd-path|command|job
BEGIN {
    injobs = 0
    job = ""; jobname = ""; hascontainer = 0
    step_open = 0; s_name = ""; s_run = ""; s_wd = ""; s_coe = 0; s_block = 0
    inblock = 0; blockmin = 0
    nout = 0; err = 0
}
function trim(s) {
    sub(/^[ \t]+/, "", s)
    sub(/[ \t]+$/, "", s)
    return s
}
function unq(s,   t) {
    t = s
    if (t ~ /^".*"$/) t = substr(t, 2, length(t) - 2)
    return t
}
function fail(msg) {
    printf "gen_ci_tiers: %s\n", msg > "/dev/stderr"
    err = 1
}
# Ein Schritt wird beim Uebergang zum naechsten Schritt oder Job ausgegeben.
function flushstep(   cmd, jn, wd) {
    if (!step_open) return
    step_open = 0
    if (!hascontainer) return          # Job ohne Container: nicht im Scope
    if (s_block) {
        fail("Schritt \"" s_name "\" im Job \"" job "\" hat einen mehrzeiligen `run:`-Block — die Tier-Liste kann ihn nicht 1:1 abbilden.")
        return
    }
    if (s_run == "") return            # Schritt ohne `run:` (uses:, if:, Upload)
    cmd = trim(s_run)
    # ANKER: der Aufruf der schnellen Stufe (`sh scripts/gates.sh fast`) ist
    # kein Eintrag der Stufe `slow` — er ist der Aufruf der Stufe `fast`.
    # Uebersprungen wird NUR dieser exakte Wert; aendert er sich, faellt der
    # Schritt in die Spiegelung zurueck und der Drift-Gate wird rot.
    if (cmd == "sh scripts/gates.sh fast") return
    jn = (jobname == "" ? job : jobname)
    wd = (s_wd == "" ? "." : trim(s_wd))
    if (index(cmd, "|") > 0) {
        fail("Kommando \"" cmd "\" enthaelt das Feldtrennzeichen '|' — die Tier-Liste kann es nicht abbilden.")
        return
    }
    if (index(jn, "|") > 0) {
        fail("Job-Name \"" jn "\" enthaelt das Feldtrennzeichen '|'.")
        return
    }
    if (index(wd, "|") > 0) {
        fail("working-directory \"" wd "\" enthaelt das Feldtrennzeichen '|'.")
        return
    }
    out[++nout] = (s_coe ? "0" : "1") "|" wd "|" cmd "|" jn
}
# Ein Schritt-Feld "key: value" (auch die erste Zeile eines Schritts nach
# dem Listendstrich "- ").
function stepkv(kv,   p, k, v) {
    p = index(kv, ":")
    if (p == 0) return
    k = trim(substr(kv, 1, p - 1))
    v = trim(substr(kv, p + 1))
    if (k == "run") {
        if (v == "|" || v == ">" || v == "|-" || v == ">-" || v == "|+" || v == ">+") {
            # Block-Skalar: der Inhalt steht tiefer eingerueckt und wird
            # uebersprungen. Ein Container-Job mit solchem Schritt ist ein
            # Fehler (siehe flushstep).
            s_block = 1; inblock = 1; blockmin = 8
        } else {
            s_run = v
        }
    } else if (k == "working-directory") {
        s_wd = v
    } else if (k == "continue-on-error") {
        s_coe = (v == "true" || v == "True" || v == "yes")
    } else if (k == "name") {
        if (s_name == "") s_name = unq(v)
    }
}
{
    line = $0
    tmp = line
    gsub(/^[ ]+/, "", tmp)
    indent = length(line) - length(tmp)

    if (inblock) {
        if (indent > blockmin) next   # Inhalt des Block-Skalars
        inblock = 0
    }
    if (!injobs) {
        if (line ~ /^jobs:[ \t]*$/) injobs = 1
        next
    }
    # Neuer Job (zweispaltige Einrueckung, Schluessel ohne Wert).
    if (line ~ /^  [A-Za-z0-9_.-]+:[ \t]*$/) {
        flushstep()
        job = line
        sub(/^  /, "", job)
        sub(/:[ \t]*$/, "", job)
        jobname = ""; hascontainer = 0
        next
    }
    if (line ~ /^    name:[ \t]/) {
        jobname = unq(trim(substr(line, index(line, ":") + 1)))
        next
    }
    if (line ~ /^    container:/) { hascontainer = 1; next }
    # Neuer Schritt: "- key: value" auf der Schritt-Ebene.
    if (line ~ /^      - /) {
        flushstep()
        step_open = 1; s_name = ""; s_run = ""; s_wd = ""; s_coe = 0; s_block = 0
        stepkv(substr(line, 9))
        next
    }
    # Weitere Felder eines Schritts (acht Leerzeichen Einrueckung).
    if (line ~ /^        [A-Za-z0-9_-]+:/) {
        stepkv(substr(line, 9))
        next
    }
}
END {
    flushstep()
    if (err) exit 2
    for (i = 1; i <= nout; i++) print out[i]
}
AWK

awk -f "$WORK/parse.awk" "$CI_FILE"
