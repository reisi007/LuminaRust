//! PREVIEW-CACHE-FEATURE (F-074): Criterion benchmarks for the hybrid neighbor
//! preview cache primitives (`feature/quality/preview-cache.md`,
//! Akzeptanzkriterium 8).
//!
//! Measures the two cache tiers the SOLL mandates to make the next image switch
//! decode-/render-free:
//!
//! - **WebP cache hit** (`preview_cache/webp_hit__{512,1024}`): decoding an
//!   already-stored lossless WebP preview — the cost of serving a neighbor from
//!   RAM/disk without a full render.
//! - **WebP cache miss** (`preview_cache/webp_miss__{512,1024}`): lossless
//!   WebP encoding of a rendered frame — the cache-specific cost a miss pays to
//!   persist a neighbor preview (the render itself is measured by
//!   `core/render_frame__*`).
//! - **RAM-LRU hit** (`preview_cache/lru_hit__7`): lookup of a resident entry
//!   in the 7-slot LRU (active + 6 neighbors) — the fast path before any disk
//!   access on navigation.
//! - **Prefetch-window planning** (`preview_cache/prefetch_window__40`): the
//!   asymmetric **+4/−2** window computation around an active image in a 40-image
//!   folder, including the mandated priority ordering.
//!
//! Fixtures are deterministic (fixed seed, locally generated RGBA8 frames —
//! identical rule as `crates/lumina-bench/bench/common/mod.rs`). No network
//! access. This harness is a native-only Criterion target in `lumina-gui`,
//! matching the F-074 "separate native harness" policy in spirit; the preview-cache primitives are addressed here
//! because they are GUI-side (`preview_ctrl.rs` / `preview_cache.rs`) and the
//! `lumina-bench` crate is a separate workspace member.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use lumina_core::{
    preview_cache::{
        encode_webp_lossless, prefetch_window, LruPreviewCache, PreviewEncode, PreviewKey,
        PreviewKind,
    },
    ImageFrame,
};
use std::hint::black_box;
use std::time::Duration;

/// Fixed seed for the synthetic fixture frames (F-074 fixture rule: frozen and
/// documented; changing it invalidates recorded preview-cache baselines).
const FIXTURE_SEED: u64 = 0x96_5E_ED;

/// A tiny deterministic RGBA8 fixture frame of `size × size` (SplitMix-style
/// mixer, no `rand` dependency — mirrors `lumina-bench`'s `make_frame`).
fn make_frame(size: u32) -> ImageFrame {
    let mut state = FIXTURE_SEED
        .wrapping_mul(0x2545_F491_4F6C_DD1D + u64::from(size).wrapping_mul(0x9E37_79B9));
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let pixels: Vec<u8> = (0..size as usize * size as usize)
        .flat_map(|i| {
            let v = next() as u8;
            // Distinct RGBA per pixel with alpha=255 (like the bench common fixture).
            [
                v ^ (i as u8),
                v.wrapping_add(7),
                v.wrapping_mul(13 ^ (i as u8)),
                255,
            ]
        })
        .collect();
    ImageFrame::new(size, size, pixels).expect("deterministic pixel count matches dimensions")
}

/// Cache-identity sample used to derive a stable key for the LRU (only the
/// digest matters for the RAM-LRU hot path).
fn sample_key() -> PreviewKey {
    PreviewKey {
        source_content_hash: "bench-source".into(),
        decode_context: "decode-v1".into(),
        pipeline_version: "bench".into(),
        virtual_copy_id: "vc-original".into(),
        render_key: "bench-render".into(),
        kind: PreviewKind::Screen,
        width: 512,
        height: 512,
        encode: PreviewEncode::default(),
    }
}

fn preview_cache_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("preview_cache");

    for &size in &[512u32, 1024u32] {
        // ---- WebP cache hit: decode a stored lossless preview ----
        let frame = make_frame(size);
        let webp = encode_webp_lossless(&frame).expect("deterministic WebP encode");
        group.bench_function(format!("webp_hit__{size}"), |b| {
            b.iter(|| {
                let decoded = lumina_core::preview_cache::decode_webp(black_box(&webp))
                    .expect("cached preview must decode");
                black_box(decoded.pixels[0]);
            })
        });

        // ---- WebP cache miss: lossless-encode a rendered frame ----
        group.bench_function(format!("webp_miss__{size}"), |b| {
            b.iter_batched(
                || frame.clone(),
                |f| {
                    let bytes =
                        encode_webp_lossless(black_box(&f)).expect("deterministic WebP encode");
                    black_box(bytes.len())
                },
                BatchSize::SmallInput,
            )
        });
    }

    // ---- RAM-LRU hit: 7-slot LRU (active + 6 neighbors), resident key ----
    let key = sample_key().digest();
    let mut lru = LruPreviewCache::default();
    for i in 0..7 {
        lru.insert(format!("neighbor-{i}"), make_frame(64));
    }
    lru.insert(key.clone(), make_frame(64));
    let hit_key = key;
    group.bench_function("lru_hit__7", |b| {
        b.iter(|| {
            let frame = lru.get(black_box(&hit_key)).expect("resident key is a hit");
            black_box(frame.pixels[0]);
        })
    });

    // ---- Prefetch-window planning (+4/−2, priority order), 40-image folder ----
    group.bench_function("prefetch_window__40", |b| {
        b.iter(|| {
            let slots = prefetch_window(black_box(20), black_box(40));
            black_box(slots.len())
        })
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = preview_cache_benches
}
criterion_main!(benches);
