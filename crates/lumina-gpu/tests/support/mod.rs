//! Shared support for the adapter-dependent `lumina-gpu` integration tests.
//!
//! The integration tests that actually drive a wgpu adapter are declared with a
//! plain `#[test]` plus the literal gate
//!
//! ```text
//! #[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
//! ```
//!
//! Without the opt-in `gpu-adapter-tests` feature that attribute makes the test
//! `#[ignore]`d, so the default `cargo test -p lumina-gpu` — exactly what CI
//! runs on a GPU-less runner — reports them as **ignored** instead of running
//! them and counting their early `return` as `passed`. The gate is written out
//! literally rather than hidden behind a `macro_rules!` wrapper so that
//! `cargo fmt` stays able to format the test bodies: rustfmt does not descend
//! into a brace-macro invocation carrying a `///` doc-comment token.
//!
//! Enabling the opt-in `gpu-adapter-tests` feature is an explicit claim that a
//! real adapter is available. [`require_adapter`] enforces that claim: it
//! guards the former silent-skip path and panics instead. A GPU-less runner
//! that nevertheless runs the suite (opt-in feature, or `--include-ignored`)
//! therefore fails loudly rather than reporting a vacuous pass — the test could
//! not make its parity assertion false (`DoD.md` §10).
//!
//! Run the full GPU suite locally on a Metal/Vulkan machine with:
//!
//! ```text
//! cargo test -p lumina-gpu --features gpu-adapter-tests
//! ```

use lumina_gpu::GpuContext;

/// Fail loudly if `ctx` has no real GPU adapter bound, and report that it does.
///
/// Used as the guard of the former `if !ctx.is_available() { … return; }`
/// skip path. It *always* panics when no adapter is bound: the adapter-
/// dependent tests are `#[ignore]`d by default, so reaching this path without
/// an adapter means the run was forced (`--include-ignored`) or the opt-in
/// `gpu-adapter-tests` feature was set — both are explicit claims that an
/// adapter exists. Returning `bool` lets the call sit in the existing `if`
/// condition rather than adding a statement per call site; the `if` body below
/// it is kept as a defensive, statically-unreachable loud skip so the skip
/// messages stay documented.
pub fn require_adapter(ctx: &GpuContext) -> bool {
    assert!(
        ctx.is_available(),
        "this test requires a real GPU adapter, but none was bound; \
         run without the `gpu-adapter-tests` feature to have it reported as ignored"
    );
    true
}
