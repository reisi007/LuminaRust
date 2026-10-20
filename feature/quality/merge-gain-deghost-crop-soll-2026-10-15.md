# SOLL — Helligkeitsangleich + Deghosting + Crop-to-valid (Merge-Pipeline)

**Status:** SOLL/Plan, kein Code. Code-only (keine Modelle, keine Owner-Gates).
**Datum:** 2026-10-15. **Branch:** `main`. **Analysemodell:** Autonomous Plan-Agent.
**Benennung:** folgt `feature/quality/`-Konvention `<slug>-<JJJJ-MM-TT>.md`
(vgl. `pipeline-crop-early-2026-10-13.md`, `mcp-matrix-parity-soll-2026-10-14.md`).

**Belegbasis (alles unten ist gelesen, nichts spekulativ; jede Angabe
verifiziert 2026-10-15):**

**Anker-Form (Stand 2026-10-20, Welle H zu `PROSE-ANCHOR-POLICY-88`):** die
Fundstellen nennen den vollständigen repo-relativen **Pfad + Symbol- bzw.
Testnamen** — **keine** Zeilennummern mehr. Die Zeilennummern dieses SOLLs
galten gegenüber dem **Plan-Stand `2aec896`** (2026-10-15) und wurden über die
Historie rekonstruiert (`git show 2aec896:<pfad> | sed -n '<zeile>p'` — der
umschließende Symbolträger). Alle genannten Symbole existieren **heute** noch
(`grep -n "<symbol>" <pfad>`); wo eine Datei seitdem umgezogen ist, nennt der
Anker den heutigen Pfad. Die inhaltlichen Aussagen dieses SOLLs sind
**unverändert** — umgestellt wurde nur die Anker-Form, nicht die Aussage.

- `crates/lumina-merge/src/merge.rs` (vollständig, 779 Z. im Plan-Stand
  `2aec896`): `merge_hdr_weighted` (Hut-Gewicht-Mittel über Radiance
  `pixel/rel`; kein Ghost-Test), `blend_panorama` (Union-Canvas; Lücken
  bleiben 0), `blend_panorama_transformed` (Lücken bleiben 0).
- `crates/lumina-merge/src/bundle.rs`: `run_merge`, `MergeRunOptions`,
  `align_and_merge`, `build_recipe`, `check_existing_bundle`.
- `crates/lumina-sidecar/src/merge_recipe.rs`: Schema (`MergeRecipe`,
  `MergeAlignment`, `MergeOutput`, `MergeTransform`, `MergeExposure`),
  `merge_digest` (Digest über `merge_version/mode/sources/alignment`),
  `validate_merge_alignment`, Grenzen (`MERGE_RECIPE_VERSION`,
  `MERGE_OUTPUT_BITS`).
- `crates/lumina-merge/src/lib.rs`: Scope-Dok (`//!`-Kopf: „No homography, no
  ghost removal", „missing EXIF exposure is `Unsupported`, never guessed from
  pixels"), `MergeError`, `HDR_SHIFT_WARN_PX`.
- `crates/lumina-core/src/merge_geom.rs`: `hdr_hat_weight`, `feather_weight`,
  `sample_bilinear` (Zero-Fill out-of-bounds).
- `crates/lumina-merge/src/align.rs`: `status_for_shift` (Residual =
  Shift-Betrag, Warnung nur `> HDR_SHIFT_WARN_PX`), `PANO_MIN_OVERLAP_PX`-Gate
  (in `estimate_pano_transform`).
- `crates/lumina-cli/src/merge.rs`: `MergeArgs` (`blend_width_px` Default 64),
  `run_merge`-Wiring, Residual-Warnung, CLI-JSON.
- `crates/lumina-mcp/src/tools/merge_common.rs`: `ALLOWED`, Defaults
  (`DEFAULT_BLEND_WIDTH_PX`), `run_mode`, `map_merge_run_error`,
  `outcome_payload`, `schema`; `merge_hdr.rs`/`merge_pano.rs`
  existieren als Einzeiler (vgl. SOLL `mcp-parity-c-soll.md` §4.3).
- `crates/lumina-stages/src/merge.rs`: `resolve_exposure`,
  `resolve_hdr_exposure`, `resolve_pano_exposure`.
- `crates/lumina-gui/src/merge_gui.rs`: `run_merge_sync` (baut
  `MergeRunOptions`, kennt heute nur
  `output/max_shift_px/blend_width_px/force`).
- Tests: `crates/lumina-cli/tests/merge_parity.rs`, `merge_e2e.rs` (799 Z.),
  `merge_mcp_parity*.rs` (4 Dateien) existieren.
- Vorgänger-SOLL-Stil: `feature/quality/mcp-parity-c-soll.md`
  (Handler-vs-Domäne-Trennregel §2, Fehlerklassen-Tabelle §3, JSON-Parität §5).

**Ist-Zustand (Lücken, code-belegt):**

1. `merge_hdr_weighted` (`crates/lumina-merge/src/merge.rs` →
   `merge_hdr_weighted`) mittelt Radiance mit Hut-Gewichten (`num/den`);
   bewegte Objekte mitteln sich ein — kein
   Geister-Test, kein Fallback auf Referenz (Scope-Dok `crates/lumina-merge/src/lib.rs`
   → `//!`-Kopf bestätigt
   „no ghost removal" als bewussten Stand).
2. `blend_panorama` (`crates/lumina-merge/src/merge.rs` → `blend_panorama`) und
   `blend_panorama_transformed` (`crates/lumina-merge/src/merge.rs` →
   `blend_panorama_transformed`) haben **keinen Gain-/Exposure-Parameter** im
   Blend; die
   Rezept-Exposure (`crates/lumina-merge/src/bundle.rs` → `run_merge`)
   fließt nur ins Rezept, nie in
   Pano-Pixel (`resolve_pano_exposure`, `crates/lumina-stages/src/merge.rs` →
   `resolve_pano_exposure`: „pixels
   ignore exposure").
3. Canvas = Union-Bounding-Box (`crates/lumina-merge/src/merge.rs` →
   `blend_panorama`, Union-Canvas-Block; transformed `crates/lumina-merge/src/merge.rs` →
   `blend_panorama_transformed`, Canvas-Block);
   unabgedeckte Pixel bleiben 0 (in beiden Blend-Funktionen). Es gibt kein
   Crop-to-valid.
4. Digest = `merge_version/mode/sources/alignment`
   (`crates/lumina-sidecar/src/merge_recipe.rs` → `merge_digest`);
   `output/created_at/status/error` sind
   bewusst ausgeschlossen (Test `crates/lumina-sidecar/src/merge_recipe.rs` →
   `digest_ignores_artefact_metadata`).

---

## A. Helligkeitsangleich (Gain Compensation) im Pano-Overlap

### A.1 Empfehlung: Statistik, nicht EXIF — mit Begründung

**Empfehlung: Median-Verhältnis der Luminanz im gemeinsamen Overlap gegen
Referenz-Frame 0 (Statistik-Pfad). EXIF wird NICHT als Gain-Quelle verwendet.**

Begründung (code-belegt):

- EXIF scheitert bei Auto-Exposure-Sprüngen zwischen Frames: `resolve_pano_exposure`
  (`crates/lumina-stages/src/merge.rs` → `resolve_pano_exposure`) behandelt
  Pano-Exposure heute ausdrücklich nur als
  Provenienz („pixels ignore exposure"), und `PANO_DEFAULT_*`
  (`crates/lumina-merge/src/bundle.rs` → `PANO_DEFAULT_EXPOSURE_S` /
  `PANO_DEFAULT_F_NUMBER`) zeigt, dass Pano-Quellen gar keine gültige
  EXIF-Exposure
  haben müssen. Ein EXIF-Gain wäre auf dem neutralen Default (0,01 s / 100 /
  f/8) aufgebaut — also auf einem erfundenen Wert — sobald eine Quelle ohne
  EXIF kommt.
- Der EXIF-Pfad ist zusätzlich fragil, weil Pano-`resolve_pano_exposure`
  Out-of-Range-Explizitwerte still auf EXIF/Default zurückfallen lässt
  (`crates/lumina-stages/src/merge.rs` → `resolve_pano_exposure`): ein
  EXIF-Gain würde auf still-normalisierten
  Eingaben rechnen, ohne dass der Aufrufer es merkt.
- Statistik misst, was wirklich da ist: Nach Alignment (`align_and_merge`,
  `crates/lumina-merge/src/bundle.rs` → `align_and_merge`) liegen die Frames im
  gemeinsamen Canvas-Koordinatensystem;
  das Median-Verhältnis der Luminanz im paarweisen Overlap (Frame i vs. Frame 0)
  ist robust gegen Outlier (Geister, Sättigung) und deterministisch
  (sortier-basiert, keine Zufallsanteile — passt zur
  Determinismus-Doktrin `crates/lumina-merge/src/lib.rs` → `//!`-Kopf).
- EXIF bleibt, was es ist: Rezept-Provenienz (`MergeExposure` je Quelle,
  `crates/lumina-sidecar/src/merge_recipe.rs` → `MergeExposure`). Der Gain wird
  *aus Pixeln geschätzt* und *als
  eigener Rezept-Wert* persistiert — keine Vermischung mit der EXIF-Provenienz.
  (Spannung zur Doktrin „never guessed from pixels", `crates/lumina-merge/src/lib.rs`
  → `//!`-Kopf: Diese gilt
  für die **HDR-Radiance-Normierung** (`relative_exposure`,
  `crates/lumina-merge/src/merge.rs` → `relative_exposure`);
  der Pano-Gain ist ein separater, explizit opt-in Blend-Parameter mit eigenem
  Flag — siehe A.3. Der SOLL markiert das bewusst als Doktrin-Ausnahme mit
  lautem Opt-in, nicht als stilles Uminterpretieren.)

### A.2 Exakte Code-Stellen

| Eingriff | Datei + Symbol | Was |
|---|---|---|
| Gain-Schätzung (neu, pure Funktion) | `crates/lumina-merge/src/pano_gain.rs` (neu; `crates/lumina-merge/src/merge.rs` extrahiert, neben `merge_hdr_weighted`) | `estimate_pano_gains(frames, matrices/offsets) -> Result<PanoGainEstimate, MergeError>` — Median-Verhältnis Luminanz im Overlap vs. Frame 0; Gain[0] = 1.0 per Definition |
| Gain-Anwendung vor Blend | `pano_gain.rs`-Kerne via `crates/lumina-merge/src/merge.rs` → `blend_panorama` (translations) + `crates/lumina-merge/src/merge.rs` → `blend_panorama_transformed` (inkl. `radiance`-Fallback) | Sample mit Gain multiplizieren (`sample * gain[i]`) **vor** `feather_weight`-Kombination; Seed/`acc`-Startwert ebenfalls gain-skaliert |
| Orchestrierung | `crates/lumina-merge/src/bundle.rs` → `align_and_merge` (Pano-Arm) + `crates/lumina-merge/src/bundle.rs` → `MergeRunOptions` | neues Options-Feld (A.3), Gain-Schätzung nach Alignment, vor Blend; Gains an `crates/lumina-merge/src/bundle.rs` → `build_recipe` durchreichen |
| Rezept + Digest | `crates/lumina-sidecar/src/merge_recipe.rs` → `MergeAlignment`, `merge_digest`, `validate_merge_alignment` | neues Feld `gain_compensation` (A.3); Digest-relevant (Teil von `alignment`, damit automatisch im Digest) |
| CLI | `crates/lumina-cli/src/merge.rs` → `MergeArgs` (Options-Mapping in `run_merge`) | neues Flag (A.3) |
| MCP | `crates/lumina-mcp/src/tools/merge_common.rs` → `ALLOWED`, `run_mode`, `schema` | neues Feld, Parität (A.5) |
| GUI | `crates/lumina-gui/src/merge_gui.rs` → `run_merge_sync` | Parameter durchreichen (GUI-Signatur-Erweiterung, Default = CLI-Default) |

### A.3 Neue/veränderte Typen + Validierung

```rust
// lumina-sidecar, crates/lumina-sidecar/src/merge_recipe.rs — neben MergeAlignment:
pub struct PanoGainCompensation {
    pub method: PanoGainMethod,   // C: `OverlapMedian` (einzige 1.0-Variante)
    pub gains: Vec<f64>,         // len == sources.len(), gains[0] == 1.0
    pub min_overlap_px: u64,     // Overlap-Pixel, auf denen die Statistik steht
}
// MergeAlignment erhält: pub gain_compensation: Option<PanoGainCompensation>
// (Option, nicht Default-Wert: Abwesenheit = Verhalten wie heute; explizit.)
```

Validierung (`crates/lumina-sidecar/src/merge_recipe.rs` →
`validate_merge_alignment`, neue Zweige):

- `gains.len() == sources.len` (sonst `Invalid` — Len-Mismatch nie still
  auffüllen/kürzen); `gains[0] == 1.0` exakt (Referenz-Definition).
- Jedes Gain endlich und `> 0.0` (sonst `Invalid`); **kein Clamp** — Out-of-Range
  wird abgelehnt, nie still begrenzt (Parallele zu `blend_width_px`-Doktrin,
  `mcp-parity-c-soll.md` §2 Pkt. 4: „nie geclippt").
- `min_overlap_px >= A_MIN_OVERLAP_STAT_PX` (neue Konstante, Vorschlag 1024;
  finaler Wert Implementierungs-Slice) — darunter lehnt die **Schätzung**
  laut ab (A.6), ein persistiertes Rezept darunter lehnt die **Validierung** ab.
- `deny_unknown_fields` bleibt (in `crates/lumina-sidecar/src/merge_recipe.rs`
  → `MergeAlignment` und `MergeRecipe`): alte Sidecars ohne das Feld
  parsen weiter (fehlendes `Option` = `None`); neue unbekannte Methodenwerte
  scheitern laut — konsistent zu `merge_version`-Pin
  (`crates/lumina-sidecar/src/merge_recipe.rs` → `validate_merge_recipe`).

Digest: Feld lebt in `MergeAlignment` → automatisch Teil von `merge_digest`
(`crates/lumina-sidecar/src/merge_recipe.rs` → `merge_digest`,
`"alignment": recipe.alignment`). Gains sind damit
Identität: gleiche Quellen + andere Gains = anderer Digest = `stale` statt
stillem Re-Use (`crates/lumina-merge/src/bundle.rs` → `check_existing_bundle`).

Interaktion mit `blend_width_px`:

- Gain wirkt **multiplikativ vor** der Feather-Kombination, `blend_width_px`
  steuert nur die **räumliche Rampe** (`feather_weight`,
  `crates/lumina-core/src/merge_geom.rs` → `feather_weight`).
  Keine Kopplung der Parameter: `blend_width_px == 0` (hard seam,
  `crates/lumina-merge/src/merge.rs` → `panorama_hard_seam_with_zero_blend_width`)
  + Gain = harte Kante zwischen gain-korrigierten Frames
  (dokumentieren, nicht verbieten).
- Gain-Schätzung nutzt das **volle geometrische Overlap**, nicht die
  Feather-Rampe (Rampen-Pixel sind blend-verzerrt und damit keine saubere
  Statistik-Basis). TODO: exakte Overlap-Definition für den
  `transformed`-Pfad (invers-transformierte Abdeckung vs. Bounding-Box-Schnitt)
  legt der Implementierungs-Slice fest; Kriterium: nur Pixel, die in beiden
  Frames echt sampeln (`sx/sy`-Bounds-Check, `crates/lumina-merge/src/merge.rs`
  → `blend_panorama_transformed`).

### A.4 Gain-Grenzen: kein stiller Clamp

- Schätzung liefert unbegrenzte positive Gains; **keine** Kappung in
  `estimate_pano_gains` und **keine** in der Anwendung.
- Stattdessen: Validierungs-Obergrenze `MAX_PANO_GAIN` (Vorschlag 8.0 ≈ ±3 EV;
  finaler Wert Implementierungs-Slice, als `pub const` neben
  `MAX_MERGE_BLEND_WIDTH_PX`, `crates/lumina-sidecar/src/merge_recipe.rs` →
  `MAX_MERGE_BLEND_WIDTH_PX`). Überschreitung =
  `MergeError::Unsupported` mit gemessenem Wert im Text („gain 12.4 exceeds
  8.0 — frames differ by more than 3 EV, refusing to guess").
- Begründung: Ein Gain > 8× signalisiert fast immer falsches Overlap
  (Fehl-Alignment), nicht echte Belichtungsdifferenz. Still clampen würde einen
  Alignmentschaden in einen Helligkeitsfehler verwandeln.

### A.5 CLI-Flag + MCP-Feld (Parität)

- CLI (`MergeArgs`, `crates/lumina-cli/src/merge.rs` → `MergeArgs`):
  `--gain-compensation <mode>`
  mit `default_value = "off"`, Werte `off | overlap-median`. (String-Enum statt
  Bool: erweiterbar für spätere Methoden, ohne Flag-Bruch.) Pano-only: bei
  `merge-hdr` mit Wert ≠ `off` laut ablehnen („gain compensation is panorama
  only") — spiegelt die `blend_width_px`-Asymmetrie-Doktrin
  (`crates/lumina-cli/src/merge.rs` → `MergeArgs`: HDR speichert nur
  Metadaten).
- MCP (`crates/lumina-mcp/src/tools/merge_common.rs`): Feld
  `gain_compensation: string | null`, Default
  `null`/`"off"`; `ALLOWED` +1; `schema()` + Property
  mit `enum: ["off", "overlap-median"]` (**beschreibend**, Durchgriff in
  `run_merge`, per Trennregel `mcp-parity-c-soll.md` §2); `run_mode`-Mapping
  auf dieselbe Options-Struktur wie CLI.
- GUI (`crates/lumina-gui/src/merge_gui.rs` → `run_merge_sync`):
  `gain_compensation`-Parameter an
  `run_merge_sync` (Default `"off"`); keine eigene Logik.
- Byte-Parität CLI≡MCP: gleiche Eingaben + gleiche `gain_compensation` ⇒
  identischer Digest + Checksum (Gains stehen im Digest, s. A.3).

### A.6 Fehlerklassen für Ablehnungen

| Situation | Klasse | Text-Präfix | Ort |
|---|---|---|---|
| Overlap-Pixel < `A_MIN_OVERLAP_STAT_PX` | `MergeError::Unsupported` → `merge unsupported:` | `gain compensation needs ≥ N overlap pixels, got M` | neue Schätzfunktion, `pano_gain.rs` |
| Gain > `MAX_PANO_GAIN` | `Unsupported` → `merge unsupported:` | `gain {g} exceeds {MAX}` (A.4) | dto. |
| `--gain-compensation` bei `merge-hdr` | `MergeRunError::Failed` → `merge failed:` (Options-Validierung, wie `max_shift_px < 0`, in `crates/lumina-merge/src/bundle.rs` → `run_merge`) | `invalid --gain-compensation …: panorama only` | `crates/lumina-merge/src/bundle.rs` → `run_merge` |
| Unbekannter Methoden-String (MCP/CLI) | Transport-Ebene: CLI clap-Usage / MCP `InvalidParams` (Shape, per Trennregel) | — | Frontend |
| Persistiertes Gain-Rezept verletzt Grenzen | `SidecarError::Invalid` → `merge failed:` (via `recipe.validate()`, in `crates/lumina-merge/src/bundle.rs` → `run_merge`) | `merge alignment: …` | `crates/lumina-sidecar/src/merge_recipe.rs` |

Prinzip: **laut ablehnen statt raten** — kein Fallback auf „Gain 1.0 + Warnung",
kein EXIF-Ersatzgain, wenn die Statistik nicht reicht.

### A.7 Testmatrix

| ID | Art | Ort | Prüft | Fixture |
|---|---|---|---|---|
| A-T1 | [U] | `lumina-merge` `tests/pano_gain.rs` | synthetische Helligkeitsrampe: Frame B = Frame A × 2.0 im Overlap ⇒ `gains == [1.0, 0.5]` (±1e-6); B = A ⇒ `[1.0, 1.0]` | 2× 32×24 Solid + Rampe, Offset (8,0) |
| A-T2 | [U] | dto. | Outlier-Robustheit: 10 % der Overlap-Pixel als Geist (s. B) verunreinigt ⇒ Median-Gain driftet < 1 % gegenüber sauberem Overlap (Mittelwert würde versagen — Negativ-Kontrolle im Test dokumentieren) | wie A-T1 + synthetischer Geist-Block |
| A-T3 | [U] | dto. | Negativ: Overlap < `A_MIN_OVERLAP_STAT_PX` ⇒ `Unsupported`, Text enthält Pixelzahlen | 8×8 Frames, Offset (7,0) |
| A-T4 | [U] | dto. | Negativ: Helligkeitsverhältnis 16× ⇒ Gain > MAX ⇒ `Unsupported` (kein Clamp: Output-Byte darf nie dem geclampten Wert entsprechen — Assert auf Fehler, nicht auf Pixel) | Solid 0.05 vs. 0.8 |
| A-T5 | [U] | `lumina-sidecar` | Rezept-Roundtrip mit Gain (byte-stabil, Muster `crates/lumina-sidecar/src/merge_recipe.rs` → `json_roundtrip_is_byte_stable`); Digest ändert sich bei Gain-Änderung (Muster `crates/lumina-sidecar/src/merge_recipe.rs` → `digest_changes_on_identity_change`); `None` parst auf alten JSON (Kompat, A.9) | `sample_recipe`-Derivat |
| A-T6 | [U] | `lumina-sidecar` | Validierungsmatrix: `gains.len` ≠ sources, `gains[0]` ≠ 1.0, Gain ≤ 0 / NaN / Inf, `min_overlap_px` < Minimum ⇒ je `Invalid` | wie `crates/lumina-sidecar/src/merge_recipe.rs` → `validation_matrix_rejects_every_error_path` |
| A-T7 | [P] | neue Datei `merge_gain_parity.rs` | CLI `--gain-compensation overlap-median` ≡ MCP `gain_compensation` ⇒ identischer `digest` + `checksum` + DNG-Bytes | A-T1-Fixture als PNG-Paar |
| A-T8 | [N] | `merge_gain_parity.rs`-Spiegel | `--gain-compensation overlap-median` bei `merge-hdr` ⇒ `merge failed:`; Ablehnung hinterlässt nichts (Snapshot-Vergleich) | 2 PNG |
| A-T9 | [P] | dto. | Default-Parität: ohne Flag ⇒ Digest **identisch zum ALTEN Stand** — KEIN Stale-Fenster, KEINE Golden-Regeneration nötig (A.9-Abweichung: `skip_serializing_if` ⇒ `None` fällt aus dem Digest-Input; T21-Pins laufen unverändert grün); No-Flag-Läufe bleiben pixel-identisch | neuer `merge_gain_parity.rs`-Anker + bestehende `merge_parity.rs`/T21-Anker (unverändert) |

### A.8 Abnahmekriterien (stub-tötend)

1. A-T1 grün mit exakten Gain-Werten (Stub „gibt `[1.0, 1.0]` zurück" scheitert).
2. A-T2: Median schlägt Mittelwert-Kontrolle (im Test beide rechnen, Assert nur
   Median-Toleranz — Stub mit Mittelwert scheitert).
3. A-T3/A-T4/A-T8: Fehlerklassen + Präfixe exakt (`unsupported:` vs. `failed:` —
   vertauschte Klassen scheitern).
4. A-T7: Byte-Parität CLI≡MCP inkl. DNG-Bytes.
5. A-T9: Ohne Flag kein Pixel-Drift gegen den regenerierten Stand (schützt bestehende Goldens nach der Migration; gegen alte Goldens scheitert der Digest per Design — Stale-Fenster).

### A.9 Migrations-/Kompatibilitäts-Aussage

- Rezept-Schema: `gain_compensation: Option<…>` — alte Sidecars parsen weiter
  (`None` = Verhalten wie heute).
- **ABWEICHUNG VOM SOLL-PLAN (Implementierungs-Slice A, entschieden und
  code-belegt 2026-10-15):** Die obige Planung sagte ein Digest-Stale-Fenster
  voraus („`None` serialisiert als `"gain_compensation": null` … jeder Digest
  ändert sich, sobald das Feld existiert"). Implementiert ist stattdessen
  `#[serde(default, skip_serializing_if = "Option::is_none")]`
  (`crates/lumina-sidecar/src/merge_recipe.rs`, `MergeAlignment.gain_compensation`):
  ein `None`-Feld
  fällt aus der kanonischen JSON-Serialisierung **vollständig heraus** (der Key
  existiert nicht, er wird nicht als `null` geschrieben). `merge_digest`
  (in `crates/lumina-sidecar/src/merge_recipe.rs` → `merge_digest`)
  serialisiert das typisierte `MergeAlignment` über
  `serde_json::json!` — der Digest-Input eines Rezepts ohne Gain ist damit
  **byte-identisch** zum prä-Slice-Input. Folge: **kein Stale-Fenster für
  Slice A** — bestehende Bundles bleiben `cached`-gültig, committete Goldens
  brauchen **keine** Regeneration, die gepinnten Digest-Anker (`merge_parity.rs`
  Hash-Anker, `merge_mcp_parity_gate.rs`-T21-Pins) laufen unverändert grün.
  Code-Beleg: `PanoGainMode::Off` bzw. fehlendes Feld ⇒
  `gain_compensation == None` ⇒ Key fehlt im JSON (A-T9 prüft
  `alignment.get("gain_compensation").is_none()` auf beiden Transporten;
  A-T5 prüft `absent.digest() == pre_gain_shape.digest()`; T21 läuft mit den
  ALTEN gepinnten Digests grün). Ein **gesetztes** Gain (≠ `None`) bleibt
  digest-relevant: gleiche Quellen + andere Gains = anderer Digest = `stale`
  (A-T5/A-T7).
- `MERGE_RECIPE_VERSION` bleibt 1 (Feld ist additiv-optional, kein
  Versionsbruch — `deny_unknown_fields` + `Option` = vorwärts-lesbar; ein
  unbekannter Methoden-String scheitert laut, A-T5).
- CLI-Default `off` ⇒ ohne Flag keine Verhaltensänderung **und** kein
  Digest-Drift (A-T9).

### A.10 Risiken + Rollback

- **Digest-Drift durch `Option`-Key** (A.9): **im Implementierungsstand
  aufgelöst** — `skip_serializing_if = "Option::is_none"` hält den Digest-Input
  eines Off-Laufs byte-identisch zum prä-Slice-Input (T21-Pins grün). Rest-
  Risiko: entfernt jemand das Attribut, kehrt das Stale-Fenster zurück —
  deswegen steht der Beleg in A-T5/A-T9 und in den gepinnten Ankern.
- **Median auf echtem Rauschen**: Overlap-Statistik auf homogenen Flächen
  (Himmel) kann durch Rauschen/Quantisierung streuen. Mitigation:
  `min_overlap_px`-Schwelle + dokumentierte Grenze (nur globale Gains, keine
  lokalen — Vignettierung wird NICHT korrigiert, s. Offene Fragen 3).
- **Interaktion Fehl-Alignment**: Gain schätzt auf falsch überlappenden Pixeln
  Müll. Mitigation: `MAX_PANO_GAIN`-Ablehnung (A.4) + bestehende
  `residual_warning`-Kette bleibt vorgeschaltet (Warnung, kein Abbruch —
  `crates/lumina-cli/src/merge.rs` → `run_merge`).
- **Rollback (ein Satz):** Schlägt A-T7 (Parität) oder A-T9 (No-Drift) fehl und
  erfordert eine Änderung an `merge_digest`, `feather_weight` oder der
  EXIF-Resolve-Kette, wird der Slice verworfen statt die bestehende
  Digest-/Paritäts-Referenz zu verbiegen.

---

## B. Deghosting im HDR

### B.1 Exakte Code-Stellen

| Eingriff | Datei + Symbol | Was |
|---|---|---|
| Geister-Maske (neu, pure Funktion) | `crates/lumina-merge/src/merge.rs` (neu, neben `merge_hdr_weighted`) | `deghost_mask(radiances, threshold) -> Vec<bool>` — pro Pixel: Radiance-Ausreißer-Test über Frames nach Alignment-Shifts |
| Gewichts-Eingriff | `crates/lumina-merge/src/merge.rs` → `merge_hdr_weighted` (Pixel-Schleife) | Maskierte Frames: Hut-Gewicht → 0; alle- maskiert ⇒ Fallback Referenz-Frame statt Mittel (heute: `rad_sum/len`) |
| Schwelle/Modus | `crates/lumina-merge/src/bundle.rs` → `align_and_merge` (HDR-Arm) + `crates/lumina-merge/src/bundle.rs` → `MergeRunOptions` | neue Options-Felder (B.3); Maske/Schwelle an `crates/lumina-merge/src/bundle.rs` → `build_recipe` |
| Rezept + Digest | `crates/lumina-sidecar/src/merge_recipe.rs` → `MergeAlignment`, `merge_digest`, `validate_merge_alignment` | neues Feld `deghost` in `MergeAlignment` (digest-relevant) |
| CLI / MCP / GUI | `crates/lumina-cli/src/merge.rs` → `MergeArgs` (+ Options-Mapping in `run_merge`); `crates/lumina-mcp/src/tools/merge_common.rs` → `ALLOWED`, `run_mode`, `schema`; `crates/lumina-gui/src/merge_gui.rs` → `run_merge_sync` | Flag + Feld + Parität (B.5) |

### B.2 Verfahren (deterministisch, code-only)

Nach Alignment (Shifts aus `estimate_hdr_translation`, `crates/lumina-merge/src/bundle.rs`
→ `align_and_merge`)
liegt pro Pixel und Kanal die Radiance je Frame vor
(`radiance = pixel / rel[i]`, `crates/lumina-merge/src/merge.rs` →
`merge_hdr_weighted`):

1. Pro Pixel/Kanal: Referenz-Radiance = Frame 0 (Referenz-Definition, konsistent
   mit „frame 0 at (0,0)"-Konvention, `crates/lumina-merge/src/merge.rs` →
   `blend_panorama`; die Konvention steht dort im Doc-Header: „frame 0
   conventionally at `(0, 0)`").
2. Abweichung `|radiance_i − radiance_0| / max(radiance_0, eps)` (relativ, damit
   helle und dunkle Bereiche vergleichbar; `eps`-Sockel gegen Division durch 0 —
   Wert legt Implementierungs-Slice fest, Vorschlag `1e-3`, dokumentiert).
3. Frame i gilt an diesem Pixel als Geist, wenn die Abweichung in **≥1 Kanal**
   den Schwellwert überschreitet (Kanäle nicht mitteln — Farb-Geister, z. B.
   rotes Auto vor grauer Wand, würden im Luminanz-Mittel untergehen).
4. Maskierte Frames: Gewicht 0 in `num/den` (`crates/lumina-merge/src/merge.rs` →
   `merge_hdr_weighted`). Bleibt nur
   Frame 0 übrig (oder alle maskiert): **Fallback Referenz-Frame**
   (`radiance_0`), nicht das heutige Mittel (dieselbe Funktion). Begründung: Das Mittel
   über Geister ist genau der Fehler, der entfernt wird; die Referenz ist die
   einzige definierte Wahrheit („first = reference", `crates/lumina-cli/src/merge.rs` →
   `MergeArgs`).
5. Determinismus: reine `f64`-Arithmetik über abbreviativ-iterierte Frames,
   keine Sortierung nötig (Schwellvergleich, kein Median) — same-machine
   bit-identisch (Doktrin `crates/lumina-merge/src/lib.rs` → `//!`-Kopf).

Dokumentierte Grenzen (was NICHT erkannt wird — laut in Modul-Dok + Plan):

- Bewegung **innerhalb** der Schwelltoleranz (langsame Objekte, weiche Schatten).
- Bewegung in **allen** Frames gleich (Referenz selbst enthält den Geist —
  kein Referenzvergleich kann das finden; Referenz-Wahl bleibt Verantwortung
  des Aufrufers, `inputs[0]`).
- Subpixel-Bewegung unterhalb der Bilinear-Interpolation
  (`sample_bilinear`, `crates/lumina-merge/src/merge.rs` →
  `merge_hdr_weighted`): interpolierte Mischpixel können unter
  der Schwelle bleiben.
- Gesättigte Bereiche (Hut-Gewicht 0, `crates/lumina-core/src/merge_geom.rs` →
  `hdr_hat_weight`): dort trägt kein
  Frame bei — Deghosting ändert nichts, Fallback-Regel greift wie heute.

### B.3 Empfehlung Schwellwert-Modell: parametrierbar mit Default, enge Range

**Empfehlung: parametrierbar (`--deghost <off|threshold:float>`, Default `off`),
Validierungs-Range `(0.05, 2.0]`, Default-Schwelle bei Opt-in 0.25 (25 %
relative Radiance-Abweichung).**

Begründung:

- Fix schlägt fehl an der Motiv-Varianz: 25 % passt für Stadt-Geister, reißt
  bei Rausch-Frames (dunkle Brackets) Piercing-Löcher. Parametrierbar + enge
  Range + laute Ablehnung außerhalb ist der etablierte Kompromiss des Repos
  (Muster `blend_width_px`: parametriert, `0..=65536`, nie geclippt,
  `crates/lumina-sidecar/src/merge_recipe.rs` → `MAX_MERGE_BLEND_WIDTH_PX` bzw.
  `validate_merge_alignment`).
- `off`-Default: HDR-Verhalten ohne Flag bit-identisch (Kompat §B.9, Test B-T8).
- Schwelle ins Rezept (Zahl, nicht nur Bool): Zwei Läufe mit Schwelle 0.25 vs.
  0.5 erzeugen verschiedene Pixel aus denselben Quellen — das **muss** den
  Digest ändern (Identität), sonst lügt der Cache.

Typen:

```rust
// merge_recipe.rs — in MergeAlignment:
pub deghost: Option<HdrDeghost>;
// wobei:
pub struct HdrDeghost { pub threshold: f64, pub reference_fallback: bool }
// reference_fallback immer true in 1.0 (Fallback-Politik ist fixiert, aber
// explizit persistiert — kein stilles Verhalten; zukünftige Politiken
// scheitern an Validierung, bis sie implementiert sind).
```

Validierung: `threshold` endlich, `0.05 < t <= 2.0` (sonst `Invalid`);
`reference_fallback == true` erforderlich (sonst `Invalid` — unbekannte Politik
laut ablehnen).

### B.4 Zusammenspiel mit `residual_warning`

- Orthogonal, beide bleiben: `residual_warning` (`crates/lumina-merge/src/align.rs` →
  `status_for_shift`,
  `HDR_SHIFT_WARN_PX = 16.0`, `crates/lumina-merge/src/lib.rs` →
  `HDR_SHIFT_WARN_PX`) misst **globale Alignmentschwäche**
  (Shift-Betrag); Deghosting misst **lokale Radiance-Ausreißer**. Ein großer
  Shift erzeugt viele Ausreißer — das ist korrekt (Fehl-Alignment ⇒ viele
  Geister ⇒ Referenz-Fallback an vielen Pixeln), kein Widerspruch.
- Reihenfolge: Alignment → Residual bestimmen (Warnung wie heute,
  `crates/lumina-merge/src/bundle.rs` → `align_and_merge`) → Merge mit Maske.
  Die Warnung blockiert nichts
  (Doktrin „never a silent abort", `crates/lumina-merge/src/lib.rs` → `//!`-Kopf);
  die Maske schützt die Pixel
  trotzdem.
- Sidecar/JSON: `warning`-Feld (`crates/lumina-mcp/src/tools/merge_common.rs` →
  `outcome_payload`) bleibt allein für
  Residual; Deghosting bekommt **kein** Warnfeld (es ist kein Warnzustand,
  sondern angefordertes Verhalten — Parameter steht im Rezept).

### B.5 CLI-Flag + MCP-Feld (Parität)

- CLI: `--deghost <mode>` mit `default_value = "off"`, Syntax
  `off | <threshold-float>` (z. B. `--deghost 0.25`). HDR-only: bei
  `merge-pano` mit Wert ≠ `off` laut ablehnen (`merge failed:`, wie A.6).
- MCP: Feld `deghost: string | number | null` — **TODO-Entscheidung**:
  String-Form (`"off"`/`"0.25"`) 1:1 wie CLI, oder `number|null`-Form
  (`null` = off)? Empfehlung des SOLL: **String-Form** (CLI-Text-Parität,
  keine zwei Parser). `ALLOWED` +1, `schema()` mit `anyOf` + Beschreibung der
  Range (**beschreibend**, Durchgriff `recipe.validate()`).
- GUI: `deghost`-Parameter an `run_merge_sync` (Default `"off"`).
- Byte-Parität CLI≡MCP über identische Schwellen-Strings.

### B.6 Fehlerklassen

| Situation | Klasse | Ort |
|---|---|---|
| Schwelle außerhalb `(0.05, 2.0]` / nicht-numerisch (CLI-seitig geparst) | CLI: Usage/`CliError::Message`; MCP: `InvalidParams` (Shape); Rezept-Ebene: `Invalid` → `merge failed:` | Frontend + `merge_recipe.rs` |
| `--deghost` bei `merge-pano` ≠ off | `merge failed:` (Options-Validierung, `crates/lumina-merge/src/bundle.rs` → `run_merge`-Muster) | `run_merge` |
| `reference_fallback == false` persistiert | `Invalid` → `merge failed:` | `validate_merge_alignment` |

Kein neuer `MergeError`-Variant: Masken-Logik scheitert nie eigenständig
(Schwellvergleich ist total); alle Ablehnungen sind Parameter-Validierung.

### B.7 Testmatrix

| ID | Art | Ort | Prüft | Fixture |
|---|---|---|---|---|
| B-T1 | [U] | `merge.rs` | synthetischer Geist: 3 Frames gleiche Radiance, Frame 1 mit hellem Block (1 Region) ⇒ Block-Pixel fallen auf Referenz zurück (±1e-6), Rest bleibt Mittel | 16×16 Solid + Block, gleiche Exposure, Shifts 0 |
| B-T2 | [U] | dto. | Schwellen-Sensitivität: Abweichung 10 % bei Schwelle 0.25 ⇒ kein Eingriff (Mittel wie heute); bei Schwelle 0.05 ⇒ Eingriff | wie B-T1, zwei Schwellen |
| B-T3 | [U] | dto. | Alle-maskiert-Fallback: alle Nicht-Referenz maskiert ⇒ Pixel == Referenz-Radiance (nicht Mittel!) | 2 Frames, Frame 1 vollflächig abweichend |
| B-T4 | [U] | dto. | Negativ-Dok: Abweichung unter Schwelle ⇒ bit-identisch zum heutigen Mittel (kein Over-Blocking) | B-T1 mit Schwelle 2.0 |
| B-T5 | [U] | `lumina-sidecar` | Rezept-Roundtrip + Digest-Sensitivität (Schwelle 0.25 ≠ 0.5 ≠ None); Validierungsmatrix (Schwelle 0/NaN/Inf/0.04/2.1, `reference_fallback: false`) | `sample_recipe`-Derivat |
| B-T6 | [P] | Parity-Spiegel | CLI `--deghost 0.25` ≡ MCP ⇒ identischer Digest/Checksum/DNG | B-T1-Fixture als PNG-Triplett |
| B-T7 | [N] | E2E-Spiegel | `--deghost 0.25` bei `merge-pano` ⇒ `merge failed:`; nichts hinterlassen | 2 PNG |
| B-T8 | [P] | Parity-Anker | ohne Flag ⇒ Digest identisch zum regenerierten Stand (Post-Migration, wie A-T9) | `merge_parity.rs`-Anker, nach Regeneration neu verankert |

### B.8 Abnahmekriterien (stub-tötend)

1. B-T1: Block-Pixel == Referenz (Stub ohne Maske mittelt ⇒ scheitert).
2. B-T3: Fallback == Referenz, nicht Mittel (Stub mit heutigem Mittel ⇒ scheitert).
3. B-T2: zwei Schwellen, zwei verschiedene Ergebnisse (Stub mit Fix-Schwelle ⇒ scheitert).
4. B-T6: Byte-Parität CLI≡MCP. 5. B-T8: No-Drift ohne Flag.

### B.9 Migration/Kompatibilität

- `deghost: Option<…>` — alte Sidecars parsen (`None` = heutiges Mittel).
  Digest-Key-Effekt wie A.9 (alle Bundles einmal `stale`) — gemeinsamer
  Migrationsplan §D.1 (ein Stale-Fenster für A+B+C zusammen, nicht drei).
- `MERGE_RECIPE_VERSION` bleibt 1.

- **ABWEICHUNG VOM SOLL-PLAN (Implementierungs-Slice B, entschieden und
  code-belegt 2026-10-15):** Die obige Planung sagte für B denselben
  Digest-Key-Effekt voraus wie A.9 („jeder Digest ändert sich, sobald das
  Feld existiert"). Implementiert ist stattdessen — das in A.9 bereits
  gewählte Muster — `#[serde(default, skip_serializing_if =
  "Option::is_none")]` an `MergeAlignment.deghost`
  (`crates/lumina-sidecar/src/merge_recipe.rs` → `MergeAlignment`): ein
  `None`-Feld fällt aus der kanonischen
  JSON-Serialisierung **vollständig heraus** (der Key existiert nicht, er
  wird nicht als `null` geschrieben). `merge_digest`
  (`crates/lumina-sidecar/src/merge_recipe.rs` → `merge_digest`) serialisiert
  das typisierte `MergeAlignment`
  über `serde_json::json!` — der Digest-Input eines Laufs ohne `--deghost`
  ist damit **byte-identisch** zum prä-Slice-Input. Folge: **auch B erzeugt
  kein Stale-Fenster**; bestehende Bundles bleiben `cached`-gültig,
  committete Goldens brauchen **keine** Regeneration, die gepinnten
  Digest-Anker (`merge_parity.rs`, `merge_mcp_parity*.rs`, T21-Pins) laufen
  unverändert grün (B-T8 prüft `alignment.get("deghost").is_none()` auf
  beiden Transporten; die Vollsuite `test -p lumina-merge -p
  lumina-sidecar -p lumina-stages -p lumina-mcp -p lumina-cli` läuft mit den
  ALTEN Pins grün). Ein **gesetztes** Deghosting bleibt digest-relevant:
  gleiche Quellen + andere Schwelle = anderer Digest = `stale` (B-T6 prüft
  0.25 vs. `off`, B-T5 die 0.25/0.5-Sensitivität auf der Rezept-Ebene).
  §D.1 Punkt 2 (Golden-Regeneration) entfällt für A **und** B.
- **Zweite dokumentierte Abweichung, gleicher Absatz (B.5-TODO, SOLL
  §D.4 Punkt 7):** Das MCP-Feld `deghost` ist ein **numerisches** Feld
  (`number` | `"off"` | `null`, `schema()` mit `anyOf`), nicht die vom SOLL
  empfohlene String-Form. Begründung: die CLI-Syntax ist bereits
  `off | <threshold-float>`, und ein Zahl-Feld lässt den Schwellwert
  unverändert über beide Transporte laufen (`--deghost 0.25` ≡
  `{"deghost": 0.25}`) — ein zweiter Text-Parser würde genau die Drift
  erzeugen, die die Paritätsregel §D.2 verbietet. Die Range wird im
  Handler als `InvalidParams` **und** in `recipe.validate()` als `Invalid`
  (`merge failed:`) geprüft — keine Clamp, kein Default (B-T5-Matrix).
- **Dritte dokumentierte Abweichung (Modul-Lage):** Maske und HDR-Pass
  liegen im Modul `crates/lumina-merge/src/hdr_deghost.rs` (Ratchet-getrieben,
  nicht wie in der §B-Tabelle genannt in `merge.rs`/`bundle.rs`); die
  Baseline-Kommentare und die Modul-Doku in `merge.rs`/`hdr_deghost.rs`
  dokumentieren den Move. **Stand 2026-10-20:** das Modul ist heute ein
  **Verzeichnis** `crates/lumina-merge/src/hdr_deghost/` (`mod.rs`, `merge.rs`,
  `mask.rs`); `merge_hdr_weighted_deghost` liegt in
  `crates/lumina-merge/src/hdr_deghost/merge.rs`, die Maske in
  `crates/lumina-merge/src/hdr_deghost/mask.rs`. Öffentlichkeit unverändert
  (`merge_hdr_weighted` = `deghost == None`-Spelling,
  `merge_hdr_weighted_deghost` = Vollpfad).
- `MERGE_RECIPE_VERSION` bleibt 1 (Feld ist additiv-optional, kein
  Versionsbruch — `deny_unknown_fields` + `Option` = vorwärts-lesbar; ein
  unbekannter Feldinhalt scheitert laut).
- CLI-Default `off` ⇒ ohne Flag keine Verhaltensänderung **und** kein
  Digest-Drift (B-T8).

### B.10 Risiken + Rollback

- **Over-Blocking bei Rauschen**: dunkle Brackets rauschen über die Schwelle ⇒
  Referenz-Fallback-Löcher. Mitigation: enge Range-Untergrenze 0.05 +
  dokumentierte Grenze + `off`-Default. Kein Auto-Tuning (kein Raten).
- **Referenz enthält den Geist**: prinzipiell unlösbar (B.2-Grenzen) — laut
  dokumentiert, kein Failover auf „beste Frame"-Heuristik.
- **Rollback (ein Satz):** Scheitert B-T1/B-T3/B-T6 und erfordert eine Änderung
  an `hdr_hat_weight`, `relative_exposure` oder der Hut-Mittel-Formel, wird der
  Slice verworfen statt die HDR-Referenz-Mathematik zu verbiegen.

---

## C. Crop-to-valid-Option für Pano

### C.1 Exakte Code-Stellen

| Eingriff | Datei + Symbol | Was |
|---|---|---|
| Valid-Rechteck (neu, pure Funktion) | `crates/lumina-merge/src/merge.rs` (neu, neben `blend_panorama`) | `valid_pano_rect(offsets, w, h) -> Option<(x0,y0,x1,y1)>` — Schnitt aller Frame-Rechtecke (integer Offsets); `transformed`-Variante aus inversen Matrizen/Bounding-Boxen |
| Canvas-Umschaltung | `crates/lumina-merge/src/merge.rs` → `blend_panorama` (Union-Canvas + Schleife) bzw. `crates/lumina-merge/src/merge.rs` → `blend_panorama_transformed` | Bei Flag: Canvas = Valid-Rechteck statt Union; kein Resampling, nur Ursprung + Dimension ändern |
| Orchestrierung | `crates/lumina-merge/src/bundle.rs` → `align_and_merge` + `crates/lumina-merge/src/bundle.rs` → `MergeRunOptions` | neues Options-Feld (C.3); Geometrie an `crates/lumina-merge/src/bundle.rs` → `build_recipe` + DNG-Writer (Dimension folgt automatisch aus `pass.merged`) |
| Rezept + Digest | `crates/lumina-sidecar/src/merge_recipe.rs` → `MergeAlignment`/`MergeOutput`, `merge_digest` | Canvas-Geometrie ist Identität (C.3) |
| CLI / MCP / GUI | wie A.2 | Flag + Feld + Parität (C.5) |

Berechnung (integer Pfad): `x0 = max(ox_i)`, `y0 = max(oy_i)`,
`x1 = min(ox_i + w)`, `y1 = min(oy_i + h)`; leer wenn `x1 <= x0 || y1 <= y0`
⇒ `Unsupported` (kein gemeinsames Rechteck — laut, kein leeres DNG).
Transformed-Pfad: Schnitt der transformierten Frame-Boxen (`bounds`,
`crates/lumina-merge/src/merge.rs` → `blend_panorama_transformed`); TODO:
Schnitt auf echten (rotierten) Footprints vs.
Box-Schnitt legt Implementierungs-Slice fest — Box-Schnitt ist konservativ
(eher kleiner, nie ungültig); exakte Footprint-Geometrie wäre größer, aber
komplexer. Empfehlung: Box-Schnitt (laut dokumentiert als konservativ).

### C.2 Empfehlung Default: Union wie heute (Kompatibilitäts-Begründung)

**Empfehlung: Default = Union-Verhalten wie heute (`--crop-valid off`
bzw. kein Flag); Crop-to-valid nur per explizitem Opt-in.**

Begründung:

- Jede existierende Pano-Golden (`crates/lumina-cli/tests/merge_e2e.rs` →
  `merge_pano_produces_wider_dng` prüft Canvas-Wachstum um
  Offset) und jeder existierende Digest hängt an der Union-Geometrie. Default
  `valid` würde **alle** Pano-Bundles still in ihrer Geometrie ändern — das verletzt die
  No-silent-Reinterpretation-Doktrin (`crates/lumina-sidecar/src/merge_recipe.rs` →
  `validate_mode_alignment`-Geist:
  „rejected loudly, no silent reinterpretation").
- Still wegfallende Randpixel sind Datenverlust ohne Zustimmung; wer cropt,
  soll es anfordern (laut dokumentiert, was wegfällt — C.4).
- Der Digest-Key-Effekt (C.6) tritt trotzdem ein (neues Options-Feld im
  Digest-Input), wird aber über §D.1 einmalig migriert statt pro Bundle
  schleichend.

### C.3 Typen + Validierung + Digest

```rust
// merge_recipe.rs — in MergeAlignment (Pano-Arm):
pub crop_valid: bool;   // false = Union wie heute; true = Schnitt-Rechteck
// + dokumentierte Geometrie im Rezept (Identität, ableitbar aber explizit):
pub canvas: MergeCanvas { pub width: u32, pub height: u32,
                          pub origin_x: i32, pub origin_y: i32 }
```

- `crop_valid: bool` (kein `Option`: Bool-Default `false` ist selbstdokumentierend;
  alte JSON ohne das Feld scheitern an `deny_unknown_fields`-invers — **TODO**:
  `#[serde(default)]` erforderlich für Rückwärts-Parse, explizit im
  Implementierungs-Slice testen (C-T5). Alternative `Option<bool>` vermeiden —
  dreiwertige Semantik (`None` vs `Some(false)`) wäre still-mehrdeutig.
- `canvas`: Breite/Höhe > 0, endlich; bei `crop_valid == true` muss
  `canvas == Schnitt(offsets/dims)` aus den Transforms ableitbar sein —
  Validierung prüft nur Form (kein Re-Alignment in der Validierung; die
  Gleichheit sichert der Parity-Test C-T6).
- Digest: beide Felder gehören in `MergeAlignment` (digest-relevant) — ausdrücklich
  NICHT in `MergeOutput`, das per `digest()`-Dok + `digest_ignores_artefact_metadata`-Test
  digest-exkludiert ist (sonst scheitert C-T5: gleiche Geometrie ⇒ gleicher Digest wäre
  dann nicht mehr garantiert).
  **Canvas-Geometrie ist Identität**: gleicher Inhalt auf anderem Canvas (Union
  vs. valid) ⇒ verschiedener Digest (sonst würde der Cache ein gecropptes DNG
  für ein Union-DNG ausgeben — byte-falsch).

### C.4 Kein stiller Beschnitt

- Bei Opt-in loggt `run_merge` (info, Muster `crates/lumina-merge/src/bundle.rs` →
  `run_merge`) das
  wegfallende Rechteck: `crop-valid: canvas {cw}x{ch} origin ({x0},{y0});
  removed left/right/top/bottom {l}/{r}/{t}/{b}px` — was wegfällt, steht im Log
  und (als `canvas`) im Rezept/Sidecar.
- Leerer Schnitt ⇒ `MergeError::Unsupported` („no common rectangle covered by
  all N frames") — nie ein 0-Byte-DNG, nie ein 1px-DNG.
- Kein Resampling: Crop verschiebt nur den Canvas-Ursprung und die Dimension;
  Pixelwerte im Rest-Rechteck sind **identisch** zum Union-Lauf (Assert in C-T1).

### C.5 CLI-Flag + MCP-Feld (Parität)

- CLI: `--crop-valid` (Bool-Flag, Default false; kein Wert — Muster `--force`,
  `crates/lumina-cli/src/merge.rs` → `MergeArgs`). Pano-only: bei `merge-hdr` laut ablehnen
  (`merge failed:` — HDR-Canvas ist immer Frame-Größe, Croppen sinnlos).
- MCP: Feld `crop_valid: boolean`, Default `false`; `ALLOWED` +1, `schema()`
  + Property. Keine Range-Domäne (Bool ist Shape: Nicht-Bool ⇒
  `InvalidParams`, per Trennregel).
- GUI: `crop_valid`-Parameter an `run_merge_sync` (Default `false`).
- Byte-Parität CLI≡MCP inkl. Canvas-Dimension im DNG
  (`encode_linear_dng`, `crates/lumina-merge/src/bundle.rs` → `run_merge` —
  Dimension folgt `pass.merged`).

### C.6 Testmatrix

| ID | Art | Ort | Prüft | Fixture |
|---|---|---|---|---|
| C-T1 | [U] | `crates/lumina-merge/src/merge.rs` | Lücken-Canvas: 2× 8×4, Offset (4,0) ⇒ Union 12×4 mit heutigem Verhalten; mit Flag ⇒ 4×4, Pixel **byte-identisch** zum Union-Ausschnitt x=4..8 | Solid A/B (vgl. `crates/lumina-merge/src/merge.rs` → `panorama_blend_range_and_seam`-Muster) |
| C-T2 | [U] | dto. | Vertikal-Versatz: Offset (4,2), 8×8 ⇒ valid 4×6 | dto. |
| C-T3 | [U] | dto. | Negativ: kein gemeinsamer Schnitt (z. B. dritter Frame disjunkt im Schnitt, aber paarweise überlappend) ⇒ `Unsupported` | 3 Frames Kette |
| C-T4 | [U] | dto. | Transformed-Pfad: Rotation 2° (Muster `crates/lumina-merge/src/merge.rs` → `pano_blend_transformed_applies_rotation`) ⇒ valid-Canvas ⊆ Union, alle Pixel aus echten Samples (kein 0-Fill im Output — Scan auf exakte 0, wo Abdeckung garantiert) | Rampen-Frame |
| C-T5 | [U] | `lumina-sidecar` | alte JSON (ohne `crop_valid`/`canvas`) parst mit Defaults; neue Roundtrips byte-stabil; Digest Union ≠ Digest valid bei gleichen Quellen | `sample_panorama_recipe`-Derivat |
| C-T6 | [P] | Parity-Spiegel | CLI `--crop-valid` ≡ MCP `crop_valid:true` ⇒ identischer Digest/Checksum/DNG-Bytes + Dimension | C-T1 als PNG-Paar |
| C-T7 | [N] | E2E-Spiegel | `--crop-valid` bei `merge-hdr` ⇒ `merge failed:`; nichts hinterlassen | 2 PNG |
| C-T8 | [P] | Anker | ohne Flag ⇒ Digest identisch zum regenerierten Stand (Post-Migration, wie A-T9) | `merge_parity.rs`-Anker (Pano-Teil), nach Regeneration neu verankert |

### C.7 Abnahmekriterien (stub-tötend)

1. C-T1: Dimension 4×4 + Byte-Identität zum Union-Ausschnitt (Stub, der nur die
   Dimension ändert ohne umzubasieren, scheitert am Pixelvergleich).
2. C-T3: `Unsupported` bei leerem Schnitt (Stub, der leeres DNG schreibt, scheitert).
3. C-T6: Byte-Parität CLI≡MCP. 4. C-T8: No-Drift ohne Flag.

### C.8 Migration/Kompatibilität

- Neue Pflichtfelder mit `#[serde(default)]` (TODO verifizieren, C-T5): alte
  Sidecars lesen weiter; neue Läufe ohne Flag erzeugen Union-Digests, die sich
  **trotzdem** von alten unterscheiden (Digest-Key-Effekt wie A.9) → §D.1.
- `MERGE_RECIPE_VERSION` bleibt 1.

### C.9 Risiken + Rollback

- **`#[serde(default)]` vs. `deny_unknown_fields`**
  (`crates/lumina-sidecar/src/merge_recipe.rs` → `MergeAlignment`/`MergeRecipe`):
  `deny_unknown_fields` lehnt *unbekannte* Felder ab, fehlende Felder mit Default
  parsen — kombinierbar, aber explizit testen (C-T5). Risiko klein, Test Pflicht.
- **Box-Schnitt zu konservativ** (C.1-TODO): Nutzer verlieren mehr Rand als nötig.
  Mitigation: als konservativ dokumentieren; exakte Footprint-Geometrie als
  Folge-Slice offen halten (kein Scope-Creep jetzt).
- **Rollback (ein Satz):** Scheitert C-T1 (Byte-Identität) oder C-T6 (Parität)
  und erfordert eine Änderung an `feather_weight`, `sample_bilinear` oder dem
  Union-Canvas-Pfad, wird der Slice verworfen statt den bestehenden
  Blend-Pfad zu verbiegen.

---

## D. Übergreifend

### D.1 Kompatibilitäts-Plan (ein Stale-Fenster für A+B+C)

Problem: Jedes neue Digest-Feld ändert den kanonischen Digest-Input
(`merge_digest`, `crates/lumina-sidecar/src/merge_recipe.rs` → `merge_digest`),
auch bei Default/`None` — alle
bestehenden Bundles werden einmal `stale` (`crates/lumina-merge/src/bundle.rs` →
`check_existing_bundle`), inkl.
Goldens mit pin-nierten Digests (`merge_parity.rs`, `merge_e2e.rs`,
`merge_mcp_parity*.rs`).

Plan (einmalig, gemeinsam für A+B+C — kein dreifaches Stale-Fenster):

> **Stand Slice A (2026-10-15):** A löst das Problem für sich **nicht** aus —
> `skip_serializing_if = "Option::is_none"` (A.9-Abweichung) hält den
> Digest-Input eines Off-Laufs byte-identisch zum prä-Slice-Input. A erzeugt
> deshalb keine Stale-Welle und keine Golden-Regeneration; der Plan unten
> gilt weiterhin für B und C, sofern deren Felder nicht ebenso serialisiert
> werden.

1. Alle drei Felder in **einem** Schema-Slice landen (ein Commit-Fenster, eine
   Digest-Änderung). Getrennte Slices würden drei aufeinanderfolgende
   Stale-Wellen erzeugen.
2. Goldens mit pin-nierten Digest/Checksum-Werten werden im selben Slice
   regeneriert (`--force`-Läufe) und neu verankert; der Commit vermerkt im
   Footer, dass Digests sich durch Rezept-Erweiterung (nicht durch Pixel-Änderung)
   geändert haben — No-Flag-Läufe bleiben pixel-identisch (A-T9/B-T8/C-T8 sichern das gegen den regenerierten Stand).
3. Cache-Semantik: `cached=true`-Fälle (`crates/lumina-merge/src/bundle.rs` →
   `check_existing_bundle`) vor der Migration
   bleiben gültig, bis ein neuer Lauf den neuen Digest schreibt; danach gilt der
   neue Digest. Kein manuelles Sidecar-Umschreiben (gegen
   `expected_route`-Analogie-Doktrin: Sidecars werden nie für Self-Validation
   modifiziert).
4. `MERGE_RECIPE_VERSION` bleibt 1 (additive Felder, kein Bruch); falls der
   Implementierungs-Slice feststellt, dass `deny_unknown_fields` + neue
   Pflichtfelder alte Sidecars unlesbar machen, ist Version 2 mit expliziter
   Ablehnung von v1 die Alternative — Entscheidung im Impl-Slice, nicht hier.

### D.2 Paritäts-Regel (verbindlich für A+B+C)

Trennregel aus `mcp-parity-c-soll.md` §2 gilt unverändert: Handler prüft nur
JSON-Shape/Typ (`InvalidParams`), jede Domänen-Regel (Ranges, Arity-Analogien,
Pano-/HDR-only-Gates, Gain-/Schwellen-Grenzen) lebt in `run_merge` /
`resolve_exposure` / `recipe.validate()` und surft als
`merge missing:|stale:|unsupported:|failed:`-Klasse über
`map_merge_run_error` (`crates/lumina-mcp/src/tools/merge_common.rs` →
`map_merge_run_error`) — CLI-stderr-Präfix und
MCP-`data.error` bleiben klassen-gleich (T17-Analogie: neue Fehlerpfade brauchen
je einen Klassen-Gleichheitstest, A-T8/B-T7/C-T7).

### D.3 Implementierungs-Reihenfolge (Vorschlag)

1. Schema-Slice (`lumina-sidecar`): drei Felder + Validierung + Tests
   (A-T5/A-T6/B-T5/C-T5). Digest-Änderung ab hier wirksam.
2. Math-Slice (`lumina-merge`): `estimate_pano_gains`, Deghost-Maske,
   `valid_pano_rect` + Unit-Tests (A-T1–A-T4/B-T1–B-T4/C-T1–C-T4).
3. Orchestrierungs-Slice (`bundle.rs` + Frontends): Options, Wiring, Flags,
   MCP-Schema, GUI-Parameter + Parity/Negativ-Tests (A-T7–A-T9/B-T6–B-T8/C-T6–C-T8).
4. Golden-Regeneration + Migrations-Footer (§D.1).

### D.4 Offene Fragen (TODO, kein Raten — keine ist code-belegt)

1. `A_MIN_OVERLAP_STAT_PX`: Vorschlag 1024 — finaler Wert braucht Messung auf
   Real-Frames (Rausch-Streuung des Medians vs. Overlap-Größe). Wer misst?
2. `MAX_PANO_GAIN`: Vorschlag 8.0 (±3 EV) — aus Alignmentschaden-Heuristik,
   nicht gemessen. Real-Serien (Auto-Exposure-Panos) zur Kalibrierung nötig;
   Fixture-Lücke analog `mcp-parity-c-soll.md` §7 (keine committete Pano-Reihe).
3. Vignettierung/lokale Gains: A korrigiert nur globale Gains. Lokale
   Vignettierungs-Korrektur ist explizit **nicht** enthalten — als Folge-Slice
   oder als dokumentierte Nicht-Abdeckung bestätigen?
4. Schwellen-Range `(0.05, 2.0]` + Default 0.25: unkalibriert (keine Real-Brackets
   im Repo). Kalibrierungs-Serie nötig, oder Range als vorläufig markieren?
5. `eps`-Sockel der relativen Abweichung (B.2): Vorschlag 1e-3 — Sensitivität in
   Tiefen hängt daran. Messung oder Konvention?
6. Box-Schnitt vs. Footprint-Schnitt (C.1): Box-Schnitt empfohlen (konservativ).
   Bestätigen, oder exakte Geometrie fordern?
7. MCP-`deghost`-Form (B.5-TODO): String (`"0.25"`) vs. Zahl — Empfehlung String
   (CLI-Parität). Bestätigen?
8. `canvas`-Pflicht vs. abgeleitet (C.3): explizit persistiert (empfohlen) oder
   aus Transforms ableiten (weniger Redundanz, aber Validierung müsste
   Re-Geometrie rechnen)? Empfehlung explizit — bestätigen?
9. GUI-Signatur-Bruch (`run_merge_sync`, `crates/lumina-gui/src/merge_gui.rs` →
   `run_merge_sync`): drei neue
   Parameter auf einmal vs. Options-Struct — GUI-Owner-Entscheid (kein Code
   in diesem SOLL).
10. Real-RAW-Gates für A/B/C (Belichtungsreihe mit Geist, Auto-Exposure-Pano,
    Lücken-Pano): keine Fixtures im Repo (Lücke belegt wie in
    `mcp-parity-c-soll.md` §7) — synthetische Fixtures tragen die Abnahme,
    Real-Gates bleiben `#[ignore]`-offen, bis Serien + Provenienz/Lizenz
    (`sample-data/raw/README.md`-Muster) vorliegen.
