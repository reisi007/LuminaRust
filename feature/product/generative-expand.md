# Generativer Modus „Entfernen + Erweitern“ (GEN-EXPAND-1)

**Feature:** GEN-EXPAND-1 Optionaler generativer Modus „Entfernen + Erweitern“

## Inhaltsverzeichnis

- [Ziel und Abgrenzung](#ziel-und-abgrenzung)
- [Ist-Stand](#ist-stand)
- [Normative Invarianten](#normative-invarianten)
- [Rezeptmodell `GenerativeEdit`](#rezeptmodell-generativeedit)
  - [Auto-Fill Transparent nach Lens Correction](#auto-fill-transparent-nach-lens-correction-normativ)
  - [Manueller Expand mit Checkbox `expand_beyond_image`](#manueller-expand-mit-checkbox-expand_beyond_image-normativ)
  - [Crop-Entscheidung `keep_generative_content`](#crop-entscheidung-keep_generative_content-normativ)
- [Binäres Sidecar-Artefakt und Prüfsumme](#binäres-sidecar-artefakt-und-prüfsumme)
- [Identität und Veraltung](#identität-und-veraltung)
- [Pipeline-Platzierung und Koordinatensystem](#pipeline-platzierung-und-koordinatensystem)
- [Interaktion mit Crop/Geometry](#interaktion-mit-cropgeometry)
- [Modell, Capability und Lizenz](#modell-capability-und-lizenz)
- [UI-Flow (GUI)](#ui-flow-gui)
- [Abgrenzung zu Source-Actions und KI-Denoise](#abgrenzung-zu-source-actions-und-ki-denoise)
- [Testanforderungen](#testanforderungen)
- [Abnahme](#abnahme)
- [Offene Punkte und Abhängigkeiten](#offene-punkte-und-abhängigkeiten)

## Ziel und Abgrenzung

GEN-EXPAND-1 beschreibt einen **optionalen generativen Bearbeitungsmodus**, der
zwei zusammengehörige Operationen nicht-destruktiv erlaubt:

1. **Entfernen (inpainting):** Ein Objekt/Störfaktor wird aus dem Bild entfernt
   und der Bereich kohärent neu generiert.
2. **Erweitern (outpainting/canvas expansion):** Das Bild wird **über die
   ursprüngliche Bildfläche hinaus** erweitert (> 100 %), der neue Randbereich
   wird generativ ergänzt.

Beide Operationen sind **eine** versionierte Rezept-Stufe (`GenerativeEdit`),
weil sie im selben generativen Durchlauf und auf demselben Ziel-Canvas wirken.
Das Original bleibt unverändert; das Ergebnis ist ein aus Original + Rezept +
Modell ableitbares Artefakt (Agents.md, Produktprinzipien).

Nicht-Destruktion und Reproduzierbarkeit haben Vorrang vor stillen Fallbacks:
Eine Generative-Edit-Operation ist nur dann „aktuell", wenn Quelle,
Decode-Kontext, Modellkontext, Prompt, Seed, Canvas-Geometrie und
Artefakt-Prüfsumme übereinstimmen (analog zu den Regeln für AI-Masken,
Agents.md „AI-Masken").

## Ist-Stand

**Stand 2026-09-14 (GENERATIVE-WELLE, User-Entscheid):** `generative_edit` ist
die einzige verbleibende laute CPU-Route und bleibt das bis zur
ONNX-Integration — als bewusste, dokumentierte Ausnahme (kein stiller
Fallback): Der Heuristik-BFS (`fill_transparent_heuristic`,
`lumina-core/src/generative.rs:23-107`) ist nicht paritätserhaltend
GPU-portierbar (globaler Hash-Sort + FIFO-Tie-Break; datenparallele
Approximation weicht um maxAbsDiff 255 / ~9 dB ab, F-043 verlangt ≤1–2 /
≥48 dB; unabhängig verifiziert BESTANDEN). User-Bedingungen: (1) der
einmalige Expand ist ok, darf aber NICHT bei jedem Rendering neu laufen —
Ergebnis cachen/wiederverwenden (→ GEN-EXPAND-CACHE-1); (2) ONNX-Pfad ASAP
nachziehen (→ GEN-ONNX-1), danach entfällt der BFS-Platzhalter zugunsten von
Artefakt-Compositing (GPU-portierbar).

**Stand 2026-09-15 (GEN-ONNX-1 Welle 2a, ohne GUI; Verifizierung BESTANDEN, Re-Verifizierung 2026-09-16):** Die laute CPU-Route für
generative Rezepte ist entfallen; das Artefakt-Compositing läuft auf GPU **und**
CPU. **(1) GPU-Injektionspunkt:** `GpuContext::render_with_gpu_and_generative`
adoptiert das `GenerativeCanvasInput`; die GPU-Geometrie-Kette fügt einen
`Substitute`-Schritt an derselben mittigen Position wie der CPU-Oracle ein
(`Lens → [auto-fill] → Perspective → CA → [expand] → Crop`), validiert
Rolle/Dimensionen/Canvas-Bounds laut und ist byte-identisch
(`generative_canvas_compositing_is_gpu_parity` mit Crop nach der Substitution,
`generative_auto_fill_compositing_is_gpu_parity` für den Substitute-only-Pfad,
beide `maxAbsDiff == 0`). **Blocker-Fix (Re-Verifizierung offen):** Ein
**trailing** `Substitute` (letzter Plan-Schritt) wurde zuvor still verworfen,
wenn ein Render-Pass davor lag (z.B. Lens/Perspective + Identitäts-Crop + Expand);
der finale Kopiervorgang ist jetzt an `plan.steps.last().is_substitute()`
gekoppelt (der letzte Render-Pass schreibt ggf. in ein Zwischenziel, danach das
Artefakt nach `final_view`) und durch
`generative_trailing_substitute_after_render_pass_is_gpu_parity` gepinnt
(vor dem Fix: maxAbsDiff 235 statt 0). `unsupported_gpu_stages*` führt `generative_edit`
**nicht mehr** als Reason; ein artifact-blinder Render (rezept-only
`render_with_gpu`, CPU-`render_frame`, `render_to_vram` ohne Readback) lehnt
eine aktive generative Stufe **laut** ab statt sie pauschal auf die CPU zu
routen (CLI-Reason `main.rs` entfernt; die CLI ruft den artifact-aware Pfad).
**(2) Negativ-Prompt-Schema:** additives Top-Level-Feld `negative_prompt`
(`GenerativeEdit::negative_prompt`/`set_negative_prompt`, extras-gestützt, siehe
Schema-Entscheid unten); Produzent (`produce_canvas`) und CLI-Verifier nutzen es
(statt hart-`None`), die Identität trägt es bereits — Negativ-Prompt-Wechsel →
`Stale`/laut (CLI-E2E-Test). **(3)** `feature/architecture/pipeline.md`
Stufen-Doku nachgezogen. Erledigt in **Welle 2b (2026-09-16, Verifizierung
BESTANDEN, Re-Verifizierung nach Befunden):** GUI-Anbindung
(Preview/Export-Hook + Badge für den artefakt-blinden VRAM-Present, siehe
GUI-GPU-Entscheid unten), 3 kittest-Goldens (VISION-OK), Doppelrollen-Record
(GUI + CLI angeglichen).

**Schema-Entscheid `negative_prompt` (Welle 2a, normativ):** Das Feld ist
additiv als **Top-Level-JSON-Schlüssel** `"negative_prompt"` (String | null)
geführt und wird in der Rust-`GenerativeEdit` über die bestehende, geflattete
`extras`-Map gehalten (typisierte Accessors `negative_prompt()` /
`set_negative_prompt()`), **nicht** als neues Struct-Feld. Grund: ein neues
Feld würde jedes `GenerativeEdit`-Struct-Literal außerhalb dieser Welle brechen
(insbesondere `lumina-gui`, Welle 2b). Das JSON-Schema ist damit exakt das
additive SOLL-Feld; ein vorhandener, aber nicht-String/nicht-null-Wert wird
**laut abgelehnt** (`validate_edit_extras`, eingebunden in die
Dokumentvalidierung) — kein stilles Ignorieren. Die Stufen-Version
`GenerativeEdit.version` bleibt **1**: das Feld ist rein additiv, unbekannte
Felder werden via `extras` roundtrip-erhalten, es gibt keine inkompatible
Migration.

**Auto-Fill-Caller-Konvention (Welle 2a, normativ):** Der Renderer übernimmt für
die Auto-Fill-Rolle **genau dann**, wenn der Caller `auto_fill = Some(artifact)`
übergibt; `auto_fill = None` ist für die GPU die Identität (keine Substitution).
Die CPU prüft zusätzlich selbst `has_transparent_pixels` auf dem Post-Lens-Frame
und substituiert nur dann (sonst Identität). Daraus folgt verbindlich: **Der
Caller MUSS `auto_fill = None` übergeben, wenn nach Lens keine transparenten
Pixel existieren** (kein Artefakt nötig), und `Some` genau dann, wenn
transparente Pixel vorliegen. Die GPU kann Transparenz nicht ohne Readback
prüfen und vertraut daher dem Caller-Signal; die CLI erfüllt die Konvention
(`resolve_generative_artifacts` gibt ohne Transparenz `None` zurück, ohne ein
Artefakt zu verlangen) und pinnt sie per Test. Für die Expand-Rolle ist ein
fehlendes Artefakt dagegen immer ein lauter Fehler (`generative_artifact.expand.missing`).

**Stand 2026-09-15 (GEN-ONNX-1 Welle 1, ohne GUI):** Der modellbasierte
Inpaint/Outpaint-Pfad ist für die nicht-GUI-Crates umgesetzt. Die Render-Stufe
ist **Artefakt-Compositing** (die sequentielle BFS-Heuristik entfällt aus dem
Renderpfad): `lumina-onnx` erzeugt das vollständige kompositierte Canvas
(`kind = 2 generative_canvas`, RGBA8) samt **vollständiger Identität** — Rolle,
Seed, Canvas, BLAKE3-Pixel-Digest des Eingabeframes **plus Prompt,
Negativ-Prompt und Modell-Hash** (`GenerativeIdentity`); ein Prompt- oder
Modellwechsel invalidiert das Artefakt damit sichtbar (`stale`), kein stilles
Adoptieren eines veralteten Canvas. `lumina-core`
**adoptiert** das per Caller-Hook durchgereichte Canvas (`GenerativeCanvasInput`,
`render_frame_with_generative` / `render_frame_from_base_with_generative` /
`export_image_with_generative`) ohne eigene Pixelarithmetik — dadurch inhärent
GPU-portierbar (die GPU-Parität ist per Oracle/Parity-Test belegt, siehe
Testanforderungen). Ein aktiver `expand_beyond_image`/`auto_fill_transparent`
**ohne** Artefakt bricht laut ab (`CoreError::InvalidAdjustment`), es gibt
keinen stillen BFS-/„als wäre nichts generiert"-Fallback mehr. Die durable
`zdata`-Verdrahtung ist aktiv: Identität schreiben (`GenerativeArtifactRef`
mit `generative_identity`-Digest) + persistiertes Canvas per Caller-Hook in den
Render zurückspeisen; Core bleibt I/O-frei. CLI-End-to-End:
`lumina generative --generate|--status|--remove` (Exit-Codes laut; fehlendes
Modell/Artefakt/Identitätsdrift → Exit 1). **Modell-Entscheid und
Restrisiko siehe § Modell, Capability und Lizenz.** Präzisierung zur Identität:
`negative_prompt` ist derzeit immer `None`, weil `GenerativeEdit` noch kein
Negativ-Prompt-Feld persistiert (additives Schema-Entscheid offen); die
Identität trägt das Feld bereits, sodass die Schema-Erweiterung die Identität
nicht wieder blind macht. Erledigt in **Welle 2b (2026-09-16):**
GUI-Anbindung (Preview/Export-Hook, artifact-aware CPU-Pfad), 3
kittest-Goldens (VISION-OK), GPU-Injektionspunkt-Badge (siehe GUI-GPU-Entscheid
unten) sowie Doppelrollen-Record in GUI **und** CLI (CLI hart ablehnend →
angeglichen, `resolve_generative_artifacts` + `--generate` lösen beide Rollen).

**Rework 2026-09-15 (Verifizierung Befund BLOCKER + F1–F3 behoben):** (a) Prompt,
Negativ-Prompt und Modell-Hash sind in `GenerativeIdentity`/`GenerativeCacheKey`
aufgenommen und gehen in den Digest ein (längenpräfixiert); Drift-Tests für
Prompt- und Modellwechsel (Core-Digest + CLI-E2E `Stale`/laut + ONNX-Produzent).
F1: der reale `GenerativeModelSource::Artifact`-Pfad verweigert
`pending-integration` hart (`UnsupportedModel`), inkl. Test; die Fixture bleibt
`Verified`. F2: GPU-Doku/Kommentare nennen als Routing-Grund den fehlenden
GPU-Injektionspunkt fürs Artefakt-Compositing (nicht mehr „sequential global
BFS"). F3: Test für die Doppelrollen-Ablehnung (Render + `--generate`).

**Stand 2026-09-14 (GEN-EXPAND-CACHE-1 BESTANDEN):** RAM-Cache erfüllt die
User-Bedingung — Zweit-Render ohne BFS (thread-lokales LRU, Key aus Rolle +
Seed + Canvas + BLAKE3-Pixel-Digest, exakte Identität, stale nie serviert,
`trace!`-Logging; 9 Core- + 1 GUI-Test). **Korrektur 2026-09-15 (GEN-ONNX-1
BLOCKER-Fix):** der Identitäts-Digest enthält zusätzlich Prompt,
Negativ-Prompt und Modell-Hash (`GenerativeIdentity`); die ursprüngliche
Formulierung „Rolle + Seed + Canvas + Pixel-Digest" war prompt-/modellblind.
Scope-Entscheid (verifiziert
akzeptiert): die durable `zdata`-Verdrahtung (Identität schreiben +
persistiertes Canvas in den Render zurückspeisen) kommt mit GEN-ONNX-1, wo
Artefakt-Compositing den BFS ersetzt — die `GenerativeArtifactRef`-Identität
(`extras["generative_identity"]`, additiv) und `generative_artifact_status`
(`Available` nur bei Identität, sonst `Stale`/`Missing`/`Corrupt`) liegen
dafür bereit.

**Stand 2026-09-03 (GEN-FILL-03 BESTANDEN verifiziert 2026-09-03, c7aede7+9cc8f45+0d3033d):** `keep_generative_content` (`null→true` Default, `effective_keep`, `keep_true` Canvas bleibt, `keep_false` materialisiert `canvas=crop_rect` `source_offset-crop_offset` Translation, verkürzt Canvas, validiert, `recipe_hash` ändert sich, `resolve_canvas_for_recipe` keep true→clone false→`materialize_canvas_for_crop` inkl. `Aspect`/`Free` normiert `round/clamp`, `materialize_with_source` OOB→`InvalidAdjustment` kein stiller Fallback, `validate_with_source` `output>source` + Bounds), `lumina-core::generative` (`effective_keep`, `materialize_canvas_for_crop`, `resolve_canvas_for_recipe`, `recipe_hash`), `lumina-sidecar` `GenerativeEdit`/`GenerativeCanvas` `validate`, `lumina-core` `pub mod generative` re-export, 15 generative Tests (`305p` `core`, `86p` `sidecar`, `155p` `gui`), `clippy -D warnings`/`fmt`/`wasm` grün, kein Datenverlust still.

**Stand 2026-09-04 (GUI-DOUBLE-EXPAND-FIX 98b0be6, GEN-PIPELINE-DECOUPLE b80eb62):** Single-Expand — Core rendert `GenerativeEdit(expand)` intern (`Lens→Fill→Perspective→Expand→Crop`, `render_frame_from_base`, Fehler → `InvalidAdjustment` laut); GUI-Post-Render-Checker-`apply_generative_expand` (Preview/Export) ersatzlos gestrichen (war doppelte Pipeline-Implementierung), Preview/Export nutzen den Core-Frame direkt. GUI-Tests 174p (Preview/Export Single-Expand 8→12 inner byte-identisch, Expand-ohne-Canvas laut). Heuristischer Fill, noch kein ONNX-Modell (`pending-integration`), `zdata`-Persistenz kind=2 + Rezept-Link vorhanden (GEN-ZDATA-PERSIST 1e0ccbd, GEN-ZDATA-LINK-1 69dad91), Core-`recipe_hash`/`RenderKey`-Einbezug der Link-Felder = Follow-up.

**Stand 2026-09-03 (GEN-FILL-02 BESTANDEN verifiziert 2026-09-03, 0d3033d):** `GenerativeEdit` `canvas` + `expand_beyond_image` implementiert — `GenerativeCanvas` `validate_with_source` (`output_* > source_*` + `source_offset` Bounds), `GenerativeEdit::effective_expand` Default `false`, `validate` (`expand true→canvas pflichtig`, `expand false→canvas verboten`), GUI `set_expand_beyond_image` (false→`canvas=None`, true→`w+4/h+4 offset 2,2` validiert) + `set_expand_canvas` + `draw_generative_expand` (Checkbox „auf Bild beschneiden", DragValue W/H/X/Y, Apply-Frame), `recipe_hash`/`RenderKey` (`preview 12×12` vs `8×8`), `preview_generation` bump, 6 headless Tests (`core`/`sidecar`/`gui`); Pipeline heuristisch `Perspective→Expand→Crop` (GUI Apply nach Render, Core `apply_generative_expand` separat), noch nicht als entkoppelte `Pipeline::default` Stufe (5-in-1 `apply_geometry`), kein stiller Fallback, kein `zdata` yet.

**Stand 2026-09-02 (GEN-EXPAND-1 BESTANDEN verifiziert 2026-09-02, 46f6baf):** Nur dokumentiert, Implementierung **nicht** begonnen
(repo-weit verifiziert: kein Code, keine `GenerativeEdit`-Stufe; Doku-first). Es gibt
keine Inpainting-/Outpainting-Modelle im Workspace; `lumina-onnx` ist die
vorgesehene Heimat lokaler Modelle (F-082/F-083-SAM-Adapter existiert, echte
Modellgewichte weiterhin `pending-integration`). Dieses Dokument ist das
normative SOLL für die spätere Umsetzung (Feldbestand `GenerativeEdit`, Canvas >100% Expand, Pipeline Decode→SourceActions→GenerativeEdit→Lens→Perspective→Crop, `.lumina.zdata` `kind=generative_canvas` atomar, Identität/Veraltung analog AI-Masken, kein stiller Fallback, Capability lokal vs Cloud, Lizenz F-078 — unabhängig verifiziert BESTANDEN, kein Code). Die Implementierung erfolgt später in
`lumina-onnx` (Modellverwaltung, Inferenz) und `lumina-core` (Pipeline-Stufe)
sowie `lumina-sidecar` (Schema, Validierung, Migration) nach GUI-STAGE-1.
Dazu gehört die durable `zdata`-Verdrahtung aus GEN-EXPAND-CACHE-1
(Scope-Entscheid 2026-09-14: persistiertes Canvas per Identität in den Render
zurückspeisen, Caller-Auflösungs-Hook — Core bleibt I/O-frei).
Bis dahin wird **kein Crate-Code** angelegt.

## Normative Invarianten

Für GEN-EXPAND-1 gelten die Produktprinzipien und Persistenzregeln aus
`Agents.md` unverändert und werden hier für die generative Stufe konkretisiert:

- **Original unverändert:** Die Quelldatei wird niemals überschrieben,
  verschoben oder durch das generierte Canvas ersetzt. Ein Export schreibt eine
  neue Datei.
- **Sidecar ist Quelle der Wahrheit:** Die Stufe `GenerativeEdit` und der
  Verweis auf ihr binäres Ergebnis leben ausschließlich im Sidecar
  (`<original>.lumina.json` + `<original>.lumina.zdata`). Eine optionale
  Index-DB darf sie nur spiegeln und muss aus Sidecars rekonstruierbar sein.
- **Deklaratives, versioniertes Rezept:** Jede generative Operation ist eine
  versionierte Rezeptstufe mit stabiler Semantik (`version`, getrennt
  versioniert von `pipeline_version`). Rezeptänderung → `recipe_hash` / RenderKey
  ändert sich, Cache invalidiert gezielt.
- **Artefakt ableitbar und löschbar:** Vorschauen, Renderings und Exporte sind
  aus Original + Rezept + Modell + Artefakt reproduzierbar. Das binäre
  Canvas-Artefakt darf gelöscht werden und ist bei identischem Kontext
  byte-identisch re-generierbar.
- **Kein stiller Fallback:** Fehlendes Modell, fehlendes/verfälschtes Artefakt
  oder veralteter Kontext werden sichtbar als `missing`/`stale`/`corrupt`
  gemeldet. Es gibt keinen stillen Ersatz durch ein anderes Modell, keine
  stille Re-Generierung als einzige Option und kein „als wäre nichts generiert
  worden".
- **Relative Pfade:** Artefaktverweise enthalten ausschließlich relative Pfade;
  absolute Pfade sind verboten. Ein verschobenes Sidecar-Bundle bleibt gültig.
- **Atomarer Write:** Sidecar- und Artefakt-Schreibvorgänge erfolgen atomar
  (temporäre Datei + Rename). Unvollständige Dateien gelten nie als gültig.

## Rezeptmodell `GenerativeEdit`

Die Stufe wird als additive, versionierte Rezept-Operation der jeweiligen
virtuellen Kopie gespeichert (analog `source_actions` als Pre-MVP-Muster;
Schema- und Migrationsentscheidung vor Einführung, Agents.md
Änderungsregeln). Struktur (normativ, Felder siehe Tabelle):

```json
{
  "type": "generative_edit",
  "version": 1,
  "model": {
    "name": "inpaint-outpaint-xl",
    "version": "1.0.0",
    "hash": "sha256:<64 hex>"
  },
  "prompt": "Remove the person on the left; extend the sky to the right",
  "negative_prompt": "blurry, distorted",
  "seed": 42,
  "inference_resolution": { "width": 1024, "height": 1024 },
  "canvas": {
    "output_width": 6000,
    "output_height": 4000,
    "source_offset_x": 500,
    "source_offset_y": 0
  },
  "region": {
    "x": 0.1,
    "y": 0.2,
    "width": 0.3,
    "height": 0.4,
    "space": "source-normalized"
  },
  "mask_reference": {
    "mask_id": "mask-abc123",
    "artifact": {
      "path": "IMG_0001.lumina.zdata",
      "format": "lumina-zdata",
      "checksum": "blake3:<hex>",
      "width": 1024,
      "height": 1024,
      "channels": 1,
      "data_version": 1
    }
  },
  "artifact": {
    "path": "IMG_0001.lumina.zdata",
    "format": "lumina-zdata",
    "checksum": "blake3:<hex>",
    "width": 6000,
    "height": 4000,
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
| `type` | string | ja | Literal `"generative_edit"` |
| `version` | u32 | ja | `1` im MVP; unbekannte Version → Ablehnung, Migration erforderlich |
| `model.name` | string | ja | Modellname, nicht aus Artefaktnamen geraten; deklariert via `ModelManifest` |
| `model.version` | string | ja | Modellversion (Semver oder Hersteller-Tag, dokumentiert) |
| `model.hash` | string | ja | `sha256:<64 hex>` über exakte `.onnx`-Bytes; `pending-integration` nur pre-Integration, zur Laufzeit nie gültig; Mismatch → `stale`/`corrupt`, kein stiller Ersatz |
| `prompt` | string | ja | Freitext; gehört zur Identität, roundtrip-stabil; leerer Prompt zulässig, muss persistiert werden |
| `negative_prompt` | string \| null | nein | Optionaler Negativ-Prompt; `null`/Abwesenheit ist Identität (nicht implizit leer) |
| `seed` | u64 | ja | Deterministische Reproduktion; gleiche Quelle + Modellkontext + Prompt + Seed + Canvas → byte-identisches Artefakt |
| `inference_resolution` | `{width,height}` | ja | Inferenzauflösung (z. B. 1024×1024); Teil von `ModelInputSpec`/`input_spec_digest`; Änderung invalidiert |
| `canvas.output_width` / `output_height` | u32 | ja | Ziel-Canvas in Pixeln; `> 0`; beim reinen Entfernen == Quellabmessung; beim Erweitern **> 100 %** der Quelle (siehe unten) |
| `canvas.source_offset_x` / `source_offset_y` | i32 | ja | Platzierung der Quell-Ecke im Canvas; `0` = bündig links/oben; negativ zulässig, wenn Quelle zentriert erweitert wird; deterministisch |
| `region` | Objekt | bedingt | Rechteck in Quell-normalisierten Koordinaten `0..=1` ODER `mask_reference`; für reines Erweitern optional (Randbereich wird abgeleitet) |
| `mask_reference` | Objekt | bedingt | Referenz auf persistierte Maske (ID + `ArtifactReference`); analog AI-Masken; für Inpainting pflichtig, wenn `region` keine Maske trägt |
| `artifact` | `ArtifactReference` | ja (nach Generierung) | Relativer Pfad, Format, BLAKE3-Prüfsumme, Auflösung (`width`/`height`), Kanaltyp, `data_version`; siehe [Binärdaten](#binäres-sidecar-artefakt-und-prüfsumme) |
| `created_at` | RFC3339 | ja | Erstellungszeitpunkt |
| `status` | enum | ja | `valid` \| `stale` \| `missing` \| `corrupt` (analog AI-Masken) |
| `error` | string \| null | nein | Optionaler Fehlertext bei `corrupt`/`missing` |
| `auto_fill_transparent` | bool | nein | `false` Default; `true` = automatisches generatives Füllen transparenter Pixel nach Lens Correction (siehe unten); Teil von Identität und `recipe_hash` |
| `expand_beyond_image` | bool | nein | `false` Default = „auf Bild beschneiden"; `true` = Canvas größer ziehen (manueller Expand); Teil von Identität und `recipe_hash` |
| `keep_generative_content` | bool \| null | nein | `null`/Abwesenheit = `true` Default; `true` = generatives Canvas behalten auch wenn Crop schneidet; `false` = auf aktuelle Ansicht zuschneiden; Teil von Identität |

Validierung: unbekannte Felder bleiben via `serde(flatten)` roundtrip-erhalten
(pre-MVP, `feature/architecture/sidecar.md`), unbekannte `version` wird
abgelehnt. `seed`/`canvas`/`prompt`/`model`/`auto_fill_transparent`/
`expand_beyond_image`/`keep_generative_content` gehen in `recipe_hash` und
`RenderKey` ein; jede Änderung invalidiert Preview/Export ab `GenerativeEdit`,
nicht Decode. `false`/`null` ist Identität (nicht implizit `true`).

### Canvas-Koordinaten > 100 % Expand (normativ)

- Das Canvas ist der neue Zielrahmen. Beim **reinen Entfernen** gilt
  `output_width == source_width` und `output_height == source_height` sowie
  `source_offset == (0,0)`.
- Beim **Erweitern** gilt mindestens eine Kante `output_* > source_*`. Das
  heißt „> 100 % der ursprünglichen Bildfläche". Beispiel: Quelle 4000×3000,
  Canvas 6000×4000 mit `source_offset_x = 500` zentriert die Quelle horizontal
  und generiert je 500 px links/rechts plus 1000 px in der Höhe.
- `source_offset_x/y` definieren die Translation Quelle → Canvas deterministisch.
  Negative Offsets sind zulässig (Quelle nicht an Canvas-Ursprung), solange
  `0 <= source_offset + source_dim <= output_dim`. Verletzung → Validierungsfehler.
- Der Randbereich außerhalb `source_offset + source_dim` ist der
  Outpainting-Bereich und benötigt keine separate Maske; er wird aus `canvas`
  abgeleitet. Die Inpainting-Region/Maske referenziert immer Quellkoordinaten;
  ihre Abbildung ins Canvas erfolgt über die Offset-Translation.
- Die Canvas-Geometrie ist Teil der Identität (siehe
  [Identität und Veraltung](#identität-und-veraltung)); eine Änderung macht
  nachgelagerte Geometrie veraltet (kein stilles Re-Interpretieren).

Ein `GenerativeEdit` ohne aktives Modell oder ohne gültiges Artefakt ist
`missing`/`stale` und wird sichtbar gemeldet — es gibt **keinen** stillen
Fallback (kein „weiter so, als wäre nichts generiert worden").

### Auto-Fill Transparent nach Lens Correction (normativ)

**Ziel:** Nach einer Objektivkorrektur (F-098, `lens_correction`) entstehen an
den Rändern transparente Pixel (Keile/Lücken durch Entzerrung). Statt sie zu
beschneiden oder schwarz zu füllen, kann die `GenerativeEdit`-Stufe sie
**automatisch** generativ füllen.

- **Trigger:** `auto_fill_transparent == true` **und** nach `LensCorrection`
  existieren transparente Pixel (`alpha == 0` oder außerhalb des
  entzerrten Quellbereichs). Kein Trigger bei `false` oder ohne transparente
  Pixel → Identität (kein Artefakt, kein `missing`).
- **Pipeline-Einordnung:** `LensCorrection → GenerativeEdit(auto-fill) → Perspective → Crop`.
  Der Auto-Fill ist ein `GenerativeEdit`-Durchlauf **nach** Lens, **vor**
  Perspektive/Crop. Er ist damit von einem manuellen Expand (siehe unten)
  unterscheidbar, der ebenfalls `GenerativeEdit` nutzt, aber als
  canvas-definierende Expand-Operation gilt.
- **Modell/Prompt/Seed:** `model` ist pflichtig wenn `auto_fill_transparent == true`
  (Validierung: Modellfähigkeit `inpaint` oder `outpaint` muss vorhanden sein,
  sonst Ablehnung, kein stiller Fallback). `prompt`/`negative_prompt`/`seed`
  sind optional; Default ist `prompt = ""`, `seed = 0` (deterministisch, in
  Identität enthalten); `inference_resolution` pflichtig wie oben.
  Die Region ist implizit: die transparente Maske nach Lens (kein `region`/
  `mask_reference` nötig; falls gesetzt, wird sie als zusätzliche Inpainting-
  Region interpretiert und in Canvas-Koordinaten translatiert).
- **Artefakt:** Wie bei `GenerativeEdit` das vollständige kompositierte Canvas
  (identische `ArtifactReference`-Semantik, `kind = "generative_canvas"`,
  BLAKE3, relativ, atomar). Bei `auto_fill_transparent` ist
  `canvas.output_*` initial `==` Quellabmessung nach Lens (kein Expand);
  der gefüllte Bereich ersetzt nur transparente Pixel.
- **Identität/Veraltung:** Zusätzlich zu den generellen Identitätsfeldern
  gehört `auto_fill_transparent` + die transparente Maske nach Lens (implizit
  aus `lens_correction` + Quellgeometrie abgeleitet) zur Identität. Änderung
  von `lens_correction`, Quelle/Decode/Orientierung oder Modellkontext →
  `stale`/`missing`/`corrupt` sichtbar, keine stille Neuberechnung.
  Ohne transparente Pixel bleibt der Status `valid` mit unverändertem Canvas
  (kein Artefakt nötig).
- **Persistenz:** `auto_fill_transparent` liegt in `GenerativeEdit` pro
  virtueller Kopie (eigenes Rezept, stabile ID). Relative Pfade, atomar
  (`Temp + Rename` unter `.zdata.lock`), Schema `version` wie `GenerativeEdit`.

### Manueller Expand mit Checkbox `expand_beyond_image` (normativ)

**Ziel:** Der Benutzer kann den Bildrahmen **explizit** über die Quellfläche
hinaus vergrößern (>100 %) und den neuen Rand generativ füllen. Die
Entscheidung wird über eine Checkbox gesteuert.

- **Feld `expand_beyond_image`:** `bool`, **Default `false`** = „auf Bild
  beschneiden". Label in der GUI: „auf Bild beschneiden" (aus) vs. „Canvas
  größer ziehen / Expand" (an). `false` → kein Expand, `GenerativeEdit` darf
  dann nur Inpainting ohne Canvas-Vergrößerung (`canvas.output_* == source_*`);
  ein Expand-Rahmen wird nicht persistiert. `true` → manueller Expand aktiv,
  `canvas.output_* > source_*` pflichtig, sonst Validierungsfehler.
- **UI-Flow „Rahmen ziehen":**
  1. Checkbox aus (`false`) ist Ausgangszustand; Vorschau zeigt Lens-bereinigten
     Frame, transparente Ränder würden beschnitten.
  2. Benutzer aktiviert Checkbox → Expand-Modus an; ein ziehbarer Rahmen
     erscheint über die Bildkante hinaus (skizzierter Rand, transparent).
  3. Ziehen des Rahmens definiert `canvas.output_width`/`height` +
     `source_offset_x`/`y` (Translation wie im Canvas-Abschnitt). Das Seiten-
     verhältnis kann als Vorgabe dienen (z. B. 16:9), ist aber nicht bindend.
  4. Optional Prompt/Negativ-Prompt/Seed eingeben; „Generieren" startet den
     generativen Durchlauf (`model` pflichtig, Fähigkeit `outpaint`).
  5. Bestätigen setzt die Rezept-Canvas-Geometrie und das Artefakt atomar;
     Abbrechen verwirft sie (kein Schreiben pro Drag, kein stilles Verwerfen).
  6. Deaktivieren der Checkbox setzt `expand_beyond_image = false` und macht
     das Expand-Artefakt `stale` (sichtbar), löscht es aber nicht automatisch.
- **Pipeline-Einordnung:** Manueller Expand ist eine `GenerativeEdit`-Stufe
  **vor** `Crop` (und nach `LensCorrection`/`Perspective`): `LensCorrection →
  (GenerativeEdit auto-fill falls an) → Perspective → GenerativeEdit(expand) → Crop`.
  Vereinfacht dokumentiert als `Lens → GenerativeEdit(expand) → Perspective → Crop`,
  wenn Auto-Fill und Expand nicht gleichzeitig aktiv sind; bei gleichzeitiger
  Nutzung gilt die explizite Zweistufen-Reihenfolge oben (zwei
  `GenerativeEdit`-Records mit unterschiedlicher Rolle, beide versioniert).
  Für den MVP darf auch ein einzelner `GenerativeEdit`-Record beide Rollen
  tragen (`auto_fill_transparent` + `expand_beyond_image` gleichzeitig);
  die Reihenfolge bleibt Lens → GenerativeEdit → Perspective → Crop und die
  transparente Maske wird vor dem Expand abgeleitet.
- **Persistenz:** `expand_beyond_image` + `canvas` pro virtueller Kopie,
  versioniert (`version` 1), relative Artefaktpfade, atomar, kein absoluter Pfad.

### Crop-Entscheidung `keep_generative_content` (normativ)

**Ziel:** Nach einem generativen Expand/Auto-Fill entscheidet der Benutzer,
ob das generative Canvas erhalten bleibt oder auf die aktuelle Ansicht
zugeschnitten wird.

- **Feld `keep_generative_content`:** `bool` (nullable, Default `true`);
  `null`/Abwesenheit ≡ `true`. `true` = „generatives Canvas behalten auch
  wenn Crop schneidet" (generatives Artefakt bleibt volle Canvas-Größe,
  Crop ist nur Ansicht/Export-Ausschnitt). `false` = „auf aktuelle Ansicht
  zuschneiden" (Crop wird materialisiert: das persistierte Canvas wird auf
  das normierte Crop-Rechteck zugeschnitten, `canvas.output_*` wird auf
  Crop-Größe reduziert, `source_offset` neu berechnet, Artefakt neu
  referenziert/präfixiert, alter Artefakt-Record bleibt bis explizites
  Pruning erhalten — kein stilles Überschreiben).
- **Rezept-Felder:** `canvas.output_width`/`height` + `source_offset_x`/`y`
  beschreiben das **generative Canvas** (Zielrahmen vor Crop). `recipe.geometry.crop`
  (`x/y/width/height` in `0..=1` auf dem post-GenerativeEdit-Canvas) beschreibt
  den sichtbaren Ausschnitt. Bei `keep_generative_content == true` bleibt
  `canvas` unverändert, Crop ist reine Ansicht. Bei `false` wird beim
  Bestätigen `"auf Ansicht zuschneiden"` die Canvas-Geometrie **ersetzt**
  durch die Crop-Geometrie: `new_canvas = crop_rect_in_canvas_pixels` (gerundet
  auf ganze Pixel, `width >= 1 && height >= 1`), `source_offset` wird um
  `crop.x * canvas_width`/`crop.y` translatiert (`new_source_offset = old_source_offset - crop_offset`);
  das Ergebnis wird validiert (`0 <= source_offset + source_dim <= output_dim`),
  in `recipe_hash` aufgenommen und atomar persistiert.
- **Koordinaten-Translation:** Alle nachgelagerten normierten Koordinaten
  (Crop, Masken, Perspektive-Shift, F-041-Messbereich, Vignette/Grain) wurden
  bereits auf dem post-GenerativeEdit-Canvas definiert (siehe
  Koordinatenreferenzrahmen). Ein Wechsel von `keep_generative_content`
  invalidiert den Geometrie-Digest sichtbar (`recipe_hash` + `output_dimensions`
  ändern sich); abhängige Werte werden nie still umgedeutet, sondern als
  veraltet gemeldet und vom Benutzer neu bestätigt/migriert.
- **Persistenz:** `keep_generative_content` gehört zur `GenerativeEdit`-Stufe
  der virtuellen Kopie (eigenes Rezept, stabile ID). Together mit `canvas`
  und `crop` versioniert; relative Pfade; atomar (`Temp + Rename`); Schema-
  Migration via `version`-Bump, unbekannte Felder roundtrip-erhalten, unbekannte
  Version abgelehnt. Ein Sidecar-Bundle-Verschieben hält alle Referenzen
  gültig.

## Binäres Sidecar-Artefakt und Prüfsumme

- Das Ergebnis ist das **vollständige, kompositierte Canvas** (inklusive
  unveränderter Quellpixel), gespeichert als binäres Sidecar-Artefakt —
  analog AI-Masken in `.lumina.zdata` (Record mit `kind`-Diskriminator,
  unverändertem Container-`VERSION`-Muster wie Repair-Regionen aus F-042-N1)
  oder einem dokumentierten, versionierten Format mit Prüfsumme.
  Kind-Vorschlag: `kind = "generative_canvas"` (eigener Diskriminator, damit
  Masken- und Repair-Records unverändert bleiben).
- Große Ergebnisse werden **nicht** als unkomprimierte Arrays ins JSON
  geschrieben; das JSON referenziert das Artefakt (relativer Pfad, Format,
  Prüfsumme, Auflösung, Kanaltyp, Datenversion). `path` ist relativ zum
  Sidecar-Bundle; absolute Pfade sind verboten (analog
  `feature/architecture/sidecar.md`).
- Prüfsumme ist **BLAKE3** über den unkomprimierten Pixelstrom
  (RGBA8, Little-Endian, Zeilen-major, konsistent zur zdata-Semantik).
  Ein bitflipped Artefakt zählt nie als verfügbar (`Corrupt`).
- Das Original wird nie überschrieben; das Artefakt ist löschbar und durch
  Re-Generierung (bei identischem Kontext byte-identisch) ersetzbar.
- Atomarer Write: Generierte Artefakte werden wie Maskenpayloads über
  temporäre Datei + atomaren Rename in `.lumina.zdata` geschrieben
  (Read-Modify-Write unter `.zdata.lock` auf nativ). Unvollständige
  temporäre Dateien gelten nie als gültig; `artifact_status` unterscheidet
  `Available`/`Missing`/`Corrupt` eager (Prüfsumme beim Laden geprüft, nicht
  lazy). Artefakte werden nie still ersetzt.

## Identität und Veraltung

Eine Generative-Edit-Operation ist **gültig** (`valid`), wenn alle folgenden
Punkte übereinstimmen; jede Abweichung markiert sie als `stale` (sichtbar, keine
automatische Re-Generierung als einzige Option, Agents.md „AI-Masken").
Die Identität ist analog zu AI-Masken vollständig und umfasst:

- **Quelle:** `source.content_hash` (BLAKE3 über Quellbytes) und relevante
  Decode-/Geometrieparameter (Decoder, `decode_version` inkl. `+luminaabiN`,
  Orientierung/Geometrie des Quellbildes);
- **Modellkontext:** `model.name`, `model.version`, `model.hash` (`sha256:<hex>`,
  Artefakt-Pin), sowie `inference_resolution`, `InputNormalization`
  (mean/std), Kanal-Layout, Tensor-Format und Tensor-Namen — zusammengefasst
  im `input_spec_digest` (`sha256:<hex>` unter `ModelIdentity.extras`);
- **Prompt-Kontext:** `prompt` inkl. optionalem `negative_prompt` (exakt,
  roundtrip-stabil) und `seed` (`u64`);
- **Canvas-Geometrie:** `canvas.output_width`/`output_height` und
  `source_offset_x`/`source_offset_y` (exakt, siehe >100 %-Regel);
- **Region-/Masken-Referenz:** Inpainting-Region (`region` in
  `source-normalized` Koordinaten) bzw. `mask_reference` inkl. deren
  Artefakt-Referenz (Pfad, Format, Auflösung, Prüfsumme);
- **Koordinatensystem und Ausrichtung:** Quell-Koordinatensystem
  (Orientierung aus `RawMetadata.orientation` / `source.orientation`),
  Vorverarbeitung und Nachskalierungsverfahren;
- **Artefaktmetadaten:** `artifact.format`, `artifact.width`/`height`,
  `artifact.channels`, `artifact.data_version`, `artifact.checksum` (BLAKE3);
- **Erstellungs- und Statusmetadaten:** `created_at`, `status`, optionaler
  `error`-Text;
- **Versionen:** `GenerativeEdit.version` und `pipeline_version` /
  `recipe_version` der Stufe.

Statuswerte wie bei AI-Masken: `valid`, `stale`, `missing`, `corrupt`. Ein
fehlendes Modell, ein `pending-integration`-Hash oder ein fehlendes/
checksummenabweichendes Artefakt wird sichtbar gemeldet; die GUI bietet
Neuberechnung explizit an, die CLI verhält sich analog zu `--update-masks`
(Warn-and-continue bzw. `strict`). Es gibt keinen stillen Fallback auf ein
anderes Modell oder auf „ohne generative Stufe rendern".

## Pipeline-Platzierung und Koordinatensystem

`GenerativeEdit` ist eine eigene Pipeline-Stufe nach Decode/Source-Actions und
**vor** Auto-Analyse/Adjustments/Masken/Crop, mit einer für Auto-Fill vs.
manuellen Expand differenzierten Geometrie-Einordnung:

```text
Decode → SourceActions → GenerativeEdit → AutoAnalysis → Adjustments → Masks → Crop → Output
  (generische Einordnung; für Geometrie siehe unten differenziert)
```

Begründung: Die Stufe ändert (a) Pixelinhalte wie eine Source-Action und (b)
die **Canvas-Geometrie** (Ausgabegröße und Platzierung der Quelle), sodass
alle nachgelagerten Geometrie- und Messbezüge (Masken-Koordinaten, Crop,
Perspektive, F-041-Messbereich) auf dem **post-GenerativeEdit-Canvas** laufen.

Innerhalb der dokumentierten Geometrie-Unterreihenfolge aus
`feature/architecture/pipeline.md` gilt verbindlich — differenziert nach
Auto-Fill vs. manuellem Expand:

```text
LensCorrection (F-098)
  → GenerativeEdit(auto-fill, wenn auto_fill_transparent == true,
                    füllt transparente Pixel nach Lens)
  → Perspective/Upright (F-099)
  → GenerativeEdit(expand, wenn expand_beyond_image == true,
                    vergrößert Canvas >100 %)
  → Crop (F-093, inklusive Rotation/Spiegelung, gesteuert durch keep_generative_content)
  → Output
```

Vereinfacht (wenn nur eine GenerativeEdit-Rolle aktiv ist):

```text
GenerativeEdit(auto-fill nach Lens) → Perspective → Crop
GenerativeEdit(manueller Expand vor Crop) → Crop
Kombiniert: Lens → GenerativeEdit(auto-fill) → Perspective → GenerativeEdit(expand) → Crop
```

Für den MVP darf ein einzelner `GenerativeEdit`-Record beide Flags tragen;
die Reihenfolge bleibt dann `Lens → GenerativeEdit → Perspective → Crop` und
die transparente Maske wird vor dem Expand abgeleitet. `GenerativeEdit`
definiert das Canvas; alle späteren geometrischen Stufen operieren auf diesem
Canvas, nicht auf der ursprünglichen Quellgeometrie. Widersprüche zu
`pipeline.md` sind hiermit aufgelöst: Auto-Fill ist **nach** Lens,
manueller Expand **vor** Crop (und nach Perspective).

### Koordinatenreferenzrahmen (normativ)

- Das **post-GenerativeEdit-Canvas** ist der neue Referenzrahmen für alle
  nachgelagerten, normierten Koordinaten (Crop `x/y/width/height` in `0..=1`,
  Masken-Koordinaten, Perspektive-Shift, F-041-Messbereich, Vignette/Grain).
- Die Platzierung der Quelle wird als `canvas.source_offset_x/y` +
  Quell-Dimensionen festgehalten; damit ist der Übergang vom Quell- ins
  Canvas-Koordinatensystem deterministisch reproduzierbar.
- **Regel:** Ändert sich `canvas` (Größe oder Offset), sind alle
  geometrieabhängigen Rezeptwerte, die nach dem Canvas referenzieren, neu zu
  validieren — sie werden **nie still** auf das neue Canvas umgedeutet.
  Stattdessen markiert der RenderKey einen Canvas-Wechsel als Änderung des
  geometrischen Kontexts (`output_dimensions` + `canvas` im `recipe_hash`);
  abhängige Werte werden als veraltet sichtbar gemeldet und (durch den Benutzer)
  neu gesetzt oder migriert.
- Masken, die auf dem Original gezeichnet wurden, bleiben über ihre eigene
  Referenz (Quell-Koordinaten) eindeutig; bei der Auswertung auf dem
  erweiterten Canvas werden sie über die Quell-Platzierung verschoben
  (dokumentierte Translation `+source_offset`, Teil der Maskenidentität).
- Der F-041-Messbereich (Match Total Exposure) misst nach Crop/Geometrie auf
  dem **erweiterten Canvas** (finaler sichtbarer Messbereich).

## Interaktion mit Crop/Geometry

- **Crop (F-093) und `keep_generative_content`:** Normierte Crop-Koordinaten
  referenzieren das post-GenerativeEdit-Canvas. Bei
  `keep_generative_content == true` (Default) bleibt das generative Canvas
  vollständig erhalten — Crop ist nur Ansicht/Export-Ausschnitt; das Artefakt
  (`generative_canvas`) behält `output_*` und wird nicht beschnitten. Bei
  `false` („auf aktuelle Ansicht zuschneiden") wird Crop materialisiert:
  `canvas.output_*` wird auf das normierte Crop-Rechteck reduziert und
  `source_offset` translatiert (siehe Crop-Entscheidung oben); das neue
  Canvas geht in `recipe_hash`/`output_dimensions` ein und invalidiert den
  Geometrie-Digest sichtbar. In beiden Fällen gilt: Das Erweitern über 100 %
  vergrößert den verfügbaren Crop-Bereich; ein vorher gesetzter Crop behält
  seine normierten Werte im Canvas-Rahmen, wird aber nicht automatisch
  verschoben — eine Änderung von `canvas` invalidiert den Geometrie-Digest
  und macht den sichtbaren Ausschnitt neu zu bestätigen (kein stilles
  Re-Interpretieren). `expand_beyond_image == false` verbietet
  `output_* > source_*` (Validierungsfehler).
- **Rotation/Perspektive (F-093/F-099):** Rotations- und Perspektive-Parameter
  beziehen sich auf das Canvas; der F-041-Messbereich (Match Total Exposure)
  misst nach Crop/Geometrie auf dem erweiterten Canvas (bei `keep == false`
  nach Materialisierung auf dem zugeschnittenen Canvas).
- **Objektivkorrektur (F-098) und Auto-Fill:** Verzeichnung/Vignette/CA werden
  auf dem Canvas angewandt; der Auto-Fill (`auto_fill_transparent == true`)
  füllt transparente Pixel **nach** Lens und **vor** Perspektive/Crop (siehe
  Pipeline). Ein manueller Expand (`expand_beyond_image == true`) vergrößert
  das Canvas **vor** Crop. Eine Lensfun-Capability bleibt dabei native-only.
- **GUI-Interaktion „Expand-Rahmen" und Checkbox:** Checkbox Default
  `expand_beyond_image == false` („auf Bild beschneiden") — kein Expand.
  Aktiviert (`true`) erscheint der ziehbare Expand-Rahmen (skizzierter Rand,
  transparent); die Rezept-Canvas-Geometrie wird erst beim Bestätigen gesetzt
  (kein Schreiben bei jedem Drag, kein stilles Verwerfen bei Abbruch).
  Danach bietet die Crop-Leiste die Entscheidung `keep_generative_content`
  (behalten vs. zuschneiden); beide Werte sind persistiert und gehen in die
  Identität ein.
- **Virtuelle Kopien:** `GenerativeEdit` (inkl. `auto_fill_transparent`,
  `expand_beyond_image`, `keep_generative_content`) gehört zur jeweiligen
  virtuellen Kopie (eigenes Rezept, stabile ID). Das generierte Artefakt kann
  auf Quellbild-Ebene geteilt werden, wenn Quellkontext, Modellkontext und
  Canvas-Geometrie identisch sind; Masken-Layer, Invertierung und lokale
  Anpassungen bleiben kopienspezifisch (Agents.md, „Virtuelle Kopien").
- **History/Reproduzierbarkeit:** Ein Rezept-Snapshot rendert byte-identisch
  erneut, wenn alle obigen Identitätsbestandteile (inkl. der drei neuen Flags)
  unverändert sind.

## Modell, Capability und Lizenz

- **Heimat lokaler Modelle:** `lumina-onnx` (native). Inpainting-/
  Outpainting-Modelle werden wie BiRefNet/SAM 2 über `ModelManifest` mit
  deklarierten Fähigkeiten eingebunden; die Modellfähigkeit wird aus dem
  Manifest gelesen, nicht aus dem Namen erraten. Geplante Fähigkeiten:
  `inpaint` und `outpaint` (jeweils getrennt prüfbar; ein Modell darf nur eine
  davon deklarieren).
- **Lokal vs. Cloud getrennt dokumentieren:** Die Capability-Matrix
  (`feature/platform/capability-matrix.md`) führt lokale ONNX-Inferenz und
  eine (nicht geplante) Cloud-API als **getrennte** Capabilities. Cloud-
  Rechenzentrums-Verarbeitung ist kein stiller Fallback für lokale Inferenz
  und umgekehrt; ohne dokumentierte Capability-Entscheidung gibt es keine
  Cloud-Anbindung. Vorgeschlagener Matrix-Eintrag:

  | Fähigkeit | native CLI | Desktop (eframe) |
  | --- | --- | --- |
  | Generatives Entfernen (`inpaint`, lokal ONNX) | geplant, `lumina-onnx` | geplant, `lumina-onnx` |
  | Generatives Erweitern (`outpaint`/`canvas expansion >100 %`, lokal ONNX) | geplant | geplant |
  | Generatives Entfernen/Erweitern (Cloud-API) | nicht geplant — nur mit expliziter Capability-Entscheidung | nicht geplant |

  (Browser-Spalte ENTFERNT 2026-09-04.)
- **Lizenz:** Die Modelle werden **vor Integration** lizenz- und
  hash-gepinnt dokumentiert (F-078, `feature/quality/fixtures-licensing.md`);
  keine spontanen Downloads, keine Tests gegen Netz. Ein Modell ohne
  dokumentierte Lizenz/Provenienz wird nicht eingebunden. `THIRD-PARTY-NOTICES.md`
  führt Modell-Lizenzen vor dem ersten Commit der Gewichte.
- **Fähigkeitspflicht:** Das gewählte Modell muss mindestens die Fähigkeiten
  für Inpainting bzw. Outpainting deklarieren; ein Modell ohne passende
  Fähigkeit wird abgelehnt (kein stiller Ersatz durch ein anderes Modell).
  Die Auswahl ist deterministisch (keine stille Variantenwahl zur Laufzeit).

### Modell-Entscheid Welle 1 (2026-09-15, normativ)

**Entscheid:** Es wird **kein** generatives Gewichtsmodell committet oder
heruntergeladen. Der getestete Pfad ist ein **deterministisches, hash-gepinntes
Fixture-Modell**; der Anschluss echter Gewichte ist als dokumentierte
Schnittstelle implementiert. Begründung: kein lizenzsauberes
Inpaint-/Outpaint-Modell liegt im Workspace vor, und die Doktrin verbietet
spontane Downloads bzw. Netzabhängigkeit in Tests (F-078).

- **Fixture-Identität (real, nicht `pending-integration`):**
  `lumina_onnx::fixture_manifest(role)` liefert das Manifest mit
  `model_hash = sha256:<64 hex>` — der SHA-256 über die kanonische, versionierte
  Fixture-Spezifikation (`lumina-generative-fixture-v1`: Name, Version,
  Inferenzauflösung, Tensor-Contract, Normalisierung).
  `verify_fixture_manifest` rechnet den Pin nach (`Verified`); das ist ein
  echter, prüfbarer Pin, kein Phantom. Der Fixture-Hash ist ein
  **Spezifikations-Hash** (es existieren keine Gewichte), nicht ein
  `.onnx`-Datei-Hash — das ist die bewusste, dokumentierte Abweichung der
  Pre-Integration-Fixture und wird nicht als Gewichts-Hash ausgegeben.
- **Reale Gewichte (Schnittstelle, dokumentiert):**
  `GenerativeModelSource::artifact(manifest, path)` verbindet ein lokales
  `.onnx`-Artefakt mit einem gepinnten Manifest. `resolve_manifest`
  stream-hasht die Datei (SHA-256) und **verweigert** bei
  `Mismatch`/fehlender Datei laut (`ModelArtifactStale`/`MissingModel`); ein
  `pending-integration`-Manifest wird auf dem realen Artefaktpfad **hart
  abgelehnt** (`UnsupportedModel`) — ohne gepinnten Hash keine Inferenz (kein
  Phantom-Grün). Die geplanten
  Gewichtsdeskriptoren (`inpaint_heal_manifest`, `outpaint_expand_manifest`)
  behalten bewusst den `pending-integration`-Platzhalter: das fehlende echte
  Modell ist damit sichtbar „nicht verfügbar", nie still durch die Fixture
  ersetzt.
- **Lizenz:** Für die Fixture existieren keine Gewichte; die Platzhalter-Lizenz
  der Deskriptoren (`Apache-2.0`) bleibt eine Vorintegrations-Deklaration.
  Vor dem ersten Commit echter Gewichte gilt unverändert: Lizenz/Provenienz +
  Hash in `feature/quality/fixtures-licensing.md` und
  `THIRD-PARTY-NOTICES.md` dokumentieren; viele State-of-the-Art-Inpaint-/
  Outpaint-Gewichte sind nicht-kommerziell (AGPL-/NC-Falle, vgl. §5 der
  Fixtures-Doku). Tests laufen ausschließlich gegen Fixture/Stub — **kein Netz,
  kein Download**.
- **Kein Phantom-Grün:** Ein fehlendes Modell/eine fehlende Datei ist ein
  lauter Fehler (`MissingModel`/`ModelUnavailable`), kein stilles Überspringen;
  `lumina generative --status` meldet `missing`/`stale`/`corrupt` mit Exit 1.

## UI-Flow (GUI)

Nach GUI-STAGE-1/GUI-WGPU-PRESENT-1 (Native Desktop):

1. Im Develop-Modul aktiviert der Benutzer den generativen Modus
   „Entfernen + Erweitern".
2. **Entfernen:** Objekt über Pinsel/Box auf dem Bild markieren (Maske),
   optional Prompt (und Negativ-Prompt) eingeben, „Generieren" starten.
3. **Auto-Fill Transparent (Lens):** Ist `lens_correction` aktiv und
   `auto_fill_transparent` angehakt, zeigt die Vorschau nach Lens
   transparente Keile schraffiert/skizziert; „Generieren" füllt sie
   generativ (implizite transparente Maske, kein manuelles `region`/
   `mask_reference` nötig). Deaktiviert bleibt die Lücke
   beschnitten/schwarz (kein Artefakt).
4. **Erweitern (Checkbox + Rahmen ziehen):** Unter dem Expand-Panel steht
   die Checkbox „auf Bild beschneiden" (Default **aus** → `expand_beyond_image
   == false`). Aus: kein Expand, Rahmen nicht ziehbar, `canvas == source`.
   An (`true`): „Canvas größer ziehen" — ein ziehbarer „Expand-Rahmen" über
   die Bildkante erscheint (transparenter/skizzierter Rand); Zielformat
   (Seitenverhältnis, z. B. 16:9) kann als Orientierung dienen; optional
   Prompt für den Randbereich. Der Rahmen definiert die Canvas-Geometrie
   (`output_width`/`height` + `source_offset`); >100 % ist dabei explizit
   erlaubt. Die Geometrie wird erst beim Bestätigen persistiert (kein
   Schreiben pro Drag).
5. **Crop-Entscheidung nach Expand/Auto-Fill:** Nach Bestätigen des
   generativen Canvas bietet die Crop-Leiste die Wahl
   `keep_generative_content`: „Generatives Canvas behalten" (`true`, Default)
   — Crop ist nur Ansicht, Artefakt bleibt voll; vs. „auf aktuelle Ansicht
   zuschneiden" (`false`) — Canvas wird auf das Crop-Rechteck
   materialisiert (Translation `source_offset - crop_offset`, validiert,
   `recipe_hash` ändert sich). Die Entscheidung bleibt persistiert und ist
   jederzeit umschaltbar (sichtbare Invalidierung, keine stille
   Re-Interpretation).
6. Generierung läuft als sichtbarer Job (Jobstatus, kein Hintergrund-
   Stilllauf); bei fehlendem Modell/Artefakt wird die Capability-Abwesenheit
   (`missing`/`stale`/`corrupt`) angezeigt, nicht eine gefälschte Vorschau.
7. Ergebnis wird als Artefakt (`.lumina.zdata`, `kind = "generative_canvas"`)
   persistiert und im Sidecar-Rezept referenziert; Vorschau/Export rendern
   über die gemeinsame Pipeline auf dem (ggf. zugeschnittenen) Canvas.
   „Verwerfen" löscht nur das Rezept/das Artefakt, nie das Original.
8. **Visuelle Analyse automatisch:** Jede der drei Aktionen (Auto-Fill,
   Expand, Crop-Entscheidung) ist visuell verifizierbar: Vorher/Nachher-
   Vergleich (`Y`), Navigator-Badge (`valid`/`stale`/`missing`/`corrupt`),
   Histogramm-Overlay und automatische kittest-Snapshots (siehe
   Testanforderungen, kein manueller Screenshot nötig).


## Abgrenzung zu Source-Actions und KI-Denoise

- **Source-Actions (F-042):** enthalten kontext-übergebene Reparaturregionen
  (u16-Region + RGBA8-Ersatz) **ohne** Canvas-Vergrößerung und ohne
  Modell-/Prompt-Identität. `GenerativeEdit` erweitert dieses Muster um
  Canvas-Geometrie, Modell-/Prompt-/Seed-Identität und Veraltungslogik. Beide
  Stufen wirken nach Decode und vor Auto-Analyse; `GenerativeEdit` ist dabei
  die canvas-definierende Stufe.
- **KI-Denoise:** bleibt eine separate, optionale Erweiterung (F-096 sieht nur
  den deterministischen CPU-Pfad vor) und ist unabhängig von GEN-EXPAND-1.
- **AI-Masken (F-004):** teilen Modell-Identität, `ArtifactReference`-Muster
  und zdata-Container, generieren aber Alpha-Matten, keine RGB-Canvas-Inhalte.
  Eine generative Matte ist kein Masken-Layer-Ersatz.

## Testanforderungen

Jede Implementierung von GEN-EXPAND-1 muss vor Verifizierung mindestens diese
Prüfungen bestehen (Agents.md § Verifizierung und Tests; Analogie zu
AI-Masken/virtuellen Kopien):

- **Roundtrip und Schema:** JSON-Roundtrip für `GenerativeEdit` (alle Felder,
  inkl. `negative_prompt`/`seed`/`canvas`/`region`/`artifact` **plus**
  `auto_fill_transparent`/`expand_beyond_image`/`keep_generative_content`),
  unbekannte Felder bleiben erhalten, ungültige `version`/Bereiche werden
  abgelehnt; Migration v1→v2 (sobald verschachtelte Felder hinzukommen) mit
  Backup/`migrate`. Default-Werte (`false`/`true`/`null`) roundtrippen stabil
  und sind Identität.
- **Nicht-Destruktion:** Originalbytes unverändert nach Generierung, Speichern,
  Löschen und Re-Generierung; Export schreibt neue Datei.
- **Determinismus:** Identische Eingaben (Quelle, Modell-Hash, Prompt,
  Negativ-Prompt, Seed, Canvas, Region, plus Flags) → byte-identisches Artefakt
  (BLAKE3 über unkomprimierten RGBA8-Strom).
- **Veraltung (stale/missing/corrupt):** Tests für jede einzelne
  Identitätsabweichung: Quell-Hash, Decode-Kontext/Orientierung, Modell-Hash,
  `input_spec_digest` (Auflösung/Vorverarbeitung), Prompt/Seed, Canvas-Geometrie,
  Region/Maskenref, Artefakt-Prüfsumme; sowie fehlendes Modell (`pending`),
  fehlendes Artefakt und bitflipped Artefakt (BLAKE3-Fehler). Kein stiller
  Fallback, Status sichtbar, Neuberechnung nur explizit. Zusätzlich: Änderung
  von `lens_correction` invalidiert Auto-Fill-Artefakt (`stale`).
- **Artefakt und zdata:** `artifact_status` (`Available`/`Missing`/`Corrupt`)
  für generative Artefakte (Pfad fehlt, keine reguläre Datei, Magic/Version/
  Prüfsummenfehler); relative Pfade bleiben nach Bundle-Verschiebung gültig;
  atomarer Write (Temp + Rename) und `.zdata.lock`-Serialisierung.
- **Canvas und Geometrie:** >100 %-Expand (Canvas größer als Quelle,
  Offset-Translation), reines Entfernen (Canvas == Quelle), Offset-Grenzen
  (negativ zulässig, Bounds geprüft); Crop/Perspektive/Lens-Koordinaten
  referenzieren das post-GenerativeEdit-Canvas; Canvas-Wechsel invalidiert
  Geometrie-Digest sichtbar.
- **Auto-Fill Transparent (je Modus):** Trigger nur bei `auto_fill_transparent == true`
  **und** transparenten Pixeln nach Lens; ohne transparente Pixel kein Artefakt
  nötig (`valid` ohne Generierung); mit transparenten Pixeln generiert das
  Artefakt exakt die Lücke (golden: synthetische Lens-Keile, byte-identische
  Nicht-Transparent-Pixel). Änderung von `lens_correction` → `stale` sichtbar.
- **Manueller Expand (je Modus):** `expand_beyond_image == false` (Default)
  verbietet `output_* > source_*` (Validierungsfehler); `true` erfordert
  `output_* > source_*` und setzt `canvas`; Ziehen des Rahmens ohne
  Bestätigen schreibt nicht; `Expand-Rahmen`-Koordinaten-Translation getestet.
- **Crop-Entscheidung (je Modus):** `keep_generative_content == true` (Default)
  lässt `canvas` unverändert (Crop nur Ansicht); `false` materialisiert
  `canvas = crop_rect_in_canvas_pixels` und translatiert `source_offset`
  (`new = old - crop_offset`), validiert Bounds, geht in `recipe_hash` ein.
  Umschalten `true↔false` invalidiert Geometrie-Digest sichtbar, kein stilles
  Re-Interpretieren. `canvas.output_*` vs. `crop` rect getestet.
- **Virtuelle Kopien:** Stabile ID, eigenes Rezept pro Kopie; Artefakt-Sharing
  auf Quell-Ebene nur bei identischer Identität (inkl. der drei neuen Flags);
  Masken-Layer/Invertierung bleiben kopienspezifisch.
- **Capability und Lizenz:** Fehlendes Modell wird ohne Crash gemeldet;
  Capability-Matrix trennt lokal ONNX vs. Cloud (kein Fallback); Lizenz/Hash-Pin
  vor Integration dokumentiert; Tests ohne Netz/Download, nur lokale Fixtures.
- **CLI/GUI:** CLI warnt bei `stale`/`missing` (analog `--update-masks`,
  `strict` vs. Warn-and-continue); GUI zeigt Status sichtbar und bietet
  Neuberechnung explizit an. Kein Modell-Download im Test.
- **Visuelle Analyse automatisch:** kittest-Snapshots für Expand-Rahmen
  (vor/nach Bestätigen), Auto-Fill-Keile (mit/ohne `auto_fill_transparent`),
  Crop-Entscheidung (`keep true` vs. `false`), plus Vorher/Nachher (`Y`),
  Navigator-Badge (`valid`/`stale`/`missing`/`corrupt`), Histogramm-Overlay;
  Goldens deterministisch, `UPDATE_SNAPSHOTS=true` dokumentiert, keine
  manuellen Screenshots als Gate.

**Welle-1-Belege (2026-09-15, ohne GUI):** `lumina-onnx` — Fixture-Pin
(`sha256:`-real, deterministisch, richtungssensitiv), Rollenwahl aus Flags
(nicht aus Namen), Hash-/Capability-Gate (`ModelArtifactStale`,
`UnsupportedModel`), `produce_canvas` deterministisch + identisch zum
Core-`GenerativeCacheKey`-Digest, fehlendes Modell laut. `lumina-core` —
`composite_expand`/`composite_auto_fill` adoptieren das Artefakt byteweise,
laut bei fehlendem/falschem/verkehrt-dimensionalem Artefakt; der Renderpfad
ruft **kein** BFS mehr auf (`bfs_runs`-Zähler bleibt konstant). GPU-Parität:
`lumina-gpu/tests/parity.rs::generative_canvas_compositing_is_gpu_parity`
(CPU-Oracle-Compositing == GPU über dem adoptierten Canvas, `maxAbsDiff == 0`,
Metal lokal) plus die verschärfte Refusal-Parität für Expand ohne Artefakt.
`lumina-sidecar` — `GenerativeArtifactRef::from_generative_canvas`,
`save_generative_canvas(replace=true)` (expliziter Regenerationspfad) und
`GenerativeCanvasArtifact`-Replace; `generative_artifact_status` unverändert
identitätsgeprüft. CLI — `lumina generative --generate|--status|--remove`
inkl. Exit 1 bei `missing`/`stale`/`corrupt` und identitätsgeprüftem
Render-Hook. **Welle 2a (2026-09-15, ohne GUI; Verifikation OFFEN — Re-Verifizierung nach
Blocker-Fix „trailing Substitute"):** `lumina-gpu`
`render_with_gpu_and_generative` + `Substitute`-Schritt; Parity-Tests
`generative_canvas_compositing_is_gpu_parity`,
`generative_auto_fill_compositing_is_gpu_parity` und
`generative_trailing_substitute_after_render_pass_is_gpu_parity` (`maxAbsDiff == 0`),
`artifact_blind_generative_render_is_loud_not_routed`; `unsupported_gpu_stages*`
ohne `generative_edit`; CLI-Reason entfernt. `lumina-sidecar`
`GenerativeEdit::{negative_prompt,set_negative_prompt,validate_edit_extras}` +
Dokumentvalidierung; CLI `--negative-prompt` + Drift-Test,
Auto-Fill-Caller-Konventionstest
(`generative_auto_fill_without_transparency_needs_no_artifact`). **Welle 2b (2026-09-16,
Verifizierung BESTANDEN nach Rework + VISION-OK):**
GUI-Preview/Export-Hook (`render_frame_from_base_with_generative` /
`export_image_with_generative`, GUI-eigener Resolver, Generate-Aktion mit
atomarer zdata-Persistenz, Auto-Fill-Caller-Konvention + CPU-Defense-in-Depth),
3 kittest-Goldens (Expand off/ready, Auto-Fill; deterministische
Relativ-Fixtures), Doppelrollen-Record in GUI + CLI (Expand-Link-Vorrang,
Auto-Fill per identitätsabgeleiteter Record-ID, Reihenfolge-Test, Reload via
`load_sidecar`), F4-Render-Error-Fix (kein `let _ = self.render()` mehr),
VRAM-Refusal-Badge (E2E-Test), 4-stufiger Rollen-Status
(valid/stale/missing/corrupt).

## Abnahme

- Original bleibt byteweise unverändert; das Ergebnis ist ein ableitbares,
  löschbares Artefakt (`.lumina.zdata`, `kind = "generative_canvas"`).
- `GenerativeEdit`-Rezept-Roundtrip: Modell (Name/Version/Hash), Prompt
  (inkl. Negativ-Prompt), Seed, Inferenzauflösung, Canvas-Geometrie,
  Region/Maskenref und Prüfsumme überstehen Persistenz/Laden verlustfrei.
- Identische Eingaben (Quelle, Modellkontext, Prompt, Seed, Canvas) erzeugen
  ein byte-identisches Artefakt (Determinismus, getestet, BLAKE3).
- Quell-, Decode-, Modell-, Prompt-, Seed- oder Canvas-Änderung markieren die
  Operation als `stale`; fehlendes Modell/Artefakt oder Prüfsummenfehler werden
  sichtbar als `missing`/`corrupt` gemeldet — kein stiller Fallback.
- Canvas >100 % ist zulässig (`output_* > source_*`), Quelle wird via
  `source_offset` deterministisch platziert (inkl. negativer Offsets innerhalb
  Bounds).
- Crop/Geometry-Koordinaten referenzieren das post-GenerativeEdit-Canvas;
  ein Canvas-Wechsel invalidiert den Geometrie-Kontext sichtbar statt still
  zu verschieben; Reihenfolge `Lens → GenerativeEdit(auto-fill) → Perspective → GenerativeEdit(expand) → Crop →
  Rotation → Mirror` eingehalten (vereinfacht `Lens → GenerativeEdit → Perspective → Crop` wenn nur eine Rolle aktiv; siehe Pipeline-Platzierung), sowie Abnahme für Auto-Fill (`auto_fill_transparent`), manuellen Expand (`expand_beyond_image`) und Crop-Entscheidung (`keep_generative_content`) — jeweils Roundtrip, Trigger/Bounds/Translation und sichtbare Invalidierung getestet.
- Capability-Matrix trennt lokal ONNX vs. Cloud (kein stiller Fallback);
  Lizenz/Hash-Pin vor Integration dokumentiert (F-078).
- Veraltungs-, Artefakt-, Canvas- und Geometrie-Interaktionstests sind durch
  einen unabhängigen Verifizierungs-Agenten bestätigt.
- `cargo check --workspace` grün.

## Offene Punkte und Abhängigkeiten

- **Abhängigkeiten:** F-082/F-083 (SAM-Adapter, `lumina-onnx`) existiert;
  echte Inpainting-/Outpainting-Gewichte bleiben `pending-integration`
  (Welle-1-Entscheid: Fixture-Pfad + dokumentierte
  `GenerativeModelSource::artifact`-Schnittstelle); GUI-Flow erst nach
  GUI-STAGE-1/GUI-WGPU-PRESENT-1. Der GPU-Injektionspunkt ist mit Welle 2a
  umgesetzt; nur der readback-freie VRAM-Present-Pfad lehnt eine aktive
  generative Stufe weiter laut ab (kein Injektionspunkt ohne Readback).
- **Offen (Welle 2b, erledigt 2026-09-16, Verifizierung BESTANDEN):**
  GUI-Preview/Export-Anbindung des Artefakt-Hooks (CPU-artifact-aware; zum
  GPU-Present siehe GUI-GPU-Entscheid unten), 3 kittest-Goldens für
  Expand-Rahmen/Auto-Fill (VISION-OK), ein Record mit beiden Rollen
  (`auto_fill_transparent` + `expand_beyond_image`) samt Reihenfolge-Test in
  GUI **und** CLI. Cloud-API-Capability bleibt bewusst getrennt und nicht
  geplant.
- **GUI-GPU-Entscheid (2026-09-16, normativ):** Die GUI rendert Preview und
  Export über den artefakt-bewussten CPU-Pfad
  (`render_frame_from_base_with_generative` / `export_image_with_generative`);
  der GPU-Pfad ist ausschließlich der readback-freie VRAM-Present
  (`render_to_vram`), der artefakt-blind ist und eine aktive generative Stufe
  laut ablehnt (kein VRAM-Injektionspunkt ohne Readback).
  `GpuContext::render_with_gpu_and_generative` hat daher bewusst keinen
  GUI-Aufrufer; die Ablehnung wird als sichtbares Routing-Badge klassifiziert
  (kein stiller CPU-Fallback). Ein künftiger VRAM-Present-Pfad mit
  Generative-Injektion ersetzt die Ablehnung; bis dahin bleibt die CPU die
  vollständige Referenz.
- **Folgearbeit (nicht blockierend, dokumentiert statt Todo — kein neuer Task):**
  GUI-Control + Golden für die Crop-Entscheidung `keep_generative_content`
  (UI-Flow Schritt 5, derzeit kein Bedienelement); Auto-Fill-Golden mit echter
  Lens-Verzerrung; Core-Helfer gegen GUI↔CLI-Duplikation
  (`produce/persist/generative_expand_input`); F-074-Perf für
  Doppelrolle/Drag ungemessen; Sekundärkonsumenten (Navigator/Neighbor/Matrix)
  degradieren sichtbar; GUI↔CLI-Digest-Paritätstest für Doppelrollen-Bundle.
- **Entschieden (Welle 2a, 2026-09-15):** GPU-Artefakt-Injektion
  (`Substitute`-Schritt, CPU-Oracle-Parität byte-identisch, keine pauschale
  CPU-Route mehr); additives `negative_prompt`-Feld (extras-gestützt, siehe
  Schema-Entscheid oben).
- **Entschieden (Welle 1, 2026-09-15):** Modellwahl = deterministisches
  Fixture-Modell + dokumentierte Echtgewicht-Schnittstelle (siehe § Modell,
  Capability und Lizenz); Render-Stufe = Artefakt-Compositing (BFS entfällt);
  durable `zdata`-Verdrahtung aktiv (Identität schreiben + Caller-Hook).
