# Third-Party Notices & License Inventory

**Project:** LuminaRust
**Generated:** 2026-08-19
**Updated:** 2026-08-25 — LibRaw licensing corrected to dual (LGPL/CDDL;
"LibRaw Software License" was removed upstream with v0.18) and the bundled
license-text inventory (`licenses/`, release script
`scripts/release/bundle-licenses.sh`) added (F-078-R3/R4).
**Scope of this document:** Complete, machine-checkable inventory of every
third-party crate resolved by the workspace, plus the distribution terms of the
native libraries and ML models that LuminaRust links against or distributes.

This document is the authoritative companion to
[`docs/fixtures-and-licensing.md`](docs/fixtures-and-licensing.md) and the
feature spec [`feature/quality/fixtures-licensing.md`](feature/quality/fixtures-licensing.md)
(F-073 / F-078). It was produced from `cargo metadata --all-features` and cross-checked
against `Cargo.lock` and each source `Cargo.toml`.

## How to regenerate

```sh
cargo metadata --all-features --format-version=1 \
  | jq -r '.packages[] | "\(.name)\t\(.version)\t\(.license // "NO-LICENSE-FIELD")"' \
  | sort -u
```

All licenses below are SPDX expressions taken verbatim from each crate's
`Cargo.toml` `license` field. Where a crate uses an `OR` expression, the
downstream project may comply by satisfying **any one** of the listed licenses.

## License compatibility summary

| Result | Finding |
| --- | --- |
| OSI-approved permissive (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, 0BSD, CC0-1.0, Unlicense, BSL-1.0) | ✅ Vast majority of the tree |
| OSI-approved with weak/copyleft exceptions (Apache-2.0 WITH LLVM-exception, Unicode-3.0, OFL-1.1, Ubuntu-font-1.0) | ✅ Acceptable; attribution required (see obligations) |
| Weak copyleft (`LGPL-2.1-or-later`) | ⚠️ Only `r-efi`, and **only for `uefi` targets** — never compiled for shipped desktop/WASM targets (see below) |
| Strong copyleft (GPL / AGPL / SSPL / MPL / EPL) | ❌ **None found** in the entire dependency graph (default **and** all-features) |
| Native C library with copyleft (LibRaw) | ⚠️ Linked **dynamically** via `vendor/libraw-sys` → system `libraw_r`. See obligations. |
| Native C library with weak copyleft (Lensfun) | ⚠️ Linked **dynamically** via `lumina-lensfun` → `pkg-config`, **only** when the `native` feature is enabled (default **off** — not compiled into default/WASM/CI builds). Dynamic linking means LGPL obligations do **not** extend to the whole work. See obligations. |

**Conclusion:** The Rust crate tree contains **no GPL/AGPL/SSPL/MPL/EPL**
dependency. The only copyleft-licensed crate (`r-efi`, `MIT OR Apache-2.0 OR
LGPL-2.1-or-later`) is reachable exclusively under the `uefi` target (pulled in
transitively by `getrandom`'s UEFI backend) and is therefore **not part of any
shipped macOS / Linux / Windows / WASM build**. Even where it is compiled, its
`OR` clause lets us comply under plain MIT or Apache-2.0. The single real
distribution obligation for **default** builds is **LibRaw**, addressed under
"Attribution obligations". **Lensfun** is a second, **feature-gated**
(`native`, default off) obligation that applies only to builds that enable that
feature and therefore link `liblensfun`; because it is linked **dynamically**,
its LGPL obligations do not extend to the whole work. See obligations.

## Attribution obligations

The following components impose attribution / notice / source-availability
obligations that MUST be honored in any distributed build:

1. **LibRaw (native C library, linked via `vendor/libraw-sys`)**
   - License: dual **LGPL-2.1-or-later / CDDL-1.0** (distributor's choice).
     *Correction 2026-08-25:* earlier revisions of this document described a
     third option ("LibRaw Software License 27032010"); that option was
     **removed upstream with LibRaw 0.18** ("all signed agreements have
     expired", libraw.org/news/libraw-0-18-released) and is not part of
     LibRaw 0.22.2 — the upstream `COPYRIGHT` at tag 0.22.2 names exactly two
     licenses. There is no "Common Clause" anywhere in LibRaw's licensing.
   - License texts shipped verbatim in `licenses/libraw/`
     (`LICENSE.LGPL`, `LICENSE.CDDL`, upstream `COPYRIGHT` including the
     embedded dcraw / DCB-FBDD / X3F / Adobe DNG SDK notices); written source
     offer for the pinned version: `licenses/libraw/SOURCE-OFFER.md`.
   - `libraw-sys` (the Rust FFI bindings, `vendor/libraw-sys`) is **MIT**
     (© David Cuddeback).
   - `lumina-raw` links to the **system** `libraw_r` shared library via
     `pkg-config` (`build.rs` emits `cargo:rustc-link-lib`). It does **not**
     vendor or statically embed LibRaw.
   - **Obligation:** Keep the dynamic-link arrangement (do not statically link
     LibRaw into the binary, which would extend LGPL to the whole work); ship
     LibRaw's license text and a written offer / pointer to LibRaw source for the
     exact version used (CI pins LibRaw **0.22.2**, recorded in the
     `lumina.libraw_version` OCI label of the `lumina-ci` image); keep the
     pinned version in sync with `licenses/libraw/SOURCE-OFFER.md`.
2. **Bundled fonts in `epaint_default_fonts` (egui)** — `((MIT OR Apache-2.0)
   AND OFL-1.1 AND Ubuntu-font-1.0)`. The SIL Open Font License (OFL-1.1) and
   Ubuntu Font License 1.0 require font attribution and prohibit selling the
   fonts unchanged on their own. Honor by including this notice; do not modify
   or redistribute the fonts standalone.
3. **Unicode Data / ICU crates (`icu_*`, `tinystr`, `litemap`, `yoke`,
   `zerovec`, `writeable`, `potential_utf`, `zerofrom`, `zerotrie`)** —
   `Unicode-3.0` (Unicode License v3), OSI-approved; include the Unicode
   license/notice.
4. **Every MIT / BSD / Apache-2.0 crate** — include the copyright line and
   license text (full list below). Apache-2.0 additionally requires carrying
   `NOTICE` files if present.
5. **Lensfun (native C library, linked via `lumina-lensfun` → `pkg-config`)** —
   automatic lens correction (F-098-N1), feature-gated behind the `native`
   feature (default **off**).
   - **Library license (verified 2026-08-20 against upstream):** the Lensfun
     README at v0.3.4 (the installed version) states the libraries (`libs/`) are
     licensed **LGPL-3.0** (scope: exactly version 3), the applications (`apps/`)
     and the build system are **GPL-3.0** (not shipped by LuminaRust). The
     installed `lensfun.h` header additionally carries the older LGPL-2.1
     boilerplate sentence ("version 2 of the License, or (at your option) any
     later version"); Fedora's SPDX expression for the package is consequently
     `LGPL-3.0-only AND CC-BY-SA-3.0 AND LGPL-2.1-or-later AND GPL-3.0-only`.
     For LuminaRust the relevant license of the linked library is
     **LGPL-3.0-or-later (Sammelwerk-Hinweis, dynamic link)**, conservative
     reading of both statements.
   - `lumina-lensfun` (the Rust wrapper, `crates/lumina-lensfun`) is a workspace
     crate and currently declares **NO `license` field** (see R2 in
     `feature/quality/fixtures-licensing.md`). Lensfun itself is **not** in the
     crate table above — it is a native dependency resolved through `pkg-config`,
     not a crate.
   - `build.rs` emits `cargo:rustc-link-lib=dylib=lensfun` **only** when the
     `native` feature is enabled; default, WASM and CI builds link nothing and
     stay green. (Version **0.3.4** confirmed via `brew info lensfun` and the
     `LF_VERSION_*` macros in `/opt/homebrew/include/lensfun/lensfun.h`.)
   - **Obligation:** Keep the **dynamic-link** arrangement (do not statically
     embed Lensfun into the binary, which would extend LGPL to the whole work);
     ship Lensfun's license text and a written offer / pointer to Lensfun source
     for the exact version used (**0.3.4**) in the release bundle — both are
     provided by `licenses/lensfun/` (`COPYING.LGPL-3.0`,
     `COPYING.CC-BY-SA-3.0`, `SOURCE-OFFER.md`).
   - **Lensfun database (camera/lens profiles,
     `/opt/homebrew/share/lensfun/version_1/*.xml`):** licensed **CC-BY-SA-3.0**
     (verified 2026-08-20 against upstream `data/COPYING.CC_BY-SA_3.0` in the
     lensfun source archive and the README: "The lens database is licensed under
     the Creative Commons Attribution-Share Alike 3.0 license"). The Homebrew
     formula's shorter `CC-BY-3.0` is imprecise; the authoritative upstream
     statement is CC-BY-SA-3.0. No new bundled binaries are introduced —
     LuminaRust reads the **system** database at runtime (no vendored DB).
     Database attribution must be included in the release bundle.
6. **ML models (adapter manifests in `lumina-onnx`; weights are **not**
   committed, `model_hash = "pending-integration"` until hash-pinned fixtures
   land)** —
   - **BiRefNet** (automatic subject segmentation): **MIT** (© 2024 ZhengPeng;
     `github.com/ZhengPeng7/BiRefNet` `LICENSE` and HF model card
     `ZhengPeng7/BiRefNet` `license: mit` — verified 2026-08-20). Earlier
     "Apache-2.0" annotations in the manifest/docs were corrected.
   - **SAM 2.1** (`sam2.1_hiera_*` promptable segmentation): **Apache-2.0 for
     code and weights** (facebookresearch/sam2 `LICENSE`, HF model cards, Meta
     announcement — verified 2026-08-20). Export path deliberately avoids the
     `ultralytics` PyPI package (AGPL-3.0); ONNX comes from the Meta checkpoints
     via the MIT-licensed Microsoft ORT export tooling or Apache-2.0
     redistributed community artifacts.
   - **ONNX Runtime** (`ort` 2.0.0-rc.13): **MIT OR Apache-2.0** (`ort`,
     `ort-sys`); optional `onnx-rt` feature, not in default builds. ORT
     prebuilt-binary redistribution terms to be re-checked before release
     (R4).
   - **Obligation:** bundle the respective license texts (MIT / Apache-2.0)
     with any distributed weights or binaries — provided verbatim in
     `licenses/models/` together with provenance pointers
     (`MODEL-SOURCE-OFFER.md`). SAM 2 ships no separate `NOTICE` file (checked
     2026-08-25), so nothing additional has to be forwarded under Apache-2.0
     §4(d).

## Release bundle: license texts & source offers (F-078-R3/R4)

All native-library and model license texts plus written source offers live in
[`licenses/`](licenses/README.md) and are copied into every distributable
bundle by [`scripts/release/bundle-licenses.sh`](scripts/release/bundle-licenses.sh)
(default destination `dist/licenses`; verifies every required file, writes a
SHA256 manifest, aborts loudly on anything missing). Inventory:

| Bundle path | Content |
| --- | --- |
| `THIRD-PARTY-NOTICES.md` | this document (crate table + obligations) |
| `licenses/libraw/` | upstream `COPYRIGHT` @ tag 0.22.2, LGPL-2.1 text, CDDL-1.0 text, written source offer for **LibRaw 0.22.2** (R3) |
| `licenses/lensfun/` | LGPL-3.0 text (= upstream `lgpl-3.0.txt`), CC-BY-SA-3.0 legalcode (lens database), source offer for **Lensfun 0.3.4** + DB attribution |
| `licenses/models/` | BiRefNet MIT, SAM 2.1 Apache-2.0, ONNX Runtime MIT; provenance/export-path notes (incl. R4 reminder) |
| `CHECKSUMS.sha256` | SHA256 manifest over the bundled license payload |

Short pre-release checklist (full version in `licenses/README.md`):

1. Run `scripts/release/bundle-licenses.sh <bundle-dir>` and include its output
   in every distributable artifact.
2. Builds linking LibRaw (native decode, default): keep the link dynamic;
   `licenses/libraw/SOURCE-OFFER.md` must name the pinned version — update it
   in the same commit as any CI pin change.
3. Builds with feature `native` (Lensfun): bundle must contain
   `licenses/lensfun/` and surface the CC-BY-SA-3.0 database attribution.
4. Bundles shipping model weights: `licenses/models/` present; manifest hashes
   match the shipped weights; export path avoided `ultralytics` (AGPL).
5. ORT redistribution terms re-checked at release time if the download channel
   changes (open item R4); ONNX Runtime itself is MIT (© Microsoft Corporation),
   redistribution permitted with the bundled notice.

## Complete crate license table (all-features resolve)

> Columns: Crate · Version · License (SPDX). `NO-LICENSE-FIELD` marks crates
> that do not declare a `license` in their manifest (see "Open items").

| Crate | Version | License (SPDX) |
| --- | --- | --- |
| `epaint_default_fonts` | 0.31.1 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 |
| `unicode-ident` | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `adler2` | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| `ab_glyph` | 0.2.32 | Apache-2.0 |
| `ab_glyph_rasterizer` | 0.1.10 | Apache-2.0 |
| `ciborium` | 0.2.2 | Apache-2.0 |
| `ciborium-io` | 0.2.2 | Apache-2.0 |
| `ciborium-ll` | 0.2.2 | Apache-2.0 |
| `codespan-reporting` | 0.11.1 | Apache-2.0 |
| `gethostname` | 1.1.0 | Apache-2.0 |
| `gl_generator` | 0.14.0 | Apache-2.0 |
| `glutin` | 0.32.3 | Apache-2.0 |
| `glutin_egl_sys` | 0.7.1 | Apache-2.0 |
| `glutin_glx_sys` | 0.6.1 | Apache-2.0 |
| `glutin_wgl_sys` | 0.6.1 | Apache-2.0 |
| `khronos_api` | 3.1.0 | Apache-2.0 |
| `lzma-rust2` | 0.15.8 | Apache-2.0 |
| `openssl` | 0.10.81 | Apache-2.0 |
| `owned_ttf_parser` | 0.25.1 | Apache-2.0 |
| `spirv` | 0.3.0+sdk-1.3.268.0 | Apache-2.0 |
| `winit` | 0.30.13 | Apache-2.0 |
| `fnv` | 1.0.7 | Apache-2.0 / MIT |
| `dpi` | 0.1.2 | Apache-2.0 AND MIT |
| `async-channel` | 2.5.0 | Apache-2.0 OR MIT |
| `async-executor` | 1.14.0 | Apache-2.0 OR MIT |
| `async-fs` | 2.2.0 | Apache-2.0 OR MIT |
| `async-io` | 2.6.0 | Apache-2.0 OR MIT |
| `async-lock` | 3.4.2 | Apache-2.0 OR MIT |
| `async-net` | 2.0.0 | Apache-2.0 OR MIT |
| `async-process` | 2.5.0 | Apache-2.0 OR MIT |
| `async-signal` | 0.2.14 | Apache-2.0 OR MIT |
| `async-task` | 4.7.1 | Apache-2.0 OR MIT |
| `atomic-waker` | 1.1.2 | Apache-2.0 OR MIT |
| `autocfg` | 1.5.1 | Apache-2.0 OR MIT |
| `base64ct` | 1.8.3 | Apache-2.0 OR MIT |
| `bit-set` | 0.8.0 | Apache-2.0 OR MIT |
| `bit-vec` | 0.8.0 | Apache-2.0 OR MIT |
| `blocking` | 1.6.2 | Apache-2.0 OR MIT |
| `concurrent-queue` | 2.5.0 | Apache-2.0 OR MIT |
| `criterion` | 0.5.1 | Apache-2.0 OR MIT |
| `der` | 0.8.1 | Apache-2.0 OR MIT |
| `equivalent` | 1.0.2 | Apache-2.0 OR MIT |
| `event-listener` | 5.4.2 | Apache-2.0 OR MIT |
| `event-listener-strategy` | 0.5.4 | Apache-2.0 OR MIT |
| `fastrand` | 2.5.0 | Apache-2.0 OR MIT |
| `futures-lite` | 2.6.1 | Apache-2.0 OR MIT |
| `idna_adapter` | 1.2.2 | Apache-2.0 OR MIT |
| `indexmap` | 2.14.0 | Apache-2.0 OR MIT |
| `nohash-hasher` | 0.2.0 | Apache-2.0 OR MIT |
| `parking` | 2.2.1 | Apache-2.0 OR MIT |
| `pem-rfc7468` | 1.0.0 | Apache-2.0 OR MIT |
| `pin-project` | 1.1.13 | Apache-2.0 OR MIT |
| `pin-project-internal` | 1.1.13 | Apache-2.0 OR MIT |
| `pin-project-lite` | 0.2.17 | Apache-2.0 OR MIT |
| `polling` | 3.11.0 | Apache-2.0 OR MIT |
| `portable-atomic` | 1.15.0 | Apache-2.0 OR MIT |
| `portable-atomic-util` | 0.2.7 | Apache-2.0 OR MIT |
| `rustc-hash` | 2.1.3 | Apache-2.0 OR MIT |
| `simd_cesu8` | 1.2.0 | Apache-2.0 OR MIT |
| `tinytemplate` | 1.2.1 | Apache-2.0 OR MIT |
| `utf8_iter` | 1.0.4 | Apache-2.0 OR MIT |
| `utf8parse` | 0.2.2 | Apache-2.0 OR MIT |
| `uuid` | 1.24.1 | Apache-2.0 OR MIT |
| `zeroize` | 1.9.0 | Apache-2.0 OR MIT |
| `linux-raw-sys` | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `linux-raw-sys` | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `rustix` | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `rustix` | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `wasip2` | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `wit-bindgen` | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `pollster` | 0.4.0 | Apache-2.0/MIT |
| `rustc-hash` | 1.1.0 | Apache-2.0/MIT |
| `arrayref` | 0.3.9 | BSD-2-Clause |
| `zerocopy` | 0.8.56 | BSD-2-Clause OR Apache-2.0 OR MIT |
| `zerocopy-derive` | 0.8.56 | BSD-2-Clause OR Apache-2.0 OR MIT |
| `moxcms` | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| `pxfm` | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| `num_enum` | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |
| `num_enum_derive` | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |
| `clipboard-win` | 5.4.1 | BSL-1.0 |
| `error-code` | 3.4.0 | BSL-1.0 |
| `hexf-parse` | 0.2.1 | CC0-1.0 |
| `blake3` | 1.8.6 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| `constant_time_eq` | 0.4.2 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| `webpki-root-certs` | 1.0.9 | CDLA-Permissive-2.0 |
| `hmac-sha256` | 1.1.14 | ISC |
| `libloading` | 0.8.9 | ISC |
| `android-properties` | 0.2.2 | MIT |
| `ashpd` | 0.11.1 | MIT |
| `block` | 0.1.6 | MIT |
| `block2` | 0.5.1 | MIT |
| `block2` | 0.6.2 | MIT |
| `bytes` | 1.12.1 | MIT |
| `calloop` | 0.13.0 | MIT |
| `calloop` | 0.14.4 | MIT |
| `calloop-wayland-source` | 0.3.0 | MIT |
| `calloop-wayland-source` | 0.4.1 | MIT |
| `cfg_aliases` | 0.2.2 | MIT |
| `combine` | 4.6.7 | MIT |
| `crunchy` | 0.2.4 | MIT |
| `dispatch` | 0.2.0 | MIT |
| `dlib` | 0.5.3 | MIT |
| `endi` | 1.1.1 | MIT |
| `fax` | 0.2.7 | MIT |
| `glutin-winit` | 0.5.0 | MIT |
| `is-terminal` | 0.4.17 | MIT |
| `libraw-sys` | 0.1.1 | MIT |
| `libredox` | 0.1.20 | MIT |
| `malloc_buf` | 0.0.6 | MIT |
| `memoffset` | 0.9.1 | MIT |
| `objc` | 0.2.7 | MIT |
| `objc-sys` | 0.3.5 | MIT |
| `objc2` | 0.5.2 | MIT |
| `objc2` | 0.6.4 | MIT |
| `objc2-app-kit` | 0.2.2 | MIT |
| `objc2-cloud-kit` | 0.2.2 | MIT |
| `objc2-contacts` | 0.2.2 | MIT |
| `objc2-core-data` | 0.2.2 | MIT |
| `objc2-core-image` | 0.2.2 | MIT |
| `objc2-core-location` | 0.2.2 | MIT |
| `objc2-encode` | 4.1.0 | MIT |
| `objc2-foundation` | 0.2.2 | MIT |
| `objc2-foundation` | 0.3.2 | MIT |
| `objc2-link-presentation` | 0.2.2 | MIT |
| `objc2-metal` | 0.2.2 | MIT |
| `objc2-quartz-core` | 0.2.2 | MIT |
| `objc2-symbols` | 0.2.2 | MIT |
| `objc2-ui-kit` | 0.2.2 | MIT |
| `objc2-uniform-type-identifiers` | 0.2.2 | MIT |
| `objc2-user-notifications` | 0.2.2 | MIT |
| `oorandom` | 11.1.5 | MIT |
| `openssl-sys` | 0.9.117 | MIT |
| `orbclient` | 0.3.55 | MIT |
| `ordered-float` | 4.6.0 | MIT |
| `plotters` | 0.3.7 | MIT |
| `plotters-backend` | 0.3.7 | MIT |
| `plotters-svg` | 0.3.7 | MIT |
| `quick-xml` | 0.41.0 | MIT |
| `redox_syscall` | 0.4.1 | MIT |
| `redox_syscall` | 0.5.18 | MIT |
| `redox_syscall` | 0.9.2 | MIT |
| `rfd` | 0.15.4 | MIT |
| `schannel` | 0.1.29 | MIT |
| `simd-adler32` | 0.3.10 | MIT |
| `slab` | 0.4.12 | MIT |
| `smithay-client-toolkit` | 0.19.2 | MIT |
| `smithay-client-toolkit` | 0.20.0 | MIT |
| `smithay-clipboard` | 0.7.3 | MIT |
| `strsim` | 0.11.1 | MIT |
| `strum` | 0.26.3 | MIT |
| `strum_macros` | 0.26.4 | MIT |
| `synstructure` | 0.13.2 | MIT |
| `tiff` | 0.11.3 | MIT |
| `tracing` | 0.1.44 | MIT |
| `tracing-attributes` | 0.1.31 | MIT |
| `tracing-core` | 0.1.36 | MIT |
| `uds_windows` | 1.2.1 | MIT |
| `urlencoding` | 2.1.3 | MIT |
| `wayland-backend` | 0.3.17 | MIT |
| `wayland-client` | 0.31.15 | MIT |
| `wayland-csd-frame` | 0.3.0 | MIT |
| `wayland-cursor` | 0.31.14 | MIT |
| `wayland-protocols` | 0.32.13 | MIT |
| `wayland-protocols-experimental` | 20250721.0.1 | MIT |
| `wayland-protocols-misc` | 0.3.12 | MIT |
| `wayland-protocols-plasma` | 0.3.12 | MIT |
| `wayland-protocols-wlr` | 0.3.12 | MIT |
| `wayland-scanner` | 0.31.11 | MIT |
| `wayland-sys` | 0.31.11 | MIT |
| `winnow` | 1.0.4 | MIT |
| `x11-dl` | 2.21.0 | MIT |
| `xcursor` | 0.3.11 | MIT |
| `xkbcommon-dl` | 0.4.2 | MIT |
| `xml-rs` | 0.8.29 | MIT |
| `zbus` | 5.19.0 | MIT |
| `zbus_macros` | 5.19.0 | MIT |
| `zbus_names` | 4.3.4 | MIT |
| `zcheapstr` | 1.1.0 | MIT |
| `zmij` | 1.0.23 | MIT |
| `zstd` | 0.13.3 | MIT |
| `zvariant` | 5.14.0 | MIT |
| `zvariant_derive` | 5.14.0 | MIT |
| `zvariant_utils` | 4.1.0 | MIT |
| `cgl` | 0.3.2 | MIT / Apache-2.0 |
| `ahash` | 0.8.12 | MIT OR Apache-2.0 |
| `android_system_properties` | 0.1.6 | MIT OR Apache-2.0 |
| `android-activity` | 0.6.1 | MIT OR Apache-2.0 |
| `anes` | 0.1.6 | MIT OR Apache-2.0 |
| `anstream` | 1.0.0 | MIT OR Apache-2.0 |
| `anstyle` | 1.0.14 | MIT OR Apache-2.0 |
| `anstyle-parse` | 1.0.0 | MIT OR Apache-2.0 |
| `anstyle-query` | 1.1.5 | MIT OR Apache-2.0 |
| `anstyle-wincon` | 3.0.11 | MIT OR Apache-2.0 |
| `arboard` | 3.6.1 | MIT OR Apache-2.0 |
| `arrayvec` | 0.7.8 | MIT OR Apache-2.0 |
| `as-raw-xcb-connection` | 1.0.1 | MIT OR Apache-2.0 |
| `ash` | 0.38.0+1.3.281 | MIT OR Apache-2.0 |
| `async-broadcast` | 0.7.2 | MIT OR Apache-2.0 |
| `async-recursion` | 1.1.1 | MIT OR Apache-2.0 |
| `async-trait` | 0.1.92 | MIT OR Apache-2.0 |
| `base64` | 0.23.1 | MIT OR Apache-2.0 |
| `bitflags` | 2.13.1 | MIT OR Apache-2.0 |
| `bumpalo` | 3.20.3 | MIT OR Apache-2.0 |
| `cast` | 0.3.0 | MIT OR Apache-2.0 |
| `cc` | 1.4.3 | MIT OR Apache-2.0 |
| `cfg-if` | 1.0.4 | MIT OR Apache-2.0 |
| `clap` | 4.6.6 | MIT OR Apache-2.0 |
| `clap_builder` | 4.6.6 | MIT OR Apache-2.0 |
| `clap_derive` | 4.6.4 | MIT OR Apache-2.0 |
| `clap_lex` | 1.1.0 | MIT OR Apache-2.0 |
| `colorchoice` | 1.0.5 | MIT OR Apache-2.0 |
| `core-foundation` | 0.10.1 | MIT OR Apache-2.0 |
| `core-foundation` | 0.9.4 | MIT OR Apache-2.0 |
| `core-foundation-sys` | 0.8.7 | MIT OR Apache-2.0 |
| `core-graphics` | 0.23.2 | MIT OR Apache-2.0 |
| `core-graphics-types` | 0.1.3 | MIT OR Apache-2.0 |
| `cpufeatures` | 0.3.0 | MIT OR Apache-2.0 |
| `crc32fast` | 1.5.0 | MIT OR Apache-2.0 |
| `crossbeam-deque` | 0.8.7 | MIT OR Apache-2.0 |
| `crossbeam-epoch` | 0.9.20 | MIT OR Apache-2.0 |
| `crossbeam-utils` | 0.8.22 | MIT OR Apache-2.0 |
| `displaydoc` | 0.2.7 | MIT OR Apache-2.0 |
| `document-features` | 0.2.12 | MIT OR Apache-2.0 |
| `ecolor` | 0.31.1 | MIT OR Apache-2.0 |
| `eframe` | 0.31.1 | MIT OR Apache-2.0 |
| `egui` | 0.31.1 | MIT OR Apache-2.0 |
| `egui_glow` | 0.31.1 | MIT OR Apache-2.0 |
| `egui-wgpu` | 0.31.1 | MIT OR Apache-2.0 |
| `egui-winit` | 0.31.1 | MIT OR Apache-2.0 |
| `either` | 1.17.0 | MIT OR Apache-2.0 |
| `emath` | 0.31.1 | MIT OR Apache-2.0 |
| `enumflags2` | 0.7.12 | MIT OR Apache-2.0 |
| `enumflags2_derive` | 0.7.12 | MIT OR Apache-2.0 |
| `epaint` | 0.31.1 | MIT OR Apache-2.0 |
| `errno` | 0.3.14 | MIT OR Apache-2.0 |
| `fdeflate` | 0.3.7 | MIT OR Apache-2.0 |
| `find-msvc-tools` | 0.1.11 | MIT OR Apache-2.0 |
| `flate2` | 1.1.9 | MIT OR Apache-2.0 |
| `form_urlencoded` | 1.2.2 | MIT OR Apache-2.0 |
| `futures-channel` | 0.3.34 | MIT OR Apache-2.0 |
| `futures-core` | 0.3.34 | MIT OR Apache-2.0 |
| `futures-io` | 0.3.34 | MIT OR Apache-2.0 |
| `futures-macro` | 0.3.34 | MIT OR Apache-2.0 |
| `futures-task` | 0.3.34 | MIT OR Apache-2.0 |
| `futures-util` | 0.3.34 | MIT OR Apache-2.0 |
| `getrandom` | 0.3.4 | MIT OR Apache-2.0 |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 |
| `gpu-alloc` | 0.6.2 | MIT OR Apache-2.0 |
| `gpu-alloc-types` | 0.3.1 | MIT OR Apache-2.0 |
| `gpu-descriptor` | 0.3.2 | MIT OR Apache-2.0 |
| `gpu-descriptor-types` | 0.2.0 | MIT OR Apache-2.0 |
| `half` | 2.7.1 | MIT OR Apache-2.0 |
| `hashbrown` | 0.15.5 | MIT OR Apache-2.0 |
| `hashbrown` | 0.17.1 | MIT OR Apache-2.0 |
| `heck` | 0.5.0 | MIT OR Apache-2.0 |
| `hermit-abi` | 0.5.2 | MIT OR Apache-2.0 |
| `hex` | 0.4.3 | MIT OR Apache-2.0 |
| `http` | 1.5.0 | MIT OR Apache-2.0 |
| `httparse` | 1.10.1 | MIT OR Apache-2.0 |
| `idna` | 1.1.0 | MIT OR Apache-2.0 |
| `image` | 0.25.10 | MIT OR Apache-2.0 |
| `image-webp` | 0.2.4 | MIT OR Apache-2.0 |
| `is_terminal_polyfill` | 1.70.2 | MIT OR Apache-2.0 |
| `itoa` | 1.0.18 | MIT OR Apache-2.0 |
| `jni` | 0.22.4 | MIT OR Apache-2.0 |
| `jni-macros` | 0.22.4 | MIT OR Apache-2.0 |
| `jni-sys` | 0.3.1 | MIT OR Apache-2.0 |
| `jni-sys` | 0.4.1 | MIT OR Apache-2.0 |
| `jni-sys-macros` | 0.4.1 | MIT OR Apache-2.0 |
| `jobserver` | 0.1.35 | MIT OR Apache-2.0 |
| `js-sys` | 0.3.104 | MIT OR Apache-2.0 |
| `libc` | 0.2.189 | MIT OR Apache-2.0 |
| `litrs` | 1.0.0 | MIT OR Apache-2.0 |
| `lock_api` | 0.4.14 | MIT OR Apache-2.0 |
| `log` | 0.4.33 | MIT OR Apache-2.0 |
| `memmap2` | 0.9.11 | MIT OR Apache-2.0 |
| `metal` | 0.31.0 | MIT OR Apache-2.0 |
| `naga` | 24.0.0 | MIT OR Apache-2.0 |
| `native-tls` | 0.2.18 | MIT OR Apache-2.0 |
| `ndarray` | 0.17.2 | MIT OR Apache-2.0 |
| `ndk` | 0.9.0 | MIT OR Apache-2.0 |
| `ndk-context` | 0.1.1 | MIT OR Apache-2.0 |
| `ndk-sys` | 0.5.0+25.2.9519653 | MIT OR Apache-2.0 |
| `ndk-sys` | 0.6.0+11769913 | MIT OR Apache-2.0 |
| `num-complex` | 0.4.6 | MIT OR Apache-2.0 |
| `num-integer` | 0.1.47 | MIT OR Apache-2.0 |
| `num-traits` | 0.2.19 | MIT OR Apache-2.0 |
| `once_cell` | 1.21.4 | MIT OR Apache-2.0 |
| `once_cell_polyfill` | 1.70.2 | MIT OR Apache-2.0 |
| `openssl-probe` | 0.2.1 | MIT OR Apache-2.0 |
| `ordered-stream` | 0.2.0 | MIT OR Apache-2.0 |
| `ort` | 2.0.0-rc.13 | MIT OR Apache-2.0 |
| `ort-sys` | 2.0.0-rc.13 | MIT OR Apache-2.0 |
| `parking_lot` | 0.12.5 | MIT OR Apache-2.0 |
| `parking_lot_core` | 0.9.12 | MIT OR Apache-2.0 |
| `paste` | 1.0.15 | MIT OR Apache-2.0 |
| `percent-encoding` | 2.3.2 | MIT OR Apache-2.0 |
| `piper` | 0.2.5 | MIT OR Apache-2.0 |
| `pkg-config` | 0.3.34 | MIT OR Apache-2.0 |
| `png` | 0.18.1 | MIT OR Apache-2.0 |
| `ppv-lite86` | 0.2.21 | MIT OR Apache-2.0 |
| `proc-macro-crate` | 3.5.0 | MIT OR Apache-2.0 |
| `proc-macro2` | 1.0.107 | MIT OR Apache-2.0 |
| `profiling` | 1.0.18 | MIT OR Apache-2.0 |
| `proptest` | 1.11.0 | MIT OR Apache-2.0 |
| `quote` | 1.0.47 | MIT OR Apache-2.0 |
| `rand` | 0.9.5 | MIT OR Apache-2.0 |
| `rand_chacha` | 0.9.0 | MIT OR Apache-2.0 |
| `rand_core` | 0.9.5 | MIT OR Apache-2.0 |
| `rand_xorshift` | 0.4.0 | MIT OR Apache-2.0 |
| `rayon` | 1.12.0 | MIT OR Apache-2.0 |
| `rayon-core` | 1.13.0 | MIT OR Apache-2.0 |
| `regex` | 1.13.1 | MIT OR Apache-2.0 |
| `regex-automata` | 0.4.18 | MIT OR Apache-2.0 |
| `regex-syntax` | 0.8.11 | MIT OR Apache-2.0 |
| `renderdoc-sys` | 1.1.0 | MIT OR Apache-2.0 |
| `rustc_version` | 0.4.1 | MIT OR Apache-2.0 |
| `rustls-pki-types` | 1.15.1 | MIT OR Apache-2.0 |
| `rustversion` | 1.0.23 | MIT OR Apache-2.0 |
| `scopeguard` | 1.2.0 | MIT OR Apache-2.0 |
| `security-framework` | 3.7.0 | MIT OR Apache-2.0 |
| `security-framework-sys` | 2.17.0 | MIT OR Apache-2.0 |
| `semver` | 1.0.28 | MIT OR Apache-2.0 |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_derive` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 |
| `serde_repr` | 0.1.21 | MIT OR Apache-2.0 |
| `shlex` | 2.0.1 | MIT OR Apache-2.0 |
| `signal-hook-registry` | 1.4.8 | MIT OR Apache-2.0 |
| `simdutf8` | 0.1.5 | MIT OR Apache-2.0 |
| `smallvec` | 1.15.2 | MIT OR Apache-2.0 |
| `smol_str` | 0.2.2 | MIT OR Apache-2.0 |
| `stable_deref_trait` | 1.2.1 | MIT OR Apache-2.0 |
| `static_assertions` | 1.1.0 | MIT OR Apache-2.0 |
| `syn` | 2.0.119 | MIT OR Apache-2.0 |
| `syn` | 3.0.3 | MIT OR Apache-2.0 |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 |
| `thiserror` | 1.0.69 | MIT OR Apache-2.0 |
| `thiserror` | 2.0.20 | MIT OR Apache-2.0 |
| `thiserror-impl` | 1.0.69 | MIT OR Apache-2.0 |
| `thiserror-impl` | 2.0.20 | MIT OR Apache-2.0 |
| `toml_datetime` | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| `toml_edit` | 0.25.13+spec-1.1.0 | MIT OR Apache-2.0 |
| `toml_parser` | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| `ttf-parser` | 0.25.1 | MIT OR Apache-2.0 |
| `unarray` | 0.1.4 | MIT OR Apache-2.0 |
| `unicode-segmentation` | 1.13.3 | MIT OR Apache-2.0 |
| `unicode-width` | 0.1.14 | MIT OR Apache-2.0 |
| `unicode-xid` | 0.2.6 | MIT OR Apache-2.0 |
| `ureq` | 3.4.0 | MIT OR Apache-2.0 |
| `ureq-proto` | 0.6.1 | MIT OR Apache-2.0 |
| `url` | 2.5.8 | MIT OR Apache-2.0 |
| `utf8-zero` | 0.8.1 | MIT OR Apache-2.0 |
| `wasm-bindgen` | 0.2.127 | MIT OR Apache-2.0 |
| `wasm-bindgen-futures` | 0.4.77 | MIT OR Apache-2.0 |
| `wasm-bindgen-macro` | 0.2.127 | MIT OR Apache-2.0 |
| `wasm-bindgen-macro-support` | 0.2.127 | MIT OR Apache-2.0 |
| `wasm-bindgen-shared` | 0.2.127 | MIT OR Apache-2.0 |
| `web-sys` | 0.3.104 | MIT OR Apache-2.0 |
| `web-time` | 1.1.0 | MIT OR Apache-2.0 |
| `webbrowser` | 1.2.4 | MIT OR Apache-2.0 |
| `weezl` | 0.1.12 | MIT OR Apache-2.0 |
| `wgpu` | 24.0.5 | MIT OR Apache-2.0 |
| `wgpu-core` | 24.0.5 | MIT OR Apache-2.0 |
| `wgpu-hal` | 24.0.4 | MIT OR Apache-2.0 |
| `wgpu-types` | 24.0.0 | MIT OR Apache-2.0 |
| `windows` | 0.58.0 | MIT OR Apache-2.0 |
| `windows_aarch64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_aarch64_gnullvm` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_aarch64_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_aarch64_msvc` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_i686_gnu` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_i686_gnu` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_i686_gnullvm` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_i686_gnullvm` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_i686_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_i686_msvc` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_x86_64_gnu` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_gnu` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_x86_64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_gnullvm` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_x86_64_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_msvc` | 0.53.1 | MIT OR Apache-2.0 |
| `windows-core` | 0.58.0 | MIT OR Apache-2.0 |
| `windows-implement` | 0.58.0 | MIT OR Apache-2.0 |
| `windows-interface` | 0.58.0 | MIT OR Apache-2.0 |
| `windows-link` | 0.2.1 | MIT OR Apache-2.0 |
| `windows-result` | 0.2.0 | MIT OR Apache-2.0 |
| `windows-strings` | 0.1.0 | MIT OR Apache-2.0 |
| `windows-sys` | 0.52.0 | MIT OR Apache-2.0 |
| `windows-sys` | 0.59.0 | MIT OR Apache-2.0 |
| `windows-sys` | 0.60.2 | MIT OR Apache-2.0 |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 |
| `windows-targets` | 0.52.6 | MIT OR Apache-2.0 |
| `windows-targets` | 0.53.5 | MIT OR Apache-2.0 |
| `x11rb` | 0.13.2 | MIT OR Apache-2.0 |
| `x11rb-protocol` | 0.13.2 | MIT OR Apache-2.0 |
| `zstd-safe` | 7.2.4 | MIT OR Apache-2.0 |
| `r-efi` | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| `r-efi` | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| `cursor-icon` | 1.2.0 | MIT OR Apache-2.0 OR Zlib |
| `glow` | 0.16.0 | MIT OR Apache-2.0 OR Zlib |
| `raw-window-handle` | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| `xkeysym` | 0.2.1 | MIT OR Apache-2.0 OR Zlib |
| `zune-core` | 0.5.3 | MIT OR Apache-2.0 OR Zlib |
| `zune-jpeg` | 0.5.15 | MIT OR Apache-2.0 OR Zlib |
| `miniz_oxide` | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| `bitflags` | 1.3.2 | MIT/Apache-2.0 |
| `criterion-plot` | 0.5.0 | MIT/Apache-2.0 |
| `downcast-rs` | 1.2.1 | MIT/Apache-2.0 |
| `foreign-types` | 0.3.2 | MIT/Apache-2.0 |
| `foreign-types` | 0.5.0 | MIT/Apache-2.0 |
| `foreign-types-macros` | 0.2.4 | MIT/Apache-2.0 |
| `foreign-types-shared` | 0.1.1 | MIT/Apache-2.0 |
| `foreign-types-shared` | 0.3.1 | MIT/Apache-2.0 |
| `gcc` | 0.3.55 | MIT/Apache-2.0 |
| `itertools` | 0.10.5 | MIT/Apache-2.0 |
| `khronos-egl` | 6.0.0 | MIT/Apache-2.0 |
| `matrixmultiply` | 0.3.11 | MIT/Apache-2.0 |
| `openssl-macros` | 0.1.1 | MIT/Apache-2.0 |
| `plain` | 0.2.3 | MIT/Apache-2.0 |
| `quick-error` | 1.2.3 | MIT/Apache-2.0 |
| `quick-error` | 2.0.1 | MIT/Apache-2.0 |
| `rawpointer` | 0.2.1 | MIT/Apache-2.0 |
| `rusty-fork` | 0.3.1 | MIT/Apache-2.0 |
| `scoped-tls` | 1.0.1 | MIT/Apache-2.0 |
| `socks` | 0.3.4 | MIT/Apache-2.0 |
| `type-map` | 0.5.1 | MIT/Apache-2.0 |
| `vcpkg` | 0.2.15 | MIT/Apache-2.0 |
| `version_check` | 0.9.5 | MIT/Apache-2.0 |
| `wait-timeout` | 0.2.1 | MIT/Apache-2.0 |
| `winapi` | 0.3.9 | MIT/Apache-2.0 |
| `winapi-i686-pc-windows-gnu` | 0.4.0 | MIT/Apache-2.0 |
| `winapi-x86_64-pc-windows-gnu` | 0.4.0 | MIT/Apache-2.0 |
| `zstd-sys` | 2.0.16+zstd.1.5.7 | MIT/Apache-2.0 |
| `lumina-bench` | 0.1.0 | NO-LICENSE-FIELD |
| `lumina-cli` | 0.1.0 | NO-LICENSE-FIELD |
| `lumina-core` | 0.1.0 | NO-LICENSE-FIELD |
| `lumina-gui` | 0.1.0 | NO-LICENSE-FIELD |
| `lumina-lensfun` | 0.1.0 | NO-LICENSE-FIELD |
| `lumina-mcp` | 0.1.0 | NO-LICENSE-FIELD |
| `lumina-onnx` | 0.1.0 | NO-LICENSE-FIELD |
| `lumina-raw` | 0.1.0 | NO-LICENSE-FIELD |
| `lumina-sidecar` | 0.1.0 | NO-LICENSE-FIELD |
| `icu_collections` | 2.3.0 | Unicode-3.0 |
| `icu_locale_core` | 2.3.0 | Unicode-3.0 |
| `icu_normalizer` | 2.3.0 | Unicode-3.0 |
| `icu_normalizer_data` | 2.3.0 | Unicode-3.0 |
| `icu_properties` | 2.3.0 | Unicode-3.0 |
| `icu_properties_data` | 2.3.0 | Unicode-3.0 |
| `icu_provider` | 2.3.0 | Unicode-3.0 |
| `litemap` | 0.8.3 | Unicode-3.0 |
| `potential_utf` | 0.1.6 | Unicode-3.0 |
| `tinystr` | 0.8.4 | Unicode-3.0 |
| `writeable` | 0.6.4 | Unicode-3.0 |
| `yoke` | 0.8.3 | Unicode-3.0 |
| `yoke-derive` | 0.8.2 | Unicode-3.0 |
| `zerofrom` | 0.1.8 | Unicode-3.0 |
| `zerofrom-derive` | 0.1.7 | Unicode-3.0 |
| `zerotrie` | 0.2.5 | Unicode-3.0 |
| `zerovec` | 0.11.7 | Unicode-3.0 |
| `zerovec-derive` | 0.11.4 | Unicode-3.0 |
| `aho-corasick` | 1.1.5 | Unlicense OR MIT |
| `byteorder` | 1.5.0 | Unlicense OR MIT |
| `byteorder-lite` | 0.1.0 | Unlicense OR MIT |
| `memchr` | 2.8.3 | Unlicense OR MIT |
| `termcolor` | 1.4.1 | Unlicense OR MIT |
| `winapi-util` | 0.1.11 | Unlicense OR MIT |
| `same-file` | 1.0.6 | Unlicense/MIT |
| `walkdir` | 2.5.0 | Unlicense/MIT |
| `foldhash` | 0.1.5 | Zlib |
| `slotmap` | 1.1.1 | Zlib |
| `bytemuck` | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| `bytemuck_derive` | 1.12.0 | Zlib OR Apache-2.0 OR MIT |
| `dispatch2` | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| `objc2-app-kit` | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-foundation` | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-graphics` | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| `objc2-io-surface` | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
