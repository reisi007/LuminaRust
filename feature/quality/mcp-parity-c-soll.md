# SOLL — MCP-PARITY-C (HDR + Panorama, Release 2.0)

**Scope (eingegrenzt seit User-Order 2026-09-26):** Byte- und Verhaltens-Parität
zwischen `lumina merge-hdr` / `lumina merge-pano` (CLI) und zwei neuen
MCP-Tools, über dieselbe Eingabemenge, denselben Merge-Digest und denselben
DNG-Checksum-Anchor. Matrix bleibt eigener Sub-Slice (Abgrenzung, §7).

## 1. Ziel

- MCP erhält **zwei** path-basierte Tools (ein Aufruf = eine Merge-Job, **kein**
  `image_id`, kein Session-State): `lumina_merge_hdr` und `lumina_merge_pano`.
  Analogie: `lumina_generative`/`lumina_relocate` (MCP-PARITY-B,
  `crates/lumina-mcp/src/tools/mod.rs:95-118`).
- Beide Tools rufen **dieselbe** Orchestrierung wie CLI + GUI auf:
  `lumina_merge::bundle::run_merge` (CLI-Knoten
  `crates/lumina-cli/src/merge.rs:92-106`). Keine zweite Merge-Mathematik, kein
  zweiter DNG-Writer.
- Resultat: byte-identisches DNG + identischer Merge-Digest für dieselbe
  Eingabemenge über CLI, GUI-headless und MCP
  (`crates/lumina-cli/tests/merge_parity.rs:74-147`).

## 2. CLI + MCP verb mapping

| CLI-Flag (`MergeArgs`, `merge.rs:45-80`) | MCP-JSON-Feld (snake_case) | Typ / Notes |
| --- | --- | --- |
| `--input` (≥2, ≤256) | `inputs` | string[] — absolute Quellpfade, Merge-Reihenfolge; erstes = Referenz |
| `--output` | `output` | string \| null — default `<ref-stem>-HDR.dng` / `-Pano.dng` |
| `--exposure-times` | `exposure_times` | number[] — leer oder pro `--input` |
| `--isos` | `isos` | integer[] — leer oder pro `--input` |
| `--f-numbers` | `f_numbers` | number[] — leer oder pro `--input` |
| `--max-shift-px` (def. 16) | `max_shift_px` | integer, default 16 |
| `--blend-width-px` (def. 64) | `blend_width_px` | integer, default 64 |
| `--force` | `force` | boolean, default false |
| `--json` | (entfällt) | Ergebnis als `structuredContent` (digest/checksum/output/sidecar/cached/status) |

CLI-Subcommand → MCP-Tool: `merge-hdr` → `lumina_merge_hdr`,
`merge-pano` → `lumina_merge_pano`
(`crates/lumina-cli/src/main.rs:647-651`).
Fehlerklasse laut SOLL: `merge missing:` / `merge stale:` / `merge unsupported:` /
`merge failed:` (Kanonische Präfixe, `crates/lumina-cli/src/merge.rs:14-19`).
MCP mappt diese auf `McpError`-Varianten mit denselben stabilen `error`-Namen
(`FileNotFound`-analog / `Stale` / `Unsupported` / `MergeFailed`); keine
`debug!`-oder-stiller No-Op-Pfad.

## 3. Invarianten

1. **Eine Implementierung:** MCP nutzt `run_merge` (wie CLI `merge.rs:83-90`),
   kein eigenes Decode/Blend. F6-Dedup bleibt unangetastet.
2. **Decode-Kontext spiegelt die Quelle:** CLI-Adapter setzt `decoder` auf
   `"libraw"` wenn `raw.is_some()` sonst `"image"`, plus `libraw_decode_version()`
   bzw. `CARGO_PKG_VERSION`, `orientation` aus RAW-Metadaten
   (`crates/lumina-cli/src/merge.rs:172-180`). MCP-Adapter muss diese Werte aus
   denselben Routen beziehen, sonst driftet der Digest.
3. **Eingabe-Validierung vor Verarbeitung:** 2 ≤ `inputs.len()` ≤ 256;
   explizite Listen (`exposure_times`/`isos`/`f_numbers`) sind entweder leer oder
   genau `len(inputs)` (`check_explicit_list_lengths`,
   `crates/lumina-cli/src/merge.rs:142-155`). Abbruch laut, keine Teil-Ergebnisse.
4. **HDR-Exposure nie geraten:** explicit wins, sonst EXIF `exposure_time_s`/`iso`/
   `f_number`, sonst `unsupported`
   (`resolve_hdr_exposure`, `crates/lumina-cli/src/merge.rs:239-294`).
5. **Pano-Exposure ist nur Provenienz:** EXIF oder neutrale Defaults
   `PANO_DEFAULT_EXPOSURE_S/_ISO/_F_NUMBER`; Pixel-Merge nutzt Exposure nicht
   (`resolve_pano_exposure`, `crates/lumina-cli/src/merge.rs:302-340`).
6. **Stale/Missing ist hart:** fehlende Quelle/DNG/Sidecar → `missing`;
   Hash-/Decode-/DNG-Prüfsummen-Abweichung → `stale`; Re-Merge nur mit
   `force` (`crates/lumina-cli/src/merge.rs:14-19` + Persistenz-Invarianten in
   `feature/decisions/LRPAR-G13-MERGE-15.md` §Normative Invarianten).
7. **Originale unverändert; relative Pfade; atomarer Write; kein stiller
   Fallback** (dasselbe SOLL-Dokument §Normative Invarianten, Absolutpfade
   verboten).
8. **Rückgabe identisch:** MCP liefert `command/output/sidecar/digest/checksum/
   cached/status` 1:1 wie CLI-JSON (`crates/lumina-cli/src/merge.rs:123-135`).
   Cache-Hit (`cached=true`) schreibt nichts neu, exit-equivalent 0.
9. **Residual-Warnung erhalten:** `residual_warning` wird als `warning: ...` auf
   stderr (bzw. MCP `warnings[]`) gemeldet, nicht unterdrückt
   (`crates/lumina-cli/src/merge.rs:108-116`).

## 4. Fixtures / Goldens

- **Hash-Parity-Anker (synthetisch):** zwei 64×48 Graustufen-PNGs im Gradienten
  `level + x % 16` (`crates/lumina-cli/tests/merge_parity.rs:34-49`) mit
  expliziten Exposure/ISO/f-number-Werten; byte-stabiler Digest + DNG-Checksum
  (`merge_parity.rs:74-147`).
- **MCP-Parity-Test (neu, SOLL):** derselbe Anker aus MCP-Seite aufrufen
  (MCP-Adapter-Decode + `run_merge`) und `digest`/`checksum` mit CLI-Run
  vergleichen — Spiegel von `merge_parity.rs`, nur Transport differs.
- **Fehlerpfad-Fixtures:** leere Eingabe, 1 Quelle, Listenlängen-Mismatch,
  HDR ohne EXIF/Explicit, Out-of-Range-Werte (merge-eigene Unit-Tests in
  `merge.rs:374-413`); Expected `Unsupported`.
- **Real-RAW-Gate (Abnahme-Gate, Agents.todo.md MCP-PARITY-C):** mindestens
  eine echte Kamera-RAW-Belichtungsreihe (≥3 Frames) und eine Panorama-Reihe;
  PNG-Mocks decken I/O/Decoder nicht ab. Goldens als BLAKE3-Digest + DNG-Checksum.
- **Toleranzen:** keine für synthetische/quantisierte Pfade (deterministisch).
  Float-Toleranzen falls alignment-subpixel variieren — nur dokumentiert, falls
  `_noisy_`/` tolerance` nötig (siehe LRPAR-G13-MERGE-15 §Offene Risiken).

## 5. Akzeptanz

- Registry: `tool_names::TOOL_NAMES` + `list_tool_definitions` +
  `is_known_tool` um zwei Namen gewachsen, Zählguard
  (`crates/lumina-mcp/tests/tool_names/mod.rs:19-74`) + `mcp_onnx.rs` second pin
  grün. Kein doppelter Name.
- `tools/list` liefert beide Schemas (`additionalProperties:false`, `inputs`
  required, default-Werte dokumentiert) — MCP-Vertrag aus §2.
- MCP+CLI Produzieren denselben `digest`+`checksum` auf synthetischem Anker
  (SOLL-Test grün) und auf Real-RAW-Gate.
- Fehlerpfade loud: missing/stale/unsupported mit denselben Prefixen, keine
  silent partial, keine `cached`-Fälschung.
- `cargo test -p lumina-mcp` + `cargo test -p lumina-cli --test merge_parity`
  grün; `cargo fmt --all -- --check` grün. (Kein `cargo` während SOLL-Phase.)

## 6. Roadmap (SOLL → Impl → Verify)

1. **SOLL (dieses Doc):** Review/Sign-off; ggf. Tool-Namen finalisieren.
2. **Impl-A — geteilten Adapter freilegen:** Merge-Decode-Adapter aus
   `merge.rs:158-197` in `lumina-stages` (oder `lumina-merge`-Crate) ziehen,
   damit MCP exakt dieselbe Decode-Logik wie CLI nutzt (keine Kopie).
3. **Impl-B — Tool-Module:** `crates/lumina-mcp/src/tools/merge_hdr.rs`,
   `merge_pano.rs` angelegt, analog `generative.rs`; Register in
   `tools/mod.rs` (list + dispatch) und `tool_names/mod.rs` (2 Namen).
4. **Impl-C — Error-Mapping:** CLI-`MergeRunError` (Missing/Stale/Unsupported/Failed)
   → `McpError` mit stabilen Namen; Prefizes erhalten.
5. **Impl-D — Parity-Tests:** MCP-Seite Spiegel von `merge_parity.rs` +
   Listenlängen-/Exposure-Fehlerpfad-Tests; Real-RAW-Gate-Test (`#[ignore]`
   bis Fixture vorhanden).
6. **Verify:** CI:fmt + clippy + unit/parity + Real-RAW-Gate auf
   Referenzmaschine; Verifikationsbericht (End-to-End-Kette, Spez→Test-Mapping)
   gem. `DoD.md` §7.

## 7. Abgrenzung

- **Nicht Teil:** `matrix` (eigener Sub-Slice von MCP-PARITY-C), Denoise
  (`LRPAR-G14`), Cull, Smart-Collections, Auto-Tone.
- **Keine Änderung:** Merge-Mathematik, DNG-Writer, Sidecar-Schema, DNG-Tags,
  Alignment-Parameter; GUI nutzt weiter `run_merge` unverändert.
- **Kein Modellpfad:** Merge (HDR/Pano) ist modellfrei; „laut scheitern bei
  fehlendem Modell" aus Agents.todo gilt nicht für diese beiden Commands (nur
  für matrix/denoise/generative).
- **HDR-Tonemapping als Regler ist keine Merge-Stufe** (LRPAR-G13-MERGE-15
  §Ziel und Abgrenzung).
- **Kein `expected_route`-Anfassen** (gilt für matrix, hier irrelevant, aber
  analog: Merge-Sidecar wird nie für self-validation modifiziert).
- **Kein stiller No-Op:** leere/short Eingaben → lauter `Unsupported`, kein
  „Einzelbild als Merge tarnt sich".