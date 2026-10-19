# Skills-Marker

agents-skills-consumed: 787670a
geprüft am: 2026-10-19

## Skill-Stand

| Skill | angewendet? | wo im Projekt | offene Position |
|---|---|---|---|
| `build-verify` | ja | `Agents.md` §„Rollen und Delegation" → „Build-Agent" (Zeiger auf den Skill + `.agents/rules/build-verify.md`, ohne Kopie) und §„Verifizierung und Tests" (repo-spezifische Gates); `Verify:`-Footer je Runde in der Historie | |
| `skills-marker` | ja | diese Datei (`AGENTS.skills.md`); der Skill ist jetzt **hidden** (nur per exakter ID ladbar), der Abschnitt „Fortschreiben" unten ist die lokale Kopie des Ablaufs | |
| `github-ci-filters` | ja | `.github/workflows/ci.yml` — `paths-ignore` für `push` **und** `pull_request` (Commit `c4e2fd8`) | |
| `codegraph-project-setup` | ja | `.codegraph/` (Index) + `.githooks/pre-commit` Gate 1 „CodeGraph index sync" (`core.hooksPath=.githooks`) — **kein** CodeGraph-Abschnitt in `Agents.md` (der Anker der Vorversion zeigte ins Leere) | |
| `docker-test-image` | ja | `docker/Dockerfile` (LibRaw 0.22.2, Digest-Pin) + fünf `container:`-Jobs in `.github/workflows/ci.yml` | |
| `ghcr-visibility` | ja | `.github/workflows/ci.yml` — `ghcr.io/reisi007/luminarust/lumina-ci:latest` in denselben fünf `container:`-Jobs | |
| `security-review` (neu) | nein (trifft zu, nicht drin) | erreichbare Oberflächen: MCP-Server (`feature/platform/mcp-server.md`), CLI (`crates/lumina-cli/`), Datei-/Sidecar-I/O | `Agents.todo.md` → `SECURITY-REVIEW-REGISTER` |
| `ui-review` | nein | als Playwright-Flow trifft es nicht zu: native egui-GUI, der visuelle Review läuft über `egui_kittest`-Goldens/PSNR und den Fenster-Review `UI-VISION-REVIEW-70`. Die **geänderte** Regel derselben Runde (kein Vision-Subagent) trifft die Projekt-Pflicht in `DoD.md` §6 → dort geführt | `Agents.todo.md` → `VISION-SUBAGENT-PFLEGE` |
| `agent-config` | nein | | Maschinenkonfiguration (globale `opencode.jsonc`, globale `AGENTS.md`); die projektbezogene Aussage der Änderung — ein Projekt verweist auf die Regeldatei, statt den Flow zu wiederholen — ist im Projekt erfüllt |
| `laravel` | nein | | Rust-Workspace, kein PHP |
| `node-deps` | nein | | keine `package.json`/pn-Pins; Abhängigkeiten laufen über Cargo |
| `permissions` | nein | | Maschinenpolicy (`opencode.jsonc`), nicht Projektstand |
| `playwright-parallel` | nein | | keine Playwright-Suite; E2E sind `egui_kittest`-Targets in `crates/lumina-gui/tests/` |
| `react-i18n` | nein | | GUI ist egui/Rust mit eigener `Str`-i18n, kein React/Lingui |
| `tailscale-serve` | nein | | headless-Host, kein Dev-Server-Expose |
| `update-models` (neu; ersetzt `model-updater` + `update-opencode-models`) | nein | | Modell-Registry ist Maschinenstand (`opencode.jsonc`); die Projekt-Entscheidung `MODELL` (nur freie Subagenten) bleibt unberührt — die Maschine fährt `opencode-go/minimax-m3.1-flash-free` in Root und allen vier Rollen |
| `web-audit` | nein | | kein Web-Frontend; LuminaRust ist Desktop-GUI + CLI |
| Konvention: 500-Zeilen-Ratetch | ja | `scripts/check_file_sizes.sh` (`THRESHOLD=500`), `scripts/file_size_baseline.txt`, `Agents.md` §„Verifizierungs-Agent" | |
| Konvention: Projektsprache Deutsch (Doku + UI) | ja | `Agents.md`, `Agents.todo.md`, `feature/` | |
| Konvention: Regel referenziert, nicht kopiert | ja | `Agents.md` §„Build-Agent" → `.agents/rules/build-verify.md` (Pfad-Verweis, keine Kopie); global eingebettet ist nur die Frage-Regel (`~/.config/opencode/AGENTS.md`, Quelle `ask.md`) | `Agents.todo.md` → `ALWAYSON-REGEL-WORTLAUT` (nur die Bezeichnung „always-on Kernel" ist überholt: der `instructions`-Array ist leer, die Regeldatei allein liefert nichts) |
| Konvention: visueller Review mit Vision-Agent (DoD §6) | nein (trifft zu, nicht drin) | `DoD.md` §6 (KI-Validierungs-Loop, Schritt 2), `Agents.todo.md` `F-103-N6-GUI-COVERAGE-27` Punkt 11, `feature/platform/lightroom-ux-parity.md` (Abnahme) | `Agents.todo.md` → `VISION-SUBAGENT-PFLEGE` |

## Offen aus dem Bereich `86d677e..787670a`

Geprüft am 2026-10-19 gegen `787670a` (27 Commits, 21 Skills berührt). Drei offene
Positionen sind in `Agents.todo.md` angelegt: `VISION-SUBAGENT-PFLEGE`,
`SECURITY-REVIEW-REGISTER`, `ALWAYSON-REGEL-WORTLAUT`.

**Neu**

- `security-review` (`82a9abb`, hidden) — trifft zu (erreichbare Oberflächen:
  MCP-Server, CLI, Datei-/Sidecar-I/O) und ist **nicht drin**: das Repo hat weder
  ein Sicherheits-/Risikoregister noch eine Security-Review-Pflicht in
  `Agents.md`/`DoD.md`, der Skill verlangt in Stage 6 aber ein Register im Repo.
  → offene Position `SECURITY-REVIEW-REGISTER`.
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
  „always-on Kernel" in `Agents.md` deckt das nicht mehr → offene Position
  `ALWAYSON-REGEL-WORTLAUT`;
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
  trifft die Projekt-Pflicht in `DoD.md` §6 → dort geführt (offene Position
  `VISION-SUBAGENT-PFLEGE`).
- `skills-marker` — trifft zu und ist angewendet: diese Datei. Zwei Änderungen
  mit Projektbezug: der Ablauf-Abschnitt unten folgt jetzt der neuen
  Drift-Definition (Drift = **geänderte Pflicht**, kein Wortlaut-Diff), und der
  Skill ist jetzt **hidden** (nur per exakter ID ladbar) — die Projektdatei
  enthält den Ablauf ohnehin als eigenen Abschnitt.

**Entfernt** (die Zeile fällt mit diesem Fortschreiben weg, der Befund bleibt im Log)

- `vision-agents` — **entfernt in `dccdd40`**, mit ihm der `vision-creative`-
  Subagent. Das Projekt erwartet den Vision-Subagenten weiterhin: `DoD.md` §6
  nennt `vision-technical` (max. 10 Bilder), `Agents.todo.md`
  `F-103-N6-GUI-COVERAGE-27` Punkt 11 nennt „plus Vision-Review",
  `feature/platform/lightroom-ux-parity.md` (Abnahme) verweist auf „DoD §6
  Vision-Review". Folge: die Pflicht ist in der jetzigen Fassung **nicht
  ausführbar** → offene Position `VISION-SUBAGENT-PFLEGE`.
- `model-updater` — **entfernt in `1c51237`**, in `models` (→ `update-models`)
  aufgegangen; die Zeile ist durch die `update-models`-Zeile ersetzt.
- `update-opencode-models` — **entfernt in `1c51237`**, in `models` (→
  `update-models`) aufgegangen; dieselbe Ersetzung.

**Ohne Skill-Bezug** (Commits der Range, die dieses Projekt nicht betreffen)

- `787670a` (Root-`AGENTS.md` des Skills-Repos auf Regeln reduziert, die
  Entscheidungen wandern ins README), `2d5793b` (Command-Templates im
  Skills-Repo versioniert), `c3f3e10` (Skill-Budget-Gate in der CI des
  Skills-Repos) — alle drei betreffen das Skills-Repo selbst.

## Fortschreiben

1. `agents-skills-consumed` ablesen → `<alt>`.
2. Im Skills-Repo die Range bilden:
   `git -C /projects/agents-skills log --oneline <alt>..HEAD`
   `git -C /projects/agents-skills log --name-only --format= <alt>..HEAD -- .agents/skills | sort -u`
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
   `agents-skills-consumed: $(git -C /projects/agents-skills rev-parse --short HEAD)`,
   `geprüft am:` auf heute, Abschnitte ersetzen.
6. Falls Regeln oder Skills betroffen waren: die eingebetteten Regeln in der
   globalen `~/.config/opencode/AGENTS.md` gegen ihre Quelle in
   `.agents/rules/*` prüfen und bei geänderter **Pflicht** sinngemäß nachziehen.
   Drift ist eine geänderte Pflicht, kein abweichender Wortlaut — die Einbettung
   ist die Kurzform der Pflicht, kein wörtliches Abbild, und ein Text-Diff gegen
   die Quelle ist **kein** Drift. Dann `touch ~/.config/opencode/opencode.jsonc`
   (Reload der globalen Config; das `instructions`-Array ist leer und liefert
   nichts).
