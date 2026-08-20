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
- native C-Bibliothek LibRaw (über `vendor/libraw-sys`);
- native C-Bibliothek Lensfun (über `crates/lumina-lensfun`, F-098-N1,
  feature-gated `native`).

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

## 4. Rohdaten-Provenance & Lizenz — ✅ GELÖST (2026-08-20)

**Befund (Stand 2026-08-20, Ermittlung siehe `sample-data/raw/README.md`):**
Die beiden committeten `.cr3`-Fixtures in `sample-data/raw/` haben **keine
explizite Lizenzgewährung** — weder im einführenden Commit `1e388bf` noch als
separates `LICENSE`/`README`. Der Eigentümer ist jedoch **aus den Binär-Metadaten
selbst ableitbar**: Beide Dateien tragen in EXIF `Artist`/`Copyright`
`reisinger.pictures/Florian Reisinger` sowie `Owner Name: Florian Reisinger`; der
einführende Commit `1e388bf` („Add tone controls and RAW sample fixtures“,
2026-08-17, Author Florian Reisinger) bestätigt denselben Urheber. Damit ist der
**Autor/Provenance teilweise belegt** — es fehlt weiterhin eine **explizite
Lizenz** (das EXIF-`Copyright`-Feld ist keine Lizenzgewährung; es existiert kein
IPTC/XMP-`License`/`Rights`-Feld).

Ermittelte Metadaten (via `exiftool`):

| Datei | Kamera | Objektiv | Aufnahme (EXIF) | Orientierung | Maße |
| --- | --- | --- | --- | --- | --- |
| `aircraft-landscape.cr3` | Canon EOS R1 | RF200-800mm F6.3-9 IS USM | 2026:08:14 20:16:49, 1/1000 s, ISO 1000, 800 mm | 1 (Horizontal) | 6032×4024 |
| `aircraft-portrait.cr3` | Canon EOS R1 | RF200-800mm F6.3-9 IS USM | 2026:08:14 20:17:32, 1/1000 s, ISO 1250, 800 mm | 5 (Rotate 270 CW) | 4024×6032 |

Das ist ein **Release-Blocker** (F-078): urheberrechtlich geschützte Kamera-RAWs
ohne Lizenzgewährung zu distribuieren ist ein rechtliches Risiko — unabhängig
von der (MIT) Rust-Code-Lizenz. Die `*_fixture_*`-Tests hängen hart an genau
diesen Bytes.

**Status (2026-08-20):** **GELÖST** — Autor/Provenance belegt (EXIF `Artist`/
`Copyright`/`Owner Name` = reisinger.pictures/Florian Reisinger + Commit
`1e388bf`) und **explizite Lizenzgewährung dokumentiert**: Der Projekteigentümer
hat am 2026-08-20 eine **uneingeschränkte Nutzungs- und Distributionsgewährung
für das LuminaRust-Projekt** erteilt (eingetragen im Provenienz-Block in
`sample-data/raw/README.md`). R1 gilt damit als geschlossen (Verifikation
durch unabhängigen Agenten steht im Rahmen der F-078-Abnahme aus).

Ausgetragene Alternativen (nur noch relevant, falls die Gewährung
zurückgezogen wird):
1. Austausch gegen generierte/CC0-lizenzierte Fixtures (siehe unten, „Was ein
   Austausch bedeuten würde").

**Was ein Austausch bedeuten würde:** Betroffen sind die `lumina-raw`-Tests
`aircraft_landscape_fixture_*` / `aircraft_portrait_fixture_*` (sie referenzieren
die Dateien per `include_bytes!` fest verdrahtet) sowie die Decode-Benchmarks in
`crates/lumina-bench/bench/decode.rs` (lesen das Verzeichnis via
`LUMINA_RAW_FIXTURE`). Die Decode-Pipeline benötigt funktional nur RAW-Bytes;
LuminaRust kann mit **einem** RAW-Fixture arbeiten. Das generierte Fixture müsste
jedoch **beide Rollen** erfüllen (landscape-Orientierung 1 **und**
portrait-Orientierung 5), d.h. entweder zwei generierte Dateien oder eine
Test-Refaktorierung. Die `#[ignore]`-Test
`optional_real_fixture_checks_decode_orientation_and_dimensions` bleibt
unabhängig (eigene, separat lizenzierte RAW via Env-Var). Die Fixture-DATEIEN
selbst wurden in diesem Schritt **nicht** verändert/entfernt — die Entscheidung
liegt beim Build-Agenten/Eigentümer.

---

## 5. Modell-Inventar & Lizenzen

| Modell | Rolle | Lizenz | Status |
| --- | --- | --- | --- |
| **BiRefNet** (Zheng et al., arXiv:2401.03407) | erstes automatisches Subjekt-Modell | **MIT** (GitHub `LICENSE` = MIT, Copyright (c) 2024 ZhengPeng; HF-Card `ZhengPeng7/BiRefNet` `license: mit` — verifiziert 2026-08-20, R6; Manifest korrigiert) | Gewichte *pending integration* (`model_hash = "pending-integration"`) |
| **SAM 2.1** (`sam2.1_hiera_*`) | erstes interaktives Box/Pinsel-Modell (F-082) | **Apache-2.0** für Code **und** Gewichte (facebookresearch/sam2 `LICENSE`, HF-Model-Cards, Meta-Announcement „code and weights … permissive Apache 2.0" — verifiziert 2026-08-20, R6) | Adapter integriert (Commit `452d8a4`); Gewichte *pending integration* (`model_hash = "pending-integration"`) |
| **ONNX Runtime** (`ort` 2.0.0-rc.13) | Inferenz-Runtime | **MIT** (ORT `MIT OR Apache-2.0`, `ort-sys` `MIT OR Apache-2.0`) | **optional**, Feature `onnx-rt`, nicht im Default-Build |

**SAM-2-Export-Pfad (AGPL-Falle):** Die SAM-2.1-Gewichte sind Apache-2.0,
aber der übliche Lade-/Inferenzweg über das PyPI-Paket **`ultralytics`
ist AGPL-3.0** und würde als Abhängigkeit das Gesamtwerk betreffen.
LuminaRust nutzt diesen Weg **nicht**: Der ONNX-Export erfolgt aus den
Meta-Checkpoints (092824) über das Microsoft-ORT-Export-Tooling
(`convert_to_onnx.py`, MIT) bzw. veröffentlichte Community-ONNX-Artefakte
(Redistribution unter Apache-2.0); die Inferenz selbst läuft über
`lumina-onnx`/`ort` (MIT), ohne `ultralytics`. Bei der Gewichts-Pinning-
Folgearbeit (F-082-Nachlauf) ist ausschließlich der Apache-2.0-konforme
Exportweg zulässig.

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
- **Einzig reale Pflicht (Default-Build):** LibRaw (siehe §6.3). Zusätzlich
  **feature-gated** (nur bei aktiviertem `native`-Feature): Lensfun (siehe §6.5).

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
| **LibRaw** (LGPL/CDDL/LibRaw-SW) | ⚠️ schwach | **einzige Pflicht** (Default) | dynamisch linken + Notice/Quellangebot |
| **Lensfun** (LGPL-3.0, DB CC-BY-SA) | ⚠️ schwach | nur bei `native`-Feature | dynamisch linken (Feature `native`, Default aus) + Notice/Quellangebot + DB-Attribution; s. §6.5 |

### 6.5 Lensfun (F-098-N1 / F-098-N4) — native, feature-gated

`lumina-lensfun` (F-098-N1) ist ein dünner, sicherer Rust-Wrapper um die
System-`liblensfun` für **automatische Objektivkorrektur** (Verzeichnung +
Vignettierung), wenn in der installierten Datenbank ein passendes
Kamera/Objektiv-Profil gefunden wird. Die Integration ist **Pre-MVP** (verifiziert
2026-08-20, F-098-N1), die Distributions-Doku ist Teil von **F-098-N4** (S8) und
zählt zur F-078-Abnahme.

| Punkt | Befund |
| --- | --- |
| Rolle | Automatische Objektivkorrektur (Distortion + Vignetting); CA bleibt manuell (F-098-N1-MVP-Grenze) |
| Integration | Pre-MVP (F-098-N1), verifiziert 2026-08-20 |
| Feature-Gating | `native`-Feature im Crate `lumina-lensfun` — **Standard AUS**; Default-, WASM- und CI-Builds linken nichts und bleiben grün |
| Linkart | **dynamisch** über `pkg-config` (`build.rs` → `cargo:rustc-link-lib=dylib=lensfun`), nur wenn `native` an |
| Version (bewiesen) | **0.3.4** — `brew info lensfun` **und** `LF_VERSION_*` in `/opt/homebrew/include/lensfun/lensfun.h` (`LF_VERSION_MAJOR 0` / `_MINOR 3` / `_MICRO 4`) |
| Bibliotheks-Lizenz | **LGPL-3.0-or-later** laut Projekt-FFI und Header-Text („version 2 … or (at your option) any later version“); Homebrew-Formel deklariert `LGPL-3.0-only AND GPL-3.0-only AND CC-BY-3.0 AND LicenseRef-Homebrew-public-domain` → **zu verifizieren** (exakte SPDX gegen upstream `COPYING`/`README`) |
| Datenbank-Lizenz | Profil-DB (`/opt/homebrew/share/lensfun/version_1/*.xml`): **CC-BY-SA-3.0** laut Projektdoku/F-098-N4; Homebrew-Formel nennt nur `CC-BY-3.0` → **zu verifizieren** (SA vs. kein-SA gegen `lensfun-data`) |
| Neue gebündelte Binaries | **Keine** — LuminaRust liest zur Laufzeit die **System**-Datenbank (kein vendored DB) |
| Verpflichtung | Dynamisches Linken beibehalten (kein statisches Einbetten → würde LGPL aufs Gesamtwerk ausweiten); Lensfun-Lizenztext + Quellangebot für **0.3.4** + DB-Attribution im Release-Bundle |
| Querverweis | `THIRD-PARTY-NOTICES.md` (Attribution obligations, Nr. 5) |

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
| **R1** | ✅ Gelöst (2026-08-20) | `.cr3`-Fixtures (§4): Autor aus EXIF + Commit belegt (Florian Reisinger / reisinger.pictures); **uneingeschränkte Nutzungs-/Distributionsgewährung für LuminaRust** am 2026-08-20 durch den Eigentümer erteilt und im Provenienz-Block (`sample-data/raw/README.md`) dokumentiert | Keine Aktion mehr; Verifikation der Doku im Rahmen der F-078-Abnahme |
| **R2** | 🟠 Hoch | Alle 9 Workspace-Crates ohne `license`-Feld; Repo-Root ohne `LICENSE`/`NOTICE` — Projekt bewusst unlizenziert / kommerziell bis MVP | Lizenz bei MVP entscheiden (siehe `Agents.todo.md`, Antworten des Eigentümers 2026-08-20 → LIZ interim proprietär); dann `license` + Root-`LICENSE` konsistent ergänzen |
| **R3** | 🟠 Hoch | LibRaw-Dynamik-Link-Verpflichtung (§6.3) | Dynamisches Linken beibehalten; LibRaw-Lizenz + Quellangebot für 0.22.2 im Release bündeln |
| **R4** | 🟡 Mittel | `onnx-rt`-Pfad lädt ORT-Prebuilt-Binaries (Netz) | Bei Release-Freigabe ORT-Redistribution + Prebuilt-Terms prüfen, Pin `=2.0.0-rc.13` halten, Modell-Lizenzen/Hashes erfassen |
| **R5** | ✅ Gelöst (2026-08-20) | ADR 0002 nannte LibRaw „dual"; upstream ist **dreifach** (permissiv fehlte) | ADR 0002 um die dritte, permissive Option (LibRaw Software License) ergänzt |
| **R6** | ✅ Gelöst (2026-08-20) | SAM-2-Lizenz ungeprüft; BiRefNet „Apache-2.0" aus Manifest, nicht aus der Gewichtsquelle | Beide an der tatsächlichen Quelle verifiziert und in §5 erfasst: **SAM 2.1 = Apache-2.0** (Code + Gewichte, facebookresearch/sam2 `LICENSE` + Meta-Announcement), **BiRefNet = MIT** (GitHub `LICENSE` + HF-Card `license: mit`) — Manifest-`license`-Feld und Doku korrigiert (Commit folgt); AGPL-Falle via `ultralytics` in §5 dokumentiert |
| **R7** | 🟢 Niedrig | `r-efi` trägt `LGPL-2.1-or-later`-Option | Keine Aktion: UEFI-only, nie ausgeliefert; bei UEFI-Build unter MIT/Apache erfüllen |

Es existiert **keine** GPL/AGPL/SSPL/starke-Copyleft-Abhängigkeit im gesamten
Baum — die einzige copyleft-lizenzierte Crate ist `r-efi` und für ausgelieferte
Targets nicht erreichbar.

---

## 9. Abnahmekriterien (F-073 / F-078)

- [x] Fixture-Inventar dokumentiert (synthetisch + RAW + Generierungsvertrag).
- [x] Modell-Inventar mit Lizenzen dokumentiert (BiRefNet MIT, SAM 2.1
  Apache-2.0, ORT MIT — jeweils an der tatsächlichen Quelle verifiziert, R6).
- [x] Vollständige Abhängigkeits-Lizenztabelle erstellt (`THIRD-PARTY-NOTICES.md`).
- [x] Keine GPL/AGPL/SSPL gefunden; LibRaw als einzige Pflicht benannt.
- [x] Versionierungs-Policy dokumentiert.
- [x] **R1** (RAW-Fixture-Lizenz) geschlossen (2026-08-20, Eigentümer-Gewährung in `sample-data/raw/README.md` dokumentiert).
- [ ] **R2** (Workspace-Crate-Lizenzen + Root-LICENSE) umgesetzt.
- [ ] **R3** (LibRaw-Notice im Release-Bundle) umgesetzt.

---

## 10. Verweise & Verifikation

- `docs/fixtures-and-licensing.md` — ausführliche Audit-Doku (Englisch).
- `THIRD-PARTY-NOTICES.md` — vollständige Crate-Lizenz-Tabelle.
- `docs/adr/0002-raw-backend.md` — RAW-Backend-Entscheidung (LibRaw).
- `README.md` — LibRaw-Hinweis.
- Reproduktion: `cargo metadata --all-features` (siehe `docs/fixtures-and-licensing.md` §7).
