# Skills-Marker

agents-skills-consumed: 00dbd22
geprüft am: 2026-10-20

## Skill-Stand

| Skill | angewendet? | wo im Projekt | offene Position |
|---|---|---|---|
| `build-verify` | ja | `Agents.md` §„Rollen und Delegation" → „Build-Agent" (Zeiger auf den Skill + `.agents/rules/build-verify.md`, ohne Kopie) und §„Verifizierung und Tests" (repo-spezifische Gates); `Verify:`-Footer je Runde in der Historie | |
| `skills-marker` | ja | diese Datei (`AGENTS.skills.md`); der Skill ist jetzt **hidden** (nur per exakter ID ladbar), der Abschnitt „Fortschreiben" unten ist die lokale Kopie des Ablaufs | |
| `github-ci-filters` | ja | `.github/workflows/ci.yml` — `paths-ignore` für `push` **und** `pull_request` (Commit `c4e2fd8`) | |
| `codegraph-project-setup` | ja | `.codegraph/` (Index) + `.githooks/pre-commit` Gate 1 „CodeGraph index sync" (`core.hooksPath=.githooks`) — **kein** CodeGraph-Abschnitt in `Agents.md` (der Anker der Vorversion zeigte ins Leere) | |
| `docker-test-image` | ja | `docker/Dockerfile` (LibRaw 0.22.2, Digest-Pin) + fünf `container:`-Jobs in `.github/workflows/ci.yml` | |
| `ghcr-visibility` | ja | `.github/workflows/ci.yml` — `ghcr.io/reisi007/luminarust/lumina-ci:latest` in denselben fünf `container:`-Jobs | |
| `security-review` (neu) | ja | Register: `feature/quality/security-review-register.md` (Form nach Stage 6 — Zwei-Hälften-Form, Registertabelle, Pflege durch den Build-Agent; Enumerationsbasis mit den erreichbaren Oberflächen (vollständig enumeriert in Welle 1, 2026-10-20; die Zahl steht im generierten Block **surfaces** in §5 des Registers), **bisher null Befunde**; die erste Review-Welle ist die offene Task `SEC-REVIEW-WELLE-1-85` (Teil a erledigt, die Teile b/c brauchen Läufe); die **Scope-Frage GUI** ist **entschieden** (Eigentümerentscheid 2026-10-20: GUI-Oberflächen werden **gleichrangig** in jeder Welle mitgeprüft) und im Register entsprechend vermerkt) | |
| `ui-review` | nein | als Playwright-Flow trifft es nicht zu: native egui-GUI, der visuelle Review läuft über `egui_kittest`-Goldens/PSNR und den Fenster-Review `UI-VISION-REVIEW-70`. Die **geänderte** Regel derselben Runde (kein Vision-Subagent: das konfigurierte Modell liest die Bilder selbst) trifft die Projekt-Pflicht in `DoD.md` §6 → dort **umgesetzt** | |
| `ratchet-gates` (neu) | ja | `scripts/gates.sh` (Stufen `fast`/`slow`/`all`/`list`/`list-ci`, Floor `MIN_CHECKS=9`, keine Codegen-Stufe `fast`), `feature/quality/local-gates.md` (Tier-Tabelle wortgleich zu `gates.sh list`, Floor-Regel, Lückenliste), `Agents.md` §„Verifizierung und Tests", CI-Schritt „Run fast tier (scripts/gates.sh fast)" im Job „Rust fast checks" (`.github/workflows/ci.yml`), Gate-Header der **elf** `scripts/check_*.sh` (Defektklasse, Anlass, Ratchet-Richtung, benannte Grenze) | |
| `generated-docs` (neu) | ja | `scripts/gen_doc_facts.sh` (Generator: regelbasiert, deterministisch, git-frei, POSIX sh; liest die Schwellen 500/3000 aus den Gate-Skripten, fuehrt nie etwas aus einem gelesenen Dokument aus) + `scripts/check_doc_facts_drift.sh` (Drift-Gate: erzeugen -> Block aus der Zieldatei extrahieren -> diff; meldet, schreibt nie) als `fast-8` und `scripts/check_doc_facts_generator.sh` (Hygiene-Gate des Generators: Determinismus + Git-Freiheit) als `fast-9` der Stufe `fast`; generierte Bloecke `file-sizes` + `population` in `feature/quality/file-size-policy.md`, `goldens` in `feature/README.md` + `feature/quality/golden-references.md` + `feature/quality/golden-fixtures.md`, `mcp-tools` in `feature/platform/mcp-server.md` + `feature/quality/security-review-register.md`, `perf-baseline` in `feature/quality/performance-benchmarks.md`, `crates` in `feature/README.md` und `surfaces` in `feature/quality/security-review-register.md`; CI-Schritt „Run fast tier (scripts/gates.sh fast)" im Job „Rust fast checks" (`.github/workflows/ci.yml`) | `Agents.todo.md` → `DOC-ZAHLEN-DRIFT-82` (Welle 1 umgesetzt; **Welle 2 im Repo umgesetzt** (2026-10-20): `mcp-tools`, `perf-baseline`, `crates`, `surfaces` — der Todo-Eintrag `DOC-ZAHLEN-DRIFT-82` ist nachgezogen; **offen bleibt Welle 3**: der R/C-Split der Goldens (14/52) und die Verteilung auf die Test-Targets bleiben handgepflegt (ihre Klassifikation ist es auch), Testlaufzahlen sind nicht ableitbar, und das CLI-Coverage-Handinventar „Abgedeckt (29)“ in `feature/platform/mcp-server.md` braucht eine eigene Quelle oder einen datierten Stand) |
| `agent-config` | nein | | Maschinenkonfiguration (globale `opencode.jsonc`, globale `AGENTS.md`); die projektbezogene Aussage der Änderung — ein Projekt verweist auf die Regeldatei, statt den Flow zu wiederholen — ist im Projekt erfüllt; die versteckten Skills der Range (`70d49e0`, `00dbd22`) sind für dieses Projekt ohne Wirkung |
| `laravel` | nein | | Rust-Workspace, kein PHP |
| `node-deps` | nein | | keine `package.json`/pn-Pins; Abhängigkeiten laufen über Cargo |
| `permissions` | nein | | Maschinenpolicy (`opencode.jsonc`), nicht Projektstand |
| `playwright-parallel` | nein | | keine Playwright-Suite; E2E sind `egui_kittest`-Targets in `crates/lumina-gui/tests/` |
| `react-i18n` | nein | | GUI ist egui/Rust mit eigener `Str`-i18n, kein React/Lingui |
| `tailscale-serve` | nein | | headless-Host, kein Dev-Server-Expose |
| `update-models` (neu; ersetzt `model-updater` + `update-opencode-models`) | nein | | Modell-Registry ist Maschinenstand (`opencode.jsonc`), kein Projektstand; die Projekt-Entscheidung `MODELL` (nur freie Subagenten) bleibt unberührt. **Belegter Stand (gelesen am 2026-10-20 aus `opencode.jsonc` und `model-preferences.md`):** `namespaceRule: opencode-go-only`; Root-Modell und die vier Rollen `plan`/`general`/`build`/`explore` fahren `opencode-go/minimax-m3.1-flash-free`. Die Zeile folgt damit der upstream-Angleichung (`219171c`, `8c6b9af`): die Namespace-Regel steht auf der Entscheidung, nicht mehr auf einem Kontext-/Preisargument, und der `$0`-Vermerk gilt dem konfigurierten Modell |
| `web-audit` | nein | | kein Web-Frontend; LuminaRust ist Desktop-GUI + CLI |
| Konvention: 500-Zeilen-Ratetch | ja | `scripts/check_file_sizes.sh` (`THRESHOLD=500`), `scripts/file_size_baseline.txt`, `Agents.md` §„Verifizierungs-Agent" | |
| Konvention: Projektsprache Deutsch (Doku + UI) | ja | `Agents.md`, `Agents.todo.md`, `feature/` | |
| Konvention: Regel referenziert, nicht kopiert | ja | `Agents.md` §„Build-Agent" → Skill `build-verify` (Lieferweg: **Skill-Trigger**) und die Minimalregeln in `.agents/rules/build-verify.md` (agents-skills, Pfad-Verweis, keine Kopie); global eingebettet ist nur die Frage-Regel (`~/.config/opencode/AGENTS.md`, Quelle `ask.md`) | |
| Konvention: visueller Review mit dem konfigurierten Modell (DoD §6) | ja | `DoD.md` §6 (KI-Validierungs-Loop, Schritt 2), `Agents.todo.md` `F-103-N6-GUI-COVERAGE-27` Punkt 11, `feature/platform/lightroom-ux-parity.md` (Abnahme), `crates/lumina-gui-harness/README.md` — alle vier Stellen nennen **das konfigurierte Modell selbst** (max. 10 Bilder je Durchgang); ein Vision-Subagent existiert nicht | |

## Offen aus dem Bereich `86d677e..787670a`

Geprüft am 2026-10-19 gegen `787670a` (26 Commits — `git -C <skills-repo> rev-list --count
86d677e..787670a`; 21 Skill-Verzeichnisse berührt). Drei offene
Positionen waren in `Agents.todo.md` angelegt: `VISION-SUBAGENT-PFLEGE`,
`SECURITY-REVIEW-REGISTER`, `ALWAYSON-REGEL-WORTLAUT` — **keine davon ist heute
noch offen**: `VISION-SUBAGENT-PFLEGE` ist am 2026-10-20 geschlossen und aus dem
Plan entfernt (`cfc8ebd`), `ALWAYSON-REGEL-WORTLAUT` am 2026-10-20 (`d73f313`),
`SECURITY-REVIEW-REGISTER` am 2026-10-20 durch die Eigentümerentscheidung (Flow
übernommen, Register angelegt, `905f4b5`); offen ist daraus allein die Folge
`SEC-REVIEW-WELLE-1-85` (erste Review-Welle) im Abschnitt „Folge aus
`SECURITY-REVIEW-REGISTER`" des Plans.

**Neu**

- `security-review` (`82a9abb`, hidden) — trifft zu (erreichbare Oberflächen:
  MCP-Server, CLI, Datei-/Sidecar-I/O). **Stand 2026-10-20: drin** — das Repo hat
  das Sicherheits-/Risikoregister `feature/quality/security-review-register.md`
  (Form nach Stage 6, Enumerationsbasis mit den erreichbaren Oberflächen — die
  Zahl steht im generierten Block **surfaces** in §5 des Registers —, null
  Befunde). Die offene Position `SECURITY-REVIEW-REGISTER` ist am 2026-10-20
  geschlossen und aus dem Plan entfernt (`905f4b5`, Eigentümerentscheid „Flow
  übernommen"); offen ist daraus allein die Folge `SEC-REVIEW-WELLE-1-85` — die
  **erste** Review-Welle (Finden, dann Schließen in Wellen).
- `models` (`1a096ba`), umbenannt in `update-models` (`fc28ea3`) — trifft nicht
  zu: Modell-Registry ist Maschinenstand (`opencode.jsonc`), nicht Projektstand.

**Geändert**

- `build-verify` — trifft zu und ist angewendet: der Pflicht-Loop (Pull →
  Delegation → unabhängige Verifikation → Commit je Runde → Push + CI) läuft
  unverändert; belegt in `Agents.md` §„Build-Agent"/§„Verifizierungs-Agent" und
  im `Verify:`-Footer der Historie. Zwei Änderungen mit Projektbezug:
  (a) **Lieferweg der Regel** — upstream sagt jetzt, die Regeldatei werde
  **nicht** über den `instructions`-Array geliefert (der Array ist leer), sondern
  über den Skill-Trigger und den `AGENTS.md` des Projekts; die Formulierung
  „always-on Kernel" in `Agents.md` deckte das nicht mehr → offene Position
  `ALWAYSON-REGEL-WORTLAUT` — **am 2026-10-20 geschlossen und aus dem Plan
  entfernt** (`d73f313`): `Agents.md` §„Build-Agent" nennt jetzt den
  Skill-Trigger als Lieferweg und verweist auf `.agents/rules/build-verify.md`;
  (b) der **optionale** Meilenstein-Review (nur bei Releases, N pro Projekt) —
  vom Projekt nicht festgelegt; der Pflicht-Loop bleibt unberührt, für dieses
  Projekt kein Fall.
- `agent-config` — trifft nicht zu (Maschinenkonfiguration). Die
  projektbezogene Aussage der Änderung („ein Projekt verweist auf die
  Regeldatei und wiederholt den Flow nicht") ist im Projekt erfüllt.
- `codegraph-project-setup` — trifft zu und ist angewendet: `.codegraph/` und
  der Pre-Commit-Hook (Gate 1) sind da. **Ankerkorrektur:** der Anker der
  Vorversion („`AGENTS.md` CodeGraph-Abschnitt") zeigte ins Leere — `Agents.md`
  enthält keinen CodeGraph-Abschnitt. Die gestrichene Vision-Eskalation
  (`vision-creative`) betrifft das Projekt nicht: `Agents.md` nennt keinen
  Vision-Subagenten.
- `docker-test-image`, `ghcr-visibility`, `github-ci-filters` — treffen zu und
  sind angewendet (Digest-Pin im `docker/Dockerfile`, fünf `container:`-Jobs,
  `paths-ignore` für `push` und `pull_request`). Die Änderungen sind
  Beschreibungs-Kürzungen plus `autoinvoke: false` — die drei Skills sind jetzt
  **hidden** und nur noch per exakter ID ladbar; das Projekt nennt sie in
  keiner Datei, es gibt also nichts umzuschreiben. **Ankerkorrektur:** die
  `ci.yml`-Zeilennummern der Vorversion (`120-121,257-258,306-307` bzw.
  `111-121`) waren veraltet; `container:`/`ghcr.io` stehen heute in den Jobs
  „Rust fast checks", „Rust GUI tests", „Rust tests (excluding GUI)", „Rust
  agent-harness" und „Benchmarks (optional)".
- `laravel`, `node-deps`, `permissions`, `playwright-parallel`, `react-i18n`,
  `tailscale-serve`, `web-audit` — treffen nicht zu: Beschreibungs-Kürzungen und
  `autoinvoke: false`; das Repo hat weder PHP, Node, Playwright oder React noch
  ein Web-Frontend, `permissions` ist Maschinenpolicy.
- `ui-review` — als Playwright-Flow trifft es nicht zu (native egui-GUI). Die
  **geänderte** Regel derselben Runde (kein Vision-Subagent: das konfigurierte
  Modell ist vision-fähig und liest harte Fälle selbst nach, statt zu raten)
  traf die Projekt-Pflicht in `DoD.md` §6 → **dort umgesetzt**: die vier
  Fundstellen (`DoD.md` §6, `Agents.todo.md` `F-103-N6-GUI-COVERAGE-27` Punkt 11,
  `feature/platform/lightroom-ux-parity.md`, `crates/lumina-gui-harness/README.md`)
  nennen jetzt **das konfigurierte Modell selbst** (max. 10 Bilder je Durchgang);
  die offene Position `VISION-SUBAGENT-PFLEGE` ist am 2026-10-20 geschlossen und
  aus dem Plan entfernt (`cfc8ebd`).
- `skills-marker` — trifft zu und ist angewendet: diese Datei. Zwei Änderungen
  mit Projektbezug: der Ablauf-Abschnitt unten folgt jetzt der neuen
  Drift-Definition (Drift = **geänderte Pflicht**, kein Wortlaut-Diff), und der
  Skill ist jetzt **hidden** (nur per exakter ID ladbar) — die Projektdatei
  enthält den Ablauf ohnehin als eigenen Abschnitt.

**Entfernt** (die Zeile fällt mit diesem Fortschreiben weg, der Befund bleibt im Log)

- `vision-agents` — **entfernt in `dccdd40`**, mit ihm der `vision-creative`-
  Subagent. Das Projekt erwartete den Vision-Subagenten zum Zeitpunkt des
  Marker-Runs: `DoD.md` §6 nannte `vision-technical` (max. 10 Bilder),
  `Agents.todo.md` `F-103-N6-GUI-COVERAGE-27` Punkt 11 nannte „plus Vision-Review",
  `feature/platform/lightroom-ux-parity.md` (Abnahme) verwies auf „DoD §6
  Vision-Review". Folge war: die Pflicht war in der damaligen Fassung **nicht
  ausführbar** → offene Position `VISION-SUBAGENT-PFLEGE` — **am 2026-10-20
  geschlossen und aus dem Plan entfernt** (`cfc8ebd`): `DoD.md` §6 nennt jetzt
  das konfigurierte Modell selbst und hält fest, dass es keinen Vision-Subagenten
  gibt; dieselbe Umstellung tragen `Agents.todo.md` Punkt 11, die Abnahme in
  `feature/platform/lightroom-ux-parity.md` (der Verweis „DoD §6 Vision-Review"
  steht weiter) und `crates/lumina-gui-harness/README.md`.
- `model-updater` — **entfernt in `1c51237`**, in `models` (→ `update-models`)
  aufgegangen; die Zeile ist durch die `update-models`-Zeile ersetzt.
- `update-opencode-models` — **entfernt in `1c51237`**, in `models` (→
  `update-models`) aufgegangen; dieselbe Ersetzung.

**Ohne Skill-Bezug** (Commits der Range, die dieses Projekt nicht betreffen)

- `787670a` (Root-`AGENTS.md` des Skills-Repos auf Regeln reduziert, die
  Entscheidungen wandern ins README), `2d5793b` (Command-Templates im
  Skills-Repo versioniert), `c3f3e10` (Skill-Budget-Gate in der CI des
  Skills-Repos) — alle drei betreffen das Skills-Repo selbst.

## Offen aus dem Bereich `787670a..00dbd22`

Geprüft am 2026-10-20 gegen `00dbd22` (Range `787670a..00dbd22`).

**Neu**

- `ratchet-gates` (`70d49e0`, hidden) — trifft zu und ist **angewendet**: der
  Skill verlangt zwei benannte, dokumentierte und lokal lauffähige
  Gate-Stufen, einen Floor für die schnelle Stufe und eine Lückenliste
  zwischen den Stufen. Umgesetzt in `scripts/gates.sh` (Stufen `fast`
  (Sekunden, ohne Codegen, Floor `MIN_CHECKS=9`), `slow` (Minuten, CI-Mirror)
  und `list` als Quelle der Wahrheit für die Tier-Tabelle), in
  `feature/quality/local-gates.md` (Tier-Tabelle wortgleich zu
  `sh scripts/gates.sh list`, Floor-Regel, Lückenliste mit Klasse, Grund,
  Owner und Task-ID je Lücke), in `Agents.md` §„Verifizierung und Tests" und
  im CI-Schritt „Run fast tier (scripts/gates.sh fast)" (Job „Rust fast
  checks"), damit Definition und Ausführung dieselbe sind. Zusätzlich
  tragen die **elf** `scripts/check_*.sh` den Gate-Header (Defektklasse, der
  konkrete Anlass, die Ratchet-Richtung, die benannte Grenze) — reine
  Kommentarzeilen, keine Verhaltensaenderung.
- `generated-docs` (`00dbd22`, hidden) — trifft zu und ist **angewendet**: der
  Generator `scripts/gen_doc_facts.sh` (regelbasiert, deterministisch, git-frei,
  POSIX sh) und das Drift-Gate `scripts/check_doc_facts_drift.sh` (erzeugen →
  Block aus der Zieldatei extrahieren → diff; meldet, schreibt nie) sind umgesetzt,
  dazu das Hygiene-Gate `scripts/check_doc_facts_generator.sh` (`fast-9`:
  Vollständigkeit, Determinismus, Git-Freiheit). Zehn Blöcke in sieben Dateien sind
  generiert; die Prosa-Zahlen dieser Populationen stehen nur im generierten Block.
  **Offen** (Position `DOC-ZAHLEN-DRIFT-82`, Welle 3): der R/C-Split der Goldens
  (14/52) und die Verteilung auf die Test-Targets bleiben handgepflegt (ihre
  Klassifikation ist es auch), Testlaufzahlen sind nicht ableitbar, und das
  CLI-Coverage-Handinventar „Abgedeckt (29)“ in `feature/platform/mcp-server.md`
  altert stumm (eine eigene Population, kein Gate) — Folgeaufgabe.

**Geändert**

- `ui-review` (`fa2268f`) — die Findings-Vorlage nennt keinen „vision-agent"
  mehr. Als Playwright-Flow trifft der Skill dieses Projekt weiterhin nicht
  (native egui-GUI); die Projekt-Pflicht in `DoD.md` §6 wird von der
  geänderten Regel berührt und ist dort umgesetzt → die offene Position
  `VISION-SUBAGENT-PFLEGE` ist am 2026-10-20 geschlossen und aus dem Plan
  entfernt (`cfc8ebd`).
- `update-models` (`219171c`, `8c6b9af`) — die Namespace-Regel steht jetzt auf
  der Entscheidung (`opencode-go-only`) statt auf einem
  Kontext-/Preisargument, und der `$0`-Vermerk gilt dem konfigurierten
  Modell. Der Skill trifft dieses Projekt nicht (Modell-Registry ist
  Maschinenstand). Die Marker-Zeile wurde gegen die tatsächliche Konfiguration
  gelesen (`opencode.jsonc`, `model-preferences.md`, Stand 2026-10-20) und
  nennt den belegten Stand: `opencode-go/minimax-m3.1-flash-free` im Root und
  in den vier Rollen `plan`/`general`/`build`/`explore`.
- `agent-config` (`70d49e0`, `00dbd22`) — nur die Liste der versteckten Skills
  im Skills-Repo hat sich geändert (die Zähler 12 → 13 → 14). Das Projekt
  hat keine eigene Agent-Konfiguration; die projektbezogene Aussage der
  Änderung (ein Projekt verweist auf die Regeldatei, statt den Flow zu
  wiederholen) ist im Projekt erfüllt → keine Änderung nötig.

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
   `geprüft am:` auf heute, Abschnitte ersetzen.
6. Falls Regeln oder Skills betroffen waren: die eingebetteten Regeln in der
   globalen `~/.config/opencode/AGENTS.md` gegen ihre Quelle in
   `.agents/rules/*` prüfen und bei geänderter **Pflicht** sinngemäß nachziehen.
   Drift ist eine geänderte Pflicht, kein abweichender Wortlaut — die Einbettung
   ist die Kurzform der Pflicht, kein wörtliches Abbild, und ein Text-Diff gegen
   die Quelle ist **kein** Drift. Dann `touch ~/.config/opencode/opencode.jsonc`
   (Reload der globalen Config; das `instructions`-Array ist leer und liefert
   nichts).
