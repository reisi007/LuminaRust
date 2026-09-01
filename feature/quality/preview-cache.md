# Hybrid-Preview-Cache

**Feature:** PREVIEW-CACHE-FEATURE (siehe `Agents.todo.md`, Block A „Phase 11:
Qualität, Performance und Release", PRIO mittel)

## Inhaltsverzeichnis

- [Ziel und Geltungsbereich](#ziel-und-geltungsbereich)
- [Ausgangslage: Befunde aus review.notes und R2](#ausgangslage-befunde-aus-reviewnotes-und-r2)
- [SOLL-Architektur im Überblick](#soll-architektur-im-überblick)
- [Aktives Bild: GPU-Textur (VRAM)](#aktives-bild-gpu-textur-vram)
- [Nachbarn: WebP-Cache (Disk/RAM)](#nachbarn-webp-cache-diskram)
- [Cache-Key und Veraltung](#cache-key-und-veraltung)
- [Asymmetrisches Prefetch-Fenster](#asymmetrisches-prefetch-fenster)
- [LRU-Eviction und Speicherbudget](#lru-eviction-und-speicherbudget)
- [Hintergrund-Threads statt IdleQueue](#hintergrund-threads-statt-idlequeue)
- [Kein stiller Fallback, Veraltung sichtbar](#kein-stiller-fallback-veraltung-sichtbar)
- [Abgrenzung zu F-103-N6 und F-103-N1](#abgrenzung-zu-f-103-n6-und-f-103-n1)
- [Abgrenzung zu bestehenden Cache-Schichten](#abgrenzung-zu-bestehenden-cache-schichten)
- [Plattform-Abgrenzung (Capability-Matrix)](#plattform-abgrenzung-capability-matrix)
- [Akzeptanzkriterien und Tests](#akzeptanzkriterien-und-tests)
- [Bewusste Nichtziele](#bewusste-nichtziele)
- [Offene Punkte und Implementierungsrisiken](#offene-punkte-und-implementierungsrisiken)
- [Status](#status)

## Ziel und Geltungsbereich

Ziel ist sofortiges, ruckelfreies Scrollen und Navigieren durch Ordner mit
**40+ Bildern** in der Desktop-GUI (Develop/Library). Beim Wechsel vom
aktuellen Bild zum Nachbarn soll die Vorschau ohne sichtbare
Decode-/Render-Wartezeit erscheinen, ohne dass der Haupt-Thread oder die
interaktive Render-Pipeline des aktiven Bildes belastet wird.

Das Feature ist eine **Hybrid-Architektur** (User-Entscheidung, siehe
`Agents.todo.md`):

- das **aktive Bild** wird als vollwertige, interaktiv bearbeitbare
  **GPU-Textur (VRAM)** gehalten;
- die **Nachbarbilder** werden als **WebP-Cache** auf Screen- oder
  1:1-Auflösung vorgehalten — lazy gerendert, auf Disk und RAM gecacht,
  mit Alpha-Unterstützung.

WebP statt JPEG, weil der Kanal Alpha trägt und die Kompression bei
Fotografie und Masken-Vorschauen besser ist. Der WebP-Encoder/-Decoder ist in
`lumina-core` bereits verfügbar (image-Crate 0.25, Feature `webp`; genutzt
auch im F-037-Exportpfad), das Feature fügt dort also keine neue
Native-Abhängigkeit hinzu.

Dieses Dokument ist das verbindliche SOLL für die spätere Implementierung.
Es enthält **keinen Code**; es legt Architektur, Cache-Semantik, Prefetch-
Fenster, Eviction, Threading und Abnahmekriterien fest.

## Ausgangslage: Befunde aus review.notes und R2

Das Feature adressiert die dokumentierten Performance-/UX-Befunde zum
Bildwechsel und Filmstrip:

- **`review.notes.md` (2026-08-22, kritisch):** „Hauptthread-Blockierung durch
  synchrone Thumbnail-Generierung" — die IdleQueue (Kapazität 32) lief im
  UI-Thread und blockierte den Frame beim Decode. „Thumbnails bleiben dauerhaft
  Platzhalter bei >32 Bildern pro Ordner" — volle IdleQueue verhungerte
  sichtbare Zellen. Beides ist inzwischen behoben (echte Worker-Threads,
  unbounded Channel + Dedup, `crates/lumina-gui/src/filmstrip.rs`; positive
  Bestätigung im R2-Bericht). Der hier spezifizierte Nachbar-Preview-Pfad muss
  **dieselbe Worker-Thread-Struktur** verwenden und darf nicht in die IdleQueue
  zurückfallen (siehe [Hintergrund-Threads statt IdleQueue](#hintergrund-threads-statt-idlequeue)).
- **`review.notes.md` (Screenshot 2026-08-22):** falsche Preview-Skalierung /
  blockierte Interaktion im Develop-Modul — die Preview muss aspekt-treu
  fensterfüllend (`fit`/Letterboxing) sein. Der WebP-Nachbarcache hält
  Vorschauen in der Ziel-Auflösung; das Hochskalieren kleiner Thumbnails auf
  Panel-Größe ist kein Ersatz für Screen-Previews.
- **R2 (docs/reviews/2026-08-26-full-review.md, Performance):**
  - `R2-PERF-01` (`analyze_tone` allokiert n×f64 pro Render-Tick),
  - `R2-GUIMOD-02` (Vollbild-Copy + Textur-Reupload bei jedem Repaint ohne
    Dirty-Gate),
  - `R2-GPU-01` (Input-Textur wird pro Drag-Tick neu erzeugt, ~96 MB Upload),
  - `R2-GUIMOD-04` (CPU-Draft läuft auf GPU-Pfaden redundant mit).
  Diese Befunde betreffen den **interaktiven Render-Hotpath des aktiven
  Bildes**. Der Nachbar-Prefetch darf diesen Hotpath nicht zusätzlich
  belasten: Nachbarn werden auf **reduzierter Auflösung** (Screen/1:1) und in
  **Hintergrund-Worker-Threads** gerendert, nicht im UI-Thread und nicht im
  VRAM-Renderpfad des aktiven Bildes.

## SOLL-Architektur im Überblick

```
        Bild N-2   Bild N-1   [ BILD N ]   Bild N+1   Bild N+2   Bild N+3   Bild N+4
                               (aktiv)
       ────────────────────────── RAM-LRU (7 Slots) ──────────────────────────
                              │
                    ┌─────────┴──────────┐
                    │                    │
        VRAM-GPU-Textur           WebP-Cache (Screen/1:1)
        (aktives Bild,            Disk + RAM, lazy gerendert,
         interaktiv)              decode-/render-frei beim Scrollen
```

- **Ebene 1 — aktives Bild:** Vollauflösungs-Render als GPU-Textur im VRAM
  (egui-Native-Texture bzw. vorhandener GPU-Present-Pfad, PERF-GUI-2).
  Interaktion (Slider, Masken, Before/After) läuft unverändert auf diesem
  Pfad.
- **Ebene 2 — Nachbarn:** WebP-kodierte Vorschauen in der Ziel-Auflösung.
  Beim Erreichen eines Nachbarns wird dessen WebP aus dem RAM-LRU (falls
  vorhanden) oder von Disk geladen und ohne Decode-/Render-Wartezeit
  angezeigt; nur ein kompletter Miss rendert lazy im Hintergrund neu.

Das aktive Bild wird **nie** über den WebP-Cache angezeigt — es ist immer die
GPU-Textur. Der WebP-Cache ist ausschließlich für Nachbarn (Vorbereitung des
nächsten Sichtwechsels) und damit eine reine Performance-Schicht.

## Aktives Bild: GPU-Textur (VRAM)

- Der aktuelle Frame liegt als GPU-Textur im VRAM und wird über den
  bestehenden Present-/VRAM-Pfad angezeigt (keine zweite Renderpipeline).
- Änderungen am Rezept rendern in den bestehenden interaktiven Pfad
  (CPU-Draft/GPU-Draft inklusive Debounce wie bisher); der Nachbar-Cache
  beobachtet den Render-Key und markiert betroffene Nachbarn als veraltet
  (siehe [Cache-Key und Veraltung](#cache-key-und-veraltung)).
- Der Wechsel zum Nachbarbild „promotet" dessen RAM-LRU-Eintrag zum aktiven
  Bild: WebP-Decode → GPU-Textur, danach wird das bisher aktive Bild als
  Nachbar in den WebP-Cache übernommen (Kodierung bei Bedarf).

## Nachbarn: WebP-Cache (Disk/RAM)

- **Auflösung:** Screen-Auflösung (Panel-fit, aspekt-treu, Letterboxing) als
  Standard. Eine 1:1-Vorschau folgt der geerbten Ordneroption aus dem
  `DiskFolderCache`-Settings-Modell (Standardmäßig aus, siehe
  `feature/README.md` / `feature/architecture/pipeline.md`).
- **Kodierung:** WebP. Verlustfrei (lossless) oder hochwertig verlustbehaftet
  — konfigurierbar; lossless ist der sichere Default für Vorschau-Zwecke,
  hochwertig verlustbehaftet für große Ordner/Speicherbudget. Alpha (RGBA)
  wird immer erhalten — WebP trägt Alpha nativ.
- **Lazy:** Es wird nur gerendert, was durch das Prefetch-Fenster oder einen
  echten Navigationsbedarf angefordert wird. Kein Vorab-Rendern des gesamten
  Ordners.
- **Disk-Tier:** persistente WebP-Dateien unter `.lumina/previews/` (nicht
  autoritativ, löschbar, Prune verwaister Einträge analog `DiskFolderCache`).
  Key = Cache-Key (siehe unten). Atomare Writes; unvollständige Dateien werden
  als Miss behandelt, nie als gültiger Hit.
- **RAM-Tier:** der LRU hält die dekodierten Frames (bzw. Texturen) der
  Fenster-Slots, damit der Bildwechsel ohne Disk-I/O auskommt (siehe
  [LRU-Eviction und Speicherbudget](#lru-eviction-und-speicherbudget)).

## Cache-Key und Veraltung

Jeder WebP-Eintrag trägt einen vollständigen, aus Eingaben abgeleiteten Key.
Er kombiniert mindestens:

- **Quell-Content-Hash** (blake3, wie im RenderKey/`recipe_hash`-Modell der
  Pipeline; der schnelle Fingerprint genügt nicht als alleiniger Key),
- **Decode-/Geometrie-Kontext** (Decode-Version, Pipeline-Version,
  ROI/Rahmengeometrie),
- **Virtual-Copy-ID** und **Render-Key/Rezept-Hash** (Rezeptversion, alle
  rezeptabhängigen Stufen),
- **Preview-Kind und -Auflösung** (Screen vs. 1:1, Zielbreite/-höhe),
- **Encoder-/Formatparameter** (lossless vs. Qualität, Farbraum/Alpha).

Veraltung wird **am Key gemessen**, nicht am Zeitstempel:

- Quelle geändert (Content-Hash weicht ab) → Eintrag **stale**.
- Rezept der virtuellen Kopie geändert (Render-Key weicht ab) → Eintrag
  **stale**.
- Preview-Kind/Auflösung geändert (z. B. Ordneroption 1:1) → Eintrag für die
  alte Auflösung wertlos, aber kein Korruptionsfall.

Stale Einträge werden **nicht still angezeigt**: Die Zelle zeigt einen
sichtbaren Veraltet-Zustand und der lazy Re-Render wird angewiesen. Der
schnelle Quell-Fingerprint darf — wie in der Pipeline festgelegt — nur bei
kritischen Operationen durch den vollständigen BLAKE3-Hash ersetzt werden;
die Cache-Key-Validierung beim Laden eines Nachbarn ist eine solche
kritische Operation.

## Asymmetrisches Prefetch-Fenster

Beim Aktivieren von Bild N wird ein **asymmetrisches Fenster** vorbereitet:

- **+4 vorwärts / -2 rückwärts** relativ zum aktiven Bild.
- Das Fenster umfasst aktiv + 6 Nachbarn = **7 Bilder im RAM-LRU**.
- **Prefetch-Priorität** (absteigend): `+1 > +2 > -1 > +3 > -2 > +4`.
  Vorwärts wird stärker gewichtet als rückwärts, weil der übliche
  Arbeitsfluss im Ordner vorwärts läuft (nächste Aufnahme, nächste Kopie).
- **Rand:** Am Ordnerende werden nur die vorhandenen Nachbarn vorbereitet —
  **kein Wrap-Around** vom Ende zum Anfang und umgekehrt. Der RAM-LRU fasst
  dann entsprechend weniger Einträge (nicht mehr als 7).
- Ein Prefetch-Job wird pro Cache-Key nur **einmal** angestoßen (Dedup über
  den Key; in-flight-Markierung analog zum `ThumbnailManager`).

## LRU-Eviction und Speicherbudget

- Der RAM-LRU hält **maximal 7 Einträge** (aktiv + 6 Nachbarn) und ist
  zusätzlich **byte-budgetiert**: **GUI (Desktop) 8 GB gesamt (RAM+VRAM
  kombiniert, dynamisch/LRU, aktiv nie evictet)** — damit passen 7
  Vollauflösungs-Frames je 96 MB (RGBA8, 24 MP) ≈ 672 MB (selbst 45 MP ≈
  1,3 GB) locker ins Budget; **CLI/Headless minimal (kein Preload, nur
  aktiver Frame, ~512 MB)**, **wasm32 48 MiB** (F-075-abgestimmt). 1:1 braucht
  keine Slot-Reduktion oder WebP-im-RAM-Kompression.
- **Eviction:** Least-Recently-Used. Das aktive Bild wird nie evictet
  (promotet bei jedem Zugriff). Die evictierte Frame wird bei vorhandenem
  WebP-Disk-Eintrag einfach verworfen (Disk bleibt); ohne Disk-Eintrag wird
  sie vor dem Verwerfen kodiert, sofern noch relevant.
- Der Disk-Cache wird nicht über das RAM-LRU verwaltet, sondern über
  Lebensdauer-/Größenregeln des `.lumina/`-Caches (löschbar, Prune bei
  fehlender Quelle).

## Hintergrund-Threads statt IdleQueue

- Nachbar-Rendering und WebP-Kodierung/Decodierung laufen auf **dedizierten
  Hintergrund-Worker-Threads** (fester kleiner Pool, Ergebnis-Channel zum
  UI-Thread). **Nicht** über die `IdleQueue` (deferred Work im UI-Thread).
- Vorbild ist die bestehende Filmstrip-Thumbnail-Pipeline
  (`crates/lumina-gui/src/filmstrip.rs` + Worker-Pool in `lib.rs`), die seit
  dem review.notes-Befund echte Worker-Threads nutzt. Der Nachbar-Prefetch
  teilt sich diesen Pool nicht unbegrenzt: Die Prefetch-Priorität muss in der
  Auftragsvergabe des Pools berücksichtigt werden (Prioritätswarteschlange
  statt reiner FIFO), damit ein sichtbarer Zellbedarf nicht hinter
  Prefetch-Aufträgen zurücksteht.
- Worker-Fehler werden **immer** als Ergebnis zurückgemeldet (nie
  still geschluckt) und führen zu einem sichtbaren Zell-Zustand — gleiches
  Muster wie `ThumbnailOutcome::Failed`.
- Parallele Preview-Ergebnisse, die nicht mehr zum aktuellen
  Navigations-/Rezeptstand gehören, werden verworfen (entspricht der
  bestehenden Pipeline-Regel „Parallele Preview-Ergebnisse werden verworfen,
  wenn sie nicht mehr zum aktuellen Rezeptstand gehören").

## Kein stiller Fallback, Veraltung sichtbar

Es gelten die Agents.md-Invarianten:

- Ein Cache-Miss ist ein **Performance-Ereignis, kein Fallback**: Die Zelle
  zeigt den Lade-/Berechnungszustand sichtbar an („wird vorbereitet"), bis
  der Nachbar fertig ist. Es wird **nie** ein falsches Bild oder eine
  hochskalierte Thumbnail-Größe still angezeigt.
- **Veraltet** (stale Key) wird sichtbar als „Veraltet"/„Bild geändert"-
  Badge ausgezeichnet und lazy neu gerendert — kein stilles Anzeigen eines
  alten Frames.
- **Fehler** (Decode/Encode/Corrupt) erscheinen sichtbar in der Zelle,
  inklusive begrenzter Retries (Muster aus `ThumbnailManager`,
  `THUMBNAIL_MAX_ATTEMPTS`).
- Eine unvollständige/temporäre WebP-Datei (abgebrochener Write) wird als
  Miss behandelt und nie als gültiger Hit gewertet.

## Abgrenzung zu F-103-N6 und F-103-N1

- **F-103-N1** (Filmstrip): Der Filmstrip zeigt kleine Thumbnails (max.
  200 px Kante) aus Disk-Cache + Worker-Jobs. Der Hybrid-Preview-Cache ist
  die **nächste Stufe**: Screen-/1:1-Vorschauen der Nachbarn für weiches
  Scrollen in der Hauptansicht, nicht nur Miniaturansichten. Die
  Thumbnail-Pipeline bleibt als solche bestehen (kleine Zellen, eigener
  Bedarf) und profitiert indirekt vom WebP-Cache.
- **F-103-N6** (erster visueller User-Test): PREVIEW-CACHE-FEATURE
  **blockiert F-103-N6 nicht** (Zusicherung in `Agents.todo.md`). Allerdings
  soll der manuelle Test bereits die optimierte Version zeigen — d. h. das
  Feature ist **vor** F-103-N6 zu implementieren, damit der Test das
  Scroll-Erlebnis mit Hybrid-Cache bewertet. Abnahme für den manuellen Test:
  40+ Bilder-Ordner, Scrollen in beide Richtungen, Ränder des Ordners, kein
  Ruckeln/Hänger, sichtbare Vorbereitungs- und Veraltet-Zustände.

## Abgrenzung zu bestehenden Cache-Schichten

| Schicht | Zweck | Schlüssel | Autonomie |
| --- | --- | --- | --- |
| `DiskFolderCache` (F-086, `.lumina/previews/`) | persistente Standard-/1:1-Vorschau pro Quelle + Kopie beim Verlassen | Quelle + VC + Kind | nicht autoritativ, löschbar |
| `StageFrameCache` (PERF-GUI-1, RAM-LRU) | rezeptblinder Basis-Decode für die interaktive Pipeline | `RenderKey::stage_digest` (Basis) | RAM-only |
| GPU-Pfad (PERF-GUI-2, VRAM-Pool/Texturen) | interaktiver Render/Present des aktiven Bildes | Render-Key/Textur-Generation | VRAM |
| **Hybrid-Preview-Cache (dieses Feature)** | **Nachbar-Vorschauen für sofortiges Scrollen** | Content-Hash + Render-Key + Kind/Auflösung | Disk + RAM |

Der Hybrid-Preview-Cache **ersetzt** keine dieser Schichten, sondern
orchestriert über ihnen: Er hält das aktive Bild im GPU-Pfad, nutzt den
`DiskFolderCache`-Mechanismus (atomare Writes, Prune, Settings-Vererbung für
1:1) für die WebP-Disk-Ablage und den `StageFrameCache` unverändert für die
interaktive Rezept-Arbeit. Ein WebP-Eintrag, der noch dem aktuellen
`RenderKey` entspricht, darf als sofortiger Anzeige-Kandidat beim
Bildwechsel dienen; ein abweichender wird verworfen bzw. sichtbar als
veraltet markiert und lazy neu gerendert.

## Plattform-Abgrenzung (Capability-Matrix)

Siehe `feature/platform/capability-matrix.md`:

- **Desktop (nativ):** vollständige Zielplattform — GPU-Textur (VRAM) +
  WebP-Disk/RAM-Cache. RAW-Nachbarn werden über den nativen LibRaw-Pfad auf
  Worker-Threads lazy dekodiert.
- **CLI/Headless:** kein Scroll-/GUI-Kontext; der WebP-Cache ist hier kein
  Feature. Die WebP-Encode-Decode-Fähigkeit bleibt über den F-037-Exportpfad
  in `lumina-core` verfügbar.
- **Browser (WASM):** post-MVP. RAW fehlt (Capability-Grenze), native
  Datei-I/O fehlt; ein RAM-only-LRU ohne Disk-Tier wäre der maximal
  mögliche Umfang. Kein Funktionsentwicklungsziel im MVP.

## Akzeptanzkriterien und Tests

Abnahme der Implementierung gegen dieses SOLL:

1. **Scroll-Erlebnis:** Ordner mit 40+ Bildern; Vorwärts- und Rückwärts-
   Navigation zeigt beim Erreichen eines Nachbarn dessen Vorschau ohne
   sichtbaren Decode-/Render-Hänger (Cache-Hit) oder einen klar sichtbaren
   „wird vorbereitet"-Zustand (Miss). Keine UI-Thread-Blockade.
2. **Prefetch-Fenster:** Bei Aktivierung von N werden in Priorität
   `+1 > +2 > -1 > +3 > -2 > +4` genau die vorhandenen Nachbarn vorbereitet;
   am Ordnerrand weniger, **kein Wrap**.
3. **RAM-LRU:** Maximal 7 Einträge; Eviction nach LRU; das aktive Bild wird
   nie evictet; Byte-Budget (**GUI 8 GB gesamt RAM+VRAM**, CLI minimal ~512 MB,
   wasm 48 MiB, F-075-abgestimmt) wird eingehalten — Test inkl. 1:1-Fall
   (7×24 MP ≈ 672 MB liegt weit unter 8 GB).
4. **Cache-Key/Veraltung:** Quelländerung (Content-Hash) oder Rezeptänderung
   (Render-Key) macht einen gecachten Nachbarn sichtbar „veraltet" und löst
   lazy Re-Render aus; kein stilles Anzeigen des alten Frames.
5. **Alpha:** WebP-Einträge erhalten den Alpha-Kanal (Roundtrip-Test mit
   semitransparenten Pixeln).
6. **Threading:** Nachbar-Rendering läuft auf Worker-Threads, nicht in der
   IdleQueue; Worker-Fehler werden sichtbar gemeldet (Retry-Politik wie beim
   ThumbnailManager); veraltete Parallel-Ergebnisse werden verworfen.
7. **Disk-Cache:** atomare Writes; unvollständige Dateien gelten als Miss;
   Prune verwaister Einträge; kompletter Löschbarkeitstest.
8. **Performance (F-074):** neue Benchmark-IDs für WebP-Cache-Hit/-Miss und
   Prefetch-Pipeline inkl. Budgets gegen die committeten Baseline-/Budget-
   Stores (`scripts/perf/compare.mjs`); Messung, dass der Prefetch den
   interaktiven Render-Hotpath (R2-PERF-01/R2-GUIMOD-02/R2-GPU-01) nicht
   verschlechtert.
9. **Manueller Test (F-103-N6-Kopplung):** die optimierte Version wird im
   ersten visuellen User-Test gezeigt; Scrollen durch 40+ Bilder ist Teil der
   Abnahme.

## Bewusste Nichtziele

- Kein Wrap-Around des Prefetch-Fensters am Ordnerrand.
- Keine prädiktive Vorhersage der Scrollrichtung (kein ML); das feste
  asymmetrische Fenster + LRU ist das Modell.
- Kein WebP-Cache für das **aktive** Bild (dieses bleibt immer GPU-Textur).
- Kein Austausch oder Umbau des interaktiven GPU-Renderpfads (PERF-GUI-2).
- Kein automatisches Prerendern des gesamten Ordners.
- Keine Änderung am Sidecar-/Persistenzmodell: Der Cache bleibt nicht
  autoritativ und vollständig löschbar.

## Offene Punkte und Implementierungsrisiken

- **RAM-Budget bei 1:1:** Mit **GUI 8 GB** passen 7 Vollauflösungs-Frames (672 MB @24 MP, ~1,3 GB @45 MP) locker ins Budget — keine Slot-Reduktion oder WebP-im-RAM-Kompression nötig. CLI bleibt minimal ohne Preload.
- **Prioritätswarteschlange des Worker-Pools:** Der bestehende
  Thumbnail-Pool ist FIFO; Prefetch-Priorität erfordert eine
  Prio-Queue oder getrennte Auftragsklassen — Änderung am gemeinsamen
  GUI-Pool (Konfliktfläche mit dem lumina-gui-Agenten).
- **Grenze Screen-/1:1-Umschaltung:** Wechsel der Ordneroption zur
  Laufzeit invalidiert die gecachten Auflösungen; Verhalten und
  Migrationsstrategie für bestehende WebP-Einträge sind zu definieren.
- **Interaktion mit R2-GPU-01/R2-GUIMOD-02:** Der WebP-Decode auf dem
  UI-Thread darf keinen Vollbild-Copy/Re-Upload erzeugen; Decode-Ergebnis
  muss fertig als Textur ankommen (Worker → UI-Thread, ein Upload).
- **WebP-Qualitätsparameter:** lossless vs. verlustbehaftet — Default und
  Konfigurationsort (Settings-Vererbung) sind noch offen.
- **Doku-Konsistenz:** `feature/platform/cli-gui-wasm.md` (Implementierungs-
  status F-103, 2026-08-21) beschreibt die Thumbnail-Generierung noch als
  „via IdleQueue" — veraltet seit dem Worker-Thread-Umbau; beim nächsten
  Status-Update in jenem Dokument mitkorrigieren.

## Status

- **Doku-first (2026-09-01):** SOLL-Architektur verfasst, im
  `feature/README.md` verlinkt, PREVIEW-CACHE-FEATURE in `Agents.todo.md`
  um das asymmetrische +4/-2-Fenster ergänzt.
- **Implementierung:** offen. Kein Crate-Code in diesem Stand.
