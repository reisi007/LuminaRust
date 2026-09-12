# Lightroom-UX-Parität (1:1-Klon-Ziel, UI/UX)

**Status (2026-09-12):** Bestandsaufnahme + Zielbild. User-Entscheid: Die
Oberfläche soll ein 1:1-Klon von Adobe Lightroom Classic werden (UI/UX;
Engine-Parität läuft weiter über die LRPAR-Tasks). Dieses Dokument ersetzt
nicht die F-100-Konventionen
(`platform/cli-gui-wasm.md`), sondern inventarisiert die Lücken dagegen und
definiert die Slices zum Schließen. Jede Slice-Implementierung beginnt mit
der hier vermerkten SOLL-Entscheidung (Doku-first-Regel); ohne sie kein Code.

**Quellen:** UX-Gap-Analyse „Lightroom-Classic-Klon" (15 Gaps, general-Agent,
read-only, 2026-09-12), UX-Gap-Analyse „Dust Removal" (8 Gaps, general-Agent,
read-only, 2026-09-12), `.goal/Goal.md` G-01…G-16,
`.goal/lighroom screenshots batch 1/` + `Content.md`.

## Gap-Inventory Klon (UXG)

| ID | Lücke (Classic vs. Lumina) | Evidenz | SOLL-Entscheid nötig | Task-Anker |
| --- | --- | --- | --- | --- |
| UXG-01 | Crop nicht interaktiv (nur Badge + x/y/w/h-Felder statt Rahmen/Handles/Drag/Enter) | `lib.rs:12758`, `:14287` | ja (Commit-Semantik) | neu |
| UXG-02 | Presets/History/Rating rechts statt links; Snapshots-Panel fehlt | `lib.rs:18418`, `:16725` | ja (Layout) | neu |
| UXG-03 | Library-Chrome fehlt (Catalog-Panel, View-Toolbar, Filterleiste, Quick Develop, Import-Dialog) | `lib.rs:15924`, `:18208` | ja (G-09-Scope) | neu |
| UXG-04 | Keine Werkzeugleiste über dem Bild (Q/K/M/R nur via Panel) | `lib.rs:17059` | eher nein | LRPAR-G11-OVERLAYS |
| UXG-05 | Panels ohne Kontext-Dynamik (Sections sind kollabierbar, aber nichts blendet je nach Tool/Selektion ein/aus) | `lib.rs:16735` | ja (Panel-Dynamik) | neu |
| UXG-06 | Develop-Reihenfolge ≠ Classic; Calibration-Panel fehlt | `lib.rs:16651` | ja (F-100-Reihenfolge) | neu |
| UXG-07 | History zeigt Maschinen-IDs statt Namen/Werte/Zeit | `lib.rs:16526` | ja (Schema/Extras) | neu |
| UXG-08 | Loupe/Compare/Survey sind Thumbnail-Stubs (kein Zoom/Pan, kein Render) | `lib.rs:16203` | ja (G-09-Scope) | neu |
| UXG-09 | Filmstreifen ohne Badges/Filter/Sort/Zähler | `lib.rs:16768` | nein | neu |
| UXG-10 | Before/After unvollständig (kein echtes Split); Softproof nur Badge | `lib.rs:13034` | teils | LRPAR-G10-VIEWER |
| UXG-11 | Keine Menüleiste; Modul-Leiste nicht Classic-artig | `lib.rs:18364` | ja (Menüumfang) | neu |
| UXG-12 | Presets = Datei-Browser statt Baum (kein Amount/Gruppen) | `lib.rs:16418` | ja (Amount-Semantik) | neu |
| UXG-13 | Kürzel `O`/`Z`/`Shift+L`/`I` fehlen; `E`-Alias unsauber | `lib.rs:135` | nein (F-100 erweitern) | LRPAR-G16-POWER |
| UXG-14 | Keine Canvas-Cursor; keine Stepper/Reset-Affordanz | Grep `CursorIcon` leer | nein | neu |
| UXG-15 | Navigator-Rail: Größe/Drag-Affordanz schwach (Duplikat-These visuell nicht belegt, niedrig prior) | `lib.rs:17347` | ja (Layout) | neu |

## Gap-Inventory Dust Removal (UXD, G-04)

| ID | Lücke | Evidenz | SOLL-Entscheid nötig |
| --- | --- | --- | --- |
| UXD-01 | Spot-Tool armiert Vorschau nicht (Klick/Zug wirkungslos — „kein brush") | `lib.rs:12223`, `:12432` | nein (SOLL deckt ab) |
| UXD-02 | Kein Brush-Cursor (Kreis = Radius, folgt Maus) | kein `CursorIcon` | klein (Cursor-SOLL präzisieren) |
| UXD-03 | Size/Feather nur Panel-Slider (Rad = Zoom statt Size) | `lib.rs:12158` | klein (Shift-Feather präzisieren) |
| UXD-04 | Pins: einzelner Fixpunkt, kein Target/Source-Paar, kein Drag/Delete/`/` | `lib.rs:4188` | **ja** (Pin-Semantik) |
| UXD-05 | Panel statisch/überladen statt kontextuell | `lib.rs:15123` | **ja** (Panel-Zielbild) |
| UXD-06 | Visualize = Dauer-Tint statt werkzeuggekoppelter High-Contrast-Ansicht | `lib.rs:10695` | teils |
| UXD-07 | Kein Quell-Pin/Alt-Ziehen (Offset unsichtbar) | `commit_spot_heal` | nein (SOLL deckt ab) |
| UXD-08 | Kein Einzel-Klick-Anlegen (nur Detect→Apply/CLI) | `lib.rs:15163` | nein (SOLL deckt ab) |

Direkt umsetzbar ohne Doc-Entscheid: UXD-01, -02, -03, -07, -08.

## Zielbild (3 Slices)

- **Slice A — Makro-Geometrie:** Menüleiste + zentrierte Modul-Leiste;
  Develop links Navigator + Presets + Snapshots + History; rechts Histogram +
  Basic … Calibration in Classic-Reihenfolge; darunter Filmstreifen mit
  Badges + Filter-/Zähl-Leiste. (UXG-02, -03 tlw., -06, -09, -11, -12, -15)
- **Slice B — Interaktives Werkzeug-Fundament:** Tool-Strip über dem Bild;
  On-Canvas-Crop mit Handles/Enter-Commit; werkzeugabhängige Cursor;
  Spot-Heal als echter Retusche-Modus (Brush-Cursor, Klick-Anlegen,
  Target/Source-Pins, Rad = Size); echte Split-Before/After- und
  Loupe-/Compare-/Survey-Renderansichten. (UXG-01, -04, -08, -10, -14; UXD)
- **Slice C — Library-Verwaltung:** Catalog-/Collections-Panel links;
  permanente Filter- und Quick-Develop-Leiste rechts; Import-Dialog; humane
  History-Labels; kontextuelle Panels (Tool-/Selektions-getrieben).
  (UXG-03 Rest, -05, -07, -13)

## Vision-Korrekturen (2026-09-12, 8 Goldens gesichtet)

UXG-05/UXG-15 wie oben präzisiert; UXG-09 verschärft (Filmstrip teils ganz
ohne Thumbs, nicht nur ohne Badges — Thumb-Infrastruktur aus Library
wiederverwenden). Zusätzlich gesichtet: Render-Hash als Canvas-Text
(Debug-Noise), doppelte Labels („Export/Export to"), weißer Randstreifen
neben Metadata-Panel (Layout-Bruch), kahler Library-Empty-State. Erster
Implementierungs-Slice: UX-SLICE-1 (Agents.todo.md).

## Offene SOLL-Entscheide (vor jeweiliger Slice-Implementierung)

Crop-Commit-Semantik, Seiten-Layout (Presets/History links, Menüumfang),
G-09-Scope-Anhebung (Catalog/Import/Loupe-Render), F-100-Reihenfolge
(Calibration, Color-Split, Optics vor Effects), History-Schema (Labels),
Pin-Semantik (Auswahl/Delete/`/`), Panel-Dynamik-Zielbild, Softproof-Anchor.

## Abnahme (je Slice)

DoD §6 Vision-Review der geänderten Layouts + kittest-Goldens (neu/geändert)
+ Headless-Asserts für Tool-Zustände; manueller Abgleich gegen die
`.goal`-Screenshots; unabhängige Verifizierung (BESTANDEN) vor Commit.
