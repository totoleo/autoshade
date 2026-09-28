// One part of the engine's tests (src/render/tests.rs includes it): bitmap gating, the radial falloff and its table, mask composition, morphing, guided refine and bounded rasters.

#[test]
fn bitmap_masks_gate_by_the_raster_and_fail_inert() {
    use crate::recipe::{LocalAdjustment, MaskGeometry};
    // A left-white / right-black raster driving an exposure-up local mask:
    // the white half must brighten vs a control render through the SAME
    // pipeline, the black half must stay byte-identical to the control.
    std::fs::create_dir_all("out").ok();
    let mask_p = "out/_bitmap_mask.png";
    image::GrayImage::from_fn(40, 20, |x, _| image::Luma([if x < 20 { 255u8 } else { 0 }]))
        .save(mask_p)
        .unwrap();
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 20, image::Rgb([100, 100, 100])));
    let control = develop_preview(&base, &EditRecipe::default()).to_rgb8();
    let masked = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: mask_p.into() },
            exposure_ev: 1.5,
            ..Default::default()
        }],
        ..Default::default()
    };
    let out = develop_preview(&base, &masked).to_rgb8();
    let (white_side, ctrl_w) = (out.get_pixel(5, 10)[0], control.get_pixel(5, 10)[0]);
    let (black_side, ctrl_b) = (out.get_pixel(35, 10)[0], control.get_pixel(35, 10)[0]);
    assert!(
        white_side as i32 > ctrl_w as i32 + 25,
        "white half must brighten: {white_side} vs control {ctrl_w}"
    );
    assert_eq!(black_side, ctrl_b, "black half must be untouched by the mask");

    // A missing raster renders the mask INERT (weight 0, stderr warning),
    // never a crash and never a stuck full-frame adjustment.
    let missing = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: "out/_no_such_mask_xyz.png".into() },
            exposure_ev: 1.5,
            ..Default::default()
        }],
        ..Default::default()
    };
    let inert = develop_preview(&base, &missing).to_rgb8();
    assert_eq!(inert.get_pixel(5, 10)[0], ctrl_w, "missing raster ⇒ mask inert");
    assert_eq!(inert.get_pixel(35, 10)[0], ctrl_b);
}

/// R29-1 acceptance ②: the PURE-PIXEL preview arm carries typed identity —
/// or, here, typed ABSENCE of it.
///
/// `develop_preview` is handed a buffer, a width and a height. Under R28
/// Batch-5 5c its mask-raster loader was passed a bare `None` and the
/// resulting stderr line named nothing, with the reason living only in a
/// comment; the registration called it "the residue of 5c". The disclosure
/// now arrives as a `diag::Line` whose subject IS `Subject::PixelOnly` —
/// a state a sink can match on — and the injected form lets a caller that
/// DOES know the photograph say so instead.
#[test]
fn the_pure_pixel_preview_arm_states_that_it_has_no_photograph() {
    use crate::diag::{Collector, Diag, Subject};
    use crate::recipe::MaskGeometry;
    let base =
        DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, image::Rgb([120, 120, 120])));
    // A path nothing else in the suite touches: `load_mask_bitmap` caches
    // its negative result per (path, mtime), so a shared fixture would let
    // a neighbouring test's first hit swallow the line under test.
    let dead = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap {
                path: "out/_r29_pixel_only_probe_no_such_mask.png".into(),
            },
            exposure_ev: 1.5,
            ..Default::default()
        }],
        ..Default::default()
    };
    let sink = Collector::new();
    let _ = develop_preview_with(&base, &dead, &Diag::pixels_only(&sink));
    let lines = sink.take();
    assert_eq!(lines.len(), 1, "the dead raster must disclose exactly once: {lines:?}");
    assert_eq!(
        lines[0].subject,
        Subject::PixelOnly,
        "the preview arm must state that it has no photograph, not pass a bare None"
    );
    assert!(
        lines[0].text.contains("could not be loaded"),
        "unexpected line: {}",
        lines[0].text
    );
    // …and the shipped rendering of a PixelOnly line carries no stem, which
    // is what this arm has always printed.
    assert!(
        lines[0].shipped().starts_with("⚠ bitmap mask '"),
        "PixelOnly must render without an attribution: {}",
        lines[0].shipped()
    );
}

#[test]
fn missing_bitmap_raster_is_inert_even_when_inverted() {
    use crate::recipe::MaskGeometry;
    // An unloadable raster carries no coverage, so `inverted` must NOT turn
    // its zero weight into full-frame coverage — render AND overlay have to
    // match a no-mask control for both inversion states.
    let base =
        DynamicImage::ImageRgb8(RgbImage::from_pixel(24, 16, image::Rgb([100, 100, 100])));
    let control = develop_preview(&base, &EditRecipe::default()).to_rgb8();
    for inverted in [false, true] {
        let adj = LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: "out/_no_such_mask_inverted.png".into() },
            exposure_ev: 2.0,
            saturation: -100.0,
            inverted,
            ..Default::default()
        };
        let r = EditRecipe { masks: vec![adj.clone()], ..Default::default() };
        let out = develop_preview(&base, &r).to_rgb8();
        assert_eq!(
            out.as_raw(),
            control.as_raw(),
            "missing raster (inverted={inverted}) must render byte-identically to no mask"
        );
        let cov = mask_coverage(&adj, &base, MaskFrame::AsRendered);
        assert!(
            cov.as_raw().iter().all(|&v| v == 0),
            "overlay must show zero coverage (inverted={inverted})"
        );
    }
}

#[test]
fn radial_feather_zero_stays_finite_on_the_boundary() {
    use crate::recipe::MaskGeometry;
    // Full-frame radial, feather 0: the normalised distance is EXACTLY 1.0
    // at (0.0, 0.5), where the unguarded smoothstep divided 0/0 → NaN.
    let g = MaskGeometry::Radial {
        top: 0.0,
        left: 0.0,
        bottom: 1.0,
        right: 1.0,
        feather: 0.0,
        roundness: 0.0,
        flipped: false,
        angle: 0.0,
        midpoint: 50.0,
        mask_version: 2,
    };
    assert_eq!(mask_weight(&g, 0.0, 0.5, None), 0.0, "hard edge: boundary is outside");
    assert_eq!(mask_weight(&g, 0.5, 0.5, None), 1.0, "hard edge: centre is inside");
    for i in 0..=40 {
        for j in 0..=40 {
            let w = mask_weight(&g, i as f32 / 40.0, j as f32 / 40.0, None);
            assert!(w.is_finite() && (0.0..=1.0).contains(&w), "weight {w} at ({i},{j})");
        }
    }
    // User-visible symptom: a NaN weight blends to NaN and casts to 0 (a
    // black pixel). 40×20 puts pixel (0,10) exactly on the boundary.
    let base =
        DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 20, image::Rgb([120, 120, 120])));
    let r = EditRecipe {
        masks: vec![LocalAdjustment { mask: g, exposure_ev: 1.0, ..Default::default() }],
        ..Default::default()
    };
    let out = develop_preview(&base, &r).to_rgb8();
    assert!(
        out.as_raw().iter().all(|&v| v > 100),
        "feather 0 must brighten or leave pixels alone, never produce black"
    );
}

/// The α(ρ) table published as `b7-analysis-2.md` §3.1 and, for the three
/// rungs me3 added, `me3-a-report.md`'s dense profiles — asserted against
/// what `radial_falloff` actually renders. That is the whole point of the
/// R29 Batch-7-2 landing and of me3's insertion on top of it, and the pin
/// that makes the LUT a MEASUREMENT rather than 3190 numbers nobody can
/// trace.
///
/// These rows are the reports' own printed tables (green channel, Δρ = 0.005
/// bins of ≥400 px, normalised by the f = 0 interior), transcribed here for
/// all eleven measured feather rungs: eight from `b7-analysis-2.md` §3.1,
/// and f = 15/35/65 from me3's `a_05_dense` (printed in
/// `scripts-archive/me3-a/a_all_outputs.log`), which reproduces the eight
/// B7-2 columns bit for bit where the two overlap — checked here, since
/// those eight values are the ones this test already asserted. The f = 0
/// column is deliberately NOT among them: it is analytic here, and
/// `radial_feather_zero_is_an_exact_hard_edge` owns it.
///
/// TOLERANCE, and where it comes from: 0.0025, the cost of the two
/// conditionings `radial_falloff` documents. Eleven of these 187 published
/// values exceed 1.0 (up to 1.0023 — the α calibration's own overshoot) and
/// are clamped; the rest sit within 0.0013, which is the ρ-monotone
/// regression flattening the f = 1 column's noisy near-unity plateau. It is
/// NOT a slack budget: at 0.0025 a one-row or one-column shift of the table
/// fails by two orders of magnitude.
///
/// MUTATION THIS CATCHES: shift any column by one rung, transpose the two
/// interpolation axes, drop the `B0` normalisation, or regenerate the table
/// off a different ρ grid — every one of them moves rows here by ≫ 0.0025.
#[test]
fn the_radial_falloff_reproduces_the_measured_alpha_table() {
    // Columns f = 1 / 5 / 10 / 15 / 25 / 35 / 50 / 65 / 75 / 90 / 100.
    #[rustfmt::skip]
    const PUBLISHED: [(f32, [f32; 11]); 17] = [
        (0.10, [1.0022, 1.0022, 1.0022, 1.0022, 1.0022, 1.0023, 1.0015, 0.9816, 0.9685, 0.9491, 0.9361]),
        (0.20, [0.9987, 0.9986, 0.9987, 0.9987, 0.9986, 0.9977, 0.9920, 0.9430, 0.9107, 0.8626, 0.8307]),
        (0.30, [0.9987, 0.9986, 0.9986, 0.9986, 0.9977, 0.9917, 0.9770, 0.9004, 0.8499, 0.7749, 0.7254]),
        (0.40, [0.9992, 0.9991, 0.9989, 0.9985, 0.9941, 0.9750, 0.9477, 0.8481, 0.7825, 0.6855, 0.6214]),
        (0.50, [0.9990, 0.9987, 0.9979, 0.9962, 0.9788, 0.9338, 0.8899, 0.7771, 0.7027, 0.5927, 0.5201]),
        (0.60, [0.9998, 0.9989, 0.9965, 0.9905, 0.9390, 0.8545, 0.7893, 0.6782, 0.6051, 0.4963, 0.4242]),
        (0.70, [1.0003, 0.9984, 0.9931, 0.9714, 0.8540, 0.7279, 0.6419, 0.5490, 0.4874, 0.3958, 0.3357]),
        (0.80, [1.0010, 0.9979, 0.9827, 0.9061, 0.7041, 0.5624, 0.4719, 0.4063, 0.3629, 0.2987, 0.2560]),
        (0.90, [1.0012, 0.9947, 0.8842, 0.6923, 0.4789, 0.3784, 0.3156, 0.2764, 0.2504, 0.2110, 0.1847]),
        (0.95, [1.0008, 0.9429, 0.6680, 0.4816, 0.3443, 0.2882, 0.2494, 0.2204, 0.2010, 0.1719, 0.1523]),
        (1.00, [0.3718, 0.2448, 0.2226, 0.2159, 0.2096, 0.2037, 0.1919, 0.1713, 0.1575, 0.1368, 0.1229]),
        (1.05, [0.0002, 0.0027, 0.0226, 0.0591, 0.1083, 0.1336, 0.1433, 0.1292, 0.1198, 0.1056, 0.0962]),
        (1.10, [0.0001, 0.0007, 0.0048, 0.0173, 0.0535, 0.0838, 0.1040, 0.0946, 0.0884, 0.0790, 0.0728]),
        (1.20, [0.0001, 0.0004, 0.0011, 0.0034, 0.0129, 0.0285, 0.0482, 0.0441, 0.0415, 0.0374, 0.0347]),
        (1.30, [0.0000, 0.0002, 0.0004, 0.0009, 0.0035, 0.0080, 0.0167, 0.0148, 0.0134, 0.0115, 0.0101]),
        (1.40, [0.0000, 0.0000, 0.0001, 0.0001, 0.0003, 0.0007, 0.0016, 0.0012, 0.0009, 0.0005, 0.0002]),
        (1.45, [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000]),
    ];
    let mut worst = 0.0f32;
    for (rho, row) in PUBLISHED {
        for (col, want) in RADIAL_FALLOFF_F.iter().zip(row) {
            let got = radial_falloff(col / 100.0, rho);
            let dev = (got - want).abs();
            worst = worst.max(dev);
            assert!(
                dev <= 0.0025,
                "feather {col} at ρ = {rho}: rendered {got}, §3.1 says {want} \
                 (off by {dev})"
            );
        }
    }
    // A floor as well as a ceiling: if the table ever became EXACT the
    // conditioning would have been silently dropped, and with it the
    // monotonicity the renderer relies on.
    assert!(worst > 0.001, "the documented clamp/regression cost vanished: {worst}");
}

/// The constant on disk IS the generated artefact, entry for entry — the
/// drift gate that came in with me3's f = 15/35/65 columns.
///
/// The measurement test above is a TOLERANCE check (0.0025) sampled at 17 ρ
/// values: it cannot see one entry drifting by a few thousandths, it never
/// looks at the 273 rows in between, and it reads the feather ladder rather
/// than pinning it. This one hashes every `f32` in the table and in the
/// ladder, so any edit to any of the 3190 entries fails — inserted column
/// or carried-over one.
///
/// The digests are FNV-1a-64 over each value's little-endian `to_bits()`,
/// row-major, computed from the generator's own output file
/// `…/r29-materials/scripts-archive/me3-a/cache-out/RADIAL_FALLOFF_11col.rs.txt`
/// (27 725 B, sha256 `cd993fc7d73cf6f5302bcfaa8384f0325e51344ac9d0ee6af93b9a5d118ad7bb`,
/// written by `a_09_table.py`). The eight B7-2 columns inside that file are
/// the previously shipped ones bit for bit (`a_09`: max |Δ| = 0.000000 over
/// their 2320 entries), so this pins B7-2's landing as well as me3's.
///
/// MUTATION THIS CATCHES: change one value in an inserted column — which
/// the tolerance test above cannot — or reorder, shift or mistype the
/// feather ladder, or regenerate the table from a different analysis run.
#[test]
fn the_radial_falloff_table_is_the_generated_artefact() {
    fn fnv1a64(values: impl IntoIterator<Item = f32>) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        for v in values {
            for byte in v.to_bits().to_le_bytes() {
                h = (h ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }
    assert_eq!(
        fnv1a64(RADIAL_FALLOFF.iter().flatten().copied()),
        0x5b52_0111_2f58_37c5,
        "RADIAL_FALLOFF is no longer the table `a_09_table.py` generated"
    );
    assert_eq!(
        fnv1a64(RADIAL_FALLOFF_F),
        0x00a9_21c9_2f45_e96f,
        "the feather ladder is no longer [1, 5, 10, 15, 25, 35, 50, 65, 75, 90, 100]"
    );
}

/// The requirement that decided the shape of this landing: the LUT must be
/// better than the law it replaces on EVERY feather rung, not just the wide
/// ones.
///
/// `1 − smoothstep(1 − f, 1 + f/2, d)` was already CORRECT for f ≤ 5 —
/// rms(α) 0.009–0.010 against the measurement, α ≥ 0.5 area within 0.5 %
/// (`b7-analysis-2.md` §6) — so a replacement fitted only where the gap was
/// visible could have shipped a REGRESSION at the narrow end and nobody
/// would have noticed. Scored on §3.1's own seventeen rows the ratio runs
/// 31× at f = 1 and 26× at f = 5, up to 5448× at f = 100; the three rungs
/// me3 inserted score 107× / 187× / 2783× at f = 15/35/65 on the same rows.
///
/// MUTATION THIS CATCHES: putting the ramp back (every rung fails), or
/// building the LUT from a fit that trades the narrow rungs for the wide
/// ones — f = 1 and f = 5 fail first, which is exactly the failure this
/// batch was told to avoid.
#[test]
fn the_radial_falloff_beats_the_refuted_ramp_on_every_feather() {
    #[rustfmt::skip]
    const PUBLISHED: [(f32, [f32; 11]); 8] = [
        (0.30, [0.9987, 0.9986, 0.9986, 0.9986, 0.9977, 0.9917, 0.9770, 0.9004, 0.8499, 0.7749, 0.7254]),
        (0.60, [0.9998, 0.9989, 0.9965, 0.9905, 0.9390, 0.8545, 0.7893, 0.6782, 0.6051, 0.4963, 0.4242]),
        (0.80, [1.0010, 0.9979, 0.9827, 0.9061, 0.7041, 0.5624, 0.4719, 0.4063, 0.3629, 0.2987, 0.2560]),
        (0.95, [1.0008, 0.9429, 0.6680, 0.4816, 0.3443, 0.2882, 0.2494, 0.2204, 0.2010, 0.1719, 0.1523]),
        (1.00, [0.3718, 0.2448, 0.2226, 0.2159, 0.2096, 0.2037, 0.1919, 0.1713, 0.1575, 0.1368, 0.1229]),
        (1.05, [0.0002, 0.0027, 0.0226, 0.0591, 0.1083, 0.1336, 0.1433, 0.1292, 0.1198, 0.1056, 0.0962]),
        (1.20, [0.0001, 0.0004, 0.0011, 0.0034, 0.0129, 0.0285, 0.0482, 0.0441, 0.0415, 0.0374, 0.0347]),
        (1.40, [0.0000, 0.0000, 0.0001, 0.0001, 0.0003, 0.0007, 0.0016, 0.0012, 0.0009, 0.0005, 0.0002]),
    ];
    for (j, col) in RADIAL_FALLOFF_F.iter().enumerate() {
        let f = col / 100.0;
        let (mut lut, mut old) = (0.0f32, 0.0f32);
        for (rho, row) in PUBLISHED {
            lut += (radial_falloff(f, rho) - row[j]).powi(2);
            // The refuted law, spelled out here rather than referenced, so
            // deleting it from the renderer cannot silently weaken this.
            old += (1.0 - ramp(1.0 - f, 1.0 + f / 2.0, rho) - row[j]).powi(2);
        }
        let n = PUBLISHED.len() as f32;
        let (lut, old) = ((lut / n).sqrt(), (old / n).sqrt());
        assert!(
            lut * 10.0 < old,
            "feather {col}: the LUT must beat the refuted ramp by an order of \
             magnitude — rms {lut} against {old}"
        );
    }
    // The two rungs the old law got right, pinned as absolute numbers: a
    // regression here is the one this batch was specifically told to avoid.
    for (col, want) in [(1.0f32, 0.00088f32), (5.0, 0.00055)] {
        let mut acc = 0.0f32;
        for (rho, row) in PUBLISHED {
            acc += (radial_falloff(col / 100.0, rho)
                - row[RADIAL_FALLOFF_F.iter().position(|c| *c == col).unwrap()])
            .powi(2);
        }
        let rms = (acc / PUBLISHED.len() as f32).sqrt();
        assert!(rms < want * 3.0, "feather {col}: rms {rms} against the landed {want}");
    }
}

/// Feather 0 is a HARD EDGE, exactly — the one place this LUT is analytic
/// rather than measured.
///
/// The measured f = 0 column is 0.0084 wide in ρ, but that width is the
/// JPEG-plus-capture-sharpening blur floor (8.7 px on the measured frame's
/// major axis), not Lightroom's edge: at Feather 0 Lightroom draws a step.
/// Using the measured column would have smeared every hard-edged radial by
/// ~9 px, so f = 0 takes its own branch and `d == 1.0` counts as OUTSIDE —
/// byte-identical to the degenerate-`ramp` guard this replaces, which is
/// what keeps `radial_feather_zero_stays_finite_on_the_boundary` and the
/// four polarity cells green without re-pinning.
///
/// MUTATION THIS CATCHES: drop the `f <= 0.0` branch and feather 0 reads
/// the f = 1 column instead — 0.372 on the boundary rather than 0, and a
/// soft rim on every hard radial.
#[test]
fn radial_feather_zero_is_an_exact_hard_edge() {
    for d in [0.0f32, 0.5, 0.9, 0.99, 0.999, 0.9999] {
        assert_eq!(radial_falloff(0.0, d), 1.0, "solid inside the ellipse at d = {d}");
    }
    for d in [1.0f32, 1.0001, 1.01, 1.5, 3.0] {
        assert_eq!(radial_falloff(0.0, d), 0.0, "nothing at or outside d = {d}");
    }
    // …and it is a LIMIT, not a cliff: the family stays continuous in
    // feather across the analytic/measured seam. Lightroom only ever writes
    // whole feather units, but this app's own slider is continuous, so a
    // discontinuity here would be a visible ring nobody asked for.
    for d in [0.97f32, 0.99, 1.0, 1.02, 1.05] {
        let (a, b) = (radial_falloff(0.0001, d), radial_falloff(0.0002, d));
        assert!(
            (a - b).abs() < 0.02,
            "feather 0.01 → 0.02 must not jump at d = {d}: {a} vs {b}"
        );
    }
}

/// The four structural properties the measurement establishes independently
/// of any curve fit, asserted on the shipped table rather than on the
/// report: α(0) = 1 at every feather, zero past the support, non-increasing
/// in ρ, and non-increasing in feather INSIDE the ellipse.
///
/// Each one has its own provenance. α(0) = 1 is measured, not normalised —
/// mask centres are pixel-identical to the feather-0 frame on all eight
/// rungs (`b7-analysis-2.md` §3.4), which is what refuted the free-endpoint
/// refit's `d_in = −0.228` at f = 100. The support is baked into the column
/// tails deliberately, with no `d_out` constant anywhere in this file: B7's
/// `1.4335 ± 0.002` was measuring JPEG 8×8 block spill (§3.3), me3's four
/// shape-free instruments put the value at √2 and exclude both 1.43 and
/// 1.4335 (`me3-a-report.md` §0-Q2), and the tails carry it either way.
///
/// The feather monotonicity is asserted only for ρ < 1, and that scope is
/// the measurement's: OUTSIDE the ellipse the order genuinely reverses (more
/// feather reaches further), and the f = 50 tail is measurably FATTER than
/// f = 75's and f = 100's out there — a real non-monotonicity reproduced
/// independently in the raw DN profile (B7 §3.1). Asserting it globally
/// would be asserting a tidiness the data does not have.
///
/// MUTATION THIS CATCHES: drop the pool-adjacent-violators pass and the ρ
/// sweep fails on the noisy near-unity plateaux; drop the running-minimum
/// pass and the feather sweep fails; let any column tail short of zero and
/// the support check fails.
#[test]
fn the_radial_falloff_holds_its_structural_invariants() {
    let feathers: Vec<f32> = (0..=200).map(|i| i as f32 / 200.0).collect();
    for &f in &feathers {
        assert_eq!(radial_falloff(f, 0.0), 1.0, "α(0) must be 1 at feather {f}");
        // 1.42 / 1.43 / 1.44 are INSIDE the table — every column has already
        // reached zero by ρ = 1.4175 — so these exercise the tail itself and
        // not the past-the-end early return. Both matter: a tail that never
        // quite reaches zero paints the whole frame at 0.2 %, which is
        // invisible in a preview and wrong in an export.
        for d in [1.42f32, 1.43, 1.44, 1.45, 1.5, 2.0, 10.0] {
            assert_eq!(radial_falloff(f, d), 0.0, "feather {f} must be spent by d = {d}");
        }
        // Non-increasing in ρ, swept finer than the table's own rows so an
        // interpolation bug shows up too.
        let mut prev = f32::INFINITY;
        for i in 0..=1500 {
            let a = radial_falloff(f, i as f32 / 1000.0);
            assert!((0.0..=1.0).contains(&a), "α = {a} out of range at feather {f}");
            assert!(a <= prev + 1e-6, "feather {f} rises at ρ = {}: {prev} → {a}", i as f32 / 1000.0);
            prev = a;
        }
    }
    // Non-increasing in feather, INSIDE the ellipse only (see above).
    for i in 0..100 {
        let rho = i as f32 / 100.0;
        let mut prev = f32::INFINITY;
        for &f in &feathers {
            let a = radial_falloff(f, rho);
            assert!(a <= prev + 1e-6, "ρ = {rho} rises at feather {f}: {prev} → {a}");
            prev = a;
        }
    }
    // The reverse, outside: at ρ = 1.2 more feather really does reach
    // further, which is why the sweep above stops at the ellipse.
    assert!(
        radial_falloff(0.25, 1.2) > radial_falloff(0.10, 1.2) * 5.0,
        "the outer branch must GROW with feather"
    );
    // A non-finite sample point must not become a non-finite WEIGHT: NaN
    // casts to row 0, blends to NaN, survives the `wgt <= 0.001` early-out
    // and lands as a black pixel. The old degenerate-`ramp` guard existed
    // for the 0/0 half of this; the LUT has no division to go degenerate,
    // so the guard moved to the input.
    for f in [0.0f32, 0.005, 0.5, 1.0] {
        for d in [f32::NAN, f32::INFINITY] {
            let w = radial_falloff(f, d);
            assert_eq!(w, 0.0, "feather {f} at d = {d} must be inert, got {w}");
        }
    }
    // The other half, and the one `f32::clamp` gets wrong by propagating:
    // a NaN FEATHER on a perfectly ordinary sample point. It degrades to the
    // hard edge rather than to a NaN — reachable only from a hand-edited
    // `recipe.json`, the same threat model `brush_kernel_exponents` names.
    for d in [0.0f32, 0.5, 0.999, 1.0, 1.2, 2.0] {
        let w = radial_falloff(f32::NAN, d);
        assert!(w.is_finite(), "a NaN feather must not become a NaN weight at d = {d}");
        assert_eq!(w, if d < 1.0 { 1.0 } else { 0.0 }, "…and degrades to the hard edge");
    }
}

/// v0.32.0 — the polarity truth table, all four cells, closed on the pixel.
///
/// This engine spells "which side gets the effect" as the XOR of TWO flags
/// (`Radial::flipped` in `mask_weight`, `LocalAdjustment::inverted` in the
/// weight loop); Lightroom spells it once. Both of Lightroom's observed
/// spellings are rendered here as the flags the importer produces for them,
/// and both were measured on real exports: `crs:Flipped="true"` +
/// `crs:MaskInverted="false"` darkens the ellipse INTERIOR (8 frames,
/// `#6` at +4.4 stops inside), `crs:Flipped="false"` +
/// `crs:MaskInverted="true"` darkens the EXTERIOR (`P23`, +3.40 stops
/// exterior-minus-interior, with the level sets GROWING as the threshold
/// rises). `PROBE2-VERDICT.md` §6.
///
/// The other two cells are engine-only — 201/201 real radials are
/// anti-correlated, so Lightroom writes neither — and they are pinned
/// because the recipe can hold them and a user's Flip checkbox produces
/// one of them.
///
/// MUTATION THIS CATCHES: change either XOR arm (drop the `1.0 - wgt` in
/// `mask_weight`, or the `1.0 - wgt` in the weight loop) and two rows flip.
#[test]
fn the_radial_polarity_truth_table_is_closed_on_all_four_cells() {
    use crate::recipe::MaskGeometry;
    // A small centred ellipse, hard-edged so "inside" and "outside" are
    // unambiguous at the two sample points.
    let g = |flipped: bool| MaskGeometry::Radial {
        top: 0.3,
        left: 0.3,
        bottom: 0.7,
        right: 0.7,
        feather: 0.0,
        roundness: 0.0,
        flipped,
        angle: 0.0,
        midpoint: 50.0,
        mask_version: 2,
    };
    // (flipped, inverted) → does the effect land INSIDE?
    for (flipped, inverted, inside) in [
        // Lightroom's `Flipped="true" MaskInverted="false"`, as the
        // importer reads it (the bit comes from MaskInverted alone).
        (false, false, true),
        // Lightroom's `Flipped="false" MaskInverted="true"` — `P23`.
        (false, true, false),
        // Engine-only, from this app's own Flip checkbox.
        (true, false, false),
        (true, true, true),
    ] {
        let m = LocalAdjustment {
            mask: g(flipped),
            inverted,
            exposure_ev: -3.0,
            ..Default::default()
        };
        let base =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 40, image::Rgb([160, 160, 160])));
        let r = EditRecipe { masks: vec![m], ..Default::default() };
        let out = develop_preview(&base, &r).to_rgb8();
        let centre = out.get_pixel(20, 20)[0];
        let corner = out.get_pixel(1, 1)[0];
        assert!(
            if inside { centre < corner } else { corner < centre },
            "flipped={flipped} inverted={inverted}: expected the effect \
             {} — centre {centre}, corner {corner}",
            if inside { "INSIDE" } else { "OUTSIDE" }
        );
    }
}

#[test]
fn radial_roundness_is_a_documented_no_op() {
    use crate::recipe::MaskGeometry;
    // CONTRACT (see `MaskGeometry::Radial` in recipe.rs): roundness is
    // carried by recipe/XMP/AI schema but NOT rendered. Its DOMAIN is no
    // longer the gap — v0.31.1 measured it as Lightroom's ±100 integer
    // slider (24/24 real radials write a bare signed integer) and both the
    // clamp and the importer's gate moved to that band. R29 B7 then
    // measured what the number DOES at +100 / Feather=0: nothing, to
    // 0.1 px and JPEG noise — so this no-op is Lightroom's measured
    // behaviour there, and carrying the value verbatim stays right.
    // The negatives this loop has always exercised (`-100/-35/-1`) are no
    // longer a guess either: R29 B7-2 measured the Roundness×Feather cross
    // term at +100/Feather=50 and R29 me3-b measured −100 at Feather=0 and
    // the whole-frame Roundness×Feather identity at Feather=50, so the
    // assertions below now pin MEASURED behaviour rather than a documented
    // assumption — the values did not have to change for that. me6-2026-09
    // group D then added the second geometry AND the non-zero tilt (box
    // 0.4 × 0.3 centred (0.6, 0.4), `crs:Angle="30"`, Roundness −100/0/+100
    // crossed with Feather 25/50/75): nine exports, max|Δ| = 0 DN in every
    // triple, pinned from the pack itself in `render::lr_pack::
    // lightroom_and_the_engine_both_draw_one_ellipse_for_every_roundness`.
    // Pinning the no-op so any future falloff-shape implementation lands
    // together with the doc and the XMP round-trip.
    let radial = |roundness: f32| MaskGeometry::Radial {
        top: 0.2,
        left: 0.1,
        bottom: 0.8,
        right: 0.7,
        feather: 0.5,
        roundness,
        flipped: false,
        angle: 0.0,
        midpoint: 50.0,
        mask_version: 2,
    };
    for i in 0..=10 {
        for j in 0..=10 {
            let (nx, ny) = (i as f32 / 10.0, j as f32 / 10.0);
            let base = mask_weight(&radial(0.0), nx, ny, None);
            for r in [-100.0, -35.0, -1.0, 1.0, 35.0, 100.0] {
                assert_eq!(
                    mask_weight(&radial(r), nx, ny, None),
                    base,
                    "roundness {r} must not change the weight at ({nx},{ny})"
                );
            }
        }
    }
}

#[test]
fn mask_components_compose_add_subtract_intersect() {
    use crate::recipe::{LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry};
    // Two orthogonal linear gradients give exact hand-computable weights
    // after the shipped profile: base = Eased(nx) (horizontal ramp),
    // component = Eased(ny) (vertical ramp). The
    // algebra is the MaskCombine doc contract — union without a seam,
    // subtract carves, intersect restricts.
    let with = |mode| LocalAdjustment {
        mask: MaskGeometry::Linear { zero_x: 0.0, zero_y: 0.5, full_x: 1.0, full_y: 0.5 },
        components: vec![MaskComponent {
            inverted: false,
            geometry: MaskGeometry::Linear {
                zero_x: 0.5,
                zero_y: 0.0,
                full_x: 0.5,
                full_y: 1.0,
            },
            mode,
        }],
        ..Default::default()
    };
    for i in 0..=4 {
        for j in 0..=4 {
            let (nx, ny) = (i as f32 / 4.0, j as f32 / 4.0);
            let (b, c) = (
                linear_coverage(nx, LINEAR_FALLOFF),
                linear_coverage(ny, LINEAR_FALLOFF),
            );
            for (mode, want) in [
                (MaskCombine::Add, 1.0 - (1.0 - b) * (1.0 - c)),
                (MaskCombine::Subtract, b * (1.0 - c)),
                (MaskCombine::Intersect, b * c),
            ] {
                let got = combined_mask_weight(&with(mode), nx, ny, None, &[None], None, (1.0, 1.0));
                assert!(
                    (got - want).abs() < 1e-6,
                    "{mode:?} at ({nx},{ny}): got {got}, want {want}"
                );
            }
        }
    }
    // No components = exactly the base geometry (v1 compatibility).
    let plain = LocalAdjustment {
        mask: MaskGeometry::Linear { zero_x: 0.0, zero_y: 0.5, full_x: 1.0, full_y: 0.5 },
        ..Default::default()
    };
    assert_eq!(
        combined_mask_weight(&plain, 0.3, 0.9, None, &[], None, (1.0, 1.0)),
        linear_coverage(0.3, LINEAR_FALLOFF)
    );
    // Components fold IN LIST ORDER: subtract-then-add differs from
    // add-then-subtract, so a reorder is a real semantic change.
    let vertical = MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.5, full_y: 1.0 };
    let sub_then_add = LocalAdjustment {
        mask: MaskGeometry::Linear { zero_x: 0.0, zero_y: 0.5, full_x: 1.0, full_y: 0.5 },
        components: vec![
            MaskComponent { inverted: false, geometry: vertical.clone(), mode: MaskCombine::Subtract },
            MaskComponent { inverted: false, geometry: vertical.clone(), mode: MaskCombine::Add },
        ],
        ..Default::default()
    };
    let (nx, ny) = (0.5, 0.75);
    let want = {
        let b = linear_coverage(nx, LINEAR_FALLOFF);
        let c = linear_coverage(ny, LINEAR_FALLOFF);
        let w = b * (1.0 - c);
        1.0 - (1.0 - w) * (1.0 - c)
    };
    let got = combined_mask_weight(&sub_then_add, nx, ny, None, &[None, None], None, (1.0, 1.0));
    assert!((got - want).abs() < 1e-6, "sequential fold: got {got}, want {want}");
}

/// An inverted component whose raster could not be loaded contributes
/// NOTHING — not the whole frame. `mask_weight_in` answers 0 for a missing
/// raster, and `1 − 0` is coverage everywhere: an inverted Add then
/// covered the frame at full strength and an inverted Subtract wiped the
/// base. A parametric component with no raster is unaffected (it never
/// had one to lose).
#[test]
fn an_inverted_component_without_its_raster_covers_nothing() {
    use crate::recipe::{LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry};
    let base = MaskGeometry::Linear { zero_x: 0.0, zero_y: 0.5, full_x: 1.0, full_y: 0.5 };
    let dead = MaskGeometry::Bitmap { path: "nowhere.png".into() };
    for mode in [MaskCombine::Add, MaskCombine::Subtract, MaskCombine::Intersect] {
        let m = LocalAdjustment {
            mask: base.clone(),
            components: vec![MaskComponent { inverted: true, geometry: dead.clone(), mode }],
            ..Default::default()
        };
        let plain = LocalAdjustment { mask: base.clone(), ..Default::default() };
        for nx in [0.1f32, 0.5, 0.9] {
            let got = combined_mask_weight(&m, nx, 0.5, None, &[None], None, (1.0, 1.0));
            let want = combined_mask_weight(&plain, nx, 0.5, None, &[], None, (1.0, 1.0));
            assert!((got - want).abs() < 1e-6, "{mode:?} at {nx}: got {got}, the base alone is {want}");
        }
    }
}

#[test]
fn morph_mask_grows_and_shrinks_by_the_given_radius() {
    // A single white pixel in an 9×9 black field: dilate(+2) must produce
    // a 5×5 white block (square element), erode(−1) of that block must
    // shrink it back to 3×3, and radius 0 is the identity.
    let mut g = image::GrayImage::new(9, 9);
    g.put_pixel(4, 4, image::Luma([255]));
    let grown = morph_mask(&g, 2);
    let white = |img: &image::GrayImage| {
        img.enumerate_pixels().filter(|(_, _, p)| p[0] == 255).count()
    };
    assert_eq!(white(&grown), 25, "dilate r=2: 5×5 block");
    let shrunk = morph_mask(&grown, -1);
    assert_eq!(white(&shrunk), 9, "erode r=1: back to 3×3");
    assert_eq!(morph_mask(&g, 0), g, "radius 0 is the identity");
    // Erode of the single dot wipes it — no wraparound resurrection.
    assert_eq!(white(&morph_mask(&g, -1)), 0, "erode kills a 1px dot");
}

#[test]
fn refine_mask_guided_snaps_a_soft_boundary_onto_the_guide_edge() {
    // Guide: hard vertical edge (left black, right white) at full res.
    // Mask: LOW-RES version of the same selection whose upsampled
    // boundary would smear across ~8 px. The guided output must be
    // decisively dark left of the edge and bright right of it — i.e.
    // the boundary re-attaches to the guide's edge.
    let (w, h) = (64u32, 32u32);
    let guide = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, _| {
        if x < w / 2 { image::Rgb([10, 10, 10]) } else { image::Rgb([245, 245, 245]) }
    }));
    // 8× smaller mask of the same half-split (boundary lands between
    // texels — the upsample alone leaves a wide gray ramp).
    let small = image::GrayImage::from_fn(w / 8, h / 8, |x, _| {
        if x < w / 16 { image::Luma([0]) } else { image::Luma([255]) }
    });
    let refined = refine_mask_guided(&small, &guide, 4, 1e-4);
    assert_eq!(refined.dimensions(), (w, h), "output at guide resolution");
    // Sample rows away from borders; 4 px from the edge on each side.
    let y = h / 2;
    let far_l = refined.get_pixel(w / 2 - 4, y)[0];
    let far_r = refined.get_pixel(w / 2 + 4, y)[0];
    assert!(
        far_l < 40,
        "left of the guide edge must read as deselected, got {far_l}"
    );
    assert!(
        far_r > 215,
        "right of the guide edge must read as selected, got {far_r}"
    );
    // …and feather_mask is a smoke-checked smoothing: the hard edge
    // gains intermediate values without moving the extremes.
    let feathered = feather_mask(&refined, 2.0);
    assert_eq!(feathered.dimensions(), (w, h));
    assert!(feathered.get_pixel(2, y)[0] < 40 && feathered.get_pixel(w - 3, y)[0] > 215);
}

#[test]
fn a_missing_component_raster_makes_the_whole_adjustment_inert() {
    use crate::recipe::{EditRecipe, LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry};
    // A lost Subtract raster contributes 0 coverage — folding that in
    // would WIDEN the effect area, so the whole adjustment must go inert
    // (apply_masks / mask_coverage) and the strict raster snapshot must
    // refuse the deliverable, exactly like a lost BASE raster.
    let m = LocalAdjustment {
        exposure_ev: 1.0, // engine-active, so the export gate cares
        mask: MaskGeometry::Linear { zero_x: 0.0, zero_y: 0.5, full_x: 1.0, full_y: 0.5 },
        components: vec![MaskComponent {
            inverted: false,
            geometry: MaskGeometry::Bitmap {
                path: "Z:/__autoshade_definitely_missing__/raster.png".into(),
            },
            mode: MaskCombine::Subtract,
        }],
        name: "carved".into(),
        ..Default::default()
    };
    let r = EditRecipe { masks: vec![m], ..Default::default() };
    let err = load_mask_raster_snapshot(&r, &crate::diag::pixels())
        .expect_err("a component raster counts for the deliverable refusal");
    assert!(
        err.to_string().contains("carved"),
        "the refusal names the mask whose edit would be dropped: {err:#}"
    );
    let cov = mask_coverage(&r.masks[0], &DynamicImage::new_rgb8(8, 8), MaskFrame::AsRendered);
    assert!(
        cov.pixels().all(|p| p[0] == 0),
        "the overlay must not advertise coverage the render will not apply"
    );
}

/// L08: the mask list's ⚠ badge — only geometries whose raster cannot
/// load are reported; a readable one is not.
#[test]
fn dead_bitmap_rasters_reports_only_unloadable_geometries() {
    use crate::recipe::{LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry};
    let dir = std::env::temp_dir().join(format!("autoshade-dead-raster-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("good-raster.png");
    image::GrayImage::from_pixel(4, 4, image::Luma([200])).save(&good).unwrap();
    let missing = "Z:/__autoshade_definitely_missing__/raster.png";
    let m = LocalAdjustment {
        exposure_ev: 1.0,
        mask: MaskGeometry::Bitmap { path: good.to_string_lossy().into_owned() },
        components: vec![MaskComponent {
            inverted: false,
            geometry: MaskGeometry::Bitmap { path: missing.into() },
            mode: MaskCombine::Subtract,
        }],
        name: "badge".into(),
        ..Default::default()
    };
    assert_eq!(dead_bitmap_rasters(&m), vec![missing.to_string()]);
    let all_good = LocalAdjustment {
        exposure_ev: 1.0,
        mask: MaskGeometry::Bitmap { path: good.to_string_lossy().into_owned() },
        name: "clean".into(),
        ..Default::default()
    };
    assert!(dead_bitmap_rasters(&all_good).is_empty());
}

/// L02: the bounded mask-decode gate — a header claiming absurd
/// dimensions is refused BEFORE the decoder allocates (the fixture is a
/// header-only PNG with no pixel data: if the gate did not fire first,
/// the decode would fail with a non-budget error and the assert catches
/// the difference). A real small raster passes.
#[test]
fn open_mask_bounded_refuses_oversized_headers() {
    let dir = std::env::temp_dir().join(format!("autoshade-mask-bounded-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("small.png");
    image::GrayImage::from_pixel(4, 4, image::Luma([1])).save(&good).unwrap();
    assert!(open_mask_bounded(&good).is_ok());
    // A PNG signature + one IHDR chunk claiming 100000×100000 (a 40 GB
    // decode) and nothing else. The IHDR CRC must be real — the png
    // reader verifies it before yielding dimensions.
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { 0xEDB8_8320 ^ (crc >> 1) } else { crc >> 1 };
            }
        }
        crc ^ 0xFFFF_FFFF
    }
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(b"IHDR");
    ihdr.extend_from_slice(&100_000u32.to_be_bytes()); // width
    ihdr.extend_from_slice(&100_000u32.to_be_bytes()); // height
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]); // 8-bit greyscale, no interlace
    let mut png: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(&ihdr);
    png.extend_from_slice(&crc32(&ihdr).to_be_bytes());
    // The dimension probe reads chunks up to the first IDAT header —
    // give it an empty IDAT (and IEND) so it succeeds with zero pixel
    // data on disk.
    png.extend_from_slice(&0u32.to_be_bytes());
    png.extend_from_slice(b"IDAT");
    png.extend_from_slice(&crc32(b"IDAT").to_be_bytes());
    png.extend_from_slice(&0u32.to_be_bytes());
    png.extend_from_slice(b"IEND");
    png.extend_from_slice(&crc32(b"IEND").to_be_bytes());
    let huge = dir.join("huge.png");
    std::fs::write(&huge, &png).unwrap();
    let err = open_mask_bounded(&huge).unwrap_err();
    assert!(err.to_string().contains("budget"), "{err}");
    let err = mask_from_memory_bounded(&png).unwrap_err();
    assert!(err.to_string().contains("budget"), "{err}");
}
