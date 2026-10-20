# HDR + Panorama aus RAW als DNG — Status 2026-10-14 (MCP-PARITY-C Merge-Teil)

Status: **RAW-Pfad als Einzelpaar byte-identisch bewiesen.** `lumina merge-hdr` /
`lumina merge-pano` (CLI) und `lumina_merge_hdr` / `lumina_merge_pano` (MCP)
erzeugen für dieselbe RAW-Eingabemenge identische Digest-/Checksum-, DNG- und
Sidecar-Bytes — über denselben Adapter und dieselbe Orchestrierung wie die GUI.
**Nicht** bewiesen ist eine echte Belichtungsreihe bzw. Panorama-Serie (kein
solches Fixture im Inventar) und damit der T21-Serien-Zweig. Der Task
`MCP-PARITY-C` in `Agents.todo.md` bleibt offen; der Merge-Teil steht auf
„geliefert und verifiziert", Rest sind `matrix` als eigener Sub-Slice und das
Real-RAW-Serien-Gate.

**Anker-Stand:** die verbleibenden Zeilenangaben dieses Statusberichts sind
**Teil des Befunddatums** vom 2026-10-14 und beziehen sich auf den
Code-Stand `dc8f701`; sie sind ueber
`git show dc8f701:<pfad> | sed -n '<zeile>p'` rekonstruierbar. Die
pfadgebundenen Fundstellen stehen als **Pfad + Symbol- bzw. Testname**.

## 1. Fähigkeit

- HDR- und Panorama-Merge aus von `lumina-raw` dekodierbaren RAW-Dateien als
  lineares DNG plus Merge-Sidecar, auf **beiden** Transporten: CLI-Subcommands
  und MCP-Tools.
- **Eine Implementierung:** beide Frontends laufen über denselben
  Decode-/Exposure-Adapter (`crates/lumina-stages/src/merge.rs`, Commit
  `189b7d9`) und dieselbe Merge-Orchestrierung
  `lumina_merge::bundle::run_merge` (`crates/lumina-merge/src/bundle.rs` → `run_merge`) —
  wie die GUI (`crates/lumina-gui/src/merge_gui.rs` → `run_merge_sync`). Keine zweite
  Merge-Mathematik, kein zweiter DNG-Writer (SOLL §1, §6 Invariante 1).
- Exposition: HDR aus expliziten Listen, sonst RAW-EXIF, sonst laut
  `merge unsupported:`; Pano-Exposition ist reine Provenienz (EXIF oder neutrale
  Defaults) und für den Merge nicht nötig (SOLL §6 Invarianten 4/5).
- „Byte-identisch" heißt hier konkret: `digest`, `checksum`, DNG-Bytes und
  Sidecar-Bytes sind auf CLI und MCP gleich; `checksum` ist das blake3 genau
  der geschriebenen DNG-Datei.

## 2. Was bewiesen ist

### 2.1 Synthetische Parität T11–T20 (Default-Suite, läuft in jedem Gate)

Fixture: zwei 64×48-Graustufen-PNGs
(`crates/lumina-cli/tests/merge_parity.rs` → `write_png`); Prinzip je Test: CLI-Prozess
auf Baum 1 gegen MCP (in-process und über die `tools/call`-Naht) auf dem
byte-identischen Baum 2.

| ID | Aussage | Beleg (Datei:Zeile, Testfunktion) |
| --- | --- | --- |
| T11 | Payload 1:1 — genau die sieben Felder, kein Zusatzfeld | `merge_mcp_parity.rs:61` `t11_hdr_anchor_payload_is_the_cli_json_field_for_field` |
| T12 | DNG byte-identisch, re-importiert an der Ankergeometrie | `merge_mcp_parity.rs:129` `t12_the_anchor_dng_is_byte_identical_and_re_imports_at_64x48` |
| T13 | Sidecar byte-identisch, trägt das Merge-Envelope | `merge_mcp_parity.rs:178` `t13_the_anchor_sidecar_is_byte_identical_and_carries_the_merge_envelope` |
| T14 | Pano-Spiegel mit neutralen Defaults, Canvas wächst | `merge_mcp_parity.rs:236` `t14_pano_parity_with_neutral_defaults_and_a_canvas_that_grows` |
| T15 | Cache-Hit: `cached: true`, kein Byte neu geschrieben | `merge_mcp_parity.rs:293` `t15_a_second_identical_call_is_a_cache_hit_that_rewrites_no_byte` |
| T16 | Force/Stale-Kette: mutierte Quelle → laut `stale`, `force` re-mergt auf den CLI-Digest | `merge_mcp_parity.rs:357` `t16_a_stale_bundle_fails_loudly_until_force_re_merges_to_the_cli_digest` |
| T17 | Fehlerklassen-Gleichheit: CLI-Präfix, MCP-`data.error` und Code je Klasse identisch, Text identisch | `merge_mcp_parity_errors.rs:193` `t17_every_merge_error_class_and_text_matches_the_cli` |
| T18 | Jede Ablehnung hinterlässt nichts (Baum-Snapshot vorher == nachher) | `merge_mcp_parity_errors.rs:259` `t18_every_rejection_leaves_no_artifact_behind` |
| T19 | Quell-Bytes und Quell-Sidecars unberührt | `merge_mcp_parity_errors.rs:310` `t19_the_merge_touches_neither_the_sources_nor_their_sidecars` |
| T20 | Fremder/unbekannter Envelope wird 1:1 abgelehnt | `merge_mcp_parity_envelope.rs:47` `t20_a_foreign_or_unknown_envelope_is_rejected_identically` |

### 2.2 RAW-Pfad — T21, `#[ignore]` (`crates/lumina-cli/tests/merge_mcp_parity_gate.rs`)

Gegen die zwei committeten CR3-Fixtures (`sample-data/raw/README.md`:
`aircraft-landscape.cr3` 6032×4024 Orient. 1, `aircraft-portrait.cr3`
4024×6032 Orient. 8), im Zweig **ohne** `LUMINA_RAW_MERGE_SERIES`
(`committed_fixture_pair_parity`):

- **LibRaw-Dekodierung beider Fixtures** vor jeder Merge-Prüfung — belegt über
  den CLI-stderr-Log (`(6032x4024, libraw` / `(4024x6032, libraw`,
  `mixed_pair_refuses_identically_in_both_frontends`, ~:269-272).
- **EXIF-Exposure-Auflösung:** Pano läuft ohne jede Exposure-Liste über die
  RAW-EXIF-Provenienz; HDR mit expliziten Listen (explicit wins) und im
  EXIF-Fallback — beide Zweige resolved, `unsupported` tritt nicht auf
  (SOLL §6 Inv. 4/5).
- **`decode_context` je Quelle** = `decoder: "libraw"` +
  `libraw_decode_version()` + `orientation` aus den RAW-Metadaten (8, aus der
  Datei gelesen, nicht aus einem Literal) — SOLL §6 Invariante 2.
- **DNG-Writer + Byte-Parität:** DNG- und Sidecar-Bytes CLI ≡ MCP identisch;
  `checksum` == blake3 der DNG-Datei; die vier gepinnten Anker (2.4) halten auf
  **beiden** Transporten.
- **Cache-Hit auf RAW:** zweiter identischer Aufruf → `cached: true`,
  `status: ok`, gleicher Digest/Checksum, kein DNG-/Sidecar-Rewrite — in
  **beiden** Pipelines.
- **DNG-Re-Import** via `lumina_raw::decode_bytes` an der sichtbaren
  RAW-Geometrie.
- **Sidecar-Inhalt:** `type: merge`, `mode` hdr resp. panorama,
  `merge_version` 1, `status: ok`, `merge_artifact.checksum` == gemeldete
  Checksum, keine Absolutpfade.
- **T18 auf RAW:** eine Ablehnung lässt in beiden Bäumen kein Byte zurück
  (Snapshot vorher == nachher).

### 2.3 Laut-identisches Scheitern bei untauglicher Paarung

Gemischte Geometrie (6032×4024 vs. 4024×6032): beide Frontends scheitern an
derselben Merge-Domänen-Grenze — der Dimensionsprüfung **nach** LibRaw-Dekodierung
und Exposure-Auflösung — mit wortgleicher Clause:

> `merge unsupported: invalid merge input: hdr alignment needs equal dimensions, got 6032x4024 vs 4024x6032`

- CLI: Exit-Code != 0, Clause im stderr-Cause.
- MCP: `MergeUnsupported`, Code `-32012`, identisch in-process **und** über die
  `tools/call`-Naht; die Clause ist nach Tempdir-Normalisierung zwischen
  CLI und MCP gleich.
- Drei Durchgänge — HDR mit expliziten Listen, HDR per EXIF-Fallback (die
  committeten CR3s tragen EXIF-Belichtung), Pano ohne jede Exposure — bleiben
  in derselben Klasse; der stderr-Log belegt jeweils die vorherige
  LibRaw-Dekodierung.

### 2.4 Gepinnte Messwerte als Regressionsanker

Quelle der Hex-Werte: `crates/lumina-cli/tests/merge_mcp_parity_gate.rs`,
Pin-Block ~:375-385 (je Pin auf CLI- **und** MCP-Payload geprüft):

| Paar / Pipeline | digest | checksum |
| --- | --- | --- |
| Same-Frame-Paar, HDR (`frame-a.cr3`/`frame-b.cr3`) | `blake3:2c421f7ddacd5f609614bcd00d41fb2166fe6cb0f15a83695c97d3b0b0f42b97` | `blake3:a465795c8bdf7ae572a2dbc3064e2be74c2adbf4e4ccbc72df82000dafecfe78` |
| Same-Frame-Paar, Pano | `blake3:8d0e44abc0e3d265bf5d345bb43590215677ac00455d14057cf66d75ccd8c6bd` | `blake3:245b915218fad32d9135d9cf4c144e474628cb806c44cdff70817341fc93d602` |

Pin-Voraussetzungen — jede dieser Änderungen bricht die Pins **absichtlich
sichtbar**:

- Fixture: `aircraft-portrait.cr3` in **beiden** Slots, kopiert unter den Namen
  `frame-a.cr3` / `frame-b.cr3` — die **Quelldateinamen gehen ins digestierte
  Rezept**.
- `--max-shift-px 0` (identische Frames haben keinen Shift) und
  `--blend-width-px` 64 (Default).
- HDR mit expliziten Exposure-Listen (0,001/0,00125 s, ISO 1000/1250, f/8);
  Pano über RAW-EXIF-Provenienz.
- `merge_version` 1; LibRaw **0.22.2**, CI-gepinnt
  (`.github/workflows/ci-libraw-image.yml`) — ein LibRaw-Wechsel ist ein
  absichtlicher, sichtbarer Digest-Wechsel, kein stiller Drift.

## 3. Wie es läuft

| Kommando | Was es beweist | Kosten |
| --- | --- | --- |
| `cargo test -p lumina-cli --test merge_mcp_parity_gate -- --ignored` | den RAW-Pfad: gemischte Paarung (lautes Scheitern) + Same-Frame-Paar (ganze RAW→DNG-Strecke, beide Pipelines, beide Transporte) | ~5–6 min |
| `LUMINA_RAW_MERGE_SERIES=<dir mit ≥ 2 RAW-Frames>` dito | den Echte-Serie-Zweig (SOLL §7, T21 [G]) — braucht ein Fixture, das es noch nicht gibt | dito |
| Default-Suite des Slice (`cargo test -p lumina-mcp -p lumina-cli -p lumina-stages -p lumina-merge`) | T11–T20 + Registry/Validierung | Verifikationslauf 2026-10-14: **543 passed / 0 failed / 2 ignored**, ~34 s; die Datei dokumentiert 66 s für diese 4-Crate-Variante |

**Kostennachweis für `#[ignore]`:** ein RAW-Merge kostet im Debug-Profil ~50 s
**pro Frontend** — zwei 24-MP-LibRaw-Dekodierungen, die Alignment-Pässe, ein
145-MB-lineares DNG-Encode (`merge_mcp_parity_gate.rs:4-12,51-59`); der
vollständige CLI≡MCP-Beweis damit ~5 min gegen ein Default-Gate von ~34 s
bzw. 66 s. Die zwei Ignorierten der Default-Suite sind dieser T21-Gate und
`real_matrix_against_committed_goldens` (`crates/lumina-cli/tests/matrix_e2e.rs` → `real_matrix_against_committed_goldens`).
Frischheits-Hinweis: nach neuen Testdateien unter `crates/lumina-cli/tests/`
braucht `scripts/check_test_orphans.sh lumina-cli` einen frischen
`cargo test`-Lauf (crate-weite Freshness-Liste).

## 4. Was offen bleibt

1. **Echte Belichtungsreihe / Panorama-Serie fehlt (Abnahme-Gate).** Die zwei
   committeten CR3s sind **keine** Reihe — verschiedene Motive, 1/1000 s /
   ISO 1000 bzw. 1250 (`sample-data/raw/README.md`; SOLL §7, §13 Pkt. 3). Der
   T21-Serien-Zweig bleibt deshalb `#[ignore]`-offen; bewiesen ist der **Pfad**,
   nicht die **Serie**. Provenienz/Lizenz muss der Eigentümer nach dem Muster
   von `sample-data/raw/README.md` (Provenienz-Block, R1/F-078,
   `feature/quality/fixtures-licensing.md` §4) stellen.
2. **Kostenwarnung 24-MP-SAD-Suche im Debug-Profil.** ~50 s pro Frontend pro
   Lauf — auf Referenzmaschine/CI einplanen, nicht ins Default-Gate.
3. **SOLL-§13-Eigentümerfragen, die den Merge betreffen** (wortgenauer Stand:
   `feature/quality/mcp-parity-c-soll.md` §13):
   - **Pkt. 2 — `mode`-Zusatzfeld:** `MergeRunOutcome` hat es
     (`bundle.rs:279`), das CLI-JSON gibt es nicht aus. Eins-zu-eins oder
     additive Abweichung? (Additiv würde CLI und MCP textuell unterscheiden.)
   - **Pkt. 4 — oberes Limit `max_shift_px`:** im Code gibt es nur `>= 0`
     (`bundle.rs:370-375`, `crates/lumina-merge/src/align.rs` →
     `estimate_hdr_translation`/`estimate_pano_transform`), kein dokumentiertes Maximum.
     Schema ohne `maximum` (Vertrag folgt dem Code) oder bewusstes Limit?
   - **Pkt. 5 — Form der Residual-Warnung:** Feldname `warning` vs.
     `warnings[]`, und genau welcher Text (CLI-Zeile `crates/lumina-cli/src/merge.rs` →
     `run_merge` vs. gekürzt)? Ein `warnings[]`-Array wäre die einzige Stelle, an der MCP ein
     Feld mehr hat als die CLI.
   - **Pkt. 6 — CLI-Erfolgssatz im `content[0].text`:** `tool_result_response`
     serialisiert automatisch das Payload; soll der CLI-Satz `merged <path>` /
     `merge already current: <path>` (`crates/lumina-cli/src/merge.rs` →
     `run_merge`) ergänzend erscheinen?
   - **Pkt. 8 — `structuredContent`-Immer-Gefüllt-Annahme:** der CLI erzwingt
     `--json` nicht (Default ist der menschenlesbare Satz); Parität ist damit
     nur über `--json` definiert — zu bestätigen, dass MCP-seitig
     `structuredContent` immer gefüllt ist (die Annahme in §5; durch den
     T11-Grader belegt, aber nicht benannt).

   Anmerkung dazu: Pkt. 1 (Ziel-Crate des Adapters) ist entschieden
   (`lumina-stages`). Pkt. 7 (Fehler-API) ist in `Agents.todo.md` (c) als
   Eigentümerfrage mitgeführt; **implementiert** sind drei eigene Varianten
   `MergeStale`/`MergeUnsupported`/`MergeFailed` (-32011..-32013), `merge
   missing:` mappt auf das bestehende `FileNotFound`
   (`crates/lumina-mcp/src/error.rs` → `McpError`).
4. **`matrix`-Sub-Slice bleibt eigener Task** (`Agents.todo.md`,
   MCP-PARITY-C; SOLL §12): `lumina matrix` bringt `expected_route`
   (`testdata/matrix/recipe-set.v1.json`) und ein eigenes Gate; kein
   Blindschreiben von `expected_route`. Mitziehen im Lockstep:
   `MERGE_IMAGE_DECODER_VERSION` (`crates/lumina-stages/src/merge.rs` → `MERGE_IMAGE_DECODER_VERSION`) bei
   Version-Bumps.
5. **Digest-Sensitivität.** Die Quelldateinamen gehen ins digestierte Rezept
   (Pin-Namen `frame-a.cr3`/`frame-b.cr3` dokumentieren das); ein
   LibRaw-Versionswechsel bricht die Pins absichtlich sichtbar; die
   EXIF-Orientierung 8 des Portrait-Fixtures ist eigener Test-Pin
   (`merge_mcp_parity_gate.rs` ~:413-416: „a replaced fixture needs these pins
   re-measured").
6. **PIPELINE-CROP-EARLY-9 steht auf E1** (Analyse
   `feature/quality/pipeline-crop-early-2026-10-13.md`): die Crop-Umordnung ist
   Owner-blockt, nur die per-pixel-Tonhälfte ist ohne Byte-Wirkung auf
   Crop-only-Rezepte verschiebbar, und mit aktivem Warp ändert eine Verschiebung
   Bytes (14,58 % der Kanäle bei 10 % Restfläche, max Δ12). Für den
   Gesamtkontext: dieselbe Byte-Paritäts-Disziplin wie hier — der Merge-Teil
   davon unberührt, aber keine der beiden Aufgaben darf die andere relativieren.

## 5. Nicht-Behauptetes

- **Kein echter Bracket-Merge mit Shift ≠ 0:** der Same-Frame-Pin setzt
  `--max-shift-px 0`, weil beide Slots dieselben Bytes tragen; die
  SAD-Suche/Alignment-Qualität ist damit nicht beansprucht.
- **Keine echte Panorama-Reihe, keine echte Belichtungsreihe** — kein
  Fixture im Inventar (Punkt 1).
- **Keine Photo-Content-Performance:** alle Laufzeitzahlen sind Debug-Profil
  auf zwei 24-MP-CR3-Fixtures bzw. kleinen Synthetik-Frames; kein
  Photo-Content-Bench, keine Release-Profil-Zahlen.
- **Kein eigener GUI-headless-Merge-Paritätslauf:** die GUI teilt die
  Orchestrierung (`merge_gui.rs:230` → `run_merge`); ein GUI-eigener Byte-Lauf
  ist nicht Teil dieses Standes.
- **Kein** anderes Betriebssystem, keine andere LibRaw-Version, kein GPU-Pfad
  (Merge ist CPU/pure-Rust, SOLL §8 „nicht behauptete Gates").
- **Nicht** behauptet: dass die committeten CR3s ein Bracket oder eine
  Pano-Reihe sind — sie sind es nicht (`merge_mcp_parity_gate.rs:22-39`).

## Bezirk-Referenzen

- Task: `Agents.todo.md` §`[PRIO: niedrig] MCP-PARITY-C` (~:409)
- Vertrag: `feature/quality/mcp-parity-c-soll.md` (§1, §5, §6 Invarianten,
  §7, §8 T11–T21, §9.1, §12, §13)
- Tests: `crates/lumina-cli/tests/merge_mcp_parity{,_envelope,_errors,_gate}.rs`
  + `merge_mcp_parity_common/`, `crates/lumina-mcp/tests/merge_tools*.rs`
- Fixtures/Provenienz: `sample-data/raw/README.md`,
  `feature/quality/fixtures-licensing.md` §4
- Nachbar-Status: `feature/quality/pipeline-crop-early-2026-10-13.md`
  (PIPELINE-CROP-EARLY-9, E1)
- DoD §9 (Wanduhr-/Messungskonvention), `Agents.md` (Hardware-Gate)
