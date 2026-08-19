# Fixtures, Modelle & Lizenz-/Distributionsprüfung

**Feature-IDs:** F-073 (kleine versionierte Referenzbilder, RAW-Fixtures und
Modelle inkl. Lizenzinformationen) · F-078 (Lizenz-, Modell- und
Distributionsprüfung vor dem ersten Release)
**Status:** SOLL dokumentiert; Umsetzung als Dokumentation + Audit abgeschlossen,
noch nicht verifiziert.
**Autorität:** `Agents.todo.md` (Phase 11, F-073/F-078); begleitende Audit-Docs:
[`docs/fixtures-and-licensing.md`](../../docs/fixtures-and-licensing.md) und
[`THIRD-PARTY-NOTICES.md`](../../THIRD-PARTY-NOTICES.md).
**Verwandt:** `feature/README.md`, `docs/adr/0002-raw-backend.md`,
`feature/quality/performance-benchmarks.md` (Fixtures-Regeln), `README.md`
(LibRaw-Hinweis).

---

## 1. Ziel

LuminaRust muss vor dem ersten Release

1. kleine, versionierte, **reproduzierbar erzeugte** Test-Fixtures und
   referenzierbare Rohdaten bereitstellen (F-073);
2. **alle** Abhängigkeiten, Modelle und nativen Bibliotheken auf
   Lizenzkompatibilität prüfen und dokumentieren (F-078).

Reproduzierbarkeit und Lizenzklarheit sind Release-Gates — kein stillschweigender
Fallback bei unklaren Artefakt- oder Lizenzlagen.

---

## 2. Geltungsbereich

Dieses Dokument erfasst:

- synthetische Benchmark-Fixtures (`crates/lumina-bench/bench/common/mod.rs`);
- committete RAW-Fixtures (`sample-data/raw/*.cr3`);
- ML-Modelle (BiRefNet, SAM 2, ONNX Runtime) inkl. Lizenz;
- die vollständige Rust-Abhängigkeitsmenge (Cargo-Metadata, default + all-features);
- native C-Bibliothek LibRaw (über `vendor/libraw-sys`).

Nicht Gegenstand: Golden-Image-Tests (deferred F-043/F-073), zentrale DB
(es gibt in v1 keine).

---

## 3. Fixture-Inventar (SOLL/Ist)

### 3.1 Synthetische Benchmark-Fixtures

Vollständig lokal und deterministisch erzeugt (`bench/common/mod.rs`), kein
Netzwerk:

- `FIXTURE_SEED: u64 = 0x5EED` (eingefroren);
- PRNG: `SplitMix64` (dependency-free, im Modul), per Größe abgeleitet;
- `SIZES = [512, 1024, 2048]`;
- Hilfsfunktionen: `make_frame`, `make_recipe`, `make_mask_fixture`,
  `make_cache_fixture`.

Vertrag: Seed-Änderung macht `perf/baseline.json` ungültig → vor
Median/p95-Vergleich neu aufzeichnen.

### 3.2 Committete RAW-Fixtures

| Datei | Maße | Orientierung | Verwendung |
| --- | --- | --- | --- |
| `sample-data/raw/aircraft-landscape.cr3` | 6032×4024 | 1 | `lumina-raw`-Test `aircraft_landscape_fixture_*`; Decode-Bench |
| `sample-data/raw/aircraft-portrait.cr3` | 4024×6032 | 5 | `lumina-raw`-Test `aircraft_portrait_fixture_*`; Decode-Bench |

Decode-Benchmarks lesen das Verzeichnis über die Env-Variablen
**`LUMINA_RAW_FIXTURE`**; ohne sie wird sauber übersprungen (kein Panic, kein
Fallback). Ein separater, `#[ignore]`-Test erwartet eine *eigene lizenzierte*
RAW über dieselbe Env-Variablen.

### 3.3 Golden-Referenzbilder

Noch nicht vorhanden (deferred F-043/F-073). Bei Einführung: selbe
Versionierungs-/Determinismusregeln wie §3.1, explizite Lizenz (bevorzugt
generiert/CC0).

---

## 4. Rohdaten-Provenance & Lizenz — ⚠️ OFFENE LÜCKE

**Befund:** Die beiden committeten `.cr3`-Fixtures in `sample-data/raw/` haben
**keine dokumentierte Quelle, keinen Autor, keine Lizenz**. Kein `LICENSE`/`README`
in `sample-data/`; der einführende Commit (`1e388bf`) nennt keine Provenance.

Das ist ein **Release-Blocker** (F-078): urheberrechtlich geschützte Kamera-RAWs
ohne Lizenzgewährung zu distribuieren ist ein rechtliches Risiko — unabhängig
von der (MIT) Rust-Code-Lizenz. Die `*_fixture_*`-Tests hängen hart an genau
diesen Bytes.

→ Empfohlene Maßnahme R1 (siehe §7): Provenance + Lizenz dokumentieren **oder**
durch generierte/synthetische bzw. CC0-lizenzierte Fixtures ersetzen, bevor
distribuiert wird.

---

## 5. Modell-Inventar & Lizenzen

| Modell | Rolle | Lizenz | Status |
| --- | --- | --- | --- |
| **BiRefNet** (Zheng et al., arXiv:2401.03407) | erstes automatisches Subjekt-Modell | **Apache-2.0** (Literal in `manifest.rs`) | Gewichte *pending integration* (`model_hash = "pending-integration"`) |
| **SAM 2** | erstes interaktives Box/Pinsel-Modell | **TBD** (bei Integration verifizieren) | nur geplant |
| **ONNX Runtime** (`ort` 2.0.0-rc.13) | Inferenz-Runtime | **MIT** (ORT `MIT OR Apache-2.0`, `ort-sys` `MIT OR Apache-2.0`) | **optional**, Feature `onnx-rt`, nicht im Default-Build |

- Es sind **keine Modellgewichte** committet; die Lizenzpflicht entsteht erst
  beim Bündeln der Gewichte (F-048).
- ONNX Runtime (echt) lädt **Prebuilt-Binaries zur Build-Zeit** (Netz); bei
  Release-Freigabe von `onnx-rt` dessen Redistribution prüfen (R4).

---

## 6. Abhängigkeits-Audit (F-078)

### 6.1 Methode

`cargo metadata` (default → **441** Pakete; `--all-features` inkl. `ort` →
**478** Pakete), Lizenzen aus dem `license`-SPDX-Feld, gegen `Cargo.lock`
abgeglichen. Vollständige Tabelle: `THIRD-PARTY-NOTICES.md`.

### 6.2 Ergebnis

- **Keine** GPL/AGPL/SSPL/MPL/EPL-Abhängigkeit (weder default noch all-features).
- Dominanz: MIT / `MIT OR Apache-2.0` / Apache-2.0 / BSD / ISC / Zlib / 0BSD /
  CC0-1.0 / Unlicense / BSL-1.0 — alles OSI-konform.
- Schwach-copyleft (`LGPL-2.1-or-later`): **nur `r-efi`**, und **nur für
  `uefi`**-Targets (transitiv über `getrandom`-UEFI-Backend); in keiner
  ausgelieferten macOS/Linux/Windows/WASM-Build kompiliert. Über die `OR`-Klausel
  unter MIT/Apache erfüllbar.
- **Einzig reale Pflicht:** LibRaw (siehe §6.3).

### 6.3 LibRaw (einzige reale Verpflichtung)

`lumina-raw` → `vendor/libraw-sys` (MIT, © David Cuddeback, via
`[patch.crates-io]` gepinnt). Dessen `build.rs` linkt die **System**-Bibliothek
`libraw_r` über `pkg-config` (**dynamisch**, nicht vendored/statisch).

- Upstream LibRaw ist **dreifach lizenziert**: LGPL-2.1-or-later **ODER**
  CDDL-1.0 **ODER** *LibRaw Software License* (permissiv, BSD-artig).
  > Hinweis: `docs/adr/0002-raw-backend.md` nennt bisher nur „dual
  > (LGPL/CDDL)“ — die dritte, permissive Option fehlt dort (R5).
- **Verpflichtung:** Dynamisches Linken beibehalten (statisches Einbetten würde
  LGPL auf das Gesamtwerk ausweiten); LibRaw-Lizenztext + Quellangebot für die
  verwendete Version mitliefern. CI pinnt **LibRaw 0.22.2** (OCI-Label
  `lumina.libraw_version`). Bevorzugt die permissive *LibRaw Software License*
  nutzen.
- Bereits im `README.md` vermerkt („LibRaw steht unter der LGPL-2.1-or-later;
  Distributionen müssen die LibRaw-Lizenz …“).

### 6.4 Kompatibilitätsmatrix (Kurzform)

| Lizenzfamilie | OSI | Risiko | Aktion |
| --- | --- | --- | --- |
| MIT / Apache-2.0 / BSD / ISC / Zlib / 0BSD / CC0 / Unlicense / BSL | ✅ | keine | Notice bündeln |
| Unicode-3.0, OFL-1.1/Ubuntu-Font, Apache-2.0+LLVM-exc | ✅ | keine (Attribution) | Notice/Font-Lizenz bündeln |
| `r-efi` (LGPL-2.1-or-later, UEFI-only) | ✅ unter MIT/Apache | nur bei UEFI-Build | nicht ausgeliefert → keine Aktion |
| **LibRaw** (LGPL/CDDL/LibRaw-SW) | ⚠️ schwach | **einzige Pflicht** | dynamisch linken + Notice/Quellangebot |

---

## 7. Versionierungs-Policy

| Artefakt | Pin-Mechanismus | Wert |
| --- | --- | --- |
| Rust-Baum gesamt | committetes `Cargo.lock`, `resolver = "2"` | autoritativ |
| `ort` / `ort-sys` | exakte Version in `lumina-onnx/Cargo.toml` | `=2.0.0-rc.13` |
| `libraw-sys` | `[patch.crates-io]` → `vendor/libraw-sys` | gepatcht `0.1.1` |
| LibRaw (nativ) | CI-Image OCI-Label `lumina.libraw_version` | `0.22.2` |
| Benchmark-Fixtures | eingefrorener `FIXTURE_SEED = 0x5EED` + `SplitMix64` | Änderung ⇒ Re-Baseline |
| Modellgewichte | `ModelManifest.model_hash` als Identität | BiRefNet aktuell `pending-integration` |

Policy: `Cargo.lock` committet halten; native Abhängigkeiten über das immutable
CI-Image pinnen; Modellgewichte bei Integration über Hash + Version + Lizenz +
Quell-URL erfassen; Fixture-Seeds eingefroren.

---

## 8. Offene Punkte & empfohlene Maßnahmen

| ID | Schwere | Punkt | Maßnahme |
| --- | --- | --- | --- |
| **R1** | 🔴 Blocker | `.cr3`-Fixtures ohne dokumentierte Lizenz/Provenance (§4) | Provenance + Lizenz dokumentieren **oder** durch generierte/CC0-Fixtures ersetzen, bevor distribuiert wird |
| **R2** | 🟠 Hoch | Alle 8 Workspace-Crates ohne `license`-Feld; Repo-Root ohne `LICENSE`/`NOTICE` — Projekt bewusst unlizenziert / kommerziell bis MVP | Lizenz bei MVP entscheiden (siehe `Agents.todo.md` LIZ-ENTSCHEIDUNG); dann `license` + Root-`LICENSE` konsistent ergänzen |
| **R3** | 🟠 Hoch | LibRaw-Dynamik-Link-Verpflichtung (§6.3) | Dynamisches Linken beibehalten; LibRaw-Lizenz + Quellangebot für 0.22.2 im Release bündeln |
| **R4** | 🟡 Mittel | `onnx-rt`-Pfad lädt ORT-Prebuilt-Binaries (Netz) | Bei Release-Freigabe ORT-Redistribution + Prebuilt-Terms prüfen, Pin `=2.0.0-rc.13` halten, Modell-Lizenzen/Hashes erfassen |
| **R5** | 🟡 Mittel | ADR 0002 nennt LibRaw „dual“; upstream ist **dreifach** (permissiv fehlt) | ADR 0002 um die dritte, permissive Option ergänzen |
| **R6** | 🟡 Mittel | SAM-2-Lizenz ungeprüft; BiRefNet Apache-2.0 aus Manifest (nicht aus Gewichtsquelle) | Bei Integration (F-048/F-080) gegen tatsächliche Gewichtsquelle verifizieren und in §5 erfassen |
| **R7** | 🟢 Niedrig | `r-efi` trägt `LGPL-2.1-or-later`-Option | Keine Aktion: UEFI-only, nie ausgeliefert; bei UEFI-Build unter MIT/Apache erfüllen |

Es existiert **keine** GPL/AGPL/SSPL/starke-Copyleft-Abhängigkeit im gesamten
Baum — die einzige copyleft-lizenzierte Crate ist `r-efi` und für ausgelieferte
Targets nicht erreichbar.

---

## 9. Abnahmekriterien (F-073 / F-078)

- [x] Fixture-Inventar dokumentiert (synthetisch + RAW + Generierungsvertrag).
- [x] Modell-Inventar mit Lizenzen dokumentiert (BiRefNet Apache-2.0, ORT MIT).
- [x] Vollständige Abhängigkeits-Lizenztabelle erstellt (`THIRD-PARTY-NOTICES.md`).
- [x] Keine GPL/AGPL/SSPL gefunden; LibRaw als einzige Pflicht benannt.
- [x] Versionierungs-Policy dokumentiert.
- [ ] **R1** (RAW-Fixture-Lizenz) vor Release geschlossen.
- [ ] **R2** (Workspace-Crate-Lizenzen + Root-LICENSE) umgesetzt.
- [ ] **R3** (LibRaw-Notice im Release-Bundle) umgesetzt.

---

## 10. Verweise & Verifikation

- `docs/fixtures-and-licensing.md` — ausführliche Audit-Doku (Englisch).
- `THIRD-PARTY-NOTICES.md` — vollständige Crate-Lizenz-Tabelle.
- `docs/adr/0002-raw-backend.md` — RAW-Backend-Entscheidung (LibRaw).
- `README.md` — LibRaw-Hinweis.
- Reproduktion: `cargo metadata --all-features` (siehe `docs/fixtures-and-licensing.md` §7).
