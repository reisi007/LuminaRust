# Architecture Decision Records

Dieses Verzeichnis enthält Architecture Decision Records (ADRs) für das
LuminaRust-Projekt. ADRs dokumentieren bewusste, fundierte Entscheidungen über
die Architektur, Technologiewahl und organisatorische Grenzen.

## Workflow

1. **Auslöser:** Eine Entscheidung betrifft Schema, Pipeline, Plattformgrenzen,
   Technologiewahl oder Konsistenz zwischen `lumina-core`, `lumina-raw`,
   `lumina-onnx`, `lumina-sidecar`, `lumina-cli` oder `lumina-gui`.
2. **Erstellung:** Der ADR wird als Datei `docs/adr/NNNN-titel.md` angelegt.
   Die Nummer ist fortlaufend, der Titel in lowercase-kebab-case.
3. **Inhalt:** Jeder ADR enthält最少 Kontext, Entscheidung, Alternativen,
   Begründung und Konsequenzen. Die betroffenen Feature-IDs werden verlinkt.
4. **Status:** ADRs sind vorgeschlagen, akzeptiert oder abgelehnt. Akzeptierte
   ADRs werden nicht geändert; bei Änderung entsteht ein neuer ADR, der den
   alten referenziert.
5. **Referenz:** `feature/README.md`, `feature/decisions.md` und
   `feature/architecture/pipeline.md` verweisen bei Bedarf auf relevante ADRs.

## Index

| Nr | Titel | Status | Feature-IDs |
| --- | --- | --- | --- |
| 0001 | Sidecar-first | akzeptiert | F-001 |
| 0002 | RAW-Backend native LibRaw plus Post-MVP libraw-wasm | akzeptiert | F-007, F-010 |
| 0003 | Performance-Benchmarking | akzeptiert | F-074, F-075 |
