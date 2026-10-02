//! GPU-MASKPLANE-CLEAR-PERF-51 — ein EINMALIGER Adapter-/Entscheidungsbench
//! (keine Gate-Klasse).
//!
//! ## Was gemessen wird
//!
//! Wenn ein Render **ohne** Maskenebene läuft, schreibt
//! `sync_mask_plane_to_vram` in `lumina-gui` eine Null-Plane über die aktive
//! VRAM-`R16Uint`-Maske (`crates/lumina-gui/src/present_mask_plane.rs`,
//! `clear_active_vram_mask_plane`): sie zerlegt die Karte in Bänder
//! (`zero_band_rows(width, 1 MiB)` volle Breite) und ruft pro Band
//! `upload_mask_tile` (→ `shaders::write_mask_tile` → `queue.write_texture`).
//! Gemessen ist bisher nur die **Host-Seite** (0,004 ms, auflösungsunabhängig,
//! weil `vec![0u16; …]` über `alloc_zeroed` geht): genau deshalb ist die
//! **Queue-Seite** offen.
//!
//! Dieser Bench misst die Queue-Seite über die reale Produktionsnaht
//! (`GpuContext::upload_mask_tile`, identisch zur GUI-Schleife) für die drei im
//! Task genannten Auflösungen mit der exakten Bandrechnung:
//!
//! | Auflösung     | Bandzeilen | Bänder | Bytes   |
//! | ------------- | ---------: | -----: | ------: |
//! | 160 × 120     | 3276 → 120 | 1      | 38 400  |
//! | 1280 × 853    | 409        | 3      | 2 183 680 |
//! | 6000 × 4000   | 87         | 46     | 48 000 000 |
//!
//! Zusätzlich misst eine **Kontrolle** dasselbe Volumen in **einem** Aufruf
//! (`upload_mask_plane`, die GPU-STAGE-1-Naht): sie trennt „der Klärvorgang
//! kostet X" von „die Bänderung kostet X". Ohne sie wäre ein hoher Bandwert
//! nicht von einem hohen Datenvolumen unterscheidbar.
//!
//! ## Warum hier und nicht in `lumina-bench`
//!
//! Die Produktionsnaht `upload_mask_tile` ist `pub` in `lumina-gpu` und die
//! Bandberechnung liegt in `lumina-gui`. `lumina-gpu` darf nicht von
//! `lumina-gui` abhängen (Architekturgrenze). Der Bench liegt daher hier, mit
//! einer **wörtlichen Kopie** von `zero_band_rows` und `ZERO_BAND_BYTES`
//! (identisch zu `present_mask_plane.rs`).
//!
//! ## Adapter-Gating
//!
//! Wie `cargo bench -p lumina-bench --bench gpu`: ohne gebundenen Adapter wird
//! die Gruppe sauber übersprungen (`GPU adapter unavailable - skipped …`), kein
//! Panic, kein Netzwerk, **keine erfundene Zahl**. Der Bench ist ein
//! `[[bench]]`-Target dieses Crates und wird NICHT von `lumina-bench` /
//! `perf/baseline.json` eingelesen; die Ausgabe dient der Entscheidung im Task.
//!
//! Kommando:
//! ```text
//! cargo bench -p lumina-gpu --bench mask_plane_clear
//! ```
//! Maschinenlesbar: Criterion schreibt
//! `target/criterion/gpu/mask_plane_clear__<W>x<H>/new/estimates.json`.

// The measured seam (`GpuContext::upload_mask_tile`) is the GPU-hardware
// signature; the pure-CPU build exposes a different stub signature. The whole
// bench is therefore gated behind the `gpu` feature, exactly like
// `lumina-bench`'s `bench/gpu.rs` (declared via `required-features`).
#![cfg(feature = "gpu")]

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use lumina_gpu::GpuContext;
use std::hint::black_box;

/// 1 MiB transienter Zero-Band-Puffer — wörtliche Kopie der Konstante aus
/// `crates/lumina-gui/src/present_mask_plane.rs` (`ZERO_BAND_BYTES`).
const ZERO_BAND_BYTES: usize = 1 << 20;

/// Volle-Breite-Zeilen eines Zero-Bands — wörtliche Kopie von
/// `zero_band_rows` aus `crates/lumina-gui/src/present_mask_plane.rs`.
fn zero_band_rows(width: u32, byte_budget: usize) -> u32 {
    if width == 0 {
        return 1;
    }
    let bytes_per_row = (width as usize).saturating_mul(2);
    (byte_budget / bytes_per_row).max(1) as u32
}

/// Die drei Auflösungen aus `GPU-MASKPLANE-CLEAR-PERF-51`.
const SIZES: [(u32, u32); 3] = [(160, 120), (1280, 853), (6000, 4000)];

fn build_gpu_context() -> Option<GpuContext> {
    match GpuContext::new() {
        Ok(ctx) => {
            if ctx.is_available() {
                Some(ctx)
            } else {
                eprintln!("GPU adapter unavailable - skipped mask_plane_clear bench");
                None
            }
        }
        Err(e) => {
            eprintln!("GPU adapter unavailable - skipped mask_plane_clear bench: {e}");
            None
        }
    }
}

/// Führt die exakte Produktionsschleife aus `clear_active_vram_mask_plane`
/// gegen den gegebenen Kontext aus: gleiche Bandaufteilung, gleiche Reihenfolge,
/// gleiche Naht (`upload_mask_tile`).
fn clear_like_production(ctx: &GpuContext, width: u32, height: u32) {
    let rows_per_band = zero_band_rows(width, ZERO_BAND_BYTES);
    let band = vec![0u16; width as usize * rows_per_band as usize];
    let mut y = 0u32;
    while y < height {
        let rows = rows_per_band.min(height - y);
        let bytes = bytemuck_bytes(&band[..width as usize * rows as usize]);
        ctx.upload_mask_tile(0, y, width, rows, bytes)
            .expect("mask tile upload (clear band)");
        y += rows;
    }
}

/// Kontrolle: dasselbe (Null-)Volumen in **einem** `write_texture`-Aufruf
/// (GPU-STAGE-1-Naht `upload_mask_plane`) statt in `height/rows_per_band`
/// Bandaufrufen. Zeigt den reinen Bänderungs-Aufschlag.
fn single_write_like_plane_push(ctx: &GpuContext, width: u32, height: u32, plane: &[u16]) {
    ctx.upload_mask_plane(width, height, plane)
        .expect("single mask plane upload");
}

/// `bytemuck::cast_slice` wäre die GUI-Naht; hier genügt ein äquivalenter
/// Byte-Slice (`u16` little-endian = zwei Nullbytes).
fn bytemuck_bytes(values: &[u16]) -> &[u8] {
    // SAFETY: `u16` is `Pod`; reinterpreting `&[u16]` as `&[u8]` of twice the
    // length is the exact operation `bytemuck::cast_slice` performs.
    unsafe { std::slice::from_raw_parts(values.as_ptr() as *const u8, values.len() * 2) }
}

fn mask_plane_clear_benches(c: &mut Criterion) {
    let Some(ctx) = build_gpu_context() else {
        // Keine Gruppe anlegen -> keine erfundene Zahl im Report.
        return;
    };

    let mut group = c.benchmark_group("gpu");
    group
        .sample_size(100)
        .warm_up_time(std::time::Duration::from_secs(1))
        .measurement_time(std::time::Duration::from_secs(3));

    for &(w, h) in &SIZES {
        // Die Auflösungen werden in `ensure_vram` als eigene Pool-Einträge
        // gehalten; 6000×4000 braucht ~48 MB (Output) + ~48 MB (Maske) + Input,
        // was der M5-Pro-VRAM ohne Weiteres trägt.
        ctx.ensure_vram(w, h)
            .expect("ensure_vram for mask-plane clear");

        group.bench_function(format!("mask_plane_clear__{w}x{h}"), |b| {
            b.iter(|| clear_like_production(black_box(&ctx), w, h))
        });
    }

    for &(w, h) in &SIZES {
        ctx.ensure_vram(w, h).expect("ensure_vram (control)");
        let plane = vec![0u16; w as usize * h as usize];
        group.bench_function(format!("mask_plane_clear_singlewrite__{w}x{h}"), |b| {
            b.iter_batched(
                || (),
                |()| single_write_like_plane_push(black_box(&ctx), w, h, black_box(&plane)),
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();

    // Gegenprobe der Bandrechnung des Tasks (keine Messaussage, Selbstkontrolle).
    for &(w, h) in &SIZES {
        let rows = zero_band_rows(w, ZERO_BAND_BYTES);
        let bands = h.div_ceil(rows);
        let band_bytes = std::mem::size_of::<u16>() as u64 * w as u64 * rows.min(h) as u64;
        if std::env::var("LUMINA_MASKPLANE_VERBOSE").as_deref() == Ok("1") {
            eprintln!(
                "LUMINA mask_plane_clear {w}x{h}: rows_per_band={rows}, bands={bands}, \
                 band_bytes={band_bytes}, total_bytes={}",
                2 * w as u64 * h as u64
            );
        }
    }
}

criterion_group!(benches, mask_plane_clear_benches);
criterion_main!(benches);
