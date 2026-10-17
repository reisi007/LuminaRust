#!/bin/sh
# CORE-PROD-FS-FREE-GATE-76 (2026-10-17, Befund aus der Abnahme von
# LUMINA-CACHE-EXTRACT-70): die Produktions-Dateisystemfreiheit von
# `lumina-core` (Entscheid `CACHE-SCHICHT`, Befund R4) war eine gepruefte
# Behauptung, kein erzwungener Zustand — kein Gate wurde rot, wenn erneut
# `std::fs` in den Produktionscode von `lumina-core` geriet. Dieses Skript
# erzwingt die Regel.
#
# Gemessene Trefferlage am 2026-10-17 (HEAD 692f406): genau 6 Treffer, alle
# test-only — `lensfun_map.rs` Testmodul (Zeile 230ff), `render/tests/
# tca_isolation_tests.rs`, `lens_row_tests.rs`. Ein wasm-Ziel wird nicht
# gebaut (WASM ist gestrichen).
#
# Was geprueft wird: kein `std::fs` im Produktionscode von
# `crates/lumina-core/src`. Ein Treffer gilt als test-only, wenn eine der
# Regeln greift:
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
#   - Brace-Import: `use std::{fs, ...}` wird erkannt, aber `use std as x;`
#     + `x::fs::...` nicht — der Alias entzieht sich dem textuellen Scan.
#   - Regionsregel (c) setzt das Testmodul am Dateiende voraus und wuerde
#     ein mittiges `#[cfg(test)]`-Item ueberdecken.
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

    # Finde alle Treffer von std::fs (Direkt- und Brace-Import-Form) in dieser Datei
    grep -nE 'std::fs|std::\{[^}]*fs' "$file" 2>/dev/null | while IFS= read -r hit; do
        hit_line=${hit%%:*}
        hit_content=${hit#*:}

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
