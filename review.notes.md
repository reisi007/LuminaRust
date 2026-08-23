# Lumina GUI — Manueller Test, Review Notes

- **Datum:** 2026-08-22
- **Build:** `cargo build -p lumina-gui` (dev/target/debug/lumina-gui)
- **Startbefehl:**
  `./target/debug/lumina-gui "/Users/florianreisinger/Pictures/Sport/LASK/2026-27/2026-08-14-R3-Ried"`
- **Testordner:** 2026-08-14-R3-Ried (enthält .cr3 RAW + .jpg + .xmp/.acr Begleitdateien)
- **Status:** GUI startet, Workdir-CLI-Parameter wirkt (Verzeichnis wird als Startverzeichnis gesetzt).

---

## Rohbericht (unverarbeitet, vom Tester)

### Beobachtung 1 — Performance / Blockierung (vermutlich KRITISCH)
- Ein Loading-Spinner ist sichtbar.
- Vermutung: Viel läuft NICHT asynchron im Hintergrund, sondern blockiert den
  Prozess/Hauptthread.
- Solange nicht alle Thumbnails gerendert wurden, hat "nichts funktioniert"
  (UI nicht bedienbar / antwortet nicht).

### Beobachtung 2 — Weißabgleich-Fehler gefolgt von Crash (vermutlich KRITISCH)
- "Die GUI hat sich wegen falschen Weißabgleich beschwert und ist dann
  gecrasht."
- D. h. es gab eine Warn-/Fehlermeldung zum Weißabgleich, danach stürzte die
  GUI ab (vermutlich Panic bzw. Prozessende ohne strukturierten Absturzbericht,
  siehe Beobachtung 3).

### Beobachtung 3 — Zu wenig Log / keine Diagnosebarkeit
- "Wir brauchen während des Testens auf alle Fälle mehr Log, damit Fehler
  nachvollziehbar sind."
- Ziel: Fundament schaffen, damit nachvollziehbar ist, was VOR dem Crash
  passiert ist.

---

## Angereicherte & gruppierte Befunde

### [KRITISCH] Hauptthread-Blockierung durch synchrone Thumbnail-Generierung
- **Betroffene Dateien:** `crates/lumina-gui/src/lib.rs`, `crates/lumina-gui/src/filmstrip.rs`
- **Root-Cause:** Es gibt KEINE Hintergrund-Threads. Grep über die GUI-Crate
  bestätigt: kein `thread::spawn`, kein `rayon`, kein `tokio`, keine
  async/await-Nutzung im Thumbnail-Pfad (einziger `.await`-Treffer,
  `lib.rs:3666`, gehört nicht zum Thumbnail/WB-Pfad). Die `IdleQueue`
  (`crates/lumina-gui/src/lib.rs:116`) ist nur eine **per-Frame-Warteschlange
  auf dem Hauptthread**: `update()` (`lib.rs:3449`) nimmt pro Frame maximal
  EINEN Task via `pop_next()` (`lib.rs:3488`; Queue-Logik `lib.rs:140-166`)
  und führt ihn **synchron im UI-Thread** aus.
- Jeder Task ist `IdleTask::Thumbnail` → `generate_thumbnail()`
  (`lib.rs:3256`), das vollständig blockierend läuft:
  `std::fs::read` der vollen RAW-Datei (`lib.rs:3257`), vollständiger
  RAW-Decode via libraw (`lumina_raw::decode_bytes`, `lib.rs:3266`; bei
  ~30-MP-CR3 realistisch hunderte ms bis Sekunden), Nearest-Neighbor-
  Downscale (`filmstrip.rs:59-81`), PNG-Encode (`lib.rs:3300`),
  Disk-Cache-Write (`lib.rs:3304-3306`) und Texture-Upload
  (`make_thumbnail_texture`, `lib.rs:3244-3253`). Während dieses einen Decodes
  friert das gesamte egui-Frame ein — keine Eingaben, kein Repaint.
- Zusätzlicher Stopp-Effekt: Das Pointer-Gate
  `if !ctx.input(|input| input.pointer.any_down())` (`lib.rs:3487`) pausiert
  die Queue-Abarbeitung **komplett**, solange Maus/Finger gedrückt ist
  (Klick oder Drag). Bei fortgesetzter Interaktion bleibt jede Zelle
  Platzhalter, bis der Nutzer die Maus ruhen lässt.
- **Code-Referenzen:** `crates/lumina-gui/src/lib.rs:116-166` (IdleQueue),
  `lib.rs:342` (`IdleQueue::new(32)`), `lib.rs:3449` (update),
  `lib.rs:3483-3498` (Idle-Block + Pointer-Gate), `lib.rs:3210-3242`
  (ensure_thumbnail), `lib.rs:3256-3309` (generate_thumbnail),
  `crates/lumina-gui/src/filmstrip.rs:25-54` (ThumbnailManager),
  `crates/lumina-gui/src/main.rs:8-18` (`eframe::run_native`, single-threaded
  App-Setup).
- **Auswirkung:** Exakt das Testerlebnis „solange nicht alle Thumbnails
  gerendert sind, funktioniert nichts": Bei N Bildern friert die UI N-mal für
  die Dauer je eines vollständigen RAW-Decodes ein; dazwischen ist sie zwar
  technisch bedienbar, aber Thumbnails schreiten erst voran, wenn der Pointer
  losgelassen ist. Der Verdacht „läuft nichts asynchron" ist damit
  code-belegt richtig.
- **Lösungsrichtung:**
  1. Thumbnail-Generierung in echte Worker-Threads verlagern
     (`std::thread::spawn` + `mpsc`/Crossbeam-Channel zurück zum UI-Thread;
     Ergebnisse per `ctx.request_repaint()` einspeisen). Decode/Disk-IO
     gehören dabei komplett aus dem UI-Thread. Alternativ Rayon-Pool mit
     begrenzter Parallelität (Decode ist CPU-lastig).
  2. Pointer-Gate nur für den Start neuer Tasks verwenden, nicht als
     Abarbeitungs-Sperre, sobald Tasks asynchron laufen (Gate wird damit
     obsolet).
  3. Bereits generierte Disk-Previews (`DiskFolderCache`) weiterhin zuerst
     nutzen (`ensure_thumbnail`, `lib.rs:3217-3233`) — Cache-Hits können dann
     auch asynchron geladen werden.

### [KRITISCH / BUG] Thumbnails bleiben dauerhaft Platzhalter bei >32 Bildern pro Ordner
- **Betroffene Dateien:** `crates/lumina-gui/src/lib.rs`, `crates/lumina-gui/src/filmstrip.rs`
- **Root-Cause:** `ensure_thumbnail()` markiert einen Eintrag mit
  `mark_probed()` (`lib.rs:3228` Cache-Hit-Pfad bzw. `lib.rs:3234` vor dem
  Enqueue), **bevor** der Task eingereiht wird. `enqueue()`
  (`lib.rs:140-148`) verwirft den Task still, wenn die Queue voll ist
  (`if self.tasks.len() >= self.capacity { return None; }`,
  `lib.rs:141-143`); der Rückgabewert `None` wird mit `let _ =`
  ignoriert (`lib.rs:3235`). Die Kapazität ist hart auf 32 gesetzt
  (`idle_queue: IdleQueue::new(32)`, `lib.rs:342`).
- Da `draw_filmstrip` alle Entries pro Frame an `ensure_thumbnail`
  übergibt (`lib.rs:3204-3206`) und `probed`-Einträge nie zurückgesetzt
  werden (`filmstrip.rs:46-53`), bleiben Einträge ab Position 33 (genauer:
  sobald beim ersten Frame mehr als 32 Tasks konkurrenzieren) **für immer**
  unthumbnai­liert: probed=true, aber kein Task in der Queue.
- **Code-Referenzen:** `crates/lumina-gui/src/lib.rs:3234-3241` (mark_probed +
  verworfenes enqueue), `lib.rs:141-143` (stilles Verwerfen),
  `lib.rs:342` (Kapazität 32), `filmstrip.rs:50-53` (kein Un-Probing).
- **Auswirkung:** Korrektheitsbug unabhängig von der Performance:
  Verzeichnisse mit >32 unterstützten Bilddateien zeigen dauerhaft graue
  Platzhalterzellen (`lib.rs:3190-3193`) — auch wenn die CPU längst idle ist.
  Zusätzlich maskiert der stille Drop den Fehler (verstößt gegen das
  Projektprinzip „keine stillen Fallbacks").
- **Lösungsrichtung:** Entweder (a) `mark_probed` erst nach erfolgreichem
  `enqueue(...).is_some()` aufrufen und bei `None` den Eintrag im nächsten
  Frame erneut versuchen; oder (b) Kapazität deutlich erhöhen/dynamisch an
  die Entry-Anzahl koppeln; langfristig (c) mit echten Worker-Threads
  (siehe oben) eine ungebundene Jobliste im Worker führen und die
  UI-seitige Queue nur als Submission-Kanal nutzen.

### [INFO] Der sichtbare „Loading-Spinner" ist kein Spinner, sondern die Filmstrip-Platzhalterzelle
- **Betroffene Dateien:** `crates/lumina-gui/src/lib.rs`
- **Root-Cause:** Es gibt im GUI-Code keine Spinner-/Progress-Komponente.
  Was der Tester als „Loading-Spinner" sieht, ist die graue Ersatzzelle im
  Filmstrip: solange `thumbnails.get(&entry.name)` `None` liefert, wird nur
  ein ausgefülltes Rechteck (`Color32::from_gray(40)`) plus Dateiname
  gezeichnet (`crates/lumina-gui/src/lib.rs:3190-3193` in `draw_filmstrip`,
  ab `lib.rs:3174`). Dokumentiertes Verhalten laut Modulkopf
  (`filmstrip.rs:1-12`): „Until a thumbnail is ready a placeholder cell is
  shown".
- **Auswirkung:** Rein wahrnehmungsseitig — trägt aber zur Fehldiagnose
  „lädt noch" bei, obwohl (a) die Erzeugung gerade wegen des Pointer-Gates
  pausiert ist (`lib.rs:3487`) oder (b) der Task durch die volle Queue
  still verworfen wurde (siehe Bug oben).
- **Lösungsrichtung:** Optional UX-Verbesserung: Fortschrittsanzeige
  („x/y Thumbnails"), Pulsieren/Spinner-Icon für aktive Jobs und ein
  sichtbarer Hinweis, wenn Erzeugung pausiert (Pointer down) oder verworfen
  wurde (Queue voll) — konsistent mit dem Prinzip „sichtbar melden statt
  still warten".

### [KRITISCH] White-Balance-Fehler mit Folge-Desaster („beschwert sich, dann Crash")
- **Betroffene Dateien:** `crates/lumina-gui/src/lib.rs`,
  `crates/lumina-raw/src/lib.rs`, `crates/lumina-core/src/lib.rs`
- **Root-Cause (verifizierter Datenfluss):**
  1. libraw liefert die As-Shot-Verstärkungen ungeprüft:
     `camera_white_balance = data.color.cam_mul`
     (`crates/lumina-raw/src/lib.rs:285`, unverändert in `RawMetadata`
     übernommen, `lib.rs:313`; Typ `[f32; 4]`, **nicht** Option —
     `crates/lumina-raw/src/lib.rs:69`). Für manche RAWs (u. a. CR3-Varianten)
     kann `cam_mul` 0.0/NaN/Inf enthalten.
  2. Die GUI speichert diesen Wert **ungeprüft** bei jedem RAW-Load:
     `Some(image.metadata.camera_white_balance)` in `load_bytes`
     (`crates/lumina-gui/src/lib.rs:1181`, Feldzuweisung `lib.rs:1190`,
     Felddeklaration `lib.rs:187`).
  3. Jede Render-/Export-Aufruf reicht ihn weiter
     (`render()`: `lib.rs:1328`; Export: `lib.rs:1669`).
  4. Der Core validiert sauber und **panic-t nicht**:
     `apply_recipe_with_scale_and_white_balance` verlangt finite und > 0 für
     alle vier Werte und liefert sonst
     `CoreError::InvalidAdjustment { name: "camera_white_balance", .. }`
     (`crates/lumina-core/src/lib.rs:478-489`), Display-Text
     „invalid camera_white_balance: must be finite and in … got …"
     (`crates/lumina-core/src/lib.rs:178-183`). Diese Meldung landet als rotes
     Banner im Header (`crates/lumina-gui/src/lib.rs:3521-3523` via
     `show_error`, `lib.rs:1456-1460`) — das ist die „Beschwerde wegen
     falschem Weißabgleich", die der Tester gesehen hat.
  5. **Der eigentliche Desaster-Mechanismus:** Nach diesem Fehler ist die App
     irreversibel in einem toten Zustand — `self.camera_white_balance` wird
     bei Fehler **nie zurückgesetzt oder sanitiert**; jeder weitere
     Render-Versuch (Slider `set_adjustment` → `render` über
     Render-Key-Invalidierung, Presets, Export) scheitert mit derselben
     Meldung. Das Bild erscheint geladen (`self.original` ist schon gesetzt,
     `lib.rs:1197`, bevor `render()` am Ende von `load_bytes` fehlschlägt,
     `lib.rs:1201`), aber Preview/Textur aktualisieren sich nie →
     „nichts funktioniert mehr".
  6. **Zum Crash selbst:** Ohne Stacktrace (siehe Logging-Befund unten) ist
     die exakte Crash-Stelle nicht beweisbar. Code-verifizierte Kandidaten im
     selben Prozess:
     - Native libraw-Decodes laufen synchron im UI-Thread — sowohl für die
       Hauptansicht (`load_path` → `decode_bytes`) als auch je Thumbnail
       (`generate_thumbnail`, `crates/lumina-gui/src/lib.rs:3266`). Ein
       Segfault/Abort im C-libraw beendet den Prozess sofort und
       meldungslos — äußerlich exakt „GUI crasht kurz nach der WB-Meldung".
     - Ungeschützter Indexzugriff `document.virtual_copies[0]`
       (`crates/lumina-gui/src/lib.rs:1536` in `load_path`) panic-t bei
       leerer Copy-Liste. Aktuell durch die Sidecar-Validierung abgeschirmt
       (`crates/lumina-sidecar/src/lib.rs:1410-1412` erzwingt ≥ 1 Kopie;
       Validierung läuft im Load-Pfad, `sidecar lib.rs:1320`) — aber ein
       fragiles Muster, das eine entfernte Invariante voraussetzt.
     - Integer-Overflow-Panics im Debug-Build (overflow-checks) in u32-
       Pixelarithmetik, z. B. `pick_white_balance_at`
       (`crates/lumina-gui/src/lib.rs:2099`) und `downscale_rgba`
       (`filmstrip.rs:75-76`) — erst bei extrem großen Bildern relevant,
       hier unwahrscheinlich.
- **Code-Referenzen (Kernkette):** `crates/lumina-raw/src/lib.rs:285,313,69`;
  `crates/lumina-gui/src/lib.rs:1181,1190,1328,1669,1201`;
  `crates/lumina-core/src/lib.rs:478-489,178-183`;
  `crates/lumina-gui/src/lib.rs:1456-1460,3521-3523`.
- **Auswirkung:** Für betroffene RAWs: Fehlerbanner statt Bild, danach
  dauerhaft unbedienbare Bearbeitungsansicht; im Testfall folgte ein
  Prozess-Crash, dessen genaue Ursache mangels Logging nicht rekonstruierbar
  ist. Zwei Probleme verschachteln sich: unsanitisierter Decoderwert
  (Datenfehler) + fehlender kontrollierter Fehlerpfad/Recovery (Behandlung).
- **Lösungsrichtung:**
  1. **Sanitisierung an der Quelle:** In `lumina-raw` (oder spätestens in
     `load_bytes`) `cam_mul` prüfen; bei NaN/Inf/≤ 0 (oder unplausiblen
     Werten) den As-Shot-Kontext zu `None` degradieren und dies als
     **sichtbare Warnung** melden („As-Shot-Weißabgleich ungültig, Rezept-WB
     bleibt nutzbar") — kein stiller Fallback, sondern expliziter Status.
  2. **Recovery statt Sackgasse:** Trifft der Core trotzdem
     `InvalidAdjustment("camera_white_balance")`, soll die GUI genau dieses
     Feld zurücksetzen (`camera_white_balance = None`), neu rendern und den
     Vorfall als Warnung protokollieren/anzeigen.
  3. Härtung: `document.virtual_copies[0]` → `.first()`-Muster mit
     kontrolliertem Fehlerpfad (`lib.rs:1536`).
  4. Unit-Tests mit `cam_mul = [0.0, …]`/NaN/Inf über den vollen Pfad
     (decode → load_bytes → render) sowie Property-Tests gegen die
     Core-Validierungsgrenzen.

### [HOCH] Keine strukturierte Logging-Grundlage — Crashes und Fehlerketten sind nicht rekonstruierbar
- **Betroffene Dateien:** gesamter Workspace, insbesondere
  `crates/lumina-gui/src/main.rs`, `crates/lumina-gui/src/lib.rs`,
  `crates/*/Cargo.toml`
- **Root-Cause (grep-verifiziert):** Im Workspace existiert **kein**
  Logging-Framework. Weder `log` noch `tracing`/`env_logger` sind in irgendeinem
  `Cargo.toml` deklariert (grep über `crates/*/Cargo.toml` + Workspace-Root:
  0 Treffer). In der GUI gibt es null Aufrufe von `log::`/`tracing::`/
  `warn!`/`error!`/`info!`/`debug!` — die einzigen Grep-Treffer sind
  Substring-Falschtreffer von `rfd::FileDialog::new()` („…alog::new…"). Auch
  `eprintln!`/`println!`/`dbg!` fehlen in der GUI komplett. Die CLI nutzt
  18× `println!`/`eprintln!` (`crates/lumina-cli/src/main.rs`) ohne Severity
  oder Struktur.
- Damit ist der einzige Laufzeit-Output der GUI der Statusstring
  (`self.status`, Header-Label `crates/lumina-gui/src/lib.rs:3519`) und das
  rote Error-Banner (`self.error`, `lib.rs:3521-3523`) — beide leben nur im
  RAM und sind **beim Crash weg**. Es existiert kein Panic-Hook, kein
  Datei-/Stderr-Sink, kein Ringbuffer. Gestartet per Doppelklick/Finder geht
  zudem stderr verloren.
- **Auswirkung:** Direkte Folge im Test: Der WB-Crash (Befund oben) ist
  weder in Ursache noch Reihenfolge rekonstruierbar — „was passierte VOR dem
  Crash" ist prinzipiell unbeantwortbar. Das blockiert jeden weiteren
  manuellen Testzyklus (Testbarkeit/Diagnose-Blocker, kein neues Feature,
  sondern Fundament).
- **Benötigte Severities:** `trace` (per-Frame/per-Entry Probes, Queue-Op),
  `debug` (Cache-Hit/Miss, Enqueue/Pop, Render-Key-Entscheidungen),
  `info` (Bild geladen, Export fertig, Directory-Scan),
  `warn` (Mask-Warnings, stale Auto-Tone, verworfene Queue-Tasks, degradierter
  As-Shot-WB), `error` (Render-/Sidecar-/Export-Fehler, Panics).
- **Lösungsrichtung (minimal-invasiv, Architektur-konform):**
  1. `log`-Crate als Facade-Dependency für alle Crates (keine nativen/FS-
     Abhängigkeiten — `lumina-core` bleibt plattformneutral; WASM-Pfad bleibt
     capability-getrennt, dort später `console_log`/Web-Sys-Hook gemäß
     Capability-Matrix).
  2. Initialisierung nativ in `crates/lumina-gui/src/main.rs`: `env_logger`
     (oder egui-kompatibler Logger) mit `RUST_LOG`-Envfilter, Default z. B.
     `info`, Target stderr + optionale Rotating-Datei neben dem
     Disk-Cache-Verzeichnis.
  3. Panic-Hook (`std::panic::set_hook`) in `main.rs`: schreibt Location +
     Message via `log::error` **plus** die letzten N Statusmeldungen der App
     (kleiner Ringbuffer in `LuminaApp`), damit „VOR dem Crash" belegbar ist.
  4. Erste Instrumentierung an den kritischen Pfaden:
     `generate_thumbnail`-Fehlerzweige (`crates/lumina-gui/src/lib.rs:3259-3278`),
     `load_bytes`/`render`-Fehler (`lib.rs:1201`, `lib.rs:1532,1555`),
     `pick_white_balance_at` (`lib.rs:2104`), Sidecar save/load
     (`save_sidecar` ab `lib.rs:1567`), Export (`lib.rs:1675-1676`).
  5. CLI analog auf `log` umstellen bzw. initialisieren (ersetzt die 18
     unstrukturierten `println!/eprintln!`-Aufrufe mittelfristig).

---

## Offene Punkte / Follow-ups

- Reproduktion des Crashes mit dem Original-Testordner nach Umsetzung von
  Logging + WB-Sanitisierung; dabei prüfen, ob der Crash mit dem libraw-Decode
  eines konkreten CR3 korreliert.
- Entscheidung (ADR-würdig): Thumbnail-Pipeline auf Worker-Threads umstellen
  (betrifft F-103-N1-Dokumentation: „background job" ist aktuell ein
  Main-Thread-Idle-Job — Doku und Implementierung weichen hier bereits
  sprachlich voneinander ab).

### [ANPASSUNG 2026-08-22] Trace-Logging für GUI-Interaktionen + nur RAW im Filmstrip
- **Änderung:** `log::trace!` für `set_adjustment`, `set_presence`, `reset_single_adjustment`, `toggle_before_after`, `set_module`, `apply_preset`, `pick_white_balance_at` ergänzt (`crates/lumina-gui/src/lib.rs`); Logger-Default von `Debug` auf `Trace` angehoben (`crates/lumina-gui/src/main.rs` via `logger::init_logging(LevelFilter::Trace)`), damit bei `RUST_LOG=trace` (bzw. Default-Trace) alle Slider-/Modul-/Preset-Interaktionen einzeln in stderr sichtbar sind. `RUST_LOG` überschreibt weiter.
- **Änderung:** `is_supported_image` auf reine RAW-Erkennung reduziert (`crates/lumina-gui/src/lib.rs:3408`); `png/jpg/jpeg/webp`-Zweig entfernt — Tester-Wunsch: Filmstrip/Thumbnails zeigen nur RAWs, JPGs werden ignoriert.

### [KRITISCH-FÜR-NÄCHSTE-SITZUNG] Preview-Skalierung falsch & Interaktion blockiert bei ungültigem WB (Screenshot 2026-08-22)
- **Beobachtung (Screenshot):** Im `Develop`-Modul ist oben ein roter Fehlerbanner `invalid camera_white_balance: must be finite and in 0.000…117549… got 0` zu sehen (sehr lange Zahl, Formatierungsbug). Darunter Histogramm, dann die zentrale Preview: statt „Bild als Ganzes" nur ein kleiner, verrauschter Blau-Ausschnitt (links ca. 55 % Breite, Höhe abgeschnitten), rechts das eingeklappte `Basic/Tone Curve/…`-Panel. Im `Filmstrip`unten sind nur 4 RAWs als Thumbs sichtbar. Tester meldet: „Bild als Ganzes“ nicht sichtbar; Skalierung wirkt nicht korrekt; keine Interaktion möglich (Slider ziehen, Module wechseln etc. zeigen keine Wirkung).
- **Erwartet:** Preview soll das geladene RAW **vollständig, aspekt-treue, fenster-füllend** zeigen (`fit` mit Letterboxing, resizable mit Fenstergröße, zentriert). Interaktionen (Slider, Preset, WB-Pipette, Before/After) sollen trotz Nebenfehlern weiter funktionieren oder kontrolliert warnen.
- **Tatsächlich:** Render-Pipeline bricht wegen `camera_white_balance = [0,0,0,0]` (aus `lumina-raw` `data.color.cam_mul`) in `crates/lumina-core/src/lib.rs:478-489` (`InvalidAdjustment`) mit `?` ab. In `lumina-gui/src/lib.rs:1206ff` wurde nach `load_bytes` bisher `self.render()` ohne `camera_white_balance`-Sanitisierung aufgerufen; der Fehler wird als roter Banner (`show_error`) angezeigt, aber `self.camera_white_balance` bleibt auf dem ungültigen Wert — **jeder weitere Render-Versuch (Slider → `set_adjustment` → `mark_dirty` → `render`) schlägt identisch fehl**. Damit ist die gesamte Develop-Interaktion blockiert.
- **Skalierungs-Teil:** Der `draw_preview`-Pfad (`lumina-gui/src/lib.rs:1800ff`) verwendet die aktuelle Texturgröße, aber bei konstantem Render-Fehler wird nie eine neue Preview-Textur gebaut; die angezeigte blaue Kachel ist die letzte (oder leere) Textur mit falscher `available_size`-/`max_size`-Berechnung — sie füllt das Panel nicht aus und skaliert nicht mit der Fenstergröße. Bedürftig ist ein `fit` (Aspect-Ratio erhalten, `ui.available_size` als Bounding-Box, `Image::fit_to_*`/`max_size` korrekt) und ein Fallback-Render ohne As-Shot-WB.
- **Formatierungs-Nebenbug:** Die Fehlermeldung druckt die validen Grenzen als extrem lange Dezimalkette (`0.000000…1175…`) statt kurzer, menschenlesbarer Range; Fix in `crates/lumina-core/src/lib.rs:178-183` Display-Format vorsehen.
- **Repro:** Beliebiges CR3 aus `2026-08-14-R3-Ried`, das `cam_mul=[0,0,0,0]` liefert, laden → roter WB-Banner, dann beliebigen Slider ziehen → `trace`-Log `set_adjustment …` erscheint, aber `render` bleibt im `error!`-Pfad, Bild ändert sich nicht.
- **Geplant für nächste Sitzung (nicht in dieser):** `cam_mul`-Sanitisierung (ungültig → `None` + `warn!` „As-Shot-WB verworfen“) und Recovery-Pfad (`camera_white_balance` bei `InvalidAdjustment` auf `None` zurücksetzen + neu rendern), `is_supported_image` bereits fix, Preview-`fit`-Fix (zentriert, aspekt-treu, Letterboxing) separat testen; Fehlertext kürzen. Slider-`trace`-Logs bereits vorhanden, damit Interaktionen dann exakt nachvollziehbar sind.

### [REVIEW-TODO] GUI-UI-State (Layout) wird nicht persistiert — bewusst OHNE aktuelles Bild
- **Betroffene Dateien:** `crates/lumina-gui/src/lib.rs` (App-State/Felder, `new`
  ~290), `crates/lumina-gui/src/main.rs` (`eframe::NativeOptions::default()`,
  kein `on_exit`).
- **Befund:** Verstellbare UI-Elemente — Panel-Breiten, aufgeklappt/zugeklappte
  Sektionen (`CollapsingHeader`), aktives Modul (Library/Develop/Export) — werden
  bei keinem Neustart gesichert. Verifiziert durch grep über die GUI-Crate nach
  `save|store|persist|on_exit|Memory|storage|Serialize|serde`: außer dem
  expliziten `SaveRecipe`→Sidecar-Pfad (`lib.rs:3108`, `save_sidecar` ab
  `lib.rs:1567`) und `serde_json::Value` (Parsing) gibt es **keinerlei**
  UI-State-Persistenz. `LuminaApp` leitet `Serialize` nicht ab; es existiert
  weder ein `on_exit`-Hook noch ein egui-`ctx.storage()`/`Persistence`-Mechanismus.
  Nach jedem Start gelten die egui-Defaults → der Nutzer muss Panel/Sektionen bei
  jedem Start neu arrangieren.
- **Auswirkung:** Komfort/UX, kein Datenverlustrisiko für das Bild selbst.
- **Scope-Klarstellung (Tester):** Das **aktuelle Bild** (Pfad, offene virtuelle
  Kopie, ungesicherter Recipe-Draft) ist von diesem Todo **bewusst
  ausgeklammert** — falls überhaupt, gehört das in eine separate, klar
  abgegrenzte Persistenz (z. B. Draft/Session-Datei, nicht das autoritative
  Sidecar; keine stillen Overwrites gemäß Agents.md). Dieses Todo betrifft NUR
  das reine Layout/UI-State.
- **Lösungsrichtung (konzeptionell, nicht implementiert):** UI-Layout-State über
  egui `ctx.storage()` / `Persistence` (Viewport) oder eine kleine portable
  `ui-state.json` sichern (keine absoluten Pfade); Initialisierung in `main.rs`
  bzw. im `LuminaApp`-Lifecycle; WASM-Pfad über `localStorage`-Adapter
  berücksichtigen.
