# LuminaRust Umsetzungsplan

Dieser Plan ist eine lebende Arbeitsliste. Er wird während der Implementierung
fortgeschrieben. Erledigte Aufgaben werden nach bestandener unabhängiger
Verifizierung und bestätigter Testabdeckung aus dieser Datei entfernt — es gibt
keine dauerhafte Liste abgehakter Aufgaben. Details zu Erledigtem liegen in den
Feature-Dokumenten und der Git-Historie.

## Gepinnte Entscheidungen und Absprachen

- **LIZ / Projektlizenz (F-073-R2, MVP-Release-Gate):** interim proprietär/
  kommerziell — bewusst kein `license`-Feld, keine `LICENSE`-Datei (Entscheidung
  des Projekteigentümers 2026-08-20). Sobald entschieden (MIT / Apache-2.0 /
  Dual / MPL-2.0): `license`-Felder + Root-`LICENSE` ergänzen. Fixtures-R1 ist
  geschlossen (uneingeschränkte Nutzungs-/Distributionsgewährung für
  LuminaRust, dokumentiert in `sample-data/raw/README.md` §4/§8). Lensfun
  (LGPL-3.0 dynamisch gelinkt, DB CC-BY-SA-3.0) ist in
  `THIRD-PARTY-NOTICES.md` dokumentiert und gilt unabhängig von der Wahl.
- **MVP-Grenze:** MVP = CLI + native Desktop-GUI inkl. nativem RAW. WASM/Browser
  ist ersatzlos gestrichen (2026-09-04, kein Post-MVP). Cache- und
  Mehrbild-Synchronisierung sind bewusst Post-MVP. Architektur bleibt nativ
  (einheitlicher `decode_bytes`/`RawMetadata`-Vertrag).
- **Release-Staffel (User-Entscheid 2026-09-03, MVP = 1.0; Aktualisierung
  2026-09-04):** IPTC-/Metadaten-Presets + -Verwaltung wurden per
  User-Entscheid 2026-09-04 von 1.5 nach **1.0** vorgezogen (LRPAR-G15-IPTC-
  S1…S8; Entscheid: `feature/decisions/LRPAR-G15-META-15.md`, Sidecar-Draft,
  pure-Rust-Bake-In ohne Runtime-Abhängigkeit, JPEG-only, GUI-Panel gleich
  mit). Damit: 1.5 = HDR/Panorama-Merge, Rote Augen, Auto-Upright;
  2.0 = Gesichtserkennung, KI-Denoise; 2.5 = KI-Culling; nie Ziel:
  Karten-Modul/GPS, Veröffentlichungsdienste. Lensfun-Vollausbau,
  Keywords/Filter/Sammlungen/Smart-Sammlungen sind MVP (1.0).
- **Sidecar-Schema Pre-MVP:** Schemaänderungen sind bis zum MVP Breaking Changes
  (keine Abwärtskompatibilitätspflicht, Altdateien müssen nicht lesbar
  bleiben); die Migrations-Maschinerie (`migrate_sidecar_file`, `.bak`-Backup,
  `migrate_json`) bleibt dauerhaft im Code und wird ab dem MVP für
  Release-Migrationen genutzt; „Tests für jede Migration" gilt ab dem MVP. Der
  v1→v2-Migrationspfad mit Tests bleibt als Muster umgesetzt. Pre-Alpha-
  Ergänzung: `schema_version` bleibt 1; der Loader lehnt inkompatible Sidecars
  laut ab (keine stille Normalisierung außer dem historischen v0→v1-Bump).
- **Dependency-Pins (kein Upgrade ohne ADR):** libraw-sys vendored
  `[patch.crates-io]` (macOS-C++-Fix), `ort =2.0.0-rc.13` (= neueste RC),
  LibRaw 0.22.2 + Ubuntu-24.04-lensfun-Distro-Pin (Determinismus;
  Upgrade-Pfad skizziert: neuer Image-Tag parallel → Golden-Rebaseline wegen
  CR3-Dimensionen → alter Tag erst dann entfernen).
- **CI-Gate `onnx-rt` (2026-09-02, CI-ONNX-RT):** `onnx-rt` wird jetzt im CI geprüft — Image liefert `libssl-dev` + `clang` für `openssl-sys`, `ci.yml` führt `cargo clippy -p lumina-onnx --features onnx-rt` und `cargo test -p lumina-onnx --features onnx-rt` aus. Nur **GPU bleibt hartes CI-Nein** (kein Metal auf Runnern, nur `cargo check -p lumina-gpu --features gpu`).
- **Toolchain:** CI fährt `@stable` → neue Clippy-Lints schlagen automatisch an
  (Beispiel `chunks_exact_to_as_chunks`). Lokal vor jedem Push `rustup update`
  + workspace-clippy laufen lassen.
- **Post-MVP Backlog (nicht MVP-blockierend):** F-019 (siehe Phase 2), Phase 9
  Index (F-064…F-067), MCP-Erweiterungen (siehe
  F-101-F1; die Metadaten-Tools/-Resource sind mit LRPAR-G15-IPTC ab 1.0
  normativ und gehören nicht in dieses Backlog),
  Lensfun-Ausbau (CA via Lensfun, automatische Profil-Erkennung per
  EXIF), Produktnamen-Entscheidung (`docs/naming-brainstorm.md`,
  Brainstorm-Phase offen bis MVP-Entscheidung). WASM-Browser (F-069…F-071)
  ist ersatzlos gestrichen, kein Backlog.

## Arbeitsregeln

- Vor jeder Umsetzung `Agents.md`, `feature/README.md` und das betroffene
  Feature-Dokument lesen.
- Wenn Code und SOLL-Zustand widersprechen, zuerst den Zielzustand klären und
  dokumentieren.
- Jede Aufgabe erhält bei Delegation eine Feature-ID, einen klaren Umfang und
  Abnahmekriterien.
- Der Build-Agent delegiert die Implementierung und anschließend die Prüfung an
  unterschiedliche Subagenten.
- Implementierungs-Agenten werden als `general`-Agenten delegiert (nicht als
  `build`-Agenten); Verifikation läuft immer über einen davon unabhängigen
  `general`-Agenten.
- Der unabhängige Verifizierungs-Agent muss Korrektheit und Testabdeckung
  bestätigen, bevor die Aufgabe aus dieser Datei entfernt wird.
- Eine fehlgeschlagene Verifizierung lässt die Aufgabe offen und erzeugt eine
  konkrete Folgeaufgabe.

## Offene Tasks — Legende der drei Blöcke und Releaseplan

Alle offenen Aufgaben sind in drei Blöcke gegliedert. Innerhalb jedes Blocks
gilt die Sortierung `[PRIO: hoch]` → `[PRIO: mittel]` → `[PRIO: niedrig]`;
die Priorisierung bewertet technische Tragweite/Risiko (kritische
Korrektheits-Bugs = hoch, Kosmetik/Doku = niedrig). Stand 2026-09-26:
21 offene Tasks (Checkbox-Zählung dieser Datei) — Block A: 16,
Block B: 1, Block C: 4.
Der Abschnitt `Releaseplan` ordnet jede Task-ID genau einer Version zu
(1.0 = MVP, 1.5, 2.0, 2.5, nie) — für Mensch und Maschine lesbar.

**User-Anweisung 2026-09-24:** Nach dem aktuellen Release-Block sollen
Future-Release-Tasks proaktiv vorgezogen werden, sobald keine aktuelle
Abhängigkeit und kein unüberwindbares Hardware-/User-Gate blockiert. Die
Releaseplan-Zuordnung und die bestehenden Abnahme-Gates bleiben dabei unverändert.

- **Block A — „Vor dem nächsten manuellen GUI/User-Test umsetzbar“:** alles,
  was ohne Rückfrage direkt umgesetzt werden kann und nicht von einem
  manuellen Test abhängt (Reihenfolge: PRIO hoch → mittel → niedrig).
  **Korrektur 2026-09-25:** Block A war zum Einführen nicht mehr
  vollständig ohne User-Interaktion abarbeitbar. Die fünf
  PRIO-hoch-GUI-Tasks (`R5-BRUSH-24`, `R5-MASKVIS-25`,
  `R5-DUST-23-FOLLOWUP`, `MASK-LOCAL-P0`, `MASK-LOCAL-P1.1`) waren
  implementiert und code-seitig verifiziert, aber alle durch
  **ein** undefiniertes Gate blockiert: einen Lauf auf echter Hardware
  gegen eine definierte Referenz-Umgebung. Dieses Gate ist inzwischen
  als `GOLDEN-REF-30`/`GOLDEN-FIXT-31`/`GOLDEN-BASELINE-32` explizit
  zerlegt; siehe [GUI-Test-Arbeit: Gate-Struktur](#gui-test-arbeit-gate-struktur).
  Die erneut Block-A-fähigen, reinen Code-/Doku-Aufgaben stehen
  unter **PRIO: mittel**/**niedrig**.
- **Block B — „Offene Rückfragen“:** Tasks, bei denen eine User-Entscheidung
  oder Klärung fehlt (Produkt-, Naming-, Lizenz-/Schema- oder Übernahme-
  Fragen). Dieser Block blockiert Block A nicht.
- **Block C — „Nach dem nächsten manuellen GUI-Test“:** Tasks, die erst nach
  dem nächsten manuellen GUI-Test sinnvoll/erforderlich sind (Verifikations-
  und Abschluss-Tasks, die auf Testergebnissen aufbauen).

## Releaseplan (User-Entscheide 2026-09-03; MVP = 1.0)

Maschinenlesbar: Die Tabelle `Version | Task-ID | Goal | Stichwort` ist die
verbindliche Zuordnung. Jede Task-ID kommt genau einmal vor; Ausführungsort
bleiben Block A/B/C. Tasks ohne expliziten User-Versionsentscheid gelten als
MVP-Annahme (1.0) und können per User-Entscheid umgebucht werden.
`fortlaufend` = ab 1.0 aktiv, gilt für alle Releases (CI-Strategie im Task).

| Version | Task-ID | Goal | Stichwort |
| --- | --- | --- | --- |
| 1.0 | NAMING-F1 | kein Goal | Produktname |
| 1.0 | R2-GUIMOD-04b | G-10 | GPU-Drossel-Entscheid |
| 1.0 | R2-GUIMOD-04c | G-10 | GPU-Histogramm |
| 1.0 | F-103-N6 | alle G | visueller User-Test |
| 1.0 | F-103-N6-GUI-COVERAGE-27 | alle G | vollständige manuelle GUI-/Persistenz-Checks |
| 1.0 | GUI-SRCACC-1 | F-042/F-010 | GUI-Quellaktionen + Cache-Identität |
| 1.0 | GPU-PARITY-HW-28 | alle G | Hardware-GPU-Parität |
| 1.0 | GPU-RENDER-DENOISE-19 | G-14 | Denoise-WGSL-Pass |
| 1.0 | GPU-RENDER-PREVIEW-19 | alle G | GPU-Preview |
| 1.0 | GPU-RENDER-EXPORT-19 | alle G | GPU-Export |
| 1.0 | GPU-RENDER-MASK-19 | alle G | Masken-Pixelpass |
| 1.0 | LRPAR-G15-STACK-15 | G-15 | Bilderstapel |
| 1.0 | LRPAR-G09-SORT-09 | G-09 | Sortierung + Custom-Sort |
| 1.0 | LRPAR-G03-MASKGROUP-03 | G-03 | Maskengruppen |
| 1.0 | R5-DUST-23-FOLLOWUP | G-04 | Spot-Heal-Followup |
| 1.0 | R5-BRUSH-24 | G-03 | Pinsel-Masken |
| 1.0 | R5-MASKVIS-25 | G-03 | Masken-Overlay |
| 1.0 | MASK-LOCAL-P0 | F-003/F-005/F-010/F-049 | lokale Mask-Adjustments |
| 1.0 | GOLDEN-REF-30 | alle G | Golden-Referenzplattform |
| 1.0 | GOLDEN-FIXT-31 | alle G | Golden-Fixture-Vertrag |
| 1.0 | GOLDEN-BASELINE-32 | alle G | eine geprüfte Golden-Baseline |
| 1.0 | LENSFUN-DB-33 | G-10 | portabler Lensfun-DB-Pfad |
| fortlaufend | CI-WATCH-1 | alle G | CI-Beobachtung |
| fortlaufend | CI-SHARD-26 | alle G | CI-Shard |
| fortlaufend | GUITEST-STRUCT-34 | alle G | Test-Suite-Struktur |
| fortlaufend | TEST-AUDIT-36 | alle G | Test-Redundanz-Audit (1×/Woche) |
| 1.0 | THUMB-HASH-PERF-35 | G-03/G-09 | UI-Thread-Volldatei-Hash pro Frame |
| 2.0 | LRPAR-G12-FACE-IMPL-20 | G-12 | Gesichtserkennung-Impl |
| 2.0 | LRPAR-G12-FACE-ADAPTER-25 | G-12 | Face-I/O-Adapter |
| 2.0 | LRPAR-G14-DENOISE-IMPL-20 | G-14 | KI-Denoise-Impl |
| 2.5 | LRPAR-G09-CULL-IMPL-25 | G-09 | KI-Culling-Impl |
| nie | — | G-12 | Karten-Modul/GPS (Nicht-Ziel) |
| nie | — | G-15 | Veröffentlichungsdienste (Nicht-Ziel) |

## GUI-Test-Arbeit: Gate-Struktur

**Ermittelt 2026-09-25 auf `ci-shard-gui-tests` @ `83c0a2f`, macOS,
Apple M5 Pro, Metal 4, LibRaw 0.22.2.** Grundlage: `cargo test -p
lumina-gui --all-targets` (grün), `cargo test -p lumina-gui --test
kittest_snapshots -- --ignored` (12/56 rot, reproduziert).

### Befund

Die offenen Tasks wirkten unabhängig (13 zu Beginn dieser Analyse, 19 nach
der Zerlegung). Sie sind es nicht: **8 von ihnen warten auf ein und
dasselbe undefinierte Gate** — einen Lauf auf echter Hardware gegen eine
*definierte* Referenz-Umgebung. Weil weder die Referenzumgebung noch der
Fixture-Vertrag je festgeschrieben wurde, erzeugt jeder „nur die Goldens
einmal refreshen"-Schritt dasselbe Blockade-Muster erneut.

**Zahlenbasis, korrigiert 2026-09-25 (die alte Angabe „12 von 56" war
eine Verwechslung zweier Nenner):** committet sind **62** Golden-PNGs
über 6 Golden-Targets plus Lib-Target. `kittest_snapshots.rs` enthält
**56 `#[test]`s**, davon **46** mit `harness.snapshot(...)` — die übrigen 10
assertieren etwas anderes. Nachgemessen wurde bisher nur dieses eine
Target: **12 von 56 Tests dort schlagen fehl.** Die Fehlerzahl für den
**vollständigen** 62er-Korpus ist damit **offen** und wird nach
`GOLDEN-FIXT-31` einmalig gemessen (`GOLDEN-BASELINE-32`). Die Corpus-
Zählung wurde gegen die Call-Sites geprüft: **keine verwaisten Goldens**
(acht `parity_paths_*` werden per `format!` erzeugt,
`mask_management_controls` aus `src/tests/brush_management.rs`, also aus
dem Lib-Target).

Vier konkrete Defekte, gemessen:

1. **Die Goldens sind nicht reproduzierbar.** Auf dem Target
   `kittest_snapshots` schlagen 12/56 Tests fehl, nicht die 3 in
   `MASK-LOCAL-P0`/`P1.1` dokumentierten. Ein Blatt-Vergleich
   (`library_compare`) zeigt Layout- und **Farb**-Differenzen, nicht
   nur Text-Antialiasing; `develop_section_rating` und
   `library_loupe`/`library_survey` sind ebenfalls betroffen. Ursache
   ist mit hoher Wahrscheinlichkeit fehlende Plattform-Pins (wgpu-
   Backend, Font-Stack, Skalierung, LibRaw-Patchstand) — **nicht
   abschließend bewiesen**, siehe Abnahme von `GOLDEN-REF-30`.
2. **Die Doku über das Ausmaß der Abweichung ist bereits falsch.**
   Commit `83c0a2f` und die Task-Stände nennen 3 betroffene Goldens;
   gemessen sind es 12. Die recenten Matrix-Commits (`2f5561d`,
   `46e2917`, `a4eb230`) haben 7 Goldens erneuert — 3 davon
   (`develop_overlay_mask`, `develop_overlay_pins`,
   `develop_section_masking`) schlagen hier *trotzdem* weiterhin fehl.
   Werkszeug-Status im Plan war also nicht verlässlich.
3. **Viele Goldens sichern keinen Render-Zustand, sondern einen
   Fehler-Zustand.** `crates/lumina-gui/tests/fixtures/library_views/
   a01.arw` ist 18 Byte groß und enthält wörtlich
   `lumina-raw-fixture` — keine RAW-Datei. Der Decoder schlägt fehl,
   das committed Golden `library_compare.png` zeigt den roten Banner
   „LibRaw opening input failed (-100009)" mit Farbklötzen als
   Platzhalter. Diese Bilder können echte Bild-Render-Regressionen
   prinzipiell nicht aufdecken; sie belegen nur das Zustandbild eines
   Fehlers.
4. **Ein als „vorbestehend" abgehakter Testbefund — die Diagnose war
   falsch, der Befund nicht.** `Agents.md` verbietet, rote Tests als
   „vorbestehend" abzuhaken. Der Plan nannte 10 rote
   Lensfun-Datenbank-Tests mit der Begründung „panic auf
   `LensfunDb::load_system()`, weil `/usr/share/lensfun` fehlt".
   **Nachmessung 2026-09-25: auf dieser Maschine sind die Tests grün
   (0 rot).** `load_system()` ruft `lf_db_load()` auf, das den in der
   Homebrew-dylib kompilierten `LENSFUN_DATADIR` nutzt — korrekt
   `/opt/homebrew/...`; der Kommentar in `lib.rs` benennt den macOS-Pfad
   sogar ausdrücklich. Die rote Meldung stammt aus einer *anderen*
   Laufumgebung (eigenes Target `target/p11-remediate` + gepinnte
   `env.sh`), nicht aus dem Code. Der echte, unabhängig davon
   bestätigte Defekt: die Auflösung war nicht portabel, nicht
   dokumentiert, nicht testbar und fehlschlug über ein **stilles
   `None`** (verwechselbar mit „kein passendes Profil", also ein stiller
   byte-identischer No-op — Verstoß gegen „keine stillen Fallbacks").
   → `LENSFUN-DB-33`. **Merksatz für künftige Läufe:** keinen
   Root-Cause aus einer Fehlermeldung in einem Task-Text ableiten, ohne
   den Fehler selbst zu reproduzieren.

Zusätzlich, ohne eigenen Task: In CI sind alle 6 kittest-Binaries zu
100 % `#[ignore]`d und laufen nie, werden aber im 3:11-Shard
`rust-test-gui` weiterhin kompiliert und gelinkt.

### Auflösung

```
Welle 0  Gate überhaupt erst herstellbar   (Block A, rein, kein Hardwarebedarf)
  GOLDEN-REF-30      → Referenzplattform + Fingerabdruck + Pin im Repo
  GOLDEN-FIXT-31     → Fixture-Vertrag, Fehler-Zustand ≠ Render-Gate
  LENSFUN-DB-33      → 10 rote Tests ehrlich auflösen
  GUITEST-STRUCT-34  → 91 flache Testdateien thematisch gruppieren
        │
Welle 1  genau eine geprüfte Baseline       (Block A nach Welle 0)
  GOLDEN-BASELINE-32 → EIN Refresh, jede Änderung visuell begründet
        │
        ├──► MASK-LOCAL-P0         (offener Posten 1 entfällt)
        └──► MASK-LOCAL-P1.1       (offener Posten 1 entfällt)

Welle 2  Hardware-Lauf                   (bereits auf M5/Metal möglich!)
  GPU-PARITY-HW-28   → Primär-Gate der Welle
        ├──► R5-BRUSH-24            (offen: Metal/Vulkan/DPI/Pointer)
        ├──► R5-MASKVIS-25          (dito)
        └──► R5-DUST-23-FOLLOWUP    (offen: echter GPU-Nachweis)

Welle 3  manueller GUI-Lauf             (Block C, unverändert)
  F-103-N6
        ├──► F-103-N6-GUI-COVERAGE-27
        ├──► R2-GUIMOD-04b / 04c    (brauchen 04a-Zahlen)
        └──► DPI-/Pointer-Anteile aus R5-BRUSH-24/-MASKVIS-25
```

**Der Hebel ist Welle 0, nicht Welle 2.** Welle 2 ist nur *ein*
`cargo test`-Kommando auf dieser Maschine (Metal 4 vorhanden,
Headless-wgpu-Adapter nachweislich funktionsfähig). Dass sie als
offen geführt wird, ist ein Dokumentations-, kein Hardwareproblem.
Welle 0 verwandelt außerdem jede künftige Baseline-Erzeugung aus
einem ungeprüften `UPDATE_SNAPSHOTS=1` in einen nachvollziehbaren,
auf eine gepinnte Umgebung beschränkten Schritt.

## Phase 3–5: Renderpipeline, RAW, Auto-Tone

Keine offenen Punkte. SOLL: `feature/architecture/pipeline.md` und
`feature/quality/performance-benchmarks.md`.

## Block A – „Vor dem nächsten manuellen GUI/User-Test umsetzbar“

**Welle 0 (dieser Block) ist vollständig ohne User-Interaktion und ohne
Hardware-Gate abarbeitbar.** Sie stellt die Gates her, an denen die
PRIO-hoch-Tasks dieses Blocks hängen; die Aufgaben selbst werden erst
in Welle 1/2 (Abschnitt [GUI-Test-Arbeit](#gui-test-arbeit-gate-struktur))
abgeschlossen. Reihenfolge: PRIO hoch → mittel → niedrig.

### PRIO: hoch — Welle 0: Gates herstellen

Ziel dieser Welle: die Voraussetzungen schaffen, unter denen die bereits
implementierten GUI-Tasks dieses Blocks überhaupt abgeschlossen werden können.
Reihenfolge und Abhängigkeiten: Abschnitt
[GUI-Test-Arbeit: Gate-Struktur](#gui-test-arbeit-gate-struktur).
SOLL **vor** Code, je Task mit eigener Feature-ID.

- [ ] **[PRIO: hoch] GOLDEN-REF-30 (Release 1.0, Doku-first; Vorbedingung für `GOLDEN-BASELINE-32` — Verifikations-Runde 2 läuft 2026-09-25)** Die 56 `kittest_snapshots`-Goldens sind auf keiner Maschine reproduzierbar: 12/56 schlagen auf dieser M5/Metal-Maschine fehl, laut Task-Doku nur 3 — die Doku ist also bereits falsch. Nirgends ist eine Referenzumgebung festgeschrieben. **User-Entscheid 2026-09-25:** Referenz ist **macOS/Metal** (dieser Maschinenklasse), nicht Linux/CI — CI-Runner haben keinen GPU-Zugriff, die Goldens können dort nie verifiziert werden und bleiben ein **lokales** Gate. **SOLL zuerst:** die verbindliche Golden-Referenzplattform beschreiben (OS, wgpu-Backend/Adapter, Font-Stack und -auflösung, Skalierung, LibRaw-Version, Fixture-Set, exakte Regenerations-CLI) als neuer Abschnitt in `feature/quality/`; `feature/platform/capability-matrix.md` bleibt `LENSFUN-DB-33` vorbehalten. Danach `scripts/golden_ref.sh`: druckt einen Fingerabdruck (Toolchain, wgpu-Backend/Adapter, Fonts, Skalierung, LibRaw) und verweigert `UPDATE_SNAPSHOTS=1` bei Abweichung vom im Repo gepinnten Fingerabdruck — mit `record --confirm "<Diff-Begründung>"` als bewusstem, begründetem Neu-Pin (es gibt **kein** `--force-record`; `record` *ist* der bewusste Re-Pin, druckt den old→new-Diff und verlangt ≥ 20 Zeichen Grund). Zusätzlich Pre-Commit-Gate in `.githooks/pre-commit`: wer `tests/snapshots/*.png` staged, muss `scripts/golden_ref.lock` **im selben Commit** mit geänderter `# Grund:`-Zeile mitstagen. **Ausdrückliche Grenze (SOLL §11.1):** ein nacktes `UPDATE_SNAPSHOTS=1 cargo test -p lumina-gui -- --ignored` umgeht den Mechanismus vollständig — es gibt keinen `.cargo`-Alias, kein Makefile/justfile, keinen Wrapper. Das Skript sperrt **seine eigenen** `check`/`gate`-Läufe, nicht die Umgebungsvariable. Die verbleibenden Schranken sind das Pre-Commit-Gate und die menschliche Diff-Prüfung. **Verifikations-Runde 1: NICHT BESTANDEN.** Der Comparator selbst wurde lobend bestätigt (alle 20 Werte unabhängig nachgerechnet, alle 21 Schlüssel einzeln perturbiert, Digest-Tamper-Evidenz holds, kein `rm`/Netzwerk/Schreibzugriff außerhalb des Repo, `UPDATE_SNAPSHOTS`-Wahrheitswert eine nachgewiesene Obermenge des egui_kittest-0.36.2-Trigger-Satzes). Beanstandet wurden drei Punkte: **F-2 der Kern — der Wächter war nicht tragend** (nichts fängt ein nacktes `UPDATE_SNAPSHOTS=1 cargo test` ab), während SOLL/README „erzwingt"/„sperrt `UPDATE_SNAPSHOTS`"/„nie durch Editieren des Locks zu lösen" behaupteten — letzteres war widerlegbar, weil der Lock unsignierter Text ist und `lock_value` per `grep | head -n 1` arbeitet; **F-1** `record` warnte als einziger schreibender Unterbefehl *nicht* bei abweichendem Lock-Pfad, im Widerspruch zur SOLL; **F-3** der Pin entstand gegen einen schmutzigen Worktree (9 `absent`, 1 untracked) und wird deshalb einmal legitim fehlschlagen, wenn `GOLDEN-FIXT-31` landet. Dazu F-4 bis F-7 und Nits (toter `?x?`-Glob, Newline-Injektion via `--confirm`, nicht erzwungene Lock-Invarianten, 1-Zeichen-Grund, kein Diff). **Runde 2 (Implementierungs-Agent):** Pre-Commit-Gate ergänzt — dabei wurde ein **echter Bug** gefunden, der das Gate wirkungslos gemacht hätte: auf git 2.54 liefert `git diff --cached --name-only --diff-filter=ACDR -- <glob>` für eine **modifizierte** Datei nichts, also genau für den Fall, den das Gate fangen soll. Solverkanten-Formular jetzt erzwungen, `record` druckt den Pin-Diff, Gründe ≥ 20 Zeichen, `UPDATE_SNAPSHOTS` aus dem gated Kommando heraus. **Bewusst offen:** F-3 (einmaliger legitimer Mismatch beim Landen von `GOLDEN-FIXT-31`, in `GOLDEN-BASELINE-32(e)` sequenziert) und F-8 (Lock noch untracked, vom Build-Agent zu committen).
- [ ] **[PRIO: hoch] GOLDEN-FIXT-31 (Release 1.0, Doku-first; Vorbedingung für `GOLDEN-BASELINE-32`)** Die committeten RAW-Fixtures in `crates/lumina-gui/tests/fixtures/` sind synthetisch: 11 `.arw`-Dateien (u. a. `library_views/{a01,a02,b01}.arw`, `library_badges/**`, `library_rated/**`, `library_stack/images/**`), die aus 18 Byte wörtlichem Text `lumina-raw-fixture` bestehen. Der Decode schlägt fehl, und das committed Golden `library_compare.png` zeigt den roten Banner „LibRaw opening input failed (-100009)" mit Farbklötzen als Platzhalter. Ein solches Bild kann **keine** Bild-Render-Regression aufdecken. Dazu kommt: `LuminaApp::sample_image_png()` ist ein **4×3-Pixel**-Synthetik-PNG, das auf die Previewgröße hochskaliert wird — die Develop-Goldens sichern also UI-Chrome/Layout, keine fotografische Bildqualität. **User-Entscheid 2026-09-25:** (a) die beiden echten, lizenzierten RAW-Fixtures `sample-data/raw/aircraft-portrait.cr3` und `aircraft-landscape.cr3` (Canon EOS R1, 6032×4024, Lizenz/Provenienz in `sample-data/raw/README.md` §4/§8) werden als RAW-Fixtures verwendet; (b) die synthetischen `.arw`-Dateien und andere synthetische RAW-Fixtures werden **entfernt**; (c) das kleine synthetische PNG bleibt für **Smoke-/Layout-Tests** erhalten und ist als genau das klassifiziert. **SOLL zuerst:** Fixture-Vertrag in `feature/quality/` — jede der 56 Golden-Dateien wird als *Render-Invariante* (echter Bildinhalt, echte RAW-/Bildpipeline) oder *Chrome-/Layout-Invariante* (UI-Rahmen, Smoke) klassifiziert; eine Chrome-Invariante darf keine Bildpipeline-Regression belegen. **Abnahme:** keine committete `.arw`-Datei enthält mehr den Platzhaltertext; die Library-Goldens rendern echte CR3-Thumbnails statt eines Fehlerbanners; das Klassifikations-Inventar aller 56 liegt versioniert im Repo; ein Test schlägt an, wenn eine Chrome-Invariante als Beleg für eine Bildpipeline-Regression herangezogen wird; das Produktprinzip „keine stillen Fallbacks" bleibt unberührt. **Abgrenzung:** Test-/Fixture-/Doku-Arbeit in `lumina-gui`, kein Produktcode, keine Baseline-Erzeugung. **Offene Messfrage (vor Abschluss zu klären):** ein 12-MB-CR3-Dekod pro Golden-Test ist ein realer Performance-Risikofaktor — der Implementierungs-Agent misst die Laufzeit und berichtet; falls inakzeptabel, Vorschlag (downgescaletes echtes RAW-Derivat) zurückmelden statt eigenmächtig eine neue Fixture-Formatierung zu erfinden.
- [ ] **[PRIO: hoch] LENSFUN-DB-33 (Release 1.0, keine Vorbedingung — Verifikations-Runde 2 läuft 2026-09-25)** Die Lensfun-DB-Auflösung war nicht portabel, nicht dokumentiert, nicht testbar und **fehlschlug still**: `load_system() -> Option` machte eine fehlende Datenbank von „kein passendes Profil" ununterscheidbar, also ein stiller byte-identischer No-op (Verstoß gegen „keine stillen Fallbacks"). **SOLL zuerst:** Auflösungsreihenfolge in `feature/platform/capability-matrix.md` — Umgebungsvariable → kompiliertes `LENSFUN_DATADIR` (pkg-config, target-bewusst) → plattformabhängiger Default → **lauter, benannter** Fehler. **Stand 2026-09-25 (Implementierungs-Agent + Build-Agent-Nachprüfung):** `db_path.rs` (reine Auflösungslogik + `SystemDbError`), `system_load.rs` (FFI-Laden), hermetische Präzedenztests + Real-DB-/End-to-End-Override-Tests; `build.rs` emittiert `LUMINA_LENSFUN_COMPILED_DATADIR`; `lib.rs` 1789 → **1747** Zeilen (unter Baseline); `lumina-core` auf `resolve_system()` umgestellt (2 Call-Sites, 3503 ≤ 3505). Tests 27 → **40**, keine Assertion abgeschwächt, alle Distortions-/Vignettierungs-Zahlenvergleiche byte-identisch. **Premise-Korrektur:** Die ursprüngliche Aufgabenstellung („10 rote Tests", „hart verdrahteter Linux-Pfad") war **falsch** — `load_system()` delegierte an `lf_db_load()` mit dem in der Homebrew-dylib kompilierten Datadir und war auf macOS grün; der Kommentar in `lib.rs` benennt `/opt/homebrew/share/lensfun/version_1` ausdrücklich. Die rote Meldung stammte aus einer anderen Laufumgebung, nicht aus dem Code. Der behobene Defekt (Stille-Auflösung, fehlender Override, fehlende Tests, undokumentierter Vertrag) war dennoch real und unabhängig bestätigt. **Linux-Container-Pfad** bleibt gültig: `cfg!(target_os = "macos")` ist dort false, kompilierter Datadir und Default deduplizieren auf `/usr/share/lensfun` — empirisch über einen containerförmigen Temp-Baum geprüft (echter Nikon-D40-Lookup erfolgreich), nicht nur argumentiert. **Verifikations-Runde 1: NICHT BESTANDEN** (3 Befunde, davon 1 Regression). Der Verifizierungs-Agent bewies mit einem C-Programm gegen dieselbe dylib, dass die neue per-Datei-Ladung die **User-DB** (`~/.local/share/lensfun/*.xml`, von `lfDatabase::Load()` upstream **immer** mitgemergt) stillschweigend fallen ließ — 949 Kameras via `lf_db_load` gegen „user camera nicht gefunden" über den neuen Pfad, Kontrolle 948/948. Zusätzlich: `eprintln!` statt Log-Level (in der GUI unsichtbar, pro Render erneut), falsche `unsafe { set_var }`-Begründung, `db_lock` entwertete den Concurrency-Regressionstest, tote `MissReason`-Variante, falsche SOLL-Aussage zur Quellen-Priorität, drei §7.5-Aussagen ohne Testanker. **Runde 2 (Implementierungs-Agent):** Der User-DB-Merge aus F1 ist echt behoben (C-Probe: User-Kamera und Nikon D40 werden jetzt gefunden). **Die behauptete „volle Parität" ist jedoch widerlegt** — der Agent hatte „superset/bitgleich" erneut behauptet. **Verifikations-Runde 2: NICHT BESTANDEN**, weil die Paritäts-Remediierung selbst einen **neuen, schlimmeren stillen Verlust** einbaute: `FsProbe::newest_mtime` nimmt den **Datei-mtime**, upstream `_lf_read_database_timestamp` liest den **Inhalt von `timestamp.txt`**. Folge: ein `updates/version_1` **ohne** `timestamp.txt` (upstream Timestamp 0, also *nie* ein Update) gewinnt im neuen Code zuverlässig und **verdrängt die komplette System-Datenbank** — C-Probe: 55 Systemdateien weg, `Nikon D40 → none`, während upstream 949 Kameras lädt. **Verschärfend: der Test-Fixture `probe_fixture.rs` modelliert Zeit selbst als `newest_mtime`, die Tests kodieren also die falsche Semantik und sind selbstbestätigend.** Auf dieser Maschine und im CI ist der Defekt **latent** (kein `~/.local/share/lensfun/updates/`). Weitere Befunde: `db_path::Source` und `db_layers::LayerOrigin` widersprechen sich (Operator pinnt `/custom`, geladen wird `/var/lib/lensfun-updates/version_1` — **stillschweigend**, im Widerspruch zur SOLL-Regel „kein stilles Weiterfallen"); SOLL behauptet weiter „Obermenge … und identisch"; Tie-Break invertiert; `load_layers` in Tests **ohne** den globalen Lock (latenter CI-Crash); `Unreadable` ohne erzeugenden Test; `user_data_dir_from` weicht von glib 2.88 ab. F3/F4/F5/F7 und die Nits sind echt behoben. Tests 40 → **60**; `lib.rs` 1789 → **1745**. **Offen:** die GUI-/CLI-Caller nutzen weiterhin `load_system() -> Option` und machen daraus eine stille Identitätskorrektur — `Diagnostics` ist damit **toter API-Bestand**.
- [ ] **[PRIO: hoch] GOLDEN-BASELINE-32 (Release 1.0; blockiert durch `GOLDEN-REF-30` + `GOLDEN-FIXT-31`)** Nach beiden Vorbedingungen wird die Baseline **genau einmal** und vollständig geprüft erneuert: den **vollen** 62er-Korpus (6 Golden-Targets + Lib-Target) auf der gepinnten Referenzplattform neu schreiben und danach **jede** abweichende Änderung einzeln begründen (alt-vs-neu-Vergleich) und dokumentieren. **Abnahme:** (a) die Fehlerzahl des **Gesamt**-Korpus ist **vor** dem Refresh einmal gemessen und im Task-Stand festgehalten (bisher nur `kittest_snapshots`: 12/56); (b) nach dem Refresh ist **jedes** der 6 Golden-Targets 0 rot, kein Test `FAILED`; (c) jede geänderte Golden-Datei ist einzeln begründet — als *beabsichtigte* UI-Änderung (Pins/Masken-Overlay/Local-WB-Regler) oder als *Plattform-Drift* (dann nicht committen); (d) **kein** Golden wird stillschweigend aktualisiert; (e) `sh scripts/golden_ref.sh record --confirm "<Begründung>"` wird **einmal abschließend** ausgeführt, nachdem Fixtures und Goldens final sind, damit `fixtures.digest`/`goldens.digest` den Endzustand pinnen; (f) `sh scripts/golden_ref.sh check` ist danach grün. **Abgrenzung:** nimmt keine Produktänderung vor und löst `MASK-LOCAL-P0`/`P1.1` nicht selbst ab — es räumt deren offenen Posten 1 („3 Goldens brauchen `UPDATE_SNAPSHOTS`") weg, worauf die beiden Tasks mit eigener unabhängiger Verifikation folgen.

### PRIO: hoch — LR-Parität (Welle 1/2; Gate-Abhängigkeit siehe oben)

**LR-Parität aus `.goal/Goal.md` (Batch 1 + User-Featureliste, Stand 2026-09-03, 16 Goals, Aggregat ~39,1 %)**

Ziel: UI zum Verwechseln ähnlich zu Lightroom Classic; jedes Feature auf
CLI- **und** GUI-Ebene getestet. Quelle: `.goal/Goal.md` G-01…G-16, Beleg:
`.goal/lighroom screenshots batch 1/Content.md`. Umsetzung je Task via
`general`-Implementierungs-Agent + unabhängiger `general`-Verifizierungs-Agent
(Regel oben). LR-Parität-Batch aus 2026-09-04 (teils erledigt s. Git-Historie; Rest s. Releaseplan/Blöcke).

- [x] **[PRIO: hoch] GUI-SRCACC-1 (Release 1.0, Doku-first 2026-09-24)** Persistierte Repair-Regionen werden in der GUI derzeit als leere Source-Action-Liste gerendert: Preview und Export können damit vom CLI/Core-Pfad abweichen, während Cache-Identität und Stand-ins keinen verlässlichen Artefaktschutz bieten. Ein gemeinsamer strikter GUI-Resolver muss Bundle/Referenz/Pfad/Checksumme/u16-Plane/RGBA8-Replacement/Quellauflösung vor jeder Pixelmutation prüfen und gültige Aktionen durch `prepare_source_base`/`RenderContext` in Preview und Export sowie durch Navigator/Neighbor/Thumbnail führen; Missing/stale/corrupt/invalid bleibt laut. Abnahme: `crates/lumina-gui/src/tests/source_actions.rs` plus Render-Cache-/Export-Paritätstests für gültige, fehlende, checksum-abweichende, ungültige und dimensioniert falsche Artefakte; Aktions-/Artefaktänderung invalidiert Preview- und Exportcache; GUI-Pixel/Export byte-identisch zum CLI/Core; keine stillen Recipe-only-/Originalbild-Stand-ins. Targets bleiben LibRaw 0.22.2/getrennt. **Stand 2026-09-24:** SOLL ergänzt; unabhängige Verifikation PASS; Implementierung als `9a24fd2` committed/gepusht, CI/Matrix grün.
- [x] **[PRIO: hoch] CI-SHARD-26 (User-Order 2026-09-20)** CI-Wandzeit ~5:00 → ~3:00: `rust`-Job in `ci.yml` splitten (`rust-fast`: fmt+check+alle Clippy+Ratchet; `rust-test-gui`: nur `lumina-gui --all-targets` = Long Pole 2:35; `rust-test-rest`: Rest + libraw-ABI + zdata + lensfun + onnx-rt), Cargo-Cache über alle Jobs teilen. Analyse in Konversation 2026-09-20 (Run 35531727105). Abnahme: CI grün + Wandzeit-Beleg + unabhängige Verifizierung. **Stand 2026-09-23:** Shard-Implementierung, Cache-/Gate-Prüfung, Actionlint und unabhängige statische Verifizierung eingebaut; doppelte zdata-/Dokument-Gates bereinigt und GPU-Laufzeittests aus dem headless Rest-Shard ausgeschlossen. GitHub-Lauf `35908233116` grün (3:45 Gesamt-Laufzeit; GUI 3:11, Rest 3:25); manueller Matrix-Nightly `35905160512` grün (10:07). Abnahme erfüllt; GPU-Parität bleibt separat als `GPU-PARITY-HW-28` offen. **CI-Watch 2026-09-24:** Run `36052595716` für Cull-Commit `a7a915b` grün (3:47 Gesamtzeit).
- [ ] **[PRIO: hoch] R5-DUST-23-FOLLOWUP (User-Order 2026-09-20; Remediierung 2026-09-23)** Dust-Erweiterung: Anzeige nur bei Auswahl (Ausgewähltes wie Maske), Entfernungen bearbeitbar inkl. Neu-Generierung, Typ Generate/KI-generiert (Clone nie verwendet). **Stand nach R5-DUST-23-FOLLOWUP-Remediierung:** CLI-Kontradiktionsmatrix mit atomarem No-Write-Failure, eindeutige Spot-IDs/compat-ID-Materialisierung für typed-only generative Einträge, Referenz-basierter missing/stale/corrupt-Status in GUI+CLI und 1024×720-Selected-Detail-Anordnung sind implementiert und headless abgedeckt; die unabhängige Funktionsverifizierung ist bestanden. Der Task bleibt wegen des separaten Hardware-Gates offen. **Konkreter manueller GPU-Nachweis bleibt erforderlich:** auf echter Hardware (Metal/Vulkan, nicht llvmpipe/Software) `RUST_LOG=trace cargo test -p lumina-gpu --features gpu --test parity` mit genau einer App-Instanz und Log-Redirect ausführen; GUI-R5 darf bis dahin nicht als vollständig abgeschlossen gelten.
- [ ] **[PRIO: hoch] R5-BRUSH-24 (User-Order 2026-09-20, User-Urteil „großer Fail", nach Sort-Mini-Welle)** Pinsel-Masken auf Lightroom-Niveau: Größe/Weichheit/Fluss einstellbar (Slider + `[`/`]`-Shortcuts als Alias), Kreis-Cursor mit live Größe am Zeiger, mehrere Masken pro Bild anlegbar + einzeln wählbar (Pin-Liste klickbar), dazu vollständige Maskenverwaltung (Liste aller Masken mit Sichtbarkeits-Toggle, Umbenennen, Löschen, Reihenfolge, Duplizieren — alles klickbare Buttons). **Stand 2026-09-24:** Code-Level-Funktionsverifizierung bestanden: CPU/no-GPU, Persistenz-Atomizität, kumulative Live-Plane/Overlay, 110-Action-Audit und 1024×720-Struktur/Golden sind grün. Der Task bleibt bis zum echten Metal/Vulkan-/DPI-/Pointer-Nachweis offen; der Native-Kittest-Golden wird separat mit `--ignored` geprüft.
- [ ] **[PRIO: hoch] R5-MASKVIS-25 (User-Order 2026-09-20, präzisiert, nach Sort-Mini-Welle)** Masken-Overlay nur sichtbar, wenn die Masken-Ansicht geöffnet ist; togglebar: nur Pins für alle Masken vs. volles Overlay für die aktuell ausgewählten Maske. Dazu Bildbereich vergrößern (Seiten-Panels ausblendbar für maximale Preview). **Stand 2026-09-24:** State-Machine, CPU/GPU-Gates, Overlay-Farbparität, 110-Action-Audit und R5-Goldens unabhängig verifiziert; der Task bleibt bis zum echten Metal/Vulkan-/DPI-/Pointer-Nachweis offen.
- [ ] **[PRIO: hoch] MASK-LOCAL-P0 (User-Order 2026-09-25; SOLL zuerst, Release 1.0)** Füge lokale Mask-Adjustments als versioniertes typed `MaskLayer.local_adjustments` hinzu und migriere Legacy-`adjustment_*`-Extras verlustfrei bzw. mit lautem Konflikt-/Unknown-/Range-Fehler. Lokale Layer werden sequenziell auf dem global adjustierten Ergebnis angewendet; globales WB bleibt absolute-only mit explizitem Reset to As Shot; überlappende Layer folgen der persistierten Listenreihenfolge. P0: CPU-Compositor nach Global-/Geometry-Pfad mit fractional Alpha, Alpha-0-Byte-Identität, Alpha-1-Unmasked-Rezept, strikter atomarer Prevalidierung sowie korrekter Full-Frame/ROI/Crop/Aspect/90°-Rotation/Mirror-Ausrichtung ohne falschen Resize-Fallback. GUI/CLI/Draft/Navigator/Neighbor/Thumbnail müssen denselben Kontext auflösen oder sichtbar verweigern. Nur Exposure/Contrast/Highlights/Shadows; lokale WB/Tone/Color bleiben P1. History/Reset/Previous erhält vollständige additive Layer-Snapshots. **Abnahme:** exakte CPU-Goldens für Alpha 0/partial/1, Outside-Mask, Reihenfolge, ROI/Crop/Aspect/Rotation/Mirror, ungültige Schemata, Cache-Identität, Draft-Refusal, CLI/GUI-Parität und Regressionen; LibRaw 0.22.2/getrenntes Target, fmt, Workspace-Clippy `-D warnings`, File-Size-Ratchet und Diff-Gate. Die Parent-Gates `GPU-RENDER-MASK-19`, `GPU-PARITY-HW-28`, `R5-BRUSH-24` und `R5-MASKVIS-25` bleiben offen; CPU-P0 schließt sie nicht. **Blocker-Auflösung 2026-09-25:** Der einzige verbleibende offene Posten (1) — die drei Goldens `develop_basic`, `develop_sections_expanded`, `develop_section_masking` — ist als `GOLDEN-BASELINE-32` ausgelagert und **nicht** durch ein freies `UPDATE_SNAPSHOTS=1` zu erledigen: gemessen scheitern auf der Referenzmaschine **12** Goldens, nicht 3, weil die Referenzumgebung nirgends gepinnt ist. Dieser Task wird erst nach `GOLDEN-REF-30` + `GOLDEN-FIXT-31` + `GOLDEN-BASELINE-32` abgeschlossen. Posten (2) ist ausgelagert an `LENSFUN-DB-33` (dessen „10 rote Tests" sich bei Nachmessung als Fehldiagnose erwiesen haben — dort dokumentiert). **Stand Implementierungs-Lauf 2026-09-25 (Build-Agent, unabhängige Code-Verifikation PASS):** Schema/Migration (`lumina-sidecar/src/local_adjustments.rs`, atomare Dokument-Normalisierung, loud Konflikt/Unknown/Range/Version), CPU-Compositor (`lumina-core/src/render/local_adjustments.rs` + `mask_alignment.rs`, fraktionales Alpha, Alpha-0-Byte-Identität, persistierte Reihenfolge, sequenziell nach Global/Geometry), Render-Identität (`RenderKey::with_mask_local_state_digest`, Mask+Render, nicht Decode/Base), CLI (`mask_local.rs`, `--local-layer/--set-local-adjustment/--reset-local-adjustment`), GUI (typed Regler + Reset, `Reset to As Shot`, Draft/Navigator/Neighbor/Thumbnail/GPU-Route sichtbare Verweigerung, `MaskStateSnapshot` in History) und Tests sind implementiert. Gates grün: `cargo fmt --all -- --check`, `sh scripts/check_file_sizes.sh`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets --features lumina-sidecar/zdata,lumina-bench/raw-bench -- -D warnings`, `cargo test -p lumina-core` (464), `-p lumina-sidecar --features zdata` (304), `-p lumina-cli --all-targets`, `-p lumina-gui --all-targets` (845), Workspace-Suite 1178 Tests, `cargo clippy -p lumina-core --features lensfun`, `cargo check -p lumina-gpu --features gpu`, `-p lumina-bench --all-features`, gegen das gepinnte LibRaw 0.22.2 (sha256 verifiziert). **Offen:** (1) die drei durch die UI-Änderung betroffenen kittest-Goldens `develop_basic`, `develop_sections_expanded`, `develop_section_masking` brauchen ein `UPDATE_SNAPSHOTS=true`-Refresh auf der Referenz-Maschine (headless wgpu-Adapter nötig); (2) `cargo test -p lumina-lensfun --features native -p lumina-core --features lensfun` meldet 2 Lensfun-Datenbank-Tests rot (fehlendes `/usr/share/lensfun`, P0-fremd); (3) die unabhängige Verifikation durch einen zweiten Subagenten steht aus — erst danach darf der Punkt entfernt werden.

- [ ] **[PRIO: hoch] MASK-LOCAL-P1.1 (User-Order 2026-09-25; SOLL zuerst, Release 1.0)** Erweitere das versionierte typed `MaskLocalRecipe`/`local_adjustments` um einen separaten relativen WB-Delta (`temperature_delta_k`, `tint_delta`, endlich und range-validiert; kein absolutes oder lokales Alias). Globales WB bleibt absolute-only mit explizitem **Reset to As Shot**; lokales WB wird pro ausgewählter Maske sequenziell nach dem globalen Ergebnis und vor local Basic (`exposure → contrast → shadows → highlights`) angewendet, mit Float-Gains und einer abschließenden Quantisierungsgrenze. P0-Felder und Legacy-v1/`adjustment_*`-Migration bleiben erhalten; Layerreihenfolge, Alpha/Outside-Mask/Fraktionalblend, Strict/Warn-Atomizität, Digest, History/Reset/Previous und CPU/GPU-Routing müssen exakt erhalten bzw. CPU-refused werden. **Cross-image Previous/Sync bleibt recipe-only (P0-Vertrag):** Der dateibasierte CLI-Befehl `previous` überträgt keine lokalen Masken-Layer und keine P0-/P1.1-Deltas. Nicht-neutraler lokaler Zustand auf der Quell-Kopie bricht den Lauf mit Exit 1 ab, ohne ein Ziel anzufassen; nicht-neutraler Zustand auf einem Ziel markiert nur dieses Ziel als `failed` (Exit 3) mit unveränderten Ziel-Bytes. Sync Settings bleibt recipe-only. Ein expliziter Full-Look-/Masken-Kopiervorgang ist eine spätere, getrennte Aktion und ausdrücklich nicht Teil von P1.1; ein Delta allein (Tint-only oder Temperature-only) ist bereits ein Verweigerungsgrund. Das GUI-Previous auf eine Auswahl im selben Lauf überträgt den vollständigen Masken-Snapshot unverändert bei kompatiblem Ziel-Maskkontext (bestehender GUI-Vertrag). **Lazy Source-Stage:** `RenderOutput::effective_source_stage` ist `Option`; nur die GUI-Route der lokalen WB-Pick-Session fordert sie an (`render_frame_from_base_with_source_stage`), CLI/Export/Global-/Stand-in-Routen zahlen keine Full-Frame-Kopie. Stage und Render-Digest werden immer paarweise gesetzt bzw. verworfen. Picker/Setter/CLI verwenden dieselbe ausgewählte Layer- und effektive-Quellstage mit expliziter Sample-Provenance/Staleness; außerhalb der Maske oder bei stale/missing sample sichtbarer Fehler, kein globaler Fallback, kein Auto-WB. Abnahme: exakte CPU-Goldens für Delta-only, Tint-/Temperature-only, zwei überlappende Layer mit Temperatur- **und** Tint-Delta in persistierter Reihenfolge, Full/Half/Zero-Mask, Global/Reset, Alpha/Outside, Legacy, invalide Werte, explizite Nicht-Zahlen (u. a. `null` und ein sentinel-ähnliches Objekt) als lauter Fehler statt stillem `0`, Digest/History/Reload, CLI/GUI-Parity, Picker und ein No-Local-Pfad-Nachweis; P1.2 Tone/Color/Presence/Detail/Optics bleiben deaktiviert. GPU-/Stand-ins müssen bis zur Parität sichtbar CPU-routen/verweigern. SOLL: `feature/product/ai-masks.md` §P1.1. **Stand Implementierungs-Lauf 2026-09-25 (Build-Agent, unabhängige Verifikation PASS; nicht-blockierende Befunde behoben):** Schema v2 mit `temperature_delta_k` (`-5000..=5000`) und `tint_delta` (`-1..=1`), endlich- und range-validiert; `MaskLocalRecipe` bleibt Typ-Alias auf `LocalAdjustments`; Legacy-v1/`adjustment_*` akzeptiert nur die vier P0-Keys und verweigert absolute/lokale Aliase laut (`local_adjustments/{wire,migration}.rs`). Der frühere `__lumina_missing_local_delta__`-Sentinel ist entfernt: `WireDelta` unterscheidet jetzt ausschließlich „Feld fehlt" (`#[serde(default)]`, nie durch Payload-Daten erreichbar) von einem explizit geschriebenen Nicht-Zahlenwert, der laut abgelehnt wird. Float-Kernel `lumina-core/src/render/local_wb.rs` (`warmth = temperature_delta_k / 5500`, Gains `[1-w·0.35, 1-t·0.20, 1+w·0.35]`) mit genau einer abschließenden RGBA8-Quantisierungsgrenze; neutraler Delta delegiert an den P0-Kernel. `lumina-gui/src/effective_source_stage.rs` pinnt Stage und `RenderKey`-Digest paarweise und fordert die Stage nur an, wenn eine lokale Pick-Session läuft; Picker prüft Digest/Current-Render/Maske-Matte und verweigert außerhalb der Maske sowie bei stale/missing Sample sichtbar (kein globaler Fallback, kein Auto-WB). Der kombinierte Local-WB-Commit loggt jetzt `mask.local.white_balance` statt des irreführenden `mask.local.temperature_delta_k`, sodass ein Tint-only-Pick nicht als Temperaturänderung berichtet wird. CLI: generische `--set-local-adjustment temperature_delta_k=…|tint_delta=…` (keine dedizierten Flags wegen `main.rs`-Ratchet); `previous` ist wieder recipe-only und verweigert nicht-neutralen lokalen Zustand auf Quelle und Ziel laut. GPU/Stand-ins verweigern lokal WB sichtbar mit CPU-Route. **Verifikations-Rundlauf 2026-09-25 gegen das gepinnte LibRaw 0.22.2 (sha256 `627928088300ecde6ca91ffd202e189203f04ad61ad12f0fe9dc57b9a7a0fb3c`, via `target/tmp/env.sh`, eigenes Target `target/p11-remediate`):** grün — `cargo fmt --all -- --check`; `sh scripts/check_file_sizes.sh` (Baseline unverändert, keine neuen Einträge); `git diff --check`; `cargo check --workspace --all-targets --features lumina-sidecar/zdata,lumina-bench/raw-bench`; `cargo clippy --workspace --all-targets --features lumina-sidecar/zdata,lumina-bench/raw-bench -- -D warnings`; `cargo clippy -p lumina-core --features lensfun --all-targets -- -D warnings`; `cargo check -p lumina-gpu --features gpu`; `cargo test -p lumina-core --lib` (473 passed); `-p lumina-sidecar --lib --features zdata` (307 passed); `-p lumina-cli --all-targets` (272 passed, 0 failed); `-p lumina-gui --lib` (858 passed, 5 ignored); `-p lumina-gui --all-targets` (exit 0 — der im vorigen Lauf offen gebliebene Posten); `cargo test --workspace --exclude lumina-gui --features lumina-sidecar/zdata,lumina-bench/raw-bench` (1599 passed, 0 failed); `cargo test --workspace --no-fail-fast --features …` (2501 passed / 10 failed). Alle 10 Fehler sind die vorbestehenden, P1.1-fremden Lensfun-Datenbank-Tests (2× `render::tests::lensfun_*` in `lumina-core` + 8× `lumina-lensfun --features native`, panic auf `LensfunDb::load_system()`, weil `/usr/share/lensfun` fehlt); per HEAD-Worktree gegengeprüft: exakt dieselben 2 + 8 Fehler auf unverändertem `2753ce6`, `crates/lumina-lensfun` ist unverändert. Neue/gezielte Testanker: `render::tests::local_white_balance::two_overlapping_local_wb_layers_use_persisted_order_with_both_delta_axes` (überlappende Layer, persistierte Reihenfolge, Temperatur- **und** Tint-Delta, Reverse-Reihenfolge liefert andere Bytes), `::source_stage_capture_is_opt_in_and_never_changes_pixels`, `::global_only_render_without_masks_never_captures_a_source_stage`, `tests::local_adjustments::explicit_non_number_wb_deltas_are_loud_never_a_silent_zero`, `tests::mask_local::no_local_work_means_no_effective_source_stage_capture`, `::local_wb_commit_labels_both_delta_axes_in_the_log_action`, CLI `previous_never_transfers_local_mask_state_between_images` und `previous_refuses_recipe_only_local_mask_transfer_on_source_or_target`. **Offen:** (1) die drei durch die UI-Änderung betroffenen kittest-Goldens `develop_basic`, `develop_sections_expanded`, `develop_section_masking` brauchen einen `UPDATE_SNAPSHOTS=true`-Refresh auf der Referenz-Maschine (headless wgpu-Adapter nötig) — **das ist das verbleibende native Golden-Gate dieses Tasks** — **Blocker-Auflösung 2026-09-25:** ausgelagert an `GOLDEN-REF-30` → `GOLDEN-FIXT-31` → `GOLDEN-BASELINE-32`; die Doku „3 Goldens" ist gemessen unvollständig (12/56 rot, weil die Referenzumgebung nicht gepinnt ist), ein pauschales `UPDATE_SNAPSHOTS=1` wäre hier ein stilles Akzeptieren von Regressionsrisiko und ist ausdrücklich **nicht** die Abnahme. (2) die 10 vorbestehenden Lensfun-Datenbank-Tests bleiben P1.1-fremd rot (fehlendes `/usr/share/lensfun`, auf HEAD reproduziert) — **korrigiert 2026-09-25:** bei unabhängiger Nachmessung auf dieser Maschine **0 rot**; `load_system()` nutzte den in der dylib kompilierten Datadir, nicht einen verdrahteten Linux-Pfad. Die Rotmeldung stammt aus der abweichenden Laufumgebung dieses Laufs (eigenes Target + gepinnte `env.sh`). Der reale, behobene Defekt war die stille `None`-Auflösung → `LENSFUN-DB-33`; (3) P1.2 Tone/Color/Presence/Detail/Optics bleiben bewusst deaktiviert und sind der nächste Schnitt — **in P1.1 wurde kein lokales Tone/Color nachgerüstet**; (4) die Parent-Gates `GPU-RENDER-MASK-19`, `GPU-PARITY-HW-28`, `R5-BRUSH-24`, `R5-MASKVIS-25` bleiben offen — lokales WB schließt sie nicht.

### PRIO: hoch — Produktbefund aus GOLDEN-FIXT-31

- [ ] **[PRIO: hoch] THUMB-HASH-PERF-35 (Release 1.0, Produktcode; gefunden 2026-09-25 bei `GOLDEN-FIXT-31`)** Der UI-Thread hasht **jede sichtbare RAW-Datei im Ganzzen, in jedem Frame.** Kette: `ensure_thumbnail_priority` (`library_grid.rs:462`, `filmstrip_frame.rs:223`, `library_views.rs:124/193/297`) → `ensure_thumbnail` → `FilmstripManager::refresh_source` (`filmstrip.rs:168`) → `source_identity` → `persisted_action_identity` (`sidecar_snapshot.rs:186`) → `FileContentIdentity::from_path` (`source_actions.rs`), das die Datei **in 64-KB-Blöcken vollständig liest und BLAKE3-hasht** — **vor** jedem Early-Out. Gemessen: 0,105 s Hash pro 12-MB-CR3, **0,79 s pro UI-Frame** bei 3 Zellen, Decode 0,69 s. Die 18-Byte-`.arw`-Sentinel-Fixtures haben das **vollständig verdeckt** (Hashen von 18 Byte ist gratis). **Nutzerfolge:** ein Nutzer mit einem Ordner echter RAWs bekommt ~1 fps; das ist kein Testartefakt, sondern der Normalfall. **SOLL zuerst:** in `feature/platform/cli-gui-wasm.md` (F-100/Performance) das Identitäts-Caching-Verfahren normativ beschreiben — Cache-Schlüssel `(Pfad, mtime, len)` als Ersatz für die teure Volldatei-Identität, und/oder die Berechnung aus dem UI-Thread herausziehen. **Abnahme:** `ensure_thumbnail` hasht pro Frame **nicht** erneut bei unveränderter Datei (Nachweis per Test: Zähler oder Zeitmessung); die Korrektheit bleibt erhalten — eine **geänderte** Quelle (mtime/len) muss weiterhin eine neue Identität erzwingen und darf **nicht** als unverändert durchgewunken werden; die Cache-Identität bleibt mit dem Persistenz-Vertrag konsistent (kein „cached ok" für eine tatsächlich geänderte Datei); ein BLAKE3-Aufruf pro Frame und pro Zelle ist messbar ausgeschlossen; `cargo test -p lumina-gui --all-targets` bleibt grün mit unveränderter Testanzahl. **Abgrenzung:** Produktcode in `lumina-gui`, keine Änderung an Schema, Sidecar, Renderpipeline oder Persistenz; keine Änderung an Fixture-/Golden-Struktur (das ist `GOLDEN-FIXT-31`/`GOLDEN-BASELINE-32`). **Warum PRIO hoch:** blockiert die zeitlich vertretbare Golden-Baseline und ist ein echter Nutzer-Defekt, kein Test-Thema.

### PRIO: mittel

- [ ] **[PRIO: mittel] GUITEST-STRUCT-34 (Release fortlaufend, Hygiene; unabhängig von allen Gates)** Die headless GUI-Testsuite ist organisatorisch verstopft: 91 Dateien liegen flach in **einem** `mod tests`-Block in der 12.970-zeiligen `src/lib.rs`, 745 `#[test]`s. 24 % der Dateien (22) sind nach Incident/Task-ID benannt statt nach Feature — `instrdbg_*` ×7, `f100_*` ×4, `r3_*` ×4, `g01`/`g15`/`g16_*`, `w3_*` — während 3 Dateien exakt auf der 500-Zeilen-Ratchet-Grenze stehen (`navigator.rs`, `library_sort.rs`, `g15_stacks.rs`) und 25 weitere innerhalb von 50 Zeilen davon. Jeder neue GUI-Test erzwingt damit sofort einen Split, und die Dateinamen erklären den Inhalt nicht mehr. **SOLL zuerst:** in `feature/platform/cli-gui-wasm.md` (F-100-Abschnitt) die Namens- und Gliederungsregel für `crates/lumina-gui/src/tests/` festschreiben — thematische Domänenmodule statt Incident-Präfixen, eine `mod`-Ebene pro Domäne, gemeinsame Helder in `tests/support.rs`. **Abnahme:** die 22 Incident-Dateinamen sind auf Feature-Namen umgestellt (mit `git mv`, History über `git log --follow` nachvollziehbar); die 3 Dateien auf der Ratchet-Grenze sind unter 500 Zeilen; `scripts/check_file_sizes.sh` grün **ohne** neue Baseline-Einträge; keine Kommentar-/Doku-/Test-Löschung als Kompensation (Diff-Gate); `cargo test -p lumina-gui --all-targets` unverändert grün und mit identischer Testanzahl (745) — reine Umorganisation, keine Verhaltens- oder Abdeckungsänderung. **Abgrenzung:** keine neue Testabdeckung, kein Produktcode, keine Golden-/Fixture-Anzahl (das ist `GOLDEN-FIXT-31`).
- [ ] **[PRIO: mittel] GPU-PARITY-HW-28 (User-Order 2026-09-23; Welle-2-Gate)** Bestehende GPU-Parity-Laufzeitprüfung auf einer echten Hardware-Lösung (Metal/Vulkan) mit renderbaren `R32Float`-Render-Targets wiederholen. Der lokale `llvmpipe`-/GL-Softwareadapter ist keine zulässige Paritätsreferenz und erzeugte die reproduzierten `detail_stage_stack`-/Sharpening-Fehler; keine Toleranzen, Ignores oder CI-Skips als Ersatz. Abnahme: Hardware-Run mit `cargo test -p lumina-gpu --features gpu --test parity`, Protokoll und unabhängige Verifizierung. **Stand 2026-09-25 (Build-Agent):** Die benötigte Hardware ist auf der Arbeitsmaschine **vorhanden** — Apple M5 Pro, `Metal Support: Metal 4` — und der headless-wgpu-Adapter ist als funktionsfähig nachgewiesen (die `kittest_snapshots` rendern und erzeugen Diff-Bilder, statt an einem Adapter zu scheitern). Dieser Task ist damit **kein Hardware-Blocker, sondern ein Ausführungstask** und sollte vor `GOLDEN-BASELINE-32` laufen, weil sein Protokoll die Referenzumgebung für Welle 1/2 mitbelegt.

### PRIO: niedrig

- [ ] **[PRIO: niedrig] TEST-AUDIT-36 (fortlaufend, User-Regel 2026-09-26)** Redundanz-Audit der Testsuite: **höchstens einmal pro Woche** einen Build-Agenten starten, der prüft, ob es unnötige Tests gibt. Der Build-Agent **berichtet nur** und entfernt nichts eigenmächtig; jede Löschung läuft über SOLL → Implementierungs-Agent → unabhängige Verifikation. **Prüfkatalog:** (a) Tests, die eine Konstante auf sich selbst prüfen; (b) doppelte Abdeckung derselben Aussage in mehreren Dateien; (c) Tests, die eine Implementierungsentscheidung statt des Verhaltens pinnen; (d) Tests, die bei Wegfall der Logik nichts verlieren würden; (e) Tests ohne Fehlsignal — die grün bleiben, egal ob die Aussage stimmt; (f) **Laufzeit-Ballast**, der die Suite verlangsamt, ohne Aussage zu tragen (der 3:11-GUI-Shard ist das aktuelle Beispiel); (g) Assertions, die nach einem Refactor nur noch den Refactor beschreiben. **Abnahme:** ein priorisierter Befundbericht mit Test-Name, Datei und der konkret entfallenden Aussage; keine Löschung ohne Begründung; keine Reduzierung der Netto-Aussagenabdeckung — eine beim Audit entdeckte **Lücke** (Feature, Klausel, Fehlerpfad ohne Anker) wird als **neue offene Aufgabe** angelegt, nicht durch Streichen eines Nachbarn ersetzt. Grundlage: Policy in `Agents.md` §„Testabdeckungs-Politik". **Abgrenzung:** keine Produkt- oder Schemaänderung; der Audit verändert keine Abnahmegate und ersetzt keine DoD-Prüfung.
- [ ] **[PRIO: niedrig] CI-WATCH-1 (fortlaufend)** Nach jedem Push (morgen als erstes): CI-Runs prüfen (`gh run watch` / `gh run list --branch main`), Ergebnis im Tagesstand vermerken. Bei Rot: als Next-Task in `Agents.todo.md` dokumentieren, NICHT still umsetzen (User-Vorgabe). Abnahme: jeder Push hat ein geprüftes CI-Verdict.
- Veröffentlichungsdienste bleiben explizit nie Ziel (kein Task).
- [ ] **[PRIO: niedrig] LRPAR-G14-DENOISE-IMPL-20 (Release: 2.0)** KI-Denoise-Implementierung nach Entscheid `feature/decisions/LRPAR-G14-DENOISE-20.md` (F-078-Fixture-Entscheid ✓, Schema ✓, Pipeline-Stufe + Persistenz + GPU-Refusal ✓, ONNX-Backend ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only, F1-Input-Spec-v2 inkl. Core-Blend-/Assembly-Identität ✓, F3-zdata-Feldvalidierung ✓; offen: echte Gewichte). Der Parent-Checkbox bleibt bis zum F-078-Gate (Gewichts-Lizenz, Provenienz und Hash-Pin) offen; keine stillen Stub-/Produktions-Fallbacks. Abnahme: CLI + GUI-headless + Golden/PSNR, kein stiller Fallback.
- [ ] **[PRIO: niedrig] LRPAR-G09-CULL-IMPL-25 (Release: 2.5)** KI-Culling-Implementierung nach Entscheid `feature/decisions/LRPAR-G09-CULL-25.md` (Schema ✓, Heuristik `lumina-cull` ✓, CLI ✓, GUI ✓, F-074-Budgets ✓ report-only; **Follow-up 2026-09-24:** CLI/GUI erzwingen Live-Quellenidentität und verweigern Sidecar-Writes bei Hash-/Längenkonflikt, inklusive `--force`/Batch-Isolation; offen: optional ONNX Stufe 2 und kittest). Abnahme: CLI-Exit-Codes + GUI-headless + kittest, Vorschlag schreibt nie Rating/Flag/Label. **Parent bleibt wegen Stage 2/kittest offen.**

### PRIO: niedrig (Block A, nicht-LRPAR)

**Phase 6: Persistente AI-Masken**

_(keine offenen Tasks — F-082-FOLLOWUP BESTANDEN verifiziert 2026-09-02, 107p `onnx-rt`, wasm32 `onnx-rt` grün, Commit 49f4f76)_

**Phase 9: Optionale zentrale Indizierung (Post-MVP)**

_(keine offenen Tasks — F-064…F-067 BESTANDEN verifiziert 2026-09-02, Commit 1520ac5, Doc-only: minimaler Umfang/Cacheverweise, SQLite non-default `index` `assets`/`jobs`/`cache_refs` WAL/`user_version`/`.lumina/index/`, Rebuild/Locking/`integrity_check`/corrupt sichtbar/Sidecar-only, Löschsicherheit Delete→Rebuild identisch; `cargo check --workspace` grün)_

Ist-Stand 2026-09-02: kein Index-Modul im Workspace; CLI-`reindex` ist nur ein
Sidecar-Scan (zählt valide Sidecars, persistiert nichts); `feature/architecture/index.md` ist normativ vervollständigt und verifiziert.

**Phase 10: WASM und Plattformen — ENTFERNT 2026-09-04**

_(WASM/Browser ersatzlos gestrichen; F-069…F-071 entfallen, WASM-CI-Job gelöscht.
Historisch: F-069…F-071 Doku-first 2026-09-02, Commits 287fe75/e60a9ad)_

**Phase 10b: Generatives Entfernen + Erweitern (Post-MVP)**

_(keine offenen Tasks — GEN-EXPAND-1 BESTANDEN verifiziert 2026-09-02, Commit 46f6baf: Doku-first GenerativeEdit Felder sha256 Prompt/Seed inference_resolution canvas>100% region/mask ref artifact .lumina.zdata kind=generative_canvas atomar, Pipeline Decode→SourceActions→GenerativeEdit→Lens→Perspective→Crop, Identität/Veraltung analog AI-Masken, kein stiller Fallback, Capability lokal vs Cloud getrennt, Lizenz F-078; kein Code)_


## Block B – „Offene Rückfragen“

Tasks, bei denen eine User-Entscheidung/Klärung fehlt (Produkt-, Naming-,
Lizenz-/Schema- oder Übernahme-Fragen). Blockiert Block A nicht; sollte aber,
wo möglich, vor dem nächsten manuellen GUI-Test geklärt werden.

### PRIO: mittel

**Produktname (Rest von F-101-F1)**

- [ ] **[PRIO: mittel] NAMING-F1 (kein Goal, Release: 1.0)** Produktname final entscheiden
  (`docs/naming-brainstorm.md`). **User-Entscheidung 2026-08-25:** Brainstorm
  läuft bewusst weiter, Naming bleibt offen. **User-Entscheidung 2026-09-04:**
  Naming erst kurz vor MVP entscheiden — bis dahin keine Naming-Arbeit, kein
  Vorziehen. Die übrigen F-101-F1-Anteile
  (MCP-Scope) wurden zur Umsetzung freigegeben und stehen in Block A.

## Block C – „Nach dem nächsten manuellen GUI-Test“

Tasks, die erst nach dem nächsten manuellen GUI-Test sinnvoll/erforderlich
sind (Verifikations- und Abschluss-Tasks, die auf Testergebnissen aufbauen).

### PRIO: hoch

**Phase 8: Desktop-GUI (F-103, MVP)**

UI-Konventionen F-100 sind spezifiziert, verifiziert und für jede GUI-Arbeit
verbindlich — normativ in `feature/platform/cli-gui-wasm.md` (Abschnitt
F-100). SOLL für den MVP-Scope: ebenda „Desktop-GUI" und „Erster visueller
User-Test". Die implementierten Slices (Module, Develop-Sektionen, interaktive
Maskenwerkzeuge, Exportmodul, i18n, Presence/Vibrance, kittest-Snapshots) sind
unabhängig verifiziert; Details in Git-Historie und Feature-Dokument.

Vor F-103-N6 bleibt die nachstehende vollständige GUI-Checkliste offen; die
Review-Befunde (REVIEW-CORE-CROP-1, REVIEW-GUI-DEBOUNCE-1,
REVIEW-GUI-MASKRENDER-1) sind mit Marker-Kommentaren im Code implementiert;
die F-103-N6-Runde 1 hat eigene Befunde erzeugt (GUI-CLICK-ALL-17,
GUI-ROUTING-N6, GUI-INSTRDBG-17 — alle BESTANDEN verifiziert).

- [ ] **[PRIO: hoch] F-103-N6-GUI-COVERAGE-27 (User-Order 2026-09-23; manueller GUI-Gate offen)** Vollständiger manueller/visueller Abnahmelauf für alle noch nicht durch einen normalen `cargo test -p lumina-gui` (ohne `--ignored`) belegten Desktop-GUI-Fälle. Diese Task bleibt offen, bis ein Lauf auf einer Display-/GPU-Maschine mit **genau einer** App-Instanz, `RUST_LOG=trace` und Log-Redirect nach `/tmp/lumina_manual_2026-09-23.log` erfolgt; die headless CI ersetzt diesen Lauf nicht. Jeder Fall wird mit Testanker, Logauszug und Ergebnis dokumentiert.

  **P0 — DnD-/Persistenz-Risiko (`F-103-N6-DND-PERSIST-01`):** nativer Drop einer PNG/JPEG/WebP und einer CR3 in eine leere App; Drop von Quelle B nach geladener Quelle A; Exposure/Contrast-, Spot- und Maskenedit nach Drop; Sidecar adjacent zu B, frischer Neustart und byte-identisches Original; unlesbarer/unsupported Drop mit sichtbarem Fehler und unveränderter vorheriger Quelle. **Stand 2026-09-24:** Path-first-/Deferred-Decode-Produktionsfix, generation-gebundene Async-Scan-Nachbarplanung und headless Regressionen sind eingebaut; der echte OS-DnD-Nachweis bleibt offen.

  **1. Start, Eingabe und Shell:** leere App, Ordner-CLI, `--module library|develop|export`, `--fullscreen`, Open/Refresh/Up/Breadcrumb, nativer Datei-/Ordnerdialog, Drag-and-drop, Auto-Load des ersten Bildes, RAW-Orientierung/Metadaten, Status/Banner/Modal-Dialog, Toasts, Neustart und Rendern des tatsächlich geladenen Pfades.

  **2. Responsive Layout und View-Toggles:** 1024×720, 1280×800, schmale/verbreiterte Panels, linke Rail/Navigator, rechte Histogramm-/Develop-Panels, Filmstrip, Toolbar, Lights-out/Fullscreen sowie `Tab`, `Shift+Tab`, `L`, `F`; die Preview-Rechtecke müssen tatsächlich wachsen/schrumpfen, nicht nur State-Flags toggeln. Vorher/Nachher-Goldens und Clip/Overlap-Prüfung sind Pflicht.

  **3. Library-Navigation und Auswahl:** Grid/Loupe/Compare/Survey/People, Ordner-/Unterordnerbaum, Empty-State/CTA, Thumbnail-Größe, Filter, Name/Aufnahmedatum/Custom-Sortierung, Custom-Drag-Insertion, Click/Cmd/Shift-Auswahl, Doppelklick, Home/End/Enter/Esc, Rating/Flag/Color/Badges, Stacks (collapse/expand/select), R6-SCAN-1 erster Paint mit großem echten Verzeichnis.

  **4. Develop-Grundlagen:** alle sichtbaren Basic-Slider (Exposure, Contrast, Highlights, Shadows, Whites, Blacks), Doppelklick-/Alt-Scroll-/Section-Reset, Preset/Profil/Treatment, Auto-Tone, WB-Pipette, Before/After und Split, Original/Softproof/Clipping, Histogramm, Export/Apply/Save/Reset/Match/Regenerate; jede Änderung muss `Edit → Debounce/Commit → Sidecar → Fresh Reopen → Wert/Preview` belegen.

  **5. Farbe, Kurven und Detail:** Tone-Curve-Grafik (Punkt setzen/ziehen/löschen, Kanal Master/R/G/B), HSL/Color-Mixer, Point Color, Color Grading, Presence Texture/Clarity/Dehaze, Vibrance/Saturation, Detail (Schärfen, Rauschreduzierung, Rote-Augen-Picker/Detect/Apply/Remove/Clear), Denoise-Status, Effects (Vignette/Grain), Optics (Profil, Bokeh/Enable), Lens Blur und Generative-Controls inklusive Fehlermodi.

  **6. Geometrie und Preview:** Crop-Handles/Drag/Aspect/Enter/Esc, Straighten, Rotate/Mirror, Perspective/Auto-Upright, Zoom Fit/100–200 %/Fit Width, Pan/Navigator-Drag, Loupe-Vergleich, Draft/Stale/Ready/Failed-Badges, GPU-Present versus CPU-Fallback und sichtbare Route-Gründe.

  **7. Masken (`R5-BRUSH-24`/`R5-MASKVIS-25`):** Mask-Liste mit Anlegen/Mehrfachmasken, Auswahl, Pin-Liste, Sichtbarkeits-Auge, Umbenennen, Löschen, Reihenfolge, Duplizieren, Copy-vs-Duplicate-Gruppen, lokale Layerwerte; Brush-Größe/Weichheit/Fluss, `[`/`]`-Alias, live Kreis-Cursor, Pinsel/Gradient/Radial, Invert/Feather/Blur/Density, Show/Overlay-Farbe, Overlay nur bei geöffneter Masken-Ansicht, Modus „nur Pins aller Masken“ versus „volles Overlay der ausgewählten Maske“ und Panel-Hide mit vergrößerter Preview. Headless-Anker (`brush_marks_roundtrip_through_sidecar`, `g03_*`, `g11_*`, `brush_interaction`, `brush_management_controls_have_headless_structural_action_coverage`) decken Modell, Widget/Layout und echten Preview-Pointer ab; der kombinierte 1024×720-Snapshot bleibt ein explizit ignorierter Native-Adapter-Gate, der unabhängige Metal/Vulkan/DPI-Nachweis bleibt offen.

  **8. Spot-Heal (`R5-DUST-23-FOLLOWUP`):** Toolbar-Heal/Q, Quick/Generativ-Modus, Size/Feather/Opacity, `[`/`]`, Cursor, Klick-Dab, Spot-Auswahl, Liste nur mit ausgewählter Entfernung, Typ/Status, Parameter editieren, Löschen einzelner Spots, Detect/Apply, Distraction-Schalter, Visualize, Regenerate-Variante ohne Clone-Fallback und Reload; fehlende/veraltete/fehlerhafte generative Artefakte müssen sichtbar bleiben.

  **9. Metadaten, Sync und Export:** Keywords, Collections, Smart Collections, IPTC-Draft/Historie/Presets/Sync, Stack-/Batch-Aktionen, Sync Settings/Match Total Exposure/Previous, Merge, Exportziel/Format/Qualität, native Save-Dialog, PNG/JPEG/WebP/RAW-Export, Same-Path-Guard, Metadaten-Bake-In und Exportfehler.

  **10. Persistenz, Fehler und Nebenläufer:** Sidecar-Roundtrip über mindestens zwei virtuelle Kopien, History/Undo/Reset, Source-Changed/Missing/Corrupt/Stale, CAS-Konflikt, echter Zwei-Prozess-Schreibkonflikt, Cache löschen/rebuilden, fehlende Modelle, Panic/Recovery, Abbruch während Debounce sowie Original-Bytes byteweise unverändert.

  **11. Vollständigkeitsnachweis:** alle `ALL_GUI_ACTIONS` (aktuell 110) gegen eine gezeichnete, klickbare Fläche und einen PASS/FAIL-Test mappen; jede neue `GuiAction`/Enum-Variante erzwingt den Audit. Für visuelle Änderungen `egui_kittest`-Golden/PSNR/Histogram plus Vision-Review; für unveränderte Modelle genügt der jeweilige headless Test nicht als manueller GUI-Erfolg.

  **Abnahme:** native PNG/JPEG/WebP/CR3- und DnD-Kette, Sidecar-Datei inspiziert, App neu gestartet, `cargo test -p lumina-gui`, gezielte Regressionstests, `cargo fmt --check`, Clippy mit `-D warnings`, `sh scripts/check_file_sizes.sh` und unabhängiger Verifizierungsbericht mit DoD-§7-Mapping; kein Befund darf als „manuell geprüft“ ohne Log-/Testbeleg gelten.

- [ ] **[PRIO: mittel] R2-GUIMOD-04b (→ G-10, Release: 1.0)** (nach manuellem Test + 04a-Zahlen): CPU-Draft-Drossel auf GPU-Pfaden entscheiden (throttlen vs. GPU-Histogramm 04c vs. lassen). Eingang: 04a-Messwerte aus F-103-N6.
- [ ] **[PRIO: mittel] R2-GUIMOD-04c (→ G-10, Release: 1.0)** (nach manuellem Test, Alternative zu 04b): Histogramm per GPU-Compute aus VRAM (1-KB-Readback statt Full-Frame-Analyse). Nur wenn 04a-Zahlen den Aufwand rechtfertigen; CPU-Pfad bleibt für Non-GPU (als Fallback, nicht WASM — WASM ist gestrichen).

- [ ] **[PRIO: hoch] F-103-N6 (→ Querschnitt, alle G, Release: 1.0)** Erster visueller User-Test: `RUST_LOG=trace cargo run -p lumina-gui` (Trace-Pflicht nach DoD §6) mit
  PNG/JPEG/WebP + nativen RAW per Pfad und Drag&drop; Preview + Exposure/
  Contrast ändern den Renderstand; Sidecar wird geschrieben und beim Neustart
  wiederhergestellt. Abnahme:
  reproduzierbare Befehle aus cli-gui-wasm.md + Log-Ausschnitt; unabhängiger Verifizierungs-
  Agent bestätigt F-100-Checkliste + Tests (BESTANDEN). Letzter Schritt vor
  Abschluss von Phase 8.
  **Bereit für Runde 2 (Release-Build, Stand 2026-09-18):** Geladet: GUI-GPU-AUDIT-17
  (verifiziert + committet — Audit-Test + Timing-Baseline), GPU-LENSFUN-Kern +
  -Wiring (gemeinsam verifiziert BESTANDEN; Commit nach SEC-Abgrenzung der
  Baseline — die Lensfun-Ausnahme steht nicht mehr im Runde-2-Log).
  GPU-RENDER-DENOISE/PREVIEW/EXPORT/MASK-19 sind bewusst dokumentiert-only
  (keine Implementierung vor Runde 2 — User-Entscheid 2026-09-18); Runde 2
  protokolliert deren CPU-Routen als bekannte Gaps, nicht als neue Befunde.
  GUI-PARITY-GOLDENS-18 empfohlen, nicht Pflicht (nur CI-ignorierte Goldens).
  R2-GUIMOD-04b/04c brauchen 04a-Zahlen aus Runde 2 (danach). UX-LOOK-18 sind
  unabhängig (Look beeinflusst Routing-/Perf-Messung nicht).

## Abnahmekriterien

Die erste produktiv nutzbare Version muss mindestens Folgendes erfüllen:

- Ein RAW kann ohne zentrale Datenbank importiert, bearbeitet und exportiert
  werden.
- Nach dem Neustart werden Bearbeitungsrezept und virtuelle Kopien ausschließlich
  aus dem Sidecar wiederhergestellt.
- Zwei virtuelle Kopien desselben Originals können unterschiedliche Rezepte,
  Masken-Layer und Exporte besitzen.
- Eine gültige persistierte AI-Maske wird wiederverwendet und nicht ungefragt
  neu berechnet.
- Änderungen an Quelle, Modell, Decode-Kontext oder Maskenartefakt werden als
  veraltet erkannt.
- Vorschauen und Exporte sind über einen reproduzierbaren Render-Key cachebar.
- Das Löschen eines optionalen zentralen Indexes zerstört keine Bearbeitung.
- Originaldateien bleiben byteweise unverändert.
- Sidecar-, Migration-, Cache-, Masken- und virtuelle-Kopien-Tests sind durch
  einen unabhängigen Verifizierungs-Agenten bestätigt.

## Festgelegte Produktentscheidungen

Die fachlichen Entscheidungen sind in `feature/README.md` und den verlinkten
SOLL-Dokumenten festgeschrieben. Neue offene Punkte werden als konkrete
Implementierungsaufgaben mit Feature-ID ergänzt, nicht als unpriorisierte
Entscheidungsliste gesammelt.
