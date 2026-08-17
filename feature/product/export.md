# F-037 Export

Der Core exportiert das gerenderte Bild als sRGB in PNG, JPEG oder WebP. PNG
ist verlustfrei. JPEG ist verlustbehaftet; WebP unterstützt verlustbehafteten
und verlustfreien Export (MVP-API verwendet den Qualitätswert für den
verlustbehafteten Weg). Qualität liegt bei `1..=100`, Standard ist 90.

Der MVP-Ausgaberaum ist RGBA8/sRGB. 16 Bit pro Kanal ist eine ausdrücklich
dokumentierte Post-MVP-Grenze. sRGB ist das einzige Exportprofil; ein ICC-Profil
muss im MVP nicht eingebettet werden, eine ICC-Datei ist Post-MVP. EXIF- und
XMP-Weitergabe ist ebenfalls Post-MVP; der MVP schreibt keine Metadaten und
macht darüber keine stillen Annahmen.

`ExportOptions` enthält `format`, `bit_depth`, `quality`, `dither` und einen
deterministischen `seed`. `Default` wählt PNG, 8 Bit, Qualität 90, Dithering
an und Seed 0. `ImageFrame::encode(format)` bleibt rückwärtskompatibel;
`encode_with_options` validiert Werte und reicht JPEG/WebP-Qualität an den
Encoder weiter. Das portable `image`-WebP-Modul bietet im MVP nur VP8L
verlustfrei; für Qualität kleiner als 100 wird deshalb vor dem VP8L-Schritt
deterministisch quantisiert (Qualität 100 bleibt verlustfrei). Ein nativer
libwebp-Encoder mit echtem VP8-Lossy ist eine spätere Capability-Erweiterung.
Dithering ist eine optionale, deterministische Banding-
Reduktion beim 8-Bit-Roundtrip (xorshift-basierte Folge); gleicher Seed und
gleiche Eingabe liefern identische Bytes.

Abnahme: PNG-Dekodierung ist pixelgenau, Format-Erkennung funktioniert für alle
drei Formate, Qualität 1..=100 wird abgelehnt bzw. akzeptiert, und gleiche
Dither-Seeds liefern gleiche Ergebnisse. Qualitätstests prüfen mindestens,
dass Encoderaufrufe unterschiedliche, gültige Dateien erzeugen; Dateigröße
ist wegen Encoderimplementierungen kein fachlicher Monotonie-Vertrag.
