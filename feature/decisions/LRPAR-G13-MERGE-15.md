# HDR-/Panorama-Merge (LRPAR-G13-MERGE-15, G-13, Release 1.5, Doku-first)

**Task-ID:** LRPAR-G13-MERGE-15 · **Goal:** G-13 HDR- & Panorama-Merge ·
**Release:** 1.5 · **Stand:** Doku-first (Entscheid, kein Code) ·
**Quelle:** `.goal/Goal.md` G-13 („Belichtungsreihen (`Cmd/Ctrl+H`) +
Panoramen (`Cmd/Ctrl+M`) → neues DNG", Ist ~0 %)

Dieses Dokument ist der normative Entscheid für den HDR-/Panorama-Merge.
Es legt Pipeline-Einordnung, DNG-Schreibung, Ausrichtungs-Scope,
Rezept-Stufen-Vorschlag, Persistenz (Sidecar-first), Capability-Einordnung,
Lizenz/Dependencies, MVP-Abgrenzung, Folge-Tasks und Abnahmekriterien fest.
Die Implementierung beginnt erst nach Freigabe dieses Entscheids; bis dahin
wird **kein Crate-Code** angelegt oder geändert.

## Inhaltsverzeichnis

- [Ziel und Abgrenzung](#ziel-und-abgrenzung)
- [Ist-Stand](#ist-stand)
- [Normative Invarianten](#normative-invarianten)
- [Pipeline-Einordnung](#pipeline-einordnung)
- [DNG-Schreibung (Scope)](#dng-schreibung-scope)
- [Ausrichtung / Alignment (Scope)](#ausrichtung--alignment-scope)
- [Rezept-Stufen-Vorschlag](#rezept-stufen-vorschlag)
- [Persistenz (Sidecar-first)](#persistenz-sidecar-first)
- [Capability-Matrix-Einordnung](#capability-matrix-einordnung)
- [Lizenz und Dependencies](#lizenz-und-dependencies)
- [Abgrenzung MVP (nicht 1.0)](#abgrenzung-mvp-nicht-10)
- [Folge-Implementierungstasks (Vorschlag)](#folge-implementierungstasks-vorschlag)
- [Abnahmekriterien (CLI + GUI-headless + Golden-Gates)](#abnahmekriterien-cli--gui-headless--golden-gates)
- [Offene Risiken](#offene-risiken)

## Ziel und Abgrenzung

G-13 beschreibt zwei zusammengehörige, aber getrennte Merge-Pfade mit
Lightroom-ähnlicher Anmutung:

1. **HDR-Merge (`Cmd/Ctrl+H`):** Eine Belichtungsreihe desselben Motivs
   (gleiche Kamera, unterschiedliche Belichtung) wird zu **einem neuen DNG**
   zusammengeführt (erweiterter Dynamikumfang, lineare Szene-daten).
2. **Panorama-Merge (`Cmd/Ctrl+M`):** Überlappende Einzelbilder werden
   ausgerichtet und zu **einem neuen DNG** zusammengesetzt (erweitertes
   Sichtfeld).

Beide Pfade erzeugen ein **neues, eigenständiges Quellbild** (DNG-Datei
neben den Originalen), das danach wie jedes andere RAW/DNG importiert,
mit Rezept bearbeitet und exportiert wird. Das Merge-Ergebnis ist kein
Rezept-Overlay über den Quellen, sondern ein ableitbares Artefakt aus
Quellen + Merge-Rezept + Versionen.

Nicht-Ziele (explizit außerhalb von G-13):

- HDR-Tonemapping als Regler (das ist Develop-Arbeit, F-036/F-089ff.).
- Fokus-Stacking, Super-Resolution, Median-Stacking (kein Task).
- Cloud-Merge oder proprietäre Panorama-Dienste.
- Karten-Modul/GPS und Veröffentlichungsdienste (nie Ziel, s. Releaseplan).

## Ist-Stand

**Stand 2026-09-04 (dieser Entscheid):** Doku-first, kein Code.
Repo-weit gilt: kein Merge-Pfad in `lumina-core`/`lumina-cli`/`lumina-gui`
(G-13 ~0 % laut `.goal/Goal.md`). Es gibt keine DNG-Schreibung, keine
Ausrichtungsstufe und kein Merge-Rezept im Sidecar-Schema. Dieses Dokument
ist das normative SOLL für die spätere Umsetzung.

**Stand 2026-09-16 (MERGE-CORE-1-Rework + MERGE-DNG-1, Verifizierung BESTANDEN):**
`merge_hdr_weighted` linear (Ebenen einmalig), `pano_matrix` zentrumskorrekt,
`blend_panorama_transformed` rotationsfähig (neu, noch ungenutzt — Interim),
y-Überlapp-Gate, Subpixel-/64000-Testanker. **MERGE-CLI-1 (2026-09-16,
Verifizierung BESTANDEN):** CLI nutzt die volle Matrix
(`blend_panorama_transformed`, Rotation wirkt end-to-end, rotierter
E2E-Test), Envelope-`\"type\": \"merge\"`-Diskriminator, HDR-Residual-Warnung.
**Stand 2026-09-17 (MERGE-IMPL-15, F6-Dedup + F-074-N7, Verifizierung
ausstehend):** Die früher in `lumina-cli/src/merge.rs` und
`lumina-gui/src/merge_gui.rs` gespiegelte Orchestrierung ist in
`lumina-merge::bundle` zusammengeführt: `run_merge` ist der eine Pfad
(Validierung → Decode-Adapter → Exposure-Policy → Alignment → Blend → Rezept →
Digest → Envelope-/Stale-/Missing-Gate → linearer DNG → atomare
DNG+Sidecar-Publikation), den CLI und GUI aufrufen; die Frontends liefern nur
noch Decode-Adapter und Exposure-Policy sowie ihre Oberfläche (CLI-Exit-Codes/
JSON, GUI-Jobsteuerung via Poll/Status). Zusätzlich: Merge-Benchmark-Klasse
`merge/*` (F-074-N7) mit Baseline/Budget in den Stores (`gate: false`,
report-only) und Re-Baseline aller messbaren Core-/Batch-/GPU-IDs
(`feature/quality/performance-benchmarks.md`).

**Offen (explizit):** unabhängige Verifizierung der F6-Dedup + F-074-N7;
Golden-Gates unverändert (Hasher/Re-Import-Anker).

## Normative Invarianten

Für G-13 gelten die Produktprinzipien und Persistenzregeln aus `Agents.md`
unverändert und werden hier für den Merge konkretisiert:

- **Originale unverändert:** Keine Quelldatei wird überschrieben,
  verschoben oder durch das Merge-DNG ersetzt. Das Merge-DNG ist eine neue
  Datei; Exporte schreiben weitere neue Dateien.
- **Sidecar ist Quelle der Wahrheit:** Merge-Rezept und Verweise auf
  Quellen/Artefakte leben in Sidecars. Eine optionale Index-DB darf sie nur
  spiegeln und muss aus Sidecars rekonstruierbar sein.
- **Deklarativ, versioniert, reproduzierbar:** Gleicher Quellsatz +
  gleiches Merge-Rezept + gleiche Versionen → byte-identisches DNG
  (deterministisch, dokumentierte Toleranz nur dort, wo Fließkomma-
  Zusammenführung unvermeidbar ist; dann Golden mit Toleranz statt
  Byte-Identität, Toleranz im Folge-Task begründet).
- **Kein stiller Fallback:** Fehlende Quellen, inkompatible Geometrie/
  Belichtung, fehlende DNG-Schreibfähigkeit oder veralteter Kontext werden
  sichtbar als `missing`/`stale`/`unsupported` gemeldet. Es gibt keine
  stille Einzelbild-Verwendung („als wäre kein Merge gewollt"), kein
  stilles Herunterskalieren und keine stille Re-Generierung als einzige
  Option.
- **Relative Pfade:** Quellenverweise im Merge-Sidecar sind relativ zum
  Sidecar-Bundle; absolute Pfade sind verboten. Ein verschobenes Bundle
  (Quellen + Merge-DNG + Sidecars) bleibt gültig, solange die relative
  Struktur erhalten ist.
- **Atomarer Write:** Merge-DNG- und Sidecar-Schreibvorgänge erfolgen atomar
  (temporäre Datei + Rename). Unvollständige Dateien gelten nie als gültig.

## Pipeline-Einordnung

Der Merge ist **keine Stufe der Einzelbild-Renderpipeline**
(`Decode → SourceActions → AutoAnalysis → Adjustments → Masks → Crop →
Output`, normativ in `feature/architecture/pipeline.md`). Er ist ein
**Mehrbild-Vorlauf**, der vor der Einzelbild-Pipeline liegt:

```text
Quellen (N RAWs, je eigenes Sidecar/Rezept lesbar)
  → Decode je Quelle (gleicher Decoder, dokumentierter Decode-Kontext)
  → Alignment (HDR: Translation-Ausgleich; Panorama: Homographie, s. u.)
  → Zusammenführung (HDR: gewichtetes lineares Merge; Panorama: Blend)
  → DNG-Schreibung (neue Datei, linear, s. u.)
  → neues DNG als eigenständige Quelle → normale Einzelbild-Pipeline
```

Konsequenzen:

- `Pipeline::default()` bleibt unverändert; kein Merge-Code in
  `apply_recipe`-Pfaden. Der Merge lebt als eigenes Modul (vorgeschlagen:
  neues Crate `lumina-merge`, s. Folge-Tasks), das `lumina-raw`
  (Decode), `lumina-core` (Geometrie-/Blend-Primitive, soweit vorhanden)
  und `lumina-sidecar` (Schema) konsumiert, aber keine zweite
  Renderpipeline implementiert.
- `lumina-core` bleibt plattformneutral (keine Dateisystem-/DNG-IO im
  Core; IO liegt in `lumina-merge`/`lumina-cli`).
- Rezeptversion und Pipelineversion werden getrennt validiert (Agents.md,
  Architekturgrenzen); die Merge-Stufe erhält eine eigene Version
  (`merge_version`, s. Rezept-Vorschlag).
- Shortcuts `Cmd/Ctrl+H` (HDR) und `Cmd/Ctrl+M` (Panorama) sind GUI-Aktionen,
  die denselben Merge-Einstiegspunkt aufrufen wie die CLI-Commands
  (keine GUI-eigene Bildlogik außerhalb der gemeinsamen Pipeline/des
  gemeinsamen Merge-Moduls).

## DNG-Schreibung (Scope)

Das Merge-Ergebnis wird als **lineares DNG** geschrieben (neue Datei neben
den Quellen, Dateiname deterministisch aus Quellnamen + Suffix, z. B.
`<basis>-HDR.dng` / `<basis>-Pano.dng`; exakte Namensregel legt der
Folge-Task fest):

- **Umfang MVP (1.5):** Lineares DNG (demosaizierte lineare RGB-Daten,
  16 Bit), notwendige DNG-Tags für Import als Quelle (Dimensionen,
  Farbraum-Tag linear, Belichtungs-/Weißabgleich-Basis soweit für den
  Re-Import nötig), EXIF-Übernahme der Referenzquelle (Kamera/Objektiv/
  Aufnahmezeitpunkt der ersten Quelle; Merge-Provenienz zusätzlich im
  Merge-Sidecar, nicht per proprietärem EXIF-Hack).
- **Außerhalb 1.5:** Mosaik-(CFA-)DNG-Schreibung, verlustfreie
  Original-RAW-Rekompression, proprietäre MakerNotes-Rekonstruktion,
  XMP-Einbettung (XMP ist in v1 nicht unterstützt).
- **Decoder-Rückweg:** Das geschriebene DNG muss über den bestehenden
  RAW-Decoder (`lumina-raw`/LibRaw) als Quelle lesbar sein; andernfalls ist
  der Merge-Pfad `unsupported` (lauter Fehler, kein stiller Fallback auf
  TIFF/PNG als „DNG-Ersatz").
- **Abhängigkeit:** DNG-Schreibung erfordert eine dokumentierte
  Schreibbibliothek (z. B. reines Rust-DNG/TIFF-Schreiben ohne native
  Zusatzabhängigkeit bevorzugt; Alternative mit Begründung). Keine neue
  native Dependency ohne Capability-Entscheid und Lizenzprüfung (s. unten).
  Bis zur Entscheidung ist der DNG-Writer der größte technische Risikopunkt.

### MERGE-DNG-1 Konkretisierung (2026-09-05, normativ für die Umsetzung)

- **Writer-Auswahl:** `tiff` 0.11.3 (reines Rust, MIT, exakter Pin `=0.11.3`,
  Upgrade nur per ADR). Begründung: DNG ist TIFF/EP-basiert; die Crate schreibt
  alle benötigten Tags (Basis-Tags, DNG-Private-Tags 50706ff., EXIF-Sub-IFD via
  unverketteter Extra-Directory) ohne nativen Code — kein LGPL-/Capability-
  Risiko, kein neues Capability-Gate in `feature/platform/capability-matrix.md`
  (`lumina-merge` bleibt pure-Rust, nativ immer verfügbar), kein neuer
  THIRD-PARTY-NOTICES-Eintrag (`tiff` 0.11.3 MIT ist bereits inventarisiert).
  Keine native Alternative evaluiert, weil der Re-Import-Beleg (s. u.) mit
  purem Rust gelingt. Größter Risikopunkt damit geschlossen.
- **Schreibumfang:** lineares 16-Bit-DNG, unkomprimiert (Compression = 1),
  PhotometricInterpretation = LinearRaw (34892), Chunky, SampleFormat UINT,
  ein Strip. DNG-Tags: DNGVersion/DNGBackwardVersion 1.4.0.0, UniqueCameraModel
  `LuminaMerge HDR` bzw. `LuminaMerge Pano`, CalibrationIlluminant1 D65 (21),
  ColorMatrix1 (sRGB-D65→XYZ-D50, Bradford, Nenner 10⁷, im Code dokumentiert),
  AsShotNeutral 1:1:1, WhiteLevel 65535. EXIF-Übernahme aus der Referenzquelle
  (erste Quelle): Make/Model (Top-Level-Tags), ExposureTime/FNumber/
  ISOSpeedRatings/DateTimeOriginal (UTC, `YYYY:MM:DD HH:MM:SS`)/LensModel
  (EXIF-Sub-IFD); fehlende Referenzfelder → Tag entfällt (kein Raten, kein
  Default). Merge-Provenienz ausschließlich im Merge-Sidecar, kein
  proprietärer EXIF-Hack. Orientation wird als 1 geschrieben (Merge-Frames
  sind bereits orientiert).
- **Wertabbildung:** `u16 = round(clamp(radiance / white_scale, 0, 1) * 65535)`
  mit `white_scale = max(1.0, Spitzen-Radiance)` (deterministisch aus den
  Pixeln; kein stilles Lichter-Clippen, kein BaselineExposure-Tag — der
  LibRaw-Decode bleibt vorhersagbar). Quantisierungsfehler ≤ 0,5/65535.
- **LibRaw-Grenzen (belegt an LibRaw 0.22.2, `src/metadata/identify.cpp`):**
  Breite und Höhe je ≥ 22 px (`!load_raw || height < 22 || width < 22` →
  `is_raw = 0`), je ≤ 64000 px; unkomprimierte Integer-DNGs dekodiert LibRaw
  via `packed_dng_load_raw`. Kleinere/größere Frames lehnt der Writer laut als
  `unsupported` ab. Baseline-TIFFs ohne DNG-Tags lehnt LibRaw ab — daher sind
  die DNG-Tags Pflicht, kein TIFF-Ersatz.
- **Dateiname:** `<Basis>-HDR.dng` / `<Basis>-Pano.dng`; Basis = Dateistemm
  (ohne letzte Extension) der Referenzquelle (ersten Quelle). Laut abgelehnt
  statt umgedeutet: leere Basis, Pfad-Trennzeichen (`/`/`\`), Laufwerks-`:`,
  absolute Pfade, `..`-Segmente.
- **Atomarität:** Temp-Datei im Zielverzeichnis + Rename; unvollständige
  Dateien gelten nie als gültig. Alle Pfade relativ, absolute Pfade verboten.
- **Re-Import:** Das DNG muss via `lumina-raw` (gepinntes LibRaw 0.22.2) als
  Quelle dekodieren (Geometrie, Make/Model, EXIF-Belichtung lesbar); andernfalls
  ist der Pfad `unsupported`. Der Decode-Kontext (`libraw_decode_version`,
  Weißabgleich-/Farb-Pipeline von `decode_file`) gehört zur Test-Doku, nicht
  in die DNG-Bytes.
- **`"type": "merge"`-Envelope (Entscheid):** Der Diskriminator gehört auf
  Dokument-Ebene des Merge-DNG-Sidecars, nicht in `MergeRecipe` (dort bewusst
  kein Feld — ein `"type"`-Schlüssel im Rezept-JSON wird per
  `deny_unknown_fields` laut abgelehnt, s. MERGE-SCHEMA-1). Regel: fehlendes
  `type` → Standard-Sidecar; `"type": "merge"` ohne valides `merge_recipe` →
  laut ablehnen; unbekanntes `type` → laut ablehnen. Das Merge-Sidecar ist ein
  volles Sidecar-Dokument (Standardkopie mit eigenem Rezept) plus Pflichtfeld
  `merge_recipe` und DNG-Artefaktverweis (relativ, BLAKE3, Auflösung, Kanaltyp,
  Datenversion). Umsetzung (Schema-Code) in MERGE-CLI-1; dieser Entscheid ist
  Doku-only.

## Ausrichtung / Alignment (Scope)

- **HDR (1.5-Scope):** Translation-Ausgleich (ganzzahlig + subpixel light)
  gegen Geisterbilder-Artefakte minimieren; bewusst **kein** volles
  Ghost-Removal und **keine** Homographie. Bei Überschreitung einer
  dokumentierten Verschiebungsschwelle → sichtbare Warnung (`aligned_with_
  residual`, Pixelmaß im Merge-Status), kein stiller Abbruch und kein
  stilles Croppen ohne Meldung.
- **Panorama (1.5-Scope):** dichte intensitätsbasierte SAD-Suche über
  ganzzahlige Translation + Rotation-light (±2°, um das Frame-Zentrum),
  zylindrische Projektion als dokumentierte Grenze (nur zylindrisch, keine
  sphärische/fisheye-Projektion in 1.5), lineares Blending im Überlapp
  (Federung, keine Mehrband-Blending in 1.5). Nicht überlappende oder
  nicht verkettbare Bildsätze → harter `unsupported`-Fehler, kein
  Teil-Panorama als stilles Ergebnis.
- **Gemeinsam:** Alignment-Parameter (Transformationen je Quelle,
  Projektion, Blend-Breite) sind Teil des Merge-Rezepts und damit
  reproduzierbar. Belichtungsanpassung vor dem Blend erfolgt ausschließlich
  aus EXIF-Belichtungswerten (`exposure_time`, `iso`, `f_number`); fehlende
  EXIF-Belichtung → HDR-Merge `unsupported` (kein Raten aus Pixeln als
  stiller Ersatz). Geometrie-Kontext (Orientierung/Decode-Geometrie je
  Quelle) gehört zur Merge-Identität (s. Persistenz).

## Rezept-Stufen-Vorschlag

Neues, additives Merge-Rezept (Version 1, eigene `merge_version`,
getrennt von `recipe_schema_version`/`pipeline_version`). Vorschlag für
den Folge-Task (Schema-Entscheid vor Code, Agents.md Änderungsregeln):

```json
{
  "type": "merge",
  "merge_version": 1,
  "mode": "hdr | panorama",
  "sources": [
    {
      "path": "IMG_0001.ARW",
      "content_hash": "blake3:<hex>",
      "decode_context": { "decoder": "libraw", "decode_version": "<libraw>+luminaabiN", "orientation": 1 },
      "exposure": { "exposure_time_s": 0.01, "iso": 100, "f_number": 8.0 }
    }
  ],
  "alignment": {
    "method": "hdr_translate | pano_cylindrical_homography",
    "transforms": [{ "source_index": 1, "matrix_3x3": [1,0,0, 0,1,0, 0,0,1] }],
    "residual_px": 0.4,
    "projection": "none | cylindrical",
    "blend_width_px": 64
  },
  "output": { "file": "IMG_0001-HDR.dng", "bits": 16, "mosaic": false },
  "created_at": "<rfc3339>",
  "status": "ok | stale | missing | unsupported",
  "error": "<optionaler Fehlertext>"
}
```

Regeln:

- `sources[].path` relativ zum Merge-Sidecar; `content_hash` je Quelle
  (BLAKE3-Vertrag wie AI-Masken-Quell-Hash).
- Jede Änderung (Quellsatz, Alignment, Modus) ändert den Merge-Digest →
  Merge-DNG gilt als `stale` und wird nur auf ausdrückliche Anforderung neu
  erzeugt (keine automatische Neuberechnung als einzige Option).
- Validierung: unbekannte Modi/Methoden und unendliche/Out-of-Range-Werte
  werden abgelehnt, nicht geclippt (konsistent mit pipeline.md-Clipping).
- Das Merge-Rezept gehört zur **neuen** Quelle (Merge-DNG-Sidecar trägt die
  Provenienz), nicht als Mutation in die Quell-Sidecars.

### MERGE-SCHEMA-1 Konkretisierung (2026-09-05, normativ für die Umsetzung)

Der Schema-Folge-Task (`lumina-sidecar`, Modul `merge_recipe`) legt fest:

- `merge_version` ist auf 1 gepinnt; fremde Versionen werden laut
  abgelehnt (Pre-MVP: keine Abwärtskompatibilitätspflicht, aber versioniert).
- Quellen: mindestens 2, höchstens 256 (`MIN_`/`MAX_MERGE_SOURCES`).
- `content_hash` je Quelle im Vertrag `blake3:<64 lowercase hex>`.
- `decode_context.orientation` ist 1..=8; `exposure` verlangt endliches
  `exposure_time_s` in (0, 86400 s], `iso` in 1..=10 000 000,
  endliches `f_number` in (0, 256].
- `residual_px` ist endlich in 0..=100 000; `blend_width_px` ist
  0..=65 536; jede `matrix_3x3`-Komponente ist endlich.
  `transforms[].source_index` liegt unter der Quellenzahl und ist eindeutig.
- 1.5-Scope-Paarung (laut abgelehnt statt umgedeutet): `hdr` verlangt
  `hdr_translate` + `none`; `panorama` verlangt
  `pano_cylindrical_homography` + `cylindrical`.
- `output` ist 1.5-Scope linear 16 Bit nicht-mosaik (`bits` = 16,
  `mosaic` = false); `file` ist relativ wie jede Quelle.
- `created_at` folgt dem Sidecar-RFC-3339-UTC-Vertrag; `status` `ok`
  trägt keinen Fehlertext, sonst ist `error` optional (nicht-leer).
- Digest: BLAKE3 über kanonischem JSON (schlüsselsortiert) aus
  `merge_version` + `mode` + `sources` + `alignment`, gerendert als
  `blake3:<hex>`; `output`/`created_at`/`status`/`error` sind
  Artefakt-Metadaten und kein Teil der Merge-Identität.
- Der `"type": "merge"`-Schlüssel des Skizzen-JSON ist kein Feld des
  Rezept-Typs: die Envelope-Unterscheidung gehört zum einbettenden
  Sidecar-Dokument (Entscheid in MERGE-DNG-1).

## Persistenz (Sidecar-first)

- **Autoritativ:** Das Merge-DNG erhält ein eigenes Sidecar-Bundle
  (`<merge-basis>.dng.lumina.json` + ggf. `.lumina.zdata`-Verweis);
  darin liegt das Merge-Rezept (Provenienz: Quell-Hashes, Decode-Kontexte,
  Alignment, DNG-Prüfsumme). Die Quell-Sidecars werden **nicht** verändert.
- **Virtuelle Kopien:** Das Merge-DNG erhält mindestens die
  Standardkopie mit eigenem vollständigem Rezept (Agents.md-Regel);
  Merge-Provenienz liegt auf Quellbild-Ebene und darf kopienübergreifend
  geteilt werden, lokale Anpassungen gehören zur jeweiligen Kopie.
- **Artefaktverweise:** DNG-Verweis mit relativem Pfad, Format, Prüfsumme
  (BLAKE3), Auflösung, Kanaltyp, Datenversion (analog Masken-Artefakt-
  Metadaten, Agents.md). Absolute Pfade verboten.
- **Veraltung:** Merge-DNG ist gültig, wenn Quell-Hashes, Decode-Kontexte
  und DNG-Prüfsumme übereinstimmen. Bei Abweichung → Status `stale`
  (sichtbar in CLI/GUI), Re-Merge nur auf Anforderung. Fehlendes DNG oder
  fehlende Quelle → `missing`/`unsupported`, lauter Fehler.
- **Atomarität:** DNG + Sidecar werden atomar geschrieben (Temp + Rename);
  unvollständige Dateien gelten nie als gültig. Index-DB (falls vorhanden)
  spiegelt nur und ist aus Sidecars rekonstruierbar.

## Capability-Matrix-Einordnung

(vgl. `feature/platform/capability-matrix.md`; WASM/Browser entfallen —
nur native CLI/Desktop.)

| Fähigkeit | native CLI | Desktop (eframe) |
| --- | --- | --- |
| HDR-Merge → lineares DNG | geplant 1.5 (`lumina-merge` + DNG-Writer) | geplant 1.5 (gleicher Einstiegspunkt, Jobsteuerung in GUI) |
| Panorama-Merge → lineares DNG | geplant 1.5 (gleicher Scope wie oben) | geplant 1.5 |
| Alignment (HDR-Translation / Pano-Homographie) | geplant, CPU, `lumina-merge` | geplant, gleiche Logik (keine GPU-Pflicht in 1.5) |
| DNG-Re-Import als Quelle | via bestehendem RAW-Decoder (muss verifiziert werden) | dto. |
| Cloud-Merge / proprietäre Dienste | nicht geplant | nicht geplant |

Kein stiller Fallback zwischen CLI und GUI: Beide durchlaufen dieselbe
Merge-Schrittfolge (Decode-Adapter → Exposure-Policy → Alignment/Merge →
Rezept → Digest → Bundle-Gate → Encode → Stage → Sidecar → Publish) — seit
MERGE-IMPL-15 (2026-09-17, F6) in **einem** gemeinsamen Pfad
`lumina_merge::bundle::run_merge`; fehlende Capability (z. B. DNG-Writer nicht
verfügbar) ist auf beiden Pfaden derselbe harte Fehler. Die Fehlertexte nutzen
beide dieselben kanonischen Präfixe (`merge missing:` / `merge stale:` /
`merge unsupported:` / `merge failed:`) mit neutralem Hinweis „regenerate the
bundle explicitly with force"; die CLI-Exit-Codes (0 Erfolg/bereits aktuell, 1
lauter Fehler, 2 clap) und ihr `--json`-Report bleiben unverändert. Der frühere F6-Befund
(gespiegelte Orchestrierung mit Drift: CLI-`--output` + `StagedArtifact` vs.
GUI-`{pid}.tmp`) ist damit geschlossen: die CLI- und GUI-Module enthalten nur
noch ihren Decode-Adapter und ihre Exposure-Policy; DNG-Pfadauflösung,
Rezept/Digest, Envelope-Gate und atomare Publikation liegen genau einmal im
gemeinsamen Modul. Verbleibende (bewusste, kleine) Frontend-Duplikation ist
allein der Decode-Aufruf (`decode_input` vs. `decode_selection_frame`) und die
Flag-/`Option`-Exposure-Policy; ein Paritätstest (`lumina-cli/tests/
merge_parity.rs`) belegt denselben Digest/Checksum-Anker für denselben
Raster-Input über beide Verträge.

## Lizenz und Dependencies

- **DNG-SDK/TIFF-Writer:** Vor Integration prüfen und dokumentieren
  (F-078-Verfahren, `feature/quality/fixtures-licensing.md` +
  `THIRD-PARTY-NOTICES.md`). Präferenz: reines Rust ohne native
  Zusatzabhängigkeit (kein LGPL-Risiko, kein Capability-Gate). Falls eine
  native Bibliothek nötig wird: dynamisch linken, Capability-Entscheid in
  `feature/platform/capability-matrix.md` nachziehen, Lizenztext +
  Quellangebot im Release bündeln (analog Lensfun, s. pipeline.md F-098).
- **Alignment/Geometrie:** Keine Modellabhängigkeit in 1.5 (klassische
  Bildverarbeitung, keine ONNX-Modelle, keine Modell-Lizenzfragen).
  Falls später KI-Deghosting/Blending folgt: eigener Modell-/Lizenz-/
  Capability-Entscheid (F-078) vorab.
- **Dependency-Pins:** Neue Dependencies werden mit Pin + Upgrade-Pfad
  eingeführt (Analogie: LibRaw-/ort-Pins in Agents.todo.md); kein Upgrade
  ohne ADR.
- **Projektlizenz:** Interims proprietär/kommerziell (s. Agents.todo.md
  LIZ); DNG- und Alignment-Code ändern daran nichts, Lizenzwahl bleibt
  Eigentümer-Entscheid.

## Abgrenzung MVP (nicht 1.0)

G-13 ist **Release 1.5** (User-Entscheid 2026-09-03, s. Agents.todo.md
Releaseplan). Es ist **kein MVP (1.0)**-Blocker:

- MVP (1.0) enthält keinen Merge-Pfad; `Cmd/Ctrl+H`/`Cmd/Ctrl+M` dürfen in
  1.0 fehlen oder als „geplant (1.5)" sichtbar deaktiviert sein — aber nie
  als stiller No-Op, der ein Merge vortäuscht.
- Schema: Pre-MVP sind Schemaänderungen Breaking Changes ohne
  Abwärtskompatibilitätspflicht (Agents.todo.md); das Merge-Schema wird
  dennoch von Anfang an versioniert (`merge_version` 1) eingeführt, damit
  1.5-Migrationen ab MVP der Migrations-Maschinerie folgen.
- Kein Merge-Code, keine Merge-Tests und keine Merge-Golden vor dem
  Folge-Task; dieser Entscheid ist die einzige 1.5-Vorleistung.

## Folge-Implementierungstasks (Vorschlag)

Der Build-Agent wandelt diesen Entscheid in getrennte Tasks um (je ein
Implementierungs-Agent pro Crate, serielle Schema-Welle zuerst):

1. **MERGE-SCHEMA-1 (Schema-Welle, `lumina-sidecar`):** Merge-Rezept-Typen,
   Validierung, Roundtrip- + Migrations-Tests (Pre-MVP-Muster).
2. **MERGE-CORE-1 (`lumina-merge` neu + `lumina-core`-Primitive):**
   Alignment (HDR-Translation, Pano-Homographie zylindrisch),
   Zusammenführung (gewichtetes lineares HDR, Feder-Blend),
   deterministischer Digest; Unit-/Property-Tests (Wertebereiche,
   Monotonie, Clipping), kein Dateisystem im Core-Anteil.
3. **MERGE-DNG-1 (DNG-Writer + Re-Import):** Writer-Auswahl (Lizenz-/Capability-
   Entscheid), lineares 16-Bit-DNG, EXIF-Übernahme, Re-Import via
   `lumina-raw` als Quelle; Roundtrip-Tests (DNG lesen → Hash-stabil).
4. **MERGE-CLI-1 (`lumina-cli`):** `merge-hdr`/`merge-pano`-Commands,
   Exit-Codes, `missing`/`stale`/`unsupported` laut auf stderr, atomarer
   Write; CLI-End-to-End-Tests.
5. **MERGE-GUI-1 (`lumina-gui`):** `Cmd/Ctrl+H`/`Cmd/Ctrl+M`-Aktionen,
   Jobsteuerung + sichtbarer Merge-Status (`ok`/`stale`/`missing`),
   headless GUI-Tests (egui-Context + LuminaApp, tempdir), kein manueller
   Test als einzige Absicherung; `cargo test -p lumina-gui` ohne GPU grün.
6. **MERGE-GOLDEN-1 (Golden-Gates):** HDR- und Panorama-Golden mit
   dokumentierten Toleranzen (synthetische Fixtures bevorzugt, CC0/generiert;
   keine Benutzerfotos, keine privaten Pfade), Budget-Einordnung nach
   `feature/quality/performance-benchmarks.md` (F-074).

## Abnahmekriterien (CLI + GUI-headless + Golden-Gates)

Der Entscheid gilt als umgesetzt, wenn alle Punkte erfüllt und durch einen
unabhängigen Verifizierungs-Agenten bestätigt sind:

- [ ] Merge-Rezept-Schema (`merge_version` 1) mit Roundtrip- und
  Validierungstests (Ablehnung statt Stillem Clippen/Fallback).
- [ ] CLI: `merge-hdr` und `merge-pano` erzeugen aus dokumentierten
  Fixtures ein lineares DNG + Sidecar-Bundle (atomar, relative Pfade);
  Exit-Code 0 bei Erfolg, ≠ 0 mit klarem stderr bei
  `missing`/`stale`/`unsupported`.
- [ ] GUI-headless: `Cmd/Ctrl+H`/`Cmd/Ctrl+M` durchlaufen dieselbe
  Merge-Schrittfolge wie die CLI (s. §Kein stiller Fallback, F6-Notiz);
  Merge-Status sichtbar (`ok`/`stale`/`missing`);
  `cargo test -p lumina-gui` grün ohne GPU.
- [ ] Golden-Gates: HDR-Golden (Belichtungsreihe → erweiterter Dynamikumfang
  messbar) und Panorama-Golden (Überlapp-Geometrie byte-stabil bzw. mit
  begründeter Toleranz) grün; Toleranzen dokumentiert. Umsetzungsstand
  GUI-Slice (2026-09-16, F7): GUI-Golden als Datenanker (BLAKE3-Hash +
  Force-Re-Run byte-identisch + Re-Import-Geometrie); die
  Dynamikumfang-Messung bleibt als Abnahme am CLI/Core-Anker zu verorten.
- [ ] Re-Import: Merge-DNG ist als Quelle lesbar (Decode-Kontext
  dokumentiert); Rezept/virtuelle Kopien/Export funktionieren darauf wie
  auf jeder anderen Quelle.
- [ ] Veraltung: Geänderte Quelle (Hash-Mismatch) oder fehlendes DNG wird
  als `stale`/`missing` gemeldet; kein stiller Fallback, keine automatische
  Neuberechnung als einzige Option.
- [ ] Originale byte-identisch unverändert; keine absoluten Pfade im
  Sidecar; Formatierung + Clippy grün.
- [ ] Verifizierungsbericht beantwortet die BESTANDEN-Checkliste aus
  `DoD.md` §7 mit Belegen (End-to-End-Kette, Spez→Test-Mapping, Gates).

## Offene Risiken

- **DNG-Writer-Auswahl (hoch):** Noch keine Bibliothek evaluiert; nativen
  Code vermeiden (Lizenz-/Capability-Risiko). Fällt die Evaluierung auf
  eine native Lib, braucht es Capability-Matrix-Nachtrag + F-078-Prüfung.
- **DNG-Re-Import via LibRaw (mittel):** Ungeprüft, ob linear geschriebene
  DNGs vom gepinnten LibRaw 0.22.2 in allen Dimensionen stabil dekodieren
  (CR3-Präzedenz: Dimensionswechsel zwischen LibRaw-Versionen). Absicherung
  über MERGE-DNG-1-Roundtrip-Tests; Pin beachten (kein Upgrade ohne ADR).
- **Panorama-Qualität in 1.5 (mittel):** Nur zylindrische Projektion +
  Feder-Blend; komplexe Szenen (Parallaxe, Belichtungswechsel) liefern
  sichtbare Nähte — bewusst als dokumentierte Grenze, kein Qualitäts-
  versprechen über den Golden-Fixtures hinaus.
- **HDR-Deghosting fehlt (niedrig):** Bewegte Objekte in der Reihe
  erzeugen Geister; 1.5 meldet nur Residuen, entfernt sie nicht per KI.
  KI-Deghosting wäre ein eigener Modell-/Lizenz-Entscheid (Post-1.5).
- **Performance großer Panoramen (niedrig):** Volle Auflösung × N Quellen
  sprengt ggf. interaktive Budgets. F-074-Einordnung ist erfolgt (F-074-N7,
  2026-09-17): die Merge-Klasse ist gemessen und in `perf/baseline.json` /
  `perf/budgets.json` mit `gate: false` registriert (report-only, bis
  unabhängig kalibriert); eine harte interaktive Garantie bleibt außerhalb
  dieses Entscheids. `estimate_pano_transform` ist bewusst als interaktiver
  Latenzpfad (nicht als Per-Frame-Kernel) nicht budgetiert.
