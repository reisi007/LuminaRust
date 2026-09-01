# Generativer Modus „Entfernen + Erweitern“ (GEN-EXPAND-1)

**Feature:** GEN-EXPAND-1 Optionaler generativer Modus „Entfernen + Erweitern“

## Inhaltsverzeichnis

- [Ziel und Abgrenzung](#ziel-und-abgrenzung)
- [Ist-Stand](#ist-stand)
- [Rezeptmodell `GenerativeEdit`](#rezeptmodell-generativeedit)
- [Binäres Sidecar-Artefakt und Prüfsumme](#binäres-sidecar-artefakt-und-prüfsumme)
- [Identität und Veraltung](#identität-und-veraltung)
- [Pipeline-Platzierung und Koordinatensystem](#pipeline-platzierung-und-koordinatensystem)
- [Interaktion mit Crop/Geometry](#interaktion-mit-cropgeometry)
- [Modell, Capability und Lizenz](#modell-capability-und-lizenz)
- [UI-Flow (GUI)](#ui-flow-gui)
- [Abgrenzung zu Source-Actions und KI-Denoise](#abgrenzung-zu-source-actions-und-ki-denoise)
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
normative SOLL für die spätere Umsetzung.

## Rezeptmodell `GenerativeEdit`

Die Stufe wird als additive, versionierte Rezept-Operation der jeweiligen
virtuellen Kopie gespeichert (analog `source_actions` als Pre-MVP-Muster;
Schema- und Migrationsentscheidung vor Einführung, Agents.md
Änderungsregeln). Struktur:

```json
{
  "type": "generative_edit",
  "version": 1,
  "model": {
    "name": "…",
    "version": "…",
    "hash": "…"
  },
  "prompt": "Remove the person on the left; extend the sky to the right",
  "seed": 42,
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
    "mask_id": "…",
    "artifact": { "…": "analog AI-Masken: relativer Pfad, Format, Prüfsumme, Auflösung, Kanaltyp, Datenversion" }
  },
  "artifact": {
    "path": "…",
    "format": "lumina-zdata / png",
    "checksum": "…",
    "width": 6000,
    "height": 4000,
    "channels": 4,
    "data_version": 1
  }
}
```

Pflichtfelder je Operation:

- **Modell:** `model.name`, `model.version`, `model.hash` — identisch zur
  AI-Masken-Identität; `model_hash` muss den tatsächlich geladenen Artefakt-
  Hash bezeichnen (kein `pending` zur Laufzeit, keine stille Variantenwahl).
- **Prompt:** freier Text; optional mit Negativ-Prompt. Der Prompt gehört zur
  Identität und wird roundtrip-stabil persistiert.
- **Seed:** `u64` für deterministische Reproduktion. Gleiche Quelle, gleicher
  Modellkontext, gleicher Prompt, gleicher Seed und gleiche Canvas-Geometrie
  → byte-identisches Artefakt (Abnahme).
- **Canvas-Geometrie:** `canvas.output_width`/`output_height` und die
  Platzierung der Quelle im erweiterten Canvas (`source_offset_x`/
  `source_offset_y`; siehe [Koordinatensystem](#pipeline-platzierung-und-koordinatensystem)).
  Beim reinen Entfernen entspricht das Canvas dem Quellformat.
- **Region/Maske:** Bereich für die Inpainting-Operation als Referenz auf eine
  persistierte Maske (`mask_reference`, analog AI-Masken) oder ein
  Rechteck/geometrischer Prompt in Quellkoordinaten. Für die Erweiterung wird
  der Randbereich (außerhalb der Quelle, im Canvas-Koordinatensystem)
  abgeleitet und benötigt keine separate Maske.
- **Artefakt-Prüfsumme:** `artifact.checksum` des generierten Ergebnisses
  (siehe nächster Abschnitt).

Ein `GenerativeEdit` ohne aktives Modell oder ohne gültiges Artefakt ist
`missing`/`stale` und wird sichtbar gemeldet — es gibt **keinen** stillen
Fallback (kein „weiter so, als wäre nichts generiert worden").

## Binäres Sidecar-Artefakt und Prüfsumme

- Das Ergebnis ist das **vollständige, kompositierte Canvas** (inklusive
  unveränderter Quellpixel), gespeichert als binäres Sidecar-Artefakt —
  analog AI-Masken in `.lumina.zdata` (Record mit `kind`-Diskriminator,
  unverändertem Container-`VERSION`-Muster wie Repair-Regionen aus F-042-N1)
  oder einem dokumentierten, versionierten Format mit Prüfsumme.
- Große Ergebnisse werden **nicht** als unkomprimierte Arrays ins JSON
  geschrieben; das JSON referenziert das Artefakt (relativer Pfad, Format,
  Prüfsumme, Auflösung, Kanaltyp, Datenversion).
- Prüfsumme ist BLAKE3 über den unkomprimierten Pixelstrom (konsistent zur
  zdata-Semantik). Ein bitflipped Artefakt zählt nie als verfügbar.
- Das Original wird nie überschrieben; das Artefakt ist löschbar und durch
  Re-Generierung (bei identischem Kontext byte-identisch) ersetzbar.

## Identität und Veraltung

Eine Generative-Edit-Operation ist **gültig**, wenn alle folgenden Punkte
übereinstimmen; jede Abweichung markiert sie als `stale` (sichtbar, keine
automatische Re-Generierung als einzige Option, Agents.md „AI-Masken"):

- Quell-Content-Hash und relevante Decode-/Geometrieparameter;
- Modellname, Modellversion und Modell-Hash;
- Prompt (inkl. optionalem Negativ-Prompt) und Seed;
- Canvas-Geometrie (Ausgabegröße, Quell-Offset);
- Region-/Masken-Referenz (inkl. Inpainting-Region);
- Artefakt-Format, Auflösung, Kanäle und Prüfsumme;
- Erstellungszeitpunkt, Status und optionaler Fehlertext;
- Pipeline-/Rezeptversion der Stufe.

Statuswerte wie bei AI-Masken: `valid`, `stale`, `missing`, `corrupt`. Ein
fehlendes Modell oder fehlendes Artefakt wird sichtbar gemeldet; die GUI bietet
Neuberechnung explizit an, die CLI verhält sich analog zu `--update-masks`
(Warn-and-continue bzw. `strict`).

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

### Koordinatenreferenzrahmen (normativ)

- Das **post-GenerativeEdit-Canvas** ist der neue Referenzrahmen für alle
  nachgelagerten, normierten Koordinaten (Crop `x/y/width/height` in `0..=1`,
  Masken-Koordinaten, Perspektive-Shift, F-041-Messbereich).
- Die Platzierung der Quelle wird als `canvas.source_offset_x/y` +
  Quell-Dimensionen festgehalten; damit ist der Übergang vom Quell- ins
  Canvas-Koordinatensystem deterministisch reproduzierbar.
- **Regel:** Ändert sich `canvas` (Größe oder Offset), sind alle
  geometrieabhängigen Rezeptwerte, die nach dem Canvas referenzieren, neu zu
  validieren — sie werden **nie still** auf das neue Canvas umgedeutet.
  Stattdessen markiert der RenderKey einen Canvas-Wechsel als Änderung des
  geometrischen Kontexts; abhängige Werte werden als veraltet sichtbar
  gemeldet und (durch den Benutzer) neu gesetzt oder migriert.
- Masken, die auf dem Original gezeichnet wurden, bleiben über ihre eigene
  Referenz (Quell-Koordinaten) eindeutig; bei der Auswertung auf dem
  erweiterten Canvas werden sie über die Quell-Platzierung verschoben
  (dokumentierte Transformation, Teil der Maskenidentität).

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
- **GUI-Interaktion „Expand-Rahmen":** Beim Ziehen des Erweiterungsrahmens
  zeigt die Vorschau das vergrößerte Canvas mit transparentem/skizziertem
  Randbereich; die Rezept-Canvas-Geometrie wird erst beim Bestätigen gesetzt
  (kein Schreiben bei jedem Drag, kein stilles Verwerfen bei Abbruch).
- **Virtuelle Kopien:** `GenerativeEdit` gehört zur jeweiligen virtuellen Kopie
  (eigenes Rezept). Das generierte Artefakt kann auf Quellbild-Ebene geteilt
  werden, wenn Quellkontext, Modellkontext und Canvas-Geometrie identisch
  sind; Masken-Layer, Invertierung und lokale Anpassungen bleiben kopienspezifisch
  (Agents.md, „Virtuelle Kopien").

## Modell, Capability und Lizenz

- **Heimat lokaler Modelle:** `lumina-onnx` (native). Inpainting-/
  Outpainting-Modelle werden wie BiRefNet/SAM 2 über `ModelManifest` mit
  deklarierten Fähigkeiten eingebunden; die Modellfähigkeit wird aus dem
  Manifest gelesen, nicht aus dem Namen erraten.
- **Lokal vs. Cloud getrennt dokumentieren:** Die Capability-Matrix führt
  lokale ONNX-Inferenz und eine (nicht geplante) Cloud-API als **getrennte**
  Capabilities. Cloud-Rechenzentrums-Verarbeitung ist kein stiller Fallback
  für lokale Inferenz und umgekehrt; ohne dokumentierte Capability-
  Entscheidung gibt es keine Cloud-Anbindung.
- **Browser:** ONNX im Browser ist eine optionale Fähigkeit (F-070,
  `onnx-wasm`, off by default) — eine `GenerativeEdit`-Nutzung im Browser ist
  erst mit dieser Capability möglich und wird sichtbar ausgewiesen.
- **Lizenz:** Die Modelle werden **vor Integration** lizenz- und
  hash-gepinnt dokumentiert (F-078, `feature/quality/fixtures-licensing.md`);
  keine spontanen Downloads, keine Tests gegen Netz.
- **Fähigkeitspflicht:** Das gewählte Modell muss mindestens die Fähigkeiten
  für Inpainting bzw. Outpainting deklarieren; ein Modell ohne passende
  Fähigkeit wird abgelehnt (kein stiller Ersatz durch ein anderes Modell).

## UI-Flow (GUI)

Nach GUI-STAGE-1/GUI-WGPU-PRESENT-1 (Native Desktop):

1. Im Develop-Modul aktiviert der Benutzer den generativen Modus
   „Entfernen + Erweitern".
2. **Entfernen:** Objekt über Pinsel/Box auf dem Bild markieren (Maske),
   optional Prompt eingeben, „Generieren" starten.
3. **Erweitern:** „Expand-Rahmen" über die Bildkante ziehen; Zielformat
   (Seitenverhältnis, z. B. 16:9) kann als Orientierung dienen; optional
   Prompt für den Randbereich.
4. Generierung läuft als sichtbarer Job; bei fehlendem Modell/Artefakt wird
   die Capability-Abwesenheit angezeigt, nicht eine gefälschte Vorschau.
5. Ergebnis wird als Artefakt persistiert und im Sidecar-Rezept referenziert;
   Vorschau/Export rendern über die gemeinsame Pipeline auf dem erweiterten
   Canvas.

## Abgrenzung zu Source-Actions und KI-Denoise

- **Source-Actions (F-042):** enthalten kontext-übergebene Reparaturregionen
  (u16-Region + RGBA8-Ersatz) **ohne** Canvas-Vergrößerung und ohne
  Modell-/Prompt-Identität. `GenerativeEdit` erweitert dieses Muster um
  Canvas-Geometrie, Modell-/Prompt-/Seed-Identität und Veraltungslogik.
- **KI-Denoise:** bleibt eine separate, optionale Erweiterung (F-096 sieht nur
  den deterministischen CPU-Pfad vor) und ist unabhängig von GEN-EXPAND-1.

## Abnahme

- Original bleibt byteweise unverändert; das Ergebnis ist ein ableitbares,
  löschbares Artefakt.
- `GenerativeEdit`-Rezept-Roundtrip: Modell, Prompt, Seed, Canvas-Geometrie,
  Region und Prüfsumme überstehen Persistenz/Laden verlustfrei.
- Identische Eingaben (Quelle, Modellkontext, Prompt, Seed, Canvas) erzeugen
  ein byte-identisches Artefakt (Determinismus, getestet).
- Quell-, Modell-, Prompt-, Seed- oder Canvas-Änderung markieren die Operation
  als `stale`; fehlendes Modell/Artefakt wird sichtbar gemeldet — kein stiller
  Fallback.
- Crop/Geometry-Koordinaten referenzieren das post-GenerativeEdit-Canvas;
  ein Canvas-Wechsel invalidiert den Geometrie-Kontext sichtbar statt still
  zu verschieben.
- Veraltungs-, Artefakt- und Canvas-Interaktionstests sind durch einen
  unabhängigen Verifizierungs-Agenten bestätigt.

## Offene Punkte und Abhängigkeiten

- **Abhängigkeiten:** F-082/F-083 (SAM-Adapter, `lumina-onnx`) existiert;
  lokale Inpainting/Outpainting-Modelle und deren Artefakte
  (`pending-integration`); GUI-Flow erst nach GUI-STAGE-1/
  GUI-WGPU-PRESENT-1; `lumina-gpu`/Present-Pfad berührt.
- **Offen:** Modellauswahl (Modellfamilie mit dynamischer Variantenwahl wie bei
  SAM 2.1 oder fixe Modelle), Cloud-API-Capability (bewusst getrennt, siehe
  oben), WASM-Pfad (F-070), Schema-/Migrationsentscheidung für die
  Rezept-Stufe vor Implementierung.
