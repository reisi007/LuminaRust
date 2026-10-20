#!/bin/sh
# Lokale Gate-Stufen fuer LuminaRust (LOCAL-GATE-TIERS-81; Skill
# `ratchet-gates`, agents-skills `70d49e0`).
#
# Zweck: ZWEI BENANNTE, DOKUMENTIERTE und LOKAL LAEUFBARE Gate-Stufen. Der
# Skill nennt eine Stufe, die niemand benennen, laufen oder reproduzieren
# kann, keine Stufe; beide Stufen stehen deshalb hier, und ihre Definition
# ist die EINZIGE Quelle der Wahrheit fuer die Doku
# (`feature/quality/local-gates.md`).
#
#   fast  — Sekunden, OHNE Codegen. Sechs Checks in fester Reihenfolge:
#           1) cargo fmt --all -- --check   (nur rustfmt, kein Compile)
#           2) sh scripts/check_file_sizes.sh
#           3) sh scripts/check_plan_format.sh
#           4) sh scripts/check_env_gate_reasons.sh
#           5) sh scripts/check_core_prod_fs_free.sh
#           6) sh scripts/check_ci_tier_drift.sh  (Drift-Gate: die Tier-Liste
#              gegen die generierte Liste aus `ci.yml`; nur Textvergleich)
#           `fast` ruft KEIN `cargo build/check/clippy/test/bench` auf. Das
#           Skript prueft das VOR dem Lauf (Verstoss = harter Fehler), damit
#           die Definition nicht still in eine Codegen-Stufe kippt.
#   slow  — Minuten, CI-Mirror der Rust-Jobs aus `.github/workflows/ci.yml`
#           (Fail fast: der erste Fehlschlag bricht ab, das dauert Minuten).
#           Die Kommandos sind eins zu eins aus `ci.yml` uebernommen; je
#           Eintrag stehen der Job-Name und das `working-directory` in der
#           Tier-Definition (keine Zeilennummer — sie driftet, der Job-Name
#           nicht; siehe die Begruendung am Feldformat).
#           Schritte, die in der CI `continue-on-error` tragen (`slow-21`,
#           `slow-22`), sind auch hier NICHT blockierend: ihr Fehlschlag wird
#           gemeldet (Exit-Code + Hinweis), macht die Stufe aber nicht rot —
#           der lokale Runner ist nicht strenger als die CI.
#   list  — die Tier-Tabelle (eine Quelle der Wahrheit fuer die Doku).
#   list-ci — die ABLEITBARE Sicht der CI-Spiegelung (das, was der Generator
#           `scripts/gen_ci_tiers.sh` aus `ci.yml` zieht): eine Zeile je
#           Eintrag `blocking|wd-path|command|job`. Das ist die
#           Vergleichsgrundlage des Drift-Gates
#           (`scripts/check_ci_tier_drift.sh`).
#   all   — fast, danach slow.
#   help  — Usage.
#
# FLOOR (Skill `ratchet-gates`): eine auswahlbasierte Stufe laeuft in einem
# clean checkout 0 Checks und meldet gruen — eine tote Schranke. Der Runner
# zaehlt deshalb die GESTARTETEN Checks und erzwingt MIN_CHECKS=6 fuer die
# Stufe `fast`: darunter ist das Ergebnis ein FEHLER (Exit 1), kein Pass.
# Eine leere Tier-Liste ist ebenfalls ein Fehler, kein Pass. Ein fehlendes
# Pruefskript zaehlt als FEHLSCHLAG, nicht als Ueberspringen.
# MIN_CHECKS wurde von 5 auf 6 gehoeben, weil die Stufe `fast` um
# `fast-6` (Drift-Gate) gewachsen ist — die Untergrenze muss die Zahl der
# Checks abbilden, sonst bleibt das Entfernen eines Checks still (2026-10-20,
# GATE-DRIFT-UND-LUECKEN-84).
#
# Das Skript schreibt NICHTS ins Repo, legt keine Dateien an, braucht keine
# Sonderrechte und laeuft ohne Netz.
#
# POSIX sh (dash, kein Bash). Aufruf: sh scripts/gates.sh <fast|slow|all|list|help>
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Mindeststand der schnellen Stufe: unterhalb dieser Zahl gestarteter Checks
# ist das Ergebnis ein Fehler (Floor), nicht ein Pass.
MIN_CHECKS=6

# `cargo` steht in manchen nicht-interaktiven Shells nicht im PATH (z. B. auf
# diesem Host); dann wird die rustup-Installation des Benutzers benutzt. In der
# CI ist `cargo` bereits im PATH — der Zweig ist dort wirkungslos.
if ! command -v cargo >/dev/null 2>&1; then
    if [ -x "${HOME:-/nonexistent}/.cargo/bin/cargo" ]; then
        PATH="${HOME}/.cargo/bin:$PATH"
        export PATH
        echo "HINWEIS: cargo nicht im PATH gefunden, benutze ${HOME}/.cargo/bin" >&2
    fi
fi

# ---------------------------------------------------------------------------
# Tier-Definitionen. Feldformat je Zeile (7 Felder):
#   id | blockierend (1/0) | Arbeitsverzeichnis (Anzeige) | Arbeitsverzeichnis
#   (Pfad, relativ zur Repo-Wurzel) | Bedarf | Kommando | Job in ci.yml
# Die Kommandos stehen hier und nirgends sonst: `list` und der Lauf lesen
# dieselbe Liste.
#
# Herkunft: Feld 7 nennt den **Job-Namen** in `.github/workflows/ci.yml`, in
# dem das Kommando steht. Bewusst **keine Zeilennummer**: eine handgepflegte
# Kopie der `ci.yml`-Struktur waere genau die zweite Quelle der Wahrheit, die
# der Skill `ratchet-gates` vermeiden will ("a generated file that embeds a
# hand-maintained list has a second source of truth"). Der Job-Name driftet
# nicht, die Zeilennummer driftet — in einem einzigen Kommentar-Change schon
# um drei Zeilen.
#
# GATE-DRIFT-UND-LUECKEN-84 (2026-10-20): die Kopie ist nicht mehr
# handgepflegt, sondern **geprueft**. `scripts/gen_ci_tiers.sh` zieht die
# ableitbaren Felder (blockierend, Arbeitsverzeichnis, Kommando, Job) aus
# `ci.yml`; `scripts/check_ci_tier_drift.sh` (Eintrag `fast-6`) difft diese
# generierte Liste gegen die committete Tier-Liste und wird rot bei jeder
# Abweichung. ANNOTATIONEN bleiben handgepflegt: die `id`, die Anzeige des
# Arbeitsverzeichnisses und der "Bedarf" — der Generator darf sie nicht
# erfinden. Die Herkunft ist damit automatisch exakt: der Job-Name steht nicht
# mehr nur hier, sondern wird aus `ci.yml` gelesen; Feld 7 ist der (mit
# `list-ci`) maschinell verglichene Wert.
# ---------------------------------------------------------------------------

FAST_ENTRIES='fast-1|1|Repo-Wurzel|.|keine Infrastruktur|cargo fmt --all -- --check||
fast-2|1|Repo-Wurzel|.|keine Infrastruktur|sh scripts/check_file_sizes.sh||
fast-3|1|Repo-Wurzel|.|keine Infrastruktur|sh scripts/check_plan_format.sh||
fast-4|1|Repo-Wurzel|.|keine Infrastruktur|sh scripts/check_env_gate_reasons.sh||
fast-5|1|Repo-Wurzel|.|keine Infrastruktur|sh scripts/check_core_prod_fs_free.sh||
fast-6|1|Repo-Wurzel|.|keine Infrastruktur|sh scripts/check_ci_tier_drift.sh||'

# CI-Mirror. Quelle ist `.github/workflows/ci.yml` (Stand 2026-10-20); der
# Job-Name steht je Eintrag (Feld 7). **kein Schritt** setzt ein schrittweises
# `env:` — die vier Rust-Jobs setzen job-level `CARGO_TERM_COLOR: always`
# (Farbausgabe only, keine Pruef-Semantik). Die einzige `working-directory`-
# Angabe ist `agent-harness` (zweites Cargo-Workspace).
# `slow-21` und `slow-22` sind im CI `continue-on-error: true` und deshalb
# hier nicht blockierend.
# `slow-10b` ist der Orphan-Test-Gate (TEST-ORPHAN-97): er haengt an
# `slow-10`, weil nur der Test-Build die dep-info erzeugt, die das Skript
# braucht (Vorbedingung, siehe `feature/quality/local-gates.md` §5.1). Die
# `id` traegt ein `b`, damit die 22 bisherigen `slow`-Ids **unveraendert**
# bleiben — eine Umnummerierung waere eine stille Aenderung an der
# committeten Tier-Liste.
SLOW_ENTRIES='slow-1|1|Repo-Wurzel|.|Build-Artefakt|cargo check --workspace --all-targets|Rust fast checks
slow-2|1|Repo-Wurzel|.|Build-Artefakt|cargo clippy --workspace --all-targets --features "lumina-sidecar/zdata,lumina-bench/raw-bench" -- -D warnings|Rust fast checks
slow-3|1|Repo-Wurzel|.|Build-Artefakt|cargo clippy -p lumina-core --features lensfun -- -D warnings|Rust fast checks
slow-4|1|Repo-Wurzel|.|Build-Artefakt|cargo clippy -p lumina-onnx --features onnx-rt -- -D warnings|Rust fast checks
slow-5|1|Repo-Wurzel|.|Build-Artefakt|cargo check -p lumina-onnx --features onnx-rt|Rust fast checks
slow-6|1|Repo-Wurzel|.|Build-Artefakt|cargo check -p lumina-gpu --features gpu|Rust fast checks
slow-7|1|Repo-Wurzel|.|Build-Artefakt|sh scripts/check_cli_features.sh|Rust fast checks
slow-8|1|Repo-Wurzel|.|Build-Artefakt|sh scripts/check_feature_matrix.sh|Rust fast checks
slow-9|1|Repo-Wurzel|.|Build-Artefakt|cargo check -p lumina-bench --all-features|Rust fast checks
slow-10|1|Repo-Wurzel|.|Build-Artefakt|cargo test -p lumina-gui --all-targets|Rust GUI tests
slow-10b|1|Repo-Wurzel|.|Build-Artefakt|sh scripts/check_test_orphans.sh lumina-gui|Rust GUI tests
slow-11|1|Repo-Wurzel|.|Build-Artefakt|cargo test -p lumina-gui --lib --features janklog|Rust GUI tests
slow-12|1|Repo-Wurzel|.|Build-Artefakt|cargo test --workspace --all-targets --exclude lumina-gui --exclude lumina-sidecar --exclude lumina-bench|Rust tests (excluding GUI)
slow-13|1|Repo-Wurzel|.|Build-Artefakt|cargo test -p libraw-sys|Rust tests (excluding GUI)
slow-14|1|Repo-Wurzel|.|Build-Artefakt|cargo test -p lumina-sidecar --features zdata|Rust tests (excluding GUI)
slow-15|1|Repo-Wurzel|.|Build-Artefakt|cargo test -p lumina-lensfun --features native -p lumina-core --features lensfun -p lumina-cli --features lensfun|Rust tests (excluding GUI)
slow-16|1|Repo-Wurzel|.|Build-Artefakt|cargo test -p lumina-onnx --features onnx-rt|Rust tests (excluding GUI)
slow-17|1|Repo-Wurzel|.|Build-Artefakt|cargo test -p lumina-cli --bin lumina-cli --features gpu,lensfun|Rust tests (excluding GUI)
slow-18|1|agent-harness|agent-harness|keine Infrastruktur|cargo fmt --check|Rust agent-harness
slow-19|1|agent-harness|agent-harness|Build-Artefakt|cargo clippy --all-targets -- -D warnings|Rust agent-harness
slow-20|1|agent-harness|agent-harness|Build-Artefakt|cargo test|Rust agent-harness
slow-21|0|Repo-Wurzel|.|Build-Artefakt|cargo bench -p lumina-bench -- --sample-size 50 --warm-up-time 2|Benchmarks (optional)
slow-22|0|Repo-Wurzel|.|Build-Artefakt|node scripts/perf/compare.mjs --mode warn --report-dir perf/results|Benchmarks (optional)'

usage() {
    cat <<'EOF'
Usage: sh scripts/gates.sh <fast|slow|all|list|list-ci|help>

  fast   Schnelle Stufe (Sekunden, kein Codegen): cargo fmt + 5 Textgates.
         Floor: mindestens 6 gestartete Checks, sonst FAIL (Exit 1).
  slow   Langsame Stufe (Minuten): CI-Mirror der Rust-Jobs aus
         .github/workflows/ci.yml, Fail fast.
  all    fast, danach slow.
  list   Tier-Tabelle (eine Quelle der Wahrheit fuer die Doku) auf stdout.
  list-ci  Ableitbare CI-Sicht (blocking|wd-pfad|kommando|job) auf stdout —
         das Vergleichsformat des Drift-Gates (scripts/check_ci_tier_drift.sh).
  help   Diese Hilfe.

Das Skript laeuft aus dem Repo-Wurzelverzeichnis (selbst ermittelt), schreibt
nichts ins Repo und braucht keine Sonderrechte.
EOF
}

# Tier-Tabelle. Deterministisch: feste Reihenfolge, keine Zeitstempel, keine
# absoluten Pfade.
print_list() {
    printf '| ID | Stufe | Kommando | Arbeitsverzeichnis | Budget | Bedarf |\n'
    printf '| --- | --- | --- | --- | --- | --- |\n'
    printf '%s\n' "$FAST_ENTRIES" |
        awk -F'|' -v tier=fast -v budget=Sekunden \
            '{printf "| %s | %s | `%s` | %s | %s | %s |\n", $1, tier, $6, $3, budget, $5}'
    printf '%s\n' "$SLOW_ENTRIES" |
        awk -F'|' -v tier=slow -v budget=Minuten \
            '{printf "| %s | %s | `%s` | %s | %s | %s |\n", $1, tier, $6, $3, budget, $5}'
}

# Die ABLEITBARE Sicht der CI-Spiegelung — genau die Felder, die der
# Generator `scripts/gen_ci_tiers.sh` aus `ci.yml` zieht:
#     blocking|wd-pfad|kommando|job
# Die Annotationen (id, wd-Anzeige, Bedarf) stehen bewusst NICHT hier: sie
# sind handgepflegt und werden nicht verglichen. Reihenfolge wie in der
# Tier-Liste (= Reihenfolge wie in `ci.yml`).
print_ci_mirror() {
    printf '%s\n' "$SLOW_ENTRIES" |
        awk -F'|' '{printf "%s|%s|%s|%s\n", $2, $4, $6, $7}'
}

# Die Stufe `fast` darf keinen Codegen ausfuehren. Die Pruefung laeuft vor dem
# Lauf, damit die Definition nicht still zur Build-Stufe wird.
# Erkannt wird jedes Kommando, in dem ein `cargo`-Wort (auch als Pfad wie
# `$HOME/.cargo/bin/cargo`) nicht von `fmt` gefolgt wird — der Trenner darf
# ein Leerzeichen oder ein Tab sein. Anfuehrungszeichen werden vor dem
# Vergleich entfernt, sonst umginge `sh -c "cargo check --workspace"` den
# Detektor. Das Muster prueft die Worte der Kommandozeile, nicht ihre
# Schreibweise.
assert_no_codegen() {
    rc=0
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        cmd=$(printf '%s' "$entry" | cut -d'|' -f6)
        bad=$(printf '%s' "$cmd" | awk '
            {
                sq = sprintf("%c", 39)
                dq = sprintf("%c", 34)
                lead = "^[" sq dq "]+"
                trail = "[" sq dq "]+$"
                for (i = 1; i <= NF; i++) {
                    w = $i
                    gsub(lead, "", w)
                    gsub(trail, "", w)
                    if (w == "cargo" || w ~ /\/cargo$/) {
                        nxt = ""
                        if (i < NF) {
                            nxt = $(i + 1)
                            gsub(lead, "", nxt)
                            gsub(trail, "", nxt)
                        }
                        if (i == NF || nxt != "fmt") {
                            print (i == NF) ? w : w " " nxt
                            exit
                        }
                    }
                }
            }')
        if [ -n "$bad" ]; then
            echo "FEHLER: die Stufe fast darf kein Codegen ausfuehren (gefunden: $bad)" >&2
            rc=1
        fi
    done <<EOF
$FAST_ENTRIES
EOF
    return "$rc"
}

# Ein Tier laufen lassen.
#   $1 Tiername, $2 Eintragsliste, $3 Floor (0 = kein Floor)
run_tier() {
    tier="$1"
    entries="$2"
    floor="$3"
    executed=0
    failed=0
    soft_failed=0
    summary=''

    if [ -z "$entries" ]; then
        echo "FEHLER: die Tier-Liste '$tier' ist leer — das ist ein Fehler, kein Pass." >&2
        return 1
    fi

    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        id=$(printf '%s' "$entry" | cut -d'|' -f1)
        blocking=$(printf '%s' "$entry" | cut -d'|' -f2)
        wd_name=$(printf '%s' "$entry" | cut -d'|' -f3)
        wd_path=$(printf '%s' "$entry" | cut -d'|' -f4)
        cmd=$(printf '%s' "$entry" | cut -d'|' -f6)

        executed=$((executed + 1))
        echo "[$tier/$id] ($wd_name) $cmd"
        start=$(date +%s)

        rc=0
        missing=''
        case "$cmd" in
            'sh scripts/'*)
                script=$(printf '%s' "$cmd" | awk '{print $2}')
                if [ ! -f "$ROOT/$script" ]; then
                    missing="$script"
                fi
                ;;
        esac
        if [ -n "$missing" ]; then
            echo "FEHLER: Pruefskript fehlt: $missing — ein fehlender Check ist ein Fehlschlag, kein Ueberspringen." >&2
            rc=127
        else
            if ( cd "$ROOT/$wd_path" && sh -c "$cmd" ); then
                rc=0
            else
                rc=$?
            fi
        fi
        end=$(date +%s)
        secs=$((end - start))

        if [ "$rc" -eq 0 ]; then
            note='gruen'
        elif [ "$blocking" = "1" ]; then
            note='rot'
            failed=$((failed + 1))
        else
            # CI-Semantik: `continue-on-error` heisst nicht blockierend. Der
            # Fehlschlag wird gemeldet (Exit-Code, Hinweis), zaehlt aber
            # NICHT in `failed` und macht die Stufe nicht rot — der lokale
            # Runner darf nicht strenger sein als die CI.
            note='rot (nicht blockierend)'
            soft_failed=$((soft_failed + 1))
        fi
        summary="${summary}$(printf '  %-8s exit=%-4s %3ss  %-26s %s' "$id" "$rc" "$secs" "$note" "$cmd")
"
        if [ "$rc" -ne 0 ] && [ "$blocking" = "1" ]; then
            echo "FEHLER: $tier/$id ist fehlgeschlagen (Exit $rc): $cmd" >&2
            break
        fi
        if [ "$rc" -ne 0 ]; then
            echo "HINWEIS: $tier/$id ist fehlgeschlagen (Exit $rc), ist aber nicht blockierend (CI: continue-on-error): $cmd" >&2
        fi
    done <<EOF
$entries
EOF

    echo "Zusammenfassung (Stufe $tier):"
    printf '%s' "$summary"
    if [ "$floor" -gt 0 ] && [ "$executed" -lt "$floor" ]; then
        echo "FEHLER: floor nicht erreicht: nur $executed von $floor Checks ausgefuehrt — der Mindeststand (MIN_CHECKS=$floor) ist nicht erfuellt; das Ergebnis ist KEIN Pass." >&2
        return 1
    fi
    if [ "$soft_failed" -gt 0 ]; then
        echo "$tier: $executed Checks gestartet, $((executed - failed - soft_failed)) gruen, $failed rot, $soft_failed rot (nicht blockierend, CI: continue-on-error)"
    else
        echo "$tier: $executed Checks gestartet, $((executed - failed)) gruen, $failed rot"
    fi
    if [ "$failed" -ne 0 ]; then
        return 1
    fi
    return 0
}

case "${1:-}" in
    fast)
        assert_no_codegen || exit 1
        run_tier fast "$FAST_ENTRIES" "$MIN_CHECKS" || exit 1
        ;;
    slow)
        run_tier slow "$SLOW_ENTRIES" 0 || exit 1
        ;;
    all)
        assert_no_codegen || exit 1
        run_tier fast "$FAST_ENTRIES" "$MIN_CHECKS" || exit 1
        run_tier slow "$SLOW_ENTRIES" 0 || exit 1
        ;;
    list)
        print_list
        ;;
    list-ci)
        print_ci_mirror
        ;;
    help | -h | --help)
        usage
        ;;
    '')
        usage >&2
        exit 2
        ;;
    *)
        echo "FEHLER: unbekanntes Argument: $1" >&2
        usage >&2
        exit 2
        ;;
esac
