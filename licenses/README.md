# Third-party license texts bundled for distribution

This directory holds the verbatim license texts and written source offers for
the native libraries and ML models LuminaRust links against or distributes.
It is the payload that `scripts/release/bundle-licenses.sh` copies into every
release bundle. The authoritative inventory and obligations live in
[`../THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md); the normative feature
spec is [`feature/quality/fixtures-licensing.md`](../feature/quality/fixtures-licensing.md)
(F-073/F-078). Project code itself is deliberately unlicensed until the MVP
license decision (LIZ, see `Agents.todo.md`) — this directory only covers
**third-party** material.

## Layout

```
licenses/
├── README.md                        ← this file (inventory + release checklist)
├── libraw/                          ← native RAW decoder (default builds link it)
│   ├── COPYRIGHT                    upstream notice file @ tag 0.22.2 (verbatim;
│   │                                includes dcraw/DCB-FBDD/X3F/Adobe-DNG embedded notices)
│   ├── LICENSE.LGPL                 GNU LGPL-2.1 (canonical gnu.org text)
│   ├── LICENSE.CDDL                 CDDL-1.0 (verbatim from LibRaw @ 0.22.2)
│   └── SOURCE-OFFER.md              written source offer for LibRaw 0.22.2 (R3)
├── lensfun/                         ← native lens correction (feature "native", default off)
│   ├── COPYING.LGPL-3.0             GNU LGPL-3.0 (= upstream lgpl-3.0.txt)
│   ├── COPYING.CC-BY-SA-3.0         Creative Commons BY-SA 3.0 Unported legalcode (lens DB)
│   └── SOURCE-OFFER.md              source offer Lensfun 0.3.4 + DB attribution
└── models/                          ← ML models + inference runtime
    ├── BiRefNet-LICENSE-MIT.txt     © 2024 ZhengPeng (MIT)
    ├── SAM-2-LICENSE-Apache-2.0.txt Meta SAM 2.1 (Apache-2.0, code + weights; no NOTICE file exists)
    ├── ONNXRuntime-LICENSE-MIT.txt  © Microsoft Corporation (MIT)
    └── MODEL-SOURCE-OFFER.md        provenance pins, export-path rules, R4 note
```

All texts were fetched verbatim on **2026-08-25** from their canonical sources
(`libraw.org` / GitHub tags of the upstream projects, gnu.org,
creativecommons.org). Do not edit license texts; update them only by replacing
them with a fresh verbatim copy when a version pin changes.

## Release checklist — what must be where

Run before packaging any distributable build:

```sh
scripts/release/bundle-licenses.sh [DEST_DIR]   # default: dist/licenses
```

1. **Every build** (any platform): bundle contains
   `THIRD-PARTY-NOTICES.md` + all of `licenses/` (script enforces presence and
   writes `CHECKSUMS.sha256`; missing files fail the script loudly — no silent
   fallback).
2. **Builds linking LibRaw** (native decode, default): `licenses/libraw/`
   included, dynamic link preserved, source offer names the pinned version
   (0.22.2). If the CI pin changes, update
   `licenses/libraw/SOURCE-OFFER.md` in the same commit.
3. **Builds with feature `native` (Lensfun)**: additionally verify
   `licenses/lensfun/` is in the bundle and the CC-BY-SA-3.0 database
   attribution appears in the visible about/notices screen.
4. **Bundles shipping model weights**: `licenses/models/` included; manifest
   hashes match the shipped weights; export path did not use `ultralytics`.
5. The GUI/CLI should surface the bundled notices location (follow-up task, not
   part of this directory).

