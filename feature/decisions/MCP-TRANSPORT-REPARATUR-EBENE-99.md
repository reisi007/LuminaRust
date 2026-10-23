# MCP-TRANSPORT-REPARATUR-EBENE-99 — Reparatur-Ebene per Datei

**Stand: umgesetzt und unabhängig verifiziert (2026-10-23).** Eigentümerentscheid:
`lumina_dust_removal` nimmt für
`repair_region` einen Dateiverweis statt eines Inline-/Base64-Payloads.

`region_path` bezeichnet eine headerlose Binärdatei mit row-major angeordneten,
little-endian-u16-Werten. `width` und `height` bleiben die maßgeblichen
Dimensionsfelder. Die Datei muss exakt `width * height * 2` Bytes enthalten;
Trunkierung und zusätzliche Bytes werden abgelehnt. Dimensionen und Dateigröße
sind vor Allokation/Lesen sicher zu validieren, mit begrenztem Lesen und ohne
Integer-Überlauf. `replacement_path` und die übrige Semantik bleiben unverändert.

Der stdio-Transport behält seine harte Frame-Obergrenze von 1,25 MiB. Der
Dateiverweis entfernt die JSON-Verstärkung der Ebene, hebt jedoch **nicht** die
Grenzen von `RepairRegionArtifact::validate()` oder des persistierten
zdata-Payloads auf. Insbesondere wird nicht zugesagt, dass jede 24-MP-Ebene als
Artefakt akzeptiert wird.

Abnahme: Schema, Tool-Dokumentation und Beispiele verwenden `region_path`;
Tests decken exakte LE-Dekodierung/Roundtrip, falsche Byte-Längen,
überdimensionierte Eingaben und Tool-Ausführung ab. Keine Änderung am
persistierten Artefaktformat oder Rendering.
