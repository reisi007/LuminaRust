# Fixtures, Modelle & Lizenz-/Distributionsprüfung

**Feature-IDs:** F-073 (kleine versionierte Referenzbilder, RAW-Fixtures und
Modelle inkl. Lizenzinformationen) · F-078 (Lizenz-, Modell- und
Distributionsprüfung vor dem ersten Release)
**Status:** SOLL dokumentiert; Umsetzung als Dokumentation + Audit abgeschlossen,
noch nicht verifiziert.
**Autorität:** `Agents.todo.md` (Phase 11, F-073/F-078); begleitende Audit-Docs:
[`docs/fixtures-and-licensing.md`](../../docs/fixtures-and-licensing.md) und
[`THIRD-PARTY-NOTICES.md`](../../THIRD-PARTY-NOTICES.md).
**Verwandt:** `feature/README.md`, `docs/adr/0002-raw-backend.md`,
`feature/quality/performance-benchmarks.md` (Fixtures-Regeln), `README.md`
(LibRaw-Hinweis).

---

## 1. Ziel

LuminaRust muss vor dem ersten Release

1. kleine, versionierte, **reproduzierbar erzeugte** Test-Fixtures und
   referenzierbare Rohdaten bereitstellen (F-073);
2. **alle** Abhängigkeiten, Modelle und nativen Bibliotheken auf
   Lizenzkompatibilität prüfen und dokumentieren (F-078).

Reproduzierbarkeit und Lizenzklarheit sind Release-Gates — kein stillschweigender
Fallback bei unklaren Artefakt- oder Lizenzlagen.

---

## 2. Geltungsbereich

Dieses Dokument erfasst:

- synthetische Benchmark-Fixtures (`crates/lumina-bench/bench/common/mod.rs`);
- committete RAW-Fixtures (`sample-data/raw/*.cr3`);
- ML-Modelle (BiRefNet, SAM 2, ONNX Runtime) inkl. Lizenz;
- die vollständige Rust-Abhängigkeitsmenge (Cargo-Metadata, default + all-features);
- native C-Bibliothek LibRaw (über `vendor/libraw-sys`);
- native C-Bibliothek Lensfun (über `crates/lumina-lensfun`, F-098-N1,
  feature-gated `native`).

Nicht Gegenstand: Golden-Image-Tests (deferred F-043/F-073), zentrale DB
(es gibt in v1 keine).

---

## 3. Fixture-Inventar (SOLL/Ist)

### 3.1 Synthetische Benchmark-Fixtures

Vollständig lokal und deterministisch erzeugt (`bench/common/mod.rs`), kein
Netzwerk:

- `FIXTURE_SEED: u64 = 0x5EED` (eingefroren);
- PRNG: `SplitMix64` (dependency-free, im Modul), per Größe abgeleitet;
- `SIZES = [512, 1024, 2048]`;
- Hilfsfunktionen: `make_frame`, `make_recipe`, `make_mask_fixture`,
  `make_cache_fixture`.

Vertrag: Seed-Änderung macht `perf/baseline.json` ungültig → vor
Median/p95-Vergleich neu aufzeichnen.

### 3.2 Committete RAW-Fixtures

| Datei | Maße | Orientierung | Verwendung |
| --- | --- | --- | --- |
| `sample-data/raw/aircraft-landscape.cr3` | 6032×4024 | 1 | `lumina-raw`-Test `aircraft_landscape_fixture_*`; Decode-Bench; **GUI-Drop-Nachweis** `dropped_raw_path_preserves_orientation_metadata_and_identity` + sein Wächter-Metatest; **Erwartungsquelle des Wächters** (Inventar, §3.2.1) |
| `sample-data/raw/aircraft-portrait.cr3` | 4024×6032 | 8 | `lumina-raw`-Test `aircraft_portrait_fixture_*`; Decode-Bench (Korrektur 2026-09-26: Orientation war hier mit **5** verzeichnet; gemessen und in `lumina-raw/src/lib.rs:1238` assertionsgesetzt ist **8** — gleicher Wert wie `kittest_fixtures_support/mod.rs:44`); **GUI-Drop-Nachweis** + Wächter-Metatest; **Erwartungsquelle des Wächters** (Inventar, §3.2.1) |

Decode-Benchmarks lesen das Verzeichnis über die Env-Variablen
**`LUMINA_RAW_FIXTURE`**; ohne sie wird sauber übersprungen (kein Panic, kein
Fallback).

#### 3.2.1 Verbindliche Regeln für den GUI-Drop-Nachweis (Build-Agent 2026-09-28)

**Status, verbindlich (Build-Agent, 2026-09-29, nach sechs Verifikationsrunden).** Dieser Abschnitt trennt, was den **Nachweis** schützt, von dem, was nur den **Test selbst** absicherte:

| Regel | Status | schützt |
| --- | --- | --- |
| 1 | **Zusage** | den Nachweis: kein Env-Gate, `#[ignore]` nur wegen Kosten |
| 2 | **Zusage** | den Nachweis: Inventar als unabhängige Erwartungsquelle |
| 3 | **Zusage** | den Nachweis: fehlender committeter Fixture → Fail, nicht Skip |
| 5 | **Zusage** | den Nachweis: Override ist additiv, nie substituierend |
| 4 | **zurückgenommen als Zusage** | nur den Test selbst — ersetzt durch **benannte Grenze** unten |
| 6 | **zurückgenommen als Zusage** | nur den Test selbst — ersetzt durch **benannte Grenze** unten |

**Warum Regel 4 und 6 zurückgenommen werden.** Beide schützten die Aussage „der Nachweis decodiert **jede** committete Fixture" — eine **Zusicherung über den Test selbst**, nicht über sein Verhalten. Sechs Runden, sechs Orte derselben Fehlerklasse: ein Panic, eine selbstbezügliche Assertion, ein `return` oberhalb des Wächters, ein Typ, eine Transformationsstelle nach dem Wächter, und zuletzt eine **Zählstelle, die von der Arbeitsstelle unabhängig ist** (`raw_fixture_consumption.rs:45-49` — der Name konnte **vor** dem Dekodieren eingetragen und die Arbeit per `continue` übersprungen werden: Exit 0, eine von zwei Fixtures dekodiert). **Jeder Fix verlagerte den einzigen Vertrauenspunkt, und die nächste Mutation zielte dorthin.** Das ist die in `DoD.md` §10 beschriebene Form, und die Konsequenz ist die dortige: eine Klausel-Invariant ohne tragfähige Form wird **als benannte Grenze ausgewiesen**, nicht endlos verteidigt.

**Die benannte Grenze, die dafür tritt:** die Abdeckung der committeten Fixtures ist **eine im Code lesbare Eigenschaft, keine durchgesetzte Invariante.** Wer eine Zeile aus der Fixture-Tabelle entfernt, ändert den Test — und der Test beweist danach für die verbleibenden Fixtures weiterhin genau das, wofür er da ist: **dass ein verworfenes CR3 Orientierung, Metadaten und Identität behält.** Das ist die Aussage, für die der Test existiert, und sie hat **nie** von Regel 4 oder 6 abgehangen.

**Was das kostet, ausdrücklich benannt:** mit der Rücknahme entfallen **zehn** in sechs Runden gemessene, mutationsbewiesene Widerstandsfälle (leerer Satz, gelöschte Inventarzeile, fehlende Datei, neutralisierter Wächter, `CI`-Early-Return, Filter/continue/retain **an vier Stellen**, koordinierte Zwei-Quellen-Verkleinerung). Das ist ein **realer Verlust an Absicherung** und wird nicht beschönigt. Er wird bezahlt mit einem Helfer, den ein Leser in einem Durchgang versteht, und mit dem Ende der Rundenschleife. Ein Wächter, der sechs Runden und eine Zusage gebraucht, um eine Eigenschaft über den **Test selbst** zu verteidigen, ist mehr Maschinerie, als diese Eigenschaft wert ist.

**Regel 1 — der Nachweis ist nicht optional und nicht env-gegatet.** Der Test
`dropped_raw_path_preserves_orientation_metadata_and_identity` in
`crates/lumina-gui/src/dropped_files.rs` liest seine Fixture **deterministisch
aus diesem committeten Inventar** (Workspace-Root, `sample-data/raw/`). Er trägt
`#[ignore]` allein wegen der realen Decode-Kosten (zwei 12-MB-CRDekodierungen),
nicht wegen einer fehlenden Umgebung. Ein Betreiber, der `LUMINA_RAW_FIXTURE`
setzt, erhält einen **ausdrücklich schwächeren** Nachweis, und das wird an
echtem `stderr` **sichtbar** gemeldet (`WEAKENED …`), niemals still.

**Regel 2 — das Inventar ist die Erwartungsquelle, und zwar diese Tabelle plus
der Verzeichnisinhalt.** Der Wächter vergleicht zur Laufzeit den aus dem Baum
gelieferten Satz gegen die Zeilen dieser Tabelle und gegen `read_dir` über
`sample-data/raw`. **Konsequenz für dieses Dokument:** eine Änderung an §3.2,
an der Tabelle oder an `sample-data/raw/README.md`, die nicht mit dem
Verzeichnisinhalt übereinstimmt, macht den Test **rot**, nicht grün. Das ist
beabsichtigt — das Inventar ist die unabhängige Erwartung, und eine
stillschweigende Verengung ist die Fehlerform, die der Wächter verhindert.

**Regel 3 — ein fehlender committeter Fixture ist ein defekter Baum, kein
Betreiberwunsch.** `documented`-Quellen unterscheiden die beiden Fälle:
aus dem Inventar stammend und **fehlend** → **Fail** (Exit ≠ 0) mit konkretem
Pfad; aus `LUMINA_RAW_FIXTURE` stammend und **fehlend** → `SKIPPED` + Exit 0.
„Keine stillen Fallbacks bei fehlenden Artefakten" (`Agents.md`) gilt hier
wörtlich: die Fußnote ändert die Klasse nicht, denn ein Gate, das auf den
Exit-Code schließt, sieht grün.

**Regel 4 — der committete Satz ist bedingungslos; es gibt nichts zu
freistellen.** (Normativ, aus der Verifikations-Runde 3 zu `KITT-IGNORED-PANIC-56`,
in Runde 4 **präzisiert**.) Der Umfangsnachweis wird an der Aufrufstelle aus
der **Umgebung** entschieden, nicht aus einem Feld, das der Produzent selbst
setzen kann: `raw_fixtures()` liefert **ausschließlich** committete Fixtures,
und `CommittedFixture.documented` ist **kein** `Option`, so dass die
Beschriftung eines committeten Fixtures als „optional" **innerhalb des
Typs** nicht darstellbar ist.

**Berichtigung 2026-09-28 (Runde 4):** eine frühere Fassung dieser Regel
schloss mit „der Zustand ist nicht mehr ausdrückbar". **Das war zu absolut
und ist widerlegt.** Die Freistellung ist weiterhin erreichbar — über die
Stelle, an der der Optional-Operand entsteht (`operator_operand()`), mit
**einer** Zeile, und ohne dass ein Lauf rot wird. Eine Regel, die einen Weg
abschließt statt einer Klasse, ist eine Absichtserklärung. **Regel 5** ist
die Antwort darauf.

**Regel 5 — der Override ist additiv, niemals substituierend.** (Normativ,
2026-09-28.) Der Nachweis dekodiert **immer** den vollständigen committeten
Satz. `LUMINA_RAW_FIXTURE` **ergänzt** eine Datei, es **ersetzt** nichts.

- Eine Zusatzdatei wird mit ausdrücklich **schwächerer** Aussage geprüft: nur
  gegen `lumina_raw::read_metadata`, ohne dokumentierten Geometrie-Anker, und
  das wird an echtem `stderr` sichtbar gemeldet.
- **Kein** Zusatz-Fixture betritt den Umfangsvergleich und **kein** Zusatz
  kann eine committete Fixture ersetzen, verdrängen oder aus dem committeten
  Satz herausnehmen.
- Eine Zusatzdatei, deren Dateiname einer committeten Fixture entspricht, ist
  damit **kein** Fall, der eine Ausnahme braucht — sie wird schlicht als
  überflüssig behandelt und gemeldet, weil die committete Datei ohnehin
  dekodiert wird.

**Warum additiv und nicht „nur committete plus Operator“:** die vorherige Form
hatte genau **einen** Entscheidungspunkt, an dem der Umfang abgeschaltet
werden konnte — und die Abschaltung war der **Normalfall** des vorgesehenen
Features. Vier Verifikationsrunden haben dieselbe Fehlerklasse an vier
verschiedenen Stellen gefunden, weil jeder Fix den einzigen Vertrauenspunkt
verlagerte und die nächste Mutation dorthin zielte. Bei additivem Override
**entfällt der Entscheidungspunkt**: es gibt nichts abzuschalten, weil der
committete Satz bedingungslos ist. Damit ist die Klasse nicht verlagert,
sondern beseitigt.

**Kosten, ausdrücklich benannt** (alle Werte gemessen, `--ignored`; N=3 für den
Soll-Lauf): der Soll-Lauf dekodiert **zwei** committete CR3, 3,82 / 3,96 /
4,13 s. Eine **echte** Zusatzdatei (anderer Dateiname) dekodiert **drei** CR3,
5,12 s. Ein **gleichnamiger** Zusatz wird als überflüssig **gemeldet und nicht
dekodiert**, also **zwei** CR3, ~3,4 s. *Berichtigung 2026-09-29:* eine frühere
Fassung nannte nur „drei CR3, rund 4,9 s" und verschwieg, dass der gleichnamige
Fall **zwei** Dekodierungen kostet. Das ist der Preis der Regel, und er ist der
richtige: er kauft die Eigenschaft, dass **kein** Ausführungspfad den
Umfangsnachweis abschalten kann.

**Regel 6 — geprüft wird das ERGEBNIS, nicht der Pfad.** (Normativ,
2026-09-29, aus Verifikations-Runde 5.) Nach der Dekodierschleife gilt: die
Anzahl **tatsächlich dekodierter** committeter Fixtures ist **gleich** der
Anzahl der Inventarzeilen in `sample-data/raw/README.md`. Die Erwartungsquelle
ist das **Inventar**, nicht die Tabelle, aus der iteriert wird; die Anzahl wird
**aktiv gezählt**, nicht über eine Konstante, die jemand mit der Tabelle
synchron hält.

Diese Regel ist der **Ansatzwechsel**, nicht eine weitere Absicherung. Die
Runden 1–5 haben **Wege** geschlossen: ein Panic, dann eine selbstbezügliche
Assertion, dann ein `return` oberhalb des Wächters, dann ein Typ, dann eine
Transformationsstelle **nach** dem Wächter. Jeder Fix verlagerte den einzigen
Vertrauenspunkt, und die nächste Mutation zielte dorthin — fünf Runden, fünf
Orte, dieselbe Fehlerklasse. Regel 6 fragt nicht mehr *wo* der Pfad
unterbrochen werden könnte, sondern **was am Ende herauskam**: die eine Aussage
über die dekodierte Anzahl fängt eine Verkleinerung, **wo auch immer** sie
entsteht — im Produzenten, zwischen Prüfung und Schleife, in der Schleife, in
einem `filter`, in einem `skip`, in einem `continue`.

**Zwei Form-Forderungen, die dazugehören:**

1. **Der committete Satz wird nicht mit dem Betreiber-Zusatz gemischt.** Es gibt
   keine gemeinsame `Vec`, in der beide liegen; zwischen Prüfung und Verbrauch
   steht damit **keine Zeile**, die den geprüften Wert in einen anderen
   überführt. Der geprüfte Wert **ist** der verbrauchte Wert.
2. **Herkunftsangaben kommen aus der Konstruktion, nicht aus einem
   Musterabgleich.** Eine Meldung darf nicht aus einem `matches!`-Muster
   rekonstruieren, woher ein Zustand kam. Wer eine Meldung erzeugt, muss den
   Zustand **belegbar** bekommen. Begründung: in Runde 4 meldete eine sichtbare
   `WEAKENED`-Zeile eine `LUMINA_RAW_FIXTURE`-Überschreibung, die **nie
   stattgefunden hatte**, bei Exit 0 und grün. Eine Diagnose, die ihre eigene
   Herkunft erfinden kann, ist eine Fehlerquelle — Sichtbarkeit ist kein Beweis
   von Richtigkeit.

**Selbstauskunft, die zu diesem Abschnitt gehört (2026-09-29):** dieser Abschnitt
ist nach **fünf** Verifikationsrunden entstanden, und die ehrliche Bilanz ist,
dass die **Umfangs-Aussage** („der Nachweis decodiert jede committete Fixture")
einen Aufwand erzeugt hat, der ihre Beweislast für den *eigentlichen* Test
übersteigt. Der eigentliche Test trägt eine andere, einfachere Aussage: **ein
verworfenes CR3 erhält Orientierung, Metadaten und Identität.** Das ist die
Aussage, für die der Test existiert. Die Umfangsregeln 4–6 sind eine Zusicherung
über den **Test selbst** und stehen hier, weil sie einmal beansprucht wurden —
nicht, weil sie den Kern des Nachweises tragen. Wer diesen Abschnitt künftig
kürzt, kappt an der Zusicherung und **nicht** am Nachweis.

**Noch offener Rest, ausdrücklich nicht als gedeckt geführt:** der
Wächter-Metatest pinnt die *Logik* des Wächters, nicht seine *Verdrahtung* —
ein Auskommentieren des Aufrufs lässt ihn grün. Geschützt ist die Verdrahtung
allein durch die Mutation `if var("CI").is_ok() { return Vec::new(); }` (muss
Exit ≠ 0 ergeben) und durch Review.

### 3.3 Golden-Referenzbilder

**Rezept-Matrix-Goldens (LRPAR-MATRIX-RECIPE, Slice 1):** 22 committete PNGs
unter `testdata/matrix/golden/<sample-id>__<recipe-id>.png`
(2 Samples × 11 Rezepte, Vergleichsbreite 384 px). Sie werden über den
gemeinsamen CPU-Referenz-Renderpfad aus den beiden RAW-Fixtures aus §3.2
erzeugt und deterministisch bilinear herunterskaliert; die Rezeptliste und die
PSNR-Toleranzen stehen normativ in
[`conflicts-and-acceptance.md`](conflicts-and-acceptance.md) § „Rezept-Matrix".

- **Provenance/Lizenz:** Ableitungen der beiden CR3-Fixtures aus §3.2; deren
  Nutzungs-/Distributionsgewährung (Eigentümer, 2026-08-20, dokumentiert in
  [`sample-data/raw/README.md`](../../sample-data/raw/README.md), R1 gelöst)
  deckt Test-, Benchmark- und Referenzzwecke im Rahmen von LuminaRust mit ab.
  Kein Drittinhalt, keine Modellgewichte, kein Netzwerk-Download.
- **Versionierung/Determinismus:** Goldens werden ausschließlich über
  `lumina matrix --update-goldens` neu geschrieben (an die CPU-Referenz
  gepinnt); eine Golden-Änderung ist eine bewusste Rebaseline und gehört in
  denselben Commit wie die verursachende Änderung. Verifikation läuft über
  `lumina matrix` mit den dokumentierten Toleranzen.

### 3.4 Hash-gepinntes ONNX-Behavior-Fixture (`lumina-onnx`)

| Datei | Bytes | SHA-256-Pin | Verwendung |
| --- | ---: | --- | --- |
| `crates/lumina-onnx/tests/fixtures/lumina-crafted-reducemax.onnx` | 139 | `2a2ede6659e8c59b3fd972242b27677ef23cb98d3c422616a1c65f50dcaca18d` | `OrtBackend`-Verhalten: Tensor-Namen, Hash-Pin `Verified`, Output-Validierung, Inferenz; Resolver-Test; **Face-ORT-Gates** (`tests/face_ort.rs`): `OrtFaceDetector`/`OrtFaceEmbedder` (MissingModel, Stale-Gate, Tensor-Namen, kontraktwidrige Output-Form) und `try_load_face_engine` (Resolver, kein Stub-Fallback) |
| `crates/lumina-onnx/tests/fixtures/lumina-crafted-yunet.onnx` | 2 490 | `b4c76993b06bcccf1a0495fa795b2de8be8263c448b83f62630d4227da8324b1` | FACE-20-FACE-ADAPTER-25: YuNet-Per-Stride-Adapter (`tests/face_adapter_ort.rs`) — zwölf `cls_/obj_/bbox_/kps_{8,16,32}`-Outputs, Decode + NMS über den echten ORT-Pfad, fehlender Tensor/Tensor-Kontrakt laut |
| `crates/lumina-onnx/tests/fixtures/lumina-crafted-sface.onnx` | 659 | `d2919decc20b9fe62e0d99e544fd0243fda0f847f3b99f32500491f6d6a5738e` | FACE-20-FACE-ADAPTER-25: SFace-Vertrag (`data` → `fc1`) mit 5-Punkt-Alignment + `BYTE_RANGE`-Vorverarbeitung + L2-Decode |

- **Provenance:** programmatisch erzeugt aus der dokumentierten
  Proto3-Encoder-Quelle (`crates/lumina-onnx/tests/ort_backend.rs` +
  `scripts/regenerate_onnx_fixture.rs`), kein Download, keine Modellgewichte,
  daher **keine Lizenzpflicht** (trivialer, formelhaft generierter Graph).
- Kein echtes Segmentierungs- oder Gesichtsmodell als Fixture: echte
  BiRefNet/SAM-2-Gewichte bleiben `pending-integration`. Die Face-Manifeste
  (YuNet/SFace) tragen seit FACE-20-S6 (2026-09-20) verifizierte
  `sha256:<hex>`-Pins, aber die Gewichte werden weiterhin **nicht** committet
  und **nicht** zur Testzeit geladen (keine spontanen Downloads, Agents.md).
- Regenerierung: `scripts/regenerate_onnx_fixture.sh`; ein Drift zwischen
  Encoder und Fixture bzw. vom Pin ist ein harter Testfehler.

---

## 4. Rohdaten-Provenance & Lizenz — ✅ GELÖST (2026-08-20)

**Befund (Stand 2026-08-20, Ermittlung siehe `sample-data/raw/README.md`):**
Die beiden committeten `.cr3`-Fixtures in `sample-data/raw/` haben **keine
explizite Lizenzgewährung** — weder im einführenden Commit `1e388bf` noch als
separates `LICENSE`/`README`. Der Eigentümer ist jedoch **aus den Binär-Metadaten
selbst ableitbar**: Beide Dateien tragen in EXIF `Artist`/`Copyright`
`reisinger.pictures/Florian Reisinger` sowie `Owner Name: Florian Reisinger`; der
einführende Commit `1e388bf` („Add tone controls and RAW sample fixtures“,
2026-08-17, Author Florian Reisinger) bestätigt denselben Urheber. Damit ist der
**Autor/Provenance teilweise belegt** — es fehlt weiterhin eine **explizite
Lizenz** (das EXIF-`Copyright`-Feld ist keine Lizenzgewährung; es existiert kein
IPTC/XMP-`License`/`Rights`-Feld).

Ermittelte Metadaten (via `exiftool`):

| Datei | Kamera | Objektiv | Aufnahme (EXIF) | Orientierung | Maße |
| --- | --- | --- | --- | --- | --- |
| `aircraft-landscape.cr3` | Canon EOS R1 | RF200-800mm F6.3-9 IS USM | 2026:08:14 20:16:49, 1/1000 s, ISO 1000, 800 mm | 1 (Horizontal) | 6032×4024 |
| `aircraft-portrait.cr3` | Canon EOS R1 | RF200-800mm F6.3-9 IS USM | 2026:08:14 20:17:32, 1/1000 s, ISO 1250, 800 mm | 5 (Rotate 270 CW) | 4024×6032 |

Das ist ein **Release-Blocker** (F-078): urheberrechtlich geschützte Kamera-RAWs
ohne Lizenzgewährung zu distribuieren ist ein rechtliches Risiko — unabhängig
von der (MIT) Rust-Code-Lizenz. Die `*_fixture_*`-Tests hängen hart an genau
diesen Bytes.

**Status (2026-08-20):** **GELÖST** — Autor/Provenance belegt (EXIF `Artist`/
`Copyright`/`Owner Name` = reisinger.pictures/Florian Reisinger + Commit
`1e388bf`) und **explizite Lizenzgewährung dokumentiert**: Der Projekteigentümer
hat am 2026-08-20 eine **uneingeschränkte Nutzungs- und Distributionsgewährung
für das LuminaRust-Projekt** erteilt (eingetragen im Provenienz-Block in
`sample-data/raw/README.md`). R1 gilt damit als geschlossen (Verifikation
durch unabhängigen Agenten steht im Rahmen der F-078-Abnahme aus).

Ausgetragene Alternativen (nur noch relevant, falls die Gewährung
zurückgezogen wird):
1. Austausch gegen generierte/CC0-lizenzierte Fixtures (siehe unten, „Was ein
   Austausch bedeuten würde").

**Was ein Austausch bedeuten würde:** Betroffen sind die `lumina-raw`-Tests
`aircraft_landscape_fixture_*` / `aircraft_portrait_fixture_*` (sie referenzieren
die Dateien per `include_bytes!` fest verdrahtet) sowie die Decode-Benchmarks in
`crates/lumina-bench/bench/decode.rs` (lesen das Verzeichnis via
`LUMINA_RAW_FIXTURE`). Die Decode-Pipeline benötigt funktional nur RAW-Bytes;
LuminaRust kann mit **einem** RAW-Fixture arbeiten. Das generierte Fixture müsste
jedoch **beide Rollen** erfüllen (landscape-Orientierung 1 **und**
portrait-Orientierung 5), d.h. entweder zwei generierte Dateien oder eine
Test-Refaktorierung. Die `#[ignore]`-Test
`optional_real_fixture_checks_decode_orientation_and_dimensions` bleibt
unabhängig (eigene, separat lizenzierte RAW via Env-Var). Die Fixture-DATEIEN
selbst wurden in diesem Schritt **nicht** verändert/entfernt — die Entscheidung
liegt beim Build-Agenten/Eigentümer.

---

## 5. Modell-Inventar & Lizenzen

| Modell | Rolle | Lizenz | Status |
| --- | --- | --- | --- |
| **BiRefNet** (Zheng et al., arXiv:2401.03407) | erstes automatisches Subjekt-Modell | **MIT** (GitHub `LICENSE` = MIT, Copyright (c) 2024 ZhengPeng; HF-Card `ZhengPeng7/BiRefNet` `license: mit` — verifiziert 2026-08-20, R6; Manifest korrigiert) | Gewichte *pending integration* (`model_hash = "pending-integration"`) |
| **SAM 2.1** (`sam2.1_hiera_*`) | erstes interaktives Box/Pinsel-Modell (F-082) | **Apache-2.0** für Code **und** Gewichte (facebookresearch/sam2 `LICENSE`, HF-Model-Cards, Meta-Announcement „code and weights … permissive Apache 2.0" — verifiziert 2026-08-20, R6) | Adapter integriert (Commit `452d8a4`); Gewichte *pending integration* (`model_hash = "pending-integration"`) |
| **YuNet** (`face_detection_yunet_2023mar.onnx`, OpenCV Zoo) | Gesichts-Detektion (LRPAR-G12-FACE-20 / FACE-20-S6) | **MIT** © 2020 Shiqi Yu — Modellverzeichnis `LICENSE` + README „all files in this directory", verifiziert **2026-09-20** an der Quelle (`opencv/opencv_zoo` `main` @ `47534e27…`, Blob `4cdf89a4…`) | **Gewichts-Grant verifiziert**; Pin `sha256:8f2383e4…552fa4` (232 589 Byte), Manifest `model_hash`, keine Gewichte committet |
| **SFace** (`face_recognition_sface_2021dec.onnx`, MobileFaceNet, OpenCV Zoo) | Gesichts-Embedding (LRPAR-G12-FACE-20 / FACE-20-S6) | **Apache-2.0** © 2021 Shenzhen Institute of AI and Robotics for Society — Modellverzeichnis `LICENSE` + README „all files in this directory", verifiziert **2026-09-20** an der Quelle (`opencv/opencv_zoo` Blob `d6456956…`) | **Gewichts-Grant verifiziert**; Pin `sha256:0ba9fbfa…c34e79` (38 696 353 Byte), Manifest `model_hash`, keine Gewichte committet |
| **ONNX Runtime** (`ort` 2.0.0-rc.13) | Inferenz-Runtime | **MIT** (ORT `MIT OR Apache-2.0`, `ort-sys` `MIT OR Apache-2.0`) | **optional**, Feature `onnx-rt`, nicht im Default-Build |

**Face-Modelle — S6 abgeschlossen (FACE-20-S6, 2026-09-20):** YuNet und SFace
sind an der tatsächlichen Gewichtsquelle verifiziert. Maßgeblich ist, dass die
**Modellverzeichnis-`LICENSE`** nicht bloß den Demo-Code, sondern laut eigener
`README` „alle Dateien in diesem Verzeichnis" lizenziert — damit ist sie der
**Gewichts-Grant** für die danebenliegende `.onnx`-Datei (MIT für YuNet,
Apache-2.0 für SFace). Die exakten Bytes wurden einmalig bezogen und per
SHA-256 geprüft (Git-LFS-Objekt-ID = Inhalts-Hash); die Manifeste
`face_detect_manifest`/`face_embed_manifest` tragen jetzt diesen Pin
(`sha256:<hex>`) statt `pending-integration`. **Es werden weiterhin keine
Gewichte committet oder zur Testzeit heruntergeladen**; Tests laufen gegen die
deterministischen Stubs und die lokalen, hash-gepinnten Behavior-Fixtures
(`lumina-crafted-reducemax.onnx`, `lumina-crafted-yunet.onnx`,
`lumina-crafted-sface.onnx`). **Realer Adapter (FACE-20-FACE-ADAPTER-25,
2026-09-20):** Die ausgelieferten Graphen werden jetzt über ihren **echten**
I/O-Vertrag angebunden: YuNet = Eingang `input` (rohe `0..=255`-Bytes, **BGR**,
OpenCV-native Reihenfolge) mit zwölf Per-Stride-Outputs
(`cls_/obj_/bbox_/kps_{8,16,32}`, Decode + greedy-NMS); SFace = Eingang `data`
(rohe `0..=255`-Bytes, **RGB**; die `(x−127.5)·1/128`-Normierung liegt im
Graphen) mit Ausgang `fc1` (128-d). Ein Graph, der die deklarierte
Tensor-Menge/-Form nicht erfüllt, wird weiterhin beim Laden laut abgelehnt
(`InferenceFailed` listet die verfügbaren Tensoren) — kein stiller Fallback,
kein stilles Umbiegen.
Lizenztexte: `licenses/models/YuNet-LICENSE-MIT.txt`,
`licenses/models/SFace-LICENSE-Apache-2.0.txt`.
Gepinnte `input_spec_digest`-Werte (echter I/O-Vertrag, FACE-20-FACE-ADAPTER-25;
in `crates/lumina-onnx/tests/face_pins.rs` verankert): YuNet
`sha256:03ce26b03baf5d45f5905e4a84c4d6e0fe70ae6b2ceaffc398cd2dd5b178a8d9`
(640×640, **BGR**, NCHW, rohe `0..=255`-Normierung), SFace
`sha256:e2e2919ae7b8f18ef5c67598ed44d4dc9e258badc38bde47eef577af0f2b01b6`
(112×112, **RGB**, NCHW, rohe `0..=255`-Normierung). Der frühere
`ImageNet`-Vertrag war die in S6 dokumentierte kanonische Deklaration, nicht das
reale Modell (Befund B2); die Umstellung invalidiert persistierte Analysen
sichtbar (`stale`). Optionaler, netzwerkfreier Nachweis gegen die echten,
nutzer-gelieferten Gewichte:
`cargo test -p lumina-onnx --features onnx-rt real_weights_match_the_adapter_contract
-- --ignored` (mit `LUMINA_FACE_DETECT_MODEL_PATH` /
`LUMINA_FACE_EMBED_MODEL_PATH`; ohne die Variablen ein No-op).

**SAM-2-Export-Pfad (AGPL-Falle):** Die SAM-2.1-Gewichte sind Apache-2.0,
aber der übliche Lade-/Inferenzweg über das PyPI-Paket **`ultralytics`
ist AGPL-3.0** und würde als Abhängigkeit das Gesamtwerk betreffen.
LuminaRust nutzt diesen Weg **nicht**: Der ONNX-Export erfolgt aus den
Meta-Checkpoints (092824) über das Microsoft-ORT-Export-Tooling
(`convert_to_onnx.py`, MIT) bzw. veröffentlichte Community-ONNX-Artefakte
(Redistribution unter Apache-2.0); die Inferenz selbst läuft über
`lumina-onnx`/`ort` (MIT), ohne `ultralytics`. Bei der Gewichts-Pinning-
Folgearbeit (F-082-Nachlauf) ist ausschließlich der Apache-2.0-konforme
Exportweg zulässig.

- Es sind **keine Modellgewichte** committet; die Lizenzpflicht entsteht erst
  beim Bündeln der Gewichte (F-048).
- ONNX Runtime (echt) lädt **Prebuilt-Binaries zur Build-Zeit** (Netz); bei
  Release-Freigabe von `onnx-rt` dessen Redistribution prüfen (R4).

---

## 6. Abhängigkeits-Audit (F-078)

### 6.1 Methode

`cargo metadata` (default → **441** Pakete; `--all-features` inkl. `ort` →
**478** Pakete), Lizenzen aus dem `license`-SPDX-Feld, gegen `Cargo.lock`
abgeglichen. Vollständige Tabelle: `THIRD-PARTY-NOTICES.md`.

### 6.2 Ergebnis

- **Keine** GPL/AGPL/SSPL/MPL/EPL-Abhängigkeit (weder default noch all-features).
- Dominanz: MIT / `MIT OR Apache-2.0` / Apache-2.0 / BSD / ISC / Zlib / 0BSD /
  CC0-1.0 / Unlicense / BSL-1.0 — alles OSI-konform.
- Schwach-copyleft (`LGPL-2.1-or-later`): **nur `r-efi`**, und **nur für
  `uefi`**-Targets (transitiv über `getrandom`-UEFI-Backend); in keinem
  ausgelieferten Build enthalten, über die `OR`-Klausel unter MIT/Apache
  erfüllbar.
- **Einzig reale Pflicht (Default-Build):** LibRaw (siehe §6.3). Zusätzlich
  **feature-gated** (nur bei aktiviertem `native`-Feature): Lensfun (siehe §6.5).

### 6.3 LibRaw (einzige reale Verpflichtung)

`lumina-raw` → `vendor/libraw-sys` (MIT, © David Cuddeback, via
`[patch.crates-io]` gepinnt). Dessen `build.rs` linkt die **System**-Bibliothek
`libraw_r` über `pkg-config` (**dynamisch**, nicht vendored/statisch).

- Upstream LibRaw (0.22.2) ist **dual lizenziert**: LGPL-2.1-or-later **ODER**
  CDDL-1.0. Die früher zusätzlich angebotene permissive *LibRaw Software
  License* wurde upstream mit v0.18 (2017) **entfernt** und existiert für
  aktuelle Versionen nicht mehr — ein „tri-license"-Verweis ist veraltet.
  `docs/adr/0002-raw-backend.md` beschreibt LibRaw daher korrekt als dual.
- **Verpflichtung:** Dynamisches Linken beibehalten (statisches Einbetten würde
  LGPL auf das Gesamtwerk ausweiten); LibRaw-Lizenztext + Quellangebot für die
  verwendete Version mitliefern. CI pinnt **LibRaw 0.22.2** (OCI-Label
  `lumina.libraw_version`).
- Bereits im `README.md` vermerkt („LibRaw steht unter der LGPL-2.1-or-later;
  Distributionen müssen die LibRaw-Lizenz …“).

### 6.4 Kompatibilitätsmatrix (Kurzform)

| Lizenzfamilie | OSI | Risiko | Aktion |
| --- | --- | --- | --- |
| MIT / Apache-2.0 / BSD / ISC / Zlib / 0BSD / CC0 / Unlicense / BSL | ✅ | keine | Notice bündeln |
| Unicode-3.0, OFL-1.1/Ubuntu-Font, Apache-2.0+LLVM-exc | ✅ | keine (Attribution) | Notice/Font-Lizenz bündeln |
| `r-efi` (LGPL-2.1-or-later, UEFI-only) | ✅ unter MIT/Apache | nur bei UEFI-Build | nicht ausgeliefert → keine Aktion |
| **LibRaw** (LGPL/CDDL/LibRaw-SW) | ⚠️ schwach | **einzige Pflicht** (Default) | dynamisch linken + Notice/Quellangebot |
| **Lensfun** (LGPL-3.0, DB CC-BY-SA) | ⚠️ schwach | nur bei `native`-Feature | dynamisch linken (Feature `native`, Default aus) + Notice/Quellangebot + DB-Attribution; s. §6.5 |

### 6.5 Lensfun (F-098-N1 / F-098-N4) — native, feature-gated

`lumina-lensfun` (F-098-N1) ist ein dünner, sicherer Rust-Wrapper um die
System-`liblensfun` für **automatische Objektivkorrektur** (Verzeichnung +
Vignettierung), wenn in der installierten Datenbank ein passendes
Kamera/Objektiv-Profil gefunden wird. Die Integration ist **Pre-MVP** (verifiziert
2026-08-20, F-098-N1), die Distributions-Doku ist Teil von **F-098-N4** (S8) und
zählt zur F-078-Abnahme.

| Punkt | Befund |
| --- | --- |
| Rolle | Automatische Objektivkorrektur (Distortion + Vignetting + TCA/CA via Lensfun seit G-06; manuelles `ca_red`/`ca_blue`-Modell nur noch für Profile ohne TCA-Kalibrierung) |
| Integration | Pre-MVP (F-098-N1), verifiziert 2026-08-20; TCA-Vollausbau + EXIF-Lens-Erkennung + CLI/GUI-Parität mit G-06 (LRPAR-G06-GEO, MVP/1.0) |
| Feature-Gating | `native`-Feature im Crate `lumina-lensfun` — **Standard AUS**; Default- und CI-Builds linken nichts und bleiben grün |
| Linkart | **dynamisch** über `pkg-config` (`build.rs` → `cargo:rustc-link-lib=dylib=lensfun`), nur wenn `native` an |
| Version (bewiesen) | **0.3.4** — `brew info lensfun` **und** `LF_VERSION_*` in `/opt/homebrew/include/lensfun/lensfun.h` (`LF_VERSION_MAJOR 0` / `_MINOR 3` / `_MICRO 4`) |
| Bibliotheks-Lizenz | **LGPL-3.0-or-later** laut Projekt-FFI und Header-Text („version 2 … or (at your option) any later version“); Homebrew-Formel deklariert `LGPL-3.0-only AND GPL-3.0-only AND CC-BY-3.0 AND LicenseRef-Homebrew-public-domain` → **zu verifizieren** (exakte SPDX gegen upstream `COPYING`/`README`) |
| Datenbank-Lizenz | Profil-DB (`/opt/homebrew/share/lensfun/version_1/*.xml`): **CC-BY-SA-3.0** laut Projektdoku/F-098-N4; Homebrew-Formel nennt nur `CC-BY-3.0` → **zu verifizieren** (SA vs. kein-SA gegen `lensfun-data`) |
| Neue gebündelte Binaries | **Keine** — LuminaRust liest zur Laufzeit die **System**-Datenbank (kein vendored DB) |
| Verpflichtung | Dynamisches Linken beibehalten (kein statisches Einbetten → würde LGPL aufs Gesamtwerk ausweiten); Lensfun-Lizenztext + Quellangebot für **0.3.4** + DB-Attribution im Release-Bundle |
| Querverweis | `THIRD-PARTY-NOTICES.md` (Attribution obligations, Nr. 5) |

---

## 7. Versionierungs-Policy

| Artefakt | Pin-Mechanismus | Wert |
| --- | --- | --- |
| Rust-Baum gesamt | committetes `Cargo.lock`, `resolver = "2"` | autoritativ |
| `ort` / `ort-sys` | exakte Version in `lumina-onnx/Cargo.toml` | `=2.0.0-rc.13` |
| `libraw-sys` | `[patch.crates-io]` → `vendor/libraw-sys` | gepatcht `0.1.1` |
| LibRaw (nativ) | CI-Image OCI-Label `lumina.libraw_version` | `0.22.2` |
| Benchmark-Fixtures | eingefrorener `FIXTURE_SEED = 0x5EED` + `SplitMix64` | Änderung ⇒ Re-Baseline |
| ONNX-Behavior-Fixture | SHA-256-Pin in `tests/fixtures/README.md` + `tests/ort_backend.rs` | `2a2ede66…` (committetes Binär-Fixture) |
| Modellgewichte | `ModelManifest.model_hash` als Identität | BiRefNet/SAM 2 `pending-integration`; Face YuNet/SFace **gepinnt** (`sha256:<hex>`, FACE-20-S6 2026-09-20), keine Gewichte committet |

Policy: `Cargo.lock` committet halten; native Abhängigkeiten über das immutable
CI-Image pinnen; Modellgewichte bei Integration über Hash + Version + Lizenz +
Quell-URL erfassen; Fixture-Seeds eingefroren.

---

## 8. Offene Punkte & empfohlene Maßnahmen

| ID | Schwere | Punkt | Maßnahme |
| --- | --- | --- | --- |
| **R1** | ✅ Gelöst (2026-08-20) | `.cr3`-Fixtures (§4): Autor aus EXIF + Commit belegt (Florian Reisinger / reisinger.pictures); **uneingeschränkte Nutzungs-/Distributionsgewährung für LuminaRust** am 2026-08-20 durch den Eigentümer erteilt und im Provenienz-Block (`sample-data/raw/README.md`) dokumentiert | Keine Aktion mehr; Verifikation der Doku im Rahmen der F-078-Abnahme |
| **R2** | 🟠 Hoch | Alle 9 Workspace-Crates ohne `license`-Feld; Repo-Root ohne `LICENSE`/`NOTICE` — Projekt bewusst unlizenziert / kommerziell bis MVP | Lizenz bei MVP entscheiden (siehe `Agents.todo.md`, Antworten des Eigentümers 2026-08-20 → LIZ interim proprietär); dann `license` + Root-`LICENSE` konsistent ergänzen |
| **R3** | 🟠 Hoch | LibRaw-Dynamik-Link-Verpflichtung (§6.3) | Dynamisches Linken beibehalten; LibRaw-Lizenz + Quellangebot für 0.22.2 im Release bündeln |
| **R4** | 🟡 Mittel | `onnx-rt`-Pfad lädt ORT-Prebuilt-Binaries (Netz) | Bei Release-Freigabe ORT-Redistribution + Prebuilt-Terms prüfen, Pin `=2.0.0-rc.13` halten, Modell-Lizenzen/Hashes erfassen |
| **R5** | ✅ Gelöst (2026-08-25) | Veralteter „tri-license"-Verweis (permissive *LibRaw Software License*) — diese Option wurde upstream mit LibRaw 0.18 entfernt; LibRaw 0.22.2 ist **dual** (LGPL-2.1-or-later / CDDL-1.0), ADR 0002 bereits korrekt | Doku (§6.3, ADR 0002, `THIRD-PARTY-NOTICES.md`, `licenses/`) auf Dual-Lizenz korrigiert |
| **R6** | ✅ Gelöst (2026-08-20) | SAM-2-Lizenz ungeprüft; BiRefNet „Apache-2.0" aus Manifest, nicht aus der Gewichtsquelle | Beide an der tatsächlichen Quelle verifiziert und in §5 erfasst: **SAM 2.1 = Apache-2.0** (Code + Gewichte, facebookresearch/sam2 `LICENSE` + Meta-Announcement), **BiRefNet = MIT** (GitHub `LICENSE` + HF-Card `license: mit`) — Manifest-`license`-Feld und Doku korrigiert (Commit folgt); AGPL-Falle via `ultralytics` in §5 dokumentiert |
| **R7** | 🟢 Niedrig | `r-efi` trägt `LGPL-2.1-or-later`-Option | Keine Aktion: UEFI-only, nie ausgeliefert; bei UEFI-Build unter MIT/Apache erfüllen |

Es existiert **keine** GPL/AGPL/SSPL/starke-Copyleft-Abhängigkeit im gesamten
Baum — die einzige copyleft-lizenzierte Crate ist `r-efi` und für ausgelieferte
Targets nicht erreichbar.

---

## 9. Abnahmekriterien (F-073 / F-078)

- [x] Fixture-Inventar dokumentiert (synthetisch + RAW + Generierungsvertrag).
- [x] Modell-Inventar mit Lizenzen dokumentiert (BiRefNet MIT, SAM 2.1
  Apache-2.0, YuNet MIT, SFace Apache-2.0, ORT MIT — jeweils an der
  tatsächlichen Quelle verifiziert, R6/FACE-20-S6).
- [x] Vollständige Abhängigkeits-Lizenztabelle erstellt (`THIRD-PARTY-NOTICES.md`).
- [x] Keine GPL/AGPL/SSPL gefunden; LibRaw als einzige Pflicht benannt.
- [x] Versionierungs-Policy dokumentiert.
- [x] **R1** (RAW-Fixture-Lizenz) geschlossen (2026-08-20, Eigentümer-Gewährung in `sample-data/raw/README.md` dokumentiert).
- [ ] **R2** (Workspace-Crate-Lizenzen + Root-LICENSE) umgesetzt.
- [ ] **R3** (LibRaw-Notice im Release-Bundle) umgesetzt.

---

## 10. Verweise & Verifikation

- `docs/fixtures-and-licensing.md` — ausführliche Audit-Doku (Englisch).
- `THIRD-PARTY-NOTICES.md` — vollständige Crate-Lizenz-Tabelle.
- `docs/adr/0002-raw-backend.md` — RAW-Backend-Entscheidung (LibRaw).
- `README.md` — LibRaw-Hinweis.
- Reproduktion: `cargo metadata --all-features` (siehe `docs/fixtures-and-licensing.md` §7).
