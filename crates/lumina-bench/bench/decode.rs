//! Decode benchmarks for `lumina-raw` (F-074-N3).
//!
//! Decodes the committeten RAW fixtures
//! `sample-data/raw/aircraft-landscape.cr3` and `aircraft-portrait.cr3`. These
//! benchmarks are gated behind the `raw-bench` feature (see `Cargo.toml`),
//! which pulls in LibRaw, and behind the `LUMINA_RAW_FIXTURE` environment
//! variable (pointing at the directory that holds the `.cr3` files). Without
//! the variable the benchmark returns early after printing a note — no panic,
//! no network, no implicit fallback. This mirrors the env-gating of the
//! existing RAW tests in `conflicts-and-acceptance.md`.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lumina_raw::decode_bytes;
use std::path::Path;
use std::time::Duration;

/// `(id_suffix, file_name)` — the id suffix becomes `decode/raw__<suffix>`.
const FIXTURES: &[(&str, &str)] = &[
    ("aircraft-landscape", "aircraft-landscape.cr3"),
    ("aircraft-portrait", "aircraft-portrait.cr3"),
];

fn decode_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");

    let base = match std::env::var("LUMINA_RAW_FIXTURE") {
        Ok(value) => value,
        Err(_) => {
            eprintln!(
                "SKIP decode benchmarks: set LUMINA_RAW_FIXTURE to the directory containing \
                 aircraft-landscape.cr3 / aircraft-portrait.cr3"
            );
            group.finish();
            return;
        }
    };

    for (id, file) in FIXTURES {
        let path = Path::new(&base).join(file);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("SKIP decode/raw__{id}: cannot read {path:?}: {error}");
                continue;
            }
        };
        let name = file.to_string();
        group.bench_function(format!("raw__{id}"), |b| {
            b.iter(|| {
                let image = decode_bytes(black_box(&bytes), black_box(&name)).unwrap();
                black_box(image.frame.width)
            })
        });
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2))
        .sample_size(10);
    targets = decode_benches
}
criterion_main!(benches);
