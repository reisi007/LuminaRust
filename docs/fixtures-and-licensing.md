# Fixtures, Models & Licensing

**Features:** F-073 (small versioned reference images, RAW fixtures & models) ·
F-078 (license, model & distribution audit)
**Status:** Implemented (documentation + audit). Not yet verified.
**Authority:** [`feature/quality/fixtures-licensing.md`](feature/quality/fixtures-licensing.md)
**Companion:** [`THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md) (full crate table)

This document inventories LuminaRust's test fixtures and ML models, records
their licenses, and audits every dependency for license compatibility ahead of
the first release. Every license claim below is verified against `Cargo.toml` /
`Cargo.lock` / `cargo metadata` — no guesses. Open items and recommended actions
are listed in §7.

---

## 1. Fixture inventory

LuminaRust currently ships two classes of test fixtures. There are **no golden
reference images** yet (golden tests are deferred to F-043 / F-073).

### 1.1 Synthetic benchmark fixtures — `crates/lumina-bench/bench/common/mod.rs`

All benchmark inputs are generated **locally and deterministically**; no network
access, no external files.

| Fixture | Generator | Output | Notes |
| --- | --- | --- | --- |
| Frame | `make_frame(size)` → `make_pixels` | RGBA8 `size×size`, α=255 | `size ∈ {512, 1024, 2048}` (`SIZES`) |
| Recipe | `make_recipe()` | `EditRecipe` | exposure/contrast/WB/vibrance/saturation, no geometry |
| Mask | `make_mask_fixture(size)` | `VirtualCopy` + `BTreeMap<(copy,mask), MaskPlane>` | single "subject" mask, smooth gradient `0..=u16::MAX` |
| Cache | `make_cache_fixture(size)` | `FolderCache` with one PNG entry | used for hit/miss cache benches |

Determinism contract (must NOT change without re-recording baselines):

- **Frozen seed:** `pub const FIXTURE_SEED: u64 = 0x5EED;`
- **PRNG:** `SplitMix64` (dependency-free, in-file) — deliberately avoids the
  `rand` crate so the harness stays free of extra runtime deps and the output is
  auditable.
- **Per-size seed:** `FIXTURE_SEED ^ (size * 0x2545F4914F6CDD1D)`.
- Changing the seed invalidates `perf/baseline.json`; re-record before
  comparing medians/p95s (see `feature/quality/performance-benchmarks.md`).

Consumers: `bench/core.rs`, `bench/batch.rs`, `bench/decode.rs` (decode only
reads committed RAW fixtures, see §1.2).

### 1.2 Committed RAW fixtures — `sample-data/raw/`

| File | Dimensions (W×H) | EXIF orientation | Used by |
| --- | --- | --- | --- |
| `aircraft-landscape.cr3` | 6032×4024 | 1 | `lumina-raw` test `aircraft_landscape_fixture_has_expected_geometry_and_metadata`; decode benches |
| `aircraft-portrait.cr3` | 4024×6032 | 5 | `lumina-raw` test `aircraft_portrait_fixture_applies_exif_orientation`; decode benches |

- Decoded via `include_bytes!("../../../sample-data/raw/<file>.cr3")` directly in
  `crates/lumina-raw/src/lib.rs` tests (no env gating).
- Decode benchmarks in `crates/lumina-bench/bench/decode.rs` read these files
  from the directory named by the **`LUMINA_RAW_FIXTURE`** environment variable;
  without it they print a skip note and return early (no panic, no fallback).
- A separate, **ignored** test `optional_real_fixture_checks_decode_orientation_and_dimensions`
  is `#[ignore = "set LUMINA_RAW_FIXTURE to a licensed fixture"]` — it expects the
  operator to point at a *separately licensed* RAW via the env var.

### 1.3 Golden reference images

None exist today. Golden-image tests are explicitly deferred (F-043 / F-073,
per `Agents.todo.md` and `feature/README.md` conflicts-and-acceptance section).
When introduced, they must follow the same versioning/determinism rules as §1.1
and carry an explicit license (prefer generated/CC0 to avoid the §3 gap).

---

## 2. RAW fixture provenance & licensing — ⚠️ OPEN GAP

**Finding:** The two committed `.cr3` fixtures in `sample-data/raw/` have **no
documented source, author, or license**. There is no `LICENSE`/`README` in
`sample-data/`, and the introducing commit (`1e388bf "Add tone controls and RAW
sample fixtures"`) records no provenance.

This is a **release blocker** under F-078: distributing copyrighted camera
RAW files without a license grant is a legal risk, independent of the (MIT)
Rust code. The existing `*_fixture_*` tests hard-depend on these exact bytes, so
the gap is concrete, not theoretical.

See §7 (R1) for the recommended remediation.

---

## 3. Model inventory & licenses

| Model | Role | License | Status | Evidence |
| --- | --- | --- | --- | --- |
| **BiRefNet** (Zheng et al., arXiv:2401.03407) | first automatic subject model (`subject_segmentation`) | **Apache-2.0** | weights **pending integration** (`model_hash = "pending-integration"`) | `crates/lumina-onnx/src/manifest.rs::birefnet_manifest` sets `license: "Apache-2.0"`; README of `lumina-onnx` confirms |
| **SAM 2** | first interactive box/brush model | **TBD** (verify at integration) | planned only | `feature/README.md` "Festgelegte Entscheidungen" |
| **ONNX Runtime** (via `ort` 2.0.0-rc.13) | inference runtime for the above | **MIT** (ORT crate `MIT OR Apache-2.0`; `ort-sys` `MIT OR Apache-2.0`; ONNX Runtime C lib MIT) | **optional**, gated behind `onnx-rt` feature; not in default build | `crates/lumina-onnx/Cargo.toml`; `cargo metadata --all-features` |

Notes:
- No model **weights/binaries** are committed to the repo today. BiRefNet's
  manifest hash is a placeholder; actual weights arrive in F-048. The license
  obligation only materializes once weights are bundled.
- ONNX Runtime (real) is fetched as **prebuilt binaries at build time** (network)
  by `ort-sys`. When `onnx-rt` is enabled for a release, its redistribution terms
  (ORT is MIT) and the prebuilt-binary download's terms must be re-checked (R4).
- BiRefNet's Apache-2.0 status is taken from the manifest/README, which cite the
  upstream repo; it should be re-confirmed against the actual weight source at
  integration.

---

## 4. Dependency license audit (F-078)

### 4.1 Methodology

- `cargo metadata --format-version=1` (default) → **441** packages.
- `cargo metadata --all-features --format-version=1` (incl. optional `onnx-rt`
  → `ort`, `ort-sys`, `ureq`, `hmac-sha256`) → **478** packages.
- Licenses read from each crate's `license` SPDX field; cross-checked against
  `Cargo.lock`.
- Full enumerated table: **`THIRD-PARTY-NOTICES.md`**.

### 4.2 Results summary

| Category | Result |
| --- | --- |
| Strong copyleft (GPL / AGPL / SSPL / MPL / EPL) | **NONE** in default **or** all-features graph |
| OSI-approved permissive (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, 0BSD, CC0-1.0, Unlicense, BSL-1.0) | dominant majority |
| OSI-approved w/ weak exception (Apache-2.0 WITH LLVM-exception, Unicode-3.0, OFL-1.1, Ubuntu-font-1.0) | acceptable; attribution required |
| Weak copyleft (`LGPL-2.1-or-later`) | **only `r-efi`**, and **only for `uefi` targets** |
| Native C library (copyleft) | **LibRaw** (dynamic link) — see §4.4 |

### 4.3 Compatibility matrix

| License (family) | Count (all-features) | OSI-approved | Distribution risk | Action |
| --- | --- | --- | --- | --- |
| MIT / `MIT OR Apache-2.0` / `MIT/Apache-2.0` / `Apache-2.0 OR MIT` / `Apache-2.0/MIT` | ~430 | ✅ | none | bundle notice |
| Apache-2.0 | ~16 | ✅ | none (patent grant) | bundle NOTICE if present |
| BSD-2/3-Clause, ISC, Zlib, 0BSD, CC0-1.0, Unlicense | ~30 | ✅ | none | bundle notice |
| BSL-1.0 (Boost) | 2 | ✅ | none | bundle notice |
| Unicode-3.0 (ICU/data) | 18 | ✅ | none | bundle Unicode notice |
| OFL-1.1 / Ubuntu-font-1.0 (egui fonts) | 1 crate (`epaint_default_fonts`) | ✅ | font attribution | bundle font license |
| Apache-2.0 WITH LLVM-exception | ~6 | ✅ | none | bundle notice |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` (`r-efi`) | 2 | ✅ (under MIT/Apache) | only if compiled for `uefi` | never shipped → no action; otherwise comply under MIT |
| **LibRaw (LGPL-2.1 / CDDL-1.0 / LibRaw Software License)** | native, linked | ⚠️ weak copyleft | **only real obligation** | see §4.4 |

### 4.4 The single real obligation: LibRaw

`lumina-raw` depends on `vendor/libraw-sys` (MIT, © David Cuddeback), a patched
fork pinned via `[patch.crates-io]` in the workspace root `Cargo.toml`. Its
`build.rs` resolves the **system** `libraw_r` shared library through `pkg-config`
and emits `cargo:rustc-link-lib` — i.e. **dynamic linking**, not vendoring or
static embedding.

- Upstream LibRaw is **tri-licensed**: LGPL-2.1-or-later **OR** CDDL-1.0 **OR**
  the *LibRaw Software License* (a permissive, BSD-like license).
  > Note: `docs/adr/0002-raw-backend.md` currently describes LibRaw as
  > "dual-licensed LGPL-2.1 and CDDL-1.0". Upstream actually offers the third,
  > permissive option — see open item R5.
- **Obligation:** Keep the **dynamic-link** arrangement (statically embedding
  LibRaw would extend LGPL to the whole binary). Ship LibRaw's license text and
  a written offer / pointer to LibRaw source for the exact version used. CI pins
  **LibRaw 0.22.2** (recorded in the `lumina.libraw_version` OCI label of the
  `lumina-ci` image). Prefer relying on the permissive **LibRaw Software License**
  option where the distributor's context allows.
- This is already acknowledged in the top-level `README.md` (§"LibRaw steht
  unter der LGPL-2.1-or-later; Distributionen müssen die LibRaw-Lizenz …").

---

## 5. Version pinning policy

| Artifact | Pin mechanism | Value |
| --- | --- | --- |
| Whole Rust tree | committed `Cargo.lock` + `resolver = "2"` | authoritative for reproducible builds |
| `ort` / `ort-sys` | exact version in `lumina-onnx/Cargo.toml` | `=2.0.0-rc.13` |
| `libraw-sys` | `[patch.crates-io]` → `vendor/libraw-sys` (path) | patched `0.1.1` |
| LibRaw (native) | CI container `lumina-ci` OCI label `lumina.libraw_version` | `0.22.2` (Homebrew-equivalent) |
| Benchmark fixtures | frozen `FIXTURE_SEED = 0x5EED` + `SplitMix64` | change ⇒ re-record `perf/baseline.json` |
| Model weights | `ModelManifest.model_hash` is the identity | BiRefNet hash currently `pending-integration` |

Policy:
- `Cargo.lock` is committed and must stay so for reproducible, auditable builds.
- Native dependencies (LibRaw) are pinned by the immutable CI image, not by
  Cargo; document the exact version in release notes.
- Model weights, once integrated, must be recorded by content hash + exact
  version + license + source URL in the sidecar/manifest and in this doc.
- Fixture seeds are frozen; any change requires re-baselining.

---

## 6. Open items, risks & recommended actions

| ID | Severity | Item | Recommended action |
| --- | --- | --- | --- |
| **R1** | 🔴 Blocker | Committed `.cr3` fixtures (`aircraft-landscape`, `aircraft-portrait`) have **no documented license/provenance** (§2). | Document source + license, **or** replace with a generated/synthetic or CC0-licensed fixture before release. Do not distribute the current binaries until resolved. |
| **R2** | 🟠 High | All 8 workspace crates declare **no `license` field** and the repo root has **no LICENSE file** — project is intentionally unlicensed / commercial-for-now pending the MVP license decision. | Decide project license at MVP (see `Agents.todo.md` LIZ-ENTSCHEIDUNG); then add `license` to all crates + root `LICENSE` consistent with that decision. |
| **R3** | 🟠 High | LibRaw dynamic-link obligation (§4.4). | Keep dynamic linking; ship LibRaw license + source offer for 0.22.2 in the release bundle/NOTICE. |
| **R4** | 🟡 Medium | `onnx-rt` path downloads ONNX Runtime prebuilt binaries (network) and would bundle MIT-licensed ORT. | When enabling `onnx-rt` for a release, re-verify ORT/ONNX Runtime redistribution terms, keep `=2.0.0-rc.13` pin, and record model weight licenses/hashes. |
| **R5** | 🟡 Medium | ADR 0002 says LibRaw is "dual" (LGPL/CDDL); upstream is **tri**-licensed (adds permissive LibRaw Software License). | Update ADR 0002 to record the third, permissive option and recommend relying on it. |
| **R6** | 🟡 Medium | SAM 2 license unverified; BiRefNet Apache-2.0 taken from manifest (not from weight source). | Verify each model's license against the actual weight source at integration (F-048/F-080) and record in §3. |
| **R7** | 🟢 Low | `r-efi` carries an `LGPL-2.1-or-later` option. | No action: UEFI-only, never shipped for supported targets; if ever built for UEFI, comply under its MIT/Apache option. |

No GPL/AGPL/SSPL/strong-copyleft dependency exists anywhere in the tree — the
only copyleft-licensed crate is `r-efi` and it is unreachable for shipped
targets.

---

## 7. Verification commands (for the verification agent)

```sh
# 1. Reproduce the full crate license table (matches THIRD-PARTY-NOTICES.md)
cargo metadata --all-features --format-version=1 \
  | jq -r '.packages[] | "\(.name)\t\(.version)\t\(.license // "NO-LICENSE-FIELD")"' \
  | sort -u

# 2. Confirm NO strong copyleft anywhere
cargo metadata --all-features --format-version=1 \
  | jq -r '.packages[].license // empty' | grep -iE 'GPL|AGPL|SSPL|MPL|EPL|CDDL' | sort | uniq -c
#    → expect only: "MIT OR Apache-2.0 OR LGPL-2.1-or-later" (r-efi, UEFI-only)

# 3. Confirm BiRefNet license literal in source
grep -n 'license: "Apache-2.0"' crates/lumina-onnx/src/manifest.rs

# 4. Confirm fixture seed is frozen
grep -n 'FIXTURE_SEED' crates/lumina-bench/bench/common/mod.rs

# 5. Confirm raw fixtures exist and are referenced
ls -l sample-data/raw/
grep -rn 'sample-data/raw' crates/lumina-raw/src/lib.rs crates/lumina-bench/bench/decode.rs

# 6. Confirm workspace crates missing a license field (open item R2)
for f in crates/*/Cargo.toml; do grep -H '^name' "$f"; grep -H '^license' "$f" || echo "   <-- NO license field"; done
```
