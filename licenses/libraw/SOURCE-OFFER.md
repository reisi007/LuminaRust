# LibRaw — Written Source Offer & License Inventory

**Component:** LibRaw (native C/C++ RAW decoder library, `libraw_r`)
**Version used / pinned:** **0.22.2** (CI pin: OCI image label `lumina.libraw_version`;
see `feature/quality/fixtures-licensing.md` §7)
**How linked:** *dynamically* via `vendor/libraw-sys` → `pkg-config` → system
`libraw_r` (`cargo:rustc-link-lib`). LuminaRust does **not** vendor or statically
embed LibRaw.

## License

LibRaw is distributed under a choice of **two** licenses (distributor's choice):

1. **GNU LGPL-2.1-or-later** — text in [`LICENSE.LGPL`](LICENSE.LGPL)
2. **CDDL-1.0** — text in [`LICENSE.CDDL`](LICENSE.CDDL)

Upstream notice file: [`COPYRIGHT`](COPYRIGHT) (verbatim from the 0.22.2 source
archive; also carries the embedded-code notices for dcraw © Dave Coffin,
DCB/FBDD demosaic © Jacek Gozdz, X3F © Roland Karlsson, Adobe DNG SDK © Adobe).

> Note (re-verified 2026-08-25 against `libraw.org/about`, the LibRaw 0.18
> release notes and the `COPYRIGHT` file at tag 0.22.2): the former third option
> ("LibRaw Software License 27032010") was **removed upstream with LibRaw 0.18**
> ("all signed agreements have expired"). Older project documents that describe
> LibRaw as tri-licensed are outdated on this point. The conservative compliance
> path chosen by LuminaRust satisfies the LGPL obligations regardless.

## Source offer (LGPL-2.1 §4/§6(b)/(c), CDDL-1.0 §3.1)

The complete corresponding source for the exact version linked above is
available, at no charge, from the upstream sources:

- Upstream tarball: <https://www.libraw.org/data/LibRaw-0.22.2.tar.gz>
- GitHub tag mirror: <https://github.com/LibRaw/LibRaw/archive/refs/tags/v0.22.2.tar.gz>
- Browsable: <https://github.com/LibRaw/LibRaw/tree/v0.22.2>

This written offer is valid for at least three years from the first
distribution of any binary build that dynamically links LibRaw 0.22.2.
If you distribute such a build and prefer physical delivery of the source,
contact the LuminaRust project owner.

## Distribution obligations checklist

- [x] Ship this directory (`licenses/libraw/`) with every distributed build
      that links `libraw_r`.
- [x] Keep the link **dynamic** — static linking would extend LGPL-2.1 to the
      whole work.
- [x] Reference the pinned version (0.22.2) so the source offer stays exact;
      bump this file together with any CI pin change.
