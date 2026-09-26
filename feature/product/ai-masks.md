# Persistente AI-Masken

**Feature:** F-004 Persistente AI-Masken

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Maskenidentität](#maskenidentität)
- [Artefakte](#artefakte)
- [Masken-DAG](#masken-dag)
- [Benutzergeführte Segmentierung](#benutzergeführte-segmentierung)
- [Status und Wiederverwendung](#status-und-wiederverwendung)
- [Lokale Anpassungen](#lokale-anpassungen)
- [GUI- und kittest-Vertrag der mask-local Editoren (P1.2a–d)](#gui--und-kittest-vertrag-der-mask-local-editoren-p12ad)
- [Abnahme](#abnahme)

## Ziel

Lokale ONNX-Inferenz erzeugt eine Alpha-Matte einmalig und persistiert sie als
Sidecar-Artefakt. Beim erneuten Öffnen wird die Matte geladen. Ein Modell muss
nicht dauerhaft installiert sein, um eine bereits gültige Maske zu verwenden.

## Maskenidentität

Jede Maske referenziert mindestens:

- `source_content_hash`;
- RAW-Decode- und Orientierungsparameter;
- Modellname, Modellversion und Modell-Hash;
- Vorverarbeitung und Inferenzauflösung;
- Nachskalierung und Koordinatensystem;
- Datenformat, Auflösung, Kanalzahl und Artefakt-Prüfsumme;
- Erstellungszeitpunkt und Generatorversion.

### Normative Details (F-082-FOLLOWUP, SOLL)

Diese Bestandteile sind normativ und werden in `lumina-onnx`/`lumina-sidecar`
über `ModelManifest` → `ModelIdentity` + `ArtifactReference` persistiert. Eine
Maske ist nur gültig, wenn **alle** Bestandteile übereinstimmen; eine
Abweichung markiert sie als `stale` (keine stille Neuberechnung).

- **Modell-Hash (`model_hash`):** SHA-256 über die exakten Artefakt-Bytes
  (`.onnx`-Datei), hex-kodiert `sha256:<64 hex>`. Der Pin steht in
  `ModelManifest.model_hash` und wird beim Laden via
  `lumina_onnx::hash::verify_model_file` geprüft. Bis echte Gewichte
  committet sind trägt das Manifest den Platzhalter
  `pending-integration` (`ModelHashStatus::Pending` — nicht verifizierbar,
  aber nicht als `Verified` ausgebbar). Ein Mismatch ist
  `ModelArtifactStale` (harter Fehler, nie stiller Fallback). Siehe
  `crates/lumina-onnx/tests/fixtures/README.md` und
  `feature/quality/fixtures-licensing.md` §3.4 für den hash-gepinnten
  Behavior-Fixture.
- **Inferenzauflösung:** dokumentiert `1024×1024` für alle v1-Modelle
  (BiRefNet `BiRefNet`, SAM 2.1 `sam2.1_hiera_*`; Quelle:
  `ModelInputSpec.resolution`). Die Auflösung ist Teil der
  `ModelInputSpec` und fließt in den deterministischen
  `input_spec_digest` (`sha256:<hex>` unter `ModelIdentity.extras[
  "input_spec_digest"]`) ein — eine Auflösungsänderung macht persistierte
  Masken `stale`, selbst wenn Name/Version/Hash gleich bleiben (R2-ONNX-01).
- **Vorverarbeitung:** pro Manifest `InputNormalization` (ImageNet
  `mean=[0.485,0.456,0.406]`, `std=[0.229,0.224,0.225]`), Kanal-Layout
  `Rgb`, Tensor-Format `Nchw` und Tensor-Namen (`input`/`images` →
  `output`/`masks`). Die Normalisierung wird im ORT-Pfad via
  `normalize_rgb_to_nchw` angewendet (CHW-Order); Vorverarbeitung ist damit
  Teil des `input_spec_digest` und jede Änderung (mean/std, Layout,
  Tensor-Name) invalidiert persistierte Masken. Tests dürfen keine
  Gewichte aus dem Netz laden — nur lokale, hash-gepinnte Fixtures unter
  `crates/lumina-onnx/tests/fixtures/` (deterministisch, dokumentiert,
  kein spontaner Download, sonst `#[ignore]`/Env-Gate).

## Artefakte

Die JSON-Datei speichert Definition und Referenz. Die Matte selbst liegt als
komprimiertes, sidecarbezogenes Binärartefakt in `.lumina.zdata` vor. Das
Format soll Kachelung oder Multi-Resolution und mindestens 16-Bit-
Graustufengenauigkeit ermöglichen.
Unkomprimierte Vollauflösungs-`f32`-Arrays im JSON sind nicht zulässig.

## Masken-DAG

Jede virtuelle Kopie besitzt eine eigene Maskenbibliothek. Knoten können jedoch
auf Knoten anderer virtueller Kopien referenzieren. Die Auswertung bildet einen
gerichteten azyklischen Graphen; Zyklen werden bei der Validierung abgelehnt.

Unterstützte v1-Operationen sind `union`, `intersect`, `subtract` und
`invert`. `duplicate & invert` erzeugt keinen zweiten Matte-Payload, sondern
einen neuen Referenz-/Operationsknoten. Werden Cross-Copy-Referenzen durch
Löschen der Quellkopie ungültig, werden Graphdefinitionen in die Zielbibliothek
materialisiert. Identische binäre Payloads dürfen im `.zdata`-Container über
ihren Content-Hash dedupliziert bleiben.

### Auswertung

Maskendefinitionen tragen in Schema 1 das optionale Feld `operation`; fehlt es,
ist der serde-Default `source`. Eine Source-Maske hat keine Referenzen und wird
mit einer bereitgestellten `uint16`-Fläche aus `.zdata` gespeist. `invert`
benötigt genau eine Referenz, `union` und `intersect` mindestens zwei, und
`subtract` genau zwei Referenzen (`a` zuerst, `b` danach). Alle Flächen müssen
dieselbe Breite und Höhe besitzen. Pro Pixel gilt: Union ist `max`, Intersect
ist `min`, Invert ist `65535 - value`, und Subtract ist
`round(a * (1 - b / 65535))`, integer-sicher als
`(a * (65535 - b) + 32767) / 65535` berechnet. Fehlende Payloads, Ziele und
Zyklen sind Fehler; es gibt keine stillen Resizes oder leeren Fallbacks.

### Maskengruppen (Copy vs. Duplicate, User-Entscheid 2026-09-20 — Lightroom kennt das nicht)

- **Copy** erzeugt eine tiefe, unabhängige Kopie (eigene Matte/Payload, eigene
  Parameter). Änderungen an der Quelle wirken nicht auf die Kopie und umgekehrt.
- **Duplicate** erzeugt eine **Gruppe**: einen benannten logischen Container mit
  stabiler ID, dessen Mitglieder **Referenzen (Pointer)** auf den Quellknoten
  sind. Änderungen am Quellknoten propagieren an alle Mitglieder; es gibt keine
  stillen Entkopplungen (sichtbarer Gruppenstatus im Panel).
- Einzelne Masken lassen sich nachträglich unter einer Gruppe zusammenfassen
  (Mitglieder per stabiler Knoten-ID, keine Arrayposition).
- **Gruppen-Aktionen** wirken auf die logische Einheit: gemeinsam
  selektieren, aktivieren/deaktivieren, löschen, verschieben sowie gemeinsame
  Parameter-Offsets (z. B. Dichte/Feather) für alle Mitglieder. Die Gruppe ist
  im Panel aufklappbar (Mitglieder einzeln sichtbar, analog Stapeln).
- **Löschen der Quelle:** Gruppenmitglieder materialisieren als eingefrorene
  Kopien (laut, mit History-Eintrag) — konsistent zur Cross-Copy-Regel oben.
  Kein Mitglied wird still gelöscht oder entleert.
- **Persistenz:** Sidecar-first; Gruppe + Mitgliedschaften in der
  Maskenbibliothek der virtuellen Kopie, binäre Matten weiter dedupliziert über
  Content-Hash im `.zdata`-Container.

#### Normative Implementierungsdetails (LRPAR-G03-MASKGROUP-03, 2026-09-20)

- **Mitgliedschaftsbereich:** Eine Gruppe gehört genau einer virtuellen Kopie
  und darf nur Knoten **derselben Kopie** referenzieren (`member.copy_id` ==
  besitzende Kopie). Cross-Copy-Gruppenmitgliedschaft ist nicht Teil von v1.
- **Eindeutigkeit:** Ein Maskenknoten gehört innerhalb einer Kopie höchstens
  einer Gruppe an; eine doppelte Zuordnung wird laut abgelehnt (kein stilles
  Umhängen). Gruppen-IDs sind innerhalb der Kopie eindeutig.
- **Identität:** Die Gruppen-ID ist stabil und wird beim Anlegen deterministisch
  aus `(copy_id, sortierte Mitglieds-Knoten)` abgeleitet
  (`mask-group-<blake3>`), danach nie neu berechnet — Umbenennen und Umsortieren
  ändern die ID nicht. Keine Arrayposition ist Identität.
- **Anzeigename:** Nicht-leer, getrimmt, ohne Steuerzeichen. Der Zuklappzustand
  `collapsed` wird persistiert (Default `false`); bei gleichzeitigen Schreibern
  gilt last-writer-wins wie beim Stapel.
- **Schema:** Additiv-optional als geflattete Top-Level-Liste `mask_groups` am
  Virtual-Copy-Objekt (über die bestehende additive `extras`-Ablage, damit
  keine `VirtualCopy`-Struct-Literale außerhalb des Umfangs brechen — gleiches
  Muster wie der additive negative Prompt). Kein `schema_version`-Bump; ein
  vorhandener, aber fehlgeformter `mask_groups`-Wert wird laut abgelehnt (kein
  stilles Normalisieren).
- **Copy vs. Duplicate:** `Copy` ist die tiefe, unabhängige Kopie (eigene
  Definition/ID, eigener Payload; Quelländerungen propagieren nicht).
  `Duplicate` legt eine Gruppe mit einem Pointer-Mitglied auf den Quellknoten
  an; Änderungen am Quellknoten sind für alle Mitglieder sichtbar. Es gibt
  keine stille Entkopplung.
- **Gruppen-Aktionen:** gemeinsam selektieren; aktivieren/deaktivieren
  (Sichtbarkeit aller Mitglieds-Layer); auflösen (Gruppencontainer entfernen,
  Masken bleiben erhalten — kein stilles Löschen); Mitgliederreihenfolge
  verschieben; gemeinsame Parameter-Offsets (Feather/Density) auf alle
  Mitglieds-Layer als Einheit, deterministisch auf die gültigen Bereiche
  geklemmt.
- **Quell-Löschung:** Wird ein Maskenknoten gelöscht, der von Gruppenmitgliedern
  referenziert wird, wird **pro gelöschtem Quellknoten genau eine** eingefrorene
  tiefe Kopie erzeugt; alle betroffenen Mitgliedschaften und referenzierenden
  Layer werden auf die eingefrorene Kopie umgehängt, mit lautem `info!`-Log und
  History-Eintrag. Kein Mitglied wird still gelöscht oder entleert.

## Benutzergeführte Segmentierung

Neben automatischer Subject-Segmentierung soll LuminaRust ein Objekt anhand
einer Benutzerführung isolieren können. Das ist eine eigene Maskenquelle und
keine zerstörende Änderung am Original.

### Prompt-Typen

- Rechteck beziehungsweise Box als grobe Objektbegrenzung
- Pinselmaske als positive/negative Markierung oder als Masken-Prompt
- Polygon, Ellipse und weitere Grundformen als kombinierbare Promptquellen

Eine Box wird in das Koordinatensystem des Modells transformiert. Eine
Pinselmaske kann abhängig von den Fähigkeiten des konkreten ONNX-Modells als
Masken-Prompt verwendet oder in positive und negative Promptpunkte umgewandelt
werden. Diese Umwandlung muss als Teil der Maskenidentität gespeichert werden.

### Modelladapter

Der ONNX-Adapter muss Modellfähigkeiten deklarieren, mindestens:

- `box_prompt`
- `point_prompt`
- `mask_prompt`
- `class_detection`
- `instance_segmentation`

Ein interaktives Modell wie SAM 2 kann Box- und Pinsel-Prompts in eine
Objektmaske umwandeln. Ein Modell wie YOLO-Segmentation kann später zusätzlich
eine erkannte Objektklasse und Instanzmaske liefern. Beide Modellarten werden
über dieselbe versionierte Masken- und Artefaktidentität eingebunden.

BiRefNet ist das erste automatische Subject-Modell. SAM 2 ist das erste
interaktive Box-/Pinsel-Modell. Der Adapter bleibt modellagnostisch, damit
später mehrere ONNX-Modelle gleichzeitig verfügbar sein können. Automatische
Kategorien wie „Haare von Person 1“ oder „Haare aller Personen“ gehören zu einer
späteren Instanz- und Teilsegmentierung.

### Persistenz

Promptdaten bleiben neben dem erzeugten Maskenknoten erhalten. Dazu gehören
Prompttyp, Koordinaten, Pinselauflösung, positive/negative Markierungen,
Modellfähigkeiten, Modellhash und die verwendete Transformation. Die erzeugte
Matte kann dadurch später explizit neu berechnet werden, ohne die Benutzer-
auswahl zu verlieren.

**Implementierungsstatus (F-079, 2026-08-20):** Umgesetzt und unabhängig
verifiziert. Das Masken-DAG-Modell (`lumina-sidecar`) enthält nun
`MaskPrompt` (Enum `box`/`brush`/`polygon`/`ellipse`/`gradient`) mit
`PromptTransform` (`method` + `parameters`, Teil der Maskenidentität) auf
jeder Variante sowie `MaskDefinition.prompt: Option<MaskPrompt>` als additives
Schema-v2-Feld (`#[serde(default, skip_serializing_if = "Option::is_none")]`,
keine Migration nötig). `validate_prompt` (in `SidecarDocument::validate`)
weist ungültige Prompts zurück. In `lumina-core` erzeugt
`rasterize_prompt` eine deterministische, modellfreie geometrische Matte je
Prompttyp (Box-Rechteck, Ellipse, Polygon-Füllung, Gradient, Pinsel als
Positive/Negative-Disks); `MaskGraph::evaluate_node` wertet eine Prompt-
Quelle aus, indem sie eine geladene Ebene vorzieht, sonst geometrisch
rasterisiert. Damit sind Prompt-Quellen heute ohne Modell auswertbar; die
modellbasierte Segmentierung (SAM 2) folgt in F-082 und ersetzt den
geometrischen Fallback, sofern ein Modell verfügbar ist. F-081
(Prompt-Transformationen und Koordinatensysteme persistieren) ist mit
abgedeckt.

### F-082 — SAM-2.1-Modellfamilie und dynamische Variantenwahl (SOLL)

**Entscheidung (2026-08-20, Eigentümer):** „SAM 2" ist das erste interaktive
Segmentierungsmodell; **nicht** als fixe Variante, sondern als
**Modellfamilie `sam2.1_hiera_*` mit dynamischer Variantenwahl** passend zur
Geräteleistung. Lizenzprüfung abgeschlossen: **Code und Gewichte sind
Apache-2.0** (facebookresearch/sam2 `LICENSE`, HF-Model-Cards, Meta-
Announcement; R6 in `fixtures-licensing.md` verifiziert 2026-08-20).

**Varianten** (alle ONNX, Eingang 1024×1024 RGB NCHW, Encoder einmal pro
Bild → 256-d Embedding + High-Res-Features; Decoder je Prompt):

| Variante | Params | SA-V J&F | Charakter |
| --- | ---: | ---: | --- |
| `sam2.1_hiera_tiny` | 38,9 M | 76,5 | geringste CPU-Last, schnellster Encoder |
| `sam2.1_hiera_small` | 46,0 M | 76,6 | kleines Qualitäts-Plus |
| `sam2.1_hiera_base_plus` | 80,8 M | 78,2 | Metas Standard-Variante, Balance |
| `sam2.1_hiera_large` | 224,4 M | 79,5 | höchste Qualität, nur High-End |

**Dynamische Auswahl:** Der Adapter wählt die Variante zur Laufzeit über ein
`DeviceProfile` (Kernanzahl via `std::thread::available_parallelism`, optional
durch explizite Nutzer-/Umgebungsvorgabe übersteuerbar). Dokumentierte
Schwellen (Startwerte, später per Benchmark kalibrierbar): <4 Kerne → `tiny`;
4–7 → `small`; 8–15 → `base_plus`; ≥16 → `large`. Die Wahl ist **deterministisch**
und **nicht Teil der Maskenidentität** — die Identität persistiert die
tatsächlich verwendete Variante (`model_name` = exakte Variante, `model_hash`
= Artefakt-SHA256, siehe Maskenidentität), sodass Re-Runs unabhängig von der
Geräteklasse reproduzierbar bleiben.

**Artefakte:** ONNX-Export über das Microsoft-ORT-Export-Tooling
(`convert_to_onnx.py`, MIT, auf ORT-Commit gepinnt) aus den Meta-Checkpoints
(092824, Apache-2.0) ODER fertige, versionierte Community-ONNX-Artefakte
(Redistribution Apache-2.0); `model_hash` bleibt `pending-integration`, bis
lokale, hash-gepinnte Fixtures committet sind (keine spontanen Downloads in
Tests, Agents.md). Prompt-Kontrakt: `point_coords` (absolute Pixel im
1024²-Raum), `point_labels` (1 positiv / 0 negativ / −1 Padding / 2 Box-TL /
3 Box-BR), `input_masks`/`has_input_masks` (Pinsel/Polygon), Ausgabe `masks`
auf Originalgröße + `iou_predictions` + `low_res_masks` (4×-Upsampling);
Matte: Logits → u16-Graustufe im `.lumina.zdata`.

**Implementierungsumfang F-082:** `lumina-onnx` — `sam2_1_manifests()`
(4 Varianten-Deskriptoren analog `birefnet_manifest()`, Fähigkeiten
`box_prompt`/`point_prompt`/`mask_prompt`), `select_variant(DeviceProfile)`
mit den Schwellen, SAM2-Backend mit interaktivem Prompt-Interface
(Prompt → MaskPlane, Stub-basiert deterministisch für Tests; echter
ORT-Pfad hinter `onnx-rt`). **F-083:** Prompt-Roundtrip-, Modellfähigkeits-,
Re-Run- und nicht-unterstützter-Prompt-Tests. Die Einbindung in
`MaskGraph`/CLI/GUI ersetzt den geometrischen Fallback nur, wenn ein
Modell verfügbar ist (kein stiller Fallback).

**Implementierungsstatus (F-082 / F-083, 2026-08-20):** Umgesetzt und
unabhängig verifiziert (BESTANDEN, Commit `452d8a4`). `lumina-onnx`
enthält `Sam2Variant` + `sam2_1_manifest(s)`/`sam2_1_manifests()` (4 gültige
Deskriptoren, Eingang 1024² RGB NCHW, Tensor-Name `images`, Fähigkeiten
`box_prompt`/`point_prompt`/`mask_prompt`, `model_hash` = `pending-integration`
bis hash-gepinnte ONNX-Fixtures committet sind), `DeviceProfile`
(`detect()` via `available_parallelism` mit konservativem Fallback) +
`select_variant` (Schwellen exakt wie oben, Override gewinnt, deterministisch),
das Trait `PromptMaskInference` mit `SegmentationPrompt` /
`PromptPoint` / `PointLabel` / `BoxPrompt` / `MaskPromptLogits` sowie
`StubSam2Backend` (deterministische analytische Matte, keine Netze; ungültige
Prompts → `OnnxError::InvalidPrompt`, kein stiller Fallback); der
Prompt→Tensor-Kontrakt ist als Doc-Kommentar festgehalten. 17 neue
F-083-Tests (Roundtrip/Determinismus inkl. über Instanzen, Fähigkeiten,
Schwellen-Grenzfälle 3/4/7/8/15/16 + Override + Fallback, ungültige
Prompts, Stub-Matte). Bekannte Grenzen: der echte ORT-/Netzpfad ist nur als
struktureller Contract vorbereitet (folgt nach der LIZ-Entscheidung),
MaskGraph/CLI/GUI-Einbindung steht noch aus, und die Modellgewichte sind
weiterhin nicht committet (`pending-integration`, keine spontanen Downloads).
Lizenznachweis: SAM 2.1 Code+Gewichte Apache-2.0 verifiziert; BiRefNet
tatsächlich MIT (Manifest und Doku korrigiert) — siehe
`feature/quality/fixtures-licensing.md` §5/§8 (R6).

## Status und Wiederverwendung

- `valid`: Quelle, Modellkontext und Prüfsumme stimmen; Matte wird direkt
  verwendet.
- `stale`: Quelle oder Modellkontext weicht ab; alte Matte bleibt
  nachvollziehbar und kann explizit verwendet oder ersetzt werden.
- `missing`: Referenziertes Artefakt fehlt; es wird nicht stillschweigend
  inferiert.
- `corrupt`: Prüfsumme oder Format ist ungültig; Wiederherstellung oder
  explizite Neuberechnung ist erforderlich.

Eine neue Inferenz findet nur nach ausdrücklicher Aktion oder nach der
festgelegten Ungültigkeitsentscheidung statt.

Eine fehlende oder noch nicht berechnete Maske wird bei der Auswertung wie eine
leere Maske behandelt und erhält zusätzlich den sichtbaren Status `missing`
beziehungsweise `pending`. Die GUI bietet Berechnung vor dem Export oder eine
Hintergrundberechnung für nicht aktive Bilder an. Die Idle-Queue ist per
Ordner-/GUI-Einstellung deaktivierbar. Die CLI warnt standardmäßig und kann
mit `--update-masks` explizit neu berechnen.

Aktive, aber veraltete oder fehlende Masken dürfen exportiert werden; GUI und
CLI warnen. Die GUI bietet vor dem Export die Aktualisierung an. Eine Warnung
darf nicht stillschweigend in eine Neuberechnung umgewandelt werden.

## Lokale Anpassungen

Invertierung, Feathering, Blur, Dichte und lokale Regler werden als Rezept- oder
Masken-Layer-Daten gespeichert. Sie werden nicht in die Quellmatte gebrannt.
So kann dieselbe Matte in mehreren virtuellen Kopien unterschiedlich genutzt
werden.

**Implementierungsstatus (F-049, umgesetzt und verifiziert):** Die Modulation
wird nicht-destruktiv in `crates/lumina-core/src/mask_modulation.rs`
(`modulate_mask_plane`) angewendet und in `evaluate_layer` (nach bilinearem
Resample, vor Rückgabe) aufgerufen. Reihenfolge: `invert` (`u16::MAX - value`)
→ `feather` (Box-Blur, Radius `feather·max(w,h)/2`) → `blur` (Box-Blur, Radius
`blur·max(w,h)/4`) → `density` (Skalierung mit `density`, nur für `< 1.0`).
Jede Stufe ist bei ihrem Identitätswert ein No-op; die Modulation verändert die
persistierte Maske nicht. 9 Unit-Tests sichern Invert/Feather/Blur/Density und
die Reihenfolge ab.

## P0 SOLL — lokale Mask-Adjustments (`MASK-LOCAL-P0`, Release 1.0)

**Task-Eintrag 2026-09-25 (Dokument zuerst):** Die erste sichere lokale
Anpassungsebene wird als versioniertes, typed `MaskLayer`-Objekt umgesetzt und
nicht als undokumentierte `adjustment_*`-Extras. Die lokale CPU-Schicht ist
P0; die bestehenden Parent-Gates für echte Hardware/GPU-Parität
(`GPU-RENDER-MASK-19`, `GPU-PARITY-HW-28`, `R5-BRUSH-24`, `R5-MASKVIS-25`)
bleiben offen.

- **Nutzerentscheidungen:** Lokale Layer werden sequenziell auf dem global
  angepassten Ergebnis angewendet. Globales WB bleibt absolute-only und wird
  nur durch einen expliziten **Reset to As Shot** zurückgesetzt. Überlappende
  Layer werden in der persistierten Listenreihenfolge ausgewertet.
- **P0-Feld und Werte:** `local_adjustments` ist additiv-optional und
  versioniert. P0 erlaubt ausschließlich `exposure` (`-10..=10` EV),
  `contrast`, `highlights` und `shadows` (je `-1..=1`), in exakt
  `exposure → contrast → shadows → highlights`. Lokales WB/Tone/Color sind in
  **P0** nicht enthalten und dürfen dort nicht als persistiertes Alias
  auftreten; lokales relatives WB ist der getrennte Schnitt **P1.1**
  (siehe unten), Tone/Color/Presence/Detail/Optics bleiben P1.2.
- **Legacy/Migration:** Bestehende `adjustment_*`-Extras werden verlustfrei in
  das typed Objekt überführt, wenn Werte und Reihenfolge eindeutig gültig sind.
  Ein typed/legacy-Konflikt, unbekannte Version/Regler, NaN/Inf und
  Bereichsfehler sind harte Fehler; kein stilles Ignorieren, Clipping oder
  Überschreiben.
- **Masken-Kontext:** Die matte wird in den tatsächlichen Ausgabe-Raum
  transformiert. Full-Frame, Zoom-ROI, Crop/Aspect, 90°-Rotation und Spiegel
  müssen bijektiv und ohne falsche Koordinaten funktionieren. Lens,
  Perspective, generative Geometrie und inkompatible Dimensionen werden für
  lokale Edits sichtbar verweigert; es gibt keinen Resize-Fallback.
- **Render und Persistenz:** Der CPU-Compositor läuft nach dem bestehenden
  Global-/Geometry-Pfad. `alpha=0` erzeugt outside-mask Byte-Identität,
  `alpha=1` das vollständige unmaskierte lokale Rezept, partielle Alpha
  werden fraktional und deterministisch gemischt; Bild-alpha bleibt unverändert.
  Die lokale Version, persistierte Reihenfolge und Werte gehören in eine
  kanonische Mask-/Render-Digest. `strict`/`warn`, Draft-/Navigator-/Neighbor-/
  Thumbnail-Routen sind explizit und dürfen lokale Edits nie still verlieren.
  History/Reset/Previous speichert einen vollständigen additiven Layer-Snapshot.
  Das GUI-Previous auf eine Auswahl im selben Lauf trägt diesen Snapshot nur
  bei kompatiblem Ziel-Maskkontext vollständig weiter; inkompatible Copy-/Mask-
  Referenzen werden laut verworfen, während Sync Settings recipe-only bleibt.
  Der dateibasierte CLI-Befehl `previous` bleibt davon unberührt **recipe-only**
  und verweigert nicht-neutralen lokalen Maskenzustand laut.

**P0-Abnahme:** winzige exakte CPU-Goldens und Tests für Alpha 0/partial/1,
Outside-Mask-Identität, Layerreihenfolge, ROI/Crop/Aspect/90°-Rotation/Mirror,
ungültige Schemas, Cache-Identität, Draft-Refusal, CLI/GUI-Parität und
bestehende Pipeline-Regressionen. P1: lokale WB-/Tone-/Color-Regler; Hardware-
und GPU-Gates bleiben separat offen.

## P1.1 SOLL — mask-local relative White Balance (`MASK-LOCAL-P1.1`)

**User-Entscheidung (2026-09-25, verbindlich):** Globales WB bleibt
absolute-only. Es wird ausschließlich über **Reset to As Shot** gelöscht; es
gibt keine relative Umrechnung oder stilles Zurückfallen auf einen globalen
Regler. Das lokale Masken-WB ist ein eigener relativer Delta-Wert und wird nach
dem globalen Ergebnis, vor den lokalen Basic-Stufen dieses Layers, angewendet.
Layer werden weiterhin in der persistierten Listenreihenfolge ausgewertet.

- **Typed Schema und Version:** `local_adjustments` wird als versioniertes
  `MaskLocalRecipe` (Kompatibilitätsname `LocalAdjustments`) version 2
  persistiert. Die P0-Felder `exposure`, `contrast`, `highlights` und `shadows`
  bleiben unverändert erhalten. Neu sind ausschließlich
  `temperature_delta_k` (`-5000..=5000`, Kelvin) und `tint_delta`
  (`-1..=1`); beide sind endlich und werden ohne Clipping validiert. Fehlende
  Delta-Felder bedeuten `0` (Tint-only/Temperature-only sind gültig). Ein
  lokales `wb_temperature`, `wb_tint`, `temperature` oder `tint` ist kein
  gültiger Alias und wird laut abgewiesen. Version-1-Typed-Objekte und die
  historischen `adjustment_*`-Extras migrieren verlustfrei in version 2 mit
  neutralen Deltas; Typ/Version, unbekannte Felder, Konflikte, NaN/Inf und
  Bereichsfehler bleiben harte Fehler.
- **Deterministische Reihenfolge:** Für jede persistierte Layer gilt
  `global result → local relative WB → local Basic (exposure → contrast →
  shadows → highlights) → fractional mask blend`. Die WB-Werte werden als
  `f64`-Gains aus dem Delta berechnet und zusammen mit den Basic-Stufen in
  genau einer abschließenden RGBA8-Quantisierung ausgewertet; es gibt keine
  Zwischen-Quantisierung und keine Ganzzahl-Näherung. Alpha-0 lässt alle
  Bytes unverändert, Alpha-1 übernimmt das vollständige lokale Rezept und
  partielle Alpha bleiben deterministisch fraktional. Ein lokaler Layer darf
  keine globalen Rezeptfelder verändern.
- **Picker/Setter/CLI:** Die lokale WB-Pipette arbeitet nur mit der ausgewählten
  Maske und dem effektiven, bereits global verarbeiteten Quell-/Vorschau-
  Sample. Provenienz und Staleness werden explizit ausgewiesen; ein Sample
  außerhalb der Maske, ein fehlender Layer, ein nicht aktueller/ungültiger
  Sample oder ein nicht auflösbarer Quellstand schlägt sichtbar fehl. Es gibt
  keinen globalen Fallback. CLI und GUI verwenden dieselben typisierten
  Setter und Reset-/History-Transaktionen. **Auto-WB wird nicht eingeführt.**
- **Identität/Persistenz:** Version und beide relativen Deltas gehören in die
  kanonische Local-/Mask-/Render-Digest. History, Reset und Previous der
  **aktuellen** virtuellen Kopie übernehmen den vollständigen additiven
  Layer-Snapshot. GPU-/Stand-in-Pfade müssen lokales WB bis zur echten Parität
  sichtbar CPU-routen oder verweigern.
- **Cross-image Previous bleibt recipe-only (P0-Vertrag):** Der dateibasierte
  CLI-Befehl `previous` überträgt **keine** lokalen Masken-Layer und keine
  P0-/P1.1-Deltas auf andere Bilder. Ein nicht-neutraler lokaler Maskenzustand —
  auf der Quell-Kopie oder auf dem Ziel — wird laut verweigert: die Quelle
  bricht den gesamten Lauf ab (Exit 1, kein Ziel angefasst), ein betroffenes
  Ziel schlägt isoliert fehl (Exit 3, Ziel-Bytes unverändert). Sync Settings
  bleibt recipe-only. Ein expliziter Full-Look-/Masken-Kopiervorgang ist eine
  **spätere, getrennte** Aktion und ausdrücklich nicht Teil von P1.1. Ein Delta
  allein (Tint-only oder Temperature-only) ist bereits ein Verweigerungsgrund;
  es gibt keine „nur P0"-Ausnahme. Das GUI-Previous auf eine Auswahl im selben
  Lauf (inklusive des vollständigen Masken-Snapshots bei kompatiblem
  Ziel-Maskkontext) ist davon unberührt und bleibt der bestehende GUI-Vertrag.
- **Lazy effektive Quellstage:** Das Post-Global/Geometry-Quellstadium wird
  **nicht** bei jedem Render behalten. `RenderOutput::effective_source_stage`
  ist `Option`; nur die GUI-Route, die die lokale WB-Pipette versorgt, fordert
  es an. CLI, Export und jede globale/Stand-in-Route zahlen keine
  Full-Frame-Kopie. Fehlt die Stage, gilt das als „keine laufende Pick-Session"
  und nie als veraltetes Sample.
- **Abnahme:** Exakte CPU-Goldens ohne Toleranz für Delta-only, Tint-only,
  Temperature-only, Reihenfolge (inklusive zwei überlappender Layer mit
  Temperatur- **und** Tint-Delta in persistierter Reihenfolge), Full/Half/Zero-
  Mask, Basic-Nachweis, Global-Absolut/Reset-to-As-Shot, Alpha/Outside-Mask,
  Legacy-v1/Deltas, explizite Nicht-Zahlen (u. a. ein `null` oder ein
  sentinel-ähnliches Objekt) als lauter Fehler statt stillem `0`, Digest/
  History/Reload, CLI/GUI-Parität, Picker-Provenance/Staleness sowie ein
  No-Local-Pfad-Nachweis, dass ohne Pick-Session keine Stage behalten wird und
  die Pixel identisch bleiben. P1.2 Tone/Color/Presence/Detail/Optics bleiben
  deaktiviert; der lokale Renderer darf sie weder aktivieren noch als Stub
  vortäuschen.

## P1.2a SOLL — mask-local Tone Curves (`MASK-LOCAL-P1.2a`)

**Festgeschriebene Semantik (User-Entscheidung 2026-09-25, verbindlich):**
Lokale Layer werden **sequenziell auf dem global adjustierten Ergebnis**
ausgewertet, nie parallel und nie auf dem Dekoder-Frame. Je persistierter
Maske gilt exakt
`global result → local relative WB → local Basic (exposure → contrast → shadows → highlights) → local tone curve → fractional mask blend`;
der in P1.2b folgende lokale Color-Stage schließt sich danach an. Die lokale
Kurve steht damit an **exakt derselben** Stelle wie der globale Kurven-Stage —
nach den Scalar-Basic-Stufen, vor dem Color-Stage. Überlappende Layer werden in
der **persistierten Listenreihenfolge** ausgewertet; ein Reordering ist eine
Änderung der Render-Identität, keine äquivalente Umordnung. Das globale WB
bleibt **absolute-only** mit explizitem **Reset to As Shot** — kein stiller
Fallback, kein relatives globales Alias, kein Auto-WB. Bis zur echten
GPU-Parität ist die lokale Kurve **CPU-first**: GPU-/Stand-in-Routen müssen
sichtbar CPU-routen oder verweigern.

- **Typed Schema und Version:** `local_adjustments` wird auf **Version 3**
  gehoben. Neu ist ausschließlich `curves` (`Option<Curves>`), reusing der
  bestehenden `lumina_sidecar::Curves`/`CurveChannels`/`CurvePoint`-Typen und
  deren bestehenden Punkt-Regeln und Ranges: `version == 1`, 2..=32 Punkte,
  endlich, Input/Output in `0..=1`, strikt steigender Input und feste
  Endpunkte `(0,0)`/`(1,1)`. Es gibt keine lokalen HSL-/Point-Color-/
  Grading-/Presence-/Detail-Felder; diese bleiben deaktiviert und ein
  entsprechender `--set-local-adjustment`-Key ist ein lauter „unknown local
  adjustment"-Fehler statt eines stillen No-Op.
- **Migration ohne stillen Verlust:** v1 (nur P0-Scalar) und v2 (P1.1-WB-Delta)
  migrieren **verlustfrei** nach v3 mit `curves: None`. Ein v1/v2-Payload, der
  ein `curves`-Feld enthält, ist ein **lauter** Fehler — kein stiller Drop und
  kein Smuggling einer späteren Version. Die historischen `adjustment_*`-Extras
  bleiben auf die vier P0-Keys beschränkt; `adjustment_curves` bleibt „unknown
  local adjustment". Fehlende Kurvenkanäle, eine Identitätskurve
  (`[(0,0),(1,1)]`) und ein explizit gespeichertes `curves: None` sind
  byte-identisch neutral.
- **Deterministische Reihenfolge und Quantisierung:** Für jede persistierte Maske
  gilt `global result → local relative WB → local Basic (exposure → contrast →
  shadows → highlights) → local tone curve → fractional mask blend`; der in
  P1.2b folgende lokale Color-Stage schließt sich danach an. Die lokale Kurve
  steht damit an **exakt derselben** Stelle wie der globale Kurven-Stage — nach
  den Scalar-Basic-Stufen, vor dem Color-Stage. „Exakte Global-Kernel-Reihenfolge"
  heißt hier wörtlich: derselbe `Curves`-/`CurvePoint`-Typ, derselbe Validator
  (`lumina_sidecar::validate_curves`), derselbe PCHIP-Evaluator
  (`lumina_core::curve_math::monotone_curve`) und dieselbe
  `value * master / luminance`-Komposition mit `luminance > 1e-9`-Guard. Der
  lokale Layer wertet WB-Gains, die vier Basic-Stufen und die Kurve vollständig
  in `f64` ab und quantisiert **genau einmal** am Ende auf RGBA8; der globale
  Kernel rundet dazwischen, weil er auf einem `u8`-Frame arbeitet — die lokale
  Kurve sieht daher nie einen gerundeten Zwischenwert. Bild-Alpha bleibt
  unberührt. Alpha-0 lässt alle Bytes unverändert, Alpha-1 übernimmt das
  vollständige lokale Rezept, partielle Alpha bleiben deterministisch fraktional
  gemischt.
- **Zustand, Reset und Persistenz:** Expliziter Reset pro Kanal sowie für alle
  lokalen Curves; History, Reset und Previous der **aktuellen** virtuellen
  Kopie übernehmen den vollständigen additiven `MaskStateSnapshot` (inklusive
  Kurvenblock). Der Kurvenblock gehört in die kanonische
  `LocalAdjustments::digest` und `mask_layers_digest` — sowohl die
  Persistenzform als auch die Werte. Ein lokaler Layer darf keine globalen
  Rezeptfelder verändern.
- **Cross-image Previous bleibt recipe-only (P0/P1.1-Vertrag):** Der
  dateibasierte CLI-Befehl `previous` überträgt **keine** lokalen Masken-Layer
  und keine P0-/P1.1-/P1.2a-Deltas auf andere Bilder. Nicht-neutraler lokaler
  Maskenzustand — auf der Quell-Kopie oder auf dem Ziel — wird laut
  verweigert: die Quelle bricht den gesamten Lauf ab (Exit 1, kein Ziel
  angefasst), ein betroffenes Ziel schlägt isoliert fehl (Exit 3,
  Ziel-Bytes unverändert). **Eine lokale Kurve allein ist bereits ein
  Verweigerungsgrund**; es gibt keine „nur P0/P1.1"-Ausnahme. Sync Settings
  bleibt recipe-only.
- **GUI/CLI:** Die GUI erhält einen lokalen Kurven-Editor (Kanalwahl
  Master/R/G/B, Punkt setzen/verschieben/löschen, Reset pro Kanal und für alle
  lokalen Curves) mit **keiner** globalen Rezeptmutation. Der
  **Interaktionsvertrag** (geteilter Gesten-Decoder mit dem globalen Graphen,
  Pflicht-Endpunkte, eigene Widget-Id, Reset-Transaktionen) und die geforderten
  Testanker sind in
  [§ GUI- und kittest-Vertrag der mask-local Editoren](#gui--und-kittest-vertrag-der-mask-local-editoren-p12ad)
  normativ festgeschrieben. Die CLI nutzt keine
  zweite, fast identische Flag-Partei, sondern den bestehenden generischen
  Kanal: `--set-local-adjustment 'curves.<master|red|green|blue>=I,O;I,O;...'`
  (dieselbe Punkt-Syntax wie das globale `--curve-points`) und
  `--reset-local-adjustment curves[.<channel>]`.
- **Abnahme:** Exakte CPU-Goldens ohne Toleranz für Master, einzelnen Kanal,
  Stack (Master plus Kanal), Halbmaske, Überlappungs-Reihenfolge, ungültige
  Punkte, Null/Identität und Legacy-Dokumente sowie CLI/GUI-Parität. P1.2b
  Color (HSL/Point Color/Color Grading), Presence, Detail und Optics bleiben
  deaktiviert.

## P1.2b SOLL — mask-local Color (`MASK-LOCAL-P1.2b`)

**Festgeschriebene Semantik (User-Entscheidung 2026-09-25, verbindlich):**
Lokale Layer werden **sequenziell auf dem global adjustierten Ergebnis**
ausgewertet, nie parallel und nie auf dem Dekoder-Frame. Je persistierter
Maske gilt exakt

```text
global result
  → local relative WB      (P1.1)
  → local Basic            (P0, exposure → contrast → shadows → highlights)
  → local tone curve       (P1.2a)
  → local HSL              (P1.2b)
  → local Point Color      (P1.2b)
  → local Vibrance/Saturation (P1.2b)
  → local Color Grading    (P1.2b)
  → fractional mask blend  (P0)
```

Der lokale Color-Block steht damit an **exakt denselben vier Positionen** wie
der globale Color-Block im globalen Kernel
(`HSL → Point Color → Vibrance/Saturation → Color Grading`, siehe
`lumina-core` `apply_recipe_with_scale_white_balance_and_denoise`). Überlappende
Layer werden in der **persistierten Listenreihenfolge** ausgewertet; ein
Reordering ist eine Änderung der Render-Identität, keine äquivalente
Umordnung. Das globale WB bleibt **absolute-only** mit explizitem
**Reset to As Shot** — kein stiller Fallback, kein relatives globales Alias,
kein Auto-WB. Bis zur echten GPU-Parität ist der lokale Color-Stage
**CPU-first**: GPU-/Stand-in-Routen (VRAM-Present, GPU-Parity-Readback,
Navigator/Neighbor/Thumbnail/Draft) müssen sichtbar CPU-routen oder verweigern.

- **Typed Schema und Version:** `local_adjustments` wird auf **Version 4**
  gehoben. Neu sind ausschließlich `hsl` (`Option<HslAdjustments>`),
  `point_color` (`Option<PointColor>`), `color_grading` (`Option<ColorGrading>`)
  sowie die zwei Skalare `vibrance` und `saturation` (je `-1..=1`, `0` =
  neutral). Es sind **dieselben** `lumina_sidecar::HslAdjustments`/
  `HslChannel`/`PointColor`/`PointColorEntry`/`ColorGrading`/
  `ColorGradingRange`-Typen und **dieselben** Validatoren, die das globale
  Rezept benutzt (`validate_hsl`/`validate_point_color`/
  `validate_color_grading`); es gibt keine lokalen Kopien, Umbenennungen oder
  gelockerten Bereiche. `HslAdjustments`/`PointColor`/`ColorGrading` tragen wie
  global `version == 1`; eine andere Blockversion ist ein lauter Fehler. Es gibt
  weiterhin **keine** lokalen Presence-, Detail-, AI-Denoise-, Noise-Reduction-,
  Sharpening- oder Optics-Felder: diese bleiben deaktiviert, und ein
  entsprechender `--set-local-adjustment`-Key ist ein lauter
  „unknown local adjustment"-Fehler statt eines stillen No-Op.
- **Migration ohne stillen Verlust:** v1 (nur P0-Scalar), v2 (P1.1-WB-Delta) und
  v3 (P1.2a-Kurve) migrieren **verlustfrei** nach v4 mit `hsl: None`,
  `point_color: None`, `color_grading: None`, `vibrance: 0.0` und
  `saturation: 0.0`. Ein v1/v2/v3-Payload, der eines dieser Felder enthält, ist
  ein **lauter** Fehler — kein stiller Drop und kein Smuggling einer späteren
  Version; ein explizites `null` oder ein Nicht-Objekt ebenso. Die historischen
  `adjustment_*`-Extras bleiben auf die vier P0-Keys beschränkt;
  `adjustment_hsl`/`adjustment_vibrance` bleiben „unknown local adjustment".
  `None`, eine neutrale HSL-Kombination, eine leere Point-Color-Liste, eine
  neutrale Grading-Instanz und `vibrance = saturation = 0` sind byte-identisch
  neutral (und fallen nach einem Reset vollständig weg).
- **Deterministische Reihenfolge und Quantisierung:** Der lokale Layer wertet
  WB-Gains, die vier Basic-Stufen, die Kurve und alle vier Color-Stufen
  vollständig in Fließkomma ab und quantisiert **genau einmal** am Ende auf
  RGBA8. „Exakte Global-Kernel-Reihenfolge" heißt hier wörtlich: dieselben
  `HslAdjustments`/`PointColor`/`ColorGrading`-Typen, dieselben Validatoren,
  dieselben Per-Pixel-Stage-Funktionen (`hsl_stage`, `point_color_stage`,
  `vibrance_saturation_stage`, `color_grading_stage`) und dieselbe
  RGB↔HSL-Hilfsfunktion wie der globale Kernel. Die Color-Stufen sind
  rechnerisch `f32`-Kernfunktionen auf normierten Werten; die float-Kette
  davor und die Quantisierung danach bleiben `f64`. Bild-Alpha bleibt
  unberührt. Alpha-0 lässt alle Bytes unverändert, Alpha-1 übernimmt das
  vollständige lokale Rezept, partielle Alpha bleiben deterministisch fraktional
  gemischt. Die Kernel-Pfadwahl ist **inhaltsabhängig**: ohne lokalen
  Color-Block bleiben die P0-, P1.1- und P1.2a-Bytes exakt erhalten, und ohne
  Kurve sieht der Color-Stage nie einen Kurven-Zwischenwert.
- **Zustand, Reset und Persistenz:** Expliziter Reset pro HSL-Kanal, pro
  Point-Color-Eintrag, pro Grading-Range (samt `balance`/`blending`) sowie für
  den gesamten lokalen Color-Block. History, Reset und Previous der
  **aktuellen** virtuellen Kopie übernehmen den vollständigen additiven
  `MaskStateSnapshot` **inklusive des kompletten Color-Blocks**. Der
  Color-Block gehört in die kanonische `LocalAdjustments::digest` und
  `mask_layers_digest` und damit in die Render-Identität — sowohl die
  Persistenzform als auch die Werte. Ein lokaler Layer darf keine globalen
  Rezeptfelder verändern; die lokale Color-Sektion schreibt nie
  `EditRecipe::hsl`/`point_color`/`color_grading`/`adjustments["vibrance"|"saturation"]`.
- **Cross-image Previous bleibt recipe-only (P0/P1.1/P1.2a-Vertrag):** Der
  dateibasierte CLI-Befehl `previous` überträgt **keine** lokalen Masken-Layer
  und keine P0-/P1.1-/P1.2a-/P1.2b-Deltas auf andere Bilder. Nicht-neutraler
  lokaler Maskenzustand — auf der Quell-Kopie oder auf dem Ziel — wird laut
  verweigert: die Quelle bricht den gesamten Lauf ab (Exit 1, kein Ziel
  angefasst), ein betroffenes Ziel schlägt isoliert fehl (Exit 3,
  Ziel-Bytes unverändert). **Ein lokaler Color-Block allein ist bereits ein
  Verweigerungsgrund**; es gibt keine „nur P0/P1.1/P1.2a"-Ausnahme. Sync
  Settings bleibt recipe-only.
- **GUI/CLI:** Die GUI erhält einen lokalen Color-Editor (HSL-Kanalwahl mit
  Hue/Saturation/Luminance, Vibrance/Saturation, Point-Color-Einträge mit
  Hinzufügen/Entfernen, Grading-Ranges mit `balance`/`blending`, Reset pro
  Bereich und für den ganzen lokalen Color-Block) mit **keiner** globalen
  Rezeptmutation; Interaktionsvertrag und Testanker siehe
  [§ GUI- und kittest-Vertrag der mask-local Editoren](#gui--und-kittest-vertrag-der-mask-local-editoren-p12ad).
  Die CLI nutzt keine zweite, fast identische Flag-Partei,
  sondern den bestehenden generischen Kanal:
  `--set-local-adjustment 'hsl.<channel>.<hue|saturation|luminance>=<n>'`,
  `--set-local-adjustment 'vibrance=<n>'` / `'saturation=<n>'`,
  `--set-local-adjustment 'point_color.add=<hue_center,hue_range,hue_shift,sat_shift,lum_shift>'`,
  `--set-local-adjustment 'color_grading.<range>.<field>=<n>'` und
  `--reset-local-adjustment hsl[.<channel>] | point_color[.<id>] |
  color_grading[.<range>|balance|blending] | color`.
- **Abnahme:** Exakte CPU-Goldens ohne Toleranz für HSL, Vibrance/Saturation,
  Point Color, Color Grading, den vollen Stack (WB + Basic + Kurve + Color),
  Halbmaske, Überlappungs-Reihenfolge, ungültige Werte, Null/Identität,
  Legacy-Dokumente (v1/v2/v3) sowie CLI/GUI-Parität und die sichtbare
  Verweigerung der weiterhin deaktivierten Presence/Detail/AI-Denoise/Optics-
  Stufen. Presence, Detail und Optics bleiben deaktiviert.

## P1.2c SOLL — mask-local Presence (`MASK-LOCAL-P1.2c`)

**Festgeschriebene Semantik (User-Entscheidung 2026-09-25, verbindlich):**
Lokale Layer werden **sequenziell auf dem global adjustierten Ergebnis**
ausgewertet, nie parallel und nie auf dem Dekoder-Frame. Je persistierter
Maske gilt exakt

```text
global result
  → local relative WB      (P1.1)
  → local Basic            (P0, exposure → contrast → shadows → highlights)
  → local Presence         (P1.2c)   ← texture / clarity / dehaze
  → local tone curve       (P1.2a)
  → local HSL              (P1.2b)
  → local Point Color      (P1.2b)
  → local Vibrance/Saturation (P1.2b)
  → local Color Grading    (P1.2b)
  → fractional mask blend  (P0)
```

Presence steht damit an **exakt derselben Position** wie im globalen Kernel
(Kanal-LUT → Presence → Kurve → Color). Die bereits verifizierte relative
Reihenfolge von P1.2a/P1.2b (Kurve vor Color) bleibt **unverändert**; es wird
nur die bislang fehlende Stufe an ihrer globalen Position eingefügt. Überlappende
Layer werden in der **persistierten Listenreihenfolge** ausgewertet; ein
Reordering ist eine Änderung der Render-Identität, keine äquivalente
Umordnung. Das globale WB bleibt **absolute-only** mit explizitem
**Reset to As Shot** — kein stiller Fallback, kein relatives globales Alias,
kein Auto-WB. Bis zur echten GPU-Parität ist die lokale Presence
**CPU-first**: GPU-/Stand-in-Routen (VRAM-Present, GPU-Parity-Readback,
Navigator/Neighbor/Thumbnail/Draft) müssen sichtbar CPU-routen oder verweigern.

- **Typed Schema und Version:** `local_adjustments` wird auf **Version 5**
  gehoben. Neu ist ausschließlich `presence: Option<Presence>`, und zwar mit
  **wörtlich wiederverwendetem** `lumina_sidecar::Presence` (`version == 1`,
  `texture`/`clarity`/`dehaze` je endlich in `-1..=1`) und dem **wörtlich
  wiederverwendeten** globalen Presence-Validator
  (`validate_presence`, extrahiert nach
  `crates/lumina-sidecar/src/presence_block.rs` und von globalem und lokalem
  Rezept geteilt). Es gibt keine lokalen Typkopien, keine gelockerten Bereiche
  und keinen stillen Fallback. Weiterhin deaktiviert bleiben lokales **Detail**,
  **Sharpening**, **Noise Reduction**, **AI-Denoise** und **Optics**; ein
  entsprechender Key ist ein lauter „unknown local adjustment"-Fehler, und der
  lokale Renderer darf keinen Stub vortäuschen. **Optics bleibt dauerhaft
  deaktiviert**, weil Linsenkorrektur und Perspektive geometrische Stufen
  *vor* den Masken sind und nicht als per-Maske-Per-Pixel-Tone-Stufe sinnvoll
  sind; Noise Reduction/AI-Denoise bleiben bis zum F-078-Modellgate
  (Gewichts-Lizenz, Provenienz, Hash-Pin) deaktiviert.
- **Migration ohne stillen Verlust:** v1 (nur P0-Scalar), v2 (P1.1-WB-Delta),
  v3 (P1.2a-Kurve) und v4 (P1.2b-Color) migrieren **verlustfrei** nach v5 mit
  `presence: None`. Ein v1..v4-Payload, der `presence` enthält (auch ein
  explizites `null` oder ein Nicht-Objekt), ist ein **lauter** Fehler — kein
  stiller Drop und kein Smuggling einer späteren Version. Weil die Einführung
  von v5 die obere Versionsgrenze verschiebt, werden die bestehenden
  Versions-Gates **versionsrichtig** verankert: der Kurven-Gate an
  `CURVE_LOCAL_ADJUSTMENTS_VERSION` (3), der Color-Gate an
  `COLOR_LOCAL_ADJUSTMENTS_VERSION` (4) und der Presence-Gate an
  `PRESENCE_LOCAL_ADJUSTMENTS_VERSION` (5), jeweils mit `version < …`. Sonst
  müsste ein v4-Dokument seinen eigenen Color-Block verlieren. Die historischen
  `adjustment_*`-Extras bleiben auf die vier P0-Keys beschränkt;
  `adjustment_presence` bleibt „unknown local adjustment". `None` und ein
  persistierter Null-Block (`texture = clarity = dehaze = 0.0`) sind
  byte-identisch neutral und fallen nach Reset vollständig weg.
- **Nachbarschaft und Statistiken bleiben vollbildbasiert, die Maske blendet
  nur:** Die DoG-Nachbarschaft (Textur-Radius `1 + round(|texture|·2)`, Radius
  also 1..3; Klarheit-Radius `8 + round(|clarity|·24)`, also 8..32) und die
  Dark-Channel-95 %-Perzentil-Statistik des Dehaze werden über das **ganze
  Bild** berechnet, **nicht** auf die Maskenregion beschränkt und **nicht**
  ROI- oder maskenabhängig skaliert. Die Maske steuert ausschließlich die
  Blendmenge. Damit gibt es **keine** maskenabhängige Statistik, **keinen**
  Resize-Fallback und **keine** Naht an der Maskenkante: die Render-Identität
  hängt nur am persistierten Presence-Wert und am Masken-Plane, nicht an der
  Bildgeometrie. Konkret heißt das: der Presence-Stage rechnet über die
  komplette lokale Ebene, und der anschließende P0-Blend mischt das Ergebnis
  fraktional ein — ein Pixel außerhalb der Maske sieht nie einen
  maskenverkleinerten Nachbarschafts- oder Statistikwert, sondern exakt die
  vom globalen Ergebnis vorgegebenen Bytes.
- **Geteilte Mathematik, ehrlich benannte Quantisierungs-Divergenz:** Die
  Presence-Mathematik (Box-Mittel/Detail, Dark-Channel, Perzentil,
  Dehaze-Formel) wurde nach `crates/lumina-core/src/presence_stages.rs`
  extrahiert und wird vom globalen und vom lokalen Kernel **wörtlich geteilt**.
  Beide rufen dieselben Funktionen `texture_radius`/`clarity_radius`/
  `box_bounds`/`box_count`/`box_mean`/`box_detail`/`dog_value`/`dog_channel`/
  `dark_channel_pixel`/`dark_channel`/`airlight`/`dehaze_transmission`/
  `dehaze_value` auf. Was sich unterscheidet, ist nur die **Ebene**, über die
  die Nachbarschaft liest: der globale Kernel liest einen RGBA8-Snapshot
  (`Rgba8Plane`), der lokale die un-quantisierte `f32`-Kette (`FloatPlane`) —
  beide in `0..=255`, und ein `u8`-Sample konvertiert in das exakt darstellbare
  `f32` desselben Werts, ist die `f32`-Arithmetik also bitweise dieselbe
  Operation. Der globale Kernel quantisiert dabei — wie bisher — intern nach
  jeder Teilstufe auf `u8` (nach jedem DoG-Durchgang und nach dem Dehaze), der
  lokale Pfad **genau einmal** am Ende des Layers. Diese Divergenz ist
  **bewusst, dokumentiert und getestet**; es gibt und darf **keinen** Test
  geben, der Byte-Gleichheit des lokalen Presence-Pfads mit dem globalen
  Presence-Stage behauptet, weil sie per Konstruktion falsch ist. Die
  Presence-Nachbarschaft rechnet wie der globale Stage in `f32` — das ist die
  geteilte numerische Domäne, **keine** zusätzliche Quantisierungsgrenze; die
  *eine* Grenze bleibt die abschließende RGBA8-Rundung des lokalen Layers.
- **Deterministische Reihenfolge und Quantisierung:** Der lokale Layer wertet
  WB-Gains, die vier Basic-Stufen, Presence, die Kurve und alle vier
  Color-Stufen vollständig in Fließkomma ab und quantisiert **genau einmal** am
  Ende auf RGBA8. Bild-Alpha bleibt unberührt. Alpha-0 lässt alle Bytes
  unverändert, Alpha-1 übernimmt das vollständige lokale Rezept, partielle
  Alpha bleiben deterministisch fraktional gemischt. Die Kernel-Pfadwahl ist
  **inhaltsabhängig**: **ohne** lokalen Presence-Block (abwesend **oder** ein
  persistierter Null-Block) bleiben die P0-, P1.1-, P1.2a- und P1.2b-Bytes exakt
  erhalten, und ein P1.2b-Layer mit Curve+Color nimmt **ohne** Presence weiterhin
  den bestehenden f64-Kernelpfad.
- **Zustand, Reset und Persistenz:** Expliziter Reset für den gesamten
  Presence-Block (Texture, Klarheit und Dehaze gemeinsam). History, Reset und
  Previous der **aktuellen** virtuellen Kopie übernehmen den vollständigen
  additiven `MaskStateSnapshot` **inklusive des kompletten Presence-Blocks**.
  Der Block gehört in die kanonische `LocalAdjustments::digest` und
  `mask_layers_digest` und damit in die Render-Identität. Ein lokaler Layer darf
  keine globalen Rezeptfelder verändern; die lokale Presence-Sektion schreibt
  nie `EditRecipe::presence`.
- **Cross-image Previous bleibt recipe-only (P0/P1.1/P1.2a/P1.2b-Vertrag):** Der
  dateibasierte CLI-Befehl `previous` überträgt **keine** lokalen Masken-Layer
  und keine P0-/P1.1-/P1.2a-/P1.2b-/P1.2c-Deltas auf andere Bilder.
  Nicht-neutraler lokaler Maskenzustand — auf der Quell-Kopie oder auf dem Ziel
  — wird laut verweigert: die Quelle bricht den gesamten Lauf ab (Exit 1, kein
  Ziel angefasst), ein betroffenes Ziel schlägt isoliert fehl (Exit 3,
  Ziel-Bytes unverändert). **Ein lokaler Presence-Block allein ist bereits ein
  Verweigerungsgrund**; es gibt keine „nur P0/P1.1/P1.2a/P1.2b"-Ausnahme. Sync
  Settings bleibt recipe-only.
- **GUI/CLI:** Die GUI erhält einen lokalen Presence-Editor (Texture, Klarheit,
  Dehaze und Reset gesamt) mit **keiner** globalen `EditRecipe::presence`-
  Mutation; Interaktionsvertrag und Testanker siehe
  [§ GUI- und kittest-Vertrag der mask-local Editoren](#gui--und-kittest-vertrag-der-mask-local-editoren-p12ad).
  Die CLI nutzt keine zweite, fast identische Flag-Partei, sondern den
  bestehenden generischen Kanal:
  `--set-local-adjustment 'presence.texture=<n>'` /
  `'presence.clarity=<n>'` / `'presence.dehaze=<n>'` und
  `--reset-local-adjustment presence`.
- **Abnahme:** Exakte CPU-Goldens ohne Toleranz für Texture, Klarheit, Dehaze,
  den vollen Stack (WB + Basic + Presence + Kurve + Color), Halbmaske,
  Überlappungs-Reihenfolge, Null-Block-Byte-Identität, ungültige Werte
  (Blockversion, Bereich, nicht-endlich), Legacy-v1..v4-Dokumente, die sichtbare
  Verweigerung der weiterhin deaktivierten Detail/Sharpening/Noise-Reduction-/
  AI-Denoise-/Optics-Stufen sowie CLI/GUI-Parität und History/Previous/Reset.
  Die globalen Presence-Goldens müssen **unverändert** grün bleiben. Detail,
  Sharpening, Noise Reduction, AI-Denoise und Optics bleiben deaktiviert.

## P1.2d SOLL — mask-local Detail (`MASK-LOCAL-P1.2d`)

**Festgeschriebene Semantik (User-Entscheidung 2026-09-26, verbindlich):**
Lokale Layer werden **sequenziell auf dem global adjustierten Ergebnis**
ausgewertet, nie parallel und nie auf dem Dekoder-Frame. Je persistierter
Maske gilt exakt

```text
global result
  → local relative WB      (P1.1)
  → local Basic            (P0, exposure → contrast → shadows → highlights)
  → local Presence         (P1.2c)   ← texture / clarity / dehaze
  → local tone curve       (P1.2a)
  → local HSL              (P1.2b)
  → local Point Color      (P1.2b)
  → local Vibrance/Saturation (P1.2b)
  → local Color Grading    (P1.2b)
  → local Noise Reduction  (P1.2d)   ← luminance / color
  → local Sharpening       (P1.2d)   ← amount / radius / detail / masking
  → fractional mask blend  (P0)
```

Die Detail-Stufe steht damit an **exakt denselben beiden Positionen** wie im
globalen Kernel: dort wie hier läuft **Noise Reduction vor Sharpening**, und
beide liegen nach dem Color-Block (`… Color Grading → Noise Reduction →
Sharpening → …`). Die bereits verifizierte relative Reihenfolge von
P1.2a/P1.2b/P1.2c (Presence → Kurve → Color) bleibt **unverändert**; es werden
nur die zwei bislang fehlenden Stufen an ihrer globalen Position angehängt.
Überlappende Layer werden in der **persistierten Listenreihenfolge** ausgewertet;
ein Reordering ist eine Änderung der Render-Identität, keine äquivalente
Umordnung. Das globale WB bleibt **absolute-only** mit explizitem **Reset to As
Shot** — kein stiller Fallback, kein relatives globales Alias, kein Auto-WB. Bis
zur echten GPU-Parität ist das lokale Detail **CPU-first**: GPU-/Stand-in-Routen
(VRAM-Present, GPU-Parity-Readback, Navigator/Neighbor/Thumbnail/Draft) müssen
sichtbar CPU-routen oder verweigern.

- **Typed Schema und Version:** `local_adjustments` wird auf **Version 6**
  gehoben. Neu ist ausschließlich `detail: Option<Detail>` mit genau den beiden
  globalen Teil-Blöcken `sharpening: Option<Sharpening>` (`version == 1`,
  `amount` in `0.0..=3.0`, `radius` in `0.1..=10.0`, `detail` in `0.0..=1.0`,
  `masking` in `0.0..=1.0`) und `noise_reduction: Option<NoiseReduction>`
  (`version == 1`, `luminance`/`color` je in `0.0..=1.0`) — **wörtlich
  wiederverwendete** globale Typen, mit **wörtlich wiederverwendeten**
  globalen Validatoren (extrahiert nach
  `crates/lumina-sidecar/src/detail_block.rs` und von globalem und lokalem Rezept
  geteilt: `validate_sharpening`/`validate_noise_reduction` sowie die
  Feldlisten und Neutralitätsprüfungen). Es gibt **keine** lokalen Typkopien,
  **keine** gelockerten und **keine** verkleinerten Bereiche und keinen stillen
  Fallback. Deaktiviert bleiben lokales **AI-Denoise** (F-078-Modellgate offen)
  und lokales **Optics** — beide **dauerhaft**: Optics, weil Linsenkorrektur und
  Perspektive geometrische Stufen *vor* den Masken sind und nicht als
  per-Maske-Per-Pixel-Tone-Stufe sinnvoll sind; AI-Denoise, weil das Modell-Gate
  (Gewichts-Lizenz, Provenienz, Hash-Pin) offen ist. Für beide gilt: **kein**
  Schemafeld, **kein** CLI-Key, **kein** Renderer-Stub, sondern ein lauter
  „unknown local adjustment"-Fehler.
- **Migration ohne stillen Verlust:** v1 (nur P0-Scalar), v2 (P1.1-WB-Delta),
  v3 (P1.2a-Kurve), v4 (P1.2b-Color) und v5 (P1.2c-Presence) migrieren
  **verlustfrei** nach v6 mit `detail: None`. Ein v1..v5-Payload, der `detail`
  enthält (auch ein explizites `null` oder ein Nicht-Objekt), ist ein **lauter**
  Fehler — kein stiller Drop und kein Smuggling einer späteren Version. Weil die
  Einführung von v6 die obere Versionsgrenze verschiebt, bleiben die bestehenden
  Versions-Gates **versionsrichtig** verankert: der Kurven-Gate an
  `CURVE_LOCAL_ADJUSTMENTS_VERSION` (3), der Color-Gate an
  `COLOR_LOCAL_ADJUSTMENTS_VERSION` (4), der Presence-Gate an
  `PRESENCE_LOCAL_ADJUSTMENTS_VERSION` (5) und der neue Detail-Gate an
  `DETAIL_LOCAL_ADJUSTMENTS_VERSION` (6), jeweils mit `version < …`. Sonst müsste
  ein v5-Dokument seinen eigenen Presence-Block verlieren. Die historischen
  `adjustment_*`-Extras bleiben auf die vier P0-Keys beschränkt;
  `adjustment_detail`, `adjustment_sharpening` und `adjustment_noise_reduction`
  bleiben „unknown local adjustment".
- **Nachbarschaft bleibt vollbildbasiert, die Maske blendet nur:** Der
  5x5-Bilateral-Kernel der Rauschunterdrückung, das 3x3-Gradientenfenster und
  die beiden separablen Gauß-Durchgänge des Sharpenings, der
  Detail-Mixing-Pfad (`detail·d_fine + (1−detail)·d_coarse`) und die
  Flat-Area-Maskierung (`((1−masking) + masking·clamp(|gx|+|gy|/max,0,1))`)
  rechnen über das **ganze Bild**. Es gibt **keine** maskenabhängige Statistik,
  **keinen** ROI-Resize-Fallback und **keine** Kantennaht. Die lokale
  Detail-Funktion nimmt in ihrer Signatur **weder Maske noch ROI** entgegen —
  nur das Blendgewicht. Ein Pixel außerhalb der Maske sieht damit nie einen
  maskenverkleinerten Nachbarschaftswert, sondern exakt die vom globalen
  Ergebnis vorgegebenen Bytes; die Render-Identität hängt nur am persistierten
  Detail-Wert, am globalen `render_scale` und am Masken-Plane, nicht an der
  Bildgeometrie.
- **Geteilte Mathematik, ehrlich benannte Quantisierungs-Divergenz:** Die
  Detail-Mathematik (Bilateral-Gewichte und -Mittelung, Chroma-Offsets,
  Gauß-Kernel, Luminanz-Gradienten und `global_max`, Detail-Mixing,
  Flat-Area-Faktor, Radius-Formel) wurde nach
  `crates/lumina-core/src/detail_stages.rs` extrahiert und wird vom globalen und
  vom lokalen Kernel **wörtlich geteilt**. Der globale Kernel behält dabei seine
  bisherigen `u8`-Quantisierungspunkte (ein `round()`/Clamp je Sub-Stufe: nach
  NR, nach dem Scharfen, und der Lese-Zugriff auf den `u8`-Snapshot), der lokale
  Pfad quantisiert **genau einmal** am Ende des Layers. Damit ändert sich
  **kein** globaler Byte: die bestehenden globalen Sharpening-/Noise-Reduction-
  Goldens bleiben **unverändert** grün. Diese Quantisierungs-Divergenz ist
  **bewusst und dokumentiert**; es gibt und darf **keinen** Test geben, der
  Byte-Gleichheit des lokalen Detail-Pfads mit dem globalen Detail-Stage
  behauptet, weil sie per Konstruktion falsch ist. Die Detail-Nachbarschaft
  rechnet wie der globale Stage in `f32` (`0..=255`) — das ist die geteilte
  numerische Domäne, **keine** zusätzliche Quantisierungsgrenze; die *eine*
  Grenze bleibt die abschließende RGBA8-Rundung des lokalen Layers.
- **Render-Scale bleibt global (F-096-Vertrag):** Die globale
  `Sharpening`-Stufe skaliert ihren Radius über den globalen `render_scale`
  (`sigma = max(radius · scale, 0.5)`, Fein-/Grob-Radius `max(0.5·r, 0.5)` bzw.
  `max(1.5·r, 0.5)`; `sharpening_render_scale_changes_render_only` und
  `sharpening_identity_direction_and_scale` pinnen das). Der lokale Detail-Block
  folgt **demselben** globalen `render_scale`, damit die lokale und die globale
  Stufe dieselbe effektive Radius-Skala sehen. Der lokale Block bekommt **keine**
  eigene Scale-Option und darf den globalen `render_scale` **nicht** überschreiben;
  es gibt kein zusätzliches persistiertes Feld dafür. Die Render-Identität
  ändert sich mit dem globalen `render_scale` genau wie beim globalen Sharpening,
  der Decode-Digest nicht.
- **Deterministische Reihenfolge und Quantisierung:** Der lokale Layer wertet
  WB-Gains, die vier Basic-Stufen, Presence, die Kurve, alle vier Color-Stufen,
  Noise Reduction und Sharpening vollständig in Fließkomma ab und quantisiert
  **genau einmal** am Ende auf RGBA8. Die **Reihenfolge innerhalb eines Layers ist
  dabei beobachtbar, nicht kosmetisch**: Farbe ändert die Nachbar-Luminanz, und
  Nachbar-Luminanz ist die Eingangsgröße der bilateralen Ähnlichkeitsgewichte, des
  separierbaren Gauss und des Frame-weiten Gradientenmaximums. Die beiden
  Nachbarschafts-Stufen laufen deshalb über die **ganze** un-quantisierte
  Farb-Ebene. `crates/lumina-core/src/render/tests/local_detail_reference.rs`
  transponiert beide Ketten **unabhängig** aus den dokumentierten Formeln (WB +
  Basic, Kurvenkomposition, F-096-Bilateral, F-095-Gauss, Detail-Mix, Flat-Area),
  `local_detail_order_tests.rs` validiert **jede** Transkription einzeln gegen den
  echten Kernel (Farb-Hälfte gegen einen reinen Farb-Layer, Detail-Hälfte gegen
  einen reinen Detail-Layer) und pinnt dann in **einem** Layer mit Farb- *und*
  Detail-Block: das Ergebnis ist die dokumentierte Kette und unterscheidet sich
  von der vertauschten Kette deutlich — der Test schlägt also fehl, wenn die
  Reihenfolge kippt. Die Color-Stufen rechnen in ihrer geteilten `0..=1`-Domäne,
  die Nachbarschafts-Stufen in `0..=255`; die Umrechnung dazwischen ist eine reine
  Domänen-Umrechnung (mal/teil 255 in `f32`) und **keine** weitere
  Quantisierungsgrenze. Bild-Alpha bleibt unberührt. Alpha-0 lässt
  alle Bytes unverändert, Alpha-1 übernimmt das vollständige lokale Rezept,
  partielle Alpha bleiben deterministisch fraktional gemischt. Die
  Kernel-Pfadwahl ist **inhaltsabhängig**: **ohne** lokalen Detail-Block
  (abwesend **oder** beide Teil-Blöcke neutral) bleiben die P0-, P1.1-, P1.2a-,
  P1.2b- und P1.2c-Bytes exakt erhalten, und ein P1.2b-/P1.2c-Layer nimmt **ohne**
  Detail weiterhin den bestehenden f64-Kernelpfad. Die Neutralität eines
  gespeicherten Sharpening-Blocks ist dabei **genau eine** Frage — `amount == 0`,
  also der eigene Early-Return des globalen F-095-Stages. Daraus folgt beides:
  Ein `masking = 0` ist **kein** stiller No-op (Masking ist nur der
  Flat-Area-Multiplikator `((1−masking) + masking·edge)`; bei `masking = 0` ist der
  Faktor für **jedes** Pixel `1`, also die stärkste Einstellung — der ganze Frame
  inklusive Fläche wird geschärft), und der Radius kann einen Block **nie** neutral
  machen: der globale Kernel begrenzt `sigma` nach unten auf `0.5`, und selbst
  dort hat der erste Nachbartap die Gewichtung `exp(−1/(2·0.5²)) = exp(−2) ≈
  0.135 ≠ 0`, erreicht also das separierbare Kernel-Fenster die Nachbarn — jeder
  Radius im legalen `0.1..=10.0` schärft also etwas. Ein persistierter Block, der
  nur `radius`/`detail`/`masking` setzt und `amount = 0` lässt, ist damit exakt die
  F-095-Identität und ändert kein Byte.
- **Zustand, Reset und Persistenz:** Expliziter Reset **pro Teil-Block**
  (`sharpening`, `noise_reduction`) und **für den gesamten Detail-Block**
  (`detail`). History, Reset und Previous der **aktuellen** virtuellen Kopie
  übernehmen den vollständigen additiven `MaskStateSnapshot` **inklusive des
  kompletten Detail-Blocks**. Der Block gehört in die kanonische
  `LocalAdjustments::digest` und `mask_layers_digest` und damit in die
  Render-Identität. Ein lokaler Layer darf keine globalen Rezeptfelder
  verändern; die lokale Detail-Sektion schreibt nie
  `EditRecipe::sharpening`/`noise_reduction`.
- **Cross-image Previous bleibt recipe-only
  (P0/P1.1/P1.2a/P1.2b/P1.2c-Vertrag):** Der dateibasierte CLI-Befehl
  `previous` überträgt **keine** lokalen Masken-Layer und keine P0-/P1.1-/
  P1.2a-/P1.2b-/P1.2c-/P1.2d-Deltas auf andere Bilder. Nicht-neutraler lokaler
  Maskenzustand — auf der Quell-Kopie oder auf dem Ziel — wird laut verweigert:
  die Quelle bricht den gesamten Lauf ab (Exit 1, kein Ziel angefasst), ein
  betroffenes Ziel schlägt isoliert fehl (Exit 3, Ziel-Bytes unverändert).
  **Ein lokaler Detail-Block allein ist bereits ein Verweigerungsgrund**; es
  gibt keine „nur P0/P1.1/P1.2a/P1.2b/P1.2c"-Ausnahme. Sync Settings bleibt
  recipe-only.
- **GUI/CLI:** Die GUI erhält einen lokalen Detail-Editor (Sharpening
  amount/radius/detail/masking, Noise Reduction luminance/color, Reset pro
  Bereich und gesamt) mit **keiner** globalen
  `EditRecipe::sharpening`/`noise_reduction`-Mutation; Interaktionsvertrag und
  Testanker siehe
  [§ GUI- und kittest-Vertrag der mask-local Editoren](#gui--und-kittest-vertrag-der-mask-local-editoren-p12ad).
  Die CLI nutzt keine
  zweite, fast identische Flag-Partei, sondern den bestehenden generischen
  Kanal: `--set-local-adjustment 'sharpening.<field>=<n>'` /
  `'noise_reduction.<field>=<n>'` und
  `--reset-local-adjustment sharpening | noise_reduction | detail`.
- **Abnahme:** Exakte CPU-Goldens ohne Toleranz für Sharpening (amount, radius,
  detail, masking), Noise Reduction (luminance, color), die Reihenfolge
  NR-vor-Sharpen, den vollen Stack (WB + Basic + Presence + Kurve + Color + NR +
  Sharpen), Halbmaske, Überlappungs-Reihenfolge, Null-Block-Byte-Identität,
  ungültige Werte (Blockversion, Bereich **inklusive der Radiusgrenzen 0.1 und
  10.0**, nicht-endlich), Legacy-v1..v5-Dokumente, die sichtbare Verweigerung
  von AI-Denoise und Optics, CLI/GUI-Parität und History/Previous/Reset. Die
  **globalen** Sharpening-/Noise-Reduction-Goldens müssen **unverändert** grün
  bleiben. AI-Denoise und Optics bleiben deaktiviert.

## GUI- und kittest-Vertrag der mask-local Editoren (P1.2a–d)

**Feature-ID:** `GUI-INT-MASKLOCAL-38` · Stand 2026-09-26 · Release 1.0

> **Warum dieser Abschnitt existiert.** P1.2a–d wurden **headless** geliefert
> (Schema, `lumina-core`, `lumina-cli`, `lumina-sidecar`). Die mask-local
> Editoren sind aber sichtbare Funktionen, und `Agents.md` verlangt für jede
> sichtbare Funktion einen klickbaren headless GUI-Test **und** einen
> kittest-Golden. Dieser Abschnitt legt den GUI- und kittest-Vertrag fest, **bevor**
> die Integration implementiert wird — die Semantik oben (Reihenfolge, Versionen,
> Reset, Persistenz) bleibt davon unberührt.

### 1. Verdrahtung: ein Modul ohne `draw_`-Aufruf ist nicht integriert

- Jeder der vier mask-local Editoren hat **genau einen** `draw_*`-Pfad, und der
  wird aus **derselben** aufrufenden Stelle in `develop_masking.rs`
  (`LuminaApp::draw_masking`, innerhalb des `selected_mask_id.is_some()`-Zweigs)
  aufgerufen — in dieser Reihenfolge und mit `ui.separator()` dazwischen:
  `draw_mask_local_tone_curve` (P1.2a) → `draw_mask_local_color` (P1.2b) →
  `draw_mask_local_presence` (P1.2c) → `draw_mask_local_detail` (P1.2d). Die
  Reihenfolge der vier Aufrufe folgt der **Kernel-Reihenfolge** der Maske
  (Presence → Kurve → Color → Detail ist die Render-Reihenfolge; die Panel-
  Reihenfolge ist eine reine Lese-Reihenfolge und darf die Kernel-Reihenfolge
  nicht suggerieren, indem sie sie umstellt — sie steht deshalb in der
  historischen Panel-Reihenfolge Tone Curve → Color → Presence → Detail und ist
  als reine Anzeigereihenfolge dokumentiert).
- **Ohne ausgewählte Maske** wird kein mask-local Block gezeichnet. Das ist der
  einzige Sichtbarkeits-Gate; es ist derselbe Gate, den die übrigen lokalen
  Regler (Exposure/Contrast/…) benutzen, und kein stilles Leerenzeichen.
- Kein mask-local Block ist ein `#[cfg(test)]`-Pfad und keiner malt in einen
  Frame, der nicht die reale `draw_masking`-Kette durchläuft.

### 2. Interaktionsvertrag: anklickbar, nicht nur paintbar

Für **jede** sichtbare Bedienung eines der vier Editoren gilt: der Bedienweg ist
ein echtes egui-Widget, und ein Klick/drag darauf schreibt über den
**produktiven Setter** (`set_mask_local_curve_channel` /
`set_mask_local_hsl_band` / `set_mask_local_vibrance_saturation` /
`add_mask_local_point_color` / `set_mask_local_grading_field` /
`set_mask_local_presence` / `set_mask_local_sharpening` /
`set_mask_local_noise_reduction` bzw. der jeweilige `reset_*`) in das Rezept der
**ausgewählten Masken-Layer**. Konkret normativ:

- **Kurvengraph (P1.2a)** — der mask-local Graph benutzt **denselben
  Gesten-Decoder** wie der globale (`curve_graph_gesture`), dieselbe
  Hit-Radius-/Mindestabstands-Regel und dieselben Pflicht-Endpunkte
  `(0,0)`/`(1,1)`. Damit gilt der globale UXG-16-Vertrag wortgleich:
  * **Setzen:** ein Klick auf die gezeichnete Kurve fügt an der geklickten
    Input-Position einen Stützpunkt ein (die Kurve bleibt visuell zunächst
    unverändert); ein Klick **abseits** der Kurve ist ein No-Op.
  * **Verschieben:** ein Drag auf einem inneren Punkt verschiebt ihn; der Input
    wird strikt zwischen den Nachbarn geklemmt, der Output auf `0..=1`.
  * **Löschen:** ein Doppelklick auf einem inneren Punkt entfernt ihn.
  * **Endpunkte** sind weder verschiebbar noch löschbar; jeder Versuch wird
    **laut** verweigert (Statuszeile, kein Save) — nie stillschweigend ignoriert.
  * Der mask-local Graph hat eine **eigene, stabile Widget-Id**
    (`lumina.local_tone_curve_graph.<channel>`), damit er im selben UI-Baum
    neben dem globalen Graph keine egui-Interaktionszustände teilt.
    **Anker (Verifikationsbefund F-2):** ein Id-Vergleich wäre der
    „Konstante gegen sich selbst"-Defekt, also wird das **Verhalten** gefahren —
    `tests/mask_local_editors_wiring.rs::the_local_and_global_curve_graphs_do_
    not_share_interaction_state` öffnet Masking **und** Tone Curve nebeneinander,
    zieht den **globalen** Graphen und prüft, dass die lokale Kurve unverändert
    bleibt, und umgekehrt — jeweils ausgehend von einem **nicht-neutralen**
    Zustand der anderen Seite. Mutation: lässt der globale Graph die lokale Id
    benutzen, wird der Test rot.
  * Die **Kanalwahl** Master/R/G/B ist eine echte Auswahl-Reihe
    (`selectable_label`) und schaltet den dargestellten und bearbeiteten Kanal
    um; ein Kanalwechsel ist eine reine Ansichtsumschaltung und schreibt
    **nicht** in das Rezept.
  * **Reset** existiert zweimal: pro Kanal und für alle lokalen Curves. Beide
    sind vollständige Editor-Transaktionen (History + Save), keine
    Anzeige-Aktion.
- **Regler und Buttons (P1.2b/c/d)** — HSL-Bandwahl, Grading-Range-Wahl,
  Vibrance/Saturation, Point-Color-Hinzufügen/Entfernen, Presence
  (Texture/Klarheit/Dehaze), Sharpening (Amount/Radius/Detail/Masking),
  Noise Reduction (Luminance/Color) sowie alle Reset-Buttons sind echte
  egui-Widgets mit obigem Schreibpfad. Die angebotenen Wertebereiche sind
  **exakt** die globalen Bereiche (siehe die Area-SOLL), damit der Editor nie
  einen Wert anbieten kann, den der Validator verweigert.
- **Kein globaler Seiteneffekt:** nach jedem mask-local Klick gilt
  `recipe().curves.is_none()`-entsprechend: das globale Rezept
  (`EditRecipe::curves`/`hsl`/`point_color`/`color_grading`/`presence`/
  `sharpening`/`noise_reduction`/`adjustments`) ist **strukturell
  unverändert**. Die Zusage ist bewusst **kein** Bytevergleich: die Tests
  vergleichen mit `assert_eq!(app.recipe(), &before)` — ein abgeleitetes
  `PartialEq` über die `EditRecipe`-Felder — und prüfen zusätzlich die
  einzelnen globalen Felder auf `None` bzw. auf Abwesenheit im flachen
  `adjustments`-Map. *Nicht* behauptet wird: Byte-Identität des globalen
  Blocks im persistierten Sidecar. Das ist eine **Zusicherung**, keine
  Beobachtung — ein Test, der sie nicht prüft, deckt die Editor-Transaktion
  nicht ab.
- **Lauter Fehler statt stillem No-Op:** ein Refused Edit (unbekannter Kanal,
  Endpunkt, ungültige Punktliste) verlässt den Masken-Layer und den
  ausstehenden History-Snapshot **strukturell** unverändert und setzt eine
  sichtbare Statuszeile. Anker:
  `src/tests/mask_local_curves.rs::invalid_local_curve_edits_are_refused_without_mutating`
  (`assert_eq!` über `active_mask_layers_snapshot()` plus unveränderte
  History-Länge) und
  `src/tests/mask_local_curve_graph.rs::the_local_graph_refuses_to_move_an_endpoint`
  (der Endpunkt-Drag aus dem echten Gesten-Decoder). Auch hier ist „unverändert"
  ein `PartialEq` über die Strukturen, **kein** Bytevergleich der Datei.

### 3. Testabdeckung: zwei Anker je sichtbarer Fläche

Je mask-local Editor gilt verbindlich **beides**:

0. **Zwei Ansprüche, zwei Testziele.** „Wird gezeichnet" (Paint-Provenienz:
   jeder Editor wird von seinem **eigenen** `draw_*`-Pfad aus der echten
   `draw_masking`-Kette gezeichnet, und die zum Ansteuern benutzten Labels
   gehören dem **lokalen** Editor, nicht dem globalen Develop-Abschnitt) und
   „ist anklickbar" (Eingabevertrag) sind verschiedene Ansprüche und liegen
   deshalb in verschiedenen Zielen: `tests/mask_local_editors_wiring.rs`
   (Paint-Provenienz plus der Graph-Vertrag UXG-16, dessen Wert eine Punktliste
   und kein Skalar ist, plus die Id-Unabhängigkeit beider Graphen) und
   `tests/mask_local_editors.rs` (Presence/Color/Detail, deren Wert ein Skalar
   ist). Ein Miswire bricht das eine Ziel, ein nicht anklickbares Widget das
   andere. Zwei weitere Ziele kamen in der Verifikationsrunde dazu:
   `tests/mask_local_color_controls.rs` (die fünf vorher ungeklickten Controls
   der Farb-Fläche, DoD §3) und `tests/mask_local_reload.rs` (das Reload-Glied
   der Kette aus DoD §1). Geteilte Testhilfen liegen in
   `tests/mask_local_editors_support/` (Harness, Frame-Uhr, Label-Lookups,
   Persistenz-Readback), `tests/mask_local_slider_support/` (Regler-Drags),
   `tests/mask_local_label_support/` (richtungsbasierte Label-Lookups) und
   `tests/mask_local_curve_graph_support/` (Graph-Geometrie und -Gesten); keine
   Datei dieser vier Module wird von `#[allow(dead_code)]` am Liveness-Gate
   vorbeigetragen, jede wird nur von ihren Verbrauchern deklariert.
1. **Klickbarer headless Test** (läuft in `cargo test -p lumina-gui`, **ohne**
   GPU): `egui::Context` + `LuminaApp` im tempdir bzw. der 4×3-Smoke-PNG, echte
   Zeiger-Events auf die echten Widgets der echten `draw_masking`-Kette, und
   danach die **persistierte** Lage über die öffentlichen Getter
   (`selected_mask_local_*` / `has_mask_local_*` / `recipe()`). Ein reiner
   „das Widget wurde gepaint"-Test ist **kein** Interaktionsnachweis und deckt
   Abschnitt 2 nicht ab.
   **Reihenfolge-Regel (F-4):** auf **jedes Wertgest** (Drag, Feldklick) folgt
   erst `settle_persisted` und **dann** die Datei-Assertion, und erst danach das
   nächste Gest. Bewusst auf Wertgesten eingeschränkt: `mask_local_color_controls.rs`
   hat zwei aufeinanderfolgende `Add color`-Klicks ohne Settle dazwischen, weil
   beide je einen **eigenen** Listenanhang erzeugen, sich also nicht gegenseitig
   überschreiben können. Grund ist
   in 6.1 gemessen: die 150-ms-Uhr des entprellten Saves wird von jedem Edit neu
   gestartet, eine Datei-Assertion am Testende könnte also von einem *späteren*
   Gest erfüllt werden, ohne dass das geprüfte Gest etwas gespeichert hat.
2. **Reload-Glied (F-6, DoD §1):** `tests/mask_local_reload.rs` schließt die
   Kette `Edit → Commit/Debounce → Sidecar-Datei → Reload → Wert wieder da`:
   ein **zweites** `LuminaApp` über demselben tempdir liest den Wert über den
   Produktionspfad (`open_file` → Decode → `finish_decode` adoptiert das
   Sidecar, löst die virtuelle Kopie **per Identität** und selektiert die
   persistierte Layer). Kein zweites `create_mask`, denn ein frisch angelegter
   Layer würde die Aussage entwerten. Geprüft wird für **beide** Eingabearten —
   ein gezogener Skalar (Vibrance/Saturation) **und** ein geklickter Button
   (Point-Color-Eintrag, incl. stabiler id) — und die neu geladene Layer muss
   danach wieder beschreibbar sein.
3. **kittest-Golden** im feature-spezifischen Target
   `crates/lumina-gui/tests/kittest_mask_local.rs` bei `1024 × 720`, mit dem
   `#[ignore]`-Grund `headless GPU required; …` — also **lokal**es macOS-Gate
   über den wgpu-Adapter und in CI **nie** verifiziert (kein GPU-Runner). Kein
   Golden schreibt einen Decode-Fehler als Soll fest. **Anker-Korrektur
   2026-09-26 (Verifikationsbefund 13, vorbestehend seit `2e9827f`):** dieser
   Wächter heißt im Target `assert_no_decode_failure` (lokale Kopie in
   `kittest_mask_local.rs:181`), **nicht** `assert_no_raw_decode_failure` — das
   ist der Helper in `kittest_fixtures_support`, den dieses Target nicht
   aufruft. Die Funktion ist damit gemeint, der Name war falsch. Die
   Mask-local-Editoren laufen auf der S2-Smoke-Fixture und sind damit
   **Chrome-/Layout-Invarianten der Klasse C**, **keine** Render-Invarianten
   und **kein** Beleg für eine Bildpipeline-Regression.

### 4. Aktions-Audit: die mask-local Editoren sind **bewusst außerhalb**

**Korrektur 2026-09-26 (Verifikationsbefund F-1).** Eine frühere Fassung dieses
Abschnitts behauptete, die mask-local Editoren liefen „über die vorhandenen
typisierten Setter und den bestehenden `instrument_gui_action!`-Pfad der
Maskenaktionen". Das ist **falsch und war messbar falsch**:
`grep instrument_gui_action` über `mask_local_color.rs`, `mask_local_curves.rs`,
`mask_local_presence.rs`, `mask_local_detail.rs`, `mask_local_controls.rs` und
`develop_masking.rs` liefert **null** Treffer.

Was der Code tatsächlich tut:

- **Keine `GuiAction`-Variante.** Die vier Editoren erzeugen **keine** Aktion
  aus `ALL_GUI_ACTIONS`; sie sind damit **nicht** Teil des F-100-Audits
  („jede Aktion hat einen Shortcut und einen Button") und mussten die
  110-Aktionen-Pinnung in `gpu_audit_exception_table_is_complete_without_gpu`
  **nicht** anfassen.
- **Kein `instrument_gui_action!`.** Das Makro ist ein **Debug-Jank-Timer**
  (`#[cfg(debug_assertions)]` plus `#[cfg(all(feature = "janklog",
  debug_assertions))]`); in einem Release-Build expandiert es auf **nichts**. Es
  ist **nicht** der Save-Pfad und nichts, was `ALL_GUI_ACTIONS` definiert.
- **Stattdessen:** die typisierten Setter (`set_mask_local_*` / `reset_mask_local_*`)
  loggen selbst mit `info!("GUI interaction: local …")` (DoD §4: mindestens
  `info!` für jede user-sichtbare Aktion) und armen über
  `mark_recipe_dirty(action, 0.0)` — also `pending_slider_commit` plus
  `mark_dirty()`. Das ist derselbe Eintieg wie bei jedem anderen Rezept-Edit
  und erfüllt `GUI-SLIDER-SAVE-1` (CAS, entprellter Save, reine View-Edits
  schreiben nichts).

**Warum der Ausschluss gewollt ist.** `GuiAction`/`ALL_GUI_ACTIONS` ist die
**globale Kommandofläche** (Tastatur-Shortcut + Button je Aktion, F-100). Ein
mask-local Regler ist kein Kommando, sondern ein Panel-lokales Widget an genau
einer ausgewählten Masken-Layer. Eine `GuiAction`-Variante pro Feld würde für
alle masken-lokalen Felder einen Shortcut **und** einen Button erzwingen — das
ist nicht der Zweck des Audits, sondern eine Umgehung seiner Absicht.

**Was der Ausschluss kostet (laut benannt, nicht beschönigt):** ein
mask-local Edit erzeugt **keinen** Jank-Record und taucht in keiner
Action-Liste auf. Wer die Vollständigkeit der Bedienoberflächen-Aktionen
prüft, sieht die 47 Controls dieses Abschnitts nicht. Das ist eine bewusste
Lücke der *Aktions*-Instrumentierung, **keine** Lücke der *Test*-Abdeckung —
die Bedienbarkeit ist je Control in §6 klassifiziert und durch Klick-Tests
belegt.

Führt eine Änderung an diesem Vertrag doch eine `GuiAction`-Variante ein, ist
`ALL_GUI_ACTIONS` (aktuell 110 Aktionen) mitzupflegen **und** der
F-100-Button-Audit (`f100_shortcut_audit_every_action_has_a_button`) muss
weiterhin grün bleiben.

### 5. Ausdrückliche Grenze der Prüfbarkeit

Es gibt **keinen** echten Fenster-/Interaktions-Test, und keiner wird verlangt:
kein Abnahmekriterium darf ein real gerendertes Fenster, echte Maus-/Zeiger-
ereignisse auf dem Desktop, DPI- oder Multi-Monitor-Verhalten oder einen
Sichtcheck durch einen Menschen voraussetzen. Geprüft wird headless
(`egui Context` + `LuminaApp`) und pixelweise (`egui_kittest` + wgpu-Adapter).
Der einzige Weg zu einem real gestarteten Prozess ist der manuelle
Akzeptanz-Run nach R5-LOG-1 (`RUST_LOG=trace`, genau eine App-Instanz,
Log-Redirect) — das ist eine User-Aktion und **kein** Ersatz für die
headless-Anker. Was headless prinzipiell nicht belegt, ist als **laut
benannte Lücke** zu führen, nicht zu beschönigen.

### 6. Erreichter Stand, Vollständigkeits-Klassifikation und Grenzen

#### 6.1 Verifikationsstand (Stand 2026-09-26, nach F-1…F-8)

- **Verdrahtung war kein offener Punkt.** `draw_mask_local_tone_curve` ist
  bereits aufgerufen (`develop_masking.rs`, Commit `1c490b7`); es fehlten
  ausschließlich der klickbare Nachweis und die Goldens.
- **Geteilte Testhilfen ohne Liveness-Ausnahme** (Modulgrenze, siehe Punkt 0):
  Der erste Entwurf hatte eine Support-Datei mit allen Helfern; die Testziele
  ließen 13 bzw. 6 Helfer ungenutzt, was das Clippy-Gate `-D warnings` reißt.
  Die Aufteilung folgt der tatsächlichen Nutzung, nicht einer `allow`-Regel. In
  dieser Runde kam eine vierte Schnittstelle hinzu
  (`tests/mask_local_label_support/`), weil die Farb-Ziele dieselben
  richtungsbasierten Label-Lookups brauchen wie das Kurven-Ziel.
- **Nicht-Vakuums-Nachweis je neuem Test** (alle mit einer Mutation am
  **Produktionspfad** belegt, nicht durch Umbenennung):
  | Test | Mutation | Ergebnis |
  |---|---|---|
  | `the_local_and_global_curve_graphs_do_not_share_interaction_state` | der globale Graph benutzt `local_tone_curve_graph_id` | **rot** (der globale Gest landet nirgends) |
  | `the_remaining_mask_local_color_controls_…` | `local_color_slider` schreibt nicht mehr | **rot** (`got 0`) |
  | dieselbe | Per-Band-Reset räumt den **ganzen** HSL-Block | **rot** (`must NOT clear the other bands`) |
  | dieselbe | `Remove` ignoriert seine Eintrags-id | **rot** (`Remove must delete exactly one entry`) |
  | dieselbe | `Remove` löscht immer den **ersten** Eintrag | **rot** |
  | dieselbe | Per-Bereich-`Reset` ruft den Gesamtblock-Reset | **rot** (`must NOT clear the other ranges`) |
  | `a_persisted_mask_local_edit_is_restored_…` | `finish_decode` selektiert die persistierte Layer nicht | **rot** — die drei anderen *mask-local* Ziele bleiben grün. **Zusätzlich** werden drei **vorbestehende** Lib-Tests rot (`brush_lifecycle::invert_uses_the_automatic_slider_save_path_and_reloads`, `brush_management::copy_selection_and_reload_ignore_cross_copy_layers`, `mask_local_previous::previous_transfers_full_local_state_but_sync_keeps_target_layers`); das ist **mehr** Abdeckung, kein Defekt — das fette „nur" wäre irreführend gewesen. |
  | alle vier Interaktionstests | die vier `draw_mask_local_*`-Aufrufe einzeln auskommentiert | **rot**, 1:1 pro Editor |
  | die vier Goldens | `scroll_into_view`-Anker entfernt | **rot** (der Golden darf keinen Editor zeigen) |
- **`SIDECAR-SAVE-STRAND-39`: die gemeldete Ursache ist widerlegt, eine andere
  ist gemessen.** Der ursprüngliche Bericht führte den Save-Verlust auf einen
  Draft-Tick-**Hochstuf** zu einem Vollrender zurück (mask-local Edit →
  `render_tick.rs` „absolute-frame or local-mask stage active"). Der Hochstuf
  ist real und wurde bestätigt — er ist aber **nicht** die Ursache.

  **Ausgeschlossen:** `full_render_debounce_remaining` (`render_tick.rs:29-35`)
  gibt `None`, also *sofortigen* Commit, wenn `last_edit_time <= 0.0` oder das
  150-ms-Fenster abgelaufen ist. Ein **veralteter** Zeitstempel macht den
  Commit also *eifriger*, nicht strandend; der Debounce kann nur verzoegern,
  nie abbrechen. Und `pending_full_render` ist am Release-Frame immer gesetzt,
  weil `slider.rs:203-206` es bei jedem `dragged()`-Frame unbedingt neu bewaffnet.
  Ein „Edit in einem Frame verliert den entprellten Save" gibt es nicht.

  **Tatsächlich gemessen:** `render_schedule.rs:53` haengt den einzigen
  Commit-Pfad an `pending_full_render`. Jeder Vollrender, der nicht durch
  `commit_pending_slider_save` laeuft — der Fusszeilen-Button `Render / Apply`
  und ~20 weitere `render()`-Aufrufstellen — loescht das Flag in seinem Frame
  und macht den bewaffneten Token fuer den Rest der Session unerreichbar.
  Reproduziert als `a_render_apply_click_inside_the_debounce_window_does_not_
  strand_the_save` (rot ohne den Fix: `memory 0 vs disk 0.6000000238418579`).

  **Von dem Verlust unberuehrt** bleibt die F-4-Regel: der Save traegt den
  kompletten Layer-Zustand, also geht **kein** Wert verloren, und „die Datei
  enthaelt meinen Edit *jetzt*" gilt erst nach dem Debounce. Deshalb folgt in allen Interaktionszielen auf
  **jedes Wertgest** ein `settle_persisted` **und** eine Datei-Assertion, bevor
  das nächste Gest kommt (F-4). Die eine benannte Ausnahme — zwei `Add
  color`-Klicks ohne Settle — steht in `tests/mask_local_color_controls.rs` und
  ist im Doc-Kommentar derselben Datei begründet. Ein erster Entwurf von
  `mask_local_color_controls.rs` hatte genau diesen Defekt (kein Settle
  zwischen zwei Drags), und der Trace zeigte, dass der erste Commit sein
  Debounce-Fenster nie bekam.
- **Der `DRAG_STEPS`-Workaround ist entfernt.** Er war als „notwendig"
  dokumentiert; **gemessen** ist das falsch — alle Interaktionstests bestehen
  unverändert mit 0, 1, 2 **und** 3 Zwischenpositionen. Notwendig ist die
  Frame-Trennung von Press/Move/Release, nicht die Schrittzahl. Da
  `SIDECAR-SAVE-STRAND-39` ausdrücklich verlangt, dass die Reproduktion **nicht**
  durch Workaround-Logik erklärt wird („sonst ist die Abdeckung vakuos"), ist
  der Umweg weg.

#### 6.2 Vollständigkeits-Klassifikation **jedes** Controls (DoD §3)

Gezählt werden **35 Bediengruppen** (je ein Widget oder eine Widget-Familie mit
einem Schreibpfad). Zählt man jede einzelne Schaltfläche statt der Gruppe —
8 HSL-Bänder, 4 Kurvenkanäle, 3 Grading-Bereiche einzeln — sind es **47
Widgets**. DoD §3 verlangt keine Stichprobe, sondern die Klassifikation **aller**
Mitglieder; die Spalte „Klick-abgedeckt" nennt den Test, die Spalte „Mechanik"
nennt den Loop/Setter, und die letzte Spalte ist ehrlich gefüllt.

| # | Control | Klick-abgedeckt (Test) | Mechanik | Nur Mechanik / nicht abgedeckt |
|---|---|---|---|---|
| **P1.2a Tone Curve** |||||
| 1 | Kanalwahl M/R/G/B | ja — `mask_local_editors_wiring` (Klick auf „Red") | `selectable_label` + `info!` | — |
| 2 | Graph: Setzen/Verschieben/Löschen | ja — `mask_local_editors_wiring` | `curve_graph_gesture` | — |
| 3 | Reset pro Kanal | ja — `mask_local_editors_wiring` | `reset_mask_local_curve_channel` | — |
| 4 | `all local curves reset` | ja — `mask_local_editors_wiring` | `reset_mask_local_curves` | — |
| **P1.2b Color** |||||
| 5 | HSL-Bandwahl (8 Bänder) | ja — `mask_local_editors` (cyan), `mask_local_color_controls` (cyan, yellow) | `selectable_label` | — |
| 6 | HSL **Hue** | **nein** | Schleife `HSL_FIELDS` → `local_color_slider` → `set_mask_local_hsl_band` | nur Mechanik (Hue/Saturation/Luminance laufen durch **denselben** Helper; Saturation ist geklickt) |
| 7 | HSL **Saturation** | ja — `mask_local_editors` | dito | — |
| 8 | HSL **Luminance** | **nein** | dito | nur Mechanik |
| 9 | Reset pro Band | ja — `mask_local_color_controls` (Band-Scope mit Zeuge) | `reset_mask_local_hsl_band(band)` | — |
| 10 | **Vibrance** | ja — `mask_local_color_controls` | `local_color_slider` → `set_mask_local_vibrance_saturation` | — |
| 11 | **Saturation** (Vibrance-Paar) | ja — `mask_local_color_controls` (negativ gezogen) | dito | — |
| 12 | Point Color **Remove** | ja — `mask_local_color_controls` (2 Einträge, der **zweite** Button) | `remove_mask_local_point_color(id)` | — |
| 13 | Point Color `Add color` | ja — `mask_local_editors`, `mask_local_reload` | `add_mask_local_point_color` | — |
| 14 | Reset Point-Color-Block | **nein** (Button nicht geklickt) | `remove_mask_local_point_color("")` | nur Mechanik: derselbe Effekt wie der geklickte Gesamtblock-Reset auf diesen Block — `remove_mask_local_point_color("")` verzweigt auf `recipe.reset_local_point_color()` (`mask_local_color.rs:206-212`), und `reset_local_color()` ruft dieselbe Funktion (`color_grading.rs:291`). Dass dieser Pfad **persistiert** funktioniert, ist belegt: `mask_local_editors.rs:215-232` fuellt die Liste und weist `Some(1)` auf der Platte nach, `:277-278` klickt den Gesamtblock-Reset, `:302` weist `persisted.point_color.is_none()` nach. (Korrektur 2026-09-26: eine Zwischenfassung dieses Satzes behauptete, die Liste sei **zuvor** geleert und `:302` sei vakuos. Beides falsch — `:197-203` ist die globale-HSL-Assertion, und `:302` ist der einzige persistierte Zeuge. Die Zwischenfassung wurde durch Messung widerlegt und ist hiermit zurueckgenommen.) |
| 15 | Grading-Bereichswahl (3) | ja — `mask_local_editors` (midtones), `mask_local_color_controls` (midtones, highlights) | `selectable_label` | — |
| 16 | Grading **Hue** | ja — `mask_local_editors`, `mask_local_color_controls` | `set_mask_local_grading_field(range, "hue")` | — |
| 17 | Grading **Saturation** | ja — `mask_local_color_controls` (Zeuge) | dito, Feld `"saturation"` | — |
| 18 | Grading **Luminance** | **nein** | dito, Feld `"luminance"` | nur Mechanik (Hue/Saturation/Luminance sind **drei** ausgeschriebene `Slider`-Blöcke mit **derselben** Aufrufform `set_mask_local_grading_field(<range>, "<feld>", wert)`; die Validierung pro Feld ist durch die Lib-Tests der P1.2b-Fassung abgedeckt) |
| 19 | Grading **Balance** | **nein** | `set_mask_local_grading_field("balance", "value", …)` | nur Mechanik — **schwächer als Zeile 18**: Range **und** Feldname sind hier hart kodiert (`"balance"`, `"value"`) statt durchgereicht, also ein anderer Zweig *innerhalb* desselben Setters. Der Annahmepfad ist getestet (`src/tests/mask_local_color.rs:102-105` setzt, `:114-115` liest zurück, `:136-139` belegt das Überleben eines Bereichs-Resets) und der Ablehnungspfad **ebenfalls**, auf Crate-Ebene: `lumina-sidecar/src/tests/local_adjustments_color.rs::local_color_uses_the_existing_global_ranges` verweigert genau `("balance","value",1.2)` und `("blending","value",1.2)` **und** das unbekannte Target `("whites","hue",30.0)` und belegt `local.color_grading == before`. Die Zeile heißt `nein`, weil der **Klick** fehlt — nicht, weil die Semantik ungetestet wäre. (Korrektur 2026-09-26: eine frühere Fassung behauptete hier, der Ablehnungspfad sei ungedeckt, und dass der genannte Test kein unbekanntes Target kenne. Beides ist falsch; dieser Absatz war zweimal falsch.) |
| 20 | Grading **Blending** | **nein** | `set_mask_local_grading_field("blending", "value", …)` | dito wie Zeile 19 |
| 21 | Reset pro Bereich | ja — `mask_local_color_controls` (Bereichs-Scope mit Zeuge) | `reset_mask_local_grading(range)` | — |
| 22 | `all local color reset` | ja — `mask_local_editors` | `reset_mask_local_color` | — |
| **P1.2c Presence** |||||
| 23 | Texture | **nein** | `set_mask_local_presence` | nur Mechanik (drei Regler in **einer** `for`-Schleife über `PRESENCE_FIELDS`; angeklickt ist davon **nur** Dehaze, Zeile 25) |
| 24 | Clarity | **nein** | dito | nur Mechanik (`PRESENCE_FIELDS = ["texture", "clarity", "dehaze"]`, `mask_local_presence.rs:29`: Clarity ist das **zweite** Element, Dehaze/Zeile 25 das dritte) |
| 25 | Dehaze | ja — `mask_local_editors` | dito | — |
| 26 | `all local presence reset` | ja — `mask_local_editors` | `reset_mask_local_presence` | — |
| **P1.2d Detail** |||||
| 27 | Sharpening **Amount** | ja — `mask_local_editors` | Schleife `["amount","radius","detail","masking"]` → `set_mask_local_sharpening` | — |
| 28 | Sharpening **Radius** | **nein** | dieselbe Schleife | nur Mechanik |
| 29 | Sharpening **Detail** | **nein** | dieselbe Schleife | nur Mechanik |
| 30 | Sharpening **Masking** | **nein** | dieselbe Schleife | nur Mechanik |
| 31 | Noise Reduction **Luminance** | ja — `mask_local_editors` | Schleife `["luminance","color"]` → `set_mask_local_noise_reduction` | — |
| 32 | Noise Reduction **Color** | **nein** | dieselbe Schleife | nur Mechanik |
| 33 | `local Sharpening reset` | ja — `mask_local_editors` (nur Sharpening, NR bleibt) | `reset_mask_local_detail_field("sharpening")` | — |
| 34 | `local Noise Reduction reset` | **nein** | `reset_mask_local_detail_field("noise_reduction")` | nur Mechanik: **derselbe** Aufruf in **derselben** `for`-Schleife, anderer `field`-String; die Bereichs-Semantik ist am Sharpening-Fall belegt |
| 35 | `all local detail reset` | ja — `mask_local_editors` | `reset_mask_local_detail` | — |

**Bilanz:** 22 der 35 Gruppen sind angeklickt; **13 sind es nicht** und sind
oben mit ihrer Mechanik benannt. Kein Element der letzten Spalte behauptet
Klick-Abdeckung. (Korrektur 2026-09-26 nach dem Verifikationsbefund: Zeile 24
fuhr zuvor „ja — `mask_local_reload`", obwohl dieser Test **weder**
Presence **noch** Clarity anfasst — `grep -c` auf `mask_local_reload.rs` ergibt
0. Clarity wird von keinem GUI-Test angeklickt. Damit sind es 22/13, nicht
23/12.)

Für **elf** der dreizehn gilt der ehrliche Nachweis: sie teilen sich **exakt**
den Helper/Setter mit einem geklickten Geschwister, und eine Mutation des
gemeinsamen Aufrufs oder eine Vertauschung der Feldindizes macht die Abweichung
sichtbar (Tabelle unten). Für diese elf wäre ein zusätzlicher Drag-/Click-Test
der von der Testabdeckungs-Politik verlangte **unnötige** Test.

**Für zwei gilt er nicht: die HSL-Bänder Hue und Luminance (Zeilen 6 und 8).**
Bei ihnen macht eine Mutation die Abweichung **nicht** sichtbar (0↔2 bleibt
grün), und kein Test klickt ihre Schiene. Sie brauchen also **echten**
Click-Test — das ist die eine echte Lücke, die diese Tabelle offenlegt, und sie
ist hier benannt statt weggeredet. (Korrektur 2026-09-26 nach
Verifikationsbefund N1: der vorige Absatz behauptete für **alle** dreizehn, ein
Mutationsversuch mache die Abweichung sichtbar, oder er seien sonst
unnötige Tests. Für elf stimmt das, für Zeilen 6 und 8 ist es falsch.)

Geschwister. Wie gut das den Index→Feld-Zusammenhang festnagelt, ist **pro
Schleife gemessen** und nicht gleich:

| Schleife | angeklicktes Geschwister (Index) | Mutation: Feldindizes vertauscht | Ergebnis |
|---|---|---|---|
| Color (Z. 16/17/18, 20) | Hue (0), Saturation (1) | `local_color_slider` schreibt nicht mehr | **rot** (`got 0`) |
| HSL (Z. 6/7/8) | Saturation (**1**) | `HSL_FIELDS` 0↔1 | **rot** (`mask_local_color_controls.rs:225`) |
| HSL (Z. 6/8) | — | `HSL_FIELDS` **0↔2** | **grün, 926/926** |
| Presence (Z. 23/24) | Dehaze (2) | `PRESENCE_FIELDS` 0↔2 | **rot** (`mask_local_editors.rs:87`) |
| Sharpening (Z. 28/29/30) | Amount (0) | 0↔3 | **rot** (`mask_local_editors.rs:325`) |
| Noise Reduction (Z. 32) | Luminance (0) | 0↔1 | **rot** (`mask_local_editors.rs:360`) |

**Die eine gemessene Lücke der Tabelle: die HSL-Bänder Hue und Luminance
(Zeilen 6 und 8).** Der Saturation-Klick pinnt nur Index 1, eine Vertauschung
0↔2 zwischen Hue und Luminance bleibt unsichtbar, und **kein** GUI-Test klickt
die HSL-`Hue`- oder HSL-`Luminance`-Schiene (`grep` über `crates/lumina-gui/tests/`
findet keinen solchen Klick). Für **alle** anderen `nein`-Zeilen ist der
Index→Feld-Zusammenhang durch das angeklickte Geschwister messbar gedeckt.

(Korrektur 2026-09-26 nach Verifikationsbefund N1. Zwei frühere Fassungen
dieses Absatzes waren **beide** falsch und in entgegengesetzte Richtungen: die
eine behauptete, Presence- und Detail-Schleifen seien gar nicht nachweisbar —
sie sind es (alle drei Mutationen rot); die andere behauptete, der
Saturation-Klick decke jede Vertauschung der `HSL_FIELDS`-Indizes — er deckt
nur 0↔1, nicht 0↔2. Der Satz ist jetzt aus den Mutationen abgeleitet statt
aus einer Annahme über die Form des Codes.)

#### 6.3 Ausdrückliche Grenze der Prüfbarkeit (zusätzlich zu §5)

- **Kein echter Fenster-Test.** DPI, Multi-Monitor, echte Maus-Hardware und ein
  für den Menschen sichtbares Bild sind **nicht** belegt — und werden nach
  `Agents.md` auch nicht verlangt. Alles Above-the-Fold in den Goldens ist
  Layout, nicht Interaktion.
- **Bekannte Grenze der Goldens (Klasse C, Layout) — je Golden einzeln, nicht
  pauschal (Korrektur 2026-09-26, Verifikationsbefund F-5).** Eine frühere
  Fassung behauptete pauschal, die Color-Grading-Zeilen und die Reset-Buttons
  der unteren Blöcke lägen unter dem Fold. Das war **falsch und in beiden
  Richtungen**: es unterstellte weniger Sichtbarkeit, als die Frames zeigen.
  Gelesen aus den vier committeten PNGs (Vision-Pass, siehe 6.4):
  * `mask_local_tone_curve.png` — sichtbar: Channel-Reihe (Master/R/G/B),
    Graph mit beiden Pflicht-Endpunkten, Per-Kanal-`Reset`,
    `all local curves reset`, Readout `local curves.master: 2 pts, mid 0.500`.
    Unter dem Fold: der HSL-Block.
  * `mask_local_color.png` — sichtbar: HSL-Caption, alle acht Bandknöpfe,
    Hue/Saturation/Luminance, Per-Band-`Reset`, Vibrance, Saturation,
    `Point Color`-Caption, `Add color`, `Color Grading`-Caption, Bereichs-Reihe
    **und die `Hue`-Zeile**. Unter dem Fold: Grading-`Saturation`/`Luminance`/
    `Balance`/`Blending`, der Per-Bereich-`Reset` und `all local color reset`.
  * `mask_local_presence.png` — sichtbar: der **ganze** Presence-Block inklusive
    `all local presence reset` — **und** der Detail-Block bis auf seine beiden
    unteren Reset-Buttons: `Detail`-Caption, alle vier Sharpening-Zeilen,
    `Noise Reduction` mit beiden Zeilen sowie `local Sharpening reset`. Unter dem
    Fold: nur `local Noise Reduction reset` und `all local detail reset`.
    (Korrektur 2026-09-26 nach eigenem Framework-Lesen des PNG: die frühere
    Fassung schrieb „darunter der Detail-Block" und **unterstellte damit weniger
    Sichtbarkeit, als das Frame zeigt** — genau die Fehlerrichtung, die F-5
    abstellen sollte.)
  * `mask_local_detail.png` — der **ganze** Detail-Block inklusive **aller drei**
    Reset-Buttons (`local Sharpening reset`, `local Noise Reduction reset`,
    `all local detail reset`). Die Behauptung, die unteren Reset-Buttons lägen
    unter dem Fold, war für dieses Golden **falsch**.
  * Die Point-Colour-Controls `Remove` und der Point-Colour-Block-`Reset`
    sind in **keinem** Frame sichtbar — aus einem **anderen** Grund: sie werden
    nur bei nicht-leerer Eintragsliste gezeichnet (`!entries.is_empty()`), und
    die Goldens enthalten bewusst keinen Edit. Ihr Bedienpfad ist in
    `mask_local_color_controls.rs` abgedeckt.
- **Ein hue-only Grading-Block wird nicht persistiert — und das ist
  Absicht, kein Defekt.** `color_grading_is_neutral` ist das *Pixel*-Prädikat
  und ignoriert `hue_degrees` bewusst (ein Hue wählt nur den Tint; der Kernel
  überspringt `saturation == 0`). Ein Block, der nur einen Hue trägt, ist
  pixelneutral und wird deshalb nicht abgelegt — **global wie mask-local**
  (dieselbe Prädikat-Funktion). Praktische Folge: nach einem reinen
  Hue-Drag ist der Block nach einem Reload weg, sobald die Range neutral
  wird. Der Hue überlebt nur zusammen mit Saturation/Luminance. Das ist beim
  Testen sichtbar geworden und wird hier festgehalten, damit es niemand als
  Datenverlust missversteht.
- **Folgekonflikt, nicht mask-local-spezifisch (nicht hier behoben):**
  `SIDECAR-SAVE-STRAND-39`, im Kern in 6.1 gemessen. Der gemeinsame
  Render-/Save-Pfad ist betroffen, nicht die Editoren; die Aufgabe bleibt dort
  und wird **nicht** von einem Test-Helper umgangen.

#### 6.4 Vision-Pass der vier Goldens (DoD §6, 2026-09-26)

Alle vier committeten Frames wurden vor der unabhängigen Verifikation einzeln
gelesen. Befunde, keiner blockierend:

* **Kein Layout-Defekt**: keine Überlappung, kein abgeschnittenes Panel, kein
  fehlendes oder versetztes Element; die Spalten Navigator/Preview/Rechtes
  bleiben intakt, und der gepinnte Editor ist überall dort vollständig
  gerendert, wo er beansprucht wird.
* **Der Titelbanner `Warning: mask unavailable (layer layer-mask-<hash>), it is
  not applied in the preview` ist eine bewusst akzeptierte Baseline.** Er bleibt
  drin: die S2-Fixture hat eine Masken-Layer **ohne** Masken-Artefakt, das ist
  der wahre Zustand, und das Produktprinzip (Reproduzierbarkeit vor stillem
  Fallback) verlangt, das zu sagen, statt so zu rendern, als wäre eine Maske
  angewendet. Ein Golden, das ihn verbergen würde, pinnte eine Fiktion. Die
  `layer-mask-<hash>`-Id ist ein deterministischer Hash über den festen,
  je Golden verschiedenen Maskennamen — deshalb sind die Bytes über Läufe
  stabil (per `shasum` vor/nach Re-Run belegt).
* `mask_local_tone_curve.png` zeigt zusätzlich die gelbe Statuszeile `local WB
  sample is missing: render an effective source stage first`. Sie gehört zum
  **globalen** Local-White-Balance-Picker, nicht zu einem mask-local Editor,
  und folgt derselben Fallback-Regel: eine Klasse-C-Fixture hat keine effective
  source stage.
* Weil die Vorschau ein **unmaskiertes** Bild zeigt, darf **kein** frame als
  Render- oder Masken-Nachweis zitiert werden. Die Frames beanspruchen Layout.

## G-03 Maskierungs-Parität (LRPAR-G03-MASK, Release 1.0, SOLL)

Lightroom-Vorbild (`.goal/Goal.md` G-03, ~30 %): Masken-Neu (Subject/Sky/
Background/Objects/People + Teile bis Pupille/Sclera), Add/Subtract/Invert/
Duplicate-Kombinatorik im Panel, Color-/Luminance-Range, Show + Color Overlay,
Maskenliste mit Sichtbarkeits-Auge.

- **AI-Auswahl (`AiSelect`, pro Maskendefinition):** `kind` ist
  `subject`/`sky`/`background`/`objects`/`people` (kleingeschrieben
  persistiert, case-insensitiv lesbar via `AiSelectKind::parse`); `detail`
  ist optional und benennt einen Teil (`face`, `hair`, `eyes`, `pupil`,
  `sclera`, `lips`, `teeth`, `skin`, `body` — dokumentierte Startliste,
  technisch jede nicht-leere, getrimmte Zeichenkette ≤ 64 Zeichen ohne
  Steuerzeichen). Das Feld ist additiv-optional (`None` = Legacy-/Geometrie-
  Verhalten, kein Migrationspfad nötig). Regeln (laut, kein stiller
  Fallback): `ai_select` nur auf `source`-Knoten (abgeleitete
  `union`/`intersect`/`subtract`/`invert`-Knoten mit `ai_select` werden
  abgelehnt); eine AI-Maske ohne geladene/inferierte Matte rasterisiert
  **nie** geometrisch — die Auswertung meldet `MissingSourcePayload`
  (sichtbar `missing`/`pending`), selbst wenn ein `prompt` persistiert ist
  (der Prompt bleibt als Modell-Input erhalten, ersetzt aber kein Modell).
  Fehlendes Modell/Artefakt folgt den F-048/F-051-Pfaden (`stale`/`missing`/
  `corrupt` sichtbar, `--mask-policy warn|strict`, GUI-Recalc-Angebot).
- **Color-/Luminance-Range (`MaskPrompt::ColorRange`/`LuminanceRange`):**
  deterministische, modellfreie Rezept-Stufen (reine Funktionen aus
  Quellpixeln + Parametern, kein RNG, keine Wanduhr). `LuminanceRange`
  (`min`/`max`/`feather` in `0..=1`, `min <= max`) nutzt Rec.709-Luminanz
  mit Trapez-Rampen (`ramp = feather·span/2`; `feather = 0` = harter
  Schnitt); `ColorRange` (`hue_center`/`hue_width` in Grad, `sat_*`/`lum_*`
  in `0..=1`, `feather` in `0..=1`) nutzt deterministisches RGB→HSL mit
  Kreisdistanz auf Hue und Box+Rampen auf Sättigung/Luminanz. Der Loader
  (`resolve_mask_planes`) berechnet Range-Ebenen direkt aus dem Frame
  (`MaskResolvedFrom::ComputedDeterministic`, Status `Valid`) — kein Modell,
  kein Cache, keine Re-Inferenz nötig; Parameteränderung invalidiert die
  Ebene implizit (reine Funktion, keine persistierte Matte nötig).
- **Kombinatorik (Panel):** `Add` erzeugt einen `union`-Knoten,
  `Subtract` einen `subtract`-Knoten (`a` zuerst = Basis), `Invert` einen
  `invert`-Knoten (genau eine Referenz), `Duplicate` einen neuen
  Source-Knoten mit kopierter Definition (neue stabile ID, kein geteilter
  Matte-Payload-Verweis außer Deduplizierung über Content-Hash im
  `.zdata`-Container). Zyklen und falsche Stelligkeit lehnen Sidecar-
  Validierung (`validate_mask_graph`) und `MaskGraph::evaluate` laut ab.
  IDs sind stabil (`mask-<blake3>` über Art+Name+Zeitlosem), pro virtueller
  Kopie eindeutig, nie positionsbasiert.
- **Sichtbarkeit / Overlay:** `MaskLayer.visible` (Default `true`,
  pro virtueller Kopie persistiert) ist das Auge der Maskenliste — ein
  unsichtbarer Layer wird in `evaluate_mask_stage` übersprungen (explizite
  Nutzerwahl, daher ohne Warnung, aber mit Zähler-Ausschluss). `Show` +
  Overlay-Farbe sind reiner Session-Display-State (nie Rezept/Sidecar, wie
  G-11): `show_mask_overlay` (Default an) UND-verknüpft mit `OverlayMode`,
  `overlay_color` (Default Rot `[255, 0, 0]`, `0..=255` je Kanal) tönt das
  Matte-Overlay; beide mit `info!`-Log und headless Test-Anker.

## R5-BRUSH-24 — Pinsel-Masken und Maskenverwaltung (Release 1.0, SOLL)

Die User-Entscheidung vom 2026-09-20 („großer Fail“) ist normativ: Das
Maskierungs-Panel muss eine echte Lightroom-nahe Pinsel- und
Mehrfachmasken-Arbeitsfläche bieten, nicht nur API-/Sidecar-Anker. Alle
Änderungen bleiben additiv, deterministisch und nicht-destruktiv; das R5-Spot-
Heal bleibt ein getrenntes, weiterhin exklusiv armiertes Werkzeug.

### Pinsel-Parameter und Live-Cursor

- **Size** ist der normalisierte Quellradius `(0, 1]` (Anzeige in Prozent),
  Default `0.05` (5 %). **Softness/Weichheit** und **Flow/Fluss** liegen jeweils
  endlich in `0..=1`, Defaults `0.0` und `1.0`. Die drei Slider sind nur bei
  armiertem Pinselwerkzeug sichtbar; ungültige Werte werden laut abgelehnt und
  lassen den vorherigen Wert unverändert.
- `[` verkleinert und `]` vergrößert ausschließlich die **Size** des armierten
  Pinselwerkzeugs um den deterministischen Faktor `1 / 1.1` bzw. `1.1`, an den
  gültigen Grenzen geklemmt. Das Key-Event wird nie verarbeitet, wenn ein Widget
  Tastatureingabe anfordert. Bei armiertem Spot-Heal gilt unverändert der
  bestehende R5-Dust-Pfad; Maske und Spot können nie gleichzeitigarmiert sein.
- Jeder neue `BrushMark` speichert Radius, Vorzeichen, Softness und Flow im
  `MaskPrompt::Brush`; die Werte gehören damit zur Maskenidentität und
  überleben Save/Reopen. Alt-Daten ohne die additiven Felder lesen als
  `softness = 0`, `flow = 1` (harte, volle Dabs) und werden nicht still auf
  einen anderen Wert umgerechnet. Die Core-Rasterung und der GPU-Tile-Pfad
  verwenden denselben Kernel: Softness bildet eine deterministische
  Smoothstep-Rampe vom inneren harten Kern bis zum Radius; Flow skaliert die
  Dabe-Abdeckung. Pro Pixel innerhalb des Dabs gilt exakt
  `deckung = round(65535 × Smoothstep × Flow)`,
  `positiv = max(aktuell, deckung)` und
  `negativ = min(aktuell, 65535 - deckung)`. Samples außerhalb des Radius
  sind No-op; insbesondere ist `flow = 0` bei beiden Vorzeichen No-op. Das ist
  bewusst Auswahl statt Alpha-Attentuierung. `softness`/`flow` außerhalb
  `0..=1`, nicht-endliche Werte oder ein leerer Stroke sind harte
  Sidecar-/GUI-Fehler.
- Solange das Pinselwerkzeug armiert ist und der Zeiger über dem Preview liegt,
  malt der echte Preview-Painter **am Zeiger** einen Live-Kreis mit
  `normalisierter Radius × min(Quelle-Breite, Quelle-Höhe) × aktuelle
  Ansicht-Skala`. Fit, Zoom, Pan, DPI und Source-Aspect ändern nur diesen
  Bildschirmradius; der persistierte Quellradius ändert sich nicht. Der Kreis
  wird bei ausgerüsteter Tastatureingabe, außerhalb des Preview-Rechtecks und
  nach Spot-/Crop-Armierung nicht gemalt.

### Mehrfachmasken, Pins und Verwaltung

- Eine virtuelle Kopie besitzt beliebig viele `MaskDefinition`-Einträge. Jeder
  Eintrag hat eine stable ID, einen persistierten `MaskLayer`-Verweis und eine
  eigene Sichtbarkeit sowie lokale Layerwerte. Auswahl verändert nur den
  Session-State und den aktiven Layer-Verweis; sie überschreibt weder Prompt
  noch Sichtbarkeit/Layerwerte einer previously selected Maske. Beim ersten
  Anfassen wird ein fehlender Layer genau einmal angelegt; danach wird er
  wiederverwendet.
- Die Liste zeigt **alle** Masken der aktiven virtuellen Kopie in ihrer
  Sidecar-Reihenfolge mit Name, Status und Nummerierung. Eine Zeile ist eine
  echte klickbare Auswahl. Hat eine Maske einen deterministischen Prompt-Anker,
  entspricht ihre Nummer dem gemalten Edit-Pin; Listen- und Pin-Klick
  laufen beide durch `select_mask`, akzeptieren nur genau einen Treffer und
  ändern bei unbekannter/ungültiger ID nichts. Pins bleiben G-11-Display-State;
  das Anklicken ist kein neuer Brush-Dab.
- Jede Listenzeile besitzt reale, direkt zeichnbare Buttons: **Sichtbarkeit**
  (`MaskLayer.visible`, Default `true`), **Umbenennen**, **Löschen**,
  **Nach oben/Nach unten** und **Duplizieren/Kopieren**. Reihenfolge ist die
  Reihenfolge des `mask_library`-Vektors, Identität bleibt die stabile
  Masken-ID; ein Verschieben an ein Ende ist ein sichtbarer No-op bzw. nur
  einmal wirksam, kein stiller Wrap. Rename validiert Trim/Leer/Name-
  Kollision. Delete verwendet die bestehende lautere Graph-Materialisierung
  (Referenzen/Gruppen werden vor dem Löschen eingefroren, nie dangling). Copy
  bleibt tiefe unabhängige Definition, Duplicate-Group bleibt Pointer-
  Semantik gemäß G-03.
- Jede erfolgreiche Verwaltungsoperation schreibt genau einen normalen
  Sidecar-Stand (bei lokalem Quellpfad), invalidiert die Vorschau und loggt
  `info!`; unbekannte IDs, leere Namen und Validierungsfehler bleiben loud und
  lassen Sidecar, Reihenfolge und Auswahl unverändert. Source-/Originalbytes
  werden nie geschrieben.

### Abnahme und visuelle Coverage

- **Normaler No-GPU-Testlauf (keine Adapter-Voraussetzung):** die CPU-/egui-
  Modell-, Widget-, Layout- und echten Input-Pointer-Anker laufen in
  `cargo test -p lumina-gui` und im `--no-default-features`-GUI-Lauf. Dazu
  gehören `brush_interaction` (Key-Handler, Cursor-Radius/Gating, Preview-Drag
  und Pin-Klick), die Softness-/Flow-Setter-Validierung sowie
  `mask_management_controls_have_headless_structural_action_coverage`. Diese
  Tests benötigen keinen wgpu-Adapter und werden nicht pauschal übersprungen.
- **Ignorierter nativer Visual-Golden:** `mask_management_controls_have_a_representative_kittest_golden`
  ist mit einem konkreten `#[ignore = "native wgpu adapter required; …"]`-Grund
  als 1024×720-Repräsentant markiert. Er armt Brush und rendert die echten
  Size/Softness/Flow-Regler, mehrere Masken, Eye, Rename, Delete, Move und
  Copy/Duplicate über den bestehenden `egui_kittest::snapshot`-Vergleich; der
  beabsichtigte Snapshot ist `tests/snapshots/mask_management_controls.png`.
  Manueller Lauf: `cargo test -p lumina-gui --lib mask_management_controls_have_a_representative_kittest_golden -- --ignored`
  (mit `UPDATE_SNAPSHOTS=true` nur für diesen absichtlich geänderten Snapshot).
  Der Ignore gilt nur für diesen adapterabhängigen Golden, nicht für die
  strukturelle oder die CPU-/No-GPU-Suite.
- **Noch offener Hardware-Gate:** unabhängige Verifizierung auf echter
  Metal- oder Vulkan-Hardware mit DPI/Zoom/Pan, echtem Zeiger-Cursor und
  GPU-Tile-/CPU-Overlay-Parität bleibt erforderlich. Headless CPU-/kittest-
  Anker ersetzen diesen Lauf nicht; R5-BRUSH-24 bleibt bis zu dieser
  unabhängigen Bestätigung unchecked. Bestehende Pixel-/PSNR-Toleranzen und
  Goldens werden nicht geschwächt.

## R5-MASKVIS-25 — Maskenansicht, Overlay-Modi und Fokusfläche (Release 1.0, SOLL)

Die User-Entscheidung vom 2026-09-20 ist normativ: Das Maskierungs-Overlay
ist ein Arbeitsmodus der geöffneten Masken-Ansicht, kein dauerhaft sichtbarer
Zustand der Develop-Vorschau. Die Implementierung muss die bestehende
Brush-/Spot-Interaktion und die persistierten Masken unverändert lassen; der
Panel-Zustand selbst bleibt Session-Display-State.

- **Sichtbarkeits-Gate:** „Masken-Ansicht geöffnet“ bedeutet exakt
  `section_open[SECTION_MASKING] == true`. Ist sie geschlossen, malt der
  Preview-Painter weder das Masken-Matte-Overlay noch die Masken-Edit-Pins —
  auch nicht bei einem gespeicherten Prompt, einem aktiven Brush oder einem
  laufenden Drag. Globale Spot-Edit-Pins bleiben als unabhängige Retusche-
  Anker sichtbar. Das Gate ist eine einzige, headless testbare Vorbedingung
  für den Draw-Pfad; Renderer-/Export-Daten werden dadurch nicht verändert.
  Das Öffnen/Schließen der Sektion ändert weder Rezept noch Sidecar und
  disarmt Brush oder Spot-Heal nicht.
- **Zwei Masken-Overlay-Ansichten:** Ein echter, direkt zeichnbarer Toggle
  schaltet zwischen `PinsOnly` und `SelectedFull` (`SelectedFull` ist der
  Default):
  - `PinsOnly` zeigt die nummerierten Pins aller sichtbaren Masken der
    aktiven virtuellen Kopie, aber kein Masken-Matte.
  - `SelectedFull` zeigt das volle Matte der aktuell ausgewählten Maske
    (inklusive Live-Drag während eines Brush-/Gradient-/Radial-Gests) und
    kennzeichnet den ausgewählten Pin; die Pin-Anker der übrigen
    sichtbaren Masken bleiben
    für den Auswahlwechsel klickbar, werden
    aber nicht als Matte gezeichnet.
  Der Modus ist Session-Display-State, wird weder in Rezept noch Sidecar
  geschrieben und ist nach App-Neustart wieder auf seinem Default. `Show`,
  Overlay-Farbe, Masken-Auge und die globale G-11-`OverlayMode`-
  Kompatibilitätsstufe bleiben zusätzliche multiplicative Gates; ein
  `Never`/`Show=false`/unsichtbares Auge darf keine sichtbare Matte erzeugen.
- **Fokusfläche / Panel-Hide:** Ein realer, klickbarer Toggle blendet die
  linke und rechte Seitenfläche aus (Toolbar-Button sowie `Tab`-Alias; der
  bestehende `Shift+Tab`-/`All Panels`-Schalter blendet zusätzlich Navigator
  und Filmstreifen aus). Der Preview-/Bildbereich wird dadurch in derselben
  UI-Frame tatsächlich größer; `preview_pane_rect` und `preview_screen_rect`
  müssen das nachweisen. Header/Modulleiste und der Maskenansicht-Zustand
  bleiben erreichbar, damit der Toggle nicht in einen Zustand ohne Rückweg
  führt. Der Panel-Toggle verändert weder Zoom-Persistenz, Brush-/Spot-
  Armierung noch Sidecar-Daten.
- **Interaktions- und Persistenzgarantie:** Ein Pin-Klick wählt weiterhin
  genau eine Maske über `select_mask` und erzeugt keinen Brush-Dab; ein
  Brush-Drag und Spot-Heal bleiben auch bei geöffnetem Full-Overlay, Pin-only
  und Panel-Hide pointer- und renderer-seitig funktionsfähig. Dokumentierte
  Brush-/Spot-Maskenwerte bleiben nach Save/Reopen byte-/parameterstabil.
- **Headless und visuelle Abnahme:** mindestens je ein CPU-/egui-Test für
  geschlossene/geöffnete Maskenansicht, `PinsOnly`↔`SelectedFull`,
  Panel-Hide mit nachweisbar vergrößertem Preview-Rechteck sowie Brush- und
  Spot-Regression. Ein absichtlicher, ignorierter Native-Golden bei exakt
  `1024×720` (`tests/snapshots/mask_view_visibility.png`) zeigt den
  Full-Overlay-Modus mit geöffneter Maskenansicht; bestehende Goldens und
  Pixel-/PSNR-Toleranzen bleiben unverändert. Unabhängige Verifikation auf
  echter Metal-/Vulkan-Hardware mit DPI/Zoom/Pan und echtem Zeiger-Cursor
  bleibt bis dahin ein natives Hardware-Gate.

## Implementierungsstatus (F-047 / F-080)

**Stand 2026-08-19 (F-047 Adapter-Crate `lumina-onnx` implementiert):**

- Der austauschbare ONNX-Adapter existiert als native-only-Crate `lumina-onnx`
  (spiegelt `lumina-raw`). Er entlastet `lumina-core` und
  kapselt native Inferenz, Modellverwaltung und Maskenartefakte.
- `ModelManifest` (serde) trägt Modellname, -version, -hash, Lizenz,
  Eingabespezifikation (Auflösung, Kanal-Layout, Tensorname/-format) und
  `ModelCapabilities`.
- `ModelCapabilities` (F-080) bildet `box_prompt`, `point_prompt`,
  `mask_prompt`, `class_detection` und `instance_segmentation` ab;
  `subject_segmentation` ist die Basisfähigkeit. Mindestens eine Fähigkeit muss
  gesetzt sein; unbekannte Felder werden abgelehnt (`deny_unknown_fields`).
- BiRefNet-Deskriptor (`birefnet_manifest`): automatische Subject-Segmentierung,
  ein RGB-Eingang → Alpha-Matte, keine Prompts (nur `subject_segmentation`,
  übrige Fähigkeiten `false`), dokumentierte Inferenzauflösung 1024×1024,
  Lizenz `Apache-2.0` (verifiziert, kein Download).
- Austauschbare Oberfläche über das Trait `SubjectInference`
  (`infer(&ImageFrame) -> Result<MaskPlane, OnnxError>`). Ein deterministischer
  `StubBackend` (zentrierte radiale Matte, rein aus Eingabedimensionen, keine
  Gewichte/Netz) ist die vollständige, getestete Standardoberfläche.
- `OnnxError` (thiserror) kennt `UnsupportedModel`, `InferenceFailed`,
  `InvalidDimensions`, `MissingModel` (keine stillen Fallbacks).
- Reales ONNX-Runtime-Backend ist hinter dem nicht-default Feature `onnx-rt`
  (`ort` v2.0.0-rc.13, in dieser Umgebung baubar) vorbereitet; die
  numerische Validierung gegen echte Modellgewichte folgt in F-048/F-082.

**Folgearbeit (F-048+):** Die Anbindung an Sidecar (Maskenidentität
`ModelIdentity` ↔ `ModelManifest`), CLI (`mask`-Command, `--update-masks`) und
GUI (Capability-Anzeige, Hintergrundberechnung) sowie die Persistenz/
Wiederverwendung/Stale-Erkennung erfolgt in den Folge-Tasks. `lumina-onnx`
hängt bewusst noch nicht von `lumina-sidecar` ab; die Modellidentität wird in
F-048 auf das Sidecar-Modell abgebildet.

**Status (F-048 / F-051, 2026-08-19):** Umgesetzt und unabhängig verifiziert.
Die Masken-Ladeentscheidung (`lumina-core::mask_loader::resolve_mask_planes`)
bildet `ModelIdentity` ↔ `ModelManifest` ab (`ModelManifest::to_model_identity`)
und wählt pro erreichbarer Quell-Maske: gültiges persistiertes Artefakt laden
(keine Re-Inferenz), sonst Re-Inferenz über `lumina-core::MaskInference`
(StubBackend implementiert das Trait; `lumina-onnx` hängt nun von `lumina-sidecar`
für die Identitätsabbildung). F-051 ist integriert: fehlendes/nicht verfügbares
Modell → Cache-Nutzung mit Warnung bzw. harter Fehler bei fehlendem Cache. Die
CLI reicht das Ergebnis an `stderr`/`mask_warnings` durch. Offen: Persistenz der
Re-Inferenz-Ergebnisse zurück ins `.lumina.zdata`-Bundle (F-082) und die
GUI-Capability-Anzeige.

**Status (F-050, 2026-08-20):** Umgesetzt und unabhängig verifiziert. Das
Entscheidungsschicht-Modul `mask_loader.rs` besitzt nun eine vollständige
Invalidierungs-/Re-Inferenz-Testmatrix (17 Tests): fehlende Artefakte,
Modellwechsel, Quelländerung, Decode-Kontext-Änderung und `Corrupt`-Status
lösen Re-Inferenz aus (bzw. Cache-Fallback mit Warnung, wenn kein Modell
verfügbar); ein verfügbares, aber fehlschlagendes Modell führt zu einem harten
Fehler ohne stillen Cache-Fallback; `refresh` erzwingt Re-Inferenz. Falsche
Prüfsummen werden auf der zdata-Ebene (BLAKE3) abgewiesen
(`ZDataError::Checksum`), sodass das Artefakt nicht in `loaded_planes` gelangt
und vom Entscheidungslayer wie ein fehlendes Artefakt behandelt wird.

**Status (Review-Nachziehen 2026-08-25):** Umgesetzt und unabhängig
verifiziert. `lumina-onnx` verhält sich jetzt auch auf Backend-Ebene ohne
stille Fallbacks: `StubBackend::infer` gated auf `available` (→
`MissingModel`), der Modell-Hash wird beim Laden gegen das Manifest
verifiziert (`ModelHashStatus::{Verified,Pending,Mismatch}`; Mismatch →
harter Fehler `OnnxError::ModelArtifactStale` statt stiller Matte),
ORT-Preprocessing nutzt Manifest-mean/std (ImageNet-Default, korrekte
CHW-Planes) mit Tensor-/Output-Namen aus dem Manifest und validierter
Output-Shape, und `ModelManifest::validate` erzwingt non-empty
hash/license sowie gültige Auflösungen/Tensor-Namen. SAM-Prompt-Typsystem
deckt Labels −1/0/1/2/3 inklusive Source↔1024²-Mapping ab (76 Tests).

**Status (Review-Follow-ups F-082-FOLLOWUP-ORT/-HASH, 2026-08-26):**
Umgesetzt, Testausführung lokal bestätigt (unabhängige Verifizierung steht
aus). Beide Befunde sind behoben:

- Kein Panic mehr bei unbekannten Tensor-Namen: `OrtBackend::new` validiert
  die manifest-deklarierten Input-/Output-Tensor-Namen beim Laden gegen den
  geladenen Graphen und liefert bei Abweichung einen beschreibenden
  `OnnxError::InferenceFailed` (angefordertes + verfügbares Name-Set); der
  Laufzeit-Zugriff auf den Output nutzt `SessionOutputs::get` defensiv statt
  des panizierenden `Index`-Impls.
- Der ORT-Mismatch-Refuse-Zweig ist ausführbar getestet: Die Gate-Logik
  (`ModelHashStatus::enforce_inference_allowed` → `ModelArtifactStale`) ist
  feature-frei unit-getestet; End-to-End läuft der Zweig gegen ein
  **in-Test deterministisch generiertes, minimales ONNX-Modell**
  (`ReduceMax(axes=[1], keepdims=1)`, Prototyp-Bytes handkodiert,
  `crates/lumina-onnx/tests/ort_backend.rs`, Feature `onnx-rt`) mit von der
  Pin-Abweichung echtem SHA-256 — ohne committetes Binär-Fixture und ohne
  Downloads.

Bekannte Grenze bis F-082: Für die **numerische** Validierung (Tensor-Namen,
Wertebereiche, Matte-Qualität) echter Modelle werden weiterhin lokale,
hash-gepinnte BiRefNet/SAM-2-`.onnx`-Fixtures benötigt (keine spontanen
Downloads); das handgenerierte Testmodell dient ausschließlich der
Verhaltensabsicherung der Backend-Pfade.

**Status (F-082-FOLLOWUP, 2026-09-02):** ORT-Pfad + hash-gepinnte Fixtures + MaskGraph **BESTANDEN** (unabhängig verifiziert 2026-09-02, 107p unter `onnx-rt`, Commit 49f4f76; wasm-Anteile historisch — WASM gestrichen 2026-09-04).

- **Echter ORT-Pfad resolvable ohne stillen Fallback:** `lumina-onnx` bietet
  nun die Konsumenten-Fläche `try_load_onnx_engine` (in
  `lumina_onnx::resolve`, Enum `OnnxEngine`): bei aktiviertem `onnx-rt` und
  vorhandenem, hash-verifiziertem Artefakt → `OnnxRuntime(Box<dyn MaskInference>)`
  (exakt der Vertrag der `lumina-core`-Entscheidungsschicht F-048/F-051); bei
  fehlendem/ stale/fehlbenanntem Artefakt → harter `OnnxError` (`MissingModel` /
  `ModelArtifactStale` / `InferenceFailed`), **nie** ein Fallback auf den
  Stub. Ohne `onnx-rt` → explizit `RuntimeDisabled` (Capability-Statement,
  kein stiller Stub). `OrtBackend` implementiert `MaskInference` bereits, die
  CLI kann den Pfad damit ohne additives Glue übernehmen.
- **Hash-gepinntes ONNX-Fixture:** `crates/lumina-onnx/tests/fixtures/
  lumina-crafted-reducemax.onnx` ist als committetes Behavior-Fixture
  hinterlegt (139 B, SHA-256-Pin `2a2ede66…`, Provenienz in
  `tests/fixtures/README.md`, Regenerierung via
  `scripts/regenerate_onnx_fixture.sh`). Tests laden das Fixture per
  `include_bytes!`, prüfen den Pin, laden es via `OrtBackend` mit
  `model_hash` = Pin (`Verified`) und inferieren; ein Drift zwischen
  Encoder-Quelle und Fixture ist ein harter Testfehler.
- **CLI-Einbindung in den ORT-Pfad (F-082-FOLLOWUP-Rest):** `lumina-cli`
  verdrahtet die F-048/F-051-Entscheidungsschicht nun über
  `resolve_mask_inference_engine` (Feature `onnx-rt`, neu in `lumina-cli`,
  forwarded zu `lumina-onnx/onnx-rt`). Ohne `onnx-rt` bleibt der
  deterministische `StubBackend` der Default-Draht (unverändert). Mit
  `onnx-rt` wird die echte Engine **nur dann angefordert, wenn der Lauf
  Re-Inferenz brauchen kann** (aktive Kopie trägt `mask_layers` — exakt die
  Erreichbarkeit der Entscheidungsschicht; ein `--update-masks`-Refresh auf
  einer maskenlosen Kopie fordert nichts an). Das Artefakt kommt aus der
  Umgebungsvariable `LUMINA_MODEL_PATH`; ein unsetter Pfad, ein fehlendes,
  stale oder fehlbenanntes Artefakt ist ein **harter CLI-Fehler**
  (`MissingModel`/`ModelArtifactStale`/`InferenceFailed` durchgereicht), nie
  ein stiller Stub-Ersatz. Tests: CLI-Suite grün mit und ohne `onnx-rt`
  (47 Unit- + 14 E2E-Tests); unter `onnx-rt` erzeugen die Tests ein
  deterministisches, BiRefNet-kompatibles Crafted-ONNX-Modell (`input`/
  `output`, 1024×1024, ReduceMax) zur Laufzeit und belegen damit „echte
  Engine geladen und inferiert", „fehlendes Artefakt → harter Fehler",
  „Müll-Artefakt → harter Fehler" und „ohne Maskenarbeit kein Engine-Request".
  Der Resolver-MissingModel-Fall bleibt unverändert ein Fehler
  (`resolver_reports_missing_artifact_without_fallback`).
- **Offen (unverändert):** die committeten, hash-gepinnten **echten**
  Modellgewichte (BiRefNet/SAM-2, weiterhin `pending-integration`) sowie die
  **GUI**-Einbindung in den ORT-Pfad. Die `MaskGraph`-Auswertung nutzt die
  modellbasierte Segmentierung erst, wenn ein Modell verfügbar ist (kein
  stiller Fallback) — als nächster Schritt nach der CLI-Fläche (F-082-Phase-2,
  GUI-Capability-Anzeige).

## Abnahme

- Eine gültige Matte wird nach Neustart ohne Modell-Download verwendet.
- Ein verändertes Original markiert abhängige Masken als veraltet.
- Ein fehlendes oder beschädigtes Artefakt wird sichtbar gemeldet.
- Modellwechsel führen nicht zu stiller Neuberechnung.
- Eine Box- oder Pinsel-Prompt kann eine eigene Objektmaske erzeugen und
  zusammen mit ihrer Promptdefinition wieder geöffnet werden.
- Ein Modell ohne `mask_prompt` darf eine Pinselmaske nicht stillschweigend als
  gleichwertige Eingabe behandeln; die GUI zeigt die nicht unterstützte
  Fähigkeit an.
- Masken-Roundtrip, Prüfsumme, fehlendes Modell und Quelländerung sind getestet.

## Implementierungsstatus (LRPAR-G03-MASK, G-03 Maskierungs-Parität)

Umgesetzt (Sidecar + Core + CLI + GUI, Tests siehe unten): `AiSelectKind`
(`subject`/`sky`/`background`/`objects`/`people`, case-insensitiv lesbar) mit
optionalem `detail` auf `source`-Knoten (`None` = Legacy, keine Migration);
`MaskPrompt::ColorRange`/`LuminanceRange` als deterministische Rezept-Stufen
(`lumina-core::range_masks`: Rec.709-Trapez bzw. HSL-Kreisdistanz×Sättigung×
Luminanz, ohne Modell/Cache, `ComputedDeterministic`); Kombinatorik
(`union`/`subtract`/`invert` im Panel, zusätzlich `intersect` in der CLI;
`duplicate` nur für Source-Knoten; Zyklen/Stelligkeit laut abgelehnt mit
Rollback); `MaskLayer.visible` (Auge, persistiert pro virtueller Kopie,
unsichtbare Layer werden im Render übersprungen); Show + Overlay-Farbe als
Session-Display-State mit `info!`-Log. CLI: `mask --list/--add-ai-select/
--add-luminance-range/--add-color-range/--combine/--duplicate/--attach-layer/
--show-layer/--hide-layer` (stabile `mask-<blake3>`-IDs, `validate()`-Gate vor
jedem Write, Exit 1 bei Fehlern, keine absoluten Pfade). GUI: Maskenliste mit
Auge + Statuszeile, AI-/Range-/Combine-Zeilen, `draw_masking_g03` headless
getestet (alle Labels malen, 320px-Budget gehalten). Bekannte Grenzen: echte
Modellgewichte weiter `pending-integration` (AI-Masken brauchen Inferenz,
sonst laut `pending`/`missing`); der Panel-Combine deckt gezielt
Add/Subtract/Invert ab (`intersect` CLI-only, dokumentierte Entscheidung).
