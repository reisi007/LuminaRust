# Nicht-destruktive Renderpipeline

**Features:** F-003 Nicht-destruktive Entwicklung, F-005 Arbeitsfarbraum,
Pipeline-Reihenfolge, Bit-Tiefen, Clipping, Transferfunktionen und
Farbprofilstrategie, F-008 Auto-Tone und Exposure Matching

Dieses Dokument ist die **normative** Spezifikation der Renderpipeline. Die
hier beschriebenen Stufen, Formate, Bit-Tiefen und Clipping-Regeln sind an die
tatsächlich implementierte Pipeline in `crates/lumina-core/src/pipeline.rs`
angeglichen (Abweichungen wurden zugunsten des Codes korrigiert).

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Pipeline-Reihenfolge](#pipeline-reihenfolge)
- [Arbeitsfarbraum (normativ)](#arbeitsfarbraum-normativ)
- [Bit-Tiefen (normativ)](#bit-tiefen-normativ)
- [Transferfunktionen (normativ)](#transferfunktionen-normativ)
- [Clipping (normativ)](#clipping-normativ)
- [Farbprofilstrategie (normativ)](#farbprofilstrategie-normativ)
- [Optionale Stufen und Adjustment-Semantik](#optionale-stufen-und-adjustment-semantik)
- [Bearbeitungsregler](#bearbeitungsregler)
- [Reproduzierbarkeit](#reproduzierbarkeit)
- [Auto-Tone](#auto-tone)
- [Exposure Matching](#exposure-matching)
- [Cache und Invalidierung](#cache-und-invalidierung)
- [Abnahme](#abnahme)

## F-036 Globale Tonwerte und Weißabgleich

Die flache `recipe.adjustments`-Map bleibt mit Schema v1 kompatibel. F-036
verwendet die Schlüssel `wb_temperature` (Kelvin, `1500..=12000`), `wb_tint`
(`-1..=1`), `exposure` (`-10..=10` EV), `contrast`, `highlights`, `shadows`,
`whites` und `blacks` (jeweils `-1..=1`). Kelvin und die normierte Tint-Skala
sind absichtlich geräteunabhängig und vermeiden UI-spezifische Prozentwerte.
Ungültige Werte werden abgelehnt, nicht geclippt. `wb_temperature`/`wb_tint`
bilden im Raster-MVP eine deterministische RGB-Näherung; ohne WB-Schlüssel ist
die Identität (As-Shot wird erst möglich, wenn `RawMetadata.camera_white_balance`
über eine Core-API übergeben wird). Für Nicht-RAW gilt ebenfalls Identität.

Die aktive MVP-Pipeline bleibt `Decode → SourceActions → AutoAnalysis →
Adjustments → Masks → Crop → Output` und arbeitet sRGB-codiertem RGBA8. Eine
echte lineare Zwischenrepräsentation ist daher noch nicht vorhanden. Sobald ein
linearer Pfad aktiviert wird, muss Weißabgleich dort vor Tone-Mapping und vor
sRGB-Encoding erfolgen; im aktuellen Raster-MVP werden die bestehenden Regler
deterministisch im sRGB-Arbeitsraum angewandt und behalten ihre bisherige
Clipping-Semantik.

Innerhalb von `Adjustments` gilt verbindlich: `exposure → contrast →
shadows → highlights → whites → blacks` (mit WB vor diesen Tonwerten). Exposure
und Kontrast setzen den globalen Pegel und die Spreizung; Highlights/Shadows
schützen selektiv die oberen/unteren Bereiche; Whites/Blacks dehnen zuletzt die
Tonwertskala an den Rändern. Die Whites/Blacks-Anwendung ist gewichtet-linear
pro Kanal mit `x` in `0..=1` (Lightroom-Semantik: positives `whites` hebt die
Lichter an, positives `blacks` senkt die Schatten ab, 0 ist Identität):

- Whites: `weight_w = max(0, (x - 0.5) / 0.5)`, `x' = clamp(x + whites * weight_w * 0.25)`
- Blacks: `weight_b = max(0, (0.5 - x) / 0.5)`, `x' = clamp(x - blacks * weight_b * 0.25)`

Auto-Tone wird davor berechnet und angewandt;
manuelle Werte überschreiben nicht stillschweigend persistierte Auto-Ergebnisse.
0 ist für jeden Regler Identität, Operationen sind monoton und pro Kanal auf
`0..=1` geclippt. Neue flache Schlüssel sind Bestandteil des bestehenden
`recipe_hash` und invalidieren daher Preview/Export automatisch.

**Status (F-036-N1):** Die Core-API
`ImageFrame::apply_recipe_with_white_balance(recipe, camera_white_balance)` ist
implementiert. `Some(gains)` ist die explizite As-Shot-Basis aus
`RawMetadata.camera_white_balance`: Alle vier Werte müssen endlich und > 0
sein, sonst wird die Anwendung mit `CoreError::InvalidAdjustment` abgelehnt
(kein stiller Fallback, keine partielle Mutation). Ohne WB-Schlüssel bleibt die
Identität — die Gains werden nicht erneut angewandt, da der RAW-Decoder As-Shot
bereits auf den Frame angewendet hat (keine Doppel-Anwendung); mit WB-Schlüsseln
gilt unverändert die deterministische sRGB-Näherung. CLI (`process_selected`)
und GUI (`LuminaApp::load_bytes`/`render`) reichen
`RawMetadata.camera_white_balance` durch; `apply_recipe` delegiert weiterhin
ohne Kontext. GPU (seit 2026-09-14, BESTANDEN): valider As-Shot-Kontext ist
GPU-fähig — Caller binden ihn per `GpuContext::set_camera_white_balance`
(CLI/MCP/GUI tun das), der GPU-Eintritt validiert Oracle-identisch; nur
invalide Gains routen laut auf CPU
(`camera_white_balance (invalid As-Shot gains)`). Verbleibende Grenze: Die Auto-WB-Nutzung des Kontexts folgt mit
F-042, ein linearer Weißabgleichspfad später.

## Ziel

Vorschauen und Exporte werden immer aus dem unveränderten Original, der
gewählten virtuellen Kopie, dem Rezept, den Artefakten und expliziten
Versionen erzeugt. GUI und CLI verwenden dieselbe Renderpipeline.

## Pipeline-Reihenfolge

Die implementierte `Pipeline::default()` definiert die verbindliche
Stufenreihenfolge. Jede Stufe besitzt ein `(Stufe, Eingabeformat,
Ausgabeformat)`-Tupel; `Pipeline::validate()` stellt sicher, dass das
Ausgabeformat einer Stufe dem Eingabeformat der folgenden entspricht:

```text
Decode          EncodedSource  -> Rgba8Srgb
SourceActions   Rgba8Srgb      -> Rgba8Srgb
AutoAnalysis    Rgba8Srgb      -> Rgba8Srgb
Adjustments     Rgba8Srgb      -> Rgba8Srgb
Masks           Rgba8Srgb      -> Rgba8Srgb
Crop            Rgba8Srgb      -> Rgba8Srgb
Output          Rgba8Srgb      -> Output
```

Die vollständige Pipeline verbleibt im Format `Rgba8Srgb` (sRGB-codiertes
8-Bit-RGBA). Eine explizite Linearisierung in einen linearen Arbeitsraum
erfolgt im aktuellen Raster-MVP **nicht**.

> **Hinweis GenerativeEdit / Spot-Remove (GEN-EXPAND-1 / SPOT-REMOVE-1, Stand 2026-09-15):**
> Die generative Stufe `GenerativeEdit` (siehe `feature/product/generative-expand.md`) und die Spot-Stufe
> `SpotHeal` (siehe `feature/product/spot-removal.md`) sind implementiert; `Pipeline::default()`
> bleibt bewusst die grobe Top-Level-Form, die `GenerativeEdit`-Unterschritte laufen entkoppelt
> innerhalb der Geometrie. Ihre normative Reihenfolge ist:
> `Decode → SourceActions → SpotHeal(quick/generative) → LensCorrection(F-098) → GenerativeEdit(auto-fill) → Perspective(F-099) → GenerativeEdit(expand) → Crop(F-093) → Output`,
> vereinfacht `Lens → GenerativeEdit → Perspective → Crop` bzw. `SpotHeal → Lens → Perspective → Crop`.
> Auto-Fill Transparent liegt **nach** Lens, manueller Expand (`expand_beyond_image`) **vor** Crop (nach Perspective);
> `keep_generative_content` steuert, ob Crop das generative Canvas materialisiert. Spot-Generativ (`kind = "spot_heal_generative"`)
> ist von `generative_canvas` getrennt (eigene Capability `inpaint`/`inpaint_heal`).
> **GEN-ONNX-1 Welle 2a (2026-09-15):** Die Stufe ist **Artefakt-Compositing** (kein Heuristik-BFS
> mehr) und läuft auf GPU **und** CPU: Der Producer (`lumina-onnx`) erzeugt das kompositierte
> `generative_canvas`-Artefakt samt vollständiger Identität (Rolle/Seed/Canvas/Pixel-Digest +
> Prompt/Negativ-Prompt/Modell-Hash); der Renderer **adoptiert** es per Caller-Hook. GPU:
> `GpuContext::render_with_gpu_and_generative` fügt an derselben mittigen Position einen
> `Substitute`-Schritt ein (CPU-Oracle-Parität byte-identisch); ohne Artefakt ist der Render laut,
> keine pauschale CPU-Route mehr.

## Arbeitsfarbraum (normativ)

- Der interne Arbeitsfarbraum des Raster-MVP ist **sRGB-codiertes RGBA8**
  (`PipelineFormat::Rgba8Srgb`, je Kanal ein `u8` in `0..=255`, Alpha erhalten).
- Das Enum `PipelineFormat` kennt zusätzlich `LinearProPhotoRgb`. Dieses ist
  als Reservat für eine spätere, echte lineare ProPhoto-RGB-Verarbeitung
  vorgesehen, wird aber von `Pipeline::default()` **nicht** verwendet — es
  durchläuft aktuell keine Stufe. Eine Pipelinestufe ohne definierte
  Reihenfolge, Versionierung und Tests ist unzulässig.
- ProPhoto RGB und Rec.2020 dürfen nicht gleichzeitig als unbestimmte
  Arbeitsraumalternative verwendet werden. Im aktuellen MVP ist der einzige
  aktive Arbeitsraum sRGB.
- HINWEIS zur SOLL-Abweichung: `feature/README.md` listet unter
  „Festgelegte Entscheidungen“ weiterhin „lineares ProPhoto RGB“ als
  Zielarbeitsraum. Die Implementierung liefert diesen Pfad noch nicht; die
  normative Pipeline arbeitet in sRGB. Die ProPhoto-Linearisierung bleibt ein
  dokumentiertes Ziel (reserviert über `LinearProPhotoRgb`), nicht der
  implementierte Zustand.

## Bit-Tiefen (normativ)

- Die durchgehende Arbeits-Bit-Tiefe der Pipeline ist **8 Bit pro Kanal**
  (RGBA8).
- Der native RAW-Decoder (`lumina-raw`) unterstützt als Ausgabe **8 oder 16
  Bit** je Kanal (`RawDecodeOptions::output_bits`, geprüft auf `8 | 16`).
  Die dekodierten Rohdaten werden vor Eintritt in die Pipeline auf RGBA8
  reduziert (16-Bit-Werte werden um 8 Bit nach rechts geschoben), sodass alle
  nachfolgenden Stufen in RGBA8 arbeiten.
- Exportformate (PNG, JPEG, WebP) sind 8-Bit-Container; tiefere Bit-Tiefen im
  Export sind im Modell vorbereitet, aber im MVP nicht implementiert.

## Transferfunktionen (normativ)

- Der RAW-Decoder liefert bereits **sRGB-codierte** Pixeldaten
  (`libraw_set_output_color(..., 1)`), ohne automatische Helligkeitsanpassung
  (`no_auto_bright = 0` überlässt LibRaw die Entscheidung; Kamera-White-Balance
  und -Matrix sind aktiv).
- Eine zusätzliche Gamma-Dekodierung oder Linearisierung findet in der Pipeline
  nicht statt: Die Werte in `Rgba8Srgb` sind sRGB-kodiert und werden als solche
  additiv und multiplikativ bearbeitet.
- Auto-Tone und Exposure Matching messen die **sRGB-kodierte** Helligkeit: Die
  RGB-Kanäle werden auf `0..=1` normalisiert (Wert `/ 255`) und mit Rec.709
  (`0.2126 / 0.7152 / 0.0722`) gewichtet. Alpha wird ignoriert.

## Clipping (normativ)

- Jeder Kanal wird nach multiplikativen/multiplikativen Operationen auf `0..=255`
  begrenzt (`clamp(0.0, 255.0)` als `u8`). Alpha bleibt unverändert.
- Die globalen Raster-Adjustments arbeiten auf dem normalisierten `x` in
  `0..=1`; das Ergebnis wird zurück auf `0..=255` skaliert und begrenzt.
- Ungültige Adjustment-Werte werden **nicht** still geclippt, sondern mit einem
  Fehler abgelehnt:
  - `exposure` endlich und in `-10..=10` EV
  - `contrast`, `highlights`, `shadows`, `whites`, `blacks`, `wb_tint` endlich
    und in `-1..=1`
  - `wb_temperature` endlich und in `1500..=12000` Kelvin
  - unbekannte Adjustment-Keys werden mit `UnsupportedAdjustment` abgelehnt
- Auto-Tone und Exposure Matching sind explizit gegen Division durch null,
  extreme Zielwerte und Clipping abgesichert (Epsilon-Schutz, Begrenzung auf
  `-10..=10` EV).

## Farbprofilstrategie (normativ)

- **Ausgabeprofil:** Standard ist **sRGB**. Der `RenderKey` führt ein
  `output_profile`-Feld (Zeichenkette); weitere Profile sind im Modell
  vorbereitet, im MVP aber nicht implementiert.
- **Quellprofile:** Der RAW-Decoder extrahiert Kamera-Matrix
  (`camera_matrix`), Kamera-White-Balance (`camera_white_balance`),
  Vor-Multiplikatoren (`pre_multipliers`) und ein optionales eingebettetes
  ICC-Profil (`icc_profile`). Im MVP wird die sRGB-Ausgabe von LibRaw ohne
  eigene zusätzliche Farbtransformation verwendet; eine proprietäre
  Matrix-/Profilanwendung ist noch nicht in die Pipeline integriert.
- **Persistenz von Farbkontext:** Decode-Version, Pipeline-Version und
  `output_profile` gehören zum Render-Key, sodass ein Wechsel des
  Farbkontexts gezielt invalidiert.
- **Decode-Version-Identität (F-102):** Für den nativen LibRaw-Decoder
  (`decoder == "libraw"`) trägt `DecodeFingerprint.version` bzw.
  `RenderKey.decode_version` die **gelinkte LibRaw-Bibliotheksversion**
  (über `lumina_raw::libraw_version()` / `libraw_decode_version()` aus
  `crates/lumina-raw`), nicht die Anwendungsversion. Dadurch erkennt
  LuminaRust einen LibRaw-Upgrade-Versionswechsel und invalidiert Caches und
  persistierte Masken, statt sie stillschweigend wiederzuverwenden — CR3-
  Dimensionen ändern sich z.B. zwischen LibRaw 0.21.x (6160×4144) und
  0.22.x (6032×4024). Nicht-RAW-Decoder (`"image"`/raster) behalten die
  Anwendungsversion. `libraw_version()` liefert das
  Build-Suffix (z.B. `"0.22.2-Release"`); ein reiner Formatwechsel
  (Release↔Debug bei gleicher Nummer) invalidiert aktuell unnötig und könnte
  später auf das numerische Tripel normalisiert werden.
  **Generierungs-Suffix:** Ändert eine LuminaRust-seitige Korrektur das
  beobachtbare Decode-Ergebnis, ohne die gelinkte Bibliotheksversion zu
  ändern, hängt `libraw_decode_version()` ein `+luminaabiN`-Suffix an
  (`abi2`: ABI-Repinning inkl. tatsächlich angewandter `use_camera_wb`;
  `abi3`: `RawMetadata.orientation` trägt die echte EXIF-Orientation statt
  des dcraw-flip-Rohwerts, REVIEW-RAW-FLIP-1). Alte Caches und persistierte
  Artefakte veralten damit sichtbar statt stillschweigend weiterverwendet zu
  werden.

## Optionale Stufen und Adjustment-Semantik

Alle Stufen sind optional, sofern das Rezept sie nicht aktiviert. Die
verbindliche Reihenfolge für aktivierte Entwicklungsfunktionen lautet:

```text
Source-Actions
  -> Auto-WB / Auto-Tone
  -> Preset-Werte
  -> lokale Masken und Masken-Adjustments
  -> Match Total Exposure
```

Im Raster-MVP sind `exposure` endliche Werte im Bereich `-10..=10` EV und
`contrast`, `highlights` sowie `shadows` endliche Werte im Bereich `-1..=1`.
Ungültige Werte und unbekannte Adjustment-Keys werden mit einem Fehler
abgelehnt; sie werden nicht still geclippt oder ignoriert.

Die globalen Raster-Adjustments arbeiten auf jedem RGB-Kanal als `x` in
`0..=1`; Alpha bleibt unverändert. Nach Exposure und Contrast werden die
Stufen in Rezeptreihenfolge angewendet. Sind beide Stufen vorhanden, wird
`shadows` zuerst und `highlights` danach ausgeführt:

```text
shadow_weight = ((0.5 - x) / 0.5).max(0)^2
x' = clamp(x + shadows * shadow_weight * 0.25)

highlight_weight = ((x - 0.5) / 0.5).max(0)^2
x' = clamp(x + highlights * highlight_weight * 0.25)
```

Dabei bezeichnet `x` bei der zweiten Formel den Wert nach der Shadows-Stufe,
falls diese aktiv ist. Dies ist bewusst eine einfache deterministische
Raster-MVP-Heuristik und keine finale RAW-/Farbmanagement-Semantik.

Source-Actions wie nicht-destruktive Staubentfernung und spätere KI-
Teil-Ersetzung werden als Rezeptoperationen gespeichert. Sie wirken nach
Decode/Demosaic und vor Auto-Analyse. Das Original bleibt unverändert.

Auto-WB, Auto-Tone und Auto-Exposure speichern ihr Ergebnis zusammen mit einem
Analysefingerprint. Sie werden nur manuell, durch ein Preset oder bei
ungültigem Fingerprint neu berechnet.

### Source-Actions und Masken im Raster-MVP (F-042)

**Source-Actions:** Eine Source-Action ist im Raster-MVP ein
kontext-übergebenes Artefakt
`{ region: MaskPlane (u16, 0..=u16::MAX), replacement: ImageFrame (RGBA8) }`,
wobei `region` und `replacement` identische Dimensionen besitzen müssen
(andernfalls wird die Anwendung mit einem Renderfehler abgelehnt, kein stiller
Fallback). Die Anwendung erfolgt nach Decode und VOR Auto-Analyse/
Adjustments: `out = replacement` für Pixel mit `region >= 32768` (Schwellwert
50 %), sonst bleibt die Quelle erhalten. Alpha wird bei ersetzten Pixeln aus
`replacement` übernommen, sonst aus der Quelle. Keine Artefakte bedeuten
Identität. **F-042-N1 (umgesetzt):** Persistenz als Rezeptoperation ist implementiert:
das additive Schema-Feld `source_actions` im Pre-MVP-Muster (leere
Default-Liste, keine Migration nötig), das zdata-Artefaktformat für
Repair-Regionen (u16-Region + RGBA8-Ersatzbild im selben `.lumina.zdata`-Bundle,
getrennt über einen `kind`-Diskriminator bei unverändertem Container-`VERSION`)
und der CLI-Command `dust-removal` (Staubentfernung). Die CLI löst die
Rezept-Aktionen beim Rendern/Entwickeln aus dem Bundle auf und reicht sie an
`render_frame`; fehlende oder checksummenabweichende Artefakte brechen den
Render hart ab (kein stiller Fallback).

**Masken-Stufe:** Die Stufe wertet die aktiven `mask_layers` der gewählten
virtuellen Kopie aus: `MaskGraph`-Evaluierung pro Layer (inklusive der
Graph-Operationen Union/Intersect/Subtract/Invert), Artefakt-Quelle je
`MaskDefinition.status` nur bei `Valid`, sonst Meldung. Gültige Ebenen werden
auf die aktuellen Framedimensionen bilinear resampelt; die Koordinaten-
ausrichtung zwischen Maske und Frame ist eine dokumentierte Grenze
(`geometry_context` wird noch nicht zur Ausrichtung genutzt). Die
invert/feather/blur/density-Pixel-Modulation ist mit F-049 umgesetzt:
`MaskLayer` trägt `inverted`/`feather`/`blur`/`density`, und
`modulate_mask_plane` (`crates/lumina-core/src/mask_modulation.rs`) wendet sie in
`evaluate_layer` (nach Resample, vor Rückgabe) in der Reihenfolge
invert → feather → blur → density an. F-042 liefert die effektiven Ebenen im
Render-Ergebnis. **MaskPolicy:** `Strict`
(fehlendes/ungültiges Artefakt → Renderfehler) vs. `Warn` (Layer wird
übersprungen, Status im Ergebnis, Render läuft weiter — entspricht der
Konfliktmatrix „Export trotzdem erlauben").

**Einstiegspunkt:** Ein gemeinsamer Render-Einstiegspunkt in lumina-core
(`render_frame`) führt die Reihenfolge
`SourceActions → Adjustments (WB-Kontext VOR Tonwerten, inkl. Geometrie/Crop)
→ Masks → Output` aus. Auto-Tone-Berechnung und Match Total Exposure bleiben
Rezept-/Aufrufer-Orchestrierung (Fingerprint-Persistenz, F-041); der
Einstiegspunkt wendet das (ggf. auto-getonte) Rezept an. GUI und CLI verwenden
denselben Einstiegspunkt (SOLL: „GUI und CLI verwenden dieselbe
Renderpipeline").

**Status (F-042):** Implementiert sind der gemeinsame Einstiegspunkt, der
Source-Action-Mechanismus (Kontext-Artefakt, Kompositing mit 50 %-Schwellwert),
die Masken-Evaluierung und -Validierung mit `MaskPolicy`, die
CLI-/GUI-Verdrahtung (zdata-Planes, Warnungen) sowie F-042-N1 (Persistenz als
Rezeptoperation `source_actions`, zdata-Artefaktformat für Repair-Regionen und
CLI-Command `dust-removal`; die CLI löst die Aktionen beim Rendern aus dem
Bundle auf). Umgesetzt ist F-049 (Pixel-Modulation invert/feather/blur/density);
offen bleibt die Geometrie-Ausrichtung von Masken. Der Matching-Messbereich nach Crop/Masken
ist mit F-041 umgesetzt (siehe „Exposure Matching").

**Restgrenze (ehrlich):** Die GUI reicht Source-Actions beim Rendern bisher
noch nicht aus dem `.lumina.zdata`-Bundle auf (sie liefert vorerst eine leere
Liste); CLI und Renderpfad sind vollständig verdrahtet. Repair-Regionen werden
im MVP 1:1 in Quellauflösung angewandt (keine Resampling-Semantik in F-042-N1);
die Geometrie-Ausrichtung von Masken bleibt wie in F-042 dokumentiert offen.

**Status (F-048 / F-051):** Über der bisherigen zdata-Tile-Auswertung liegt
nun die intelligente Masken-Ladeentscheidung (`lumina-core::mask_loader`):
`resolve_mask_planes` prüft für jede von der aktiven Kopie erreichbare
Quell-Maske, ob ein **bestätigbar gültiges** persistiertes Artefakt vorliegt
(Status `Valid`, Artefaktreferenz + geladene zdata-Ebene vorhanden,
`source_fingerprint.content_hash`, `decode_context` und Modellidentität
stimmen) — dann wird es ohne Re-Inferenz geladen. Ist es fehlend, veraltet
(Quelle/Modell geändert) oder wird `--update-masks`/Refresh verlangt, erfolgt
die Re-Inferenz über das injizierte `MaskInference`-Trait (StubBackend/BiRefNet).
Eine Stale-Erkennung ist damit deterministisch und reproduzierbar; nie wird
stillschweigend eine veraltete Maske serviert (kann Gültigkeit nicht bestätigt
werden → gilt als fehlend). F-051: Ist kein Modell verfügbar, wird eine
vorhandene (ggf. veraltete) Maske aus dem Cache genutzt und mit Warnung
ausgegeben (`model_unavailable`); fehlt auch der Cache, ist dies ein harter
Fehler (kein stiller Fallback). Die Entscheidung liegt vollständig in
`lumina-core` und ist über `Option<&dyn MaskInference>` von `lumina-onnx`
entkoppelt; die CLI reicht Warnungen/Fehler an `stderr`/`mask_warnings` durch.
Offen bleiben die Persistenz der Re-Inferenz-Ergebnisse ins zdata-Bundle
(siehe F-082) und die GUI-Capability-Anzeige.

**Status (F-085, behaviorale Tests):** Behaviorale Tests decken die
Wechselwirkung von Source-Actions mit Auto-WB, Auto-Tone und Exposure Matching
ab: Die Reihenfolge SourceActions → Adjustments ist über differenzielle
Ausgaben belegt (ersetztes vs. nicht ersetztes Pixel unter WB; Auto-Tone- und
Matching-Messung auf dem Post-Action-Frame), ebenso die
Schwellwert-Grenzfälle (32768/32767, 0, u16::MAX), Nicht-Destruktion von Frame
und Artefakten, Determinismus und History-Reproduzierbarkeit (ein
Rezept-Snapshot rendert byte-identisch erneut) sowie das CLI-Zusammenspiel aus
History-Eintrag und gültiger Maske mit `--match-total-exposure`. Die
CLI-Durchreichung von Source-Actions ist mit F-042-N1 geschlossen: `process`
sowie `render`/`export` lösen `recipe.source_actions` beim Rendern aus dem
`.lumina.zdata`-Bundle auf und reichen sie an `render_frame`.

## Bearbeitungsregler

Dieser Abschnitt erweitert die Rezeptsemantik normativ. Alle Felder liegen in
der jeweiligen virtuellen Kopie unter `recipe` und werden mit
`recipe_schema_version` sowie der unabhängigen `pipeline_version` validiert.
Die MVP-Version dieser Regler ist `1`; unbekannte oder außerhalb des
angegebenen Bereichs liegende Werte werden abgelehnt, nicht still geclippt.
Alle Berechnungen liefern weiterhin `Rgba8Srgb`; Zwischenwerte dürfen intern
höhere Genauigkeit verwenden und werden erst am Ende der jeweiligen Operation
auf `0..=1` begrenzt.

Die top-level-Reihenfolge bleibt `Decode → SourceActions → AutoAnalysis →
Adjustments → Masks → Crop → Output`. Die in diesem Abschnitt genannten
Unterstufen innerhalb von `Adjustments` beziehungsweise `Crop` sind verbindlich
und ändern dieses Format-Tupel nicht.

**Schema-Migration (Pre-MVP):** Das Upgrade von `recipe_schema_version` 1 auf 2
ist erforderlich, sobald verschachtelte Adjustment-Felder (`curves`, `hsl`,
`color_grading`, `presence`, `sharpening`, `noise_reduction`) oder neue
Top-Level-Keys (`geometry`, `lens_correction`, `perspective`, `effects`)
verwendet werden. **Produktentscheidung (2026-08-17, präzisiert):** Bis zum MVP
ist das Schema bewusst **nicht abwärtskompatibel** — Altdateien müssen nicht
lesbar bleiben. Die Migrations-Maschinerie bleibt dauerhaft erhalten; die
v1→v2-Migration samt Tests (aus F-089/F-090) bleibt als Muster. **Pre-MVP gibt
es keinen Zwang, für jede Migration einen eigenen Test zu schreiben** — die
Regel „Tests für jede Migration" gilt ab dem MVP zusammen mit der vollen
Migrationsstrategie (verzögert mit Bestätigung, `.bak`-Backup, expliziter
Aufruf).

### F-089 Gradationskurve

**Ziel und Definition:** `recipe.adjustments.curves` enthält
`version`, `master` und `channels`. Jede Kurve ist eine Liste
`points: [{input, output}]` mit mindestens zwei und höchstens 32 Punkten.
`input` und `output` sind endliche `f32`-Werte in `0..=1`; Punkte müssen nach
`input` streng aufsteigend sein, Endpunkte `(0,0)` und `(1,1)` werden bei
fehlenden Endpunkten nicht implizit ergänzt (die Validierung verlangt sie).
`master` wirkt auf die Luminanz-/RGB-Kurve, `channels` enthält die getrennten
Kurven `red`, `green` und `blue`; nicht gesetzte Kanäle sind Identität.

Zwischen Stützpunkten wird eine monotone kubische Hermite-Interpolation mit
begrenzt berechneten Tangenten verwendet. Sie ist gegenüber Catmull-Rom
bevorzugt, weil sie bei monotonen Stützpunkten kein Überschwingen erzeugt.
Ausgaben werden nach der Interpolation auf `0..=1` geclippt. Auto-Tone wird
vor der Kurve berechnet und angewandt; die Kurve ist danach die letzte globale
Tonwertoperation (vor HSL/Color-Grading nur gemäß den nachfolgenden
Unterstufen). Die Kurve wird auf sRGB-codierten Werten angewandt, nicht auf
linearisierten ProPhoto-Werten.

**Platzierung, Cache und Abnahme:** In `Adjustments`, nach Auto-WB,
Auto-Tone/Preset und globalem Tonwert, vor lokalen Masken. Die Kurvenstruktur,
ihre Version und der wirksame Inhalt müssen im `recipe_hash` enthalten sein;
eine Änderung invalidiert ab `adjustments` alle abhängigen Preview-/Export-
Einträge, nicht Decode oder AI-Masken. Abnahme: JSON-Roundtrip, monotone
Interpolation ohne Overshoot, Kanaltrennung, Endpunktvalidierung, Clipping
und Cache-Miss werden getestet. MVP-Grenze: keine
lineare-Farbraum-Variante; spätere Pipelineversionen dürfen diese nur migriert
einführen. Abhängigkeiten: F-029, F-031, F-036, F-039 und F-041.

**Parametrik vs. Punkte je Kanal (G-02, LRPAR-G02-COLOR):** Jeder der vier
Kanäle (`master`, `red`, `green`, `blue`) besitzt genau eine aktive
Darstellung — parametrisch ODER Punkte; die zuletzt gesetzte ersetzt die
andere (Last-Write-Wins je Kanal, kein implizites Mischen):

- *Parametrisch:* vier Regions-Deltas `shadows/darks/lights/highlights` je in
  `-1..=1`, persistiert als 4-Punkte-Kurve an den Basispositionen
  `0, 1/3, 2/3, 1` mit auf `0..=1` geclippten Ausgaben (`output =
  clamp(base + delta)`). Master parametrisch entspricht dem bisherigen
  Verhalten; Kanal-Parametrik nutzt dieselbe Abbildung je Kanal.
- *Punkte:* freie Stützpunkte (`2..=32`, streng aufsteigende `input`,
  Endpunkte `(0,0)`/`(1,1)` Pflicht) ersetzen die 4-Punkte-Liste des Kanals.
- Beide Darstellungen teilen Interpolation (monotone kubische Hermite,
  kein Overshoot bei monotonen Stützpunkten) und Clipping (`0..=1` nach
  Interpolation). Die Master-Anwendung (Luminanz-Verhältnis) und die
  Kanal-Anwendung (direkt je Kanal) bleiben unverändert; Reihenfolge je Pixel:
  erst Kanal-Punkt/Parametrik-Kurve, dann Master-Skalierung.
- Bekannte MVP-Grenze (unverändert): geclippte Ausgaben sind nicht darstellbar
  (z. B. positives `shadows`-Delta hebt `(0,0)` an und verletzt die
  Endpunktpflicht) — Setzen wird laut verweigert bzw. sichtbar gemeldet,
  nie still normalisiert.

### F-090 HSL/Farbmischer

**Ziel:** Selektive Farbkorrektur ohne Änderung der übrigen Farbbereiche.
`recipe.adjustments.hsl` enthält `version` und acht Kanalobjekte mit den
Feldern `hue`, `saturation`, `luminance`. Die Kanäle heißen normativ `red`,
`orange`, `yellow`, `green`, `cyan`, `blue`, `violet`, `magenta` (Zentren
0°, 30°, 60°, 120°, 180°, 240°, 270°, 300°); `violet` und `magenta` sind
also zwei getrennte, benachbarte Kanäle, keine doppelte Magenta-Kategorie.
`hue` und `saturation` liegen in `-1..=1` (`hue` entspricht einer relativen
Drehung von höchstens 30°), `luminance` in `-1..=1`.

Die MVP-Transformation erfolgt im **sRGB-codierten** RGB-Raum nach
Normalisierung, mit HSL-Konvertierung pro Pixel. Jeder Kanal verwendet eine
zyklische, stückweise lineare Dreiecksgewichtung über die Hue-Abstände zum
eigenen Zentrum und den beiden Nachbarzentren; die Beiträge werden normiert.
Sättigung und Luminanz werden gewichtet addiert, Hue wird gewichtet gedreht,
danach wird auf `0..=1` geclippt. Das ist bewusst keine lineare- oder
ProPhoto-Farbverarbeitung.

Die Stufe liegt in `Adjustments` nach globalem Tonwert und vor Color Grading,
Masken und Crop. `recipe_hash` enthält alle acht Objekte und `version`;
Änderungen invalidieren ab dieser Unterstufe. Abnahme: Kanalzentren und
Nachbarübergänge, zyklische Hue-Grenzen, Wertevalidierung, neutrale Identität
und Cache-Invalidierung. MVP-Grenzen: keine selektive Masken-HSL-Matrix und
keine lineare HSL-Alternative. Abhängigkeiten: F-031, F-036, F-039.

### F-090b Point Color (G-02, LRPAR-G02-COLOR)

**Ziel:** Gezielte Farbauswahl mit frei wählbarem Zentrum (statt der acht
festen HSL-Zentren) plus Anpassung — deterministisch, rezept-persistiert.
`recipe.adjustments.point_color` enthält `version` und `entries` (Liste,
`0..=8` Einträge). Jeder Eintrag trägt:

- `id` (stabile, innerhalb des Rezepts eindeutige Zeichenkette, z. B.
  `pc-1`; Vergabe: laufende Nummer über dem Rezeptzustand, dokumentiert,
  nie positionsbasiert),
- Auswahl: `hue_center` in `0..=360`, `hue_range` (halbe Flankenbreite der
  Dreiecksgewichtung) in `0..=180`,
- Anpassung: `hue_shift`, `saturation_shift`, `luminance_shift` je in
  `-1..=1` (`hue_shift` entspricht einer Drehung von höchstens ±30°).

**Render-Semantik (sRGB-codiertes RGB, HSL-Konvertierung pro Pixel wie F-090):**
Gewicht `w` = zyklische Dreiecksfunktion des Hue-Abstands zu `hue_center`
(`1` im Zentrum, linear fallend auf `0` bei Abstand `>= hue_range`;
`hue_range == 0` wirkt nur auf exakt `hue_center`). Anwendung gewichtet:
`hue' = hue + hue_shift * 30° * w`, `sat' = clamp(sat + saturation_shift *
w)`, `lum' = clamp(lum + luminance_shift * w)`, danach zurück nach RGB und
Clipping auf `0..=1`. Mehrere Einträge wirken sequentiell in Listenreihenfolge.
Alle-Null-Shifts sind Identität (Pixel bleiben bis auf HSL-Rundung unverändert).

Die Stufe liegt in `Adjustments` nach HSL (F-090) und vor Vibrance/Saturation
(F-092). `version`, alle Einträge und die Listenreihenfolge gehen in den
`recipe_hash`; Änderungen invalidieren ab dieser Unterstufe. Abnahme:
JSON-Roundtrip (inkl. stabiler IDs), Wertevalidierung (laut, kein Clipping),
zyklische Hue-Grenzen (`359° ↔ 0°`), Identität bei Null-Shifts, Determinismus
(zwei Läufe byte-identisch), Selektivität (ferne Farben unverändert) und
Cache-Invalidierung. MVP-Grenzen: nur Hue-Dreiecksgewichtung (kein
Sättigungs-/Luminanz-Gating der Auswahl, keine Masken-Kopplung), sRGB-HSL
statt linearem/perzeptuellem Modell. Abhängigkeiten: F-031, F-036, F-090.

### F-091 Color Grading

**Ziel:** Unabhängige Tönung von Schatten, Mitteltönen und Lichtern mit einem
kontrollierbaren Übergang zwischen den Bereichen.
`recipe.adjustments.color_grading` enthält `version`, die Bereiche `shadows`,
`midtones`, `highlights` mit `hue_degrees` in `0..360` und `saturation` in
`0..=1`, sowie `balance` in `-1..=1`. Der Farbton ist ein Winkel (zyklisch,
0° = Rot) im sRGB-HSL-Farbmodell; die Tönungsfarbe wird in RGB umgerechnet
und mit der jeweiligen Sättigung gemischt.

Die Bereichsgewichte sind weiche, überlappende Funktionen der sRGB-
Luminanz: Schatten `smoothstep(0.65,0.0, L)`, Lichter
`smoothstep(0.35,1.0,L)`, Mitteltöne `1 - shadow - highlight`, jeweils auf
Summe 1 normiert. `balance` verschiebt die beiden Übergänge symmetrisch
zwischen Schatten und Lichtern. Color Grading folgt HSL und globaler Kurve,
liegt vor Masken und wird vor Crop ausgeführt. Damit ist die dokumentierte
Zusammenwirkung: Auto-Tone → Kurve → HSL → Point Color → Vibrance/Saturation
→ Color Grading.

`recipe_hash` berücksichtigt alle Werte und `version`; Änderungen invalidieren
Preview/Export ab Color Grading, nicht Decode/Maskenartefakte. Abnahme:
zyklische Hue-Werte, weiche Übergänge, Balance-Richtung, Identität bei
Sättigung 0 und reproduzierbarer Cache-Miss. MVP-Grenzen: sRGB-HSL statt
linearem oder perceptuellem Farbmodell, keine getrennten Blend-Modi (nur der
`blending`-Breitenparameter unten). Abhängigkeiten: F-031, F-036, F-039.

**Feinschliff Schatten/Mitten/Lichter (G-02, LRPAR-G02-COLOR):** Jeder Bereich
(`shadows`, `midtones`, `highlights`) trägt zusätzlich `luminance` in
`-1..=1` (additiver Helligkeitsversatz der Tönung, `0` = Identität); global
kommt `blending` in `0..=1` hinzu (Überlappungsweite der Bereichsgewichte,
`0.5` = bisheriges Verhalten exakt):

- `shadow_edge = 0.65 - balance * 0.15 + (blending - 0.5) * 0.2`,
  `highlight_edge = 0.35 - balance * 0.15 - (blending - 0.5) * 0.2`
  (`blending == 0.5` reproduziert die bisherigen Kanten bit-identisch;
  höheres `blending` verbreitert die Mitteltöne).
- Tönungsmischung wie bisher (kanalweise `x' = x + (tint - x) * weight *
  saturation`); danach Luminanzversatz `lum_w = Σ weight_i * luminance_i` in
  HSL: `L' = clamp(L + lum_w)`, zurück nach RGB, Clipping `0..=1`. Sind alle
  drei `luminance`-Werte `0`, entfällt der HSL-Rundlauf (bit-identisch zum
  bisherigen Pfad).
- Fehlende Felder in Altdateien bedeuten `luminance = 0`, `blending = 0.5`
  (additiv, keine Migration; identisches Renderverhalten).
- Abnahme: `luminance`-Identität bei 0, sichtbare Aufhellung/Abdunklung je
  Bereich bei ±1, `blending`-Richtung (Mitteltöne breiter/schmaler),
  Legacy-Identität (`blending = 0.5`, `luminance = 0` rendert byte-identisch
  zum Vor-Feinschliff-Pfad), Wertevalidierung laut.

### F-092 Dynamik und Sättigung

**Ziel:** Schwache Farben gezielt beleben und eine einfache globale Sättigung
anbieten, ohne bereits gesättigte Farben unnötig zu übersteuern.
`recipe.adjustments.vibrance` und `saturation` sind endliche Werte in
`-1..=1`. `saturation` skaliert die HSL-Sättigung linear um den Faktor
`1 + saturation` (sRGB-codiertes RGB, Clipping danach). `vibrance` erhöht oder
senkt sie gewichtet: Schutzgewicht = Produkt aus geringer vorhandener
Sättigung und einem Hautfarbenschutz (Hue-Bereich ungefähr 15°..55° mit
weichen Rändern); bereits gesättigte Farben und geschützte Hauttöne werden
höchstens proportional schwach verändert. Negative Vibrance wirkt ebenfalls
gewichtet, nicht als zweiter linearer Sättigungsregler.

Die Unterstufenreihenfolge ist `vibrance → saturation`, nach HSL und Point
Color (F-090b), vor Color
Grading, Masken und Crop. Werte, Schutzfunktion und `version` gehen in den
`recipe_hash`; Änderungen invalidieren ab Adjustments. Abnahme: Identität bei
0, Begrenzung, Schutz gesättigter Farben/Hauttöne und Cache-Invalidierung.
MVP-Grenze: heuristischer HSL-Hautschutz; kein kameramodell- oder
hauttonadaptives Lernen. Abhängigkeiten: F-031, F-036, F-090.

### F-093 Zuschneiden und Drehen

**Ziel:** Einen nicht-destruktiven sichtbaren Bildausschnitt samt Drehung und
Spiegelung reproduzierbar festlegen.
`recipe.geometry` enthält `version`, `crop` (`mode: "aspect"|"free"`, bei
`aspect` `preset` aus `original`, `1:1`, `4:5`, `5:4`, `3:2`, `2:3`, `4:3`,
`3:4`, `16:9`, `9:16`, bei `free` `x`, `y`, `width`, `height` in normierten
Quellkoordinaten `0..=1`), `rotation_degrees` in `-180..=180` und
`mirror_horizontal`/`mirror_vertical` als bool. Freie Rechtecke müssen eine
positive Fläche besitzen; Presets werden auf die Bildgrenzen eingepasst.

In der Crop-Stufe gilt intern: Objektivkorrektur → Perspektive → Crop →
Rotation → Spiegelung. Rotation erfolgt um das Crop-Zentrum; Ausgabemaße und
Seitenverhältnis werden deterministisch aus Transform und Outputvorgabe
berechnet. Die finale F-041-Messdomäne liegt nach Crop und Geometrie, vor
Outputprofil/Export. Geometrieparameter, `version` und resultierende
`output_dimensions` gehören zum RenderKey: Decode, Auto-Analyse und Masken
bleiben bei reiner Geometrieänderung cachebar, Preview/Export werden
invalidiert. Abnahme: Presets, normiertes Rechteck, Drehung/Spiegelung,
RenderKey-Trennung und F-041-Messbereich. Default-Crop (CROP-MAXRECT-1,
User-Entscheid 2026-09-12, ersetzt die alte „kein Auto-Crop"-Grenze): ohne
expliziten Crop und bei aktiver Lens-/Perspektiv-Korrektur (inkl. Corrector)
ist der Default das flächenmaximale achsparallele Inhaltsrechteck
(Maximum-Rectangle beliebigen Seitenverhältnisses, Constrain-to-Image,
Alpha > 0); ohne Korrektur Identität; expliziter Crop gewinnt immer;
degeneriert/leer → voller Frame (nie leerer Crop). Bewusste Grenze:
Nicht-90°-Rotation erzeugt weiterhin transparente Ecken (Default greift vor
Rotation). Abhängig von F-029, F-039, F-041, F-098 und F-099. GPU-Route
(2026-09-14): Lens/Perspective ohne expliziten Crop routen laut auf CPU
(Reason `geometry (default content crop)`, datenabhängiges MaxRect nicht
rezept-planbar); mit explizitem Crop GPU-fähig.

**Straighten + Aspect-Parität (G-06, LRPAR-G06-GEO, MVP/1.0):** Der
Straighten-Winkel (Gerade-Ausrichten, Lightroom „Angle") ist kein eigenes
Rezeptfeld, sondern ein dokumentierter Alias auf
`geometry.rotation_degrees` (`-180..=180`, Default `0`): GUI-„Straighten"-
Slider, CLI `--straighten` und `--set-rotation` committen dasselbe Feld
(identische Validierung, identisches Renderverhalten). Die Aspect-Presets
(`original`, `1:1`, `4:5`, `5:4`, `3:2`, `2:3`, `4:3`, `3:4`, `16:9`,
`9:16`) sind in GUI (Preset-Auswahl) und CLI (`--set-crop-aspect`)
vollständig verfügbar; freie Rechtecke (`--set-crop-free x,y,w,h`, GUI-
Zahlenfelder) werden laut validiert (positive Fläche, `0..=1`-Grenzen, kein
stilles Clipping). Das aktive Crop-Rechteck wird als Overlay-Rechteck in der
Vorschau gezeichnet (reiner Session-Display-State, Werte rezept-persistiert).
Jeder Geometrie-Schritt (Crop/Straighten/Aspect/Spiegelung) erzeugt genau
einen sichtbaren History-Eintrag (CLI: ein Eintrag pro mutierendem Aufruf;
GUI: ein Eintrag pro gespeichertem Commit, Slider-Drags koaleszieren).

### F-094 Präsenz

**Ziel:** Feine und mittlere lokale Bildstrukturen sowie atmosphärischen Dunst
mit getrennten, kontrollierbaren Reglern beeinflussen.
`recipe.adjustments.presence` enthält `version`, `texture`, `clarity` und
`dehaze`, jeweils endlich in `-1..=1` (UI kann -100..+100 anzeigen). Texture
ist lokaler Kontrast eines kleinen, skalenabhängigen Radius (feine Strukturen,
MVP Radius 1..3 px); Clarity ist lokaler Mittenkontrast mit großem Radius
(MVP 8..32 px). Beide verwenden eine deterministische Difference-of-Gaussians-
Heuristik. Dehaze verwendet ein dunkelkanal-basiertes Atmosphärenmodell mit
lokaler Transmission, begrenzt auf `0.05..=1`; negative Werte werden als
umgekehrter, abgeschwächter Effekt definiert.

Reihenfolge in `Adjustments`: Exposure → Contrast → Highlights/Shadows →
Presence (Texture → Clarity → Dehaze) → Kurve/HSL gemäß den dort definierten
Unterstufen; Alpha bleibt unverändert. `version` und alle Werte stehen im
`recipe_hash`, Änderungen invalidieren ab Adjustments. Abnahme: Wertefehler,
Radiusverhalten, deterministisches Clipping, Kontrast-Reihenfolge und Cache.
MVP-Grenzen: Heuristiken statt Lightroom-identischer RAW-/Dehaze-Semantik,
kein GPU-spezifischer Fallback. Abhängigkeiten: F-031, F-036, F-039, F-041.

### F-095 Schärfen

**Ziel:** Details reproduzierbar schärfen und homogene Flächen über eine
Luminanz-Kantenmaske vor Halos schützen.
`recipe.adjustments.sharpening` enthält `version`, `amount` `0..=3`, `radius`
`0.1..=10` (in Quellpixeln), `detail` `0..=1` und `masking` `0..=1`.
MVP ist eine Unsharp-Mask: Luminanz, Gauß-Blur mit Radius, Differenzsignal
und `amount`; `detail` mischt feinere und gröbere Differenzen. `masking` ist
eine aus Luminanzgradienten erzeugte Kantenmaske: hohe Maskierung unterdrückt
Flächen, Kanten bleiben wirksam. Radius wird bei Vorschau/Export proportional
zur effektiven Bildskalierung umgerechnet.

Schärfen liegt am Ende von `Adjustments`, nach Rauschreduzierung und vor
Masks/Crop/Output. Werte und Version sind im `recipe_hash`; die effektive
Skalierung wird aus Quellauflösung und Ausgabedimensionen abgeleitet und ist
im render-scopeigenen Digest enthalten (Ausgabedimensionen sind Teil des
RenderKeys). Änderungen invalidieren ab Schärfen. Abnahme: Unsharp-Verhalten,
Kantenmaske, Skalierung, Clipping und Cache. MVP-Grenzen: keine
Ausgabe-Schärfung mit eigenem Profil und kein lokales Masken-Schärfen.
Abhängigkeiten: F-031, F-036, F-096.

### F-096 Rauschreduzierung

**Ziel:** Manuelles Luminanz- und Farbrauschen vor dem Schärfen vermindern,
ohne ein nicht reproduzierbares KI-Modell vorauszusetzen.
`recipe.adjustments.noise_reduction` enthält `version`, `luminance` und
`color`, jeweils `0..=1`; 0 ist Identität. Das MVP verwendet einen
deterministischen, kantenbewussten lokalen Mittelwert (5x5-Fenster): Luminanz
wird nach Ähnlichkeit der Luminanz gewichtet geglättet, Farbrauschen wird im
Chromakanal stärker geglättet. Das ist bewusst ein einfaches, deterministisches
CPU-Modell statt eines nicht reproduzierbaren KI-Verfahrens.

Rauschreduzierung liegt in `Adjustments` vor Schärfen, Masken und Crop.
Felder/Version gehen in den `recipe_hash`; Änderungen invalidieren ab dieser
Unterstufe. Abnahme: 0-Identität, Kantenbewahrung, Kanaltrennung,
Determinismus und Schärfen-Reihenfolge. KI-Denoise ist eine optionale,
additive spätere Erweiterung und wird in F-096a präzisiert (sie ersetzt die
manuelle Stufe nie). Abhängigkeiten: F-031, F-036.

### F-096a KI-Denoise (DenoiseAI, Release 2.0)

**Status:** Kern-Slice umgesetzt (LRPAR-G14-DENOISE-IMPL-20, 2026-09-16).
ONNX-Verdrahtung, CLI/GUI-Anbindung und Perf-Budgets folgen als getrennte
Slices. Entscheid: `feature/decisions/LRPAR-G14-DENOISE-20.md`.

**Ziel:** Optionales, austauschbares KI-Denoise als additive Stufe, die das
manuelle F-096-NR **nie ersetzt**, sondern davor läuft; `None`/`enabled:false`/
`strength:0` sind Identität (MVP-Rezepte rendern byte-identisch, keine
Migration).

**Rezept/Version:** `recipe.adjustments.denoise_ai` (`version` 1,
`enabled`, `model { name, version, model_hash }`, `input_spec_digest`,
`strength` `0..=1`, `preserve_detail` `0..=1`, optionaler
`artifact`-Verweis `kind = "denoise_rgb"`). Additives Schema-v2-Feld, `None`
ist Identität; `version == 1`, endliche `0..=1`-Werte und vollständige
Modellidentität werden laut validiert (kein Clipping, keine Defaults).
Der Denoise-`model_hash` ist bis zur F-078-Freigabe `pending-integration`.

**Reihenfolge (normativ, in `Adjustments`):**
`… → Farbkorrekturen → DenoiseAI (optional) → NoiseReduction (F-096, manuell)
→ Sharpening (F-095) → Red-Eye → Effects → Masks → Crop/Output`.
DenoiseAI arbeitet wie die MVP-Pipeline sRGB-codiert auf RGBA8; Alpha bleibt
unverändert. Ist die Stufe aktiv, aber nicht verfügbar/veraltet/fehlend/
korrupt, greift **sichtbar** das manuelle F-096-NR (bzw. Identität) — nie
stilles Ummappen.

**Blend (deterministisch, CPU + GPU-portabel):**
`w = strength * (1 - preserve_detail * detail)` mit
`detail = clamp(|Y - box_mean_3x3(Y)| / 32, 0, 1)` aus der Quell-Luminanz
(Rec.709); pro Kanal `out = round(clamp(src + w * (artifact - src), 0, 255))`,
`strength == 0` ist strenge Identität (keine Inferenz, kein Modell nötig).
Gleiche Eingaben → byte-identisches Ergebnis.

**Getilte Inferenz (Nahtlosigkeit):** Der Kern besitzt die kanonische
Kachel-Assemblierung: überlappende RGB8-Kacheln werden mit
`weight = (Abstand zur nächsten Kachelkante + 1)` pro Kachel akkumuliert und
per Pixel `sum(weight * color) / sum(weight)` normalisiert. Das ist
deterministisch und nahtlos (konstante Kacheln ⇒ konstante Fläche ohne Naht);
Kachelgröße/Overlap/Blend-Verfahren sind Teil des `input_spec_digest`, eine
Änderung invalidiert persistierte Artefakte sichtbar. Eine unvollständige
Abdeckung ist ein lauter Fehler (nie stiller schwarzer Rest).

**Persistenz/Identität:** Das entrauschte RGB liegt als versionierter
`.lumina.zdata`-Eintrag `kind = "denoise_rgb"` (RGB8, relativer Pfad, BLAKE3
über den kanonischen unkomprimierten Strom, atomar unter `.zdata.lock`,
eager Prüfsumme beim Laden). `strength`, `preserve_detail`, Modell,
`input_spec_digest` und der Artefakt-Hash fließen in `recipe_hash`/Render-Key;
eine Änderung invalidiert ab der Denoise-Unterstufe (nicht Decode/AI-Masken).
Gültigkeit verlangt Übereinstimmung von Quell-Hash, Decode-/Geometrie-Kontext,
Modellkontext (Name/Version/Hash), `input_spec_digest` und
Artefakt-Prüfsumme (§6 des Entscheids).

**Produzenten-Provenienz (Nachtrag 2026-09-16):** `DenoiseAi`/das
zdata-`DenoiseRgbArtifact` tragen selbst keine Produzenten-Herkunft. Die
`recorded`-Identität (Quell-Hash, Decode-Fingerprint, Modell-Name/-Version/-Hash,
`input_spec_digest`, Artefakt-Prüfsumme) wird daher additiv und
roundtrip-legal unter dem Schlüssel `producer_identity` in `DenoiseAi.extras`
(flatten) persistiert (`set_denoise_producer_provenance` /
`denoise_producer_provenance`); `resolve_denoise_status` liest **alle**
`recorded`-Felder und meldet jede Abweichung als `stale` — keine toten Felder,
kein stilles `ready`. Eine fehlende/kaputte Provenienz wird als nicht-`ready`
(sichtbar) behandelt.

**Statusmodell (§6, sichtbar, kein stiller Fallback):**
`unavailable` (Modell/`pending-integration`/kein Pfad), `stale`
(Quelle/Decode/Modell/Input-Spec geändert), `missing` (Artefakt fehlt),
`corrupt` (Prüfsumme/Dimension ungültig) und `ready`. Policy analog
`MaskPolicy`: `Strict` bricht laut ab, `Warn` protokolliert `warn!` und lässt
das manuelle F-096-NR als ausgewiesenen Fallback laufen. Eine
Neuberechnung ist nur explizit (Button/Flag), nie die einzige Option.

**Abnahme:** Schema-Anbindung/Roundtrip, `strength:0`/`enabled:false`/`None`
= Identität (byte-identisch), Determinismus/BLAKE3, Veraltung je
Identitätsfeld, Kachel-Nahtlosigkeit, PSNR-Gates (Golden-Images folgen im
CLI/GUI-Slice), `recipe_hash`/
Render-Key. **GPU:** die Stufe ist per Definition elementar (Punkt-/3×3-
Nachbarschaft, keine dynamische Dispatch), also voll GPU-tauglich; ein
WGSL-Pass ist noch nicht verdrahtet. Um kein CPU-only-stilles Verhalten
zuzulassen, ist die Routing-Gate-Erweiterung umgesetzt (2026-09-16): ein
**aktives** `denoise_ai` wird von `unsupported_gpu_stages` als
`denoise_ai (not GPU-wired)` gemeldet, `validate_gpu_recipe` validiert die
Stufe mit demselben `CoreError::Denoise { status: "invalid" }` wie die CPU, und
der GPU-Eingang routet aktive Rezepte auf die CPU, wo die Stufe unter
`DenoisePolicy::Strict` laut abbricht (kein stiller, pixelgleicher Render).
Identität/`enabled:false`/`strength:0` bleiben GPU-eligibel. Inventory-/
Parity-Tests decken beides ab. Abhängigkeiten: F-031, F-036, F-078, F-095,
F-096.

### G-14 Rote Augen (RedEye, Release 1.5)

**Ziel:** Rote Blitz-Pupillen in markierten Regionen deterministisch
entsättigen und abdunkeln, ohne übrige Bildteile zu verändern.
`recipe.adjustments.red_eye` enthält `version` (1) und `regions` (Liste,
`0..=32` Einträge). Jede Region trägt:

- `id` (stabile, innerhalb des Rezepts eindeutige nicht-leere Zeichenkette;
  Regionen werden nie über ihre Listenposition identifiziert),
- Pupillen-Zentrum `x`/`y` in normierten Quellkoordinaten `0..=1`
  (`x * Breite`, `y * Höhe` in Pixeln),
- `radius` in normierten Einheiten `0 < radius <= 1`
  (Pixelradius `radius * min(Breite, Höhe)`),
- `desaturate`/`darken` je `0..=1` (0 = keine Wirkung).

**Render-Semantik (sRGB-codiertes RGBA8, CPU, deterministisch):** Pro Pixel in
einer Region bestimmt die Rot-Dominanz
`redness = clamp((R - max(G, B)) / max(R, ε), 0, 1)` die Wirkstärke; graue
oder nicht-rote Pixel bleiben unverändert. Gewichtung innerhalb des Radius:
volle Stärke bis 75 % des Radius, danach linearer Abfall auf 0 am Rand.
Entsättigung zieht den Rotkanal in Richtung der Rec.709-Luminanz
(`R' = R + (L - R) * desaturate * Gewicht * redness`), Abdunklung skaliert
alle drei Kanäle (`c' = c * (1 - darken * Gewicht * redness)`); Alpha bleibt
unverändert, Ergebnis pro Kanal auf `0..=255` gerundet und geclippt. Leere
Regionenliste oder nur Null-Stärken sind Identität (kein Pixel verändert).

**Validierung (laut, kein Clipping):** `version == 1`, höchstens 32 Regionen,
nicht-leere eindeutige `id`s, endliche `x`/`y` in `0..=1`, endlicher Radius
in `(0, 1]`, endliche `desaturate`/`darken` in `0..=1` — jede Abweichung
(inkl. NaN) wird mit einem Fehler abgelehnt. Unbekannte Felder bleiben
erhalten (Roundtrip-Regel).

**Erkennung im 1.5-Scope (Entscheid 2026-09-16, LRPAR-G14-REDEYE-15):** Das
SOLL schließt eine automatische Pupillen-Erkennung für 1.5 bewusst aus.
„Erkennung" bedeutet in diesem Release daher **explizites Markieren**: CLI und
GUI stellen einen Region-Picker (normierte Klick-/Drag-Zentren + Radius)
bereit, der Regionen als `recipe.adjustments.red_eye` persistiert; die
Korrektur selbst ist die obige deterministische, modellfreie Formel. Eine
automatische Rot-Dominanz-Pupillenerkennung bleibt ausdrücklich Folgearbeit
(kein Modell, keine stille Vorbefüllung). Die Korrektur läuft auf GPU mit
CPU-Oracle-Parität (Stage 3, siehe GPU-Status); ungültige Regionen bleiben
laut CPU-geroutet.

**Automatische Pupillen-Erkennung (LRPAR-G14-REDEYE-AUTO-15, Release 2.0):**
Die Erkennung bleibt eine **ausdrückliche Aktion** und ist nie eine stille
Vorbefüllung. Die Heuristik ist deterministisch und modellfrei (kein ONNX,
kein Zufall, kein Seed) und arbeitet auf den dekodierten RGBA8-Quellpixeln vor
jeder Rezeptstufe:

1. Rot-Dominanz je Pixel exakt wie die Korrektur:
   `redness = clamp((R - max(G, B)) / max(R, ε), 0, 1)`. Ein Pixel gilt als
   rot, wenn `redness >= RED_EYE_DETECT_REDNESS_THRESHOLD = 0.5`.
2. Zusammenhängende Rot-Komponenten (4-Nachbarschaft, zeilenweise
   deterministische Scan-Reihenfolge) mit mindestens
   `RED_EYE_DETECT_MIN_PIXELS = 4` Pixeln. Komponenten, deren
   einschließender Radius (maximaler Abstand eines Komponentenpixels vom
   Schwerpunkt, normiert auf `min(Breite, Höhe)`) größer als
   `RED_EYE_DETECT_MAX_RADIUS = 0.1` ist, werden verworfen: das sind flächige
   rote Bildteile (Lippen, Kleidung), keine Pupillen.
3. Kandidat: Zentrum = Komponenten-Schwerpunkt (Pixelmitten), normiert auf
   `x * Breite`/`y * Höhe`; `radius = clamp(enclosing_radius_norm *
   RED_EYE_DETECT_RADIUS_MARGIN (1.25), > 0, <= 1)`; `confidence` = mittlere
   `redness` der Komponente in `0..=1`; Korrektur-Defaults
   `desaturate = 0.8`, `darken = 0.4`.
4. `id` ist inhaltsstabil: `auto-re-<8 hex BLAKE3 über x,y,radius>`
   (quantisiert auf `1e-6`, Kollisionen deterministisch mit Suffix). Gleiche
   Quelle und gleiche Pixel ergeben damit byte-identische Regionen.
5. Kandidaten werden nach `confidence` absteigend, dann `y`, dann `x`
   sortiert (deterministisch) und auf `RED_EYE_MAX_REGIONS = 32` begrenzt.
   Überschüssige Funde werden **laut gemeldet** (Feld `dropped`), nie still
   verworfen.

**Aktionsfläche (explizit):** CLI `lumina red-eye --detect` listet die
Kandidaten schreibfrei; `--detect-apply` persistiert sie als
`recipe.adjustments.red_eye` (verlangt `--detect`). Die GUI zeigt in der
Detail-Sektion „Detect pupils" (listet) und „Apply detected" (persistiert).
Ohne rote Pupillen liefert die Erkennung eine leere Liste und ändert den
vorhandenen Stand nicht. `--detect-apply` **ersetzt ausschließlich die
automatisch erzeugten Regionen** (id-Präfix `auto-re-`) durch den frischen
Fund und lässt manuell markierte Regionen unberührt; die Aktion ist damit
idempotent. Übersteigt die Vereinigung aus manuellen und erkannten Regionen
32, bricht die Aktion **laut** ab (kein stilles Kürzen). Eine Erweiterung des
`lumina regenerate`-Modulsatzes um `redeye` ist bewusst **nicht** Teil dieses
Entscheids: `regenerate` ist der Sammel-Default für veraltete/fehlende
*aktivierte* Artefakte, und eine Erkennung im Sammel-Default würde Regionen
ohne ausdrückliche Aktion vorbefüllen. Die Aktion folgt dem Muster
`upright --analyze` / `spot --detect-objects`/`--detect-apply`. Die Heuristik
läuft auf dekodierten Pixeln und ist kein Render-/GPU-Pfad; die GPU-Parität
der eigentlichen Korrektur (Stage 3) bleibt unverändert.

**Platzierung, Cache und Abnahme:** In `Adjustments` nach Schärfen (F-095)
und vor Effekten (F-097)/Masken/Crop; das Format-Tupel bleibt unverändert.
Feld, Version und alle Regionen gehen in den `recipe_hash` (BLAKE3 über das
serialisierte Rezept); Änderungen invalidieren ab dieser Unterstufe.
Abnahme: JSON-Roundtrip, Validierungsablehnung (Out-of-Range/NaN),
Identität bei leerer Liste/Null-Stärken, Lokalität (Pixel außerhalb von
Regionen und nicht-rote Pixel unverändert), Monotonie (größeres
`desaturate`/`darken` verstärkt oder erhält die Korrektur), Determinismus
(zwei Läufe byte-identisch), Alpha-Erhalt und Clipping. Erkennungs-Abnahme
(2.0): Determinismus (gleiche Quelle ⇒ byte-identische Regionen), Schwelle
(keine roten Pixel ⇒ keine Regionen), Kap-Handling > 32 laut+deterministisch,
Property (stärkere Rot-Dominanz ⇒ höhere Konfidenz), Golden (synthetische
Fixtures mit/ohne rote Pupillen), CLI-E2E und GUI-headless (Aktion vorhanden,
explizit, kein Auto-Lauf). MVP-Grenze 1.5: Regionen wurden nur explizit
markiert; 2.0 ergänzt die Erkennung ausschließlich als ausdrückliche Aktion.
Abhängigkeiten: F-031, F-036.

### F-097 Vignettierung und Körnung (niedrige Priorität)

**Ziel:** Eine reproduzierbare Randabdunklung und eine deterministische
prozedurale Körnung als abschließende Stilmittel ermöglichen.
`recipe.effects.vignette` enthält `version`, `amount` `-1..=1`, `midpoint`
`0..=1`, `roundness` `-1..=1` und `feather` `0..=1`; der Effekt ist radial und
wird vor Output angewandt. `recipe.effects.grain` enthält `version`, `amount`,
`size` und `roughness`, jeweils `0..=1`, und `seed` als `u64`. Körnung ist
prozedurales, kanalgekoppeltes Rauschen; der effektive Seed wird aus
`seed` und dem RenderKey deterministisch abgeleitet. Beide Effektobjekte und
Versionen stehen im RenderKey/`recipe_hash` und invalidieren Preview/Export.
Die Effekte laufen innerhalb der `Adjustments`-Stufe als letzte Unterstufe
(nach Schärfen, vor Masks und Crop); das Format-Tupel bleibt unverändert.
Abnahme: radiale Parameter, deterministische Wiederholung und Seed-Wechsel.
MVP-Grenze: niedrige Priorität, kein filmspezifisches Kornmodell. Abhängigkeit:
F-031, F-093.

**Implementierungsstatus (F-097, 2026-08-20):** Umgesetzt und unabhängig
verifiziert. `recipe.effects` (`Effects { vignette: Option<Vignette>,
grain: Option<Grain> }`) ist ein additives Schema-v2-Feld auf `EditRecipe`
(`lumina-sidecar`), serde-mäßig im ROOT (wie `geometry`) abgelegt, sodass es
automatisch in `recipe_hash`/`RenderKey` fließt. `Vignette` trägt
`version`, `amount` `-1..=1`, `midpoint` `0..=1`, `roundness` `-1..=1`,
`feather` `0..=1`; `Grain` trägt `version`, `amount`/`size`/`roughness`
`0..=1` und `seed: u64`. Beide werden in `validate_nested_adjustments`
(lumina-core) und `validate_adjustments` (lumina-sidecar) auf Wertebereiche
geprüft (Version 1, finite). In `apply_recipe_with_scale_and_white_balance`
werden sie als letzte Adjustment-Unterstufe NACH Schärfen und VOR `Ok(())`
angewandt (RGB, Alpha unberührt): `apply_vignette` (radial,
min/max-normalisiert, Center-Faktor 1.0, symmetrisch, `amount>0` dunkelt
Rand/`amount<0` hellt auf) und `apply_grain` (deterministisches,
kanalgekoppeltes Korn; effektiver Seed aus `seed` + Bilddimensionen via
`grain_hash`; `amount==0` streng identisch). 13 neue Tests (11 lumina-core,
2 lumina-sidecar) decken Identität, Vorzeichenverhalten, radiale Symmetrie,
Determinismus, Seed-Wechsel, Kanalkopplung, Stufenreihenfolge und
Validierungsablehnung ab.

### F-098 Objektivkorrekturen

**Ziel:** Typische Objektivfehler im MVP manuell und ohne externe Profildaten
geometrisch, tonal und kanalbezogen korrigieren.
`recipe.lens_correction` enthält `version`, `profile` (optional benannter,
eingebauter Presetname), `distortion_k1`, `distortion_k2`, `distortion_k3` in
`-1..=1`, `vignette_c0`, `vignette_c1`, `vignette_c2` in `-1..=1`, sowie
`ca_red` und `ca_blue` in `-0.05..=0.05`; Grün ist Referenz. Die Verzeichnung
ist ein normiertes radiales Polynom `r' = r*(1+k1*r²+k2*r⁴+k3*r⁶)`, die
Vignette ein radiales Polynom, CA eine relative R-/B-Kanal-Skalierung.

Die geometrische Korrektur liegt vor Perspektive und Crop; Vignette folgt der
Geometriekorrektur, CA wird kanalweise unmittelbar vor dem Crop-Resampling
angewandt, damit Farbsaumkorrektur nicht durch spätere Geometrie verstärkt
wird. Alle Felder und `profile` gehen in den RenderKey. Abnahme: Polynom,
Kanalreferenz, Preset-Roundtrip und gezielte Cache-Invalidierung.

**Lensfun-Vollausbau (G-06, LRPAR-G06-GEO, MVP/1.0, User-Entscheid
2026-09-03):** Zusätzlich zum manuellen Modell wird die Lensfun-Datenbank
(Kamera-/Objektiv-Profile, CC-BY-SA) zur automatischen Korrektur genutzt, wenn
ein passendes Profil für die aus den RAW-Metadaten (`camera_make`,
`camera_model`, ggf. Objektivname) ermittelte Kamera/Objektiv gefunden wird.
Lensfun liefert Geometrie-, Vignette- **und CA(TCA)-Korrektur** und ersetzt
das manuelle Modell, sofern ein Profil vorliegt (Priorität: Lensfun >
manuelle Koeffizienten > Identität); sonst greift das manuelle Modell
(graceful fallback, laut sichtbar über den Profilstatus).

- **EXIF-Profil-Erkennung:** Der Corrector wird aus `camera_make`/
  `camera_model` (Pflicht, endlich vorhanden) plus `focal_length`/`aperture`
  aufgebaut; `RawMetadata.lens` (EXIF-LensModel/Makernote) wird als
  Lensfun-Lens-Name durchgereicht, wenn vorhanden. **Strikte Erkennung
  (GUI-ROUTING-N6, 2026-09-17):** Kamera und ein **benannter** Objektivname
  müssen tatsächlich in der Datenbank existieren (Groß-/Kleinschreibung egal,
  Maker-Präfix optional) — kein `LF_SEARCH_LOOSE` mehr. Vorher hat die lose
  Suche Profile erfunden (`EOS R1` → `EOS R`, `RF200-800mm F6.3-9 IS USM` →
  `RF 24-240mm F4-6.3 IS USM`) und damit eine **falsche** Korrektur still auf
  echte Fotos angewendet. Ohne Treffer gilt strikt der manuelle Pfad (kein
  geratenes Profil, Status sichtbar). Ohne Objektivnamen bleibt der
  dokumentierte Body-/Mount-Fallback (`lens_name = None` → mount-kompatibles
  Objektiv). Fehlt ein Pflichtfeld, die System-DB oder ein
  nicht-identisches Profil, gilt strikt der manuelle Pfad (kein stiller
  Fallback, Status sichtbar).
- **TCA via Lensfun:** Ist das Profil TCA-kalibriert (`LF_MODIFY_TCA`), sampled
  der Lensfun-Pfad R/G/B an getrennten Subpixel-Koordinaten
  (`lf_modifier_apply_subpixel_geometry_distortion`, Grün = Referenz wie im
  manuellen Modell); ohne TCA-Kalibrierung bleibt die manuelle
  `ca_red`/`ca_blue`-Korrektur wirksam. Der TCA-Pfad ist deterministisch
  (gleiche Eingabe → gleiche Pixel) und per `has_tca` am Corrector abfragbar.
- **Sichtbarkeit:** CLI (`lumina geometry --lensfun-status`, `info!`-Log je
  Render mit aktivem Corrector inkl. `has_distortion`/`has_vignetting`/
  `has_tca`) und GUI (Optics-adjazente Statuszeile: Profil gefunden/fehlt +
  Grund) zeigen den Corrector-Zustand; ein fehlendes Profil ist ein sichtbarer
  Zustand, nie ein stiller Identitäts-Render.

Architektur- und Lizenzgrenzen:
- Lensfun ist eine **native C-Bibliothek unter LGPL-3.0** (Datenbank CC-BY-SA).
  Sie wird **dynamisch** gelinkt (kein statisches Einbetten → keine
  LGPL-Ausweitung auf das Gesamtwerk); Lensfun-Lizenztext + Quellangebot
  müssen im Release gebündelt werden (F-078, analog LibRaw).
- Lensfun ist eine native Desktop-Capability. Die
  native Bindung lebt in einem separaten, feature-gated Crate (`lumina-lensfun`),
  damit `lumina-core` keine nativen FFI-Abhängigkeiten voraussetzt. Ohne Feature
  (oder fehlende Lib/Profile) greift automatisch das manuelle Modell.
- Die Lensfun-Profil-Datenbank wird mit dem Release distribuiert.

Abhängigkeiten: F-031, F-037, F-078, F-099.

**Status (F-098):** Implementiert und unabhängig verifiziert (2026-08-20).
`LensCorrection` (additives Schema-v2-Feld) in `lumina-sidecar`;
`validate_lens` (Wertebereiche exakt wie SOLL, Profil-Whitelist
wide-light/tele-light/standard-neutral), `apply_lens` (Newton-Iteration des
radialen Verzeichnungspolynoms + Vignette-Polynom, Grün-Referenz/RGB),
`apply_ca` (R/B-Skalierung, Grün Referenz) in `lumina-core`; Integration in
`apply_geometry` in der SOLL-Reihenfolge distortion → vignette → perspective →
CA → crop. `mask_recipe.lens_correction = None` schließt Geometrie aus dem
Masken-Hash aus; `recipe_hash` invalidiert den RenderKey. Lensfun-Integration
ist seit 2026-08-20 MVP-Ziel (native, dynamisch gelinkte LGPL-3.0-Capability,
Datenbank CC-BY-SA; Umsetzung in `lumina-lensfun` +
`lumina-core`-Feature, siehe Abschnitt oben).
**Lensfun-Capability und Pipeline-Integration sind implementiert und
unabhängig verifiziert (2026-08-20, BESTANDEN):** `lumina-lensfun` (FFI +
Safe Wrapper, 6 Native-Tests gegen die reale Profil-DB), lumina-core-Feature
`lensfun` (default off, nativ-only), per-Pixel-Verzeichnung/Vignette mit
byte-identischem Fallback auf das manuelle Modell (Test
`lensfun_none_is_byte_identical_to_default_pipeline`).
**Folgeaufgaben N2–N4 sind umgesetzt und unabhängig verifiziert
(2026-08-20, Batch-Verifikation BESTANDEN):** N2 CLI-Verdrahtung
(EXIF→Corrector via `build_lensfun_corrector`, strikter Fallback, 7 neue
feature-gated Tests), N3 CI-Container (`liblensfun-dev` im gepinnten Image,
Feature-Test-/Clippy-Steps mit synchronisierten Features, keine
Misch-Feature-Builds), N4 Lensfun-Lizenz-Eintrag in
`THIRD-PARTY-NOTICES.md`/`fixtures-licensing.md` (LGPL-3.0 dynamisch,
DB CC-BY-SA, SPDX-Detail vor Final-Release zu verifizieren).
Bekannte Grenzen: per-pixel-FFI-Overhead (Benchmark/Optimierung als
F-074-Folgeaufgabe), DB-Ladezyklus pro Render-Aufruf im CLI (MVP ok; die
GUI cacht den Corrector pro Quelle + Dimensionen), Distance-Default 10,0 m
(RawMetadata hat kein Distanzfeld).
**Lensfun-Vollausbau G-06 (LRPAR-G06-GEO, MVP/1.0):** TCA wird via
`LF_MODIFY_TCA` + Subpixel-Resampling im Lens-Stufenpass korrigiert
(`has_tca`, Grün = Referenz; das manuelle `ca_red`/`ca_blue`-Modell bleibt
für Profile ohne TCA-Kalibrierung wirksam und wird bei aktivem TCA
übersprungen — keine Doppelkorrektur); `RawMetadata.lens`
(EXIF-LensModel/Makernote) wird als Lensfun-Lens-Name durchgereicht
(Body-Match als Fallback); CLI `lumina geometry` (inkl.
`--lensfun-status`, ein History-Eintrag pro Aufruf) und GUI
(Geometrie-Sektion + Optics-Profilpicker + Lensfun-Auto-Statuszeile +
Crop-Overlay, ein History-Eintrag pro Commit) sind paritätisch verdrahtet;
die GUI baut den Corrector aus dem Decode-EXIF-Snapshot und cacht ihn pro
Quelle.
**Review-Nachziehen 2026-08-25 (verifiziert, BESTANDEN):**
Vignetting-only-Profile kollabieren das Bild nicht mehr —
`lf_modifier_apply_geometry_distortion` wird auf den Returnwert geprüft;
bei `false` werden die Koordinaten unverändert durchgereichen und der
Corrector wird nur mit `LF_MODIFY_DISTORTION` geometrisch verwendet
(`has_distortion`/`has_vignetting` aus der Initialize-Bitmaske;
Regressionstest mit Vignetting-only-Fixture verlangt exakte
Geometrie-Identität). Zusätzlich ersetzt ein Build-time-Offset-Probe in
`build.rs` (offsetof gegen installierte Header; lauter Abbruch bei
lensfun ≠ 0.3.x ohne verifizierbaren Compiler) die ABI-Wette des
hartkodierten `lf_camera_crop_factor`-Offsets.
**Thread-Sicherheit (lensfun 0.3.4):** Die Datenbank-/Suchpfade der
Distro-/Release-Bibliothek sind nicht thread-safe — `GuessParameters` →
`_lf_parse_lens_name` kompiliert global geteilte POSIX-Regexes lazy ohne
Lock (`_lf_lens_regex_refs`/`regfree` im `lfLens`-Destruktor). Parallele
DB-Loads/Suchen in einem Prozess racen auf demselben `regex_t` (UB; unter
glibc als SIGSEGV beobachtet, macOS-libc toleranter; upstream nach 0.3.4
auf `std::regex` umgestellt). Der Safe-Wrapper (`lumina-lensfun`)
serialisiert DB-Anlage, Suche und Zerstörung hinter einem globalen Mutex;
per-Corrector-`geometry`/`color_gain` bleiben lock-frei. Damit ist die API
thread-safe nutzbar (Regressionstest `concurrent_db_load_and_search_is_safe`
lädt 6 DBs + sucht parallel) und der CLI-`batch`-Pfad (rayon `.par_iter()`,
pro Bild ein Corrector) läuft unter Linux nicht in die Race. Der CI-Fehler
„SIGSEGV in 6 lumina-lensfun-native-Tests unter Ubuntu 24.04" (2026-08-20)
wurde dadurch an der Wurzel behoben — kein `--test-threads=1`-Workaround.
Grenze bleibt: die Serialisierung begrenzt den parallelen DB-Lade-/
Suchdurchsatz (für MVP-Durchsatz irrelevant, da DB-Load pro Render-Aufruf
ohnehin klein ist); ein gepatchtes/neueres lensfun würde den Lock entbehrlich
machen (F-074-Folgeaufgabe, kein MVP-Blocker).

### F-099 Upright und Perspektive

**Ziel:** Perspektivische Verzerrungen manuell korrigieren und die resultierende
Geometrie mit Objektivkorrektur und Crop kombinierbar machen.
`recipe.perspective` enthält `version`, `vertical`, `horizontal`, `rotation`,
`scale`, `aspect_ratio`, `shift_x` und `shift_y`; alle sind endliche Werte,
die Achsenkorrekturen und Rotation in `-1..=1`, `scale` in `0.1..=10`,
`aspect_ratio` in `0.1..=10` und Verschiebungen in `-1..=1`. Die vier
Eckpunkte der normierten Bildebene werden mit einer 3x3-Homographie auf die
Ausgabeebene projiziert; bilineares Resampling und definierte Randfüllung
(`transparent` wird im MVP zu Schwarz) sind verbindlich.

Die manuelle Perspektive liegt nach F-098-Verzeichnung und vor F-093-Crop;
die automatische Analyse stürzender Linien ist ausdrücklich 1.5
(LRPAR-G06-UPRIGHT-15, nicht Teil des Geometrie-MVP). Das manuelle
Perspektiv-Modell ist reines Core-Modell (bilineares Resampling, schwarze
Randfüllung) und in CLI wie GUI **ohne** Lensfun-Capability verfügbar; ein
GUI-Gate hinter ein Lensfun-Feature ist unzulässig (G-06-Parität). Alle
Parameter, Homographie-/Pipelineversion und resultierenden Dimensionen
gehören zum RenderKey; Geometrieänderungen invalidieren Preview/Export, nicht
Decode oder AI-Artefakte. Abnahme: Identität, Eckpunktprojektion,
Parametergrenzen, Zusammenspiel mit Objektivkorrektur/Crop und Cache.
Abhängigkeiten: F-029, F-031, F-041, F-098.

**LRPAR-G06-UPRIGHT-15 Auto-Upright-Analyse (Release 1.5):** Zusätzlich zur
manuellen Perspektive analysiert LuminaRust stürzende Linien automatisch und
persistiert das Ergebnis als additive Rezept-Stufe. Die Analyse ist
**klassisch, modellfrei und deterministisch** (keine ONNX-Modelle, keine
Zufallszahlen, keine Vanishing-Point-Optimierung, kein Guided-Mode).

- **Schema:** `recipe.upright` (additiv in Schema v2, top-level wie
  `perspective`; absent = keine Analyse und Identität, keine Migration).
  `Upright` trägt `version` (`1`), `enabled` (bool) und
  `analysis: Option<UprightAnalysis>`. `UprightAnalysis` trägt
  `fingerprint: AnalysisFingerprint` (Algorithmus, Version,
  `input_fingerprint`), die vorgeschlagenen Perspektivwerte `vertical`,
  `horizontal`, `rotation` (je `-1..=1`, exakt die F-099-Domäne),
  `line_count` (Zahl der stützenden Linienpixel) und `confidence` (`0..=1`).
  Das Objekt wird nie über eine Listenposition identifiziert; es gibt genau
  eine Analyse pro Rezept.
- **Analyse-Algorithmus `upright-lines-v1` (deterministisch, modellfrei):**
  Graustufen-Downsample (längste Seite ≤ 256, Box-Filter), dann ein
  deterministischer Winkel-Sweep von ±30° in 0,5°-Schritten. Für jeden
  Kandidatenwinkel wird das Bild entlang der gedrehten Achse gebinnt und die
  Schärfe des resultierenden Projektionsprofils gemessen (Summe der quadrierten
  zweiten Differenzen): Der Winkel mit dem spitzesten horizontalen bzw.
  vertikalen Profil ist der Rotationsvorschlag (Vorzeichen so, dass die
  Anwendung die Linien ausrichtet; die stärkere Achse gewinnt). Der
  Vertikal-Keystone entsteht aus dem Unterschied des besten Vertikal-Profil-
  winkels linke/rechte Bildhälfte, der Horizontal-Keystone aus obere/untere
  Hälfte. Das Projektionsprofil ist bewusst gegen das Staircase-Aliasing
  robust, das eine Per-Pixel-Gradientenorientierung bei groben Gittern
  verzerrt. Werte außerhalb `-1..=1` werden auf die Domäne begrenzt (`clamp`,
  dokumentiert, kein stilles Verwerfen); `line_count` zählt die Pixel mit
  Sobel-Magnitude ≥ 25 % des stärksten Gradienten, `confidence` die
  Prominenz des Profil-Peaks über den Mittelwert des Sweeps.
- **Ausgabeformat:** die vorgeschlagenen `vertical`/`horizontal`/`rotation`
  plus `line_count`, `confidence` und der `AnalysisFingerprint`. Der
  `input_fingerprint` wird vom Aufrufer (CLI/GUI) aus Quell-Content-Hash,
  Decode-/Geometrie-Kontext und Algorithmus-Version abgeleitet
  (`upright_input_fingerprint`), damit die Analyse ohne erneute Pixelanalyse
  auf Gültigkeit geprüft werden kann.
- **Identität/Veraltung:** Weicht der gespeicherte `input_fingerprint` vom
  aktuellen Quell-/Decode-Kontext ab, ist die Analyse **veraltet**. Veraltet
  wird sichtbar gemeldet (CLI `upright --status`, GUI-Statuszeile) und nie
  stillschweigend neu berechnet oder durch eine andere Korrektur ersetzt; das
  Rezept bleibt vollständig reproduzierbar. Eine erneute Analyse ist eine
  explizite Nutzeraktion (kein Auto-Rerun als einzige Option).
- **Einordnung (unmittelbar vor der Perspektive):** Upright ist eine
  Geometrie-Untersstufe **vor** der F-099-Perspektive. Bei `enabled = true`
  liefert die persistierte Analyse die **effektive** Perspektive
  (`EditRecipe::effective_perspective`): `vertical`/`horizontal`/`rotation`
  aus der Analyse, `scale = 1`, `aspect_ratio = 1`, `shift_x = 0`,
  `shift_y = 0`. Das manuelle `recipe.perspective` bleibt persistiert und ist
  bei `enabled = false` unverändert wirksam (Lightroom-Semantik: Upright setzt
  die Transformation; das Ausschalten stellt die manuellen Werte wieder her).
  `enabled = true` ohne `analysis` wird laut abgelehnt (kein stiller
  Identitäts-Render).
- **Cache:** `recipe.upright` liegt im Root und fließt automatisch in den
  vollen `recipe_hash`/RenderKey; wie `geometry`/`lens_correction`/
  `perspective` wird es aus der Masken-Identität (`mask_recipe`) entfernt,
  damit eine Upright-/Geometrieänderung keine quellgroßen Masken invalidiert.
- **GPU-Parität:** Die Korrektur einer aktiven Upright-Analyse ist die
  bestehende F-099-Perspektiv-Homographie und läuft damit auf GPU mit
  CPU-Oracle-Parität (explizites Crop) bzw. routet ohne explizites Crop laut
  auf CPU (`geometry (default content crop)`, dokumentierte CROP-MAXRECT-1-
  Grenze) — es gibt keinen stillen CPU-Zweig und keinen GPU-only-Weg. Die
  Analyse selbst ist eine deterministische CPU-Vorverarbeitung, die Parameter
  persistiert (wie Auto-Tone); sie ist **keine** Pixel-Renderstufe und erzeugt
  keine Render-Route.
- **Abnahme:** deterministische Analyse (zwei Läufe byte-identisch),
  Identität bei `enabled=false`/ohne Analyse, Vorschlag richtet synthetisch
  gedrehte/verkippte Linien messbar aus, Rezept mit Upright ist byte-identisch
  zu einem Rezept mit den gleichen manuellen `Perspective`-Werten,
  Fingerprint-Veraltung sichtbar, Schema-Roundtrip inkl. Validierungsablehnung,
  CLI-Status/-Analyse, GUI-headless (Analyse-Aktion + Status + Toggle).
  Abhängigkeiten: F-029, F-031, F-041, F-098, F-099.

### G-05 Lens Blur (Tiefen-Bokeh, Release 1.0)

**Ziel:** Ein deterministisches, rezept-persistiertes Tiefen-Bokeh nach
Lightroom-Vorbild (Fokus-Rahmen, Focal Range, Blur Amount, Bokeh-Formen) als
Produktfunktion — keine stille Vorstufe mehr. `recipe.lens_blur` (`LensBlur`,
additives Schema-v2-Feld, absent = Identität, keine Migration) enthält
`version` (1), `enabled` (bool), `focus_rect` (normiertes Rechteck
`x`/`y`/`width`/`height` in `0..=1`, positive Fläche, im Einheitsquadrat),
`focal_near`/`focal_far` (je `0..=1`, `near <= far`: scharfes Tiefenband),
`blur_amount` (`0..=1`, 0 = Identität), `bokeh` (`round` | `elliptical` |
`hexagonal`) und optional `depth_artifact` (Referenz auf eine externe
Tiefenkarte: relativer Pfad + SHA-256-Prüfsumme; absolute Pfade verboten).

**Tiefenquelle (Entscheid):** Ohne `depth_artifact` gilt die deterministische
Heuristik: Tiefenwert 0 im Fokus-Rechteck, außerhalb die auf die
Bilddiagonale normierte Distanz zur Rechteckkante (`0..=1`). Ist ein
`depth_artifact` referenziert, MUSS der Aufrufer die Tiefenebene liefern;
fehlt sie oder weicht die Prüfsumme ab, bricht der Render laut ab (kein
stiller Fallback auf die Heuristik — Heuristik vs. fehlend ist eine bewusste
Rezeptentscheidung, kein Laufzeit-Raten). Der Status (`off` / `heuristic
active` / `missing depth artifact`) ist in CLI (`lens-blur --list`) und GUI
(Optics-adjazente Sektion) sichtbar.

**External-Depth-Bindung (Entscheid 2026-09-16, DEPTH-PLUMBING-1):** Die
Laufzeit-Bindung einer externen Tiefenebene — Datei-/Artefaktformat, Loader in
CLI/GUI/MCP und GPU-Upload via `GpuContext::set_depth_plane` — ist bewusst
**Post-MVP**. In v1 (Release 1.0) ist `depth_artifact` damit ein
schema-reserviertes Referenzfeld: CLI und GUI persistieren die Referenz und
melden sie als `missing depth artifact`, aber **kein** Caller lädt eine Ebene
(`RenderContext::depth` ist überall `None`, `set_depth_plane` bleibt
ungenutzt). Ein Rezept mit gesetzter Referenz bricht in jedem Caller
(CLI `render`/`export`/`process`/`batch`, GUI-Preview und -Export, MCP,
Matrix-Runner) laut ab — nie ein stiller Heuristik-Fallback. Dieser laut-abbrechende
v1-Vertrag ist getestet: CLI `lens_blur_missing_depth_artifact_fails_render_loudly`,
GUI `g05_lens_blur_preview_changes_and_missing_depth_fails_loudly`, GPU-Parität
in `lumina-gpu/tests/parity.rs` — und damit keine stille Lücke. Begründung: Es gibt
weder ein definiertes Tiefenkarten-Format (kein `.lumina.zdata`-Record-Typ,
keine Dimensions-/Quantisierungs-/Prüfsummensemantik) noch einen Producer
(Tiefenschätzung) im v1-Umfang; ein geratenes Format würde Reproduzierbarkeit
durch Laufzeit-Annahmen ersetzen. Die Bindung wird erst mit einer eigenen
Format-/Schemaentscheidung (im SOLL zuerst) und einem Producer nachgezogen;
bis dahin bleibt die Fokus-Rechteck-Heuristik die einzige renderbare
Tiefenquelle. Details der reservierten Referenz: `architecture/sidecar.md`
§ Externe Tiefenkarte.

**Render-Semantik:** Pro Pixel Tiefenwert `d`, Schärfegewicht 0 im Band
`[focal_near, focal_far]`, linearer Ramp auf 1 an den Rändern; Ausgabe =
`lerp(original, bokeh_blur(original, radius), gewicht)` mit
`radius = round(blur_amount * 16)` px. `bokeh_blur` ist eine normierte
Kernfaltung (Radius 0 = Identität): `round` = Scheibe, `elliptical` =
2:1-Ellipse, `hexagonal` = Sechseck — alle deterministisch (ganzzahlige
Kernmasken, keine Zufallsanteile), kanalunabhängig auf RGB, Alpha unberührt.
Alle Parameter sind endlich; Verletzungen werden abgelehnt, nicht geclippt.

**Platzierung:** Sub-Stufe von `Crop`, nach Crop/Rotation/Spiegelung, vor
`Masks`/`Output` (`… → Crop(F-093) → LensBlur(G-05) → Masks → Output`). Das
Top-Level-Format-Tupel bleibt unverändert; `recipe_hash` enthält das
serialisierte `lens_blur` (volle Invalidierung von Preview/Export), der
`mask_recipe_hash` schließt es wie `effects` ein (Pixelwirkung ohne
Geometrieänderung). GPU rendert `lens_blur` seit 2026-09-14 (Heuristik und
externe Depth via `set_depth_plane`; Missing/Mismatch = lauter Fehler,
kein stiller Fallback).

**Abnahme:** JSON-Roundtrip, Wertebereichs-/Clipping-Tests (`blur_amount`
0-Identität, RGB-Clipping), Determinismus (zwei Läufe byte-identisch),
Bokeh-Differenz (drei Formen unterscheiden sich auf kontrastreichen
Fixtures), Golden/PSNR-Gates mit dokumentierten Toleranzen, CLI-Roundtrip
mit Exit-Codes, GUI-headless (Setter → Datei → Reload), Missing-Artefakt
bricht laut ab.

**Implementierungsstatus (G-05, LRPAR-G05-LENSBLUR):** Umgesetzt.
`LensBlur`/`FocusRect`/`BokehShape`/`DepthArtifactRef` (additives
Schema-v2-Feld `recipe.lens_blur`, absent = Identität, keine Migration) in
`lumina-sidecar` (Serialisierung als Top-Level-Key, Validierung inkl.
Focal-Order, Focus-Geometrie und portabler Relativpfade);
`lumina-core::lens_blur` (Heuristik, drei Integer-Kerne, `radius =
round(amount·16)`, RGB, Alpha unberührt, Missing-Artefakt = harter
`InvalidAdjustment`) mit Hook in `render_frame_from_base` nach Crop und vor
Masks (keine zweite Pipeline); `RenderContext::depth` für externe Ebenen (GPU-Vertrag: `set_depth_plane`;
Caller ohne Plane erhalten den lauten Oracle-Fehler) — in v1 bindet kein Caller
eine Ebene (siehe Entscheid „External-Depth-Bindung“ oben);
GPU rendert aktives `lens_blur` seit 2026-09-14 (Parität byte-identisch bzw.
maxAbsDiff ≤ 1 nach Post-Stufen). CLI `lumina lens-blur`
(setzen/lesen/listen/löschen, Exit 0/1/2 wie Bestand); GUI-Sektion in Optics
(Enable/Amount/Focal/Bokeh/Fokus-Rechteck/Status) + Fokus-Overlay im Preview
+ headless E2E-Tests (Setter → Commit → Datei → Reload, Preview-Änderung,
Missing-Artefakt-Fehler). Tests: Sidecar-Roundtrip/Validierung (2),
Core-Unit/Integration/PSNR (9), CLI (3), GUI-headless (3).

## Reproduzierbarkeit

Jeder Render-Key enthält mindestens:

```text
source_content_hash
decode_parameters
pipeline_version
virtual_copy_id
recipe_hash
mask_artifact_hashes
output_profile
output_dimensions
output_format
```

Ein Dateipfad oder Zeitstempel allein ist kein gültiger Render-Key. Der
`RenderKey` wird deterministisch gehasht; `stage_digest` ermöglicht
stufenspezifische Digests (decode / mask / histogram / render), sodass
beispielsweise eine reine Ausgabegrößenänderung den Decode-Cache nicht
invalidiert, wohl aber Preview und Export.

## Auto-Tone

Im Raster-MVP werden die RGBA8-RGB-Kanäle als sRGB-codierte Werte auf 0..=1
normalisiert und mit Rec.709 (0.2126/0.7152/0.0722) gewichtet. Alpha wird
ignoriert, auch bei transparenten Pixeln. Der Mittelwert ist die exakte
pixel-order Rec.709-Summe geteilt durch die Sample-Anzahl (seit R2-PERF-01
bit-identisch zur Messung des Exposure Matchings). Median/p01/p99 sind
dokumentierte Klassenmark-Schätzer über den gemeinsamen 256-Bin-Luminanz-
Histogramm-Pass: Die beiden den Rang `q·(n−1)` klammernden Ordnungsstatistiken
werden durch das Klassenmark ihres Bins geschätzt (Bin 0 → Unterkante `0.0`,
Bin 255 → Oberkante `1.0`, Innen-Bins → Bin-Zentrum) und mit dem exakten
Fraktionalrang linear interpoliert. Auto-Tone richtet den Median auf das Ziel
aus und bestimmt Kontrast aus der p01/p99-Spanne; Exposure ist auf -10..=10 EV
und Contrast auf -1..=1 begrenzt. Leere Bilder liefern 0, Schwarz liefert den
oberen Exposure-Fallback und Weiß den unteren — uniform schwarze/weiße Bilder
melden den Median exakt als `0.0`/`1.0`, sodass diese Fallback-Zweige
bit-identisch zum früheren sortierten Verfahren bleiben.

**Auto-Tone Mehrregler (LR-ähnlich, normativ 2026-09-04):** Auto-Tone setzt
nie einen Einzelregler auf Anschlag, sondern verteilt die Korrektur
Lightroom-ähnlich auf sechs Regler aus den Histogramm-Perzentilen
(p01/Median/p99, 256-Bin-Pass): `exposure` (Median → Ziel), `contrast`
(p01/p99-Spanne → Zielspanne, weich begrenzt, nie hart  ±1-Anschlag),
`whites`/`blacks` (p99/p01-Enden entzerren), `highlights`/`shadows`
(Lichter-/Schatten-Balance). Alle sechs Werte landen im Rezept (persistiert
mit Analyse-Fingerprint); deterministisch (gleiche Perzentile → gleiche
Werte); kein Regler darf nach Auto-Tone am harten Limit kleben, solange die
Ziele mit Reserve erreichbar sind (sonst sichtbarer Hinweis, kein stiller
Anschlag).

**Genauigkeitsvertrag (R2-PERF-01, normativ):** Gegenüber der historischen
linearen Interpolation zwischen sortierten Samples gilt für JEDES Bild
(dicht oder sparsam besetzt) die universelle Schranke
`|Schätzer − exaktes Perzentil| ≤ 1/256` (ein Bin breit): Jedes Klassenmark
liegt im Bin der Ordnungsstatistik, die es schätzt, und lineare Interpolation
ist in beiden Klammern konvex. Der Estimator ist monoton nicht-fallend in `q`
(`p01 ≤ median ≤ p99` per Konstruktion); Gleichverteilungs-Bilder liefern
`p01 == median == p99` (Spanne exakt 0 → Kontrast-Identität). Der Mittelwert
ist von der Quantisierung nicht betroffen (exakt).

> **Implementierungsstatus (F-039, 2026-08-18):** Explizite
> `LuminanceHistogram`-Repräsentation in `lumina-core` umgesetzt
> (`crates/lumina-core/src/histogram.rs`). Die Messdomäne ist Rec.709 auf
> sRGB-codierten RGBA8-Werten (Alpha ignoriert), über 256 Bins in `0..=1`.
> Quantile werden per linearer Interpolation über die kumulative Verteilung
> berechnet; die Konsistenz gegenüber `analyze_tone` ist für dicht besetzte
> Histogramme mit ≤ 1/256 Toleranz dokumentiert und getestet (Mittelwert via
> Bin-Zentren ≤ 1/512). Serde (Serialize/Deserialize) und ein stabiler
> blake3-Digest (`digest()` über Bins + Dimensionen) machen die Repräsentation
> direkt für `CacheStage::Histogram` nutzbar.

> **Implementierungsstatus (R2-PERF-01, 2026-08-26):** `analyze_tone`
> (`crates/lumina-core/src/tone.rs`) nutzt statt eines `Vec<f64>` pro Pixel
> (~8 Byte/Pixel ≈ 192 MB bei 24 MP) plus vollständiger O(n log n)-Sortierung
> jetzt einen gemeinsamen Single-Pass (`accumulate_bins_and_luminance_sum`,
> `histogram.rs`) über 256 Bins plus exakte Luminanzsumme (2 KB Stack-State,
> O(n)). Die Signatur ist unverändert; neu ist additiv
> `analyze_tone_with_histogram(frame) -> (ToneAnalysis, LuminanceHistogram)`:
> EIN Pass liefert Histogramm-Panel UND Tone-Panel dieselbe Pass-Struktur und
> konsistente Zahlen (GUI-Kopplung kann damit zwei Vollläufe ersetzen; das
> Histogramm-Panel selbst bleibt mit `LuminanceHistogram::new` byte-stabil,
> inklusive Digest). Werte-Auswirkungen, explizit und nicht still:
> - `mean`: unverändert exakt; Summationsreihenfolge wechselte von
>   sortierter Summe zu Pixel-Order — Abweichung höchstens letzte f64-Bits,
>   bit-identisch zur `match_total_exposure`-Messung (getestet).
> - `median`/`p01`/`p99`: innerhalb der oben normierten 1/256-Schranke;
>   reale Fotos (dicht belegte Bins) bewegen sich typischerweise deutlich
>   darunter. Auto-Tone-Ergebnisse können sich dadurch um bis zu
>   `(1/256)/(m·ln2)` EV ändern (bei Mitteltönen ≈ 0,02 EV; der
>   Schwarz-/Weiß-Fallback bleibt bit-exakt). Bereits persistierte Rezepte
>   speichern ihre Auto-Tone-Werte und bleiben unverändert; erst NEUE
>   Auto-Tone-Berechnungen nutzen den dokumentierten Estimator.
> - Consumer: GUI-Tone-Panel, `lumina_analyze` (MCP) und Auto-Tone lesen die
>   Werte weiter über die unveränderte `analyze_tone`-Signatur; die F-043-
>   Goldens prüfen weiterhin gegen die geschlossenen Formeln — Mittelwert
>   bei 1e-9, Quantile bei der normierten 1/256-Toleranz (Checker-Fixture
>   zusätzlich exakt bei 1e-9), siehe Status F-043.
> - Benchmark: `core/analyze_tone__2048` ging von ≈ 69 ms (Baseline) auf
>   ≈ 2–4 ms erwartungsgemäß zurück (Single-Pass + 256-Bin-Auswertung statt
>   33-MB-Allokation + pdqsort bei 2048²; `core/histogram__2048` misst den
>   reinen Pass mit 2,76 ms). Budgets in `perf/baseline.json` wurden NICHT
>   angepasst (Gate schlägt nur bei Verlangsamung an).

## Exposure Matching

`Match Total Exposure` misst nach dem Auto-Schritt die definierte gewichtete
Luminanz und berechnet `log2(target/current)` mit Epsilon, finite-Schutz und
-10..=10-Begrenzung. Die Implementierung muss Schutz gegen
Division durch null, extreme Zielwerte, Clipping und Maskeneinflüsse enthalten.
Auto-Tone misst im Raster-MVP den dekodierten aktuellen Raster-Messbereich
(alle RGBA-Pixel, Alpha ignoriert); `Match Total Exposure` misst dagegen den
finalen sichtbaren Messbereich nach Crop, Geometrie und aktiven Masken
(siehe F-041 unten), vor Outputprofil und Export-Transferfunktion.
Die Raster-MVP-Reihenfolge lautet Source-Actions (im CLI via F-042-N1
persistiert und beim Rendern aus dem Bundle angewandt), Auto-Tone, Preset,
CLI-Overrides, danach Matching. Berechnete
Auto-Werte und ein RGBA8-Analysefingerprint werden im Rezept persistiert und
bei gültigem Fingerprint wiederverwendet.

### F-041: Finaler sichtbarer Messbereich

**Messbereich:** `Match Total Exposure` misst den finalen sichtbaren
Messbereich = das Render-Ergebnis NACH Crop/Geometrie (aus `render_frame`),
nicht das dekodierte Original. Im CLI ist der gemessene Frame bereits das
post-Crop/Geometrie-Render-Ergebnis; F-041 schreibt das normativ fest und
testet es. Im GUI wird die gerenderte Vorschau gemessen (derselbe Frame, der
angezeigt wird).

**Aktive Masken:** Liegen im Render-Ergebnis aktive Masken-Layer
(`mask_layers`) vor, wird die Messung auf den maskierten sichtbaren Bereich
beschränkt: Jedes Pixel erhält ein Gewicht
`w = ∏_layer (plane_layer[pixel] / u16::MAX)` (Produkt über alle aktiven
Layer — Schnittmenge: Ein Pixel, das in irgendeinem Layer vollständig
maskiert ist (Gewicht 0), gehört nicht zum global sichtbaren Messbereich).
Der Mittelwert wird gewichtet über die sichtbaren Pixel gebildet
(Rec.709-Luminanz, Alpha weiterhin ignoriert). Ohne Masken (`None`/leer) ist
das Resultat identisch zur bisherigen Raster-Messung
(`match_total_exposure_masked` delegiert bei leerem Slice exakt an
`match_total_exposure`). Gleiches gilt, wenn ein nicht-leerer Satz von Ebenen
vorliegt, deren **jede** vollständig `u16::MAX` ist (Maske ohne Wirkung):
Auch dieser Fall delegiert bit-exakt an den ungemaskten Pfad
(All-MAX-Fast-Path, siehe Status F-043) — dokumentierter Fast-Path, kein
stiller Fallback.

**Grenzen (ehrlich):** Die visuelle Pixel-Modulation durch Masken ist mit
F-049 umgesetzt und stimmt mit der F-041-Messbereichs-Semantik überein
(Gewichte = Schnittmenge der Ebenen). Offen bleibt die Geometrie-Ausrichtung
von Masken (dokumentierte Grenze, F-042).

**Schutz:** Epsilon-, Clipping-, finite- und Fallback-Schutz der bisherigen
Implementierung bleiben erhalten (Epsilon `1e-6`, Begrenzung `-10..=10` EV,
finite-Wächter). Vollständig maskiertes Bild (kein sichtbares Pixel,
Gewichtssumme ≤ Epsilon) → definierter Fallback: Delta `0.0` (Identität,
kein Adjustmentschritt), konsistent zur `sample_count == 0`-Semantik von
`suggest_auto_tone` (Exposure `0.0`) — kein NaN, kein Panic, kein stiller
Fallback. Ein Dimensions-Mismatch zwischen Masken-Ebene und Frame wird mit
`CoreError::InvalidMaskPlane` abgelehnt (kein stiller Fallback).

**Status (F-041):** Implementiert sind der gewichtet-maskierte Messbereich in
`lumina-core` (`match_total_exposure_masked`,
`crates/lumina-core/src/tone.rs`, inklusive handgerechneter Unit-Tests), die
normative Festschreibung von Crop/Geometrie im Messbereich sowie die
CLI-/GUI-Verdrahtung: Das CLI misst das Render-Ergebnis mit den effektiven
Ebenen aus `render_output.mask_layers`; die GUI misst die gerenderte Vorschau
mit den Masken-Ebenen des letzten Renderings. `match_total_exposure` bleibt in Signatur
und Verhalten unverändert (interne Delegation auf die gemeinsame
Delta-Logik). F-049 (Pixel-Modulation invert/feather/blur/density) und
F-042-N1 (Source-Actions-Persistenz) sind umgesetzt und verifiziert.

**All-MAX-Fast-Path (F-043, Semantik-Hinweis):** Liegen Masken-Layer vor,
deren **jede** Ebene vollständig `u16::MAX` ist (jedes Pixelgewicht exakt
`1.0`, die Maske hat keine Wirkung), delegiert `match_total_exposure_masked`
bit-exakt an den ungemaskten Pfad (`matching_delta(analyze_tone(frame).mean,
…)`, identisch zu `match_total_exposure`). Das ist ein dokumentierter
Fast-Path, kein Fallback: Mathematisch sind beide Messungen identisch, aber
die Summation des ungewichteten Mittels (`mean_luminance`, Pixel-Order — seit
R2-PERF-01 auch die `analyze_tone.mean`-Definition) und die zeilenweise
Summation der gewichteten Schleife können sich im letzten f64-Bit
unterscheiden — erst die Delegation garantiert die bit-exakte Identität
`All-MAX ≡ ungemaskt`. Die `InvalidMaskPlane`-Validierung läuft vor dem
Fast-Path; eine dimensionsfehlerhafte All-MAX-Ebene wird weiterhin abgelehnt.

**Status (F-043):** Echte Property- und Referenzbildtests für Auto-Tone und
Exposure Matching sind umgesetzt:

- **Property-Tests** (`crates/lumina-core/src/tone_props.rs`, proptest):
  Invarianten für Wertebereiche/Endlichkeit, Monotonie in Helligkeit und
  Zielwert, Schwarz-/Weiß-Fallbackpfade, Alpha-Ignoranz, Fingerprint-
  Determinismus, `InvalidMaskPlane`-Ablehnung und die F-041-Maskensemantik
  (leeres Slice bit-exakt ≡ ungemaskt, All-MAX-Ebenen bit-exakt ≡ ungemaskt
  über den Fast-Path, 0/65535-Ebenen ≡ Messung auf dem sichtbaren Unterframe
  mit dokumentierter 1e-9-Toleranz — zwei verschiedene f64-Summationspfade,
  sortiert vs. zeilenweise). Fallzahlen: 64 Cases für die schweren
  Frame-/Masken-Properties, 256 (proptest-Default) für die leichten; die
  3 aufgenommenen Regression-Seeds (`proptest-regressions/tone_props.txt`)
  sind eingecheckt und laufen grün.
- **Referenzbildtests** (`crates/lumina-core/tests/reference_images.rs` +
  `tests/fixtures/`): drei 8×8-PNG-Fixtures (`reference_gradient`,
  `reference_checker`, `reference_zone`) mit programmatischer Provenance —
  deterministisch aus dokumentierten Pixelfunktionen erzeugt, keine externen
  Quellen, keine Lizenzpflicht (`tests/fixtures/README.md` dokumentiert die
  exakten Formeln und die Regeneration). Seit R2-PERF-01 wird der
  `analyze_tone`-**Mittelwert** gegen die geschlossenen Formeln mit 1e-9
  geprüft; **Median/p01/p99** mit der normierten R2-PERF-01-Toleranz von
  einem Bin (1/256 + Slack) — der Checker-Fixture pinnt zusätzlich die exakte
  Reproduktion (0.0/0.5/1.0) bei 1e-9. Auto-Tone-/Matching-Ergebnisse mit
  ±0.01; die vom bin-quantisierten Median abgeleitete Gradient-Exposure mit
  ±0.05 (dokumentierte Log-Sensitivitäts-Amplifikation); zusätzlich eine
  Monotonie-Kontrolle über alle drei Fixtures sowie Property-Tests für die
  universelle 1/256-Schranke und die Quantil-Monotonie in `tone_props.rs`.

## Cache und Invalidierung

Cache-Stufen dürfen Decode, Demosaicing, Histogramm, Preview, Masken und Export
enthalten. Jeder Eintrag kennt Eingabeschlüssel, Versionen und Prüfsumme und
kann vollständig gelöscht werden.

Der Ordner-Cache liegt unter `.lumina/` und ist nicht autoritativ. Pro Quelle
und virtueller Kopie wird standardmäßig nur die aktuelle Standardvorschau beim
Verlassen des Bildes gespeichert. Eine 1:1-Vorschau ist eine geerbte
Ordneroption und standardmäßig deaktiviert. Verwaiste Cacheeinträge dürfen
gelöscht werden, sobald ein Bild über Lumina verschoben oder umbenannt wurde
beziehungsweise beim Scan nicht mehr gefunden wird.

> **Implementierungsstatus (F-086, 2026-08-17):** Umgesetzt und unabhängig
> verifiziert. `lumina-core` besitzt eine native Disk-Schicht
> (`DiskFolderCache`, crates/lumina-core/src/cache/disk.rs): atomare Writes,
> `settings.json` mit feldweiser Eltern-Vererbung, Vorschauen pro Quelle +
> virtueller Kopie unter `.lumina/previews/` (Standard- vs. 1:1-Vorschau) und
> sofortiger Prune verwaister Einträge. Offene Folgeaufgabe: Test für partielle
> Settings-Vererbung (Kind-JSON mit nur einem gesetzten Feld).

Eine reine Crop- oder Ausgabeänderung soll keine unnötige AI-Inferenz auslösen.
Ändert sich dagegen Quelle, Decode-Kontext, Pipelineversion, Maskenartefakt oder
Rezeptabschnitt einer abhängigen Stufe, wird die betroffene Stufe invalidiert.
Parallele Preview-Ergebnisse werden verworfen, wenn sie nicht mehr zum aktuellen
Rezeptstand gehören.

> **Implementierungsstatus (PERF-GUI-1, 2026-08-26):** Interaktive
> Stufen-Cache-Schicht für die GUI-Vorschau umgesetzt. Die demosaizierte Basis
> (`Decode`/`SourceActions`/ROI-Crop, vor `Adjustments`) liegt als `ImageFrame`
> im RAM ([`StageFrameCache`, crates/lumina-core/src/stage_cache.rs],
> byte-budgetiert mit LRU; nativ 512 MiB) und wird über einen
> rezeptblinden Basis-Digest identifiziert:
> `RenderKey::stage_digest(CacheStage::Base)` (= `digest_for("base")`; deckt
> Quell-Hash, Decode-Version, Pipeline-Version, Virtual-Copy-ID,
> Source-Action-Artefakt-Hashes, ROI-Fenster und Rahmengeometrie ab, niemals
> das Rezept). Eine Exposure-/Contrast-/WB-/Farb-Änderung nullt nur die
> finale Render-Identität und trifft danach denselben Basis-Eintrag; erneut
> ausgeführt werden ausschließlich `Adjustments → Geometrie → Masken`
> (`render_frame_from_base`), nachweisbar über `StageWork`-Zähler und
> GUI-Tests (Cache-Hit/Miss). Decode/Demosaic wird bei Regleränderung nicht
> wiederholt; der blake3-Quellhash wird pro geladener Datei memoisiert statt
> pro Render-Tick berechnet. Ein Cache-Miss baut die Basis einfach neu auf
> (reines Performance-Ereignis, kein Fallback-Pfad); eine neue Quellidentität
> löscht den Cache vollständig. Pixel-Identität ist per Unit-Test bewiesen
> (gestaffelter Pfad ≡ `render_frame`, byteweise). Grenzen: CPU/RAM-only als
> MVP — eine GPU/VRAM-Variante bleibt GPU-STAGE-1 mit ADR vorbehalten
> (`lumina-core` erhält keine GPU-Abhängigkeit); Masken-/Geometrie-Stufen
> werden bei jedem Tick mit ausgeführt, solange sie downstream der Basis
> liegen (korrekt, aber nicht separat gecacht). Keine neuen F-074-
> Benchmark-IDs; bestehende Baselines/Budgets sind von der Änderung nicht
> betroffen (`render_frame` ist semantisch unverändert).

### Implementierungsstatus GPU-Pfad (PERF-GUI-2 / GPU-STAGE-1 / GUI-WGPU-PRESENT-1)

Der GPU-Beschleunigungspfad (`crates/lumina-gpu`; DAG-Spezifikation und
Detailstatus in `docs/gpu-bootstrap.md`) ist auf folgenden Stand gebracht:

- **Stufen auf GPU:** Tone+WB (Bestand), neu eine dedizierte
  **SourceAction-Stufe** als WGSL-Pass — Compositing gebundener Artefakte
  exakt mit der CPU-Oracle-Semantik (`out = replacement` bei
  Regionsabdeckung `>= 32768`, exakter Integer-Vergleich per `textureLoad` auf
  `R16Uint`; reine Texelkopie ⇒ mit neutralem Rezept **byte-identisch** zum
  CPU-Pfad). Die Routing-Validierung kennt gebundene Artefakte
  (`unsupported_gpu_stages_for`) und CPU-routet weiterhin laut sichtbar, wenn
  keine/passende Artefakte fehlen. Der **Masken-Datenpfad**
  (`combine_mask_planes` nach F-041-Schnittprodukt + `upload_mask_plane`,
  Roundtrip byte-exakt getestet) macht evaluierte Ebenen im VRAM-Composite
  sichtbar; Masken modulieren CPU-seitig noch keine Pixel (dokumentierte
  F-042-Grenze), daher existiert dafür derzeit keine Pixel-Gleichheit zu
  verifizieren.
- **Paritäts-Stufen (GPU-RENDER-PARITY-1, verifiziert BESTANDEN):** Stufe 1
  (Ton-Domäne: Presence, Curves, HSL, Point Color, Vibrance/Sättigung,
  Color Grading) + Stufe 2 (Detail-Kette: Noise Reduction → Sharpening →
  Vignette → Grain, in exakter Oracle-Reihenfolge) + Red-Eye (G-14, in
  Oracle-Reihenfolge nach Sharpening, inkl. VRAM-Pfad; ungültige Werte bleiben
  laut CPU-geroutet) laufen auf GPU mit CPU-Oracle-Parität — byte-identisch wo
  0 gemessen, sonst maxAbsDiff ≤ 1 (≤ 2 für voll gestapelte Rezepte),
  PSNR ≥ 48 dB, Bias ≤ 0.05. Erledigt (Teilwelle, 2026-09-16,
  LRPAR-G06-UPRIGHT-15): eine aktive Upright-Analyse wird über die effektive
  F-099-Perspektive identisch auf CPU und GPU geplant (`EditRecipe::
  effective_perspective`); mit explizitem Crop ist sie GPU-rendered
  (Parity-Recipe `upright_perspective_cropped`, maxAbsDiff 0/1), ohne Crop
  routet sie laut über den Default-Content-Crop-Grund — kein stiller CPU-Zweig.
  Erledigte Follow-ups: Radius>10-Ablehnung an
  beiden Eintrittspunkten, `GPU_EFFECTIVE_SCALE` zentral, Parity-Recipe
  radius 10 + Masking, schema-fremde Keys laut abgelehnt. Erledigt (Teilwelle,
  BESTANDEN): Spot-Heal-Pass (Legacy, beide Pfade, byte-identisch),
  SourceAction-Batching (>7, byte-identisch), volle Nested-Range-Validierung
  am GPU-Eintritt (`validate.rs`, Error-Parität inkl. typed-/generative-Spots).
  Rest (Ziel: kein CPU-only-Zweig, User-Entscheid 2026-09-13):
  `geometry (default content crop)` (CROP-MAXRECT-1, 2026-09-14:
  datenabhängiges MaxRect ohne expliziten Crop laut CPU-geroutet);
  lebendes Inventar:
  `cpu_routing_inventory_is_complete`. Erledigt (Teilwelle,
  BESTANDEN, Commit 2026-09-14): Geometrie-Welle inkl. GUI-LENSFUN-GATE-1
  (Details siehe Restrisiken); offene Nebenbefunde: Perf-Pooling.
  Erledigt (Teilwelle, 2026-09-15, GEN-ONNX-1 Welle 2a; Verifikation
  BESTANDEN, Re-Verifizierung 2026-09-15): `generative_edit` ist **nicht mehr**
  CPU-geroutet. Der Renderer adoptiert das kompositierte
  `generative_canvas`-Artefakt per Caller-Hook
  (`GpuContext::render_with_gpu_and_generative`); die GPU-Geometrie-Kette fügt
  einen `Substitute`-Schritt an der CPU-Oracle-Position ein
  (`Lens → [auto-fill] → Perspective → CA → [expand] → Crop`) und ist
  byte-identisch (`maxAbsDiff == 0`, siehe
  `generative_canvas_compositing_is_gpu_parity` /
  `generative_auto_fill_compositing_is_gpu_parity`). Ohne Artefakt ist der
  Render laut (kein stiller unexpandierter Render, keine pauschale CPU-Route).
  Der rezept-only VRAM-Pfad (`render_to_vram`) lehnt eine aktive generative
  Stufe weiter laut ab (kein Injektionspunkt ohne Readback). Details in
  `feature/product/generative-expand.md`.
- **Present-Pfad:** `eframe` nutzt jetzt den **wgpu**-Renderer;
  `GpuContext::from_parts` teilt sich Renderer-Device/Queue, sodass die
  VRAM-Vorschau ohne CPU-Readback präsentiert wird (`copy_vram_to_texture`
  → registrierte egui-User-Textur). Der CPU-Fallback (ColorImage-Upload)
  bleibt vollständig erhalten; die kittest-Goldens bleiben unverändert grün.
- **VRAM-Pool:** dimensionsschlüsseltes LRU (Entry-Limit + Bytebudget,
  env-konfigurierbar) ersetzt den Single-Slot.
- **Kein `lumina-core`-API-Bruch:** Core blieb vollständig unverändert; alle
  Erweiterungen sind additiv in `lumina-gpu`/`lumina-gui`.
- **Restrisiken:** (1) Rezepte mit nicht implementierten GPU-Stufen werden
  vom VRAM-Pfad vor jedem Write verweigert (Warnung) — Present fällt dort
  auf den exakten CPU-Pfad zurück, divergente Pixel werden nie geschrieben
  (seit 2026-09-14 inkl. Geometrie: explizites Crop/Rotation/Mirror sowie
  keilloses manuelles Lens/Perspective mit explizitem Crop laufen auf GPU mit
  Oracle-Parität und exakten Output-Maßen; Lens/Perspective OHNE expliziten
  Crop routen seit CROP-MAXRECT-1 laut auf CPU (Reason `geometry (default
  content crop)` — datenabhängiges MaxRect); dimensionändernde Rezepte werden
  vom VRAM-Pfad laut verweigert; bei aktivem Lensfun-Corrector routet das
  GUI-Gate (CLI-Reason-Mirror) laut auf CPU). Erledigt (Teilwelle,
  BESTANDEN, Commit 2026-09-14): LENSFUN-GATE-2 — Badge nennt präzisen Grund
  (`format_routing_fallback_reason`: Headline + [Gründe], inkl. Lensfun-Reason
  und `geometry (default content crop)`; Gründe memoized statt bool);
  Lensfun-Routing an angezeigten Render gebunden (`displayed_lensfun_active`:
  Snapshot in `render_from` vor Zoom/Navigator/Export-Slot-Clobber, Gate liest
  Snapshot, kein Flapping, Reset bei Quellwechsel/Neighbor-Adopt).
  Bekannte Grenze (LENSFUN-GATE-4): `adopt_neighbor_preview_frame` setzt
  `displayed_lensfun_active` zurück, aber nicht `vram_render_refusal`
  (transient veralteter Badge möglich); `vram_render_refusal`-Invalidierung
  (mark_dirty/load_bytes/Erfolg) ohne dedizierten Unit-Test.
  (2) >45-MP-Zoom nutzt weiterhin Volltextur-Pooling statt 512²-Tile-Cache
  (M2); (3) der Present-Pfad ist headless nicht automatisiert testbar und
  braucht den nächsten manuellen GUI-Test (Block C).
- **User-Entscheid 2026-09-12 (volle GPU-Parität, Agents.md
  Änderungsregeln):** Jede Renderstufe muss auch GPU-tauglich sein — nichts
  bleibt CPU-only. Ein CPU-Fallback wegen nicht implementierter GPU-Stufen
  („Render routed to CPU") ist ein Fail mit Fix-Pflicht (Task
  GPU-RENDER-PARITY-1), kein akzeptierter Zustand. Die CPU bleibt daneben
  die vollständige Referenz (kein GPU-only-Weg). Lebendes Inventar der noch
  nicht GPU-tauglichen Rezept-Konfigurationen:
  `cpu_routing_inventory_is_complete` (`crates/lumina-gpu/tests/parity.rs`).
- **GUI-ROUTING-N6 (F-103-N6-Runde 1, 2026-09-17):** Der gelbe Badge im
  manuellen Test wurde reproduziert: Mit den committeten RAW-Fixtures
  (`aircraft-landscape.cr3`, `aircraft-portrait.cr3`) und einem harmlosen
  Basis-Rezept (Exposure/Contrast, Zoom) erschien
  `lens_correction (Lensfun corrector)`. Ursache war **kein** fehlender
  GPU-Pass, sondern eine **lose Lensfun-Profil-Suche** (`LF_SEARCH_LOOSE`):
  Für das nicht in der Datenbank vorhandene Paar `Canon EOS R1` +
  `RF200-800mm F6.3-9 IS USM` hat lensfun ein **fremdes** Objektiv
  (`Canon RF 24-240mm F4-6.3 IS USM`) bzw. eine falsche Kamera (`EOS R`)
  geliefert und damit eine falsche Korrektur samt CPU-Route/Badge erzeugt.
  Fix: `Corrector::for_camera` sucht strikt (kein `LF_SEARCH_LOOSE`); ein
  benannter Kameratyp/Objektivname ohne echten DB-Eintrag ergibt `None` und
  der manuelle Pfad greift (SOLL: „nie ein geratenes Profil"). Damit
  verschwindet der Badge für die Fixtures, und eine reale Fehlkorrektur ist
  behoben. Der dokumentierte Body-/Mount-Fallback für `lens_name = None`
  bleibt erhalten. Tests: `named_lens_matches_exactly_and_reports_flags`,
  `absent_named_lens_is_never_substituted`,
  `absent_camera_model_is_never_fabricated`,
  `absent_lens_name_keeps_body_mount_fallback` (hermetische Fixture-DB).
  **Verbleibende, bewusst dokumentierte CPU-Route:** Ein **korrekt** (strikt)
  gematchter Lensfun-Corrector hat weiterhin **keinen WGSL-Pass**. Lensfun
  liefert beliebige Distortion-/TCA-/Vignette-Modelle als per-Pixel-Koordinaten-
  und Gewinnfunktion; eine GPU-Parität braucht eine vorberechnete Warp-/Gain-Map
  (CPU-Aufbau, GPU-Resample) als eigenes, crate-übergreifendes Slice. Bis dahin
  bleibt die Route laut sichtbar (Badge mit präzisem Grund,
  `active_lensfun_corrector_forces_gpu_fallback_without_stale_memo_hit`) — die
  Ausnahme ist hier festgeschrieben; die GPU-Umsetzung ist als Folgeaufgabe
  `GPU-LENSFUN-PARITY-1` in `Agents.todo.md` getrackt.

### G-01 Develop-Basis: Treatment, Profil, Reset-Automatik, Panel-Previous
(LRPAR-G01-BASIC, Release 1.0)

**Ziel:** Die Lightroom-Basic-Kopfzeile (Treatment, Profil) sowie das
footer-/panel-lokale Zurücksetzverhalten als Rezept-Semantik festlegen —
ohne zweite Renderpipeline, ohne Schema-Bump (alle Felder additiv,
absent = Identität/Default, keine Migration nötig).

**Treatment:** `recipe.extras["treatment"]` trägt `"color"` oder `"bw"`;
absent bedeutet `color` (Default). `"bw"` ist die Schwarz-Weiß-Behandlung:
`saturation`/`vibrance` stehen auf `-1.0` (volle Entsättigung über die
bestehende Pipeline-Stufe, keine GUI-Pixel-Logik); die Vorwerte liegen in
`extras["bw_stash"]` (`{saturation: f64|null, vibrance: f64|null}`,
absent-Key = Key war ungesetzt) und werden beim Zurückschalten exakt
wiederhergestellt (ungesetzte Keys werden entfernt, nie auf `-1`
stehen gelassen). Andere Werte sowie ein korruptes Stash-Format werden
laut abgelehnt (Sidecar-Validierung, CLI-Pre-Check); ein fehlendes Stash
beim Ausschalten fällt laut-warnend auf Identität zurück (beide Keys
entfernt), nie auf stilles Beibehalten von `-1`. `treatment`/`bw_stash`
sind Teil des serialisierten Rezepts und fließen damit in `recipe_hash`/
RenderKey ein (Preview-/Export-Invalidierung). Einziger
Mutationspfad ist `lumina-sidecar::apply_treatment` (CLI + GUI teilen
ihn; der GUI-`V`-Toggle ruft ihn auf).

**Profil:** `recipe.options["profile"]` trägt einen benannten
Entwicklungs-Look aus der Whitelist `default | neutral | vivid |
portrait | landscape | monochrome`; absent bedeutet `default`. Leere
oder unbekannte Namen werden laut abgelehnt (Sidecar-Validierung,
CLI-Pre-Check, GUI-Auswahl nur aus der Liste), niemals still auf
`default` normalisiert. Ehrliche Grenze (MVP): Das Profil ist eine
persistierte, roundtrippte Auswahlabsicht und Teil von `recipe_hash`
(Wechsel invalidiert Preview/Export), hat aber noch keine eigene
colorimetrische Renderwirkung — alle bekannten Profile rendern
identisch; die echte Profilanwendung braucht den linearen
ProPhoto-Pfad (reserviert, s. Arbeitsfarbraum) und bleibt Folgearbeit.
Absolute Pfade sind als Profilwert verboten (portables Sidecar).

**Original-Photo-Referenz:** Das Referenz-Histogramm des unbearbeiteten
Originals ist der Original-Decode (`LuminaApp.original`), gemessen mit
demselben `analyze_tone`/`LuminanceHistogram`-Pfad wie die
Before/After-Ansicht — kein zweiter Analysepfad, kein Rezept-Reset
(Reset würde „unbearbeitet" mit „Default-Rezept" verwechseln; Demosaic/
Decode bleiben auch beim Reset aktiv). Die GUI hält beide Seiten
gleichzeitig vor (`histogram_compare_data()`: Original- + Edit-Histogramm
aus dem Full-Frame-Render, nie Viewport/ROI) und zeigt bei aktivem
Vergleich das Delta (Δ Mean + normierte L1-Distanz der 256 Bins, echte
Analysewerte, kein `(0,0)`-Fallback). Der Vergleichsschalter ist reiner
Session-Display-State (nie Rezept/Sidecar). Determinismus: Zwei Messungen
desselben Decodes sind byte-identisch (headless-testbar).

**Reset Sliders Automatically:** Ordner-vererbte Einstellung
`.lumina/settings.json` (`reset_sliders_automatically`, Default `false`,
siehe `FolderCacheSettings`); kein Rezept-/Sidecar-Feld (Edit-Verhalten,
kein Bildzustand). Semantik: AUS (Default) = Bildwechsel flusht
anstehende, noch nicht committete Regler-Edits ins Sidecar des alten
Bildes (bisheriges Verhalten, kein Edit-Verlust); AN = Bildwechsel
verwirft den anstehenden Commit (Regler werden automatisch
zurückgesetzt), Persistiertes bleibt unberührt. Die Umschaltung loggt
`info!`.

**Panel-Previous/Reset:** Jede der acht Develop-Sektionen (F-100-Reihenfolge)
besitzt einen Sektions-Baseline-Snapshot (Rezept-Ausschnitt + Masken-Layer
der aktiven Kopie), aufgenommen beim Bild-Load und nach jedem
erfolgreichen Save. **Previous** stellt die Sektions-Felder aus der
Baseline wieder her (Panel-lokales Undo auf den gespeicherten Stand, kein
Mehrbild-Previous — das ist LRPAR-G08-PREVIOUS); **Reset** setzt die
Sektions-Felder auf ihre dokumentierten Defaults. Beide laufen über den
normalen Save/Render-Commit (History-Eintrag, `preview_generation`-Bump,
`info!`-Log). Sektions-Feldzuordnung (disjunkt, keine Überlappung):
Basic = `wb_temperature/wb_tint/exposure/contrast/highlights/shadows/
whites/blacks` + Treatment/Stash + Profil; Tone Curve = `curves`;
Color = `hsl/color_grading/presence` + `vibrance/saturation`;
Detail = `sharpening/noise_reduction`; Effects = `effects`;
Optics = `lens_correction/lens_blur`; Geometry = `geometry/perspective`;
Masking = `mask_layers` der aktiven Kopie (Reset = Layer der aktiven
Kopie entfernen — explizite Nutzeraktion, laut persistiert).

**Abnahme:** JSON-Roundtrip Treatment/Profil, Validierungsablehnung
(unbekannt/leer/korrupt), CLI-Roundtrip mit Exit-Codes, GUI-headless
(Setter → Datei → Reload) je Basic-Feld, Panel-Previous/Reset je Sektion
(Mapping-Test alle 8 + E2E Basic), Determinismus der Referenzmessung,
`cargo test`/`clippy`/`fmt` grün.

## Abnahme

- CPU und CLI liefern für identische Eingaben reproduzierbare Ergebnisse mit
  dokumentierten Toleranzen.
- Render-Keys unterscheiden alle relevanten Eingaben (inklusive
  Arbeitsfarbraum und `output_profile`).
- Cache-Hit, Cache-Miss und gezielte Invalidierung sind getestet.
- Auto-Tone und Exposure Matching besitzen Unit-, Property- und
  Referenzbildtests.
- Die implementierte Stufenreihenfolge und die Formatverträglichkeit sind über
  `Pipeline::validate()` abgesichert.
