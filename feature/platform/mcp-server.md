# F-101 MCP AI-Agent-Schnittstelle

**Feature:** F-101 MCP AI-Agent-Schnittstelle
**Status:** Umgesetzt und verifiziert (Crate `lumina-mcp`, 8 Tools inkl.
`lumina_analyze`, Agent-Skill `docs/skills/lumina.md`; 45 Tests grün).
**F-101-F1 erweiterter Scope (2026-08-26):** 4 zusätzliche CLI-Abdeckungs-
Tools (`lumina_import`, `lumina_batch`, `lumina_reindex`,
`lumina_dust_removal`) + `lumina mcp` als CLI-Subcommand (Feature `mcp`
in `lumina-cli`, Default an). F-101-F1 selbst brachte **12** Tools; mit den
Metadaten-Tools aus LRPAR-G15-IPTC-S7 waren es **17**, mit den vier
Stage-Editor-Tools aus **MCP-PARITY-A** **21**, mit den fünf pfadbasierten
Tools aus **MCP-PARITY-B** (2026-09-26) sind es **26**. Verträge,
dokumentierte Grenzen und die **offene** CLI-Lücke (**6** von 32
Subcommands: `previous`, `merge-hdr`, `merge-pano`, `matrix`, `denoise`,
`cull`): Abschnitt „Erweiterter MVP-Scope“.
**LRPAR-G15-IPTC-S7 (2026-09-04, umgesetzt, BESTANDEN):** 5 zusätzliche
pfadbasierte Metadaten-Tools (`lumina_get_metadata_draft`,
`lumina_update_metadata_draft`, `lumina_apply_meta_preset`,
`lumina_batch_sync_metadata`, `lumina_trigger_export`, s. Abschnitt
„Metadaten-Schnittstelle“) — Gesamtstand nach **MCP-PARITY-A** und
**MCP-PARITY-B**: **26 Tools** (26 registrierte Tools, davon 24 CLI-Subcommands abgedeckt — die Zählung ist nicht dieselbe Zahl).
Review-Verfeinerungen 2026-08-25: strenge serverseitige Parameter-Bounds
(`quality` 1..=100, `max_width` ≥ 1 — kein truncierender Cast), atomarer
Export/Preview-Write mit Extension/Format-Gate, CAS-gesichertes
Zurückschreiben (`save_sidecar_if_unchanged` + Quell-Identitätsprüfung bei
`lumina_load`; Konflikt beim Write-Through von `lumina_edit` → neuer Fehler
`SidecarConflict`/-32010 statt Lost Update) und korrekte JSON-RPC-Codetrennung
(-32700 Parse / -32600 Invalid Request / -32602 Unknown Tool; Tool-Fehler als
`isError: true`-Result statt Transportfehler).
Review-Verfeinerungen R2 (2026-08-26): `lumina_save` schreibt über denselben
nicht-destruktiven Guard wie die Bulk-Tools (`write_output_guarded`) — Ziele,
die auf Quelle oder `.lumina.*`-Bundle auflösen (inkl. Symlink- und Hardlink-
Aliase), werden laut verweigert. Das Shutdown-Cleanup der Schnellvorschauen ist
implementiert (session-geführte Liste, Default: löschen;
`LUMINA_MCP_KEEP_PREVIEWS` opt-out). `lumina_load` dekodiert die bereits
gelesenen Bytes direkt (`decode_bytes`, kein zweiter Disk-Read).
Präzisierung (R2-MCP-03): `SidecarConflict` (-32010) entsteht ausschließlich
in `lumina_edit` — `lumina_save` hat keinen Sidecar-Write-Pfad und kann
diesen Fehler daher nicht auslösen.
Extension-/Identity-Unifikation (R2-CLI-02, 2026-08-26): `lumina-mcp` trifft
alle Extension-Entscheidungen über die Single-Source
`lumina_raw::RAW_EXTENSIONS`/`is_raw_extension`; die privaten Kopien (18er
RAW-Liste und die gedriftete 9er-Batch-Liste) sind entfernt. `lumina_batch`
sammelt damit alle 18 RAW-Formate statt vorher still nur 9 (Drift-Guard- und
E2E-Test analog dem CLI). Der `SourceIdentity`-Bau
(`build_source_identity`) entspricht exakt der CLI-`source_identity`-
Semantik: Fehlender Dateiname und `fs::metadata`-Fehler brechen laut ab
(`InvalidParams` bzw. `FileNotFound`) — der bisherige stille
`bytes.len()`-Fallback ist entfernt.
**MVP-Erklärung:** letzter vor MVP offener Punkt ist geschlossen
(Präzisierung 2026-09-04: neuer MVP-Scope **LRPAR-G15-IPTC** — Metadaten-
Tools + `metadata://`-Resource — ist als SOLL im Abschnitt „Metadaten-
Schnittstelle“ dokumentiert, Umsetzung über LRPAR-G15-IPTC-S7 in
`Agents.todo.md`; bis dahin nicht implementiert)

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [MCP-Protokoll und Transport](#mcp-protokoll-und-transport)
- [Tool-Set](#tool-set)
- [Schnellvorschau](#schnellvorschau-lumina_preview)
- [Architektur](#architektur)
- [Architekturgrenzen](#architekturgrenzen)
- [Nicht-Ziele](#nicht-ziele)
- [Erweiterter MVP-Scope](#erweiterter-mvp-scope-2026-08-19-user-anforderung-f-101-f1-umgesetzt-2026-08-26)
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
| `LUMINA_MCP_KEEP_PREVIEWS` | nicht gesetzt | Auf `1`/`true`/`yes` gesetzt: Vorschau-Dateien bleiben nach dem Shutdown bestehen (Opt-out des Standard-Cleanups, siehe „Schnellvorschau“) |

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
write-through. **`lumina_edit` rendert nicht** (kein Render-Pfad im
Tool); Masken sind hier gegenstaendlich — die Render-Divergenz
(Abschnitt „Masken im MCP-Renderpfad") wird erst beim naechsten
renderfaehigen Aufruf sichtbar.

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
    "wb_tint": 0.05,
    "vibrance": 0.1,
    "saturation": -0.2
  }
}
```
`vibrance` und `saturation` sind optionale Adjustments und folgen demselben
`-1..=1`-Bereich wie die Tonwert-Regler.

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
- `vibrance` und `saturation` sind unterstützte Adjustments (F-092,
  Render-Pipeline wendet Vibrance vor globaler Sättigung an) und ebenfalls auf
  `-1..=1` begrenzt; sie sind im JSON-Schema (schema-`properties` von
  `lumina_edit`) ausgewiesen und werden vom Validator mit demselben Bereich
  abgelehnt (kein stiller Clip). Bisher waren sie nur im Validator erlaubt,
  aber im Schema und hier nicht dokumentiert (R2-MCP-09); die Pipeline
  unterstützt sie bereits.
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
- **`masks: none`** — der Export rendert ohne Masken; die CLI wendet
  dieselbe Konfiguration mit Policy `warn` an. Begruendung und Default:
  Abschnitt „Masken im MCP-Renderpfad".
- `format` akzeptiert `"png"`, `"jpeg"`, `"webp"`.
- `quality` ist optional (Default 90), nur für JPEG/WebP relevant
  (`1..=100`).
- Der Export nutzt den gemeinsamen Render-Cache (im Gegensatz zur
  Schnellvorschau).
- Nicht-destruktiv (Review R2, 2026-08-26): Ein `output_path`, der auf die
  Quelle oder deren Lumina-Bundle (`<quelle>.lumina.json`,
  `<quelle>.lumina.zdata`) auflöst — einschließlich Symlink- und
  Hardlink-Aliase, die die Extension-Gate allein nicht erkennen kann — wird
  vor dem Schreiben laut mit `EncodeError` verweigert (`write_output_guarded`,
  derselbe Guard wie bei `lumina_batch`/`lumina_dust_removal`). Das
  Bundle bleibt byte-identisch intakt.
- `lumina_save` rendert/exportiert ausschließlich; er schreibt **keinen**
  Sidecar. Daher kann hier kein `SidecarConflict` (-32010) auftreten.
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
  auf `max_width` herunterskaliert (bilineares Downscaling). Ein
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
- **`masks: none`** — die Vorschau rendert ohne Masken, auch wenn der
  Sidecar eine gueltige Maske traegt; die CLI rendert dieselbe Eingabe
  mit Policy `warn` und wendet die Maske an. Begruendung, Default und
  die beiden Divergenz-Tests: Abschnitt „Masken im MCP-Renderpfad".
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
5. **Cleanup:** Implementiert (Review R2, 2026-08-26): Der Server führt
   eine session-geführte Liste aller in dieser Sitzung geschriebenen
   Vorschau-Dateien und löscht sie am Ende des Stdio-Loops (Shutdown/EOF).
   Default: ja (löschen). Mit `LUMINA_MCP_KEEP_PREVIEWS=1` (oder
   `true`/`yes`) bleibt die Löschung aus. Fehlschläge beim Löschen werden
   als Warnung geloggt und brechen das Herunterfahren nie ab.
6. **Verzeichnis-Anlage:** Schlägt das Anlegen von `preview_dir` beim Start
   fehl (z. B. fehlende Rechte), wird das als Warnung geloggt (kein stiller
   Fehlstart); der spätere `lumina_preview` meldet den Fehler laut.

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

Fehler werden als MCP-Error-Response zurückgegeben. Codetrennung seit
2026-08-25: Parse-Fehler → `-32700` (id `null`), strukturell ungültige
Requests → `-32600` (id-Echo wenn erkennbar), unbekanntes Tool →
`-32602`; Fehler **registrierter** Tools werden als normales Result mit
`isError: true` + `structuredContent.error` gemeldet (MCP-Spec),
nicht als Transportfehler. Seitdem auch neu: `SidecarConflict`
(`-32010`) bei CAS-Konflikt in `lumina_edit` — dem einzigen Tool mit
Sidecar-Write-Through. `lumina_save` rendert/exportiert nur und schreibt
keinen Sidecar, daher kann `-32010` dort nicht auftreten (R2-MCP-03).

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

## Masken im MCP-Renderpfad: `masks: none` (dokumentierte Divergenz)

**Stand (code-verifiziert 2026-09-26):** Jeder MCP-Renderpfad konstruiert
`RenderContext` mit `masks: None` — `crates/lumina-mcp/src/util.rs:308,382,404,627`
sowie `crates/lumina-mcp/src/tools/dust_removal.rs:232`; der Generativ-Pfad
baut denselben Kontext in `crates/lumina-stages/src/generative_artifact.rs:266`.
`RenderContext` hat **kein** `mask_policy`-Feld; die Policy liegt in
`MaskContext.policy` und kann nur gesetzt werden, wenn Masken überhaupt
aufgelöst werden. Es gibt also keinen Policy-Parameter, den ein Tool
exponieren könnte — ein `mask_policy`-Feld wäre ein Feld ohne Wirkung und
eine eigene Lücke.

**Begründung (F-101-MVP-Grenze):** Die Masken-Entscheidungsschicht
(F-048/F-051: Plane-Aufloesung aus dem `.lumina.zdata`-Bundle,
Gueltigkeitspruefung, Re-Inferenz, `warn`/`strict`-Policy) ist CLI-seitig
(und GUI-seitig). Der MCP-Server loest Masken-Ebenen **nicht** auf: Kein
Tool laedt Persistenz-Ebenen, kein Tool ruft die Entscheidungsschicht.
Masken-Status bleibt ueber `lumina_inspect` sichtbar (obiger Abschnitt).

**Je renderfaehigem Tool — `masks: none`, keine Ausnahme:**

| Tool | Render-Pfad | Masken |
| --- | --- | --- |
| `lumina_preview` | `render_copy` → `render_recipe` | `masks: none` |
| `lumina_save` | `render_copy` → `render_recipe` | `masks: none` |
| `lumina_analyze` | `render_copy` → `render_recipe` | `masks: none` |
| `lumina_batch` | `render_recipe` | `masks: none` |
| `lumina_trigger_export` | `render_recipe` | `masks: none` |
| `lumina_dust_removal` (`render_out`) | `render_frame` | `masks: none` |
| `lumina_generative` | `render_frame_with_generative` (geteilte `lumina-stages`-Schicht) | `masks: none` |

**Ausnahmen, benannt damit sie nicht als stillschweigende Divergenz
gelesen werden:**

- `lumina_regenerate op="matching"` wendet Masken **an** — mit Policy
  `warn`, ueber die geteilte `lumina-stages`-Schicht
  (`crates/lumina-stages/src/regenerate.rs:376-392`). Das ist **Paritaet**
  mit der CLI, kein Divergenzfall; der Renderpfad ist gemeinsam
  (`RenderContext` inkl. Masken-Ebenen baut die geteilte Schicht, nicht
  der Adapter — siehe MCP-PARITY-B-Abschnitt).
- `lumina_edit` rendert **nicht** (Write-through nur); Masken sind hier
  gegenstaendlich. Die Divergenz wird erst beim naechsten renderfaehigen
  Aufruf sichtbar.

**Dokumentierter Default fuer den Tag, an dem Masken dazukommen**
(Folge-Task `MCP-MASK-APPLY`, echte Feature-Arbeit): Sobald der
MCP-Renderpfad Masken anwendet, ist der wirksame Policy-Default **`warn`**
— identisch zur harmonisierten CLI-Default (warn-and-continue, `strict`
nur auf explizite Anfrage). Kein stiller Default-Wechsel, kein
`mask_policy`-Tool-Feld in diesem Slice.

**Divergenz gepinnt (Tests):** `crates/lumina-mcp/tests/mask_policy.rs`
belegt ueber den echten MCP-stdio-Server, dass `lumina_preview` mit einer
gueltigen Maske im Sidecar **ohne** Masken rendert (byte-identisch zum
`masks: None`-Oracle); `crates/lumina-cli/tests/mask_policy_divergence.rs`
belegt, dass die CLI dieselbe Maske **anwendet** (byte-identisch zum
`warn`-Masken-Oracle, verschieden vom No-Mask-Oracle). Die Divergenz ist
damit nachweisbar, nicht nur behauptet.

## Nicht-Ziele (Pre-MVP)

- Kein HTTP/WebSocket-Transport — stdio reicht für Agent-in-Terminal.
- Keine Multi-Image-Parallelverarbeitung.
- Keine AI-Masken-Inferenz über MCP.
- ~~Keine Preset-Verwaltung über MCP (kein `lumina_apply_preset`)~~
  **LRPAR-G15-IPTC (SOLL, 2026-09-04): aufgehoben** für Meta-Presets
  (`lumina_apply_meta_preset`, dynamisch mit `vars`); Rezept-Edit-Presets
  bleiben außerhalb des MCP-Scopes. Normativ: „Metadaten-Schnittstelle“ +
  `feature/product/iptc-metadata.md` §9.
- ~~Keine Batch-Befehle~~ **F-101-F1 (2026-08-26): aufgehoben** für
  `lumina_batch` mit dokumentierten Grenzen (sequenziell, ohne Resume);
  die Single-Image-*Session* bleibt weiterhin single-scoped.
- Keine Authentifizierung oder Zugriffssteuerung (lokaler Prozess).
- ~~Keine `resources` oder `prompts` MCP-Capabilities~~ **LRPAR-G15-IPTC
  (SOLL, 2026-09-04): teilweise aufgehoben** — die `resources`-Capability
  wird für die Read-only-Resource `metadata://draft/<urlencoded-pfad>`
  eingeführt (pfadbasiert, weil `image_id` prozess-lokal ist); `prompts`
  bleiben nicht implementiert. Normativ: „Metadaten-Schnittstelle“ +
  `feature/product/iptc-metadata.md` §9.
- Keine Virtual-Copy-Erstellung oder Löschung über MCP (nur Lesen
  und Rezept-Bearbeitung).

## Erweiterter MVP-Scope (2026-08-19 User-Anforderung; F-101-F1 umgesetzt 2026-08-26)

Der MCP-Server deckt den session- und pfadbasierten **Kern** der
CLI-Funktionalitäten ab, nicht die vollständige Befehlsliste. Der
ursprüngliche Scope lautete „alle CLI-Funktionalitäten"; diese Zusage war
**zu weit gefasst** und wird hier auf den geprüften Stand
zurückgenommen (siehe „Ist-Abdeckung" unten).

### CLI-Abdeckung — Ist-Stand und offene Lücke (gezählt 2026-09-26)

`lumina-cli` hat **32** Subcommands. Abgedeckt sind **26**, offen sind
**6**. Der Abgleich erfolgte gegen die tatsächlich implementierten
Tool-Beschreibungen, nicht nur gegen diese Tabelle. Zählstand
2026-09-26: 17 abgedeckt / 15 offen vor MCP-PARITY-A, 21 / 11 nach A
(dort mit dem unten genannten `previous`-Zählfehler), **26 / 6 nach
MCP-PARITY-B**.

**Abgedeckt (26):**

(Die Tabelle hat 25 Datenzeilen: `lumina develop` / `lumina render` /
`lumina export` teilen sich eine Zeile, und die letzte Zeile
(`lumina mcp`) ist der Transport-Einstiegspunkt des Servers, nicht Teil
der 32 abgeglichenen Subcommands. 26 + 6 = 32.)

| CLI-Befehl | MCP-Tool | Status |
| --- | --- | --- |
| `lumina import` | `lumina_import` | F-101-F1 |
| `lumina develop` / `lumina render` / `lumina export` | `lumina_edit` + `lumina_save` (Render/Export-Choke-Point) | bereits spezifiziert |
| `lumina process` | `lumina_edit` + `lumina_save` (Rezept vor Render setzen) | bereits spezifiziert |
| `lumina inspect` | `lumina_inspect` | bereits spezifiziert |
| `lumina batch` | `lumina_batch` (ein Aufruf = ein Verzeichnis) | F-101-F1 |
| `lumina reindex` | `lumina_reindex` | F-101-F1 |
| `lumina dust-removal` | `lumina_dust_removal` | F-101-F1 |
| `lumina mask` | Maskenstatus via `lumina_inspect` sichtbar; Re-Refresh über Sidecar-Rezept (`lumina_get_recipe`); Inferenz selbst bewusst Post-MVP | Grenze dokumentiert |
| `lumina validate` | `lumina_import`/`lumina_load` (Identitäts- und Validierungsprüfung beim Laden) | bereits spezifiziert |
| `lumina meta` (Keywords, Presets, Export, Sync) | `lumina_get_metadata_draft`, `lumina_update_metadata_draft`, `lumina_apply_meta_preset`, `lumina_batch_sync_metadata`, `lumina_trigger_export` | LRPAR-G15-IPTC-S7 |
| `lumina color` | Rezeptfelder via `lumina_edit`/`lumina_get_recipe` | bereits spezifiziert |
| `lumina red-eye` | Rezeptfelder via `lumina_edit`/`lumina_get_recipe` | bereits spezifiziert |
| `lumina face` | Rezeptfelder via `lumina_edit`/`lumina_get_recipe` | bereits spezifiziert |
| `lumina spot` | `lumina_spot` (Leseweg `op=list`/`op=detect`, Schreibwege je ein Feld) | MCP-PARITY-A |
| `lumina lens-blur` | `lumina_lens_blur` (Leseweg `op=list`, Schreibwege je ein Feld) | MCP-PARITY-A |
| `lumina geometry` | `lumina_geometry` (Leseweg `op=list`, Schreibwege je ein Feld/Teilstufe) | MCP-PARITY-A |
| `lumina upright` | `lumina_upright` (Leseweg `op=list`, Schreibwege `analyze`/`enable`/`disable`/`clear`) | MCP-PARITY-A |
| `lumina collections` | `lumina_collections` (Leseweg `op=list`, Schreibwege `add`/`remove` je eine Membership) | MCP-PARITY-B |
| `lumina smart-collections` | `lumina_smart_collections` (nur `op=evaluate`, read-only) | MCP-PARITY-B |
| `lumina relocate` | `lumina_relocate` (`op=move`, Kollisions- und Sidecar-Move abgedeckt) | MCP-PARITY-B |
| `lumina generative` | `lumina_generative` (Leseweg `op=status`, Schreibwege `generate`/`remove`) | MCP-PARITY-B |
| `lumina regenerate` | `lumina_regenerate` (`op=all` lesend auf einem frischen Dokument, Schreibwege `masks`/`auto_tone`/`matching`/`all`) | MCP-PARITY-B |
| `lumina mcp` | CLI-Subcommand, startet den Server (kein Tool) | F-101-F1 |

**Offen — kein MCP-Weg (8):** **`previous`**, `batch-meta`, `keywords`,
`merge-hdr`, `merge-pano`, `matrix`, `denoise`, `cull`.

**Korrektur 2026-09-26 (MCP-PARITY-B-Verifikation):** `lumina batch-meta` und
`lumina keywords` waren hier kurz als abgedeckt geführt. Das ist **falsch**:
`lumina_update_metadata_draft` hat keinen `BatchOp`-Parameter, **weist `keywords`
explizit zurück** (mit Routing-Hinweis auf das Dokumentfeld) und antwortet einem
unbekannten `batch_op` mit `{"ok":true,"rev":0}` — ein erfolgreicher stiller
No-Op, während das CLI-Kommando selbst funktioniert. Ein `{ok:true}` auf einen
unbekannten Operations-Parameter ist zudem **kein** lautes Verweigern. Die
ehrliche Zählung ist **24 abgedeckt / 8 offen**.

**Korrektur 2026-09-26 (Zählung):** die Kopfzahl „22 abgedeckt / 10 offen"
war **falsch** und ist hiermit berichtigt. `lumina previous` stand in keiner
der beiden Listen und wurde für abgedeckt gehalten, weil das Wort „previous" in
einer Tool-Beschreibung vorkommt. Das war ein **Stichwort-False-Positive**: es
gibt kein MCP-Tool dafür, das Kommando wird in `mcp-server.md` **null Mal**
erwähnt, und es ist ein **rezeptschreibendes** Kommando, nicht nur lesend. Die
Offen-Liste nannte daraufhin 11 Einträge bei einer Kopfzahl von 10. Die
Arithmetik ist: **32 − 8 = 24 abgedeckt**, und `batch-meta` plus
`keywords` sind über die Metadaten-Tools abgedeckt (oben erstmals als Zeilen
geführt).

Diese Lücke ist **offen, nicht abgesichert**: die früher hier
formulierte Zusage „Jeder `lumina`-CLI-Befehl ist als MCP-Tool
erreichbar" trifft auf die obigen 6 nicht zu und wird nicht mehr
behauptet. Die Schliessung erfolgt in benannten Slices (Aufgabe
`MCP-PARITY-C` in `Agents.todo.md`), **nicht** durch einen einzigen
Blindumbau: `denoise` und `cull` sind über das F-078-Modellgate
gesperrt — ein Tool dafür würde immer nur den Modellfehler
durchreichen und wird deshalb als **modellgegated** geführt statt als
Stub gebaut.

**Zusätzlich offen und *nicht* durch MCP-PARITY-B geschlossen (ehrlich
benannt):** `lumina spot --regenerate-variant` (der generative Variant-Seed) hat
weiterhin **keinen** MCP-Weg. Die frühere Formulierung dieses Abschnitts
verwies dafür auf MCP-PARITY-B; dieser Slice schliesst die fünf
**Subcommands**, nicht dieses eine Flag. `lumina_spot` meldet es weiterhin
bewusst nicht, damit kein halb abgedeckter generativer Pfad entsteht.

**Namenskorrektur:** die frühere Fassung dieser Tabelle nannte
`lumina info`; das CLI-Kommando heisst `lumina inspect`. Es gibt kein
`lumina info`.

#### Gemeinsame Eigenschaften der Bulk-/Pfad-Tools

`lumina_import`, `lumina_batch`, `lumina_reindex` und
`lumina_dust_removal` sind **pfadbasiert** (`input`-Pfad statt
`image_id`): sie verändern die Single-Image-Session nicht und laufen
neben einem geladenen Bild. Sie sind reine Orchestrierung über dieselben
geprüften Bausteine wie die Session-Tools (Decode, Seitenidentität,
`render_frame`, atomare Writes) — keine zweite Pipeline.

#### `lumina_import`

Spiegelt `lumina import`: stellt sicher, dass ein gültiger Standardsidecar
existiert, ohne das Bild in die Session zu laden.

- **Input:** `{ "path": string }`.
- **Verhalten:** Datei lesen und decodieren (RAW/Raster). Existiert der
  Sidecar, wird er geladen und gegen die aktuelle Quell-Identität
  (Content-Hash, Byte-Länge) geprüft — Abweichung ist ein lauter
  `SidecarError` („source changed“), niemals still verwendend. Existiert
  er nicht, wird ein leerer Standardsidecar per Compare-and-Swap
  materialisiert (konkurrierende Erstellung wird adoptiert).
- **Output:** `{ ok: true, input, sidecar, status: "created"|"validated" }`.
- Fehler: `FileNotFound`, `UnsupportedFormat`, `DecodeError`,
  `SidecarError`.

#### `lumina_batch`

Ein Aufruf = ein Verzeichnis. Rendert jede Bilddatei des Verzeichnisses
(rekursiv, deterministische Reihenfolge) mit dem aktiven Rezept ihres
Sidecars über denselben Choke-Point wie `lumina_save` und schreibt
atomar nach `<output>/<dateiname>.<format-extension>`.
- **`masks: none`** — wie `lumina_save` rendert Batch ohne Masken; die
  CLI wendet Masken mit Policy `warn` an. Abschnitt „Masken im
  MCP-Renderpfad".

- **Input:** `{ input, output, format?, quality?, virtual_copy? }`
  (`format` Default `"png"`; `quality` Default 90, nur JPEG/WebP;
  `virtual_copy` wählt die Kopie, Default = Standardkopie).
- **Kollisionsschutz:** Wie REVIEW-CLI-BATCHCOLLIDE-1 wird der Lauf
  abgelehnt, bevor etwas geschrieben wird, wenn zwei Inputs auf
  denselben Ziel-Dateinamen mappen.
- **Nicht-destruktiv:** Ein Output, der auf Quelle oder deren
  `.lumina.*`-Bundle auflöst, wird pro Item verweigert.
- **Output:** `{ status: "ok"|"failed", succeeded, failed, results:
  [{ input, status, output?, error? }] }`. Der Aufrufer MUSS auf
  `status` verzweigen — Einzel-Fehlschläge machen das Ergebnis nie
  still erfolgreich.
- **Dokumentierte Grenzen gegenüber dem CLI-Batch:** sequenziell (kein
  `--jobs`-Pool), keine `*.status.json`-Resume-Marker, keine
  One-Shot-Flags `update_masks`/`force_render`, keine Presets/Auto-Tone
  (das sind `process`-Features, kein Batch-Bestandteil im MVP). Fehlt
  ein Sidecar, wird ein frisches Standard-Rezept (Defaultkopie ohne
  Adjustments) **im Speicher** verwendet — `lumina_batch` schreibt
  niemals Sidecars; wer Sidecars materialisieren will, ruft vorher
  `lumina_import`. Diese Einschränkungen sind bewusst und keine
  stillen Abweichungen; Resume/Parallelität bleibt Post-MVP.

#### `lumina_reindex`

Wrappt exakt das heutige CLI-Verhalten: rekursiver Scan nach
`*.lumina.json` mit Validierung jedes Dokuments.

- **Input:** `{ input }` (Verzeichnis).
- **WICHTIGE GRENZE:** Es gibt im Workspace **kein Index-Modul** —
  `reindex` ist und bleibt ein **Sidecar-Scan** (Sammeln, Laden,
  Validieren, Zählen). Er baut keinen Katalog, schreibt nichts und
  erzeugt keinen Cache. Ein echter Index-Adapter ist Post-MVP und muss
  aus Sidecars vollständig rekonstruierbar sein.
- **Output:** `{ input, sidecars, invalid, errors: [...], status:
  "ok"|"invalid-sidecars" }`. Korrupte Sidecars werden einzeln mit
  Pfad+Fehler gemeldet (niemals still ignoriert); `status` spiegelt den
  CLI-Exitcode wider.

#### `lumina_dust_removal`

Wrappt die existierende F-042-N1-Funktionalität (`lumina dust-removal`):
persistiert eine Reparatur-Region in das `.lumina.zdata`-Bundle und
trägt sie als Source-Action in das Rezept einer virtuellen Kopie ein.
Das Original bleibt unverändert.

- **Input:** `{ input, repair_region: { id, kind?,
  region_width, region_height, region_values (u16),
  replacement_path }, virtual_copy?, render_out? }`
  (`kind` Default `dust-removal`, alternativ `ai-replacement`;
  Pixel `>= 32768` der Region werden durch das Replacement ersetzt).
- **Ordnungsregel (REVIEW-CLI-N2, identisch zum CLI):** Sidecar laden,
  virtuelle Kopie auflösen und Region/Replacement vollständig
  validieren (Region-Dimensionen = Source-Frame, Replacement =
  Region) **bevor** etwas ins Bundle angehängt wird. Erst dann
  `append_repair_region`, Source-Action-Spec anfügen, Sidecar
  validieren und atomar speichern.
- **Optional `render_out`:** verifizierender Headless-Render mit
  angewandter Source-Action (Auflösung aus dem Bundle inklusive
  Prüfsummenkontrolle — fehlendes Artefakt oder Prüfsummen-Divergenz
  ist ein lauter Fehler). `render_out` darf niemals auf die Quelle
  oder deren Bundle-Dateien auflösen. Dieser Render laeuft mit
  **`masks: none`** (Abschnitt „Masken im MCP-Renderpfad"): die
  verifizierende Darstellung zeigt die Source-Action, nicht den
  Masken-Stand des Sidecars.
- **Output:** `{ ok: true, input, virtual_copy, artifact_id, bundle,
  checksum }`.
- Fehler: `FileNotFound`, `UnsupportedFormat`, `DecodeError`,
  `InvalidParams` (Dimensions-/Werte-Verstöße), `UnknownCopy`,
  `SidecarError` (fehlender Sidecar → „run lumina_import first“),
  `EncodeError`.

### Rezept-Stufen-Editoren (MCP-PARITY-A — umgesetzt)

Vier **session-basierte** Tools für die session-/rezeptnahen Stufen-Editoren.
Sie sind **keine** zweite Implementierung: `lumina-cli` und `lumina-mcp` rufen
dieselbe Funktion in `crates/lumina-stages` auf (`crates/lumina-cli/src/stages.rs`
ist der reine clap-Adapter, `crates/lumina-mcp/src/tools/` der reine
JSON-Schema-Adapter). Ein MCP-Aufruf und der äquivalente CLI-Aufruf hinterlassen
deshalb **byte-identische** Sidecars — je Stage-Editor, in lesender und
schreibender Richtung, belegt in `crates/lumina-cli/tests/stage_parity*.rs`
(dort steht das echte `lumina-cli`-Binary über `CARGO_BIN_EXE_lumina-cli` zur
Verfügung). Beide Adapter bauen denselben `*Request` und rendern denselben
`StageReport`: das CLI-`--json`-Dokument ist wortgleich das Tool-Payload.

| Tool | CLI | Leseweg | Schreibwege |
| --- | --- | --- | --- |
| `lumina_spot` | `lumina spot` | `op=list`, `op=detect` | `add`, `update`, `remove`, `set_visualize`, `set_distraction`, `detect_apply`, `clear` |
| `lumina_lens_blur` | `lumina lens-blur` | `op=list` | `enable`, `disable`, `set_amount`, `set_focal_near`, `set_focal_far`, `set_bokeh`, `set_focus_rect`, `set_depth_artifact`, `clear_depth_artifact`, `clear` |
| `lumina_geometry` | `lumina geometry` | `op=list` | `set_crop_aspect`, `set_crop_free`, `clear_crop`, `set_rotation`, `set_mirror`, `clear_geometry`, `set_lens_profile`, `set_lens_field`, `clear_lens`, `set_perspective_field`, `clear_perspective` |
| `lumina_upright` | `lumina upright` | `op=list` | `analyze`, `enable`, `disable`, `clear` |

**Gemeinsame Eigenschaften (Session-Tools):**

- **Input:** `{ image_id (Pflicht), virtual_copy?, op (Pflicht), …op-spezifische
  Felder }`. Das Schema ist `additionalProperties: false`; ein unbekanntes Feld
  wird **laut** mit `InvalidParams` (`-32602`) abgewiesen, genau wie ein
  unbekanntes `op`.
- **Read und Write sind getrennt.** Der Leseweg (`list`, und `detect` bei
  `lumina_spot`) schreibt **nie**; jeder Schreibweg benennt genau ein Feld oder
  genau eine Operation. Ein `op` ohne seinen Wert ist ein lauter Fehler, kein
  stiller No-op. Es gibt **kein** implizites Setzen ganzer Stufen.
- **Laut wie die CLI.** Unbekannte Kopie, unbekanntes Feld, unbekannter
  Spot/Preset/Spiegel/Lens-/Perspektiv-Feldname, Out-of-Range, invertierte
  Focal-Range, nicht-portabler (absoluter) Depth-Artifact-Pfad,
  fehlendes Tiefen-Artefakt, `upright --enable` ohne persistierte Analyse und
  `spot` ohne Sidecar brechen **laut** ab und ändern **keine** Bytes. Es gibt
  keinen stillen Fallback und kein „leeres Ergebnis als Erfolg".
- **Output:** der gemeinsame Stufen-Payload (`command`, `copy`, Stufenzustand,
  `actions`) plus `saved` (bei Lesewegen `false`), `action` (die Aktionen
  genau dieses Aufrufs) und `revision`.
- **Session-Semantik wie `lumina_edit`.** Der Sidecar wird pro Aufruf frisch von
  der Platte gelesen (ein extern geänderter Sidecar wird gesehen, nicht
  gecacht); der Schreibpfad läuft als Compare-and-Swap gegen die in
  `lumina_load` gesehene Revision und meldet einen Konflikt als
  `SidecarConflict` (`-32010`) statt still zu überschreiben. Danach wird die
  Session wie bei `lumina_edit` auf `document` + `sidecar_revision` rebasiert —
  mehr Session-Zustand wird nicht erfunden.
- **Historische Nicht-Reproduzierbarkeit (ehrlich benannt):** `lumina_geometry`
  und `lumina_upright` hängen pro Aufruf genau einen History-Eintrag an, dessen
  `id` und `recorded_at` eine **Millisekunde** sind (`geometry-<ms>` /
  `upright-<ms>`, bestehendes CLI-Verhalten). Zwei *Prozesse* können ihre
  Sidecars deshalb nicht bitweise vergleichen; der Paritätstest maskiert genau
  dieses Feld und schlägt bei jeder anderen Abweichung fehl. `spot` und
  `lens-blur` hängen keinen History-Eintrag an — dort ist der Vergleich ein
  echter Byte-Vergleich.

**Bewusst nicht abgedeckt (keine Stubs):**

- `spot --regenerate-variant` (generativer Variant-Seed) ist auch nach
  **MCP-PARITY-B** nicht abgedeckt. Der Verweis auf Slice B in der ersten
  Fassung war eine Zusage, die dieser Slice **nicht** einlöst: er schliesst die
  fünf Subcommands `collections`/`smart-collections`/`relocate`/`generative`/
  `regenerate`, nicht dieses eine Flag. Ein halb abgedeckter generativer Pfad
  wäre ein Stub, keine Parität — der ehrliche Stand ist also die offene Lücke
  (siehe „Pfadbasierte und Artefakt-Kommandos" unten).
- `geometry --lensfun-status` (Read-only-Lensfun-Auto-Profil-Auflösung) braucht
  die native `lensfun`-Capability, die `lumina-mcp` nicht linkt. Das Feld
  meldet `null` — exakt wie `lumina geometry --json` ohne `--lensfun-status`;
  eine hart kodierte „unavailable“-Zeichenkette wäre ein stiller Fallback.
- `--straighten` ist im CLI nur ein Alias von `--set-rotation` (gleiches Feld,
  gleiche Validierung); das MCP-Tool hat dafür nur `op=set_rotation`.
- `denoise` und `cull` bleiben hinter dem F-078-Modellgate — unberührt,
  **nicht** gestubt. `generative` und `regenerate` sind mit **MCP-PARITY-B**
  geschlossen (siehe unten).

Abnahme: Byte-Identität MCP-gegen-CLI je Stage in beiden Richtungen, ein
Still-Fehler-Test je Stage (unbekannte Kopie/Feld/ID, Out-of-Range, invertierte
Focal-Range, fehlendes Depth-Artefakt), ein Registry-Test über
`list_tool_definitions` **und** `dispatch_tool` **und** `is_known_tool`, Schema-
Tests (unbekanntes Feld abgewiesen, Pflichtfelder erzwungen) und
`cargo test -p lumina-mcp --all-targets`.

### Pfadbasierte und Artefakt-Kommandos (MCP-PARITY-B — umgesetzt)

Fünf **pfadbasierte**/`Artefakt`-Tools für die fünf Subcommands, die nach
MCP-PARITY-A noch ohne MCP-Weg waren. Sie sind **keine** zweite
Implementierung: `lumina-cli` und `lumina-mcp` rufen dieselbe Funktion in
`crates/lumina-stages` auf (`crates/lumina-cli/src/library.rs` ist der reine
clap-Adapter, `crates/lumina-mcp/src/tools/` der reine JSON-Schema-Adapter).
Ein MCP-Aufruf hinterlässt deshalb **byte-identische** Sidecars wie der
äquivalente CLI-Aufruf — je Kommando, in lesender **und** schreibender
Richtung, belegt in `crates/lumina-cli/tests/library_parity*.rs` (dort steht
das echte `lumina-cli`-Binary über `CARGO_BIN_EXE_lumina-cli` zur Verfügung).
Beide Adapter bauen denselben `*Request` und rendern denselben `BulkReport`:
das CLI-`--json`-Dokument ist wortgleich das Tool-Payload.

| Tool | CLI | Leseweg | Schreibwege |
| --- | --- | --- | --- |
| `lumina_collections` | `lumina collections` | `op=list` | `add` (eine `id=name`-Membership), `remove` (eine Id) |
| `lumina_smart_collections` | `lumina smart-collections` | `op=evaluate` | — (read-only per Vertrag) |
| `lumina_relocate` | `lumina relocate` | — | `move` (ein `from`/`to`-Paar) |
| `lumina_generative` | `lumina generative` | `op=status` | `generate`, `remove` |
| `lumina_regenerate` | `lumina regenerate` | `op=all` (auf frischem Dokument ein No-op) | `masks`, `auto_tone`, `matching`, `all` |

**Gemeinsame Eigenschaften (Pfad-Tools):**

- **Pfadbasiert, ein Aufruf = ein Pfad.** Adressiert wird über `path` (bzw.
  `from`/`to`), **nicht** über `image_id`: die Kommandos arbeiten auf der
  Bibliothek, nicht auf der geladenen Session, und laufen neben einem geladenen
  Bild, ohne dessen Zustand zu berühren. Das ist das Muster der
  F-101-F1-Bulk-Tools. `lumina_relocate` nimmt ein `from`/`to`-Paar,
  `lumina_smart_collections` zusätzlich den `catalog`-Pfad.
- **Atomar und derselbe Fehlerkanal wie die CLI.** Der Schreibpfad läuft über
  `lumina_sidecar::save_sidecar` mit `Persist::Immediately` — dieselbe Funktion
  wie in der CLI, nicht ein abweichender Agenten-Write. Kein Rezeptfeld wird
  erfunden, kein Fehler wird verschluckt.
- **Read und Write sind getrennt.** Jeder Leseweg (`list`, `evaluate`,
  `status`, `all` auf einem frischen Dokument) schreibt **nie**; jeder
  Schreibweg benennt genau eine Membership, genau ein `from`/`to`-Paar, genau
  eine Rolle bzw. genau ein Modul. Ein `op` ohne seinen Wert ist ein lauter
  `InvalidParams`, kein stiller No-op — getestet je Kommando.
- **Output:** der gemeinsame Kommando-Payload (wortgleich das CLI-`--json`)
  plus die MCP-Hülle `saved` (bei Lesewegen `false`), `action` (die Aktionen
  genau dieses Aufrufs) und `text` (die menschliche Zeile, die die CLI ohne
  `--json` druckt). Ein Kommando, das auf seinem Pfad kein JSON druckt
  (`generative op="remove"`), liefert trotzdem die Hülle — ein Agent bekommt
  nie ein leeres Objekt.
- **Dokumentierte Abweichung:** diese fünf Tools nehmen **nicht** am
  Compare-and-Swap der Session teil (`lumina_edit`,
  `lumina_update_metadata_draft`), weil sie Session-Tools nicht sind und ein
  Aufrufer einen Pfad adressieren kann, der nie geladen wurde. Eine
  zwischenzeitliche externe Änderung wird also wie in der CLI überschrieben
  und nicht als `SidecarConflict` gemeldet — identisch zu `lumina_dust_removal`.
  Wer den CAS-Vertrag braucht, nutzt `lumina_edit` auf einem geladenen Bild.
- **Lensfun:** `lumina-mcp` linkt die optionale native `lensfun`-Capability
  nicht. Die gemeinsame Schicht fragt den Corrector deshalb über einen Port
  (`CorrectorSource`) beim Aufrufer ab; MCP übergibt `NoCorrector`, was ein
  echtes „kein Profil passt" ist. Eine hart kodierte Korrektur oder ein
  Korrektur-String wäre ein stiller Fallback.

**Das Modell-/Artefakt-Gate (beide Artefakt-Kommandos bleiben dahinter):**

- `lumina_generative op="generate"` **verlangt eine aktive Rolle**
  (`expand` oder `auto_fill`) und bricht **laut** ab, wenn kein Canvas
  erzeugt werden kann: kein aktiver Role-Request, `expand` ohne `canvas`, ein
  `auto_fill` ohne transparente Pixel nach der Linsen-Stufe. In keinem Fall
  wird ein Datensatz geschrieben, ein Artefakt verlinkt oder ein Byte am
  Sidecar geändert.
- `lumina_generative op="status"` ist **kein Leer-Erfolg**: eine Rolle, deren
  Artefakt nicht `available`/`not-required` ist, wird **namentlich**
  gemeldet (`stale`/`missing`/`corrupt`) und der Aufruf **scheitert** danach —
  dieselbe Verdict-Zeile wie die CLI („no silent fallback; run `lumina
  generative --generate`"). Ein bewusst entlinktes Artefakt (`--remove`) wird
  **nie** aus dem weiterhin vorhandenen Bundle-Datensatz stillschweigend
  re-adoptiert.
- `lumina_regenerate op="matching"` leitet `matched_exposure` aus einem
  **echten Render** des aktuellen Rezepts ab — gerendert **ohne**
  generative Canvas-Eingabe, wie es `lumina regenerate` seit jeher tut. Eine
  Kopie mit **aktiver generativer Rolle** kann deshalb gar nicht gerendert
  werden: die Core-Stufe meldet `generative_artifact.*.missing`. Das Modul
  bricht dann **laut** ab und schreibt **keinen** Sidecar-Byte; es meldet
  niemals einen abgeleiteten Wert für ein Frame, das der Renderer verweigert
  hat. Ein ungültiges Rezept und ein korruptes `.lumina.zdata`-Bundle sind
  genauso laut.
- `lumina_regenerate op="masks"` ist eine **Refresh-Anforderung**, keine
  Inferenz: sie setzt die ausgewählten Quellmasken auf `Pending` (und bei
  explizitem Aufruf das kop-weite One-Shot-Flag). Es wird **nie** eine
  Stub-Matte als gültiges Artefakt persistiert (das ist der dokumentierte
  F-082-Offenpunkt).
- `lumina_relocate op="move"` deckt **Zielkollision** (Zielbild *und*
  Ziel-Companion) und **Sidecar-Move** ab: die Companions werden aus dem
  **Ziel**-Pfad abgeleitet und mitverschoben, damit ein Rename das Rezept am
  neuen Namen lässt. Fehlt die Quelle, ist das Ziel-Elternverzeichnis kein
  Verzeichnis oder schlägt ein Companion-Move fehl, ist alles laut — und der
  Fehlertext nennt den erreichten Schritt, damit kein stiller Halbzustand
  entsteht.

**Byte-Identität: gemessen, nicht behauptet.**

Alle fünf Kommandos sind **literal byte-gleich, ganz ohne Maske** — anders als
`geometry` und `upright` aus Slice A. Der Grund ist gemessen, nicht angenommen:
keines dieser fünf Kommandos hängt einen `history`-Eintrag an, also gelangt
kein Millisekunden-Stempel in den Sidecar. (`lumina render`/`process` tun das
— deshalb fährt kein Test dieses Abschnitts sie.) Der Paritätstest vergleicht
deshalb rohe Bytes; `mutation_a_non_stamp_byte_still_fails` beweist per Mutation,
dass **jede** andere Byte-Änderung weiterhin fehlschlägt, damit „byte-identisch"
eine Aussage und keine Hoffnung ist.

**Grenzen (bewusst nicht abgedeckt, keine Stubs):**

- `lumina spot --regenerate-variant` (generativer Variant-Seed) bleibt ohne
  MCP-Weg. Slice B schliesst die fünf Subcommands, nicht dieses eine Flag.
- `lumina generative --generate` nutzt — wie die CLI — den deterministischen
  **Fixture**-Produzenten aus `lumina-onnx`, nicht ein echtes
  `.onnx`-Artefakt. Ein echtes Modell ist weiterhin F-078; das Tool meldet nie
  ein Modell, das es nicht verwendet hat.
- `lumina_regenerate op="matching"` rendert über `lumina_core::render_frame`
  (die CPU-Referenz). Die CLI übergibt in einem Build **mit** `gpu` ihre
  GPU-Route; die beiden stimmen nur ohne diese optionale Capability überein —
  das ist die dokumentierte GPU/CPU-Parität, deren eigenes Gate
  (`GPU-PARITY-HW-28`) offen ist. Die Render-Eingabe (`RenderContext` inkl.
  Masken-Ebenen und Source-Actions) baut in beiden Fällen die geteilte
  Schicht, nicht der Adapter.
- `lumina_regenerate op="matching"` wendet **Masken an** (Policy `warn`,
  persistierte Ebenen aus dem Sidecar-Bundle) — ueber die geteilte
  `lumina-stages`-Schicht, also **Paritaet mit der CLI**, kein
  Divergenzfall (Abschnitt „Masken im MCP-Renderpfad").
- `lumina_generative op="generate"` rendert die Auto-Fill-/Expand-Ebene
  mit **`masks: none`** (geteilte Schicht
  `crates/lumina-stages/src/generative_artifact.rs:266`) — wie alle
  renderfaehigen MCP-Tools ausser `lumina_regenerate op="matching"`.

Abnahme: Byte-Identität MCP-gegen-CLI je Kommando in beiden Richtungen (roher
Byte-Vergleich, Mutationsnachweis), Kollisions- und Abbruch-Tests für
`relocate` (Zielbild, Ziel-Companion, fehlende Quelle, kein Verzeichnis als
Elternpfad), Gate-Tests für `generative`/`regenerate` inklusive „nicht
erfolgreich ohne produzierbares Canvas" und „nicht erfolgreich mit aktiver
generativer Rolle", ein Test je Write-op-ohne-Wert, ein Registry-Test über
`list_tool_definitions` **und** `dispatch_tool` **und** `is_known_tool`,
Schema-Tests (unbekanntes Feld abgewiesen, Pflichtfelder erzwungen,
`image_id` **nicht** erforderlich) und `cargo test -p lumina-mcp --all-targets`.

### Metadaten-Schnittstelle (LRPAR-G15-IPTC — umgesetzt, S7 BESTANDEN)

Der MCP-Server erhält fünf **pfadbasierte** Metadaten-Tools (Muster der
F-101-F1-Bulk-Tools: laufen neben der Single-Image-Session, mutieren sie
nicht) und eine Read-only-Resource. Normative Fachregeln:
`feature/product/iptc-metadata.md` §9; Entscheid:
`feature/decisions/LRPAR-G15-META-15.md`.

| Tool | Zweck | Bemerkung |
| --- | --- | --- |
| `lumina_get_metadata_draft` | `{ path }` → `{ path, embedded, draft, keywords, history_len, status }` | Embedded-Read nur JPEG IIM/XMP; sonst laut „nicht verfügbar“ |
| `lumina_update_metadata_draft` | `{ path, fields?, clear_fields? }` → `{ ok, rev }` | Write-Through per CAS; CAS-Konflikt → `SidecarConflict` (`-32010`, analog `lumina_edit`) |
| `lumina_apply_meta_preset` | `{ paths, preset, vars? }` → per-Pfad-Report | Statisch ohne `vars`; dynamisch verlangt alle Platzhalter-Variablen als JSON-Objekt — unaufgelöst/unbekannt = lauter Tool-Fehler |
| `lumina_batch_sync_metadata` | `{ source, targets, fields }` → Report | Feld-selektiv (`fields` Pflicht), pro Ziel CAS/atomar, Fehler pro Bild isoliert |
| `lumina_trigger_export` | `{ path, output_path, format, quality?, virtual_copy?, write_metadata: true }` | Wrappt denselben Choke-Point wie `lumina_save`; `write_metadata` + Nicht-JPEG = `InvalidParams` (laut); rendert mit **`masks: none`** (Abschnitt „Masken im MCP-Renderpfad") |

**Resource (Read-only):** `metadata://draft/<urlencoded-pfad>` — stabile
Adresse über den **Pfad**, nicht `image_id` (prozess-lokal, Server-Neustart →
IDs neu nummeriert). `resources/read` liefert dasselbe JSON wie
`lumina_get_metadata_draft`; `initialize` deklariert die `resources`-
Capability (`subscribe`/`listChanged` false). `prompts` bleiben nicht
implementiert.

Abnahme: Tool-Schemas in `tools/list`, Fehlerpfade (FileNotFound,
UnsupportedFormat, InvalidParams, SidecarError, `-32010`), Report-Semantik
`updated`/`unchanged`/`failed`, Capability-Deklaration — jeweils mit Tests
(`cargo test -p lumina-mcp`).

### Vision-fähiger Agent (Vorschau analysieren) — UMGESETZT

Zwei komplementäre Wege, den Bearbeitungszustand zu beurteilen:

1. **Visuell:** `lumina_preview` liefert den Pfad zur gerenderten
   Vorschau (PNG); ein vision-fähiger Agent analysiert das Bild direkt.
2. **Strukturiert (vision-frei):** `lumina_analyze` liefert als JSON:
   Luminanz-Histogramm (256 Bins) + RGB-Kanal-Histogramme, Per-Channel-
   Statistiken (Mittelwert, StdDev, Min, Max), dominante Farben
   (quantisiert, mit Häufigkeit) und einen Expositionsschätzwert
   (EV bis Mitttelgrau 0.5, Median-basiert, geclampt auf ±10 EV) —
   gerendert über denselben Choke-Point wie Preview/Save, also immer
   der aktuelle Rezeptstand, und — wie alle renderfaehigen MCP-Tools
   ausser `lumina_regenerate op="matching"` — mit **`masks: none`**
   (Abschnitt „Masken im MCP-Renderpfad"). Damit ist die SOLL-Anforderung
   (Histogramm, Farbstatistiken, Dominante Farben, Exposition)
   vollständig erfüllt; Auto-Tone-Vorschläge und Maskenstatus gehören
   bewusst NICHT zu `lumina_analyze` (Maskenstatus: `lumina_inspect`;
   Auto-Tone bleibt CLI-/Rezept-Feature).

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
  `lumina-raw` (optional, für Metadaten-Direktzugriff), `lumina-iptc`,
  `lumina-stages`, `serde`, `serde_json`, `blake3`, `log`.
- **`lumina-stages`** (MCP-PARITY-A): die EINE Implementierung der vier
  session-basierten Rezept-Stufen-Editoren. `lumina-cli` **und** `lumina-mcp`
  hängen davon ab. Das ist keine reine Umorganisation: `lumina-cli` hängt
  bereits an `lumina-mcp` (Feature `mcp`), Cargo lehnt den Paketzyklus
  `lumina-cli → lumina-mcp → lumina-cli` ab, also braucht es das dritte Crate —
  sonst wäre die Parität nur eine Behauptung statt Struktur.
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
- **Stufen-Editoren (MCP-PARITY-A):** je Stage ein Test, dass ein lauter
  Fehler **keine** Sidecar-Bytes ändert und dass der CLI dieselbe Eingabe mit
  Exit-Code 1 und derselben Fehlerzeile ablehnt (unbekannte Kopie, unbekanntes
  Feld, unbekannte ID, Out-of-Range, invertierte Focal-Range, fehlendes
  Depth-Artefakt) — `crates/lumina-cli/tests/stage_parity_errors.rs` und
  `stage_parity_geometry_upright.rs`.

### Paritätstests Stufen-Editoren (MCP-PARITY-A)

- **Byte-Identität:** `crates/lumina-cli/tests/stage_parity.rs` startet das
  echte `lumina-cli`-Binary gegen die eine Fixture und das MCP-Tool in-process
  gegen eine zweite, byte-identische Fixture; verglichen werden die
  Sidecar-Bytes **und** der gemeinsame Stufen-Payload — je Stage, in lesender
  und schreibender Richtung.
- **Registry:** `crates/lumina-mcp/tests/stage_tools.rs` prüft, dass jeder
  registrierte Name in `list_tool_definitions` steht, von `dispatch_tool`
  angenommen wird und mit `is_known_tool` übereinstimmt, indem es den
  vollständigen `tools/list`-Ergebnis über `tools/call` schickt.
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

- [ ] Alle zwölf MCP-Tools funktionieren: die sieben Editing-Tools
      (`lumina_load`, `lumina_edit`, `lumina_get_recipe`, `lumina_save`,
      `lumina_preview`, `lumina_list_virtual_copies`, `lumina_inspect`),
      `lumina_analyze` sowie die F-101-F1-CLI-Abdeckung (`lumina_import`,
      `lumina_batch`, `lumina_reindex`, `lumina_dust_removal`) und die
      dokumentierten Outputs liefern.
- [ ] `lumina_preview` erzeugt deterministische, cache-freie
      Vorschauen mit konfigurierbarer Breite.
- [ ] `lumina_edit` schreibt Sidecars atomar und idempotent.
- [ ] Der MCP-Server startet über `lumina mcp` (CLI-Subcommand, Feature
      `mcp`, Default an) oder `lumina-mcp` und beendet sich sauber.
- [ ] Der `initialize`-Handshake liefert korrekte MCP-Capabilities.
- [ ] Unit-, Integration- und Fehlerpfadtests bestehen.
- [ ] Clippy (`-D warnings`) und Formatprüfung laufen grün.
- [ ] Ein unabhängiger Verifizierungs-Agent bestätigt die Testabdeckung
      und Korrektheit.
- [ ] `feature/platform/mcp-server.md` ist das normative SOLL-Dokument
      und mit `feature/README.md` verlinkt.
