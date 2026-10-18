#!/bin/sh
# CORE-PROD-FS-FREE-GATE-76 (2026-10-17, Befund aus der Abnahme von
# LUMINA-CACHE-EXTRACT-70; erweitert 2026-10-18, Befund
# CORE-FS-GATE-IMPORT-FORM-78): die Produktions-Dateisystemfreiheit von
# `lumina-core` (Entscheid `CACHE-SCHICHT`, Befund R4) war eine gepruefte
# Behauptung, kein erzwungener Zustand — kein Gate wurde rot, wenn erneut
# `std::fs` in den Produktionscode von `lumina-core` geriet. Dieses Skript
# erzwingt die Regel.
#
# Gemessene Trefferlage am 2026-10-17 (HEAD 692f406): genau 6 Treffer, alle
# test-only — `lensfun_map.rs` Testmodul (Zeile 230ff), `render/tests/
# tca_isolation_tests.rs`, `lens_row_tests.rs`. Ein wasm-Ziel wird nicht
# gebaut (WASM ist gestrichen). Nach der Erweiterung 2026-10-18 erneut
# gemessen (HEAD cd86d0e): unveraendert 0 Produktions-Treffer, 6 test-only.
#
# Was geprueft wird: kein `std::fs` im Produktionscode von
# `crates/lumina-core/src`. Drei Formen werden erkannt:
#   1. Direktform: jede Zeile, die `std::fs` enthaelt.
#   2. Klammer-Form, einzeilig: jede Zeile, die `std::{` und `fs`
#      enthaelt — auch ueber das `}` einer tiefer verschachtelten Gruppe
#      hinweg: `use std::{fs, io}`, `use std::{io::{Read}, fs}`,
#      `use std::{io::{Read, Write}, fs}`, `use std::{fs::{File}, io}`.
#      Praezision: gefordert ist `std::{` plus `fs`, nicht `use` — eine
#      Zeile wie `let x = std::{fs}` (gar kein Import) wird ebenso
#      gemeldet; bewusst Falsch-Positiv-Richtung, denn so eine Zeile
#      waere ohnehin ein echter Verstoss gegen die Dateisystemfreiheit.
#   3. Klammer-Form, mehrzeilig: eine Zeile eroeffnet eine Import-Gruppe,
#      wenn sie `std::{` enthaelt, davor `use` hat, kein `}` nach dieser
#      Position auf derselben Zeile steht und keine `//`-Kommentarzeile
#      ist — z. B. `use std::{` oder `let _ = S { a: 1 }; use std::{`.
#      Jedes Item, das mit `fs` beginnt (`fs`, `fs,`, `fs::{...}`,
#      `fs::Path,`, `fs as f,`), ist ein Treffer — auch das erste Item auf
#      der Oeffnerzeile selbst (`use std::{fs,` …) und beliebig viele Items
#      ueber Zeilengrenzen hinweg. Dem Treffer wird die Zeile des `fs`-Items
#      zugeschrieben (die Oeffnerzeile selbst, wenn `fs` ihr erstes Item
#      ist).
# Kommentar-Zeilen sind ausgeschlossen: Zeilen, deren erste nicht-leeren
# Zeichen `//` sind (`//`, `///`, `//!`), zaehlen weder als PROD noch als
# TEST. Ein Treffer gilt als test-only, wenn eine der Regeln greift:
#   (a) der Pfad enthaelt `/tests/` — Testdateien liegen in einem
#       tests/-Verzeichnis (Repo-Konvention);
#   (b) der Dateiname endet auf `_tests.rs` — Testdateien tragen das
#       _tests.rs-Suffix (Repo-Konvention);
#   (c) die Trefferzeile liegt nach der letzten Zeile auf Spalte 0, die auf
#       `#[cfg(` ... `test` ... `)]` passt — Konvention: Testmodul am
#       Dateiende.
#
# NAMED LIMIT, not a covered claim:
#   - Textueller Scan: erreicht kein `std::fs` ueber Re-Export/Alias aus einer
#     anderen Crate. `include!`/`#[path]`-Sonderfaelle haengen an den
#     Pfadregeln (a)/(b) — eine ueber `#[path]` eingebundene Testdatei in
#     einer Produktionsdatei wird nicht als Testdatei erkannt.
#   - `use std as x;` + `x::fs::...` — auch innerhalb einer mehrzeiligen
#     Klammer-Gruppe: der Alias entzieht sich dem textuellen Scan.
#   - Regionsregel (c) setzt das Testmodul am Dateiende voraus und wuerde
#     ein mittiges `#[cfg(test)]`-Item ueberdecken.
#   - Kommentar-Regel: ausgeschlossen sind nur `//`-Kommentar-Zeilen. Ein
#     `std::fs`-Vorkommen, das ausschliesslich in einem nachgestellten
#     `//`-Kommentar auf einer Code-Zeile steht, wird weiterhin gemeldet
#     (bewusst uebermeldend, Richtung echter Treffer sicher); `/* */`-
#     Blockkommentare werden nicht ausgeschlossen.
#   - Oeffner-Falle: liegt `std::{` innerhalb eines String-Literals, kann
#     eine Schein-Gruppe geoeffnet werden, die ein zusammenhangloses
#     `fs`-Item als PROD meldet. Zaehlende Oeffner brauchen `use` vor
#     dem `std::{` und sind keine `//`-Kommentarzeilen; Restfaelle (z. B.
#     `use` innerhalb eines Strings derselben Zeile, oder `std::{` in
#     einem `/* */`-Blockkommentar) bleiben Falsch-Positiv-Richtung —
#     echte Treffer werden dadurch nie unterdrueckt.
set -eu

root="${1:-.}"
src_dir="$root/crates/lumina-core/src"

if [ ! -d "$src_dir" ]; then
    echo "core_prod_fs_free FAIL: $src_dir nicht vorhanden"
    exit 1
fi

tmpfile=$(mktemp)
trap 'rm -f "$tmpfile"' EXIT

# Sammle alle Treffer mit Kontext: TEST oder PROD pro Treffer.
find "$src_dir" -name '*.rs' -type f | sort | while IFS= read -r file; do
    rel="${file#$root/}"

    # Regel (a): Pfad enthaelt /tests/
    case "$rel" in
        */tests/*) is_test_path=1 ;;
        *) is_test_path=0 ;;
    esac

    # Regel (b): Dateiname endet auf _tests.rs
    base=$(basename "$rel")
    case "$base" in
        *_tests.rs) is_test_name=1 ;;
        *) is_test_name=0 ;;
    esac

    # Regel (c): letzte Zeile auf Spalte 0, die auf #[cfg( ... test ... )] passt
    last_cfg_test=$(grep -n '^#\[cfg(.*test.*)\]' "$file" 2>/dev/null | tail -1 | cut -d: -f1)
    [ -z "$last_cfg_test" ] && last_cfg_test=0

    # Finde alle Treffer von std::fs in dieser Datei — drei Formen:
    #   grep: Direktform `std::fs` und Klammer-Form einzeilig (auch ueber
    #         `}` einer tiefer verschachtelten Gruppe hinweg).
    #   awk:  mehrzeilige Klammer-Form. Eine Zeile eroeffnet eine Gruppe,
    #         wenn sie `std::{` mit `use` davor enthaelt, kein `}` nach
    #         diesem `std::{` auf derselben Zeile steht und keine
    #         `//`-Kommentarzeile ist; jedes Item, das mit `fs` beginnt —
    #         auch das auf der Oeffnerzeile selbst —, ist ein Treffer.
    #         `//`-Kommentar-Zeilen sind weder Oeffner noch Items.
    # Beide Quellen werden nach Zeilennummer sortiert und pro Zeile
    # dedupliziert, damit dieselbe Trefferzeile nicht doppelt zaehlt.
    {
        grep -nE 'std::fs|std::\{.*fs.*\}' "$file" 2>/dev/null || true
        awk '
            {
                line = $0
                t = line
                sub(/^[ \t]+/, "", t)
                if (in_group) {
                    if (t !~ /^\/\//) {
                        u = t
                        sub(/[ \t,]+$/, "", u)
                        if (u ~ /^fs([^A-Za-z0-9_]|$)/) printf "%d:%s\n", NR, t
                        depth += gsub(/{/, "{", line) - gsub(/}/, "}", line)
                        if (depth <= 0) in_group = 0
                    }
                }
                if (!in_group) {
                    # Oeffner: `std::{` mit `use` davor auf derselben
                    # Zeile, kein `}` nach dieser Position (ein `}` davor,
                    # z. B. `let _ = S { a: 1 }; use std::{`, ist erlaubt).
                    # `use` davor schliesst String-/Kommentar-Fallen aus.
                    lp = 1
                    while ((fp = index(substr(line, lp), "std::{")) > 0) {
                        op = lp + fp - 1
                        rest = substr(line, op + 6)
                        if (index(rest, "}") > 0 || substr(line, 1, op - 1) !~ /use[ \t]/) {
                            lp = op + 6
                            continue
                        }
                        if (t !~ /^\/\//) {
                            # `rest` hat garantiert kein `}` (Oeffner-Bedingung);
                            # die oeffnende Klammer der Oeffner-Zeile selbst
                            # wird gezaehlt: 1 + gsub(...).
                            in_group = 1
                            depth = 1 + gsub(/{/, "{", rest)
                            # Auch der Teil der Oeffner-Zeile nach `std::{`
                            # selbst ist ein Item (`use std::{fs,` …).
                            rt = rest
                            sub(/^[ \t]+/, "", rt)
                            sub(/[ \t,]+$/, "", rt)
                            if (rt ~ /^fs([^A-Za-z0-9_]|$)/) printf "%d:%s\n", NR, t
                        }
                        break
                    }
                }
            }
        ' "$file"
    } | sort -t: -k1,1n | awk -F: '!seen[$1]++' | while IFS= read -r hit; do
        hit_line=${hit%%:*}
        hit_content=${hit#*:}

        # Kommentar-Zeilen (erste nicht-leere Zeichen `//`) sind weder PROD
        # noch TEST und fallen komplett aus der Zaehlung aus.
        trimmed="${hit_content#"${hit_content%%[![:space:]]*}"}"
        case "$trimmed" in
            //*) continue ;;
        esac

        if [ "$is_test_path" -eq 1 ] || [ "$is_test_name" -eq 1 ] || { [ "$last_cfg_test" -gt 0 ] && [ "$hit_line" -gt "$last_cfg_test" ]; }; then
            echo "TEST $rel:$hit_line"
        else
            echo "PROD $rel:$hit_line: $hit_content"
        fi
    done
done > "$tmpfile"

# Zaehle und bewerte
prod_count=$(grep -c '^PROD ' "$tmpfile" || true)
test_count=$(grep -c '^TEST ' "$tmpfile" || true)

if [ "$prod_count" -gt 0 ]; then
    echo "core_prod_fs_free FAIL ($prod_count Produktions-Treffer):"
    grep '^PROD ' "$tmpfile"
    exit 1
fi

echo "core_prod_fs_free OK (0 Produktions-Treffer, $test_count test-only)"
exit 0
