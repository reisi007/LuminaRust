# F-101 MCP AI-Agent-Schnittstelle

**Feature:** F-101 MCP AI-Agent-Schnittstelle
**Status:** Plan und Dokumentation (SOLL-Spezifikation), keine Implementierung
**Letzter offener Punkt vor MVP-Erklärung**

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [MCP-Protokoll und Transport](#mcp-protokoll-und-transport)
- [Tool-Set](#tool-set)
- [Schnellvorschau](#schnellvorschau-lumina_preview)
- [Architektur](#architektur)
- [Architekturgrenzen](#architekturgrenzen)
- [Nicht-Ziele](#nicht-ziele)
- [Crate-Struktur](#crate-struktur)
- [Abhängigkeiten](#abhängigkeiten)
- [Test-Strategie](#test-strategie)
- [Abnahme](#abnahme)

## Ziel

Ein AI-Agent (z. B. Claude, Codex, lokales LLM mit MCP-Client) soll
Bilddateien laden, Rezeptparameter ändern, Sidecars speichern und eine
schnelle Vorschau erzeugen können — alles über standardisierte MCP-Tools,
ohne GUI und ohne manuelle CLI-Aufrufe.

Der MCP-Server macht LuminaRust als **Tool** für AI-Agenten zugänglich.
Ein Agent kann so in einem autonomen oder halb-autonomen Zyklus:

1. ein Bild laden und dessen aktuellen Zustand inspizieren,
2. gezielt Bearbeitungsregler setzen,
3. eine schnelle Vorschau erzeugen, um das Ergebnis zu beurteilen,
4. bei Bedarf weitere Anpassungen vornehmen,
5. das finale Ergebnis exportieren.

Der Schnellvorschau-Loop (`lumina_preview`) ist der Schlüsselmechanismus:
Er gibt dem Agenten ein schnelles, visuelles Feedback, ohne den vollen
Render-Cache zu belasten oder einen Export-Pfad zu durchlaufen. Der Agent
kann die Vorschau-Datei anschließend über ein Vision-Modell analysieren
lassen, um zu entscheiden, welche weiteren Bearbeitungsschritte nötig sind.

## MCP-Protokoll und Transport

- **Protokoll:** Model Context Protocol, Protokollversion `2024-11-05`
  (oder aktuelle Stable-Version bei Implementierungsbeginn).
- **Transport:** stdio (stdin/stdout). Der Server wird als eigenständiger
  Prozess gestartet und kommuniziert über JSON-RPC-Nachrichten auf
  stdin/stdout.
- **Kein HTTP/WebSocket im MVP.** stdio reicht vollständig für Agent-in-
  Terminal-Szenarien, bei denen der MCP-Client den Server-Prozess selbst
  steuert.
- **Capabilities:** Der Server deklariert `tools` als einzige Fähigkeit.
  `resources` und `prompts` sind im MVP nicht implementiert.

### Server-Start

Der MCP-Server wird über `lumina mcp` (als Subcommand von `lumina-cli`)
oder direkt als eigenständiges Binary `lumina-mcp` gestartet:

```bash
# Über CLI-Subcommand
lumina mcp

# Direkt
lumina-mcp
```

Der Server liest keine Kommandozeilenargumente — alle Konfiguration
erfolgt über die MCP-Handshake-Negotiation oder, im MVP, über feste
Defaults.

### Konfiguration (MVP)

Der Server unterstützt im MVP eine optionale Umgebungsvariable:

| Variable | Default | Beschreibung |
| --- | --- | --- |
| `LUMINA_MCP_PREVIEW_DIR` | `$TMPDIR/lumina-previews/` | Verzeichnis für Schnellvorschauen |
| `LUMINA_MCP_LOG` | `warn` | Log-Level (`error`, `warn`, `info`, `debug`) |

## Tool-Set

### `lumina_load`

Lädt ein Bild und gibt seine Metadaten zurück.

**Input:**
```json
{
  "path": "/pfad/zum/bild.ARW"
}
```

**Output:**
```json
{
  "image_id": "a1b2c3d4",
  "width": 6000,
  "height": 4000,
  "format": "arw",
  "virtual_copies": ["Standard"],
  "sidecar_status": "loaded"
}
```

**Verhalten:**

- Akzeptiert RAW (alle durch `lumina-raw` unterstützten Formate), PNG,
  JPEG und WebP.
- Erkennt vorhandenen Sidecar (`<dateiname>.lumina.json`) oder erzeugt
  einen leeren Standardsidecar mit einer Standard-Virtuellen-Kopie.
- `image_id` ist eine prozess-lokale, stabile ID, die für die Dauer
  der Server-Sitzung gilt. Bei Server-Neustart beginnen die IDs bei null.
- Bei mehrfachem `lumina_load` wird das vorherige Bild aus dem Speicher
  freigegeben (single-image-scoped).
- Fehler: ungültiger Pfad → `FileNotFound`, nicht unterstütztes Format
  → `UnsupportedFormat`, Beschädigte Datei → `DecodeError`.

### `lumina_edit`

Setzt globale Tonwert-Regler im Rezept und schreibt den Sidecar
write-through.

**Input:**
```json
{
  "image_id": "a1b2c3d4",
  "virtual_copy": "Standard",
  "adjustments": {
    "exposure": 0.5,
    "contrast": -0.2,
    "highlights": 0.3,
    "shadows": -0.1,
    "whites": 0.0,
    "blacks": 0.1,
    "wb_temperature": 5500,
    "wb_tint": 0.05
  }
}
```

**Output:**
```json
{
  "ok": true,
  "recipe_hash": "e5f6a7b8"
}
```

**Verhalten:**

- Alle Adjustment-Felder sind optional. Nur gesetzte Felder werden
  überschrieben; ungesetzte bleiben unverändert.
- Wertebereiche entscheiden der Pipeline-Spezifikation (F-036):
  `exposure` in `-10..=10` EV, alle anderen in `-1..=1`,
  `wb_temperature` in `1500..=12000`.
- Ungültige Werte werden mit `InvalidAdjustment` abgelehnt, nicht
  still geclippt.
- `virtual_copy` ist optional; Default ist die erste (Standard-)Kopie.
- Die Operation ist idempotent: Gleicher Input = gleicher
  `recipe_hash`.
- Write-through: Nach erfolgreichem Setzen wird der Sidecar atomar
  geschrieben.

### `lumina_get_recipe`

Liest das aktuelle Rezept einer virtuellen Kopie.

**Input:**
```json
{
  "image_id": "a1b2c3d4",
  "virtual_copy": "Standard"
}
```

**Output:**
```json
{
  "recipe": {
    "exposure": 0.5,
    "contrast": -0.2,
    "highlights": 0.3,
    "shadows": -0.1,
    "whites": 0.0,
    "blacks": 0.1,
    "wb_temperature": 5500,
    "wb_tint": 0.05
  },
  "recipe_hash": "e5f6a7b8"
}
```

**Verhalten:**

- Gibt das vollständige `EditRecipe`-Objekt zurück, nicht nur die
  override-ten Felder.
- `recipe_hash` ermöglicht dem Agenten, Änderungen nachverfolgen zu
  können.

### `lumina_save`

Rendert und exportiert das Bild.

**Input:**
```json
{
  "image_id": "a1b2c3d4",
  "output_path": "/output/bild_editiert.png",
  "format": "png",
  "quality": 90
}
```

**Output:**
```json
{
  "ok": true,
  "bytes_written": 2457600,
  "path": "/output/bild_editiert.png"
}
```

**Verhalten:**

- Nutzt den bestehenden `render_frame`-Einstiegspunkt und
  `ImageFrame::encode` (F-037).
- `format` akzeptiert `"png"`, `"jpeg"`, `"webp"`.
- `quality` ist optional (Default 90), nur für JPEG/WebP relevant
  (`1..=100`).
- Der Export nutzt den gemeinsamen Render-Cache (im Gegensatz zur
  Schnellvorschau).
- Fehler: `RenderError`, `EncodeError`, ungültiges Format.

### `lumina_preview`

Erzeugt eine schnelle, verkleinerte Vorschau — der Schlüsselmechanismus
für den AI-Agent-Feedback-Loop.

**Input:**
```json
{
  "image_id": "a1b2c3d4",
  "virtual_copy": "Standard",
  "max_width": 1024
}
```

**Output:**
```json
{
  "ok": true,
  "preview_path": "/tmp/lumina-previews/a1b2c3d4.png",
  "width": 1024,
  "height": 683,
  "size_bytes": 156000
}
```

**Verhalten:**

- Default `max_width` ist 1024px. Die Höhe wird proportional skaliert.
  Der Agent kann `max_width` auf einen kleineren Wert setzen (z. B.
  512), um die Vorschau schneller zu erzeugen.
- Die Vorschau wird **ohne Cache-Eintrag** erzeugt — sie ist bewusst
  ein Fluchtweg, kein Ersatz für den regulären Cache.
- Rendering nutzt den bestehenden `render_frame`-Einstiegspunkt mit
  reduzierter Ausgabegröße: Das volle Bild wird gerendert und danach
  auf `max_width` herunterskaliert (Bicubic-Resampling). Ein
  Pipeline-Split (niedrige Auflösung als eigene Stufe) ist bewusst
  nicht vorgesehen, um die Pipeline-Validierung nicht zu komplex zu
  machen.
- Die Vorschau wird als PNG im konfigurierten `preview_dir` geschrieben
  (Default `$TMPDIR/lumina-previews/`). Dateiname ist `image_id.png`.
  Bei erneutem `lumina_preview` desselben `image_id` wird die Datei
  überschrieben.
- Der Agent kann den `preview_path` verwenden, um die Datei über ein
  Vision-Modell analysieren zu lassen.
- Determinismus: Gleicher Rezeptstand + gleiche Quelle = gleiche
  Vorschau-Bytes (getestet).
- Die Vorschau ist ein Matrix-bild im RGBA8/sRGB-Arbeitsraum der
  Pipeline; sie enthält keine EXIF- oder Metadaten.

**Architekturentscheidung — kein Low-Res-Pipeline-Split:**

Ein alternativer Ansatz wäre ein separater Pipeline-Pfad, der das Bild
bereits beim Decode auf die Vorschauauflösung beschränkt und so Decode-
und Render-Aufwand drastisch reduziert. Dieser Ansatz wird im MVP
bewusst **nicht** gewählt, weil:

1. Er die Pipeline-Validierung (`Pipeline::validate()`) um eine
   optionale Dimension erweitern würde.
2. Die Decode-Auflösung bei RAW-Dateien ohnehin die volle Sensor-
   auflösung liefert — ein Downscale vor Decode ist mit LibRaw nicht
   direkt möglich.
3. Der cmdline Overhead für ein 6000×4000-Bild auf 1024px ist
   vertretbar (~20-50ms auf moderater Hardware, basierend auf den
   F-074-Benchmarks).

Ein dedizierter Low-Res-Pfad ist ein dokumentiertes Post-MVP-Optimierungs-
ziel, sobald die Performance-Budgets (F-074-A1…A4) eine Engstelle
belegen.

### `lumina_list_virtual_copies`

Listet alle virtuellen Kopien eines geladenen Bildes.

**Input:**
```json
{
  "image_id": "a1b2c3d4"
}
```

**Output:**
```json
{
  "copies": [
    { "id": "vc-001", "name": "Standard", "recipe_hash": "e5f6a7b8" },
    { "id": "vc-002", "name": "Schwarzweiß", "recipe_hash": "f9a0b1c2" }
  ]
}
```

### `lumina_inspect`

Zeigt den vollständigen Zustand eines geladenen Bildes.

**Input:**
```json
{
  "image_id": "a1b2c3d4"
}
```

**Output:**
```json
{
  "source_path": "/fotos/bild.ARW",
  "sidecar_path": "/fotos/bild.ARW.lumina.json",
  "recipe_version": 1,
  "pipeline_version": 1,
  "virtual_copies": 2,
  "ai_masks": [
    { "layer": "subject", "status": "valid" }
  ]
}
```

**Verhalten:**

- Liest Sidecar-Status und Metadaten, ohne das Bild zu decodieren.
- Nützlich für den Agenten, um zu prüfen, ob ein Sidecar vorhanden
  ist und welche Masken bereits existieren.

## Schnellvorschau (`lumina_preview`)

### Zweck

Der Agent erzeugt nach jeder relevanten Bearbeitungsänderung eine
Schnellvorschau, um das Ergebnis visuell beurteilen zu können. Das
Typische Nutzungsmuster:

```
Agent: lumina_load(path="/foto/portrait.ARW")
       → image_id: "a1b2c3d4", 6000×4000

Agent: lumina_edit(image_id="a1b2c3d4", adjustments={exposure: 1.2})
       → ok, recipe_hash: "b3c4d5e6"

Agent: lumina_preview(image_id="a1b2c3d4", max_width: 1024)
       → preview_path: "/tmp/lumina-previews/a1b2c3d4.png"

Agent: [analysiert Vorschau über Vision-Modell]
       → "Zu hell, Highlights überbelichtet. Reduziere Highlights."

Agent: lumina_edit(image_id="a1b2c3d4", adjustments={highlights: -0.5})
       → ok, recipe_hash: "f7g8h9i0"

Agent: lumina_preview(image_id="a1b2c3d4", max_width: 1024)
       → [neue Vorschau, Agent bestätigt Ergebnis]

Agent: lumina_save(image_id="a1b2c3d4", output_path="/output/portrait.png", format="png")
       → ok, bytes_written: 2457600
```

### Implementierungs-Spezifikation

1. **Rendering:** `render_frame` mit vollem Rezept auf Originalauflösung,
   danach bilineares Downscaling auf `max_width` (Proportionen halten).
2. **Format:** Immer PNG (verlustfrei, von jedem Vision-Modell lesbar).
3. **Ablage:** `image_id.png` in `preview_dir` (Default
   `$TMPDIR/lumina-previews/`). Verzeichnis wird beim Start erzeugt.
4. **Lebensdauer:** Dateien bleiben bestehen, bis der Server beendet
   wird oder der nächste `lumina_preview` für dasselbe `image_id`
   überschreibt. Kein automatisches Pruning.
5. **Cleanup:** Der Server kann beim Shutdown alle Vorschauen löschen
   (optional, Default: ja).

## Architektur

### Crate `lumina-mcp`

```
crates/lumina-mcp/
├── Cargo.toml
├── src/
│   ├── lib.rs           # MCP-Protokoll-Handler, Tool-Dispatch
│   ├── tools/
│   │   ├── mod.rs
│   │   ├── load.rs      # lumina_load
│   │   ├── edit.rs      # lumina_edit
│   │   ├── recipe.rs    # lumina_get_recipe
│   │   ├── save.rs      # lumina_save
│   │   ├── preview.rs   # lumina_preview
│   │   ├── copies.rs    # lumina_list_virtual_copies
│   │   └── inspect.rs   # lumina_inspect
│   ├── session.rs       # Bild-Session-State (single-image-scoped)
│   └── main.rs          # Binary-Entry (optional, oder in lumina-cli)
```

### Abhängigkeitsgraph

```
lumina-mcp
  ├── lumina-core    (render_frame, ImageFrame, EditRecipe)
  ├── lumina-sidecar (Sidecar laden/schreiben, atomare Writes)
  ├── lumina-raw     (RAW-Decode, Metadaten — indirekt über lumina-core)
  └── serde, serde_json (MCP-JSON-RPC, Tool-Schemas)
```

### Session-State

Der Server hält pro Sitzung den Zustand eines einzigen geladenen
Bildes:

```rust
struct McpSession {
    /// Aktuell geladenes Bild (image_id → ImageState)
    current: Option<ImageState>,
}

struct ImageState {
    id: String,
    source_path: PathBuf,
    frame: ImageFrame,
    raw_metadata: Option<RawMetadata>,
    sidecar: LuminaSidecar,
    active_copy: String,
}
```

Bei einem neuen `lumina_load` wird der vorherige Zustand verworfen.
Es gibt keinen Multi-Image-Speicher im MVP.

### JSON-RPC-Handling

Der Server implementiert einen Minimal-MCP-Server:

1. Liest JSON-RPC-Nachrichten von stdin.
2. `initialize` → antwortet mit Server-Name, Version und
   `tools`-Capability.
3. `tools/list` → antwortet mit der Tool-Liste (Name, Beschreibung,
   JSON-Schema für Input).
4. `tools/call` → dispatcht an die passende Tool-Funktion, gibt
   `content` (Text/JSON) oder `error` zurück.
5. `notifications/initialized` → akzeptiert, beantwortet nicht.

Fehler werden als MCP-Error-Response zurückgegeben:

```json
{
  "code": -32602,
  "message": "Invalid image_id: unknown image",
  "data": { "tool": "lumina_edit", "image_id": "unknown" }
}
```

## Architekturgrenzen

- **Keine eigene Bildverarbeitung.** Alle Renderoperationen laufen über
  `render_frame` aus `lumina-core`. Der MCP-Server ist ein reiner
  Orchestrierungs- und Interface-Layer.
- **Opener, kein zweites Backend.** Der Server kapselt dieselbe Logik
  wie CLI und GUI, er implementiert keine alternative Pipeline.
- **Atomare Sidecar-Writes.** Schreiboperationen nutzen dieselben
  Pfade wie CLI und GUI. Ein Abbruch zwischen Render und Sidecar-Write
  hinterlässt keinen korrupten Zustand (konsistentes Fehlschlags-
  verhalten mit CLI).
- **Kein eigener Render-Cache.** `lumina_save` nutzt den gemeinsamen
  Cache. `lumina_preview` ist bewusst cache-frei (niedrige Auflösung,
  häufige Überschreibung).
- **Single-image-scoped.** Der Server hält genau ein Bild im Speicher.
  Multi-Image-Parallelverarbeitung ist Post-MVP und erfordert eine
  Session-Revypsion (Mutex über Bild-Map statt `Option<ImageState>`).
- **Keine ONNX-/Masken-Inferenz.** Masken werden über `lumina_inspect`
  angezeigt, aber nicht berechnet. Inferenz über MCP ist ein Post-MVP-
  Feature, das die `lumina-onnx`-Abhängigkeit in `lumina-mcp` bringen
  würde.

## Nicht-Ziele (Pre-MVP)

- Kein HTTP/WebSocket-Transport — stdio reicht für Agent-in-Terminal.
- Keine Multi-Image-Parallelverarbeitung.
- Keine AI-Masken-Inferenz über MCP.
- Keine Preset-Verwaltung über MCP (kein `lumina_apply_preset`).
- Keine Batch-Befehle (ein Aufruf = ein Bild).
- Keine Authentifizierung oder Zugriffssteuerung (lokaler Prozess).
- Keine `resources` oder `prompts` MCP-Capabilities.
- Keine Virtual-Copy-Erstellung oder Löschung über MCP (nur Lesen
  und Rezept-Bearbeitung).

## Erweiterter MVP-Scope (2026-08-19 User-Anforderung)

Der MCP-Server soll **alle CLI-Funktionalitäten** abbilden, nicht nur die
aktuell spezifizierten 7 Tools. Zusätzliche Anforderungen:

### Volle CLI-Abdeckung
Jeder `lumina`-CLI-Befehl soll ein MCP-Tool werden:
- `lumina import` → `lumina_import`
- `lumina develop` / `lumina render` → `lumina_render` (bereits spezifiziert)
- `lumina export` → `lumina_export` (bereits spezifiziert)
- `lumina batch` → `lumina_batch` (ein Aufruf = ein Verzeichnis)
- `lumina info` → `lumina_inspect` (bereits spezifiziert)
- `lumina reindex` → `lumina_reindex`
- `lumina dust-removal` → `lumina_dust_removal`
- `lumina mcp` → Server-Selbstreferenz (nicht als Tool)

### Vision-fähiger Agent (Vorschau-analysieren)
Der MCP-Server soll einen Weg bieten, den aktuellen Edit-Zustand visuell
zu analysieren:
- `lumina_preview` liefert den Pfad zur gerenderten Vorschau (PNG/JPEG).
  Ein vision-fähiger Agent (z. B. Claude mit Vision, GPT-4V) kann das Bild
  dann direkt analysieren.
- **Alternativ:** `lumina_analyze` liefert strukturierte Bilddaten
  (Histogramm, Farbstatistiken, Dominante Farben, Exposition) als JSON —
  nützlich für Agents ohne Vision-Fähigkeit.
- **Ziel:** Der Agent soll den aktuellen Bearbeitungszustand SEHEN und
  darüber urteilen können (z. B. „die Belichtung ist zu hoch", „der
  Himmel ist过曝").

### Agent-Skill für LuminaRust
Ein OpenCode/Agent-Skill (`lumina.md` oder ähnlich), der AI-Agenten
beibringt, wie sie mit LuminaRust arbeiten:
- Erklärung der Sidecar-Philosophie (nicht-destruktiv, Rezept-basiert)
- MCP-Tool-Referenz mit Parametern und Beispielen
- Typische Workflows (Open → Edit → Preview → Export)
- Best Practices für effektive Bearbeitung via Agent
- Fehlerbehandlung und Statusinterpretation
- Wird als OpenCode-Skill bereitgestellt (oder als generischer
  MCP-Client-Guide)

### Namensfindung
Der finale Produktname wird vor dem MVP-Release festgelegt.
Aktueller Projektname: LuminaRust. Brainstorm-Liste:
`docs/naming-brainstorm.md`.

## Crate-Struktur

### `lumina-mcp` (neues Crate)

- **Typ:** Binary + Library (Library für Tests und potentielle
  Integration in `lumina-cli` als Subcommand).
- **`Cargo.toml`-Abhängigkeiten:** `lumina-core`, `lumina-sidecar`,
  `lumina-raw` (optional, für Metadaten-Direktzugriff), `serde`,
  `serde_json`, `tokio` (für stdin/stdout-Async, falls MCP-Client
  async erwartet).
- **`main.rs`:** Minimaler MCP-Server-Loop. Liest JSON-RPC von stdin,
  dispatcht an Tools, schreibt nach stdout.
- **Feature-Flag:** `lumina-mcp` ist optional und wird über ein
  Cargo-Feature in `lumina-cli` aktiviert (`--features mcp`), oder als
  eigenständiges Binary gebaut.

### Integration in `lumina-cli` (Option)

Der MCP-Server kann als Subcommand in `lumina-cli` integriert werden:

```bash
lumina mcp          # Startet den MCP-Server
```

Dies erfordert ein `mcp` Feature-Flag in `lumina-cli`, das `lumina-mcp`
als Abhängigkeit einbindet. Alternativ kann `lumina-mcp` als
eigenständiges Crate eigenständig gebaut und gepublished werden.

## Abhängigkeiten

| Abhängigkeit | Status | MUSS vorhanden sein |
| --- | --- | --- |
| `render_frame` (F-042) | Implementiert | Ja, vor Implementierung |
| `EditRecipe` + Sidecar-Serialisierung | Implementiert | Ja, vor Implementierung |
| `ExportOptions` + `ImageFrame::encode` (F-037) | Implementiert | Ja, vor Implementierung |
| `lumina-raw` (Decode + Metadaten) | Implementiert | Ja, vor Implementierung |
| `lumina-cli` (Referenz-Orchestrierung) | Implementiert | Nein, nur als Vorbild |
| Phase 6 AI-Masken (F-047…F-083) | Offen | Nein, Masken werden nur gelesen |

**Abhängigkeiten von F-101:** F-101 selbst ist keine Abhängigkeit für
andere Features. Es baut ausschließlich auf vorhandenen APIs auf.

## Test-Strategie

### Unit-Tests

- Tool-Dispatch: Jedes Tool wird mit gültigem und ungültigem Input
  getestet (Schema-Validierung, Fehlerpfade).
- JSON-Schema: Die Tool-Schemas erzeugen gültiges MCP-`tools/list`-
  Response-Format.
- Error-Response: Fehler werden als korrekte MCP-Error-Objekte mit
  Code, Message und Data zurückgegeben.

### Integrationstests

- **Roundtrip-Test:** `lumina_load` → `lumina_edit` (set exposure 1.0)
  → `lumina_preview` → `lumina_save` → Datei decodieren und
  Dimensionen/Format prüfen.
- **Determinismus-Test:** Zwei `lumina_preview`-Aufrufe mit gleichem
  Rezept und gleicher Quelle liefern byte-identische PNG-Dateien.
- **Sidecar-Persistenz:** Nach `lumina_edit` → Server-Neustart →
  `lumina_load` → `lumina_get_recipe` muss der gesetzte Wert
  wiederhergestellt sein.
- **Virtuelle Kopien:** `lumina_list_virtual_copies` nach `lumina_load`
  liefert mindestens eine Kopie.

### Fehlerpfadtests

- `lumina_load` mit ungültigem Pfad → `FileNotFound`.
- `lumina_load` mit nicht unterstütztem Format → `UnsupportedFormat`.
- `lumina_edit` mit `image_id` ohne vorheriges `lumina_load` →
  `NoImageLoaded`.
- `lumina_edit` mit ungültigen Adjustment-Werten → `InvalidAdjustment`.
- `lumina_save` mit nicht unterstütztem Format → `UnsupportedFormat`.
- `lumina_preview` ohne geladenes Bild → `NoImageLoaded`.

### MCP-Protokoll-Compliance

- `initialize`-Handshake liefert korrekte Server-Info und
  Capabilities.
- `tools/list` liefert alle sieben Tools mit gültigen Schemas.
- `tools/call` mit unbekanntem Tool-Namen → MCP-Error.
- Respons-Format entspricht MCP-Spezifikation (`content` Array mit
  `type: "text"`).

### Test-Befehle (Pre-Implementierung — Plan)

```bash
# Unit- und Integrationstests
cargo test -p lumina-mcp

# MCP-Protokoll-Compliance (manuell oder über Test-Binary)
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run -p lumina-mcp

# Clippy
cargo clippy -p lumina-mcp -- -D warnings

# Formatierung
cargo fmt -p lumina-mcp --check
```

## Abnahme

F-101 ist umgesetzt und abnahmefähig, wenn:

- [ ] Alle sieben MCP-Tools (`lumina_load`, `lumina_edit`,
      `lumina_get_recipe`, `lumina_save`, `lumina_preview`,
      `lumina_list_virtual_copies`, `lumina_inspect`) funktionieren
      und die dokumentierten Outputs liefern.
- [ ] `lumina_preview` erzeugt deterministische, cache-freie
      Vorschauen mit konfigurierbarer Breite.
- [ ] `lumina_edit` schreibt Sidecars atomar und idempotent.
- [ ] Der MCP-Server startet über `lumina mcp` (oder `lumina-mcp`)
      und beendet sich sauber.
- [ ] Der `initialize`-Handshake liefert korrekte MCP-Capabilities.
- [ ] Unit-, Integration- und Fehlerpfadtests bestehen.
- [ ] Clippy (`-D warnings`) und Formatprüfung laufen grün.
- [ ] Ein unabhängiger Verifizierungs-Agent bestätigt die Testabdeckung
      und Korrektheit.
- [ ] `feature/platform/mcp-server.md` ist das normative SOLL-Dokument
      und mit `feature/README.md` verlinkt.
