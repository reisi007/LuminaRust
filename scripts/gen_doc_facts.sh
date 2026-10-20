#!/bin/sh
# DOC-ZAHLEN-DRIFT-82 (Welle 1 + Welle 2): GENERATOR der Fakten-Bloecke in der
# Prosa.
#
# Zweck: „Eine Zahl in Prosa ist eine Kopie einer fremd gepflegten Tatsache
# und altert still." (Skill `generated-docs`.) Die Loesung ist nicht besseres
# Pflegen, sondern **erzeugen + diffen**: dieser Generator liest die Quellen
# (Textdateien, Dateisystem, committete Lockfiles) und liefert den fertigen
# Block-Text; `scripts/check_doc_facts_drift.sh` vergleicht ihn mit dem Block
# aus der Zieldatei und wird bei jeder Abweichung rot. Muster wie bei
# `scripts/gen_ci_tiers.sh` + `scripts/check_ci_tier_drift.sh`.
#
# SICHERHEITSREGEL: der Generator liest NUR TEXT und fuehrt NICHTS aus, das er
# aus einem gelesenen Dokument gelesen hat — kein `eval`, kein `sh -c` mit
# extrahierten Werten, kein `source` auf einer gelesenen Datei. Die gelesenen
# Dateien sind Eingabedokumente; wer ein daraus gelesenes Kommando ausfuehrt,
# baut aus einer Pruefung ein Ausfuehrungs-Freigabegate. Ausgefuehrt werden
# ausschliesslich Festplatzwerkzeuge (`wc`, `find`, `sort`, `awk`, `sed`,
# `grep`) mit festen Argumenten.
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
# **WELLE 2 (2026-10-20): HARTE REGEL — jeder Block ist git-frei.** Kein Block
# darf `git ls-files`, den Git-Index oder eine Umgebungsannahme brauchen. Die
# Quelle ist ausschliesslich eine **committete Datei im Repo** oder das
# Dateisystem. Der Beleg: `scripts/check_doc_facts_generator.sh` (Eintrag
# `fast-9`) laeuft den Generator in einer PATH-Sandbox **ohne** `git*` und
# vergleicht die Ausgabe byteweise mit dem Normalaufruf.
#
# KEINE handgepflegte Zahlen- oder Faktenliste: der Generator enthaelt keine
# Datei-, Zeilen- oder Mengenangabe. Er enthaelt nur
#   * das MANIFEST (Block-Id -> Zieldatei) — eine Zuordnung, keine Tatsache,
#   * die Pfade der Quellen,
#   * die beiden Schwellen, die er aus den GATE-Skripten liest (500 aus
#     `scripts/check_file_sizes.sh`, 3000 aus `scripts/check_file_ceiling.sh`),
#     damit die Werte nicht an zwei Stellen stehen,
#   * die **Regelnamen** der Bloecke (z. B. die Referenz-Stage `render_frame`
#     und der Batch-Pfad `batch/render_export_png` in `perf-baseline`), also die
#     Form der Regel — nicht deren Zahlen.
# Die Zeilenauswahl (welche Dateien/IDs in einen Block gehoeren) ist
# regelbasiert:
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
#   * `mcp-tools`  — den Aufrufen `tool_def(` in
#                     `crates/lumina-mcp/src/tools/mod.rs` (der Konstruktor
#                     `fn tool_def` steht ohne fuehrenden Leerraum und faellt
#                     damit heraus) und den Varianten des `enum Command` in
#                     `crates/lumina-cli/src/main.rs` (die Feature-Gate-Variante
#                     `Mcp` steht ohne Klammer und wird mitgezaehlt).
#   * `perf-baseline` — dem committeten Store `perf/baseline.json`: alle IDs
#                     mit Praefix `core/` (die Aufloesungs-Suffixe und die
#                     Reihenfolge ergeben sich aus dem Store), die Batch-ID
#                     (`batch/...`) je Aufloesung und die IDs mit Praefix
#                     `decode/raw__`.
#   * `crates`     — den `members`-Eintraegen in `Cargo.toml`.
#   * `surfaces`   — den Zeilen der §5-Basis-Tabelle in
#                     `feature/quality/security-review-register.md`.
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
# Repos). `LUMINA_PERF_BASELINE` ueberschreibt den Pfad des Baseline-Stores,
# aus dem der `perf-baseline`-Block liest (Default: IMMER `perf/baseline.json`
# des Repos). Die Haken existieren nur fuer den Rotbeweis „Quelle wandert";
# sie aendern nur, welche TEXTdatei gelesen wird. **Kein Haken darf den Gate
# gruen machen, ohne dass die Block-Quelle sichtbar aendert**: ein anderer
# Store erzeugt einen anderen Block — der Drift-Gate wird rot.
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
MCP_TOOLS_MOD="$ROOT/crates/lumina-mcp/src/tools/mod.rs"
CLI_MAIN="$ROOT/crates/lumina-cli/src/main.rs"
PERF_BASELINE="${LUMINA_PERF_BASELINE:-$ROOT/perf/baseline.json}"
WORKSPACE_TOML="$ROOT/Cargo.toml"
SURFACES_REGISTER="$ROOT/feature/quality/security-review-register.md"

# Regelnamen (Form der Regel, keine Zahlen):
#   * die Referenz-Stage, auf die sich die Anteile beziehen (`perf-baseline`),
#   * der Batch-Pfad, gegen den der Encode-Differenz-Anteil gerechnet wird,
#   * das Praefix der Cache-Stufen, die in EINER Zeile stehen (z. B.
#     `cache_hit` und `cache_miss` messen dieselbe Cache-Funktion),
#   * die Kopfzeile der §5-Basis-Tabelle (`surfaces`).
PERF_REFERENCE_STAGE="render_frame"
PERF_BATCH_PREFIX="batch/"
PERF_CACHE_PREFIX="cache_"
SURFACES_TABLE_HEADER="| Oberfläche |"

# MANIFEST: Block-Id | Zieldatei (relativ zur Repo-Wurzel).
# Eine Quelle der Wahrheit fuer Generator UND Drift-Gate: der Gate liest das
# Manifest hier, damit ein fehlender erwarteter Block ein Fehler ist und kein
# stilles Uebersehen.
MANIFEST='file-sizes|feature/quality/file-size-policy.md
population|feature/quality/file-size-policy.md
goldens|feature/README.md
goldens|feature/quality/golden-references.md
goldens|feature/quality/golden-fixtures.md
mcp-tools|feature/platform/mcp-server.md
mcp-tools|feature/quality/security-review-register.md
perf-baseline|feature/quality/performance-benchmarks.md
crates|feature/README.md
surfaces|feature/quality/security-review-register.md'

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
# Schmales Leerzeichen (U+2009) als Tausendertrennzeichen — die Formatvorgabe
# des Benchmark-Dokuments. Als Oktal-Escape geschrieben, damit die Unsichtbar-
# keit des Zeichens im Quelltext keinen Fehler einschleust.
THIN_SPACE=$(printf '\342\200\211')

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

# count_matches <regex> <datei> — Anzahl der Zeilen, die dem Regex entsprechen.
# `grep -c` liefert bei null Treffern Exit 1 — das ist hier kein Fehler,
# sondern ein leeres Ergebnis; der Aufrufer prueft die Zahl selbst.
count_matches() {
    re="$1"
    file="$2"
    n=$(grep -cE "$re" "$file" || true)
    case "$n" in
        ''|*[!0-9]*) die "die Zaehlung ($re) in ${file##*/} lieferte kein Ergebnis" ;;
    esac
    printf '%s\n' "$n"
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

# ----------------------------------------------------------------- mcp-tools
# Quelle: der Quelltext selbst (committet, git-frei).
#   * die registrierten MCP-Werkzeuge = die `tool_def(`-AUFUFRUFE in
#     `crates/lumina-mcp/src/tools/mod.rs`. Der Konstruktor `fn tool_def`
#     steht in derselben Datei, aber OHNE fuehrenden Leerraum — die Zaehlung
#     greift deshalb nur auf die Aufrufstellen (mit Leerraum) und zaehlt den
#     Konstruktor nicht mit. Die Existenz genau eines Konstruktors wird
#     geprueft: sonst waere die Zaehlregel nicht eindeutig.
#   * die `Command`-Varianten in `crates/lumina-cli/src/main.rs` = die
#     Zeilen `^    [A-Z][A-Za-z]*` INNERHALB des `enum Command`-Blocks. Die
#     Feature-Gate-Variante `Mcp` steht OHNE Klammer und wird von der
#     Zaehlung deshalb **mitgezählt**; die Zaehlung der Varianten MIT
#     Argument-Strukt (dieselbe Pipe, Muster mit `\\(`) zaehlt sie nicht.
# Beide Bloecke sind git-frei: sie lesen Dateien, die im Repo liegen.
gen_mcp_tools() {
    [ -f "$MCP_TOOLS_MOD" ] ||
        die "die MCP-Werkzeugliste fehlt: $MCP_TOOLS_MOD - die Zahl der registrierten Werkzeuge ist nicht ermittelbar"
    [ -f "$CLI_MAIN" ] ||
        die "die CLI-Quelle fehlt: $CLI_MAIN - die Zahl der Command-Varianten ist nicht ermittelbar"

    ctor=$(count_matches '^fn tool_def' "$MCP_TOOLS_MOD")
    [ "$ctor" = "1" ] ||
        die "in ${MCP_TOOLS_MOD##*/} steht nicht genau ein Konstruktor 'fn tool_def' - die Zaehlregel waere nicht eindeutig"
    tools=$(count_matches '^[[:space:]]*tool_def\(' "$MCP_TOOLS_MOD")
    [ "$tools" -gt 0 ] ||
        die "in ${MCP_TOOLS_MOD##*/} wurde kein tool_def-Aufruf gefunden"

    # Der `enum Command`-Block: von der Zeile `enum Command {` bis zur ersten
    # Zeile, die mit `}` beginnt (die schliessende Klammer des Enums).
    awk '/^enum Command \{/{f=1} f{print} f&&/^\}/{exit}' "$CLI_MAIN" > "$WORK/cmd_enum"
    [ -s "$WORK/cmd_enum" ] ||
        die "in ${CLI_MAIN##*/} wurde das 'enum Command' nicht gefunden"
    cmd_total=$(count_matches '^    [A-Z][A-Za-z]*' "$WORK/cmd_enum")
    cmd_args=$(count_matches '^    [A-Z][A-Za-z]*\(' "$WORK/cmd_enum")
    [ "$cmd_total" -gt 0 ] ||
        die "das 'enum Command' in ${CLI_MAIN##*/} enthaelt keine zaehlbare Variante"
    [ "$cmd_args" -le "$cmd_total" ] ||
        die "das 'enum Command' in ${CLI_MAIN##*/} ist widerspruechlich gezaehlt"

    # Die Variante OHNE Argument-Strukt (die ohne Klammer stehende) wird
    # benannt — sie ist der Grund, warum die Zaehlung `Mcp` mitzaehlt.
    bare=$(awk '/^    [A-Z][A-Za-z]*,?$/{ line=$0; sub(/,.*/,"",line); sub(/^ +/,"",line); print line }' "$WORK/cmd_enum")
    nbare=$(printf '%s\n' "$bare" | grep -c . || true)
    [ "$nbare" = "1" ] ||
        die "das 'enum Command' in ${CLI_MAIN##*/} hat $nbare Varianten ohne Argument-Strukt - die Zaehlregel waere nicht eindeutig"
    bare_name=$(printf '%s\n' "$bare" | head -n 1)

    printf '**MCP-Werkzeuge und CLI-Kommandos (Population).**\n\n'
    printf -- '- **Registrierte MCP-Werkzeuge:** **%s** (Zahlweg: die `tool_def(`-\n' "$tools"
    printf '  Aufrufe in `crates/lumina-mcp/src/tools/mod.rs`;\n'
    printf '  `grep -c "^[[:space:]]*tool_def(" crates/lumina-mcp/src/tools/mod.rs` — der\n'
    printf '  Konstruktor `fn tool_def` steht ohne fuehrenden Leerraum und zaehlt\n'
    printf '  **nicht** mit).\n'
    printf -- '- **`Command`-Varianten in `crates/lumina-cli/src/main.rs`:** **%s**\n' "$cmd_total"
    printf '  insgesamt — **%s** mit Argument-Strukt plus die Feature-Gate-Variante\n' "$cmd_args"
    printf '  `%s` (`#[cfg(feature = "mcp")]`), die **ohne** Klammer steht (Zahlweg:\n' "$bare_name"
    printf '  `awk "/^enum Command \\{/,/^\\}/" crates/lumina-cli/src/main.rs | grep -cE "^    [A-Z][A-Za-z]*"`\n'
    printf '  — `%s` **mitgezaehlt**; dieselbe Pipe mit `grep -cE "^    [A-Z][A-Za-z]*\\("`\n' "$bare_name"
    printf '  zaehlt die **%s** Varianten **mit** Argument-Strukt).\n' "$cmd_args"
    printf '\nQuelle: die beiden genannten Quelltext-Dateien (committet). Die Zahlen sind\n'
    printf 'eine Textableitung aus dem Code, keine Laufzeitmessung.\n'
}

# ------------------------------------------------------------- perf-baseline
# Quelle: der committete Store `perf/baseline.json` (Feld `benchmarks[].id`,
# `benchmarks[].median_ns`, `environment.recorded_at`). Git-frei: eine
# committete Datei, die in jeder Umgebung identisch liegt.
#
# Auswahlregel (regelbasiert, KEINE handgepflegte ID-Liste):
#   * alle IDs mit Praefix `core/` — die Aufloesungs-Suffixe (`__512`,
#     `__1024`, `__2048`) und die Reihenfolge der Stufen ergeben sich aus dem
#     Store selbst; die Anteile sind Wert / `core/render_frame__<dieselbe
#     Aufloesung>` bzw. Wert / `batch/render_export_png__<dieselbe
#     Aufloesung>`;
#   * die Batch-ID je Aufloesung (Praefix `batch/`) — daraus die
#     PNG-Encode-Zeile als Differenz `batch - render_frame`;
#   * alle IDs mit Praefix `decode/raw__` (zwei) — ohne Aufloesungs-Suffix,
#     darum als eigener Faktor-Block.
# Die Cache-Stufen (`cache_*`) stehen in EINER Zeile (dieselbe Cache-Funktion,
# zwei Messpunkte) — zwei Werte in einer Zelle.
gen_perf_baseline() {
    [ -f "$PERF_BASELINE" ] ||
        die "die Baseline-Datei fehlt: $PERF_BASELINE - die Kostenverteilung ist nicht ermittelbar"
    [ -r "$PERF_BASELINE" ] ||
        die "die Baseline-Datei ist nicht lesbar: $PERF_BASELINE - die Kostenverteilung ist nicht ermittelbar"

    cat > "$WORK/perf.awk" <<'PERF_AWK'
# Quellenparser + Tabellengenerator fuer `perf/baseline.json`.
# Regelbasiert: die Aufloesungen, die Stufen und ihre Reihenfolge kommen aus
# dem Store; keine ID-Liste im Programm.
function fint(x,   s, out, i, n, c) {
    s = sprintf("%.0f", x)
    out = ""
    n = length(s)
    for (i = 1; i <= n; i++) {
        c = substr(s, i, 1)
        if (i > 1 && (n - i + 1) % 3 == 0) out = out sep
        out = out c
    }
    return out
}
function fpct(v, b,   p, r) {
    if (b == 0) return "—"
    p = v * 100 / b
    r = sprintf("%.1f", p)
    if (p > 0 && (r + 0) == 0) return "~0 %"
    if ((p + 0) == 100) return "100 %"
    gsub(/\./, ",", r)
    return r " %"
}
function ffac(v, b,   r) {
    if (b == 0) return "—"
    r = sprintf("%.1f", v / b)
    gsub(/\./, ",", r)
    return r "×"
}
# Anteile einer Zeile (eine oder mehrere Stufen) bei einer Aufloesung,
# bezogen auf die Basis `baseid` (der ID-Pfad der Basis ohne Aufloesungs-
# Suffix, z. B. `core/render_frame` oder `batch/render_export_png`).
function shares_at(stages, nstage, resk, baseid,   out, i, s, first, allsame) {
    first = fpct(med["core/" stages[1] "__" resk], med[baseid "__" resk])
    allsame = 1
    for (i = 2; i <= nstage; i++) {
        s = fpct(med["core/" stages[i] "__" resk], med[baseid "__" resk])
        if (s != first) allsame = 0
    }
    if (allsame) return first
    out = ""
    for (i = 1; i <= nstage; i++) {
        s = fpct(med["core/" stages[i] "__" resk], med[baseid "__" resk])
        out = out (i > 1 ? " / " : "") s
    }
    return out
}
function ns_at(stages, nstage, resk,   out, i) {
    out = ""
    for (i = 1; i <= nstage; i++) {
        out = out (i > 1 ? " / " : "") fint(med["core/" stages[i] "__" resk])
    }
    return out
}
/"id"[ \t]*:[ \t]*"/ {
    line = $0
    sub(/^.*"id"[ \t]*:[ \t]*"/, "", line)
    sub(/".*$/, "", line)
    cur = line
    next
}
/"median_ns"[ \t]*:/ {
    line = $0
    sub(/^.*"median_ns"[ \t]*:[ \t]*/, "", line)
    sub(/[^0-9].*$/, "", line)
    if (cur != "") { med[cur] = line + 0; order[++n] = cur; cur = "" }
    next
}
/"recorded_at"[ \t]*:[ \t]*"/ {
    line = $0
    sub(/^.*"recorded_at"[ \t]*:[ \t]*"/, "", line)
    sub(/".*$/, "", line)
    recorded = line
    next
}
END {
    if (recorded == "") {
        print "gen_doc_facts: das Feld environment.recorded_at fehlt in der Baseline" > "/dev/stderr"
        exit 2
    }
    if (n == 0) {
        print "gen_doc_facts: die Baseline enthaelt keine Benchmarks" > "/dev/stderr"
        exit 2
    }
    # --- Aufloesungen: die Suffixe der core/-IDs ---
    nres = 0
    for (i = 1; i <= n; i++) {
        id = order[i]
        if (id !~ /^core\//) continue
        p = index(id, "__")
        if (p == 0) continue
        r = substr(id, p + 2)
        if (!(r in rseen)) { rseen[r] = 1; res[++nres] = r }
    }
    if (nres == 0) {
        print "gen_doc_facts: die Baseline enthaelt keine core/*-IDs" > "/dev/stderr"
        exit 2
    }
    # numerisch aufsteigend (mawk kennt kein asort)
    for (i = 2; i <= nres; i++) {
        v = res[i]; j = i - 1
        while (j >= 1 && (res[j] + 0) > (v + 0)) { res[j + 1] = res[j]; j-- }
        res[j + 1] = v
    }
    # --- Stufen (core/) in der Reihenfolge des Stores ---
    nstages = 0
    for (i = 1; i <= n; i++) {
        id = order[i]
        if (id !~ /^core\//) continue
        p = index(id, "__")
        if (p == 0) continue
        st = substr(id, 6, p - 6)
        if (!(st in sseen)) { sseen[st] = 1; stage[++nstages] = st }
    }
    # --- Batch-ID je Aufloesung ---
    nbatch = 0
    for (i = 1; i <= n; i++) {
        id = order[i]
        if (id !~ /^batch\//) continue
        p = index(id, "__")
        if (p == 0) continue
        r = substr(id, p + 2)
        if (r in batchid) {
            print "gen_doc_facts: mehr als eine batch/-ID je Aufloesung: " r > "/dev/stderr"
            exit 2
        }
        batchid[r] = id
        nbatch++
    }
    if (nbatch == 0) {
        print "gen_doc_facts: die Baseline enthaelt keine batch/-ID" > "/dev/stderr"
        exit 2
    }
    batchstage = ""
    for (i = 1; i <= nres; i++) {
        if (!(res[i] in batchid)) {
            print "gen_doc_facts: fuer die Aufloesung " res[i] " gibt es keine batch/-ID" > "/dev/stderr"
            exit 2
        }
    }
    # Der Batch-Pfad (ohne Aufloesungs-Suffix) ist die Regel-Konstante.
    bid = batchid[res[1]]
    p = index(bid, "__")
    batchstage = substr(bid, 1, p - 1)
    sub(/^batch\//, "", batchstage)
    # --- Referenz-Stage (die Basis der Anteile) ---
    ref = ""
    for (i = 1; i <= nstages; i++) if (stage[i] == refstage) ref = stage[i]
    if (ref == "") {
        print "gen_doc_facts: die Referenz-Stage " refstage " fehlt in der Baseline" > "/dev/stderr"
        exit 2
    }
    # --- Zeilen: die Stufen in Store-Reihenfolge; die Cache-Stufen in einer ---
    nrows = 0
    ncache = 0
    cache_row = 0
    for (i = 1; i <= nstages; i++) {
        st = stage[i]
        if (st ~ ("^" cachepfx)) {
            cachelist[++ncache] = st
            if (cache_row == 0) cache_row = ++nrows
            continue
        }
        rowlabel[++nrows] = st
        rowkind[nrows] = "single"
    }
    if (ncache > 0) {
        lbl = ""
        for (i = 1; i <= ncache; i++) lbl = lbl (i > 1 ? " / " : "") "`" cachelist[i] "`"
        rowlabel[cache_row] = lbl
        rowkind[cache_row] = "cache"
    }
    # --- Tabelle ---
    lastres = res[nres]
    printf "**Kostenverteilung je Pipeline-Stufe** — generiert aus `perf/baseline.json`\n"
    printf "(Feld `environment.recorded_at`, Stand **%s**).\n\n", substr(recorded, 1, 10)
    printf "Die Anteilsspalten rechnen Wert / `core/%s__<dieselbe Aufloesung>`\n", refstage
    printf "bzw. Wert / `batch/%s__<dieselbe Aufloesung>`. Format: Tausender-\n", batchstage
    printf "trennzeichen schmales Leerzeichen (U+2009), Dezimalkomma, eine\n"
    printf "Nachkommastelle; `100 %%`, `~0 %%` und `—` sind die Sonderfaelle.\n\n"
    printf "| Pipeline-Stufe | Quelle (ID in `perf/baseline.json`) | Median @%s (ns) | Anteil an `core/%s` (", lastres, refstage
    for (i = 1; i <= nres; i++) printf "%s%s", (i > 1 ? " / " : ""), res[i]
    printf ") | Anteil an `batch/%s` (", batchstage
    for (i = 1; i <= nres; i++) printf "%s%s", (i > 1 ? " / " : ""), res[i]
    printf ") |\n"
    printf "| --- | --- | ---: | ---: | ---: |\n"
    for (r = 1; r <= nrows; r++) {
        if (rowkind[r] == "cache") {
            nst = ncache
            for (i = 1; i <= ncache; i++) stlist[i] = cachelist[i]
            label = rowlabel[r]
        } else {
            nst = 1
            stlist[1] = rowlabel[r]
            label = (rowlabel[r] == ref) ? "`core/" ref "` (Gesamt)" : "`" rowlabel[r] "`"
        }
        src = ""
        for (i = 1; i <= nst; i++) src = src (i > 1 ? " / " : "") "`core/" stlist[i] "__" lastres "`"
        nscell = ns_at(stlist, nst, lastres)
        corecell = ""
        batchcell = ""
        for (k = 1; k <= nres; k++) {
            corecell = corecell (k > 1 ? " / " : "") shares_at(stlist, nst, res[k], "core/" ref)
            batchcell = batchcell (k > 1 ? " / " : "") shares_at(stlist, nst, res[k], "batch/" batchstage)
        }
        printf "| %s | %s | %s | %s | %s |\n", label, src, nscell, corecell, batchcell
    }
    # --- Encode-Differenz (batch - render_frame) je Aufloesung ---
    bidlast = batchid[lastres]
    ridlast = "core/" ref "__" lastres
    dsrc = "`" bidlast "` − `" ridlast "`"
    nscell = ""
    corecell = ""
    batchcell = ""
    for (i = 1; i <= nres; i++) {
        d = med[batchid[res[i]]] - med["core/" ref "__" res[i]]
        if (i == nres) nscell = fint(d)
        corecell = corecell (i > 1 ? " / " : "") fpct(d, med["core/" ref "__" res[i]])
        batchcell = batchcell (i > 1 ? " / " : "") fpct(d, med[batchid[res[i]]])
    }
    printf "| PNG-Encode (Δ `batch` − `%s`) | %s | %s | %s | %s |\n", refstage, dsrc, nscell, corecell, batchcell
    printf "\n**Decode-Klasse** (env-gated; die `decode/raw__*`-IDs tragen **kein**\n"
    printf "Aufloesungs-Suffix — die beiden CR3-Fixtures in voller Aufloesung):\n\n"
    printf "| ID | Median (ns) | Faktor zu `core/%s__%s` | Anteil an `batch/%s` |\n", refstage, lastres, batchstage
    printf "| --- | ---: | ---: | ---: |\n"
    ndec = 0
    for (i = 1; i <= n; i++) if (order[i] ~ /^decode\/raw__/) dec[++ndec] = order[i]
    if (ndec == 0) {
        print "gen_doc_facts: die Baseline enthaelt keine decode/raw__-ID" > "/dev/stderr"
        exit 2
    }
    for (i = 1; i <= ndec; i++) {
        printf "| `%s` | %s | %s | — |\n", dec[i], fint(med[dec[i]]), ffac(med[dec[i]], med["core/" ref "__" lastres])
    }
}
PERF_AWK

    if ! awk -v refstage="$PERF_REFERENCE_STAGE" \
             -v cachepfx="$PERF_CACHE_PREFIX" \
             -v sep="$THIN_SPACE" \
             -f "$WORK/perf.awk" "$PERF_BASELINE" > "$WORK/perf.out"; then
        die "die Baseline-Datei $PERF_BASELINE ist nicht auswertbar (Format oder Quelle geaendert)"
    fi
    cat "$WORK/perf.out"
}

# ------------------------------------------------------------------- crates
# Quelle: die `members` im Workspace-Manifest `Cargo.toml` (committet).
# Zaehlregel: die Eintraege des `members`-Arrays — keine handgepflegte Liste.
gen_crates() {
    [ -f "$WORKSPACE_TOML" ] ||
        die "das Workspace-Manifest fehlt: $WORKSPACE_TOML - die Zahl der Crates ist nicht ermittelbar"

    n=$(awk '
        /^[[:space:]]*members[[:space:]]*=/ { f = 1 }
        f {
            line = $0
            q = gsub(/"/, "\"", line)
            total += q
            if (index($0, "]") > 0) { print total / 2; exit }
        }
    ' "$WORKSPACE_TOML")
    case "$n" in
        ''|*[!0-9]*) die "in ${WORKSPACE_TOML##*/} steht kein auswertbares members-Array" ;;
    esac
    [ "$n" -gt 0 ] || die "in ${WORKSPACE_TOML##*/} steht das members-Array leer"

    printf '**%s** Crates im Workspace (Quelle: die `members` in `Cargo.toml`; Zahlregel:\n' "$n"
    printf 'die Eintraege des `members`-Arrays — keine handgepflegte Liste).\n'
}

# ----------------------------------------------------------------- surfaces
# Quelle: die §5-Basis-Tabelle in `feature/quality/security-review-register.md`
# (committet). Zahlregel: die DATENZEILEN dieser Tabelle — die Liste selbst
# bleibt handgepflegt, ihre Zeilenzahl wird abgeleitet (keine Handliste im
# Generator).
gen_surfaces() {
    [ -f "$SURFACES_REGISTER" ] ||
        die "das Security-Register fehlt: $SURFACES_REGISTER - die Zahl der Oberflaechen ist nicht ermittelbar"

    n=$(awk -v header="$SURFACES_TABLE_HEADER" '
        index($0, header) == 1 { f = 1; next }
        f && /^\|-/ { next }
        f && /^\| / { c++; next }
        f { print c + 0; exit }
        END { if (!f) print 0 }
    ' "$SURFACES_REGISTER")
    case "$n" in
        ''|*[!0-9]*) die "die §5-Basis-Tabelle in ${SURFACES_REGISTER##*/} ist nicht auswertbar" ;;
    esac
    [ "$n" -gt 0 ] ||
        die "die §5-Basis-Tabelle in ${SURFACES_REGISTER##*/} wurde nicht gefunden (0 Datenzeilen)"

    printf '**%s** Oberflaechen enumeriert (Quelle: die §5-Basis-Tabelle in\n' "$n"
    printf '`feature/quality/security-review-register.md`; Zahlregel: die Datenzeilen\n'
    printf 'dieser Tabelle — die Liste selbst bleibt handgepflegt).\n'
}

# --------------------------------------------------------------------- Ausgabe
gen_block() {
    case "$1" in
        file-sizes)  gen_file_sizes ;;
        population)  gen_population ;;
        goldens)     gen_goldens ;;
        mcp-tools)   gen_mcp_tools ;;
        perf-baseline) gen_perf_baseline ;;
        crates)      gen_crates ;;
        surfaces)    gen_surfaces ;;
        *)           die "unbekannter Block: $1" ;;
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
  (ohne Arg)  Alle Bloecke, je Block-Id EINMAL, in der Reihenfolge des Manifests
              — die Form zum Einfuegen in eine Zieldatei.

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
