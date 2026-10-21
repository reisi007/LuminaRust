# Skills-Marker

agents-skills-consumed: 0449b05
geprüft am: 2026-10-21

## Skill-Stand

| Skill | angewendet? | wo im Projekt | offene Position |
|---|---|---|---|
| `build-verify` | ja | `Agents.md` §„Rollen und Delegation" → „Build-Agent" (Zeiger auf den Skill + `.agents/rules/build-verify.md`, ohne Kopie) und §„Verifizierung und Tests" (repo-spezifische Gates); `Verify:`-Footer je Runde in der Historie (**40 von 40** Commits im Fenster der letzten 40 Commits **ab `e744f8b`** tragen einen `Verify:`-Footer — gemessen mit `git rev-list --max-count=40 e744f8b` und `git show -s --format=%B`; **am committeten Stand dieses Commits** im Fenster ab demselben Startpunkt ebenfalls 40/40); die **neue** Pflicht der Range (`0449b05`: jede Zahl, die etwas über den Commit behauptet, nennt ihren Bezugsrahmen — Zustand **und** Umfang; der committete Stand wird über `git archive <sha>` + `tar -x` gemessen, nie über `git checkout`) ist am Projekt **angewendet**: Commit `8162323` weist beide Bäume getrennt aus („Working Tree 29, committeter Stand 24") und misst den committeten Stand über `git archive dff6948` + `tar -x` | |
| `skills-marker` | ja | diese Datei (`AGENTS.skills.md`); der Skill ist **hidden** (nur per exakter ID ladbar), der Abschnitt „Fortschreiben" unten ist die lokale Kopie des Ablaufs (Schritt 6 trägt den Wortlaut aus `0024d7e`: kein Config-Key liefert Regeltext — es gibt keinen `instructions`-Key) | |
| `github-ci-filters` | ja | `.github/workflows/ci.yml` — `paths-ignore` für `push` **und** `pull_request` (Commit `c4e2fd8`); **am committeten Stand dieses Commits**: **2** `paths-ignore`-Blöcke (Zeilen 11 und 54) — dieselbe Zählung im **Vorgänger** `e744f8b` (`.github/workflows/ci.yml` ist seitdem unverändert). Der Skill ist seit `817c01e` **hidden** — das Projekt nennt ihn in keiner Datei außer diesem Marker, es gibt also nichts umzuschreiben | |
| `codegraph-project-setup` | ja | `.codegraph/` (Index) + `.githooks/pre-commit` Gate 1 „CodeGraph index sync" (`core.hooksPath=.githooks`) — **kein** CodeGraph-Abschnitt in `Agents.md` (der Anker der Vorversion zeigte ins Leere) | |
| `docker-test-image` | ja | `docker/Dockerfile` (LibRaw 0.22.2, `ARG LIBRAW_SHA256`-Pin des Quell-Tarballs) + **fünf** `container:`-Jobs in `.github/workflows/ci.yml` (**am committeten Stand dieses Commits**: `grep -c '^    container:'` = 5 — dieselbe Zählung im **Vorgänger** `e744f8b`; `docker/Dockerfile` und `.github/workflows/ci.yml` sind seitdem unverändert) | |
| `ghcr-visibility` | ja | `.github/workflows/ci.yml` — `ghcr.io/reisi007/luminarust/lumina-ci:latest` in denselben fünf `container:`-Jobs (**am committeten Stand dieses Commits**: **5** `image: ghcr.io`-Zeilen — dieselbe Zählung im **Vorgänger** `e744f8b`; `.github/workflows/ci.yml` ist seitdem unverändert) | |
| `security-review` | ja | Register: `feature/quality/security-review-register.md` (Form nach Stage 6 — Zwei-Hälften-Form, Registertabelle, Pflege durch den Build-Agent; Enumerationsbasis mit den erreichbaren Oberflächen, vollständig enumeriert in Welle 1 am 2026-10-20 — die Zahl **24** Oberflächen steht im generierten Block **surfaces** in §5 des Registers — **am committeten Stand dieses Commits** dieselbe Zählung wie im **Vorgänger** `e744f8b` (das Register ist seitdem unverändert); **bisher null Befunde**: die Registertabelle trägt nur die Beispielzeile `SEC-EXAMPLE`); die erste Review-Welle ist **am committeten Stand dieses Commits** die offene Task `SEC-REVIEW-WELLE-1-85` (Teil a erledigt, die Teile b/c brauchen Läufe); die **Scope-Frage GUI** ist **entschieden** (Eigentümerentscheid 2026-10-20: GUI-Oberflächen werden **gleichrangig** in jeder Welle mitgeprüft) und im Register entsprechend vermerkt. **Die geänderte Regel der Range** (Severity aus den kanonischen Stufen; `AKZEPTIERT` ist das kanonische `accepted`) trifft die Projektform des Registers: die fixe Kopfzeile hat **keine** Severity-Spalte und **keine** Ticket-Spalte | `Agents.todo.md` → `SKILLS-VOKABULAR-98` |
| `ui-review` | nein | als Playwright-Flow trifft es nicht zu: native egui-GUI, der visuelle Review läuft über `egui_kittest`-Goldens/PSNR und den Fenster-Review `UI-VISION-REVIEW-70`. Die Vision-Regel (kein Vision-Subagent: das konfigurierte Modell liest die Bilder selbst) stammt aus einer **früheren Range**, **nicht** aus der auditierten: sie steht seit `8bfc321` in `SKILL.md` („there is no vision subagent in the loop", heute Zeile 77) und wurde in `dccdd40` konsolidiert („the configured model reads images", heute Zeile 38); die **letzte** Vision-Änderung **vor** dem Cursor `00dbd22` ist `fa2268f` — sie ändert **eine** Zeile in `references/findings-report.md` (der „vision-agent"-Wortlaut). In der auditierten Range `00dbd22..0449b05` ändert `ui-review` ausschließlich `references/findings-report.md`; **keine** Vision-Zeile (`git diff 00dbd22..0449b05 -- .agents/skills/ui-review/ \| grep -i vision` ist leer). Die Regel trifft die Projekt-Pflicht in `DoD.md` §6 → dort **umgesetzt** (**am committeten Stand dieses Commits** dieselbe Aussage wie im **Vorgänger** `e744f8b`; `DoD.md` ist seitdem unverändert). Die **Findings-Vorlage** der Range (`b0d1b51`: Id, Severity, Status, Owner, Ticket) trifft dieses Projekt **nicht** — das Projekt führt keine UI-Findings-Liste nach dieser Vorlage (die UXG/UXD-Lückeinventur in `feature/platform/lightroom-ux-parity.md` ist eine SOLL-Lücke, kein Review-Report); das **kanonische Vokabular**, auf das die Vorlage zeigt, ist unter `ratchet-gates` erfasst. Der Skill wird im Projekt nur in **diesem Marker** und im **Board** (`Agents.todo.md`, Eintrag `SKILLS-VOKABULAR-98`) genannt, in keiner weiteren Projektdatei — **am committeten Stand dieses Commits**: **6 Zeilen** in **2** Dateien (`git grep -n ui-review <stand> -- '*.md'`; vorkommensweise **9**, eine Zeile trägt **4**); am **Vorgänger** `e744f8b` ist die **Dateimenge** dieselbe (**2** Dateien: `AGENTS.skills.md`, `Agents.todo.md`), der **Zeilenzähler** dort ist **5** — eine repo-weite Suche zählt die vom Commit geänderten Dateien mit; die Zusatzzeile kam mit `0cf154d`, seither bleibt der Zähler bei **6** | |
| `ratchet-gates` | ja | `scripts/gates.sh` (Stufen `fast`/`slow`/`all`/`list`/`list-ci`, Floor `MIN_CHECKS=9`, keine Codegen-Stufe `fast`), `feature/quality/local-gates.md` (Tier-Tabelle wortgleich zu `gates.sh list` — byteweise diffleer geprüft; **am committeten Stand dieses Commits** dieselbe Zählung wie im **Vorgänger** `e744f8b` — `scripts/gates.sh`, `local-gates.md` und die Gate-Skripte sind seitdem unverändert; Floor-Regel; Lückenliste), `Agents.md` §„Verifizierung und Tests" (der dortige Floor-Text nannte beim Marker-Run 2026-10-21 **vier** Textgates und **5** Checks; **korrigiert in `0cf154d`** — **am committeten Stand dieses Commits** **acht** Textgates und Floor **9**, Quellen `sh scripts/gates.sh list` und `scripts/gates.sh` `MIN_CHECKS=9`), CI-Schritt „Run fast tier (scripts/gates.sh fast)" im Job „Rust fast checks" (`.github/workflows/ci.yml`), Gate-Header der **elf** `scripts/check_*.sh` (Defektklasse, Anlass, Ratchet-Richtung, benannte Grenze; **am committeten Stand dieses Commits** **11** Skripte, jedes mit einem `Defektklasse`-Header — dieselbe Zählung im **Vorgänger** `e744f8b`) | `Agents.todo.md` → `SKILLS-VOKABULAR-98` (kanonisches Vokabular) |
| `generated-docs` | ja | `scripts/gen_doc_facts.sh` (Generator: regelbasiert, deterministisch, git-frei, POSIX sh; liest die Schwellen 500/3000 aus den Gate-Skripten, fuehrt nie etwas aus einem gelesenen Dokument aus) + `scripts/check_doc_facts_drift.sh` (Drift-Gate: erzeugen -> Block aus der Zieldatei extrahieren -> diff; meldet, schreibt nie) als `fast-8` und `scripts/check_doc_facts_generator.sh` (Hygiene-Gate des Generators: Determinismus + Git-Freiheit) als `fast-9` der Stufe `fast`; generierte Bloecke `file-sizes` + `population` in `feature/quality/file-size-policy.md`, `goldens` in `feature/README.md` + `feature/quality/golden-references.md` + `feature/quality/golden-fixtures.md`, `mcp-tools` in `feature/platform/mcp-server.md` + `feature/quality/security-review-register.md`, `perf-baseline` in `feature/quality/performance-benchmarks.md`, `crates` in `feature/README.md` und `surfaces` in `feature/quality/security-review-register.md`, `golden-classes` in `feature/quality/golden-fixtures.md` und `golden-targets` in `feature/quality/golden-references.md` (Quelle: die committete Datei `scripts/golden_inventory.txt`) — **am committeten Stand dieses Commits** **12** Bloecke in **7** Dateien (dieselbe Zählung im **Vorgänger** `e744f8b`; Generator und Zieldateien sind seitdem unverändert); **Zählregel: das Generator-Manifest** (`sh scripts/gen_doc_facts.sh --manifest`: eine Zeile je Block-Instanz, `block-id\|zieldatei` — **12** Zeilen, **7** verschiedene Zieldateien). Die Markensuche im Baum (`git grep -n '<!-- BEGIN GENERATED:' <stand> -- '*.md'`) liefert **am committeten Stand dieses Commits** **14** Treffer in **9** Dateien: die **12** echten Bloecke plus **zwei** Literal-Erwähnungen der Marker-Syntax (`feature/quality/local-gates.md` §8.1 und diese Zeile) — deshalb ist das Manifest die Zählregel, nicht die Markensuche; CI-Schritt „Run fast tier (scripts/gates.sh fast)" im Job „Rust fast checks" (`.github/workflows/ci.yml`) | `Agents.todo.md` → `DOC-ZAHLEN-DRIFT-82` (Welle 1 umgesetzt; **Welle 2 im Repo umgesetzt** (2026-10-20): `mcp-tools`, `perf-baseline`, `crates`, `surfaces` — der Todo-Eintrag `DOC-ZAHLEN-DRIFT-82` ist nachgezogen; **Welle 3 im Repo umgesetzt** (2026-10-20): der R/C-Split der Goldens und die Verteilung auf die Test-Targets werden jetzt aus der committeten Quelldatei `scripts/golden_inventory.txt` **erzeugt** (die Klassifikation selbst bleibt eine menschliche Entscheidung, die Datei ist ihre maschinenlesbare Form); **weiterhin handgepflegt**: Testlaufzahlen (nicht ableitbar, datiert) und das CLI-Coverage-Handinventar „Abgedeckt (29)" in `feature/platform/mcp-server.md` (eine eigene Population, jetzt mit Zählstand 2026-10-15 datiert)) |
| `agent-config` | nein | | Maschinenkonfiguration (globale `opencode.jsonc`, globale `AGENTS.md`); die projektbezogene Aussage der Änderung — ein Projekt verweist auf die Regeldatei, statt den Flow zu wiederholen — ist im Projekt erfüllt; die versteckten Skills der Range (`70d49e0`, `00dbd22`, `817c01e`) sind für dieses Projekt ohne Wirkung. **Belegter Stand (gelesen am 2026-10-21):** die globale `~/.config/opencode/opencode.jsonc` führt **keinen** `instructions`-Key (der Upstream-Wortlaut `0024d7e` ist damit am Projektstand bestätigt) |
| `laravel` | nein | | Rust-Workspace, kein PHP |
| `node-deps` | nein | | keine `package.json`/pn-Pins; Abhängigkeiten laufen über Cargo |
| `permissions` | nein | | Maschinenpolicy (`opencode.jsonc`), nicht Projektstand |
| `playwright-parallel` | nein | | keine Playwright-Suite; E2E sind `egui_kittest`-Targets in `crates/lumina-gui/tests/` |
| `react-i18n` | nein | | GUI ist egui/Rust mit eigener `Str`-i18n, kein React/Lingui |
| `tailscale-serve` | nein | | headless-Host, kein Dev-Server-Expose |
| `update-models` (ersetzt `model-updater` + `update-opencode-models`) | nein | | Modell-Registry ist Maschinenstand (`opencode.jsonc`), kein Projektstand; die Projekt-Entscheidung `MODELL` (nur freie Subagenten) bleibt unberührt. **Belegter Stand (gelesen am 2026-10-21 aus `opencode.jsonc` und `model-preferences.md`):** `namespaceRule: opencode-go-only`; Root-Modell und die vier Rollen `plan`/`general`/`build`/`explore` fahren `opencode-go/minimax-m3.1-flash-free`. Die Zeile folgt damit der upstream-Angleichung (`219171c`, `8c6b9af` — beide **vor** dem Cursor `00dbd22`): die Namespace-Regel steht auf der Entscheidung, nicht mehr auf einem Kontext-/Preisargument, und der `$0`-Vermerk gilt dem konfigurierten Modell. Der Skill ist seit `817c01e` **hidden** — für dieses Projekt ohne Wirkung |
| `web-audit` | nein | | kein Web-Frontend; LuminaRust ist Desktop-GUI + CLI |
| Konvention: 500-Zeilen-Ratetch | ja | `scripts/check_file_sizes.sh` (`THRESHOLD=500`), `scripts/file_size_baseline.txt`, `Agents.md` §„Verifizierungs-Agent" | |
| Konvention: Projektsprache Deutsch (Doku) | ja | die Projektdoku (`Agents.md`, `Agents.todo.md`, `feature/`, **auch diese Marker-Datei** — deutsch seit dem Anlegen in `b7c0799`; die englische Sprachregel gilt für das Skills-Repo, nicht für den Marker im Projekt). **Keine Sprachregel im Projekt** — **am committeten Stand dieses Commits** schreibt **keine** `.md` **außer diesem Marker** eine Sprachregel vor (`git grep -l 'Sprachregel' <stand> -- '*.md'` → **1** Datei: dieser Marker; die Konvention wird **beobachtet**, nicht vorgeschrieben); die **UI ist englisch** (Produktentscheid: `feature/platform/cli-gui-wasm.md` — „ist zum MVP **englischsprachig**“; `feature/platform/lightroom-ux-parity.md` UXG-17 — „F-100: englisch, 0 deutsche UI-Literale“) | |
| Konvention: Regel referenziert, nicht kopiert | ja | `Agents.md` §„Build-Agent" → Skill `build-verify` (Lieferweg: **Skill-Trigger**) und die Minimalregeln in `.agents/rules/build-verify.md` (agents-skills, Pfad-Verweis, keine Kopie); global eingebettet ist nur die Frage-Regel (`~/.config/opencode/AGENTS.md`, Quelle `ask.md`) | |
| Konvention: visueller Review mit dem konfigurierten Modell (DoD §6) | ja | `DoD.md` §6 (KI-Validierungs-Loop, Schritt 2), `Agents.todo.md` `F-103-N6-GUI-COVERAGE-27` Punkt 11, `feature/platform/lightroom-ux-parity.md` (Abnahme), `crates/lumina-gui-harness/README.md` — alle vier Stellen nennen **das konfigurierte Modell selbst** (max. 10 Bilder je Durchgang); ein Vision-Subagent existiert nicht | |
| Konvention: jede Zahl in einer Commit-Aussage nennt ihren Bezugsrahmen | ja | Zustand **und** Umfang: committeter Stand über `git archive <sha>` + `tar -x` in ein temporäres Verzeichnis (nie `git checkout`/`git restore`/`git reset` — Regel 3 von `build-verify`); die Pflicht erreicht das Projekt über den Skill-Zeiger in `Agents.md` §„Build-Agent" und ist seit Commit `8162323` praktisch angewendet (dort: „Working Tree 29, committeter Stand 24") | |

## Offen aus dem Bereich `00dbd22..0449b05`

Geprüft am 2026-10-21 gegen `0449b05` (Range `00dbd22..0449b05`; **7 Commits** —
`git -C <skills-repo> rev-list --count 00dbd22..0449b05`; **8** Skill-Verzeichnisse
berührt — `git -C <skills-repo> log --name-only --format= 00dbd22..0449b05 -- .agents/skills`
sortiert eindeutig: `agent-config`, `build-verify`, `github-ci-filters`,
`ratchet-gates`, `security-review`, `skills-marker`, `ui-review`, `update-models`).

**Neu**

- **Kein neuer Skill** in dieser Range: `git -C <skills-repo> log --diff-filter=A
  --name-only 00dbd22..0449b05 -- .agents/skills` listet nur die **zwei** neuen
  Referenzdateien `ratchet-gates/references/vocabulary.md` und
  `ratchet-gates/references/cli.md` — beide in einem **bestehenden** Skill.
  Der Skill `ratchet-gates` (`70d49e0`) stand bereits am Cursor `00dbd22`:
  die Skill-Stand-Tabelle des Marker-Commits `2334d00` (der den Cursor auf
  `00dbd22` setzte) führt `ratchet-gates` bereits als Zeile — es war **nichts**
  zu ergänzen.

**Geändert**

- `build-verify` (`817c01e`, `0024d7e`, `0449b05`) — **trifft zu und ist
  angewendet.** Drei Änderungen: (a) der Trigger wurde von **848** auf **428**
  Zeichen gekürzt (`817c01e`) und in `0024d7e` um die zwei Fälle „ein
  Repo-`AGENTS.md` dupliziert den Flow" und „Meilenstein-Review" wieder ergänzt
  (**484** Zeichen seit `0024d7e`, unverändert bis `0449b05`) — **Messgröße
  „delivered text length"**, die `node scripts/skill-budget.mjs` misst (Zeichen
  des vom Modell gesehenen Textes nach Quotes- und Escape-Auflösung; am
  Skills-Repo-HEAD `0449b05`: `maxDescriptionChars` **484**,
  `totalDescriptionChars` **689**, `activeSkills` **2**, **17** hidden); dieselben
  Skalare als **rohe YAML-Bytes** wären **853** (`00dbd22`) / **432**
  (`817c01e`) / **488** (`0024d7e` = `0449b05`) — für dieses
  Projekt ohne Wirkung (`Agents.md` §„Build-Agent" verweist auf den Skill und
  benennt den repo-spezifischen Teil, es wiederholt den Loop nicht); (b) der
  Wortlaut „nicht über einen Config-Key geliefert" statt „das
  `instructions`-Array ist leer" (`0024d7e`) — das Projekt hat keine
  Agent-Konfiguration, der Punkt ist am Projektstand bestätigt (kein
  `instructions`-Key in `~/.config/opencode/opencode.jsonc`); (c) die **neue
  Pflicht** (`0449b05`): jede Zahl in der Commit-Message, die etwas über den
  Commit oder seine Gates behauptet, nennt **was sie misst** — den Zustand
  (Arbeitsbaum / committeter Stand) **und** den Umfang (gefilterte Teilmenge,
  Range, Stufe); eine im dirty Arbeitsbaum gemessene Zahl ist nie eine Aussage
  über den Commit, beide Zustände werden getrennt berichtet; der committete
  Stand wird **gemessen, nicht ausgecheckt** (`git archive <sha>` + `tar -x`
  in ein temporäres Verzeichnis). **Am Projekt angewendet:** die Pflicht
  erreicht die Sessions über den Skill-Zeiger, und der Commit `8162323` führt
  sie bereits aus („Working Tree 29, committeter Stand 24", gemessen über
  `git archive dff6948` + `tar -x`) — der Upstream-Commit `0449b05` nennt
  denselben Commit als Zielform.
- `ratchet-gates` (`b0d1b51`, `d7888cd`) — **trifft zu**, die Projektanwendung
  ist aber **unvollständig** → offene Position `SKILLS-VOKABULAR-98`. Zwei
  Änderungen: (a) `references/vocabulary.md` legt **ein** Vokabular fest — die
  fünf Stufen `critical`/`high`/`medium`/`low`/`info`, die fünf Status
  `open`/`in-progress`/`fixed`/`accepted`/`wontfix`, die Zählregel (nur `open`
  und `in-progress` zählen; `accepted` und `wontfix` bleiben sichtbar — eine
  Entscheidung ist keine Löschung), „ein Befund braucht Owner **und** Ticket,
  außer er ist `fixed`", eine stabile ID, Severity und Status **getrennt**,
  ein unbekannter Wert ist ein Fehler und nie ein Fallback, und die Prosa
  bleibt der menschliche Record. Das Projekt hat zwei Findings-Logs mit
  eigener Form — das Register `feature/quality/security-review-register.md`
  (fixe Kopfzeile **ohne** Severity- und **ohne** Ticket-Spalte, Statuswerte
  `offen`/`in Welle`/`geschlossen`/`zurückgenommen`/`akzeptiert`) und die
  UXG/UXD-Lückeinventur in `feature/platform/lightroom-ux-parity.md` (Status
  im Lückentext, keine Severity, kein Owner) — beide sind noch nicht
  angeglichen; (b) `references/cli.md` ist die Karte vom Skill zur
  `ratchet`-Binary (eigenes Repo, `~/dev/ratchet-cli`) — **trifft nicht zu**:
  das Repo setzt die Mechanik selbst um (POSIX sh, `scripts/gates.sh` mit
  eigenen Baselines `scripts/file_size_baseline.txt` (500er, `fast-2`) /
  `scripts/file_size_ceiling.txt` (3000er, `fast-7`) und dem Drift-Gate
  `fast-6` (Tier-Liste gegen `ci.yml`)); ein externes Binary zu übernehmen wäre
  eine eigene Entscheidung, keine Pflicht des Skills. **Beim Anchor-Audit
  gefundener und geschlossener Befund:** `Agents.md` §„Verifizierung und Tests"
  nannte die schnelle Stufe falsch („vier Textgates", „mindestens 5 Checks") —
  die Zahlen sind am 2026-10-21 korrigiert (**acht** Textgates, Floor **9**,
  Quellen `sh scripts/gates.sh list` und `scripts/gates.sh` `MIN_CHECKS=9`), und
  die zugehörige Position ist aus dem Plan entfernt (Board-Konvention:
  geschlossene Positionen werden entfernt, nicht abgehakt).
- `security-review` (`b0d1b51`, `b9f140b`) — **trifft zu**, die Projektform
  trägt die neue Regel noch nicht → offene Position `SKILLS-VOKABULAR-98`.
  Stage 2 nimmt die Severity jetzt **aus den kanonischen Stufen**
  (`ratchet-gates`, `references/vocabulary.md`), die `reach × impact`-Matrix
  bleibt die Methode, nicht eine zweite Wortliste; `AKZEPTIERT` ist das
  kanonische `accepted` (eine Entscheidung, die sichtbar bleibt und nicht
  zählt); die Reihenfolge der Wellen kennt jetzt `info` (es benennt keinen
  Defekt); bei einer Wiederholungsrunde meldet der Verifikator `fixed` /
  `open` — ein wiederaufgetretener Befund ist ein **neuer seit der letzten
  Runde**, nicht ein neuer Statuswert. Das Register des Projekts hat eine
  **fixe** Kopfzeile und **null** Befunde; die erste Welle
  (`SEC-REVIEW-WELLE-1-85`, Teil b) ist noch offen — sie muss in die
  kanonische Form schreiben, sonst trägt die erste Welle eine fremde Form.
- `ui-review` (`b0d1b51`, `b9f140b`) — **trifft nicht zu**: die geänderte
  Findings-Vorlage (Spalten Id, Severity, Status, Owner, Ticket; die Stufen
  sind kanonisch; `APPROVED` erst, wenn kein `critical`/`high`-Befund
  **offen** (`open`/`in-progress`) ist; der Status ist ein eigenes Feld, nie
  ein Wert in der Severity-Zelle) gehört zum Playwright-Screenshot-Review,
  den dieses Projekt nicht hat (native egui-GUI; der visuelle Review läuft
  über `egui_kittest`-Goldens/PSNR und den Fenster-Review `UI-VISION-REVIEW-70`).
  Das Projekt nennt den Skill in **keiner Projektdatei außer diesem Marker
  und dem Board** (`Agents.todo.md`, Eintrag `SKILLS-VOKABULAR-98`) — **am
  committeten Stand dieses Commits**: **6 Zeilen** in **2** Dateien
  (`git grep -n ui-review <stand> -- '*.md'`; vorkommensweise **9**, eine Zeile
  trägt **4**); am **Vorgänger** `e744f8b` ist die **Dateimenge** dieselbe (**2**
  Dateien: `AGENTS.skills.md`, `Agents.todo.md`), der **Zeilenzähler** dort ist
  **5** — eine repo-weite Suche zählt die vom Commit geänderten Dateien mit; die
  Zusatzzeile kam mit `0cf154d`, seither bleibt der Zähler bei **6**. Das
  **kanonische Vokabular**, auf das die Vorlage zeigt, ist unter `ratchet-gates`
  erfasst — dort steht auch, was mit der UXG/UXD-Lückeinventur zu geschehen hat.
- `skills-marker` (`0024d7e`) — **trifft zu und ist angewendet** (diese
  Datei): der Wortlaut zu den eingebetteten Regeln ist korrigiert („kein
  Config-Key liefert Regeltext" statt „der `instructions`-Array ist leer"),
  ebenso der Schritt 6 der Vorlage. Der Abschnitt „Fortschreiben" unten trägt
  den neuen Wortlaut; die Skill-Stand-Tabelle bleibt unberührt, der Marker
  akkumuliert nicht.
- `agent-config` (`817c01e`, `0024d7e`) — **trifft nicht zu**
  (Maschinenkonfiguration): der Skill selbst ist jetzt **hidden**, die Liste
  der versteckten Skills wächst von 14 auf **17** (`agent-config`,
  `update-models`, `github-ci-filters` kommen hinzu; gemessen am Skills-Repo
  HEAD `0449b05` mit `grep -l 'autoinvoke: false' .agents/skills/*/SKILL.md`
  = 17 von 19 Skills, sichtbar bleiben `build-verify` und `ui-review`), und
  die Beschreibung der globalen Konfiguration ist berichtigt (kein
  `instructions`-Key). Die projektbezogene Aussage der Änderung („ein Projekt
  verweist auf die Regeldatei, statt den Flow zu wiederholen") ist im Projekt
  erfüllt.
- `github-ci-filters` (`817c01e`) — **trifft zu und ist angewendet**: die
  `paths-ignore`-Filter für `push` **und** `pull_request` stehen in
  `.github/workflows/ci.yml`. Die einzige Änderung ist `autoinvoke: false` —
  der Skill ist jetzt **hidden**; das Projekt nennt ihn in keiner Datei außer
  diesem Marker, es gibt also nichts umzuschreiben.
- `update-models` (`817c01e`) — **trifft nicht zu** (Maschinenstand): die
  einzige Änderung ist `autoinvoke: false` (jetzt **hidden**). Die
  belegten Modell-Angaben dieser Zeile wurden am 2026-10-21 erneut gelesen und
  unverändert bestätigt (`~/.config/opencode/model-preferences.md`:
  `namespaceRule: opencode-go-only`; `~/.config/opencode/opencode.jsonc`:
  Root-Modell und die Rollen `plan`/`general`/`build`/`explore` auf
  `opencode-go/minimax-m3.1-flash-free`).

**Entfernt**

- **Keine** — in dieser Range wurde kein Skill entfernt.

**Ohne Skill-Bezug** (Commits der Range, die keine Datei unter `.agents/`
berühren — geprüft je Commit mit `git show --name-only --format= <sha>`)

- `5a0c1b0` (`docs(todo): log the ratchet CLI decision`) — **1** von **7**
  Commits der Range; die einzige geänderte Datei ist `AGENTS.todo.md` des
  Skills-Repos (die Entscheidung, dass die `ratchet`-Binary ein eigenes Repo
  bekommt: `~/dev/ratchet-cli`). Die übrigen **6** Commits (`817c01e`,
  `0024d7e`, `b0d1b51`, `b9f140b`, `d7888cd`, `0449b05`) berühren jeweils
  `.agents/`.

## Fortschreiben

1. `agents-skills-consumed` ablesen → `<alt>`.
2. Im Skills-Repo die Range bilden:
   `git -C <skills-repo> log --oneline <alt>..HEAD`
   `git -C <skills-repo> log --name-only --format= <alt>..HEAD -- .agents/skills | sort -u`
3. Jeden betroffenen Skill gegen **dieses** Projekt prüfen. Trifft eine Änderung zu, wenn
   sie eine Pflicht, Schwelle, Ausnahme, Konvention oder ein Kommando betrifft, das
   **hier** gilt:
   - trifft zu und ist drin → `angewendet: ja` + stabile Referenz
   - trifft zu, ist nicht drin → offene Position: Eintrag in `Agents.todo.md` anlegen
   - trifft nicht zu (das Projekt hat das nicht) → `angewendet: nein` + kurzer Grund
4. Offene Positionen nach `Agents.todo.md`; Belege als **stabile Referenz**
   (Abschnitt/Überschrift, Task-ID, Suchstring) — Zeilennummern nur als
   Lesehilfe, sie driften. `Agents.todo.md` wird währenddessen von anderen
   Sessions angefasst: Einträge nur an einem sicheren Ende einfügen und
   **niemals** die fremden Änderungen mit committen.
5. Erst wenn für **jeden** Skill aus der Range eine Zeile steht:
   `agents-skills-consumed: $(git -C <skills-repo> rev-parse --short HEAD)`,
   `geprüft am:` auf heute, Abschnitte ersetzen — und **alle bestehenden Anker
   re-verifizieren**: Advancing ist kein Updating, der Marker akkumuliert nicht.
   **Bezugsrahmen — die gemeinsame Regel, zitiert statt kopiert:**
   `skills-marker/SKILL.md` → „Bezugsrahmen-Regel" (Skill-ID + Datei + Anker,
   **ohne Zeilennummer** — die driftet, der Anker bleibt); Wortlaut, Belege und
   Fälle: `skills-marker/references/notes.md` § „Der Bezugsrahmen". Kurzform zur
   Erinnerung, **die Fassung steht an der Zitierstelle, und weicht sie ab, gilt
   die Zitierstelle**: eine Messung, deren Umfang der Commit selbst schreibt,
   trägt **keinen** Vorgänger-Rahmen (die Zeile mit dem Suchkommando zählt mit);
   eine Aussage über **wann** etwas galt, nennt den Stand **und** den
   Messzeitpunkt; ein Marker **zitiert** die Regel, statt sie zu kopieren. Der
   Cursor `agents-skills-consumed` nennt das Skills-Repo — er ist kein Rahmen.
6. Falls Regeln oder Skills betroffen waren: die eingebetteten Regeln in der
   globalen `~/.config/opencode/AGENTS.md` gegen ihre Quelle in
   `.agents/rules/*` prüfen und bei geänderter **Pflicht** sinngemäß nachziehen.
   Drift ist eine geänderte Pflicht, kein abweichender Wortlaut — die Einbettung ist die
   Kurzform der Pflicht, kein wörtliches Abbild, und ein Text-Diff gegen die Quelle ist
   **kein** Drift. Dann `touch ~/.config/opencode/opencode.jsonc` (Reload der
   globalen Config; Always-on-Text liefert nur die Einbettung in eine
   `AGENTS.md`).
