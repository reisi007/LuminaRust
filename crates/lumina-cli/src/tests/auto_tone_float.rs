//! AUTO-TONE-CLI-6 clause (9): the `f64` JSON round-trip boundary, named,
//! quantified and pinned — not defined away.
//!
//! Since `JSON-FLOAT-ROUNDTRIP` (owner decision `JSON-ROUNDTRIP`, 2026-10-02,
//! variant (a)) this workspace builds `serde_json` 1.0.151 **with**
//! `float_roundtrip`. Every `f64` that the serializer writes is read back
//! bit-identically, so the reuse branch of `apply_auto_tone_result` takes
//! values that are identical to what the algorithm computed.
//!
//! The mechanism was **not** assumed but re-measured in this slice:
//!
//! * 200,000 pseudo-random `f64` in `[-10, 10]` (splitmix64) — **15,801
//!   (7.90 %)** lossy by exactly 1 ULP with the feature off, **0** with it on
//!   (throwaway harness, same `serde_json` 1.0.151 the tests link),
//! * 43 tracked JSON documents in the workspace (5236 float literals) —
//!   **0** lossy before and after, because a hand-set slider literal has ≤ 16
//!   significant digits and that class never lost a bit.
//!
//! **History, kept on purpose.** The previous build (feature off, measured
//! 2026-09-26) lost 1 ULP on 7.45 % (linear grid, 200,000 samples) / 7.90 %
//! (LCG) of the slider domain and on **44 of 240** real auto-tone values of
//! the 40-member family. The two constants below were verified lossy in that
//! build; they stay in the suite as the regression corpus, so a reader can see
//! *which* values used to break instead of having to trust that they existed.
//! Reverting the feature turns every test in this file **red** — that is the
//! mutation, and it is what keeps these tests from becoming vacuous.
//!
//! Migration plan (why nothing else in the workspace needed re-recording):
//! `feature/quality/json-float-roundtrip-plan.md`. Normative text:
//! `feature/architecture/pipeline.md`, § „Der `f64`-JSON-Round-Trip ist exakt".

use super::*;

/// Members of the family exercised by the literal-stability and
/// render-stability measurements. The same 40 members were additionally
/// measured out-of-tree through the real `lumina` binary (240 slider values,
/// 44 of them lossy) in the feature-off build of 2026-09-26.
const FAMILY_MEMBERS: u32 = 40;

/// The two values the contract named as known-lossy in the feature-off build,
/// each with the value that build read back for it. They stay in the suite as
/// the regression corpus; both must now round-trip **exactly**.
const FORMERLY_LOSSY: [(f64, f64); 2] = [
    (-0.20253906249999998, -0.2025390625),
    (0.013476562499999997, 0.013476562499999995),
];

/// The two halves of the mechanism, pinned separately — and both halves now
/// agree: the **emitted text** is exact (Rust's correctly rounded `str::parse`
/// reads it back bit-for-bit) **and** `serde_json`'s parser finds the same
/// `f64`. Keeping the split means a later regression is still attributable:
/// if this test fails on the `parse_exact` half the writer broke, if it fails
/// on the `serde_json` half the parser lost a bit again.
#[test]
fn the_emitted_text_and_the_parser_agree_bit_for_bit() {
    for (original, read_back) in FORMERLY_LOSSY {
        let text = serde_json::to_string(&original).unwrap();
        assert_eq!(
            parse_exact(&text).to_bits(),
            original.to_bits(),
            "the emitted text {text} must denote the value exactly — the writer is the shorter \
             half of the mechanism"
        );
        assert_eq!(
            serde_json_round_trip(original).to_bits(),
            original.to_bits(),
            "`{original}` must be read back bit-identically; the feature-off build of 2026-09-26 \
             read it back as {read_back}"
        );
    }
}

/// Over the sliders' domain there is now **no** value that loses a bit. This
/// asserts the *band* (exactly zero) and the 1-ULP bound (never more) rather
/// than a rate, so it cannot turn into a sampling-sensitive flake. The sample
/// is the same splitmix64 draw, seed and size the 2026-09-26 measurement used,
/// so the two runs are directly comparable.
#[test]
fn no_value_in_the_slider_domain_loses_a_bit() {
    let mut state = 0x243F_6A88_85A3_08D3u64; // splitmix64, as in the out-of-tree run
    let mut checked = 0u32;
    let mut lossy = 0u32;
    let mut worst = 0i64;
    for _ in 0..50_000u32 {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let unit = ((z >> 11) as f64) / ((1u64 << 53) as f64) * 2.0 - 1.0;
        let value = unit * 10.0;
        let back = serde_json_round_trip(value);
        checked += 1;
        if back.to_bits() != value.to_bits() {
            lossy += 1;
            worst = worst.max(ulp_distance(back, value).abs());
        }
    }
    assert_eq!(
        lossy, 0,
        "the round-trip must be exact for all {checked} samples (measured {lossy} lossy, worst \
         {worst} ULP) — the feature-off build measured 7.90 % on this same draw"
    );
    assert!(
        worst <= 1,
        "a loss may never exceed 1 ULP even if one ever reappears (worst measured {worst})"
    );
}

/// A **seventeen-digit** mirror, seeded by hand with a matching fingerprint, is
/// picked up by the reuse path and comes back bit-identical — the other five
/// mirrors and the fingerprint are untouched too, and the recipe still fresh
/// afterwards. This is the case the byte-golden fixture deliberately avoids:
/// the value cannot be written with fewer than 17 significant digits, which is
/// the class that lost a bit in the feature-off build.
#[test]
fn a_seventeen_digit_mirror_is_reused_bit_identically() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "seventeen-digit-mirror.png");
    let path = sidecar_path_for(&input);
    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});
    let before = load_sidecar(&path).unwrap().virtual_copies[0]
        .recipe
        .clone();
    let mut document = load_sidecar(&path).unwrap();
    document.virtual_copies[0].recipe.auto_features.auto_blacks = Some(FORMERLY_LOSSY[0].0);
    save_sidecar(&path, &document).unwrap();

    process_auto_tone(&input, &directory.path().join("out2.png"), |_| {});

    let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
    let mirrors = mirrored_values(recipe).expect("all six mirrors stay complete");
    assert_eq!(
        mirrors[3].unwrap().to_bits(),
        FORMERLY_LOSSY[0].0.to_bits(),
        "the reused mirror must be bit-identical to what was written; the feature-off build of \
         2026-09-26 read it back as {}",
        FORMERLY_LOSSY[0].1
    );
    for index in [0usize, 1, 2, 4, 5] {
        assert_eq!(
            mirrors[index].unwrap().to_bits(),
            mirrored_values(&before).unwrap()[index].unwrap().to_bits(),
            "mirror `{}` must be reused verbatim — the round-trip is a per-value property",
            MIRROR_FIELDS[index]
        );
    }
    assert_eq!(
        recipe.auto_features.analysis_fingerprint, before.auto_features.analysis_fingerprint,
        "the analysis fingerprint does not move"
    );
    assert_eq!(
        recipe.auto_features.enable_auto_tone, before.auto_features.enable_auto_tone,
        "exactness does not disturb the six-slider contract"
    );
    assert!(
        auto_tone_is_fresh(
            recipe,
            &auto_tone_input_fingerprint(&auto_tone_frame(), 0.5)
        ),
        "an exact round-trip keeps the recipe fresh — freshness is presence-based, not \
         value-based"
    );
}

/// Runs `process --auto-tone` three times over the same sidecar and returns the
/// three rendered outputs plus the three persisted slider literals.
fn three_runs(k: u32) -> (Vec<Vec<u8>>, [[String; 6]; 3]) {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_family_input(directory.path(), k);
    let path = sidecar_path_for(&input);
    let mut outputs = Vec::new();
    let mut texts: Vec<[String; 6]> = Vec::new();
    for run in 0..3 {
        let output = directory.path().join(format!("run{run}.png"));
        process_auto_tone(&input, &output, |_| {});
        outputs.push(fs::read(&output).unwrap());
        texts.push(slider_literals(&fs::read_to_string(&path).unwrap()));
    }
    (outputs, texts.try_into().unwrap())
}

/// How many significant decimal digits a literal carries. A value whose
/// shortest representation needs **17** of them cannot have its significand
/// held exactly in `f64` (`u64` significand > 2^53), which is precisely the
/// class that lost a bit in the feature-off build — so this is a cheap,
/// independent witness that the family still sits on the boundary and the
/// assertion below is not passing over a fixture that left it.
fn significant_digits(literal: &str) -> usize {
    let trimmed = literal.trim().trim_start_matches('-');
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.trim_start_matches('0').len()
}

/// All three runs write the **same** literals: there is no first run that
/// drifts, no fixed point reached only from run 2 — round-trip exactness means
/// run 1 is already the fixed point. Measured over the 40-member family (240
/// slider values).
///
/// The test is **not vacuous**: it also asserts that the family still contains
/// literals from the lossy class (≥ 17 significant digits), so "no drift"
/// cannot pass by never exercising the boundary.
#[test]
fn three_runs_write_the_same_literals() {
    let mut values = 0u32;
    let mut first_to_second = 0u32;
    let mut second_to_third = 0u32;
    let mut lossy_class_literals = 0u32;

    for k in 0..FAMILY_MEMBERS {
        let (_, texts) = three_runs(k);
        for ((first_literal, second_literal), third_literal) in
            texts[0].iter().zip(texts[1].iter()).zip(texts[2].iter())
        {
            let first = parse_exact(first_literal);
            let second = parse_exact(second_literal);
            let third = parse_exact(third_literal);
            values += 1;
            if significant_digits(first_literal) >= 17 {
                lossy_class_literals += 1;
            }
            if first.to_bits() != second.to_bits() {
                first_to_second += 1;
            }
            if second.to_bits() != third.to_bits() {
                second_to_third += 1;
            }
        }
    }

    assert_eq!(values, FAMILY_MEMBERS * 6, "240 slider values are measured");
    assert_eq!(
        first_to_second, 0,
        "run 1 -> run 2 changed {first_to_second} of {values} values; the feature-off build of \
         2026-09-26 drifted 44 of 240 here"
    );
    assert_eq!(
        second_to_third, 0,
        "the drift must not accumulate: run 2 -> run 3 changed {second_to_third} values"
    );
    assert!(
        lossy_class_literals > 0,
        "no literal in the whole family needs 17 significant digits, so this test would prove \
         nothing ({lossy_class_literals} of {values})"
    );
}

/// The rendered output stays byte-stable **and** the persisted text no longer
/// moves at all. Measured over the **whole** 40-member family (not one
/// example): all 40 members produced byte-identical PNGs in all three runs,
/// with literals equal in all three runs.
///
/// Honest scope: byte-stability of the render is an observation over 40
/// fixtures. `u8` output samples can move for a 1-ULP exposure difference in
/// principle, which is exactly why the literals are now compared too — the
/// render statement is no longer the only thing standing between a drift and a
/// silent golden change.
#[test]
fn the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move() {
    let mut stable_first = 0u32;
    let mut stable_second = 0u32;
    let mut members = 0u32;
    let mut literal_drift = 0u32;
    let mut lossy_class = 0u32;

    for k in 0..FAMILY_MEMBERS {
        let (outputs, texts) = three_runs(k);
        members += 1;
        if outputs[0] == outputs[1] {
            stable_first += 1;
        }
        if outputs[1] == outputs[2] {
            stable_second += 1;
        }
        let per_member: bool = texts[0]
            .iter()
            .zip(texts[1].iter())
            .any(|(first, second)| first != second);
        if per_member {
            literal_drift += 1;
        }
        if texts[0].iter().any(|l| significant_digits(l) >= 17) {
            lossy_class += 1;
        }
    }

    assert_eq!(members, FAMILY_MEMBERS);
    assert_eq!(
        literal_drift, 0,
        "run 1 and run 2 must persist identical text ({literal_drift}/{members} members drifted)"
    );
    assert!(
        lossy_class > 0,
        "no member carries a literal from the lossy class, so the render-stability half would be \
         vacuous here"
    );
    assert_eq!(
        stable_first, members,
        "run 1 and run 2 must render byte-identically ({stable_first}/{members} members)"
    );
    assert_eq!(
        stable_second, members,
        "run 2 and run 3 must render byte-identically ({stable_second}/{members} members)"
    );
}
