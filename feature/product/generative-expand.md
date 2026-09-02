# Generativer Modus „Entfernen + Erweitern“ (GEN-EXPAND-1)

**Feature:** GEN-EXPAND-1 Optionaler generativer Modus „Entfernen + Erweitern“

## Inhaltsverzeichnis

- [Ziel und Abgrenzung](#ziel-und-abgrenzung)
- [Ist-Stand](#ist-stand)
- [Normative Invarianten](#normative-invarianten)
- [Rezeptmodell `GenerativeEdit`](#rezeptmodell-generativeedit)
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

**Stand 2026-09-01:** Nur dokumentiert, Implementierung **nicht** begonnen
(repo-weit verifiziert: kein Code, keine `GenerativeEdit`-Stufe). Es gibt
keine Inpainting-/Outpainting-Modelle im Workspace; `lumina-onnx` ist die
vorgesehene Heimat lokaler Modelle (F-082/F-083-SAM-Adapter existiert, echte
Modellgewichte weiterhin `pending-integration`). Dieses Dokument ist das
normative SOLL für die spätere Umsetzung. Die Implementierung erfolgt später in
`lumina-onnx` (Modellverwaltung, Inferenz) und `lumina-core` (Pipeline-Stufe)
sowie `lumina-sidecar` (Schema, Validierung, Migration) nach GUI-STAGE-1.
Bis dahin wird **kein Crate-Code** angelegt — diese Datei ist die
Doku-first-Vervollständigung.

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

Validierung: unbekannte Felder bleiben via `serde(flatten)` roundtrip-erhalten
(pre-MVP, `feature/architecture/sidecar.md`), unbekannte `version` wird
abgelehnt. `seed`/`canvas`/`prompt`/`model` gehen in `recipe_hash` und
`RenderKey` ein; jede Änderung invalidiert Preview/Export ab `GenerativeEdit`,
nicht Decode.

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
  lazy).
- Auf WASM ist `zdata`/`zstd` nicht verfügbar (native-only, target-gegatet,
  `feature/platform/capability-matrix.md`); dort gilt das Artefakt als
  `missing`/`unverifizierbar` und wird nicht still ersetzt.

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
**vor** Auto-Analyse/Adjustments/Masken/Crop:

```text
Decode → SourceActions → GenerativeEdit → AutoAnalysis → Adjustments → Masks → Crop → Output
```

Begründung: Die Stufe ändert (a) Pixelinhalte wie eine Source-Action und (b)
die **Canvas-Geometrie** (Ausgabegröße und Platzierung der Quelle), sodass
alle nachgelagerten Geometrie- und Messbezüge (Masken-Koordinaten, Crop,
Perspektive, F-041-Messbereich) auf dem **post-GenerativeEdit-Canvas** laufen.

Innerhalb der dokumentierten Geometrie-Unterreihenfolge aus
`feature/architecture/pipeline.md` gilt verbindlich:

```text
GenerativeEdit
  → LensCorrection (F-098) → Perspective/Upright (F-099)
  → Crop (F-093, inklusive Rotation/Spiegelung)
  → Output
```

`GenerativeEdit` definiert das Canvas; alle späteren geometrischen Stufen
operieren auf diesem Canvas, nicht auf der ursprünglichen Quellgeometrie.

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

- **Crop (F-093):** Normierte Crop-Koordinaten referenzieren das
  post-GenerativeEdit-Canvas. Das Erweitern über 100 % vergrößert den
  verfügbaren Crop-Bereich; ein vorher gesetzter Crop behält seine normierten
  Werte im Canvas-Rahmen, wird aber nicht automatisch verschoben — eine
  Änderung von `canvas` invalidiert den Geometrie-Digest und macht den
  sichtbaren Ausschnitt neu zu bestätigen (kein stilles Re-Interpretieren).
- **Rotation/Perspektive (F-093/F-099):** Rotations- und Perspektive-Parameter
  beziehen sich auf das Canvas; der F-041-Messbereich (Match Total Exposure)
  misst nach Crop/Geometrie auf dem erweiterten Canvas.
- **Objektivkorrektur (F-098):** Verzeichnung/Vignette/CA werden auf dem
  Canvas angewandt (nach `GenerativeEdit`, vor Perspektive/Crop). Eine
  Lensfun-Capability bleibt dabei native-only.
- **GUI-Interaktion „Expand-Rahmen":** Beim Ziehen des Erweiterungsrahmens
  zeigt die Vorschau das vergrößerte Canvas mit transparentem/skizziertem
  Randbereich; die Rezept-Canvas-Geometrie wird erst beim Bestätigen gesetzt
  (kein Schreiben bei jedem Drag, kein stilles Verwerfen bei Abbruch).
- **Virtuelle Kopien:** `GenerativeEdit` gehört zur jeweiligen virtuellen Kopie
  (eigenes Rezept). Das generierte Artefakt kann auf Quellbild-Ebene geteilt
  werden, wenn Quellkontext, Modellkontext und Canvas-Geometrie identisch
  sind; Masken-Layer, Invertierung und lokale Anpassungen bleiben kopienspezifisch
  (Agents.md, „Virtuelle Kopien").
- **History/Reproduzierbarkeit:** Ein Rezept-Snapshot rendert byte-identisch
  erneut, wenn alle obigen Identitätsbestandteile unverändert sind.

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

  | Fähigkeit | native CLI | Desktop (eframe) | Browser (WASM) |
  | --- | --- | --- | --- |
  | Generatives Entfernen (`inpaint`, lokal ONNX) | geplant, `lumina-onnx` | geplant, `lumina-onnx` | nein (kein lokales ONNX im Browser ohne `onnx-wasm`) |
  | Generatives Erweitern (`outpaint`/`canvas expansion >100 %`, lokal ONNX) | geplant | geplant | nein |
  | Generatives Entfernen/Erweitern (Cloud-API) | nicht geplant — nur mit expliziter Capability-Entscheidung | nicht geplant | nicht geplant |

- **Browser:** ONNX im Browser ist eine optionale Fähigkeit (F-070,
  `onnx-wasm`, off by default) — eine `GenerativeEdit`-Nutzung im Browser ist
  erst mit dieser Capability möglich und wird sichtbar ausgewiesen.
- **Lizenz:** Die Modelle werden **vor Integration** lizenz- und
  hash-gepinnt dokumentiert (F-078, `feature/quality/fixtures-licensing.md`);
  keine spontanen Downloads, keine Tests gegen Netz. Ein Modell ohne
  dokumentierte Lizenz/Provenienz wird nicht eingebunden. `THIRD-PARTY-NOTICES.md`
  führt Modell-Lizenzen vor dem ersten Commit der Gewichte.
- **Fähigkeitspflicht:** Das gewählte Modell muss mindestens die Fähigkeiten
  für Inpainting bzw. Outpainting deklarieren; ein Modell ohne passende
  Fähigkeit wird abgelehnt (kein stiller Ersatz durch ein anderes Modell).
  Die Auswahl ist deterministisch (keine stille Variantenwahl zur Laufzeit).

## UI-Flow (GUI)

Nach GUI-STAGE-1/GUI-WGPU-PRESENT-1 (Native Desktop):

1. Im Develop-Modul aktiviert der Benutzer den generativen Modus
   „Entfernen + Erweitern".
2. **Entfernen:** Objekt über Pinsel/Box auf dem Bild markieren (Maske),
   optional Prompt (und Negativ-Prompt) eingeben, „Generieren" starten.
3. **Erweitern:** „Expand-Rahmen" über die Bildkante ziehen; Zielformat
   (Seitenverhältnis, z. B. 16:9) kann als Orientierung dienen; optional
   Prompt für den Randbereich. Der Rahmen definiert die Canvas-Geometrie
   (`output_width`/`height` + `source_offset`); >100 % ist dabei explizit
   erlaubt.
4. Generierung läuft als sichtbarer Job (Jobstatus, kein Hintergrund-
   Stilllauf); bei fehlendem Modell/Artefakt wird die Capability-Abwesenheit
   (`missing`/`stale`/`corrupt`) angezeigt, nicht eine gefälschte Vorschau.
5. Ergebnis wird als Artefakt (`.lumina.zdata`, `kind = "generative_canvas"`)
   persistiert und im Sidecar-Rezept referenziert; Vorschau/Export rendern
  über die gemeinsame Pipeline auf dem erweiterten Canvas. „Verwerfen"
   löscht nur das Rezept/das Artefakt, nie das Original.

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
  inkl. `negative_prompt`/`seed`/`canvas`/`region`/`artifact`), unbekannte
  Felder bleiben erhalten, ungültige `version`/Bereiche werden abgelehnt;
  Migration v1→v2 (sobald verschachtelte Felder hinzukommen) mit Backup/`migrate`.
- **Nicht-Destruktion:** Originalbytes unverändert nach Generierung, Speichern,
  Löschen und Re-Generierung; Export schreibt neue Datei.
- **Determinismus:** Identische Eingaben (Quelle, Modell-Hash, Prompt,
  Negativ-Prompt, Seed, Canvas, Region) → byte-identisches Artefakt (BLAKE3
  über unkomprimierten RGBA8-Strom).
- **Veraltung (stale/missing/corrupt):** Tests für jede einzelne
  Identitätsabweichung: Quell-Hash, Decode-Kontext/Orientierung, Modell-Hash,
  `input_spec_digest` (Auflösung/Vorverarbeitung), Prompt/Seed, Canvas-Geometrie,
  Region/Maskenref, Artefakt-Prüfsumme; sowie fehlendes Modell (`pending`),
  fehlendes Artefakt und bitflipped Artefakt (BLAKE3-Fehler). Kein stiller
  Fallback, Status sichtbar, Neuberechnung nur explizit.
- **Artefakt und zdata:** `artifact_status` (`Available`/`Missing`/`Corrupt`)
  für generative Artefakte (Pfad fehlt, keine reguläre Datei, Magic/Version/
  Prüfsummenfehler); relative Pfade bleiben nach Bundle-Verschiebung gültig;
  atomarer Write (Temp + Rename) und `.zdata.lock`-Serialisierung.
- **Canvas und Geometrie:** >100 %-Expand (Canvas größer als Quelle,
  Offset-Translation), reines Entfernen (Canvas == Quelle), Offset-Grenzen
  (negativ zulässig, Bounds geprüft); Crop/Perspektive/Lens-Koordinaten
  referenzieren das post-GenerativeEdit-Canvas; Canvas-Wechsel invalidiert
  Geometrie-Digest sichtbar.
- **Virtuelle Kopien:** Stabile ID, eigenes Rezept pro Kopie; Artefakt-Sharing
  auf Quell-Ebene nur bei identischer Identität; Masken-Layer/Invertierung
  bleiben kopienspezifisch.
- **Capability und Lizenz:** Fehlendes Modell wird ohne Crash gemeldet;
  Capability-Matrix trennt lokal ONNX vs. Cloud (kein Fallback); Lizenz/Hash-Pin
  vor Integration dokumentiert; Tests ohne Netz/Download, nur lokale Fixtures.
- **CLI/GUI:** CLI warnt bei `stale`/`missing` (analog `--update-masks`,
  `strict` vs. Warn-and-continue); GUI zeigt Status sichtbar und bietet
  Neuberechnung explizit an. Kein Modell-Download im Test.

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
  zu verschieben; Reihenfolge `GenerativeEdit → Lens → Perspective → Crop →
  Rotation → Mirror` eingehalten.
- Capability-Matrix trennt lokal ONNX vs. Cloud (kein stiller Fallback);
  Lizenz/Hash-Pin vor Integration dokumentiert (F-078).
- Veraltungs-, Artefakt-, Canvas- und Geometrie-Interaktionstests sind durch
  einen unabhängigen Verifizierungs-Agenten bestätigt.
- `cargo check --workspace` (und wasm-Gates) grün.

## Offene Punkte und Abhängigkeiten

- **Abhängigkeiten:** F-082/F-083 (SAM-Adapter, `lumina-onnx`) existiert;
  lokale Inpainting/Outpainting-Modelle und deren Artefakte
  (`pending-integration`); GUI-Flow erst nach GUI-STAGE-1/
  GUI-WGPU-PRESENT-1; `lumina-gpu`/Present-Pfad berührt.
- **Offen:** Modellauswahl (Modellfamilie mit dynamischer Variantenwahl wie bei
  SAM 2.1 oder fixe Modelle), Cloud-API-Capability (bewusst getrennt, siehe
  oben), WASM-Pfad (F-070, `onnx-wasm` off by default), Schema-/
  Migrationsentscheidung für die Rezept-Stufe vor Implementierung (neue
  versionierte Stufe `GenerativeEdit`, additives Schema-v2-Feld, Migration
  dokumentiert, kein Pre-MVP-Bruch ohne Bump).
