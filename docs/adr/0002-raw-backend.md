# 0002: RAW-Backend — native LibRaw + Post-MVP libraw-wasm

**Status:** akzeptiert  
**Feature-IDs:** F-007, F-010  
**Datum:** 2026-08-17  

## Kontext

LuminaRust muss RAW-Dateien decodieren. Die Decodierung muss über einen
einheitlichen Vertrag (`decode_bytes` / `RawMetadata`) in allen Plattform-
pfaden (CLI, Desktop, Browser/WASM) funktionieren.

## Entscheidung

**Native Plattformen (MVP):** RAW-Decodierung über einen gekapselten
LibRaw-Adapter (`lumina-raw` über `vendor/libraw-sys`). Der Adapter
unterstützt CR2, CR3, NEF, ARW, DNG, ORF, RAF, RW2, CRW, PEF, SRW, 3FR,
IIQ, RWL, MOS, ERF, KDC und X3F einschließlich EXIF-Orientierung.

**Browser/WASM (Post-MVP):** Für die spätere Browser-Anbindung ist
`libraw-wasm` (Emscripten/npm) vorgesehen. Die Rust-Seite wird die JS-
`LibRaw`-Klasse als `wasm-bindgen`-Extern deklarieren und `open`/`metadata`/
`imageData` in `decode_bytes` bzw. `RawMetadata` übersetzen. Das Backend ist
hinter dem Feature `wasm-js` gekapselt und nur für
`cfg(target_arch = "wasm32")` aktiv.

**Vertrag:** Derselbe `decode_bytes`-Vertrag (Orientierung, Metadaten,
8/16-bit) gilt in beiden Backends.

**Lizenz (LibRaw):** Dreifach-lizenziert unter LGPL-2.1-or-later, CDDL-1.0
oder der permissiven **LibRaw Software License** („tri-license": der Nutzer
wählt eine der drei Optionen — aktualisiert 2026-08-20, R5 in
`feature/quality/fixtures-licensing.md`).
`vendor/libraw-sys` (FFI-Bindings) steht unter MIT. Die gewählte LibRaw-
Lizenz ist vor dem ersten Release (F-078) final zu prüfen.

## Alternativen

1. **Nur eigenes Decoder-Modul:** Hoher Implementierungsaufwand für alle
   RAW-Formate; fehlende Community-Unterstützung.
2. **rawloader oder andere Rust-Crates:** Weniger Raw-Format-Unterstützung
   als LibRaw; Reifegrad unklar.
3. **Sofortiges WASM-Backend:** Emscripten/npm-Integration ist im MVP noch
   nicht produktionsreif; verzögert den MVP.

## Konsequenzen

- `lumina-raw` kapselt den native/wasm32-Unterschied über `cfg()`-Kapselung;
  `lumina-core` bleibt plattformneutral.
- WASM-RAW ist im MVP deaktiviert (`RawError::UnsupportedPlatform`); die
  Capability-Matrix weist dies klar aus.
- Die Lizenzpflichten von LibRaw (LGPL-2.1-or-later, CDDL-1.0 oder LibRaw
  Software License — die permissive Option vermeidet jede Copyleft-Wirkung)
  sind vor dem ersten Release final zu prüfen und zu dokumentieren.
- Ein unabhängiger Verifizierungs-Agent prüft später die Backend-Konsistenz.

## Verweise

- `feature/decisions.md`
- `feature/platform/capability-matrix.md`
- `feature/platform/cli-gui-wasm.md`
- `vendor/libraw-sys/README.md`
