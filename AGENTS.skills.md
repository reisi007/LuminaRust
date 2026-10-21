# Skills-Marker

agents-skills-consumed: 80664ab
geprüft am: 2026-10-21

**Lokale Ergänzung:** `<stand>` = der committete Stand dieses Commits (die SHA, die erst
mit dem Commit existiert). Der Cursor oben ist **kein** Rahmen — er zeigt den Stand
des Skills-Repos.

## Skill-Stand

| Skill | angewendet? | wo im Projekt | offene Position |
|---|---|---|---|
| `build-verify` | ja | `Agents.md` §„Rollen und Delegation" → „Build-Agent" (Zeiger auf den Skill + `.agents/rules/build-verify.md`, ohne Kopie) und §„Verifizierung und Tests" (repo-spezifische Gates); `Verify:`-Footer je Runde in der Historie (**40 von 40** Commits im Fenster der letzten 40 Commits **mit Ende `e744f8b`** tragen einen `Verify:`-Footer — gemessen mit `git rev-list --max-count=40 e744f8b` und `git show -s --format=%B`; **gemessen am Vorgänger `e744f8b`, seitdem unverändert** — das Fenster endet in der Vergangenheit, seine Menge ändert sich nicht; **am committeten Stand dieses Commits** tragen im Fenster der letzten 40 Commits **38 von 40** einen `Verify:`-Footer — die **zwei** Commits **`8123160`** und **`ee2c4d4`** tragen keinen); die **neue** Pflicht der Range (`0449b05`: jede Zahl, die etwas über den Commit behauptet, nennt ihren Bezugsrahmen — Zustand **und** Umfang; der committete Stand wird über `git archive <sha>` + `tar -x` gemessen, nie über `git checkout`) ist am Projekt **angewendet**: Commit `8162323` weist beide Bäume getrennt aus („Working Tree 29, committeter Stand 24") und misst den committeten Stand über `git archive dff6948` + `tar -x`. **Die Range `0449b05..80664ab` gibt Regel 6 eine Ausnahme** (`80664ab`: ein **eigener**, **frischer** Commit **dieser** Aufgabe darf nach dem Push mit `--force-with-lease` ersetzt werden, wenn **sechs** prüfbare Bedingungen zutreffen — eigen und aus dieser Aufgabe, der Remote-Head ist **noch genau** dieser Commit, keine fremde Arbeit im Baum, `--force-with-lease` nie `--force`, der **ersetzte SHA** wird im Body **und** im Bericht offengelegt, der Unterschied bleibt im Reflog abrufbar; ein grüner Lauf des **ersetzten** Commits ist **nichtig**, der neue SHA braucht seinen **eigenen**; **eine** Ersetzung je Commit): **am Projekt ändert sich nichts** — `Agents.md` §„Build-Agent" **verweist** auf den Skill, ohne eine Kopie, und das Projekt schreibt **keine** eigene Commit-Regel (`git grep -n -i 'amend' <stand> -- Agents.md Agents.headless.md Agents.todo.md` → **0** Treffer); die Ausnahme wird **automatisch** geerbt | |
| `skills-marker` | ja | diese Datei (`AGENTS.skills.md`); der Skill ist **hidden** (nur per exakter ID ladbar), der Abschnitt „Fortschreiben" unten ist die lokale Kopie des Ablaufs (Schritt 6 trägt den Wortlaut aus `0024d7e`: kein Config-Key liefert Regeltext — es gibt keinen `instructions`-Key). **Die Kopie ist am Stand `80664ab` nachgezogen**: ihre Kurzform führt jetzt **alle fünf** Regeln der Zitierstelle — die **Aufzählungs-Klausel** und die **Cursor-Klausel** kamen in der Range `0449b05..80664ab` dazu. **Die Zitierstelle löst am neuen Cursor auf**: `skills-marker/SKILL.md` → „Bezugsrahmen-Regel" steht am Stand `80664ab` **1**-mal in `SKILL.md`, **5**-mal in `references/notes.md` und **1**-mal in `templates/AGENTS.skills.md` (**7** Vorkommen im Skill-Verzeichnis) — am alten Cursor `0449b05` **0**-mal (der Anker kam mit `65fbecc`), deshalb musste der Cursor vorrücken | |
| `github-ci-filters` | ja | `.github/workflows/ci.yml` — `paths-ignore` für `push` **und** `pull_request` (Commit `c4e2fd8`); **gemessen am Vorgänger `8123160`, seitdem unverändert** (`git diff --stat 8123160..<stand> -- .github/workflows/ci.yml` ist leer): **2** `paths-ignore`-Blöcke (Zeilen 11 und 54) — dieselbe Zählung im **Vorgänger** `e744f8b` (`.github/workflows/ci.yml` ist seitdem unverändert). **Schritt 2 der neuen Regel (Form):** die Projektform ist die **Subtraktion** (`paths-ignore`), nicht die Auswahl — die vom neuen Wortlaut vorschriebene Form; die Auswahl-Form `paths` ist nur mit Catch-all im positiven Teil erlaubt, das Projekt braucht sie nicht. **Schritt 4 der neuen Regel (Diff-Beleg mit benannten Endpunkten):** `git diff --stat 2add471..c4e2fd8 -- .github/workflows/` → **1** Datei geändert, **+55** Zeilen, **0** Löschungen; `2add471` ist der Commit, der `.github/workflows/` zuletzt schrieb, `c4e2fd8` der Commit, der den Filter schrieb; der Diff enthält **nur** die beiden `paths-ignore`-Blöcke — keine Job-, Schritt- oder Timeout-Änderung. **Eine Abweichung ist gemessen und dem Eigentümer vorzulegen:** `.github/workflows/ci-libraw-image.yml` trägt einen **`paths:`-Block** (positiver Teil: `docker/Dockerfile`, `.github/workflows/ci-libraw-image.yml` — **kein** Catch-all, **keine** Negation), also die **Auswahl**-Form, die der neue Schritt 2 verbietet; der Workflow filtert **nicht** docs-only, sondern löst gezielt den Image-Publish aus — nach dem **Wortlaut** der Regel verboten, nach ihrer **Begründung** (still verlorene Pipeline-Abdeckung) trifft sie diesen Workflow nicht; das ist eine **Eigentümerentscheidung**, und dieser Lauf legt **keine** Board-Position an. Der Skill ist seit `817c01e` **hidden** — das Projekt nennt ihn in keiner Datei außer diesem Marker, es gibt also nichts umzuschreiben | |
| `codegraph-project-setup` | ja | `.codegraph/` (Index) + `.githooks/pre-commit` Gate 1 „CodeGraph index sync" (`core.hooksPath=.githooks`) — **kein** CodeGraph-Abschnitt in `Agents.md` (der Anker der Vorversion zeigte ins Leere) | |
| `docker-test-image` | ja | `docker/Dockerfile` (LibRaw 0.22.2, `ARG LIBRAW_SHA256`-Pin des Quell-Tarballs) + **fünf** `container:`-Jobs in `.github/workflows/ci.yml` (**gemessen am Vorgänger `8123160`, seitdem unverändert** — `git diff --stat 8123160..<stand> -- docker/Dockerfile .github/workflows/ci.yml` ist leer: `grep -c '^    container:'` = 5 — dieselbe Zählung im **Vorgänger** `e744f8b`; `docker/Dockerfile` und `.github/workflows/ci.yml` sind seitdem unverändert) | |
| `ghcr-visibility` | ja | `.github/workflows/ci.yml` — `ghcr.io/reisi007/luminarust/lumina-ci:latest` in denselben fünf `container:`-Jobs (**gemessen am Vorgänger `8123160`, seitdem unverändert** (`git diff --stat 8123160..<stand> -- .github/workflows/ci.yml` ist leer): **5** `image: ghcr.io`-Zeilen — dieselbe Zählung im **Vorgänger** `e744f8b`; `.github/workflows/ci.yml` ist seitdem unverändert) | |
| `security-review` | ja | Register: `feature/quality/security-review-register.md` (Form nach Stage 6 — Zwei-Hälften-Form, Registertabelle, Pflege durch den Build-Agent; Enumerationsbasis mit den erreichbaren Oberflächen, vollständig enumeriert in Welle 1 am 2026-10-20 — die Zahl **24** Oberflächen steht im generierten Block **surfaces** in §5 des Registers — **gemessen am Vorgänger `8123160`, seitdem unverändert** (`git diff --stat 8123160..<stand> -- feature/quality/security-review-register.md` ist leer) dieselbe Zählung wie im **Vorgänger** `e744f8b` (das Register ist seitdem unverändert); **bisher null Befunde**: die Registertabelle trägt nur die Beispielzeile `SEC-EXAMPLE`); die erste Review-Welle ist **am committeten Stand dieses Commits** die offene Task `SEC-REVIEW-WELLE-1-85` (Teil a erledigt, die Teile b/c brauchen Läufe); die **Scope-Frage GUI** ist **entschieden** (Eigentümerentscheid 2026-10-20: GUI-Oberflächen werden **gleichrangig** in jeder Welle mitgeprüft) und im Register entsprechend vermerkt. **Die geänderte Regel der Range** (Severity aus den kanonischen Stufen; `AKZEPTIERT` ist das kanonische `accepted`) trifft die Projektform des Registers: die fixe Kopfzeile hat **keine** Severity-Spalte und **keine** Ticket-Spalte | `Agents.todo.md` → `SKILLS-VOKABULAR-98` |
| `ui-review` | nein | als Playwright-Flow trifft es nicht zu: native egui-GUI, der visuelle Review läuft über `egui_kittest`-Goldens/PSNR und den Fenster-Review `UI-VISION-REVIEW-70`. Die Vision-Regel (kein Vision-Subagent: das konfigurierte Modell liest die Bilder selbst) stammt aus einer **früheren Range**, **nicht** aus der auditierten: sie steht seit `8bfc321` in `ui-review/SKILL.md` → „there is no vision subagent in the loop" und wurde in `dccdd40` konsolidiert → „the configured model reads images" — beide Stellen als **Anker** zitiert (Pfad im Skill: **ID + Datei**; Anker: **wörtliches** Textzitat, **ohne** Zeilennummer, weil die driftet), gemessen am Stand `80664ab`: **je 1** Treffer im Skill-Verzeichnis (`git grep -c 'there is no vision subagent in the loop' 80664ab -- .agents/skills/ui-review/` = **1**, `git grep -c 'the configured model reads images' 80664ab -- .agents/skills/ui-review/` = **1**); die **letzte** Vision-Änderung **vor** dem Cursor `00dbd22` ist `fa2268f` — sie ändert **eine** Zeile in `references/findings-report.md` (der „vision-agent"-Wortlaut). In der auditierten Range `00dbd22..0449b05` ändert `ui-review` ausschließlich `references/findings-report.md`; **keine** Vision-Zeile (`git diff 00dbd22..0449b05 -- .agents/skills/ui-review/ \| grep -i vision` ist leer). Die Regel trifft die Projekt-Pflicht in `DoD.md` §6 → dort **umgesetzt** (**gemessen am Vorgänger `8123160`, seitdem unverändert** — `git diff --stat 8123160..<stand> -- DoD.md` ist leer: dieselbe Aussage wie im **Vorgänger** `e744f8b`). Die **Findings-Vorlage** (`b0d1b51`: die **neun** Spalten `Id \| Severity \| Status \| Owner \| Ticket \| File:Line \| Screenshot \| Finding \| Suggested fix` — der Upstream-Skill zitiert sie seit `2e09608` **wörtlich**, vorher zitiert er nur fünf) trifft dieses Projekt **nicht** — das Projekt führt keine UI-Findings-Liste nach dieser Vorlage (die UXG/UXD-Lückeinventur in `feature/platform/lightroom-ux-parity.md` ist eine SOLL-Lücke, kein Review-Report); das **kanonische Vokabular**, auf das die Vorlage zeigt, ist unter `ratchet-gates` erfasst — der Upstream-Skill führt seit `2e09608` **selbst keine** Severity-Liste mehr, sondern zeigt auf dasselbe Vokabular. Der Skill wird im Projekt nur in **diesem Marker** und im **Board** (`Agents.todo.md`, Eintrag `SKILLS-VOKABULAR-98`) genannt, in keiner weiteren Projektdatei — **am committeten Stand dieses Commits**: **7 Zeilen** in **2** Dateien (`git grep -n ui-review <stand> -- '*.md'`; vorkommensweise **14** — `git grep -o ui-review <stand> -- '*.md'`, eine Zeile trägt **8**); am **Stand `e744f8b`** (dort gemessen — die Datei hat sich seither geändert, das ist die Form „gemessen am Stand", keine „unverändert seit"-Aussage) ist die **Dateimenge** dieselbe (**2** Dateien: `AGENTS.skills.md`, `Agents.todo.md`), der **Zeilenzähler** dort ist **5** — eine repo-weite Suche zählt die vom Commit geänderten Dateien mit; die Zusatzzeile kam mit `0cf154d`, seither bleibt der Zähler bei **6** | |
| `ratchet-gates` | ja | `scripts/gates.sh` (Stufen `fast`/`slow`/`all`/`list`/`list-ci`, Floor `MIN_CHECKS=9`, keine Codegen-Stufe `fast`), `feature/quality/local-gates.md` (Tier-Tabelle wortgleich zu `gates.sh list` — byteweise diffleer geprüft; **gemessen am Vorgänger `8123160`, seitdem unverändert** — `git diff --stat 8123160..<stand> -- scripts/gates.sh feature/quality/local-gates.md` ist leer; dieselbe Zählung wie im **Vorgänger** `e744f8b` — `scripts/gates.sh`, `local-gates.md` und die Gate-Skripte sind seitdem unverändert; Floor-Regel; Lückenliste), `Agents.md` §„Verifizierung und Tests" (der dortige Floor-Text nannte beim Marker-Run 2026-10-21 **vier** Textgates und **5** Checks; **korrigiert in `0cf154d`** — **gemessen am Vorgänger `8123160`, seitdem unverändert** (`git diff --stat 8123160..<stand> -- Agents.md` ist leer): **acht** Textgates und Floor **9**, Quellen `sh scripts/gates.sh list` und `scripts/gates.sh` `MIN_CHECKS=9`), CI-Schritt „Run fast tier (scripts/gates.sh fast)" im Job „Rust fast checks" (`.github/workflows/ci.yml`), Gate-Header der **elf** `scripts/check_*.sh` (Defektklasse, Anlass, Ratchet-Richtung, benannte Grenze; **gemessen am Vorgänger `8123160`, seitdem unverändert** — `git diff --stat 8123160..<stand> -- 'scripts/check_*.sh'` ist leer: **11** Skripte, jedes mit einem `Defektklasse`-Header — dieselbe Zählung im **Vorgänger** `e744f8b`). **Die Range `0449b05..842e055` ändert nur die CLI-Karte** (`ratchet-gates/references/cli.md`, `d5dc52f`: die neue Zeile `ratchet release`, der eigene Exit-Middle `1` der Release-Stufe und der **umgedrehte** Installationsabschnitt „**No tag was pushed yet** … build from source" → „The releases are published") — die **Gate-Substanz** (`SKILL.md`, `references/vocabulary.md`) ist **unberührt**; das Projekt setzt die Mechanik selbst um (POSIX sh, `scripts/gates.sh`), die `ratchet`-Binary ist ein eigenes Repo, ihre Release-Aussage betrifft es nicht — an der Projektanwendung ändert sich **nichts** | `Agents.todo.md` → `SKILLS-VOKABULAR-98` (kanonisches Vokabular) |
| `generated-docs` | ja | `scripts/gen_doc_facts.sh` (Generator: regelbasiert, deterministisch, git-frei, POSIX sh; liest die Schwellen 500/3000 aus den Gate-Skripten, fuehrt nie etwas aus einem gelesenen Dokument aus) + `scripts/check_doc_facts_drift.sh` (Drift-Gate: erzeugen -> Block aus der Zieldatei extrahieren -> diff; meldet, schreibt nie) als `fast-8` und `scripts/check_doc_facts_generator.sh` (Hygiene-Gate des Generators: Determinismus + Git-Freiheit) als `fast-9` der Stufe `fast`; generierte Bloecke `file-sizes` + `population` in `feature/quality/file-size-policy.md`, `goldens` in `feature/README.md` + `feature/quality/golden-references.md` + `feature/quality/golden-fixtures.md`, `mcp-tools` in `feature/platform/mcp-server.md` + `feature/quality/security-review-register.md`, `perf-baseline` in `feature/quality/performance-benchmarks.md`, `crates` in `feature/README.md` und `surfaces` in `feature/quality/security-review-register.md`, `golden-classes` in `feature/quality/golden-fixtures.md` und `golden-targets` in `feature/quality/golden-references.md` (Quelle: die committete Datei `scripts/golden_inventory.txt`) — **gemessen am Vorgänger `8123160`, seitdem unverändert** (`git diff --stat 8123160..<stand> -- scripts/gen_doc_facts.sh scripts/golden_inventory.txt feature/README.md feature/platform/mcp-server.md feature/quality/file-size-policy.md feature/quality/golden-fixtures.md feature/quality/golden-references.md feature/quality/performance-benchmarks.md feature/quality/security-review-register.md` ist leer): **12** Bloecke in **7** Dateien (dieselbe Zählung im **Vorgänger** `e744f8b`; Generator und Zieldateien sind seitdem unverändert); **Zählregel: das Generator-Manifest** (`sh scripts/gen_doc_facts.sh --manifest`: eine Zeile je Block-Instanz, `block-id\|zieldatei` — **12** Zeilen, **7** verschiedene Zieldateien — gemessen am Vorgänger `8123160`, seitdem unverändert). Die Markensuche im Baum (`git grep -n '<!-- BEGIN GENERATED:' <stand> -- '*.md'`) liefert **am committeten Stand dieses Commits** **14** Treffer in **9** Dateien: die **12** echten Bloecke plus **zwei** Literal-Erwähnungen der Marker-Syntax (`feature/quality/local-gates.md` §8.1 und diese Zeile) — deshalb ist das Manifest die Zählregel, nicht die Markensuche; CI-Schritt „Run fast tier (scripts/gates.sh fast)" im Job „Rust fast checks" (`.github/workflows/ci.yml`) | `Agents.todo.md` → `DOC-ZAHLEN-DRIFT-82` (Welle 1 umgesetzt; **Welle 2 im Repo umgesetzt** (2026-10-20): `mcp-tools`, `perf-baseline`, `crates`, `surfaces` — der Todo-Eintrag `DOC-ZAHLEN-DRIFT-82` ist nachgezogen; **Welle 3 im Repo umgesetzt** (2026-10-20): der R/C-Split der Goldens und die Verteilung auf die Test-Targets werden jetzt aus der committeten Quelldatei `scripts/golden_inventory.txt` **erzeugt** (die Klassifikation selbst bleibt eine menschliche Entscheidung, die Datei ist ihre maschinenlesbare Form); **weiterhin handgepflegt**: Testlaufzahlen (nicht ableitbar, datiert) und das CLI-Coverage-Handinventar „Abgedeckt (29)" in `feature/platform/mcp-server.md` (eine eigene Population, jetzt mit Zählstand 2026-10-15 datiert)) |
| `agent-config` | nein | | Maschinenkonfiguration (globale `opencode.jsonc`, globale `AGENTS.md`); die projektbezogene Aussage der Änderung — ein Projekt verweist auf die Regeldatei, statt den Flow zu wiederholen — ist im Projekt erfüllt; die versteckten Skills der Range (`70d49e0`, `00dbd22`, `817c01e`) sind für dieses Projekt ohne Wirkung. **Belegter Stand (gelesen am 2026-10-21):** die globale `~/.config/opencode/opencode.jsonc` führt **keinen** `instructions`-Key (der Upstream-Wortlaut `0024d7e` ist damit am Projektstand bestätigt). **Die Range `0449b05..80664ab` ergänzt §6** (`80664ab`) um die Kurzform der Amend-Ausnahme von `build-verify` samt Zeiger auf `build-verify/references/amend.md` — **am Projekt ändert sich nichts**: die globale `~/.config/opencode/AGENTS.md` trägt **keinen** build-verify-Text (`grep -n -i 'build-verify\|amend\|force-with-lease' ~/.config/opencode/AGENTS.md` → **0** Treffer), es ist nichts neu einzubetten und kein Reload nötig |
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

## Offen aus dem Bereich `0449b05..80664ab`

Geprüft am **2026-10-21**, **21:41:43 UTC** (Anfangsmessung), **21:49:26 UTC**
(Zwischenmessung) und **21:53:33 UTC** (Endmessung) gegen `80664ab` (Range
`0449b05..80664ab`; **17 Commits** — `git -C <skills-repo> rev-list --count
0449b05..80664ab`; **6** Skill-Verzeichnisse berührt — `git -C <skills-repo> log
--name-only --format= 0449b05..80664ab -- .agents/skills` sortiert eindeutig:
`agent-config`, `build-verify`, `github-ci-filters`, `ratchet-gates`, `skills-marker`,
`ui-review`). **Der Skills-Repo-Stand wurde am Anfang, in der Mitte und am Ende des
Laufs gemessen:** am Anfang stand er auf `4205142`, er ist während des Laufs zweimal
weitergegangen — erst auf `842e055` (`842e055`, `2e09608`, `80664ab`), dann auf
`80664ab`. **Alle drei nachträglich gelandeten Commits sind in dieser Range
mitgeprüft.**

**Neu**

- **Kein neuer Skill** in dieser Range: `git -C <skills-repo> log --diff-filter=A
  --name-only --format= 0449b05..80664ab -- .agents/skills` ist **leer**. Die Zahl der
  Skills ist an beiden Reven **19** (`git ls-tree -r --name-only <rev> .agents/skills |
  grep -c 'SKILL.md$'`), die Zahl der **versteckten** Skills
  (`metadata.opencode/autoinvoke: false`) an beiden Reven **17** von **19**
  (`git grep -l 'autoinvoke: false' <rev> -- '.agents/skills/*/SKILL.md'`) —
  **unverändert**; sichtbar bleiben `build-verify` und `ui-review`.

**Geändert**

- `skills-marker` (**12** Commits: `e523df9`, `21b29e4`, `7341bb0`, `992a9eb`,
  `65fbecc`, `f7b8f9c`, `820c140`, `37cefe2`, `c056e75`, `857070a`, `4205142`,
  `842e055`; **3** Dateien: `SKILL.md` (+64/-58), `references/notes.md` (+436/-1),
  `templates/AGENTS.skills.md` (+34/-0)) — **trifft zu und ist angewendet** (diese
  Datei). Die Regel selbst ändert sich in **fünf** Punkten, alle betreffen diesen
  Marker: (a) die **Zitierform** (`65fbecc`) — die gemeinsame Regel wird über
  **Pfad im Skill (ID + Datei) + Anker** zitiert, **ohne Zeilennummer**, weil die
  Zeilennummer driftet; (b) die **Cursor-Klausel** (`65fbecc`) — ein Verweis trägt nur,
  wenn der **Cursor** den Stand deckt, in dem die Zitierstelle existiert: wer zitiert,
  rückt den Cursor vor und begeht die Range dazwischen; (c) die **Aufzählungs-Klausel**
  (`37cefe2`) — eine Aufzählung trägt **zwei** Behauptungen (jedes Element trägt die
  Eigenschaft **und** die Liste ist die ganze Menge), beide sind zu belegen, eine
  handgepflegte Aufzählung wird durch ein Kommando ersetzt, das die Menge ableitet;
  (d) die **Nomen-Regel** (`37cefe2`) — die Formen (b) und (c) unterscheiden sich an der
  **Zeitform**, nicht am Wort; „Vorgänger" heißt **Vorfahr**: der genannte SHA ist der
  **Messstand**, nicht zwingend der direkte Vorgänger; (e) die **fünf Regeln** stehen
  jetzt an der Zitierstelle (`SKILL.md`, `references/notes.md` § „Der Bezugsrahmen"),
  die Vorlage trägt sie als **Kurzform zur Erinnerung** mit dem Vorrang der
  Zitierstelle. **Was das an der Anwendung im Projekt ändert:** die Zitierstelle des
  Markers (`skills-marker/SKILL.md` → „Bezugsrahmen-Regel") löst am alten Cursor
  `0449b05` **nicht** auf (**0** Treffer in `.agents/skills/skills-marker/`) — der Anker
  kam mit `65fbecc`. Nach der Cursor-Klausel muss der Cursor deshalb auf einen Stand
  vorrücken, in dem die Zitierstelle existiert, **und** die Range dazwischen muss
  begangen werden; dieser Commit tut beides (der Cursor steht jetzt auf `80664ab`, die
  Range `0449b05..80664ab` ist dieser Abschnitt). **Belege:** die Zeichenkette
  `Bezugsrahmen-Regel` steht am Stand `80664ab` **7**-mal im Skill-Verzeichnis —
  `SKILL.md` **1**, `references/notes.md` **5**, `templates/AGENTS.skills.md` **1**
  (`git grep -c 'Bezugsrahmen-Regel' 80664ab -- .agents/skills/skills-marker/`) — und
  am Stand `65fbecc` **4**-mal (`SKILL.md` **1**, `references/notes.md` **2**,
  `templates/AGENTS.skills.md` **1**). Die **Aufzählungs-Klausel** ist am Projekt
  erfüllt: jede Zählregel im Marker ist ein **Kommando**, keine Handliste
  (`git grep -c …`, `grep -c '^    container:'`, `sh scripts/gates.sh list`,
  `sh scripts/gen_doc_facts.sh --manifest`, `git rev-list --max-count=40 <sha>`) —
  keine handgepflegte Liste steht im Marker. Die lokale Kopie des Ablaufs im Abschnitt
  „Fortschreiben" ist am Stand `80664ab` **nachgezogen**: ihre Kurzform führt jetzt
  **alle fünf** Regeln (die Aufzählungs- und die Cursor-Klausel kamen in dieser Range
  dazu). **Ein Nebenbefund, ohne Änderung am Projekt:** die Vorlage nennt in Schritt 4
  noch „angewendete Zeilen mit `Datei:Zeile` belegen", während `SKILL.md` §3 die
  **stabile Referenz** mit der Zeile als Lesehilfe vorschreibt — das Projekt folgt
  `SKILL.md` §3 (das Format wird dort festgelegt, §1), die Vorlage hinkt nach.
- `ratchet-gates` (`d5dc52f`; **1** Datei: `references/cli.md`, +7/-3) — **trifft zu**,
  die Projektanwendung **trägt**. Gemessen wurde **beides**: die **Gate-Substanz**
  (`SKILL.md`, `references/vocabulary.md`) ist in der Range **unberührt**
  (`git diff --stat 0449b05..80664ab -- .agents/skills/ratchet-gates/SKILL.md
  .agents/skills/ratchet-gates/references/vocabulary.md` ist **leer**) — die Pflichten,
  die das Projekt anwendet (Floor `MIN_CHECKS=9`, Tier-Tabelle, kanonisches Vokabular),
  sind unverändert; die **CLI-Karte** (`references/cli.md`) hat **drei** Änderungen:
  die neue Zeile `ratchet release` (Version und Changelog sind abgeleitete Fakten,
  nicht handgeschrieben), der **eigene Exit-Middle** der Release-Stufe (`1`, wenn
  es nichts zu releasen gibt — das Werkzeug tat, was es soll, das Release existiert
  nicht) und der **umgedrehte** Installationsabschnitt: „**No tag was pushed yet** …
  build from source" → „The releases are published: every version tag carries the
  assets of five platforms and a `checksums.txt`". **Was das an der Anwendung im
  Projekt ändert: nichts** — das Projekt setzt die Mechanik selbst um (POSIX sh,
  `scripts/gates.sh` mit eigenen Baselines und dem Drift-Gate `fast-6`); die
  `ratchet`-Binary ist ein eigenes Repo (`~/dev/ratchet-cli`), ihre Release-Aussage
  betrifft ein Werkzeug, das dieses Projekt nicht benutzt. Der Verdikt `ja` mit der
  Board-Position `Agents.todo.md` → `SKILLS-VOKABULAR-98` **trägt**: die offene Position
  betrifft das **kanonische Vokabular** (`references/vocabulary.md`), und diese Datei
  ist in der Range unverändert. **Achte auf Selbstbezüge:** der Marker zitiert
  `ratchet-gates/references/cli.md` als **Pfad** (die Karte der `ratchet`-Binary),
  **nicht** die geänderte Aussage (die Release-Zeile) — die Messung ist damit
  **nicht selbstreferenziell**, der Vorgänger-Rahmen ist erlaubt.
- `github-ci-filters` (`7328fc3`; **2** Dateien: `SKILL.md` +78/-102,
  `references/notes.md` +111/-21) — **trifft zu und ist angewendet**. Die Regel wurde
  in **zwei** Schritten korrigiert: **Schritt 2** — **Auswahl** ist verboten,
  **Subtraktion** erlaubt: eine `paths`-Liste, deren **positiver** Teil den Catch-all
  `'**'` enthält, zieht nur ab (sie löst dieselben Pfade aus wie das
  `paths-ignore`-Gegenteil, nie weniger); ein positiver Teil, der **nur bestimmte**
  Pfade nennt, **wählt aus** und ist verboten (still verlorene Abdeckung, kein
  Fehler); `paths-ignore` bleibt die bevorzugte Form, und wo der Eigentümer die
  `paths`-Form entscheidet, ist die Entscheidung am Filter zu dokumentieren.
  **Schritt 4** — der Workflow-Diff braucht **benannte Endpunkte**:
  `git diff --stat <sha-before>..<sha-after> -- .github/workflows/`; der
  Arbeitsbaum-Diff (`git diff -- .github/workflows/`) misst den **uncommitteten**
  Zustand und ist nach dem Commit leer. **Erst geprüft, ob das Projekt den Skill
  überhaupt anwendet** (das Projekt filtert seine Workflows per Pfad — gemessen:
  `git grep -nE '^\s*(paths|paths-ignore):' -- .github/workflows/` → **3** Treffer:
  **2** `paths-ignore` in `ci.yml` (Zeilen 11 und 54) und **1** `paths:` in
  `ci-libraw-image.yml` (Zeile 21)). **Form gegen die neue Regel:** die Form von
  `ci.yml` ist die **Subtraktion** (`paths-ignore`, beide Trigger) — die vom neuen
  Wortlaut vorschriebene Form; am Projekt ändert sich **nichts**, die Erweiterung
  (Catch-all-`paths` ist erlaubt) braucht das Projekt nicht. **Der Beleg des Markers
  trug den neuen Schritt-4-Befehl nicht** (er nannte nur den Commit `c4e2fd8`, kein
  Diff-Kommando, keinen benannten Endpunkt) → jetzt getragen:
  `git diff --stat 2add471..c4e2fd8 -- .github/workflows/` → **1** Datei geändert,
  **+55** Zeilen, **0** Löschungen; `2add471` ist der Commit, der `.github/workflows/`
  zuletzt schrieb, `c4e2fd8` der Commit, der den Filter schrieb; der Diff enthält
  **nur** die beiden `paths-ignore`-Blöcke — keine Job-, Schritt- oder
  Timeout-Änderung, kein `continue-on-error`, kein Required-Check. **Eine Abweichung
  ist gemessen und dem Eigentümer vorzulegen:** `.github/workflows/ci-libraw-image.yml`
  trägt einen **`paths:`-Block** (positiver Teil: `docker/Dockerfile`,
  `.github/workflows/ci-libraw-image.yml` — **kein** Catch-all, **keine** Negation),
  also die **Auswahl**-Form, die der neue Schritt 2 verbietet. Der Workflow filtert
  **nicht** docs-only, sondern löst gezielt den Image-Publish aus; nach dem **Wortlaut**
  der Regel ist die Form verboten, nach ihrer **Begründung** (still verlorene
  Pipeline-Abdeckung) trifft sie diesen Workflow nicht — das ist eine
  **Eigentümerentscheidung**, und dieser Lauf legt **keine** Board-Position an.
- `ui-review` (`2e09608`; **1** Datei: `SKILL.md`, +5/-9) — **trifft nicht zu** (der
  Verdikt bleibt `nein`): das Projekt hat keinen Playwright-Screenshot-Review (native
  egui-GUI; der visuelle Review läuft über `egui_kittest`-Goldens/PSNR und den
  Fenster-Review `UI-VISION-REVIEW-70`). **Was die Änderung tut:** Schritt 3 des Skills
  zitierte die Report-Spalten als **fünf** (`Severity | File:Line | Screenshot | Finding
  | Suggested fix`), obwohl die Vorlage seit `b0d1b51` **neun** trägt (`Id | Severity |
  Status | Owner | Ticket | File:Line | Screenshot | Finding | Suggested fix`), und
  führte eine **eigene** Severity-Liste mit **vier** Stufen (`critical`, `high`,
  `medium`, `low`) — eine zweite Wortliste neben dem kanonischen Vokabular
  (`ratchet-gates/references/vocabulary.md`, **fünf** Stufen, `info` fehlte). Beides ist
  korrigiert: die Spalten werden wörtlich zitiert, die Severity-Liste ist ein **Zeiger**
  auf das kanonische Vokabular. **Am Projekt ändert sich nichts** — der Skill wird
  hier nicht angewendet; die Richtung der Korrektur deckt sich mit dem, was der Marker
  ohnehin festhält (das **kanonische Vokabular**, auf das die Vorlage zeigt, ist unter
  `ratchet-gates` erfasst) und mit der Board-Position `SKILLS-VOKABULAR-98`, die die
  kanonischen Stufen für die **eigenen** Findings-Listen des Projekts (Register,
  UXG/UXD-Lückeinventur) entscheidet. Die **Vision-Zeile** ist von der Änderung
  **nicht** berührt: die Anker „there is no vision subagent in the loop" und „the
  configured model reads images" stehen am Stand `80664ab` unverändert in
  `ui-review/SKILL.md` (je **1** Treffer).
- `build-verify` (`80664ab`; **4** Dateien: `.agents/rules/build-verify.md` +2/-2,
  `build-verify/SKILL.md` +1/-1, `build-verify/references/amend.md` +43/-5,
  `build-verify/references/notes.md` +6/-4) — **trifft zu und ist angewendet**; an der
  Anwendung im Projekt ändert sich **nichts**. Die Änderung gibt **Regel 6** (Amend
  nur, solange der Commit ungepusht ist) **eine** Ausnahme: ein **eigener**, **frischer**
  Commit **dieser** Aufgabe, der schon gepusht ist, darf mit `--force-with-lease`
  ersetzt werden, wenn **sechs** prüfbare Bedingungen zutreffen (eigen und aus dieser
  Aufgabe; der Remote-Head ist **noch genau** dieser Commit; keine fremde Arbeit im
  Baum; `--force-with-lease`, nie `--force`; der **ersetzte SHA** wird im Commit-Body
  **und** im Bericht an den Eigentümer offengelegt; der Unterschied bleibt im Reflog
  abrufbar). Dazu: ein grüner Lauf des **ersetzten** Commits ist **nichtig** — der neue
  SHA braucht seinen **eigenen** grünen Lauf; **eine** Ersetzung je Commit; und die
  **eigere Branch-Policy eines Repos schlägt** (das Skills-Repo selbst behält die
  strenge Regel, weil seine SHAs der `skills-marker`-Cursor anderer Projekte sind).
  **Am Projekt:** `Agents.md` §„Build-Agent" **verweist** auf den Skill und
  `.agents/rules/build-verify.md`, ohne eine Kopie (gemessen: `git grep -n -i 'amend'
  <stand> -- Agents.md Agents.headless.md Agents.todo.md` → **0** Treffer — das Projekt
  schreibt **keine** eigene Commit-Regel) → die Ausnahme wird **automatisch** geerbt,
  es ist **nichts** umzuschreiben; der Verdikt `ja` **trägt**. Der Verweis-Pfad
  `.agents/rules/build-verify.md` bleibt auflösbar (die Datei existiert am Stand
  `80664ab`).
- `agent-config` (`80664ab`; **1** Datei: `SKILL.md`, +6/-0) — **trifft nicht zu**
  (der Verdikt bleibt `nein`): Maschinenkonfiguration (globale `opencode.jsonc`,
  globale `AGENTS.md`), nicht Projektstand. Die Änderung ergänzt §6 um die Kurzform der
  Amend-Ausnahme samt Zeiger auf `build-verify/references/amend.md` und um den Satz,
  dass **das Skills-Repo selbst** die Ausnahme **nicht** nimmt (das gilt für dieses
  Repo, nicht für LuminaRust). **Am Projekt ändert sich nichts**: die globale
  `~/.config/opencode/AGENTS.md` trägt **keinen** build-verify-Text (gemessen:
  `grep -n -i 'build-verify\|amend\|force-with-lease' ~/.config/opencode/AGENTS.md` →
  **0** Treffer) → es ist nichts neu einzubetten und kein Reload nötig.

**Entfernt**

- **Keine** — in dieser Range wurde kein Skill entfernt. (Der Skills-Repo-Commit
  `2e09608` hat eine **Position im Board des Skills-Repos** entfernt — die
  upstream-eigene Liste der Severity-Stufen im `ui-review`; im Board **dieses**
  Projekts stand dazu keine Position.)

**Ohne Skill-Bezug** (Commits der Range, die keine Datei unter `.agents/` berühren —
geprüft je Commit mit `git show --name-only --format= <sha>`)

- `7910ec6` (`docs(todo): the website's findings are migrated — the re-grade lives
  there now`) — **1** von **17** Commits der Range; die einzige geänderte Datei ist
  `AGENTS.todo.md` des Skills-Repos. Die übrigen **16** Commits (`d5dc52f`,
  `e523df9`, `21b29e4`, `7341bb0`, `992a9eb`, `65fbecc`, `f7b8f9c`, `820c140`,
  `37cefe2`, `7328fc3`, `c056e75`, `857070a`, `4205142`, `842e055`, `2e09608`,
  `80664ab`) berühren jeweils `.agents/`.

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
   Lesehilfe, sie driften.
   **Lokale Ergänzung:** `Agents.todo.md` wird währenddessen von anderen
   Sessions angefasst: Einträge nur an einem sicheren Ende einfügen und
   **niemals** die fremden Änderungen mit committen.
   Bezugsrahmen — die gemeinsame Regel, **zitiert statt kopiert:**
   `skills-marker/SKILL.md` → „Bezugsrahmen-Regel" (Skill-ID + Datei + Anker,
   **ohne** Zeilennummer — die driftet, der Anker bleibt); Wortlaut, Belege und
   Fälle: `skills-marker/references/notes.md` § „Der Bezugsrahmen". Die Liste
   nennt die **Fälle**, nicht die Fassung — steht eine Form nicht an der
   Zitierstelle, ist sie hier falsch: (a) was der Commit selbst festschreibt
   (Marker, Board, geänderte Dateien); (b) eine Fremddatei, die der Commit nicht
   anfasst und die seit dem genannten Stand **unverändert** ist; (c) eine Datei,
   die sich seit dem genannten Stand **geändert** hat. Der genannte SHA ist der
   **Messstand** — ein **Vorfahr**, nicht zwingend der direkte Vorgänger; (b) und
   (c) unterscheiden sich an der **Zeitform**, nicht am Wort. Kurzform zur
   Erinnerung, **die Fassung steht an der Zitierstelle, und weicht sie ab, gilt
   die Zitierstelle**: eine Messung, deren Umfang der Commit selbst schreibt,
   trägt **keinen** Vorgänger-Rahmen (die Zeile mit dem Suchkommando zählt mit);
   eine Aussage über **wann** etwas galt, nennt den Stand **und** den
   Messzeitpunkt; ein Marker **zitiert** die Regel, statt sie zu kopieren — eine
   eigene Zusatzregel des Projekts ist erlaubt, wenn sie als **lokale Ergänzung**
   gekennzeichnet ist und die gemeinsame Regel nicht ersetzt (hier: die
   `**Lokale Ergänzung:**`-Zeile oben); eine **Aufzählung** trägt zwei Behauptungen
   (jedes Element trägt die Eigenschaft, und die Liste ist die ganze Menge — beides
   ist zu belegen; eine handgepflegte Aufzählung wird durch ein Kommando ersetzt,
   das die Menge ableitet); ein **Verweis** trägt nur, wenn der **Cursor** den Stand
   deckt, in dem die Zitierstelle existiert (wer zitiert, rückt den Cursor vor und
   begeht die Range dazwischen). Der Cursor `agents-skills-consumed` nennt
   das Skills-Repo — er ist kein Rahmen.
5. Erst wenn für **jeden** Skill aus der Range eine Zeile steht:
   `agents-skills-consumed: $(git -C <skills-repo> rev-parse --short HEAD)`,
   `geprüft am:` auf heute, Abschnitte ersetzen — und **alle bestehenden Anker
   re-verifizieren**: Advancing ist kein Updating, der Marker akkumuliert nicht.
6. Falls Regeln oder Skills betroffen waren: die eingebetteten Regeln in der
   globalen `~/.config/opencode/AGENTS.md` gegen ihre Quelle in
   `.agents/rules/*` prüfen und bei geänderter **Pflicht** sinngemäß nachziehen.
   Drift ist eine geänderte Pflicht, kein abweichender Wortlaut — die Einbettung ist die
   Kurzform der Pflicht, kein wörtliches Abbild, und ein Text-Diff gegen die Quelle ist
   **kein** Drift. Dann `touch ~/.config/opencode/opencode.jsonc` (Reload der
   globalen Config; Always-on-Text liefert nur die Einbettung in eine
   `AGENTS.md`).
