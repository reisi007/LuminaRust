# LuminaRust CI image — native build/runtime dependencies, pinned.
#
# Purpose: give GitHub Actions the SAME LibRaw version as local development
# (Homebrew `libraw` 0.22.2) without rebuilding it from source on every run.
# LibRaw changed the reported raw dimensions for Canon CR3 between 0.21.x and
# 0.22.x (6160x4144 vs 6032x4024); the lumina-raw golden fixture tests assert
# exact dimensions, so the decode version must be deterministic.
#
# Built and published by .github/workflows/ci-libraw-image.yml to
# ghcr.io/reisi007/luminarust/lumina-ci:libraw-0.22.2 (tag = LibRaw version).
#
# The image deliberately does NOT contain a Rust toolchain: `dtolnay/rust-
# toolchain` installs the current stable toolchain inside the job container.

FROM ubuntu:24.04 AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        cmake \
        g++ \
        git \
        make \
        zlib1g-dev \
        libjpeg-dev \
        liblcms2-dev \
    && rm -rf /var/lib/apt/lists/*

# Pin the exact LibRaw release tag. Keep in sync with the local Homebrew
# `libraw` version used by developers (`brew info libraw`).
ARG LIBRAW_VERSION=0.22.2
RUN git clone --depth 1 --branch "${LIBRAW_VERSION}" \
        https://github.com/LibRaw/LibRaw.git /tmp/libraw \
    && cmake -S /tmp/libraw -B /tmp/libraw/build \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX=/usr/local \
    && cmake --build /tmp/libraw/build -j"$(nproc)" \
    && cmake --install /tmp/libraw/build \
    && rm -rf /tmp/libraw

FROM ubuntu:24.04

# Runtime + cargo-build-time needs:
# - libraw_r.so (installed below, with its runtime deps: jpeg, lcms2, zlib);
# - pkg-config for the vendored `libraw-sys` build script (`libraw_r.pc`);
# - a C++ compiler: the libraw-sys build script compiles small probe programs
#   to detect LibRaw struct members (`check_member`), so g++ is required.
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        g++ \
        make \
        zlib1g \
        libjpeg-turbo8 \
        liblcms2-2 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/lib /usr/local/lib
COPY --from=builder /usr/local/include /usr/local/include
COPY --from=builder /usr/local/lib/pkgconfig /usr/local/lib/pkgconfig
# Bake the linker cache so /usr/local/lib/libraw_r.so is found at run time.
RUN ldconfig