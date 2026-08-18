# Reference-image fixtures (F-043)

Diese drei PNG-Fixtures sind die Referenzbilder für die Auto-Tone- und
Exposure-Matching-Tests in `../reference_images.rs` (F-043, „Auto-Tone und
Exposure Matching besitzen Unit-, Property- und Referenzbildtests",
`feature/architecture/pipeline.md` → „## Abnahme").

## Provenance

- **Alle Fixtures wurden programmatisch erzeugt** — es gibt keine externen
  Quellen (kein Herunterladen, kein Scannen, kein fremdes Bildmaterial).
- Es besteht **keine Lizenzpflicht**: Die Bilder sind triviales, formelhaft
  generiertes Testmaterial (einfarbige Quadrate und ein Grauverlauf) und
  entstehen ausschließlich aus den unten dokumentierten Pixelfunktionen.
- Die Erzeugung ist **deterministisch und reproduzierbar**: PNG-Encoding über
  `image` 0.25 mit Standardeinstellungen (via `ImageFrame::encode` /
  `DynamicImage::write_to(ImageFormat::Png)`) ist byte-deterministisch. Die
  drei Dateien wurden mit einem Wegwerf-Generator-Test erzeugt; bei einer
  Neu-Erzeugung aus denselben Pixelfunktionen entstehen byte-identische
  Dateien (MD5: `5e5f41362df4de7c5461a08b8c62cafa`,
  `819d3878e11e89c7daa2310d28210606`, `7db135ef0f3e11140fc87f5bbed24cc7`).
- Alle Bilder sind **8×8 Pixel, RGBA8, Alpha = 255** (vollständig deckend,
  aber Alpha ist für die Tonwert-Domäne ohnehin irrelevant).

## Exakte Pixelfunktionen

Alle Fixtures sind 8×8 (Zeilen `y` und Spalten `x` jeweils `0..=7`,
Reihenfolge zeilenweise). `gray(v) = [v, v, v, 255]`.

### `reference_gradient.png` — linearer Grauverlauf

```
pixel(x, y) = gray(y * 8 + x)        // Werte 0..=63, exakt einmal je Stufe
```

- 64 Graustufen 0..=63, monoton aufsteigend zeilenweise.
- Statistik (Formel): `mean = 31.5/255`, `median = 31.5/255`,
  `p01 = 0.63/255`, `p99 = 62.37/255` (Quantilpositionen
  `q * (n - 1)` mit linearer Interpolation, Konvention von
  `analyze_tone`).

### `reference_checker.png` — Schachbrett schwarz/weiß

```
pixel(x, y) = gray(255) falls (x + y) gerade, sonst gray(0)
```

- Exakt 32 weiße und 32 schwarze Pixel.
- Statistik (Formel): `mean = 0.5`, `median = 0.5`, `p01 = 0.0`,
  `p99 = 1.0` (jeweils bis auf eine ULP der Rec.709-Gewichtsumme).

### `reference_zone.png` — vier Tonzonen (Quadranten)

```
pixel(x, y) = gray( 20) falls x < 4 und y < 4   (oben links)
            = gray( 90) falls x >= 4 und y < 4  (oben rechts)
            = gray(160) falls x < 4 und y >= 4  (unten links)
            = gray(230) sonst                    (unten rechts)
```

- Vier 4×4-Quadranten mit 16 Pixeln je Zone.
- Statistik (Formel): `mean = 125/255`, `median = 125/255`,
  `p01 = 20/255`, `p99 = 230/255`.

## Regeneration

Die exakte Erzeugungslogik (identisch zu den Pixelfunktionen oben) steckte im
Wegwerf-Generator-Test `tests/gen_fixtures_tmp.rs` (gelöscht nach der
Erzeugung). Zur Reproduktion genügt eine beliebige PNG-Encode-Quelle mit
Standardparametern (z. B. `ImageFrame::encode(ImageFileFormat::Png)` in
`lumina-core`); die Bytes sind deterministisch, solange die
Encode-Version von `image` unverändert bleibt. Die Tests selbst lesen die
Dateien per `include_bytes!`, sodass die Fixtures Teil des Builds sind.
