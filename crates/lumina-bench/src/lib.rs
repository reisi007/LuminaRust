//! Native-only benchmark harness for Lumina.
//!
//! This crate is the single native timing harness for the project. The
//! normative methodology lives in
//! [`feature/quality/performance-benchmarks.md`](../../feature/quality/performance-benchmarks.md)
//! (F-074), the architectural decision is documented in
//! [`docs/adr/0003-performance-benchmarking.md`](../../docs/adr/0003-performance-benchmarking.md)
//! (ADR 0003).
//!
//! This crate is intentionally native-only: it is never built for `wasm32`,
//! so the portable `lumina-core` kernel stays free of native benchmark
//! dependencies. Native Criterion measurements act as the proxy for all
//! architectures because the core code paths are identical.
//!
//! Benchmarks themselves are not part of this crate yet — they follow in
//! F-074-N3. The conventions for benchmark IDs and fixtures are described in
//! [`bench/README.md`](../bench/README.md).
