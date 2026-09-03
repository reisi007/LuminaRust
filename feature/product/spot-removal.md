# Staub entfernen — schnell vs. generative KI (SPOT-REMOVE-1)

**Feature:** SPOT-REMOVE-1 Staub entfernen (Spot Heal)
**Status:** Implementiert + verifiziert BESTANDEN (2026-09-03, SPOT-REMOVE-01): heuristisch Clone/Heal in `lumina-core` (`spot_heal.rs`), generativ lokal Inpaint-Stub in `lumina-onnx` (`inpaint.rs`), GUI-Panel + Shortcut Q in `lumina-gui`; FOLLOWUP-Fix (kein `allow` statt Fix, WASM-`cfg`-Gating) enthalten.
**Verwandt:** `feature/architecture/pipeline.md` (Pipeline-Reihenfolge, F-042 Source-Actions), `feature/architecture/sidecar.md` (Artifact, Kind, zdata, relative Pfade, atomar), `feature/product/generative-expand.md` (GEN-EXPAND-1 `GenerativeEdit`, Identität/Veraltung), `feature/product/ai-masks.md` (AI-Masken Identität), `feature/platform/capability-matrix.md` (lokal ONNX vs. Cloud), `feature/quality/fixtures-licensing.md` (F-078), `docs/plans/gap-generative-fill-transparent-2026-09-02.md` (Gap G5/G6).

## Inhaltsverzeichnis

- [Ziel und Abgrenzung](#ziel-und-abgrenzung)
- [Ist-Stand](#ist-stand)
- [Normative Invarianten](#normative-invarianten)
- [Rezeptmodell](#rezeptmodell)
  - [Schneller Modus — heuristisch (kein Modell)](#schneller-modus--heuristisch-kein-modell-normativ)
  - [Generativer Modus — lokal ONNX Inpaint](#generativer-modus--lokal-onnx-inpaint-normativ)
- [Pipeline-Platzierung](#pipeline-platzierung)
- [Sidecar-Artefakt und Persistenz](#sidecar-artefakt-und-persistenz)
- [Identität und Veraltung](#identität-und-veraltung)
- [Capability und Lizenz](#capability-und-lizenz)
- [UI-Flow (GUI)](#ui-flow-gui)
- [Visuelle Analyse automatisch](#visuelle-analyse-automatisch)
- [Abgrenzung zu Source-Actions und GenerativeEdit](#abgrenzung-zu-source-actions-und-generativeedit)
- [Testanforderungen je Modus](#testanforderungen-je-modus)
- [Abnahme](#abnahme)
- [Offene Punkte und Abhängigkeiten](#offene-punkte-und-abhängigkeiten)

## Ziel und Abgrenzung

SPOT-REMOVE-1 beschreibt einen nicht-destruktiven **Staub-/Spot-Heal** in zwei Modi, die als **eine** versionierte Rezept-Stufe pro Spot gemeinsam gedacht werden (Pro Spot: entweder schnell oder generativ), analog Lightroom „Heilen/Klonen" vs. „Generatives Entfernen":

1. **Schnell (heuristisch/Clone):** Sofortiger Spot-Heal ohne Modell — Klon-/Heal-Brush (Kreis/Spot, Radius + Feather + Source-Offset), rein CPU/WASM-kompatibel, **kein Modell**, **instant**, **kein zdata-Artefakt** (nur Rezeptparameter). Ergebnis ist deterministisch aus Original + Rezept ableitbar.
2. **Generativ lokal (ONNX Inpaint):** Lokale generative Heilung — Maske malen (Pinsel/Box), optional Prompt/Negativ-Prompt/Seed, ONNX-Inpaint Modell lokal (`lumina-onnx`), Persistenz als binäres Canvas-Artefakt `.lumina.zdata` mit `kind = "spot_heal_generative"`, Identität wie AI-Masken (Quelle/Decode/Modell/Hash/Inferenzauflösung/Koordinaten/Prüfsumme), **kein stiller Fallback**.

Beide Modi sind nicht-destruktiv: Das Original wird niemals überschrieben. Vorschauen/Exporte sind aus Original + Rezept + (bei generativ) Modell + Artefakt reproduzierbar (Agents.md Produktprinzipien).

## Ist-Stand

**Stand 2026-09-03 (SPOT-Typed/Schatten/Lensfun-Fixes, verifiziert BESTANDEN, HEAD 711fe09):** Typisierte Spot-Felder + laute Validierung in `lumina-core` (`reject_unsupported_spot_modes`, 30ca7ba), Extras-Spiegelung statt Datenverlust in `lumina-sidecar` (`validate_spot_removal_extras`, Deserialize spiegelt Raw-`spot_removals` additiv nach `extras` zurueck, c000c6f), Spiegel-Schatten-Toleranz im Core-Follow-up (1bbb564), Lensfun-Batch SSE-Lane-Umgehung (3df22ea). Gates: core 328p, sidecar 101p `--lib` (139p mit `zdata`), gui 185p, Clippy/Format/wasm gruen.

**Stand 2026-09-03 (SPOT-REMOVE-01 BESTANDEN, Re-Verifizierung nach FOLLOWUP-Fix, vorher):** Implementiert — `lumina-core` `spot_heal.rs` (SpotHeuristic, `apply_spot_heals`, 10 Tests), Pipeline `SpotHeal → Lens → Perspective → Crop` (`pipeline.rs`, `render.rs`), `lumina-onnx` `inpaint.rs` (StubInpaintBackend deterministisch, `inpaint_heal`-Manifest) + `wasm_stub`, `lumina-gui` Spot-Panel (Radius/Feather/Opacity, Schnell vs. Generativ, Shortcut Q, headless Test). Gates grün: core 305p, sidecar 86p `--lib`, onnx 75p `onnx-rt`, gui 155p, `clippy --workspace --all-targets --features onnx-rt -- -D warnings` grün (kein `allow` statt Fix), `fmt --check` grün, wasm core / onnx `onnx-rt` / gui `--no-default-features` warnungsfrei (FOLLOWUP-Fix Commit 190b18b). Vorheriger Stand:

**Stand 2026-09-02 (Doku-first):** Kein Code. `F-042 Source-Actions` existiert als `SourceActionArtifact { region: MaskPlane u16, replacement: ImageFrame RGBA8 }` mit Schwellwert `>= 32768` (50 %) und identischer Dimension Quelle==Region==Replacement (`crates/lumina-core/src/render.rs`, `lumina-sidecar` `SourceActionKind::DustRemoval | AiReplacement`), CLI `dust-removal` vorhanden, aber GUI hat kein Staub-Panel (`lumina-gui` kein Dust-Tool). `lumina-onnx` kennt `subject_segmentation` (BiRefNet) + `box/point/mask_prompt` (SAM 2.1), aber keine `inpaint`/`outpaint` Capability. Dieses Dokument ist das normative SOLL für die Umsetzung in `lumina-core` (Heal-Pass), `lumina-onnx` (Inpaint-Backend), `lumina-sidecar` (Schema/Validation/Migration) und `lumina-gui` (Panel). Bis dahin **kein Crate-Code**.

## Normative Invarianten

Für SPOT-REMOVE-1 gelten `Agents.md` unverändert, konkretisiert:

- **Original unverändert:** Quelldatei wird nie überschrieben/verschoben/ersetzt; Export schreibt neue Datei.
- **Sidecar ist Quelle der Wahrheit:** Rezept (`spot_removals`) + Artefaktverweis (nur generativ) leben ausschließlich im Sidecar (`<original>.lumina.json` + `<original>.lumina.zdata`). Optionale Index-DB darf nur spiegeln, muss aus Sidecars rekonstruierbar sein.
- **Deklaratives, versioniertes Rezept:** Jeder Spot ist eine versionierte Rezept-Operation der virtuellen Kopie (`version`, getrennt von `pipeline_version`). Rezeptänderung → `recipe_hash`/RenderKey ändert sich, Cache invalidiert gezielt.
- **Kein stiller Fallback:** Fehlendes Modell, fehlendes/verfälschtes Artefakt oder veralteter Kontext werden sichtbar als `missing`/`stale`/`corrupt` gemeldet. Es gibt keinen stillen Ersatz durch den anderen Modus, kein anderes Modell, keine stille Re-Generierung als einzige Option.
- **Relative Pfade, atomar:** Ausschließlich relative Artefaktpfade; absolute Pfade verboten (Bundle-Verschiebung bleibt gültig). Sidecar- und Artefakt-Writes atomar (Temp + Rename, `.zdata.lock` auf nativ). Unvollständige Temp-Dateien gelten nie als gültig.
- **Reproduzierbarkeit vor Fallback:** Persistierte generative Spots werden wiederverwendet, nicht ungefragt neu berechnet (analog AI-Masken).

## Rezeptmodell

Die Stufe wird als additive, versionierte Rezept-Operation der jeweiligen virtuellen Kopie gespeichert (analog `source_actions`, additive Schema-Erweiterung, Migrationsentscheidung vor Einführung, Agents.md Änderungsregeln). Empfohlen als neues Top-Level-Feld `spot_removals: Vec<SpotRemoval>` (alternativ Erweiterung von `source_actions` mit `kind = "heuristic_clone" | "generative_inpaint"` — Entscheidung vor Implementierung zu dokumentieren; in beiden Fällen additives Schema-v2-Feld, Migration dokumentiert, unbekannte Version abgelehnt).

```json
{
  "type": "spot_heal",
  "version": 1,
  "mode": "heuristic",
  "center": { "x": 0.501, "y": 0.498, "space": "source-normalized" },
  "radius": 18,
  "feather": 0.5,
  "source_offset": { "dx": 0.05, "dy": -0.02, "space": "source-normalized" },
  "opacity": 1.0,
  "created_at": "2026-09-02T00:00:00Z",
  "status": "valid"
}
```

```json
{
  "type": "spot_heal",
  "version": 1,
  "mode": "generative",
  "mask_reference": {
    "mask_id": "spot-mask-001",
    "artifact": {
      "path": "IMG_0001.lumina.zdata",
      "format": "lumina-zdata",
      "checksum": "blake3:<hex>",
      "width": 512,
      "height": 512,
      "channels": 1,
      "data_version": 1
    }
  },
  "model": {
    "name": "inpaint-heal-xl",
    "version": "1.0.0",
    "hash": "sha256:<64 hex>"
  },
  "prompt": "remove dust spot, seamless texture",
  "negative_prompt": null,
  "seed": 7,
  "inference_resolution": { "width": 512, "height": 512 },
  "artifact": {
    "path": "IMG_0001.lumina.zdata",
    "format": "lumina-zdata",
    "checksum": "blake3:<hex>",
    "width": 512,
    "height": 512,
    "channels": 4,
    "data_version": 1
  },
  "created_at": "2026-09-02T00:00:00Z",
  "status": "valid"
}
```

### Normative Felddefinition

| Feld | Typ | Pflicht | Wertebereich / Semantik |
| --- | --- | --- | --- |
| `type` | string | ja | Literal `"spot_heal"` (bzw. Diskriminator des gewählten Container-Feldes) |
| `version` | u32 | ja | `1` im MVP; unbekannte Version → Ablehnung, Migration erforderlich |
| `mode` | enum | ja | `"heuristic"` \| `"generative"` — Teil der Identität |
| **Heuristisch** `center.{x,y}` | f32 | ja (heuristic) | Normierte Quellkoordinaten `0..=1` (`space = "source-normalized"`), `space` pflichtig, Teil der Identität |
| `radius` | f32/u32 | ja (heuristic) | Spot-Radius in Quellpixeln `>0 && <= 512` (MVP), endlich; Teil der Identität |
| `feather` | f32 | nein (heuristic) | `0..=1`, Default `0.0`; weicher Rand (wie Masken-Feather), Teil der Identität |
| `source_offset.{dx,dy}` | f32 | ja (heuristic) | Normierter Offset der Klonquelle relativ zu `center`, `space = "source-normalized"`; deterministisch |
| `opacity` | f32 | nein (heuristic) | `0..=1`, Default `1.0`; Teil der Identität |
| **Generativ** `mask_reference` | Objekt | ja (generative) | Referenz auf persistierte Spot-Maske (ID + `ArtifactReference`); analog AI-Masken; Pflicht wenn kein `region` |
| `region` | Objekt | bedingt | Alternative: normiertes Rechteck `0..=1` ODER `mask_reference`; für generativ eine der beiden pflichtig |
| `model.{name,version,hash}` | Objekt | ja (generative) | Wie `GenerativeEdit`/`ModelManifest`: Name/Version deklariert, `hash = sha256:<64 hex>` über exakte `.onnx`-Bytes; `pending-integration` nur pre-Integration, zur Laufzeit nie gültig; Mismatch → `stale`/`corrupt` |
| `prompt` | string | ja (generative) | Freitext, roundtrip-stabil; leer zulässig, muss persistiert werden; Teil der Identität |
| `negative_prompt` | string \| null | nein | Optional; `null`/Abwesenheit ist Identität (nicht implizit leer) |
| `seed` | u64 | ja (generative) | Deterministische Reproduktion; gleiche Quelle + Modellkontext + Prompt + Seed + Geometrie → byte-identisches Artefakt |
| `inference_resolution` | `{width,height}` | ja (generative) | z. B. `512×512`; Teil von `ModelInputSpec`/`input_spec_digest`; Änderung invalidiert |
| `artifact` | `ArtifactReference` | ja (generative, nach Generierung) | Relativer Pfad, Format, BLAKE3-Prüfsumme, Auflösung, Kanaltyp, `data_version`; siehe [Sidecar-Artefakt](#sidecar-artefakt-und-persistenz) |
| `created_at` | RFC3339 | ja | Erstellungszeitpunkt |
| `status` | enum | ja | `valid` \| `stale` \| `missing` \| `corrupt` (analog AI-Masken/`GenerativeEdit`) |
| `error` | string \| null | nein | Optionaler Fehlertext bei `corrupt`/`missing` |

Validierung: unbekannte Felder via `serde(flatten)` roundtrip-erhalten (pre-MVP, `feature/architecture/sidecar.md`), unbekannte `version` abgelehnt. **Heuristisch** ist `artifact` verboten (kein zdata-Record); **generativ** ist `artifact` nach Generierung pflichtig und `center/radius/feather/source_offset` nicht anwendbar (gegenseitige Ausschlussvalidierung, sonst Ablehnung). Alle identitätsrelevanten Felder gehen in `recipe_hash`/`RenderKey` ein; jede Änderung invalidiert Preview/Export ab Spot-Stufe, nicht Decode. `null`/`0`/`false` ist Identität (nicht implizit Default).

## Pipeline-Platzierung

Spot-Heal ist eine Pipeline-Stufe **nach** Decode und **vor** Lens/Perspective/Crop, damit die Heilquelle nicht durch Geometrie verzerrt wird:

```text
Decode → SourceActions → SpotHeal(quick heuristic) → SpotHeal(generative ONNX)
  → LensCorrection (F-098) → Perspective/Upright (F-099) → Crop (F-093) → Output
```

- **Heuristisch** wirkt wie `SourceActions` (F-042) auf `Rgba8Srgb` und ersetzt Pixel im Spot-Radius (Feather am Rand). Kein Canvas-Expand, kein Modell.
- **Generativ** nutzt ONNX-Inpaint und ersetzt den maskierten Bereich (analog `GenerativeEdit` Inpaint, aber **ohne** Canvas-Expand — Spot-Größe << Bild, `artifact` trägt die ersetzte Kachel, nicht das volle Canvas).
- Verhältnis zu `GenerativeEdit` (GEN-EXPAND-1): `SpotHeal(generative)` und `GenerativeEdit` sind **getrennte** Stufen mit getrennten Artefakt-Kinds (`spot_heal_generative` vs `generative_canvas`) und getrennten Fähigkeiten (`inpaint_heal` vs `inpaint`/`outpaint`). Reihenfolge bei gleichzeitigem Einsatz: `SpotHeal(generative)` **vor** `GenerativeEdit(auto-fill)` **vor** `GenerativeEdit(expand)` — Spot-Heal darf nicht durch Auto-Fill überschrieben werden. Für den MVP darf `SpotHeal(generative)` als Spezialisierung von `GenerativeEdit` mit `mode = generative` und kleinem `region` implementiert werden, solange die Identität/Kapazität getrennt bleibt.
- Alle Stufen arbeiten in `Rgba8Srgb` (MVP); koordinatenbasierte Felder (`center`, `region`, `mask_reference`) referenzieren den **Quell-Raum** (vor Geometrie), nicht das post-Canvas — damit bleibt ein Spot nach Lens/Crop stabil (kein stilles Re-Interpretieren bei Geometrieänderung; Wechsel invalidiert Geometrie-Digest sichtbar).

## Sidecar-Artefakt und Persistenz

- **Heuristisch:** Kein binäres Artefakt. Nur Rezeptparameter (`center`, `radius`, `feather`, `source_offset`, `opacity`) werden in `<original>.lumina.json` persistiert. Kein `ArtifactReference`, kein `zdata`-Record. Instant-Anwendung (kein Job), deterministisch CPU/WASM-kompatibel.
- **Generativ:** Das Ergebnis (ersetzte Pixel im Spot-Bereich) wird als binäres Sidecar-Artefakt persistiert — analog AI-Masken/`GenerativeEdit` in `.lumina.zdata` (Record mit `kind`-Diskriminator, unverändertem Container-`VERSION`-Muster wie Repair-Regionen aus F-042-N1). Vorgeschlagener Kind: `kind = "spot_heal_generative"` (eigener Diskriminator, damit Masken-, Repair- und Generative-Canvas-Records unverändert bleiben). JSON referenziert das Artefakt (relativer Pfad, Format, BLAKE3-Prüfsumme, Auflösung, Kanaltyp, `data_version`); absolute Pfade verboten. Prüfsumme ist **BLAKE3** über unkomprimierten Pixelstrom (RGBA8, Little-Endian, Zeilen-major, konsistent zur zdata-Semantik); bitflipped Artefakt ≡ `Corrupt` (eager, beim Laden geprüft, nie als verfügbar). Atomarer Write (Temp + Rename unter `.zdata.lock` auf nativ). Auf WASM ist `zdata`/`zstd` nicht verfügbar (native-only, target-gegatet) — dort gilt generatives Artefakt als `missing`/`unverifizierbar`.
- **Sidecar-Schema:** Versioniert (`version` 1), Schema- und Migrationsentscheidung vor Einführung (additives v2-Feld, Migration dokumentiert, kein Pre-MVP-Bruch ohne Bump). Relative Pfade, atomar, Bundle-Verschiebung erhält Referenzen. `ArtifactReference` trägt mindestens `path`, `format`, `checksum`, `width`, `height`, `channels`, `data_version`.

## Identität und Veraltung

Ein **schneller** Spot ist `valid` iff Quelle + alle Rezeptparameter (`center`, `radius`, `feather`, `source_offset`, `opacity`, `version`) übereinstimmen. Kein Artefakt, daher keine Modell-Identität. Quelle/Decode/Orientierung weicht ab → `stale` (sichtbar), keine stille Neuberechnung.

Ein **generativer** Spot ist `valid` iff alle folgenden Punkte übereinstimmen (analog `GenerativeEdit`/AI-Masken, jede Abweichung → `stale`, fehlendes/verfälschtes Artefakt → `missing`/`corrupt`, sichtbar, keine stille Re-Generierung als einzige Option):

- **Quelle:** `source.content_hash` (BLAKE3 über Quellbytes) + Decode-/Geometrieparameter (Decoder, `decode_version` inkl. `+luminaabiN`, Orientierung);
- **Modellkontext:** `model.name`/`version`/`hash` (`sha256:<hex>`, Artefakt-Pin), `inference_resolution`, `InputNormalization` (mean/std), Kanal-Layout, Tensor-Format/-Namen — zusammengefasst im `input_spec_digest` (`sha256:<hex>` unter `ModelIdentity.extras`);
- **Prompt-Kontext:** `prompt` (inkl. `negative_prompt`, exakt roundtrip-stabil) + `seed` (`u64`);
- **Geometrie/Maske:** `region` bzw. `mask_reference` inkl. Artefakt-Referenz (Pfad, Format, Auflösung, Prüfsumme) + Koordinatensystem/Ausrichtung;
- **Artefaktmetadaten:** `artifact.format`/`width`/`height`/`channels`/`data_version`/`checksum` (BLAKE3);
- **Version/Status:** `SpotRemoval.version`, `pipeline_version`/`recipe_version`, `created_at`/`status`/`error`.

Ein Spot ohne gültiges Modell (z. B. `pending-integration`) oder ohne gültiges Artefakt ist `missing`/`stale`/`corrupt` und wird sichtbar gemeldet — es gibt **keinen** stillen Fallback auf den schnellen Modus oder auf „ohne Heilung rendern".

## Capability und Lizenz

- **Heimat lokaler Modelle (generativ):** `lumina-onnx` (native). Inpaint-Heal-Modelle werden wie BiRefNet/SAM 2 über `ModelManifest` mit deklarierten Fähigkeiten eingebunden; Fähigkeit wird aus Manifest gelesen, nicht erraten. Geplante Fähigkeit: `inpaint_heal` (bzw. `inpaint` generisch, dann dokumentiert als Spot-Heal-tauglich) — getrennt prüfbar; Modell ohne passende Fähigkeit wird abgelehnt (kein stiller Ersatz, deterministisch).
- **Lokal vs. Cloud getrennt:** Capability-Matrix (`feature/platform/capability-matrix.md`) führt lokale ONNX-Inferenz und (nicht geplante) Cloud-API als **getrennte** Capabilities. Cloud ist kein stiller Fallback für lokale Inferenz und umgekehrt; ohne dokumentierte Capability-Entscheidung keine Cloud-Anbindung. Vorgeschlagener Matrix-Eintrag:

  | Fähigkeit | native CLI | Desktop (eframe) | Browser (WASM) |
  | --- | --- | --- | --- |
  | Staub schnell (heuristisch, kein ONNX) | geplant, `lumina-core` | geplant, `lumina-core` | geplant (CPU, portabler Core) |
  | Staub generativ (`inpaint_heal`, lokal ONNX) | geplant, `lumina-onnx` | geplant, `lumina-onnx` | nein (kein lokales ONNX ohne `onnx-wasm`) |
  | Staub generativ (Cloud-API) | nicht geplant — nur mit expliziter Capability-Entscheidung | nicht geplant | nicht geplant |

- **Browser:** ONNX im Browser optional (`onnx-wasm`, off by default, F-070) — generativer Spot im Browser erst mit dieser Capability, sonst sichtbar `missing`/`RuntimeDisabled`.
- **Lizenz:** Modelle vor Integration lizenz- und hash-gepinnt dokumentieren (F-078, `feature/quality/fixtures-licensing.md`); keine spontanen Downloads, keine Tests gegen Netz. Modell ohne Lizenz/Provenienz wird nicht eingebunden. `THIRD-PARTY-NOTICES.md` führt Lizenzen vor dem ersten Commit der Gewichte. Viele SOTA Inpaint-Modelle sind non-commercial — vor Einbindung Lizenz prüfen (analog `ultralytics` AGPL-Falle in `fixtures-licensing.md` §5).
- **WASM-Gating:** `zdata`/`zstd` native-only target-gegatet (`feature/platform/capability-matrix.md`); generatives Spot-Artefakt auf WASM `missing`/`unverifizierbar` (kein stiller Fallback).

## UI-Flow (GUI)

Nach GUI-STAGE-1/GUI-WGPU-PRESENT-1 (Native Desktop), Develop-Modul:

1. **Werkzeug wählen:** „Staub entfernen" (Spot-Heal) — Toggle **Schnell** (Default) vs. **Generativ**. Schnell ist Lightroom-ähnlich (Kreis/Spot, sofort), Generativ zeigt Modell-Badge (`inpaint_heal` verfügbar/nicht verfügbar).
2. **Schnell (heuristisch):** Spot klicken/ziehen (Radius via Mausrad/Slider, Feather/Opacity optional); Quelle wird automatisch gewählt (Offset) oder via Alt-Ziehen gesetzt; Vorschau heilt **instant** (kein Job, kein Modell). Bestätigen persistiert Rezeptparameter; „Rückgängig" entfernt Spot.
3. **Generativ lokal:** Maske malen (Pinsel/Box, analog `generative-expand.md`/`ai-masks.md` Prompt-Typen), optional Prompt/Negativ-Prompt/Seed eingeben, „Generieren" starten. Läuft als sichtbarer Job (Jobstatus, kein Hintergrund-Stilllauf); fehlendes Modell/Artefakt (`missing`/`stale`/`corrupt`) wird angezeigt, nicht gefälscht. Ergebnis als Artefakt (`.lumina.zdata`, `kind = "spot_heal_generative"`) persistiert und im Sidecar referenziert; Vorschau rendert über gemeinsame Pipeline. „Verwerfen" löscht nur Rezept/Artefakt, nie Original.
4. **Status sichtbar:** Jeder Spot trägt Badge `valid`/`stale`/`missing`/`corrupt` (generativ) bzw. `valid`/`stale` (schnell). Veraltete/fehlende generativ-Spots blockieren Export nicht still — GUI warnt, bietet Neuberechnung an; CLI analog `--update-masks` (`strict` vs. Warn-and-continue).
5. **Virtuelle Kopien:** `spot_removals` gehört zur jeweiligen virtuellen Kopie (eigenes Rezept, stabile ID). Artefakt-Sharing auf Quell-Ebene nur bei identischer Identität; Masken-Layer bleiben kopienspezifisch (Agents.md „Virtuelle Kopien").

## Visuelle Analyse automatisch

Alle Spot-Ergebnisse sind automatisch visuell verifizierbar — keine manuelle Prüfung nötig (Gap G10, `docs/plans/gap-generative-fill-transparent-2026-09-02.md` §3):

- **Heuristisch:** Golden-Image für Spot-Heal (synthetische Spots auf Checker-Fixture, byte-identische Heilung, toleranzfrei); kein Modell, deterministisch.
- **Generativ:** Golden-Image toleranzbehaftet (PSNR/SSIM-Gate, z. B. PSNR > 35 dB) — Seed pinnen, sonst Gate flakey; deterministisch bei identischem `seed`/`model`/`prompt`/`region`.
- **Vorher/Nachher:** `Y`-Toggle (Before/After) hält Rezept unverändert, toggelt nur View-Flag + generational bump; kittest-Snapshots für Spot-Panel (Schnell vs. Generativ, Badge-Zustände).
- **Histogram/Digest:** `LuminanceHistogram::digest` / `analyze_tone` Median/p01/p99 Delta vor/nach Heal dokumentiert Toleranz `≤ 1/256` (R2-PERF-01).
- **Keine manuellen Screenshots als Gate:** Jede Pipeline-Stufe braucht deterministischen Seed + hash-gepinnte Fixture (F-073), sonst `#[ignore]`.

## Abgrenzung zu Source-Actions und GenerativeEdit

- **Source-Actions (F-042):** Kontext-übergebene Repair-Regionen (u16-Region + RGBA8-Ersatz, Schwellwert 50 %, identische Dimensionen) **ohne** Modell/Prompt. Schneller Spot-Heal ist konzeptionell verwandt, aber **ohne** full-frame Replacement — Radius/Feather/Offset statt full-size MaskPlane. Generativer Spot erweitert dies um Modell/Prompt/Seed/Artefakt (wie `GenerativeEdit`).
- **GenerativeEdit (GEN-EXPAND-1):** Canvas-definierend (`output_* > source_*`, `source_offset`, `kind = "generative_canvas"`, Prompt/Seed, `auto_fill_transparent`/`expand_beyond_image`/`keep_generative_content`). Spot-Heal generativ ist **lokal** (kleine Maske, keine Canvas-Vergrößerung, `kind = "spot_heal_generative"`); beide teilen Identität/Veraltung/Artefakt-Pattern, sind aber getrennte Kinds/Capabilities (kein stiller Ersatz).
- **AI-Masken (F-004):** Persistente Alpha-Matten (SAM 2/BiRefNet, `lumina-onnx`), keine RGB-Heilung. Spot-Maske kann als `MaskPrompt::Brush` wiederverwendet werden, ist aber RGB-Inpaint, keine Masken-Layer-Modulation.

## Testanforderungen je Modus

Jede Implementierung muss vor Verifizierung mindestens diese Prüfungen bestehen (Agents.md § Verifizierung und Tests; Analogie zu AI-Masken/virtuellen Kopien/Source-Actions; Gap G5/G6):

- **Roundtrip und Schema:** JSON-Roundtrip für `SpotRemoval` je Modus (alle Felder, inkl. `mode`/`center`/`radius`/`feather`/`source_offset` bzw. `mask_reference`/`model`/`prompt`/`seed`/`artifact`), unbekannte Felder bleiben erhalten, ungültige `version`/Bereiche abgelehnt; gegenseitige Ausschlussvalidierung (heuristisch darf kein `artifact` tragen, generativ muss `artifact` nach Generierung tragen); Migration v1→v2 mit Backup/`migrate` sobald verschachtelte Felder hinzukommen.
- **Nicht-Destruktion:** Originalbytes unverändert nach Heilen, Speichern, Löschen, Re-Generierung; Export schreibt neue Datei.
- **Heuristisch — Determinismus und Korrektheit:** Identische Eingaben (`center`, `radius`, `feather`, `source_offset`, `opacity`, Quelle) → byte-identisches Ergebnis; Spot-Heal ersetzt nur Radius-Bereich (Feather gewichtet), außerhalb unverändert; Negativ-Fälle (Radius 0, außerhalb `0..=1`, ungültige `version`) abgelehnt; Alpha unverändert.
- **Generativ — Determinismus:** Identische Eingaben (Quelle, Modell-Hash, Prompt/Seed, Region/Maskenref) → byte-identisches Artefakt (BLAKE3 über unkomprimierten RGBA8-Strom).
- **Veraltung (stale/missing/corrupt) — generativ:** Tests für jede Identitätsabweichung: Quell-Hash, Decode-Kontext/Orientierung, Modell-Hash, `input_spec_digest` (Auflösung/Vorverarbeitung), Prompt/Seed, Region/Maskenref, Artefakt-Prüfsumme; fehlendes Modell (`pending`), fehlendes Artefakt, bitflipped Artefakt (BLAKE3-Fehler). Kein stiller Fallback auf heuristisch, Status sichtbar, Neuberechnung nur explizit. Heuristisch: Quell/Parameter-Änderung → `stale` sichtbar (kein Artefakt).
- **Artefakt und zdata — nur generativ:** `artifact_status` (`Available`/`Missing`/`Corrupt`) für Spot-Heal-Artefakte (Pfad fehlt, keine reguläre Datei, Magic/Version/Prüfsummenfehler); relative Pfade nach Bundle-Verschiebung gültig; atomarer Write (Temp + Rename) und `.zdata.lock`-Serialisierung; `kind = "spot_heal_generative"` getrennt von `generative_canvas`/`repair_region`.
- **Pipeline und Geometrie:** Beide Modi wirken vor Lens/Perspective/Crop (Reihenfolge getestet: `SpotHeal → Lens → Perspective → Crop`); Geometrie-Wechsel invalidiert Digest sichtbar (kein stilles Re-Interpretieren); schnelle Heilung und generatives Canvas behindern sich nicht (Reihenfolge SpotHeal vor GenerativeEdit getestet).
- **Virtuelle Kopien:** Stabile ID, eigenes Rezept pro Kopie; Artefakt-Sharing auf Quell-Ebene nur bei identischer Identität; Masken-Layer/Invertierung bleiben kopienspezifisch.
- **Capability und Lizenz:** Fehlendes Modell wird ohne Crash gemeldet; Capability-Matrix trennt heuristisch vs. lokal ONNX vs. Cloud (kein Fallback); Lizenz/Hash-Pin vor Integration dokumentiert; Tests ohne Netz/Download, nur lokale Fixtures. Heuristisch läuft auch ohne `onnx-rt`/WASM (portabler Core).
- **CLI/GUI:** CLI warnt bei `stale`/`missing` (generativ, analog `--update-masks`, `strict` vs. Warn-and-continue); GUI zeigt Status sichtbar und bietet Neuberechnung explizit an. Kein Modell-Download im Test. Staub-Tool Toggle Schnell↔Generativ getestet.
- **Visuelle Analyse automatisch (je Modus):** Golden-Image für heuristisch (byte-identisch) + generativ (PSNR/SSIM-Gate mit Seed-Pin), Histogram-Digest Delta (1/256-Toleranz), kittest Snapshots für Spot-Panel (Schnell/Generativ/Badge), Before/After (`Y`) Toggle hält Rezept unverändert.

## Abnahme

- Original bleibt byteweise unverändert; schnelle Spots sind ableitbare, löschbare Rezeptparameter (kein Artefakt), generative Spots sind ableitbare, löschbare Artefakte (`.lumina.zdata`, `kind = "spot_heal_generative"`).
- `SpotRemoval`-Roundtrip je Modus übersteht Persistenz/Laden verlustfrei (alle Felder, inkl. `mode`, Radius/Feather/Offset bzw. Modell/Prompt/Seed/Prüfsumme).
- Identische Eingaben erzeugen byte-identisches Ergebnis (heuristisch, deterministisch) bzw. byte-identisches Artefakt (generativ, BLAKE3).
- Quell-/Decode-/Parameter-Änderung (heuristisch) bzw. Quell-/Decode-/Modell-/Prompt-/Seed-/Masken-/Artefakt-Änderung (generativ) markiert Spot als `stale`/`missing`/`corrupt` sichtbar — kein stiller Fallback zwischen Modi oder Modellen.
- Pipeline `SpotHeal → Lens → Perspective → Crop` eingehalten; `GenerativeEdit` (GEN-EXPAND-1) und `spot_heal_generative` sind getrennte Kinds/Capabilities und behindern sich nicht.
- Capability-Matrix trennt heuristisch vs. lokal ONNX vs. Cloud (kein stiller Fallback); Lizenz/Hash-Pin vor Integration dokumentiert (F-078).
- Modus-spezifische Veraltungs-, Artefakt-, Pipeline- und Geometrie-Tests sind durch unabhängigen Verifizierungs-Agenten bestätigt.
- `cargo check --workspace` (und wasm-Gates) grün.

## Offene Punkte und Abhängigkeiten

- **Abhängigkeiten:** F-042 (Source-Actions, `lumina-onnx` existiert), F-082/F-083 (SAM-Adapter existiert; Inpaint-Modelle `pending-integration`), GUI-STAGE-1/GUI-WGPU-PRESENT-1; `lumina-gpu`/Present-Pfad berührt (Staub-Heal auf GPU optional, Post-MVP).
- **Offen:** Schema-Entscheidung vor Implementierung (neues Feld `spot_removals` vs. Erweiterung `source_actions` — additives Schema-v2-Feld, Migration dokumentiert, kein Pre-MVP-Bruch ohne Bump); Inpaint-Modellauswahl (Modellfamilie vs. fixes Modell, Lizenz F-078); Cloud-API-Capability (bewusst getrennt); WASM-Pfad (F-070 `onnx-wasm` off by default); Pipeline-Entkopplung von `apply_geometry` 5-in-1 in eigene Stages (G2).
