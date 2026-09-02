# LuminaRust CI image — native build/runtime dependencies, pinned.
#
# Purpose: give GitHub Actions the SAME LibRaw version as local development
# (Homebrew `libraw` 0.22.2) without rebuilding it from source on every run.
# LibRaw changed the reported raw dimensions for Canon CR3 between 0.21.x and
# 0.22.x (6160x4144 vs 6032x4024); the lumina-raw golden fixture tests assert
# exact dimensions, so the decode version must be deterministic.
#
# Built and published by .github/workflows/ci-libraw-image.yml. Published
# tags:
#   - <commit-sha>: immutable per build — reproducible pull by SHA
#   - latest:       moving alias referenced by ci.yml
# The LibRaw version deliberately lives in the image metadata
# (OCI label `lumina.libraw_version`) and as ARG below, NOT in the tag: a tag
# identifies the image build (which also contains base OS, g++, pkg-config
# and build flags), it does not version one of its inputs.
#
# The image deliberately does NOT contain a Rust toolchain: `dtolnay/rust-
# toolchain` installs the current stable toolchain inside the job container.
#
# Build system note: LibRaw 0.22.2 has NO upstream CMake support (README.cmake:
# cmake builds "not officially supported ... since 2014", moved to a separate
# LibRaw-cmake repo) and the GitHub tag tarball ships no generated `configure`
# script — the canonical build is Autotools: autoreconf + configure + make.

FROM ubuntu:24.04 AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        autoconf \
        automake \
        libtool \
        pkg-config \
        g++ \
        make \
        zlib1g-dev \
        libjpeg-dev \
        liblcms2-dev \
    && rm -rf /var/lib/apt/lists/*

# Pin the exact LibRaw release tag and the sha256 of its GitHub source
# tarball. Keep the version in sync with the local Homebrew `libraw` used by
# developers (`brew info libraw`).
ARG LIBRAW_VERSION=0.22.2
ARG LIBRAW_SHA256=627928088300ecde6ca91ffd202e189203f04ad61ad12f0fe9dc57b9a7a0fb3c
RUN set -eux; \
    curl -fsSL -o /tmp/libraw.tar.gz \
        "https://github.com/LibRaw/LibRaw/archive/refs/tags/${LIBRAW_VERSION}.tar.gz"; \
    echo "${LIBRAW_SHA256}  /tmp/libraw.tar.gz" | sha256sum -c -; \
    mkdir -p /tmp/libraw-src; \
    tar -xzf /tmp/libraw.tar.gz -C /tmp/libraw-src --strip-components=1; \
    rm /tmp/libraw.tar.gz \
    && cd /tmp/libraw-src \
    && autoreconf -fi \
    && ./configure --prefix=/usr/local \
    && make -j"$(nproc)" \
    && make install \
    && rm -rf /tmp/libraw-src

FROM ubuntu:24.04

# Runtime + cargo-build-time needs:
# - libraw_r.so (installed below) plus the *-dev packages: `libraw_r.pc`
#   declares `Requires: lcms2`, and pkg-config must resolve lcms2.pc /
#   libjpeg / zlib at cargo build time (the -dev packages pull the runtime
#   libraries as dependencies);
# - pkg-config for the vendored `libraw-sys` build script (`libraw_r.pc`);
# - a C++ compiler: the libraw-sys build script compiles small probe programs
#   to detect LibRaw struct members (`check_member`), so g++ is required;
# - curl/wget/ca-certificates: `dtolnay/rust-toolchain` downloads rustup and
#   the Rust toolchain inside this container over HTTPS.
# - liblensfun-dev: `lumina-lensfun`'s `native` feature (and transitively
#   `lumina-core`'s `lensfun` feature) links the system liblensfun via
#   pkg-config at cargo build time, so the -dev package (lensfun.pc + headers,
#   pulling libLensfun1 runtime transitively) must be present for the CI
#   Lensfun feature tests (F-098-N3). We deliberately use the DISTRO Lensfun
#   and introduce NO separate pinned Lensfun release tag: because this
#   container image is itself pinned and only rebuilt when docker/Dockerfile
#   changes, the Lensfun version is deterministic across CI runs (same as
#   LibRaw 0.22.2 below).
# - libglib2.0-dev: Ubuntu's `lensfun.pc` declares `Requires: glib-2.0`, so
#   pkg-config must resolve `glib-2.0.pc` at cargo build time; the runtime
#   package `libglib2.0-0` (pulled transitively by liblensfun1) does not ship
#   the .pc file. Missing this caused the CI lensfun step to fail with
#   "Package glib-2.0 was not found in the pkg-config search path"
#   (2026-08-20) — fixed here.
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        g++ \
        make \
        curl \
        wget \
        ca-certificates \
        gzip \
        liblcms2-dev \
        libjpeg-dev \
        zlib1g-dev \
        liblensfun-dev \
        libglib2.0-dev \
        libssl-dev \
        clang \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/lib /usr/local/lib
COPY --from=builder /usr/local/include /usr/local/include
COPY --from=builder /usr/local/lib/pkgconfig /usr/local/lib/pkgconfig
# Bake the linker cache so /usr/local/lib/libraw_r.so is found at run time.
RUN ldconfig