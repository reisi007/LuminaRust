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

## Implementierungsstatus (F-047 / F-080)

**Stand 2026-08-19 (F-047 Adapter-Crate `lumina-onnx` implementiert):**

- Der austauschbare ONNX-Adapter existiert als native-only-Crate `lumina-onnx`
  (spiegelt `lumina-raw`, nie im WASM-Build). Er entlastet `lumina-core` und
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

**Status (F-082-FOLLOWUP, 2026-09-02):** ORT-Pfad + hash-gepinnte Fixtures + MaskGraph **BESTANDEN** (unabhängig verifiziert 2026-09-02, 107p unter `onnx-rt`, wasm32 `onnx-rt` grün, Commit 49f4f76, wasm-gating e60a9ad).

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
