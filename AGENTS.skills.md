# Skills-Marker

agents-skills-consumed: 86d677e
geprüft am: 2026-10-12

## Skill-Stand

| Skill | angewendet? | wo im Projekt | offene Position |
|---|---|---|---|
| `build-verify` | ja | `Agents.md:149-152` (Skill-Zeiger + always-on-Kernel); Flow in der Git-Historie (`Verify:`-Footer je Runde) | |
| `skills-marker` | ja | diese Datei (`AGENTS.skills.md`) | |
| `github-ci-filters` | ja | `.github/workflows/ci.yml` — `paths-ignore` für Docs-only-Pushes (Commit `c4e2fd8`) | |
| `codegraph-project-setup` | ja | `AGENTS.md` CodeGraph-Abschnitt; `.codegraph/` vorhanden | |
| `docker-test-image` | ja | `docker/Dockerfile`, verdrahtet über `container:` in `.github/workflows/ci.yml:120-121,257-258,306-307` | |
| `ghcr-visibility` | ja | `.github/workflows/ci.yml:111-121` (`ghcr.io/reisi007/luminarust/lumina-ci:latest`) — Pull-Autorisierung ist für Fork-PRs relevant | |
| `vision-agents` | ja | `Agents.todo.md` `F-103-N6-GUI-COVERAGE-27` („plus Vision-Review" für visuelle Änderungen) | |
| Konvention: 500-Zeilen-Ratetch | ja | `scripts/check_file_sizes.sh:2-7`, `scripts/file_size_baseline.txt`, `Agents.md` | |
| Konvention: Projektsprache Deutsch (Doku + UI) | ja | `Agents.md`, `Agents.todo.md`, `feature/` | |
| Konvention: eingebettete Always-on-Regel | ja | `Agents.md:149-152` → `/projects/agents-skills/.agents/rules/build-verify.md` (Pfad-Verweis, keine Kopie) | |
| `agent-config` | nein | | Maschinenkonfiguration (`opencode.jsonc`), nicht Projektstand |
| `laravel` | nein | | Rust-Workspace, kein PHP |
| `node-deps` | nein | | keine `package.json`/pn-Pins; Abhängigkeiten laufen über Cargo |
| `permissions` | nein | | Maschinenpolicy (`opencode.jsonc`), nicht Projektstand |
| `playwright-parallel` | nein | | keine Playwright-Suite; E2E sind `egui_kittest`-Targets in `crates/lumina-gui/tests/` |
| `react-i18n` | nein | | GUI ist egui/Rust mit eigener `Str`-i18n, kein React/Lingui |
| `tailscale-serve` | nein | | headless-Host, kein Dev-Server-Expose |
| `ui-review` | nein | | native egui-GUI; Review läuft über kittest-Goldens/PSNR, nicht über Playwright-Screenshots |
| `update-opencode-models` | nein | | Modellwahl ist User-/Maschinensache; die Projekt-Regel `MODELL` betrifft freie Subagenten, nicht das Registry-Refresh |
| `model-updater` | nein | | dto. — Preis-/Modell-Tracking ist keine Projektangelegenheit |
| `web-audit` | nein | | kein Web-Frontend; LuminaRust ist Desktop-GUI + CLI |

## Offen aus dem Bereich `<alt>..<neu>`

Neuanlage des Markers 2026-10-12 — kein Vor-Stand im Projekt vorhanden, daher
keine historische Range. Geprüft und eingetragen sind der Stand `86d677e` und
die beiden Änderungen des gezogenen Pulls:

- `web-audit` (neu) — trifft nicht zu: kein Web-Frontend. Begründung in der Tabelle.
- `build-verify` (geändert: „verify the substance, not the gates") — trifft zu und
  ist ab dieser Runde angewendet: Verifier prüft Substanz, grün gemeldete Gates
  werden nicht nachgefahren (vier Ausnahmen, s. Skill-Referenz). Belegt in der
  Historie ab Commit `8fd23ce` (`JSON-FLOAT-ROUNDTRIP` Runde 4).

## Fortschreiben

1. `agents-skills-consumed` ablesen → `<alt>`.
2. Im Skills-Repo die Range bilden:
   `git -C /projects/agents-skills log --oneline <alt>..HEAD`
   `git -C /projects/agents-skills log --name-only --format= <alt>..HEAD -- .agents/skills | sort -u`
3. Jeden betroffenen Skill gegen **dieses** Projekt prüfen. Trifft eine Änderung zu, wenn
   sie eine Pflicht, Schwelle, Ausnahme, Konvention oder ein Kommando betrifft, das
   **hier** gilt:
   - trifft zu und ist drin → `angewendet: ja` + `Datei:Zeile`
   - trifft zu, ist nicht drin → offene Position: Eintrag in `Agents.todo.md` anlegen
   - trifft nicht zu (das Projekt hat das nicht) → `angewendet: nein` + kurzer Grund
4. Offene Positionen nach `Agents.todo.md`; angewendete Zeilen mit `Datei:Zeile` belegen.
5. Erst wenn für **jeden** Skill aus der Range eine Zeile steht:
   `agents-skills-consumed: $(git -C /projects/agents-skills rev-parse --short HEAD)`,
   `geprüft am:` auf heute, Abschnitte ersetzen.
6. Falls Regeln oder Skills betroffen waren: eingebettete Regel-Kopien in der
   globalen `~/.config/opencode/AGENTS.md` nachziehen (Drift!), dann
   `touch ~/.config/opencode/opencode.jsonc`.
