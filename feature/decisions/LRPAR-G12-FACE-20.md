# LRPAR-G12-FACE-20 — Gesichtserkennung (Release 2.0, Doku-first)

**Task:** `LRPAR-G12-FACE-20` (Release 2.0, G-12-Abspaltung, User-Entscheid 2026-09-03)
**Typ:** Doku-first Entscheid (kein Code, keine Tests, keine Schemaänderung)
**Status:** Entscheid vorgeschlagen, Umsetzung ausstehend
**Referenzen (normativ, unverändert):** `feature/product/ai-masks.md` (F-004, gelesen, nicht geändert),
`feature/README.md`, `feature/platform/capability-matrix.md`, `feature/quality/fixtures-licensing.md` (F-073/F-078),
`feature/decisions.md`, `.goal/Goal.md` G-12 (~0 %)
**Regeln:** `Agents.md` (Sidecar-first, kein stiller Fallback, keine absoluten Pfade, keine privaten Daten,
Lizenzprüfung vor Integration, Capability-Matrix)

## 1. Ziel und Nicht-Ziele

Ziel (Release 2.0): KI-Gesichtserkennung + Personen-Tagging im Library-Sinn
(Referenz: `.goal/Goal.md` G-12) — Gesichter finden, Personen gruppieren,
Namen vergeben, nach Personen filtern und Gesichtsregionen als Maskenquelle
nutzen.

Ausdrücklich **kein Ziel**:

- **Karten-Modul / GPS-Verortung ist nie Ziel.** Es gibt dafür keinen Task,
  keine Planung und keine Persistenzfelder. Geotags werden weder gelesen,
  geschrieben noch angezeigt. Diese Abgrenzung ist endgültig (User-Entscheid
  2026-09-03, `.goal/Goal.md` G-12, `Agents.todo.md` Releaseplan „nie").
- Keine Veröffentlichungsdienste (nie Ziel, G-15).
- Kein Cloud-Upload von Bildern, Gesichtern oder Embeddings.
- Kein automatisches Überschreiben von Keywords/Metadaten aus Personennamen
  ohne explizite Aktion.

## 2. Modell-/Lizenz-/Capability-Entscheid

### 2.1 Pipeline (drei Stufen, getrennt versioniert)

1. **Detektion (ONNX):** Gesichts-Boxen + Landmarken pro Bild. Kandidatenfamilie:
   kompakte Single-Shot-Detektoren im ONNX-Format (z. B. SCRFD-/RetinaFace-/
   YuNet-Familie; exakte Variante wird bei Integration per Benchmark und
   Lizenzprüfung gewählt). Eingang: RGB, dokumentierte Inferenzauflösung
   (z. B. 640×640, final im Manifest), Ausgabe: Boxen + Score + Landmarken.
2. **Embedding (ONNX):** pro detektierter Box ein normierter Identitätsvektor
   (Kandidatenfamilie: ArcFace-/FaceNet-Familie; Dimension z. B. 128/192/512,
   final im Manifest). Alignment per Landmarken vor dem Embedding-Lauf.
3. **Clustering (lokal, modellfrei, deterministisch):** Gruppierung der
   Embeddings zu Personen-Clustern (z. B. DBSCAN über Kosinus-Distanz mit
   dokumentierten `eps`/`min_samples` oder agglomeratives Clustering;
   Schwellen werden kalibriert und versioniert). Kein Training, kein
   Online-Lernen; Nutzer bestätigt/teilt/vereinigt Cluster manuell.

Jede Stufe erhält eigene Modell-/Verfahrensidentität (Name, Version, Hash);
ein Wechsel genau einer Stufe invalidiert nur deren abhängige Artefakte
(siehe §4).

### 2.2 Lizenz (F-078-Gate, vor jeder Integration)

- Kein Modell wird committet, gebündelt oder per Test heruntergeladen, bevor
  Lizenz (Code **und** Gewichte), Quelle, Version und Hash geprüft und in
  `feature/quality/fixtures-licensing.md` dokumentiert sind.
- Bis hash-gepinnte Gewichte committet sind, tragen alle Face-Manifeste
  `model_hash = "pending-integration"` (`ModelHashStatus::Pending` — nicht
  verifizierbar, nie als `Verified` ausgebbar), analog BiRefNet/SAM 2
  (`feature/product/ai-masks.md` Maskenidentität, `fixtures-licensing.md` §5).
- Tests nutzen ausschließlich lokale, hash-gepinnte Fixtures oder
  deterministische Stubs; kein spontaner Modell-Download, kein Netzwerk
  (Agents.md-Verifizierungsregeln).
- Bekannte Falle: Export-/Helper-Pakete mit starker Copyleft-Lizenz
  (Analogon zur `ultralytics`-AGPL-Falle in `fixtures-licensing.md` §5) sind
  für Export und Inferenz verboten; zulässig ist nur der direkte Weg
  Meta-/Upstream-Checkpoint → ONNX (MIT-Tooling) → `lumina-onnx`/`ort` (MIT).
- Projektlizenz-Vorbehalt: Die interim proprietäre Projektlizenz
  (`Agents.todo.md` LIZ) bleibt unberührt; Face-Modelle fügen keine
  Copyleft-Pflicht zum Gesamtwerk hinzu, solange der Lizenzentscheid je
  Modell dies bestätigt.

### 2.3 Capability (native-only, kein stiller Fallback)

- `lumina-onnx` bleibt **native-only** (CLI + Desktop, `onnx-rt`-Feature,
  analog `feature/platform/capability-matrix.md`, F-082-FOLLOWUP).
  Kein Browser-Pfad (WASM ersatzlos gestrichen 2026-09-04).
- Backend-Auswahl wie bestehend: `try_load_onnx_engine` → `OnnxRuntime`
  (Artefakt verifiziert) / `RuntimeDisabled` (Feature aus) / harter Fehler
  (`MissingModel` / `ModelArtifactStale` / `InferenceFailed`). Nie stiller
  Stub-Ersatz, nie stille Neuberechnung.
- **Lokal vs. Cloud sind getrennte Capabilities:** Gesichts-Inferenz läuft
  ausschließlich lokal per ONNX. Eine Cloud-Gesichtserkennung ist nicht
  geplant und bedürfte einer eigenen Capability-Entscheidung.
- Fehlendes Modell, fehlendes Artefakt oder veränderte Quelle → sichtbarer
  Status (`missing` / `stale` / `corrupt`), Warnung vor Export, explizite
  Neuberechnung (CLI-Flag analog `--update-masks`, GUI-Aktion). Kein stiller
  Fallback.
- Datenschutz-Capability: Alle Face-Daten (Boxen, Embeddings, Cluster,
  Namen) bleiben lokal in Sidecar-Artefakten. Kein Telemetrie-, kein
  Cloud-, kein Cross-Katalog-Abgleich. Namen sind Nutzerdaten und werden
  nie in Modell- oder Benchmark-Fixtures übernommen.

## 3. UI-Scope (Library, Lightroom-Parität light)

- Personen-Ansicht im Library-Modul: Cluster-Übersicht (unbenannte Cluster +
  benannte Personen), Raster der Gesichts-Crops pro Person/Cluster.
- Aktionen: Name vergeben/umbenennen, Cluster bestätigen, Cluster teilen,
  Cluster vereinigen, Gesicht ignorieren, Person löschen (nur Label, nie Pixel).
- Filter/Suche: nach Person filtern (ergänzt die `\-Leiste`, kein Index-Zwang;
  optionaler Index nur rebuildbar aus Sidecars).
- Develop-Brücke: Gesichtsregion als Maskenquelle („Person/Gesicht auswählen")
  — die erzeugte Matte folgt `feature/product/ai-masks.md` (Masken-DAG,
  lokale Modulation pro Kopie). Keine automatischen „Haare von Person N"-
  Teilkategorien in 2.0 (spätere Instanz-/Teilsegmentierung, vgl. ai-masks.md).
- Tastatur: keine neuen globalen Shortcuts in diesem Entscheid reserviert;
  ein Personen-Shortcut (Referenz `O` aus G-12) wird erst im GUI-Slice
  vergeben, wenn er nicht mit F-100-Konventionen kollidiert.
- Jede sichtbare Funktion braucht vor Implementierung ihren SOLL-Satz im
  betroffenen Feature-Dokument und danach einen headless GUI-Test
  (`cargo test -p lumina-gui`, ohne GPU), analog Agents.md-GUI-Regel.

### 3.1 Umsetzungsnotiz S5 (2026-09-16, Rework F1–F4/F8/F9)

- **Brücke umgesetzt (F1):** Die People-Ansicht (`crates/lumina-gui/src/face_gui.rs`)
  besitzt jetzt eine Develop-Brücke. `LuminaApp::create_face_mask` wandelt eine
  **persistierte, gültige Detektions-Box** in eine deterministische Maskenquelle
  der aktiven virtuellen Kopie um (`MaskPrompt::Box`, geometrisch vom
  gemeinsamen `lumina-core`-Maskengraph gerastert — derselbe modellfreie Weg wie
  jede andere Prompt-Maske). Die Provenienz (`detection_id`,
  `face_identity_digest`) wird im Prompt gespeichert; die Maske ist
  `MaskStatus::Valid` und modelliert keinen erfundenen Bereich. Die Aktion ist
  pro Gesicht in der People-Ansicht sichtbar.
- **Laut statt still (F1):** Eine fehlende (`no analysis`), `stale`, `missing`
  oder `corrupt` Analyse, eine unbekannte Detektions-ID und ein bereits
  abgeleiteter Masken-ID-Konflikt werden hart abgelehnt; kein stiller Fallback.
- **Bekannte Grenze:** Die erzeugte Matte ist die **Detektions-Box-Region**, keine
  modellproduzierte Gesichts-/Personenmatte (es gibt weiterhin keinen
  Gesichtsmatte-Modellpfad; das wäre S2/S6). Der generische
  `AiSelectKind::People`-KI-Selektor bleibt ein **getrennter**, modellabhängiger
  Pfad (er braucht eine geladene/inferierte Plane) und wird nicht still auf die
  Detektionsboxen umgebogen.
- **Statuskontrakt (F2/F4):** `FaceViewStatus` prüft die referenzierten
  Vektorartefakte mit echtem BLAKE3-Hash der Bundle-Bytes (kein
  Existenz-Schluss); ein abweichender Hash ist `corrupt`, eine fehlende Datei
  `missing`. Eine Analyse ohne persistierte Vektoren trägt keine prüfbare
  Nutzlast und meldet `missing` statt eines falschen `valid` — visuell als
  veraltet/unvollständig, nie still.

## 4. Persistenz-Scope (Sidecar-first)

- **Source of truth ist das Sidecar** (`<name>.lumina.json` + `<name>.lumina.zdata`,
  relativ, portabel, atomar, keine absoluten Pfade — Agents.md).
- **Quellbild-Ebene (geteilt):** Detektionen (Boxen, Landmarken, Scores),
  Embeddings und Cluster-Zuordnungen gehören zur Quelle, nicht zu einer
  Kopie. Begründung: gleiche Pixel, gleiche Gesichter — Re-Inferenz pro
  virtueller Kopie wäre Verschwendung und bräche die Regel „gültige
  persistierte AI-Ergebnisse werden wiederverwendet".
- **Personen-Labels:** Personen-IDs (stabile, sidecar-eindeutige IDs, nie
  Array-Position) + Anzeigenamen + Bestätigungsstatus liegen im Sidecar auf
  Quellebene (pro Bild; katalogweite Personen-Identität über Bilder hinweg
  ist in 2.0 explizit **nicht** enthalten — nur bildlokale Cluster + Namen;
  katalogweite Zusammenführung wäre ein eigener Folgeentscheid mit
  Index-Rebuild aus Sidecars).
- **Virtuelle Kopien:** Masken-Layer, Invertierung, Feathering/Blur/Density
  und lokale Regler aus Gesichtsregionen gehören zur jeweiligen virtuellen
  Kopie (eigene vollständige Rezepte, stabile Kopie-IDs, Standardkopie
  unlöschbar — Agents.md). Die Quellmatte wird pro Kopie unterschiedlich
  moduliert, nie in die Quellmatte gebrannt (analog ai-masks.md § Lokale
  Anpassungen).
- **Identität pro Face-Artefakt (analog AI-Masken):** Quell-Content-Hash +
  Decode-/Geometrieparameter, Detektionsmodell (Name/Version/Hash),
  Embedding-Modell (Name/Version/Hash), Inferenzauflösung, Vorverarbeitung
  (Alignment-Methode, Normalisierung), Nachskalierung/Koordinatensystem,
  Clustering-Verfahren + Schwellen + Version, Matte-Format/Auflösung/Kanäle/
  Prüfsumme, Erstellungszeitpunkt, Status (`valid`/`stale`/`missing`/`corrupt`)
  + optionaler Fehlertext. Große Vektoren/Masken liegen binär in
  `.lumina.zdata` (komprimiert, referenziert mit Pfad/Format/Prüfsumme/
  Auflösung/Kanaltyp/Datenversion); keine unkomprimierten Float-Arrays im JSON.
- **Gültigkeit:** Eine Face-Ablage ist gültig, wenn Quelle, Decode-Kontext,
  Modellkontexte (Detektion + Embedding), Clustering-Version und
  Artefakt-Prüfsumme übereinstimmen. Abweichung → `stale` (sichtbar,
  explizite Neuberechnung). Fehlendes Artefakt → `missing`. Prüfsummenfehler
  → `corrupt`. Modellwechsel → keine stille Neuberechnung.
- **Optionaler Index:** darf Face-Suchdaten cachen, muss aus Sidecars
  vollständig rebuildbar sein und darf nie alleinige Quelle für Personen,
  Cluster oder Rezepte sein (Agents.md, `feature/architecture/index.md`).

## 5. Abgrenzung Karten/GPS (nie Ziel)

- Kein Karten-Modul, kein GPS-Lesen/Schreiben/Anzeigen, keine Geotag-Felder
  im Sidecar, keine Karten-Ansicht in der GUI, keine Standort-Filter.
- EXIF-GPS-Tags eingehender RAWs werden ignoriert (weder persistiert noch
  exportiert); ein späterer Metadaten-Task (G-15) darf daran nichts ändern,
  ohne einen eigenen Entscheid mit Datenschutz-Folgenabschätzung.
- Es wird kein Folge-Task für Karte/GPS angelegt. Erwähnungen in G-12-Doku
  dienen nur der Abgrenzung.

## 6. Folge-Implementierungstasks (Vorschlag an den Build-Agenten)

Alle als eigene Tasks mit je Implementierungs- + unabhängigem
Verifizierungs-Agenten; Reihenfolge seriell bei Schema-/API-Berührung:

1. **FACE-20-S1 Schema:** Face-Felder (Detektionen, Embeddings-Referenzen,
   Cluster, Personen-Labels, Identität/Status) im Sidecar-Schema,
   Roundtrip- + Migrationstests, atomare Writes, keine absoluten Pfade.
2. **FACE-20-S2 ONNX:** Detektions- + Embedding-Manifeste (`ModelManifest`,
   Capabilities), Stub-Backends (deterministisch, tests-only), echter
   ORT-Pfad hinter `onnx-rt` mit Hash-Verifikation, harte Fehler ohne
   stillen Fallback.
3. **FACE-20-S3 Clustering:** deterministisches Clustering-Modul
   (Schwellen versioniert), Confirm/Split/Merge-Semantik, Unit- +
   Property-Tests (Stabilität, Leereingaben, Single-Face).
4. **FACE-20-S4 CLI:** Face-Analyse-/Update-Kommandos (Exit-Codes, Warnungen
   an `stderr`, explizites Refresh-Flag), E2E-Tests inkl. missing/stale/
   corrupt-Pfade.
5. **FACE-20-S5 GUI:** Personen-Ansicht + Develop-Brücke (Maskenquelle),
   headless Tests (`cargo test -p lumina-gui`), Golden/PSNR nur wo visuell
   relevant, kein manueller Test als einzige Absicherung.
6. **FACE-20-S6 Lizenzen/Fixtures:** Modell-Lizenzen + Pins in
   `fixtures-licensing.md` nachtragen, `THIRD-PARTY-NOTICES.md` ergänzen,
   hash-gepinnte Fixtures ohne Netzwerk.
7. **Face-Vektor-Persistenz (Folgearbeit, Stand 2026-09-16):** Es existiert
   noch kein `.lumina.zdata`-RecordKind für Embedding-Vektoren; CLI/GUI
   persistieren Detektionen/Landmarken/Cluster und melden fehlende Vektoren
   laut (`missing`, nie fälschlich `valid`; Whole-File-BLAKE3 bis der
   Record-Kind existiert). Mit dem Record-Kind: echte Record-Checksummen +
   Umstellung der Evidence-Pfade in CLI/GUI.

**Stand 2026-09-16 (S1–S5, Verifizierung BESTANDEN):** S1 Schema, S2 ONNX,
S3 Clustering, S4 CLI (Exit-Codes inkl. Usage-2, echte BLAKE3-Evidence),
S5 GUI (People-Ansicht + Face→Masken-Brücke als Box-Region, kein GPS).
Offen: S6 Gewichte/Lizenzen, Vektor-Record-Kind (Punkt 7), GUI-/CLI-
Dedup-Notizen (Face-Evidence-Helper bewusst gespiegelt, byte-identisch).

Jeder Slice braucht: SOLL-Satz im Feature-Dokument vor Code (falls Semantik
unklar), Tests mit der Implementierung, Verifizierungsbericht mit
BESTANDEN-Checkliste (`DoD.md` §7).

## 7. Abnahme (dieser Doku-Task)

- [ ] Dieser Entscheid liegt unter `feature/decisions/LRPAR-G12-FACE-20.md`
  (einzige geänderte/neue Datei dieses Tasks, kein Code).
- [ ] Modell-/Lizenz-/Capability-Entscheid (§2), Detektion + Embedding +
      Clustering + UI + Persistenz-Scope (§2–§4) und Karten/GPS-Abgrenzung
      (§5) sind dokumentiert.
- [ ] Folge-Implementierungstasks sind als Vorschlag formuliert (§6);
      der Build-Agent übernimmt sie als echte Tasks nach Verifizierung.
- [ ] Keine privaten Daten, keine echten Pfade, keine absoluten Pfade,
      keine Screenshot-/Katalognamen im Dokument.
- [ ] Unabhängiger Verifizierungs-Agent bestätigt: Entscheid vollständig,
      widerspruchsfrei zu `Agents.md` / `feature/README.md` /
      `feature/product/ai-masks.md` / `.goal/Goal.md` G-12, und der
      Folge-Task-Vorschlag ist umsetzbar.
