# Lensfun — Written Source Offer & License Inventory

**Component:** Lensfun (native C++ lens-correction library + lens profile database)
**Version used:** **0.3.4** (verified via `LF_VERSION_*` macros in the installed
`lensfun.h`; see `feature/quality/fixtures-licensing.md` §6.5)
**How linked:** *dynamically* via `crates/lumina-lensfun` → `pkg-config`
(`cargo:rustc-link-lib=dylib=lensfun`), **only when the `native` feature is
enabled** (default off). Default, WASM and CI builds link nothing.
**Database:** LuminaRust reads the **system** lens database at runtime
(e.g. `/opt/homebrew/share/lensfun/version_1/*.xml`); no database files are
vendored or bundled by LuminaRust itself.

## Licenses

| Part | License | Text |
| --- | --- | --- |
| Libraries (`libs/`) | **LGPL-3.0** (upstream README: "version 3"; older header boilerplate reads "version 2 … or later", hence conservative handling as LGPL-3.0-or-later) | [`COPYING.LGPL-3.0`](COPYING.LGPL-3.0) |
| Lens profile database (`data/`) | **CC-BY-SA-3.0** (upstream README + `data/COPYING.CC_BY-SA_3.0`; the Homebrew formula's "CC-BY-3.0" is imprecise) | [`COPYING.CC-BY-SA-3.0`](COPYING.CC-BY-SA-3.0) |

Applications and build system are GPL-3.0 — they are **not** part of what
LuminaRust links or ships.

Verified against the upstream v0.3.4 `README.md` LICENSE section
(2026-08-25).

## Source offer (LGPL-3.0 §4(d)/§6)

The complete corresponding source for the exact version referenced above:

- GitHub tag archive: <https://github.com/lensfun/lensfun/archive/refs/tags/v0.3.4.tar.gz>
- SourceForge download: <https://sourceforge.net/projects/lensfun/files/lensfun/0.3.4/>
- Browsable: <https://github.com/lensfun/lensfun/tree/v0.3.4>

This written offer is valid for at least three years from the first
distribution of any binary build that enables the `native` feature and links
Lensfun 0.3.4.

## Database attribution (CC-BY-SA-3.0)

Any release bundle that enables the `native` feature must carry the database
attribution: *"lens correction data from the Lensfun project
(https://github.com/lensfun/lensfun), licensed CC-BY-SA-3.0"* plus a copy of
[`COPYING.CC_BY-SA_3.0`](COPYING.CC-BY-SA-3.0). If the bundle ships a modified
database, ShareAlike applies to that adaptation.

## Distribution obligations checklist

- [x] Ship this directory (`licenses/lensfun/`) with every distributed build
      built with the `native` feature.
- [x] Keep the link **dynamic** — static embedding would extend LGPL to the
      whole work.
- [x] Include the database attribution (CC-BY-SA-3.0) in the visible notices.
