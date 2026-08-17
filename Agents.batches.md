# LuminaRust — Gestaffelte Batch-Ausführung

Strategie: gestaffelte Batches (Abhängigkeitsreihenfolge), innerhalb einer Stufe
parallel pro Crate. Jeder Subagent liest vorher `Agents.md`, `feature/README.md`
und das betroffene Feature-Dokument, implementiert nur die echten Lücken, baut
und testet **nur sein Crate** grün (`cargo build` / `cargo test -p <crate>`,
`PATH="$HOME/.cargo/bin:$PATH"`) und lässt fremde Crates unangetastet.

## Priorität (Stand 2026-08-17)
1. **CLI + native Desktop** (MVP, inkl. RAW)
2. Index (optional), dann Web/WASM (Post-MVP, kompatibel vorbereitet)

## Stufe 1 — Fundament (native, Prio 1)
Voraussetzung für CLI/Desktop. Crate-isoliert, parallel:
- **S1-core**: `lumina-core` — Renderpipeline-Contract, `RenderKey`,
  Cache-Invalidierung, Stale-Erkennung, Source-Actions (F-024..F-031, F-084..F-086).
- **S1-raw**: `lumina-raw` — native LibRaw härten: EXIF/Orientierung/Metadaten
  persistierbar, 8/16-bit, Fehler-/Capability-Tests, `decode_bytes`-Vertrag
  wasm-kompatibel gekapselt (F-034..F-038). Kein fremder Crate-Build.
- **S1-sidecar**: `lumina-sidecar` — Schema, virtuelle Kopien, Migrationen,
  atomare Writes/Recovery, Konfliktauflösung (F-011..F-023).

## Stufe 2 — CLI (Prio 1)
- **S2-cli**: `lumina-cli` — `import`, `inspect`, `develop`, `render`, `export`,
  `batch`, `mask`, `reindex`, `validate`; Sidecar-only ohne DB (F-052..F-056).
  Baut auf S1 auf.

## Stufe 3 — native Desktop-GUI (Prio 1)
- **S3-gui**: `lumina-gui` (eframe nativ) — Browser/Datei-Status, Vorschau+
  Histogramm an Renderstand gekoppelt, Regler/Auto-Tone/Presets nicht-destruktiv,
  Maskenwerkzeuge, Mehrbild (F-057..F-063, F-087..F-089). Baut auf S1/S2 auf.

## Stufe 4 — Optionaler Index
- **S4-index**: `lumina-index` — SQLite-Adapter, Rebuild aus Sidecars (F-064..F-067).

## Stufe 5 — Web/WASM (Post-MVP, kompatibel)
- **S5-wasm**: `lumina-raw` Feature `wasm-js` via `libraw-wasm`, GUI-Trunk-Pfad
  (F-069..F-071). Erst nach Stufe 3, nur bei freigegebenem Web-Scope.

## Verifikation
Jede Stufe wird durch einen unabhängigen Verifizierungs-Agenten (Build+Test+
Testabdeckung) abgenommen, bevor die entsprechenden Feature-IDs aus
`Agents.todo.md` entfernt werden.
