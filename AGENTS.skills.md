# Skills-Marker

agents-skills-consumed: 33cbd00
geprüft am: 2026-10-22

**Lokale Ergänzung:** `<stand>` = der committete Stand dieses Commits (die SHA, die erst
mit dem Commit existiert). Der Cursor oben ist **kein** Rahmen — er zeigt den Stand
des Skills-Repos.

## Skill-Stand

| Skill | angewendet? | wo im Projekt | offene Position |
|---|---|---|---|
| `build-verify` | ja | `AGENTS.md` §„Rollen und Delegation" → „Build-Agent" (Zeiger auf den Skill + `.agents/rules/build-verify.md`, ohne Kopie) und §„Verifizierung und Tests" (repo-spezifische Gates); `Verify:`-Footer je Runde in der Historie (**40 von 40** Commits im Fenster der letzten 40 Commits **mit Ende `e744f8b`** tragen einen `Verify:`-Footer — gemessen mit `git rev-list --max-count=40 e744f8b` und `git show -s --format=%B`; **gemessen am Vorgänger `e744f8b`, seitdem unverändert** — das Fenster endet in der Vergangenheit, seine Menge ändert sich nicht; **am committeten Stand dieses Commits** tragen im Fenster der letzten 40 Commits **33 von 40** einen `Verify:`-Footer — die **sieben** Commits **`bf6caed`**, **`50124f8`**, **`549a674`**, **`a783c08`**, **`3e9e92f`**, **`8123160`** und **`ee2c4d4`** tragen keinen (Zählregel: die Commit-Nachricht enthält den String `Verify:` — geprüft mit `git rev-list --max-count=40 <stand>` und `git show -s --format=%B`; dieselbe Regel ergibt am **Stand `e744f8b`** **40 von 40** und am **Stand `f27ea61`** **38 von 40**)); die **neue** Pflicht der Range (`0449b05`: jede Zahl, die etwas über den Commit behauptet, nennt ihren Bezugsrahmen — Zustand **und** Umfang; der committete Stand wird über `git archive <sha>` + `tar -x` gemessen, nie über `git checkout`) ist am Projekt **angewendet**: Commit `8162323` weist beide Bäume getrennt aus („Working Tree 29, committeter Stand 24") und misst den committeten Stand über `git archive dff6948` + `tar -x`. **Die Range `0449b05..33cbd00` gibt Regel 6 eine Ausnahme** (`80664ab`: ein **eigener**, **frischer** Commit **dieser** Aufgabe darf nach dem Push mit `--force-with-lease` ersetzt werden, wenn **sechs** prüfbare Bedingungen zutreffen — eigen und aus dieser Aufgabe, der Remote-Head ist **noch genau** dieser Commit, keine fremde Arbeit im Baum, `--force-with-lease` nie `--force`, der **ersetzte SHA** wird im Body **und** im Bericht offengelegt, der Unterschied bleibt im Reflog abrufbar; ein grüner Lauf des **ersetzten** Commits ist **nichtig**, der neue SHA braucht seinen **eigenen**; **eine** Ersetzung je Commit): **am Projekt ändert sich nichts** — `AGENTS.md` §„Build-Agent" **verweist** auf den Skill, ohne eine Kopie, und das Projekt schreibt **keine** eigene Commit-Regel (`git grep -n -iE 'amend|unpushed|force-with-lease|git add -A' <stand> -- AGENTS.md Agents.headless.md Agents.todo.md` → **0** Treffer — das Projekt schreibt **keine** eigene, strengere Commit-Policy); die Ausnahme wird **automatisch** geerbt. **Regel 4 der Range `5243791..33cbd00` (`e559a39`, `33cbd00`) verlangt in einem Baum mit parallelen Sessions `git commit -- <path>` und lässt nichts gestaged liegen** — ein expliziter Pfad nimmt **nur** die genannten Pfade, ein **nackter** `git commit` nimmt den **ganzen Index** und mit ihm die gestagede Arbeit fremder Sessions; die Klausel bindet **jede** Session im gemeinsamen Baum (die Pflicht liegt beim **Committer**, nicht beim Nachbarn), sie ist **nicht** per Repo-Policy überschreibbar ("except the path-commit clause of rule 4") und erzeugt **keine** Amend-Pflicht (Regel 6 und ihre Ausnahme `80664ab` bleiben unberührt). **Was das an der Anwendung im Projekt ändert: die Art des Commits, nicht die Substanz** — das Projekt schreibt **keine** eigene, strengere Commit-Policy, die Klausel gilt hier **unverändert**, es ist **nichts** umzuschreiben und **keine** neue Board-Position nötig (der Verdikt `ja` trägt). **Der Anlass ist in diesem Projekt entstanden** (gemessen am committeten Stand `e2cd316` mit `git show --numstat`): die fremden Commits `a094cb8` und `5e35b50` tragen je eine Marker-Vorrückung einer **anderen** Session mit (`AGENTS.skills.md` **+147/-151** bzw. **+82/-29**) unter fremdem Betreff; der eigene Marker-Commit `f27ea61` (`AGENTS.skills.md` **+34/-30**) trägt **selbst** **keinen** `Verify:`-Footer. **Angewendet in diesem Lauf:** der Commit entstand mit `git commit -- AGENTS.skills.md`, es bleibt **nichts** gestaged liegen | |
| `skills-marker` | ja | diese Datei (`AGENTS.skills.md`); der Skill ist **hidden** (nur per exakter ID ladbar), der Abschnitt „Fortschreiben" unten ist die lokale Kopie des Ablaufs (Schritt 6 trägt den Wortlaut aus `0024d7e`: kein Config-Key liefert Regeltext — es gibt keinen `instructions`-Key). **Die Kopie ist am Stand `5243791` nachgezogen**: ihre Kurzform führt jetzt **alle fünf** Regeln der Zitierstelle — die **Aufzählungs-Klausel** und die **Cursor-Klausel** kamen in der Range `0449b05..80664ab` dazu. **Sie bleibt gültig:** die Range `5243791..33cbd00` berührt `skills-marker` **nicht** (`git -C <skills-repo> diff --stat 5243791..33cbd00 -- .agents/skills/skills-marker/` ist **leer**) — ein Nachziehen der Kopie war **nicht** nötig. **Die Zitierstelle löst am neuen Cursor auf**: `skills-marker/SKILL.md` → „Bezugsrahmen-Regel" steht am Stand `5243791` **1**-mal in `SKILL.md`, **5**-mal in `references/notes.md` und **1**-mal in `templates/AGENTS.skills.md` (**7** Vorkommen im Skill-Verzeichnis — **gemessen am Vorgänger `5243791`, seitdem unverändert**: der Diff `5243791..33cbd00` über `.agents/skills/skills-marker/` ist **leer**, die Zähleinheit ist die **7** Vorkommen je Skill-Verzeichnis) — am alten Cursor `0449b05` **0**-mal (der Anker kam mit `65fbecc`), deshalb musste der Cursor vorrücken | |
| `github-ci-filters` | ja | `.github/workflows/ci.yml` — `paths-ignore` für `push` **und** `pull_request` (Commit `c4e2fd8`); **gemessen am Vorgänger `8123160`, seitdem unverändert** (`git diff --stat 8123160..<stand> -- .github/workflows/ci.yml` ist leer): **2** `paths-ignore`-Blöcke (Zeilen 11 und 54) — dieselbe Zählung im **Vorgänger** `e744f8b` (`.github/workflows/ci.yml` ist seitdem unverändert). **Schritt 2 der neuen Regel (Form):** die Projektform ist die **Subtraktion** (`paths-ignore`), nicht die Auswahl — die vom neuen Wortlaut vorschriebene Form; die Auswahl-Form `paths` ist nur mit Catch-all im positiven Teil erlaubt, das Projekt braucht sie nicht. **Schritt 4 der neuen Regel (Diff-Beleg mit benannten Endpunkten):** `git diff --stat 2add471..c4e2fd8 -- .github/workflows/` → **1** Datei geändert, **+55** Zeilen, **0** Löschungen; `2add471` ist der Commit, der `.github/workflows/` zuletzt schrieb, `c4e2fd8` der Commit, der den Filter schrieb; der Diff enthält **nur** die beiden `paths-ignore`-Blöcke — keine Job-, Schritt- oder Timeout-Änderung. **Eine Abweichung ist gemessen und dem Eigentümer vorzulegen:** `.github/workflows/ci-libraw-image.yml` trägt einen **`paths:`-Block** (positiver Teil: `docker/Dockerfile`, `.github/workflows/ci-libraw-image.yml` — **kein** Catch-all, **keine** Negation), also die **Auswahl**-Form, die der neue Schritt 2 verbietet; der Workflow filtert **nicht** docs-only, sondern löst gezielt den Image-Publish aus — nach dem **Wortlaut** der Regel verboten, nach ihrer **Begründung** (still verlorene Pipeline-Abdeckung) trifft sie diesen Workflow nicht; das ist eine **Eigentümerentscheidung**, und dieser Lauf legt **keine** Board-Position an. Der Skill ist seit `817c01e` **hidden** — das Projekt nennt ihn in keiner Datei außer diesem Marker, es gibt also nichts umzuschreiben | |
| `codegraph-project-setup` | ja | `.codegraph/` (Index) + `.githooks/pre-commit` Gate 1 „CodeGraph index sync" (`core.hooksPath=.githooks`) — **kein** CodeGraph-Abschnitt in `AGENTS.md` (der Anker der Vorversion zeigte ins Leere) | |
| `docker-test-image` | ja | `docker/Dockerfile` (LibRaw 0.22.2, `ARG LIBRAW_SHA256`-Pin des Quell-Tarballs) + **fünf** `container:`-Jobs in `.github/workflows/ci.yml` (**gemessen am Vorgänger `8123160`, seitdem unverändert** — `git diff --stat 8123160..<stand> -- docker/Dockerfile .github/workflows/ci.yml` ist leer: `grep -c '^    container:'` = 5 — dieselbe Zählung im **Vorgänger** `e744f8b`; `docker/Dockerfile` und `.github/workflows/ci.yml` sind seitdem unverändert) | |
| `ghcr-visibility` | ja | `.github/workflows/ci.yml` — `ghcr.io/reisi007/luminarust/lumina-ci:latest` in denselben fünf `container:`-Jobs (**gemessen am Vorgänger `8123160`, seitdem unverändert** (`git diff --stat 8123160..<stand> -- .github/workflows/ci.yml` ist leer): **5** `image: ghcr.io`-Zeilen — dieselbe Zählung im **Vorgänger** `e744f8b`; `.github/workflows/ci.yml` ist seitdem unverändert) | |
| `security-review` | ja | Register: `feature/quality/security-review-register.md` (Form nach Stage 6 — Zwei-Hälften-Form, Registertabelle, Pflege durch den Build-Agent; Enumerationsbasis mit den erreichbaren Oberflächen, vollständig enumeriert in Welle 1 am 2026-10-20 — die Zahl **24** Oberflächen steht im generierten Block **surfaces** in §5 des Registers — **gemessen am Stand `e2cd316`** (Form (c): das Register hat sich seit dem Vorgänger `8123160` **geändert** — `git diff --stat 8123160..e2cd316 -- feature/quality/security-review-register.md` → **1** Datei geändert, **+75/-46**; letzter Schreiber `a80f081`; die Zählung **24** Oberflächen ist dieselbe wie im **Vorgänger** `e744f8b`, das Register selbst ist **nicht** unverändert); **vier** Befundzeilen statt null: `SEC-1` und `SEC-4` (`geschlossen`), `SEC-2` und `SEC-3` (`ZURÜCKGENOMMEN`) — die Beispielzeile `SEC-EXAMPLE` ist am 2026-10-22 **entfallen**; die erste Review-Welle ist **abgeschlossen**: die Task `SEC-REVIEW-WELLE-1-85` ist aus dem Board **entfernt** (Projektregel: abgeschlossene Punkte werden entfernt, nicht abgehakt) — sie lebt nur noch als Querverweis in zwei offenen Positionen (`SKILLS-VOKABULAR-98`, `MCP-TRANSPORT-REPARATUR-EBENE-99`); die **Scope-Frage GUI** ist **entschieden** (Eigentümerentscheid 2026-10-20: GUI-Oberflächen werden **gleichrangig** in jeder Welle mitgeprüft) und im Register entsprechend vermerkt. **Die geänderte Regel der Range** (Severity aus den kanonischen Stufen; `AKZEPTIERT` ist das kanonische `accepted`) trifft die Projektform des Registers: die fixe Kopfzeile hat **keine** Severity-Spalte und **keine** Ticket-Spalte | `Agents.todo.md` → `SKILLS-VOKABULAR-98` |
| `ui-review` | nein | als Playwright-Flow trifft es nicht zu: native egui-GUI, der visuelle Review läuft über `egui_kittest`-Goldens/PSNR und den Fenster-Review `UI-VISION-REVIEW-70`. Die Vision-Regel (kein Vision-Subagent: das konfigurierte Modell liest die Bilder selbst) stammt aus einer **früheren Range**, **nicht** aus der auditierten: sie steht seit `8bfc321` in `ui-review/SKILL.md` → „there is no vision subagent in the loop" und wurde in `dccdd40` konsolidiert → „the configured model reads images" — beide Stellen als **Anker** zitiert (Pfad im Skill: **ID + Datei**; Anker: **wörtliches** Textzitat, **ohne** Zeilennummer, weil die driftet), gemessen am Stand `5243791`, **seitdem unverändert** (Form (b) — `git -C <skills-repo> diff --stat 5243791..33cbd00 -- .agents/skills/ui-review/` ist **leer**): **je 1** Treffer im Skill-Verzeichnis (`git grep -c 'there is no vision subagent in the loop' 5243791 -- .agents/skills/ui-review/` = **1**, `git grep -c 'the configured model reads images' 5243791 -- .agents/skills/ui-review/` = **1**); die **letzte** Vision-Änderung **vor** dem Cursor `00dbd22` ist `fa2268f` — sie ändert **eine** Zeile in `references/findings-report.md` (der „vision-agent"-Wortlaut). In der auditierten Range `00dbd22..0449b05` ändert `ui-review` ausschließlich `references/findings-report.md`; **keine** Vision-Zeile (`git diff 00dbd22..0449b05 -- .agents/skills/ui-review/ \| grep -i vision` ist leer). Die Regel trifft die Projekt-Pflicht in `DoD.md` §6 → dort **umgesetzt** (**gemessen am Vorgänger `8123160`, seitdem unverändert** — `git diff --stat 8123160..<stand> -- DoD.md` ist leer: dieselbe Aussage wie im **Vorgänger** `e744f8b`). Die **Findings-Vorlage** (`b0d1b51`: die **neun** Spalten `Id \| Severity \| Status \| Owner \| Ticket \| File:Line \| Screenshot \| Finding \| Suggested fix` — der Upstream-Skill zitiert sie seit `2e09608` **wörtlich**, vorher zitiert er nur fünf) trifft dieses Projekt **nicht** — das Projekt führt keine UI-Findings-Liste nach dieser Vorlage (die UXG/UXD-Lückeinventur in `feature/platform/lightroom-ux-parity.md` ist eine SOLL-Lücke, kein Review-Report); das **kanonische Vokabular**, auf das die Vorlage zeigt, ist unter `ratchet-gates` erfasst — der Upstream-Skill führt seit `2e09608` **selbst keine** Severity-Liste mehr, sondern zeigt auf dasselbe Vokabular. Der Skill wird im Projekt nur in **diesem Marker** und im **Board** (`Agents.todo.md`, Eintrag `SKILLS-VOKABULAR-98`) genannt, in keiner weiteren Projektdatei — **am committeten Stand dieses Commits**: **4 Zeilen** in **2** Dateien (`git grep -n ui-review <stand> -- '*.md'`; vorkommensweise **13** — `git grep -o ui-review <stand> -- '*.md'`, eine Zeile trägt **10** — das ist die `ui-review`-Zeile dieser Tabelle; der Zähler folgt dem Text des Markers, er ist kein Bestand); am **Stand `e744f8b`** (dort gemessen — die Datei hat sich seither geändert, das ist die Form „gemessen am Stand", keine „unverändert seit"-Aussage) ist die **Dateimenge** dieselbe (**2** Dateien: `AGENTS.skills.md`, `Agents.todo.md`), der **Zeilenzähler** dort ist **5** — eine repo-weite Suche zählt die vom Commit geänderten Dateien mit; die Zusatzzeile kam mit `0cf154d`; der Zähler ist am **committeten Stand dieses Commits** **4** | |
| `ratchet-gates` | ja | `scripts/gates.sh` (Stufen `fast`/`slow`/`all`/`list`/`list-ci`, Floor `MIN_CHECKS=9`, keine Codegen-Stufe `fast`), `feature/quality/local-gates.md` (Tier-Tabelle wortgleich zu `gates.sh list` — byteweise diffleer geprüft; **gemessen am Vorgänger `8123160`, seitdem unverändert** — `git diff --stat 8123160..<stand> -- scripts/gates.sh feature/quality/local-gates.md` ist leer; dieselbe Zählung wie im **Vorgänger** `e744f8b` — `scripts/gates.sh`, `local-gates.md` und die Gate-Skripte sind seitdem unverändert; Floor-Regel; Lückenliste), `AGENTS.md` §„Verifizierung und Tests" (der dortige Floor-Text nannte beim Marker-Run 2026-10-21 **vier** Textgates und **5** Checks; **korrigiert in `0cf154d`** — **gemessen am Vorgänger `8123160`, seitdem unverändert** (`git diff --stat 8123160..<stand> -- AGENTS.md` ist leer): **acht** Textgates und Floor **9**, Quellen `sh scripts/gates.sh list` und `scripts/gates.sh` `MIN_CHECKS=9`), CI-Schritt „Run fast tier (scripts/gates.sh fast)" im Job „Rust fast checks" (`.github/workflows/ci.yml`), Gate-Header der **elf** `scripts/check_*.sh` (Defektklasse, Anlass, Ratchet-Richtung, benannte Grenze; **gemessen am Vorgänger `8123160`, seitdem unverändert** — `git diff --stat 8123160..<stand> -- 'scripts/check_*.sh'` ist leer: **11** Skripte, jedes mit einem `Defektklasse`-Header — dieselbe Zählung im **Vorgänger** `e744f8b`). **Die Range `0449b05..842e055` ändert nur die CLI-Karte** (`ratchet-gates/references/cli.md`, `d5dc52f`: die neue Zeile `ratchet release`, der eigene Exit-Middle `1` der Release-Stufe und der **umgedrehte** Installationsabschnitt „**No tag was pushed yet** … build from source" → „The releases are published") — die **Gate-Substanz** (`SKILL.md`, `references/vocabulary.md`) ist **unberührt**; das Projekt setzt die Mechanik selbst um (POSIX sh, `scripts/gates.sh`), die `ratchet`-Binary ist ein eigenes Repo, ihre Release-Aussage betrifft es nicht — an der Projektanwendung ändert sich **nichts** | `Agents.todo.md` → `SKILLS-VOKABULAR-98` (kanonisches Vokabular) |
| `generated-docs` | ja | `scripts/gen_doc_facts.sh` (Generator: regelbasiert, deterministisch, git-frei, POSIX sh; liest die Schwellen 500/3000 aus den Gate-Skripten, fuehrt nie etwas aus einem gelesenen Dokument aus) + `scripts/check_doc_facts_drift.sh` (Drift-Gate: erzeugen -> Block aus der Zieldatei extrahieren -> diff; meldet, schreibt nie) als `fast-8` und `scripts/check_doc_facts_generator.sh` (Hygiene-Gate des Generators: Determinismus + Git-Freiheit) als `fast-9` der Stufe `fast`; generierte Bloecke `file-sizes` + `population` in `feature/quality/file-size-policy.md`, `goldens` in `feature/README.md` + `feature/quality/golden-references.md` + `feature/quality/golden-fixtures.md`, `mcp-tools` in `feature/platform/mcp-server.md` + `feature/quality/security-review-register.md`, `perf-baseline` in `feature/quality/performance-benchmarks.md`, `crates` in `feature/README.md` und `surfaces` in `feature/quality/security-review-register.md`, `golden-classes` in `feature/quality/golden-fixtures.md` und `golden-targets` in `feature/quality/golden-references.md` (Quelle: die committete Datei `scripts/golden_inventory.txt`) — **gemessen am Stand `e2cd316`** (Form (c) für die **drei** geänderten Zieldateien, Form (b) für die übrigen): `git diff --stat 8123160..e2cd316 -- scripts/gen_doc_facts.sh scripts/golden_inventory.txt feature/README.md feature/platform/mcp-server.md feature/quality/file-size-policy.md feature/quality/golden-fixtures.md feature/quality/golden-references.md feature/quality/performance-benchmarks.md feature/quality/security-review-register.md` → **3** Dateien geändert (`feature/platform/mcp-server.md` **+101/-0**, letzter Schreiber `14704f3`; `feature/quality/file-size-policy.md` **+7/-8**, letzter Schreiber `b84f73c`; `feature/quality/security-review-register.md` **+75/-46**, letzter Schreiber `a80f081`), Generator und `scripts/golden_inventory.txt` sind **unverändert** (Diff leer): **12** Bloecke in **7** Dateien (dieselbe Zählung im **Vorgänger** `e744f8b`, dort ebenfalls **12**/**7**); **Zählregel: das Generator-Manifest** (`sh scripts/gen_doc_facts.sh --manifest`: eine Zeile je Block-Instanz, `block-id\|zieldatei` — **12** Zeilen, **7** verschiedene Zieldateien — **gemessen am Stand `e2cd316`**, auf dem exportierten committeten Baum via `git write-tree` + `git archive <tree> | tar -x`). Die Markensuche im Baum (`git grep -n '<!-- BEGIN GENERATED:' <stand> -- '*.md'`) liefert **am committeten Stand dieses Commits** **14** Treffer in **9** Dateien: die **12** echten Bloecke plus **zwei** Literal-Erwähnungen der Marker-Syntax (`feature/quality/local-gates.md` §8.1 und diese Zeile) — deshalb ist das Manifest die Zählregel, nicht die Markensuche; CI-Schritt „Run fast tier (scripts/gates.sh fast)" im Job „Rust fast checks" (`.github/workflows/ci.yml`) | `Agents.todo.md` → `DOC-ZAHLEN-DRIFT-82` (Welle 1 umgesetzt; **Welle 2 im Repo umgesetzt** (2026-10-20): `mcp-tools`, `perf-baseline`, `crates`, `surfaces` — der Todo-Eintrag `DOC-ZAHLEN-DRIFT-82` ist nachgezogen; **Welle 3 im Repo umgesetzt** (2026-10-20): der R/C-Split der Goldens und die Verteilung auf die Test-Targets werden jetzt aus der committeten Quelldatei `scripts/golden_inventory.txt` **erzeugt** (die Klassifikation selbst bleibt eine menschliche Entscheidung, die Datei ist ihre maschinenlesbare Form); **weiterhin handgepflegt**: Testlaufzahlen (nicht ableitbar, datiert) und das CLI-Coverage-Handinventar „Abgedeckt (29)" in `feature/platform/mcp-server.md` (eine eigene Population, jetzt mit Zählstand 2026-10-15 datiert)) |
| `agent-config` | nein | | Maschinenkonfiguration (globale `opencode.jsonc`, globale `AGENTS.md`); die projektbezogene Aussage der Änderung — ein Projekt verweist auf die Regeldatei, statt den Flow zu wiederholen — ist im Projekt erfüllt; die versteckten Skills der Range (`70d49e0`, `00dbd22`, `817c01e`) sind für dieses Projekt ohne Wirkung. **Belegter Stand (gelesen am 2026-10-21):** die globale `~/.config/opencode/opencode.jsonc` führt **keinen** `instructions`-Key (der Upstream-Wortlaut `0024d7e` ist damit am Projektstand bestätigt). **Die Range `0449b05..33cbd00` ergänzt §6** (`80664ab`) um die Kurzform der Amend-Ausnahme von `build-verify` samt Zeiger auf `build-verify/references/amend.md` — **am Projekt ändert sich nichts**: die globale `~/.config/opencode/AGENTS.md` trägt **keinen** build-verify-Text (`grep -n -i 'build-verify\|amend\|force-with-lease' ~/.config/opencode/AGENTS.md` → **0** Treffer), es ist nichts neu einzubetten und kein Reload nötig. **Die Commits `e559a39` und `33cbd00` der Range `5243791..33cbd00` ändern §6 (die Kurzform) und `references/snippets.md` (das Kommando-Template)** — die geänderte Passage ist die **Commit-Konvention des Skills-Repos** (das Template committet im Repo `~/dev/agents-skills` mit `git commit -F - -- <explizite Pfade>`), **keine** Pflicht dieses Projekts; der Verweis auf die Regeldatei statt einer Kopie ist im Projekt erfüllt → **am Projekt ändert sich nichts**, der Verdikt bleibt **nein** |
| `laravel` | nein | | Rust-Workspace, kein PHP |
| `node-deps` | nein | | keine `package.json`/pn-Pins; Abhängigkeiten laufen über Cargo |
| `permissions` | nein | | Maschinenpolicy (`opencode.jsonc`), nicht Projektstand |
| `playwright-parallel` | nein | | keine Playwright-Suite; E2E sind `egui_kittest`-Targets in `crates/lumina-gui/tests/` |
| `react-i18n` | nein | | GUI ist egui/Rust mit eigener `Str`-i18n, kein React/Lingui |
| `tailscale-serve` | nein | | headless-Host, kein Dev-Server-Expose |
| `update-models` (ersetzt `model-updater` + `update-opencode-models`) | nein | | Modell-Registry ist Maschinenstand (`opencode.jsonc`), kein Projektstand; die Projekt-Entscheidung `MODELL` (nur freie Subagenten) bleibt unberührt. **Belegter Stand (gelesen am 2026-10-21 aus `opencode.jsonc` und `model-preferences.md`):** `namespaceRule: opencode-go-only`; Root-Modell und die vier Rollen `plan`/`general`/`build`/`explore` fahren `opencode-go/minimax-m3.1-flash-free`. Die Zeile folgt damit der upstream-Angleichung (`219171c`, `8c6b9af` — beide **vor** dem Cursor `00dbd22`): die Namespace-Regel steht auf der Entscheidung, nicht mehr auf einem Kontext-/Preisargument, und der `$0`-Vermerk gilt dem konfigurierten Modell. Der Skill ist seit `817c01e` **hidden** — für dieses Projekt ohne Wirkung |
| `web-audit` | nein | | kein Web-Frontend; LuminaRust ist Desktop-GUI + CLI |
| Konvention: 500-Zeilen-Ratetch | ja | `scripts/check_file_sizes.sh` (`THRESHOLD=500`), `scripts/file_size_baseline.txt`, `AGENTS.md` §„Verifizierungs-Agent" | |
| Konvention: Projektsprache Deutsch (Doku) | ja | die Projektdoku (`AGENTS.md`, `Agents.todo.md`, `feature/`, **auch diese Marker-Datei** — deutsch seit dem Anlegen in `b7c0799`; die englische Sprachregel gilt für das Skills-Repo, nicht für den Marker im Projekt). **Keine Sprachregel im Projekt** — **am committeten Stand dieses Commits** schreibt **keine** `.md` **außer diesem Marker** eine Sprachregel vor (`git grep -l 'Sprachregel' <stand> -- '*.md'` → **1** Datei: dieser Marker; die Konvention wird **beobachtet**, nicht vorgeschrieben); die **UI ist englisch** (Produktentscheid: `feature/platform/cli-gui-wasm.md` — „ist zum MVP **englischsprachig**“; `feature/platform/lightroom-ux-parity.md` UXG-17 — „F-100: englisch, 0 deutsche UI-Literale“) | |
| Konvention: Regel referenziert, nicht kopiert | ja | `AGENTS.md` §„Build-Agent" → Skill `build-verify` (Lieferweg: **Skill-Trigger**) und die Minimalregeln in `.agents/rules/build-verify.md` (agents-skills, Pfad-Verweis, keine Kopie); global eingebettet ist nur die Frage-Regel (`~/.config/opencode/AGENTS.md`, Quelle `ask.md`) | |
| Konvention: visueller Review mit dem konfigurierten Modell (DoD §6) | ja | `DoD.md` §6 (KI-Validierungs-Loop, Schritt 2), `Agents.todo.md` `F-103-N6-GUI-COVERAGE-27` Punkt 11, `feature/platform/lightroom-ux-parity.md` (Abnahme), `crates/lumina-gui-harness/README.md` — alle vier Stellen nennen **das konfigurierte Modell selbst** (max. 10 Bilder je Durchgang); ein Vision-Subagent existiert nicht | |
| Konvention: jede Zahl in einer Commit-Aussage nennt ihren Bezugsrahmen | ja | Zustand **und** Umfang: committeter Stand über `git archive <sha>` + `tar -x` in ein temporäres Verzeichnis (nie `git checkout`/`git restore`/`git reset` — Regel 3 von `build-verify`); die Pflicht erreicht das Projekt über den Skill-Zeiger in `AGENTS.md` §„Build-Agent" und ist seit Commit `8162323` praktisch angewendet (dort: „Working Tree 29, committeter Stand 24") | |

## Offen aus dem Bereich `5243791..33cbd00`

Geprüft am **2026-10-22**, **06:07:22 UTC** (Endmessung) gegen `33cbd00` (Range
`5243791..33cbd00`; **2** Commits — `git -C <skills-repo> rev-list --count
5243791..33cbd00`; **2** Skill-Verzeichnisse berührt — `git -C <skills-repo> log
--name-only --format= 5243791..33cbd00 -- .agents/skills` sortiert eindeutig:
`agent-config`, `build-verify`). **Der Skills-Repo-Stand wurde zweimal
gemessen:** beim Laufbeginn stand er auf `e559a39` (Commit 2026-10-22
03:11:57 +0000), bei der Endmessung auf `33cbd00` (Commit 2026-10-22
05:58:09 +0000) — der **nachträglich** gelandete Commit `33cbd00` ist **in
dieser Range mitgeprüft**.

**Die Commits der Range berühren nur Dateien des Skills-Repos — die Anker
dieses Markers bleiben gültig.** Gemessen: `git -C <skills-repo> show
--name-only --format= e559a39` → **5** Dateien, `git -C <skills-repo> show
--name-only --format= 33cbd00` → **5** Dateien, **keine** davon unter den
Projektpfaden; `git -C <skills-repo> diff --stat 5243791..33cbd00 -- AGENTS.md
Agents.headless.md Agents.todo.md DoD.md .github/ docker/ feature/ scripts/
crates/` ist **leer**. Die Anker der Verdikt-Tabelle tragen deshalb den
**leeren Diff-Beleg** (Form (b): „gemessen am Vorgänger `<sha>`, seitdem
unverändert"); **Ankerzahlen und Zähleinheit sind unverändert übernommen** —
sie werden nicht neu gezählt, nur ihre Rahmen werden an die gemessene Form
angepasst. **Form (c)** (die Datei hat sich seit der Messung geändert) gilt
für **drei** Projektdateien, die **nicht** von dieser Range, sondern von
Projekt-Commits seit `8123160` geschrieben wurden (`feature/platform/mcp-server.md`,
`feature/quality/file-size-policy.md`,
`feature/quality/security-review-register.md`) — sie stehen unten bei den
jeweiligen Zeilen.

**Neu**

- **Kein neuer Skill** in dieser Range: `git -C <skills-repo> log --diff-filter=A
  --name-only --format= 5243791..33cbd00 -- .agents/skills` ist **leer**. Die Zahl
  der Skills ist an beiden Reven **19** (`git ls-tree -r --name-only <rev>
  .agents/skills | grep -c 'SKILL.md$'`), die Zahl der **versteckten** Skills
  (`metadata.opencode/autoinvoke: false`) an beiden Reven **17** von **19**
  (`git grep -l 'autoinvoke: false' <rev> -- '.agents/skills/*/SKILL.md'`) —
  **unverändert**; sichtbar bleiben `build-verify` und `ui-review`.

**Geändert**

- `build-verify` — **zwei Commits**: `e559a39` (**3** Dateien:
  `.agents/rules/build-verify.md` +2/-2, `build-verify/SKILL.md` +7/-1,
  `build-verify/references/notes.md` +23/-1) und `33cbd00` (**3** Dateien:
  `.agents/rules/build-verify.md` +1/-1, `build-verify/SKILL.md` +1/-1,
  `build-verify/references/notes.md` +4/-5) — **trifft zu und ist angewendet**
  (der Verdikt `ja` bleibt). **Was die Änderung tut:** Regel 4 (Commit nach jedem
  Verify-Lauf) bekommt die **Pfad-Klausel** für Bäume, in denen andere Sessions
  arbeiten: committet wird mit `git commit -- <path>` — ein expliziter Pfad
  nimmt **nur** die genannten Pfade, ein **nackter** `git commit` nimmt den
  **ganzen Index** und mit ihm die gestagede Arbeit fremder Sessions — und es
  bleibt **nichts** gestaged liegen; `33cbd00` schärft die Klausel: die
  Ausnahme für Regeln 4–7 („a repo's own documented branch policy can override
  rules 4–7 where it says so") gilt **ausdrücklich nicht** für die
  Pfad-Klausel („**except the path-commit clause of rule 4**") — die Klausel ist
  **nicht** per Repo-Policy überschreibbar. **Was das an der Anwendung im
  Projekt ändert: die Art des Commits, nicht die Substanz.** Das Projekt
  schreibt **keine** eigene, strengere Commit-Policy (gemessen am committeten
  Stand `e2cd316`: `git grep -n -iE 'amend|unpushed|force-with-lease|git add -A'
  e2cd316 -- AGENTS.md Agents.headless.md Agents.todo.md` → **0** Treffer) — die
  Klausel gilt hier **unverändert**, es ist **nichts** umzuschreiben und
  **keine** neue Board-Position nötig (der Verdikt `ja` trägt). **Der Anlass ist
  in diesem Projekt entstanden** (gemessen am committeten Stand `e2cd316` mit
  `git show --numstat`): die fremden Commits `a094cb8` und `5e35b50` tragen je
  eine Marker-Vorrückung einer **anderen** Session mit (`AGENTS.skills.md`
  **+147/-151** bzw. **+82/-29**) unter fremdem Betreff; der eigene
  Marker-Commit `f27ea61` (`AGENTS.skills.md` **+34/-30**) trägt **selbst**
  **keinen** `Verify:`-Footer. **Angewendet in diesem Lauf:** der Commit
  entstand mit `git commit -- AGENTS.skills.md`, es bleibt **nichts** gestaged
  liegen.
- `agent-config` — **zwei Commits**: `e559a39` (**2** Dateien:
  `agent-config/SKILL.md` +1/-1, `agent-config/references/snippets.md` +2/-1) und
  `33cbd00` (**1** Datei: `agent-config/SKILL.md` +1/-0) — **trifft nicht zu**
  (der Verdikt bleibt `nein`): die geänderte Passage ist die
  **Commit-Konvention des Skills-Repos** — §6 (die Kurzform) und das
  Kommando-Template in `references/snippets.md` (`git commit -F - -- <explizite
  Pfade>` im Repo `~/dev/agents-skills`) —, also **Maschinenkonfiguration**, die
  dieses Projekt **nicht** bindet. **Am Projekt ändert sich nichts:** die globale
  `~/.config/opencode/AGENTS.md` trägt **keinen** build-verify-Text (`grep -n
  -i 'build-verify\|amend\|force-with-lease' ~/.config/opencode/AGENTS.md` →
  **0** Treffer, am 2026-10-22 erneut gemessen) → es ist **nichts** neu
  einzubetten und **kein** Reload nötig.

**Entfernt**

- **Keine** — in dieser Range wurde kein Skill entfernt.

**Ohne Skill-Bezug** (Commits der Range, die keine Datei unter `.agents/` berühren)

- **Keine** — beide Commits der Range berühren `.agents/`-Dateien (je Commit
  geprüft mit `git show --name-only --format= <sha>`). `33cbd00` berührt
  zusätzlich `README.md` des Skills-Repos (**+4/-1**): der Block „Adding skills"
  zeigt jetzt den pfad-expliziten Commit statt `git add -A && git commit && git
  push` — das betrifft **das Skills-Repo**, nicht dieses Projekt (der Weg der
  Projektregel ist der Skill-Zeiger in `AGENTS.md` §„Build-Agent"), also
  **keine** Pflicht hier.

**Cursorabhängige Stellen (alt → neu)**

| Stelle | alt | neu |
|---|---|---|
| Kopf: `agents-skills-consumed` | `5243791` | `33cbd00` |
| Kopf: `geprüft am` | `2026-10-21` | `2026-10-22` |
| `build-verify`-Zeile: „Die Range `0449b05..…` gibt Regel 6 eine Ausnahme" | `0449b05..5243791` | `0449b05..33cbd00` |
| `build-verify`-Zeile: Fenster der letzten 40 Commits | **38 von 40**, **zwei** Commits ohne `Verify:`-Footer | **33 von 40**, **sieben** Commits ohne `Verify:`-Footer |
| `skills-marker`-Zeile: „Die Kopie ist am Stand `…` nachgezogen" | `5243791` | `5243791` — **unverändert**, die Range berührt `skills-marker` nicht |
| `skills-marker`-Zeile: Zitierstelle „steht am Stand `…`" | `5243791`, **7** Vorkommen | `5243791`, **7** Vorkommen — **unverändert** (Form (b), leerer Diff) |
| `ui-review`-Zeile: Zählung der Vision-Anker | Stand `5243791`, je **1** Treffer | Stand `5243791`, je **1** Treffer — **unverändert** (Form (b), leerer Diff) |
| `security-review`-Zeile: Rahmen des Registers | Vorgänger `8123160`, unverändert | Stand `e2cd316` (Form (c)) |
| `generated-docs`-Zeile: Rahmen der Zieldateien | Vorgänger `8123160`, unverändert | Stand `e2cd316` (Form (c)) |
| `agent-config`-Zeile: „Die Range `0449b05..…` ergänzt §6" | `0449b05..5243791` | `0449b05..33cbd00` |

**Nicht cursorabhängig, beim Vorrücken des Cursors aber falsch geworden
(korrigiert)**

- `security-review`-Zeile: „**bisher null Befunde**" — das Register trägt am
  Stand `e2cd316` **vier** Befundzeilen (`SEC-1` und `SEC-4` `geschlossen`,
  `SEC-2` und `SEC-3` `ZURÜCKGENOMMEN`); die Beispielzeile `SEC-EXAMPLE` ist am
  2026-10-22 **entfallen**. Ebenso: die erste Review-Welle ist **abgeschlossen**
  — die Task `SEC-REVIEW-WELLE-1-85` ist aus dem Board **entfernt**
  (Projektregel: abgeschlossene Punkte werden entfernt, nicht abgehakt); sie
  lebt nur noch als Querverweis in zwei offenen Positionen
  (`SKILLS-VOKABULAR-98`, `MCP-TRANSPORT-REPARATUR-EBENE-99`).
- `build-verify`-Zeile: das Fenster der letzten 40 Commits hat sich verschoben
  (**20** Commits seit `f27ea61`); die Zählung gilt **33 von 40** am committeten
  Stand dieses Commits (Zählregel: die Commit-Nachricht enthält den String
  `Verify:`).

**Ungeprüfter Rest**

- **Keiner** — nach der Endmessung (2026-10-22, 06:07:22 UTC, Stand `33cbd00`) ist im
  Skills-Repo **kein** weiterer Commit gelandet; der Cursor deckt genau den
  geprüften Stand. (Wäre nach der Endmessung noch ein Commit gelandet, stünde er
  hier als ungeprüfter Rest.)

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
