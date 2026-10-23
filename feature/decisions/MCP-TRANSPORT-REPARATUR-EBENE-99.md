# MCP-TRANSPORT-REPARATUR-EBENE-99 — Reparatur-Ebene per Datei

**Stand: MCP und CLI umgesetzt und unabhängig verifiziert (2026-10-23).**
Eigentümerentscheid: `lumina_dust_removal` und `lumina-cli dust-removal`
verwenden für die Reparatur-Ebene einen Dateiverweis statt eines
Inline-/Base64-Payloads.

`region_path` bezeichnet eine headerlose Binärdatei mit row-major angeordneten,
little-endian-u16-Werten. `width` und `height` bleiben die maßgeblichen
Dimensionsfelder. Die Datei muss exakt `width * height * 2` Bytes enthalten;
Trunkierung und zusätzliche Bytes werden abgelehnt. Dimensionen und Dateigröße
sind vor Allokation/Lesen sicher zu validieren, mit begrenztem Lesen und ohne
Integer-Überlauf. `replacement_path` und die übrige Semantik bleiben unverändert.
Das CLI-JSON verwendet ebenfalls ausschließlich `region_path`; alte
`region_values`-Arrays werden strikt abgelehnt (keine Legacy-Option). Die
CLI-JSON-Definition ist auf 64 KiB begrenzt; die Plane wird mit exakter Länge
und begrenztem Lesen geladen.

Der stdio-Transport behält seine harte Frame-Obergrenze von 1,25 MiB. Der
Dateiverweis entfernt die JSON-Verstärkung der Ebene, hebt jedoch **nicht** die
Grenzen von `RepairRegionArtifact::validate()` oder des persistierten
zdata-Payloads auf.

Abnahme: Schema, Tool-Dokumentation und Beispiele verwenden `region_path`;
Tests decken exakte LE-Dekodierung/Roundtrip, falsche Byte-Längen,
überdimensionierte Eingaben und Tool-Ausführung ab. Keine Änderung am
persistierten Artefaktformat oder Rendering.
