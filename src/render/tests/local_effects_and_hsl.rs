// One part of the engine's tests (src/render/tests.rs includes it): mask dehaze, the engine's active counts, local point curves, hue and sharpness, HSL, colour grading and RGB curves.

#[test]
fn mask_dehaze_renders_only_inside_the_mask() {
    // A left-half Linear mask (weight 1 at nx=0, ramping to 0 at nx=0.5 and
    // clamped past it) with Dehaze +100: every column left of the midpoint
    // must move, every column at or right of it must be BYTE-identical —
    // the local dehaze may not leak the frame-wide airlight inversion
    // outside its coverage. Measured: columns 0..=31 changed, 32..=63 not.
    let (w, h) = (64usize, 16usize);
    let base: Vec<[f32; 3]> = (0..w * h)
        .map(|i| {
            // A hazy, low-contrast, slightly blue ramp — the case dehaze is for.
            let u = (i % w) as f32 / (w - 1) as f32;
            [0.30 + 0.5 * u, 0.34 + 0.45 * u, 0.42 + 0.40 * u]
        })
        .collect();
    let mut out = base.clone();
    apply_develop_anon(
        &mut out,
        w,
        h,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.5,
                    zero_y: 0.0,
                    full_x: 0.0,
                    full_y: 0.0,
                },
                amount: 1.0,
                dehaze: 100.0,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let changed = (0..3).any(|c| out[i][c].to_bits() != base[i][c].to_bits());
            // The eased handle is below the existing 0.001 work floor at
            // the first pixel next to the zero edge (t = 1/64,
            // smoothstep(t) < 0.001), so column 31 is intentionally part
            // of the unchanged plateau under the shipped profile.
            if x >= w / 2 - 1 {
                assert!(
                    !changed,
                    "uncovered column {x} moved: {:?} → {:?}",
                    base[i], out[i]
                );
            } else {
                assert!(changed, "covered column {x} did not move: {:?}", base[i]);
            }
        }
    }
    // Positive dehaze deepens tone: the fully-covered edge must darken.
    assert!(out[0][0] < base[0][0] - 0.05, "dehazed pixel must darken: {:?}", out[0]);
}

#[test]
fn engine_active_counts_local_clarity_dehaze_texture() {
    // The activity rule feeds the GUI's ● marker AND the mask-raster budget
    // loader. Before R22 a clarity-only mask read "parked" and its bitmap
    // was never loaded — so even after the engine learned to render it, the
    // raster would have been missing. All three must count.
    assert!(!engine_active(&LocalAdjustment::default()), "a bare mask is inert");
    for (name, m) in [
        ("clarity", LocalAdjustment { clarity: 50.0, ..Default::default() }),
        ("dehaze", LocalAdjustment { dehaze: -30.0, ..Default::default() }),
        ("texture", LocalAdjustment { texture: 15.0, ..Default::default() }),
        // R23-1b: the same rule for the two new local controls — a
        // hue-only or sharpness-only bitmap mask must load its raster and
        // read as active, or it renders nothing for the same reason.
        ("hue", LocalAdjustment { hue: 40.0, ..Default::default() }),
        ("sharpness", LocalAdjustment { sharpness: -60.0, ..Default::default() }),
    ] {
        assert!(engine_active(&m), "local {name} alone must count as active");
    }
}

/// The render's local-NR pass and [`engine_active`] read ONE gate,
/// [`LOCAL_NR_GATE`]: a mask whose only move is a Noise value at or under
/// it is inert (the pass allocates nothing there), one just over it is
/// active. `!= 0.0` once made a 0.05 mask read as active — a ● the user
/// sees for a render that does nothing.
///
/// MUTATION: `>=` or `!= 0.0` back in `engine_active`'s NR term.
#[test]
fn engine_active_reads_the_renders_own_noise_gate() {
    let nr = |v: f32| LocalAdjustment { noise_reduction: v, ..Default::default() };
    assert!(!engine_active(&nr(LOCAL_NR_GATE)), "at the gate the pass allocates nothing");
    assert!(!engine_active(&nr(LOCAL_NR_GATE * 0.5)), "…and below it too");
    assert!(engine_active(&nr(LOCAL_NR_GATE + 0.05)), "just over it the pass runs");
}

/// R25 P6. The registry's one-hot/zeroing probe
/// (`catalogue::local_tiers_agree_with_the_engines_own_activity_gate`)
/// cannot reach a `Shape::Curve` row — neither `1.0` nor `0.0` is a curve
/// — so it skips all four and their `Rendered` claim would be a free
/// declaration. This is the compensating test that probe's doc names, and
/// it probes BOTH directions the same way the scalar arm does.
#[test]
fn engine_active_counts_local_point_curves() {
    use crate::recipe::CurvePoint;
    let lift = || vec![CurvePoint { input: 64, output: 96 }];
    assert!(!engine_active(&LocalAdjustment::default()), "premise: a bare mask is inert");
    for (name, m) in [
        ("main_curve", LocalAdjustment { main_curve: lift(), ..Default::default() }),
        ("red_curve", LocalAdjustment { red_curve: lift(), ..Default::default() }),
        ("green_curve", LocalAdjustment { green_curve: lift(), ..Default::default() }),
        ("blue_curve", LocalAdjustment { blue_curve: lift(), ..Default::default() }),
    ] {
        // ONE-HOT: neutral everywhere but this curve ⇒ the mask wakes up.
        assert!(engine_active(&m), "local {name} alone must count as active");
        // ZEROING: an already-active mask with this curve emptied still
        // renders (the other term holds it up) — so the term is additive,
        // not a gate that swallows the rest.
        let with_slider = LocalAdjustment { exposure_ev: 1.0, ..m.clone() };
        assert!(engine_active(&with_slider), "premise: the two-term mask is active");
        let mut without = with_slider;
        without.main_curve.clear();
        without.red_curve.clear();
        without.green_curve.clear();
        without.blue_curve.clear();
        assert!(engine_active(&without), "clearing {name} must not mute the exposure move");
    }
}

/// R25 P6, the render half: a mask whose ONLY move is a point curve must
/// move the pixels it covers and leave every other pixel BIT-IDENTICAL.
///
/// Both halves matter. Before this batch a curve-only mask fell through
/// `tone_identity` exactly as a clarity-only mask fell through every gate
/// before R22 — it rendered nothing while the sidecar carried it — and the
/// bit-identical half is what proves the local curve is weighted by the
/// mask instead of being applied to the frame.
#[test]
fn a_local_point_curve_darkens_only_inside_the_mask() {
    use crate::recipe::CurvePoint;
    let (w, h) = (16usize, 4usize);
    let base: Vec<[f32; 3]> = (0..w * h).map(|_| [0.60, 0.50, 0.40]).collect();
    // Full effect at x=0, zero from x=w/2 on (the ramp's own convention).
    let left_half = MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.0, full_y: 0.0 };
    let run = |m: LocalAdjustment| -> Vec<[f32; 3]> {
        let mut out = base.clone();
        apply_masks(
            &mut out,
            w,
            h,
            &EditRecipe { masks: vec![m], ..Default::default() },
            &MaskRasterSnapshot::default(),
            MaskFrame::AsRendered,
            FilmScale::NATIVE,
        );
        out
    };
    // A pull-down master curve: midtones map lower, ends pinned.
    let darken = vec![
        CurvePoint { input: 0, output: 0 },
        CurvePoint { input: 128, output: 64 },
        CurvePoint { input: 255, output: 255 },
    ];
    let out = run(LocalAdjustment {
        mask: left_half.clone(),
        amount: 1.0,
        main_curve: darken,
        ..Default::default()
    });
    assert!(
        out[0][0] < base[0][0] - 0.05,
        "the fully covered pixel must darken: {:?} → {:?}",
        base[0],
        out[0]
    );
    for x in w / 2..w {
        assert_eq!(out[x], base[x], "uncovered column {x} moved on a curve-only mask");
    }

    // The per-channel arm: a RED lift touches red and nothing else, so the
    // three channel curves are wired to three different fields (a copied
    // index would show up here as green or blue moving too).
    let out = run(LocalAdjustment {
        mask: left_half,
        amount: 1.0,
        red_curve: vec![
            CurvePoint { input: 0, output: 0 },
            CurvePoint { input: 128, output: 192 },
            CurvePoint { input: 255, output: 255 },
        ],
        ..Default::default()
    });
    assert!(out[0][0] > base[0][0] + 0.05, "red must lift: {:?} → {:?}", base[0], out[0]);
    // Green and blue keep their value to within LUT-sampling rounding.
    // Not `==`: the fused pass runs the identity master curve through
    // `sample_lut` + `scale_chroma` for every covered pixel, which is a
    // ~1e-7 round trip on ANY active mask (it predates this batch). The
    // claim being made is "the red curve touched one channel", and 1e-5 is
    // four orders below the 0.05 swing above.
    for (ch, name) in [(1usize, "green"), (2, "blue")] {
        assert!(
            (out[0][ch] - base[0][ch]).abs() < 1e-5,
            "a red curve must leave {name} untouched: {:?} → {:?}",
            base[0],
            out[0]
        );
    }
    for x in w / 2..w {
        assert_eq!(out[x], base[x], "uncovered column {x} moved on a red-curve-only mask");
    }
}

/// R23-1b: the two controls the XMP writer emitted as a literal `"0"` from
/// the first sidecar on. Both must actually MOVE PIXELS, inside the mask
/// only — a slider that exports but does not render is the #15a/#10B defect
/// R22 fixed for clarity/dehaze/texture, reintroduced.
#[test]
fn local_hue_rotates_and_local_sharpness_signs_both_ways_inside_the_mask() {
    // A saturated red left half / right half, so a hue rotation is
    // measurable as a channel swing and the mask edge is unambiguous.
    let (w, h) = (16usize, 4usize);
    let base: Vec<[f32; 3]> = (0..w * h).map(|_| [0.80, 0.25, 0.20]).collect();
    // Covers the LEFT half only (the linear ramp reaches full at x=0).
    let left_half = MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.0, full_y: 0.0 };
    let run = |m: LocalAdjustment| -> Vec<[f32; 3]> {
        let mut out = base.clone();
        apply_masks(
            &mut out,
            w,
            h,
            &EditRecipe { masks: vec![m], ..Default::default() },
            &MaskRasterSnapshot::default(),
            MaskFrame::AsRendered,
            FilmScale::NATIVE,
        );
        out
    };

    // HUE: +100 is +30°, so red → orange (green rises, blue barely moves).
    let hue = run(LocalAdjustment {
        mask: left_half.clone(),
        amount: 1.0,
        hue: 100.0,
        ..Default::default()
    });
    assert!(
        hue[0][1] > base[0][1] + 0.05,
        "a +30° rotation must swing red toward orange: {:?} → {:?}",
        base[0],
        hue[0]
    );
    let right = w - 1;
    assert_eq!(hue[right], base[right], "the uncovered half must not rotate");
    // …and the rotation is a rotation: -100 goes the other way (toward
    // magenta — blue rises), not "less of the same".
    let back = run(LocalAdjustment {
        mask: left_half.clone(),
        amount: 1.0,
        hue: -100.0,
        ..Default::default()
    });
    assert!(back[0][2] > base[0][2] + 0.02, "-30° must swing the other way: {:?}", back[0]);

    // SHARPNESS: signed. A flat patch has no detail to sharpen, so measure
    // on an edge — the frame's own left column against its neighbour after
    // a step is introduced.
    let (mut edged, ew, eh) = detail_frame();
    let flat = edged.clone();
    apply_masks(
        &mut edged,
        ew,
        eh,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.0, full_y: 0.0 },
                amount: 1.0,
                sharpness: 100.0,
                ..Default::default()
            }],
            ..Default::default()
        },
        &MaskRasterSnapshot::default(),
        MaskFrame::AsRendered,
        FilmScale::NATIVE,
    );
    let energy = |d: &[[f32; 3]], lo: usize, hi: usize| -> f32 {
        let mut e = 0.0;
        for y in 0..eh {
            for x in lo..hi.saturating_sub(1) {
                e += (luma601(&d[y * ew + x + 1]) - luma601(&d[y * ew + x])).abs();
            }
        }
        e
    };
    assert!(
        energy(&edged, 0, ew / 4) > energy(&flat, 0, ew / 4) * 1.01,
        "positive local sharpness must RAISE edge energy inside the mask"
    );
    assert!(
        (energy(&edged, 3 * ew / 4, ew) - energy(&flat, 3 * ew / 4, ew)).abs() < 1e-4,
        "…and leave the uncovered side alone"
    );
    // The negative half is the point of the signed band: it SOFTENS.
    let mut softened = flat.clone();
    apply_masks(
        &mut softened,
        ew,
        eh,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.0, full_y: 0.0 },
                amount: 1.0,
                sharpness: -100.0,
                ..Default::default()
            }],
            ..Default::default()
        },
        &MaskRasterSnapshot::default(),
        MaskFrame::AsRendered,
        FilmScale::NATIVE,
    );
    assert!(
        energy(&softened, 0, ew / 4) < energy(&flat, 0, ew / 4) * 0.99,
        "negative local sharpness must LOWER edge energy (this is the blur half)"
    );
}

/// Deterministic mid-tone frame with fine and coarse structure — enough
/// detail for an unsharp mask to bite, no values near 0 or 1 where the
/// midtone weight or the clamps would mask a real difference.
fn detail_frame() -> (Vec<[f32; 3]>, usize, usize) {
    let (w, h) = (64usize, 48usize);
    let mut data = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let coarse = if (x / 8 + y / 8) % 2 == 0 { 0.08 } else { -0.08 };
            let fine = if (x + y) % 2 == 0 { 0.03 } else { -0.03 };
            let base = 0.45 + coarse + fine;
            data.push([base, base * 0.95 + 0.02, base * 0.9 + 0.04]);
        }
    }
    (data, w, h)
}

#[test]
fn hsl_adjusts_only_the_targeted_colour_band() {
    use crate::recipe::Hsl;
    // Red-band saturation -100 desaturates a red pixel toward grey but leaves
    // a blue pixel (a different band) untouched.
    let mut hsl = Hsl::default();
    hsl.saturation[0] = -100.0; // red band
    let mut data = vec![[0.8_f32, 0.1, 0.1], [0.1, 0.1, 0.8]];
    apply_hsl(&mut data, &hsl);
    let red = data[0];
    assert!(
        (red[0] - red[1]).abs() < 0.05 && (red[1] - red[2]).abs() < 0.05,
        "red pixel desaturated toward grey: {red:?}"
    );
    let blue = data[1];
    // ALL three channels — hsl_to_rgb derives them from three different
    // hue offsets, so a green-only defect is a real failure class, and
    // green is exactly the channel a blue→cyan cast moves first (U14).
    for (c, want) in [0.1f32, 0.1, 0.8].iter().enumerate() {
        assert!(
            (blue[c] - want).abs() < 0.02,
            "blue pixel untouched on every channel: {blue:?}"
        );
    }
    // EVERY band that is not red or one of its feathered neighbours
    // (orange, magenta) is a control: the single blue probe above let a
    // routing leak into yellow/green/aqua/purple pass (R12). Probe
    // pixels are generated by the engine's own hue converter at each
    // band's centre hue, saturated and mid-bright.
    for (name, hue) in
        [("yellow", 60.0f32), ("green", 120.0), ("aqua", 180.0), ("purple", 280.0)]
    {
        let (r0, g0, b0) = hsl_to_rgb(hue / 360.0, 0.7, 0.45);
        let mut probe = vec![[r0, g0, b0]];
        apply_hsl(&mut probe, &hsl);
        for c in 0..3 {
            assert!(
                (probe[0][c] - [r0, g0, b0][c]).abs() < 0.02,
                "red-band sat must not reach the {name} band: {:?} vs {:?}",
                probe[0],
                (r0, g0, b0)
            );
        }
    }
}

#[test]
fn hsl_neutral_is_identity_and_grey_is_untouched() {
    use crate::recipe::Hsl;
    // A neutral HSL is an exact no-op.
    let mut data = vec![[0.6_f32, 0.2, 0.2], [0.5, 0.5, 0.5]];
    let orig = data.clone();
    apply_hsl(&mut data, &Hsl::default());
    assert_eq!(data, orig);
    // A grey pixel has no hue, so even a strong all-band push leaves it alone.
    let hsl = Hsl { saturation: [100.0; 8], ..Hsl::default() };
    let mut grey = vec![[0.5_f32, 0.5, 0.5]];
    apply_hsl(&mut grey, &hsl);
    assert!(
        (grey[0][0] - 0.5).abs() < 1e-4
            && (grey[0][1] - 0.5).abs() < 1e-4
            && (grey[0][2] - 0.5).abs() < 1e-4,
        "grey untouched: {:?}",
        grey[0]
    );
    // A LUMINANCE push probes the chroma gate itself: the saturation
    // case is nearly vacuous for grey (s = 0 short-circuits both hue
    // converters), while a grey that slipped the gate would be SCALED
    // by the luminance term on all three channels (U14).
    let hsl_lum = Hsl { luminance: [100.0; 8], ..Hsl::default() };
    let mut grey2 = vec![[0.5_f32, 0.5, 0.5]];
    apply_hsl(&mut grey2, &hsl_lum);
    assert_eq!(grey2[0], [0.5, 0.5, 0.5], "grey must not respond to band luminance");
}

#[test]
fn hsl_does_not_blotch_a_near_grey_sky() {
    use crate::recipe::Hsl;
    // A near-grey overcast "sky": alternating pixels lean faintly blue vs
    // faintly aqua (s ≈ 3%), the way real demosaiced sky noise does. With
    // OPPOSITE luminance on the blue and aqua bands, the un-weighted code
    // would slam adjacent pixels to wildly different luma (a checkerboard
    // blotch). The saturation fade must keep the patch smooth.
    let mut data: Vec<[f32; 3]> = (0..64)
        .map(|i| if i % 2 == 0 { [0.71, 0.715, 0.726] } else { [0.71, 0.726, 0.722] })
        .collect();
    let hsl = Hsl { luminance: [0.0, 0.0, 0.0, 0.0, 60.0, -80.0, 0.0, 0.0], ..Hsl::default() };
    apply_hsl(&mut data, &hsl);
    let lumas: Vec<f32> = data.iter().map(luma601).collect();
    let spread = lumas.iter().cloned().fold(f32::MIN, f32::max)
        - lumas.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread < 0.04, "near-grey sky must not blotch — luma spread {spread}");
}

#[test]
fn color_grade_tints_the_targeted_tonal_region() {
    use crate::recipe::ColorGrade;
    // A blue shadow wheel pushes a DARK pixel toward blue; neutral is a no-op.
    let cg = ColorGrade { shadow_hue: 240.0, shadow_sat: 100.0, blending: 100.0, ..Default::default() };
    let mut data = vec![[0.15_f32, 0.15, 0.15]]; // dark grey
    apply_color_grade(&mut data, &cg);
    let p = data[0];
    assert!(p[2] > p[0] && p[2] > p[1], "shadow tinted blue: {p:?}");

    let mut d2 = vec![[0.4_f32, 0.3, 0.2]];
    let orig = d2.clone();
    apply_color_grade(&mut d2, &ColorGrade::default()); // neutral
    assert_eq!(d2, orig);
}

#[test]
fn rgb_curves_shape_each_channel_independently() {
    use crate::recipe::CurvePoint;
    // Each per-channel curve lifts ITS channel only, via the full
    // pipeline. Only the red curve used to be exercised — an engine that
    // ignored green_curve/blue_curve entirely stayed green (R12).
    let lift = || vec![
        CurvePoint { input: 0, output: 60 },
        CurvePoint { input: 255, output: 255 },
    ];
    let cases: [(&str, EditRecipe, usize); 3] = [
        ("red", EditRecipe { red_curve: lift(), ..Default::default() }, 0),
        ("green", EditRecipe { green_curve: lift(), ..Default::default() }, 1),
        ("blue", EditRecipe { blue_curve: lift(), ..Default::default() }, 2),
    ];
    for (name, r, ch) in cases {
        let mut data = vec![[0.0_f32, 0.0, 0.0]];
        apply_develop_anon(&mut data, 1, 1, &r);
        let p = data[0];
        assert!(p[ch] > 0.15, "{name} channel lifted: {p:?}");
        for c in (0..3).filter(|c| *c != ch) {
            assert!(p[c] < 0.02, "{name} curve must not move channel {c}: {p:?}");
        }
    }
}

#[test]
fn curve_lut_pins_missing_endpoints_but_keeps_explicit_ones() {
    use crate::recipe::CurvePoint;
    // One mid-curve click must NOT flatten the image to a constant: the
    // missing (0,0)/(1,1) endpoints are pinned, so the LUT stays a real
    // ramp through the clicked point.
    let one = curve_lut(&[CurvePoint { input: 128, output: 128 }]);
    assert!(one[0].abs() < 1e-6 && (one[255] - 1.0).abs() < 1e-6, "ends pinned");
    assert!((one[128] - 128.0 / 255.0).abs() < 1e-2, "clicked point honoured");
    assert!(one[64] > 0.1 && one[64] < 0.4, "shadows still a ramp, not clamped flat");
    assert!(one[192] > 0.6 && one[192] < 0.9, "highlights still a ramp");

    // Two inner points: everything outside them ramps to the pins instead
    // of freezing into crushed/blown bands.
    let two = curve_lut(&[
        CurvePoint { input: 64, output: 64 },
        CurvePoint { input: 192, output: 192 },
    ]);
    assert!(two[32] > 0.05 && two[32] < 0.2, "below-first ramps from (0,0)");
    assert!(two[224] > 0.8 && two[224] < 0.95, "above-last ramps to (1,1)");

    // An explicit endpoint stays authoritative — lifted blacks survive.
    let lifted = curve_lut(&[
        CurvePoint { input: 0, output: 40 },
        CurvePoint { input: 255, output: 255 },
    ]);
    assert!((lifted[0] - 40.0 / 255.0).abs() < 1e-3, "explicit (0,40) wins over the pin");
}
