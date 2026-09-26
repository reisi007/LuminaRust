//! AUTO-TONE-CLI-6 clause (9): the `f64` JSON round-trip boundary, named,
//! quantified and pinned — not defined away.
//!
//! This workspace builds `serde_json` 1.0.151 **without** `float_roundtrip`.
//! The serializer writes the shortest exactly-re-readable decimal, so a sidecar's
//! text is always correct; the *parser* loses the last bit, so the loss happens
//! on **load**. Measured over the sliders' domain `[-10, 10]`: **7.45 %** (linear
//! grid, 200,000 samples) and **7.90 %** (deterministic LCG, 200,000 samples) of
//! all `f64` are lossy by exactly 1 ULP; **44 of 240** real auto-tone values
//! from the 40-member fixture family (**18.33 %**) are lossy.
//!
//! The reuse branch of `apply_auto_tone_result` takes the persisted values from
//! the loaded sidecar, so for those values the reused number is not
//! bit-identical to the computed one. The tests below pin exactly that:
//!
//! * a deliberately lossy value is used and its observed outcome is pinned, so
//!   the suite covers the real case instead of hiding it behind a fixture that
//!   happens to be exact,
//! * the drift **converges after one run and does not accumulate**,
//! * the rendered output stays byte-stable across the whole fixture family.
//!
//! The byte goldens in `auto_tone_process.rs` deliberately keep the *exact*
//! fixture — the golden stays an independent quantity, the real lossy case is
//! pinned here. Normative text: `feature/architecture/pipeline.md` § Auto-Tone,
//! subsection „Die JSON-Round-Trip-Grenze der `f64`-Felder“.

use super::*;

/// Members of the family exercised by the convergence and render-stability
/// measurements. The same 40 members were additionally measured out-of-tree
/// through the real `lumina` binary (240 slider values, 44 of them lossy).
const FAMILY_MEMBERS: u32 = 40;

/// The two values the contract names as known-lossy in this workspace, each with
/// the value it is actually read back as. Both were verified against the same
/// `serde_json` build the tests link.
const KNOWN_LOSSY: [(f64, f64); 2] = [
    (-0.20253906249999998, -0.2025390625),
    (0.013476562499999997, 0.013476562499999995),
];

/// The two halves of the mechanism, pinned separately: the **emitted text** is
/// exact (Rust's correctly rounded `str::parse` reads it back bit-for-bit) and
/// the loss is in **serde_json's parser** — exactly 1 ULP. Without this split
/// "serde_json is imprecise" would be the wrong diagnosis, and a later reader
/// might "fix" the serializer instead of the parser.
#[test]
fn the_emitted_text_is_exact_and_the_parser_loses_exactly_one_ulp() {
    for (original, read_back) in KNOWN_LOSSY {
        let text = serde_json::to_string(&original).unwrap();
        assert_eq!(
            parse_exact(&text).to_bits(),
            original.to_bits(),
            "the emitted text {text} must denote the value exactly — the loss is in the parser"
        );
        assert_eq!(
            serde_json_round_trip(original).to_bits(),
            read_back.to_bits(),
            "`{original}` must be read back as {read_back} in this workspace"
        );
        assert_eq!(
            ulp_distance(read_back, original).abs(),
            1,
            "the measured loss is exactly 1 ULP, never more"
        );
    }
}

/// The lossy rate over the sliders' domain, on a deterministic sample. This
/// asserts the *band* and the 1-ULP bound (both exact) rather than a rate, so
/// it cannot turn into a sampling-sensitive flake.
#[test]
fn the_lossy_rate_over_the_slider_domain_is_measured_not_hidden() {
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
    let rate = 100.0 * f64::from(lossy) / f64::from(checked);
    assert!(
        lossy > 0,
        "the boundary must stay visible: 0 lossy of {checked} samples"
    );
    assert_eq!(worst, 1, "every loss is exactly 1 ULP");
    assert!(
        (4.0..12.0).contains(&rate),
        "measured lossy rate {rate:.2}% of {checked} samples is far outside the documented 7.45% \
         (grid) / 7.90% (LCG) band — the build or the sample changed"
    );
}

/// A deliberately **lossy** mirror, seeded by hand with a matching fingerprint,
/// is picked up by the reuse path and pinned to the value it is actually read
/// back as — one ULP away, with the other five mirrors and the fingerprint
/// untouched, and the recipe still fresh afterwards. This is exactly the case
/// the byte-golden fixture avoids.
#[test]
fn a_lossy_mirror_is_reused_one_ulp_away_and_the_rest_is_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "lossy-mirror.png");
    let path = sidecar_path_for(&input);
    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});
    let before = load_sidecar(&path).unwrap().virtual_copies[0]
        .recipe
        .clone();
    let mut document = load_sidecar(&path).unwrap();
    document.virtual_copies[0].recipe.auto_features.auto_blacks = Some(KNOWN_LOSSY[0].0);
    save_sidecar(&path, &document).unwrap();

    process_auto_tone(&input, &directory.path().join("out2.png"), |_| {});

    let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
    let mirrors = mirrored_values(recipe).expect("all six mirrors stay complete");
    assert_eq!(
        mirrors[3].unwrap().to_bits(),
        KNOWN_LOSSY[0].1.to_bits(),
        "the reused lossy mirror is read back as its 1-ULP neighbour"
    );
    assert_eq!(
        ulp_distance(mirrors[3].unwrap(), KNOWN_LOSSY[0].0).abs(),
        1,
        "the observed deviation is exactly 1 ULP"
    );
    for index in [0usize, 1, 2, 4, 5] {
        assert_eq!(
            mirrors[index].unwrap().to_bits(),
            mirrored_values(&before).unwrap()[index].unwrap().to_bits(),
            "mirror `{}` must be reused verbatim — the boundary is a per-value property",
            MIRROR_FIELDS[index]
        );
    }
    assert_eq!(
        recipe.auto_features.analysis_fingerprint, before.auto_features.analysis_fingerprint,
        "the analysis fingerprint does not move"
    );
    assert_eq!(
        recipe.auto_features.enable_auto_tone, before.auto_features.enable_auto_tone,
        "the 1-ULP drift does not disturb the six-slider contract"
    );
    assert!(
        auto_tone_is_fresh(
            recipe,
            &auto_tone_input_fingerprint(&auto_tone_frame(), 0.5)
        ),
        "a 1-ULP drift keeps the recipe fresh — freshness is presence-based, not value-based"
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

/// The reuse path **converges and does not accumulate**: run 1 writes the exact
/// text, run 2 reads it (at most 1 ULP off) and writes that value's shortest
/// representation, and from run 2 on the persisted text is a fixed point of the
/// parser. Measured over the 40-member family (240 slider values).
///
/// The test is **not vacuous**: it asserts that members actually drifted, so the
/// "0 further drift" half cannot pass by never exercising the boundary.
#[test]
fn the_reuse_path_converges_after_one_run_and_does_not_accumulate() {
    let mut values = 0u32;
    let mut first_to_second = 0u32;
    let mut second_to_third = 0u32;
    let mut worst_first = 0i64;
    let mut worst_second = 0i64;
    let mut drifting_members = 0u32;

    for k in 0..FAMILY_MEMBERS {
        let (_, texts) = three_runs(k);
        let mut drifted_here = 0u32;
        for ((first_literal, second_literal), third_literal) in
            texts[0].iter().zip(texts[1].iter()).zip(texts[2].iter())
        {
            let first = parse_exact(first_literal);
            let second = parse_exact(second_literal);
            let third = parse_exact(third_literal);
            values += 1;
            if first.to_bits() != second.to_bits() {
                first_to_second += 1;
                drifted_here += 1;
                worst_first = worst_first.max(ulp_distance(second, first).abs());
            }
            if second.to_bits() != third.to_bits() {
                second_to_third += 1;
                drifted_here += 1;
                worst_second = worst_second.max(ulp_distance(third, second).abs());
            }
        }
        if drifted_here > 0 {
            drifting_members += 1;
        }
    }

    assert_eq!(values, FAMILY_MEMBERS * 6, "240 slider values are measured");
    assert!(
        first_to_second > 0,
        "the family must actually exercise the boundary, otherwise this test proves nothing \
         (measured 0 drifting values)"
    );
    assert!(
        drifting_members >= FAMILY_MEMBERS / 4,
        "only {drifting_members} of {FAMILY_MEMBERS} members drifted — the family no longer \
         covers the lossy case"
    );
    assert_eq!(
        worst_first, 1,
        "run 1 -> run 2 must never exceed 1 ULP (worst measured {worst_first})"
    );
    assert_eq!(
        second_to_third, 0,
        "the drift must not accumulate: run 2 -> run 3 changed {second_to_third} values"
    );
    assert_eq!(worst_second, 0, "run 2 -> run 3 must be a fixed point");
}

/// The rendered output stays byte-stable while the persisted value drifts by
/// 1 ULP. Measured over the **whole** 40-member family (not one example): all 40
/// members produced byte-identical PNGs in all three runs.
///
/// Honest scope: this is an observation over 40 fixtures, not a proof that a
/// 1-ULP exposure can never move a `u8` sample. That is why the byte goldens
/// use a fixture whose values are exact.
#[test]
fn the_rendered_output_stays_byte_stable_while_the_persisted_value_drifts() {
    let mut stable_first = 0u32;
    let mut stable_second = 0u32;
    let mut members = 0u32;
    let mut drifted = 0u32;

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
            drifted += 1;
        }
    }

    assert_eq!(members, FAMILY_MEMBERS);
    assert!(
        drifted > 0,
        "no member drifted, so byte-stability would be vacuous here"
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
