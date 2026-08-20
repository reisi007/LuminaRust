# RAW-Fixtures — Provenienz & Lizenz

Dieses Verzeichnis enthält die committeten RAW-Referenz-Fixtures für
`lumina-raw`-Tests und Decode-Benchmarks. **Provenienz und Lizenz sind
dokumentiert (R1 GELÖST, 2026-08-20):** Autor/Urheber via EXIF + Commit
belegt und eine uneingeschränkte Nutzungs-/Distributionsgewährung für das
LuminaRust-Projekt durch den Eigentümer erteilt (siehe Provenienz-Block
unten). Details siehe
[`feature/quality/fixtures-licensing.md`](../../feature/quality/fixtures-licensing.md)
§4 und [`THIRD-PARTY-NOTICES.md`](../../THIRD-PARTY-NOTICES.md).

> Die Fixture-**DATEIEN** wurden in diesem Dokumentationsschritt **nicht**
> verändert. Die finale Klärung ist erfolgt: Eigentümer-Auskunft mit
> uneingeschränkter Nutzungs-/Distributionsgewährung für LuminaRust
> (2026-08-20, siehe Provenienz-Block unten).

## Inventar & Status

| Datei | Zweck (Tests / Benches) | Ermittelte Metadaten (EXIF, via `exiftool`) | Provenienz dokumentiert |
| --- | --- | --- | --- |
| `aircraft-landscape.cr3` | `lumina-raw`-Test `aircraft_landscape_fixture_*` (Geometrie/Metadaten); Decode-Bench (Env `LUMINA_RAW_FIXTURE`) | Canon EOS R1 · RF200-800mm F6.3-9 IS USM · 2026:08:14 20:16:49 · 1/1000 s · ISO 1000 · 800 mm · Orientierung 1 (Horizontal) · 6032×4024 · EXIF `Artist`/`Copyright` = `reisinger.pictures/Florian Reisinger`, `Owner Name` = `Florian Reisinger` | **JA** — Autor belegt + Lizenzgewährung dokumentiert (2026-08-20) |
| `aircraft-portrait.cr3` | `lumina-raw`-Test `aircraft_portrait_fixture_*` (EXIF-Orientierung); Decode-Bench (Env `LUMINA_RAW_FIXTURE`) | Canon EOS R1 · RF200-800mm F6.3-9 IS USM · 2026:08:14 20:17:32 · 1/1000 s · ISO 1250 · 800 mm · Orientierung 5 (Rotate 270 CW) · 4024×6032 · EXIF `Artist`/`Copyright` = `reisinger.pictures/Florian Reisinger`, `Owner Name` = `Florian Reisinger` | **JA** — Autor belegt + Lizenzgewährung dokumentiert (2026-08-20) |

Eingeführt in Commit `1e388bf` („Add tone controls and RAW sample fixtures“,
2026-08-17, Author Florian Reisinger). Dateigrößen: landscape 11.607.210 Bytes,
portrait 12.339.882 Bytes.

## Provenienz-Block (ausgefüllt, 2026-08-20)

> Ausgefüllt durch den Projekteigentümer am 2026-08-20 → **R1** (F-078) ist
> damit geschlossen. Alternative (nur noch relevant, falls die Gewährung
> zurückgezogen wird): Austausch gegen generierte/CC0-lizenzierte Fixtures
> (siehe [`feature/quality/fixtures-licensing.md`](../../feature/quality/fixtures-licensing.md)
> §4, Rest-Vorgang).

### `aircraft-landscape.cr3`

- **Author / Urheber:** `reisinger.pictures / Florian Reisinger` *(aus EXIF + Commit belegt)*
- **Quelle:** Eigene Aufnahme des Projekteigentümers (Florian Reisinger)
- **Aufnahmedatum:** 2026:08:14 *(aus EXIF belegt)*
- **Lizenz / Nutzungsgewährung:** **Uneingeschränkte Nutzungs- und
  Distributionsgewährung für das LuminaRust-Projekt** (Entscheidung des
  Eigentümers, 2026-08-20; gilt für Test-, Benchmark- und Referenzzwecke
  im Rahmen von LuminaRust)
- **Bestätigung:** Florian Reisinger, 2026-08-20

### `aircraft-portrait.cr3`

- **Author / Urheber:** `reisinger.pictures / Florian Reisinger` *(aus EXIF + Commit belegt)*
- **Quelle:** Eigene Aufnahme des Projekteigentümers (Florian Reisinger)
- **Aufnahmedatum:** 2026:08:14 *(aus EXIF belegt)*
- **Lizenz / Nutzungsgewährung:** **Uneingeschränkte Nutzungs- und
  Distributionsgewährung für das LuminaRust-Projekt** (Entscheidung des
  Eigentümers, 2026-08-20; gilt für Test-, Benchmark- und Referenzzwecke
  im Rahmen von LuminaRust)
- **Bestätigung:** Florian Reisinger, 2026-08-20

**Status R1 (F-078):** GELÖST — Autor belegt (EXIF + Commit) und explizite
Lizenzgewährung dokumentiert (oben).

## Verweise

- [`feature/quality/fixtures-licensing.md`](../../feature/quality/fixtures-licensing.md) §4 (R1, Detail + Rest-Vorgang)
- [`THIRD-PARTY-NOTICES.md`](../../THIRD-PARTY-NOTICES.md) (keine native Lensfun/RAW-Lizenz-Pflicht für den Default-Build)
- Einführender Commit: `1e388bf`
