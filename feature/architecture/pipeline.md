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
ohne Kontext. Verbleibende Grenze: Die Auto-WB-Nutzung des Kontexts folgt mit
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

> **Hinweis GenerativeEdit / Spot-Remove (GEN-EXPAND-1 / SPOT-REMOVE-1, Doku-first, 2026-09-02):**
> Die generative Stufe `GenerativeEdit` (siehe `feature/product/generative-expand.md`) und die Spot-Stufe
> `SpotHeal` (siehe `feature/product/spot-removal.md`) sind derzeit **nur dokumentiert, nicht implementiert**
> (kein Code, `Pipeline::default()` unverändert). Ihre normative Ziel-Reihenfolge ist:
> `Decode → SourceActions → SpotHeal(quick/generative) → LensCorrection(F-098) → GenerativeEdit(auto-fill) → Perspective(F-099) → GenerativeEdit(expand) → Crop(F-093) → Output`,
> vereinfacht `Lens → GenerativeEdit → Perspective → Crop` bzw. `SpotHeal → Lens → Perspective → Crop`.
> Auto-Fill Transparent liegt **nach** Lens, manueller Expand (`expand_beyond_image`) **vor** Crop (nach Perspective);
> `keep_generative_content` steuert, ob Crop das generative Canvas materialisiert. Spot-Generativ (`kind = "spot_heal_generative"`)
> ist von `generative_canvas` getrennt (eigene Capability `inpaint`/`inpaint_heal`). Bis zur Implementierung bleibt die
> MVP-Reihenfolge oben verbindlich; ein Widerspruch wird durch dieses Dokument zugunsten der dokumentierten Ziel-Reihenfolge aufgelöst.

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
und Cache-Miss werden getestet. MVP-Grenze: keine parametrische Kurve und keine
lineare-Farbraum-Variante; spätere Pipelineversionen dürfen diese nur migriert
einführen. Abhängigkeiten: F-029, F-031, F-036, F-039 und F-041.

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
Zusammenwirkung: Auto-Tone → Kurve → HSL → Color Grading.

`recipe_hash` berücksichtigt alle Werte und `version`; Änderungen invalidieren
Preview/Export ab Color Grading, nicht Decode/Maskenartefakte. Abnahme:
zyklische Hue-Werte, weiche Übergänge, Balance-Richtung, Identität bei
Sättigung 0 und reproduzierbarer Cache-Miss. MVP-Grenzen: sRGB-HSL statt
linearem oder perceptuellem Farbmodell, keine getrennte Blending-Methodik.
Abhängigkeiten: F-031, F-036, F-039.

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

Die Unterstufenreihenfolge ist `vibrance → saturation`, nach HSL und vor Color
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
RenderKey-Trennung und F-041-Messbereich. MVP-Grenze: kein Auto-Crop; abhängig
von F-029, F-039, F-041, F-098 und F-099.

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
Determinismus und Schärfen-Reihenfolge. KI-Denoise ist nur eine optionale
spätere Erweiterung und wird hier nicht spezifiziert. Abhängigkeiten: F-031,
F-036.

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

**Lensfun-Integration (MVP):** Zusätzlich zum manuellen Modell wird die
Lensfun-Datenbank (Kamera-/Objektiv-Profile, CC-BY-SA) zur automatischen
Korrektur genutzt, wenn ein passendes Profil für die aus den RAW-Metadaten
(`camera_make`, `camera_model`, ggf. Objektivname) ermittelte Kamera/Objektiv
gefunden wird. Lensfun liefert Geometrie-, Vignette- und CA-Korrektur und
ersetzt das manuelle Modell, sofern ein Profil vorliegt (Priorität: Lensfun >
manuelle Koeffizienten > Identität); sonst greift das manuelle Modell
(graceful fallback).

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
F-074-Folgeaufgabe), CA bewusst manuell, DB-Ladezyklus pro Render-Aufruf
(MVP ok), Distance-Default 10,0 m (RawMetadata hat kein Distanzfeld),
`lens_name` wird nicht befüllt (Body-Match statt falschem Lens-Match).
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
die automatische Analyse stürzender Linien ist ausdrücklich Post-MVP. Alle
Parameter, Homographie-/Pipelineversion und resultierenden Dimensionen
gehören zum RenderKey; Geometrieänderungen invalidieren Preview/Export, nicht
Decode oder AI-Artefakte. Abnahme: Identität, Eckpunktprojektion,
Parametergrenzen, Zusammenspiel mit Objektivkorrektur/Crop und Cache.
Abhängigkeiten: F-029, F-031, F-041, F-098.

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
- **Present-Pfad:** `eframe` nutzt jetzt den **wgpu**-Renderer;
  `GpuContext::from_parts` teilt sich Renderer-Device/Queue, sodass die
  VRAM-Vorschau ohne CPU-Readback präsentiert wird (`copy_vram_to_texture`
  → registrierte egui-User-Textur). Der CPU-Fallback (ColorImage-Upload)
  bleibt vollständig erhalten; die kittest-Goldens bleiben unverändert grün.
- **VRAM-Pool:** dimensionsschlüsseltes LRU (Entry-Limit + Bytebudget,
  env-konfigurierbar) ersetzt den Single-Slot.
- **Kein `lumina-core`-API-Bruch:** Core blieb vollständig unverändert; alle
  Erweiterungen sind additiv in `lumina-gpu`/`lumina-gui`.
- **Restrisiken:** (1) Rezepte mit GPU-unterstützten Stufen rendern in der
  interaktiven Drag-Vorschau weiter tone-only (mit Warnung) — Present bleibt
  dort bewusst auf dem exakten CPU-Pfad; (2) >45-MP-Zoom nutzt weiterhin
  Volltextur-Pooling statt 512²-Tile-Cache (M2); (3) der Present-Pfad ist
  headless nicht automatisiert testbar und braucht den nächsten manuellen
  GUI-Test (Block C).

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
