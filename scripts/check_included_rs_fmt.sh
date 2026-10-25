#!/bin/sh
# Format gate for Rust sources that `cargo fmt` cannot see.
#
# Defektklasse: unformatiert eingecheckter Rust-Code in einer Datei, die nur
# per `include!(...)` in den Modulbaum gelangt.
# Anlass: `crates/lumina-gui/src/app_struct.rs` haelt den Text von
# `LuminaApp` (Feld-Privatsphaere + 1000-Zeilen-Grenze, siehe
# `FILE-SIZE-1000-ALL-EXCEPTIONS-100`); `include!` ist kein Modul, deshalb
# folgt `cargo fmt --all -- --check` der Datei nicht. Ohne diesen Check
# koennte eine kuenftige Aenderung dort unformatiert landen, waehrend alle
# Gates gruen bleiben.
# Ratchet-Richtung: keine Zahl; der Check ist binaer und fuegt sich als Teil
# von `fast-1` in die bestehende Floor-Zaehlung ein (kein neuer Check).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUSTFMT="$(dirname "$(command -v cargo)")/rustfmt"
[ -x "$RUSTFMT" ] || RUSTFMT=rustfmt
"$RUSTFMT" --check --edition 2021 "$ROOT/crates/lumina-gui/src/app_struct.rs"
