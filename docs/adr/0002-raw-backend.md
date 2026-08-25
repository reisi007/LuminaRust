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

**Lizenz (LibRaw):** Dual-lizenziert unter LGPL-2.1-or-later oder CDDL-1.0
(„dual-license": der Nutzer wählt eine der beiden Optionen). Die früher
zusätzlich angebotene permissive *LibRaw Software License* wurde upstream mit
v0.18 entfernt und existiert für aktuelle Versionen (0.22.2) nicht mehr — ein
„tri-license"-Verweis ist veraltet (R5, korrigiert 2026-08-25).
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
- Die Lizenzpflichten von LibRaw (LGPL-2.1-or-later **oder** CDDL-1.0, dual —
  die früher zusätzlich angebotene permissive *LibRaw Software License* wurde
  upstream mit v0.18 entfernt) sind vor dem ersten Release final zu prüfen und
  zu dokumentieren. Der gewählte Compliance-Pfad ist dynamisches Linken.
- Ein unabhängiger Verifizierungs-Agent prüft später die Backend-Konsistenz.

## Verweise

- `feature/decisions.md`
- `feature/platform/capability-matrix.md`
- `feature/platform/cli-gui-wasm.md`
- `vendor/libraw-sys/README.md`
