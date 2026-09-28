// One part of the engine's tests (src/render/tests.rs includes it): preview white balance, blur and curve identities, sharpening, dehaze, unsharp, clarity and texture and their halos.

#[test]
fn preview_wb_is_live_and_matches_the_shared_stage() {
    // develop_preview must run the SAME apply_recipe_wb as the exports:
    // a warmer Kelvin target raises red vs blue on a grey preview, and a
    // tint-only recipe (temperature_k = None) is NOT a no-op.
    let grey = DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 2, image::Rgb([128, 128, 128])));
    let warm = EditRecipe { temperature_k: Some(8000.0), ..Default::default() };
    let w = develop_preview(&grey, &warm).to_rgb8();
    let p = w.get_pixel(0, 0);
    assert!(p[0] > p[2] + 5, "warm target must warm the preview: {p:?}");

    let tinted = EditRecipe { tint: 60.0, ..Default::default() };
    let t = develop_preview(&grey, &tinted).to_rgb8();
    let q = t.get_pixel(0, 0);
    assert!(q[1] < 126, "positive (magenta) tint must cut green: {q:?}");

    // EQUALITY with the shared stage, not just the right lean: a
    // preview-specific WB with the wrong anchor or magnitude satisfied
    // both directions above while preview and export visibly disagreed
    // (R12). With a neutral develop the preview is exactly
    // to_u8(apply_recipe_wb(pixels)).
    for recipe in [&warm, &tinted] {
        let mut manual = vec![[128.0f32 / 255.0; 3]; 4];
        apply_recipe_wb(&mut manual, recipe);
        let want = [to_u8(manual[0][0]), to_u8(manual[0][1]), to_u8(manual[0][2])];
        let got = develop_preview(&grey, recipe).to_rgb8();
        assert_eq!(
            got.get_pixel(0, 0).0,
            want,
            "preview WB must be the shared apply_recipe_wb stage, exactly"
        );
    }
}

#[test]
fn specular_white_handling_diagnosis() {
    // Push one pixel through the full per-pixel develop (1x1 → spatial ops are
    // no-ops) to learn: is bright near-white "foam" greyed by a render BUG, or
    // only by aggressive recipe values? Run with `--nocapture` to read numbers.
    fn run(px: [f32; 3], r: &EditRecipe) -> [f32; 3] {
        let mut d = vec![px];
        apply_develop_anon(&mut d, 1, 1, r);
        d[0]
    }
    let white = [1.0_f32, 1.0, 1.0];
    let foam = [0.88_f32, 0.93, 1.00]; // sky-lit foam: bright, slightly blue
    let lum = |p: [f32; 3]| 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2];
    let hsv_sat = |p: [f32; 3]| {
        let mx = p[0].max(p[1]).max(p[2]);
        let mn = p[0].min(p[1]).min(p[2]);
        if mx > 1e-4 { (mx - mn) / mx } else { 0.0 }
    };

    // (1) NEUTRAL must preserve white — guards against a standalone render bug.
    let wn = run(white, &EditRecipe::default());
    eprintln!("neutral white -> {wn:?}");
    assert!(wn[0] > 0.99 && wn[1] > 0.99 && wn[2] > 0.99, "neutral greyed white: {wn:?}");

    let (_h, hsl_s, _l) = rgb_to_hsl(foam[0], foam[1], foam[2]);
    eprintln!("foam HSL-sat={hsl_s:.3}  HSV-sat={:.3}", hsv_sat(foam));

    let mut hsl_lum = crate::recipe::Hsl::default();
    hsl_lum.luminance[5] = -60.0; // Blue band
    let blue_lum = EditRecipe { hsl: hsl_lum, ..Default::default() };

    // THE FIX: a Blue-band luminance push must NOT crush near-white foam to
    // grey (chroma ≈ 0.12 → gate ≈ 0.37), yet MUST still darken a genuinely
    // vivid blue (chroma ≈ 0.65 → gate ≈ 1.0). Pre-fix the HSL-`s` gate (s≈1.0)
    // hit foam at full strength and it landed at luma 0.71 (a blue-grey).
    let foam_out = run(foam, &blue_lum);
    let vivid = [0.20_f32, 0.45, 0.85];
    let vivid_out = run(vivid, &blue_lum);
    eprintln!("foam  + blue lum-60 -> {foam_out:?} luma {:.2}", lum(foam_out));
    eprintln!("vivid + blue lum-60 -> {vivid_out:?} luma {:.2}", lum(vivid_out));
    assert!(lum(foam_out) > 0.80, "near-white foam must stay bright, got luma {:.2}", lum(foam_out));
    assert!(lum(vivid_out) < 0.90 * lum(vivid), "vivid blue must still darken (HSL still works)");
}

#[test]
fn box_blur_preserves_uniform_plane() {
    // A flat plane must stay flat (DC preserved) after blurring.
    let (w, h) = (40usize, 30usize);
    let plane = vec![0.4_f32; w * h];
    let blurred = blur_plane(&plane, w, h, 5);
    assert!(blurred.iter().all(|&v| (v - 0.4).abs() < 1e-4));
}

#[test]
fn neutral_recipe_is_near_identity() {
    // All-zero recipe ⇒ no clarity/sat/NR/sharpen, near-identity tone LUT.
    let mut data = vec![[0.2_f32, 0.5, 0.8], [0.9, 0.1, 0.4]];
    let orig = data.clone();
    apply_develop_anon(&mut data, 2, 1, &EditRecipe::default());
    for (a, b) in data.iter().zip(orig.iter()) {
        for c in 0..3 {
            assert!((a[c] - b[c]).abs() < 0.02, "channel drift {} vs {}", a[c], b[c]);
        }
    }
}

#[test]
fn scurve_contrast_pins_ends_and_steepens_midtones() {
    // Positive contrast must keep 0→0 and 1→1 (pinned endpoints), darken a
    // shadow value, brighten a highlight value (the S shape), and stay
    // monotonic — the old linear stretch clipped instead of pinning.
    let lut = build_tone_lut(&EditRecipe { contrast: 80.0, ..Default::default() });
    assert!(sample_lut(&lut, 0.0) < 0.01, "black pinned: {}", sample_lut(&lut, 0.0));
    assert!(sample_lut(&lut, 1.0) > 0.99, "white pinned: {}", sample_lut(&lut, 1.0));
    assert!(sample_lut(&lut, 0.25) < 0.25, "shadow darkened: {}", sample_lut(&lut, 0.25));
    assert!(sample_lut(&lut, 0.75) > 0.75, "highlight brightened: {}", sample_lut(&lut, 0.75));
    let mut prev = -1.0;
    for &y in &lut {
        assert!(y >= prev - 1e-4, "non-monotonic: {y} after {prev}");
        prev = y;
    }
}

#[test]
fn region_tones_target_four_different_zones() {
    // Each region owns a DISTINCT tonal zone, and — the muddy-water fix —
    // highlights/shadows act on the UPPER/LOWER tones and leave the MIDTONES
    // alone (the old wide bands gave highlights 0.6–1.0 authority at v≈0.5–0.65,
    // crushing mid-bright water). Gentle ±30 pushes keep the curve unclamped
    // except very near white.
    let base = build_tone_lut(&EditRecipe::default());
    let d = |r: &EditRecipe, x: f32| sample_lut(&build_tone_lut(r), x) - sample_lut(&base, x);
    let whites = EditRecipe { whites: 30.0, ..Default::default() };
    let highs = EditRecipe { highlights: 30.0, ..Default::default() };
    let shadows = EditRecipe { shadows: 30.0, ..Default::default() };
    let blacks = EditRecipe { blacks: 30.0, ..Default::default() };
    // The fix: neither highlights nor shadows may touch the midtone (0.5).
    assert!(d(&highs, 0.5).abs() < 0.01, "highlights must NOT touch the midtone: {}", d(&highs, 0.5));
    assert!(d(&shadows, 0.5).abs() < 0.01, "shadows must NOT touch the midtone: {}", d(&shadows, 0.5));
    // Each region still owns its zone (upper / white-point / lower / black-point).
    assert!(d(&highs, 0.75) > 0.03, "highlights lift the upper tones: {}", d(&highs, 0.75));
    assert!(d(&whites, 0.92) > 0.03, "whites lift the white point: {}", d(&whites, 0.92));
    assert!(d(&shadows, 0.25) > 0.03, "shadows lift the lower tones: {}", d(&shadows, 0.25));
    assert!(d(&blacks, 0.08) > 0.03, "blacks lift the black point: {}", d(&blacks, 0.08));
    // Differentiation: highlights concentrate BELOW white; whites concentrate AT white.
    assert!(d(&highs, 0.75) > d(&highs, 0.97), "highlights concentrate below the white point");
    assert!(d(&whites, 0.95) > d(&whites, 0.70), "whites concentrate at the white point");
}

#[test]
fn sharpening_raises_local_contrast_at_an_edge() {
    // A vertical edge: sharpening should push the dark side darker / bright
    // side brighter (overshoot), increasing the edge step. The flat ends
    // (outside the ±4 px Gaussian support at the default 1.0 radius) must
    // NOT move — a
    // global pointwise contrast curve also grows the step, and only the
    // flat-field control tells the two apart (U14).
    let (w, h) = (12usize, 1usize);
    let mut data: Vec<[f32; 3]> = (0..w)
        .map(|x| { let v = if x < 6 { 0.3 } else { 0.7 }; [v, v, v] })
        .collect();
    let before = data[6][0] - data[5][0];
    let r = EditRecipe { sharpening: 120.0, ..Default::default() };
    apply_develop_anon(&mut data, w, h, &r);
    let after = data[6][0] - data[5][0];
    assert!(after > before, "edge step {after} should exceed {before}");
    // The unsharp is a LUMA op moving all channels by one amount — a
    // grey edge must stay grey. Every probe above reads channel 0, so a
    // red-only sharpen (chromatic halos, unsharpened green/blue) passed
    // (R12).
    for p in [&data[5], &data[6]] {
        assert!(
            (p[0] - p[1]).abs() < 1e-6 && (p[1] - p[2]).abs() < 1e-6,
            "sharpened grey must stay grey: {p:?}"
        );
    }
    for x in [0usize, 1, 10, 11] {
        let want = if x < 6 { 0.3 } else { 0.7 };
        assert!(
            (data[x][0] - want).abs() < 1e-6,
            "flat field at x={x} must not move: {} vs {want}",
            data[x][0]
        );
    }
}

// ---- dehaze -----------------------------------------------------------

/// A colourful test frame under a synthetic atmospheric veil built with
/// the actual scattering physics — `I = J·t + A·(1−t)` in LINEAR light
/// (haze is additive in radiance, not in gamma): t=0.55, airlight 0.9.
/// Lifted black, compressed contrast, desaturated.
fn hazy_frame() -> (Vec<[f32; 3]>, usize, usize) {
    let (w, h) = (64usize, 32usize);
    let (t0, a0) = (0.55f32, 0.90f32);
    let mut data = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let l = x as f32 / (w - 1) as f32;
            let p = match y * 4 / h {
                0 => [l, l, l],
                1 => [l, l * 0.6, l * 0.2],
                2 => [l * 0.2, l * 0.7, l],
                _ => [l * 0.3, l, l * 0.4],
            };
            data.push(p.map(|c| {
                linear_to_srgb(srgb_to_linear(c) * t0 + a0 * (1.0 - t0))
            }));
        }
    }
    (data, w, h)
}

fn mean_chroma(px: &[[f32; 3]]) -> f32 {
    px.iter().map(|p| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])).sum::<f32>()
        / px.len() as f32
}

fn luma_quantile_spread(px: &[[f32; 3]]) -> f32 {
    let mut lum: Vec<f32> = px.iter().map(luma601).collect();
    lum.sort_by(f32::total_cmp);
    lum[(lum.len() * 9) / 10] - lum[lum.len() / 10]
}

#[test]
fn dehaze_zero_is_exact_identity() {
    // Like neutral_recipe_is_near_identity but bit-exact: the stage is
    // gated on != 0.0 and must not run at all.
    let (data, w, _) = hazy_frame();
    let mut out = data.clone();
    apply_dehaze(&mut out, w, 0.0);
    assert_eq!(data, out, "dehaze 0 must be a bit-exact no-op");
}

#[test]
fn dehaze_positive_recovers_a_hazy_ramp() {
    // Haze removal must jointly deepen tone AND restore chroma — the
    // signature no combination of tone sliders (luma-preserving chroma)
    // reproduces.
    let (mut data, w, _) = hazy_frame();
    let spread0 = luma_quantile_spread(&data);
    let chroma0 = mean_chroma(&data);
    apply_dehaze(&mut data, w, 50.0);
    let spread1 = luma_quantile_spread(&data);
    let chroma1 = mean_chroma(&data);
    assert!(
        spread1 > spread0 * 1.15,
        "q90-q10 luma spread must grow ≥15%: {spread0:.3} → {spread1:.3}"
    );
    assert!(chroma1 > chroma0 * 1.10, "mean chroma must grow: {chroma0:.3} → {chroma1:.3}");
}

#[test]
fn dehaze_protects_bright_sky_channel_order() {
    // A bright pale-blue sky pixel near the airlight sits at the model's
    // fixed point: strong dehaze must not blow it out, flip its channel
    // order (no magenta/cyan inversions), or move it far.
    let (mut data, w, _) = hazy_frame();
    let sky = [0.80f32, 0.85, 0.92];
    data[w / 2] = sky;
    apply_dehaze(&mut data, w, 75.0);
    let p = data[w / 2];
    assert!(p[2] > p[1] && p[1] > p[0], "channel order flipped: {p:?}");
    assert!(p[2] < 0.999, "sky blew out: {p:?}");
    let moved = (0..3).map(|c| (p[c] - sky[c]).abs()).fold(0.0f32, f32::max);
    assert!(moved < 0.15, "near-airlight pixel moved {moved:.3}: {p:?}");
}

#[test]
fn dehaze_negative_adds_a_veil_without_clipping() {
    // Adding haze is a convex blend toward the airlight: black lifts,
    // chroma drops, a neutral ramp stays strictly monotone, nothing clips.
    let w = 64usize;
    let mut data: Vec<[f32; 3]> = (0..w)
        .map(|x| {
            let v = x as f32 / (w - 1) as f32;
            [v, v, v]
        })
        .collect();
    let (mut colour, cw, _) = hazy_frame();
    let chroma0 = mean_chroma(&colour);
    apply_dehaze(&mut colour, cw, -50.0);
    assert!(mean_chroma(&colour) < chroma0, "a veil must desaturate");
    apply_dehaze(&mut data, w, -50.0);
    assert!(data[0][0] > 0.05, "black point must lift under a veil: {}", data[0][0]);
    for i in 1..w {
        assert!(
            data[i][0] > data[i - 1][0],
            "veiled ramp must stay strictly increasing at {i}"
        );
    }
    for p in &data {
        assert!(p[0] < 1.0 && p[0] > 0.0, "veil must not clip: {p:?}");
    }
}

#[test]
fn dehaze_is_gentle_on_a_clean_image() {
    // On an already-clean frame (deep blacks present → low airlight-relative
    // haze density on colourful pixels) positive dehaze must be a light
    // touch, not a re-grade: a saturated midtone probe barely moves.
    let (w, h) = (64usize, 4usize);
    let mut data = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let l = x as f32 / (w - 1) as f32;
            data.push(if y == 0 { [l, l, l] } else { [l, l * 0.5, l * 0.15] });
        }
    }
    let probe_idx = w + w / 2; // saturated orange, mid ramp
    let before = data[probe_idx];
    apply_dehaze(&mut data, w, 50.0);
    let after = data[probe_idx];
    let moved = (0..3).map(|c| (after[c] - before[c]).abs()).fold(0.0f32, f32::max);
    assert!(moved < 0.05, "clean saturated midtone moved {moved:.3}: {before:?} → {after:?}");
}

#[test]
fn dehaze_airlight_does_not_phase_lock_to_any_small_period() {
    // 1024×512 = 524288 px → stride 2, i.e. the sampler sees half the
    // frame. Two periodic frames caught two different lock-ups: COLUMN
    // stripes locked a flat `step_by` sampler to one parity (U14), and a
    // CHECKERBOARD locks a +1-per-row shear to one diagonal phase (R12).
    // In both cases a one-pixel shift flipped the estimated airlight
    // between the 0.10 floor and the bright bin, so preview and export
    // disagreed on the same photo.
    //
    // The probe must be a BRIGHT pixel: for a dark one the model's
    // b = a·(1−t) = K·s·min cancels the airlight almost exactly (the two
    // estimates differ by ~4e-4 there, so a dark probe would have passed
    // with the broken sampler — R12).
    let (w, h) = (1024usize, 512usize);
    let stripes = |phase: usize, i: usize| (i % w + phase).is_multiple_of(2);
    let checker = |phase: usize, i: usize| ((i % w) + (i / w) + phase).is_multiple_of(2);
    for (name, pattern) in [
        ("columns", &stripes as &dyn Fn(usize, usize) -> bool),
        ("checkerboard", &checker as &dyn Fn(usize, usize) -> bool),
    ] {
        let frame = |phase: usize| -> Vec<[f32; 3]> {
            (0..w * h)
                .map(|i| {
                    let v = if pattern(phase, i) { 0.05 } else { 0.85 };
                    [v, v, v]
                })
                .collect()
        };
        let mut a = frame(0);
        let mut b = frame(1);
        apply_dehaze(&mut a, w, 50.0);
        apply_dehaze(&mut b, w, 50.0);
        // Value-to-value: the frames are one-pixel-shifted copies, so the
        // same INPUT value must map to the same output in both. Index 1
        // of `a` and index 0 of `b` were both the BRIGHT 0.85.
        let (pa, pb) = (a[1][0], b[0][0]);
        assert!(
            (pa - pb).abs() < 1e-3,
            "{name}: airlight phase-locked to the pattern: {pa} vs {pb}"
        );
    }
}

/// Deterministic synthetic frame for the dehaze golden: a hazy sky band
/// over a colourful ground, generated by pure integer arithmetic so the
/// bytes are identical on every platform and compiler. (Its INPUT bytes;
/// what `apply_dehaze` makes of them is not — see the golden test below,
/// which is why this helper carries the same platform gate.)
#[cfg(all(windows, target_arch = "x86_64"))]
fn dehaze_golden_frame() -> (Vec<[f32; 3]>, usize, usize) {
    let (w, h) = (16usize, 8usize);
    let mut data = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let u = x as f32 / (w - 1) as f32;
            let v = y as f32 / (h - 1) as f32;
            // Top rows: bright, low-contrast veil (the airlight source).
            // Bottom rows: saturated ground with a horizontal ramp.
            data.push(if y < 3 {
                [0.62 + 0.30 * u, 0.66 + 0.28 * u, 0.74 + 0.24 * u]
            } else {
                [0.10 + 0.70 * u, (0.08 + 0.55 * u) * (1.0 - 0.3 * v), 0.06 + 0.40 * u * v]
            });
        }
    }
    (data, w, h)
}

/// FNV-1a over the raw IEEE-754 bits of every channel — a one-`u64`
/// witness that pins the output BIT-exactly (a 0.5-ULP drift changes it).
fn frame_bits_fnv64(data: &[[f32; 3]]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for px in data {
        for c in px {
            for b in c.to_bits().to_le_bytes() {
                hash ^= b as u64;
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
        }
    }
    hash
}

/// PLATFORM-GATED to the box the golden was captured on: Windows,
/// x86_64. The goldens are frozen BITS, and bit-identity was proven
/// against the pre-split implementation ON THAT BOX — it was never
/// promised across libm implementations, and this test is the only thing
/// in the suite that would have to be re-captured to claim it.
///
/// Why the bits move: `apply_dehaze`'s two `powf` sites — the transfer
/// LUTs built in `transfer_luts`, and the airlight histogram in
/// `dehaze_airlight`, which deliberately keeps the exact `powf` — are not
/// correctly rounded, and their last bit differs between the MSVC CRT,
/// glibc and Apple's libm. Evidence from CI run 32398395462, where this
/// test first met a non-Windows runner: ubuntu (x86_64, glibc) reproduced
/// the +60 hash EXACTLY and missed only −40, while macOS (aarch64) missed
/// +60 — so it is the libm, not the word size. And on both runners pixel
/// 0 came back bit-identical to this box ([0.5183644, 0.57828087,
/// 0.68996465] at +60; [0.72724235, 0.75048673, 0.7995405] at −40,
/// re-measured here 2026-08-20), i.e. what drifted is the low bits of a
/// few of the other 127 pixels — not the airlight estimate, which would
/// have moved every pixel including that one.
///
/// What still covers the OTHER platforms, all of them ungated and all in
/// this module: `dehaze_zero_is_exact_identity` (bit-exact, but a no-op
/// so it needs no libm), `dehaze_positive_recovers_a_hazy_ramp`,
/// `dehaze_protects_bright_sky_channel_order`,
/// `dehaze_negative_adds_a_veil_without_clipping`,
/// `dehaze_is_gentle_on_a_clean_image` and
/// `dehaze_airlight_does_not_phase_lock_to_any_small_period` pin the
/// model's behaviour to tolerances a 1-ULP drift cannot break; and
/// `mask_dehaze_renders_only_inside_the_mask` covers the reuse the split
/// existed for. What none of those seven can see, and this one can, is a
/// change to the model far below their tolerances — measured 2026-08-20
/// by mutation: `DEHAZE_K` 0.75 → 0.7500001 (≈1.7 ULP) leaves all seven
/// green and turns this one red. That is the size of a refactor
/// regression, and a refactor is only ever authored on one machine.
#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn dehaze_split_is_bit_identical_to_the_pre_split_golden() {
    // R22 split `apply_dehaze` into `dehaze_airlight` + `dehaze_px` so the
    // MASKED dehaze could reuse the exact same model. These two hashes were
    // captured from the PRE-split implementation on this frame; the split is
    // a refactor only if they still hold bit-for-bit (the tone/chroma
    // assertions in the tests above would survive a 1-ULP drift, this will
    // not). Golden: 2026-08-17, before the split landed.
    for (amount, want) in [(60.0f32, 0xb0ae_c36c_5a0d_6123u64), (-40.0, 0x74e8_603b_e639_28e7)]
    {
        let (mut data, w, _) = dehaze_golden_frame();
        apply_dehaze(&mut data, w, amount);
        assert_eq!(
            frame_bits_fnv64(&data),
            want,
            "dehaze {amount} drifted from the pre-split golden; px0 = {:?}",
            data[0]
        );
    }
}

#[test]
fn unsharp_weighted_at_weight_one_is_bit_identical() {
    // `unsharp_luma` now DELEGATES to `unsharp_luma_weighted` with a
    // constant-1 weight, so comparing the two functions would be circular.
    // The reference here is the original formula written out longhand: the
    // weighted form adds `* wgt`, and float multiplication by exactly 1.0
    // is exact, so weight 1 must reproduce it to the bit.
    let (data, w, h) = detail_frame();
    for (radius, amount, midtone) in [(8usize, 0.5f32, true), (2, -0.35, false)] {
        let mut reference = data.clone();
        {
            let luma: Vec<f32> = reference.iter().map(luma601).collect();
            let blurred = blur_plane(&luma, w, h, radius);
            for (i, px) in reference.iter_mut().enumerate() {
                let l = luma[i];
                let detail = l - blurred[i];
                let m = if midtone { 1.0 - (2.0 * l - 1.0).powi(2) } else { 1.0 };
                let new_l = (l + amount * detail * m).clamp(0.0, 1.0);
                scale_chroma(px, l, new_l);
            }
        }
        let mut weighted = data.clone();
        unsharp_luma_weighted(&mut weighted, w, h, radius, amount, midtone, |_, _, _| 1.0);
        assert_eq!(
            frame_bits_fnv64(&reference),
            frame_bits_fnv64(&weighted),
            "radius {radius} amount {amount} midtone {midtone}: weight-1 must be bit-identical"
        );
        assert_ne!(
            frame_bits_fnv64(&data),
            frame_bits_fnv64(&weighted),
            "the probe frame must actually be sharpened, or the test proves nothing"
        );
    }
}

#[test]
fn mask_clarity_at_full_coverage_equals_global_clarity() {
    // The #15a/#10B fix, pinned at its strongest: a Linear mask whose zero
    // and full points COINCIDE carries weight 1 everywhere (mask_weight's
    // len2 < 1e-9 arm), so at Amount 1 a local Clarity +50 must render
    // EXACTLY what the global Clarity +50 stage renders — same radius model
    // (2% of the short edge, floored at 8 px), same midtone mask, same
    // operator. Bit-exact, measured: 0 differing channels of 9216.
    // Before R22 the local path rendered NOTHING at all.
    let (data, w, h) = detail_frame();
    let mut global = data.clone();
    apply_develop_anon(&mut global, w, h, &EditRecipe { clarity: 50.0, ..Default::default() });
    let mut local = data.clone();
    apply_develop_anon(
        &mut local,
        w,
        h,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.5,
                    zero_y: 0.5,
                    full_x: 0.5,
                    full_y: 0.5,
                },
                amount: 1.0,
                clarity: 50.0,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    assert_ne!(
        frame_bits_fnv64(&data),
        frame_bits_fnv64(&global),
        "global clarity must move this frame, or the comparison is vacuous"
    );
    assert_eq!(
        frame_bits_fnv64(&global),
        frame_bits_fnv64(&local),
        "full-coverage local clarity must equal global clarity bit-for-bit"
    );
}

#[test]
fn mask_texture_at_full_coverage_equals_the_unweighted_operator() {
    // The bare operator at texture's own calibration: small radius
    // (0.5% of the short edge, floored at 2 px) and NO midtone mask —
    // which since R25 B2 is also what the GLOBAL texture stage runs, and
    // `global_texture_at_full_coverage_equals_the_masked_operator` below
    // closes that loop directly.
    // Bit-exact, measured: 0 differing channels of 9216.
    let (data, w, h) = detail_frame();
    let radius = ((0.005 * w.min(h) as f32).round() as usize).max(2);
    assert_eq!(radius, 2, "the 48px short edge must land on the 2px floor");
    let mut reference = data.clone();
    unsharp_luma(&mut reference, w, h, radius, 0.5, false);
    let mut local = data.clone();
    apply_develop_anon(
        &mut local,
        w,
        h,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.5,
                    zero_y: 0.5,
                    full_x: 0.5,
                    full_y: 0.5,
                },
                amount: 1.0,
                texture: 50.0,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    assert_ne!(
        frame_bits_fnv64(&data),
        frame_bits_fnv64(&reference),
        "the reference operator must move this frame, or the comparison is vacuous"
    );
    assert_eq!(
        frame_bits_fnv64(&reference),
        frame_bits_fnv64(&local),
        "full-coverage local texture must equal the unweighted operator bit-for-bit"
    );
}

/// R25 B2: the strongest pin this batch can offer. R22 gave the mask
/// texture slider an operator and had to compare it against a hand-rolled
/// `unsharp_luma` call, because `EditRecipe` had no `texture` field to
/// compare with — the comment on the test above said exactly that. Now it
/// does, and a full-coverage mask must be BIT-IDENTICAL to the global
/// stage: same radius model, same amount scale, same absent midtone mask.
///
/// Change either radius formula and this fails, which is the point: the
/// two are one calibration, not two that happen to agree today.
#[test]
fn global_texture_at_full_coverage_equals_the_masked_operator() {
    let (data, w, h) = detail_frame();
    let mut global = data.clone();
    apply_develop_anon(&mut global, w, h, &EditRecipe { texture: 50.0, ..Default::default() });
    let mut local = data.clone();
    apply_develop_anon(
        &mut local,
        w,
        h,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                // The degenerate Linear gradient every full-coverage
                // comparison in this module uses: weight 1 everywhere.
                mask: MaskGeometry::Linear {
                    zero_x: 0.5,
                    zero_y: 0.5,
                    full_x: 0.5,
                    full_y: 0.5,
                },
                amount: 1.0,
                texture: 50.0,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    assert_ne!(
        frame_bits_fnv64(&data),
        frame_bits_fnv64(&global),
        "global texture must move this frame, or the comparison is vacuous"
    );
    assert_eq!(
        frame_bits_fnv64(&global),
        frame_bits_fnv64(&local),
        "full-coverage local texture must equal global texture bit-for-bit"
    );
}

/// A frame of vertical SINUSOIDAL stripes at `period` px, ±0.06 around mid
/// grey. Sinusoidal and not square: a square wave's edges are broadband, so
/// its peak-to-peak measures every frequency at once and cannot say which
/// BAND a filter took — which is the whole question below. One tone per
/// probe, and the peak-to-peak reads that tone's transfer directly.
///
/// `period` is fractional because two of the acceptance anchors are FFT bin
/// centres (64/3 px and 2.9942 px), not round pixel counts. That costs
/// nothing in accuracy: every kernel here is symmetric and therefore
/// zero-phase, so the filtered samples are the input samples scaled by the
/// transfer and the peak-to-peak RATIO is exact whatever the sampling
/// phases land on.
fn stripe_frame(w: usize, h: usize, period: f32) -> Vec<[f32; 3]> {
    let mut data = Vec::with_capacity(w * h);
    for _ in 0..h {
        for x in 0..w {
            let phase = std::f32::consts::TAU * x as f32 / period;
            let v = 0.5 + 0.06 * phase.sin();
            data.push([v, v, v]);
        }
    }
    data
}

/// The nine acceptance periods, and the closed form's own transfer at each
/// of the five ladder steps — b8-analysis-2 §6-3, the RIGHT half of the
/// table (the left half is the Lightroom measurement, quoted below as
/// ground truth but deliberately NOT asserted: it carries this frame's
/// scene dependence, and the operator is not LTI).
///
/// ```text
///   period  ν c/px   LR −10  −25   −50   −75  −100 ‖ closed −10  −25   −50   −75  −100
///     256  0.00391  0.9993 0.9982 .9965 .9951 .9939 ‖ 0.9987 0.9970 .9947 .9929 .9913
///     128  0.00781  0.9957 0.9897 .9813 .9744 .9689 ‖ 0.9952 0.9890 .9804 .9734 .9678
///      64  0.01562  0.9853 0.9655 .9376 .9151 .8969 ‖ 0.9855 0.9665 .9403 .9192 .9020
///      32  0.03125  0.9762 0.9439 .8986 .8619 .8324 ‖ 0.9743 0.9406 .8941 .8568 .8262
///      21  0.04688  0.9728 0.9359 .8840 .8419 .8081 ‖ 0.9720 0.9350 .8842 .8435 .8100
///      16  0.06250  0.9694 0.9283 .8701 .8231 .7852 ‖ 0.9700 0.9305 .8762 .8326 .7968
///       8  0.12500  0.9611 0.9091 .8358 .7769 .7291 ‖ 0.9590 0.9049 .8306 .7709 .7220
///       4  0.25000  0.9354 0.8533 .7374 .6443 .5695 ‖ 0.9378 0.8558 .7431 .6526 .5783
///       3  0.33398  0.9297 0.8438 .7224 .6254 .5466 ‖ 0.9317 0.8418 .7182 .6188 .5373
/// ```
const TEXTURE_ANCHOR_PERIODS: [f32; 9] =
    [256.0, 128.0, 64.0, 32.0, 64.0 / 3.0, 16.0, 8.0, 4.0, 2.9942];
const TEXTURE_ANCHOR_STEPS: [f32; 5] = [0.10, 0.25, 0.50, 0.75, 1.00];
const TEXTURE_ANCHOR_CLOSED: [[f32; 5]; 9] = [
    [0.9987, 0.9970, 0.9947, 0.9929, 0.9913],
    [0.9952, 0.9890, 0.9804, 0.9734, 0.9678],
    [0.9855, 0.9665, 0.9403, 0.9192, 0.9020],
    [0.9743, 0.9406, 0.8941, 0.8568, 0.8262],
    [0.9720, 0.9350, 0.8842, 0.8435, 0.8100],
    [0.9700, 0.9305, 0.8762, 0.8326, 0.7968],
    [0.9590, 0.9049, 0.8306, 0.7709, 0.7220],
    [0.9378, 0.8558, 0.7431, 0.6526, 0.5783],
    [0.9317, 0.8418, 0.7182, 0.6188, 0.5373],
];

/// Peak-to-peak luma of the middle row, sampled away from the borders so
/// the box blur's clamped edge seeding cannot answer for the interior.
fn stripe_contrast(data: &[[f32; 3]], w: usize, h: usize) -> f32 {
    let row = (h / 2) * w;
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for x in (w / 4)..(3 * w / 4) {
        let l = luma601(&data[row + x]);
        lo = lo.min(l);
        hi = hi.max(l);
    }
    hi - lo
}

/// R29 Batch-8-2 — **THE 45 ACCEPTANCE ANCHORS**, the arbiter of the
/// negative half.
///
/// Nine periods × five ladder steps against the closed form of
/// b8-analysis-2 §6-1, at the σ pair a 6240 × 4160 render raster asks for
/// (σ₁ = 12.9938 px, σ₂ = 1.1740 px) — the raster the ground truth was
/// measured on. The probe is a SYNTHETIC sinusoid, not a photograph: the
/// closed form is an LTI model and only an LTI probe can say whether this
/// implementation realises it. Lightroom's own column is quoted beside it
/// in [`TEXTURE_ANCHOR_CLOSED`]'s doc as ground truth and is NOT asserted —
/// the operator is amplitude-adaptive, so the measured column carries that
/// frame's scene dependence and belongs in a comment, not an assertion.
///
/// **Tolerance ±0.02, and it is a budget, not a round number** (§6-3): the
/// model's own rms residual 0.0048 and max residual 0.0163, the
/// cross-resolution leave-out rms 0.0048, and the JPEG noise bias < 0.0040.
/// **Do not widen it to admit an implementation** — the whole point of the
/// grid is that it rejected three cheaper kernel schemes (the numbers are
/// in [`texture_negative_pass`]'s doc). As shipped the worst anchor sits at
/// 0.0037, a fifth of the budget.
///
/// A 2048 × 64 strip rather than a 4160 px square: σ is a PARAMETER of
/// `texture_negative_pass`, the stripes are constant down the frame so the
/// vertical passes are exactly identity, and 2048 px carries eight cycles
/// of the longest anchor with the sampled window kept clear of the clamped
/// borders.
#[test]
fn texture_negative_hits_the_forty_five_lightroom_anchors() {
    let (w, h) = (2048usize, 64usize);
    // THE RASTER THE GROUND TRUTH WAS MEASURED ON — read through the
    // shipping function, so a change to the short-edge fractions or to the
    // `min(w, h)` normalisation moves the whole grid.
    let (sigma_coarse, sigma_fine) = texture_sigmas(6240, 4160);
    let mut worst = 0.0f32;
    let mut worst_at = (0.0f32, 0.0f32);
    for (pi, &period) in TEXTURE_ANCHOR_PERIODS.iter().enumerate() {
        let src = stripe_frame(w, h, period);
        let before = stripe_contrast(&src, w, h);
        for (si, &t) in TEXTURE_ANCHOR_STEPS.iter().enumerate() {
            let mut got = src.clone();
            texture_negative_pass(&mut got, w, h, t, sigma_coarse, sigma_fine, |_, _, _| 1.0);
            let transfer = stripe_contrast(&got, w, h) / before;
            let want = TEXTURE_ANCHOR_CLOSED[pi][si];
            let dev = (transfer - want).abs();
            if dev > worst {
                worst = dev;
                worst_at = (period, t);
            }
            assert!(
                dev <= 0.02,
                "anchor {period:.4} px @ t={t:.2}: got {transfer:.4}, closed form {want:.4}, \
                 off by {dev:.4} — the ±0.02 budget is b8-analysis-2 §6-3 and is not the \
                 thing to widen"
            );
        }
    }
    eprintln!("texture anchors: worst |dev| {worst:.4} at period {:.4} px, t={:.2}", worst_at.0, worst_at.1);
    // …and the grid must actually be TIGHT, or a future implementation
    // could drift most of the way across the budget unnoticed. 0.006 is
    // chosen against a measurement, not for roundness: the shipped kernels
    // sit at 0.0037, and dropping just the Gaussian-semigroup correction
    // (blurring the fine plane by the whole σ₁ instead of the residual σ′,
    // a 4 % σ error) takes the grid to 0.0092 while still inside ±0.02.
    // The arithmetic here is plain f32 with no reordering and no
    // contraction, so there is no platform drift for the margin to absorb.
    assert!(
        worst < 0.006,
        "the shipped kernels measured 0.0037 across this grid; {worst:.4} means the filter \
         changed, not that the budget was always this loose"
    );
}

/// R29 Batch-8-2 — the SHAPE, and the two operators it supersedes.
///
/// The acceptance grid above pins the numbers; this pins what the numbers
/// MEAN, which is the claim two previous designs got wrong:
///
/// * **pre-R28** ran `unsharp_luma` at `amount = −1`, whose transfer is
///   `G` exactly — a full Gaussian blur that erased fine detail (measured
///   in the visual-inspection pack, σ −92 %). It is CALLED here, not
///   paraphrased, so the comparison is against the real thing.
/// * **R28 Batch-5** replaced it with a NOTCH — `1 − |t|·(G_f − G_c)`,
///   returning to 1 at both spectral ends — and R29 B8-2 refuted the shape:
///   Lightroom's negative Texture is a monotone HIGH-SHELF, taking MOST out
///   of the finest scales, where the notch kept 0.9992 of a 4 px pattern.
///
/// So monotonicity is the historical assertion: any notch — including the
/// one this file shipped in v0.34.0 — turns back up at the fine end and
/// fails it.
#[test]
fn texture_negative_is_a_monotone_high_shelf_not_a_notch_and_not_a_blur() {
    let (w, h) = (2048usize, 64usize);
    let (sigma_coarse, sigma_fine) = texture_sigmas(6240, 4160);
    // The pre-R28 operator's own radius on that raster: 0.5 % of 4160.
    let old_radius = ((0.005 * 4160.0_f32).round() as usize).max(2);
    let mut curve = Vec::new();
    for &period in TEXTURE_ANCHOR_PERIODS.iter() {
        let src = stripe_frame(w, h, period);
        let before = stripe_contrast(&src, w, h);
        let mut now = src.clone();
        texture_negative_pass(&mut now, w, h, 1.0, sigma_coarse, sigma_fine, |_, _, _| 1.0);
        let mut pre_r28 = src.clone();
        unsharp_luma(&mut pre_r28, w, h, old_radius, -1.0, false);
        curve.push((
            period,
            stripe_contrast(&now, w, h) / before,
            stripe_contrast(&pre_r28, w, h) / before,
        ));
    }
    for (p, now, old) in &curve {
        eprintln!("texture −100 @ {p:8.4} px: now {now:.4}, pre-R28 {old:.4}");
    }

    // 1) MONOTONE from coarse to fine. `TEXTURE_ANCHOR_PERIODS` runs long
    //    period → short, so the transfer must never rise.
    for pair in curve.windows(2) {
        let (pa, ha, _) = pair[0];
        let (pb, hb, _) = pair[1];
        assert!(
            hb <= ha + 1e-3,
            "a high shelf never turns back up: {pa:.4} px keeps {ha:.4} but {pb:.4} px \
             keeps {hb:.4} — that is the notch shape B8-2 refuted"
        );
    }

    // 2) The fine end is where MOST is taken, and the plateau is the
    //    model's own `1 − (A₁+A₂) = 0.5227`, approached from above.
    let (_, fine_now, fine_old) = *curve.last().expect("nine anchors");
    assert!(
        (0.50..0.60).contains(&fine_now),
        "at −100 the finest anchor must sit on the plateau (0.5227), kept {fine_now:.4}"
    );
    assert!(
        fine_old < 0.05,
        "the pre-R28 branch really did erase it ({fine_old:.4}) — if this fails the \
         comparison is not measuring what the ledger says it measures"
    );

    // 3) …while the COARSE end is nearly untouched: H(ν→0) = 0.9996 on the
    //    clean base, and B8's +1.8 % low-frequency LIFT was pure capture
    //    sharpening, so a value above 1 here would be re-importing the
    //    confound the second batch removed.
    let (_, coarse_now, _) = curve[0];
    assert!(
        (0.98..=1.0).contains(&coarse_now),
        "a 256 px pattern must survive at −100 and must NOT be lifted above 1, kept \
         {coarse_now:.4}"
    );
}

/// R29 Batch-8-2 — the σ model and the preview clamp, in one test because
/// they are one decision: σ binds to the RENDER raster's short edge, and an
/// arm whose σ has gone sub-pixel on that raster is dropped rather than
/// approximated (user ruling 2026-08-21).
#[test]
fn texture_sigmas_track_the_render_rasters_short_edge_and_clamp_sub_pixel_arms() {
    // The measurement raster, both ways round: `min(w, h)`, never the first
    // argument and never the delivery size.
    let (c, f) = texture_sigmas(6240, 4160);
    assert!((c - 12.9938).abs() < 1e-3, "σ₁ at short edge 4160 is 12.9938 px, got {c:.4}");
    assert!((f - 1.1740).abs() < 1e-3, "σ₂ at short edge 4160 is 1.1740 px, got {f:.4}");
    assert_eq!(texture_sigmas(4160, 6240), (c, f), "the SHORT edge, whichever axis it is on");
    // …and it is proportional, not a fixed pixel count: half the raster,
    // half the σ. (Which reading Lightroom uses was settled at 16×
    // separation — b8-analysis-2 §1 ruling 4.)
    let (c2, f2) = texture_sigmas(3120, 2080);
    assert!((c2 * 2.0 - c).abs() < 1e-3 && (f2 * 2.0 - f).abs() < 1e-3);

    // The GUI preview raster (`gui/model.rs:296`: 1280 long edge, so ≈ 853
    // short) puts σ₂ at 0.241 px — under half a pixel, so the fine arm goes.
    let (pc, pf) = texture_sigmas(1280, 853);
    assert!(pf < TEXTURE_MIN_SIGMA_PX, "preview σ₂ = {pf:.4} px is the clamped case");
    assert!(pc >= TEXTURE_MIN_SIGMA_PX, "preview σ₁ = {pc:.4} px still renders");
    // 2080 is the first common raster where the fine arm survives — the
    // half-size export of the fixture set.
    assert!(texture_sigmas(3120, 2080).1 >= TEXTURE_MIN_SIGMA_PX);

    // The clamp is visible in the pixels, not just in the constants: on the
    // preview raster the fine arm's share of the depth is simply absent, so
    // a 3 px pattern keeps MORE than the full model would leave it.
    let (w, h) = (1024usize, 64usize);
    let src = stripe_frame(w, h, 3.0);
    let before = stripe_contrast(&src, w, h);
    let transfer = |sc: f32, sf: f32| {
        let mut d = src.clone();
        texture_negative_pass(&mut d, w, h, 1.0, sc, sf, |_, _, _| 1.0);
        stripe_contrast(&d, w, h) / before
    };
    let preview = transfer(pc, pf);
    // The same coarse arm with a fine arm that is NOT sub-pixel, so the
    // comparison isolates the clamp and not the σ pair.
    let both_arms = transfer(pc, 1.0);
    eprintln!("preview clamp @ 3 px: fine arm off {preview:.4}, fine arm on {both_arms:.4}");
    assert!(
        preview > both_arms + 0.15,
        "the clamped preview must be visibly WEAKER (higher transfer) than a rendered fine \
         arm — off {preview:.4}, on {both_arms:.4}"
    );

    // Below a 228 px short edge even the coarse arm's box³ radius rounds to
    // zero, and the pass declines rather than mis-applying the fine arm's
    // high-pass at the coarse arm's amplitude.
    let tiny = texture_sigmas(300, 200);
    let mut small = stripe_frame(32, 32, 4.0);
    let untouched = small.clone();
    texture_negative_pass(&mut small, 32, 32, 1.0, tiny.0, tiny.1, |_, _, _| 1.0);
    assert_eq!(
        frame_bits_fnv64(&small),
        frame_bits_fnv64(&untouched),
        "a 200 px short edge has no representable arm left — the pass must be a no-op"
    );
}

/// ONE calibration: the negative half is the same operator inside a mask as
/// outside it, bit for bit — the law the positive half is already pinned to
/// two tests above, restated on the branch that was rebuilt.
#[test]
fn mask_negative_texture_at_full_coverage_is_the_global_one_bit_for_bit() {
    let (w, h) = (800usize, 800usize);
    let recipe = EditRecipe { texture: -100.0, ..Default::default() };
    let src = stripe_frame(w, h, 16.0);
    let mut global = src.clone();
    apply_develop_anon(&mut global, w, h, &recipe);
    assert_ne!(
        frame_bits_fnv64(&src),
        frame_bits_fnv64(&global),
        "global texture −100 must move this frame, or the comparison below is vacuous"
    );
    let mut local = src.clone();
    apply_develop_anon(
        &mut local,
        w,
        h,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.5, full_x: 0.5, full_y: 0.5 },
                amount: 1.0,
                texture: -100.0,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    assert_eq!(
        frame_bits_fnv64(&global),
        frame_bits_fnv64(&local),
        "full-coverage local texture −100 must equal global texture −100 bit-for-bit"
    );
}

/// R25 P8: the honest version of the claim beside the fused curve stage.
///
/// `render.rs`'s "a full-coverage mask carrying a curve lands where the
/// same curve set globally would" was written as an ORDER argument (stage
/// 1 -> 1b -> 3, mirrored) and reads as a bit-exactness one, like the
/// clarity and texture twins above. It is not: the two paths compose the
/// same LUTs through different arithmetic — the global chain runs the
/// master curve over the whole frame and then the per-channel pass over it
/// again, while the mask fuses both into one pixel step and blends the
/// result at weight 1 — so the last bit of a deep-shadow code can differ.
/// Measured on this frame: the numbers this test prints.
///
/// The tolerance IS the claim now. One 8-bit code is the finest thing an
/// export, a preview or a Lightroom comparison can show, so a difference
/// under it is invisible everywhere the promise is made — and a difference
/// OVER it means the two paths have really come apart, which is what this
/// catches and a comment could not.
#[test]
fn mask_curves_at_full_coverage_match_the_global_curves_within_one_code() {
    use crate::recipe::CurvePoint;
    let pts = |v: &[(u8, u8)]| -> Vec<CurvePoint> {
        v.iter().map(|(i, o)| CurvePoint { input: *i, output: *o }).collect()
    };
    let main = pts(&[(0, 0), (64, 40), (192, 210), (255, 255)]);
    let red = pts(&[(0, 10), (255, 250)]);
    let green = pts(&[(0, 0), (128, 120), (255, 255)]);
    let blue = pts(&[(0, 5), (128, 140), (255, 255)]);
    // NOT `detail_frame`: its three channels sit within 0.06 of each
    // other, and the difference this test measures lives in the fused
    // path's unconditional `apply_sat_vibrance` at factor 1 — where
    // `l + (r - l)` returns `r` exactly whenever the channel is near the
    // luma. A frame with real chroma spread and real deep shadows is what
    // makes the two paths' arithmetic differ at all; on a flat one this
    // test would pass by measuring nothing.
    let (w, h) = (61usize, 97usize);
    let data: Vec<[f32; 3]> = (0..w * h)
        .map(|i| {
            let t = i as f32 / (w * h) as f32;
            [t * t, 1.0 - t, (0.5 - t).abs() * 1.8 + 0.002]
        })
        .collect();
    let mut global = data.clone();
    apply_develop_anon(
        &mut global,
        w,
        h,
        &EditRecipe {
            tone_curve: main.clone(),
            red_curve: red.clone(),
            green_curve: green.clone(),
            blue_curve: blue.clone(),
            ..Default::default()
        },
    );
    let mut local = data.clone();
    apply_develop_anon(
        &mut local,
        w,
        h,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                // The degenerate Linear gradient every full-coverage
                // comparison in this module uses: weight 1 everywhere.
                mask: MaskGeometry::Linear {
                    zero_x: 0.5,
                    zero_y: 0.5,
                    full_x: 0.5,
                    full_y: 0.5,
                },
                amount: 1.0,
                main_curve: main,
                red_curve: red,
                green_curve: green,
                blue_curve: blue,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    assert_ne!(
        frame_bits_fnv64(&data),
        frame_bits_fnv64(&global),
        "the curves must move this frame, or the comparison is vacuous"
    );
    let (mut worst, mut differing) = (0.0f32, 0usize);
    for (g, l) in global.iter().zip(&local) {
        for c in 0..3 {
            let d = (g[c] - l[c]).abs();
            if d > 0.0 {
                differing += 1;
            }
            worst = worst.max(d);
        }
    }
    let channels = global.len() * 3;
    eprintln!(
        "curve equivalence: {differing}/{channels} channel(s) differ, worst {:.6} LSB",
        worst * 255.0
    );
    // The drift has to OCCUR, or the tolerance is agreed with vacuously —
    // which is exactly how the old comment survived: on a low-chroma frame
    // the two paths really are bit-identical and nothing contradicts a
    // claim of bit-exactness.
    assert!(differing > 0, "no channel differed — this frame does not exercise the fused path");
    assert!(
        worst <= 1.0 / 255.0,
        "the fused mask curve stage drifted past one 8-bit code: worst {:.6} LSB over              {differing}/{channels} channel(s)",
        worst * 255.0
    );
}

/// R25 B2, the global twin of `mask_texture_halo_is_narrower_than_mask_
/// clarity_halo`: the two radii are the whole reason both sliders exist,
/// and the global pair must keep the same separation the masked pair has.
#[test]
fn texture_halo_is_narrower_than_clarity_halo() {
    let (w, h) = (128usize, 64usize);
    // 0.35/0.65 keeps both plateaus inside the midtone mask, which would
    // zero clarity's effect at 0.0 and 1.0.
    let edge: Vec<[f32; 3]> = (0..w * h)
        .map(|i| {
            let v = if i % w < w / 2 { 0.35f32 } else { 0.65 };
            [v, v, v]
        })
        .collect();
    let halo_of = |r: EditRecipe| -> usize {
        let mut out = edge.clone();
        apply_develop_anon(&mut out, w, h, &r);
        let row = (h / 2) * w;
        (0..w)
            .filter(|x| (out[row + x][0] - edge[row + x][0]).abs() > 1e-3)
            .map(|x| x.abs_diff(w / 2))
            .max()
            .unwrap_or(0)
    };
    let clarity = halo_of(EditRecipe { clarity: 60.0, ..Default::default() });
    let texture = halo_of(EditRecipe { texture: 60.0, ..Default::default() });
    assert!(texture >= 2, "texture must actually reach the edge: {texture}");
    assert!(
        texture * 2 < clarity,
        "texture halo ({texture}px) must be far narrower than clarity's ({clarity}px)"
    );
}

#[test]
fn mask_texture_halo_is_narrower_than_mask_clarity_halo() {
    // The two radii are the whole reason both sliders exist: clarity is
    // midtone VOLUME at a large radius, texture is fine DETAIL at a small
    // one. On a 128×64 step edge that is 8 px vs 2 px of box radius, and
    // three box passes spread each to ≈3×. Measured: 20 px vs 6 px of
    // half-width. Swapping the two radius formulas fails this.
    let (w, h) = (128usize, 64usize);
    let edge: Vec<[f32; 3]> = (0..w * h)
        .map(|i| {
            // 0.35/0.65 keeps both plateaus well inside the midtone mask,
            // which would zero clarity's effect at 0.0 and 1.0.
            let v = if i % w < w / 2 { 0.35f32 } else { 0.65 };
            [v, v, v]
        })
        .collect();
    let halo_of = |m: LocalAdjustment| -> usize {
        let mut out = edge.clone();
        apply_develop_anon(&mut out, w, h, &EditRecipe { masks: vec![m], ..Default::default() });
        let row = (h / 2) * w;
        (0..w)
            .filter(|x| (out[row + x][0] - edge[row + x][0]).abs() > 1e-3)
            .map(|x| x.abs_diff(w / 2))
            .max()
            .unwrap_or(0)
    };
    let full = MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.5, full_x: 0.5, full_y: 0.5 };
    let clarity = halo_of(LocalAdjustment {
        mask: full.clone(),
        amount: 1.0,
        clarity: 60.0,
        ..Default::default()
    });
    let texture = halo_of(LocalAdjustment {
        mask: full,
        amount: 1.0,
        texture: 60.0,
        ..Default::default()
    });
    assert!(texture >= 2, "texture must actually reach the edge: {texture}");
    assert!(
        texture * 2 < clarity,
        "texture halo ({texture}px) must be far narrower than clarity's ({clarity}px)"
    );
}
